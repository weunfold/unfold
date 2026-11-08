

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::local::fake_server::{start, Mode};

fn granite(url: &str, slot: Option<u32>, decide_slot: Option<u32>) -> local::Config {
    let (authority, base_path) = local::parse_loopback_url(url).unwrap();
    local::Config { authority, base_path, model: "granite41-3b-q4km".into(), model_sha256: None, slot, decide_slot, opener_slot: None, max_tokens: Some(64), n_ctx: 8192, first_token: Duration::from_secs(5), system_instruction: None, health_every: Duration::from_secs(60) }
}

#[test]
fn each_level_sends_deepseeks_fields_and_its_budget() {
    let rows = [(Level::Off, "disabled", "none", 0, 400), (Level::Low, "enabled", "low", 2048, 2448), (Level::High, "enabled", "high", 4096, 4496), (Level::Max, "enabled", "max", 8192, 8592)];
    for (l, kind, effort, think, max) in rows {
        let mut b = json!({});
        l.deepseek_fields(&mut b);
        assert_eq!(b, json!({"thinking": {"type": kind}, "reasoning_effort": effort}), "{l:?}");

        assert_eq!((l.think_budget(), max_tokens(Some(400), l)), (think, max), "{l:?}: max_tokens = room 400 + think");
        assert_eq!(max_tokens(None, l), crate::flash::REPLY_LIMIT, "{l:?}");
        assert_eq!(max_tokens(Some(crate::flash::REPLY_LIMIT), l), crate::flash::REPLY_LIMIT, "{l:?}: never past the limit");
        assert_eq!(estimate(None, l, max_tokens(None, l)), REPLY_ESTIMATE + think, "{l:?}: the ledger's estimate");
        assert_eq!(estimate(Some(100), l, max_tokens(Some(100), l)), 100 + think, "{l:?}");
    }
}

#[test]
fn autos_table_and_it_never_picks_max() {
    let table = [(Class::Quick, Level::Off), (Class::Some, Level::Low), (Class::Careful, Level::High), (Class::MultiStep, Level::Low)];
    for (c, l) in table {
        assert_eq!(c.level(), l, "{c:?}");
        let t = TurnLevel::auto(Check { class: Some(c), confidence: Some(0.9), ms: 3, reason: "ok" });
        assert_eq!((t.level, t.source), (l, Source::Auto));
        assert_ne!(t.level, Level::Max, "Auto never picks max");
    }
    for reason in ["timeout", "error", "no_letter", "low_confidence", "no_slot"] {
        assert_eq!(TurnLevel::auto(Check { class: None, confidence: None, ms: 150, reason }).level, Level::Off, "Unknown ({reason}) is off");
    }
}

#[test]
fn the_persons_level_wins_and_no_level_is_off() {
    assert_eq!(without_check(None, ""), TurnLevel { level: Level::Off, source: Source::Default, check: None, reply_max_tokens: None });
    for (e, l) in [(Effort::Off, Level::Off), (Effort::Low, Level::Low), (Effort::High, Level::High), (Effort::Max, Level::Max)] {
        assert_eq!(without_check(Some(e), ""), TurnLevel { level: l, source: Source::Person, check: None, reply_max_tokens: None }, "{e:?}");
        assert!(!wants_check(Some(e)));
    }
    assert!(wants_check(Some(Effort::Auto)) && !wants_check(None));
    let t = without_check(Some(Effort::Auto), "no_slot");
    assert_eq!((t.level, t.source, t.check.unwrap().reason), (Level::Off, Source::Auto, "no_slot"));
}

#[test]
fn the_provenance_names_the_level_who_chose_it_and_the_check() {
    let mut c = json!({"path": "chat", "effort": "off", "thinking": false});
    TurnLevel::auto(Check { class: Some(Class::Careful), confidence: Some(0.8), ms: 41, reason: "ok" }).extend(&mut c);
    assert_eq!(c, json!({"path": "chat", "effort": "high", "thinking": true, "effort_source": "auto", "think_budget": 4096, "max_tokens": 256000, "reply_max_tokens": null, "auto": {"class": "C", "confidence": 0.8, "ms": 41, "reason": "ok"}}));
    let mut c = json!({});
    TurnLevel::person(Effort::Max).extend(&mut c);
    assert_eq!((c["effort"].as_str(), c["effort_source"].as_str(), c.get("auto")), (Some("max"), Some("person"), None));
    let mut c = json!({});
    TurnLevel::person(Effort::High).with_reply(Some(1000)).extend(&mut c);
    assert_eq!((c["max_tokens"].as_u64(), c["reply_max_tokens"].as_u64()), (Some(5096), Some(1000)), "the room plus the thinking");
}

