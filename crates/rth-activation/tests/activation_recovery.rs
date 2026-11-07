//! Crash, resume, rollback, lease and effect recovery using synthetic fixture roots.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

const EXE: &str = env!("CARGO_BIN_EXE_rth-activate");

fn activator() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples/fixture_activator")
}
struct Fx {
    data: PathBuf,
    plan: PathBuf,
}
const PRIOR_LAUNCHER: &[u8] = b"// fictional prior launcher\n";
const FENCE: &[u8] = b"// fictional fence: RTH moved to the app\nprocess.exit(3)\n";
fn sha(b: &[u8]) -> String {
    rth_activation::boot::sha256_hex(b)
}
fn fixture(name: &str, op: &str) -> Fx {
    let base = std::env::temp_dir().join(format!("rth-activation-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();

    let base = std::fs::canonicalize(&base).unwrap();
    let data = base.join("state");
    let launcher = base.join("checkout/tools/rth-try.mjs");
    std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    std::fs::write(&launcher, PRIOR_LAUNCHER).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(base.join("fence.mjs"), FENCE).unwrap();
    let live = base.join("checkout/shell/dsh-home-bot");
    std::fs::create_dir_all(data.join("agentd-openai")).unwrap();
    std::fs::write(data.join("agentd-openai/state.json"), br#"{"instance_id":"inst-3ede03e7-fictional"}"#).unwrap();
    std::fs::create_dir_all(live.join("sessions/ws/rth-root")).unwrap();
    std::fs::write(live.join("sessions/ws/rth-root/chat.jsonl"), b"{\"text\":\"fictional\"}\n").unwrap();
    let plan = json!({
        "op_id": op,
        "test_set": [
            {"name": "agentd", "path": data.join("agentd-openai")},
            {"name": "session", "path": live.join("sessions/ws/rth-root")},
            {"name": "projcache", "path": live.join("storages/session_projcache")},
            {"name": "omni-ui", "path": live.join("omni-home/ui")},
            {"name": "delegation", "path": live.join("rth")}
        ],
        "instance_dir": "interim-0001",
        "legacy": {"launcher": launcher, "prior_sha256": sha(PRIOR_LAUNCHER), "fence": base.join("fence.mjs"), "fence_sha256": sha(FENCE)},
        "live": {"argv_markers": [marker(name)]}
    });
    let p = base.join("plan.json");
    std::fs::write(&p, plan.to_string()).unwrap();
    Fx { data, plan: p }
}
fn marker(name: &str) -> String {
    format!("rth-fixture-launcher-{}-{name}.mjs", std::process::id())
}
fn edit_plan(fx: &Fx, f: impl FnOnce(&mut Value)) {
    let mut p: Value = serde_json::from_slice(&std::fs::read(&fx.plan).unwrap()).unwrap();
    f(&mut p);
    std::fs::write(&fx.plan, p.to_string()).unwrap();
}
fn launcher(fx: &Fx) -> PathBuf {
    fx.data.parent().unwrap().join("checkout/tools/rth-try.mjs")
}
fn run(fx: &Fx, fault: Option<&str>) -> (i32, Value) {
    let mut c = Command::new(activator());
    c.args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap()]);
    if let Some(f) = fault {
        c.arg(f);
    }
    let out = c.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    (out.status.code().unwrap_or(-1), serde_json::from_str(text.trim()).unwrap_or(json!({"raw": text})))
}
fn count(fx: &Fx, step: &str) -> usize {
    std::fs::read_to_string(fx.data.join(format!("count-{step}"))).map(|s| s.lines().count()).unwrap_or(0)
}
fn activation(fx: &Fx) -> Value {
    serde_json::from_slice(&std::fs::read(fx.data.join("activation.json")).unwrap()).unwrap()
}

#[test]
fn a_clean_activation_captures_the_test_set_and_activates_one_interim_instance() {
    let fx = fixture("clean", "op-clean");
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
    let a = activation(&fx);
    assert_eq!(a["activation_state"], "interim_active");
    assert_eq!(a["instance_id"], "inst-0123456789abcdef");
    assert_eq!(a["brain"]["brain_id"], "brn_fakeint");
    for step in ["mint", "brain", "bind", "verify"] {
        assert_eq!(count(&fx, step), 1, "{step} ran once");
    }
    assert!(!fx.data.join("agentd-openai").exists() && fx.data.join("backups/op-clean/test/agentd/state.json").exists(), "the test set was captured, not deleted");
    let root = fx.data.join("instances/interim-0001");
    assert!(root.join("agentd/state.json").exists() && root.join("agentd/memory-binding-record.json").exists());
    let nonce = std::fs::metadata(root.join("birth.nonce")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(nonce.permissions().mode() & 0o777, 0o600);
    let rec = std::fs::read_to_string(fx.data.join("rth.op.json")).unwrap();
    let nonce_text = std::fs::read_to_string(root.join("birth.nonce")).unwrap();
    assert!(!rec.contains(&nonce_text), "only the nonce's sha256 is recorded");

    let (code, _) = run(&fx, None);
    assert_eq!(code, 0);
    for step in ["mint", "brain", "bind", "verify"] {
        assert_eq!(count(&fx, step), 1, "{step} not re-run");
    }
}

#[test]
fn an_effect_that_ran_but_lost_its_acknowledgement_is_adopted_never_rerun() {
    for step in ["mint", "brain", "bind", "verify"] {
        let fx = fixture(&format!("ran-{step}"), &format!("op-ran-{step}"));
        let (code, _) = run(&fx, Some(&format!("{step}:ran")));
        assert_eq!(code, 99, "{step}: crashed after the effect ran");
        assert_eq!(count(&fx, step), 1);
        let (code, out) = run(&fx, None);
        assert_eq!(code, 0, "{step}: recovered: {out}");
        assert_eq!(activation(&fx)["activation_state"], "interim_active");
        assert_eq!(count(&fx, step), 1, "{step}: adopted from its evidence, never re-run (verify too: D3-R3)");
    }
}

#[test]
fn a_verification_without_its_bound_receipt_stops_and_is_never_rerun() {
    let fx = fixture("verify-lost", "op-verify-lost");
    assert_eq!(run(&fx, Some("verify:ran")).0, 99);
    let receipt = fx.data.join("instances/interim-0001/verify.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();

    v["bound"]["attempt"] = json!("someone-else");
    std::fs::remove_file(&receipt).unwrap();
    std::fs::write(&receipt, v.to_string()).unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("another attempt"), "{out}");
    std::fs::remove_file(&receipt).unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("outcome unknown"), "{out}");
    assert_eq!(count(&fx, "verify"), 1, "never re-run");
    assert_ne!(activation(&fx)["activation_state"], "interim_active");
}

#[test]
fn a_verification_whose_writer_survives_it_is_not_adopted_until_quiescent() {
    let fx = fixture("verify-linger", "op-verify-linger");

    std::fs::write(fx.data.join("verify-linger"), b"").unwrap();
    std::fs::write(fx.data.join("quiesce-within-ms"), b"500").unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("not quiescent"), "{out}");
    assert_ne!(activation(&fx)["activation_state"], "interim_active");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let out = loop {
        let (code, out) = run(&fx, None);
        if code == 0 {
            break out;
        }
        assert!(out["error"].as_str().unwrap().contains("verify"), "{out}");
        assert!(std::time::Instant::now() < end, "still not quiescent: {out}");
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    assert_eq!(out["ok"]["activation_state"], "interim_active");
    assert_eq!(count(&fx, "verify"), 1, "adopted after quiescence, never re-run");
}

#[test]
fn a_verification_whose_writer_survives_it_briefly_is_adopted_once_quiescent_in_one_run() {
    let fx = fixture("verify-linger-short", "op-verify-linger-short");

    std::fs::write(fx.data.join("verify-linger"), b"").unwrap();
    let t = std::time::Instant::now();
    let (code, out) = run(&fx, None);
    let took = t.elapsed();
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["ok"]["activation_state"], "interim_active");
    assert!(took >= std::time::Duration::from_millis(1900), "adopted only after the straggler ended ({took:?})");
    assert_eq!(count(&fx, "verify"), 1, "run once");
    println!("verification a 2 s straggler was waited for: the run took {:.1} s", took.as_secs_f64());
}

