

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const TTL: Duration = Duration::from_secs(10 * 60);

pub const RELEASE: &str = "/effect release ";

pub const DECLINE: &str = "/effect decline ";

const MAX_PENDING: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Held {
    Delegate { generation: u64, call_id: String, name: String, args: Value },

    Forget { memory_id: String, revision: u64 },

    Restore { memory_id: String, undoes: String },

    FolderGrant { request_id: String, task_id: String, root: String },

    WebFetch { task_id: String, url: String },

    PartOn { part_id: String, digest: String },

    PartCall { task_id: String, part_id: String, tool: String, args: Value },

    DshPrompt { session_id: String, text: String },

    DshWatch { change: Value },
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub kind: &'static str,
    pub call: Held,
    pub args_sha256: String,
    pub summary: String,
    pub why: String,
    pub expires_utc_ms: u64,
    token: String,
    created: Instant,
}

impl Pending {
    pub fn token(&self) -> &str {
        &self.token
    }
}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn new_token() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).expect("random bytes for a card token");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Unknown,
    Expired,
    WrongToken,
    Replayed,
    Malformed,

    Superseded,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::Unknown => "unknown",
            Refusal::Expired | Refusal::Superseded => "expired",
            Refusal::WrongToken => "wrong_token",
            Refusal::Replayed => "replayed",
            Refusal::Malformed => "malformed",
        }
    }

    pub fn why(self) -> &'static str {
        match self {
            Refusal::Unknown => "Nothing ran: that approval is no longer waiting (it may be from before a restart).",
            Refusal::Expired => "Nothing ran: that approval expired. Ask again if you still want it.",
            Refusal::WrongToken | Refusal::Malformed => "Nothing ran: that approval did not match.",
            Refusal::Replayed => "Nothing ran again: that approval was already used.",
            Refusal::Superseded => "Nothing ran: another choice was already used.",
        }
    }
}

#[derive(Default)]
pub struct Store {
    pending: HashMap<String, Pending>,
    order: Vec<String>,
    spent: Ids,

    siblings: HashMap<String, Vec<String>>,
    superseded: Ids,
}

#[derive(Default)]
struct Ids {
    set: HashSet<String>,
    order: std::collections::VecDeque<String>,
}

const MAX_REMEMBERED: usize = 4096;

impl Ids {
    fn insert(&mut self, id: String) {
        if self.set.insert(id.clone()) {
            self.order.push_back(id);
            if self.order.len() > MAX_REMEMBERED
                && let Some(old) = self.order.pop_front()
            {
                self.set.remove(&old);
            }
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }
}

impl Store {

    #[allow(clippy::too_many_arguments)]
    pub fn hold(&mut self, effect_id: &str, kind: &'static str, call: Held, args_sha256: String, summary: String, why: String, now_utc_ms: u64) -> Pending {
        if self.order.len() >= MAX_PENDING {
            let oldest = self.order.remove(0);
            self.pending.remove(&oldest);
        }
        let p = Pending { kind, call, args_sha256, summary, why, expires_utc_ms: now_utc_ms + TTL.as_millis() as u64, token: new_token(), created: Instant::now() };
        self.pending.insert(effect_id.to_string(), p.clone());
        self.order.push(effect_id.to_string());
        p
    }

    pub fn release(&mut self, effect_id: &str, token: &str) -> Result<Pending, Refusal> {
        if self.spent.contains(effect_id) {
            return Err(Refusal::Replayed);
        }
        if self.superseded.contains(effect_id) {
            return Err(Refusal::Superseded);
        }
        let Some(p) = self.pending.get(effect_id) else { return Err(Refusal::Unknown) };
        if p.created.elapsed() > TTL {
            self.forget(effect_id);
            return Err(Refusal::Expired);
        }

        let (a, b) = (sha256_hex(&p.token), sha256_hex(token));
        if a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) != 0 {
            return Err(Refusal::WrongToken);
        }
        let p = self.forget(effect_id).expect("present");
        self.spent.insert(effect_id.to_string());

        for s in self.siblings.remove(effect_id).unwrap_or_default() {
            self.siblings.remove(&s);
            if self.forget(&s).is_some() {
                self.superseded.insert(s);
            }
        }
        Ok(p)
    }

