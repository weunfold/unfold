use super::*;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rth-memory-{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("memory.jsonl")
}

fn src(call: &str) -> Source {
    Source { provider_generation: 1, call_id: call.into(), response_id: Some("r1".into()), command_id: Some("c1".into()) }
}

fn ok() -> Option<Acceptance> {
    Some(Acceptance { accepted_by: "typed_command".into(), acceptance_event_id: "c1".into() })
}

fn remember(t: &str) -> Request {
    Request::Remember { text: t.into(), kind: None, speaker: None }
}

fn open(p: &Path) -> (Ledger, Recovered) {
    Ledger::open(p, "inst", PRIVATE_SCOPE, "b1").unwrap()
}

#[test]
fn a_remembered_item_survives_reopen_with_its_id_revision_and_source() {
    let p = dir("reopen");
    let (mut l, _) = open(&p);
    let o = l.act("k1", remember("The test code word is lantern-42."), src("call-1"), ok(), 10).unwrap();
    assert_eq!((o.act.state, o.act.memory_id.as_deref(), o.act.revision, o.duplicate), (State::Committed, Some("m-1"), Some(1), false));
    drop(l);
    let (l, rec) = open(&p);
    assert_eq!(rec, Recovered { truncated_bytes: 0, acts: 1, items: 1 });
    let r = l.recall();
    assert_eq!(r.items.len(), 1);
    assert_eq!((r.items[0].memory_id.as_str(), r.items[0].revision, r.items[0].text.as_str()), ("m-1", 1, "The test code word is lantern-42."));
    let line = std::fs::read_to_string(&p).unwrap();
    assert!(line.contains("\"call_id\":\"call-1\"") && line.contains("\"command_id\":\"c1\""), "provenance is on disk");
}

#[test]
fn act_duplicate_retry_returns_the_first_outcome_and_writes_nothing() {
    let p = dir("dup");
    let (mut l, _) = open(&p);
    let a = l.act("k1", remember("x"), src("call-1"), ok(), 10).unwrap();
    let len = std::fs::metadata(&p).unwrap().len();
    let b = l.act("k1", remember("something else"), src("call-1"), ok(), 11).unwrap();
    assert!(b.duplicate);
    assert_eq!(a.act, b.act);
    assert_eq!(std::fs::metadata(&p).unwrap().len(), len);
    assert_eq!(l.recall().total, 1);
}

#[test]
fn a_correction_appends_a_revision_and_keeps_history() {
    let p = dir("correct");
    let (mut l, _) = open(&p);
    l.act("k1", remember("The code word is lantern-42."), src("c-1"), ok(), 10).unwrap();
    let c = l.act("k2", Request::Correct { memory_id: "m-1".into(), expected_revision: 1, text: "The code word is harbor-17.".into() }, src("c-2"), ok(), 20).unwrap();
    assert_eq!((c.act.state, c.act.revision), (State::Committed, Some(2)));
    drop(l);
    let (l, _) = open(&p);
    let item = l.item("m-1").unwrap();
    assert_eq!(item.revisions.len(), 2);
    assert_eq!(item.revisions[0].text, "The code word is lantern-42.");
    assert_eq!(item.current().text, "The code word is harbor-17.");
    let r = l.recall();
    assert_eq!((r.items[0].revision, r.items[0].text.as_str(), r.items[0].first_saved_utc_ms), (2, "The code word is harbor-17.", 10));
}

#[test]
fn act_stale_correction_is_rejected_and_changes_nothing() {
    let p = dir("stale");
    let (mut l, _) = open(&p);
    l.act("k1", remember("a"), src("c-1"), ok(), 10).unwrap();
    l.act("k2", Request::Correct { memory_id: "m-1".into(), expected_revision: 1, text: "b".into() }, src("c-2"), ok(), 20).unwrap();
    let s = l.act("k3", Request::Correct { memory_id: "m-1".into(), expected_revision: 1, text: "c".into() }, src("c-3"), ok(), 30).unwrap();
    assert_eq!((s.act.state, s.act.reason.as_deref()), (State::Rejected, Some("stale_revision")));
    assert_eq!(l.item("m-1").unwrap().current().text, "b");
    let u = l.act("k4", Request::Correct { memory_id: "m-9".into(), expected_revision: 1, text: "c".into() }, src("c-4"), ok(), 40).unwrap();
    assert_eq!(u.act.reason.as_deref(), Some("unknown_target"));
}

#[test]
fn equal_text_is_refused_naming_the_live_item() {
    let p = dir("equal");
    let (mut l, _) = open(&p);
    l.act("k1", remember("same"), src("c-1"), ok(), 10).unwrap();
    let d = l.act("k2", remember("same"), src("c-2"), ok(), 11).unwrap();
    assert_eq!((d.act.state, d.act.reason.as_deref(), d.act.memory_id.as_deref()), (State::Rejected, Some("duplicate_of"), Some("m-1")));
    l.act("k3", remember("other"), src("c-3"), ok(), 12).unwrap();
    assert_eq!(l.item("m-2").unwrap().current().text, "other", "a refusal allocates no id");
    drop(l);
    let (_, rec) = open(&p);
    assert_eq!((rec.acts, rec.items), (3, 2));
}

#[test]
fn the_reference_store_passes_the_conformance_suite() {
    let p = dir("conformance");
    let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
    crate::contract::conformance::run(&mut open_ref, "ref");
}

