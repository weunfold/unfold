

use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_rth-agentd");

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentd-launch-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn agent(cwd: &Path) -> Command {
    let mut c = Command::new(BIN);
    c.env_clear().env("PATH", "/usr/bin:/bin").current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c
}

fn finish(mut c: Command) -> Output {
    let mut child = c.spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("the launch did not return: it started when it should have been refused");
}
fn entries(d: &Path) -> Vec<String> {
    std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()
}
fn bound(port: u16) -> bool {
    TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_millis(200)).is_ok()
}

fn contract_binary() {

    static PROBES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cwd = tmp(&format!("probe-{}", PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let mut c = agent(&cwd);
    c.arg("--help");
    let out = finish(c);
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("launch contract"), "not a launch-contract binary: {BIN}");
}
fn desktop(c: &mut Command, root: &Path, bind: &str) {
    c.env("RTH_INSTANCE_ROOT", root).env("RTH_STATE", root.join("agentd-state/state.json")).env("RTH_JOURNAL", root.join("agentd-state/journal.jsonl")).env("RTH_AGENT_BIND", bind).env("RTH_BACKEND", "fake");
}
fn refused_without_effects(name: &str, setup: impl FnOnce(&mut Command, &Path, u16), needle: &str) {
    contract_binary();
    let cwd = tmp(&format!("{name}-cwd"));
    let root = tmp(&format!("{name}-root"));
    let port = free_port();
    let mut c = agent(&cwd);
    setup(&mut c, &root, port);
    let out = finish(c);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{name}: refused with exit 2 ({err})");
    assert!(err.contains(needle), "{name}: the refusal names {needle:?}: {err}");
    assert!(entries(&root).is_empty() && entries(&cwd).is_empty(), "{name}: nothing written ({:?} {:?})", entries(&root), entries(&cwd));
    assert!(!bound(port), "{name}: nothing bound on {port}");
}

#[test]
fn help_and_version_return_at_once_and_start_nothing() {
    for arg in ["--help", "-h", "--version", "-V"] {
        let cwd = tmp(&format!("hv{}", arg.trim_start_matches('-')));
        let mut c = agent(&cwd);
        c.arg(arg);
        let out = finish(c);
        assert!(out.status.success(), "{arg}");
        assert!(!out.stdout.is_empty(), "{arg} prints");
        assert!(entries(&cwd).is_empty(), "{arg} writes nothing");
    }
}

#[test]
fn unknown_arguments_are_refused_even_with_an_admitted_environment() {
    refused_without_effects("unknown", |c, root, port| { desktop(c, root, &format!("127.0.0.1:{port}")); c.arg("--bind").arg("0.0.0.0:7700"); }, "unknown arguments");
}

#[test]
fn an_empty_environment_is_refused_never_defaulting_to_rth_or_port_7700() {
    refused_without_effects("empty", |_, _, _| {}, "RTH_INSTANCE_ROOT is required");
}

#[test]
fn missing_and_malformed_configuration_is_refused() {
    refused_without_effects("no-bind", |c, root, _| { desktop(c, root, ""); }, "RTH_AGENT_BIND is required");
    refused_without_effects("no-journal", |c, root, port| { desktop(c, root, &format!("127.0.0.1:{port}")); c.env_remove("RTH_JOURNAL"); }, "RTH_JOURNAL is required");
    refused_without_effects("relative", |c, root, port| { desktop(c, root, &format!("127.0.0.1:{port}")); c.env("RTH_STATE", "state.json"); }, "absolute");
    refused_without_effects("outside", |c, root, port| {
        desktop(c, root, &format!("127.0.0.1:{port}"));
        c.env("RTH_JOURNAL", root.parent().unwrap().join(format!("agentd-launch-it-{}-outside.jsonl", std::process::id())));
    }, "not inside");
    refused_without_effects("bad-mode", |c, root, port| { desktop(c, root, &format!("127.0.0.1:{port}")); c.env("RTH_AGENT_MODE", "server"); }, "desktop or box");

    refused_without_effects("bad-decide", |c, root, port| { desktop(c, root, &format!("127.0.0.1:{port}")); c.env("RTH_MEMORY_DECIDE", "sometimes"); }, "RTH_MEMORY_DECIDE");
}

#[test]
fn wildcard_and_non_loopback_binds_are_refused() {
    refused_without_effects("wildcard4", |c, root, port| desktop(c, root, &format!("0.0.0.0:{port}")), "not a loopback");
    refused_without_effects("wildcard6", |c, root, port| desktop(c, root, &format!("[::]:{port}")), "not a loopback");
    refused_without_effects("lan", |c, root, port| desktop(c, root, &format!("192.168.1.5:{port}")), "not a loopback");
    refused_without_effects("name", |c, root, port| desktop(c, root, &format!("localhost:{port}")), "literal address");
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_admitted_desktop_launch_starts_on_its_explicit_loopback_bind_and_paths() {
    contract_binary();
    let cwd = tmp("ok-cwd");
    let root = tmp("ok-root");
    std::fs::create_dir_all(root.join("agentd-state")).unwrap();
    let port = free_port();
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{port}"));
    c.stdout(Stdio::null()).stderr(Stdio::null());
    let _agent = Running(c.spawn().unwrap());
    let end = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            break s;
        }
        assert!(Instant::now() < end, "the admitted launch listens on 127.0.0.1:{port}");
        std::thread::sleep(Duration::from_millis(50));
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut hello = String::new();
    BufReader::new(stream).read_line(&mut hello).unwrap();
    assert!(hello.contains("\"kind\":\"hello\""), "its Hello: {hello}");
    assert!(root.join("agentd-state/state.json").exists() && root.join("agentd-state/journal.jsonl").exists(), "state and journal at the explicit paths");
    assert!(entries(&cwd).is_empty(), "nothing written to the working directory");
}

fn sha(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap()
}

fn refused(mut c: Command) -> String {
    let out = finish({
        c.stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    });
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(2), "refused with exit 2: {err}");
    err
}

