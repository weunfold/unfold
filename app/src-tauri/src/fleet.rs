

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::Value;
use unfold_speech::Cancel;

const IDLE: Duration = Duration::from_secs(60);

const POLL: Duration = Duration::from_millis(200);

const EVENT_MAX: usize = 4 << 20;

#[derive(Debug, PartialEq)]
pub enum Failed {

    Busy(Option<u32>),
    Cancelled,
    Synthesis(String),
}

pub fn stream(base_url: &str, key: Option<&str>, body: &Value, cancel: &Cancel, on_audio: &mut dyn FnMut(&[i16])) -> Result<bool, Failed> {
    let fail = |what: &str| Failed::Synthesis(what.to_string());
    let (mut r, head) = ask(base_url, key, body, cancel)?;
    let header = |name: &str| head.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim().to_string());
    if !header("content-type:").is_some_and(|t| t.starts_with("text/event-stream")) {
        return Err(fail("the reply is not an event stream"));
    }
    let chunked = header("transfer-encoding:").is_some_and(|t| t.contains("chunked"));

    let (mut text, mut ended) = (Vec::new(), false);
    loop {
        while let Some(i) = event_end(&text) {
            let event: Vec<u8> = text.drain(..i).collect();
            if let Some(done) = dispatch(&event, on_audio)? {
                return Ok(done);
            }
        }
        if ended {
            return Err(fail("the reply ended before done"));
        }
        if text.len() > EVENT_MAX {
            return Err(fail("an event is too long"));
        }
        if !chunked {
            text.append(&mut r.raw);
            ended = !r.more()?;
            text.append(&mut r.raw);
            continue;
        }

        let line = loop {
            if let Some(i) = r.raw.windows(2).position(|w| w == b"\r\n") {
                break i;
            }
            if r.raw.len() > 1024 || !r.more()? {
                return Err(fail("a bad chunk"));
            }
        };
        let size = std::str::from_utf8(&r.raw[..line]).ok().and_then(|l| usize::from_str_radix(l.split(';').next().unwrap_or("").trim(), 16).ok()).ok_or_else(|| fail("a bad chunk size"))?;
        if size > EVENT_MAX {
            return Err(fail("a chunk is too long"));
        }
        while r.raw.len() < line + 2 + size + 2 {
            if !r.more()? {
                return Err(fail("the reply ended inside a chunk"));
            }
        }
        text.extend_from_slice(&r.raw[line + 2..line + 2 + size]);
        r.raw.drain(..line + 2 + size + 2);
        ended = size == 0;
    }
}

pub fn warm(base_url: &str, key: Option<&str>, body: &Value) -> Result<(), Failed> {
    let cancel = Cancel::new();
    let (mut r, _) = ask(base_url, key, body, &cancel)?;
    while r.more()? {
        r.raw.clear();
    }
    Ok(())
}

fn ask<'a>(base_url: &str, key: Option<&str>, body: &Value, cancel: &'a Cancel) -> Result<(Reader<'a>, String), Failed> {
    let fail = |what: &str| Failed::Synthesis(what.to_string());
    let rest = base_url.strip_prefix("http://").ok_or_else(|| fail("node_url must be http://host:port"))?;
    let host = rest.trim_end_matches('/');
    let addr = std::net::ToSocketAddrs::to_socket_addrs(host).ok().and_then(|mut a| a.next()).ok_or_else(|| fail("no address for node_url"))?;
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| fail(&format!("connect: {}", e.kind())))?;
    let payload = body.to_string();
    let auth = key.map(|k| format!("Authorization: Bearer {k}\r\n")).unwrap_or_default();
    s.set_write_timeout(Some(Duration::from_secs(10))).map_err(|e| fail(&format!("socket: {}", e.kind())))?;
    let head = format!("POST /v1/audio/speech HTTP/1.1\r\nHost: {host}\r\nX-LocalFleet-Client: unfold\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(payload.as_bytes())).map_err(|e| fail(&format!("send: {}", e.kind())))?;
    s.set_read_timeout(Some(POLL)).map_err(|e| fail(&format!("socket: {}", e.kind())))?;
    let mut r = Reader { s, cancel, raw: Vec::new(), last: Instant::now() };

    let at = loop {
        if let Some(i) = r.raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if r.raw.len() > 64 * 1024 || !r.more()? {
            return Err(fail("the reply has no head"));
        }
    };
    let head = String::from_utf8_lossy(&r.raw[..at]).to_ascii_lowercase();
    r.raw.drain(..at + 4);
    let status = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let header = |name: &str| head.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim().to_string());

    if status == "429" || status == "503" {
        return Err(Failed::Busy(header("retry-after:").and_then(|v| v.parse().ok())));
    }
    if status != "200" {
        return Err(fail(&format!("HTTP {status}")));
    }
    Ok((r, head))
}

