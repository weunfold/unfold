

#![cfg_attr(not(test), allow(dead_code))]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::SocketAddr;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

pub const SOCKET: &str = "control.sock";
pub const TOKEN_FILE: &str = "control.token";

pub const OBSERVER_FILE: &str = "observer";

pub const ENV_OBSERVER: &str = "UNFOLD_AGENTD_OBSERVER";
pub const ENV_SOCKET: &str = "UNFOLD_CONTROL_SOCKET";
pub const ENV_TOKEN_FILE: &str = "UNFOLD_CONTROL_TOKEN_FILE";

const MAX_REQUEST: u64 = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    pub pid: i32,
    pub pgid: i32,
    pub start_time: u64,
    pub command: String,
}

pub trait Children: Send + Sync {
    fn instance_child(&self) -> Option<Recorded>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub ppid: i32,
    pub start_us: u64,
    pub path: String,
}

pub fn proc_of(pid: i32) -> Option<Proc> {
    let mut i: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    if unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut i as *mut libc::proc_bsdinfo).cast(), size) } != size {
        return None;
    }
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(Proc { ppid: i.pbi_ppid as i32, start_us: i.pbi_start_tvsec * 1_000_000 + i.pbi_start_tvusec, path: String::from_utf8(buf).ok()? })
}

pub fn peer_pid(s: &UnixStream) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let r = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len) };
    (r == 0 && pid > 0).then_some(pid)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginHost {
    pub path: String,
    pub sha256: String,
}

pub fn file_sha256(path: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path).ok()?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).ok()?;
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn admit(peer: i32, rec: &Recorded, since_us: u64, host: Option<&PluginHost>, procs: &dyn Fn(i32) -> Option<Proc>, hash: &dyn Fn(&str) -> Option<String>) -> Result<(), &'static str> {
    if rec.start_time < since_us {
        return Err("recorded before this app started");
    }
    let p = procs(peer).ok_or("the peer is gone")?;
    if peer == rec.pid {
        return if p.start_us == rec.start_time { Ok(()) } else { Err("a recycled pid: another start time") };
    }
    let Some(host) = host else { return Err("not the instance child") };
    if p.ppid != rec.pid {
        return Err("not the instance child or its direct child");
    }
    let parent = procs(rec.pid).ok_or("the instance child is gone")?;
    if parent.start_us != rec.start_time {
        return Err("a recycled parent pid: another start time");
    }
    if p.start_us < rec.start_time {
        return Err("started before the instance child");
    }
    if p.path != host.path {
        return Err("not the plugin host's wrapper");
    }
    match hash(&host.path) {
        Some(h) if same(h.as_bytes(), host.sha256.as_bytes()) => Ok(()),
        Some(_) => Err("the plugin host's wrapper changed"),
        None => Err("the plugin host's wrapper is unreadable"),
    }
}

pub type Submit = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

pub trait AnswerWith: Send + Sync {
    fn facts(&self) -> Value;
    fn choose(&self, choice: Option<&str>, effort: Option<&str>, ack: bool) -> Result<Value, (u16, &'static str)>;
}

pub struct Config {

    pub answer: Option<Arc<dyn AnswerWith>>,

    pub dir: PathBuf,

    pub session: Option<String>,

    pub plugin_host: Option<PluginHost>,
    pub journal: PathBuf,
}

pub struct Endpoint {
    socket: PathBuf,
    token_file: PathBuf,
    observer_file: PathBuf,

    bound: (u64, u64),
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Endpoint {
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn token_file(&self) -> &Path {
        &self.token_file
    }

    pub fn child_env(&self) -> Vec<(String, String)> {
        [(ENV_OBSERVER, &self.observer_file), (ENV_SOCKET, &self.socket), (ENV_TOKEN_FILE, &self.token_file)].into_iter().map(|(k, p)| (k.to_string(), p.display().to_string())).collect()
    }

    pub fn publish_observer(&self, observer: SocketAddr) -> Result<(), String> {
        if !observer.ip().is_loopback() || observer.port() == 0 {
            return Err(format!("{observer} is not a loopback address with a port"));
        }
        let tmp = self.observer_file.with_extension("new");
        clear(&tmp)?;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&tmp)
            .and_then(|mut f| writeln!(f, "{observer}"))
            .and_then(|_| std::fs::rename(&tmp, &self.observer_file))
            .map_err(|e| format!("{}: {e}", self.observer_file.display()))
    }

    pub fn withdraw_observer(&self) {
        let _ = std::fs::remove_file(&self.observer_file);
    }

    pub fn stop(mut self) {
        self.halt();
    }

    fn owns_socket(&self) -> bool {
        std::fs::symlink_metadata(&self.socket).is_ok_and(|m| (m.dev(), m.ino()) == self.bound)
    }

    fn halt(&mut self) {
        let Some(t) = self.thread.take() else { return };
        self.stop.store(true, Ordering::SeqCst);

        if !self.owns_socket() {
            return;
        }

        if UnixStream::connect(&self.socket).is_ok() {
            let _ = t.join();
        }
        if !self.owns_socket() {
            return;
        }
        let _ = std::fs::remove_file(&self.observer_file);
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.token_file);
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.halt();
    }
}

struct State {
    answer: Option<Arc<dyn AnswerWith>>,
    session: Option<String>,
    plugin_host: Option<PluginHost>,
    token: String,
    children: Arc<dyn Children>,
    submit: Submit,
    journal: PathBuf,
    log: crate::Log,

    since_us: u64,
}

struct Row {
    verb: &'static str,
    peer: Option<i32>,
    accepted: bool,
    turn_id: Option<String>,
    refused: Option<&'static str>,

    ids: Option<Value>,
}

