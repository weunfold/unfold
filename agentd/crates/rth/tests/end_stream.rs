

use std::process::Stdio;
use std::time::{Duration, Instant};

use rth_protocol::io::read_frame;
use rth_protocol::{Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("rth-endstream-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wav(path: &std::path::Path, ms: usize) {
    let n = 16 * ms;
    let data: Vec<u8> = (0..n).flat_map(|i| (((i as f64 / 16_000.0 * 300.0 * std::f64::consts::TAU).sin() * 8000.0) as i16).to_le_bytes()).collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&16_000u32.to_le_bytes());
    b.extend_from_slice(&32_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(path, b).unwrap();
}

async fn fake_agent(listener: TcpListener, frames: mpsc::UnboundedSender<Option<Event>>) {
    let (stream, _) = listener.accept().await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let g = Stamper::new(Source::Guest, "guest-es", Box::new(MonotonicClock::start()));
    let hello = g.ephemeral(Event::Hello {
        protocol: PROTOCOL_VERSION,
        agent: "rth-agentd".into(),
        version: "test".into(),
        boot_id: "es".into(),
        utc_ms_at_origin: 0,
        journal: "j".into(),
        backend: Some("fake@test".into()),
        deployment: None,
        instance: None,
        command_generation: None,
    });
    write.write_all(hello.to_line().as_bytes()).await.unwrap();
    let mut g = g;
    while let Ok(line) = read_frame(&mut reader, &mut buf).await {
        let env = Envelope::parse_from(&line, Source::Host).unwrap();
        if let Event::Attach { attachment_id, .. } = &env.event {
            let ready = g.stamp(Event::Ready { attachment_id: attachment_id.clone(), role: Role::Controller }, None);
            write.write_all(ready.to_line().as_bytes()).await.unwrap();
        }
        let _ = frames.send(Some(env.event));
    }
    let _ = frames.send(None);
}

fn host(agent: std::net::SocketAddr, dir: &std::path::Path, mic: Option<&std::path::Path>) -> tokio::process::Child {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_rth-host"));
    cmd.args(["--agent", &agent.to_string(), "--listen", "127.0.0.1:0", "--data"]).arg(dir).args(["--label", "es-test", "--log"]).arg(dir.join("host.log"));
    if let Some(m) = mic {
        cmd.arg("--mic-wav").arg(m);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    cmd.spawn().unwrap()
}

async fn next_end(rx: &mut mpsc::UnboundedReceiver<Option<Event>>) -> (String, u64) {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("a frame").unwrap() {
            Some(Event::UserAudioEnd { stream_id, chunks, .. }) => return (stream_id, chunks),
            Some(_) => {}
            None => panic!("the link closed"),
        }
    }
}

#[tokio::test]
async fn end_stream_rotates_the_scripted_mic_and_quit_stays_immediate() {
    let dir = tmp("on");
    let w = dir.join("mic.wav");
    wav(&w, 600);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(fake_agent(listener, tx));
    let mut child = host(addr, &dir, Some(&w));
    let mut stdin = child.stdin.take().unwrap();

    loop {
        if let Some(Event::UserAudio { .. }) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("mic audio").unwrap() {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    stdin.write_all(b"end-stream\n").await.unwrap();
    let (s1, c1) = next_end(&mut rx).await;
    assert!(c1 > 0, "stream 1 carried audio");

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(child.try_wait().unwrap().is_none(), "the host kept running");
    stdin.write_all(b"end-stream\n").await.unwrap();
    let (s2, _) = next_end(&mut rx).await;
    assert_ne!(s1, s2, "a later stream has a new id");

    let burst = "end-stream\n".repeat(500) + "quit\n";
    let t = Instant::now();
    stdin.write_all(burst.as_bytes()).await.unwrap();
    let status = tokio::time::timeout(Duration::from_secs(3), child.wait()).await.expect("host exits").unwrap();
    eprintln!("quit behind 500 end-stream lines: exit after {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    assert!(status.success());
    let log = std::fs::read_to_string(dir.join("host.log")).unwrap();
    assert!(log.contains(r#""outcome":"admitted""#), "admissions are logged");
}

#[tokio::test]
async fn end_stream_without_a_scripted_mic_is_ignored_and_logged() {
    let dir = tmp("off");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(fake_agent(listener, tx));
    let mut child = host(addr, &dir, None);
    let mut stdin = child.stdin.take().unwrap();
    loop {
        if let Some(Event::Attach { .. }) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.expect("attach").unwrap() {
            break;
        }
    }
    stdin.write_all(b"end-stream\n").await.unwrap();
    let mut log = String::new();
    for _ in 0..100 {
        log = std::fs::read_to_string(dir.join("host.log")).unwrap_or_default();
        if log.contains("end_stream_request") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(log.contains(r#""outcome":"ignored""#), "{log}");
    assert!(!matches!(rx.try_recv(), Ok(Some(Event::UserAudioEnd { .. }))));
    assert!(child.try_wait().unwrap().is_none());
    stdin.write_all(b"quit\n").await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), child.wait()).await.expect("exit").unwrap().success());
}