#[test]
fn letters_are_read_from_the_top_logprobs_and_renormalized() {
    let answer = |top: serde_json::Value| json!({"choices": [{"logprobs": {"content": [{"token": "C", "top_logprobs": top}]}}]});
    let p = letters(&answer(json!([{"token": "C", "logprob": 0.6f64.ln()}, {"token": " B", "logprob": 0.2f64.ln()}, {"token": "b", "logprob": 0.1f64.ln()}, {"token": "The", "logprob": 0.1f64.ln()}]))).unwrap();
    assert!((p[2] - 0.6 / 0.9).abs() < 1e-9 && (p[1] - 0.3 / 0.9).abs() < 1e-9 && p[0] == 0.0 && p[3] == 0.0, "{p:?}");
    let (c, conf) = classify(p);
    assert_eq!(c, Class::Careful);
    assert!((conf - (4.0 * 0.6 / 0.9 - 1.0) / 3.0).abs() < 1e-9);
    assert_eq!(letters(&answer(json!([{"token": "Sure", "logprob": -0.1}]))), None, "no letter: Unknown");
    assert_eq!(letters(&json!({"choices": [{"message": {"content": "A"}}]})), None, "no logprobs: Unknown");
    assert!((classify([0.25; 4]).1).abs() < 1e-9, "a uniform answer has confidence 0");
}

#[test]
fn the_check_body_is_one_token_with_logprobs_on_its_own_slot() {
    let long = "x".repeat(5000);
    let b = body("granite41-3b-q4km", &long, 1);
    assert_eq!((b["max_tokens"].as_u64(), b["temperature"].as_u64(), b["stream"].as_bool(), b["logprobs"].as_bool(), b["top_logprobs"].as_u64(), b["id_slot"].as_u64()), (Some(1), Some(0), Some(false), Some(true), Some(20), Some(1)));
    assert_eq!(b["chat_template_kwargs"], json!({"enable_thinking": false}), "thinking forced off");
    assert_eq!(b["messages"][0]["content"].as_str(), Some(CHECK_PROMPT));
    assert_eq!(b["messages"][1]["content"].as_str().unwrap().len(), 2000, "the head of a long paste");
}

#[test]
fn the_check_needs_its_own_slot_apart_from_the_reply_and_decide() {
    let g = |s, d| granite("http://127.0.0.1:9/v1", s, d);
    assert!(Checker::new(g(Some(0), Some(0)), None).is_err(), "no slot: no check");
    assert!(Checker::new(g(Some(0), Some(0)), Some("")).is_err());
    assert!(Checker::new(g(Some(0), Some(0)), Some("16")).is_err());
    assert!(Checker::new(g(None, Some(0)), Some("1")).is_err(), "the reply's slot must be set");
    assert!(Checker::new(g(Some(0), None), Some("1")).is_err(), "decide's slot must be set");
    assert!(Checker::new(g(Some(1), Some(0)), Some("1")).is_err(), "never the reply's slot");
    assert!(Checker::new(g(Some(0), Some(1)), Some("1")).is_err(), "never decide's slot");
    assert_eq!(Checker::new(g(Some(0), Some(0)), Some("1")).unwrap().slot, 1);
}

#[tokio::test]
async fn the_check_reads_the_letter_from_the_local_model() {
    let f = start(Mode::Mixed { pieces: vec![], answer: String::new(), warm_delay: Duration::ZERO }, Duration::ZERO).await;
    let c = Checker::new(granite(&f.url, Some(0), Some(0)), Some("1")).unwrap();
    let r = check(&c, "check:C prove that the square root of 2 is irrational", Duration::from_secs(2)).await;
    assert_eq!((r.class, r.reason), (Some(Class::Careful), "ok"), "{r:?}");
    assert!(r.confidence.unwrap() > 0.7, "{r:?}");
    let sent = f.requests.lock().unwrap()[0].clone();
    assert_eq!((sent["id_slot"].as_u64(), sent["logprobs"].as_bool()), (Some(1), Some(true)));
    let r = check(&c, "check:A hi!", Duration::from_secs(2)).await;
    assert_eq!((r.class, TurnLevel::auto(r).level), (Some(Class::Quick), Level::Off));
}