#[test]
fn a_helper_recorded_but_never_released_runs_once_on_recovery() {
    let fx = fixture("helper", "op-helper");
    let (code, _) = run(&fx, Some("mint:helper"));
    assert_eq!(code, 99);
    assert_eq!(count(&fx, "mint"), 0, "no go was written: nothing ran");
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(count(&fx, "mint"), 1);
}

#[test]
fn released_without_evidence_stops_as_unknown_and_runs_nothing() {
    let fx = fixture("released", "op-released");
    let (code, _) = run(&fx, Some("brain:released"));
    assert_eq!(code, 99);
    assert_eq!(count(&fx, "brain"), 0);
    let (code, out) = run(&fx, None);
    assert_eq!(code, 1, "stops: {out}");
    assert!(out["error"].as_str().unwrap().contains("brain: outcome unknown"), "{out}");
    assert_eq!(count(&fx, "brain"), 0, "never re-run, never a second brain");
    assert_ne!(activation(&fx)["activation_state"], "interim_active");
}

#[test]
fn a_second_operation_and_a_running_rth_are_refused() {
    let fx = fixture("refuse", "op-first");
    assert_eq!(run(&fx, None).0, 0);
    let other = fx.plan.with_file_name("plan2.json");
    let mut p: Value = serde_json::from_slice(&std::fs::read(&fx.plan).unwrap()).unwrap();
    p["op_id"] = json!("op-second");
    std::fs::write(&other, p.to_string()).unwrap();
    let (code, out) = run(&Fx { data: fx.data.clone(), plan: other }, None);
    assert_eq!(code, 2);
    assert!(out["error"].as_str().unwrap().contains("already active"), "{out}");

    let fx2 = fixture("busy", "op-busy");
    std::fs::create_dir_all(&fx2.data).unwrap();
    let _app = rth_activation::lease::Lease::acquire(&fx2.data.join("rth.op.lock"), rth_activation::lease::Mode::Shared).unwrap();
    let (code, out) = run(&fx2, None);
    assert_eq!(code, 2);
    assert!(out["error"].as_str().unwrap().contains("holds the operation lock"), "{out}");
    assert!(fx2.data.join("agentd-openai/state.json").exists(), "nothing moved");
    drop(_app);

    let _first = rth_activation::lease::Lease::acquire(&fx2.data.join("rth.op.lock"), rth_activation::lease::Mode::Exclusive).unwrap();
    let (code, out) = run(&fx2, None);
    assert_eq!(code, 2);
    assert!(out["error"].as_str().unwrap().contains("holds the operation lock"), "{out}");
    assert_eq!(std::fs::read(launcher(&fx2)).unwrap(), PRIOR_LAUNCHER);
}

