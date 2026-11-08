

use std::cell::Cell;

use rth_self::{BoundedText, Drive, DriveSource};

use super::*;
use crate::reason::Lang;

const NOW: u64 = 1_792_972_800_000;
const H: u64 = 60 * 60 * 1000;

fn proposal(id: &str, kind: &str, status: &str, evidence: &[&str]) -> Proposal {
    let text = if kind == "name" { "Lua" } else { "learn the guitar" };
    let line = serde_json::json!({"id": id, "kind": kind, "text": text, "why": "it came up twice this week", "evidence_ids": evidence, "created_utc_ms": 1, "status": status, "decided_utc_ms": null});
    Proposal::parse(&line.to_string()).unwrap()
}

fn horizon(id: &str, layer: &str, status: &str, created: u64, touched: u64) -> Horizon {
    let line = serde_json::json!({"id": id, "layer": layer, "status": status, "text": "plan the trip", "created_utc_ms": created, "last_touched_utc_ms": touched, "task_id": null});
    Horizon::parse(&line.to_string()).unwrap()
}

fn drive(text: &str, source: DriveSource) -> Drive {
    Drive { text: BoundedText::new(text).unwrap(), source }
}

fn drives() -> Drives {
    Drives { motivations: vec![drive("make the person laugh", DriveSource::Person)], aspirations: vec![drive("learn to paint", DriveSource::Agent)], routine: vec![drive("ask about the week", DriveSource::Person)] }
}

fn all_on() -> Kinds {
    Kinds { name_wish: true, overnight: true, curiosity: true, open_threads: true }
}

fn mem(id: &str) -> MemoryItem {
    MemoryItem { id: id.into(), text: format!("secret text of {id}") }
}

#[derive(Default)]
struct FakeMemories {
    items: Vec<MemoryItem>,
    forgotten: Vec<String>,
    unknown: bool,
    fail: bool,
    reads: Cell<usize>,
    asked: Cell<usize>,
}

impl Memories for FakeMemories {
    fn recent(&self, limit: usize) -> Result<Vec<MemoryItem>, String> {
        self.reads.set(self.reads.get() + 1);
        self.asked.set(limit);
        if self.fail { Err("unavailable".into()) } else { Ok(self.items.clone()) }
    }
    fn is_forgotten(&self, memory_id: &str) -> bool {
        self.forgotten.iter().any(|f| f == memory_id)
    }
    fn eligibility_unknown(&self) -> bool {
        self.unknown
    }
}

struct Fx {
    kinds: Kinds,
    proposal: Option<Proposal>,
    horizons: Vec<Horizon>,
    drives: Drives,
    memories: FakeMemories,
    overnight: Vec<Reflection>,
    grant: bool,
}

impl Fx {
    fn new() -> Fx {
        Fx { kinds: all_on(), proposal: None, horizons: vec![], drives: Drives::default(), memories: FakeMemories::default(), overnight: vec![], grant: true }
    }

    fn gather(&self) -> Gathered {
        let night = FakeOvernight(self.overnight.clone());
        gather(&Sources {
            kinds: &self.kinds,
            now_utc_ms: NOW,
            name_proposal: self.proposal.as_ref(),
            horizons: &self.horizons,
            drives: &self.drives,
            memories: &self.memories,
            overnight: &night,
            egress_granted: self.grant,
        })
    }
}

fn keys(g: &Gathered) -> Vec<(Kind, String, Egress)> {
    g.candidates.iter().map(|c| (c.kind, c.subject_id.clone(), c.egress)).collect()
}

fn skip(kind: Kind, subject: Option<&str>, reason: SkipReason) -> Skip {
    Skip { kind, subject_id: subject.map(String::from), reason }
}

#[test]
fn b5_a_pending_name_proposal_is_one_candidate_keyed_by_its_id() {
    let mut fx = Fx::new();
    fx.proposal = Some(proposal("p-1", "name", "pending", &[]));
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::NameWish, "p-1".to_string(), Egress::None)]);
    let c = &g.candidates[0];
    assert_eq!(c.data, [Datum { label: Label::Name, text: "Lua".into() }, Datum { label: Label::Why, text: "it came up twice this week".into() }]);
    assert!(c.memory_refs.is_empty() && !c.held);
    assert_eq!(c.reason().render(Lang::En), "Why now: I have a name idea waiting for your OK.");
    assert!(g.skipped.is_empty());
}

