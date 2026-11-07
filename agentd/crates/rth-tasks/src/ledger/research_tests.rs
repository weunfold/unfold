

use super::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rth-tasks-research-{name}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("tasks.jsonl")
}

fn key(call: &str) -> CallKey {
    CallKey { instance_id: "inst".into(), provider_generation: 0, call_id: call.into() }
}

fn open(path: &Path, boot: &str) -> Ledger {
    let (mut l, _) = Ledger::open(path, "inst", boot).unwrap();
    l.set_internal(true);
    l
}

fn running(l: &mut Ledger, call: &str, now: u64) -> String {
    let a = l.tool_call("research", key(call), &json!({"task": "which port does billing use?"}), now).unwrap();
    assert_eq!(a.output["status"], "accepted", "{}", a.output);
    let id = a.task_id.unwrap();
    l.output_sent(&id, now).unwrap().unwrap();
    let me = l.internal();
    let up = |seq, kind| Update { task_id: id.clone(), executor_id: me.executor_id.clone(), incarnation: me.incarnation.clone(), seq, pub_epoch: None, request_id: None, kind };
    assert_eq!(l.update(&up(1, Kind::Claim), now).unwrap().unwrap().execution, Exec::Claimed);
    let child = l.get(&id).unwrap().child_id.clone();
    assert_eq!(l.update(&up(2, Kind::Running { child_id: child, input_sha256: "in".into() }), now).unwrap().unwrap().execution, Exec::Running);
    id
}

fn receipt(id: &str, seq: u64) -> Value {
    json!({"task_id": id, "call_seq": seq, "tool": "files.read", "outcome": "ok"})
}

#[test]
fn research_is_accepted_without_an_executor_lease_and_assigned_to_agentd() {
    let p = tmp("accept");
    let (mut off, _) = Ledger::open(&p, "inst", "b1").unwrap();
    assert_eq!(off.tool_call("research", key("c0"), &json!({"task": "x"}), 0).unwrap().output["reason"], "disabled", "off until agentd's worker is on");
    drop(off);
    let mut l = open(&p, "b1");
    let a = l.tool_call("research", key("c1"), &json!({"task": "  find the port  ", "folder": "/x/docs"}), 0).unwrap();
    assert_eq!(a.output["status"], "accepted");
    assert!(a.output["say"].as_str().unwrap().contains("answer will appear"), "{}", a.output);
    let rec = a.record.unwrap();
    assert_eq!(rec.assignment, l.internal());
    assert_eq!(rec.assignment.executor_id, INTERNAL_EXECUTOR);
    assert_eq!(rec.args, json!({"purpose": "research", "task": "find the port", "folder": "/x/docs"}));
    let bad = l.tool_call("research", key("c2"), &json!({"task": "x", "send_to": "team"}), 0).unwrap();
    assert_eq!(bad.output["reason"], "bad_arguments");
    let busy = l.tool_call("research", key("c3"), &json!({"task": "another"}), 0).unwrap();
    assert_eq!(busy.output["reason"], "busy", "one slot, shared with report tasks");
    assert_eq!(l.tool_call("delegate", key("c4"), &json!({"purpose": "report", "task": "x"}), 0).unwrap().output["reason"], "no_executor", "report tasks still need the DSH executor's lease");
}

#[test]
fn a_research_task_gets_the_longer_run_grant() {
    let mut l = open(&tmp("deadline"), "b1");
    let id = running(&mut l, "c1", 1_000);
    assert!(l.tick(1_000 + RUN_MS + 1).unwrap().is_empty(), "past the report grant, still running");
    assert_eq!(l.get(&id).unwrap().execution, Exec::Running);
    let recs = l.tick(1_000 + RESEARCH_RUN_MS + 1).unwrap();
    assert_eq!((recs.len(), recs[0].execution, recs[0].cancel.as_deref()), (1, Exec::CancelRequested, Some("deadline")));
}

#[test]
fn tool_call_receipts_are_ordered_per_task_and_survive_a_restart() {
    let p = tmp("receipts");
    let mut l = open(&p, "b1");
    assert_eq!(l.tool_call_receipt("b1-t9", &receipt("b1-t9", 1)).unwrap(), Err("unknown_task"));
    let a = l.tool_call("research", key("c1"), &json!({"task": "x"}), 0).unwrap();
    let early = a.task_id.unwrap();
    assert_eq!(l.tool_call_receipt(&early, &receipt(&early, 1)).unwrap(), Err("not_running"), "no calls before the worker runs");
    drop(l);
    let p = tmp("receipts2");
    let mut l = open(&p, "b1");
    let id = running(&mut l, "c1", 0);
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 1)).unwrap(), Ok(()));
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 2)).unwrap(), Ok(()));
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 2)).unwrap(), Err("stale_call_seq"));
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 4)).unwrap(), Err("call_seq_gap"));
    assert_eq!(l.tool_call_receipt(&id, &receipt("other", 3)).unwrap(), Err("task_mismatch"));

    let me = l.internal();
    l.update(&Update { task_id: id.clone(), executor_id: me.executor_id.clone(), incarnation: me.incarnation.clone(), seq: 0, pub_epoch: None, request_id: Some("u1".into()), kind: Kind::CancelIntent { by: "user".into() } }, 0).unwrap().unwrap();
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 3)).unwrap(), Ok(()));
    l.update(&Update { task_id: id.clone(), executor_id: me.executor_id.clone(), incarnation: me.incarnation.clone(), seq: 3, pub_epoch: None, request_id: None, kind: Kind::Cancelled }, 0).unwrap().unwrap();
    assert_eq!(l.get(&id).unwrap().execution, Exec::Cancelled);
    assert_eq!(l.tool_call_receipt(&id, &receipt(&id, 4)).unwrap(), Err("not_running"));
    drop(l);
    let l = open(&p, "b2");
    let got: Vec<u64> = l.receipts(&id).iter().map(|r| r["call_seq"].as_u64().unwrap()).collect();
    assert_eq!(got, [1, 2, 3], "folded back at boot");
    let lines = std::fs::read_to_string(&p).unwrap();
    assert_eq!(lines.lines().filter(|x| x.contains("\"rec\":\"tool_call\"")).count(), 3);
}

