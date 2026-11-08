

use super::*;
use serde_json::{json, Value};

const KEY: &str = "reply-limit-test-key-not-a-secret-0123456789";
const FLASH_LIMIT: u64 = 256_000;

fn body_of(raw: &str) -> Value {
    serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

fn content(text: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {"content": text}}]}).to_string()
}

fn finish(reason: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {}, "finish_reason": reason}]}).to_string()
}

async fn send_with(c: &mut Client, id: &str, text: &str, extra: &[(&str, Value)]) {
    let line = c.stamper.stamp(reasoning_limits_submit(id, 0, text, None), None).to_line();
    let mut v: Value = serde_json::from_str(line.trim_end()).unwrap();
    for (k, x) in extra {
        v[*k] = x.clone();
    }
    c.raw(&format!("{v}\n")).await;
}

async fn one_turn_with(rig: &Rig, text: &str, extra: &[(&str, Value)]) -> Vec<Envelope> {
    let (mut c, _) = Client::attach(rig, "host", Role::Controller).await;
    send_with(&mut c, "c1", text, extra).await;
    turn_lifecycle_until(rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await
}

async fn local_uncapped(url: &str) -> local::Config {
    let bridge = crate::flash::test_server::start_keyed(crate::flash::test_server::Answer::Stream { events: vec![], gap: Duration::ZERO, hang: false }, KEY).await;
    let base = crate::dsh::config_with_model(KEY, &bridge.url, "reply-limit/base", None).unwrap();
    let (authority, base_path) = local::parse_loopback_url(url).unwrap();
    local::Config { authority, base_path, model: "granite41-3b-q4km".into(), n_ctx: 4096, first_token: Duration::from_secs(5), ..base }
}

#[tokio::test]
async fn deepseek_direct_sends_dshs_route_default_not_400() {
    use crate::flash::test_server::{start, Answer, ca};
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::from_millis(1), hang: false }).await;
    let cfg = crate::flash::config(KEY, &s.url, Some(ca())).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "write me a long program"})], &tx, None).await.unwrap();
    let b = body_of(&s.requests.lock().unwrap()[0]);
    assert_eq!(b["max_tokens"], json!(FLASH_LIMIT), "{b}");
}

#[tokio::test]
async fn the_bridge_sends_no_max_tokens_and_does_not_cut_a_long_reply() {
    use crate::flash::test_server::{start_keyed, Answer};
    let events: Vec<String> = (0..900).map(|i| content(&format!("word{i} "))).collect();
    let s = start_keyed(Answer::Stream { events, gap: Duration::ZERO, hang: false }, KEY).await;
    let cfg = crate::dsh::config_with_model(KEY, &s.url, "openai-codex/gpt-5.6", None).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let done = local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "go on"})], &tx, None).await.unwrap();
    drop(tx);
    let mut got = String::new();
    while let Some(t) = rx.recv().await {
        got += &t;
    }
    let b = body_of(&s.requests.lock().unwrap()[0]);
    assert!(b.get("max_tokens").is_none(), "DSH fills its per-model value: {b}");
    assert_eq!(b["reasoning_effort"], json!("none"), "{b}");
    assert!(!done.capped && got.contains("word899"), "the whole reply: {} chars", got.chars().count());
}

#[tokio::test]
async fn the_local_model_gets_its_slots_room_after_the_prompt() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let f = fake_llama(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let cfg = local_uncapped(&f.url).await;
    let msgs = vec![json!({"role": "system", "content": "You are Unfold. ".repeat(40)}), json!({"role": "user", "content": "write me a long program"})];
    let (chars, n) = local::request_size(&msgs);
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    local::stream_chat_metered(&cfg, msgs, &tx, None).await.unwrap();
    let b = f.requests.lock().unwrap().iter().find(|r| r["stream"] == true).cloned().unwrap();
    let room = 4096 - local::est_tokens(chars) - local::PER_MESSAGE_TOKENS * n - local::CTX_MARGIN_TOKENS;
    assert_eq!(b["max_tokens"], json!(room), "{b}");
}

#[tokio::test]
async fn no_reply_path_sends_400() {
    use crate::flash::test_server::{start, start_keyed, Answer, ca};
    use crate::local::fake_server::{start as fake_llama, Mode};
    let ask = || vec![json!({"role": "user", "content": "hello"})];
    let (tx, _rx) = tokio::sync::mpsc::channel(64);
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::ZERO, hang: false }).await;
    local::stream_chat_metered(&crate::flash::config(KEY, &s.url, Some(ca())).unwrap(), ask(), &tx, None).await.unwrap();
    let k = start_keyed(Answer::Stream { events: vec![content("Hi.")], gap: Duration::ZERO, hang: false }, KEY).await;
    local::stream_chat_metered(&crate::dsh::config_with_model(KEY, &k.url, "deepseek-official/deepseek-v4-flash", None).unwrap(), ask(), &tx, None).await.unwrap();
    let f = fake_llama(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    local::stream_chat_metered(&local_uncapped(&f.url).await, ask(), &tx, None).await.unwrap();
    let mut sent = vec![body_of(&s.requests.lock().unwrap()[0])["max_tokens"].clone(), body_of(&k.requests.lock().unwrap()[0])["max_tokens"].clone()];
    sent.extend(f.requests.lock().unwrap().iter().filter(|r| r["stream"] == true).map(|r| r["max_tokens"].clone()));
    assert!(sent.iter().all(|m| *m != json!(400)), "{sent:?}");
}

