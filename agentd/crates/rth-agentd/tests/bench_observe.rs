//! Device-free receiver durability; no model, effect or real Brain.
#[allow(dead_code)]
#[path = "../src/observe_store.rs"]
mod observe_store;
use observe_store::Receiver;
use rth_protocol::observe::{MemoryReceipt, Observation, Payload, Span, Terminal};
use std::path::PathBuf;
fn path() -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("observe-receiver-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("observe.jsonl")
}
fn user() -> Observation {
    Observation {
        session: "session".into(),
        event_id: "observe-one".into(),
        turn_id: "turn-one".into(),
        response_id: "reply-one".into(),
        version: 1,
        parent: String::new(),
        payload: Payload::User {
            input: "Public question.".into(),
        },
    }
}
#[test]
fn lost_ack_restart_is_one_history_item_and_one_brain_job() {
    let path = path();
    let item = user();
    let mut r = Receiver::open(&path, "session").unwrap();
    let receipt = r.accept(item.clone()).unwrap();
    drop(r);
    let mut r = Receiver::open(&path, "session").unwrap();
    assert_eq!(r.accept(item.clone()).unwrap(), receipt);
    assert_eq!(r.history().len(), 1);
    assert_eq!(r.pending_memory().len(), 1);
    let memory = MemoryReceipt {
        receipt,
        items: vec![("memory-one".into(), 1)],
        outcome: rth_protocol::observe::MemoryOutcome::Stored,
    };
    r.record_memory(memory.clone()).unwrap();
    drop(r);
    let mut r = Receiver::open(&path, "session").unwrap();
    r.accept(item).unwrap();
    assert!(r.pending_memory().is_empty());
    assert_eq!(r.memory("observe-one"), Some(memory));
}
#[test]
fn conflicting_content_session_and_unobserved_assistant_leave_history_unchanged() {
    let mut r = Receiver::open(&path(), "session").unwrap();
    let first = user();
    r.accept(first.clone()).unwrap();
    let mut changed = first.clone();
    changed.payload = Payload::User {
        input: "Different input.".into(),
    };
    assert!(r.accept(changed).is_err());
    let mut assistant = first.clone();
    assistant.version = 2;
    assistant.parent = first.receipt().digest;
    assistant.event_id = "assistant-one".into();
    assistant.turn_id = "unobserved".into();
    assistant.payload = Payload::Assistant {
        spans: vec![Span {
            first: 0,
            end: 800,
            text: "Rendered only.".into(),
        }],
        terminal: Terminal::Interrupted,
    };
    assert!(r.accept(assistant.clone()).is_err());
    assistant.turn_id = first.turn_id;
    assistant.session = "another".into();
    assert!(r.accept(assistant).is_err());
    assert_eq!(r.history().len(), 1);
}
#[test]
fn terminal_renderer_evidence_is_once_and_no_generated_suffix() {
    let mut r = Receiver::open(&path(), "session").unwrap();
    let first = user();
    r.accept(first.clone()).unwrap();
    let mut assistant = first.clone();
    assistant.version = 2;
    assistant.parent = first.receipt().digest;
    assistant.event_id = "assistant-one".into();
    assistant.payload = Payload::Assistant {
        spans: vec![Span {
            first: 0,
            end: 800,
            text: "Rendered only.".into(),
        }],
        terminal: Terminal::Interrupted,
    };
    r.accept(assistant.clone()).unwrap();
    r.accept(assistant.clone()).unwrap();
    assert_eq!(r.history().len(), 2);
    assert_eq!(r.history()[1].payload.text(), "Rendered only.");
    assistant.event_id = "new-id-same-turn".into();
    assistant.version = 3;
    assistant.parent = r.snapshot().tip;
    assert!(r.accept(assistant).is_err());
    assert_eq!(r.history().len(), 2);
}
#[test]
fn torn_tail_recovers_complete_corruption_and_concurrent_writer_refused() {
    use std::io::Write;
    let path = path();
    let mut r = Receiver::open(&path, "session").unwrap();
    r.accept(user()).unwrap();
    assert!(Receiver::open(&path, "session").is_err());
    drop(r);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{torn")
        .unwrap();
    let r = Receiver::open(&path, "session").unwrap();
    assert_eq!(r.history().len(), 1);
    drop(r);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"bad\n")
        .unwrap();
    assert!(Receiver::open(&path, "session").is_err());
}
