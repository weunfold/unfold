

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use rth_protocol::ResponseStatus;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

use crate::backend::{Cap, Capabilities, Command, Evidence, Flags, Heard, Output, Session};
use crate::gemini::{close_reason, payload, tls_connector};

pub const OPENAI_RATE: u32 = 24_000;

pub const PRICE_TABLE: &str = "vox-2026-10-16";
const SETUP_EVENT: &str = "rth-setup";

const SETUP_TIMEOUT: Duration = Duration::from_secs(15);

const TURN_STALL: Duration = Duration::from_secs(45);

const READY_WAIT: Duration = Duration::from_secs(20);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

const MAX_TRACKED: usize = 256;

const MAX_LOGGED: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Dialect {
    OpenAI,

    Grok,
}

impl Dialect {

    pub fn name(self) -> &'static str {
        match self {
            Dialect::OpenAI => "openai-realtime@v1",
            Dialect::Grok => "grok-voice@v1",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Dialect::OpenAI => "gpt-realtime-2.1",
            Dialect::Grok => "grok-voice-3.0",
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Dialect::OpenAI => "wss://api.openai.com/v1/realtime",
            Dialect::Grok => "wss://api.x.ai/v1/realtime",
        }
    }

    pub fn input_rate(self) -> u32 {
        match self {
            Dialect::OpenAI => OPENAI_RATE,
            Dialect::Grok => 16_000,
        }
    }

    fn opened_policy(self) -> &'static str {
        match self {
            Dialect::OpenAI => "provider:openai-realtime-vad@v1",
            Dialect::Grok => "provider:grok-voice-vad@v1",
        }
    }

    pub fn flags(self) -> Flags {
        match self {
            Dialect::OpenAI => Flags {
                explicit_replies: Cap::Yes(Evidence::Documented),
                deferred_replies: Cap::Yes(Evidence::Documented),
                live_turn_options: Cap::Yes(Evidence::Documented),
                native_resumption: Cap::No(Evidence::Documented),
                dynamic_tools: Cap::Yes(Evidence::Documented),
                clears_input: Cap::Yes(Evidence::Documented),
                truncate: Cap::Yes(Evidence::Documented),
                truncate_returns_text: Cap::No(Evidence::Documented),
                quiet_close_s: None,
            },
            Dialect::Grok => Flags {
                explicit_replies: Cap::Yes(Evidence::Client),
                deferred_replies: Cap::Unknown,
                live_turn_options: Cap::Unknown,
                native_resumption: Cap::Yes(Evidence::Documented),
                dynamic_tools: Cap::Yes(Evidence::Client),
                clears_input: Cap::Yes(Evidence::Client),
                truncate: Cap::Yes(Evidence::Client),
                truncate_returns_text: Cap::Yes(Evidence::Client),
                quiet_close_s: None,
            },
        }
    }
}

pub struct Config {
    pub dialect: Dialect,
    pub url: String,
    pub model: String,

    api_key: Option<String>,

    pub ca_file: Option<String>,

    pub system_instruction: Option<String>,

    pub persona: &'static str,

    pub response_prefix: String,

    pub vad: String,

    pub eagerness: Option<String>,
    pub create_response: bool,
    pub interrupt_response: bool,
    pub transcription_model: String,

    pub transcription_lang: Option<String>,
    pub voice: String,
    pub noise_reduction: Option<String>,
    pub reasoning_effort: Option<String>,

    pub cancel_ack: Duration,

    pub associate_wait: Duration,

    pub tools: Vec<crate::tools::Tool>,

    pub text_only: bool,

    pub max_output_tokens: Option<u32>,

    pub usage_log: Option<std::path::PathBuf>,

    pub spend: Option<crate::spend::Spend>,

    pub setup_gate: Option<tokio::sync::watch::Receiver<bool>>,
}

macro_rules! knob {
    ($var:expr, $name:literal) => {
        $var(concat!("RTH_OPENAI_", $name)).or_else(|| option_env!(concat!("RTH_BUILD_OPENAI_", $name)).map(str::to_string))
    };
}

fn flag(v: Option<String>, name: &str, default: bool) -> Result<bool> {
    match v.as_deref() {
        None => Ok(default),
        Some("1" | "true") => Ok(true),
        Some("0" | "false") => Ok(false),
        Some(other) => bail!("RTH_OPENAI_{name}={other:?} (expected 1 or 0)"),
    }
}

impl Config {

