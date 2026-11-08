

use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rth-tasks-{}-{}-{name}", std::process::id(), rand_tag()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("tasks.jsonl")
}

fn rand_tag() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed) + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64 % 1_000_000
}

fn inc(launch: &str, pi: &str) -> Incarnation {
    Incarnation { launch_id: launch.into(), pid: 4242, plugin_instance: pi.into() }
}

fn key(call: &str) -> CallKey {
    CallKey { instance_id: "inst".into(), provider_generation: 1, call_id: call.into() }
}

fn args() -> Value {
    json!({"purpose":"report","task":"relatório consolidado pro meu agente de código","audience":"coding_agent"})
}

struct H {
    l: Ledger,
    e1: Incarnation,
    seq: u64,
}

impl H {
    fn new(name: &str) -> (H, PathBuf) {
        let p = tmp(name);
        let (mut l, _) = Ledger::open(&p, "inst", "b1").unwrap();
        let e1 = inc("L1", "p1");
        l.register_launch("L1", &sha_hex(b"secret-1")).unwrap();
        l.executor_status("dsh", &e1, 1, true, 15_000, 0).unwrap().unwrap();
        (H { l, e1, seq: 0 }, p)
    }

    fn up(&mut self, task: &str, kind: Kind, now: u64) -> TaskState {
        self.seq += 1;
        let u = Update { task_id: task.into(), executor_id: "dsh".into(), incarnation: self.e1.clone(), seq: self.seq, pub_epoch: None, request_id: None, kind };
        self.l.update(&u, now).unwrap().unwrap()
    }

    fn to_returned(&mut self, call: &str) -> String {
        let a = self.l.tool_call("delegate", key(call), &args(), 10).unwrap();
        assert_eq!(a.output["status"], "accepted", "{}", a.output);
        let id = a.task_id.unwrap();
        self.l.output_sent(&id, 11).unwrap().unwrap();
        assert_eq!(self.up(&id, Kind::Claim, 12).outcome, Outcome::Applied);
        let child = self.l.get(&id).unwrap().child_id.clone();
        assert_eq!(self.up(&id, Kind::Running { child_id: child, input_sha256: "in".into() }, 13).execution, Exec::Running);
        assert_eq!(self.up(&id, Kind::Returned { result_sha256: "R".into(), chars: 900 }, 20).execution, Exec::Returned);
        id
    }
}

fn owner(h: &H, task: &str, who: &Incarnation, epoch: u64, seq: u64, kind: Kind) -> Update {
    let _ = h;
    Update { task_id: task.into(), executor_id: "dsh".into(), incarnation: who.clone(), seq, pub_epoch: Some(epoch), request_id: None, kind }
}

fn req(task: &str, who: &Incarnation, rid: &str, kind: Kind) -> Update {
    Update { task_id: task.into(), executor_id: "dsh".into(), incarnation: who.clone(), seq: 0, pub_epoch: None, request_id: Some(rid.into()), kind }
}

fn receipt(id: &str, digest: &str) -> Receipt {
    Receipt { task_id: id.into(), key: format!("rth-task-{id}"), seq: 41, result_sha256: digest.into(), session_lineage: "rth-root@2026-10-22T20:30Z".into() }
}

#[test]
fn answer_after_fsync_and_replay_by_call_key() {
    let (mut h, p) = H::new("admit");
    let a = h.l.tool_call("delegate", key("c1"), &args(), 10).unwrap();
    assert_eq!(a.output["status"], "accepted");

    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("\"execution\":\"accepted\""));

    let again = h.l.tool_call("delegate", key("c1"), &args(), 11).unwrap();
    assert_eq!((again.output.clone(), again.task_id.clone()), (a.output.clone(), a.task_id.clone()));

    let other = h.l.tool_call("delegate", key("c1"), &json!({"purpose":"report","task":"outra coisa"}), 12).unwrap();
    assert_eq!(other.output["reason"], "conflict");
    assert_eq!(h.l.tasks.len(), 1);
}

