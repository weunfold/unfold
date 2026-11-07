

use std::io::Write;

#[test]
fn a_panic_on_any_thread_leaves_a_journal_row_and_a_last_panic_record() {
    let dir = std::env::temp_dir().join(format!("rth-panic-record-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("host")).unwrap();
    let journal = dir.join("rth-host.jsonl");
    std::fs::File::create(&journal).unwrap().write_all(b"{\"kind\":\"started\"}\n").unwrap();
    let last = dir.join("host/last-panic.json");
    rth::crash::record_panics(Some(journal.clone()), last.clone(), "host-4242".into());

    let r = std::thread::Builder::new().name("tokio-rt-worker".into()).spawn(|| panic!("assertion failed: state == REGISTERING")).unwrap().join();
    assert!(r.is_err(), "the thread panicked");

    let rows: Vec<serde_json::Value> = std::fs::read_to_string(&journal).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rows.len(), 2, "the existing row is kept and one row is appended: {rows:?}");
    let row = &rows[1];
    assert_eq!(row["kind"], "host_panic");
    assert_eq!(row["thread"], "tokio-rt-worker");
    assert_eq!(row["message"], "assertion failed: state == REGISTERING");
    assert!(row["location"].as_str().unwrap().contains("panic_record.rs"), "{row}");
    assert_eq!(row["pid"], std::process::id());
    assert!(row["utc_ms"].as_u64().unwrap() > 1_700_000_000_000);
    assert!(row.get("backtrace").is_none(), "the journal row stays short");

    let rec: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&last).unwrap()).unwrap();
    assert_eq!(rec["kind"], "host_panic");
    assert_eq!(rec["label"], "host-4242");
    assert_eq!(rec["message"], row["message"]);
    assert!(rec["backtrace"].as_str().unwrap().contains("panic_record"), "a forced backtrace, even without RUST_BACKTRACE");

    let _ = std::thread::spawn(|| panic!("frame {} of {}", 3, 4)).join();
    let rec: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&last).unwrap()).unwrap();
    assert_eq!(rec["message"], "frame 3 of 4");
    let _ = std::fs::remove_dir_all(&dir);
}
