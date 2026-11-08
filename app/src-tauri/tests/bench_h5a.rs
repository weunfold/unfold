
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
use conversation_log::{Owner, Store};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn path() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()));
    let dir = root.join(format!("h5a-fixture-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("conversation.jsonl")
}

#[test]
fn crash_or_ack_loss_reuses_the_canonical_user_turn() {
    let path = path();
    let mut store = Store::open(&path, "session-a").unwrap();
    let first = store.commit("input-1", "A public request.").unwrap();
    store.select_owner(&first.id, Owner::Front).unwrap();
    store.first_rendered(&first.id).unwrap();
    let pending = store.pending_observations();
    assert_eq!(pending.len(), 1);
    drop(store);
    let mut recovered = Store::open(&path, "session-a").unwrap();
    assert_eq!(recovered.commit("input-1", "A public request.").unwrap(), recovered.turns()[0]);
    assert_eq!(recovered.turns().len(), 1);
    assert_eq!(recovered.pending_observations(), pending, "stable replay identity and payload");
    recovered.ack_observation(&first.id).unwrap();
    recovered.ack_observation(&first.id).unwrap();
    drop(recovered);
    let mut recovered = Store::open(&path, "session-a").unwrap();
    assert!(recovered.pending_observations().is_empty());
    assert_eq!(recovered.commit("input-1", "A public request.").unwrap().id, first.id);
    assert_eq!(recovered.turns().len(), 1);
    assert!(recovered.commit("input-1", "Different content.").is_err());
}

#[test]
fn local_commit_and_render_do_not_need_an_agentd_or_mirror_ack() {
    let mut store = Store::open(&path(), "session-b").unwrap();
    let turn = store.commit("input-2", "A short local answer request.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    assert!(store.pending_observations().is_empty(), "Observe is held until first render");
    store.first_rendered(&turn.id).unwrap();
    assert_eq!(store.pending_observations().len(), 1);
    assert!(!store.turns()[0].acknowledged);
}

#[test]
fn all_committed_turns_replay_and_owner_selection_survives_restart() {
    let path = path();
    let mut store = Store::open(&path, "session-c").unwrap();
    let mut ids = Vec::new();
    for index in 0..32 {
        let turn = store.commit(&format!("nonce-{index}"), &format!("Public input {index}.")).unwrap();
        let owner = if index % 2 == 0 { Owner::Front } else { Owner::Legacy };
        store.select_owner(&turn.id, owner).unwrap();
        if owner == Owner::Front { store.first_rendered(&turn.id).unwrap(); }
        ids.push((turn.id, owner));
    }
    drop(store);
    let store = Store::open(&path, "session-c").unwrap();
    assert_eq!(store.turns().len(), ids.len(), "100% committed replay coverage");
    for (turn, (id, owner)) in store.turns().iter().zip(ids) {
        assert_eq!((&turn.id, turn.owner), (&id, Some(owner)));
    }
    assert_eq!(store.pending_observations().len(), 16);
}

#[test]
fn legacy_send_is_attempted_once_and_never_changes_owner_afterward() {
    let path = path();
    let mut store = Store::open(&path, "session-d").unwrap();
    let turn = store.commit("legacy-1", "A request for the existing route.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    store.select_owner(&turn.id, Owner::Legacy).unwrap();
    assert!(store.select_owner(&turn.id, Owner::Front).is_err(), "Legacy never becomes Front, even before sending");
    assert!(store.begin_legacy_send(&turn.id).unwrap());
    drop(store);
    let mut store = Store::open(&path, "session-d").unwrap();
    assert!(!store.begin_legacy_send(&turn.id).unwrap(), "uncertain send requires lookup, never blind resend");
    assert!(store.select_owner(&turn.id, Owner::Front).is_err());
    assert!(store.first_rendered(&turn.id).is_err());
    assert!(store.pending_observations().is_empty(), "legacy must not also Observe");
    store.accept_legacy(&turn.id, "backend-response-1").unwrap();
    store.accept_legacy(&turn.id, "backend-response-1").unwrap();
    assert!(store.accept_legacy(&turn.id, "different-response").is_err());
}

#[test]
fn rendered_front_cannot_switch_to_legacy_even_before_ack() {
    let mut store = Store::open(&path(), "session-e").unwrap();
    let turn = store.commit("front-1", "A public input.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    store.first_rendered(&turn.id).unwrap();
    assert!(store.select_owner(&turn.id, Owner::Legacy).is_err());
    assert!(store.begin_legacy_send(&turn.id).is_err());
}

#[test]
fn torn_tail_is_recovered_but_complete_corruption_is_not_hidden() {
    use std::io::Write;
    let path = path();
    let mut store = Store::open(&path, "session-f").unwrap();
    let turn = store.commit("persisted-1", "Durable public input.").unwrap();
    drop(store);
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{\"incomplete").unwrap();
    let mut recovered = Store::open(&path, "session-f").unwrap();
    assert_eq!(recovered.turns()[0].id, turn.id);
    recovered.commit("persisted-2", "Another public input.").unwrap();
    drop(recovered);
    let reopened = Store::open(&path, "session-f").unwrap();
    assert_eq!(reopened.turns().len(), 2, "recovered tail was truncated before the next append");
    drop(reopened);
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"not-json\n").unwrap();
    assert!(Store::open(&path, "session-f").is_err());
}

#[test]
fn conflicting_session_concurrent_writer_and_linked_paths_are_refused() {
    let path = path();
    let store = Store::open(&path, "session-g").unwrap();
    assert!(Store::open(&path, "session-g").is_err());
    drop(store);
    assert!(Store::open(&path, "another-session").is_err());
    let link = path.with_extension("linked");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(Store::open(&link, "session-g").is_err());
    let hard = path.with_extension("hard");
    std::fs::hard_link(&path, &hard).unwrap();
    assert!(Store::open(&hard, "session-g").is_err());
}

#[test]
fn pending_capacity_never_discards_a_committed_input() {
    let path = path();
    let mut store = Store::open(&path, "session-h").unwrap();
    for index in 0..64 { store.commit(&format!("bound-{index}"), "Public input.").unwrap(); }
    assert_eq!(store.commit("bound-0", "Public input.").unwrap().nonce, "bound-0", "retry still deduplicates at capacity");
    assert!(store.commit("over-limit", "Must not silently drop.").is_err());
    drop(store);
    assert_eq!(Store::open(&path, "session-h").unwrap().turns().len(), 64);
}

#[test]
fn complete_record_tampering_is_detected_before_replay() {
    let path = path();
    let mut store = Store::open(&path, "session-i").unwrap();
    store.commit("input-i", "Public original input.").unwrap();
    drop(store);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("original"));
    std::fs::write(&path, text.replace("original", "modified")).unwrap();
    assert!(Store::open(&path, "session-i").is_err());
}

#[test]
fn crash_writer_child() {
    let Some(path) = std::env::var_os("H5A_CRASH_LOG") else { return };
    let mut store = Store::open(std::path::Path::new(&path), "crash-session").unwrap();
    let turn = store.commit("crash-nonce", "Committed before process exit.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    store.first_rendered(&turn.id).unwrap();
    std::process::exit(0);
}

#[test]
fn process_exit_without_destructors_preserves_committed_replay() {
    let path = path();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_writer_child", "--test-threads", "1"])
        .env("H5A_CRASH_LOG", &path).status().unwrap();
    assert!(status.success());
    let mut recovered = Store::open(&path, "crash-session").unwrap();
    assert_eq!(recovered.turns().len(), 1);
    assert_eq!(recovered.pending_observations().len(), 1);
    recovered.commit("crash-nonce", "Committed before process exit.").unwrap();
    assert_eq!(recovered.turns().len(), 1);
}

#[test]
fn recovered_pre_render_front_is_uncertain_and_cannot_blindly_fall_back() {
    let path = path();
    let mut store = Store::open(&path, "presentation-session").unwrap();
    let turn = store.commit("presentation-nonce", "A committed public input.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    drop(store);
    let mut recovered = Store::open(&path, "presentation-session").unwrap();
    assert!(recovered.select_owner(&turn.id, Owner::Legacy).is_err(), "absence of a render record is not proof of silence after restart");
    assert!(recovered.first_rendered(&turn.id).is_err(), "recovery cannot invent a rendered sample");
    assert_eq!(recovered.turns().len(), 1, "committed input is preserved for reconciliation");
}

#[test]
fn insecure_mode_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let path = path(); drop(Store::open(&path, "mode-session").unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Store::open(&path, "mode-session").is_err());
}