pub fn serve(cfg: Config, children: Arc<dyn Children>, submit: Submit, log: &crate::Log) -> Result<Endpoint, String> {
    let refuse = |why: String| {
        log.line(&format!("dsh control: not served: {why}"));
        why
    };
    crate::dsh_sockdir::check(&cfg.dir, unsafe { libc::geteuid() }).map_err(|e| refuse(format!("{}: {e}", cfg.dir.display())))?;
    let socket = crate::dsh_sockdir::socket_path(&cfg.dir, SOCKET).map_err(refuse)?;
    let token_file = cfg.dir.join(TOKEN_FILE);
    let observer_file = cfg.dir.join(OBSERVER_FILE);
    let since_us = proc_of(std::process::id() as i32).map(|p| p.start_us).ok_or_else(|| refuse("this app's own start time is unknown".into()))?;

    if let Ok(probe) = UnixStream::connect(&socket) {
        if peer_pid(&probe) != Some(std::process::id() as i32) {
            return Err(refuse(format!("{}: a live endpoint of another process serves here", socket.display())));
        }
    }
    clear(&socket).and_then(|_| clear(&token_file)).and_then(|_| clear(&observer_file)).map_err(refuse)?;
    let token = new_token().map_err(refuse)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&token_file)
        .and_then(|mut f| writeln!(f, "{token}"))
        .map_err(|e| refuse(format!("{}: {e}", token_file.display())))?;
    let listener = UnixListener::bind(&socket).and_then(|l| std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).map(|_| l));
    let listener = listener.and_then(|l| std::fs::symlink_metadata(&socket).map(|m| (l, (m.dev(), m.ino()))));
    let (listener, bound) = match listener {
        Ok(l) => l,
        Err(e) => {
            let _ = std::fs::remove_file(&socket);
            let _ = std::fs::remove_file(&token_file);
            return Err(refuse(format!("{}: {e}", socket.display())));
        }
    };
    let st = State { answer: cfg.answer, session: cfg.session, plugin_host: cfg.plugin_host, token, children, submit, journal: cfg.journal, log: log.clone(), since_us };
    let stop = Arc::new(AtomicBool::new(false));
    let halt = stop.clone();
    let thread = std::thread::Builder::new()
        .name("dsh-control".into())
        .spawn(move || {
            for conn in listener.incoming() {
                if halt.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    handle(s, &st);
                }
            }
        })
        .map_err(|e| refuse(format!("no thread: {e}")))?;
    log.line(&format!("dsh control: serving {}", socket.display()));
    Ok(Endpoint { socket, token_file, observer_file, bound, stop, thread: Some(thread) })
}

fn clear(p: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
        Ok(m) if (m.file_type().is_socket() || m.file_type().is_file()) && m.uid() == unsafe { libc::geteuid() } => std::fs::remove_file(p).map_err(|e| format!("{}: {e}", p.display())),
        Ok(_) => Err(format!("{}: not a socket or file of this user", p.display())),
    }
}

fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("no random token: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn handle(mut s: UnixStream, st: &State) {
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    let peer = peer_pid(&s);
    let mut row = Row { verb: "-", peer, accepted: false, turn_id: None, refused: None, ids: None };
    let admitted = match (peer, st.children.instance_child()) {
        (Some(p), Some(rec)) => admit(p, &rec, st.since_us, st.plugin_host.as_ref(), &proc_of, &file_sha256),
        (None, _) => Err("no peer pid"),
        (_, None) => Err("no recorded instance child"),
    };
    if let Err(why) = admitted {
        st.log.line(&format!("dsh control: refused peer {} ({why})", peer.unwrap_or(0)));
        row.refused = Some("peer");
        journal(st, &row);
        let _ = s.write_all(b"refused: not the instance child\n");
        return;
    }
    let (status, body) = route(&s, st, &mut row);

    if s.set_nonblocking(true).is_ok() {
        let mut sink = [0u8; 4096];
        let mut left = MAX_REQUEST as usize;
        while left > 0 {
            match (&s).read(&mut sink) {
                Ok(n) if n > 0 => left = left.saturating_sub(n),
                _ => break,
            }
        }
        let _ = s.set_nonblocking(false);
    }
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Service Unavailable",
    };
    let body = body.to_string();
    journal(st, &row);
    let _ = write!(s, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

#[derive(Default)]
struct Head {
    auth: Option<String>,
    len: Option<usize>,
    refused: bool,
}

fn read_head(r: &mut impl BufRead) -> Option<(String, String, Head)> {
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.trim_end_matches(['\r', '\n']).split(' ');
    let (method, path, version) = (parts.next()?.to_string(), parts.next()?.to_string(), parts.next()?);
    if parts.next().is_some() || version != "HTTP/1.1" {
        return None;
    }
    let mut head = Head::default();
    loop {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            return Some((method, path, head));
        }
        let (k, v) = l.split_once(':')?;
        match k.trim().to_ascii_lowercase().as_str() {
            "authorization" => head.auth = Some(v.trim().to_string()),
            "content-length" if head.len.is_none() => match v.trim().parse() {
                Ok(n) => head.len = Some(n),
                Err(_) => head.refused = true,
            },
            "content-length" | "transfer-encoding" => head.refused = true,
            _ => {}
        }
    }
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Clone, Copy, PartialEq)]
enum Verb {
    Submit,
    AnswerGet,
    AnswerPost,
}

fn route(s: &UnixStream, st: &State, row: &mut Row) -> (u16, Value) {
    let refused = |row: &mut Row, code: u16, why: &'static str| {
        row.refused = Some(why);
        (code, json!({"refused": why}))
    };
    let mut r = BufReader::new(s.take(MAX_REQUEST));
    let Some((method, path, head)) = read_head(&mut r) else { return refused(row, 400, "request") };
    let verb = match (method.as_str(), path.as_str()) {
        ("POST", "/submit") => Verb::Submit,
        ("GET", "/answer-with") => Verb::AnswerGet,
        ("POST", "/answer-with") => Verb::AnswerPost,
        _ => {
            row.verb = "unknown";
            return refused(row, 404, "not_found");
        }
    };
    row.verb = if verb == Verb::Submit { "submit" } else { "answer_with" };
    let token = head.auth.as_deref().and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !same(token.as_bytes(), st.token.as_bytes()) {
        return refused(row, 401, "token");
    }
    let mut body = vec![0u8; head.len.filter(|n| !head.refused && *n as u64 <= MAX_REQUEST).unwrap_or(0)];
    if verb == Verb::AnswerGet {

        if head.refused || head.len.is_some_and(|n| n > 0) {
            return refused(row, 400, "body");
        }
        let Some(a) = &st.answer else { return refused(row, 503, "unavailable") };
        row.accepted = true;
        return (200, a.facts());
    }
    if body.is_empty() || r.read_exact(&mut body).is_err() {
        return refused(row, 400, "body");
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    if verb == Verb::AnswerPost {
        return answer_post(st, row, &v);
    }
    let fields = v.as_object().filter(|o| o.len() == 2).and_then(|o| Some((o.get("session")?.as_str()?, o.get("text")?.as_str()?.trim())));
    let Some((session, text)) = fields.filter(|(_, t)| !t.is_empty() && t.chars().count() <= crate::setup::SAY_MAX) else {
        return refused(row, 400, "body");
    };

    if text.starts_with('/') {
        return refused(row, 400, "command");
    }
    if st.session.as_deref() != Some(session) {
        return refused(row, 403, "session");
    }
    match (st.submit)(text) {
        Ok(turn_id) => {
            row.accepted = true;
            row.turn_id = Some(turn_id.clone());
            (200, json!({"accepted": true, "turn_id": turn_id}))
        }

        Err(_) => {
            st.log.line("dsh control: the session did not take the turn (unavailable)");
            row.refused = Some("unavailable");
            (503, json!({"accepted": false, "refused": "unavailable"}))
        }
    }
}

