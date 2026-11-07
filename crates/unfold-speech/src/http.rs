

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::client::Cancel;

const MAX_HEAD: u64 = 64 * 1024;
const MAX_LINE: u64 = 4 * 1024 * 1024;

pub(crate) struct Response {
    pub status: u16,
    headers: Vec<(String, String)>,
    pub body: Body<BufReader<Wire>>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

pub(crate) fn post(
    host: &str,
    port: u16,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
    cancel: Option<Cancel>,
) -> io::Result<Response> {
    let addrs: Vec<_> = (host, port).to_socket_addrs()?.collect();
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address for host");
    let mut stream = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = e,
        }
    }
    let stream = stream.ok_or(last)?;
    stream.set_read_timeout(Some(if cancel.is_some() { Cancel::TICK.min(timeout) } else { timeout }))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;

    let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\n\
         Accept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut writer = &stream;
    writer.write_all(head.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    drop(head);

    let mut reader = BufReader::new(Wire { stream, timeout, cancel });
    let mut read = 0u64;
    let status_line = line(&mut reader, MAX_HEAD)?;
    read += status_line.len() as u64;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|_| status_line.starts_with("HTTP/1."))
        .ok_or_else(|| invalid("not an HTTP/1.x status line"))?;
    let mut headers = Vec::new();
    loop {
        let l = line(&mut reader, MAX_HEAD - read.min(MAX_HEAD))?;
        read += l.len() as u64 + 2;
        if read > MAX_HEAD {
            return Err(invalid("response head over 64 KiB"));
        }
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    let framing = if chunked {
        Framing::Chunked(ChunkState::Size)
    } else if let Some((_, v)) = headers.iter().find(|(k, _)| k == "content-length") {
        Framing::Length(v.parse().map_err(|_| invalid("bad content-length"))?)
    } else {
        Framing::Close
    };
    Ok(Response { status, headers, body: Body { inner: reader, framing } })
}

pub(crate) struct Wire {
    stream: TcpStream,
    timeout: Duration,
    cancel: Option<Cancel>,
}

impl Read for Wire {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let Some(cancel) = &self.cancel else { return self.stream.read(out) };
        let since = Instant::now();
        loop {
            if cancel.is_cancelled() {
                return Err(io::Error::other("cancelled"));
            }
            match self.stream.read(out) {
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    if since.elapsed() >= self.timeout {
                        return Err(e);
                    }
                }
                r => return r,
            }
        }
    }
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

fn line(r: &mut impl BufRead, max: u64) -> io::Result<String> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(max).read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    if buf.last() != Some(&b'\n') {
        return Err(if (n as u64) < max { io::ErrorKind::UnexpectedEof.into() } else { invalid("line too long") });
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| invalid("line is not UTF-8"))
}

enum ChunkState {
    Size,
    In(u64),
    Finished,
}

enum Framing {
    Chunked(ChunkState),
    Length(u64),
    Close,
}

pub(crate) struct Body<R> {
    inner: R,
    framing: Framing,
}

impl<R: BufRead> Read for Body<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        match &mut self.framing {
            Framing::Close => self.inner.read(out),
            Framing::Length(0) => Ok(0),
            Framing::Length(rem) => {
                let want = out.len().min(usize::try_from(*rem).unwrap_or(usize::MAX));
                let n = self.inner.read(&mut out[..want])?;
                if n == 0 {
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                *rem -= n as u64;
                Ok(n)
            }
            Framing::Chunked(state) => loop {
                match state {
                    ChunkState::Finished => return Ok(0),
                    ChunkState::Size => {
                        let l = line(&mut self.inner, 1024)?;
                        let hex = l.split(';').next().unwrap_or("").trim();
                        let size = u64::from_str_radix(hex, 16).map_err(|_| invalid("bad chunk size"))?;
                        if size == 0 {

                            while let Ok(t) = line(&mut self.inner, MAX_HEAD) {
                                if t.is_empty() {
                                    break;
                                }
                            }
                            *state = ChunkState::Finished;
                        } else {
                            *state = ChunkState::In(size);
                        }
                    }
                    ChunkState::In(rem) => {
                        let want = out.len().min(usize::try_from(*rem).unwrap_or(usize::MAX));
                        let n = self.inner.read(&mut out[..want])?;
                        if n == 0 {
                            return Err(io::ErrorKind::UnexpectedEof.into());
                        }
                        *rem -= n as u64;
                        if *rem == 0 {
                            if !line(&mut self.inner, 2)?.is_empty() {
                                return Err(invalid("chunk not followed by CRLF"));
                            }
                            *state = ChunkState::Size;
                        }
                        return Ok(n);
                    }
                }
            },
        }
    }
}

pub(crate) struct Sse<R> {
    inner: BufReader<R>,
}

impl<R: Read> Sse<R> {
    pub fn new(body: R) -> Self {
        Sse { inner: BufReader::with_capacity(64 * 1024, body) }
    }

    pub fn next_event(&mut self) -> io::Result<Option<(String, String)>> {
        let mut kind: Option<String> = None;
        let mut data: Option<String> = None;
        loop {
            let l = match line(&mut self.inner, MAX_LINE) {
                Ok(l) => l,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e),
            };
            if l.is_empty() {
                if kind.is_some() || data.is_some() {
                    return Ok(Some((kind.unwrap_or_else(|| "message".into()), data.unwrap_or_default())));
                }
                continue;
            }
            if l.starts_with(':') {
                continue;
            }
            let (field, value) = match l.split_once(':') {
                Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
                None => (l.as_str(), ""),
            };
            match field {
                "event" => kind = Some(value.to_string()),
                "data" => match &mut data {
                    Some(d) => {
                        d.push('\n');
                        d.push_str(value);
                    }
                    None => data = Some(value.to_string()),
                },
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(raw: &[u8]) -> Body<&[u8]> {
        Body { inner: raw, framing: Framing::Chunked(ChunkState::Size) }
    }

    #[test]
    fn chunked_body_decodes_with_extensions_and_trailers() {
        let mut out = String::new();
        body(b"5;x=1\r\nhello\r\n6\r\n world\r\n0\r\nX-T: 1\r\n\r\n").read_to_string(&mut out).unwrap();
        assert_eq!(out, "hello world");
    }

    #[test]
    fn chunked_body_cut_short_is_unexpected_eof() {
        let mut out = String::new();
        let e = body(b"5\r\nhello\r\n6\r\n wo").read_to_string(&mut out).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn sse_joins_data_lines_and_drops_an_unterminated_block() {
        let mut sse = Sse::new(&b": hi\r\nevent: audio\r\ndata: a\r\ndata: b\r\n\r\nevent: done\ndata: {}"[..]);
        assert_eq!(sse.next_event().unwrap(), Some(("audio".into(), "a\nb".into())));
        assert_eq!(sse.next_event().unwrap(), None);
    }
}