#[test]
fn b5_a_decided_or_other_proposal_is_no_candidate() {
    for p in [proposal("p-1", "name", "approved", &[]), proposal("p-2", "name", "declined", &[]), proposal("p-3", "drive", "pending", &[])] {
        let mut fx = Fx::new();
        fx.proposal = Some(p);
        assert_eq!(fx.gather(), Gathered::default());
    }
}

#[test]
fn b5_a_name_proposal_with_evidence_carries_memory_and_needs_the_grant() {
    let mut fx = Fx::new();
    fx.proposal = Some(proposal("p-1", "name", "pending", &["memory:m-1", "chat:host-a:3", "sleep:run-1", "m-2"]));
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::NameWish, "p-1".to_string(), Egress::Memory)]);
    assert_eq!(g.candidates[0].memory_refs, ["m-1", "m-2"], "the bare ids a forget names; an untyped id may be a memory");
    fx.proposal = Some(proposal("p-1", "name", "pending", &["chat:host-a:3"]));
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::NameWish, "p-1".to_string(), Egress::Memory)], "any evidence counts as memory egress");
    assert!(g.candidates[0].memory_refs.is_empty());
    fx.proposal = Some(proposal("p-1", "name", "pending", &["memory:m-1"]));
    fx.grant = false;
    let curiosity_unread = skip(Kind::Curiosity, None, SkipReason::NoEgress);
    assert_eq!(fx.gather(), Gathered { candidates: vec![], skipped: vec![skip(Kind::NameWish, Some("p-1"), SkipReason::NoEgress), curiosity_unread] });
}

#[test]
fn b5_a_task_thread_is_offered_once_untouched_for_more_than_a_day() {
    let mut fx = Fx::new();
    fx.horizons = vec![horizon("h-1", "thread", "open", NOW - 3 * DAY_MS - H, NOW - THREAD_IDLE_MS)];
    assert_eq!(fx.gather(), Gathered::default(), "exactly 24 h is not more than a day");
    fx.horizons = vec![horizon("h-1", "thread", "open", NOW - 3 * DAY_MS - H, NOW - THREAD_IDLE_MS - 1)];
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::OpenThreads, "h-1".to_string(), Egress::None)]);
    let c = &g.candidates[0];
    assert_eq!(c.data, [Datum { label: Label::Thread, text: "plan the trip".into() }]);
    assert_eq!(c.reason().render(Lang::En), "Why now: this thread has been open for 3 days.", "days since it opened");
    assert!(c.memory_refs.is_empty());
}

#[test]
fn b5_only_open_task_threads_are_offered() {
    let old = NOW - 5 * DAY_MS;
    let mut fx = Fx::new();
    fx.horizons = vec![
        horizon("h-goal", "goal", "open", old, old),
        horizon("h-sugg", "thread", "suggested", old, old),
        horizon("h-done", "thread", "done", old, old),
        horizon("h-decl", "thread", "declined", old, old),
        horizon("h-new", "thread", "open", old, NOW - H),
        horizon("h-ok", "thread", "open", old, old),
    ];
    assert_eq!(keys(&fx.gather()), [(Kind::OpenThreads, "h-ok".to_string(), Egress::None)]);
}

