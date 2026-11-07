

use std::time::Duration;

use serde_json::{json, Value};

use crate::flash::test_server::{start, start_keyed, Answer, ca};
use crate::local;

const KEY: &str = "reasoning-test-key-not-a-secret-0123456789";

fn body_of(raw: &str) -> Value {
    serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

fn content(text: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {"content": text}}]}).to_string()
}

fn reasoning(text: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {"reasoning_content": text}}]}).to_string()
}

fn finish(reason: &str) -> String {
    json!({"model": "deepseek-flash", "choices": [{"delta": {}, "finish_reason": reason}]}).to_string()
}

fn assert_off(b: &Value) {
    assert_eq!(b["thinking"], json!({"type": "disabled"}), "{b}");
    assert_eq!(b["reasoning_effort"], json!("none"), "{b}");
    assert!(b.get("chat_template_kwargs").is_none(), "the official API ignores chat_template_kwargs: {b}");
}

#[tokio::test]
async fn a_flash_reply_sends_deepseeks_own_off_fields() {
    let s = start(Answer::Stream { events: vec![content("Hi.")], gap: Duration::from_millis(1), hang: false }).await;
    let cfg = crate::flash::config(KEY, &s.url, Some(ca())).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap();
    let b = body_of(&s.requests.lock().unwrap()[0]);
    assert_off(&b);
    assert_eq!(b["max_tokens"], json!(crate::flash::REPLY_LIMIT), "regression: the model's limit, no room set");
}

#[tokio::test]
async fn a_length_finish_with_no_visible_reply_is_a_failure_not_an_empty_bubble() {
    let events = vec![reasoning("Let me think about this carefully. "), reasoning("First, the integral..."), finish("length")];
    let s = start(Answer::Stream { events, gap: Duration::from_millis(1), hang: false }).await;
    let cfg = crate::flash::config(KEY, &s.url, Some(ca())).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let r = local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "prove it"})], &tx, None).await;
    drop(tx);
    let mut got = String::new();
    while let Some(t) = rx.recv().await {
        got += &t;
    }
    assert!(got.trim().is_empty(), "nothing visible was sent: {got:?}");
    assert!(r.is_err(), "an empty reply at the cap must fail: {r:?}");
}

#[tokio::test]
async fn the_bridge_always_gets_an_explicit_reasoning_effort() {
    let s = start_keyed(Answer::Stream { events: vec![content("Hi.")], gap: Duration::from_millis(1), hang: false }, KEY).await;
    let cfg = crate::dsh::config_with_model(KEY, &s.url, "openai-codex/gpt-5.6", Some("GPT-5.6")).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap();
    let b = body_of(&s.requests.lock().unwrap()[0]);
    assert_eq!(b["reasoning_effort"], json!("none"), "{b}");
    assert!(b.get("max_tokens").is_none(), "regression: DSH fills its per-model value: {b}");
}

#[tokio::test]
async fn agentd_enforces_the_reply_cap_on_the_bridge() {
    let events: Vec<String> = (0..900).map(|i| content(&format!("word{i} "))).collect();
    let s = start_keyed(Answer::Stream { events, gap: Duration::ZERO, hang: false }, KEY).await;
    let cfg = local::Config { max_tokens: Some(400), ..crate::dsh::config_with_model(KEY, &s.url, "openai-codex/gpt-5.6", None).unwrap() };
    let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
    let done = local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "go on"})], &tx, None).await.unwrap();
    drop(tx);
    let mut got = String::new();
    while let Some(t) = rx.recv().await {
        got += &t;
    }
    assert!(done.capped, "the reply reached agentd's cap");
    assert!(!got.contains("word899"), "the stream was not read past the cap");

    let marker = local::est_tokens(local::cut_marker(local::Lang::En).len());
    assert!(local::est_tokens(got.chars().count()) <= 400 + 16 + marker, "about the 400-token reply budget: {} chars", got.chars().count());
}

#[test]
fn memory_decide_on_flash_sends_the_off_fields() {
    assert_off(&crate::decide::body_flash("deepseek-flash", "hi", &[]));
}

#[tokio::test]
async fn a_side_call_sends_the_off_fields() {
    let s = start(Answer::Json(json!({"model": "deepseek-flash", "choices": [{"message": {"role": "assistant", "content": "{}"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}}).to_string())).await;
    let root = std::env::temp_dir().join(format!("rth-agentd-reasoning-side-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let ledger = std::sync::Arc::new(crate::flash_spend::Ledger::open(&root, crate::flash::SIDE_MAX_TOKENS));
    let cfg = crate::flash::config(KEY, &s.url, Some(ca())).unwrap();
    let side = crate::flash_side::FlashSide::for_session("flash+local@granite", Some(cfg), Some(ledger), "b1", "").unwrap();
    let call = crate::flash_side::SideCall { purpose: crate::flash_spend::Purpose::Reachout, system: "S".into(), user: "U".into(), schema: json!({"type": "json_schema"}), max_tokens: 160, carries_memory: false, deadline: Duration::from_secs(3) };
    let _ = side.call(call).await;
    let b = body_of(&s.requests.lock().unwrap()[0]);
    assert_off(&b);
    let _ = std::fs::remove_dir_all(&root);
}
