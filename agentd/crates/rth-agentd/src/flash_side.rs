

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::flash_spend::{Ledger, Purpose, Status};
use crate::local;

const TEMPERATURE: f64 = 1.0;
const TOP_P: f64 = 1.0;

pub struct SideCall {
    pub purpose: Purpose,
    pub system: String,
    pub user: String,

    pub schema: Value,
    pub max_tokens: u32,
    pub carries_memory: bool,
    pub deadline: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideError {
    FlashOff,
    BreakerOpen,
    NoEgressGrant,

    Refused(String),
    Timeout,
    Unavailable(String),

    Invalid(String),
}

#[derive(Debug)]
pub struct SideReply {
    pub attempt_id: String,

    pub text: String,
    pub provider_model: Option<String>,

    pub spend: Status,

    pub provenance: Value,
}

pub struct FlashSide {
    cfg: local::Config,
    ledger: Arc<Ledger>,
    boot: String,
    next: AtomicU64,
    egress: bool,
}

impl FlashSide {

    pub fn for_session(backend: &str, flash: Option<local::Config>, ledger: Option<Arc<Ledger>>, boot: &str, egress: &str) -> Option<FlashSide> {
        let (cfg, ledger) = (flash?, ledger?);

        if !backend.starts_with("flash+") || matches!(local::dial(&cfg.authority), local::Dial::Loopback) {
            return None;
        }
        Some(FlashSide { cfg, ledger, boot: boot.to_string(), next: AtomicU64::new(1), egress: crate::memory::egress_allowed(backend, egress) })
    }

    pub fn admitted_bytes(c: &SideCall) -> usize {
        let (chars, n) = local::request_size(&[json!({"content": c.system}), json!({"content": c.user})]);
        chars + 16 * n
    }

    pub fn config(purpose: Purpose) -> Value {
        json!({"path": purpose.as_str(), "temperature": TEMPERATURE, "top_p": TOP_P, "thinking": false, "effort": "off", "thread": "background"})
    }

    fn body(&self, c: &SideCall) -> Value {
        json!({
            "model": self.cfg.model,
            "messages": [{"role": "system", "content": c.system}, {"role": "user", "content": c.user}],
            "stream": false,
            "max_tokens": c.max_tokens,
            "temperature": TEMPERATURE,
            "top_p": TOP_P,
            "response_format": c.schema,

            "thinking": {"type": "disabled"},
            "reasoning_effort": crate::route::Level::Off.reasoning_effort(),
        })
    }

    pub async fn call(&self, c: SideCall) -> Result<SideReply, SideError> {
        if local::breaker_for(&self.cfg.authority).lock().unwrap().open(tokio::time::Instant::now()) {
            return Err(SideError::BreakerOpen);
        }
        if c.carries_memory && !self.egress {
            return Err(SideError::NoEgressGrant);
        }
        let body = self.body(&c);
        let attempt_id = format!("x-{}-{}", self.boot, self.next.fetch_add(1, Ordering::SeqCst));
        let bytes = FlashSide::admitted_bytes(&c);
        let ledger = self.ledger.clone();
        let (id, purpose) = (attempt_id.clone(), c.purpose);
        let reservation = tokio::task::spawn_blocking(move || ledger.admit_for(&id, bytes, purpose)).await.map_err(|e| SideError::Refused(e.to_string()))?.map_err(SideError::Refused)?;
        let path = format!("{}/chat/completions", self.cfg.base_path);
        let sent = local::post_json(&self.cfg.authority, &path, &body, c.deadline).await;
        let parsed = match sent {
            Err(f) if f.reason == local::FIRST_TOKEN_TIMEOUT => Err(SideError::Timeout),
            Err(f) => Err(SideError::Unavailable(format!("{}: {}", f.reason, f.detail))),
            Ok((status, _)) if status != 200 => Err(SideError::Unavailable(format!("HTTP {status}"))),
            Ok((_, raw)) => serde_json::from_slice::<Value>(&raw).map_err(|e| SideError::Invalid(format!("response not json: {e}"))),
        };
        let usage = parsed.as_ref().ok().and_then(|v| v.get("usage").filter(|u| u.is_object()).cloned());
        let ledger = self.ledger.clone();
        let spend = tokio::task::spawn_blocking(move || ledger.settle(&reservation, usage.as_ref())).await.map_err(|e| SideError::Unavailable(e.to_string()))?;
        let v = parsed?;
        let text = v.pointer("/choices/0/message/content").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or_else(|| SideError::Invalid("no text in the first choice".into()))?;
        let provider_model = v.get("model").and_then(Value::as_str).map(|m| local::dial(&self.cfg.authority).redact(m)).filter(|m| !m.is_empty());
        let provenance = json!({"model": self.cfg.model, "prompt_sha256": crate::backend::text_sha256(&c.system), "config": FlashSide::config(c.purpose)});
        Ok(SideReply { attempt_id, text: text.to_string(), provider_model, spend, provenance })
    }
}

#[cfg(test)]
#[path = "flash_side_tests.rs"]
mod tests;