#[test]
fn b5_curiosity_offers_each_drive_then_at_most_three_memories() {
    let mut fx = Fx::new();
    fx.drives = drives();
    fx.memories.items = vec![mem("m-1"), mem("m-2"), mem("m-3"), mem("m-4")];
    let g = fx.gather();
    let want: Vec<(Kind, String, Egress)> = [
        (drive_subject("make the person laugh"), Egress::None),
        (drive_subject("learn to paint"), Egress::None),
        (drive_subject("ask about the week"), Egress::None),
        ("memory:m-1".to_string(), Egress::Memory),
        ("memory:m-2".to_string(), Egress::Memory),
        ("memory:m-3".to_string(), Egress::Memory),
    ]
    .into_iter()
    .map(|(s, e)| (Kind::Curiosity, s, e))
    .collect();
    assert_eq!(keys(&g), want);
    assert_eq!(fx.memories.asked.get(), CURIOSITY_MEMORIES);
    let labels: Vec<Label> = g.candidates.iter().map(|c| c.data[0].label).collect();
    assert_eq!(labels, [Label::Motivation, Label::Aspiration, Label::Routine, Label::Memory, Label::Memory, Label::Memory]);
    assert_eq!(g.candidates[3].memory_refs, ["m-1"]);
    assert_eq!(g.candidates[3].data, [Datum { label: Label::Memory, text: "secret text of m-1".into() }]);
    assert!(g.candidates[..3].iter().all(|c| c.memory_refs.is_empty()));
    assert_eq!(g.candidates[0].reason().render(Lang::En), "Why now: this has been on my mind since we talked.");
}

#[test]
fn b5_a_forgotten_memory_is_never_a_candidate() {
    let mut fx = Fx::new();
    fx.memories.items = vec![mem("m-1"), mem("m-2")];
    fx.memories.forgotten = vec!["m-1".into()];
    fx.proposal = Some(proposal("p-1", "name", "pending", &["memory:m-9", "chat:host-a:3", "memory:m-1"]));
    fx.overnight = vec![Reflection { id: "n-1".into(), text: "about m-1".into(), memory_refs: vec!["m-1".into()] }];
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::Curiosity, "memory:m-2".to_string(), Egress::Memory)]);
    assert_eq!(g.skipped, [skip(Kind::NameWish, Some("p-1"), SkipReason::Forgotten), skip(Kind::Overnight, Some("n-1"), SkipReason::Forgotten), skip(Kind::Curiosity, Some("memory:m-1"), SkipReason::Forgotten)]);
}

#[test]
fn b5_without_the_grant_curiosity_offers_drives_only_and_never_reads_memory() {
    let mut fx = Fx::new();
    fx.grant = false;
    fx.drives = drives();
    fx.memories.items = vec![mem("m-1")];
    let g = fx.gather();
    assert_eq!(g.candidates.len(), 3);
    assert!(g.candidates.iter().all(|c| c.kind == Kind::Curiosity && c.egress == Egress::None));
    assert_eq!(g.skipped, [skip(Kind::Curiosity, None, SkipReason::NoEgress)]);
    assert_eq!(fx.memories.reads.get(), 0, "memory is not read without the grant");
    assert_eq!(serde_json::to_value(&g.skipped[0]).unwrap(), serde_json::json!({"kind": "curiosity", "subject_id": null, "reason": "no_egress"}), "the journaled shape");
}

#[test]
fn b5_eligibility_unknown_or_unreadable_memory_skips_the_memory_kinds() {
    let mut fx = Fx::new();
    fx.drives = Drives { motivations: vec![drive("make the person laugh", DriveSource::Person)], ..Drives::default() };
    fx.memories.items = vec![mem("m-1")];
    fx.memories.unknown = true;
    fx.proposal = Some(proposal("p-1", "name", "pending", &["memory:m-1"]));
    fx.overnight = vec![Reflection { id: "n-1".into(), text: "a thought".into(), memory_refs: vec![] }];
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::Curiosity, drive_subject("make the person laugh"), Egress::None)]);
    let unknown = SkipReason::EligibilityUnknown;
    assert_eq!(g.skipped, [skip(Kind::NameWish, Some("p-1"), unknown), skip(Kind::Overnight, Some("n-1"), unknown), skip(Kind::Curiosity, None, unknown)]);
    assert_eq!(fx.memories.reads.get(), 0);
    fx.memories.unknown = false;
    fx.memories.fail = true;
    fx.proposal = None;
    fx.overnight = vec![];
    let g = fx.gather();
    assert_eq!(g.candidates.len(), 1, "the drive still goes");
    assert_eq!(g.skipped, [skip(Kind::Curiosity, None, SkipReason::MemoryUnavailable)]);
}

