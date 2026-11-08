

use std::collections::HashMap;
use std::time::Duration;

use rth_protocol::ResponseStatus;

pub use crate::context::Turn;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryInjection {

    pub epoch: u64,
    pub block: String,

    pub found_nothing: bool,
}

pub fn accept_memory(text: String, memory: Option<MemoryInjection>, live: bool, context_losses: u64) -> (String, Option<bool>) {
    match memory {
        Some(m) if live && m.epoch == context_losses => (format!("{}\n\n{text}", m.block), Some(true)),
        Some(_) => (text, Some(false)),
        None => (text, None),
    }
}

pub fn text_sha256(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug)]
pub enum Command {

    UserText { opener: Option<crate::opener::Ready>, request_id: String, response_id: String, text: String, context: Vec<Turn>, memory: Option<MemoryInjection>, owner: bool, people: Option<String>, reply_voice: Option<rth_protocol::ReplyVoice>, level: crate::route::TurnLevel },
    Cancel { response_id: String },

    #[allow(dead_code)]
    Respond { request_id: String, response_id: String, decision_id: String, utterance_id: Option<String> },

    Audio { sample_rate: u32, pcm: Vec<u8> },

    AudioEnd { stream_id: String },

    Restore { turns: Vec<Turn> },

    SessionMemories { text: String, items: Vec<String>, refs: Vec<rth_protocol::MemoryRef> },

    SavedUnknown,

    SavedWithheld,

    SelfPrompts { spec: std::sync::Arc<rth_self::SelfSpec>, consented: rth_self::compile::Consented, templates: crate::being::Templates },

    ToolOutput { call_id: String, name: String, output: serde_json::Value },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Heard {
    pub utterance_id: Option<String>,
    pub seq: Option<u64>,
    pub finished: Option<bool>,
    pub response_id: Option<String>,
    pub association: Option<&'static str>,
    pub boundary: Option<&'static str>,
}

#[derive(Debug, PartialEq)]
pub enum Output {
    Started { response_id: String, request_id: String },

    Opened { response_id: String, utterance_id: Option<String> },

    InputTranscript { text: String, heard: Heard },

    ContextInjected { turns: u64, bytes: u64, omitted: u64, truncated: u64 },

    ContextLost { why: String },

    MemoryInjection { request_id: String, response_id: String, applied: bool, text_sha256: String },

    MemorySent { response_id: String, hosted: bool, saved: Vec<rth_protocol::MemoryRef>, block: bool },

