

use super::*;
use crate::flash_spend::{PRICE_OUT, STOP_PUSD};
use serde_json::{json, Value};

const KEY: &str = "long-reply-test-key-not-a-secret-0123456789";

fn content(text: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {"content": text}}]}).to_string()
}

fn finish(reason: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {}, "finish_reason": reason}]}).to_string()
}

fn ledger_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rth-agentd-flash-spend-{name}-{}", std::process::id()))
}

fn row(root: &std::path::Path, name: &str) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(root.join("2026-10").join(name)).ok()?).ok()
}

async fn settled(root: &std::path::Path, n: u64) -> Option<Value> {
    let end = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(v) = row(root, &format!("call-{n:06}.settle.json")) {
            return Some(v);
        }
        if tokio::time::Instant::now() > end {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn no_frame_from_a_long_reply_passes_max_frame() {
    use crate::flash::test_server::{start, Answer};
    let cases = [("md", "write the whole guide", "**Step {i}**: run `cmd{i}` and check it.\n"), ("held", "what did we talk about last time we chatted?", "I don't remember that chat, but here is line {i} of a long answer.\n")];
    for (name, ask, line) in cases {
        let mut events: Vec<String> = (0..6000).map(|i| content(&line.replace("{i}", &i.to_string()))).collect();
        if name == "held" {
            events.insert(0, content("I don't remember that chat. "));
        }
        let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: false }).await;
        let dir = command_dir(&format!("long-reply-frame-{name}"));
        let (rig, _f) = reasoning_limits_rig(&dir, "f1", &s.url, false).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        c.send(submit("c1", 0, ask)).await;
        let recs = turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await;
        assert!(reply_of(&recs).len() > 150_000, "{name}: the whole reply arrived");

        let mut done = false;
        let end = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < end {
            match tokio::time::timeout(Duration::from_millis(300), read_frame(&mut c.reader, &mut c.buf)).await {
                Ok(Ok(line)) => {
                    let e = Envelope::parse_from(&line, Source::Guest).unwrap();
                    if matches!(e.event, Event::ResponseDiagnostic { .. }) || (done && matches!(e.event, Event::Heartbeat { .. })) {
                        break;
                    }
                    done |= matches!(e.event, Event::AgentDone { .. });
                }
                Ok(Err(e)) => panic!("{name}: a frame the reader refuses: {e}"),
                Err(_) if done => break,
                Err(_) => {}
            }
        }
        let journal = std::fs::read_to_string(&rig.journal).unwrap();
        let longest = journal.lines().map(str::len).max().unwrap();
        assert!(longest < rth_protocol::MAX_FRAME, "{name}: a journal line of {longest} bytes");
    }
}

#[tokio::test]
async fn continue_sees_where_the_long_reply_stopped() {
    use crate::flash::test_server::{start, Answer};
    let mut events: Vec<String> = (0..400).map(|i| content(&format!("Sentence number {i} is here. "))).collect();
    events.push(finish("length"));
    let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("long-reply-continue");
    let (rig, _f) = reasoning_limits_rig(&dir, "k1", &s.url, false).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "count for me")).await;
    turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() == 1).await;
    c.send(submit("c2", 1, "continue")).await;
    turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() == 2).await;
    let second = flash_bodies(&s)[1]["messages"].to_string();
    assert!(second.contains("Sentence number 0 is here."), "the head stays");
    assert!(second.contains("Sentence number 398 is here."), "the end of the reply is in the next request");
    assert!(second.contains("\\\"continue\\\""), "and the notice with it");
}

#[tokio::test]
async fn near_the_stop_a_chat_call_reserves_its_worst_case() {
    use crate::flash::test_server::{start, Answer};
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("long-reply-near-stop");
    let ledger = routing_ledger("long-reply-near");
    let root = ledger_root("long-reply-near");
    std::fs::create_dir_all(root.join("2026-10")).unwrap();
    let committed = STOP_PUSD - 400_000_000_000;
    std::fs::write(root.join("2026-10/call-000000.json"), json!({"n": 0, "month": "2026-10", "attempt_id": "p-0", "reserve_pusd": committed, "purpose": "chat", "day": "2026-10-26"}).to_string()).unwrap();
    let (rig, _f) = router_rig_spend(&dir, "n1", &s.url, crate::flash::FIRST_TOKEN, "allow:flash", Some(ledger)).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "hello")).await;
    turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await;
    let reserve = row(&root, "call-000001.json").expect("the call was reserved")["reserve_pusd"].as_u64().unwrap();
    let sent = flash_bodies(&s)[0]["max_tokens"].as_u64().unwrap();
    assert!(reserve >= sent * PRICE_OUT, "the worst case: {reserve} for {sent} tokens");
}

