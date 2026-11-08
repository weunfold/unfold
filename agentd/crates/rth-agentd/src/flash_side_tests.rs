

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::flash::test_server::{Answer, ca, Server, start};

async fn within<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(30), f).await.expect("the test ended within 30 s")
}

const KEY: &str = "side-test-key-0123456789";

const HOSTED: &str = "https://flash.invalid/v1";

fn root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("rth-agentd-flash-side-{name}-{}", std::process::id()))
}

fn settled(name: &str, n: u64) -> bool {
    root(name).join("2026-10").join(format!("call-{n:06}.settle.json")).exists()
}

fn ledger(name: &str) -> Arc<Ledger> {
    let root = root(name);
    let _ = std::fs::remove_dir_all(&root);
    let mut l = Ledger::open(&root, crate::flash::SIDE_MAX_TOKENS);
    l.month = Some("2026-10".into());
    l.day = Some("2026-10-26".into());
    Arc::new(l)
}

fn side(s: &Server, ledger: &Arc<Ledger>, egress: &str) -> (FlashSide, String) {
    let cfg = crate::flash::config(KEY, &s.url, Some(ca())).unwrap();
    let authority = cfg.authority.clone();
    (FlashSide::for_session("flash+local@granite", Some(cfg), Some(ledger.clone()), "b1", egress).unwrap(), authority)
}

fn call(carries_memory: bool) -> SideCall {
    SideCall {
        purpose: Purpose::Reachout,
        system: "SYSTEM TEXT".into(),
        user: "USER TEXT".into(),
        schema: rth_expression::draft::schema(),
        max_tokens: 160,
        carries_memory,
        deadline: Duration::from_secs(3),
    }
}

