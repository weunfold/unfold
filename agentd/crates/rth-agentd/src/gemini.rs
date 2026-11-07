

use std::collections::VecDeque;
use std::sync::Arc;
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

pub const DEFAULT_URL: &str =
    "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
pub const DEFAULT_MODEL: &str = "gemini-3.8-live";

pub const OUTPUT_RATE: u32 = 24_000;

const SETUP_TIMEOUT: Duration = Duration::from_secs(15);

const TURN_STALL: Duration = Duration::from_secs(45);

const READY_WAIT: Duration = Duration::from_secs(20);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

pub const POLICY_PTBR_ADDRESSEE_V1: &str = "[rth-voice-policy ptbr-addressee-v1]
You are RTH, a voice assistant. You hear a room: the user may also be talking with other people there, or with someone on a phone call on speaker, and you may hear them while you are speaking.
Reply only when the user is talking to you: they call you by name (RTH), ask you something directly, or continue an exchange they are clearly having with you. When the speech belongs to a conversation with someone else, or you are not sure it is meant for you, stay silent: produce no audio at all, not even a short acknowledgement. Never answer a voice that is not the user's (for example the other side of a phone call) unless the user asks you to.
Speak Brazilian Portuguese (pt-BR) by default. Reply in the language the user speaks to you; when the user asks you to switch language, switch, and switch back when asked. Do not answer in Spanish unless the user speaks Spanish to you.
Be brief: one or two short sentences, unless the user asks for more.";

pub fn vad_config(knob: &str) -> Option<Value> {
    match knob {
        "silence-v1" => Some(json!({ "automaticActivityDetection": { "disabled": false, "silenceDurationMs": 900 } })),
        "low-v1" => Some(json!({
            "automaticActivityDetection": {
                "disabled": false,
                "startOfSpeechSensitivity": "START_SENSITIVITY_LOW",
                "prefixPaddingMs": 300,
                "silenceDurationMs": 900,
            }
        })),
        _ => None,
    }
}

pub struct Config {
    pub url: String,
    pub model: String,

    api_key: Option<String>,

    pub ca_file: Option<String>,
    pub system_instruction: Option<String>,
    pub resumption: bool,
    pub compression: bool,

    pub response_prefix: String,

    pub vad: String,

    pub policy: String,

    pub proactive: bool,

    pub transcription_langs: Vec<String>,

    pub persona: &'static str,

    pub tools: Vec<crate::tools::Tool>,

    pub setup_gate: Option<tokio::sync::watch::Receiver<bool>>,
}

impl Config {

    pub fn from_env() -> Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let url = var("RTH_GEMINI_URL").unwrap_or_else(|| DEFAULT_URL.to_string());
        let api_key = var("GEMINI_API_KEY");
        if api_key.is_none() && url.starts_with("wss://generativelanguage.googleapis.com") {
            bail!("RTH_BACKEND=gemini needs GEMINI_API_KEY (in the guest: the microsandbox placeholder)");
        }
        Ok(Self {
            url,
            model: var("RTH_GEMINI_MODEL").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            api_key,
            ca_file: var("RTH_CA_FILE"),

            system_instruction: var("RTH_GEMINI_SYSTEM").or_else(|| option_env!("RTH_BUILD_GEMINI_SYSTEM").map(str::to_string)),
            resumption: var("RTH_GEMINI_RESUMPTION").as_deref() != Some("0"),
            compression: var("RTH_GEMINI_COMPRESSION").as_deref() != Some("0"),
            response_prefix: "v".into(),
            vad: var("RTH_GEMINI_VAD").or_else(|| option_env!("RTH_BUILD_GEMINI_VAD").map(str::to_string)).unwrap_or_else(|| "server".into()),
            policy: var("RTH_GEMINI_POLICY").or_else(|| option_env!("RTH_BUILD_GEMINI_POLICY").map(str::to_string)).unwrap_or_else(|| "ptbr-addressee-v1".into()),
            proactive: var("RTH_GEMINI_PROACTIVE").or_else(|| option_env!("RTH_BUILD_GEMINI_PROACTIVE").map(str::to_string)).as_deref() == Some("1"),
            transcription_langs: var("RTH_GEMINI_TRANSCRIPTION_LANGS")
                .or_else(|| option_env!("RTH_BUILD_GEMINI_TRANSCRIPTION_LANGS").map(str::to_string))
                .map(|v| v.split(',').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
                .unwrap_or_default(),
            persona: crate::persona::parse(var("RTH_GEMINI_PERSONA").or_else(|| option_env!("RTH_BUILD_GEMINI_PERSONA").map(str::to_string)).as_deref())
                .map_err(|e| anyhow::anyhow!("RTH_GEMINI_PERSONA: {e}"))?,
            tools: crate::tools::knob(var("RTH_GEMINI_TOOLS").or_else(|| option_env!("RTH_BUILD_GEMINI_TOOLS").map(str::to_string))),
            setup_gate: None,
        })
    }