#[test]
fn an_interrupted_capture_resumes_root_by_root_from_its_records() {

    for fault in ["capture:released", "capture:moved:agentd"] {
        let fx = fixture(&format!("cap-{}", fault.replace(':', "-")), &format!("op-{}", fault.replace(':', "-")));
        let (code, _) = run(&fx, Some(fault));
        assert_eq!(code, 99, "{fault}");
        let (code, out) = run(&fx, None);
        assert_eq!(code, 0, "{fault}: {out}");
        assert!(fx.data.join(format!("backups/op-{}/test/agentd/state.json", fault.replace(':', "-"))).exists());
        assert!(!fx.data.join("agentd-openai").exists());
    }
}

#[test]
fn a_root_moved_but_not_checkpointed_is_adopted_by_its_recorded_digest() {

    let fx = fixture("cap-gap", "op-cap-gap");
    assert_eq!(run(&fx, Some("capture:released")).0, 99);
    rth_activation::capture::make_backup_dir(&fx.data.join("backups/op-cap-gap/test")).unwrap();
    std::fs::rename(fx.data.join("agentd-openai"), fx.data.join("backups/op-cap-gap/test/agentd")).unwrap();
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_root_changed_after_its_digest_was_recorded_stops_the_capture() {
    let fx = fixture("cap-chg", "op-cap-chg");
    assert_eq!(run(&fx, Some("capture:released")).0, 99);
    std::fs::write(fx.data.join("agentd-openai/state.json"), b"changed").unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0, "{out}");
    assert!(Path::new(&fx.data.join("agentd-openai/state.json")).exists(), "the changed root is not moved");
}

fn rollback(fx: &Fx, fault: Option<&str>) -> (i32, Value) {
    let mut c = Command::new(activator());
    c.args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap(), "rollback"]);
    if let Some(f) = fault {
        c.arg(f);
    }
    let out = c.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    (out.status.code().unwrap_or(-1), serde_json::from_str(text.trim()).unwrap_or(json!({"raw": text})))
}
fn digest(p: &Path) -> String {
    rth_activation::capture::sums(p).unwrap().digest
}

