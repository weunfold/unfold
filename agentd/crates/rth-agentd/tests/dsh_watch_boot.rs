

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BIN: &str = env!("CARGO_BIN_EXE_rth-agentd");
mod tls { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/tls.rs")); }
const KEY: &str = "dsh-watch-boot-key-not-a-secret-0123";

const ROUTE_FACT: &str = "While Unfold watches the person's DSH agents, those agents' messages also reach the model that writes your replies";
const LOCAL_FACT: &str = "Unfold also watches the person's DSH agents. Your replies are written by Granite 4.1 3B";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agentd-watch-boot-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Fake {
    port: u16,
    bodies: Arc<Mutex<Vec<Value>>>,
}

fn answer<S: Read + Write>(mut s: S, bodies: &Mutex<Vec<Value>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
    while buf.len() < head_end + len {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let reply = |s: &mut S, ctype: &str, body: &str| {
        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = s.flush();
    };
    if head.starts_with("GET ") {
        return reply(&mut s, "application/json", r#"{"status":"ok","data":[{"id":"m"}]}"#);
    }
    let body: Value = serde_json::from_slice(&buf[head_end..head_end + len]).unwrap_or(Value::Null);
    let streamed = body["stream"] == json!(true);
    bodies.lock().unwrap().push(body);
    if !streamed {
        let act = json!({"choices": [{"message": {"role": "assistant", "content": r#"{"d":"no_op"}"#}, "finish_reason": "stop"}]});
        return reply(&mut s, "application/json", &act.to_string());
    }
    let events = [json!({"choices": [{"delta": {"content": "Ok."}}]}), json!({"choices": [{"delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}})];
    let sse: String = events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>() + "data: [DONE]\n\n";
    reply(&mut s, "text/event-stream", &sse);
}

impl Fake {
    fn start(tls: bool) -> Fake {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        let cfg = tls.then(|| {
            use rustls_pki_types::pem::PemObject;
            let _ = rustls::crypto::ring::default_provider().install_default();
            let certs = rustls_pki_types::CertificateDer::pem_slice_iter(&tls::material().cert).collect::<Result<Vec<_>, _>>().unwrap();
            let key = rustls_pki_types::PrivateKeyDer::from_pem_slice(&tls::material().key).unwrap();
            Arc::new(rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key).unwrap())
        });
        std::thread::spawn(move || {
            for tcp in l.incoming().flatten() {
                let (seen, cfg) = (seen.clone(), cfg.clone());
                std::thread::spawn(move || match cfg {
                    Some(cfg) => answer(rustls::StreamOwned::new(rustls::ServerConnection::new(cfg).unwrap(), tcp), &seen),
                    None => answer(tcp, &seen),
                });
            }
        });
        Fake { port, bodies }
    }

    fn reply_system(&self) -> Option<String> {
        let bodies = self.bodies.lock().unwrap();
        let b = bodies.iter().find(|b| b["stream"] == json!(true))?;
        b["messages"].as_array()?.iter().find(|m| m["role"] == "system")?["content"].as_str().map(str::to_string)
    }
}

struct Agent(Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn boot(dir: &Path, backend: &str, extra: &[(&str, String)]) -> (Agent, u16) {
    let port = free_port();
    let inst = dir.join("inst");
    std::fs::create_dir_all(inst.join("agentd")).unwrap();
    let mut c = Command::new(BIN);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("RTH_AGENT_MODE", "desktop")
        .env("RTH_INSTANCE_ROOT", &inst)
        .env("RTH_STATE", inst.join("agentd/state.json"))
        .env("RTH_JOURNAL", inst.join("agentd/journal.jsonl"))
        .env("RTH_AGENT_BIND", format!("127.0.0.1:{port}"))
        .env("RTH_BACKEND", backend)

        .env("RTH_DSH_WATCH", "1")
        .env("RTH_DSH_WATCH_URL", format!("http://127.0.0.1:{}", free_port()))
        .env("RTH_DSH_WATCH_KEY", KEY)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("agentd.log")).unwrap());
    for (k, v) in extra {
        c.env(k, v);
    }
    (Agent(c.spawn().unwrap()), port)
}

fn first_reply_provenance(dir: &Path, port: u16) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    let stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("agentd did not listen: {e}; {}", std::fs::read_to_string(dir.join("agentd.log")).unwrap_or_default()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut w = stream.try_clone().unwrap();
    let mut lines = BufReader::new(stream).lines();
    let mut next = || -> Value { serde_json::from_str(&lines.next().expect("a frame").expect("readable")).unwrap() };
    let mut seq = 0u64;
    let mut send = |w: &mut TcpStream, ev: Value| {
        seq += 1;
        let mut env = json!({"v": 2, "id": format!("boot-test:{seq}"), "producer": "boot-test", "seq": seq, "t_ns": seq, "source": "host"});
        env.as_object_mut().unwrap().extend(ev.as_object().unwrap().clone());
        writeln!(w, "{env}").unwrap();
    };
    let hello = loop {
        let f = next();
        if f["kind"] == "hello" {
            break f;
        }
    };
    let session = hello["instance"]["session_id"].as_str().expect("an instance session").to_string();
    send(&mut w, json!({"kind": "attach", "protocol": 2, "attachment_id": "boot-test", "role": "controller"}));
    while next()["kind"] != "ready" {}
    send(&mut w, json!({"kind": "command_submitted", "command_id": "boot-test:c1", "session_id": session, "expected_generation": hello["command_generation"].as_u64().unwrap_or(0), "text": "hello"}));
    while next()["kind"] != "agent_done" {}
    let journal = dir.join("inst/agentd/journal.jsonl");
    for _ in 0..100 {
        let text = std::fs::read_to_string(&journal).unwrap_or_default();
        if let Some(p) = text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|e| e["kind"] == "response_provenance") {
            return p;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no response_provenance in {}", journal.display());
}

fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn check(backend: &str, dir: &Path, port: u16, fake: &Fake, name: &str, fact: &str) {
    let p = first_reply_provenance(dir, port);
    assert_eq!(p["prompt"].as_str(), Some(name), "{backend}: {p}");
    let system = fake.reply_system().unwrap_or_else(|| panic!("{backend}: the reply reached no model"));
    assert!(system.contains(fact), "{backend}: the reply's prompt carries the watch's fact");
    assert_eq!(p["prompt_sha256"].as_str(), Some(sha256_hex(&system).as_str()), "{backend}: the journaled hash is the prompt that was sent");
}

#[test]
fn the_watch_fact_reaches_the_first_reply_prompt_on_local_dsh_and_flash() {
    let granite = Fake::start(false);
    let local_url = format!("http://127.0.0.1:{}/v1", granite.port);
    let local = [("RTH_LOCAL_LLM_URL", local_url.clone()), ("RTH_LOCAL_MODEL", "granite41-3b-q4km".to_string())];

    let dir = tmp("local");
    let (_a, port) = boot(&dir, "local", &local);
    check("local", &dir, port, &granite, "unfold-local-text-v4.3.1", LOCAL_FACT);

    let dsh = Fake::start(false);
    let dir = tmp("dsh");
    let mut env = local.to_vec();
    env.extend([("RTH_DSH_URL", format!("http://127.0.0.1:{}/v1", dsh.port)), ("RTH_DSH_KEY", "dsh-reply-key-not-a-secret-0123456".to_string()), ("RTH_DSH_MODEL", "deepseek-official/deepseek-v4-flash".to_string())]);
    let (_a, port) = boot(&dir, "dsh", &env);
    check("dsh", &dir, port, &dsh, "unfold-dsh-text-v1.2.1", ROUTE_FACT);

    let flash = Fake::start(true);
    let dir = tmp("flash");
    let mut env = local.to_vec();
    env.extend([("RTH_FLASH_URL", format!("https://127.0.0.1:{}/v1", flash.port)), ("RTH_FLASH_KEY", "flash-key-not-a-secret-0123456789".to_string()), ("RTH_FLASH_MODEL", "flash-test-model".to_string()), ("RTH_CA_FILE", tls::ca().to_string())]);
    let (_a, port) = boot(&dir, "flash", &env);
    check("flash", &dir, port, &flash, "unfold-flash-text-v3.2.1", ROUTE_FACT);
}