    pub fn knobs(&self) -> String {
        format!(
            "vad={} policy={} proactive={} transcription_langs={} system_extra={} persona={}{}",
            self.vad,
            self.policy,
            self.proactive,
            if self.transcription_langs.is_empty() { "-".into() } else { self.transcription_langs.join(",") },
            self.system_instruction.is_some(),
            crate::persona::persona_name(self.system_instruction.as_deref(), self.persona),
            crate::tools::label(&self.tools)
        )
    }

    #[cfg(test)]
    pub fn for_test(url: String) -> Self {
        Self {
            url,
            model: DEFAULT_MODEL.into(),
            api_key: Some("test-placeholder".into()),
            ca_file: None,
            system_instruction: None,
            resumption: true,
            compression: false,
            response_prefix: "v".into(),
            vad: "server".into(),
            policy: "ptbr-addressee-v1".into(),
            proactive: false,
            transcription_langs: Vec::new(),
            persona: crate::persona::DEFAULT_PERSONA,
            tools: Vec::new(),
            setup_gate: None,
        }
    }
}

pub fn setup_message(cfg: &Config, resume_handle: Option<&str>) -> Value {
    let mut setup = json!({
        "model": format!("models/{}", cfg.model),
        "generationConfig": { "responseModalities": ["AUDIO"] },
        "outputAudioTranscription": {},
        "inputAudioTranscription": {},
    });
    if !cfg.transcription_langs.is_empty() {
        setup["inputAudioTranscription"] = json!({ "languageCodes": cfg.transcription_langs });
    }
    if let Some(rt) = vad_config(&cfg.vad) {
        setup["realtimeInputConfig"] = rt;
    }
    if cfg.proactive {
        setup["proactivity"] = json!({ "proactiveAudio": true });
    }
    if cfg.resumption {
        setup["sessionResumption"] = match resume_handle {
            Some(h) => json!({ "handle": h }),
            None => json!({}),
        };
    }
    if cfg.compression {
        setup["contextWindowCompression"] = json!({ "slidingWindow": {} });
    }
    let policy = match cfg.policy.as_str() {
        "ptbr-addressee-v1" => Some(POLICY_PTBR_ADDRESSEE_V1),
        _ => None,
    };

    if let Some(text) = crate::persona::system_instruction(policy, cfg.system_instruction.as_deref(), cfg.persona) {
        setup["systemInstruction"] = json!({ "parts": [{ "text": crate::persona::with_tools(text, &cfg.tools) }] });
    }

    if !cfg.tools.is_empty() {
        setup["tools"] = crate::tools::render_gemini(&cfg.tools);
    }
    json!({ "setup": setup })
}

#[cfg(test)]
pub fn setup_identity(cfg: &Config) -> String {
    identity_of(&setup_message(cfg, None))
}

pub fn identity_of(v: &Value) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(v.to_string().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn text_turn(text: &str) -> Value {
    json!({ "clientContent": { "turns": [{ "role": "user", "parts": [{ "text": text }] }], "turnComplete": true } })
}

pub fn audio_input(pcm: &[u8], sample_rate: u32) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(pcm);
    json!({ "realtimeInput": { "audio": { "data": data, "mimeType": format!("audio/pcm;rate={sample_rate}") } } })
}

pub fn audio_stream_end() -> Value {
    json!({ "realtimeInput": { "audioStreamEnd": true } })
}

#[derive(Debug, Default, PartialEq)]
pub struct ServerMsg {
    pub setup_complete: bool,

    pub audio: Vec<(u32, Vec<u8>)>,
    pub transcript: Option<String>,

    pub input_transcript: Option<String>,

    pub input_finished: Option<bool>,
    pub interrupted: bool,
    pub turn_complete: bool,
    pub generation_complete: bool,
    pub go_away: Option<String>,

    pub resume_handle: Option<String>,
    pub total_tokens: Option<u64>,

    pub voice_activity: Option<String>,

    pub other: Vec<String>,

    pub tool_calls: Vec<(String, String, Value)>,

    pub tool_cancellations: Vec<String>,
}

