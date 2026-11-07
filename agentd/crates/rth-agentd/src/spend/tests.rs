

use super::*;

fn realtime() -> Value {
    json!({ "dir": "realtime", "cap_pusd": 4_300_000_000_000u64, "max_attempts": 12, "attempt_bound_pusd": 536_600_000_000u64, "model": "gpt-realtime-2.1", "context_window": 128_000,
        "options": { "text_only": true, "vad": "none", "max_output_tokens": 1024 },
        "prices": { "text_in": 4_000_000, "text_cached": 400_000, "text_out": 24_000_000, "audio_in": 32_000_000, "audio_cached": 400_000, "audio_out": 64_000_000, "source": "test fixture", "read": "2026-10-23" } })
}

pub(crate) fn expect() -> Expect {
    Expect { model: "gpt-realtime-2.1".into(), text_only: true, vad: "none".into(), max_output_tokens: Some(1024) }
}

pub(crate) fn init(layer: Value) -> (PathBuf, String) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let k = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!("rth-spend-{}-{}-{k}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let dir = root.join("spend");
    fs::create_dir_all(&dir).unwrap();
    let rec = json!({ "schema": SCHEMA, "accept_id": "acc-rt", "stop": "stop.json", "layers": { "realtime": layer } });
    write_once(&dir.join("control.json"), &rec).unwrap();
    fs::create_dir(dir.join("realtime")).unwrap();
    write_once(&dir.join("realtime/open.json"), &json!({ "accept_id": "acc-rt", "layer": "realtime" })).unwrap();
    let sha = sha256_hex(&fs::read(dir.join("control.json")).unwrap());
    (dir.join("control.json"), sha)
}

pub(crate) fn usage(text_in: u64, text_out: u64) -> Value {
    json!({ "input_tokens": text_in, "output_tokens": text_out, "input_token_details": { "text_tokens": text_in, "audio_tokens": 0, "cached_tokens": 0 }, "output_token_details": { "text_tokens": text_out, "audio_tokens": 0 } })
}

fn files(c: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(c.parent().unwrap().join("realtime")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|n| n != "open.json").collect();
    v.sort();
    v
}

#[test]
fn sb_r1_the_pinned_record_and_the_effective_configuration_must_match() {
    let (c, sha) = init(realtime());
    assert!(Spend::open(&c, &sha, &expect()).is_ok());
    assert!(Spend::open(&c, &"0".repeat(64), &expect()).unwrap_err().contains("not the pinned one"));
    let mut e = expect();
    e.max_output_tokens = Some(4096);
    assert!(Spend::open(&c, &sha, &e).unwrap_err().contains("max_output_tokens"));
    let mut e = expect();
    e.text_only = false;
    assert!(Spend::open(&c, &sha, &e).unwrap_err().contains("text_only"));

    let mut l = realtime();
    l["cap_pusd"] = json!(4_300_000_000_001u64);
    let (c2, _) = init(l);
    assert!(Spend::open(&c2, &sha, &expect()).unwrap_err().contains("not the pinned one"));

    let mut l = realtime();
    l["cap_pusd"] = json!(5_000_000_000_001u64);
    let (c3, sha3) = init(l);
    assert!(Spend::open(&c3, &sha3, &expect()).unwrap_err().contains("exceed"));
}

#[test]
fn sb_r1_a_missing_established_ledger_is_never_recreated() {
    let (c, sha) = init(realtime());
    fs::remove_dir_all(c.parent().unwrap().join("realtime")).unwrap();
    assert!(Spend::open(&c, &sha, &expect()).unwrap_err().contains("never recreated"));
    assert!(!c.parent().unwrap().join("realtime").exists());
}

#[test]
fn sb_r1_attempts_are_claimed_once_and_survive_a_restart() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    let first = a.admit("b1-v:1:q1", json!({ "request_id": "q1" })).unwrap();
    assert_eq!(first.n, 0);
    assert_eq!(a.settle(&first, "resp_1", Some(&usage(1500, 34))).unwrap(), 1500 * 4_000_000 + 34 * 24_000_000);

    let mut b = Spend::open(&c, &sha, &expect()).unwrap();
    assert_eq!(b.admit("b2-v:1:q2", json!({})).unwrap().n, 1);
    assert_eq!(files(&c), vec!["attempt-000.json", "attempt-000.settle.json", "attempt-001.json"]);

    let (c2, sha2) = init(realtime());
    let handles: Vec<_> = (0..8).map(|i| { let (c2, sha2) = (c2.clone(), sha2.clone()); std::thread::spawn(move || Spend::open(&c2, &sha2, &expect()).unwrap().admit(&format!("t{i}"), json!({})).map(|a| a.n)) }).collect();
    let mut ns: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap().unwrap()).collect();
    ns.sort();
    assert_eq!(ns, (0..8).collect::<Vec<_>>());
}