#[tokio::test]
async fn a_cancelled_reply_settles_at_what_it_streamed() {
    use crate::flash::test_server::{start, Answer};
    let events: Vec<String> = (0..20000).map(|i| content(&format!("word{i} "))).collect();
    let s = start(Answer::Stream { events, gap: Duration::from_millis(1), hang: true }).await;
    let dir = command_dir("long-reply-cancel");
    let ledger = routing_ledger("long-reply-cancel");
    let root = ledger_root("long-reply-cancel");
    let (rig, _f) = router_rig_spend(&dir, "x1", &s.url, crate::flash::FIRST_TOKEN, "allow:flash", Some(ledger)).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "go on and on")).await;
    let (mut seen, mut rid) = (0usize, String::new());
    while seen < 8000 {
        if let Event::AgentDelta { response_id, text } = next_matching(&mut c, |e| matches!(e, Event::AgentDelta { .. })).await.event {
            seen += text.len();
            rid = response_id;
        }
    }
    c.send(Event::Cancel { response_id: rid }).await;
    let reserve = row(&root, "call-000000.json").unwrap()["reserve_pusd"].as_u64().unwrap();
    let settle = settled(&root, 0).await.expect("a cancelled call is settled");
    let cost = settle["cost_pusd"].as_u64().unwrap();
    assert!(cost >= seen as u64 * PRICE_OUT && cost > reserve, "{cost} for {seen} bytes streamed (estimate {reserve})");
}

#[tokio::test]
async fn a_stalled_reply_settles_at_what_it_streamed() {
    use crate::flash::test_server::{start, Answer};
    let events: Vec<String> = (0..3000).map(|i| content(&format!("word{i} "))).collect();
    let streamed: usize = (0..3000).map(|i| format!("word{i} ").len()).sum();
    let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: true }).await;
    let dir = command_dir("long-reply-stall");
    let ledger = routing_ledger("long-reply-stall");
    let root = ledger_root("long-reply-stall");
    let (rig, _f) = router_rig_spend(&dir, "y1", &s.url, crate::flash::FIRST_TOKEN, "allow:flash", Some(ledger)).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "go on and on")).await;
    let settle = settled(&root, 0).await.expect("the failed call is settled");
    let cost = settle["cost_pusd"].as_u64().unwrap();
    assert!(cost >= streamed as u64 * PRICE_OUT, "{cost} for {streamed} bytes streamed");
}

#[tokio::test]
async fn a_local_slot_with_no_room_fails_as_empty_at_cap() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let f = fake_llama(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let bridge = crate::flash::test_server::start_keyed(crate::flash::test_server::Answer::Stream { events: vec![], gap: Duration::ZERO, hang: false }, KEY).await;
    let base = crate::dsh::config_with_model(KEY, &bridge.url, "long-reply/base", None).unwrap();
    let (authority, base_path) = local::parse_loopback_url(&f.url).unwrap();
    let cfg = local::Config { authority, base_path, model: "granite41-3b-q4km".into(), n_ctx: 4096, first_token: Duration::from_secs(5), ..base };
    let msgs = vec![json!({"role": "system", "content": "You are Unfold. ".repeat(900)}), json!({"role": "user", "content": "hi"})];
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let r = local::stream_chat_metered(&cfg, msgs, &tx, None).await;
    drop(tx);
    let mut got = String::new();
    while let Some(t) = rx.recv().await {
        got += &t;
    }
    assert_eq!(r.as_ref().err().map(|f| f.reason), Some(local::REPLY_EMPTY_AT_CAP), "{r:?}: {got:?}");
    assert!(got.is_empty() && f.requests.lock().unwrap().iter().all(|r| r["stream"] != true), "nothing sent, nothing shown: {got:?}");
}