#[test]
fn refusals_say_why_and_create_nothing() {
    let p = tmp("refuse");
    let (mut l, _) = Ledger::open(&p, "inst", "b1").unwrap();

    assert_eq!(l.tool_call("delegate", key("a"), &args(), 0).unwrap().output["reason"], "no_executor");
    l.executor_status("dsh", &inc("L1", "p1"), 1, true, 15_000, 0).unwrap().unwrap();

    assert_eq!(l.tool_call("delegate", key("b"), &json!({"purpose":"report","task":""}), 1).unwrap().output["reason"], "bad_arguments");

    l.set_enabled(false);
    assert_eq!(l.tool_call("delegate", key("c"), &args(), 1).unwrap().output["reason"], "disabled");
    l.set_enabled(true);

    assert_eq!(l.tool_call("delegate", key("d"), &args(), 20_000).unwrap().output["reason"], "no_executor");
    assert!(l.tasks.is_empty());

    l.executor_status("dsh", &inc("L1", "p1"), 2, true, 15_000, 20_000).unwrap().unwrap();
    assert_eq!(l.tool_call("delegate", key("e"), &args(), 20_001).unwrap().output["status"], "accepted");
    assert_eq!(l.tool_call("delegate", key("f"), &args(), 20_002).unwrap().output["reason"], "busy");
}

#[test]
fn rate_limit_counts_accepted_tasks_per_hour() {
    let (mut h, _) = H::new("rate");
    for i in 0..RATE_PER_HOUR {
        let a = h.l.tool_call("delegate", key(&format!("r{i}")), &args(), 100 + i as u64).unwrap();
        let id = a.task_id.unwrap();

        h.l.output_sent(&id, 100).unwrap();
        h.l.executor_status("dsh", &h.e1, 2 + i as u64, true, 10_000_000, 100).unwrap().unwrap();
        h.l.tick(100 + START_MS + 1).unwrap();
        assert_eq!(h.l.get(&id).unwrap().execution, Exec::StartRevoked);
    }
    assert_eq!(h.l.tool_call("delegate", key("over"), &args(), 200_000).unwrap().output["reason"], "budget");
}

#[test]
fn update_idempotency_conflict_and_stale_seq() {
    let (mut h, _) = H::new("upd");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap();
    let id = a.task_id.unwrap();
    h.l.output_sent(&id, 11).unwrap();
    let u = Update { task_id: id.clone(), executor_id: "dsh".into(), incarnation: h.e1.clone(), seq: 1, pub_epoch: None, request_id: None, kind: Kind::Claim };
    let first = h.l.update(&u, 12).unwrap().unwrap();
    assert_eq!(first.outcome, Outcome::Applied);

    let dup = h.l.update(&u, 13).unwrap().unwrap();
    assert_eq!((dup.outcome, dup.rev), (Outcome::Duplicate, first.rev));

    let mut c = u.clone();
    c.kind = Kind::Failed { reason: "x".into() };
    assert_eq!(h.l.update(&c, 13).unwrap().unwrap().reason.as_deref(), Some("conflict"));

    let mut s = u.clone();
    s.seq = 0;
    s.kind = Kind::ExecutionDone;
    assert_eq!(h.l.update(&s, 13).unwrap().unwrap().outcome, Outcome::Refused);
}

#[test]
fn old_executor_and_old_heartbeat_are_refused() {
    let (mut h, _) = H::new("old");
    let id = h.to_returned("c");

    let e2 = inc("L1", "p2");
    h.l.executor_status("dsh", &e2, 1, true, 15_000, 30).unwrap().unwrap();
    assert_eq!(h.l.executor_status("dsh", &h.e1, 99, true, 15_000, 31).unwrap(), Err("stale_executor"), "old heartbeat");

    let u = Update { task_id: id.clone(), executor_id: "dsh".into(), incarnation: e2.clone(), seq: 50, pub_epoch: None, request_id: None, kind: Kind::ExecutionDone };
    assert_eq!(h.l.update(&u, 32).unwrap().unwrap().reason.as_deref(), Some("not_assigned"));

    assert!(h.l.slot_holder().is_some());
}

#[test]
fn restart_rules_revoke_unclaimed_and_keep_claimed_unknown() {
    let (mut h, p) = H::new("boot");
    let a = h.l.tool_call("delegate", key("a"), &args(), 10).unwrap().task_id.unwrap();
    drop(h);
    let (l, rec) = Ledger::open(&p, "inst", "b2").unwrap();

    assert_eq!(l.get(&a).unwrap().execution, Exec::StartRevoked);
    assert_eq!(l.get(&a).unwrap().reason.as_deref(), Some("output_unconfirmed"));
    assert_eq!(rec.revoked, vec![a.clone()]);

    let (mut h, p) = H::new("boot2");
    let id = h.to_returned("c");
    drop(h);
    let (l, rec) = Ledger::open(&p, "inst", "b2").unwrap();

    assert_eq!(l.get(&id).unwrap().execution, Exec::Unknown);
    assert_eq!(rec.unknown, vec![id.clone()]);
    assert!(l.slot_holder().is_some());
}