#[test]
fn b5_the_overnight_source_flows_and_always_counts_as_memory() {
    let mut fx = Fx::new();
    fx.overnight = vec![Reflection { id: "n-1".into(), text: "a thought".into(), memory_refs: vec![] }, Reflection { id: "n-2".into(), text: "another".into(), memory_refs: vec!["m-4".into()] }];
    let g = fx.gather();
    assert_eq!(keys(&g), [(Kind::Overnight, "n-1".to_string(), Egress::Memory), (Kind::Overnight, "n-2".to_string(), Egress::Memory)]);
    assert_eq!(g.candidates[1].memory_refs, ["m-4"]);
    assert_eq!(g.candidates[0].data, [Datum { label: Label::Reflection, text: "a thought".into() }]);
    assert_eq!(g.candidates[0].reason().render(Lang::En), "Why now: this came out of last night's reflection.");
    fx.grant = false;
    let no = SkipReason::NoEgress;
    assert_eq!(fx.gather().skipped, [skip(Kind::Overnight, Some("n-1"), no), skip(Kind::Overnight, Some("n-2"), no), skip(Kind::Curiosity, None, no)]);
    assert!(NoOvernight.reflections(NOW).is_empty(), "no reflection producer is enabled");
}

#[test]
fn b5_a_kind_that_is_off_is_not_read() {
    let mut fx = Fx::new();
    fx.proposal = Some(proposal("p-1", "name", "pending", &[]));
    fx.horizons = vec![horizon("h-1", "thread", "open", NOW - 5 * DAY_MS, NOW - 5 * DAY_MS)];
    fx.drives = drives();
    fx.memories.items = vec![mem("m-1")];
    fx.overnight = vec![Reflection { id: "n-1".into(), text: "a thought".into(), memory_refs: vec![] }];
    let all = fx.gather();
    assert_eq!(all.candidates.iter().map(|c| c.kind).collect::<std::collections::BTreeSet<_>>().len(), 4);
    assert_eq!(fx.memories.reads.get(), 1);
    for k in Kind::ALL {
        let mut off = all_on();
        match k {
            Kind::NameWish => off.name_wish = false,
            Kind::Overnight => off.overnight = false,
            Kind::Curiosity => off.curiosity = false,
            Kind::OpenThreads => off.open_threads = false,
        }
        fx.kinds = off;
        fx.memories.reads.set(0);
        let g = fx.gather();
        assert!(g.candidates.iter().all(|c| c.kind != k), "{k:?} off");
        assert_eq!(g.candidates.len(), all.candidates.iter().filter(|c| c.kind != k).count(), "{k:?} off leaves the others");
        assert_eq!(fx.memories.reads.get(), usize::from(k != Kind::Curiosity), "{k:?} off");
    }
}

#[test]
fn b5_candidates_come_in_kind_order() {
    let mut fx = Fx::new();
    fx.horizons = vec![horizon("h-1", "thread", "open", NOW - 5 * DAY_MS, NOW - 5 * DAY_MS)];
    fx.drives = Drives { aspirations: vec![drive("learn to paint", DriveSource::Agent)], ..Drives::default() };
    fx.overnight = vec![Reflection { id: "n-1".into(), text: "a thought".into(), memory_refs: vec![] }];
    fx.proposal = Some(proposal("p-1", "name", "pending", &[]));
    let kinds: Vec<Kind> = fx.gather().candidates.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, Kind::ALL);
}

#[test]
fn b5_a_drive_key_is_its_text_hash() {
    let a = drive_subject("learn to paint");
    assert_eq!(a, drive_subject("learn to paint"));
    assert_ne!(a, drive_subject("learn to paint!"));
    assert!(a.starts_with("drive:") && a.len() == "drive:".len() + 16, "{a}");
}

#[test]
fn b5_debug_prints_no_text() {
    let mut fx = Fx::new();
    fx.drives = drives();
    fx.memories.items = vec![mem("m-1")];
    fx.proposal = Some(proposal("p-1", "name", "pending", &[]));
    let printed = format!("{:?} {:?} {:?}", fx.gather(), mem("m-1"), Reflection { id: "n-1".into(), text: "a thought".into(), memory_refs: vec![] });
    for secret in ["secret text", "make the person laugh", "Lua", "it came up", "a thought"] {
        assert!(!printed.contains(secret), "{secret} in {printed}");
    }
    assert!(printed.contains("memory:m-1"), "ids stay: {printed}");
}