    pub fn from_env(dialect: Dialect) -> Result<Self> {
        Self::from_lookup(dialect, &|k: &str| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    pub fn from_lookup(dialect: Dialect, var: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        if dialect == Dialect::Grok {
            bail!("the grok-voice dialect is a stub (only the OpenAI dialect is built)");
        }
        let model = var("RTH_OPENAI_MODEL").unwrap_or_else(|| dialect.model().to_string());
        let url = var("RTH_OPENAI_URL").unwrap_or_else(|| format!("{}?model={model}", dialect.endpoint()));
        let api_key = var("OPENAI_API_KEY");
        if api_key.is_none() && url.starts_with("wss://api.openai.com") {
            bail!("RTH_BACKEND=openai-realtime needs OPENAI_API_KEY (in the guest: the microsandbox placeholder)");
        }
        let vad = knob!(var, "VAD").unwrap_or_else(|| "semantic_vad".into());
        if !matches!(vad.as_str(), "semantic_vad" | "server_vad" | "none") {
            bail!("RTH_OPENAI_VAD={vad:?} (expected semantic_vad, server_vad or none)");
        }
        let eagerness = match knob!(var, "EAGERNESS").as_deref() {
            None | Some("auto") => None,
            Some(e @ ("low" | "medium" | "high")) => Some(e.to_string()),
            Some(other) => bail!("RTH_OPENAI_EAGERNESS={other:?} (expected auto, low, medium or high)"),
        };
        let text_only = flag(knob!(var, "TEXT_ONLY"), "TEXT_ONLY", false)?;
        if text_only && vad != "none" {
            bail!("RTH_OPENAI_TEXT_ONLY=1 needs RTH_OPENAI_VAD=none (no provider-created responses)");
        }
        let max_output_tokens = match knob!(var, "MAX_OUTPUT_TOKENS") {
            None => None,
            Some(v) => match v.parse::<u32>() {
                Ok(n @ 1..=4096) => Some(n),
                _ => bail!("RTH_OPENAI_MAX_OUTPUT_TOKENS={v:?} (expected an integer from 1 to 4096)"),
            },
        };
        let spend_knobs = (var("RTH_SPEND_CONTROL"), var("RTH_SPEND_CONTROL_SHA256"));
        if spend_knobs.0.is_some() != spend_knobs.1.is_some() {
            bail!("RTH_SPEND_CONTROL and RTH_SPEND_CONTROL_SHA256 come together");
        }
        if spend_knobs.0.is_some() && !text_only {
            bail!("the A2 spend ledger needs RTH_OPENAI_TEXT_ONLY=1");
        }
        let spend = match spend_knobs {
            (Some(control), Some(sha)) => {
                let e = crate::spend::Expect { model: model.clone(), text_only, vad: vad.clone(), max_output_tokens };
                Some(crate::spend::Spend::open(std::path::Path::new(&control), &sha, &e).map_err(|e| anyhow::anyhow!(e))?)
            }
            _ => None,
        };
        let noise_reduction = knob!(var, "NOISE_REDUCTION");
        if let Some(n) = noise_reduction.as_deref().filter(|n| !matches!(*n, "near_field" | "far_field")) {
            bail!("RTH_OPENAI_NOISE_REDUCTION={n:?} (expected near_field or far_field)");
        }
        Ok(Self {
            dialect,
            url,
            model,
            api_key,
            ca_file: var("RTH_CA_FILE"),
            system_instruction: knob!(var, "SYSTEM"),
            persona: crate::persona::parse(knob!(var, "PERSONA").as_deref()).map_err(|e| anyhow::anyhow!("RTH_OPENAI_PERSONA: {e}"))?,
            response_prefix: "v".into(),
            vad,
            eagerness,
            create_response: flag(knob!(var, "CREATE_RESPONSE"), "CREATE_RESPONSE", true)?,
            interrupt_response: flag(knob!(var, "INTERRUPT_RESPONSE"), "INTERRUPT_RESPONSE", true)?,
            transcription_model: knob!(var, "TRANSCRIPTION_MODEL").unwrap_or_else(|| "gpt-transcribe".into()),
            transcription_lang: knob!(var, "TRANSCRIPTION_LANG"),
            voice: knob!(var, "VOICE").unwrap_or_else(|| "marin".into()),
            noise_reduction,
            reasoning_effort: knob!(var, "REASONING"),
            cancel_ack: Duration::from_secs(5),
            associate_wait: Duration::from_secs(10),
            tools: crate::tools::knob(knob!(var, "TOOLS")),
            text_only,
            max_output_tokens,
            usage_log: var("RTH_OPENAI_USAGE_LOG").map(std::path::PathBuf::from),
            spend,
            setup_gate: None,
        })
    }

    pub fn knobs(&self) -> String {
        let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
        format!(
            "vad={} eagerness={} create_response={} interrupt_response={} transcription={} transcription_lang={} voice={} noise_reduction={} reasoning={} persona={}{}",
            self.vad,
            self.eagerness.as_deref().unwrap_or("auto"),
            self.create_response,
            self.interrupt_response,
            self.transcription_model,
            opt(&self.transcription_lang),
            self.voice,
            opt(&self.noise_reduction),
            opt(&self.reasoning_effort),
            crate::persona::persona_name(self.system_instruction.as_deref(), self.persona),
            crate::tools::label(&self.tools)
        ) + &self.limits_label()
    }

    fn limits_label(&self) -> String {
        let mut l = String::new();
        if self.text_only {
            l.push_str(" text_only=1");
        }
        if let Some(n) = self.max_output_tokens {
            l.push_str(&format!(" max_output_tokens={n}"));
        }
        l
    }

    fn manual(&self) -> bool {
        self.vad == "none"
    }

    fn commits_audio(&self) -> bool {
        self.manual() && !self.text_only
    }

    #[cfg(test)]
    pub fn for_test(url: String) -> Self {
        Self {
            dialect: Dialect::OpenAI,
            url,
            model: Dialect::OpenAI.model().into(),
            api_key: Some(tests::TEST_KEY.into()),
            ca_file: None,
            system_instruction: None,
            persona: crate::persona::DEFAULT_PERSONA,
            response_prefix: "v".into(),
            vad: "semantic_vad".into(),
            eagerness: None,
            create_response: true,
            interrupt_response: true,
            transcription_model: "gpt-transcribe".into(),
            transcription_lang: None,
            voice: "marin".into(),
            noise_reduction: None,
            reasoning_effort: None,
            text_only: false,
            max_output_tokens: None,
            usage_log: None,
            spend: None,
            setup_gate: None,
            cancel_ack: Duration::from_millis(300),
            associate_wait: Duration::from_millis(300),
            tools: Vec::new(),
        }
    }
}

pub fn setup_message(cfg: &Config) -> Value {
    let pcm = json!({ "type": "audio/pcm", "rate": OPENAI_RATE });
    let turn_detection = if cfg.manual() {
        Value::Null
    } else {
        let mut td = json!({ "type": cfg.vad, "create_response": cfg.create_response, "interrupt_response": cfg.interrupt_response });
        if let Some(e) = &cfg.eagerness
            && cfg.vad == "semantic_vad"
        {
            td["eagerness"] = json!(e);
        }
        td
    };
    let mut transcription = json!({ "model": cfg.transcription_model });
    if let Some(lang) = &cfg.transcription_lang {
        transcription["language"] = json!(lang);
    }
    let mut input = json!({ "format": pcm, "transcription": transcription, "turn_detection": turn_detection });
    if let Some(n) = &cfg.noise_reduction {
        input["noise_reduction"] = json!({ "type": n });
    }
    let mut session = json!({
        "type": "realtime",
        "output_modalities": [if cfg.text_only { "text" } else { "audio" }],
        "audio": { "input": input, "output": { "format": pcm, "voice": cfg.voice } },
    });
    if let Some(n) = cfg.max_output_tokens {
        session["max_output_tokens"] = json!(n);
    }
    if let Some(text) = crate::persona::system_instruction(None, cfg.system_instruction.as_deref(), cfg.persona) {
        session["instructions"] = json!(crate::persona::with_tools(text, &cfg.tools));
    }

    if !cfg.tools.is_empty() {
        session["tools"] = crate::tools::render_openai(&cfg.tools);
        session["tool_choice"] = json!("auto");
    }
    if let Some(effort) = &cfg.reasoning_effort {
        session["reasoning"] = json!({ "effort": effort });
    }
    json!({ "type": "session.update", "event_id": SETUP_EVENT, "session": session })
}

#[cfg(test)]
pub fn setup_identity(cfg: &Config) -> String {
    setup_identity_of(cfg, &setup_message(cfg))
}

pub fn setup_identity_of(cfg: &Config, setup: &Value) -> String {
    use sha2::Digest;
    let v = json!({ "model": cfg.model, "dialect": cfg.dialect.name(), "setup": setup });
    let digest = sha2::Sha256::digest(v.to_string().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn user_text(text: &str, event_id: &str) -> Value {
    json!({
        "type": "conversation.item.create",
        "event_id": event_id,
        "item": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": text }] },
    })
}

pub fn response_create(event_id: &str, request_id: &str, generation: u64) -> Value {
    json!({
        "type": "response.create",
        "event_id": event_id,
        "response": { "metadata": { "rth_request": request_id, "rth_gen": generation.to_string() } },
    })
}

pub fn response_create_for(cfg: &Config, event_id: &str, request_id: &str, generation: u64) -> Value {
    let mut v = response_create(event_id, request_id, generation);
    if cfg.text_only {
        v["response"]["output_modalities"] = json!(["text"]);
    }
    if let Some(n) = cfg.max_output_tokens {
        v["response"]["max_output_tokens"] = json!(n);
    }
    v
}

pub fn usage_complete(u: &Value) -> bool {
    let n = |p: &str| u.pointer(p).is_some_and(Value::is_u64);
    ["/input_tokens", "/output_tokens", "/input_token_details/text_tokens", "/input_token_details/audio_tokens", "/input_token_details/cached_tokens", "/output_token_details/text_tokens", "/output_token_details/audio_tokens"]
        .iter()
        .all(|p| n(p))
}

pub fn function_output(event_id: &str, call_id: &str, output: &Value) -> Value {
    json!({
        "type": "conversation.item.create",
        "event_id": event_id,
        "item": { "type": "function_call_output", "call_id": call_id, "output": output.to_string() },
    })
}

pub fn response_cancel(event_id: &str, provider_response_id: &str) -> Value {
    json!({ "type": "response.cancel", "event_id": event_id, "response_id": provider_response_id })
}

pub fn audio_append(pcm: &[u8]) -> Value {
    json!({ "type": "input_audio_buffer.append", "audio": base64::engine::general_purpose::STANDARD.encode(pcm) })
}

pub fn input_commit(event_id: &str) -> Value {
    json!({ "type": "input_audio_buffer.commit", "event_id": event_id })
}

pub fn history_items(turns: &[crate::context::Turn]) -> (Vec<Value>, crate::gemini::HistorySent) {
    let (kept, sent) = crate::gemini::settled_history(turns);
    let items = kept
        .iter()
        .map(|t| {
            let (role, kind) = if t.role == "model" { ("assistant", "output_text") } else { ("user", "input_text") };
            json!({ "type": "conversation.item.create", "item": { "type": "message", "role": role, "content": [{ "type": kind, "text": t.text }] } })
        })
        .collect();
    (items, sent)
}

#[derive(Debug)]
pub struct Resampler {
    from: u32,
    to: u32,

    consumed: u64,

    produced: u64,

    last: Option<i16>,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { from: from.max(1), to: to.max(1), consumed: 0, produced: 0, last: None }
    }

    pub fn input_rate(&self) -> u32 {
        self.from
    }

    pub fn push(&mut self, pcm: &[u8]) -> Vec<u8> {
        if self.from == self.to {
            return pcm.to_vec();
        }
        let input: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
        if input.is_empty() {
            return Vec::new();
        }

        let base = self.consumed - self.last.is_some() as u64;
        let window: Vec<i16> = self.last.into_iter().chain(input.iter().copied()).collect();
        let total = self.consumed + input.len() as u64;
        let (from, to) = (self.from as u64, self.to as u64);
        let mut out = Vec::new();
        loop {
            let pos = self.produced * from;
            let i = pos / to;
            if i + 1 >= total {
                break;
            }
            let r = (pos % to) as i64;
            let a = window[(i - base) as usize] as i64;
            let b = window[(i + 1 - base) as usize] as i64;
            let v = (a * (to as i64 - r) + b * r) / to as i64;
            out.extend_from_slice(&(v as i16).to_le_bytes());
            self.produced += 1;
        }
        self.consumed = total;
        self.last = input.last().copied();
        out
    }
}

#[derive(Debug, PartialEq)]
pub enum ServerEvent {
    SessionCreated,
    SessionUpdated,
    SpeechStarted { item_id: Option<String> },
    SpeechStopped { item_id: Option<String> },
    Committed { item_id: Option<String> },
    TranscriptDelta { item_id: Option<String>, delta: String },
    TranscriptCompleted { item_id: Option<String>, transcript: String },
    TranscriptFailed { item_id: Option<String>, detail: String },
    ResponseCreated { response_id: String, rth_request: Option<String>, rth_gen: Option<String> },

    ItemAdded { item_id: String, previous_item_id: Option<String>, role: Option<String> },

    OutputItemAdded { response_id: String, item_id: String },

    FunctionCall { response_id: String, call_id: String, name: String, arguments: String },
    AudioDelta { response_id: Option<String>, item_id: Option<String>, content_index: u64, pcm: Vec<u8> },

    TextDelta { response_id: Option<String>, item_id: Option<String>, delta: String },
    ResponseDone { response_id: String, status: String, reason: Option<String>, usage: Option<Value> },

    Error { client_event: Option<String>, error_type: Option<String>, code: String, message: String },

    Ignored(String),

    Unknown(String),
}

const IGNORED: &[&str] = &[
    "conversation.created",
    "conversation.item.done",
    "conversation.item.retrieved",
    "conversation.item.deleted",
    "conversation.item.truncated",
    "input_audio_buffer.cleared",
    "input_audio_buffer.timeout_triggered",
    "conversation.item.input_audio_transcription.segment",
    "response.content_part.added",
    "response.content_part.done",
    "response.output_audio.done",
    "response.audio.done",
    "response.output_audio_transcript.done",
    "response.audio_transcript.done",
    "response.output_text.done",
    "response.text.done",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "rate_limits.updated",
];

pub fn parse_server(bytes: &[u8]) -> Result<ServerEvent> {
    let v: Value = serde_json::from_slice(bytes).context("server event is not JSON")?;
    let ty = v.get("type").and_then(Value::as_str).context("server event has no type")?;
    let s = |p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_string);
    let item = || s("/item_id").filter(|i| !i.is_empty());
    Ok(match ty {
        "session.created" => ServerEvent::SessionCreated,
        "session.updated" => ServerEvent::SessionUpdated,
        "input_audio_buffer.speech_started" => ServerEvent::SpeechStarted { item_id: item() },
        "input_audio_buffer.speech_stopped" => ServerEvent::SpeechStopped { item_id: item() },
        "input_audio_buffer.committed" => ServerEvent::Committed { item_id: item() },
        "conversation.item.input_audio_transcription.delta" => ServerEvent::TranscriptDelta { item_id: item(), delta: s("/delta").unwrap_or_default() },
        "conversation.item.input_audio_transcription.completed" => {
            ServerEvent::TranscriptCompleted { item_id: item(), transcript: s("/transcript").unwrap_or_default() }
        }
        "conversation.item.input_audio_transcription.failed" => {
            ServerEvent::TranscriptFailed { item_id: item(), detail: s("/error/message").unwrap_or_else(|| "unknown".into()) }
        }
        "response.created" => ServerEvent::ResponseCreated {
            response_id: s("/response/id").context("response.created without response.id")?,
            rth_request: s("/response/metadata/rth_request"),
            rth_gen: s("/response/metadata/rth_gen"),
        },
        "conversation.item.added" | "conversation.item.created" => ServerEvent::ItemAdded {
            item_id: s("/item/id").context("conversation item without an id")?,
            previous_item_id: s("/previous_item_id").filter(|i| !i.is_empty()),
            role: s("/item/role"),
        },
        "response.output_item.done" if s("/item/type").as_deref() == Some("function_call") => ServerEvent::FunctionCall {
            response_id: s("/response_id").context("function call without a response id")?,
            call_id: s("/item/call_id").context("function call without a call id")?,
            name: s("/item/name").unwrap_or_default(),
            arguments: s("/item/arguments").unwrap_or_default(),
        },
        "response.output_item.done" => ServerEvent::Ignored(ty.to_string()),
        "response.output_item.added" => ServerEvent::OutputItemAdded {
            response_id: s("/response_id").context("output item without a response id")?,
            item_id: s("/item/id").context("output item without an id")?,
        },

        "response.output_audio.delta" | "response.audio.delta" => {
            let data = s("/delta").unwrap_or_default();
            let pcm = base64::engine::general_purpose::STANDARD.decode(data).context("audio delta is not base64")?;
            ServerEvent::AudioDelta {
                response_id: s("/response_id"),
                item_id: item(),
                content_index: v.get("content_index").and_then(Value::as_u64).unwrap_or(0),
                pcm,
            }
        }
        "response.output_audio_transcript.delta" | "response.audio_transcript.delta" | "response.output_text.delta" | "response.text.delta" => {
            ServerEvent::TextDelta { response_id: s("/response_id"), item_id: s("/item_id"), delta: s("/delta").unwrap_or_default() }
        }
        "response.done" => ServerEvent::ResponseDone {
            response_id: s("/response/id").context("response.done without response.id")?,
            status: s("/response/status").unwrap_or_else(|| "unknown".into()),
            reason: s("/response/status_details/reason"),
            usage: v.pointer("/response/usage").cloned(),
        },
        "error" => ServerEvent::Error {
            client_event: s("/error/event_id"),
            error_type: s("/error/type"),
            code: s("/error/code").or_else(|| s("/error/type")).unwrap_or_else(|| "unknown".into()),
            message: s("/error/message").unwrap_or_default(),
        },
        other if IGNORED.contains(&other) => ServerEvent::Ignored(other.to_string()),
        other => ServerEvent::Unknown(other.to_string()),
    })
}

pub fn map_status(status: &str) -> ResponseStatus {
    match status {
        "completed" => ResponseStatus::Completed,
        "cancelled" => ResponseStatus::Cancelled,
        _ => ResponseStatus::Failed,
    }
}

fn append_synced(path: &std::path::Path, row: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(format!("{row}\n").as_bytes())?;
    f.sync_data()
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_LOGGED).collect()
}

pub fn connect(cfg: Config) -> Session {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let (out_tx, out_rx) = mpsc::channel::<Output>(1024);
    let d = cfg.dialect;
    let prompt = crate::persona::persona_name(cfg.system_instruction.as_deref(), cfg.persona);
    let provenance = crate::backend::Provenance { model: cfg.model.clone(), model_sha256: None, prompt: prompt.into(), prompt_sha256: None };
    tokio::spawn(run(cfg, cmd_rx, out_tx));
    Session {
        caps: Capabilities {
            name: d.name(),
            cancel: true,
            audio_in: true,
            audio_out: true,
            opened_policy: d.opened_policy(),
            restore: crate::context::BackendKind::OpenAIRealtime,
            provenance,
            turn_provenance: false,
            flags: d.flags(),
        },
        commands: cmd_tx,
        outputs: out_rx,
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Pending {
    request_id: String,
    response_id: String,
    sent_at: Instant,

    cancelled: bool,
}

struct Live {
    response_id: String,

    cancel_at: Option<Instant>,
    dropped_audio: u64,
    dropped_text: u64,

    audio_parts: Vec<(Option<String>, u64)>,
    provider_samples: u64,

    text_item: Option<String>,
    text_last: Option<char>,
}

impl Live {
    fn new(response_id: String) -> Self {
        Live { response_id, cancel_at: None, dropped_audio: 0, dropped_text: 0, audio_parts: Vec::new(), provider_samples: 0, text_item: None, text_last: None }
    }
}

struct Quarantined {
    why: &'static str,
    dropped_audio: u64,
    dropped_text: u64,
}

enum Waiting {
    Text { request_id: String, response_id: String, text: String, at: Instant },
    Respond { request_id: String, response_id: String, at: Instant },
}

impl Waiting {
    fn response_id(&self) -> &str {
        match self {
            Waiting::Text { response_id, .. } | Waiting::Respond { response_id, .. } => response_id,
        }
    }
    fn at(&self) -> Instant {
        match self {
            Waiting::Text { at, .. } | Waiting::Respond { at, .. } => *at,
        }
    }
}

enum Ended {

    Completed(String),

    Rejected,
}

const MAX_RETIRED: usize = 4096;

struct Utterance {
    id: String,
    seq: u64,
    sent: String,
}

enum End {
    Shutdown,
    Reconnect(String),
}

struct State {
    cfg: Config,
    out: mpsc::Sender<Output>,

    tool_calls: HashMap<String, String>,
    generation: u64,

    context_losses: u64,
    next_event: u64,
    pending: VecDeque<Pending>,

    live: HashMap<String, Live>,
    quarantined: HashMap<String, Quarantined>,

    given_up: HashMap<String, String>,

    client_events: VecDeque<(String, String, &'static str)>,
    waiting: VecDeque<Waiting>,
    opened: u64,
    utterances: HashMap<String, Utterance>,
    utterance_count: u64,

    items: HashMap<String, String>,
    item_order: VecDeque<String>,

    ended_items: HashMap<String, Ended>,

    retired: VecDeque<String>,

    saturated: bool,

    item_prev: HashMap<String, Option<String>>,
    item_response: HashMap<String, String>,

    local_split: bool,
    resampler: Option<Resampler>,
    audio_dropped_offline: u64,

    audio_refused: u64,

    attempts: std::collections::HashMap<String, crate::spend::Attempt>,

    settled: std::collections::HashMap<String, crate::spend::Attempt>,
    history: Option<Vec<crate::context::Turn>>,
    had_session: bool,

    unknown_logged: HashSet<String>,
}

impl State {
    fn new(cfg: Config, out: mpsc::Sender<Output>) -> Self {
        State {
            cfg,
            out,
            tool_calls: HashMap::new(),
            generation: 0,
            context_losses: 0,
            next_event: 0,
            pending: VecDeque::new(),
            live: HashMap::new(),
            quarantined: HashMap::new(),
            given_up: HashMap::new(),
            client_events: VecDeque::new(),
            waiting: VecDeque::new(),
            opened: 0,
            utterances: HashMap::new(),
            utterance_count: 0,
            items: HashMap::new(),
            item_order: VecDeque::new(),
            ended_items: HashMap::new(),
            retired: VecDeque::new(),
            saturated: false,
            item_prev: HashMap::new(),
            item_response: HashMap::new(),
            local_split: false,
            resampler: None,
            audio_dropped_offline: 0,
            audio_refused: 0,
            attempts: std::collections::HashMap::new(),
            settled: std::collections::HashMap::new(),
            history: None,
            had_session: false,
            unknown_logged: HashSet::new(),
        }
    }

    async fn emit(&self, out: Output) {
        let _ = self.out.send(out).await;
    }

    async fn log(&self, level: &'static str, msg: String) {
        self.emit(Output::Log { level, msg }).await;
    }

    fn event_id(&mut self, kind: &str) -> String {
        self.next_event += 1;
        format!("rth-{}-{kind}{}", self.generation, self.next_event)
    }

    fn tagged_event(&mut self, kind: &'static str, response_id: &str) -> String {
        let ev = self.event_id(kind);
        if self.client_events.len() >= MAX_TRACKED * 4 {
            self.client_events.pop_front();
        }
        self.client_events.push_back((ev.clone(), response_id.to_string(), kind));
        ev
    }

    async fn fail_local(&self, response_id: &str, reason: &str) {
        let diag = Output::Diagnostic {
            response_id: response_id.to_string(),
            session_generation: self.generation,
            provider_status: None,
            provider_reason: None,
            outcome_source: "local",
            local_reason: Some(reason.to_string()),
            truncated: None,
            raw_text: None,
            history_guard_applied: false,
        };
        self.emit(diag).await;
        self.emit(Output::Done { response_id: response_id.to_string(), status: ResponseStatus::Failed }).await;
    }

    fn remember_given_up(&mut self, provider_id: String, response_id: String) {
        if self.given_up.len() >= MAX_TRACKED
            && let Some(k) = self.given_up.keys().next().cloned()
        {
            self.given_up.remove(&k);
        }
        self.given_up.insert(provider_id, response_id);
    }

    fn quarantine(&mut self, provider_id: String, why: &'static str) {
        if self.quarantined.len() >= MAX_TRACKED
            && let Some(k) = self.quarantined.keys().next().cloned()
        {
            self.quarantined.remove(&k);
        }
        self.quarantined.insert(provider_id, Quarantined { why, dropped_audio: 0, dropped_text: 0 });
    }

    fn newest_live(&self) -> Option<String> {
        self.live.values().map(|l| l.response_id.clone()).next()
    }

    async fn on_transcript(&mut self, item_id: Option<String>, text: String, finished: bool) {
        let key = item_id.unwrap_or_default();
        if self.saturated {
            self.log("warn", format!("openai transcript event for item {key} refused: item memory is saturated and the session is being reset")).await;
            return;
        }

        if !key.is_empty() {
            let ended = match self.ended_items.get(&key) {
                Some(Ended::Completed(whole)) if finished && *whole == text => Some("a duplicate of its completion; ignored"),
                Some(Ended::Completed(_)) => Some("after its completion; refused"),
                Some(Ended::Rejected) => Some("after its completion was rejected; refused"),
                None if self.retired.contains(&key) => Some("for an item no longer tracked; refused"),
                None => None,
            };
            if let Some(why) = ended {
                let kind = if finished { "completed" } else { "delta" };
                self.log("warn", format!("openai transcript {kind} for item {key} {why}")).await;
                return;
            }
        }
        let boundary = if !self.utterances.contains_key(&key) && std::mem::take(&mut self.local_split) { Some("local") } else { None };
        let uid = self.utterance_of(&key);
        let u = self.utterances.entry(key.clone()).or_insert_with(|| Utterance { id: uid, seq: 0, sent: String::new() });

        let part = if finished {
            match text.strip_prefix(u.sent.as_str()) {
                Some(rest) => rest.to_string(),
                None if u.sent.is_empty() => text,
                None => {
                    let msg = format!(
                        "openai transcript of item {key} (utterance {}) completed as {:?}, which does not extend the {:?} sent as parts; the parts stay unfinished and nothing is final",
                        u.id,
                        clip(&text),
                        clip(&u.sent)
                    );
                    self.utterances.remove(&key);
                    if !key.is_empty() {
                        self.ended_items.insert(key.clone(), Ended::Rejected);
                    }
                    self.local_split = true;
                    self.log("warn", msg).await;
                    return;
                }
            }
        } else {
            text
        };
        if part.is_empty() && !finished {
            return;
        }
        u.sent.push_str(&part);

        let heard = Heard {
            utterance_id: Some(u.id.clone()),
            seq: Some(u.seq),
            finished: if finished { Some(true) } else { None },
            response_id: None,
            association: Some("unknown"),
            boundary,
        };
        u.seq += 1;
        if finished {
            let whole = u.sent.clone();
            self.utterances.remove(&key);
            if !key.is_empty() {
                self.ended_items.insert(key.clone(), Ended::Completed(whole));
            }
        }
        self.emit(Output::InputTranscript { text: part, heard }).await;
    }

    fn utterance_of(&mut self, item: &str) -> String {
        if let Some(uid) = self.items.get(item) {
            return uid.clone();
        }
        self.utterance_count += 1;
        let uid = format!("{}u{}", self.cfg.response_prefix.trim_end_matches('v'), self.utterance_count);
        if self.item_order.len() >= MAX_TRACKED
            && let Some(old) = self.item_order.pop_front()
        {
            self.items.remove(&old);
            self.ended_items.remove(&old);
            if self.retired.len() >= MAX_RETIRED {

                self.saturated = true;
                self.retired.pop_front();
            }
            self.retired.push_back(old);
        }
        self.items.insert(item.to_string(), uid.clone());
        self.item_order.push_back(item.to_string());
        uid
    }

    async fn on_item(&mut self, item: &str) {
        let (Some(prev), Some(pid)) = (self.item_prev.get(item).cloned(), self.item_response.get(item).cloned()) else {
            if self.item_prev.len() + self.item_response.len() > MAX_TRACKED * 2 {
                self.item_prev.clear();
                self.item_response.clear();
            }
            return;
        };
        self.item_prev.remove(item);
        self.item_response.remove(item);
        let rid = self.live.get(&pid).map(|l| l.response_id.clone()).unwrap_or_else(|| "-".into());
        let after = prev.as_deref().unwrap_or("-");
        let uid = prev.as_deref().and_then(|p| self.items.get(p)).map(String::as_str).unwrap_or("-");
        let msg = format!("openai item position (adjacency only, not an answers-to link): response {pid} ({rid}) output item {item} follows item {after} (utterance {uid})");
        self.log("debug", msg).await;
    }

    async fn on_created(&mut self, ws: &mut Ws, provider_id: String, rth_request: Option<String>, rth_gen: Option<String>) -> Result<(), String> {
        let ours = rth_gen.as_deref() == Some(self.generation.to_string().as_str());
        if let Some(req) = rth_request.as_deref() {
            if let Some(i) = self.pending.iter().position(|p| ours && p.request_id == req) {
                let p = self.pending.remove(i).unwrap();
                self.emit(Output::Started { response_id: p.response_id.clone(), request_id: p.request_id }).await;
                let mut live = Live::new(p.response_id);
                if p.cancelled {
                    live.cancel_at = Some(Instant::now());
                    let ev = self.tagged_event("cancel", &live.response_id);
                    send(ws, response_cancel(&ev, &provider_id)).await?;
                }
                self.live.insert(provider_id, live);
                return Ok(());
            }

            let why = if !ours { "metadata from another session" } else { "no such pending request" };
            self.log("warn", format!("openai response {provider_id} quarantined: {why} (rth_request {req}); cancelling it")).await;
            self.quarantine(provider_id.clone(), "unbound metadata");
            let ev = self.event_id("cancel");
            return send(ws, response_cancel(&ev, &provider_id)).await;
        }

        if self.cfg.create_response && self.pending.is_empty() {
            self.opened += 1;
            let response_id = format!("{}{}", self.cfg.response_prefix, self.opened);
            self.emit(Output::Opened { response_id: response_id.clone(), utterance_id: None }).await;
            self.live.insert(provider_id, Live::new(response_id));
            return Ok(());
        }
        if self.cfg.create_response {
            self.log("warn", format!("openai response {provider_id} quarantined: no metadata while {} request(s) are pending (fail closed)", self.pending.len())).await;
            self.quarantine(provider_id, "ambiguous");
            return Ok(());
        }

        self.log("warn", format!("openai anomaly: unsolicited response {provider_id} with create_response=false; quarantined and cancelled")).await;
        self.quarantine(provider_id.clone(), "unsolicited");
        let ev = self.event_id("cancel");
        send(ws, response_cancel(&ev, &provider_id)).await
    }

    async fn spend_done(&mut self, provider_id: &str, usage: Option<&Value>) {
        let ours = self.live.get(provider_id).map(|l| l.response_id.clone()).or_else(|| self.given_up.get(provider_id).cloned());

        let attempt = ours.as_ref().and_then(|r| self.attempts.remove(r)).or_else(|| self.settled.get(provider_id).cloned());
        let Some(sp) = self.cfg.spend.as_mut() else { return };
        let r = match attempt {
            Some(a) => {
                let r = sp.settle(&a, provider_id, usage).map(|c| format!("spend: attempt {} settled at {c} pUSD", a.n));
                self.settled.insert(provider_id.to_string(), a);
                r
            }
            None => Err(sp.anomaly(provider_id, usage)),
        };
        match r {
            Ok(m) => self.log("info", m).await,
            Err(why) => {
                self.log("error", why.clone()).await;
                self.emit(Output::Error { detail: why }).await;
            }
        }
    }

    async fn on_done(&mut self, provider_id: String, status: String, reason: Option<String>, usage: Option<Value>) {
        if let Some(u) = &usage {
            self.log("info", format!("openai usage gen={} response={provider_id} {}", self.generation, clip(&u.to_string()))).await;
        }
        if self.cfg.spend.is_some() {
            self.spend_done(&provider_id, usage.as_ref()).await;
        }
        if let Some(path) = self.cfg.usage_log.clone() {
            let ours = self.live.get(&provider_id).map(|l| ("live", l.response_id.clone()))
                .or_else(|| self.given_up.get(&provider_id).map(|r| ("given_up", r.clone())))
                .or_else(|| self.quarantined.get(&provider_id).map(|_| ("quarantined", String::new())));
            let row = json!({
                "kind": "usage",
                "generation": self.generation,
                "response_prefix": self.cfg.response_prefix,
                "provider_response_id": provider_id,
                "rth_response_id": ours.as_ref().map(|o| o.1.clone()).filter(|r| !r.is_empty()),
                "binding": ours.map(|o| o.0).unwrap_or("unbound"),
                "status": status,
                "complete": usage.as_ref().is_some_and(usage_complete),
                "usage": usage,
            });
            if let Err(e) = append_synced(&path, &row) {
                self.log("error", format!("spend: usage record not written ({e}); response={provider_id}")).await;
            }
        }
        if let Some(live) = self.live.remove(&provider_id) {
            let mapped = map_status(&status);
            if live.cancel_at.is_some() {
                let msg = format!(
                    "openai stop of {}: provider outcome {status} (reason {}) {} ms after response.cancel; dropped {} audio, {} text deltas",
                    live.response_id,
                    reason.as_deref().unwrap_or("none"),
                    live.cancel_at.map(|t| t.elapsed().as_millis()).unwrap_or(0),
                    live.dropped_audio,
                    live.dropped_text
                );
                self.log("info", msg).await;
            }
            if live.audio_parts.len() > 1 {
                self.log("debug", format!("openai response {} carried {} audio parts ({} samples)", live.response_id, live.audio_parts.len(), live.provider_samples)).await;
            }
            if mapped == ResponseStatus::Cancelled && reason.as_deref() == Some("turn_detected") {
                self.emit(Output::Signal { response_id: Some(live.response_id.clone()), signal: "interrupted" }).await;
            }
            if status != "completed" {
                let diag = Output::Diagnostic {
                    response_id: live.response_id.clone(),
                    session_generation: self.generation,
                    provider_status: Some(status),
                    provider_reason: reason,
                    outcome_source: "provider",
                    local_reason: None,
                    truncated: None,
                    raw_text: None,
                    history_guard_applied: false,
                };
                self.emit(diag).await;
            }
            self.emit(Output::Done { response_id: live.response_id, status: mapped }).await;
        } else if let Some(rid) = self.given_up.remove(&provider_id) {

            self.quarantined.remove(&provider_id);
            let diag = Output::Diagnostic {
                response_id: rid,
                session_generation: self.generation,
                provider_status: Some(status),
                provider_reason: reason,
                outcome_source: "provider",
                local_reason: Some("after_local_terminal".into()),
                truncated: None,
                raw_text: None,
                history_guard_applied: false,
            };
            self.emit(diag).await;
        } else if let Some(q) = self.quarantined.remove(&provider_id) {
            let msg = format!("openai quarantined response {provider_id} ({}) ended {status}; dropped {} audio, {} text deltas", q.why, q.dropped_audio, q.dropped_text);
            self.log("info", msg).await;
        } else {

            self.log("debug", format!("openai response.done for unknown or ended response {provider_id} ({status})")).await;
        }
    }

    async fn on_output(&mut self, provider_id: Option<String>, audio: Option<(Option<String>, u64, Vec<u8>)>, text: Option<(Option<String>, String)>) {
        let Some(pid) = provider_id else {
            self.log("debug", "openai output without a response id dropped".into()).await;
            return;
        };
        if let Some(q) = self.quarantined.get_mut(&pid) {
            q.dropped_audio += audio.is_some() as u64;
            q.dropped_text += text.is_some() as u64;
            return;
        }
        let Some(live) = self.live.get_mut(&pid) else { return };
        if live.cancel_at.is_some() {
            live.dropped_audio += audio.is_some() as u64;
            live.dropped_text += text.is_some() as u64;
            return;
        }
        let rid = live.response_id.clone();

        let text = text.map(|(item, t)| {
            let new_item = item.is_some() && live.text_item.is_some() && item != live.text_item;
            let sep = new_item && live.text_last.is_some_and(|c| !c.is_whitespace()) && !t.starts_with(char::is_whitespace);
            if item.is_some() {
                live.text_item = item;
            }
            if let Some(c) = t.chars().last() {
                live.text_last = Some(c);
            }
            (sep, t)
        });
        if let Some((item, index, pcm)) = audio {
            if !live.audio_parts.iter().any(|(i, c)| *i == item && *c == index) {
                live.audio_parts.push((item, index));
            }
            live.provider_samples += (pcm.len() / 2) as u64;
            self.emit(Output::Audio { response_id: rid.clone(), sample_rate: OPENAI_RATE, pcm }).await;
        }
        if let Some((sep, t)) = text {
            if sep {
                self.emit(Output::Delta { response_id: rid.clone(), text: " ".into() }).await;
            }
            self.emit(Output::Delta { response_id: rid, text: t }).await;
        }
    }

    async fn on_error(&mut self, client_event: Option<String>, error_type: Option<String>, code: String, message: String) -> Result<(), String> {
        let found = client_event.as_deref().and_then(|e| self.client_events.iter().find(|(id, _, _)| id == e)).map(|(_, r, k)| (r.clone(), *k));
        let detail = format!("openai error {code}: {}", clip(&message));
        let Some((rid, kind)) = found else {
            self.emit(Output::Error { detail }).await;
            return Ok(());
        };
        let Some(i) = self.pending.iter().position(|p| p.response_id == rid) else {

            self.log("warn", format!("openai response {rid}: {kind} {detail}")).await;
            return Ok(());
        };
        let p = self.pending.remove(i).unwrap();
        self.log("error", format!("openai response {rid} failed: {kind} {detail}")).await;
        let ty = error_type.as_deref().unwrap_or("unknown");
        self.fail_local(&rid, &format!("{kind}_rejected: type={ty} code={code}")).await;
        Err(format!("reset after an error on the {kind} of request {}: its create may still run unlabelled", p.request_id))
    }

    async fn on_server(&mut self, ws: &mut Ws, ev: ServerEvent) -> Result<(), String> {
        match ev {
            ServerEvent::SessionCreated | ServerEvent::SessionUpdated => {}
            ServerEvent::SpeechStarted { .. } => {
                let rid = self.newest_live();
                self.emit(Output::Signal { response_id: rid, signal: "voice_activity_start" }).await;
            }
            ServerEvent::SpeechStopped { .. } => {
                let rid = self.newest_live();
                self.emit(Output::Signal { response_id: rid, signal: "voice_activity_end" }).await;
            }
            ServerEvent::Committed { item_id } => {
                let uid = item_id.as_deref().map(|i| self.utterance_of(i));
                self.log("debug", format!("openai input committed item={} utterance={}", item_id.as_deref().unwrap_or("-"), uid.as_deref().unwrap_or("-"))).await;
            }
            ServerEvent::ItemAdded { item_id, previous_item_id, role } => {
                if role.as_deref() == Some("assistant") {
                    self.item_prev.insert(item_id.clone(), previous_item_id);
                    self.on_item(&item_id).await;
                }
            }
            ServerEvent::OutputItemAdded { response_id, item_id } => {
                if self.live.contains_key(&response_id) {
                    self.item_response.insert(item_id.clone(), response_id);
                    self.on_item(&item_id).await;
                }
            }

            ServerEvent::FunctionCall { response_id, call_id, name, arguments } => match self.live.get(&response_id) {
                Some(live) if live.cancel_at.is_none() => {
                    let args = serde_json::from_str(&arguments).unwrap_or(Value::String(arguments));
                    let local = live.response_id.clone();
                    self.tool_calls.insert(call_id.clone(), local.clone());
                    self.emit(Output::ToolCall { response_id: Some(local), generation: self.generation, call_id, name, args }).await;
                }
                Some(_) => self.log("warn", format!("openai function call {call_id} from a cancel-fenced response ({response_id}): ignored, no tool effect")).await,
                None => self.log("warn", format!("openai function call {call_id} from a response that is not runtime-visible ({response_id}): ignored, no tool effect")).await,
            },
            ServerEvent::TranscriptDelta { item_id, delta } => self.on_transcript(item_id, delta, false).await,
            ServerEvent::TranscriptCompleted { item_id, transcript } => self.on_transcript(item_id, transcript, true).await,
            ServerEvent::TranscriptFailed { item_id, detail } => {
                self.log("warn", format!("openai input transcription failed item={}: {}", item_id.as_deref().unwrap_or("-"), clip(&detail))).await;
            }
            ServerEvent::ResponseCreated { response_id, rth_request, rth_gen } => self.on_created(ws, response_id, rth_request, rth_gen).await?,
            ServerEvent::AudioDelta { response_id, item_id, content_index, pcm } => self.on_output(response_id, Some((item_id, content_index, pcm)), None).await,
            ServerEvent::TextDelta { response_id, item_id, delta } => self.on_output(response_id, None, Some((item_id, delta))).await,
            ServerEvent::ResponseDone { response_id, status, reason, usage } => self.on_done(response_id, status, reason, usage).await,
            ServerEvent::Error { client_event, error_type, code, message } => self.on_error(client_event, error_type, code, message).await?,
            ServerEvent::Ignored(_) => {}
            ServerEvent::Unknown(ty) => {
                if self.unknown_logged.insert(ty.clone()) {
                    self.log("debug", format!("openai ignored unknown event type {ty}")).await;
                }
            }
        }
        Ok(())
    }

    async fn request(&mut self, ws: &mut Ws, request_id: String, response_id: String, text: Option<String>, commit: bool) -> Result<(), String> {

        if let Some(sp) = self.cfg.spend.as_mut() {
            let attempt_id = format!("{}:{}:{request_id}", self.cfg.response_prefix, self.generation);
            match sp.admit(&attempt_id, json!({ "request_id": request_id, "rth_response_id": response_id, "generation": self.generation })) {
                Ok(a) => {
                    self.attempts.insert(response_id.clone(), a);
                }
                Err(why) => {
                    self.log("error", format!("openai response {response_id} refused: {why}")).await;
                    self.fail_local(&response_id, "spend_refused").await;
                    return Ok(());
                }
            }
        }
        self.pending.push_back(Pending { request_id: request_id.clone(), response_id: response_id.clone(), sent_at: Instant::now(), cancelled: false });
        let r = self.write_request(ws, &request_id, &response_id, text, commit).await;
        if let Err(e) = &r {

            if let Some(sp) = self.cfg.spend.as_mut()
                && self.attempts.contains_key(&response_id)
            {
                let why = sp.fail(&format!("spend: send of {response_id} failed, delivery unknown ({e})"));
                self.log("error", why).await;
            }
            self.pending.retain(|p| p.response_id != response_id);
            self.log("error", format!("openai response {response_id} failed: {e}")).await;
            self.fail_local(&response_id, "send_failed").await;
        }
        r
    }

    async fn write_request(&mut self, ws: &mut Ws, request_id: &str, response_id: &str, text: Option<String>, commit: bool) -> Result<(), String> {
        if let Some(t) = text {
            let ev = self.tagged_event("item", response_id);
            send(ws, user_text(&t, &ev)).await?;
        }
        if commit {
            let ev = self.tagged_event("commit", response_id);
            send(ws, input_commit(&ev)).await?;
        }
        let ev = self.tagged_event("create", response_id);
        let mut create = response_create_for(&self.cfg, &ev, request_id, self.generation);
        if let Some(a) = self.attempts.get(response_id) {
            create["response"]["metadata"]["rth_attempt"] = json!(a.attempt_id);
        }
        send(ws, create).await
    }

    async fn cancel(&mut self, ws: Option<&mut Ws>, response_id: &str) -> Result<(), String> {
        if let Some(i) = self.waiting.iter().position(|w| w.response_id() == response_id) {

            self.waiting.remove(i);
            self.emit(Output::Done { response_id: response_id.into(), status: ResponseStatus::Cancelled }).await;
            return Ok(());
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.response_id == response_id) {

            p.cancelled = true;
            return Ok(());
        }
        let Some((pid, live)) = self.live.iter_mut().find(|(_, l)| l.response_id == response_id) else {
            self.log("debug", format!("openai stop of {response_id} after its terminal: terminal kept, nothing sent (the late cut is slice 2)")).await;
            return Ok(());
        };
        if live.cancel_at.is_some() {
            return Ok(());
        }
        live.cancel_at = Some(Instant::now());
        let pid = pid.clone();
        let Some(ws) = ws else { return Ok(()) };
        let ev = self.tagged_event("cancel", response_id);
        send(ws, response_cancel(&ev, &pid)).await
    }

    async fn expire(&mut self) -> Option<String> {
        let now = Instant::now();
        let mut reset = None;
        while self.pending.front().is_some_and(|p| now.duration_since(p.sent_at) >= self.cfg.associate_wait) {
            let p = self.pending.pop_front().unwrap();
            self.log("error", format!("openai response {} failed: no echoed response.created within {} ms (unassociated)", p.response_id, self.cfg.associate_wait.as_millis())).await;
            self.fail_local(&p.response_id, "unassociated").await;
            reset = Some(format!("reset after unassociated request {}: its response could arrive unlabelled", p.request_id));
        }
        let expired: Vec<String> =
            self.live.iter().filter(|(_, l)| l.cancel_at.is_some_and(|t| now.duration_since(t) >= self.cfg.cancel_ack)).map(|(k, _)| k.clone()).collect();
        for pid in expired {
            let live = self.live.remove(&pid).unwrap();
            self.log("error", format!("openai stop of {}: no response.done within {} ms (cancel_unacknowledged)", live.response_id, self.cfg.cancel_ack.as_millis())).await;
            self.quarantine(pid.clone(), "after local terminal");
            self.remember_given_up(pid, live.response_id.clone());
            self.fail_local(&live.response_id, "cancel_unacknowledged").await;
        }
        reset
    }

    fn next_deadline(&self) -> Option<Instant> {
        let a = self.pending.front().map(|p| p.sent_at + self.cfg.associate_wait);
        let c = self.live.values().filter_map(|l| l.cancel_at.map(|t| t + self.cfg.cancel_ack)).min();
        a.into_iter().chain(c).min()
    }

    async fn fail_all(&mut self, why: &str) {
        for p in std::mem::take(&mut self.pending) {
            self.log("error", format!("openai response {} failed: {why}", p.response_id)).await;
            self.fail_local(&p.response_id, "session_ended").await;
        }
        for (_, l) in std::mem::take(&mut self.live) {
            self.log("error", format!("openai response {} failed: {why}", l.response_id)).await;
            self.fail_local(&l.response_id, "session_ended").await;
        }
        self.quarantined.clear();
        self.given_up.clear();
        self.items.clear();
        self.item_order.clear();
        self.ended_items.clear();
        self.retired.clear();
        self.saturated = false;
        self.item_prev.clear();
        self.item_response.clear();
        self.client_events.clear();
        self.unknown_logged.clear();
        if !self.utterances.is_empty() {
            self.utterances.clear();
            self.local_split = true;
        }
        self.resampler = None;
    }

    async fn fail_stale_waiting(&mut self) {
        while self.waiting.front().is_some_and(|w| w.at().elapsed() > READY_WAIT) {
            let w = self.waiting.pop_front().unwrap();
            self.log("error", format!("openai response {} failed: no session within {} s", w.response_id(), READY_WAIT.as_secs())).await;
            self.fail_local(w.response_id(), "no_session").await;
        }
    }

    async fn offline(&mut self, cmd: Command) {
        match cmd {
            Command::UserText { request_id, response_id, text, memory, .. } => {

                let (text, applied) = crate::backend::accept_memory(text, memory, false, self.context_losses);
                if let Some(applied) = applied {
                    self.emit(Output::MemoryInjection { request_id: request_id.clone(), response_id: response_id.clone(), applied, text_sha256: crate::backend::text_sha256(&text) }).await;
                }
                self.waiting.push_back(Waiting::Text { request_id, response_id, text, at: Instant::now() })
            }
            Command::Respond { request_id, response_id, .. } => self.waiting.push_back(Waiting::Respond { request_id, response_id, at: Instant::now() }),
            Command::Restore { turns } => self.history = Some(turns),
            Command::SessionMemories { .. } | Command::SavedUnknown | Command::SavedWithheld | Command::SelfPrompts { .. } => {}
            Command::Cancel { response_id } => {
                let _ = self.cancel(None, &response_id).await;
            }
            Command::Audio { .. } => self.audio_dropped_offline += 1,
            Command::AudioEnd { .. } => {}

            Command::ToolOutput { call_id, .. } => self.log("warn", format!("openai tool output for {call_id} dropped: no live session")).await,
        }
    }

    fn resample(&mut self, sample_rate: u32, pcm: &[u8]) -> Vec<u8> {
        let to = self.cfg.dialect.input_rate();
        if self.resampler.as_ref().is_none_or(|r| r.input_rate() != sample_rate) {
            self.resampler = Some(Resampler::new(sample_rate, to));
        }
        self.resampler.as_mut().unwrap().push(pcm)
    }
}

async fn send(ws: &mut Ws, v: Value) -> Result<(), String> {
    ws.send(Message::text(v.to_string())).await.map_err(|e| format!("send failed: {e}"))
}

async fn run(cfg: Config, mut cmd_rx: mpsc::Receiver<Command>, out: mpsc::Sender<Output>) {

    let mut cfg = cfg;
    if let Some(mut gate) = cfg.setup_gate.take() {

        if gate.wait_for(|open| *open).await.is_err() {
            return;
        }
    }
    let connector = if cfg.url.starts_with("wss://") {
        let _ = rustls::crypto::ring::default_provider().install_default();
        match tls_connector(cfg.ca_file.as_deref()) {
            Ok(c) => Some(c),
            Err(e) => {
                let _ = out.send(Output::Error { detail: format!("openai TLS roots: {e:#}") }).await;
                None
            }
        }
    } else {
        None
    };
    let mut st = State::new(cfg, out);
    let mut backoff = Duration::from_millis(500);
    loop {
        st.generation += 1;
        let opened = Instant::now();

        let setup = setup_message(&st.cfg);
        let sent_identity = setup_identity_of(&st.cfg, &setup);
        let dial = Dial { url: st.cfg.url.clone(), api_key: st.cfg.api_key.clone(), setup };
        let connected = tokio::select! {
            r = open(dial, connector.clone()) => r,
            closed = async {
                loop {
                    match cmd_rx.recv().await {
                        None => return true,
                        Some(cmd) => st.offline(cmd).await,
                    }
                }
            } => {
                if closed { return; }
                continue;
            }
        };
        let ws = match connected {
            Ok((ws, ws_ms, setup_ms)) => {
                backoff = Duration::from_millis(500);
                if st.had_session {
                    st.context_losses += 1;
                    st.emit(Output::ContextLost { why: format!("provider session gen={} started (no native resumption)", st.generation) }).await;
                }
                st.had_session = true;
                let msg = format!(
                    "openai-realtime ready gen={} dialect={} model={} ws_connect_ms={ws_ms:.1} setup_ms={setup_ms:.1} setup_to_ready_ms={:.1} {} setup_id={} price_table={PRICE_TABLE} caps: {}",
                    st.generation,
                    st.cfg.dialect.name(),
                    st.cfg.model,
                    opened.elapsed().as_secs_f64() * 1000.0,
                    st.cfg.knobs(),
                    sent_identity,
                    st.cfg.dialect.flags().summary()
                );
                st.log("info", msg).await;
                if st.audio_dropped_offline > 0 {
                    let n = std::mem::take(&mut st.audio_dropped_offline);
                    st.log("warn", format!("openai dropped {n} microphone chunks while no session was live")).await;
                }
                ws
            }
            Err(e) => {
                st.log("error", format!("openai connect gen={} failed: {e:#}", st.generation)).await;
                st.fail_stale_waiting().await;
                let wait = tokio::time::sleep(backoff);
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        _ = &mut wait => break,
                        cmd = cmd_rx.recv() => match cmd {
                            None => return,
                            Some(cmd) => st.offline(cmd).await,
                        },
                    }
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        match session(&mut st, ws, &mut cmd_rx).await {
            End::Shutdown => return,
            End::Reconnect(why) => {
                st.log("warn", format!("openai session gen={} ended: {why}", st.generation)).await;
                st.fail_all(&why).await;
            }
        }
    }
}

struct Dial {
    url: String,
    api_key: Option<String>,
    setup: Value,
}

async fn open(dial: Dial, connector: Option<Connector>) -> Result<(Ws, f64, f64)> {
    let mut req = dial.url.as_str().into_client_request().context("bad RTH_OPENAI_URL")?;
    if let Some(key) = &dial.api_key {
        let mut value = HeaderValue::from_str(&format!("Bearer {key}")).context("OPENAI_API_KEY is not a valid header value")?;
        value.set_sensitive(true);
        req.headers_mut().insert("authorization", value);
    }
    let t0 = Instant::now();
    let (mut ws, _resp) = tokio_tungstenite::connect_async_tls_with_config(req, None, true, connector)
        .await
        .map_err(|e| anyhow::anyhow!("websocket connect: {e}"))?;
    let ws_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    ws.send(Message::text(dial.setup.to_string())).await.context("sending session.update")?;
    let deadline = tokio::time::sleep(SETUP_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => bail!("no session.updated within {} s", SETUP_TIMEOUT.as_secs()),
            msg = ws.next() => {
                let bytes = match msg {
                    None => bail!("closed before session.updated"),
                    Some(Err(e)) => bail!("before session.updated: {e}"),
                    Some(Ok(Message::Close(frame))) => bail!("closed before session.updated: {}", close_reason(frame.as_ref())),
                    Some(Ok(m)) => match payload(m) { Some(b) => b, None => continue },
                };
                match parse_server(&bytes)? {
                    ServerEvent::SessionUpdated => return Ok((ws, ws_ms, t1.elapsed().as_secs_f64() * 1000.0)),
                    ServerEvent::Error { code, message, .. } => bail!("setup refused: {code}: {}", clip(&message)),
                    _ => {}
                }
            }
        }
    }
}

async fn send_history(st: &mut State, ws: &mut Ws) -> Result<(), String> {
    let Some(turns) = st.history.take() else { return Ok(()) };
    if turns.is_empty() {
        return Ok(());
    }
    let (items, sent) = history_items(&turns);
    for item in items {
        if let Err(e) = send(ws, item).await {
            st.history = Some(turns);
            return Err(format!("history {e}"));
        }
    }
    st.emit(Output::ContextInjected { turns: sent.turns, bytes: sent.bytes, omitted: sent.omitted, truncated: sent.truncated }).await;
    Ok(())
}

async fn session(st: &mut State, mut ws: Ws, cmd_rx: &mut mpsc::Receiver<Command>) -> End {

    if let Err(e) = send_history(st, &mut ws).await {
        return End::Reconnect(e);
    }
    while let Some(w) = st.waiting.pop_front() {
        let r = match w {
            Waiting::Text { request_id, response_id, text, .. } => st.request(&mut ws, request_id, response_id, Some(text), false).await,
            Waiting::Respond { request_id, response_id, .. } => {
                let commit = st.cfg.commits_audio();
                st.request(&mut ws, request_id, response_id, None, commit).await
            }
        };
        if let Err(e) = r {
            return End::Reconnect(e);
        }
    }
    let mut last_progress = Instant::now();
    loop {
        if st.history.is_some()
            && st.live.is_empty()
            && st.pending.is_empty()
            && let Err(e) = send_history(st, &mut ws).await
        {
            return End::Reconnect(e);
        }
        let deadline = st.next_deadline();
        let bound = tokio::time::sleep_until(deadline.unwrap_or_else(|| Instant::now() + TURN_STALL).into());
        let stall = tokio::time::sleep_until((last_progress + TURN_STALL).into());
        let busy = !st.live.is_empty() || !st.pending.is_empty();
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let r = match cmd {
                    None => {
                        let _ = ws.close(None).await;
                        return End::Shutdown;
                    }
                    Some(Command::Restore { turns }) => {
                        st.history = Some(turns);
                        Ok(())
                    }
                    Some(Command::SessionMemories { .. } | Command::SavedUnknown | Command::SavedWithheld | Command::SelfPrompts { .. }) => Ok(()),
                    Some(Command::UserText { request_id, response_id, text, memory, .. }) => {
                        if !busy {
                            last_progress = Instant::now();
                        }
                        let (text, applied) = crate::backend::accept_memory(text, memory, true, st.context_losses);
                        if let Some(applied) = applied {
                            st.emit(Output::MemoryInjection { request_id: request_id.clone(), response_id: response_id.clone(), applied, text_sha256: crate::backend::text_sha256(&text) }).await;
                        }
                        st.request(&mut ws, request_id, response_id, Some(text), false).await
                    }
                    Some(Command::Respond { request_id, response_id, decision_id, utterance_id }) => {
                        if !busy {
                            last_progress = Instant::now();
                        }
                        let commit = st.cfg.commits_audio();
                        let msg = format!("openai respond {response_id} for decision {decision_id} utterance {} commit={commit}", utterance_id.as_deref().unwrap_or("-"));
                        st.log("debug", msg).await;
                        st.request(&mut ws, request_id, response_id, None, commit).await
                    }
                    Some(Command::Cancel { response_id }) => st.cancel(Some(&mut ws), &response_id).await,
                    Some(Command::ToolOutput { call_id, output, .. }) => {

                        let rid = st.tool_calls.remove(&call_id).unwrap_or_default();
                        let ev = st.tagged_event("tool_output", &rid);
                        let r = send(&mut ws, function_output(&ev, &call_id, &output)).await;
                        if r.is_ok() {
                            st.emit(Output::ToolOutputSent { call_id }).await;
                        }
                        r
                    }
                    Some(Command::Audio { .. }) if st.cfg.text_only => {
                        st.audio_refused += 1;
                        if st.audio_refused == 1 {
                            st.log("warn", "openai text-only: input audio refused (never sent)".into()).await;
                        }
                        Ok(())
                    }
                    Some(Command::Audio { sample_rate, pcm }) => {
                        let out = st.resample(sample_rate, &pcm);
                        if out.is_empty() { Ok(()) } else { send(&mut ws, audio_append(&out)).await }
                    }

                    Some(Command::AudioEnd { .. }) => {
                        st.resampler = None;
                        Ok(())
                    }
                };
                if let Err(e) = r {
                    return End::Reconnect(e);
                }
            }
            msg = ws.next() => {
                let bytes = match msg {
                    None => return End::Reconnect("socket closed".into()),
                    Some(Err(e)) => return End::Reconnect(format!("socket error: {e}")),
                    Some(Ok(Message::Close(frame))) => return End::Reconnect(format!("server closed: {}", close_reason(frame.as_ref()))),
                    Some(Ok(m)) => match payload(m) { Some(b) => b, None => continue },
                };
                last_progress = Instant::now();
                match parse_server(&bytes) {
                    Ok(ev) => {
                        if let Err(e) = st.on_server(&mut ws, ev).await {
                            return End::Reconnect(e);
                        }
                        if st.saturated {
                            let _ = ws.close(None).await;
                            return End::Reconnect(format!("item memory saturated ({MAX_TRACKED} tracked + {MAX_RETIRED} retired): a new session, so no forgotten item can reopen"));
                        }
                    }
                    Err(e) => st.log("warn", format!("openai unreadable server event: {e:#}")).await,
                }
            }
            _ = bound, if deadline.is_some() => {
                if let Some(why) = st.expire().await {
                    let _ = ws.close(None).await;
                    return End::Reconnect(why);
                }
            }
            _ = stall, if busy => {
                return End::Reconnect(format!("no server event for {} s with an open response", TURN_STALL.as_secs()));
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