#[test]
fn sb_r1_the_cap_and_the_allowance_hold_with_unsettled_reservations() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();

    for i in 0..8 {
        a.admit(&format!("q{i}"), json!({})).unwrap();
    }
    assert!(a.admit("q8", json!({})).unwrap_err().contains("cap would be exceeded"));
    let mut l = realtime();
    l["max_attempts"] = json!(2);
    let (c2, sha2) = init(l);
    let mut b = Spend::open(&c2, &sha2, &expect()).unwrap();
    let x = b.admit("q0", json!({})).unwrap();
    b.settle(&x, "r0", Some(&usage(0, 0))).unwrap();
    b.admit("q1", json!({})).unwrap();
    assert!(b.admit("q2", json!({})).unwrap_err().contains("allowance of 2 is used"), "a zero-cost settlement frees room, never the allowance");
}

#[test]
fn sb_r1_sb_r2_a_torn_reserve_fails_and_writes_the_stop_marker_first() {
    let (c, sha) = init(realtime());
    fs::write(c.parent().unwrap().join("realtime/attempt-000.json"), "{\"n\":0,").unwrap();
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    assert!(a.admit("q", json!({})).unwrap_err().contains("does not parse"));
    assert!(c.parent().unwrap().join("stop.json").exists());
    assert_eq!(a.stopped().as_deref(), Some("spend: stopped"));
}

#[test]
fn sb_r2_missing_usage_keeps_the_reservation_and_a_restart_stays_stopped() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    let x = a.admit("q", json!({})).unwrap();
    assert!(a.settle(&x, "r", None).unwrap_err().contains("no usage"));
    let mut clipped = usage(10, 10);
    clipped["output_token_details"].as_object_mut().unwrap().remove("audio_tokens");
    let (c2, sha2) = init(realtime());
    let mut b = Spend::open(&c2, &sha2, &expect()).unwrap();
    let y = b.admit("q", json!({})).unwrap();
    assert!(b.settle(&y, "r", Some(&clipped)).unwrap_err().contains("incomplete usage"));
    assert_eq!(files(&c), vec!["attempt-000.json"], "the reservation is kept");
    let mut restarted = Spend::open(&c, &sha, &expect()).unwrap();
    assert_eq!(restarted.admit("q2", json!({})).unwrap_err(), "spend: stopped");
}

#[test]
fn sb_r1_identical_replay_is_harmless_and_a_conflicting_settlement_fails() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    let x = a.admit("q", json!({})).unwrap();
    a.settle(&x, "r", Some(&usage(100, 10))).unwrap();
    a.settle(&x, "r", Some(&usage(100, 10))).unwrap();
    assert_eq!(a.stopped(), None);
    assert!(a.settle(&x, "r", Some(&usage(100, 11))).unwrap_err().contains("conflicting settlement"));
    assert!(a.stopped().is_some());
}

#[test]
fn sb_r2_audio_unpriced_input_or_a_cost_above_the_bound_fail() {
    for (u, why) in [
        (json!({ "input_tokens": 10, "output_tokens": 5, "input_token_details": { "text_tokens": 10, "audio_tokens": 0, "cached_tokens": 0 }, "output_token_details": { "text_tokens": 0, "audio_tokens": 5 } }), "audio tokens"),
        (json!({ "input_tokens": 20, "output_tokens": 5, "input_token_details": { "text_tokens": 10, "audio_tokens": 0, "cached_tokens": 0 }, "output_token_details": { "text_tokens": 5, "audio_tokens": 0 } }), "unpriced input"),
        (usage(128_010, 1024), "above its bound"),
    ] {
        let (c, sha) = init(realtime());
        let mut a = Spend::open(&c, &sha, &expect()).unwrap();
        let x = a.admit("q", json!({})).unwrap();
        let e = a.settle(&x, "r", Some(&u)).unwrap_err();
        assert!(e.contains(why), "{e}");
        assert!(a.stopped().is_some(), "{why}");
    }
}