pub fn parse_server(bytes: &[u8]) -> Result<ServerMsg> {
    let v: Value = serde_json::from_slice(bytes).context("server message is not JSON")?;
    let obj = v.as_object().context("server message is not an object")?;
    let mut m = ServerMsg::default();
    for (key, val) in obj {
        match key.as_str() {
            "setupComplete" => m.setup_complete = true,
            "serverContent" => {
                if let Some(parts) = val.pointer("/modelTurn/parts").and_then(Value::as_array) {
                    for part in parts {
                        let Some(inline) = part.get("inlineData") else { continue };
                        let mime = inline.get("mimeType").and_then(Value::as_str).unwrap_or("");
                        if !mime.starts_with("audio/pcm") {
                            m.other.push(format!("inlineData {mime}"));
                            continue;
                        }
                        let data = inline.get("data").and_then(Value::as_str).unwrap_or("");
                        let pcm = base64::engine::general_purpose::STANDARD.decode(data).context("audio part is not base64")?;
                        m.audio.push((rate_of(mime), pcm));
                    }
                }
                m.transcript = val.pointer("/outputTranscription/text").and_then(Value::as_str).map(str::to_string);
                m.input_transcript = val.pointer("/inputTranscription/text").and_then(Value::as_str).map(str::to_string);
                m.input_finished = val.pointer("/inputTranscription/finished").and_then(Value::as_bool);
                m.interrupted = val.get("interrupted").and_then(Value::as_bool).unwrap_or(false);
                m.turn_complete = val.get("turnComplete").and_then(Value::as_bool).unwrap_or(false);
                m.generation_complete = val.get("generationComplete").and_then(Value::as_bool).unwrap_or(false);
            }
            "goAway" => m.go_away = Some(val.get("timeLeft").and_then(Value::as_str).unwrap_or("?").to_string()),
            "sessionResumptionUpdate" => {
                let resumable = val.get("resumable").and_then(Value::as_bool).unwrap_or(false);
                let handle = val.get("newHandle").and_then(Value::as_str).filter(|h| !h.is_empty());
                if resumable {
                    m.resume_handle = handle.map(str::to_string);
                }
            }
            "usageMetadata" => m.total_tokens = val.get("totalTokenCount").and_then(Value::as_u64),
            "toolCall" => {
                for c in val.get("functionCalls").and_then(Value::as_array).into_iter().flatten() {
                    let id = c.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    let name = c.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    m.tool_calls.push((id, name, c.get("args").cloned().unwrap_or(Value::Null)));
                }

                if m.tool_calls.is_empty() {
                    m.other.push("toolCall".into());
                }
            }
            "toolCallCancellation" => {
                m.tool_cancellations = val.get("ids").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            }
            "voiceActivity" => {
                let ty = val.get("type").or_else(|| val.get("voiceActivityType")).and_then(Value::as_str).unwrap_or("TYPE_UNSPECIFIED");
                m.voice_activity = Some(ty.to_string());
            }
            other => m.other.push(other.to_string()),
        }
    }
    Ok(m)
}

fn rate_of(mime: &str) -> u32 {
    mime.split(';').find_map(|p| p.trim().strip_prefix("rate=")).and_then(|r| r.parse().ok()).unwrap_or(OUTPUT_RATE)
}