struct Reader<'a> {
    s: std::net::TcpStream,
    cancel: &'a Cancel,
    raw: Vec<u8>,
    last: Instant,
}

impl Reader<'_> {

    fn more(&mut self) -> Result<bool, Failed> {
        let mut buf = [0u8; 16 * 1024];
        loop {
            if self.cancel.is_cancelled() {
                return Err(Failed::Cancelled);
            }
            match self.s.read(&mut buf) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.raw.extend_from_slice(&buf[..n]);
                    self.last = Instant::now();
                    return Ok(true);
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) => {
                    if self.last.elapsed() > IDLE {
                        return Err(Failed::Synthesis("no reply in time".into()));
                    }
                }
                Err(e) => return Err(Failed::Synthesis(format!("read: {}", e.kind()))),
            }
        }
    }
}

fn event_end(b: &[u8]) -> Option<usize> {
    let lf = b.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    let crlf = b.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    match (lf, crlf) {
        (Some(a), Some(c)) => Some(a.min(c)),
        (a, c) => a.or(c),
    }
}

fn dispatch(event: &[u8], on_audio: &mut dyn FnMut(&[i16])) -> Result<Option<bool>, Failed> {
    let fail = |what: &str| Failed::Synthesis(what.to_string());
    let text = String::from_utf8_lossy(event);
    let (mut kind, mut data) = ("message", String::new());
    for line in text.lines() {
        if let Some(k) = line.strip_prefix("event:") {
            kind = k.trim();
        } else if let Some(d) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        }
    }
    match kind {
        "audio" => {
            let a: Value = serde_json::from_str(&data).map_err(|_| fail("an audio event is unreadable"))?;
            if a["format"] != "s16le" || a["sample_rate"] != unfold_speech::SAMPLE_RATE {
                return Err(fail("an audio event is not s16le at 24 kHz"));
            }
            let bytes = base64::engine::general_purpose::STANDARD.decode(a["pcm_base64"].as_str().unwrap_or("")).map_err(|_| fail("an audio event has bad base64"))?;
            if bytes.len() % 2 != 0 {
                return Err(fail("an audio event has an odd byte count"));
            }
            let samples: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            on_audio(&samples);
            Ok(None)
        }
        "done" => {
            let d: Value = serde_json::from_str(&data).map_err(|_| fail("the done event is unreadable"))?;
            match d["error"]["code"].as_str() {
                Some(code) => Err(Failed::Synthesis(format!("LocalFleet: {}", code.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(64).collect::<String>()))),
                None => Ok(Some(d["truncated"] == true)),
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serve(reply: impl FnOnce(&mut std::net::TcpStream) + Send + 'static) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();

            let (mut req, mut buf) = (Vec::new(), [0u8; 64 * 1024]);
            let _ = c.set_read_timeout(Some(Duration::from_millis(500)));
            loop {
                if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&req[..i]).to_ascii_lowercase();
                    let n: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    if req.len() >= i + 4 + n {
                        break;
                    }
                }
                match c.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            reply(&mut c);
            std::thread::sleep(Duration::from_millis(200));
        });
        url
    }

    fn audio(n: usize) -> String {
        let pcm: Vec<u8> = std::iter::repeat_n(1000i16.to_le_bytes(), n).flatten().collect();
        format!("event: audio\ndata: {}\n\n", json!({"format": "s16le", "sample_rate": 24000, "pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm)}))
    }
    const DONE: &str = "event: done\ndata: {\"truncated\":false}\n\n";
    const EVENTS: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n";

    fn run(url: &str) -> (Result<bool, Failed>, usize) {
        let mut got = 0;
        let r = stream(url, Some("k-secret"), &json!({"voice": "x"}), &Cancel::new(), &mut |s| got += s.len());
        (r, got)
    }

    #[test]
    fn a_plain_reply_closed_after_done_is_read_whole() {
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\n{}{}{DONE}", audio(100), audio(50));
        });
        assert_eq!(run(&url), (Ok(false), 150));
    }

    #[test]
    fn events_split_across_chunks_or_sharing_one_are_read() {
        let body = format!("{}{}{DONE}", audio(10), audio(20));
        let url = serve(move |c| {
            let _ = write!(c, "{EVENTS}Transfer-Encoding: chunked\r\n\r\n");
            for part in body.as_bytes().chunks(7) {
                let _ = write!(c, "{:x}\r\n", part.len());
                let _ = c.write_all(part);
                let _ = c.write_all(b"\r\n");
            }
            let _ = c.write_all(b"0\r\n\r\n");
        });
        assert_eq!(run(&url), (Ok(false), 30));
    }

    #[test]
    fn events_ended_by_crlf_are_read() {
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\n{}{}", audio(5).replace('\n', "\r\n"), DONE.replace('\n', "\r\n"));
        });
        assert_eq!(run(&url), (Ok(false), 5));
    }

    #[test]
    fn a_reply_that_ends_before_done_fails() {
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\n{}", audio(5));
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("the reply ended before done".into())));
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}Transfer-Encoding: chunked\r\n\r\n5\r\nevent");
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("the reply ended inside a chunk".into())));
    }

    #[test]
    fn a_chunk_an_event_and_the_head_are_bounded() {
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}Transfer-Encoding: chunked\r\n\r\nffffffff\r\n");
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("a chunk is too long".into())));
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\n");
            let junk = vec![b'x'; 64 * 1024];
            for _ in 0..80 {
                if c.write_all(&junk).is_err() {
                    break;
                }
            }
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("an event is too long".into())));
        let url = serve(|c| {
            let _ = c.write_all(&vec![b'h'; 80 * 1024]);
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("the reply has no head".into())));
    }

    #[test]
    fn busy_another_type_and_a_done_error_say_only_their_codes() {
        let url = serve(|c| {
            let _ = c.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n");
        });
        assert_eq!(run(&url).0, Err(Failed::Busy(None)));
        let url = serve(|c| {
            let _ = c.write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 4\r\nContent-Length: 0\r\n\r\n");
        });
        assert_eq!(run(&url).0, Err(Failed::Busy(Some(4))), "503 is busy too");
        let url = serve(|c| {
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}");
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("the reply is not an event stream".into())));

        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\nevent: done\ndata: {}\n\n", json!({"error": {"code": "bad voice <someone>", "message": "the server's words"}}));
        });
        assert_eq!(run(&url).0, Err(Failed::Synthesis("LocalFleet: badvoicesomeone".into())));
    }

    #[test]
    fn a_cancel_ends_a_silent_reply_at_once() {
        let url = serve(|c| {
            let _ = write!(c, "{EVENTS}\r\n");
            std::thread::sleep(Duration::from_secs(3));
        });
        let cancel = Cancel::new();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.cancel()
        });
        let t = Instant::now();
        assert_eq!(stream(&url, None, &json!({}), &cancel, &mut |_| {}), Err(Failed::Cancelled));
        assert!(t.elapsed() < Duration::from_millis(800), "{:?}", t.elapsed());
    }

    #[test]
    fn a_failure_to_reach_the_gateway_never_carries_the_key() {

        for url in ["http://127.0.0.1:1", "ftp://x", "http://no-port"] {
            let r = stream(url, Some("k-secret"), &json!({}), &Cancel::new(), &mut |_| {});
            assert!(r.is_err() && !format!("{r:?}").contains("k-secret"), "{r:?}");
        }
    }
}