#[test]
fn ad_r1_a_parent_symlink_out_of_the_root_writes_nothing_outside() {
    contract_binary();
    let cwd = tmp("r1p-cwd");
    let root = tmp("r1p-root");
    let out = tmp("r1p-outside");
    std::os::unix::fs::symlink(&out, root.join("alias")).unwrap();
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{}", free_port()));
    c.env("RTH_STATE", root.join("alias/state.json")).env("RTH_JOURNAL", root.join("alias/journal.jsonl"));
    let err = refused(c);
    assert!(err.contains("symlink"), "{err}");
    assert!(entries(&out).is_empty(), "nothing written outside: {:?}", entries(&out));
}

#[test]
fn ad_r1_a_journal_leaf_symlink_leaves_the_foreign_file_unchanged() {
    contract_binary();
    let cwd = tmp("r1l-cwd");
    let root = tmp("r1l-root");
    let out = tmp("r1l-outside");
    std::fs::create_dir_all(root.join("agentd-state")).unwrap();
    let foreign = out.join("canary.jsonl");
    std::fs::write(&foreign, b"{\"fictional\":\"canary\"}\n{\"torn").unwrap();
    let before = sha(&foreign);
    std::os::unix::fs::symlink(&foreign, root.join("agentd-state/journal.jsonl")).unwrap();
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{}", free_port()));
    let err = refused(c);
    assert!(err.contains("symlink"), "{err}");
    assert_eq!(sha(&foreign), before, "the foreign file is unchanged (no repair, no truncation)");
    assert!(!root.join("agentd-state/state.json").exists(), "no identity was written");
}

#[test]
fn ad_r2_one_file_for_two_roles_is_refused_with_nothing_touched() {
    contract_binary();
    let cwd = tmp("r2s-cwd");
    let root = tmp("r2s-root");
    std::fs::create_dir_all(root.join("agentd-state")).unwrap();
    let same = root.join("agentd-state/same.json");
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{}", free_port()));
    c.env("RTH_STATE", &same).env("RTH_JOURNAL", &same);
    assert!(refused(c).contains("same file"));
    assert!(!same.exists(), "nothing created");

    let state = root.join("agentd-state/state.json");
    std::fs::write(&state, b"{\"fictional\":true}").unwrap();
    std::fs::hard_link(&state, root.join("agentd-state/journal.jsonl")).unwrap();
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{}", free_port()));
    assert!(refused(c).contains("same file"));
    assert_eq!(std::fs::read(&state).unwrap(), b"{\"fictional\":true}", "state untouched");
}

