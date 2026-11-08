

use rth_self::store::sha256_hex;
use rth_self::{Drives, Horizon, HorizonLayer, HorizonStatus, Kinds, Proposal, ProposalKind, ProposalStatus};
use serde::Serialize;

use crate::policy::{Candidate, Datum, Egress, Kind, Label};

pub const THREAD_IDLE_MS: u64 = 24 * 60 * 60 * 1000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

pub const CURIOSITY_MEMORIES: usize = 3;

#[derive(Clone, PartialEq, Eq)]
pub struct MemoryItem {
    pub id: String,
    pub text: String,
}

impl std::fmt::Debug for MemoryItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryItem({}, {} bytes)", self.id, self.text.len())
    }
}

pub trait Memories {

    fn recent(&self, limit: usize) -> Result<Vec<MemoryItem>, String>;
    fn is_forgotten(&self, memory_id: &str) -> bool;

    fn eligibility_unknown(&self) -> bool;
}

#[derive(Clone, PartialEq, Eq)]
pub struct Reflection {
    pub id: String,
    pub text: String,
    pub memory_refs: Vec<String>,
}

impl std::fmt::Debug for Reflection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Reflection({}, {} bytes, {} refs)", self.id, self.text.len(), self.memory_refs.len())
    }
}

pub trait Overnight {
    fn reflections(&self, now_utc_ms: u64) -> Vec<Reflection>;
}

pub struct NoOvernight;

impl Overnight for NoOvernight {
    fn reflections(&self, _now_utc_ms: u64) -> Vec<Reflection> {
        vec![]
    }
}

pub struct FakeOvernight(pub Vec<Reflection>);

impl Overnight for FakeOvernight {
    fn reflections(&self, _now_utc_ms: u64) -> Vec<Reflection> {
        self.0.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {

    NoEgress,

    Forgotten,
    EligibilityUnknown,

    MemoryUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skip {
    pub kind: Kind,
    pub subject_id: Option<String>,
    pub reason: SkipReason,
}

pub struct Sources<'a> {
    pub kinds: &'a Kinds,
    pub now_utc_ms: u64,

    pub name_proposal: Option<&'a Proposal>,

    pub horizons: &'a [Horizon],
    pub drives: &'a Drives,
    pub memories: &'a dyn Memories,
    pub overnight: &'a dyn Overnight,

    pub egress_granted: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gathered {

    pub candidates: Vec<Candidate>,
    pub skipped: Vec<Skip>,
}

pub fn drive_subject(text: &str) -> String {
    format!("drive:{}", &sha256_hex(text.as_bytes())[..16])
}

fn candidate(kind: Kind, subject_id: String, egress: Egress, reason_days: u32, data: Vec<Datum>, memory_refs: Vec<String>) -> Candidate {
    Candidate { kind, subject_id, egress, held: false, reason_days, data, memory_refs }
}

fn memory_refs(evidence_ids: &[String]) -> Vec<String> {
    let memory = |e: &String| match e.split_once(':') {
        Some(("memory", id)) => Some(id.to_string()),
        Some(("chat" | "sleep", _)) => None,
        _ => Some(e.clone()),
    };
    evidence_ids.iter().filter_map(memory).collect()
}

fn datum(label: Label, text: &str) -> Datum {
    Datum { label, text: text.to_string() }
}

impl Gathered {

    fn offer(&mut self, s: &Sources, c: Candidate) {
        let memory = c.egress == Egress::Memory || !c.memory_refs.is_empty();
        let reason = if c.egress == Egress::Memory && !s.egress_granted {
            Some(SkipReason::NoEgress)
        } else if memory && s.memories.eligibility_unknown() {
            Some(SkipReason::EligibilityUnknown)
        } else if c.memory_refs.iter().any(|m| s.memories.is_forgotten(m)) {
            Some(SkipReason::Forgotten)
        } else {
            None
        };
        match reason {
            None => self.candidates.push(c),
            Some(reason) => self.skipped.push(Skip { kind: c.kind, subject_id: Some(c.subject_id), reason }),
        }
    }
}

pub fn gather(s: &Sources) -> Gathered {
    let mut g = Gathered::default();
    let pending = s.name_proposal.filter(|p| p.kind == ProposalKind::Name && p.status == ProposalStatus::Pending);
    if s.kinds.name_wish
        && let Some(p) = pending
    {
        let egress = if p.evidence_ids.is_empty() { Egress::None } else { Egress::Memory };
        let data = vec![datum(Label::Name, p.text.as_str()), datum(Label::Why, p.why.as_str())];
        g.offer(s, candidate(Kind::NameWish, p.id.as_str().to_string(), egress, 0, data, memory_refs(&p.evidence_ids)));
    }
    if s.kinds.overnight {

        for r in s.overnight.reflections(s.now_utc_ms) {
            g.offer(s, candidate(Kind::Overnight, r.id, Egress::Memory, 0, vec![Datum { label: Label::Reflection, text: r.text }], r.memory_refs));
        }
    }
    if s.kinds.curiosity {
        let d = s.drives;
        for (label, list) in [(Label::Motivation, &d.motivations), (Label::Aspiration, &d.aspirations), (Label::Routine, &d.routine)] {
            for x in list {
                g.offer(s, candidate(Kind::Curiosity, drive_subject(x.text.as_str()), Egress::None, 0, vec![datum(label, x.text.as_str())], vec![]));
            }
        }
        let unread = if !s.egress_granted {
            Some(SkipReason::NoEgress)
        } else if s.memories.eligibility_unknown() {
            Some(SkipReason::EligibilityUnknown)
        } else {
            None
        };
        let recent = match unread {
            Some(reason) => Err(reason),
            None => s.memories.recent(CURIOSITY_MEMORIES).map_err(|_| SkipReason::MemoryUnavailable),
        };
        match recent {
            Err(reason) => g.skipped.push(Skip { kind: Kind::Curiosity, subject_id: None, reason }),
            Ok(items) => {
                for m in items.into_iter().take(CURIOSITY_MEMORIES) {
                    g.offer(s, candidate(Kind::Curiosity, format!("memory:{}", m.id), Egress::Memory, 0, vec![Datum { label: Label::Memory, text: m.text }], vec![m.id]));
                }
            }
        }
    }
    if s.kinds.open_threads {
        let idle = |h: &&Horizon| h.layer == HorizonLayer::Thread && h.status == HorizonStatus::Open && s.now_utc_ms.saturating_sub(h.last_touched_utc_ms) > THREAD_IDLE_MS;
        for h in s.horizons.iter().filter(idle) {
            let days = u32::try_from(s.now_utc_ms.saturating_sub(h.created_utc_ms) / DAY_MS).unwrap_or(u32::MAX);
            g.offer(s, candidate(Kind::OpenThreads, h.id.as_str().to_string(), Egress::None, days, vec![datum(Label::Thread, h.text.as_str())], vec![]));
        }
    }
    g
}

#[cfg(test)]
#[path = "sources_tests.rs"]
mod tests;