pub fn connect(cfg: Config) -> Session {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);
    let (out_tx, out_rx) = mpsc::channel::<Output>(1024);
    let prompt = crate::persona::persona_name(cfg.system_instruction.as_deref(), cfg.persona);
    let provenance = crate::backend::Provenance { model: cfg.model.clone(), model_sha256: None, prompt: prompt.into(), prompt_sha256: None };
    tokio::spawn(run(cfg, cmd_rx, out_tx));
    Session {
        caps: Capabilities {
            name: "gemini-live@v1",
            cancel: true,
            audio_in: true,
            audio_out: true,
            opened_policy: "provider:gemini-auto-vad@v1",
            restore: crate::context::BackendKind::Gemini,
            provenance,
            turn_provenance: false,
            flags: Flags {

                explicit_replies: Cap::No(Evidence::Documented),
                live_turn_options: Cap::No(Evidence::Documented),
                native_resumption: Cap::Yes(Evidence::Documented),
                dynamic_tools: Cap::No(Evidence::Documented),
                truncate: Cap::No(Evidence::Documented),
                ..Flags::UNKNOWN
            },
        },
        commands: cmd_tx,
        outputs: out_rx,
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Turn {
    response_id: String,

    cancelled_at: Option<Instant>,
    dropped_audio: u64,
    dropped_text: u64,
}

enum End {

    Shutdown,

    Reconnect(String),
}

struct State {
    cfg: Config,
    out: mpsc::Sender<Output>,

    turns: VecDeque<Turn>,

    waiting: VecDeque<(String, String, String, Instant)>,
    resume_handle: Option<String>,
    generation: u64,

    context_losses: u64,

    after_interrupt: bool,
    go_away: bool,

    opened: u64,

    audio_dropped_offline: u64,

    utterance: Option<(String, u64)>,
    utterances: u64,

    next_boundary: Option<&'static str>,

    activity_seen: bool,

    history: Option<Vec<crate::context::Turn>>,

    had_session: bool,
}

const MAX_UTTERANCE_PARTS: u64 = 256;

const MAX_HISTORY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySent {
    pub turns: u64,
    pub bytes: u64,

    pub omitted: u64,

    pub truncated: u64,
}

pub fn history_message(turns: &[crate::context::Turn]) -> (Value, HistorySent) {
    let (kept, sent) = settled_history(turns);
    let kept: Vec<Value> = kept
        .iter()
        .map(|t| json!({ "role": if t.role == "model" { "model" } else { "user" }, "parts": [{ "text": t.text }] }))
        .collect();
    (json!({ "clientContent": { "turns": kept, "turnComplete": false } }), sent)
}

pub fn settled_history(turns: &[crate::context::Turn]) -> (Vec<&crate::context::Turn>, HistorySent) {
    let settled: Vec<_> = turns
        .iter()
        .filter(|t| match t.role.as_str() {
            "model" => t.status.as_deref() == Some("completed"),

            _ => t.kind != "speech" || (t.finished == Some(true) && t.boundary.is_none()),
        })
        .collect();
    let omitted = (turns.len() - settled.len()) as u64;
    let mut kept = Vec::new();
    let mut bytes = 0usize;
    for t in settled.iter().rev() {
        if bytes + t.text.len() > MAX_HISTORY_BYTES {
            break;
        }
        bytes += t.text.len();
        kept.push(*t);
    }
    kept.reverse();
    let sent = HistorySent { turns: kept.len() as u64, bytes: bytes as u64, omitted, truncated: (settled.len() - kept.len()) as u64 };
    (kept, sent)
}

impl State {
    async fn emit(&self, out: Output) {
        let _ = self.out.send(out).await;
    }

    async fn log(&self, level: &'static str, msg: String) {
        self.emit(Output::Log { level, msg }).await;
    }

    async fn fail_open_turns(&mut self, why: &str) {
        while let Some(t) = self.turns.pop_front() {
            if t.cancelled_at.is_none() {
                self.emit(Output::Done { response_id: t.response_id.clone(), status: ResponseStatus::Failed }).await;
                self.log("error", format!("gemini response {} failed: {why}", t.response_id)).await;
            }
        }
    }

    async fn cancel(&mut self, response_id: &str) {
        if let Some(i) = self.waiting.iter().position(|(_, r, _, _)| r == response_id) {
            self.waiting.remove(i);
            self.emit(Output::Done { response_id: response_id.into(), status: ResponseStatus::Cancelled }).await;
            return;
        }
        let Some(turn) = self.turns.iter_mut().find(|t| t.response_id == response_id) else { return };
        if turn.cancelled_at.is_none() {
            turn.cancelled_at = Some(Instant::now());
            self.emit(Output::Done { response_id: response_id.into(), status: ResponseStatus::Cancelled }).await;
        }
    }

    async fn close_front(&mut self, status: ResponseStatus, how: &str) {
        let Some(t) = self.turns.pop_front() else { return };
        match t.cancelled_at {
            None => self.emit(Output::Done { response_id: t.response_id.clone(), status }).await,
            Some(at) => {
                let msg = format!(
                    "gemini cancelled response {}: server {how} {} ms after cancel; dropped {} audio parts, {} transcript parts",
                    t.response_id,
                    at.elapsed().as_millis(),
                    t.dropped_audio,
                    t.dropped_text
                );
                self.log("info", msg).await;
            }
        }
    }

    fn end_utterance_at_turn(&mut self) {
        if self.utterance.take().is_some() {
            self.next_boundary = Some("provider_turn");
        }
    }

    async fn on_server(&mut self, mut m: ServerMsg) {

        for (call_id, name, args) in std::mem::take(&mut m.tool_calls) {
            let response_id = self.turns.front().map(|t| t.response_id.clone());
            self.emit(Output::ToolCall { response_id, generation: self.generation, call_id, name, args }).await;
        }
        if !m.tool_cancellations.is_empty() {
            self.emit(Output::ToolCancelled { call_ids: std::mem::take(&mut m.tool_cancellations) }).await;
        }

        if m.voice_activity.as_deref() == Some("ACTIVITY_START") {
            self.activity_seen = true;
            self.end_utterance_at_turn();
        }
        let has_output = !m.audio.is_empty() || m.transcript.is_some();
        if has_output {
            self.after_interrupt = false;
        }
        if let Some(text) = m.input_transcript.filter(|t| !t.is_empty()) {

            let boundary = if self.utterance.is_none() { self.next_boundary.take() } else { None };
            let (id, seq) = match self.utterance.take() {
                Some((id, n)) => (id, n),
                None => {
                    self.utterances += 1;
                    (format!("{}u{}", self.cfg.response_prefix.trim_end_matches('v'), self.utterances), 0)
                }
            };
            let finished = m.input_finished;
            if finished != Some(true) {
                if seq + 1 >= MAX_UTTERANCE_PARTS {
                    self.next_boundary = Some("local");
                } else {
                    self.utterance = Some((id.clone(), seq + 1));
                }
            }
            let heard = Heard { utterance_id: Some(id), seq: Some(seq), finished, response_id: None, association: Some("unknown"), boundary };
            self.emit(Output::InputTranscript { text, heard }).await;
        }

        if has_output && self.turns.is_empty() {
            self.opened += 1;
            let response_id = format!("{}{}", self.cfg.response_prefix, self.opened);
            self.emit(Output::Opened { response_id: response_id.clone(), utterance_id: None }).await;
            self.turns.push_back(Turn { response_id, cancelled_at: None, dropped_audio: 0, dropped_text: 0 });
        }
        match self.turns.front_mut() {
            Some(t) if t.cancelled_at.is_some() => {
                t.dropped_audio += m.audio.len() as u64;
                t.dropped_text += m.transcript.is_some() as u64;
            }
            Some(t) => {
                let rid = t.response_id.clone();
                for (sample_rate, pcm) in m.audio {
                    self.emit(Output::Audio { response_id: rid.clone(), sample_rate, pcm }).await;
                }
                if let Some(text) = m.transcript {
                    self.emit(Output::Delta { response_id: rid, text }).await;
                }
            }
            None => {}
        }

        if let Some(ty) = m.voice_activity.as_deref() {
            let signal = match ty {
                "ACTIVITY_START" => Some("voice_activity_start"),
                "ACTIVITY_END" => Some("voice_activity_end"),
                _ => None,
            };
            match signal {
                Some(signal) => {
                    let rid = self.turns.front().map(|t| t.response_id.clone());
                    self.emit(Output::Signal { response_id: rid, signal }).await;
                }
                None => self.log("debug", format!("gemini voiceActivity of type {ty}")).await,
            }
        }
        if let Some(tokens) = m.total_tokens {
            self.log("info", format!("gemini usage gen={} total_tokens={tokens}", self.generation)).await;
        }

        if (m.interrupted || m.turn_complete) && !self.activity_seen {
            self.end_utterance_at_turn();
        }
        if m.interrupted {
            let rid = self.turns.front().map(|t| t.response_id.clone());
            self.emit(Output::Signal { response_id: rid, signal: "interrupted" }).await;
            self.close_front(ResponseStatus::Cancelled, "interrupted").await;

            self.after_interrupt = !m.turn_complete;
        }
        if m.turn_complete {
            if self.after_interrupt && !has_output && !m.interrupted {
                self.after_interrupt = false;
                self.emit(Output::Signal { response_id: None, signal: "turn_complete" }).await;
            } else if !m.interrupted {
                let rid = self.turns.front().map(|t| t.response_id.clone());
                self.emit(Output::Signal { response_id: rid, signal: "turn_complete" }).await;
                self.close_front(ResponseStatus::Completed, "turnComplete").await;
            }
        }
        if let Some(h) = m.resume_handle {
            self.resume_handle = Some(h);
        }
        if let Some(left) = m.go_away {
            self.go_away = true;
            self.log("warn", format!("gemini goAway (timeLeft {left}); reconnecting when idle")).await;
        }
        if !m.other.is_empty() {
            self.log("debug", format!("gemini ignored {:?}", m.other)).await;
        }
    }
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
                let _ = out.send(Output::Error { detail: format!("gemini TLS roots: {e:#}") }).await;
                None
            }
        }
    } else {
        None
    };
    let mut st = State {
        cfg,
        out,
        turns: VecDeque::new(),
        waiting: VecDeque::new(),
        resume_handle: None,
        generation: 0,
        context_losses: 0,
        after_interrupt: false,
        go_away: false,
        opened: 0,
        audio_dropped_offline: 0,
        utterance: None,
        utterances: 0,
        next_boundary: None,
        activity_seen: false,
        history: None,
        had_session: false,
    };
    let mut backoff = Duration::from_millis(500);
    loop {
        st.generation += 1;
        let opened = Instant::now();

        let base = setup_message(&st.cfg, None);
        let sent_identity = identity_of(&base);
        let mut sent_setup = base;
        if st.cfg.resumption
            && let Some(h) = st.resume_handle.as_deref()
        {
            sent_setup["setup"]["sessionResumption"] = json!({ "handle": h });
        }
        let dial = Dial {
            url: st.cfg.url.clone(),
            api_key: st.cfg.api_key.clone(),
            setup: sent_setup.clone(),
        };
        let connected = tokio::select! {
            r = open(dial, connector.clone()) => r,

            closed = drain_while_connecting(&mut st, &mut cmd_rx) => {
                if closed { return; }
                continue;
            }
        };
        let ws = match connected {
            Ok((ws, ws_ms, setup_ms)) => {
                backoff = Duration::from_millis(500);
                let resumed = st.cfg.resumption && st.resume_handle.is_some();
                if st.had_session && !resumed {
                    st.context_losses += 1;
                    st.emit(Output::ContextLost { why: format!("provider session gen={} started without resumption", st.generation) }).await;
                }
                st.had_session = true;
                let msg = format!(
                    "gemini ready gen={} model={} ws_connect_ms={ws_ms:.1} setup_ms={setup_ms:.1} setup_to_ready_ms={:.1} resumed={resumed} {} setup_id={}",
                    st.generation,
                    st.cfg.model,
                    opened.elapsed().as_secs_f64() * 1000.0,
                    st.cfg.knobs(),
                    sent_identity
                );
                st.log("info", msg).await;
                if st.audio_dropped_offline > 0 {
                    let n = std::mem::take(&mut st.audio_dropped_offline);
                    st.log("warn", format!("gemini dropped {n} microphone chunks while no session was live")).await;
                }
                ws
            }
            Err(e) => {
                let why = format!("{e:#}");
                st.log("error", format!("gemini connect gen={} failed: {why}", st.generation)).await;

                if st.resume_handle.is_some() && why.contains("session not found or expired") {
                    st.resume_handle = None;
                    st.log("warn", "gemini resumption handle expired; the next session starts without it".into()).await;
                }
                fail_stale_waiting(&mut st).await;
                let wait = tokio::time::sleep(backoff);
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        _ = &mut wait => break,
                        cmd = cmd_rx.recv() => match cmd {
                            None => return,
                            Some(cmd) => queue_or_cancel(&mut st, cmd).await,
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
                st.log("warn", format!("gemini session gen={} ended: {why}", st.generation)).await;
                st.fail_open_turns(&why).await;
                st.after_interrupt = false;
                st.go_away = false;

                if st.utterance.take().is_some() {
                    st.next_boundary = Some("local");
                }
                st.activity_seen = false;
            }
        }
    }
}

