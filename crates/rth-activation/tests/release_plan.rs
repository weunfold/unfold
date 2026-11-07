//! Synthetic launcher fencing and operation supersession.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
const EXE: &str = env!("CARGO_BIN_EXE_rth-activate");
fn base(name: &str) -> PathBuf {
    let b = std::env::temp_dir().join(format!("rth-release-fixture-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&b);
    std::fs::create_dir_all(&b).unwrap();
    std::fs::canonicalize(&b).unwrap()
}
fn sha(b: &[u8]) -> String {
    rth_activation::boot::sha256_hex(b)
}
fn activator() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples/fixture_activator")
}
const PRIOR: &[u8] = b"// fictional launcher\n";
const FENCE: &[u8] = b"// fictional fence\nprocess.exit(3)\n";
struct Fx {
    base: PathBuf,
    data: PathBuf,
    plan: PathBuf,
}
fn fixture(name: &str, op: &str, launcher: Option<&[u8]>) -> Fx {
    let base = base(name);
    let data = base.join("state");
    std::fs::create_dir_all(&data).unwrap();
    let l = base.join("checkout/tools/rth-try.mjs");
    if let Some(bytes) = launcher {
        std::fs::create_dir_all(l.parent().unwrap()).unwrap();
        std::fs::write(&l, bytes).unwrap();
    }
    std::fs::write(base.join("fence.mjs"), FENCE).unwrap();
    let plan = json!({
        "op_id": op, "test_set": [], "instance_dir": "interim-0001",
        "legacy": {"launcher": l, "prior_sha256": sha(PRIOR), "fence": base.join("fence.mjs"), "fence_sha256": sha(FENCE)}
    });
    let p = base.join("plan.json");
    std::fs::write(&p, plan.to_string()).unwrap();
    Fx { base, data, plan: p }
}
fn run(fx: &Fx, rollback: bool) -> (i32, Value) {
    let mut c = Command::new(activator());
    c.args([EXE, fx.data.to_str().unwrap(), fx.plan.to_str().unwrap()]);
    if rollback {
        c.arg("rollback");
    }
    let out = c.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    (out.status.code().unwrap_or(-1), serde_json::from_str(text.trim()).unwrap_or(json!({"raw": text})))
}
fn record(fx: &Fx) -> Value {
    serde_json::from_slice(&std::fs::read(fx.data.join("rth.op.json")).unwrap()).unwrap()
}
fn launcher(fx: &Fx) -> PathBuf {
    fx.base.join("checkout/tools/rth-try.mjs")
}

#[test]
fn an_absent_launcher_is_recorded_absent_and_the_activation_goes_on() {
    let fx = fixture("absent", "op-absent", None);
    let (code, out) = run(&fx, false);
    assert_eq!(code, 0, "{out}");
    let r = record(&fx);
    assert_eq!(r["effects"]["fence"]["outcome"], json!({"absent": true}), "{}", r["effects"]["fence"]);
    assert!(!launcher(&fx).exists(), "nothing was written at the launcher's path");
    assert_eq!(run(&fx, false).0, 0, "a second run re-verifies the absent launcher");
    let (code, out) = run(&fx, true);
    assert_eq!(code, 0, "{out}");
    assert!(!launcher(&fx).exists(), "the rollback restores nothing");
}

#[test]
fn a_launcher_already_fenced_with_no_record_is_adopted_and_a_rollback_leaves_the_fence() {
    let fx = fixture("adopt", "op-adopt", Some(FENCE));
    let (code, out) = run(&fx, false);
    assert_eq!(code, 0, "{out}");
    let r = record(&fx);
    assert_eq!(r["effects"]["fence"]["outcome"], json!({"installed": false, "adopted": true}), "{}", r["effects"]["fence"]);
    assert!(!fx.data.join("backups/op-adopt/legacy").exists(), "no backup: there was no prior to keep");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE);
    let (code, out) = run(&fx, true);
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE, "the fence stays in place");
}

#[test]
fn the_prior_launcher_is_still_fenced_and_restored_as_before() {
    let fx = fixture("prior", "op-prior", Some(PRIOR));
    assert_eq!(run(&fx, false).0, 0);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), FENCE);
    assert_eq!(run(&fx, true).0, 0);
    assert_eq!(std::fs::read(launcher(&fx)).unwrap(), PRIOR);
}

#[test]
fn a_recorded_operation_that_ran_no_effect_is_superseded_by_the_new_plan() {
    let fx = fixture("supersede", "op-new", None);

    let stale = json!({"seq": 0, "op_id": "op-refused-first-run", "plan": {"op_id": "op-refused-first-run", "test_set": [], "instance_dir": "rth-interim"}, "effects": {}});
    std::fs::write(fx.data.join("rth.op.json"), stale.to_string()).unwrap();
    let (code, out) = run(&fx, false);
    assert_eq!(code, 0, "{out}");
    let r = record(&fx);
    assert_eq!(r["op_id"], "op-new");
    assert_eq!(r["supersedes"]["op_id"], "op-refused-first-run");
    let kept: Value = serde_json::from_slice(&std::fs::read(fx.data.join("rth.op.superseded-1.json")).unwrap()).unwrap();
    assert_eq!(kept, stale, "the old record is kept, byte for byte in meaning");

    let fx2 = fixture("ran", "op-new", None);
    let ran = json!({"seq": 1, "op_id": "op-old", "plan": {}, "effects": {"fence": {"intent": {"launcher": "/x"}}}});
    std::fs::write(fx2.data.join("rth.op.json"), ran.to_string()).unwrap();
    let (code, out) = run(&fx2, false);
    assert_eq!(code, 2, "{out}");
    assert!(out["error"].as_str().unwrap().contains("another operation"), "{out}");
}