#[test]
fn rollback_preserves_the_interim_set_and_restores_the_test_set_byte_for_byte() {
    let fx = fixture("rollback", "op-rb");
    let before = digest(&fx.data.join("agentd-openai"));
    assert_eq!(run(&fx, None).0, 0);
    let (code, out) = rollback(&fx, None);
    assert_eq!(code, 0, "{out}");
    let a = activation(&fx);
    assert_eq!(a["activation_state"], "test_active");
    assert_eq!(digest(&fx.data.join("agentd-openai")), before, "the test set is back, unchanged");
    let preserved = PathBuf::from(a["preserved"].as_str().unwrap());
    assert!(preserved.join("instance/agentd/memory-binding-record.json").exists(), "the interim instance and its binding are preserved, never deleted");
    assert!(!fx.data.join("instances/interim-0001").exists());
    assert_eq!(count(&fx, "brain"), 1, "the interim brain is untouched (no effect ran)");
    assert_eq!(rollback(&fx, None).0, 0, "rolling back again is a no-op");
}

#[test]
fn rollback_refuses_a_foreign_occupant_before_moving_anything() {
    let fx = fixture("rb-foreign", "op-rb-foreign");
    assert_eq!(run(&fx, None).0, 0);
    std::fs::create_dir_all(fx.data.join("agentd-openai")).unwrap();
    std::fs::write(fx.data.join("agentd-openai/someone.json"), b"foreign").unwrap();
    let (code, out) = rollback(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("occupied"), "{out}");
    assert_eq!(activation(&fx)["activation_state"], "interim_active", "nothing changed");
    assert!(fx.data.join("instances/interim-0001/agentd/state.json").exists());
    assert_eq!(std::fs::read(fx.data.join("agentd-openai/someone.json")).unwrap(), b"foreign");
}

#[test]
fn an_interrupted_rollback_resumes() {
    let fx = fixture("rb-resume", "op-rb-resume");
    let before = digest(&fx.data.join("agentd-openai"));
    assert_eq!(run(&fx, None).0, 0);
    assert_eq!(rollback(&fx, Some("rollback:preserved")).0, 99);
    assert_eq!(activation(&fx)["activation_state"], "interim_preserved");
    let (code, out) = rollback(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(digest(&fx.data.join("agentd-openai")), before);
}

#[test]
fn a_live_swap6_is_refused_before_anything_moves_and_is_never_signalled() {
    let fx = fixture("live", "op-live");
    let lock = fx.data.join("agentd-openai/agent.flock");
    std::fs::write(&lock, b"").unwrap();
    edit_plan(&fx, |p| p["live"]["flocks"] = json!([lock]));

    let holder = std::fs::File::open(&lock).unwrap();
    use std::os::fd::AsRawFd;
    assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("Quit the current RTH to continue"), "{out}");
    assert!(fx.data.join("agentd-openai/state.json").exists(), "nothing moved");
    drop(holder);
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "once it quit: {out}");
}

#[test]
fn the_fence_is_installed_before_the_live_check_and_a_paused_old_launcher_refuses_the_capture() {
    let fx = fixture("paused", "op-paused");

    let mut old = Command::new("/bin/sh").args(["-c", "read x", &marker("paused")]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("Quit the current RTH to continue"), "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER, "a refusal leaves the launcher as it was");
    assert!(fx.data.join("backups/op-paused/legacy/launcher").exists(), "it was fenced first (its backup), then unfenced");
    assert!(fx.data.join("agentd-openai/state.json").exists(), "nothing captured");
    assert!(!fx.data.join("instances").exists());
    drop(old.stdin.take());
    old.wait().unwrap();
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(launcher(&fx)).unwrap().permissions().mode() & 0o777, 0o755, "the prior mode is kept");
}