#[test]
fn reply_max_tokens_rides_on_both_turn_frames_and_a_bad_one_refuses_the_frame() {
    for frame in [r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi""#, r#"{"kind":"user_text","request_id":"q","text":"hi""#] {
        let e: Event = serde_json::from_str(&format!(r#"{frame},"reply_max_tokens":2000}}"#)).unwrap();
        assert_eq!(serde_json::to_value(&e).unwrap()["reply_max_tokens"], json!(2000), "{frame}");
        let old: Event = serde_json::from_str(&format!("{frame}}}")).unwrap();
        assert!(serde_json::to_value(&old).unwrap().get("reply_max_tokens").is_none(), "absent stays absent: {frame}");
        for bad in ["0", "-1", "1.5", r#""2000""#, "1048577", "null", "true", "{}"] {
            assert!(serde_json::from_str::<Event>(&format!(r#"{frame},"reply_max_tokens":{bad}}}"#)).is_err(), "{bad} on {frame}");
        }
    }
}

#[tokio::test]
async fn a_frame_with_a_bad_reply_max_tokens_is_rejected_not_answered() {
    use crate::flash::test_server::{start, Answer};
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("reply-limit-bad-reply-max");
    let (rig, _f) = reasoning_limits_rig(&dir, "x1", &s.url, false).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    send_with(&mut c, "c1", "hello", &[("reply_max_tokens", json!(0))]).await;
    let got = next_matching(&mut c, |e| matches!(e, Event::Rejected { .. } | Event::AgentDone { .. })).await;
    assert!(matches!(got.event, Event::Rejected { .. }), "{:?}", got.event);
    assert!(s.requests.lock().unwrap().is_empty(), "nothing was sent to the model");
}

#[tokio::test]
async fn reply_max_tokens_is_the_reply_room_with_thinking_on_top_under_the_model_limit() {
    use crate::flash::test_server::{start, Answer};
    for (effort, r, want) in [("off", 1000u64, 1000u64), ("high", 1000, 5096), ("max", 1_000_000, FLASH_LIMIT), ("low", 50, 2098)] {
        let events: Vec<String> = (0..900).map(|i| content(&format!("word{i} "))).collect();
        let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: false }).await;
        let dir = command_dir(&format!("reply-limit-reply-max-{effort}-{r}"));
        let (rig, _f) = reasoning_limits_rig(&dir, "m1", &s.url, false).await;
        let recs = one_turn_with(&rig, "write it all", &[("effort", json!(effort)), ("reply_max_tokens", json!(r))]).await;
        let b = &flash_bodies(&s)[0];
        assert_eq!(b["max_tokens"].as_u64(), Some(want), "{effort} R={r}: {b}");
        let p = &routing_provenance(&recs)[0];
        assert_eq!((p["config"]["max_tokens"].as_u64(), p["config"]["reply_max_tokens"].as_u64()), (Some(want), Some(r)), "{p}");
        if r == 50 {
            let text = reply_of(&recs);
            assert!(!text.contains("word899") && local::est_tokens(text.chars().count()) <= 50 + 48, "held to R: {text:?}");
        }
    }
}

#[tokio::test]
async fn reply_max_tokens_reaches_the_bridge_and_the_local_model() {
    use crate::flash::test_server::{start_keyed, Answer};
    let dir = command_dir("reply-limit-reply-max-dsh");
    let s = start_keyed(Answer::Stream { events: vec![content("Oi!")], gap: Duration::ZERO, hang: false }, DSH_KEY).await;
    let (rig, _f) = dsh_rig(&dir, "d1", &s.url, "", r#"{"d":"no_op"}"#).await;
    one_turn_with(&rig, "oi", &[("reply_max_tokens", json!(300))]).await;
    assert_eq!(flash_bodies(&s)[0]["max_tokens"].as_u64(), Some(300), "the bridge forwards R");

    let dir = command_dir("reply-limit-reply-max-local");
    let (rig, f) = turn_lifecycle_rig(&dir, "l1", r#"{"d":"no_op"}"#).await;
    one_turn_with(&rig, "hello", &[("reply_max_tokens", json!(300))]).await;
    let sent: Vec<_> = f.requests.lock().unwrap().iter().filter(|r| r["stream"] == true && r["max_tokens"] != 1).map(|r| r["max_tokens"].clone()).collect();
    assert_eq!(sent, vec![json!(300)]);
}

#[tokio::test]
async fn a_reply_at_its_limit_keeps_its_text_and_says_continue_with_no_retry() {
    use crate::flash::test_server::{start, Answer};
    for (text, lang) in [("Here is the first part. And the second", "en"), ("Aqui está a primeira parte. E a segunda", "pt")] {
        let s = start(Answer::Stream { events: vec![content(text), finish("length")], gap: Duration::from_millis(1), hang: false }).await;
        let dir = command_dir(&format!("reply-limit-continue-{lang}"));
        let (rig, f) = reasoning_limits_rig(&dir, "k1", &s.url, false).await;
        let recs = one_turn_with(&rig, if lang == "pt" { "me explica tudo" } else { "explain everything" }, &[]).await;
        let reply = reply_of(&recs);
        let first = text.split(". ").next().unwrap();
        assert!(reply.starts_with(first), "{reply:?}");
        assert!(reply.contains("\"continue\""), "{lang}: {reply:?}");
        assert!(!reply.ends_with(if lang == "pt" { " (cortado)" } else { " (cut short)" }), "{reply:?}");
        assert!(recs.iter().any(|e| matches!(e.event, Event::AgentDone { status: ResponseStatus::Completed, .. })));
        assert_eq!(s.requests.lock().unwrap().len(), 1, "no retry");
        assert!(f.requests.lock().unwrap().iter().all(|r| r["stream"] != true || r["max_tokens"] == 1), "no local fallback");
    }
}

#[tokio::test]
async fn the_ledger_reserves_an_estimate_and_settles_at_the_usage() {
    use crate::flash::test_server::{start, Answer};
    use crate::flash_spend::{PRICE_IN, PRICE_OUT};
    let usage = json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 3000}}).to_string();
    let s = start(Answer::Stream { events: vec![content("A long answer."), usage], gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("reply-limit-ledger");
    let ledger = routing_ledger("reply-limit");
    let (rig, _f) = router_rig_spend(&dir, "l1", &s.url, crate::flash::FIRST_TOKEN, "allow:flash", Some(ledger.clone())).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "hello")).await;
    turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. })) && r.iter().any(|e| matches!(e.event, Event::SpendStatus { spent_pico, .. } if spent_pico > 0))).await;
    let root = std::env::temp_dir().join(format!("rth-agentd-flash-spend-reply-limit-{}", std::process::id()));
    let row: Value = serde_json::from_slice(&std::fs::read(root.join("2026-10/call-000000.json")).unwrap()).unwrap();
    let reserve = row["reserve_pusd"].as_u64().unwrap();
    let sent = flash_bodies(&s)[0]["max_tokens"].as_u64().unwrap();
    assert!(reserve < sent * PRICE_OUT, "an estimate, not the limit's worst case: {reserve} vs {sent} tokens");
    assert!(reserve >= 400 * PRICE_OUT, "the estimate covers a typical reply: {reserve}");
    assert_eq!(ledger.status().spent_pusd, 1000 * PRICE_IN + 3000 * PRICE_OUT, "settled at the usage, past the estimate");
}

