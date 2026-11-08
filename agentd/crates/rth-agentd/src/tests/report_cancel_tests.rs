

use super::*;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_during_the_turns_own_report_sends_one_provenance_and_one_terminal() {
    use crate::flash::test_server::{start, Answer};
    const MARK: &str = "report-cancel-report-mark";
    let parts = ["The answer is 4.", " The report-cancel-report-mark ends it."];
    let (hold, mut seen, go) = local::race::hold_report(MARK);
    let events = parts.iter().map(|t| json!({"model": "deepseek-flash", "choices": [{"delta": {"content": t}}]}).to_string()).collect();
    let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("report-cancel-report");
    let (rig, _f) = reasoning_limits_rig(&dir, "report1", &s.url, false).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "what is 2 + 2?")).await;

    assert_eq!(seen.recv().await, Some("report held"));
    let rid = hold.rid.lock().unwrap().clone().unwrap();
    c.send(Event::Cancel { response_id: rid.clone() }).await;

    assert_eq!(seen.recv().await, Some("abort requested"));
    go.send(()).unwrap();

    let mut left = vec!["turn took the slot", "cancel finished"];
    while !left.is_empty() {
        let m = seen.recv().await.expect("hold points");
        left.retain(|x| *x != m);
    }
    assert_eq!(hold.sent.load(std::sync::atomic::Ordering::SeqCst), 1, "agentd sent the turn's provenance once");
    let recs = turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(&e.event, Event::AgentDone { response_id, .. } if *response_id == rid))).await;
    let provenance = recs.iter().filter(|e| matches!(&e.event, Event::ResponseProvenance { response_id, .. } if *response_id == rid)).count();
    let terminals = recs.iter().filter(|e| matches!(&e.event, Event::AgentDone { response_id, .. } if *response_id == rid)).count();
    assert_eq!((provenance, terminals), (1, 1), "one response_provenance and one terminal in the journal");
}
