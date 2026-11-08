

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const VERIFY_FAKE_KEY: &str = "rth-verify-fake-key-not-a-secret-0000";

fn bind_port(env: &[(String, String)]) -> io::Result<u16> {
    env.iter()
        .find(|(k, _)| k == "RTH_AGENT_BIND")
        .and_then(|(_, v)| v.rsplit(':').next()?.parse().ok())
        .ok_or_else(|| io::Error::other("no RTH_AGENT_BIND port"))
}

fn spawn(agentd: &Path, env: &[(String, String)]) -> io::Result<Child> {
    Command::new(agentd).env_clear().envs(env.iter().map(|(k, v)| (k, v))).current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}

fn stop(mut child: Child) -> io::Result<()> {
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let end = Instant::now() + Duration::from_secs(10);
    let mut exited = false;
    while Instant::now() < end {
        if child.try_wait()?.is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if !exited {
        child.kill()?;
        child.wait()?;
    }

    let left = crate::effect::reap_own_group(Duration::from_secs(1), Duration::from_secs(5), Duration::from_secs(1));
    if !left.is_empty() {
        let _ = writeln!(io::stderr(), "rth-activate: still in the effect's group after the reap: {left:?}");
    }
    Ok(())
}

fn hello(port: u16, child: &mut Child, within: Duration) -> io::Result<String> {
    let end = Instant::now() + within;
    loop {
        if let Some(s) = child.try_wait()? {
            return Err(io::Error::other(format!("agentd exited {s} before its Hello")));
        }
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut line = String::new();
            BufReader::new(s).read_line(&mut line)?;
            if line.contains("\"kind\":\"hello\"") {
                return Ok(line);
            }
        }
        if Instant::now() > end {
            return Err(io::Error::other("no Hello within the bound"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn boot_once(agentd: &Path) -> io::Result<()> {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let port = bind_port(&env)?;
    let mut child = spawn(agentd, &env)?;
    let r = hello(port, &mut child, Duration::from_secs(20));
    stop(child)?;
    r.map(|_| ())
}

pub fn verify_boot(agentd: &Path, out: &Path) -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("ws://127.0.0.1:{}/v1/realtime?model=gpt-realtime-2.1", listener.local_addr()?.port());
    let bound: serde_json::Value = std::env::var("RTH_VERIFY_BOUND").ok().and_then(|b| serde_json::from_str(&b).ok()).ok_or_else(|| io::Error::other("RTH_VERIFY_BOUND is required"))?;
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists: a receipt is never overwritten", out.display())));
    }
    let agentd_sha256 = sha256_hex(&crate::record::read_bytes_strict(agentd)?.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{}: missing", agentd.display())))?);
    let mut env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| !["RTH_BACKEND", "RTH_OPENAI_URL", "OPENAI_API_KEY", "RTH_VERIFY_BOUND"].contains(&k.as_str())).collect();
    env.push(("RTH_BACKEND".into(), "openai-realtime".into()));
    env.push(("RTH_OPENAI_URL".into(), url));
    env.push(("OPENAI_API_KEY".into(), VERIFY_FAKE_KEY.into()));
    let brain_id = env.iter().find(|(k, _)| k == "RTH_MEMORY_BRAIN_ID").map(|(_, v)| v.clone());
    let root = env.iter().find(|(k, _)| k == "RTH_INSTANCE_ROOT").map(|(_, v)| std::path::PathBuf::from(v)).ok_or_else(|| io::Error::other("no RTH_INSTANCE_ROOT"))?;
    let child = spawn(agentd, &env)?;
    let boot_pid = child.id();
    listener.set_nonblocking(true)?;
    let end = Instant::now() + Duration::from_secs(60);
    let setup = loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false)?;
                break first_text_frame(s);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() > end {
                    break Err(io::Error::other("agentd made no setup within the bound"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => break Err(e),
        }
    };
    stop(child)?;
    let setup: serde_json::Value = serde_json::from_str(&setup?).map_err(|e| io::Error::other(format!("setup: {e}")))?;
    let ins = setup["session"]["instructions"].as_str().unwrap_or_default();
    let readiness = ins.lines().find_map(|l| l.strip_prefix("- memory: ")).unwrap_or("absent").to_string();

    let instance_id = crate::agentd_env::minted_instance(&root)?;
    let record = crate::record::read_json_strict(&crate::agentd_env::state_dir(&root).join("memory-binding-record.json"))?;
    let sentinel_ref = record.as_ref().and_then(|r| r["sentinel_ref"].as_str().map(str::to_string));
    let v = serde_json::json!({"bound": bound, "readiness": readiness, "brain_id": brain_id, "instance_id": instance_id, "sentinel_ref": sentinel_ref, "agentd_sha256": agentd_sha256, "boot_pid": boot_pid, "setup_type": setup["type"]});
    write_receipt(out, &v)?;
    if !receipt_agrees(&v) {
        return Err(io::Error::other(format!("this boot does not match its bound operation: {v}")));
    }
    if readiness.starts_with("attached") { Ok(()) } else { Err(io::Error::other(format!("memory readiness at setup: {readiness}"))) }
}

pub fn receipt_agrees(v: &serde_json::Value) -> bool {
    let b = &v["bound"];
    b.is_object() && v["instance_id"] == b["instance_id"] && v["brain_id"] == b["brain_id"] && v["sentinel_ref"] == b["sentinel_ref"] && v["agentd_sha256"] == b["agentd_sha256"]
}

fn write_receipt(out: &Path, v: &serde_json::Value) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(out)?;
    f.write_all(format!("{v:#}\n").as_bytes())?;
    f.sync_all()
}

pub fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn first_text_frame(mut s: TcpStream) -> io::Result<String> {
    s.set_read_timeout(Some(Duration::from_secs(20)))?;
    let mut req = Vec::new();
    let mut b = [0u8; 1];
    while !req.ends_with(b"\r\n\r\n") {
        if s.read(&mut b)? == 0 || req.len() > 16 * 1024 {
            return Err(io::Error::other("bad handshake"));
        }
        req.push(b[0]);
    }
    let text = String::from_utf8_lossy(&req);
    let key = text.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("sec-websocket-key")).map(|(_, v)| v.trim().to_string())).ok_or_else(|| io::Error::other("no websocket key"))?;
    let accept = base64(&sha1(format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes()));
    s.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").as_bytes())?;
    let mut msg = Vec::new();
    loop {
        let mut h = [0u8; 2];
        s.read_exact(&mut h)?;
        let (fin, op, masked) = (h[0] & 0x80 != 0, h[0] & 0x0f, h[1] & 0x80 != 0);
        let mut len = (h[1] & 0x7f) as u64;
        if len == 126 {
            let mut e = [0u8; 2];
            s.read_exact(&mut e)?;
            len = u16::from_be_bytes(e) as u64;
        } else if len == 127 {
            let mut e = [0u8; 8];
            s.read_exact(&mut e)?;
            len = u64::from_be_bytes(e);
        }
        if len > 4 * 1024 * 1024 {
            return Err(io::Error::other("frame too large"));
        }
        let mut mask = [0u8; 4];
        if masked {
            s.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len as usize];
        s.read_exact(&mut payload)?;
        if masked {
            for (i, x) in payload.iter_mut().enumerate() {
                *x ^= mask[i % 4];
            }
        }
        match op {
            0x1 | 0x0 => {
                msg.extend_from_slice(&payload);
                if fin {
                    return String::from_utf8(msg).map_err(|_| io::Error::other("not UTF-8"));
                }
            }
            0x8 => return Err(io::Error::other("closed before a setup")),
            _ => {}
        }
    }
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut m = data.to_vec();
    let bits = (data.len() as u64) * 8;
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_be_bytes());
    for chunk in m.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h = [h[0].wrapping_add(a), h[1].wrapping_add(b), h[2].wrapping_add(c), h[3].wrapping_add(d), h[4].wrapping_add(e)];
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha1_and_base64_match_the_rfc_6455_example() {

        let k = "dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
        assert_eq!(super::base64(&super::sha1(k.as_bytes())), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        assert_eq!(super::base64(b"ab"), "YWI=");
    }
}