#[test]
fn sb_r2_the_stop_marker_present_unreadable_or_unwritable_refuses() {
    let (c, sha) = init(realtime());
    fs::write(c.parent().unwrap().join("stop.json"), "{\"layer\":\"worker\"}").unwrap();
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    assert_eq!(a.admit("q", json!({})).unwrap_err(), "spend: stopped", "the other layer's stop");
    let (c2, sha2) = init(realtime());
    let mut b = Spend::open(&c2, &sha2, &expect()).unwrap();
    b.set_stop_path(c2.join("stop.json"));
    assert!(b.stopped().unwrap().contains("unreadable"));
    let (c3, sha3) = init(realtime());
    let mut d = Spend::open(&c3, &sha3, &expect()).unwrap();
    d.set_stop_path(c3.parent().unwrap().join("no-such-dir/stop.json"));
    d.fail("test failure");
    assert!(d.stopped().unwrap().contains("stop marker not written"));
    assert!(d.admit("q", json!({})).unwrap_err().contains("stop marker not written"));
}

#[test]
fn sb_r2_an_unbound_response_is_recorded_at_the_full_bound_and_stops() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    a.anomaly("resp_ghost", None);
    assert_eq!(files(&c), vec!["anomaly-resp_ghost.json"]);
    assert!(a.stopped().is_some());
    assert!(a.reread().unwrap_err().contains("fatal record"), "fatal on replay, with or without its stop");
}

#[test]
fn regression_anomaly_crash_before_stop_stays_stopped_after_reopen() {
    let (c, sha) = init(realtime());
    let root = c.parent().unwrap();

    write_once(&root.join("realtime/anomaly-resp_ghost.json"), &json!({"provider_response_id":"resp_ghost", "usage":null, "bound_pusd":536_600_000_000u64})).unwrap();
    let mut reopened = Spend::open(&c, &sha, &expect()).unwrap();
    assert!(reopened.admit("after-crash", json!({})).is_err(), "fatal anomaly reopened admission");
}

#[test]
fn regression_foreign_attempt_cannot_settle_local_reservation() {
    let (c, sha) = init(realtime());
    let mut sp = Spend::open(&c, &sha, &expect()).unwrap();
    let mut a = sp.admit("owned", json!({})).unwrap();
    a.attempt_id = "foreign-acceptance".into();
    assert!(sp.settle(&a,"foreign-response",Some(&usage(0,0))).is_err(), "foreign attempt settled owned reservation");
}

#[test]
fn regression_foreign_settlement_cannot_release_replayed_budget() {
    let (c, sha) = init(realtime());
    let mut sp = Spend::open(&c, &sha, &expect()).unwrap();
    for n in 0..8 { sp.admit(&format!("owned-{n}"),json!({})).unwrap(); }
    assert!(sp.admit("ninth",json!({})).is_err());
    write_once(&c.parent().unwrap().join("realtime/attempt-000.settle.json"),&json!({"n":0,"accept_id":"foreign","layer":"worker","attempt_id":"other", "usage":usage(0,0),"cost_pusd":0})).unwrap();
    let mut reopened = Spend::open(&c,&sha,&expect()).unwrap();
    assert!(reopened.admit("ninth",json!({})).is_err(), "foreign settlement released budget");
}

#[test]
fn sb3_r2_a_stop_seen_after_the_claim_keeps_the_reservation_and_refuses_dispatch() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();

    a.stop_after_claim = true;
    let e = a.admit("q0", json!({})).unwrap_err();
    assert!(e.contains("stays reserved, not dispatched"), "{e}");
    assert_eq!(files(&c), vec!["attempt-000.json"], "the reservation stays");
}

#[test]
fn sb3_r1_metadata_cannot_overwrite_the_authoritative_fields() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    a.admit("owned", json!({ "bound_pusd": 0, "n": 9, "attempt_id": "y", "accept_id": "x" })).unwrap();
    let row: Value = serde_json::from_slice(&fs::read(c.parent().unwrap().join("realtime/attempt-000.json")).unwrap()).unwrap();
    assert_eq!((row["n"].clone(), row["attempt_id"].clone(), row["bound_pusd"].clone(), row["accept_id"].clone()), (json!(0), json!("owned"), json!(536_600_000_000u64), json!("acc-rt")));
}

#[test]
fn sb3_r1_a_replayed_settlement_must_be_its_usage_at_the_pinned_prices() {
    let (c, sha) = init(realtime());
    let mut a = Spend::open(&c, &sha, &expect()).unwrap();
    let x = a.admit("q", json!({})).unwrap();
    a.settle(&x, "r", Some(&usage(10, 1))).unwrap();
    let p = c.parent().unwrap().join("realtime/attempt-000.settle.json");
    let mut row: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    row["cost_pusd"] = json!(0);
    fs::remove_file(&p).unwrap();
    fs::write(&p, row.to_string()).unwrap();
    let mut reopened = Spend::open(&c, &sha, &expect()).unwrap();
    assert!(reopened.admit("q2", json!({})).unwrap_err().contains("not its usage at the pinned prices"));
}