#[tokio::test]
async fn a_check_past_its_deadline_is_unknown_and_off() {
    let f = start(Mode::Silent(Duration::from_secs(2)), Duration::ZERO).await;
    let c = Checker::new(granite(&f.url, Some(0), Some(0)), Some("1")).unwrap();
    let t = std::time::Instant::now();
    let r = check(&c, "check:C prove it", CHECK_DEADLINE).await;
    assert!(t.elapsed() < Duration::from_millis(400), "bounded by the 150 ms deadline: {:?}", t.elapsed());
    assert_eq!((r.class, r.reason, TurnLevel::auto(r).level), (None, "timeout", Level::Off));
}

#[tokio::test]
async fn a_check_with_no_server_is_unknown_and_off() {
    let c = Checker::new(granite("http://127.0.0.1:9/v1", Some(0), Some(0)), Some("1")).unwrap();
    let r = check(&c, "prove it", CHECK_DEADLINE).await;
    assert_eq!((r.class, TurnLevel::auto(r.clone()).level), (None, Level::Off), "{r:?}");
    assert!(matches!(r.reason, "error" | "timeout"), "{r:?}");
}

#[tokio::test]
#[ignore]
async fn live_dev_set() {
    let url = std::env::var("RTH_ROUTE_LIVE_URL").expect("RTH_ROUTE_LIVE_URL");
    let set = std::fs::read_to_string(std::env::var("RTH_ROUTE_DEV_SET").expect("RTH_ROUTE_DEV_SET")).unwrap();
    let mut cfg = granite(&url, Some(0), Some(0));
    cfg.model = std::env::var("RTH_ROUTE_LIVE_MODEL").unwrap_or(cfg.model);
    let c = Checker::new(cfg, Some("1")).unwrap();

    let _ = check(&c, "hi", Duration::from_secs(30)).await;
    let mut rows = Vec::new();
    for line in set.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let text = v["text"].as_str().unwrap();

        let (r, d) = tokio::join!(check(&c, text, Duration::from_secs(5)), crate::decide::run_with(crate::decide::Saves::Realtime, &c.cfg, Some(0), text, &[], crate::decide::DEADLINE));
        let within = r.ms <= CHECK_DEADLINE.as_millis() as u64;
        let level = if within { TurnLevel::auto(r.clone()).level } else { Level::Off };
        println!("ROW {} want={} got={} conf={:?} ms={} decide_ms={} added_ms={} level={} reason={}", v["id"].as_str().unwrap_or("?"), v["class"].as_str().unwrap_or("?"), r.class.map_or("-", Class::letter), r.confidence.map(|c| (c * 100.0).round() / 100.0), r.ms, d.ms, r.ms.saturating_sub(d.ms), level.as_str(), r.reason);
        rows.push((v["class"].as_str().unwrap_or("?").to_string(), r.clone(), level, r.ms.saturating_sub(d.ms)));
    }
    let pcts = |mut ms: Vec<u64>| {
        ms.sort();
        let pct = |p: f64| ms[((ms.len() as f64 - 1.0) * p).round() as usize];
        format!("n={} p50={} p95={} max={}", ms.len(), pct(0.5), pct(0.95), ms[ms.len() - 1])
    };
    println!("LATENCY check {}", pcts(rows.iter().map(|r| r.1.ms).collect()));
    println!("LATENCY added_over_decide {}", pcts(rows.iter().map(|r| r.3).collect()));
    for want in ["A", "B", "C", "D"] {
        let of: Vec<_> = rows.iter().filter(|r| r.0 == want).collect();
        let hit = of.iter().filter(|r| r.1.class.map(Class::letter) == Some(want)).count();
        let levels = ["off", "low", "high", "max"].map(|l| of.iter().filter(|r| r.2.as_str() == l).count());
        println!("CLASS {want} n={} exact={hit} off={} low={} high={} max={}", of.len(), levels[0], levels[1], levels[2], levels[3]);
    }
}
