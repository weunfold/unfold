
use super::{child::Lease, Error, Result};
use serde_json::Value;
use std::{
    io::{BufReader, Read, Write},
    net::TcpStream,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
const MAX: usize = 256 << 10;
#[derive(Default, Debug)]
pub struct Burst {
    pub text: String,
    pub usage: Value,
    pub timings: Value,
    pub first_content: Option<Duration>,
    pub submitted: Option<Duration>,
    pub finish: Option<Duration>,
}
#[derive(Default)]
pub struct Sse {
    pending: Vec<u8>,
    burst: Burst,
    stopped: bool,
    done: bool,
    total: usize,
}
impl Sse {
    pub fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.total += bytes.len();
        if self.total > MAX {
            return Err(Error::Stream);
        }
        self.pending.extend_from_slice(bytes);
        loop {
            let crlf = self.pending.windows(4).position(|b| b == b"\r\n\r\n").map(|n| (n, 4));
            let lf = self.pending.windows(2).position(|b| b == b"\n\n").map(|n| (n, 2));
            let Some((end, delimiter)) = crlf.into_iter().chain(lf).min_by_key(|p| p.0) else { break };
            let row: Vec<_> = self.pending.drain(..end + delimiter).collect();
            self.event(&row[..end])?;
        }
        if self.pending.len() > 64 << 10 {
            return Err(Error::Stream);
        }
        Ok(())
    }
    fn event(&mut self, bytes: &[u8]) -> Result<()> {
        let text = std::str::from_utf8(bytes).map_err(|_| Error::Stream)?;
        let mut data = Vec::new();
        for line in text.lines() {
            if line.starts_with(':') {
                continue;
            }
            if let Some(s) = line.strip_prefix("data:") {
                data.push(s.strip_prefix(' ').unwrap_or(s));
            } else if !line.is_empty() {
                return Err(Error::Stream);
            }
        }
        if data.is_empty() {
            return Ok(());
        }
        if self.done {
            return Err(Error::Stream);
        }
        let data = data.join("\n");
        if data == "[DONE]" {
            if !self.stopped {
                return Err(Error::Stream);
            }
            self.done = true;
            return Ok(());
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| Error::Stream)?;
        let choices = value["choices"].as_array().ok_or(Error::Stream)?;
        if let Some(usage) = value.get("usage").filter(|v| !v.is_null()) {
            if usage["completion_tokens"].as_u64().is_none_or(|n| n > 96) {
                return Err(Error::Stream);
            }
            self.burst.usage = usage.clone();
        }
        if let Some(timings) = value.get("timings") {
            self.burst.timings = timings.clone();
        }
        if choices.is_empty() {
            return Ok(());
        }
        if choices.len() != 1 || self.stopped {
            return Err(Error::Stream);
        }
        let c = &choices[0];
        if c["index"] != 0 {
            return Err(Error::Stream);
        }
        let delta = &c["delta"];
        if delta.get("tool_calls").is_some()
            || delta.get("function_call").is_some()
            || delta.get("reasoning_content").is_some_and(|v| v.as_str().is_none_or(|s| !s.is_empty()))
        {
            return Err(Error::Stream);
        }
        if let Some(content) = delta.get("content").filter(|v| !v.is_null()) {
            self.burst.text.push_str(content.as_str().ok_or(Error::Stream)?);
            if self.burst.text.len() > 8192 {
                return Err(Error::Stream);
            }
        }
        if let Some(reason) = c.get("finish_reason").filter(|v| !v.is_null()) {
            if reason != "stop" {
                return Err(Error::Stream);
            }
            self.stopped = true;
        }
        Ok(())
    }
    pub fn finish(self) -> Result<Burst> {
        if !self.done
            || !self.stopped
            || self.burst.text.trim().is_empty()
            || !self.pending.iter().all(u8::is_ascii_whitespace)
        {
            return Err(Error::Stream);
        }
        Ok(self.burst)
    }
}
struct Wire<'a> {
    stream: TcpStream,
    lease: &'a Lease,
    bytes: usize,
    first: Option<&'a AtomicBool>,
}
impl Wire<'_> {
    fn check(&self) -> Result<()> {
        self.lease.check()?;
        if self.first.is_some_and(|seen| !seen.load(Ordering::Relaxed))
            && self.lease.start.elapsed() > Duration::from_secs(2)
        {
            return Err(Error::Deadline);
        }
        Ok(())
    }
}
impl Read for Wire<'_> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        loop {
            self.check().map_err(|_| std::io::Error::other("front request fenced"))?;
            match self.stream.read(b) {
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Ok(n) => {
                    self.bytes += n;
                    if self.bytes > MAX {
                        return Err(std::io::Error::other("front reply too large"));
                    }
                    return Ok(n);
                }
                other => return other,
            }
        }
    }
}
struct Body<'a> {
    reader: BufReader<Wire<'a>>,
    left: Option<usize>,
    chunked: bool,
    chunk: usize,
    ended: bool,
}
fn byte(r: &mut impl Read) -> Result<Option<u8>> {
    let mut b = [0];
    match r.read(&mut b) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(b[0])),
        Err(_) => Err(Error::Http),
    }
}
fn line(r: &mut impl Read) -> Result<String> {
    let mut b = Vec::new();
    while !b.ends_with(b"\r\n") {
        if b.len() > 4096 {
            return Err(Error::Http);
        }
        b.push(byte(r)?.ok_or(Error::Http)?);
    }
    String::from_utf8(b[..b.len() - 2].to_vec()).map_err(|_| Error::Http)
}
impl Body<'_> {
    fn next(&mut self) -> Result<Option<u8>> {
        self.reader.get_ref().check()?;
        if self.ended {
            return Ok(None);
        }
        if self.chunked && self.chunk == 0 {
            let size = line(&mut self.reader)?;
            self.chunk = usize::from_str_radix(size.split(';').next().unwrap_or(""), 16).map_err(|_| Error::Http)?;
            if self.chunk > MAX {
                return Err(Error::Http);
            }
            if self.chunk == 0 {
                if !line(&mut self.reader)?.is_empty() {
                    return Err(Error::Http);
                }
                self.ended = true;
                return Ok(None);
            }
        }
        if self.left == Some(0) {
            self.ended = true;
            return Ok(None);
        }
        let b = byte(&mut self.reader)?;
        if b.is_none() && (self.left.is_some() || self.chunked) {
            return Err(Error::Http);
        }
        if let Some(n) = &mut self.left {
            *n -= 1;
        }
        if self.chunked {
            self.chunk -= 1;
            if self.chunk == 0 && !line(&mut self.reader)?.is_empty() {
                return Err(Error::Http);
            }
        }
        Ok(b)
    }
}
fn ask<'a>(lease: &'a Lease, method: &str, path: &str, payload: &Value, first: Option<&'a AtomicBool>) -> Result<Body<'a>> {
    lease.check()?;
    lease.live()?;
    if !lease.address().ip().is_loopback() {
        return Err(Error::Closed);
    }
    let body = if path == "/health" { String::new() } else { payload.to_string() };
    if body.len() > 64 << 10 {
        return Err(Error::Context);
    }
    let mut stream =
        TcpStream::connect_timeout(&lease.address(), Duration::from_millis(200)).map_err(|_| Error::Http)?;
    stream.set_read_timeout(Some(Duration::from_millis(25))).map_err(|_| Error::Http)?;
    stream.set_write_timeout(Some(Duration::from_millis(100))).map_err(|_| Error::Http)?;
    lease.register(&stream)?;
    let head=format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",lease.address(),body.len());
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes())).map_err(|_| Error::Http)?;
    let mut reader = BufReader::new(Wire { stream, lease, bytes: 0, first });
    let status = line(&mut reader)?;
    let mut status = status.split_whitespace();
    if !matches!(status.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(Error::Http);
    }
    match status.next() {
        Some("200") => {}
        Some("429" | "503") => return Err(Error::Busy),
        _ => return Err(Error::Http),
    }
    let mut headers = std::collections::HashMap::new();
    let mut size = 0;
    loop {
        let row = line(&mut reader)?;
        size += row.len() + 2;
        if size > 16384 {
            return Err(Error::Http);
        }
        if row.is_empty() {
            break;
        }
        let (k, v) = row.split_once(':').ok_or(Error::Http)?;
        if headers.insert(k.to_ascii_lowercase(), v.trim().to_ascii_lowercase()).is_some() {
            return Err(Error::Http);
        }
    }
    if !headers
        .get("content-type")
        .is_some_and(|s| s.starts_with(if first.is_some() { "text/event-stream" } else { "application/json" }))
    {
        return Err(Error::Http);
    }
    let chunked = match headers.get("transfer-encoding") {
        None => false,
        Some(s) if s == "chunked" => true,
        _ => return Err(Error::Http),
    };
    let left = headers.get("content-length").map(|n| n.parse::<usize>().map_err(|_| Error::Http)).transpose()?;
    if (chunked && left.is_some()) || left.is_some_and(|n| n > MAX) {
        return Err(Error::Http);
    }
    Ok(Body { reader, left, chunked, chunk: 0, ended: false })
}
pub fn json_request(lease: &Lease, path: &str, payload: &Value) -> Result<Value> {
    if !matches!(path, "/health" | "/apply-template" | "/tokenize" | "/completion") {
        return Err(Error::Http);
    }
    let method = if path == "/health" { "GET" } else { "POST" };
    let mut body = ask(lease, method, path, payload, None)?;
    let mut bytes = Vec::new();
    while let Some(b) = body.next()? {
        bytes.push(b);
    }
    lease.live()?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Http)
}
pub fn generate_payload(lease: &Lease, payload: &Value) -> Result<Burst> {
    if payload["model"] != "voice-front"
        || payload["max_tokens"] != 96
        || payload["id_slot"] != 0
        || payload["stream"] != true
        || payload["cache_prompt"] != true
        || payload["chat_template_kwargs"]["enable_thinking"] != false
    {
        return Err(Error::Context);
    }
    let seen = AtomicBool::new(false);
    let submitted = lease.start.elapsed();
    let result = (|| {
        let mut body = ask(lease, "POST", "/v1/chat/completions", payload, Some(&seen))?;
        let mut s = Sse::default();
        let mut first = None;
        while let Some(b) = body.next()? {
            s.push(&[b])?;
            if !seen.load(Ordering::Relaxed) && !s.burst.text.is_empty() {
                seen.store(true, Ordering::Relaxed);
                first = Some(lease.start.elapsed());
            }
        }
        let mut burst = s.finish()?;
        burst.first_content = first;
        burst.submitted = Some(submitted);
        burst.finish = Some(lease.start.elapsed());
        Ok(burst)
    })();
    lease.check()?;
    lease.live()?;
    if !seen.load(Ordering::Relaxed) && lease.start.elapsed() >= Duration::from_secs(2) {
        return Err(Error::Deadline);
    }
    result
}

pub fn ready(lease: &Lease) -> Result<bool> {
    Ok(json_request(lease, "/health", &Value::Null)?["status"] == "ok")
}
