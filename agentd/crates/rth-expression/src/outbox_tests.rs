

use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::policy::{DropReason, Facts, TurnState, Verdict, decide, local_day};

const OFF: i32 = -180;

const NOON: u64 = 1_793_026_800_000;
const H: u64 = 3_600_000;

fn dir() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!("rth-expression-outbox-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn held(id: &str, at: u64, refs: &[&str]) -> Held {
    Held {
        id: id.into(),
        kind: Kind::Curiosity,
        subject_id: format!("s-{id}"),
        title: "Lua".into(),
        text: format!("the secret text of {id}"),
        reason_days: 0,
        memory_refs: refs.iter().map(|r| r.to_string()).collect(),
        egress: if refs.is_empty() { Egress::None } else { Egress::Memory },
        held_since_utc_ms: at,
    }
}

fn day(at: u64) -> i64 {
    local_day(at, OFF)
}

fn ids(o: &Outbox) -> Vec<String> {
    o.held().iter().map(|h| h.id.clone()).collect()
}

#[test]
fn b4_a_restart_keeps_the_days_count_and_the_dedupe() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.record_sent("x-1", Kind::Curiosity, "c-1", NOON - 2 * H, day(NOON - 2 * H), Destination::Chat, "one").unwrap();
    o.record_sent("x-2", Kind::OpenThreads, "h-1", NOON - H, day(NOON - H), Destination::Notification, "two").unwrap();
    o.hold(held("x-3", NOON - 1_000, &[]), day(NOON - 1_000)).unwrap();
    drop(o);
    let o = Outbox::open(&d).unwrap();
    assert_eq!(o.history().len(), 3, "held counts as an attempt");
    assert_eq!(ids(&o), ["x-3"]);
    let prefs = rth_self::SelfSpec::release_default("t").expression;
    let p = crate::policy::Presence { locked: false, display_asleep: false, idle_s: 0, output_busy: false, last_origin: "mac".into(), frontmost: true, notify_ok: true };
    let f = Facts {
        now_utc_ms: NOON,
        utc_offset_min: OFF,
        prefs: &prefs,
        presence: Some(&p),
        history: o.history(),
        turn: TurnState::default(),
        consents: &[],
        omni_recipient: None,
        omni_egress_granted: false,
    };
    let fresh = Candidate { kind: Kind::Curiosity, subject_id: "c-9".into(), egress: Egress::None, held: false, reason_days: 0, data: vec![], memory_refs: vec![] };
    assert_eq!(decide(&f, &fresh), Verdict::Drop(DropReason::Cap));
    let again = Candidate { subject_id: "c-1".into(), ..fresh.clone() };
    assert_eq!(decide(&f, &again), Verdict::Drop(DropReason::Duplicate));
    assert_eq!(decide(&f, &o.held()[0].candidate()), Verdict::Send(Destination::Chat), "surfacing doesn't count twice");
}

#[test]
fn b4_a_torn_tail_is_cut_and_an_earlier_bad_line_refuses() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.record_sent("x-1", Kind::Curiosity, "c-1", NOON, day(NOON), Destination::Chat, "one").unwrap();
    drop(o);
    let log = d.join(LOG_FILE);
    let whole = std::fs::read(&log).unwrap();
    std::fs::write(&log, [whole.as_slice(), br#"{"event":"attempt","id":"x-2","ki"#].concat()).unwrap();
    let mut o = Outbox::open(&d).unwrap();
    assert_eq!(o.history().len(), 1);
    assert_eq!(std::fs::read(&log).unwrap(), whole, "the torn tail is cut");
    o.record_sent("x-3", Kind::Curiosity, "c-3", NOON, day(NOON), Destination::Chat, "three").unwrap();
    assert_eq!(Outbox::open(&d).unwrap().history().len(), 2, "the next line starts clean");
    std::fs::write(&log, [b"not json\n".as_slice(), whole.as_slice()].concat()).unwrap();
    assert_eq!(Outbox::open(&d).unwrap_err().kind(), io::ErrorKind::InvalidData);
}

#[test]
fn b4_a_held_message_expires_after_a_day() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.hold(held("x-1", NOON, &[]), day(NOON)).unwrap();
    o.hold(held("x-2", NOON + H, &[]), day(NOON + H)).unwrap();
    assert_eq!(o.take_oldest(NOON + HELD_TTL_MS, |_| false).unwrap().map(|h| h.id), Some("x-2".into()), "x-1 is 24 h old");
    assert!(o.take_oldest(NOON + HELD_TTL_MS, |_| false).unwrap().is_none());
    assert!(Outbox::open(&d).unwrap().held().is_empty());
    let mut o = Outbox::open(&d).unwrap();
    o.hold(held("x-3", NOON, &[]), day(NOON)).unwrap();
    assert_eq!(o.take_oldest(NOON + HELD_TTL_MS - 1, |_| false).unwrap().map(|h| h.id), Some("x-3".into()), "a millisecond before");
}