async fn drain_while_connecting(st: &mut State, cmd_rx: &mut mpsc::Receiver<Command>) -> bool {
    loop {
        match cmd_rx.recv().await {
            None => return true,
            Some(cmd) => queue_or_cancel(st, cmd).await,
        }
    }
}

async fn queue_or_cancel(st: &mut State, cmd: Command) {
    match cmd {
        Command::UserText { request_id, response_id, text, memory, .. } => {

            let (text, applied) = crate::backend::accept_memory(text, memory, false, st.context_losses);
            if let Some(applied) = applied {
                st.emit(Output::MemoryInjection { request_id: request_id.clone(), response_id: response_id.clone(), applied, text_sha256: crate::backend::text_sha256(&text) }).await;
            }
            st.emit(Output::Started { response_id: response_id.clone(), request_id: request_id.clone() }).await;
            st.waiting.push_back((request_id, response_id, text, Instant::now()));
        }
        Command::Restore { turns } => st.history = Some(turns),
        Command::SessionMemories { .. } | Command::SavedUnknown | Command::SavedWithheld | Command::SelfPrompts { .. } => {}
        Command::Cancel { response_id } => st.cancel(&response_id).await,
        Command::Audio { .. } => st.audio_dropped_offline += 1,
        Command::AudioEnd { .. } => {}
        Command::Respond { response_id, .. } => refuse_respond(st, response_id).await,

        Command::ToolOutput { call_id, .. } => st.log("warn", format!("gemini tool output for {call_id} dropped: no live session")).await,
    }
}

