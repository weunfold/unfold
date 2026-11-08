

use std::process::Stdio;
use std::time::{Duration, Instant};

use rth_protocol::io::read_frame;
use rth_protocol::{Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

async fn fake_agent(listener: TcpListener, attached: tokio::sync::oneshot::Sender<()>) -> Instant {
    let (stream, _) = listener.accept().await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let mut g = Stamper::new(Source::Guest, "guest-lifeline", Box::new(MonotonicClock::start()));
    let hello = g.ephemeral(Event::Hello {
        protocol: PROTOCOL_VERSION,
        agent: "rth-agentd".into(),
        version: "test".into(),
        boot_id: "lifeline".into(),
        utc_ms_at_origin: 0,
        journal: "j".into(),
        backend: Some("fake@test".into()),
        deployment: None,
        instance: None,
        command_generation: None,
    });
    write.write_all(hello.to_line().as_bytes()).await.unwrap();
    let line = read_frame(&mut reader, &mut buf).await.unwrap();
    let Event::Attach { attachment_id, role: Role::Controller, .. } = Envelope::parse_from(&line, Source::Host).unwrap().event else { panic!("{line}") };
    let ready = g.stamp(Event::Ready { attachment_id, role: Role::Controller }, None);
    write.write_all(ready.to_line().as_bytes()).await.unwrap();
    let _ = attached.send(());

    while read_frame(&mut reader, &mut buf).await.is_ok() {}
    Instant::now()
}

fn host(agent: std::net::SocketAddr, dir: &std::path::Path, lifeline: bool) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_rth-host"));
    cmd.args(["--agent", &agent.to_string(), "--listen", "127.0.0.1:0", "--data"]).arg(dir).args(["--label", "lifeline-test"]);
    if lifeline {
        cmd.arg("--lifeline-stdin");
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    cmd.spawn().unwrap()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rth-lifeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test]
async fn stdin_eof_with_the_flag_quits_and_releases_the_lease() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let agent = tokio::spawn(fake_agent(listener, tx));
    let mut child = host(addr, &tmp("on"), true);
    tokio::time::timeout(Duration::from_secs(10), rx).await.expect("host attached").unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let closed_stdin = Instant::now();
    drop(child.stdin.take());
    let released = tokio::time::timeout(Duration::from_secs(3), agent).await.expect("lease released").unwrap();
    let status = tokio::time::timeout(Duration::from_secs(3), child.wait()).await.expect("host exited").unwrap();
    let bound = Duration::from_millis(1500);
    let took = released.duration_since(closed_stdin);
    eprintln!("stdin EOF → controller link closed in {:.1} ms; exit {status}", took.as_secs_f64() * 1e3);
    assert!(took < bound, "lease released after {took:?}");
    assert!(status.success(), "{status}");
}

#[tokio::test]
async fn stdin_eof_without_the_flag_keeps_the_host_running() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut agent = tokio::spawn(fake_agent(listener, tx));
    let mut child = host(addr, &tmp("off"), false);
    tokio::time::timeout(Duration::from_secs(10), rx).await.expect("host attached").unwrap();
    drop(child.stdin.take());
    assert!(tokio::time::timeout(Duration::from_millis(1500), &mut agent).await.is_err(), "the link must stay up");
    assert!(child.try_wait().unwrap().is_none(), "the host must keep running");
    child.kill().await.unwrap();
}