fn answer_post(st: &State, row: &mut Row, v: &Value) -> (u16, Value) {
    let refused = |row: &mut Row, code: u16, why: &'static str| {
        row.refused = Some(why);
        (code, json!({"refused": why}))
    };
    let Some(o) = v.as_object().filter(|o| o.keys().all(|k| ["choice", "effort", "ack"].contains(&k.as_str()))) else { return refused(row, 400, "body") };
    let (choice, effort, ack) = (o.get("choice"), o.get("effort"), o.get("ack"));
    if choice.is_some_and(|c| !c.is_string()) || effort.is_some_and(|e| !e.is_string()) || ack.is_some_and(|a| !a.is_boolean()) || (choice.is_none() && effort.is_none()) {
        return refused(row, 400, "body");
    }
    let (choice, effort) = (choice.and_then(Value::as_str), effort.and_then(Value::as_str));

    let id = |x: Option<&str>| x.map(|x| x.chars().take(200).collect::<String>());
    row.ids = Some(json!({"choice": id(choice), "effort": id(effort), "ack": ack == Some(&json!(true))}));
    let Some(a) = &st.answer else { return refused(row, 503, "unavailable") };
    match a.choose(choice, effort, ack == Some(&json!(true))) {
        Ok(out) => {
            row.accepted = true;
            (200, out)
        }
        Err((code, why)) => refused(row, code, why),
    }
}