async fn refuse_respond(st: &mut State, response_id: String) {
    st.emit(Output::Done { response_id: response_id.clone(), status: ResponseStatus::Failed }).await;
    st.log("error", format!("gemini response {response_id} failed: this backend has no explicit replies")).await;
}

async fn fail_stale_waiting(st: &mut State) {
    while st.waiting.front().is_some_and(|(_, _, _, at)| at.elapsed() > READY_WAIT) {
        let (_, rid, _, _) = st.waiting.pop_front().unwrap();
        st.emit(Output::Done { response_id: rid.clone(), status: ResponseStatus::Failed }).await;
        st.log("error", format!("gemini response {rid} failed: no session within {} s", READY_WAIT.as_secs())).await;
    }
}

struct Dial {
    url: String,
    api_key: Option<String>,
    setup: Value,
}

async fn open(dial: Dial, connector: Option<Connector>) -> Result<(Ws, f64, f64)> {
    let mut req = dial.url.as_str().into_client_request().context("bad RTH_GEMINI_URL")?;
    if let Some(key) = &dial.api_key {
        let mut value = HeaderValue::from_str(key).context("GEMINI_API_KEY is not a valid header value")?;
        value.set_sensitive(true);
        req.headers_mut().insert("x-goog-api-key", value);
    }
    let t0 = Instant::now();
    let (mut ws, _resp) = tokio_tungstenite::connect_async_tls_with_config(req, None, true, connector)
        .await
        .map_err(|e| anyhow::anyhow!("websocket connect: {e}"))?;
    let ws_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    ws.send(Message::text(dial.setup.to_string())).await.context("sending setup")?;
    let deadline = tokio::time::sleep(SETUP_TIMEOUT);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => bail!("no setupComplete within {} s", SETUP_TIMEOUT.as_secs()),
            msg = ws.next() => {
                let bytes = match msg {
                    None => bail!("closed before setupComplete"),
                    Some(Err(e)) => bail!("before setupComplete: {e}"),
                    Some(Ok(Message::Close(frame))) => bail!("closed before setupComplete: {}", close_reason(frame.as_ref())),
                    Some(Ok(m)) => match payload(m) { Some(b) => b, None => continue },
                };
                if parse_server(&bytes)?.setup_complete {
                    return Ok((ws, ws_ms, t1.elapsed().as_secs_f64() * 1000.0));
                }
            }
        }
    }
}