#[test]
fn a_foreign_launcher_or_a_wrong_fence_refuses_before_anything_changes() {
    let fx = fixture("foreign-launcher", "op-foreign-launcher");
    std::fs::write(launcher(&fx), b"// edited by hand\n").unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("foreign"), "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), b"// edited by hand\n");
    let fx = fixture("wrong-fence", "op-wrong-fence");
    std::fs::write(fx.data.parent().unwrap().join("fence.mjs"), b"// not the pinned fence\n").unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("not the pinned one"), "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER);
    let fx = fixture("no-fence", "op-no-fence");
    edit_plan(&fx, |p| p["legacy"] = Value::Null);
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("fences no legacy launcher"), "{out}");
    assert!(fx.data.join("agentd-openai/state.json").exists());
}

#[test]
fn a_fence_interrupted_after_its_backup_resumes_and_a_fenced_launcher_that_changed_stops() {
    let fx = fixture("fence-crash", "op-fence-crash");
    assert_eq!(run(&fx, Some("fence:backup")).0, 99);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER, "crashed before the rename");
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE);

    std::fs::write(launcher(&fx), PRIOR_LAUNCHER).unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("no longer the fence"), "{out}");
}

#[test]
fn rollback_restores_the_legacy_launcher_last_after_the_test_set() {
    let fx = fixture("rb-launcher", "op-rb-launcher");
    let before = digest(&fx.data.join("agentd-openai"));
    assert_eq!(run(&fx, None).0, 0);
    assert_eq!(rollback(&fx, Some("rollback:restored")).0, 99);
    assert_eq!(digest(&fx.data.join("agentd-openai")), before, "the state is back first");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE, "while the launcher is still fenced");
    let (code, out) = rollback(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(launcher(&fx)).unwrap().permissions().mode() & 0o777, 0o755);
    assert_eq!(activation(&fx)["launcher_restored"], true);
}

#[test]
fn rollback_after_only_the_fence_puts_the_launcher_back() {
    let fx = fixture("rb-fence-only", "op-rb-fence-only");

    assert_eq!(run(&fx, Some("fence:done")).0, 99);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE);
    let (code, out) = rollback(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["ok"]["launcher_restored"], true);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER);
    assert!(fx.data.join("agentd-openai/state.json").exists());
}

#[test]
fn a_plan_name_that_is_not_one_plain_segment_refuses() {
    for (k, bad) in [("instance_dir", "../x"), ("op_id", "a/b"), ("instance_dir", ".hidden"), ("op_id", "")] {
        let fx = fixture("bad-name", "op-bad-name");
        edit_plan(&fx, |p| p[k] = json!(bad));
        let (code, out) = run(&fx, None);
        assert_eq!(code, 2, "{k}={bad:?}: {out}");
        assert!(out["error"].as_str().unwrap().contains("use 1-64 letters"), "{out}");
    }
    let fx = fixture("bad-root-name", "op-bad-root-name");
    edit_plan(&fx, |p| p["test_set"][0]["name"] = json!("../../etc"));
    assert_eq!(run(&fx, None).0, 2);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER, "nothing changed");
}

#[test]
fn a_root_reached_through_a_symlink_or_an_unauthenticated_plan_refuses_before_anything_moves() {
    let fx = fixture("symlink-root", "op-symlink-root");
    let real = fx.data.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(real.join("x")).unwrap();
    let link = fx.data.parent().unwrap().join("checkout/shell/dsh-home-bot/omni-home/ui");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_dir_all(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("foreign"), "{out}");
    assert!(fx.data.join("agentd-openai/state.json").exists(), "nothing moved");
    assert!(real.join("x").exists());
    let bytes = std::fs::read(&fx.plan).unwrap();
    assert!(rth_activation::activation::load_plan(&bytes, &"0".repeat(64)).is_err(), "a plan that is not the authenticated one");
    assert!(rth_activation::activation::load_plan(&bytes, &sha(&bytes)).is_ok());
}

#[test]
fn a_fresh_root_interrupted_while_staged_is_completed_from_its_staging() {
    let fx = fixture("roots-staged", "op-roots-staged");
    assert_eq!(run(&fx, Some("roots:staged")).0, 99);
    assert!(!fx.data.join("instances/interim-0001").exists(), "nothing at the root yet");
    assert!(fx.data.join("instances/interim-0001.op-op-roots-staged/.rth-op").exists());
    let (code, out) = run(&fx, None);
    assert_eq!(code, 0, "{out}");
    assert!(!fx.data.join("instances/interim-0001.op-op-roots-staged").exists());
    assert_eq!(activation(&fx)["activation_state"], "interim_active");
}