#[test]
fn recovery_before_new_work_and_late_return_for_unknown() {
    let (mut h, p) = H::new("late");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap().task_id.unwrap();
    h.l.output_sent(&a, 11).unwrap();
    h.up(&a, Kind::Claim, 12);
    let child = h.l.get(&a).unwrap().child_id.clone();
    h.up(&a, Kind::Running { child_id: child, input_sha256: "in".into() }, 13);
    let e1 = h.e1.clone();
    let seq = h.seq;
    drop(h);
    let (mut l, _) = Ledger::open(&p, "inst", "b2").unwrap();
    assert_eq!(l.get(&a).unwrap().execution, Exec::Unknown);

    l.executor_status("dsh", &e1, 10, true, 15_000, 0).unwrap().unwrap();
    assert_eq!(l.tool_call("delegate", key("c2"), &args(), 1).unwrap().output["reason"], "busy");

    let u = Update { task_id: a.clone(), executor_id: "dsh".into(), incarnation: e1, seq: seq + 1, pub_epoch: None, request_id: None, kind: Kind::Returned { result_sha256: "R".into(), chars: 5 } };
    assert_eq!(l.update(&u, 2).unwrap().unwrap().execution, Exec::Returned);
    assert_eq!(l.update(&u, 3).unwrap().unwrap().outcome, Outcome::Duplicate);
}

#[test]
fn torn_tail_is_cut() {
    let (h, p) = H::new("torn");
    drop(h);
    std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"{\"rec\":\"task\",\"task_").unwrap();
    let (_, rec) = Ledger::open(&p, "inst", "b2").unwrap();
    assert!(rec.truncated_bytes > 0);
}

#[test]
fn deadline_cancels_then_teardown_unknown_holds_the_slot() {
    let (mut h, _) = H::new("deadline");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap().task_id.unwrap();
    h.l.output_sent(&a, 11).unwrap();
    h.up(&a, Kind::Claim, 12);
    let child = h.l.get(&a).unwrap().child_id.clone();
    h.up(&a, Kind::Running { child_id: child, input_sha256: "in".into() }, 13);
    h.l.executor_status("dsh", &h.e1, 2, true, 10_000_000, 13).unwrap().unwrap();

    h.l.tick(12 + RUN_MS + 1).unwrap();
    assert_eq!(h.l.get(&a).unwrap().execution, Exec::CancelRequested);

    h.l.tick(12 + RUN_MS + 1 + TEARDOWN_MS + 1).unwrap();
    assert_eq!(h.l.get(&a).unwrap().execution, Exec::TeardownUnknown);

    assert_eq!(h.l.tool_call("delegate", key("c2"), &args(), 200_000).unwrap().output["reason"], "busy");

    assert_eq!(h.up(&a, Kind::Cancelled, 200_001).execution, Exec::Cancelled);
    assert!(h.l.slot_holder().is_none());
}

#[test]
fn late_claim_and_late_result_are_refused() {
    let (mut h, _) = H::new("lateclaim");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap().task_id.unwrap();
    h.l.output_sent(&a, 11).unwrap();

    assert_eq!(h.up(&a, Kind::Claim, 11 + START_MS + 1).reason.as_deref(), Some("claim_not_allowed"));

    let (mut h, _) = H::new("lateres");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap().task_id.unwrap();
    h.l.output_sent(&a, 11).unwrap();
    h.up(&a, Kind::Claim, 12);
    let child = h.l.get(&a).unwrap().child_id.clone();
    h.up(&a, Kind::Running { child_id: child, input_sha256: "in".into() }, 13);
    let c = h.l.tool_call("cancel_task", key("x"), &json!({}), 14).unwrap();
    assert_eq!(c.output["status"], "cancel_requested");
    let r = h.up(&a, Kind::Returned { result_sha256: "R".into(), chars: 1 }, 15);
    assert_eq!((r.outcome, r.reason.as_deref()), (Outcome::Refused, Some("late_after_cancel")));

    assert_eq!(h.up(&a, Kind::PublishRequest { result_sha256: "R".into() }, 16).outcome, Outcome::Refused);
}