    pub fn decline(&mut self, effect_id: &str, token: &str) -> Result<Pending, Refusal> {
        self.release(effect_id, token)
    }

    pub fn retire_id(&mut self, effect_id: &str) {
        if self.forget(effect_id).is_some() {
            self.siblings.remove(effect_id);
            self.superseded.insert(effect_id.to_string());
        }
    }

    pub fn retire(&mut self, matches: impl Fn(&Held) -> bool) {
        let gone: Vec<String> = self.order.iter().filter(|e| self.pending.get(*e).is_some_and(|p| matches(&p.call))).cloned().collect();
        for e in gone {
            self.forget(&e);
            self.siblings.remove(&e);
            self.superseded.insert(e);
        }
    }

    pub fn close(&mut self, effect_id: &str) -> Option<Pending> {
        let p = self.forget(effect_id)?;
        self.siblings.remove(effect_id);
        self.spent.insert(effect_id.to_string());
        Some(p)
    }

    pub fn held(&self, matches: impl Fn(&Held) -> bool) -> Vec<(String, Pending)> {
        self.order.iter().filter_map(|e| self.pending.get(e).filter(|p| matches(&p.call)).map(|p| (e.clone(), p.clone()))).collect()
    }

    pub fn alternatives(&mut self, ids: &[String]) {
        if ids.len() < 2 {
            return;
        }
        for id in ids {
            self.siblings.insert(id.clone(), ids.iter().filter(|s| *s != id).cloned().collect());
        }
    }

    fn forget(&mut self, effect_id: &str) -> Option<Pending> {
        self.order.retain(|e| e != effect_id);
        self.pending.remove(effect_id)
    }

    pub fn waiting(&self) -> Vec<(String, Pending)> {
        self.order.iter().filter_map(|e| self.pending.get(e).filter(|p| p.created.elapsed() <= TTL).map(|p| (e.clone(), p.clone()))).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tap {
    Release,
    Decline,
}

pub fn parse_tap(text: &str) -> Option<(Tap, String, String)> {
    if let Some((id, token)) = parse_release(text) {
        return Some((Tap::Release, id, token));
    }
    let rest = text.trim().strip_prefix(DECLINE.trim_end())?;
    parse_release(&format!("{RELEASE}{}", rest.trim())).map(|(id, token)| (Tap::Decline, id, token))
}

pub fn parse_release(text: &str) -> Option<(String, String)> {
    let rest = text.trim().strip_prefix(RELEASE.trim_end())?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    let mut parts = rest.split_whitespace();
    let (id, token) = (parts.next()?, parts.next()?);
    let ok = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b':');
    (parts.next().is_none() && ok(id, 128) && ok(token, 128)).then(|| (id.to_string(), token.to_string()))
}

pub fn is_reserved(text: &str) -> bool {
    text.trim_start().strip_prefix("/effect").is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace))
}