#[test]
fn recovery_holds_the_effect_fence_through_reconciliation_and_the_checkpoint() {
    use rth_activation::lease::{Lease, LeaseError, Mode};
    for (name, point) in [("hold-settled", "recover:mint:settled"), ("hold-saved", "recover:mint:saved")] {
        let fx = fixture(name, &format!("op-{name}"));
        assert_eq!(run(&fx, Some("mint:ran")).0, 99, "the mint ran; its acknowledgement is lost");
        let fence = fx.data.join("rth.effect.lock");
        let mut child = Command::new(activator()).args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap(), &format!("pause:{point}")]).stdout(std::process::Stdio::null()).spawn().unwrap();
        let reached = fx.data.join(format!("pause-{point}.reached"));
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !reached.exists() {
            assert!(std::time::Instant::now() < end, "{point} never reached");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(Lease::acquire(&fence, Mode::Exclusive), Err(LeaseError::Busy)), "{point}: the fence is still held");
        std::fs::write(fx.data.join(format!("pause-{point}.go")), b"").unwrap();
        assert!(child.wait().unwrap().success(), "{point}");
        assert!(Lease::acquire(&fence, Mode::Exclusive).is_ok(), "{point}: free once the operation ends");
        assert_eq!(count(&fx, "mint"), 1, "adopted, never re-run");
    }
}

fn wait_for(p: &Path, what: &str) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !p.exists() {
        assert!(std::time::Instant::now() < end, "{what} never reached");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn two_first_run_windows_activating_at_once_run_every_effect_once() {
    let fx = fixture("two-windows", "op-two-windows");
    let spawn = || Command::new(activator()).args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap()]).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let (a, b) = (spawn(), spawn());
    let codes = [a.wait_with_output().unwrap().status.code(), b.wait_with_output().unwrap().status.code()];
    assert!(codes.contains(&Some(0)), "one window activates: {codes:?}");
    assert!(codes.iter().all(|c| *c == Some(0) || *c == Some(2)), "the other is refused (busy) or finds it done: {codes:?}");
    for step in ["mint", "brain", "bind", "verify"] {
        assert_eq!(count(&fx, step), 1, "{step} ran once");
    }
    assert_eq!(activation(&fx)["activation_state"], "interim_active");
}

#[test]
fn both_acquisition_orders_exclude_and_a_paused_app_revalidates_from_the_records() {
    use rth_activation::activation::{admitted, Paths};
    use rth_activation::lease::{Lease, LeaseError, Mode};
    let fx = fixture("orders", "op-orders");
    let lock = fx.data.join("rth.op.lock");
    std::fs::create_dir_all(&fx.data).unwrap();

    let app = Lease::acquire(&lock, Mode::Shared).unwrap();
    let paths = Paths { data: fx.data.clone() };
    let stale = admitted(&paths).unwrap();
    assert_eq!(stale, None, "the app's view before activation: not admitted");
    assert_eq!(run(&fx, None).0, 2);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER);

    drop(app);
    let mut side = Command::new(activator()).args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap(), "pause:before:mint"]).stdout(std::process::Stdio::null()).spawn().unwrap();
    wait_for(&fx.data.join("pause-before:mint.reached"), "before:mint");

    assert!(matches!(Lease::acquire(&lock, Mode::Shared), Err(LeaseError::Busy)));
    std::fs::write(fx.data.join("pause-before:mint.go"), b"").unwrap();
    assert!(side.wait().unwrap().success());

    let _app = Lease::acquire(&lock, Mode::Shared).unwrap();
    let fresh = admitted(&paths).unwrap().expect("admitted now");
    assert_ne!(Some(fresh.clone()), stale);
    assert_eq!(fresh["activation_state"], "interim_active");
}