fn journal(st: &State, row: &Row) {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut line = json!({"ts": ts, "verb": row.verb, "peer_pid": row.peer, "accepted": row.accepted, "turn_id": row.turn_id, "refused": row.refused});
    if let Some(ids) = &row.ids {
        line["ids"] = ids.clone();
    }
    let opened = std::fs::OpenOptions::new().create(true).append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&st.journal);
    match opened.and_then(|mut f| writeln!(f, "{line}")) {
        Ok(()) => {}
        Err(e) => st.log.line(&format!("dsh control: journal: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsh_sockdir::tests::{remove, test_id};
    use std::io::{Read, Write};
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::process::Command;
    use std::sync::Mutex;
    use std::time::Duration;

    struct Fixed(Mutex<Option<Recorded>>);
    impl Children for Fixed {
        fn instance_child(&self) -> Option<Recorded> {
            self.0.lock().unwrap().clone()
        }
    }

    fn me() -> Recorded {
        let pid = std::process::id() as i32;
        let p = proc_of(pid).expect("the kernel knows this process");
        Recorded { pid, pgid: unsafe { libc::getpgrp() }, start_time: p.start_us, command: p.path }
    }

    fn exe() -> String {
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap().display().to_string()
    }

    fn wrapper() -> PluginHost {
        let path = exe();
        PluginHost { sha256: file_sha256(&path).expect("the wrapper hashes"), path }
    }

    fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit).ok()
    }

    fn record_pid(pid: u32, what: &str) {
        if let Ok(path) = std::env::var("UFD_SPAWNED_PIDS") {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {

                let _ = f.write_all(format!("{pid} {what}\n").as_bytes());
            }
        }
    }

    struct FakeAnswer {
        facts: Mutex<serde_json::Value>,
        replying: std::sync::atomic::AtomicBool,
        picks: Mutex<Vec<String>>,
    }
    impl FakeAnswer {
        fn new() -> FakeAnswer {
            let mut o = crate::answer::options(&crate::answer::Sources { local_name: "Granite 4.1 3B".into(), deepseek: Some(("DeepSeek V4.1 Flash".into(), false)), bridge: vec![crate::dsh::Model { provider: Some("ChatGPT".into()), ready: Some(true), ..crate::dsh::Model::new("openai-codex/gpt-5.5", "GPT-5.5") }] });
            o[2]["confirm"] = serde_json::json!(true);
            FakeAnswer { facts: Mutex::new(crate::answer::facts(1, "deepseek", Some("deepseek"), None, "auto", o)), replying: std::sync::atomic::AtomicBool::new(false), picks: Mutex::new(Vec::new()) }
        }
    }
    impl AnswerWith for FakeAnswer {
        fn facts(&self) -> serde_json::Value {
            self.facts.lock().unwrap().clone()
        }
        fn choose(&self, choice: Option<&str>, effort: Option<&str>, ack: bool) -> Result<serde_json::Value, (u16, &'static str)> {
            let mut f = self.facts.lock().unwrap();
            crate::answer::check(&f, choice, effort, ack)?;
            self.picks.lock().unwrap().push(format!("{choice:?} {effort:?} {ack}"));
            let rev = f["rev"].as_u64().unwrap() + 1;
            f["rev"] = serde_json::json!(rev);
            if let Some(e) = effort {
                f["effort"]["chosen"] = serde_json::json!(e);
            }
            let after = choice.is_some_and(|c| f["chosen"] != c) && self.replying.load(std::sync::atomic::Ordering::SeqCst);
            if let Some(c) = choice {
                f["chosen"] = serde_json::json!(c);
                if !after {
                    f["answering"] = serde_json::json!(c);
                }
            }
            Ok(serde_json::json!({"rev": rev, "after_reply": after}))
        }
    }

    struct Harness {
        answer: Arc<FakeAnswer>,
        dir: PathBuf,
        logs: PathBuf,
        session: String,
        children: Arc<Fixed>,
        got: Arc<Mutex<Vec<String>>>,

        down: Arc<std::sync::atomic::AtomicBool>,
        ep: Option<Endpoint>,
    }

    impl Harness {
        fn start(rec: Option<Recorded>, plugin_host: Option<PluginHost>) -> Harness {
            let id = test_id();
            let dir = crate::dsh_sockdir::ensure(&id).unwrap();
            let logs = std::env::temp_dir().join(format!("{id}-logs"));
            std::fs::create_dir_all(&logs).unwrap();
            let log = crate::Log::open_in(None, &logs);
            let session = format!("session-{id}");
            let children = Arc::new(Fixed(Mutex::new(rec)));
            let got = Arc::new(Mutex::new(Vec::new()));
            let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (sink, off) = (got.clone(), down.clone());
            let submit: Submit = Arc::new(move |t: &str| {
                if off.load(std::sync::atomic::Ordering::SeqCst) {

                    return Err(format!("no chat took {t}"));
                }
                let mut g = sink.lock().unwrap();
                g.push(t.to_string());
                Ok(format!("cmd:test:{}", g.len()))
            });
            let answer = Arc::new(FakeAnswer::new());
            let cfg = Config { answer: Some(answer.clone()), dir: dir.clone(), session: Some(session.clone()), plugin_host, journal: logs.join("control.jsonl") };
            let ep = serve(cfg, children.clone(), submit, &log).unwrap();
            Harness { answer, dir, logs, session, children, got, down, ep: Some(ep) }
        }
        fn ep(&self) -> &Endpoint {
            self.ep.as_ref().unwrap()
        }
        fn token(&self) -> String {
            std::fs::read_to_string(self.ep().token_file()).unwrap().trim().to_string()
        }
        fn submit(&self, token: Option<&str>, session: &str, text: &str) -> (u16, String) {
            let body = serde_json::json!({"session": session, "text": text}).to_string();
            call(self.ep().socket(), "POST", "/submit", token, Some(&body))
        }
        fn journal(&self) -> Vec<serde_json::Value> {
            std::fs::read_to_string(self.logs.join("control.jsonl")).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        }
        fn texts(&self) -> String {
            let j = std::fs::read_to_string(self.logs.join("control.jsonl")).unwrap_or_default();
            let a = std::fs::read_to_string(self.logs.join("app.log")).unwrap_or_default();
            format!("{j}\n{a}")
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            if let Some(ep) = self.ep.take() {
                ep.stop();
            }
            remove(&self.dir);
            remove(&self.logs);
        }
    }

    fn call(sock: &Path, method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> (u16, String) {
        let mut s = UnixStream::connect(sock).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: unfold\r\n");
        if let Some(t) = token {
            req.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if let Some(b) = body {
            req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", b.len()));
        }
        req.push_str("\r\n");
        req.push_str(body.unwrap_or(""));
        let _ = s.write_all(req.as_bytes());
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.strip_prefix("HTTP/1.1 ").and_then(|r| r.get(..3)).and_then(|c| c.parse().ok()).unwrap_or(0);
        (status, out)
    }

    fn body_of(resp: &str) -> serde_json::Value {
        serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap_or_default()
    }

    #[test]
    fn a_valid_submit_from_the_recorded_child_is_accepted_once() {
        let h = Harness::start(Some(me()), None);
        let text = format!("oi from the session {}", std::process::id());
        let (code, resp) = h.submit(Some(&h.token()), &h.session, &text);
        assert_eq!(code, 200, "{resp}");
        let b = body_of(&resp);
        assert_eq!(b["accepted"], true);
        assert_eq!(b["turn_id"], "cmd:test:1", "the session's own command id: {b}");
        assert_eq!(*h.got.lock().unwrap(), [text]);
        assert_eq!(h.journal()[0]["turn_id"], "cmd:test:1");
    }

    #[test]
    fn an_owner_command_is_refused_and_never_submitted() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        for cmd in ["/effect release e1 tok", "/research start x", "/grant revoke all", "/part off brain", "/dsh sub undo c1", "/remember this", "/memory confirm m1", "  /memory correct m1 x", "/"] {
            let (code, resp) = h.submit(Some(&t), &h.session, cmd);
            assert_eq!(code, 400, "{cmd}: {resp}");
            assert_eq!(body_of(&resp)["refused"], "command", "{cmd}");
        }
        assert!(h.got.lock().unwrap().is_empty());
        assert!(h.journal().iter().all(|r| r["refused"] == "command" && r["accepted"] == false));
        assert_eq!(h.submit(Some(&t), &h.session, "a/b and 1/2 are fine").0, 200, "only a leading slash");
    }

    #[test]
    fn a_session_that_cannot_take_the_turn_is_503_and_not_accepted() {
        let h = Harness::start(Some(me()), None);
        h.down.store(true, std::sync::atomic::Ordering::SeqCst);
        let (code, resp) = h.submit(Some(&h.token()), &h.session, "oi");
        assert_eq!(code, 503, "{resp}");
        assert_eq!(body_of(&resp), serde_json::json!({"accepted": false, "refused": "unavailable"}));
        let rows = h.journal();
        assert_eq!((rows.len(), rows[0]["accepted"].clone(), rows[0]["refused"].clone(), rows[0]["turn_id"].clone()), (1, false.into(), "unavailable".into(), serde_json::Value::Null));
    }

    #[test]
    fn with_no_unfold_session_every_submit_is_403() {
        let h = Harness::start(Some(me()), None);
        let log = crate::Log::open_in(None, &h.logs);
        let id = test_id();
        let dir = crate::dsh_sockdir::ensure(&id).unwrap();
        let cfg = Config { answer: None, dir: dir.clone(), session: None, plugin_host: None, journal: h.logs.join("none.jsonl") };
        let ep = serve(cfg, h.children.clone(), Arc::new(|_: &str| Ok("x".into())), &log).unwrap();
        let token = std::fs::read_to_string(ep.token_file()).unwrap();
        let body = serde_json::json!({"session": "", "text": "oi"}).to_string();
        assert_eq!(call(ep.socket(), "POST", "/submit", Some(token.trim()), Some(&body)).0, 403);
        ep.stop();
        remove(&dir);
    }

    #[test]
    fn a_missing_or_wrong_token_is_refused_with_no_submit() {
        let h = Harness::start(Some(me()), None);
        let wrong: String = h.token().chars().rev().collect();
        for t in [None, Some(wrong.as_str()), Some("")] {
            let (code, resp) = h.submit(t, &h.session, "oi");
            assert_eq!(code, 401, "{resp}");
        }
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn a_session_that_is_not_the_unfold_session_is_refused() {
        let h = Harness::start(Some(me()), None);
        let (code, resp) = h.submit(Some(&h.token()), "another-session", "oi");
        assert_eq!(code, 403, "{resp}");
        assert_eq!(body_of(&resp)["refused"], "session");
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn a_bad_body_is_refused_with_no_submit() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        let s = &h.session;
        let long = "a".repeat(crate::setup::SAY_MAX + 1);
        for body in [
            "not json".to_string(),
            serde_json::json!({"session": s}).to_string(),
            serde_json::json!({"session": s, "text": "   "}).to_string(),
            serde_json::json!({"session": s, "text": long}).to_string(),
            serde_json::json!({"session": s, "text": "oi", "shell": "ls"}).to_string(),
        ] {
            let (code, resp) = call(h.ep().socket(), "POST", "/submit", Some(&t), Some(&body));
            assert_eq!(code, 400, "{body}: {resp}");
        }
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn every_other_method_and_path_is_404_with_no_side_effect() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        let body = serde_json::json!({"session": h.session, "text": "oi"}).to_string();
        let walk = [
            ("GET", "/submit"), ("PUT", "/submit"), ("DELETE", "/submit"), ("PATCH", "/submit"), ("OPTIONS", "/submit"), ("HEAD", "/submit"),
            ("POST", "/"), ("GET", "/"), ("POST", "/submit/"), ("POST", "/submit?x=1"), ("POST", "/SUBMIT"), ("POST", "//submit"),
            ("POST", "/v1/commands"), ("GET", "/v1/status"), ("POST", "/cancel"), ("POST", "/exec"), ("POST", "/shell"), ("POST", "/run"),
            ("GET", "/file"), ("POST", "/file"), ("GET", "/token"), ("POST", "/fetch"), ("POST", "/proxy"), ("GET", "/journal"),
            ("POST", "/submit/../exec"), ("CONNECT", "unfold:80"),
        ];
        for (m, p) in walk {
            let (code, resp) = call(h.ep().socket(), m, p, Some(&t), Some(&body));
            assert_eq!(code, 404, "{m} {p}: {resp}");
        }
        assert!(h.got.lock().unwrap().is_empty(), "no submit");
        assert!(h.journal().iter().all(|r| r["accepted"] == false && r["verb"] == "unknown"));
        assert!(!h.texts().contains("/exec") && !h.texts().contains("/shell"), "paths are never journaled");
    }

    #[test]
    fn a_process_that_is_not_the_recorded_child_is_refused_even_with_the_token() {
        let mut other = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        record_pid(other.id(), "sleep: a recorded child that is not the peer");
        let p = proc_of(other.id() as i32).unwrap();
        let rec = Recorded { pid: other.id() as i32, pgid: other.id() as i32, start_time: p.start_us, command: p.path };
        let h = Harness::start(Some(rec), Some(wrapper()));
        let (code, resp) = h.submit(Some(&h.token()), &h.session, "oi");
        unsafe { libc::kill(other.id() as i32, libc::SIGKILL) };
        let _ = other.wait();
        assert_eq!(code, 0, "{resp}");
        assert_eq!(resp.lines().count(), 1, "one line and nothing else: {resp:?}");
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn a_recycled_pid_with_another_start_time_is_refused() {
        let mut rec = me();
        rec.start_time += 1;
        let h = Harness::start(Some(rec), None);
        let (code, resp) = h.submit(Some(&h.token()), &h.session, "oi");
        assert_eq!((code, resp.lines().count()), (0, 1), "{resp}");
        assert!(h.got.lock().unwrap().is_empty());
        let (code, _) = {
            *h.children.0.lock().unwrap() = None;
            h.submit(Some(&h.token()), &h.session, "oi")
        };
        assert_eq!(code, 0, "no recorded child: every peer is refused");
    }

    fn from_child(h: &Harness, hop: &str) -> u16 {
        let out = h.logs.join(format!("hop-{hop}.status"));
        let mut c = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "dsh_control::tests::peer_hop", "--ignored", "--quiet", "--test-threads=1"])
            .env("UFD_TEST_HOP", hop)
            .env("UFD_TEST_SOCKET", h.ep().socket())
            .env("UFD_TEST_TOKEN_FILE", h.ep().token_file())
            .env("UFD_TEST_SESSION", &h.session)
            .env("UFD_TEST_OUT", &out)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        record_pid(c.id(), &format!("peer_hop {hop}"));
        assert!(c.wait().unwrap().success());
        std::fs::read_to_string(&out).unwrap().trim().parse().unwrap()
    }

    #[test]
    #[ignore = "a helper: the plugin-host tests run it as their own child"]
    fn peer_hop() {
        let Ok(hop) = std::env::var("UFD_TEST_HOP") else { return };
        if hop == "serve" {

            let _l = std::os::unix::net::UnixListener::bind(std::env::var("UFD_TEST_SOCKET").unwrap()).unwrap();
            println!("ufd-ready");
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            return;
        }
        if hop == "2" {
            let mut c = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "dsh_control::tests::peer_hop", "--ignored", "--quiet", "--test-threads=1"])
                .env("UFD_TEST_HOP", "1")
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            record_pid(c.id(), "peer_hop 1 (grandchild)");
            assert!(c.wait().unwrap().success());
            return;
        }
        let v = |k: &str| std::env::var(k).unwrap();
        let token = std::fs::read_to_string(v("UFD_TEST_TOKEN_FILE")).unwrap();
        let body = serde_json::json!({"session": v("UFD_TEST_SESSION"), "text": "from a plugin host"}).to_string();
        let (code, _) = call(Path::new(&v("UFD_TEST_SOCKET")), "POST", "/submit", Some(token.trim()), Some(&body));
        std::fs::write(v("UFD_TEST_OUT"), code.to_string()).unwrap();
    }

    #[test]
    fn a_second_serve_takes_over_and_the_old_drop_leaves_it_alone() {
        let mut h = Harness::start(Some(me()), None);
        let log = crate::Log::open_in(None, &h.logs);
        let cfg = Config { answer: None, dir: h.dir.clone(), session: Some(h.session.clone()), plugin_host: None, journal: h.logs.join("b.jsonl") };
        let b = serve(cfg, h.children.clone(), Arc::new(|_: &str| Ok("cmd:b:1".into())), &log).unwrap();
        b.publish_observer("127.0.0.1:7801".parse().unwrap()).unwrap();
        let a = h.ep.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(a);
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(IO_TIMEOUT + Duration::from_secs(1)).is_ok(), "the old endpoint's drop returns");
        for f in [b.socket().to_path_buf(), b.token_file().to_path_buf(), h.dir.join(OBSERVER_FILE)] {
            assert!(std::fs::symlink_metadata(&f).is_ok(), "{} is left in place", f.display());
        }
        let token = std::fs::read_to_string(b.token_file()).unwrap();
        let body = serde_json::json!({"session": h.session, "text": "oi"}).to_string();
        let (code, resp) = call(b.socket(), "POST", "/submit", Some(token.trim()), Some(&body));
        assert_eq!((code, body_of(&resp)["turn_id"].clone()), (200, "cmd:b:1".into()), "the new endpoint still answers: {resp}");
        b.stop();
    }

    #[test]
    fn serve_refuses_a_live_socket_another_process_serves() {
        let id = test_id();
        let dir = crate::dsh_sockdir::ensure(&id).unwrap();
        let logs = std::env::temp_dir().join(format!("{id}-logs"));
        let sock = dir.join(SOCKET);
        let mut other = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "dsh_control::tests::peer_hop", "--ignored", "--nocapture", "--test-threads=1"])
            .env("UFD_TEST_HOP", "serve")
            .env("UFD_TEST_SOCKET", &sock)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        record_pid(other.id(), "peer_hop serve: another process's endpoint");
        let (out, d, l, s) = (other.stdout.take().unwrap(), dir.clone(), logs.clone(), sock.clone());
        let run = within(Duration::from_secs(10), move || {

            if !std::io::BufReader::new(out).lines().map_while(Result::ok).any(|line| line.ends_with("ufd-ready")) {
                return Err("the helper never served".to_string());
            }
            let log = crate::Log::open_in(None, &l);
            let cfg = Config { answer: None, dir: d, session: Some("s".into()), plugin_host: None, journal: l.join("control.jsonl") };
            let served = serve(cfg, Arc::new(Fixed(Mutex::new(Some(me())))), Arc::new(|_: &str| Ok("x".into())), &log);
            let still = std::fs::symlink_metadata(&s).is_ok_and(|m| m.file_type().is_socket() && m.permissions().mode() & 0o777 != 0o600);
            Ok((served.err(), still))
        });

        unsafe { libc::kill(other.id() as i32, libc::SIGKILL) };
        let _ = other.wait();
        remove(&dir);
        remove(&logs);
        let (err, still) = run.expect("bounded: finished within 10 s").unwrap();
        let e = err.expect("refused while another process serves the socket");
        assert!(e.contains("live"), "{e}");
        assert!(still, "the other process's socket is left alone");
    }

    #[test]
    fn the_recorded_childs_direct_child_with_the_plugin_host_command_is_admitted() {
        let h = Harness::start(Some(me()), Some(wrapper()));
        assert_eq!(from_child(&h, "1"), 200);
        assert_eq!(*h.got.lock().unwrap(), ["from a plugin host"]);
    }

    #[test]
    fn a_grandchild_in_the_childs_group_is_refused_even_with_the_token() {
        let h = Harness::start(Some(me()), Some(wrapper()));
        assert_eq!(from_child(&h, "2"), 0);
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn a_direct_child_with_another_command_is_refused() {
        let sh = PluginHost { path: "/bin/sh".into(), sha256: file_sha256("/bin/sh").unwrap() };
        let h = Harness::start(Some(me()), Some(sh));
        assert_eq!(from_child(&h, "1"), 0);
        let h = Harness::start(Some(me()), None);
        assert_eq!(from_child(&h, "1"), 0, "no plugin host: only the child itself");
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn a_direct_child_of_a_recycled_parent_is_refused() {
        let mut rec = me();
        rec.start_time += 1;
        let h = Harness::start(Some(rec), Some(wrapper()));
        assert_eq!(from_child(&h, "1"), 0);
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn admit_matches_parent_pid_start_time_and_command_all_three() {
        let rec = Recorded { pid: 100, pgid: 100, start_time: 5_000, command: "/launcher".into() };
        let procs = |pid: i32| match pid {
            100 => Some(Proc { ppid: 1, start_us: 5_000, path: "/dsh/node".into() }),
            200 => Some(Proc { ppid: 100, start_us: 6_000, path: "/pack/plugin-host".into() }),
            201 => Some(Proc { ppid: 100, start_us: 4_000, path: "/pack/plugin-host".into() }),
            202 => Some(Proc { ppid: 100, start_us: 6_000, path: "/tool".into() }),
            203 => Some(Proc { ppid: 100, start_us: 6_000, path: "/dsh/node".into() }),
            300 => Some(Proc { ppid: 200, start_us: 7_000, path: "/pack/plugin-host".into() }),
            _ => None,
        };
        let host = PluginHost { path: "/pack/plugin-host".into(), sha256: "a".repeat(64) };
        let hash = |p: &str| (p == "/pack/plugin-host").then(|| "a".repeat(64));
        let host = Some(&host);
        let since = 4_000;
        assert_eq!(admit(100, &rec, since, None, &procs, &hash), Ok(()), "the child itself, after its exec");
        assert_eq!(admit(200, &rec, since, host, &procs, &hash), Ok(()));
        assert!(admit(200, &rec, since, None, &procs, &hash).is_err(), "no plugin host named");
        assert!(admit(201, &rec, since, host, &procs, &hash).is_err(), "started before its parent");
        assert!(admit(202, &rec, since, host, &procs, &hash).is_err(), "another command");
        assert_eq!(admit(203, &rec, since, host, &procs, &hash), Err("not the plugin host's wrapper"), "a plain node child running the same entry");
        assert!(admit(300, &rec, since, host, &procs, &hash).is_err(), "a grandchild in the group");
        assert!(admit(999, &rec, since, host, &procs, &hash).is_err(), "gone");
        let changed = |_: &str| Some("b".repeat(64));
        assert_eq!(admit(200, &rec, since, host, &procs, &changed), Err("the plugin host's wrapper changed"));
        assert!(admit(200, &rec, since, host, &procs, &|_: &str| None).is_err(), "an unreadable wrapper");
        let old = Recorded { start_time: 4_999, ..rec.clone() };
        assert!(admit(100, &old, since, None, &procs, &hash).is_err() && admit(200, &old, since, host, &procs, &hash).is_err(), "recycled");

        assert_eq!(admit(100, &rec, 5_001, None, &procs, &hash), Err("recorded before this app started"));
        assert_eq!(admit(200, &rec, 5_001, host, &procs, &hash), Err("recorded before this app started"));
    }

    #[test]
    fn a_plugin_host_wrapper_whose_hash_changed_is_refused() {
        let h = Harness::start(Some(me()), Some(PluginHost { path: exe(), sha256: "0".repeat(64) }));
        assert_eq!(from_child(&h, "1"), 0);
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[test]
    fn the_journal_has_one_row_per_call_and_never_the_text_or_the_token() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        let text = format!("texto-privado-{}", std::process::id());
        assert_eq!(h.submit(Some(&t), &h.session, &text).0, 200);
        assert_eq!(h.submit(None, &h.session, &text).0, 401);
        assert_eq!(h.submit(Some(&t), "other", &text).0, 403);
        assert_eq!(call(h.ep().socket(), "GET", "/token", Some(&t), None).0, 404);
        assert_eq!(h.submit(Some(&t), &h.session, &format!("/effect {text}")).0, 400, "an owner command");
        let bad = serde_json::json!({"session": h.session, "text": text, "extra": text}).to_string();
        assert_eq!(call(h.ep().socket(), "POST", "/submit", Some(&t), Some(&bad)).0, 400, "a bad body");
        h.down.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(h.submit(Some(&t), &h.session, &text).0, 503, "the session took no turn");
        h.down.store(false, std::sync::atomic::Ordering::SeqCst);
        let mut rec = me();
        rec.start_time += 1;
        *h.children.0.lock().unwrap() = Some(rec);
        assert_eq!(h.submit(Some(&t), &h.session, &text).0, 0);
        let rows = h.journal();
        assert_eq!(rows.len(), 8, "{rows:?}");
        let codes: Vec<&str> = rows.iter().map(|r| r["refused"].as_str().unwrap_or("-")).collect();
        assert_eq!(codes, ["-", "token", "session", "not_found", "command", "body", "unavailable", "peer"]);
        let fixed = ["peer", "request", "not_found", "token", "body", "command", "session", "unavailable"];
        assert!(rows.iter().all(|r| r["refused"].is_null() || r["refused"].as_str().is_some_and(|c| fixed.contains(&c))), "{rows:?}");
        let me = std::process::id() as i64;
        for r in &rows {
            let mut keys: Vec<&str> = r.as_object().unwrap().keys().map(|k| k.as_str()).collect();
            keys.sort();
            assert_eq!(keys, ["accepted", "peer_pid", "refused", "ts", "turn_id", "verb"], "{r}");
            assert_eq!(r["peer_pid"].as_i64(), Some(me));
            assert!(r["ts"].as_u64().is_some());
        }
        assert_eq!(rows.iter().filter(|r| r["accepted"] == true).count(), 1);
        assert!(rows[0]["turn_id"].is_string() && rows[1..].iter().all(|r| r["turn_id"].is_null()));
        let all = h.texts();
        assert_eq!(all.matches(&text).count(), 0, "no submitted text in the journal or the log");
        assert_eq!(all.matches(&t).count(), 0, "no token bytes");
        assert_eq!(all.matches(&t[..16]).count(), 0, "no token prefix");
    }

    #[test]
    fn the_socket_is_0600_its_dir_0700_and_no_tcp_is_served() {
        let h = Harness::start(Some(me()), None);
        let m = std::fs::symlink_metadata(h.ep().socket()).unwrap();
        assert!(m.file_type().is_socket());
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::symlink_metadata(&h.dir).unwrap().permissions().mode() & 0o777, 0o700);
        let tm = std::fs::symlink_metadata(h.ep().token_file()).unwrap();
        assert!(tm.file_type().is_file());
        assert_eq!(tm.permissions().mode() & 0o777, 0o600);
        assert_eq!(h.ep().socket(), h.dir.join(SOCKET));
        assert!(h.ep().socket().as_os_str().len() <= crate::dsh_sockdir::SUN_PATH_MAX);
    }

    #[test]
    fn the_token_rotates_per_start_and_stop_removes_the_socket_and_token() {
        let h = Harness::start(Some(me()), None);
        let first = h.token();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        let mut h = h;
        let ep = h.ep.take().unwrap();
        let (sock, tok) = (ep.socket().to_path_buf(), ep.token_file().to_path_buf());
        ep.stop();
        assert!(std::fs::symlink_metadata(&sock).is_err() && std::fs::symlink_metadata(&tok).is_err());
        let log = crate::Log::open_in(None, &h.logs);
        let cfg = Config { answer: None, dir: h.dir.clone(), session: Some(h.session.clone()), plugin_host: None, journal: h.logs.join("control.jsonl") };
        h.ep = Some(serve(cfg, h.children.clone(), Arc::new(|_: &str| Ok("x".into())), &log).unwrap());
        assert_ne!(h.token(), first);
    }

    #[test]
    fn a_refused_dir_means_the_endpoint_is_not_served() {
        let id = test_id();
        let dir = crate::dsh_sockdir::ensure(&id).unwrap();
        let log = crate::Log::open_in(None, &std::env::temp_dir().join(format!("{id}-logs")));
        let children: Arc<dyn Children> = Arc::new(Fixed(Mutex::new(Some(me()))));
        let cfg = |d: &Path| Config { answer: None, dir: d.to_path_buf(), session: Some("s".into()), plugin_host: None, journal: std::env::temp_dir().join(format!("{id}-logs/control.jsonl")) };
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(serve(cfg(&dir), children.clone(), Arc::new(|_: &str| Ok("x".into())), &log).is_err());
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "no socket, no token");
        remove(&dir);
        let target = std::env::temp_dir().join(format!("{id}-target"));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&target, &dir).unwrap();
        assert!(serve(cfg(&dir), children, Arc::new(|_: &str| Ok("x".into())), &log).is_err());
        assert!(std::fs::read_dir(&target).unwrap().next().is_none(), "nothing through the link");
        remove(&dir);
        remove(&target);
        remove(&std::env::temp_dir().join(format!("{id}-logs")));
    }

    #[test]
    fn the_child_env_is_three_paths_and_the_observer_file_exists_only_while_published() {
        let h = Harness::start(Some(me()), None);
        let env = h.ep().child_env();
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, [ENV_OBSERVER, ENV_SOCKET, ENV_TOKEN_FILE]);
        let file = h.dir.join(OBSERVER_FILE);
        assert_eq!([Path::new(&env[0].1), Path::new(&env[1].1), Path::new(&env[2].1)], [file.as_path(), h.ep().socket(), h.ep().token_file()]);
        assert!(env.iter().all(|(_, v)| !v.contains(&h.token())), "paths only");
        assert!(std::fs::symlink_metadata(&file).is_err(), "nothing published before the app holds the lease");
        let observer: SocketAddr = "127.0.0.1:7801".parse().unwrap();
        h.ep().publish_observer(observer).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "127.0.0.1:7801\n");
        assert_eq!(std::fs::symlink_metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        h.ep().publish_observer("127.0.0.1:7802".parse().unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "127.0.0.1:7802\n", "a new agentd's port replaces the old one");
        assert!(h.ep().publish_observer("192.168.1.5:7801".parse().unwrap()).is_err(), "loopback only");
        h.ep().withdraw_observer();
        assert!(std::fs::symlink_metadata(&file).is_err(), "withdrawn before the lease is let go");
        h.ep().publish_observer(observer).unwrap();
        let mut h = h;
        h.ep.take().unwrap().stop();
        assert!(std::fs::symlink_metadata(&file).is_err(), "stop withdraws it too");
    }

    fn pick(h: &Harness, token: Option<&str>, body: &str) -> (u16, serde_json::Value) {
        let (code, resp) = call(h.ep().socket(), "POST", "/answer-with", token, Some(body));
        (code, body_of(&resp))
    }

    #[test]
    fn answer_with_get_returns_the_facts_to_the_admitted_peer_with_the_token() {
        let h = Harness::start(Some(me()), None);
        let (code, resp) = call(h.ep().socket(), "GET", "/answer-with", Some(&h.token()), None);
        assert_eq!(code, 200, "{resp}");
        let f = body_of(&resp);
        assert_eq!((f["chosen"].clone(), f["answering"].clone(), f["effort"]["levels"].clone()), (serde_json::json!("deepseek"), serde_json::json!("deepseek"), serde_json::json!(["auto", "off", "low", "high", "max"])));
        assert_eq!(f["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect::<Vec<_>>(), ["granite", "deepseek", "openai-codex/gpt-5.5"]);
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "{resp}");
        let j = h.journal();
        assert_eq!((j[0]["verb"].clone(), j[0]["accepted"].clone()), (serde_json::json!("answer_with"), serde_json::json!(true)));
    }

    #[test]
    fn answer_with_refuses_a_missing_or_wrong_token_and_reads_nothing() {
        let h = Harness::start(Some(me()), None);
        let wrong = "0".repeat(64);
        for t in [None, Some(wrong.as_str()), Some("")] {
            assert_eq!(call(h.ep().socket(), "GET", "/answer-with", t, None).0, 401, "{t:?}");
            assert_eq!(pick(&h, t, r#"{"choice": "granite"}"#).0, 401, "{t:?}");
        }
        assert!(h.answer.picks.lock().unwrap().is_empty(), "no pick reached the app");
        assert_eq!(h.answer.facts()["chosen"], "deepseek");
        assert!(!h.texts().contains(&h.token()), "the token is never logged");
    }

    #[test]
    fn answer_with_refuses_a_process_that_is_not_the_recorded_child() {
        let mut other = me();
        other.pid = 1;
        let h = Harness::start(Some(other), None);
        let (code, resp) = call(h.ep().socket(), "GET", "/answer-with", Some(&h.token()), None);
        assert!(code == 0 && resp.starts_with("refused: not the instance child"), "{code} {resp}");
        let (code, resp) = call(h.ep().socket(), "POST", "/answer-with", Some(&h.token()), Some(r#"{"choice": "granite"}"#));
        assert!(code == 0 && resp.starts_with("refused: not the instance child"), "{code} {resp}");
        assert!(h.answer.picks.lock().unwrap().is_empty());
        assert!(h.journal().iter().all(|r| r["refused"] == "peer"));
    }

    #[test]
    fn answer_with_bodies_follow_the_contract() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        for bad in [r#"{}"#, r#"{"ack": true}"#, r#"{"choice": 1}"#, r#"{"effort": true}"#, r#"{"choice": "granite", "ack": "yes"}"#, r#"{"choice": "granite", "model": "x"}"#, r#"["granite"]"#, "not json", r#""granite""#] {
            assert_eq!(pick(&h, Some(&t), bad), (400, serde_json::json!({"refused": "body"})), "{bad}");
        }
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "deepseek-official/deepseek-flash"}"#), (409, serde_json::json!({"refused": "not_offered"})), "never DeepSeek through the bridge");
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "glm/glm-5"}"#), (409, serde_json::json!({"refused": "not_offered"})));
        assert_eq!(pick(&h, Some(&t), r#"{"effort": "medium"}"#), (409, serde_json::json!({"refused": "not_offered"})), "a level not offered");
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "openai-codex/gpt-5.5"}"#), (409, serde_json::json!({"refused": "needs_ack"})));
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "openai-codex/gpt-5.5", "ack": false}"#), (409, serde_json::json!({"refused": "needs_ack"})));
        assert!(h.answer.picks.lock().unwrap().is_empty(), "nothing refused reached the app");
        let (code, resp) = call(h.ep().socket(), "POST", "/answer-with", Some(&t), Some(r#"{"choice": "glm/glm-5"}"#));
        assert!(code == 409 && resp.starts_with("HTTP/1.1 409 Conflict\r\n"), "{resp}");
        assert_eq!(call(h.ep().socket(), "GET", "/answer-with", Some(&t), Some(r#"{"choice": "granite"}"#)).0, 400, "a GET carries no body");
        for (m, p) in [("PUT", "/answer-with"), ("DELETE", "/answer-with"), ("GET", "/answer-with/"), ("POST", "/answer_with"), ("GET", "/ANSWER-WITH")] {
            assert_eq!(call(h.ep().socket(), m, p, Some(&t), Some("{}")).0, 404, "{m} {p}");
        }
    }

    #[test]
    fn a_pick_answers_after_the_reply_in_progress_and_a_level_at_once() {
        let h = Harness::start(Some(me()), None);
        let t = h.token();
        assert_eq!(pick(&h, Some(&t), r#"{"effort": "high"}"#), (200, serde_json::json!({"rev": 2, "after_reply": false})), "a level: no restart");
        h.answer.replying.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "granite"}"#), (200, serde_json::json!({"rev": 3, "after_reply": true})), "a new model waits for the reply in progress");
        let f = h.answer.facts();
        assert_eq!((f["chosen"].clone(), f["answering"].clone(), f["effort"]["chosen"].clone()), (serde_json::json!("granite"), serde_json::json!("deepseek"), serde_json::json!("high")));
        h.answer.replying.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "openai-codex/gpt-5.5", "ack": true}"#), (200, serde_json::json!({"rev": 4, "after_reply": false})));
        assert_eq!(pick(&h, Some(&t), r#"{"choice": "openai-codex/gpt-5.5", "effort": "low", "ack": true}"#).1["after_reply"], false, "the same model: nothing to wait for");
        let j = h.journal();
        let rows: Vec<_> = j.iter().filter(|r| r["verb"] == "answer_with").collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[1]["ids"], serde_json::json!({"choice": "granite", "effort": null, "ack": false}), "the ids only");
        assert!(rows.iter().all(|r| r.as_object().unwrap().keys().all(|k| ["ts", "verb", "peer_pid", "accepted", "turn_id", "refused", "ids"].contains(&k.as_str()))));
        assert!(h.got.lock().unwrap().is_empty(), "no pick is ever a turn");
    }

    #[test]
    fn with_no_answer_backend_both_verbs_are_503() {
        let id = test_id();
        let dir = crate::dsh_sockdir::ensure(&id).unwrap();
        let logs = std::env::temp_dir().join(format!("{id}-logs"));
        std::fs::create_dir_all(&logs).unwrap();
        let log = crate::Log::open_in(None, &logs);
        let submit: Submit = Arc::new(|_: &str| Ok("c".into()));
        let ep = serve(Config { answer: None, dir: dir.clone(), session: Some("s".into()), plugin_host: None, journal: logs.join("control.jsonl") }, Arc::new(Fixed(Mutex::new(Some(me())))), submit, &log).unwrap();
        let t = std::fs::read_to_string(ep.token_file()).unwrap().trim().to_string();
        assert_eq!(call(ep.socket(), "GET", "/answer-with", Some(&t), None).0, 503);
        assert_eq!(call(ep.socket(), "POST", "/answer-with", Some(&t), Some(r#"{"choice": "granite"}"#)).0, 503);
        ep.stop();
        remove(&dir);
        remove(&logs);
    }
}
