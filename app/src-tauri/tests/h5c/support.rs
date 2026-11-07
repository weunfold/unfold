use super::voice_front::{child::*, Error};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Child, Command},
    time::Duration,
};
pub fn event(value: Value) -> String {
    format!("data: {value}\r\n\r\n")
}
pub fn stream(parts: &[&str], reason: &str) -> String {
    let mut out = String::new();
    for text in parts {
        out += &event(json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}));
    }
    out += &event(json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}));
    out + "data: [DONE]\r\n\r\n"
}
pub struct Fake;
impl Launch for Fake {
    fn verify(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn spawn(&mut self, _port: u16) -> Result<Child, Error> {
        Command::new("/bin/sleep").arg("60").spawn().map_err(|_| Error::Child)
    }
}
pub fn budget() -> Budget {
    Budget { pressure: 1, physical: 48 << 30, available: 20 << 30, other_reserved: Some(5 << 30) }
}
pub fn bound() -> (Owner, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let owner = Owner::for_test_listener(&mut Fake, budget, &listener).unwrap();
    (owner, listener)
}
pub fn serve(listener: TcpListener, wire: String) -> std::thread::JoinHandle<Value> {
    std::thread::spawn(move || {
        use std::os::fd::AsRawFd;
        listener.set_nonblocking(true).unwrap();
        let mut fd = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        assert!(unsafe { libc::poll(&mut fd, 1, 2000) } > 0, "fake server request deadline");
        let (mut conn, _) = listener.accept().unwrap();
        conn.set_nonblocking(false).unwrap();
        conn.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut bytes = Vec::new();
        let mut b = [0; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            conn.read_exact(&mut b).unwrap();
            bytes.push(b[0]);
        }
        let head = String::from_utf8(bytes).unwrap();
        let mut request = head.lines().next().unwrap().split_whitespace();
        let method = request.next().unwrap();
        let path = request.next().unwrap();
        assert!(matches!(path, "/health" | "/v1/chat/completions" | "/apply-template" | "/tokenize" | "/completion"));
        assert_eq!(method, if path == "/health" { "GET" } else { "POST" }, "wrong method for {path}");
        let len: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).unwrap().parse().unwrap();
        let mut body = vec![0; len];
        conn.read_exact(&mut body).unwrap();
        let body = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
        conn.write_all(wire.as_bytes()).unwrap();
        body
    })
}
pub fn response(body: &str, chunked: bool) -> String {
    if chunked {
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",body.len(),body)
    } else {
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}", body.len(), body)
    }
}
pub fn json_server(listener: TcpListener, outputs: Vec<Value>) -> std::thread::JoinHandle<Vec<Value>> {
    std::thread::spawn(move || {
        outputs
            .into_iter()
            .map(|output| {
                let text = output.to_string();
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    text.len(),
                    text
                );
                serve(listener.try_clone().unwrap(), wire).join().unwrap()
            })
            .collect()
    })
}
