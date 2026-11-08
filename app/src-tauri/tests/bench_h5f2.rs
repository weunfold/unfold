
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
use conversation_log::{Owner, SilentWindow, Store, Turn, SILENT_BATCH};
use std::sync::atomic::{AtomicU64, Ordering};
fn path() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir =
        std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string())).join(format!(
            "f2-window-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("log")
}
fn cancel(s: &mut Store, nonce: &str) -> Turn {
    let t = s.commit(nonce, "public cancelled input").unwrap();
    s.select_owner(&t.id, Owner::Front).unwrap();
    s.cancel_before_render(&t.id).unwrap();
    t
}
fn accepted(s: &mut Store, nonce: &str) {
    let t = s.commit(nonce, "public legacy input").unwrap();
    s.select_owner(&t.id, Owner::Legacy).unwrap();
    assert!(s.begin_legacy_send(&t.id).unwrap());
    s.accept_legacy(&t.id, &format!("backend-{nonce}")).unwrap();
}
#[test]
fn silent_cancel_window_bounds_each_drain_and_never_repeats_released_ids() {
    let mut s = Store::open(&path(), "session").unwrap();
    let ids: Vec<_> = (0..100)
        .map(|i| cancel(&mut s, &format!("cancel-{i}")).id)
        .collect();
    accepted(&mut s, "release");
    let mut window = SilentWindow::default();
    let mut got = vec![];
    for _ in 0..8 {
        let batch = s.ready_silent_cancels(&mut window);
        assert!(batch.inspected <= SILENT_BATCH);
        assert!(batch.turns.len() <= SILENT_BATCH);
        got.extend(batch.turns.into_iter().map(|t| t.id));
        if !batch.more {
            break;
        }
    }
    assert_eq!(got, ids);
    let batch = s.ready_silent_cancels(&mut window);
    assert!(batch.turns.is_empty());
    assert_eq!(batch.inspected, 0);
    accepted(&mut s, "later-terminal");
    assert!(s.ready_silent_cancels(&mut window).turns.is_empty());
}
#[test]
fn silent_cancel_window_waits_at_unresolved_and_unreleased_turns() {
    let mut s = Store::open(&path(), "session").unwrap();
    let first = s.commit("unselected", "public unresolved input").unwrap();
    let second = cancel(&mut s, "second");
    let mut window = SilentWindow::default();
    let blocked = s.ready_silent_cancels(&mut window);
    assert!(blocked.turns.is_empty());
    assert!(!blocked.more);
    s.cancel_before_render(&first.id).unwrap();
    assert!(s.ready_silent_cancels(&mut window).turns.is_empty());
    accepted(&mut s, "release");
    assert_eq!(
        s.ready_silent_cancels(&mut window)
            .turns
            .iter()
            .map(|t| &t.id)
            .collect::<Vec<_>>(),
        [&first.id, &second.id]
    );
}
#[test]
fn silent_cancel_window_bounds_skipping_legacy_history_and_survives_compaction() {
    let mut s = Store::open(&path(), "session").unwrap();
    for i in 0..100 {
        accepted(&mut s, &format!("legacy-{i}"));
    }
    let t = cancel(&mut s, "cancel");
    accepted(&mut s, "release");
    let mut window = SilentWindow::default();
    let mut visited = 0;
    let mut ids = vec![];
    for i in 0..8 {
        if i == 2 {
            s.compact().unwrap();
        }
        let batch = s.ready_silent_cancels(&mut window);
        assert!(batch.inspected <= SILENT_BATCH);
        visited += batch.inspected;
        ids.extend(batch.turns.into_iter().map(|t| t.id));
        if !batch.more {
            break;
        }
    }
    assert_eq!(visited, 102);
    assert_eq!(ids, [t.id]);
    assert_eq!(s.ready_silent_cancels(&mut window).inspected, 0);
}
#[test]
fn silent_cancel_window_rebinds_on_reopen_and_separate_store() {
    let p = path();
    let mut s = Store::open(&p, "session").unwrap();
    let t = cancel(&mut s, "cancel");
    accepted(&mut s, "release");
    let mut window = SilentWindow::default();
    assert_eq!(s.ready_silent_cancels(&mut window).turns[0].id, t.id);
    drop(s);
    let s = Store::open(&p, "session").unwrap();
    assert_eq!(s.ready_silent_cancels(&mut window).turns[0].id, t.id);
    let mut other = Store::open(&path(), "session").unwrap();
    let other_t = cancel(&mut other, "other");
    accepted(&mut other, "release");
    assert_eq!(
        other.ready_silent_cancels(&mut window).turns[0].id,
        other_t.id
    );
}