pub fn redacted(text: &str) -> String {
    match parse_tap(text) {
        Some((Tap::Release, id, token)) => format!("{RELEASE}{id} sha256:{}", sha256_hex(&token)),
        Some((Tap::Decline, id, token)) => format!("{DECLINE}{id} sha256:{}", sha256_hex(&token)),
        None => "/effect (malformed; not journaled)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> Held {
        Held::Delegate { generation: 1, call_id: "c1".into(), name: "delegate".into(), args: serde_json::json!({"purpose": "p", "task": "t"}) }
    }

    #[test]
    fn a_held_effect_runs_once_and_only_with_its_token() {
        let mut s = Store::default();
        let p = s.hold("e1", "delegate", held(), "ab".into(), "t".into(), "why".into(), 0);
        assert_eq!(p.token().len(), 32);
        assert_eq!(s.release("e1", "0".repeat(32).as_str()).unwrap_err(), Refusal::WrongToken);
        assert_eq!(s.release("e2", p.token()).unwrap_err(), Refusal::Unknown);
        assert_eq!(s.release("e1", p.token()).unwrap().call, held());
        assert_eq!(s.release("e1", p.token()).unwrap_err(), Refusal::Replayed);
        assert!(s.waiting().is_empty());
    }

    #[test]
    fn releasing_one_alternative_expires_the_others_and_they_are_never_re_sent() {
        let mut s = Store::default();
        let f = |id: &str| Held::Forget { memory_id: id.into(), revision: 1 };
        let a = s.hold("a", "forget", f("m1"), "x".into(), "one".into(), "why".into(), 0);
        let _b = s.hold("b", "forget", f("m2"), "x".into(), "two".into(), "why".into(), 0);
        let c = s.hold("c", "forget", f("m3"), "x".into(), "three".into(), "why".into(), 0);
        s.alternatives(&["a".into(), "b".into()]);
        assert!(s.release("a", a.token()).is_ok());
        assert!(matches!(s.release("b", "whatever"), Err(Refusal::Superseded)));
        assert_eq!((Refusal::Superseded.as_str(), Refusal::Superseded.why()), ("expired", "Nothing ran: another choice was already used."));
        assert_eq!(s.waiting().iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["c"], "only the unrelated card is re-sent");
        assert!(s.release("c", c.token()).is_ok(), "an effect outside the set is untouched");
    }

    #[test]
    fn tokens_differ_and_the_store_is_bounded() {
        let mut s = Store::default();
        let a = s.hold("e1", "delegate", held(), "x".into(), "t".into(), "w".into(), 0);
        let b = s.hold("e2", "delegate", held(), "x".into(), "t".into(), "w".into(), 0);
        assert_ne!(a.token(), b.token());
        for i in 0..MAX_PENDING {
            s.hold(&format!("f{i}"), "delegate", held(), "x".into(), "t".into(), "w".into(), 0);
        }
        assert_eq!(s.waiting().len(), MAX_PENDING);
        assert_eq!(s.release("e1", a.token()).unwrap_err(), Refusal::Unknown, "the oldest was dropped");
    }

    #[test]
    fn s2_a_card_can_be_declined_once_with_its_token_and_a_retired_card_cannot_be_tapped() {
        let mut s = Store::default();
        let g = || Held::FolderGrant { request_id: "gr-1".into(), task_id: "b-t1".into(), root: "/x/docs".into() };
        let p = s.hold("e1", "folder_grant", g(), "d".into(), "Let Unfold read /x/docs?".into(), "why".into(), 0);
        assert_eq!(s.decline("e1", "0".repeat(32).as_str()).unwrap_err(), Refusal::WrongToken);
        assert_eq!(s.decline("e1", p.token()).unwrap().call, g());
        assert_eq!(s.release("e1", p.token()).unwrap_err(), Refusal::Replayed, "a declined card cannot be approved after");
        let q = s.hold("e2", "folder_grant", g(), "d".into(), "s".into(), "w".into(), 0);
        s.retire_id("e2");
        assert_eq!(s.release("e2", q.token()).unwrap_err(), Refusal::Superseded, "its task ended");
        assert!(s.waiting().is_empty());
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_tap(&format!("/effect decline e2 {t}")), Some((Tap::Decline, "e2".into(), t.into())));
        assert_eq!(parse_tap(&format!("/effect release e2 {t}")), Some((Tap::Release, "e2".into(), t.into())));
        assert_eq!(parse_tap(&format!("/effect releasee2 {t}")), None);
        assert_eq!(parse_tap("/effect decline e2"), None);
        let r = redacted(&format!("/effect decline e2 {t}"));
        assert!(r.starts_with("/effect decline e2 sha256:") && !r.contains(t), "{r}");
    }

    #[test]
    fn the_journaled_tap_keeps_only_the_tokens_digest() {
        let t = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_release(&format!("/effect release b1-e7 {t}")), Some(("b1-e7".into(), t.into())));
        let r = redacted(&format!("/effect release b1-e7 {t}"));
        assert!(!r.contains(t) && r.ends_with(&sha256_hex(t)), "{r}");
        assert_eq!(parse_release("/effect release b1-e7"), None);
        assert_eq!(parse_release("/effect release b1-e7 tok extra"), None);
        assert_eq!(parse_release("/effect release b1-e7 tok;rm"), None);
        assert!(!redacted("/effect release x y z").contains('y'));
    }
}
