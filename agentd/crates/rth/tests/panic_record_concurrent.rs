

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn panic_rows_stay_whole_lines_beside_a_concurrent_journal_writer() {
    let dir = std::env::temp_dir().join(format!("rth-panic-concurrent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("host")).unwrap();
    let journal = dir.join("rth-host.jsonl");
    std::fs::write(&journal, b"").unwrap();

    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let planted = info.payload().downcast_ref::<String>().is_some_and(|m| m.starts_with("assertion failed: state == REGISTERING ("));
        if !planted {
            default(info);
        }
    }));
    rth::crash::record_panics(Some(journal.clone()), dir.join("host/last-panic.json"), "host-concurrent".into());

    let done = Arc::new(AtomicBool::new(false));
    let writer = {
        let (journal, done) = (journal.clone(), done.clone());
        std::thread::spawn(move || {
            let mut f = std::fs::OpenOptions::new().append(true).open(journal).unwrap();
            let mut i = 0u64;
            while !done.load(Ordering::Relaxed) || i < 2_000 {
                f.write_all(format!("{{\"kind\":\"ordinary\",\"seq\":{i},\"pad\":\"{}\"}}\n", "x".repeat(64)).as_bytes()).unwrap();
                i += 1;
            }
            i
        })
    };
    const PANICS: usize = 40;
    for n in 0..PANICS {

        let _ = std::thread::spawn(move || panic!("assertion failed: state == REGISTERING ({n}) {}", "m".repeat(300))).join();
    }
    done.store(true, Ordering::Relaxed);
    let ordinary = writer.join().unwrap();

    let text = std::fs::read_to_string(&journal).unwrap();
    let rows: Vec<Result<serde_json::Value, _>> = text.lines().map(serde_json::from_str::<serde_json::Value>).collect();
    let malformed = rows.iter().filter(|r| r.is_err()).count();
    let panics: Vec<&serde_json::Value> = rows.iter().filter_map(|r| r.as_ref().ok()).filter(|v| v["kind"] == "host_panic").collect();
    let ordinary_rows = rows.iter().filter_map(|r| r.as_ref().ok()).filter(|v| v["kind"] == "ordinary").count();
    assert_eq!(malformed, 0, "every line parses ({} lines, {PANICS} panics, {ordinary} ordinary rows written)", rows.len());
    assert_eq!(panics.len(), PANICS, "every panic left one parseable host_panic row");
    assert_eq!(ordinary_rows as u64, ordinary, "no ordinary row was damaged");
    assert!(panics.iter().all(|p| p["message"].as_str().unwrap().starts_with("assertion failed: state == REGISTERING (")), "{:?}", panics[0]);
    let _ = std::fs::remove_dir_all(&dir);
}
