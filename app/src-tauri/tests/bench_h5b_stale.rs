#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
#[test]
fn restart_must_not_authorize_speech_for_a_stale_unsent_turn() {
    use conversation_log::{Owner, Store};
    let target = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()));
    let dir = target.join(format!("h5b-stale-base-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log");
    let mut store = Store::open(&path, "session").unwrap();
    let turn = store.commit("stale", "An old public request.").unwrap();
    store.select_owner(&turn.id, Owner::Legacy).unwrap();
    drop(store);
    let mut store = Store::open(&path, "session").unwrap();
    assert!(
        store.begin_legacy_send(&turn.id).is_err(),
        "restart must not authorize speech for old unsent work"
    );
}