#[test]
fn ad_r2_separate_paths_boot_and_restart() {
    contract_binary();
    let cwd = tmp("r2r-cwd");
    let root = tmp("r2r-root");
    std::fs::create_dir_all(root.join("agentd-state")).unwrap();
    for boot in 0..2 {
        let port = free_port();
        let mut c = agent(&cwd);
        desktop(&mut c, &root, &format!("127.0.0.1:{port}"));
        c.stdout(Stdio::null()).stderr(Stdio::null());
        let _agent = Running(c.spawn().unwrap());
        let end = Instant::now() + Duration::from_secs(10);
        let stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            assert!(Instant::now() < end, "boot {boot} listens");
            std::thread::sleep(Duration::from_millis(50));
        };
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut hello = String::new();
        BufReader::new(stream).read_line(&mut hello).unwrap();
        assert!(hello.contains("\"kind\":\"hello\""), "boot {boot}: {hello}");
    }
    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("agentd-state/state.json")).unwrap()).unwrap();
    assert!(state["instance_id"].as_str().is_some(), "one valid instance across both boots");
}

fn alive(pid: &str) -> bool {
    Command::new("/bin/kill").args(["-0", pid]).stderr(Stdio::null()).status().unwrap().success()
}

#[test]
fn sigterm_stops_brain_mcp_and_exits_cleanly() {
    contract_binary();
    let cwd = tmp("term-cwd");
    let root = tmp("term-root");
    std::fs::create_dir_all(root.join("agentd-state")).unwrap();
    let home = root.join("brain-home");
    std::fs::create_dir_all(&home).unwrap();
    let fake = root.join("fake_brain_mcp.py");
    std::fs::write(&fake, include_str!("../../rth-memory-brain/tests/fake_brain_mcp.py")).unwrap();
    let mcp = root.join("brain-mcp");

    std::fs::write(&mcp, format!("#!/bin/sh\necho $$ > \"$HOME/brain-mcp.pid\"\nexec /usr/bin/python3 -c 'import runpy, sys, time\ntry:\n    runpy.run_path(sys.argv[1], run_name=\"__main__\")\nfinally:\n    time.sleep(30)' {}\n", fake.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&mcp, std::fs::Permissions::from_mode(0o700)).unwrap();
    let port = free_port();
    let mut c = agent(&cwd);
    desktop(&mut c, &root, &format!("127.0.0.1:{port}"));
    c.env("RTH_MEMORY", "1").env("RTH_MEMORY_PROVIDER", "brain").env("RTH_MEMORY_BRAIN_ID", "brn-fake").env("RTH_MEMORY_BRAIN_MCP", &mcp).env("RTH_MEMORY_BRAIN_HOME", &home).env("RTH_MEMORY_BRAIN_DATABASE", "fake-only").env("RTH_MEMORY_BRAIN_SANDBOX", "0");
    c.stdout(Stdio::null()).stderr(Stdio::null());
    let mut agentd = Running(c.spawn().unwrap());
    let pid_file = home.join("brain-mcp.pid");
    let end = Instant::now() + Duration::from_secs(15);
    while !(bound(port) && pid_file.exists()) {
        assert!(Instant::now() < end, "agentd listens and brain-mcp started (pid file {})", pid_file.display());
        std::thread::sleep(Duration::from_millis(50));
    }
    let mcp_pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
    assert!(alive(&mcp_pid), "brain-mcp runs");
    assert!(Command::new("/bin/kill").args(["-TERM", &agentd.0.id().to_string()]).status().unwrap().success());
    let sent = Instant::now();
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = agentd.0.try_wait().unwrap() {
            break Some(s);
        }
        if Instant::now() >= end {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let took = sent.elapsed();
    let survived = alive(&mcp_pid);
    if survived {
        let _ = Command::new("/bin/kill").args(["-KILL", &mcp_pid]).status();
    }
    assert!(!survived, "brain-mcp {mcp_pid} outlived agentd");
    let status = status.expect("agentd exits within 10 s of SIGTERM");
    assert_eq!(status.code(), Some(0), "a clean exit: {status:?}");

    assert!(took < Duration::from_secs(3), "agentd and brain-mcp gone {took:?} after SIGTERM");
    let journal = std::fs::read_to_string(root.join("agentd-state/journal.jsonl")).unwrap();
    assert!(journal.contains("agentd stopping: SIGTERM"), "the journal says why the session ended");
    drop(agentd);
}
