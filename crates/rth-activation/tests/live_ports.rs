use rth_activation::live::{live, marked_processes, LiveChecks};
use std::process::Command;

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_bound_port_is_live_on_loopback_or_wildcard_and_a_closed_one_is_not() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for addr in ["127.0.0.1:0", "0.0.0.0:0", "[::1]:0"] {
        let l = std::net::TcpListener::bind(addr).unwrap();
        let port = l.local_addr().unwrap().port();
        let c = LiveChecks { ports: vec![port], ..Default::default() };
        assert!(!live(&c).unwrap().is_empty(), "{addr} listening is live");
        drop(l);
        assert!(live(&c).unwrap().is_empty(), "{addr} closed is not");
    }
}

#[test]
fn a_process_of_this_user_carrying_a_marker_is_live_and_its_argv_is_never_reported() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let marker = format!("rth-live-marker-{}-launcher.mjs", std::process::id());

    let before = marked_processes(std::slice::from_ref(&marker)).unwrap();
    assert!(before.is_empty(), "{before:?}");

    let mut child = Command::new("/bin/sh").args(["-c", "read x", &marker]).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let found = marked_processes(std::slice::from_ref(&marker)).unwrap();
    let pid = child.id();
    assert!(found.iter().any(|r| r == &format!("process {pid} belongs to the current RTH")), "{found:?}");
    assert!(found.iter().all(|r| !r.contains(&marker)), "argv never reported");
    drop(child.stdin.take());
    child.wait().unwrap();
    assert!(marked_processes(std::slice::from_ref(&marker)).unwrap().iter().all(|r| !r.contains(&format!("process {pid} "))));
}