#[test]
fn b4_a_forget_means_the_held_message_never_surfaces() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.hold(held("x-1", NOON, &["m-1", "m-2"]), day(NOON)).unwrap();
    o.hold(held("x-2", NOON + 1, &["m-3"]), day(NOON)).unwrap();
    assert_eq!(o.forget("m-2", NOON + 2).unwrap(), 1);
    assert_eq!(ids(&o), ["x-2"]);
    assert_eq!(ids(&Outbox::open(&d).unwrap()), ["x-2"], "and after a restart");

    assert!(o.take_oldest(NOON + 3, |m| m == "m-3").unwrap().is_none());
    assert!(Outbox::open(&d).unwrap().held().is_empty());
    assert_eq!(Outbox::open(&d).unwrap().history().len(), 2, "both still count as attempts");
}

#[test]
fn b4_a_crash_between_the_log_and_the_rewrite_never_sends_twice() {
    let d = dir();
    let unsent = d.join(UNSENT_FILE);
    let mut o = Outbox::open(&d).unwrap();
    o.hold(held("x-1", NOON, &[]), day(NOON)).unwrap();
    let before = std::fs::read(&unsent).unwrap();
    assert_eq!(o.take_oldest(NOON + 1, |_| false).unwrap().map(|h| h.id), Some("x-1".into()));

    std::fs::write(&unsent, &before).unwrap();
    let mut o = Outbox::open(&d).unwrap();
    assert!(o.held().is_empty(), "the log already closed it");
    assert!(o.take_oldest(NOON + 2, |_| false).unwrap().is_none());
    assert!(std::fs::read(&unsent).unwrap().is_empty(), "the stale copy is gone from disk too");

    o.hold(held("x-2", NOON + 3, &[]), day(NOON)).unwrap();
    std::fs::write(&unsent, b"").unwrap();
    let o = Outbox::open(&d).unwrap();
    assert!(o.held().is_empty());
    assert_eq!(o.history().len(), 2);

    std::fs::write(&unsent, [serde_json::to_vec(&held("x-9", NOON, &[])).unwrap(), b"\n".to_vec()].concat()).unwrap();
    assert!(Outbox::open(&d).unwrap().held().is_empty());
}

#[test]
fn b4_the_log_never_holds_text_and_debug_never_prints_it() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.record_sent("x-1", Kind::Curiosity, "c-1", NOON, day(NOON), Destination::Chat, "the secret text of x-1").unwrap();
    o.hold(held("x-2", NOON, &["m-1"]), day(NOON)).unwrap();
    o.take_oldest(NOON + 1, |_| false).unwrap();
    let log = std::fs::read_to_string(d.join(LOG_FILE)).unwrap();
    assert!(!log.contains("secret") && !log.contains("Lua"), "{log}");
    assert!(log.contains(&rth_self::store::sha256_hex(b"the secret text of x-1")));
    let h = held("x-3", NOON, &[]);
    assert!(!format!("{h:?}").contains("secret"), "{h:?}");
    assert_eq!(h.message().reason().render(crate::Lang::En), "Why now: this has been on my mind since we talked.");
}

#[test]
fn b4_at_most_nine_are_held_and_the_oldest_goes_first() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    for i in 0..=MAX_HELD as u64 {
        o.hold(held(&format!("x-{i}"), NOON + i, &[]), day(NOON)).unwrap();
    }
    assert_eq!(o.held().len(), MAX_HELD);
    assert_eq!(o.held()[0].id, "x-1");
    assert_eq!(Outbox::open(&d).unwrap().held().len(), MAX_HELD);
}

#[test]
fn b7_a_held_message_can_be_dropped_by_id_or_by_a_forgotten_memory_and_stays_gone() {
    let d = dir();
    let mut o = Outbox::open(&d).unwrap();
    o.hold(held("x-1", NOON, &[]), day(NOON)).unwrap();
    o.hold(held("x-2", NOON + 1, &["m-2"]), day(NOON)).unwrap();
    o.hold(held("x-3", NOON + 2, &[]), day(NOON)).unwrap();
    assert_eq!(o.drop_held("x-1", NOON + 3).unwrap(), 1);
    assert_eq!(o.drop_held("x-9", NOON + 3).unwrap(), 0, "an unknown id drops nothing");
    assert_eq!(o.drop_forgotten(NOON + 4, |m| m == "m-2").unwrap(), 1);
    assert_eq!(ids(&o), ["x-3"]);
    assert_eq!(ids(&Outbox::open(&d).unwrap()), ["x-3"], "and after a restart");
    let log = std::fs::read_to_string(d.join("expression.jsonl")).unwrap();
    assert!(log.contains(r#""event":"dropped","id":"x-1""#) && log.contains(r#""event":"forgotten","id":"x-2""#), "{log}");
    assert_eq!(Outbox::open(&d).unwrap().history().len(), 3, "all three still count as attempts");
}
