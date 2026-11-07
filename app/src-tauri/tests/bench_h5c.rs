
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
#[allow(dead_code)]
#[path = "h5c/support.rs"]
mod support;
#[allow(dead_code)]
#[path = "../src/voice_front.rs"]
mod voice_front;
use serde_json::{json, Value};
use std::{
    io::Write,
    process::{Child, Command},
    time::{Duration, Instant},
};
use support::*;
use voice_front::{child::*, http::*, marker::*, Error};
fn payload() -> Value {
    json!({"model":"voice-front","messages":[{"role":"user","content":"exact input"}],"stream":true,"max_tokens":96,"id_slot":0,"cache_prompt":true,"chat_template_kwargs":{"enable_thinking":false}})
}
fn classify(route: Route) -> &'static str {
    match route {
        Route::Answer(_) => "answer",
        Route::Delegate => "delegate",
        Route::Fallback => "fallback",
    }
}
#[test]
fn synthetic_marker_positions_and_quoting_match_the_parser() {
    let rows: Value = serde_json::from_str(include_str!("fixtures/h5c-markers.json")).unwrap();
    assert_eq!(rows["probes"].as_array().unwrap().len(), 11);
    assert_eq!(rows["synthetic"].as_array().unwrap().len(), 3);
    for row in rows["probes"].as_array().unwrap() {
        assert_eq!(classify(route(row["input"].as_str().unwrap(), "delegation marker")), row["expected"], "{row}");
    }
    for row in rows["synthetic"].as_array().unwrap() {
        assert_eq!(classify(route(row["reply"].as_str().unwrap(), "delegation marker")), "delegate");
    }
}
#[test]
fn split_sse_utf8_and_nonleading_markers_are_buffered_until_complete() {
    let wire = stream(&["Olá. [DE", "LEGATE] trailing prose"], "stop");
    for split in 0..=wire.len() {
        let mut s = Sse::default();
        s.push(&wire.as_bytes()[..split]).unwrap();
        s.push(&wire.as_bytes()[split..]).unwrap();
        let burst = s.finish().unwrap();
        assert_eq!(route(&burst.text, "marcador de delegação"), Route::Delegate);
    }
    assert!(
        matches!(route("The token `[DELEGATE]` is literal.","delegation marker"),Route::Answer(s) if !s.contains("[DELEGATE]"))
    );
    for bad in ["partial [DELE", "unbalanced `literal", "'open", "\"open"] {
        assert_eq!(route(bad, "marker"), Route::Fallback);
    }
}
#[test]
fn broken_truncated_oversized_and_tool_streams_never_become_answers() {
    for wire in [
        stream(&["text"], "length"),
        stream(&["text"], "tool_calls"),
        stream(&["valid content"], "stop").replace("data: [DONE]\r\n\r\n", ""),
        event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{}]}}]})) + &stream(&["valid content"], "stop"),
        "data: [DONE]\n\n".into(),
        stream(&["text"], "stop") + "data: {}\n\n",
        event(json!({"choices":[{"index":0,"delta":{"tool_calls":[{}]}}]})),
        event(json!({"choices":[{"index":0,"delta":{"reasoning_content":"thinking"}}]})),
    ] {
        let mut s = Sse::default();
        assert!(s.push(wire.as_bytes()).and_then(|_| s.finish()).is_err());
    }
    let mut s = Sse::default();
    assert!(s.push(stream(&[&"x".repeat(8193)], "stop").as_bytes()).and_then(|_| s.finish()).is_err());
    let mut s = Sse::default();
    assert!(s.push(b"data: {broken}\n\n").is_err());
    let mut s = Sse::default();
    s.push(event(json!({"choices":[{"index":0,"delta":{"content":"never finished"}}]})).as_bytes()).unwrap();
    assert!(s.finish().is_err());
}
#[test]
fn unknown_or_insufficient_memory_refuses_before_launch() {
    struct Never;
    impl Launch for Never {
        fn verify(&mut self) -> Result<(), Error> {
            panic!("must refuse before assets")
        }
        fn spawn(&mut self, _: u16) -> Result<Child, Error> {
            panic!("must never spawn")
        }
    }
    for b in [
        Budget { pressure: 2, ..budget() },
        Budget { other_reserved: None, ..budget() },
        Budget { available: 10 << 30, ..budget() },
        Budget { other_reserved: Some(15 << 30), ..budget() },
    ] {
        assert!(Owner::start(&mut Never, || b).is_err());
    }
}
#[test]
fn direct_client_uses_owned_slot_and_accepts_both_http_framings_without_speaking() {
    for chunked in [false, true] {
        let (mut owner, listener) = bound();
        let server = serve(listener, response(&stream(&["A complete reply."], "stop"), chunked));
        let lease = owner.request().unwrap();
        assert!(owner.request().is_err());
        let p = payload();
        let burst = generate_payload(&lease, &p).unwrap();
        assert_eq!(burst.text, "A complete reply.");
        let sent = server.join().unwrap();
        assert_eq!(sent, p.clone());
        owner.close().unwrap();
        assert!(!lease.valid());
        assert!(owner.request().is_err());
    }
}
#[test]
fn close_and_cancel_fence_late_results_and_stop_only_the_owned_child() {
    let mut unrelated = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let mut owner = Owner::start(&mut Fake, budget).unwrap();
    let lease = owner.request().unwrap();
    owner.cancel();
    assert!(!lease.valid());
    drop(lease);
    let lease = owner.request().unwrap();
    let pid = owner.pid().unwrap();
    owner.close().unwrap();
    owner.close().unwrap();
    assert!(!lease.valid());
    assert!(owner.pid().is_none());
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert!(unrelated.try_wait().unwrap().is_none());
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
}
#[test]
fn http_failure_and_cancellation_never_release_a_partial_burst() {
    for wire in [
        "HTTP/1.1 429 Busy\r\nContent-Length: 0\r\n\r\n".to_string(),
        "HTTP/1.1 503 Busy\r\nContent-Length: 0\r\n\r\n".to_string(),
        response(&stream(&["half"], "length"), true),
        response("data: {bad}\n\n", false),
    ] {
        let (mut owner, listener) = bound();
        let busy = wire.starts_with("HTTP/1.1 429") || wire.starts_with("HTTP/1.1 503");
        let server = serve(listener, wire);
        let lease = owner.request().unwrap();
        let p = payload();
        assert!(matches!(generate_payload(&lease, &p), Err(e) if e == if busy { Error::Busy } else { Error::Stream }));
        server.join().unwrap();
        owner.close().unwrap();
    }
    let owner = Owner::start(&mut Fake, budget).unwrap();
    let lease = owner.request().unwrap();
    owner.cancel();
    let start = Instant::now();
    assert!(lease.check().is_err());
    assert!(start.elapsed() < Duration::from_millis(100));
}
#[test]
fn close_during_a_blocked_stream_prevents_any_late_completed_result() {
    let (mut owner, listener) = bound();
    let (ready, rx) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
        ready.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        let _ = socket.write_all(stream(&["late result"], "stop").as_bytes());
    });
    let lease = owner.request().unwrap();
    let p = payload();
    let request = std::thread::spawn(move || generate_payload(&lease, &p));
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    owner.close().unwrap();
    release.send(()).unwrap();
    assert!(matches!(request.join().unwrap(), Err(Error::Cancelled)));
    server.join().unwrap();
}
#[test]
fn pinned_runtime_configuration_is_explicit() {
    let a = args(5555);
    for pair in [["-np", "1"], ["-c", "4096"], ["--cache-ram", "0"], ["-t", "4"]] {
        assert!(a.windows(2).any(|p| p == pair));
    }
    assert!(a.iter().any(|s| s == "{\"enable_thinking\":false}"));
}
#[test]
fn a_silent_server_hits_the_first_content_deadline_without_releasing_a_burst() {
    let (mut owner, listener) = bound();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
        wait.recv_timeout(Duration::from_secs(4)).unwrap();
    });
    let lease = owner.request().unwrap();
    let start = Instant::now();
    let result = generate_payload(&lease, &payload());
    release.send(()).unwrap();
    server.join().unwrap();
    owner.close().unwrap();
    assert!(matches!(result, Err(Error::Deadline)));
    assert!(start.elapsed() < Duration::from_secs(4));
}
#[test]
fn admission_is_rechecked_after_asset_verification_before_the_only_spawn() {
    struct Never;
    impl Launch for Never {
        fn verify(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn spawn(&mut self, _: u16) -> Result<Child, Error> {
            panic!("gate changed: no child")
        }
    }
    let mut reads = 0;
    assert!(Owner::start(&mut Never, || {
        reads += 1;
        if reads == 1 {
            budget()
        } else {
            Budget { pressure: 2, ..budget() }
        }
    })
    .is_err());
    assert_eq!(reads, 2);
}

#[test]
fn quotes_code_runs_and_ambiguous_http_framing_do_not_expose_controls_or_partial_text() {
    for text in [
        "The token 'don't [DELEGATE]' is literal.",
        "A `literal `` [DELEGATE] `` span`.",
        "The token «[DELEGATE]» is literal.",
    ] {
        assert!(matches!(route(text,"delegation marker"),Route::Answer(s) if !s.contains("[DELEGATE]")));
    }
    let body = stream(&["valid content"], "stop");
    let duplicate = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nContent-Length: {}\r\n\r\n{}",body.len(),body.len(),body);
    for wire in [
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        duplicate.as_str(),
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\n",
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 50\r\n\r\nshort",
    ] {
        let (owner,listener)=bound();let server=serve(listener,wire.into());let lease=owner.request().unwrap();assert!(generate_payload(&lease,&payload()).is_err());server.join().unwrap();
    }
}

#[test]
fn readiness_is_separate_from_spawn_and_expired_leases_cannot_send() {
    let (owner,listener) = bound();
    let server = json_server(listener, vec![json!({"status":"ok"})]);
    let mut lease = owner.request().unwrap();
    assert!(ready(&lease).unwrap());
    assert_eq!(server.join().unwrap(), vec![Value::Null]);
    lease.start = Instant::now() - Duration::from_secs(6);
    assert_eq!(lease.check(), Err(Error::Deadline));
    assert!(matches!(generate_payload(&lease, &payload()), Err(Error::Deadline)));
}

#[test]
fn mismatched_asset_bytes_are_refused_before_executing_a_child() {
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
        .join(format!("front-bad-assets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad");
    std::fs::write(&path, b"public invalid asset").unwrap();
    let mut serve = Serve { binary: path.clone(), model: path, home: dir };
    assert!(matches!(Owner::start(&mut serve, budget), Err(Error::Assets)));
}

#[test]
fn an_exited_owned_child_is_a_front_failure_and_cannot_accept_another_request() {
    struct Exited;
    impl Launch for Exited {
        fn verify(&mut self) -> Result<(), Error> {
            Ok(())
        }
        fn spawn(&mut self, _: u16) -> Result<Child, Error> {
            let mut c = Command::new("/usr/bin/true").spawn().unwrap();
            c.wait().unwrap();
            Ok(c)
        }
    }
    let owner = Owner::start(&mut Exited, budget).unwrap();
    assert!(matches!(owner.request(), Err(Error::Child)));
}