#[test]
fn the_reference_store_invalidates_and_restores_without_deleting() {
    let p = dir("conformance-invalidate");
    let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
    crate::contract::conformance::run_invalidate(&mut open_ref, "ref");

    let (l, _) = Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap();
    let kinds: Vec<Kind> = l.by_key.values().filter(|a| a.state == State::Committed).map(|a| a.kind).collect();
    assert!(kinds.contains(&Kind::Remember) && kinds.contains(&Kind::Invalidate) && kinds.contains(&Kind::Restore), "{kinds:?}");
}

#[test]
fn without_a_trusted_acceptance_an_act_is_only_proposed() {
    let p = dir("proposed");
    let (mut l, _) = open(&p);
    let o = l.act("k1", remember("spoken fact"), src("c-1"), None, 10).unwrap();
    assert_eq!((o.act.state, o.act.reason.as_deref(), o.act.memory_id.as_deref()), (State::Proposed, Some("not_accepted"), None));
    assert_eq!(l.recall().total, 0);
    drop(l);
    let (l, rec) = open(&p);
    assert_eq!((rec.acts, rec.items, l.recall().total), (1, 0, 0));
}

#[test]
fn size_limits_refuse_before_any_change() {
    let p = dir("limits");
    let (mut l, _) = open(&p);
    let long = "x".repeat(MAX_TEXT_BYTES + 1);
    let o = l.act("k1", remember(&long), src("c-1"), ok(), 10).unwrap();
    assert_eq!((o.act.state, o.act.reason.as_deref(), o.act.text.as_str()), (State::Rejected, Some("text_too_long"), ""));
    let e = l.act("k2", remember("   "), src("c-2"), ok(), 10).unwrap();
    assert_eq!(e.act.reason.as_deref(), Some("empty_text"));
    assert_eq!(l.recall().total, 0);
}

#[test]
fn a_torn_tail_is_cut_and_persisted_before_the_next_append() {
    let p = dir("torn");
    {
        let (mut l, _) = open(&p);
        l.act("k1", remember("kept"), src("c-1"), ok(), 10).unwrap();
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"{\"schema\":\"rth-mem").unwrap();
    drop(f);
    let (mut l, rec) = open(&p);
    assert_eq!((rec.truncated_bytes, rec.items), (18, 1));
    l.act("k2", remember("after the tear"), src("c-2"), ok(), 20).unwrap();
    drop(l);
    let (l, rec) = open(&p);
    assert_eq!((rec.truncated_bytes, rec.acts, rec.items), (0, 2, 2), "the acknowledged act survives reopen");
    assert_eq!(l.item("m-2").unwrap().current().text, "after the tear");
}

#[test]
fn interior_damage_and_foreign_instances_refuse_the_ledger() {
    let p = dir("damage");
    {
        let (mut l, _) = open(&p);
        l.act("k1", remember("a"), src("c-1"), ok(), 10).unwrap();
    }
    let good = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, format!("not json\n{good}")).unwrap();
    assert!(Ledger::open(&p, "inst", PRIVATE_SCOPE, "b2").is_err());
    std::fs::write(&p, &good).unwrap();
    assert!(Ledger::open(&p, "other", PRIVATE_SCOPE, "b2").is_err(), "another instance's ledger");
    assert!(Ledger::open(&p, "inst", "shared", "b2").is_err(), "another scope");
    assert!(Ledger::open(&p, "inst", PRIVATE_SCOPE, "b2").is_ok());
}

#[test]
fn a_read_error_is_not_an_empty_ledger() {
    let p = dir("readerr");
    std::fs::create_dir_all(&p).unwrap();
    assert!(Ledger::open(&p, "inst", PRIVATE_SCOPE, "b1").is_err());
}

#[test]
fn recall_is_bounded_and_counts_what_it_omits() {
    let p = dir("bound");
    let (mut l, _) = open(&p);
    for i in 0..(RECALL_MAX_ITEMS + 5) {
        l.act(&format!("k{i}"), remember(&format!("fact {i}")), src(&format!("c-{i}")), ok(), i as u64).unwrap();
    }
    let r = l.recall();
    assert_eq!((r.items.len(), r.total, r.omitted), (RECALL_MAX_ITEMS, RECALL_MAX_ITEMS + 5, 5));
    assert_eq!(r.items[0].text, format!("fact {}", RECALL_MAX_ITEMS + 4), "newest first");
}

mod pv05_g6 {
    use super::*;

    #[test]
    fn a_person_item_never_reaches_recall_on_the_reference_store() {
        let p = dir("pv05-g6-person");
        let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
        crate::contract::conformance::run_person(&mut open_ref, "ref");
    }

    #[test]
    fn pv05_g9_an_item_keeps_its_speaker_on_the_reference_store() {
        let p = dir("pv05-g9-speaker");
        let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
        crate::contract::conformance::run_speaker(&mut open_ref, "ref");
    }

    #[test]
    fn pv05_g9_a_people_correction_retags_an_item_on_the_reference_store() {
        let p = dir("pv05-g9-retag");
        let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
        crate::contract::conformance::run_retag(&mut open_ref, "ref");
    }

    #[test]
    fn pv05_g9_every_suite_runs_on_one_store_under_one_tag() {
        let p = dir("pv05-g9-all");
        let mut open_ref = || -> Box<dyn crate::contract::Provider> { Box::new(Reference { ledger: Ledger::open(&p, "conformance", PRIVATE_SCOPE, "b").unwrap().0 }) };
        crate::contract::conformance::run(&mut open_ref, "ref");
        crate::contract::conformance::run_person(&mut open_ref, "ref");
        crate::contract::conformance::run_speaker(&mut open_ref, "ref");
        crate::contract::conformance::run_retag(&mut open_ref, "ref");
    }
}