pub(crate) fn payload(m: Message) -> Option<Vec<u8>> {
    match m {
        Message::Text(t) => Some(t.as_str().as_bytes().to_vec()),
        Message::Binary(b) => Some(b.to_vec()),
        _ => None,
    }
}

pub(crate) fn close_reason(frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>) -> String {
    match frame {
        Some(f) => format!("{} {}", u16::from(f.code), f.reason.chars().take(300).collect::<String>()),
        None => "no close frame".into(),
    }
}

async fn send_history(st: &mut State, ws: &mut Ws) -> Result<(), String> {
    let Some(turns) = st.history.take() else { return Ok(()) };
    if turns.is_empty() {
        return Ok(());
    }
    let (msg, sent) = history_message(&turns);
    if let Err(e) = ws.send(Message::text(msg.to_string())).await {
        st.history = Some(turns);
        return Err(format!("history send failed: {e}"));
    }
    st.emit(Output::ContextInjected { turns: sent.turns, bytes: sent.bytes, omitted: sent.omitted, truncated: sent.truncated }).await;
    Ok(())
}

async fn session(st: &mut State, mut ws: Ws, cmd_rx: &mut mpsc::Receiver<Command>) -> End {

    if let Err(e) = send_history(st, &mut ws).await {
        return End::Reconnect(e);
    }

    while let Some((_, rid, text, _)) = st.waiting.pop_front() {
        if let Err(e) = ws.send(Message::text(text_turn(&text).to_string())).await {
            st.emit(Output::Done { response_id: rid, status: ResponseStatus::Failed }).await;
            return End::Reconnect(format!("send failed: {e}"));
        }
        st.turns.push_back(Turn { response_id: rid, cancelled_at: None, dropped_audio: 0, dropped_text: 0 });
    }
    let mut last_progress = Instant::now();
    loop {
        if st.history.is_some()
            && st.turns.is_empty()
            && let Err(e) = send_history(st, &mut ws).await
        {
            return End::Reconnect(e);
        }
        if st.go_away && st.turns.is_empty() {
            let _ = ws.close(None).await;
            return End::Reconnect("goAway honoured while idle".into());
        }
        let stall = tokio::time::sleep_until((last_progress + TURN_STALL).into());
        tokio::select! {
            cmd = cmd_rx.recv() => match cmd {
                None => {
                    let _ = ws.close(None).await;
                    return End::Shutdown;
                }
                Some(Command::Restore { turns }) => {

                    st.history = Some(turns);
                    if st.turns.is_empty()
                        && let Err(e) = send_history(st, &mut ws).await
                    {
                        return End::Reconnect(e);
                    }
                }
                Some(Command::SessionMemories { .. } | Command::SavedUnknown | Command::SavedWithheld | Command::SelfPrompts { .. }) => {}
                Some(Command::UserText { request_id, response_id, text, memory, .. }) => {
                    let (text, applied) = crate::backend::accept_memory(text, memory, true, st.context_losses);
                    if let Some(applied) = applied {
                        st.emit(Output::MemoryInjection { request_id: request_id.clone(), response_id: response_id.clone(), applied, text_sha256: crate::backend::text_sha256(&text) }).await;
                    }
                    st.emit(Output::Started { response_id: response_id.clone(), request_id }).await;
                    if let Err(e) = ws.send(Message::text(text_turn(&text).to_string())).await {
                        st.emit(Output::Done { response_id, status: ResponseStatus::Failed }).await;
                        return End::Reconnect(format!("send failed: {e}"));
                    }
                    if st.turns.is_empty() {
                        last_progress = Instant::now();
                    }
                    st.turns.push_back(Turn { response_id, cancelled_at: None, dropped_audio: 0, dropped_text: 0 });
                }
                Some(Command::Cancel { response_id }) => st.cancel(&response_id).await,
                Some(Command::Respond { response_id, .. }) => refuse_respond(st, response_id).await,
                Some(Command::ToolOutput { call_id, name, output }) => {
                    let msg = json!({ "toolResponse": { "functionResponses": [{ "id": call_id, "name": name, "response": output }] } });
                    if let Err(e) = ws.send(Message::text(msg.to_string())).await {
                        return End::Reconnect(format!("toolResponse send failed: {e}"));
                    }
                    st.emit(Output::ToolOutputSent { call_id }).await;
                }
                Some(Command::Audio { sample_rate, pcm, .. }) => {
                    if let Err(e) = ws.send(Message::text(audio_input(&pcm, sample_rate).to_string())).await {
                        return End::Reconnect(format!("audio send failed: {e}"));
                    }
                }
                Some(Command::AudioEnd { stream_id }) => {
                    if let Err(e) = ws.send(Message::text(audio_stream_end().to_string())).await {
                        return End::Reconnect(format!("audioStreamEnd send failed: {e}"));
                    }
                    st.log("debug", format!("gemini audioStreamEnd sent for {stream_id}")).await;
                }
            },
            msg = ws.next() => {
                let bytes = match msg {
                    None => return End::Reconnect("socket closed".into()),
                    Some(Err(e)) => return End::Reconnect(format!("socket error: {e}")),
                    Some(Ok(Message::Close(frame))) => return End::Reconnect(format!("server closed: {}", close_reason(frame.as_ref()))),
                    Some(Ok(m)) => match payload(m) { Some(b) => b, None => continue },
                };
                last_progress = Instant::now();
                match parse_server(&bytes) {
                    Ok(m) => st.on_server(m).await,
                    Err(e) => st.log("warn", format!("gemini unreadable server message: {e:#}")).await,
                }
            }
            _ = stall, if !st.turns.is_empty() => {
                return End::Reconnect(format!("no server message for {} s with an open turn", TURN_STALL.as_secs()));
            }
        }
    }
}

pub(crate) fn tls_connector(ca_file: Option<&str>) -> Result<Connector> {
    Ok(Connector::Rustls(Arc::new(client_config(ca_file)?)))
}

pub(crate) fn client_config(ca_file: Option<&str>) -> Result<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    let (added, _) = roots.add_parsable_certificates(native.certs);
    if let Some(path) = ca_file {
        use rustls_pki_types::pem::PemObject;
        for cert in rustls_pki_types::CertificateDer::pem_file_iter(path).with_context(|| format!("reading {path}"))? {
            roots.add(cert.with_context(|| format!("parsing {path}"))?).with_context(|| format!("adding {path}"))?;
        }
    } else if added == 0 {
        bail!("no system trust anchors found and RTH_CA_FILE unset");
    }
    Ok(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth())
}

#[cfg(test)]
pub(crate) mod tests;
