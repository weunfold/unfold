use rth_activation::effect::{self, Classification, Identity, Quiescence};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_rth-activate");

fn owner_bin() -> PathBuf {
    std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples/fixture_owner")
}
fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rth-barrier-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}
struct Owner {
    child: Child,
    helper: Identity,
}
fn owner(d: &Path, point: &str, command: &[&str]) -> Owner {
    let mut child = Command::new(owner_bin())
        .args([EXE, d.join("rth.effect.lock").to_str().unwrap(), d.join("op.json").to_str().unwrap(), point, "--"])
        .args(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    Owner { child, helper: serde_json::from_str(line.trim()).unwrap() }
}

fn reached(d: &Path, seq: Option<u64>) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let now = rth_activation::record::read(&d.join("op.json")).ok().flatten().and_then(|v| v["seq"].as_u64());
        if now >= seq {
            return;
        }
        assert!(Instant::now() < end, "the owner did not reach seq {seq:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn kill_owner(mut o: Owner) -> Identity {
    o.child.kill().unwrap();
    o.child.wait().unwrap();
    o.helper
}

fn gone(id: Identity) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while effect::start_us(id.pid) == Some(id.start_us) {
        if Instant::now() > end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}
fn record(d: &Path) -> Option<serde_json::Value> {
    rth_activation::record::read(&d.join("op.json")).unwrap()
}
fn classify(d: &Path) -> Classification {
    match record(d) {
        None => Classification::NotAuthorized,
        Some(r) => effect::classify(r["intent"].as_bool().unwrap_or(false), r["release_intent"].as_bool().unwrap_or(false)),
    }
}

#[test]
fn a_owner_dies_after_the_helper_starts_before_any_record_no_effect() {
    let d = dir("a");
    let marker = d.join("marker");
    let o = owner(&d, "started", &["/usr/bin/touch", marker.to_str().unwrap()]);
    std::thread::sleep(Duration::from_millis(200));
    let h = kill_owner(o);
    assert!(gone(h), "the helper exits on EOF of the real pipe");
    assert!(!marker.exists(), "no effect ran");
    assert_eq!(classify(&d), Classification::NotAuthorized);
}

#[test]
fn b_owner_dies_after_recording_before_release_no_effect_classified_not_run() {
    let d = dir("b");
    let marker = d.join("marker");
    let o = owner(&d, "intent", &["/usr/bin/touch", marker.to_str().unwrap()]);
    reached(&d, Some(1));
    let h = kill_owner(o);
    assert!(gone(h));
    assert!(!marker.exists(), "EOF without go: the effect did not run");
    assert_eq!(classify(&d), Classification::NotRun);

    let end = Instant::now() + Duration::from_secs(10);
    while !matches!(effect::quiesce(&d.join("rth.effect.lock"), Some(h.pgid)).unwrap(), Quiescence::Quiet(_)) {
        assert!(Instant::now() < end, "the group empties once the helper is reaped");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn i5r2_a_completed_effect_whose_checkpoint_was_lost_is_adopted_never_rerun() {
    let d = dir("c");
    let marker = d.join("marker");
    let o = owner(&d, "released", &["/bin/sh", "-c", &format!("echo once >> {}", marker.display())]);
    reached(&d, Some(2));

    let end = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(20));
    }
    let h = kill_owner(o);
    assert!(gone(h));
    assert_eq!(classify(&d), Classification::MayHaveRun, "release_intent recorded: may have run, whatever followed");
    let end = Instant::now() + Duration::from_secs(10);
    let _fence = loop {
        match effect::quiesce(&d.join("rth.effect.lock"), Some(h.pgid)).unwrap() {
            Quiescence::Quiet(g) => break g,
            _ => assert!(Instant::now() < end, "a finished effect leaves the fence free and the group empty"),
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "once\n");
}

#[test]
fn d_a_descendant_holding_the_fence_refuses_takeover_until_it_ends() {
    let d = dir("d");
    let pidfile = d.join("descendant.pid");
    let o = owner(&d, "done", &["/bin/sh", "-c", &format!("/bin/sleep 30 & echo $! > {}", pidfile.display())]);
    reached(&d, Some(3));
    let h = kill_owner(o);
    let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    let start = effect::start_us(pid).expect("the descendant runs");
    assert!(matches!(effect::quiesce(&d.join("rth.effect.lock"), Some(h.pgid)).unwrap(), Quiescence::FenceBusy), "the leader exited but its descendant still holds the fence");

    assert_eq!(effect::start_us(pid), Some(start));
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match effect::quiesce(&d.join("rth.effect.lock"), Some(h.pgid)).unwrap() {
            Quiescence::Quiet(_) => break,
            _ => assert!(Instant::now() < end, "the fence frees once the descendant is gone"),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_pending_helper_cannot_run_while_recovery_holds_the_fence() {

    let d = dir("e");
    let fence = d.join("rth.effect.lock");
    let marker = d.join("marker");
    let Quiescence::Quiet(guard) = effect::quiesce(&fence, None).unwrap() else { panic!() };
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let p = effect::start(Path::new(EXE), &fence, &["/usr/bin/touch".to_string(), marker.display().to_string()], &env, Path::new("/"), Stdio::null()).unwrap();
    let mut child = p.go().unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(!marker.exists(), "no effect while the fence is held");
    drop(guard);
    let status = child.wait().unwrap();
    assert!(status.success() && marker.exists(), "it runs once the fence is free");
}