    Signal { response_id: Option<String>, signal: &'static str },

    Delta { response_id: String, text: String },

    Replace { response_id: String, text: String },

    Spoken { response_id: String },

    Audio { response_id: String, sample_rate: u32, pcm: Vec<u8> },
    Done { response_id: String, status: ResponseStatus },

    Provenance { response_id: String, turn: TurnProvenance },

    Shown { response_id: String, markdown_stripped: bool, sentence_cased: bool },

    FixedReply { response_id: String, what: &'static str },

    TurnPrompt { response_id: String, provenance: Provenance, self_version: Option<(&'static str, String)> },

    PtGender { response_id: String, outcome: &'static str, hit: String },

    PtOpener { response_id: String, outcome: &'static str, opener: String },

    SpendStatus { status: crate::flash_spend::Status },

    Diagnostic {
        response_id: String,
        session_generation: u64,
        provider_status: Option<String>,
        provider_reason: Option<String>,

        outcome_source: &'static str,
        local_reason: Option<String>,

        truncated: Option<String>,

        raw_text: Option<String>,

        history_guard_applied: bool,
    },

    #[allow(dead_code)]
    Error { detail: String },

    Log { level: &'static str, msg: String },

    ToolCall { response_id: Option<String>, generation: u64, call_id: String, name: String, args: serde_json::Value },

    ToolCancelled { call_ids: Vec<String> },

    ToolOutputSent { call_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub model: String,
    pub model_sha256: Option<String>,
    pub prompt: String,
    pub prompt_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub text: String,
    pub template_version: &'static str,
    pub spec_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnProvenance {
    pub provenance: Provenance,

    pub fallback_reason: Option<&'static str>,

    pub provider_model: Option<String>,

    pub config: serde_json::Value,

    pub first_token_ms: Option<u64>,

    pub self_version: Option<(&'static str, String)>,
}

pub struct Capabilities {
    pub name: &'static str,
    pub cancel: bool,
    pub audio_in: bool,
    pub audio_out: bool,

    pub opened_policy: &'static str,

    pub restore: crate::context::BackendKind,

    pub provenance: Provenance,

    pub turn_provenance: bool,

    #[allow(dead_code)]
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Evidence {
    Documented,
    Client,
    Probed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Cap {
    Yes(Evidence),
    No(Evidence),
    Unknown,
}

impl Cap {

    pub fn label(self) -> String {
        let grade = |e: Evidence| match e {
            Evidence::Documented => "documented",
            Evidence::Client => "client",
            Evidence::Probed => "probed",
        };
        match self {
            Cap::Yes(e) => format!("yes:{}", grade(e)),
            Cap::No(e) => format!("no:{}", grade(e)),
            Cap::Unknown => "unknown".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {

    pub explicit_replies: Cap,

    pub deferred_replies: Cap,

    pub live_turn_options: Cap,
    pub native_resumption: Cap,
    pub dynamic_tools: Cap,

    pub clears_input: Cap,

    pub truncate: Cap,

    pub truncate_returns_text: Cap,

    pub quiet_close_s: Option<u64>,
}

impl Flags {

    pub fn summary(&self) -> String {
        let quiet = self.quiet_close_s.map(|s| s.to_string()).unwrap_or_else(|| "never".into());
        format!(
            "explicit_replies={} deferred_replies={} live_turn_options={} native_resumption={} dynamic_tools={} clears_input={} truncate={} truncate_returns_text={} quiet_close={quiet}",
            self.explicit_replies.label(),
            self.deferred_replies.label(),
            self.live_turn_options.label(),
            self.native_resumption.label(),
            self.dynamic_tools.label(),
            self.clears_input.label(),
            self.truncate.label(),
            self.truncate_returns_text.label()
        )
    }

    pub const UNKNOWN: Flags = Flags {
        explicit_replies: Cap::Unknown,
        deferred_replies: Cap::Unknown,
        live_turn_options: Cap::Unknown,
        native_resumption: Cap::Unknown,
        dynamic_tools: Cap::Unknown,
        clears_input: Cap::Unknown,
        truncate: Cap::Unknown,
        truncate_returns_text: Cap::Unknown,
        quiet_close_s: None,
    };
}

pub struct Session {
    pub caps: Capabilities,
    pub commands: mpsc::Sender<Command>,
    pub outputs: mpsc::Receiver<Output>,
}

pub const BARGE_DBFS: f64 = -40.0;

pub const BARGE_MS: u64 = 60;

pub fn rms_dbfs(pcm: &[u8]) -> f64 {
    let n = pcm.len() / 2;
    if n == 0 {
        return f64::NEG_INFINITY;
    }
    let sum: f64 = pcm.chunks_exact(2).map(|b| (i16::from_le_bytes([b[0], b[1]]) as f64).powi(2)).sum();
    20.0 * ((sum / n as f64).sqrt() / 32768.0).log10()
}

pub const FAKE_RATE: u32 = 24_000;
pub const FAKE_CHUNK_SAMPLES: usize = 2_400;

pub fn tone_chunk(n: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(FAKE_CHUNK_SAMPLES * 2);
    for i in 0..FAKE_CHUNK_SAMPLES as u64 {
        let t = (n * FAKE_CHUNK_SAMPLES as u64 + i) as f64 / FAKE_RATE as f64;
        let v = ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn fake(chunk_delay: Duration, uptime: impl Fn() -> Duration + Send + 'static, prefix: String) -> Session {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Command>(64);
    let (out_tx, out_rx) = mpsc::channel::<Output>(1024);
    tokio::spawn(async move {
        let mut active: HashMap<String, JoinHandle<()>> = HashMap::new();
        let mut heard_samples: u64 = 0;
        let mut heard_rate: u32 = 16_000;
        let fake_losses = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut opened: u64 = 0;

        let mut voice_turn: Option<String> = None;

        let mut audible_samples: u64 = 0;
        while let Some(cmd) = cmd_rx.recv().await {
            active.retain(|_, h| !h.is_finished());
            if voice_turn.as_ref().is_some_and(|r| !active.contains_key(r)) {
                voice_turn = None;
            }
            match cmd {
                Command::UserText { request_id, response_id, text, context, memory, .. } => {
                    let (text, applied) = accept_memory(text, memory, true, fake_losses.load(std::sync::atomic::Ordering::SeqCst));
                    if let Some(applied) = applied {
                        let _ = out_tx.send(Output::MemoryInjection { request_id: request_id.clone(), response_id: response_id.clone(), applied, text_sha256: text_sha256(&text) }).await;
                    }

                    if let Some(task) = text.strip_prefix("delegate:").or_else(|| text.strip_prefix("act-on-data:")) {
                        opened += 1;
                        let _ = out_tx.send(Output::ToolCall {
                            response_id: Some(response_id.clone()),
                            generation: 1,
                            call_id: format!("{prefix}fake-call-{opened}"),
                            name: "delegate".into(),
                            args: serde_json::json!({ "purpose": "report", "task": task.trim(), "audience": "coding_agent" }),
                        }).await;
                    }

                    if text.starts_with("lose-context:") {
                        fake_losses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let _ = out_tx.send(Output::ContextLost { why: "fake: lose-context".into() }).await;
                    }
                    if text.starts_with("lose-context-later") {
                        let tx = out_tx.clone();
                        let losses = fake_losses.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            losses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let _ = tx.send(Output::ContextLost { why: "fake: lose-context-later".into() }).await;
                        });
                    }
                    if let Some((name, args)) = fake_memory_call(&text) {
                        opened += 1;
                        let _ = out_tx.send(Output::ToolCall {
                            response_id: Some(response_id.clone()),
                            generation: 1,
                            call_id: format!("{prefix}fake-call-{opened}"),
                            name: name.into(),
                            args,
                        }).await;
                    }

                    let lower = text.to_lowercase();
                    let delay = if lower.contains("slowly") { Duration::from_millis(250) } else { chunk_delay };
                    let audio_chunks = if lower.contains("aloud") { 20 } else { 0 };
                    let words = script(&text, uptime(), &context);
                    let out = out_tx.clone();
                    let id = response_id.clone();
                    let task = tokio::spawn(async move {
                        let _ = out.send(Output::Started { response_id: id.clone(), request_id }).await;
                        speak(&out, &id, words, audio_chunks, delay, if audio_chunks > 0 { Duration::from_millis(50).max(delay) } else { delay }).await;
                        let _ = out.send(Output::Done { response_id: id, status: ResponseStatus::Completed }).await;
                    });
                    active.insert(response_id, task);
                }
                Command::Cancel { response_id } => {
                    if let Some(task) = active.remove(&response_id) {
                        task.abort();
                        let _ = out_tx.send(Output::Done { response_id, status: ResponseStatus::Cancelled }).await;
                    }
                }
                Command::Audio { sample_rate, pcm } => {

                    if rms_dbfs(&pcm) >= BARGE_DBFS {
                        audible_samples += (pcm.len() / 2) as u64;
                    } else {
                        audible_samples = 0;
                    }
                    let audible = audible_samples * 1000 >= BARGE_MS * sample_rate.max(1) as u64;
                    if audible
                        && let Some(rid) = voice_turn.take()
                        && let Some(task) = active.remove(&rid)
                    {
                        task.abort();
                        let _ = out_tx.send(Output::Signal { response_id: Some(rid.clone()), signal: "interrupted" }).await;
                        let _ = out_tx.send(Output::Done { response_id: rid, status: ResponseStatus::Cancelled }).await;
                    }
                    heard_rate = sample_rate;
                    heard_samples += (pcm.len() / 2) as u64;
                }
                Command::AudioEnd { stream_id } => {
                    let ms = heard_samples * 1000 / heard_rate.max(1) as u64;
                    heard_samples = 0;
                    if ms < 100 {
                        continue;
                    }
                    opened += 1;
                    let rid = format!("{prefix}-v{opened}");
                    let uid = format!("{prefix}-u{opened}");
                    let heard = Heard {
                        utterance_id: Some(uid.clone()),
                        seq: Some(0),
                        finished: Some(true),
                        response_id: Some(rid.clone()),
                        association: Some("provider"),
                        boundary: None,
                    };
                    let _ = out_tx.send(Output::InputTranscript { text: format!("[{ms} ms of audio]"), heard }).await;
                    let _ = out_tx.send(Output::Opened { response_id: rid.clone(), utterance_id: Some(uid) }).await;

                    if stream_id.starts_with("recall-") {
                        let call = Output::ToolCall { response_id: Some(rid.clone()), generation: 1, call_id: format!("{prefix}fake-call-v{opened}"), name: "recall".into(), args: serde_json::json!({ "query": "favourite café" }) };
                        let _ = out_tx.send(call).await;
                    }
                    let out = out_tx.clone();
                    let id = rid.clone();
                    let words: Vec<String> = format!("I heard {ms} ms of audio.").split_inclusive(' ').map(str::to_string).collect();
                    let task = tokio::spawn(async move {
                        speak(&out, &id, words, 10, chunk_delay, Duration::from_millis(50)).await;
                        let _ = out.send(Output::Done { response_id: id, status: ResponseStatus::Completed }).await;
                    });
                    active.insert(rid.clone(), task);
                    voice_turn = Some(rid);
                }

                Command::Restore { .. } | Command::SessionMemories { .. } | Command::SavedUnknown | Command::SavedWithheld | Command::SelfPrompts { .. } => {}

                Command::ToolOutput { call_id, .. } => {
                    let _ = out_tx.send(Output::ToolOutputSent { call_id }).await;
                }

                Command::Respond { response_id, .. } => {
                    let _ = out_tx.send(Output::Done { response_id, status: ResponseStatus::Failed }).await;
                }
            }
        }
    });
    Session {
        caps: Capabilities {
            name: "fake@v2",
            cancel: true,
            audio_in: true,
            audio_out: true,
            opened_policy: "provider:fake-endpoint@v1",
            restore: crate::context::BackendKind::Fake,
            provenance: Provenance { model: "fake".into(), model_sha256: None, prompt: "none".into(), prompt_sha256: None },
            turn_provenance: false,
            flags: Flags::UNKNOWN,
        },
        commands: cmd_tx,
        outputs: out_rx,
    }
}

fn fake_memory_call(text: &str) -> Option<(&'static str, serde_json::Value)> {
    if let Some(fact) = text.strip_prefix("remember:") {
        return Some(("remember", serde_json::json!({ "text": fact.trim() })));
    }
    if let Some(q) = text.strip_prefix("recall:") {
        return Some(("recall", serde_json::json!({ "query": q.trim() })));
    }
    let rest = text.strip_prefix("correct:")?.trim();
    let mut parts = rest.splitn(3, ' ');
    let (id, rev, fact) = (parts.next()?, parts.next()?.parse::<u64>().ok()?, parts.next()?);
    Some(("correct_memory", serde_json::json!({ "memory_id": id, "revision": rev, "text": fact })))
}

async fn speak(out: &mpsc::Sender<Output>, id: &str, words: Vec<String>, chunks: u64, word_delay: Duration, audio_delay: Duration) {
    let mut words = words.into_iter();
    let mut next_chunk = 0u64;
    let mut word = words.next();
    while word.is_some() || next_chunk < chunks {
        if let Some(w) = word.take() {
            tokio::time::sleep(word_delay).await;
            let _ = out.send(Output::Delta { response_id: id.to_string(), text: w }).await;
            word = words.next();
        }
        if next_chunk < chunks {
            if word.is_none() {
                tokio::time::sleep(audio_delay).await;
            }
            let _ = out.send(Output::Audio { response_id: id.to_string(), sample_rate: FAKE_RATE, pcm: tone_chunk(next_chunk) }).await;
            next_chunk += 1;
        }
    }
}

fn script(input: &str, uptime: Duration, context: &[Turn]) -> Vec<String> {
    let lower = input.trim().to_lowercase();

    let earlier = context.split_last().map(|(_, rest)| rest).unwrap_or(&[]);
    let text = if lower.starts_with("hi") {
        format!(
            "hi! I'm alive inside the rth-core microVM, up {}s. Every event you see is in my journal. \
             Press Ctrl-X while I talk to interrupt me.",
            uptime.as_secs()
        )
    } else if lower.contains("remember") && lower.contains("what") && lower.contains('?') {
        match crate::context::remembered(earlier) {
            Some(fact) => format!("You asked me to remember: {fact}."),
            None => "You haven't asked me to remember anything in this context.".to_string(),
        }
    } else if lower.starts_with("remember") {
        "OK, I'll remember that.".to_string()
    } else {
        format!("I heard \"{}\". My real model isn't wired yet; I'm the fake backend.", input.trim())
    };
    text.split_inclusive(' ').map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(request: &str, response: &str) -> Command {
        Command::UserText { opener: None, request_id: request.into(), response_id: response.into(), text: "hi".into(), context: vec![], memory: None, owner: true, people: None, reply_voice: None, level: Default::default() }
    }

    #[tokio::test]
    async fn accepts_input_while_streaming_and_cancels_one_response() {
        let mut s = fake(Duration::from_millis(5), || Duration::ZERO, "t".into());
        s.commands.send(text("q1", "r1")).await.unwrap();
        assert_eq!(s.outputs.recv().await.unwrap(), Output::Started { response_id: "r1".into(), request_id: "q1".into() });

        s.commands.send(text("q2", "r2")).await.unwrap();
        s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();

        let mut r1_done = None;
        let mut r2_done = None;
        while r1_done.is_none() || r2_done.is_none() {
            match s.outputs.recv().await.unwrap() {
                Output::Done { response_id, status } if response_id == "r1" => r1_done = Some(status),
                Output::Done { response_id, status } if response_id == "r2" => r2_done = Some(status),
                Output::Delta { response_id, .. } if r1_done.is_some() => assert_ne!(response_id, "r1", "delta after cancel"),
                _ => {}
            }
        }
        assert_eq!(r1_done, Some(ResponseStatus::Cancelled));
        assert_eq!(r2_done, Some(ResponseStatus::Completed));
    }

    fn mic(hz: f64, ms: usize) -> Command {
        let pcm: Vec<u8> = (0..16 * ms)
            .flat_map(|i| (if hz == 0.0 { 0i16 } else { ((i as f64 / 16_000.0 * hz * std::f64::consts::TAU).sin() * 8000.0) as i16 }).to_le_bytes())
            .collect();
        Command::Audio { sample_rate: 16_000, pcm }
    }

    #[tokio::test]
    async fn only_audible_input_interrupts_the_fakes_spoken_turn() {
        for (hz, expect_interrupt) in [(0.0, false), (300.0, true)] {
            let mut s = fake(Duration::from_millis(5), || Duration::ZERO, "t".into());
            s.commands.send(mic(300.0, 300)).await.unwrap();
            s.commands.send(Command::AudioEnd { stream_id: "m1".into() }).await.unwrap();
            loop {
                if let Output::Opened { .. } = s.outputs.recv().await.unwrap() {
                    break;
                }
            }

            for _ in 0..10 {
                s.commands.send(mic(hz, 20)).await.unwrap();
            }
            let (interrupted, status) = loop {
                match s.outputs.recv().await.unwrap() {
                    Output::Signal { signal: "interrupted", .. } => {
                        let Output::Done { status, .. } = s.outputs.recv().await.unwrap() else { panic!() };
                        break (true, status);
                    }
                    Output::Done { status, .. } => break (false, status),
                    _ => {}
                }
            };
            assert_eq!(interrupted, expect_interrupt, "tone {hz} Hz");
            assert_eq!(status, if expect_interrupt { ResponseStatus::Cancelled } else { ResponseStatus::Completed });
        }
        assert!(rms_dbfs(&[0, 0, 0, 0]).is_infinite());
    }
}