#[tokio::test]
async fn the_spend_stop_still_sends_the_turn_local() {
    use crate::flash::test_server::{start, Answer};
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("reply-limit-stop");
    let ledger = routing_ledger("reply-limit-stop");
    let root = std::env::temp_dir().join(format!("rth-agentd-flash-spend-reply-limit-stop-{}", std::process::id()));
    std::fs::create_dir_all(root.join("2026-10")).unwrap();
    std::fs::write(root.join("2026-10/stop.json"), r#"{"spent_pusd":49000000000000,"stop_pusd":49000000000000}"#).unwrap();
    let (rig, _f) = router_rig_spend(&dir, "t1", &s.url, crate::flash::FIRST_TOKEN, "allow:flash", Some(ledger)).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "hello")).await;
    let recs = turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await;
    assert!(s.requests.lock().unwrap().is_empty(), "Flash was not called");
    assert_eq!(routing_provenance(&recs)[0]["fallback_reason"].as_str(), Some("cap"));
}

#[tokio::test]
async fn a_flash_turn_still_carries_the_chat_so_far() {
    use crate::flash::test_server::{start, Answer};
    let s = start(Answer::Stream { events: vec![content("Noted.")], gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("reply-limit-history");
    let (rig, _f) = reasoning_limits_rig(&dir, "h1", &s.url, false).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "my cat is called Mingau")).await;
    turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() == 1).await;
    c.send(submit("c2", 1, "what is my cat called?")).await;
    turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() == 2).await;
    let second = &flash_bodies(&s)[1];
    assert!(second["messages"].to_string().contains("Mingau"), "{second}");
}