fn answer(text: &str) -> Answer {
    Answer::Json(json!({"model": "deepseek-ai/DeepSeek-V4.1-Flash-1017", "choices": [{"message": {"role": "assistant", "content": text}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 120, "completion_tokens": 20}}).to_string())
}

fn bodies(s: &Server) -> Vec<Value> {
    s.requests.lock().unwrap().iter().map(|raw| serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()).collect()
}

fn reachout_spend(l: &Ledger) -> u64 {
    l.status().by_purpose.get("reachout").copied().unwrap_or(0)
}

#[tokio::test]
async fn b6_a_side_call_is_one_non_streamed_request_with_thinking_off() {
    within(async {
        let s = start(answer(r#"{"send": true, "text": "hi"}"#)).await;
        let l = ledger("body");
        let (side, _) = side(&s, &l, "");
        let r = side.call(call(false)).await.unwrap();
        assert_eq!((r.attempt_id.as_str(), r.text.as_str()), ("x-b1-1", r#"{"send": true, "text": "hi"}"#));
        let b = &bodies(&s)[0];
        assert_eq!(b["stream"], false);
        assert_eq!(b["max_tokens"], 160);

        assert_eq!((b["thinking"].clone(), b["reasoning_effort"].clone(), b.get("chat_template_kwargs")), (json!({"type": "disabled"}), json!("none"), None));
        assert_eq!(b["response_format"], rth_expression::draft::schema());
        assert_eq!(b["messages"], json!([{"role": "system", "content": "SYSTEM TEXT"}, {"role": "user", "content": "USER TEXT"}]));
        assert!(b.get("stream_options").is_none() && b.get("cache_prompt").is_none());
        assert_eq!(r.provenance["config"], json!({"path": "reachout", "temperature": 1.0, "top_p": 1.0, "thinking": false, "effort": "off", "thread": "background"}));
        assert_eq!(r.provenance["model"], crate::flash::MODEL);
        assert_eq!(r.provider_model.as_deref(), Some("deepseek-ai/DeepSeek-V4.1-Flash-1017"));
        assert!(reachout_spend(&l) > 0 && settled("body", 0), "a reachout purpose row, settled: {:?}", l.status());
        assert_eq!(r.spend.by_purpose.get("chat"), None, "never counted as chat");
        assert_eq!(side.call(call(false)).await.unwrap().attempt_id, "x-b1-2");
    })
    .await
}

#[test]
fn b6_there_is_no_side_call_when_flash_is_off() {
    let l = ledger("off");
    let cfg = || Some(crate::flash::config(KEY, HOSTED, None).unwrap());
    assert!(FlashSide::for_session("local@granite", cfg(), Some(l.clone()), "b1", "allow:flash").is_none(), "the local backend");
    assert!(FlashSide::for_session("fake", cfg(), Some(l.clone()), "b1", "").is_none());
    assert!(FlashSide::for_session("flash+local@granite", None, Some(l.clone()), "b1", "").is_none(), "no Flash config");
    assert!(FlashSide::for_session("flash+local@granite", cfg(), None, "b1", "").is_none(), "no ledger");
    let loopback = local::Config { authority: "127.0.0.1:9".into(), ..crate::flash::config(KEY, HOSTED, None).unwrap() };
    assert!(FlashSide::for_session("flash+local@granite", Some(loopback), Some(l.clone()), "b1", "").is_none(), "a config that dials a local server");
    assert!(FlashSide::for_session("flash+local@granite", cfg(), Some(l), "b1", "").is_some());
}

#[tokio::test]
async fn b6_an_open_breaker_skips_and_a_side_failure_never_touches_it() {
    within(async {
        let s = start(Answer::Status(503, "{}".into())).await;
        let l = ledger("breaker");
        let (side, authority) = side(&s, &l, "");
        assert!(matches!(side.call(call(false)).await, Err(SideError::Unavailable(_))));
        assert_eq!(local::breaker_for(&authority).lock().unwrap().fails, 0, "the chat breaker is chat's");
        assert!(settled("breaker", 0), "a sent call is settled, failed or not");
        let before = (s.requests.lock().unwrap().len(), l.status());
        let now = tokio::time::Instant::now();
        for _ in 0..local::BREAKER_FAILS {
            local::breaker_for(&authority).lock().unwrap().failed("local_server_error", now);
        }
        assert_eq!(side.call(call(false)).await.unwrap_err(), SideError::BreakerOpen);
        assert_eq!((s.requests.lock().unwrap().len(), l.status()), before, "no request, no reservation");
        assert!(local::breaker_for(&authority).lock().unwrap().open(tokio::time::Instant::now()), "peeking leaves it open");

        *local::breaker_for(&authority).lock().unwrap() = local::Breaker::default();
    })
    .await
}

#[tokio::test]
async fn b6_memory_goes_only_with_the_egress_grant() {
    within(async {
        let s = start(answer(r#"{"send": false, "text": ""}"#)).await;
        let l = ledger("egress");
        let (closed, _) = side(&s, &l, "");
        assert_eq!(closed.call(call(true)).await.unwrap_err(), SideError::NoEgressGrant);
        assert!(s.requests.lock().unwrap().is_empty() && reachout_spend(&l) == 0);
        let (other, _) = side(&s, &l, "allow:gemini");
        assert_eq!(other.call(call(true)).await.unwrap_err(), SideError::NoEgressGrant, "a grant for another backend");
        let (open, _) = side(&s, &l, "allow:flash");
        assert!(open.call(call(true)).await.is_ok());
        assert_eq!(s.requests.lock().unwrap().len(), 1);
    })
    .await
}

#[tokio::test]
async fn b6_a_refused_admission_sends_nothing() {
    within(async {
        let s = start(answer(r#"{"send": true, "text": "hi"}"#)).await;
        let l = ledger("ceiling");
        let mut n = 0;

        while l.admit_for(&format!("x-fill-{n}"), 1, Purpose::Reachout).is_ok() {
            n += 1;
            assert!(n < 10_000, "the day ceiling never came");
        }
        let (side, _) = side(&s, &l, "");
        let e = side.call(call(false)).await.unwrap_err();
        assert!(matches!(&e, SideError::Refused(why) if why.contains("day ceiling")), "{e:?}");
        assert!(s.requests.lock().unwrap().is_empty());
        assert!(l.admit_for("r-1", 4000, Purpose::Chat).is_ok(), "chat is still admitted");
    })
    .await
}

#[tokio::test]
async fn b6_a_slow_or_empty_answer_is_a_failed_call_and_still_settled() {
    within(async {
        let slow = start(Answer::LateErrorDrip { head: Duration::from_secs(5), gap: Duration::from_millis(10), pieces: 1 }).await;
        let l = ledger("slow");
        let (slow_side, _) = side(&slow, &l, "");
        let quick = SideCall { deadline: Duration::from_millis(300), ..call(false) };
        assert_eq!(slow_side.call(quick).await.unwrap_err(), SideError::Timeout);
        assert!(settled("slow", 0), "settled with no usage: the whole reservation");
        let empty = start(Answer::Json(json!({"choices": []}).to_string())).await;
        let l = ledger("empty");
        let (empty_side, _) = side(&empty, &l, "");
        assert!(matches!(empty_side.call(call(false)).await, Err(SideError::Invalid(_))));
        assert!(settled("empty", 0));
    })
    .await
}