#[test]
fn an_app_that_dies_while_its_agent_lives_keeps_the_sidecar_out_until_the_agent_ends() {
    let fx = fixture("app-dies", "op-app-dies");
    std::fs::create_dir_all(&fx.data).unwrap();
    let app_bin = activator().with_file_name("fixture_app");
    let out = Command::new(app_bin).args([fx.data.join("rth.op.lock").to_str().unwrap(), "30"]).output().unwrap();
    assert!(out.status.success(), "the app exited");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let (pid, start) = (v["agent_pid"].as_i64().unwrap() as i32, v["agent_start_us"].as_u64().unwrap());
    let (code, out) = run(&fx, None);
    assert_eq!(code, 2, "its agent still holds the inherited shared lease: {out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR_LAUNCHER, "nothing changed");

    assert_eq!(rth_activation::effect::start_us(pid), Some(start), "the recorded agent, not a reused pid");
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (code, out) = run(&fx, None);
        if code == 0 {
            break;
        }
        assert_eq!(code, 2, "{out}");
        assert!(std::time::Instant::now() < end, "the lease never ended with its agent");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(activation(&fx)["activation_state"], "interim_active");
}

#[test]
fn a_sidecar_that_dies_while_its_effect_lives_admits_nothing_until_the_effect_is_quiet() {
    use rth_activation::activation::{admitted, Paths};
    let fx = fixture("side-dies", "op-side-dies");
    std::fs::write(fx.data.join("verify-linger"), b"").unwrap();
    assert_eq!(run(&fx, Some("verify:ran")).0, 99, "the sidecar died after verify's leader, its writer still alive");
    let paths = Paths { data: fx.data.clone() };
    assert_eq!(admitted(&paths).unwrap(), None, "the app admits nothing meanwhile");
    let (code, out) = run(&fx, None);
    assert_ne!(code, 0);
    assert!(out["error"].as_str().unwrap().contains("fence is held"), "{out}");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while run(&fx, None).0 != 0 {
        assert!(std::time::Instant::now() < end, "never quiescent");
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert_eq!(count(&fx, "verify"), 1, "adopted, never re-run");
    assert!(admitted(&paths).unwrap().is_some());
}

#[test]
fn full_admission_holds_the_shared_lease_and_refuses_any_unproven_part() {
    use rth_activation::lease::{Lease, Mode};
    use rth_activation::seam::admit;
    let fx = fixture("admit", "op-admit");
    let service = json!({"label": "fixture-service", "version": "v3.2.2", "plist": "/fixture.plist", "program": "/fixture/.local/share/autopg/v3.2.2/autopg", "runtime_dir": "/fixture/rt", "socket_dir": "/fixture/rt/pgserve", "port": 5999, "pid": 77, "autopg_pid": 78});
    assert!(admit(&fx.data, &service).err().unwrap().contains("not interim_active"), "before activation");
    assert_eq!(run(&fx, None).0, 0);
    let a = admit(&fx.data, &service).expect("admitted (a restarted postmaster is the same service)");
    assert_eq!(a.record["activation_state"], "interim_active");
    let (code, _) = run(&fx, None);
    assert_eq!(code, 2, "the admission's shared lease excludes the sidecar");
    drop(a);
    let mut moved = service.clone();
    moved["port"] = json!(5433);
    assert!(admit(&fx.data, &moved).err().unwrap().contains("memory service changed"));

    let accepted = fx.data.join("instances/interim-0001/memory-service.json");
    let was = std::fs::read(&accepted).unwrap();
    let on = |v: &str, program: &str| {
        let mut s = service.clone();
        s["version"] = json!(v);
        s["program"] = json!(program);
        s
    };
    let up = on("v3.3.1", "/fixture/.local/share/autopg/v3.3.1/autopg");
    drop(admit(&fx.data, &up).expect("v3.2.2 -> v3.3.1, all else equal"));
    let with = |k: &str, v: Value| {
        let mut s = up.clone();
        s[k] = v;
        s
    };
    for (bad, why) in [
        (with("port", json!(5433)), "port"),
        (with("socket_dir", json!("/fixture/other/pgserve")), "socket dir"),
        (with("runtime_dir", json!("/fixture/other")), "runtime dir"),
        (on("v3.2.1", "/fixture/.local/share/autopg/v3.2.1/autopg"), "v3.2.1"),
        (on("v9.9.9", "/fixture/.local/share/autopg/v9.9.9/autopg"), "v9.9.9"),
        (on("v3.3.1", "/elsewhere/.local/share/autopg/v3.3.1/autopg"), "another autopg root"),
        (on("v3.3.1", "/fixture/autopg/v3.3.1/autopg"), "outside .local/share/autopg"),
    ] {
        assert!(admit(&fx.data, &bad).err().unwrap().contains("memory service changed"), "{why}");
    }
    assert_eq!(std::fs::read(&accepted).unwrap(), was, "the accepted record is never rewritten");
    let rec = fx.data.join("instances/interim-0001/agentd/memory-binding-record.json");
    let good = std::fs::read(&rec).unwrap();
    std::fs::write(&rec, br#"{"instance_id":"inst-0123456789abcdef","brain_id":"brn_fakeint","sentinel_ref":"mem_other"}"#).unwrap();
    assert!(admit(&fx.data, &service).err().unwrap().contains("binding record"));
    std::fs::write(&rec, &good).unwrap();
    let st = fx.data.join("instances/interim-0001/agentd/state.json");
    let good_st = std::fs::read(&st).unwrap();
    std::fs::write(&st, br#"{"instance_id":"inst-someone-else"}"#).unwrap();
    assert!(admit(&fx.data, &service).err().unwrap().contains("not the recorded instance"));
    std::fs::write(&st, &good_st).unwrap();
    let x = Lease::acquire(&fx.data.join("rth.op.lock"), Mode::Exclusive).unwrap();
    assert!(admit(&fx.data, &service).err().unwrap().contains("not admitted"), "busy: setup or recovery");
    drop(x);
    assert!(admit(&fx.data, &service).is_ok());
}

#[test]
fn admission_and_status_refuse_linked_dangling_non_regular_or_unreadable_records() {
    use rth_activation::lease::{Lease, Mode};
    use rth_activation::seam::{admit, records_status};
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture("admit-strict", "op-admit-strict");
    let service = json!({"label": "fixture-service", "version": "v3.2.2", "plist": "/fixture.plist", "program": "/fixture/.local/share/autopg/v3.2.2/autopg", "runtime_dir": "/fixture/rt", "socket_dir": "/fixture/rt/pgserve", "port": 5999, "pid": 2, "autopg_pid": 3});
    assert_eq!(records_status(&fx.data)["activation_state"], "test_active", "first run: no records is fresh");
    assert_eq!(run(&fx, None).0, 0);
    assert!(admit(&fx.data, &service).is_ok(), "intact records admit");
    let lock = fx.data.join("rth.op.lock");
    for (name, schedule) in [("activation.json", "link"), ("rth.op.json", "link"), ("rth.op.json", "dangling"), ("activation.json", "directory"), ("activation.json", "unreadable")] {
        let p = fx.data.join(name);
        let aside = fx.data.join(format!("{name}.aside"));
        match schedule {
            "link" => {
                std::fs::rename(&p, &aside).unwrap();
                std::os::unix::fs::symlink(&aside, &p).unwrap();
            }
            "dangling" => {
                std::fs::rename(&p, &aside).unwrap();
                std::os::unix::fs::symlink(fx.data.join("nowhere.json"), &p).unwrap();
            }
            "directory" => {
                std::fs::rename(&p, &aside).unwrap();
                std::fs::create_dir(&p).unwrap();
            }
            "unreadable" => std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap(),
            _ => unreachable!(),
        }
        let before_aside = std::fs::read(&aside).ok();
        let e = admit(&fx.data, &service).err().unwrap_or_else(|| panic!("{name} {schedule}: admitted"));
        assert!(e.contains("not admitted"), "{name} {schedule}: {e}");

        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while Lease::acquire(&lock, Mode::Exclusive).is_err() {
            assert!(std::time::Instant::now() < end, "{name} {schedule}: the shared lease was released");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let st = records_status(&fx.data);
        assert_eq!(st["activation_state"], "unreadable", "{name} {schedule}: {st}");
        assert_eq!(st["admitted"], false);

        let m = std::fs::symlink_metadata(&p).unwrap();
        match schedule {
            "link" | "dangling" => assert!(m.file_type().is_symlink(), "{name} {schedule}"),
            "directory" => assert!(m.is_dir()),
            _ => assert_eq!(m.permissions().mode() & 0o777, 0, "{name} {schedule}"),
        }
        assert_eq!(std::fs::read(&aside).ok(), before_aside, "{name} {schedule}: the record itself untouched");

        match schedule {
            "unreadable" => std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap(),
            "directory" => {
                std::fs::remove_dir(&p).unwrap();
                std::fs::rename(&aside, &p).unwrap();
            }
            _ => {
                std::fs::remove_file(&p).unwrap();
                std::fs::rename(&aside, &p).unwrap();
            }
        }
        assert!(admit(&fx.data, &service).is_ok(), "{name} {schedule}: intact again");
    }
}