#[test]
fn cancel_task_is_exact_or_single_and_never_guesses() {
    let (mut h, _) = H::new("cancel");
    assert_eq!(h.l.tool_call("cancel_task", key("x0"), &json!({}), 1).unwrap().output["reason"], "ambiguous");
    let a = h.l.tool_call("delegate", key("c"), &args(), 10).unwrap().task_id.unwrap();
    assert_eq!(h.l.tool_call("cancel_task", key("x1"), &json!({"task_id": a}), 11).unwrap().output["status"], "cancel_requested");
    assert_eq!(h.l.tool_call("cancel_task", key("x2"), &json!({"task_id":"nope"}), 12).unwrap().output["reason"], "ambiguous");
}

#[test]
fn ui_cancel_intent_is_idempotent_by_request_id() {
    let (mut h, _) = H::new("ui");
    let id = h.to_returned("c");
    let u = req(&id, &h.e1, "ui-1", Kind::CancelIntent { by: "user".into() });
    let r = h.l.update(&u, 30).unwrap().unwrap();
    assert_eq!((r.execution, r.cancel.as_deref()), (Exec::CancelRequested, Some("user")));
    assert_eq!(h.l.update(&u, 31).unwrap().unwrap().outcome, Outcome::Duplicate);
}

#[test]
fn cancel_before_commit_wins_after_commit_reports_committed() {
    let (mut h, _) = H::new("pubcancel");
    let id = h.to_returned("c");

    h.l.tool_call("cancel_task", key("x"), &json!({"task_id": id}), 21).unwrap();
    assert_eq!(h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 22).reason.as_deref(), Some("publication_not_allowed"));

    let (mut h, _) = H::new("pubcancel2");
    let id = h.to_returned("c");
    assert_eq!(h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21).publication, Publication::Committed);

    let c = h.l.tool_call("cancel_task", key("x"), &json!({"task_id": id}), 22).unwrap();
    assert_eq!(c.output["status"], "publication_committed");
    let t = h.l.get(&id).unwrap();
    assert!(t.after_commit && t.publication == Publication::Committed);
}

#[test]
fn only_the_owner_delivers_and_the_receipt_is_validated() {
    let (mut h, _) = H::new("ana");
    let id = h.to_returned("c");
    h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21);
    let e1 = h.e1.clone();

    for (i, bad) in [
        Receipt { task_id: "other".into(), ..receipt(&id, "R") },
        Receipt { key: "rth-task-x".into(), ..receipt(&id, "R") },
        Receipt { result_sha256: "Z".into(), ..receipt(&id, "R") },
        Receipt { session_lineage: String::new(), ..receipt(&id, "R") },
    ]
    .into_iter()
    .enumerate()
    {
        let seq = i as u64 + 1;
        let r = h.l.update(&owner(&h, &id, &e1, 1, seq, Kind::Delivered { chat_ref: json!({}), receipt: bad }), 22).unwrap().unwrap();
        assert_eq!(r.outcome, Outcome::Refused, "{:?}", r.reason);
        assert_eq!(h.l.get(&id).unwrap().publication, Publication::Committed);
    }

    let e9 = inc("L9", "p9");
    let r = h.l.update(&owner(&h, &id, &e9, 1, 2000, Kind::DeliveryFailed { reason: "absent".into() }), 23).unwrap().unwrap();
    assert_eq!(r.reason.as_deref(), Some("not_publication_owner"));

    let ok = h.l.update(&owner(&h, &id, &e1, 1, 3000, Kind::Delivered { chat_ref: json!({"seq":41}), receipt: receipt(&id, "R") }), 24).unwrap().unwrap();
    assert_eq!(ok.publication, Publication::Delivered);

    h.up(&id, Kind::ExecutionDone, 25);
    assert!(h.l.slot_holder().is_none());
}

#[test]
fn visible_observed_is_recorded_only() {
    let (mut h, _) = H::new("observe");
    let id = h.to_returned("c");
    h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21);
    let e2 = inc("L1", "p2");
    h.l.executor_status("dsh", &e2, 1, true, 15_000, 22).unwrap().unwrap();

    let u = req(&id, &e2, "obs-1", Kind::VisibleObserved { seq: 41, result_sha256: "R".into() });
    let r = h.l.update(&u, 23).unwrap().unwrap();
    assert_eq!((r.outcome, r.publication), (Outcome::Applied, Publication::Committed));

    assert_eq!(h.l.update(&u, 24).unwrap().unwrap().outcome, Outcome::Duplicate);
    let e1 = h.e1.clone();
    let d = h.l.update(&owner(&h, &id, &e1, 1, 1, Kind::Delivered { chat_ref: json!({}), receipt: receipt(&id, "R") }), 25).unwrap().unwrap();
    assert_eq!(d.publication, Publication::Delivered, "the owner's pub_seq 1 is free");
}