#[test]
fn a_restart_settles_agentds_own_tasks_and_frees_the_slot() {
    let p = tmp("restart");
    let id = {
        let mut l = open(&p, "b1");
        running(&mut l, "c1", 0)
    };
    let (mut l, rec) = Ledger::open(&p, "inst", "b2").unwrap();
    l.set_internal(true);
    let t = l.get(&id).unwrap();
    assert_eq!((t.execution, t.reason.as_deref()), (Exec::SettledExited, Some("agentd_restarted")), "the worker died with the old agentd");
    assert_eq!(rec.interrupted, vec![id.clone()]);
    assert!(rec.unknown.is_empty());
    assert!(l.slot_holder().is_none());
    assert_eq!(l.tool_call("research", key("c2"), &json!({"task": "again"}), 0).unwrap().output["status"], "accepted");
    assert_ne!(l.internal().incarnation, t_incarnation(&p, &id), "a new boot is a new incarnation");
}

fn t_incarnation(p: &Path, id: &str) -> Incarnation {
    let text = std::fs::read_to_string(p).unwrap();
    let first = text.lines().find(|l| l.contains(id) && l.contains("\"rec\":\"task\"")).unwrap();
    let v: Value = serde_json::from_str(first).unwrap();
    serde_json::from_value(v["assignment"]["incarnation"].clone()).unwrap()
}

#[test]
fn research_arguments_are_strict() {
    for bad in [json!({}), json!({"task": ""}), json!({"task": "a".repeat(501)}), json!({"task": "x", "folder": 3}), json!({"task": "x", "folder": "relative/path"}), json!([])] {
        assert!(schema::validate_research(&bad).is_err(), "{bad}");
    }
    let ok = schema::validate_research(&json!({"task": "x", "folder": "~/docs"})).unwrap();
    assert_eq!(ok.folder.as_deref(), Some("~/docs"));
}

#[test]
fn the_research_tool_is_declared_and_described_only_when_given() {
    use schema::Tool;
    let o = schema::render_openai(&[Tool::Research]);
    assert_eq!((o[0]["name"].as_str(), o[0]["parameters"]["required"].clone()), (Some("research"), json!(["task"])));
    assert!(o[0]["description"].as_str().unwrap().contains("It changes nothing"));
    let only = schema::capability_text(&[Tool::Research]).unwrap();
    assert!(only.contains("with research") && !only.contains("delegate"), "{only}");
    let both = schema::capability_text(&[Tool::Delegate, Tool::CancelTask, Tool::Research]).unwrap();
    assert!(both.starts_with(&schema::capability_text(&[Tool::Delegate, Tool::CancelTask]).unwrap()), "the report text is unchanged, research is appended");
    assert!(!schema::capability_text(&[Tool::Delegate]).unwrap().contains("research"));
}

#[test]
fn pv05_g9_a_task_keeps_its_origin_speaker_across_a_restart() {
    let p = tmp("speaker");
    let mut l = open(&p, "b1");
    let a = l.tool_call_by("research", key("c1"), &json!({"task": "x"}), 0, Some("person:p-lia")).unwrap();
    let id = a.task_id.unwrap();
    assert_eq!(a.record.unwrap().origin_speaker.as_deref(), Some("person:p-lia"));
    drop(l);
    let l = open(&p, "b2");
    let t = l.get(&id).unwrap().clone();
    assert_eq!(t.origin_speaker.as_deref(), Some("person:p-lia"), "the speaker survives a restart");
    let mut before = serde_json::to_value(&t).unwrap();
    before.as_object_mut().unwrap().remove("origin_speaker");
    assert_eq!(serde_json::from_value::<TaskState>(before).unwrap().origin_speaker, None, "a pre-0.5 line reads as the owner's");
    let mut l = open(&tmp("speaker-none"), "b1");
    let rec = l.tool_call("research", key("c1"), &json!({"task": "x"}), 0).unwrap().record.unwrap();
    assert_eq!(rec.origin_speaker, None);
    assert!(serde_json::to_value(&rec).unwrap().get("origin_speaker").is_none(), "no speaker, no field: the line is 0.4's");
}
