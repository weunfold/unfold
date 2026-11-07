
use super::{Error, Result};
use crate::conversation_log::{
    observe::{Payload, Terminal},
    Turn,
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
pub trait Counter {
    fn text(&mut self, text: &str) -> Result<usize>;
    fn prompt(&mut self, messages: &[Value]) -> Result<usize>;
}
pub struct Tail {
    pub locale: String,
    pub status: String,
    pub evidence: String,
}
#[derive(Clone)]
struct Exchange {
    id: String,
    user: String,
    rendered: String,
}
pub struct Context {
    stable: String,
    history: Vec<Exchange>,
    revision: u64,
    needs_prefill: Arc<AtomicBool>,
    prefilled_generation: Option<u64>,
    child_generation: Option<u64>,
    active: Arc<AtomicBool>,
}
pub struct Plan {
    body: Value,
    _permit: Permit,
    pub prompt_tokens: usize,
    pub revision: u64,
    pub(crate) prefilled_generation: Option<u64>,
    needs_prefill: Arc<AtomicBool>,
}
pub struct Prefix {
    pub(crate) messages: Vec<Value>,
    pub(crate) revision: u64,
    _permit: Permit,
}
pub struct Prefilled {
    pub(crate) messages: Vec<Value>,
    pub(crate) revision: u64,
    pub(crate) generation: u64,
}
struct Permit(Arc<AtomicBool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl Plan {
    pub fn payload(&self) -> &Value {
        &self.body
    }
}
pub struct Change {
    pub revision: u64,
    pub removed_tokens: usize,
    pub kind: &'static str,
}
fn exchange(turn: &Turn) -> Result<Exchange> {
    let Some(Payload::Assistant { spans, terminal }) = &turn.presentation else { return Err(Error::Context) };
    let rendered = spans.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
    if !turn.rendered
        || turn.presentation_uncertain
        || *terminal == Terminal::PresentationUnknown
        || turn.input.is_empty()
        || turn.input.len() > 8192
        || rendered.is_empty()
        || rendered.len() > 8192
    {
        return Err(Error::Context);
    }
    Ok(Exchange { id: turn.id.clone(), user: turn.input.clone(), rendered })
}
impl Context {
    pub fn new(stable: &str) -> Result<Self> {
        if stable.is_empty() || stable.len() > 8192 {
            return Err(Error::Context);
        }
        Ok(Self {
            stable: stable.into(),
            history: vec![],
            revision: 0,
            needs_prefill: Arc::new(AtomicBool::new(false)),
            prefilled_generation: None,
            child_generation: None,
            active: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn child_started(&mut self, generation: u64) -> bool {
        if self.child_generation == Some(generation) { return false; }
        self.child_generation = Some(generation);
        self.prefilled_generation = None;
        self.needs_prefill.store(true, Ordering::SeqCst);
        true
    }
    pub fn needs_prefill(&self) -> bool { self.needs_prefill.load(Ordering::SeqCst) }
    pub fn append_rendered(&mut self, turn: &Turn) -> Result<()> {
        if self.active.load(Ordering::SeqCst)
            || self.history.len() >= 64
            || self.history.iter().any(|h| h.id == turn.id)
        {
            return Err(Error::Context);
        }
        self.history.push(exchange(turn)?);
        Ok(())
    }

    pub fn append_silent_cancel(&mut self, turn: &Turn) -> Result<()> {
        if turn.rendered || !turn.silent_replay || turn.presentation_uncertain || turn.legacy_attempted
            || !matches!(&turn.presentation, Some(Payload::Assistant { spans, terminal: Terminal::Interrupted }) if spans.is_empty()) {
            return Err(Error::Context);
        }
        if self.active.load(Ordering::SeqCst) || self.history.len() >= 64
            || self.history.iter().any(|h| h.id == turn.id) || turn.input.trim().is_empty() || turn.input.len() > 8192 {
            return Err(Error::Context);
        }
        self.history.push(Exchange { id: turn.id.clone(), user: turn.input.clone(),
            rendered: "[Assistant was cut off before speaking; no reply was heard.]".into() });
        Ok(())
    }
    pub fn prefix(&self) -> Vec<Value> {
        let mut rows = vec![json!({"role":"system","content":self.stable})];
        for h in &self.history {
            rows.push(json!({"role":"user","content":h.user}));
            rows.push(json!({"role":"assistant","content":h.rendered}));
        }
        rows
    }
    fn permit(&self) -> Result<Permit> {
        self.active.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).map_err(|_| Error::Busy)?;
        Ok(Permit(self.active.clone()))
    }

    pub fn prepare(&self, count: &mut impl Counter, tail: &Tail, input: &str) -> Result<Plan> {
        if self.needs_prefill.load(Ordering::SeqCst) {
            return Err(Error::Prefill);
        }
        let permit = self.permit()?;
        if input.trim().is_empty()
            || input.len() > 8192
            || tail.evidence.len() > 8192
            || tail.status.len() > 8192
            || !matches!(tail.locale.as_str(), "pt-BR" | "en")
            || count.text(input)? > 480
            || count.text(&tail.evidence)? > 600
            || count.text(&self.stable)? + count.text(&tail.status)? > 512
        {
            return Err(Error::Context);
        }
        let history = self.history.iter().map(|h| format!("{}\n{}", h.user, h.rendered)).collect::<Vec<_>>().join("\n");
        if count.text(&history)? > 2048 {
            return Err(Error::Context);
        }
        let mut messages = self.prefix();
        messages.push(json!({"role":"system","content":json!({"reply_locale":tail.locale,"status":tail.status,"quoted_evidence":tail.evidence}).to_string()}));
        messages.push(json!({"role":"user","content":input}));
        let prompt_tokens = count.prompt(&messages)?;
        if prompt_tokens > 4000 {
            return Err(Error::Context);
        }
        Ok(Plan {
            body: json!({"model":"voice-front","messages":messages,"stream":true,"stream_options":{"include_usage":true},"temperature":0,"max_tokens":96,"cache_prompt":true,"id_slot":0,"chat_template_kwargs":{"enable_thinking":false}}),
            _permit: permit,
            prompt_tokens,
            revision: self.revision,
            prefilled_generation: self.prefilled_generation,
            needs_prefill: self.needs_prefill.clone(),
        })
    }

    pub fn trim(&mut self, count: &mut impl Counter, quiet: bool) -> Result<Change> {
        if !quiet || self.active.load(Ordering::SeqCst) || self.needs_prefill.load(Ordering::SeqCst) {
            return Err(Error::Context);
        }
        let mut removed = 0;
        let mut n = 0;
        while n + 4 < self.history.len() && removed < 512 {
            let h = &self.history[n];
            removed += count.text(&format!("{}\n{}", h.user, h.rendered))?;
            n += 1;
        }
        if removed < 512 {
            return Err(Error::Context);
        }
        self.history.drain(..n);
        self.revision += 1;
        self.needs_prefill.store(true, Ordering::SeqCst);
        Ok(Change { revision: self.revision, removed_tokens: removed, kind: "trim" })
    }
    pub fn correct_rendered(&mut self, turn: &Turn, quiet: bool) -> Result<Change> {
        if !quiet || self.active.load(Ordering::SeqCst) {
            return Err(Error::Context);
        }
        let replacement = exchange(turn)?;
        let h = self.history.iter_mut().find(|h| h.id == turn.id).ok_or(Error::Context)?;
        if h.user != replacement.user {
            return Err(Error::Context);
        }
        *h = replacement;
        self.revision += 1;
        self.needs_prefill.store(true, Ordering::SeqCst);
        Ok(Change { revision: self.revision, removed_tokens: 0, kind: "rendered_correction" })
    }

    pub fn prefill_plan(&self, quiet: bool) -> Result<Prefix> {
        if !quiet || !self.needs_prefill.load(Ordering::SeqCst) {
            return Err(Error::Prefill);
        }
        Ok(Prefix { messages: self.prefix(), revision: self.revision, _permit: self.permit()? })
    }
    pub fn mark_prefilled(&mut self, receipt: Prefilled) -> Result<()> {
        if self.child_generation.is_some_and(|g| g != receipt.generation)
            || receipt.revision != self.revision || receipt.messages != self.prefix() {
            return Err(Error::Prefill);
        }
        self.needs_prefill.store(false, Ordering::SeqCst);
        self.prefilled_generation = Some(receipt.generation);
        Ok(())
    }
}

use super::child::Lease;
pub struct RemoteCounter<'a>(pub &'a Lease);
impl Counter for RemoteCounter<'_> {
    fn text(&mut self, text: &str) -> Result<usize> {
        let out = super::http::json_request(self.0, "/tokenize", &json!({"content":text}))?;
        Ok(out["tokens"].as_array().ok_or(Error::Context)?.len())
    }
    fn prompt(&mut self, messages: &[Value]) -> Result<usize> {
        let out = template(self.0, messages)?;
        self.text(&out)
    }
}
fn template(lease: &Lease, messages: &[Value]) -> Result<String> {
    super::http::json_request(
        lease,
        "/apply-template",
        &json!({"messages":messages,"add_generation_prompt":true,"chat_template_kwargs":{"enable_thinking":false}}),
    )?["prompt"]
        .as_str()
        .map(String::from)
        .ok_or(Error::Context)
}
pub fn prefill(lease: &Lease, prefix: &Prefix) -> Result<Prefilled> {
    let prompt = template(lease, &prefix.messages)?;
    let out = super::http::json_request(
        lease,
        "/completion",
        &json!({"prompt":prompt,"n_predict":0,"id_slot":0,"cache_prompt":true,"stream":false}),
    )?;
    if out["tokens_predicted"] != 0 || !lease.valid() {
        return Err(Error::Prefill);
    }
    Ok(Prefilled { messages: prefix.messages.clone(), revision: prefix.revision, generation: lease.generation })
}
pub fn generate(lease: &Lease, plan: &Plan) -> Result<super::http::Burst> {
    if plan.prefilled_generation.is_some_and(|g| g != lease.generation) {

        plan.needs_prefill.store(true, Ordering::SeqCst);
        return Err(Error::Prefill);
    }
    super::http::generate_payload(lease, plan.payload())
}
