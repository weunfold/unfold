

use super::*;
use serde_json::json;

fn shown(recs: &[Envelope]) -> String {
    let mut s = String::new();
    for e in recs {
        match &e.event {
            Event::AgentDelta { text, .. } => s.push_str(text),
            Event::AgentReplace { text, .. } => s = text.clone(),
            _ => {}
        }
    }
    s
}

fn untouched(recs: &[Envelope], parts: &[&str], case: &str) {
    assert!(!recs.iter().any(|e| matches!(e.event, Event::AgentReplace { .. })), "{case}: no agent_replace");
    assert!(!recs.iter().any(|e| matches!(&e.event, Event::Log { msg, .. } if msg.contains("visible reasoning"))), "{case}: nothing logged as dropped reasoning");
    assert_eq!(shown(recs), parts.concat(), "{case}: the whole reply, as it came");
}

fn events(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|t| json!({"model": "deepseek-flash", "choices": [{"delta": {"content": t}}]}).to_string()).collect()
}

#[tokio::test]
async fn a_cancel_never_turns_a_queued_flash_piece_into_local_text() {
    use crate::flash::test_server::{start, Answer};
    const MARK: &str = "cancel-race-mark";

    let parts = ["Real answer here.", " cancel-race-mark names the tag </think> in this answer."];
    let (hold, mut seen) = local::race::hold(MARK);
    let s = start(Answer::Stream { events: events(&parts), gap: Duration::ZERO, hang: true }).await;
    let dir = command_dir("cancel-race");
    let (rig, _f) = reasoning_limits_rig(&dir, "race1", &s.url, false).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "how do I strip the think block from a reply?")).await;

    assert_eq!(seen.recv().await, Some("piece held"));
    let rid = hold.rid.lock().unwrap().clone().unwrap();

    turn_lifecycle_until(&rig, |r| shown(r).starts_with("Real answer")).await;
    assert!(!shown(&journal_records(&rig.journal, |_| true).await).contains(MARK), "the marked piece is still held");
    c.send(Event::Cancel { response_id: rid.clone() }).await;

    assert_eq!(seen.recv().await, Some("cancel read the provenance"));
    hold.piece.add_permits(1);

    turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentReplace { .. })) || shown(r).contains(MARK)).await;
    hold.cancel.add_permits(1);
    let recs = turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(&e.event, Event::AgentDone { response_id, .. } if *response_id == rid))).await;
    assert!(recs.iter().any(|e| matches!(&e.event, Event::AgentDone { status: ResponseStatus::Cancelled, .. })), "cancelled");

    assert!(!recs.iter().any(|e| matches!(e.event, Event::AgentReplace { .. })), "no agent_replace");
    assert!(!recs.iter().any(|e| matches!(&e.event, Event::Log { msg, .. } if msg.contains("visible reasoning"))), "nothing logged as dropped reasoning");
    let text = shown(&recs);
    assert!(parts.concat().starts_with(&text) && text.contains("</think>"), "{text}");
}

#[tokio::test]
async fn a_routed_dsh_only_model_reply_streams_unchanged() {
    use crate::flash::test_server::{start_keyed, Answer};
    use crate::local::fake_server::{start as fake_llama, Mode};
    let parts = ["Use a regex on <think>.*?</think> to drop it. ", "Then keep the rest."];
    let s = start_keyed(Answer::Stream { events: events(&parts), gap: Duration::ZERO, hang: false }, DSH_KEY).await;
    let f = fake_llama(Mode::Mixed { pieces: vec!["Okay".into()], answer: r#"{"d":"no_op"}"#.into(), warm_delay: Duration::ZERO }, Duration::ZERO).await;
    let (authority, base_path) = local::parse_loopback_url(&f.url).unwrap();
    let granite = local::Config { authority, base_path, model: "granite42-3b-q4km".into(), model_sha256: Some("cd".repeat(32)), slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(64), n_ctx: 4096, first_token: Duration::from_secs(5), system_instruction: None, health_every: Duration::from_secs(60) };
    let dsh = crate::dsh::config_with_model(DSH_KEY, &s.url, "cancel/dsh-only-model", None).unwrap();
    assert!(crate::dsh::is_model("cancel/dsh-only-model") && !crate::flash::is_model("cancel/dsh-only-model"));
    let opts = StartOpts { memory_egress: Some("allow:dsh".into()), ..turn_lifecycle_opts(&granite) };
    let rig = state_rig(&command_dir("cancel-dsh-only"), "d1", local::connect_router(crate::dsh::PROVIDER, dsh, granite, true, None), opts).await;
    let recs = reasoning_limits_one_turn(&rig, "how do I drop the think block?", None).await;
    untouched(&recs, &parts, "routed DSH-only model");
}

#[tokio::test]
async fn direct_hosted_and_keyed_replies_stream_unchanged() {
    use crate::flash::test_server::{start, start_keyed, Answer};
    let parts = ["Strip <think>.*?</think> with a regex. ", "Nothing else changes."];
    let s = start(Answer::Stream { events: events(&parts), gap: Duration::ZERO, hang: false }).await;
    let hosted = crate::flash::config(RC5_KEY, &s.url, Some(crate::flash::test_server::ca())).unwrap();
    assert!(matches!(local::dial(&hosted.authority), local::Dial::Hosted(_)));
    let k = start_keyed(Answer::Stream { events: events(&parts), gap: Duration::ZERO, hang: false }, DSH_KEY).await;
    let keyed = crate::dsh::config_with_model(DSH_KEY, &k.url, "cancel/direct-keyed", None).unwrap();
    assert!(matches!(local::dial(&keyed.authority), local::Dial::Keyed(_)));
    for (case, cfg) in [("direct Hosted", hosted), ("direct Keyed", keyed)] {
        let rig = state_rig(&command_dir(&format!("cancel-{}", case.replace(' ', "-"))), "h1", local::connect(cfg), ctx_opts()).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        assert_eq!(ctx_turn(&rig, &mut c, "c0", "how do I strip the think block?", 0).await, "Completed", "{case}");
        untouched(&journal_records(&rig.journal, |_| true).await, &parts, case);
    }
}