#[test]
fn takeover_needs_the_owners_exit_receipt_and_the_expected_epoch() {
    let (mut h, _) = H::new("takeover");
    let id = h.to_returned("c");
    h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21);
    let e1 = h.e1.clone();

    let same = inc("L1", "p2");
    h.l.executor_status("dsh", &same, 1, true, 15_000, 22).unwrap().unwrap();
    let t = req(&id, &same, "tk-0", Kind::PublicationTakeover { expected_owner: e1.clone(), expected_epoch: 1, result_sha256: "R".into() });
    assert_eq!(h.l.update(&t, 23).unwrap().unwrap().reason.as_deref(), Some("takeover_not_allowed"));

    h.l.register_launch("L2", &sha_hex(b"secret-2")).unwrap();
    let e2 = inc("L2", "p1");
    h.l.executor_status("dsh", &e2, 1, true, 15_000, 24).unwrap().unwrap();
    let early = req(&id, &e2, "tk-early", Kind::PublicationTakeover { expected_owner: e1.clone(), expected_epoch: 1, result_sha256: "R".into() });
    assert_eq!(h.l.update(&early, 25).unwrap().unwrap().reason.as_deref(), Some("takeover_not_allowed"), "no takeover without the exit receipt");

    assert_eq!(h.l.exit_receipt("L1", "wrong", None, None).unwrap(), Err(Outcome::Refused));

    let recs = h.l.exit_receipt("L1", "secret-1", Some(0), None).unwrap().unwrap();
    assert!(!recs.is_empty());
    let s = h.l.get(&id).unwrap();
    assert_eq!((s.execution, s.publication), (Exec::SettledExited, Publication::Unknown));
    assert!(h.l.slot_holder().is_none(), "the worker slot is free; publication is reconciled on its own");

    assert_eq!(h.l.exit_receipt("L1", "secret-1", Some(0), None).unwrap(), Err(Outcome::Duplicate));

    let wrong = req(&id, &e2, "tk-1", Kind::PublicationTakeover { expected_owner: e1.clone(), expected_epoch: 2, result_sha256: "R".into() });
    assert_eq!(h.l.update(&wrong, 31).unwrap().unwrap().outcome, Outcome::Refused);
    let ok = req(&id, &e2, "tk-2", Kind::PublicationTakeover { expected_owner: e1.clone(), expected_epoch: 1, result_sha256: "R".into() });
    let r = h.l.update(&ok, 32).unwrap().unwrap();
    assert_eq!((r.publication, r.pub_epoch, r.pub_owner.clone()), (Publication::Committed, 2, Some(e2.clone())));

    let e3 = inc("L3", "p1");
    h.l.register_launch("L3", &sha_hex(b"s3")).unwrap();
    h.l.executor_status("dsh", &e3, 1, true, 15_000, 33).unwrap().unwrap();
    let second = req(&id, &e3, "tk-3", Kind::PublicationTakeover { expected_owner: e1.clone(), expected_epoch: 1, result_sha256: "R".into() });
    assert_eq!(h.l.update(&second, 34).unwrap().unwrap().outcome, Outcome::Refused);

    let late = owner(&h, &id, &e1, 1, 9, Kind::Delivered { chat_ref: json!({}), receipt: receipt(&id, "R") });
    assert_eq!(h.l.update(&late, 35).unwrap().unwrap().reason.as_deref(), Some("not_publication_owner"));

    let v = h.l.update(&owner(&h, &id, &e2, 2, 1, Kind::VisibleUnconfirmed { seq: 41 }), 36).unwrap().unwrap();
    assert_eq!(v.publication, Publication::VisibleUnconfirmed);
}

