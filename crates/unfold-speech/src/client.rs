

use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;

use crate::http::{self, Sse};
use crate::catalog::CatalogError;
use crate::wire::{AudioChunk, AudioData, Done, ServerError, SpeechRequest, Streamed, Synthesis};

pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> Self {
        ApiKey(key.into())
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {

    pub const TICK: Duration = Duration::from_millis(100);

    pub fn new() -> Self {
        Cancel::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Playback {

    #[default]
    Live,

    HoldUntilDone,
}

pub struct Client {
    host: String,
    port: u16,
    key: Option<ApiKey>,

    pub client_label: Option<String>,

    pub max_input_chars: usize,

    pub timeout: Duration,
    pub playback: Playback,

    pub language: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("key", &self.key)
            .field("client_label", &self.client_label)
            .field("max_input_chars", &self.max_input_chars)
            .field("timeout", &self.timeout)
            .field("playback", &self.playback)
            .field("language", &self.language)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {

    BaseUrl(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::BaseUrl(why) => write!(f, "base URL: {why}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, PartialEq)]
pub enum StepError {

    Catalog(CatalogError),

    Io { kind: io::ErrorKind },

    Http { status: u16, code: Option<String>, message: Option<String> },

    Busy { retry_after_s: Option<u32> },

    NotEventStream { status: u16, content_type: String },

    Server { code: String, message: String, audio_seconds: f64 },

    NoDone { audio_events: usize, cause: Option<io::ErrorKind> },

    Protocol { what: String },

    Cancelled,
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Catalog(e) => write!(f, "voice catalogue: {e}"),
            StepError::Io { kind } => write!(f, "request failed: {kind}"),
            StepError::Http { status, code, message } => write!(
                f,
                "HTTP {status} {}: {}",
                code.as_deref().unwrap_or("-"),
                message.as_deref().unwrap_or("-")
            ),
            StepError::Busy { retry_after_s } => match retry_after_s {
                Some(s) => write!(f, "speech worker busy; retry after {s} s"),
                None => write!(f, "speech worker busy"),
            },
            StepError::NotEventStream { status, content_type } => write!(
                f,
                "HTTP {status} {content_type:?} is not an event stream; this client requires stream_format \"events\""
            ),
            StepError::Server { code, message, audio_seconds } => {
                write!(f, "synthesis failed after {audio_seconds} s of audio: {code}: {message}")
            }
            StepError::NoDone { audio_events, cause } => {
                write!(f, "stream ended after {audio_events} audio events with no done event")?;
                match cause {
                    Some(k) => write!(f, " ({k})"),
                    None => Ok(()),
                }
            }
            StepError::Protocol { what } => write!(f, "unreadable event stream: {what}"),
            StepError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for StepError {}

impl From<CatalogError> for StepError {
    fn from(e: CatalogError) -> Self {
        StepError::Catalog(e)
    }
}

impl Client {
    pub fn new(base_url: &str, key: Option<ApiKey>) -> Result<Client, ConfigError> {
        let rest = base_url.strip_prefix("http://").ok_or(ConfigError::BaseUrl("only http:// is supported"))?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.is_empty() || rest.contains(['/', '@', '?', '#']) || rest.chars().any(char::is_whitespace) {
            return Err(ConfigError::BaseUrl("expected http://host[:port] with no path or user info"));
        }
        let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
            let (h, p) = v6.split_once(']').ok_or(ConfigError::BaseUrl("unclosed ["))?;
            (h, p)
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h, p),
                None => (rest, ""),
            }
        };
        let port = match port.strip_prefix(':').unwrap_or(port) {
            "" => 80,
            p => p.parse::<u16>().map_err(|_| ConfigError::BaseUrl("bad port"))?,
        };
        if host.is_empty() || host.contains(':') && !rest.starts_with('[') {
            return Err(ConfigError::BaseUrl("bad host"));
        }
        Ok(Client {
            host: host.to_string(),
            port,
            key,
            client_label: Some("unfold".into()),
            max_input_chars: crate::MAX_INPUT_CHARS,
            timeout: Duration::from_secs(30),
            playback: Playback::default(),
            language: None,
        })
    }

    pub fn synthesize(&self, request: &SpeechRequest) -> Result<Synthesis, StepError> {
        self.synthesize_until(request, None)
    }

    pub(crate) fn synthesize_until(&self, request: &SpeechRequest, cancel: Option<&Cancel>) -> Result<Synthesis, StepError> {
        let mut chunks = Vec::new();
        let end = self.stream_until(request, cancel, |c| chunks.push(c))?;
        Ok(Synthesis { chunks, done: end.done, frame_budget: end.frame_budget })
    }

    pub fn stream(&self, request: &SpeechRequest, on_chunk: impl FnMut(AudioChunk)) -> Result<Streamed, StepError> {
        self.stream_until(request, None, on_chunk)
    }

    pub fn stream_until(
        &self,
        request: &SpeechRequest,
        cancel: Option<&Cancel>,
        mut on_chunk: impl FnMut(AudioChunk),
    ) -> Result<Streamed, StepError> {
        let cancelled = || cancel.is_some_and(Cancel::is_cancelled);
        if cancelled() {
            return Err(StepError::Cancelled);
        }
        let body = serde_json::to_vec(request).map_err(|e| StepError::Protocol { what: format!("request: {e}") })?;
        let auth = self.key.as_ref().map(|k| format!("Bearer {}", k.0));
        let mut headers = Vec::new();
        if let Some(a) = &auth {
            headers.push(("Authorization", a.as_str()));
        }
        if let Some(l) = &self.client_label {
            headers.push(("X-LocalFleet-Client", l.as_str()));
        }
        let response = http::post(&self.host, self.port, "/v1/audio/speech", &headers, &body, self.timeout, cancel.cloned())
            .map_err(|e| if cancelled() { StepError::Cancelled } else { StepError::Io { kind: e.kind() } });
        drop(auth);
        let mut response = response?;

        if response.status != 200 {
            let retry_after_s = response.header("retry-after").and_then(|v| v.parse().ok());
            let mut raw = Vec::new();
            let _ = response.body.by_ref().take(64 * 1024).read_to_end(&mut raw);
            let error = serde_json::from_slice::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| serde_json::from_value::<ServerError>(v["error"].clone()).ok());
            if response.status == 429 {
                return Err(StepError::Busy { retry_after_s });
            }
            return Err(StepError::Http {
                status: response.status,
                code: error.as_ref().map(|e| self.scrub(&e.code)),
                message: error.as_ref().map(|e| self.scrub(&e.message)),
            });
        }
        let content_type = response.header("content-type").unwrap_or("").to_string();
        if !content_type.to_ascii_lowercase().starts_with("text/event-stream") {
            return Err(StepError::NotEventStream { status: response.status, content_type: self.scrub(&content_type) });
        }

        let frame_budget = response.header("x-granite-frame-budget").and_then(|v| v.trim().parse::<u32>().ok());
        let mut sse = Sse::new(response.body);
        let mut audio_events = 0;

        let mut unreadable: Option<String> = None;
        loop {

            if cancelled() {
                return Err(StepError::Cancelled);
            }
            let (kind, data) = match sse.next_event() {
                Ok(Some(e)) => e,
                Ok(None) => return Err(StepError::NoDone { audio_events, cause: None }),
                Err(_) if cancelled() => return Err(StepError::Cancelled),
                Err(e) => return Err(StepError::NoDone { audio_events, cause: Some(e.kind()) }),
            };
            match kind.as_str() {
                "audio" => match audio(&data) {
                    Ok(chunk) => {
                        on_chunk(chunk);
                        audio_events += 1;
                    }
                    Err(what) => {
                        unreadable.get_or_insert(what);
                    }
                },
                "done" => {
                    let done: Done = serde_json::from_str(&data)
                        .map_err(|e| StepError::Protocol { what: self.scrub(&format!("done event: {e}")) })?;
                    if let Some(e) = &done.error {
                        return Err(StepError::Server {
                            code: self.scrub(&e.code),
                            message: self.scrub(&e.message),
                            audio_seconds: done.audio_seconds,
                        });
                    }
                    if let Some(what) = unreadable {
                        return Err(StepError::Protocol { what: self.scrub(&what) });
                    }
                    return Ok(Streamed { done, frame_budget });
                }
                _ => {}
            }
        }
    }

    pub(crate) fn scrub(&self, text: &str) -> String {
        match &self.key {
            Some(k) if !k.0.is_empty() => text.replace(&k.0, "<redacted>"),
            _ => text.to_string(),
        }
    }
}

fn audio(data: &str) -> Result<AudioChunk, String> {
    let bad = |what: &str| format!("audio event: {what}");
    let a: AudioData = serde_json::from_str(data).map_err(|e| bad(&e.to_string()))?;
    if a.format != "s16le" {
        return Err(bad("format is not s16le"));
    }
    if a.sample_rate != crate::SAMPLE_RATE {
        return Err(bad("sample rate is not 24000"));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(a.pcm_base64.as_bytes()).map_err(|_| bad("bad base64"))?;
    if bytes.len() % 2 != 0 {
        return Err(bad("odd byte count for 16-bit PCM"));
    }
    let samples = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    Ok(AudioChunk { samples, sample_rate: a.sample_rate })
}