#[test]
fn abandonment_fences_before_commit_and_only_suppresses_after() {
    let (mut h, _) = H::new("abandon");
    let id = h.to_returned("c");
    let e1 = h.e1.clone();
    let r = h.l.update(&req(&id, &e1, "ab-1", Kind::Abandon), 21).unwrap().unwrap();
    assert!(r.abandoned);

    assert_eq!(h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 22).reason.as_deref(), Some("abandoned"));
    assert_eq!(h.up(&id, Kind::Cancelled, 23).reason.as_deref(), Some("abandoned"));
    assert!(h.l.slot_holder().is_some());

    h.l.exit_receipt("L1", "secret-1", Some(0), None).unwrap().unwrap();
    assert!(h.l.slot_holder().is_none());

    let (mut h, _) = H::new("abandon2");
    let id = h.to_returned("c");
    let e1 = h.e1.clone();
    h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21);
    let r = h.l.update(&req(&id, &e1, "ab-1", Kind::Abandon), 22).unwrap().unwrap();
    assert!(r.after_commit && !r.abandoned);
    let d = h.l.update(&owner(&h, &id, &e1, 1, 1, Kind::Delivered { chat_ref: json!({}), receipt: receipt(&id, "R") }), 23).unwrap().unwrap();
    assert_eq!(d.publication, Publication::Delivered);
    assert!(d.after_commit, "notice suppression is kept");
}

#[test]
fn launch_registry_is_register_once_and_survives_restart() {
    let (mut h, p) = H::new("registry");

    assert_eq!(h.l.register_launch("L1", &sha_hex(b"other")).unwrap(), Outcome::Refused);
    assert_eq!(h.l.register_launch("L1", &sha_hex(b"secret-1")).unwrap(), Outcome::Duplicate);
    drop(h);

    let (mut l, _) = Ledger::open(&p, "inst", "b2").unwrap();
    assert_eq!(l.exit_receipt("L1", "secret-1", None, Some("SIGKILL".into())).unwrap(), Ok(vec![]));

    assert!(!std::fs::read_to_string(&p).unwrap().contains("secret-1"));
}

#[test]
fn a_failed_write_poisons_the_ledger() {
    let (mut h, _) = H::new("poison");
    h.l.poisoned = Some("injected".into());
    assert!(h.l.tool_call("delegate", key("c"), &args(), 10).is_err(), "no answer without a durable record");
}

#[test]
fn a_host_verified_exit_settles_the_same_way_and_replays_as_duplicate() {
    let (mut h, _) = H::new("verified");
    let id = h.to_returned("c");
    h.up(&id, Kind::PublishRequest { result_sha256: "R".into() }, 21);
    let recs = h.l.exit_verified("L1", Some(0), None).unwrap().unwrap();
    assert_eq!(recs.len(), 1);
    let s = h.l.get(&id).unwrap();
    assert_eq!((s.execution, s.publication), (Exec::SettledExited, Publication::Unknown));
    assert_eq!(h.l.exit_verified("L1", Some(0), None).unwrap(), Err(Outcome::Duplicate));
    assert_eq!(h.l.exit_verified("", None, None).unwrap(), Err(Outcome::Refused));
}

#[test]
fn inventory_lists_every_unresolved_task_plus_the_most_recent_others_oldest_first_and_survives_reopen() {
    let (mut h, p) = H::new("inventory");

    let t1 = h.to_returned("c1");
    assert_eq!(h.up(&t1, Kind::ExecutionDone, 21).execution, Exec::ExecutionDone);

    let t2 = h.to_returned("c2");
    h.up(&t2, Kind::ExecutionDone, 22);
    assert_eq!(h.up(&t2, Kind::PublishRequest { result_sha256: "R".into() }, 23).publication, Publication::Committed);

    let t3 = h.to_returned("c3");
    let ids = |v: Vec<TaskState>| v.into_iter().map(|t| t.task_id).collect::<Vec<_>>();
    assert_eq!(ids(h.l.inventory(0)), vec![t2.clone(), t3.clone()], "every unresolved task, with no recent budget");
    assert_eq!(ids(h.l.inventory(1)), vec![t1.clone(), t2.clone(), t3.clone()], "plus the most recent resolved ones");
    assert!(h.l.inventory(0).iter().all(|t| t.outcome == Outcome::Applied && t.rev == h.l.get(&t.task_id).unwrap().rev), "the current committed records");
    drop(h);
    let (l, _) = Ledger::open(&p, "inst", "b2").unwrap();
    assert_eq!(ids(l.inventory(5)), vec![t1, t2, t3], "rebuilt from the log on reopen");
}
