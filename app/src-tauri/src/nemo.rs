

use crate::dictation::Recognizer;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub trait Launch: Send {

    fn spawn(&mut self, port: u16, key: &str) -> Result<Child, String>;
}

pub const NO_NETWORK_PROFILE: &str = "(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote unix-socket))(allow network-outbound (remote ip \"localhost:*\"))";

pub struct Serve {
    pub bin: PathBuf,
    pub model: PathBuf,

    pub itn: Option<PathBuf>,

    pub home: PathBuf,

    pub warmup: bool,

    pub sandbox: bool,
}

impl Launch for Serve {
    fn spawn(&mut self, port: u16, key: &str) -> Result<Child, String> {
        let mut c = if self.sandbox {
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(NO_NETWORK_PROFILE).arg(&self.bin);
            c
        } else {
            Command::new(&self.bin)
        };
        c.args(["serve", "--host", "127.0.0.1", "--port"]).arg(port.to_string()).args(["--no-ui", "--threads", "2", "--device", "metal", "--endpointing", "--asr-model"]).arg(&self.model);
        if let Some(itn) = &self.itn {
            c.arg("--itn-model-dir").arg(itn);
        }
        if !self.warmup {
            c.arg("--no-warmup");
        }
        own_group(&mut c);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("NEMO_SPEECH_HTTP_API_KEY", key)
            .current_dir(&self.home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("nemo-speech: {e}"))
    }
}

pub fn own_group(c: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    unsafe {
        c.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            Ok(())
        });
    }
    c.process_group(0)
}

pub struct Sidecar {
    child: Option<Child>,

    pgid: Option<libc::pid_t>,
    pub port: u16,
    key: String,
}

pub struct Start<'a> {
    pub launch: &'a mut dyn Launch,
    pub pidfile: PathBuf,

    pub bin: PathBuf,
    pub ready_within: Duration,

    pub port: &'a mut dyn FnMut() -> Result<u16, String>,

    pub closing: &'a AtomicBool,
}

const TRIES: usize = 3;

impl Sidecar {
    pub fn start(s: Start, log: &dyn Fn(&str)) -> Result<Sidecar, String> {
        let mut last = String::new();
        for _ in 0..TRIES {
            if s.closing.load(Ordering::SeqCst) {
                return Err(QUITTING.into());
            }
            let port = (s.port)()?;
            let key = new_key()?;
            let mut child = s.launch.spawn(port, &key)?;
            let pgid = record(&s.pidfile, child.id(), &s.bin);
            log(&format!("dictation: nemo-speech started (pid {}, 127.0.0.1:{port})", child.id()));
            match wait_ready(&mut child, port, &key, s.ready_within, s.closing) {
                Ok(()) => return Ok(Sidecar { child: Some(child), pgid, port, key }),
                Err(e) => {
                    log(&format!("dictation: nemo-speech pid {} not ready: {e}; {}", child.id(), stop_child(&mut child, pgid)));
                    last = e;
                }
            }
        }
        Err(format!("nemo-speech did not start: {last}"))
    }

    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, |c| c.id())
    }

    pub fn alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    pub fn stop(mut self) -> String {
        let pgid = self.pgid;
        self.child.take().map(|mut c| stop_child(&mut c, pgid)).unwrap_or_default()
    }

    pub fn stream(&self, lang: &str) -> Result<Stream, String> {
        Stream::open(self.port, &self.key, lang)
    }

    pub fn settled_stream(&self, lang: &str) -> Result<crate::dictation_settle::Settled<Stream, impl FnMut(&[f32], bool) -> Result<Vec<crate::dictation_settle::Word>, String> + use<>>, String> {
        self.settled_stream_with_endpoint(lang, None)
    }

    pub fn settled_stream_with_endpoint(&self, lang: &str, endpoint_ms: Option<u32>) -> Result<crate::dictation_settle::Settled<Stream, impl FnMut(&[f32], bool) -> Result<Vec<crate::dictation_settle::Word>, String> + use<>>, String> {
        let (port, key, lang_owned) = (self.port, self.key.clone(), lang.to_string());
        Ok(crate::dictation_settle::Settled::new(Stream::open_with_endpoint(self.port, &self.key, lang, endpoint_ms)?, move |audio: &[f32], last: bool| transcribe_words(port, &key, &lang_owned, audio, if last { LAST_PASS_WITHIN } else { PASS_WITHIN })))
    }

    pub fn endpoint(&self) -> (u16, String) {
        (self.port, self.key.clone())
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            stop_child(&mut c, self.pgid);
        }
    }
}

pub struct Supervisor {
    running: Option<(String, Sidecar)>,
    failed_at: Option<Instant>,
    pub pidfile: PathBuf,
    pub bin: PathBuf,
    pub ready_within: Duration,
    pub retry_after: Duration,

    launch: Box<dyn FnMut(&str) -> Result<Box<dyn Launch>, String> + Send>,
    port: Box<dyn FnMut() -> Result<u16, String> + Send>,
    log: Box<dyn Fn(&str) + Send>,
    closing: Arc<AtomicBool>,
}

impl Supervisor {
    pub fn new(pidfile: PathBuf, bin: PathBuf, launch: Box<dyn FnMut(&str) -> Result<Box<dyn Launch>, String> + Send>, log: Box<dyn Fn(&str) + Send>) -> Self {
        Supervisor { running: None, failed_at: None, pidfile, bin, ready_within: Duration::from_secs(30), retry_after: Duration::from_secs(60), launch, port: Box::new(free_port), log, closing: Arc::default() }
    }

    pub fn closing(&self) -> Arc<AtomicBool> {
        self.closing.clone()
    }

    #[cfg(test)]
    pub fn with_ports(mut self, port: Box<dyn FnMut() -> Result<u16, String> + Send>) -> Self {
        self.port = port;
        self
    }

    pub fn sidecar(&mut self, id: &str) -> Result<&mut Sidecar, String> {
        if let Some((have, s)) = &mut self.running {
            if !s.alive() {
                (self.log)(&format!("dictation: nemo-speech pid {} exited; starting it again", s.pid()));
                self.running = None;
            } else if have != id {
                self.running = None;
            }
        }
        if self.running.is_none() {
            if self.closing.load(Ordering::SeqCst) {
                return Err(QUITTING.into());
            }
            if self.failed_at.is_some_and(|t| t.elapsed() < self.retry_after) {
                return Err("nemo-speech failed to start a moment ago".into());
            }
            let mut l = (self.launch)(id)?;
            let started = Sidecar::start(Start { launch: &mut *l, pidfile: self.pidfile.clone(), bin: self.bin.clone(), ready_within: self.ready_within, port: &mut *self.port, closing: &self.closing }, &*self.log);
            match started {
                Ok(s) => {
                    self.failed_at = None;
                    self.running = Some((id.to_string(), s));
                }
                Err(e) => {
                    self.failed_at = Some(Instant::now());
                    return Err(e);
                }
            }
        }
        Ok(&mut self.running.as_mut().unwrap().1)
    }

    pub fn stop(&mut self) {
        if let Some((_, s)) = self.running.take() {
            let pid = s.pid();
            (self.log)(&format!("dictation: nemo-speech pid {pid} {}", s.stop()));
        }
    }

    pub fn running_pid(&self) -> Option<u32> {
        self.running.as_ref().map(|r| r.1.pid())
    }
}

const QUITTING: &str = "the app is quitting";

const TERM_GRACE: Duration = Duration::from_secs(2);
const KILL_GRACE: Duration = Duration::from_secs(1);

fn stop_child(child: &mut Child, pgid: Option<libc::pid_t>) -> String {
    match child.try_wait() {
        Ok(Some(s)) => return format!("had already exited ({s}){}", stop_remnants(pgid)),
        Err(e) => return format!("liveness unknown ({e}); not signalled"),
        Ok(None) => {}
    }
    let pid = child.id() as libc::pid_t;
    let group = pgid == Some(pid);
    let signal = |sig| unsafe {
        if group {
            libc::killpg(pid, sig);
        } else {
            libc::kill(pid, sig);
        }
    };
    signal(libc::SIGTERM);
    let termed = exited_within(pid, TERM_GRACE);
    if group || !termed {
        signal(libc::SIGKILL);
    }
    let status = match child.wait() {
        Ok(s) => s.to_string(),
        Err(e) => format!("wait failed: {e}"),
    };
    let left = if group { group_left(pid, KILL_GRACE) } else { vec![] };
    format!("stopped: exited ({status}) after {}{}", if termed { "SIGTERM" } else { "SIGKILL" }, if left.is_empty() { String::new() } else { format!("; still in its group: {left:?}") })
}

fn stop_remnants(pgid: Option<libc::pid_t>) -> String {

    let Some(g) = pgid.filter(|&g| g > 1 && proc_info(g as u32).is_none() && !group_members(g).is_empty()) else { return "; not signalled".into() };
    unsafe { libc::killpg(g, libc::SIGKILL) };
    match group_left(g, KILL_GRACE) {
        left if left.is_empty() => "; what it started is stopped (SIGKILL)".into(),
        left => format!("; still in its group after SIGKILL: {left:?}"),
    }
}

fn exited_within(pid: libc::pid_t, within: Duration) -> bool {
    let end = Instant::now() + within;
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
        if r == 0 && unsafe { info.si_pid() } == pid {
            return true;
        }
        if r != 0 || Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn group_left(pgid: libc::pid_t, within: Duration) -> Vec<i32> {
    let end = Instant::now() + within;
    loop {
        let left = group_members(pgid);
        if left.is_empty() || Instant::now() >= end {
            return left;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn group_members(pgid: libc::pid_t) -> Vec<i32> {
    let mut buf = vec![0 as libc::pid_t; 1024];
    let n = unsafe { libc::proc_listpgrppids(pgid, buf.as_mut_ptr().cast(), (buf.len() * std::mem::size_of::<libc::pid_t>()) as i32) };
    if n <= 0 {
        return Vec::new();
    }
    buf.retain(|&p| p > 0);
    buf
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcInfo {
    pub start_us: u64,
    pub pgid: libc::pid_t,
    pub ppid: u32,
}

pub fn proc_info(pid: u32) -> Option<ProcInfo> {
    let mut i: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut i as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then(|| ProcInfo { start_us: i.pbi_start_tvsec * 1_000_000 + i.pbi_start_tvusec, pgid: i.pbi_pgid as libc::pid_t, ppid: i.pbi_ppid })
}

pub fn free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| format!("no free port: {e}"))
}

fn new_key() -> Result<String, String> {
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("no random key: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn record(ledger: &Path, pid: u32, bin: &Path) -> Option<libc::pid_t> {
    let info = proc_info(pid);
    let me = std::process::id();
    let owner = proc_info(me).map_or(0, |i| i.start_us);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(ledger) {
        let _ = writeln!(f, "{pid} {} owner={me}:{owner} {}", info.map_or(0, |i| i.start_us), bin.display());
    }
    info.filter(|i| i.pgid == pid as libc::pid_t).map(|i| i.pgid)
}

fn pid_path(pid: u32) -> Option<PathBuf> {
    extern "C" {
        fn proc_pidpath(pid: libc::c_int, buf: *mut libc::c_void, size: u32) -> libc::c_int;
    }
    let mut buf = vec![0u8; 4096];
    let n = unsafe { proc_pidpath(pid as libc::c_int, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned()))
}

pub fn stop_leftover(ledger: &Path, log: &dyn Fn(&str)) {
    let Ok(text) = std::fs::read_to_string(ledger) else { return };
    let mut prev = ledger.as_os_str().to_owned();
    prev.push(".prev");
    let _ = std::fs::rename(ledger, prev);
    let mut kept = String::new();
    for line in text.lines() {
        let Some((pid, rest)) = line.split_once(' ') else { continue };
        let (start, rest) = match rest.split_once(' ') {
            Some((s, b)) if !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()) => (s.parse::<u64>().ok(), b),
            _ => (None, rest),
        };
        let (owner, bin) = match rest.split_once(' ') {
            Some((o, b)) if o.starts_with("owner=") => (o["owner=".len()..].split_once(':').and_then(|(p, t)| Some((p.parse::<u32>().ok()?, t.parse::<u64>().ok()?))), b),
            _ => (None, rest),
        };
        if owner.is_some_and(|(p, t)| proc_info(p).is_some_and(|i| i.start_us == t)) {

            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        let Ok(pid) = pid.parse::<u32>() else { continue };
        let Some(info) = proc_info(pid) else { continue };
        let (want, got) = (std::fs::canonicalize(bin).ok(), pid_path(pid).and_then(|p| std::fs::canonicalize(p).ok()));
        if pid <= 1 || want.is_none() || want != got || start.is_some_and(|s| s != info.start_us) {
            continue;
        }
        log(&format!("dictation: the last run's nemo-speech (pid {pid}) is still running: {}", stop_recorded(pid, info)));
    }
    if !kept.is_empty() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(ledger) {
            let _ = f.write_all(kept.as_bytes());
        }
    }
}

pub(crate) fn stop_recorded(pid: u32, info: ProcInfo) -> String {
    let same = || proc_info(pid).is_some_and(|i| i.start_us == info.start_us);
    let group = info.pgid == pid as libc::pid_t;
    let signal = |sig| {
        if same() {
            unsafe {
                if group {
                    libc::killpg(pid as libc::pid_t, sig);
                } else {
                    libc::kill(pid as libc::pid_t, sig);
                }
            }
        }
    };
    let gone_within = |d: Duration| {
        let end = Instant::now() + d;
        while same() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        !same()
    };
    signal(libc::SIGTERM);
    let how = if gone_within(TERM_GRACE) {
        "exited after SIGTERM"
    } else {
        signal(libc::SIGKILL);
        if !gone_within(KILL_GRACE) {
            return "NOT stopped: still running after SIGKILL".into();
        }
        "killed after SIGTERM was ignored"
    };

    match stop_remnants(group.then_some(pid as libc::pid_t)).as_str() {
        r if r.starts_with("; still") => format!("NOT stopped: {how}{r}"),
        r if r == "; not signalled" => format!("stopped ({how})"),
        r => format!("stopped ({how}{r})"),
    }
}

fn http_get(port: u16, path: &str, key: Option<&str>) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(2))).map_err(|e| e.to_string())?;
    let auth = key.map(|k| format!("Authorization: Bearer {k}\r\n")).unwrap_or_default();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Connection: close\r\n\r\n").map_err(|e| e.to_string())?;
    let mut r = String::new();
    s.read_to_string(&mut r).map_err(|e| e.to_string())?;
    let status = r.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or("not an HTTP reply")?;
    Ok((status, r.split_once("\r\n\r\n").map(|x| x.1.to_string()).unwrap_or_default()))
}

fn wait_ready(child: &mut Child, port: u16, key: &str, within: Duration, closing: &AtomicBool) -> Result<(), String> {
    let until = Instant::now() + within;
    loop {
        if closing.load(Ordering::SeqCst) {
            return Err(QUITTING.into());
        }
        if let Ok(Some(s)) = child.try_wait() {
            return Err(format!("it exited ({s})"));
        }
        if let Ok((200, _)) = http_get(port, "/ready", None) {
            match http_get(port, "/v1/models", Some(key)) {
                Ok((200, _)) if matches!(child.try_wait(), Ok(None)) => return Ok(()),

                Ok((200, _)) => {}
                Ok((code, _)) => return Err(format!("the server on {port} refused our key ({code})")),

                Err(_) => {}
            }
        }
        if Instant::now() >= until {
            return Err(format!("not ready after {} s", within.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub struct Stream {
    ws: tungstenite::WebSocket<TcpStream>,

    heard: Heard,
    ended: bool,

    done: std::collections::VecDeque<String>,
    pub error: Option<String>,
}

#[derive(Default, Debug)]
pub struct Heard {
    pub text: String,

    marks: Vec<(usize, f32)>,

    start: f32,

    unstamped: bool,
}

impl Heard {
    pub fn push(&mut self, delta: &str, at: Option<f32>) {
        if !delta.is_empty() {
            self.unstamped |= at.is_none();
            self.marks.push((self.text.len(), at.unwrap_or(0.0) - self.start));
            self.text.push_str(delta);
        }
    }

    pub fn timed(&self) -> bool {
        !self.unstamped
    }

    pub fn next_segment(&mut self, at: Option<f32>) {
        self.text.clear();
        self.marks.clear();
        self.unstamped = false;
        if let Some(at) = at {
            self.start = at;
        }
    }

    pub fn after(&self, t: f32) -> String {
        let Some(&(p, _)) = self.marks.iter().find(|(_, at)| *at > t) else { return String::new() };

        let from = if self.text[p..].starts_with(char::is_whitespace) { p } else { self.text[..p].char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8()) };
        self.text[from..].trim().to_string()
    }
}

const FINISH_WITHIN: Duration = Duration::from_millis(3500);

impl Stream {
    pub fn open(port: u16, key: &str, lang: &str) -> Result<Stream, String> {
        Self::open_with_endpoint(port, key, lang, None)
    }

    pub fn open_with_endpoint(port: u16, key: &str, lang: &str, endpoint_ms: Option<u32>) -> Result<Stream, String> {
        use tungstenite::client::IntoClientRequest;
        let tcp = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).map_err(|e| format!("nemo-speech socket: {e}"))?;
        tcp.set_nodelay(true).map_err(|e| e.to_string())?;
        tcp.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        tcp.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let mut req = format!("ws://127.0.0.1:{port}/v1/audio/transcriptions/realtime").into_client_request().map_err(|e| e.to_string())?;
        req.headers_mut().insert("Authorization", format!("Bearer {key}").parse().map_err(|_| "bad key")?);
        let (ws, _) = tungstenite::client(req, tcp).map_err(|e| format!("nemo-speech handshake: {e}"))?;
        let mut s = Stream { ws, heard: Heard::default(), ended: false, done: Default::default(), error: None };
        let mut session = json!({"sample_rate": crate::dictation::RATE});
        if let Some(code) = language(lang) {
            session["language"] = json!(code);
        }
        if let Some(ms) = endpoint_ms { session["endpointing_ms"] = json!(ms); }
        s.send_json(json!({"type": "session.update", "session": session}))?;

        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match s.next(Some(until))? {
                Some(e) if e["type"] == "session.updated" => break,
                Some(e) if e["type"] == "error" => return Err(format!("nemo-speech refused the session: {}", e["error"]["message"].as_str().or(e["message"].as_str()).unwrap_or("?"))),

                Some(_) if Instant::now() >= until => return Err("nemo-speech: no session".into()),
                Some(_) => {}
                None => return Err("nemo-speech: no session".into()),
            }
        }
        s.ws.get_mut().set_read_timeout(Some(Duration::from_millis(1))).map_err(|e| e.to_string())?;
        Ok(s)
    }

    fn send_json(&mut self, v: Value) -> Result<(), String> {
        self.ws.send(tungstenite::Message::text(v.to_string())).map_err(|e| format!("nemo-speech send: {e}"))
    }

    fn next(&mut self, until: Option<Instant>) -> Result<Option<Value>, String> {
        use tungstenite::{Error, Message};
        loop {
            match self.ws.read() {
                Ok(Message::Text(t)) => return Ok(serde_json::from_str(t.as_str()).ok()),
                Ok(Message::Close(_)) => return Err("nemo-speech closed the socket".into()),

                Ok(_) if until.is_some_and(|u| Instant::now() >= u) => return Ok(None),
                Ok(_) => {}
                Err(Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if until.is_none_or(|u| Instant::now() >= u) {
                        return Ok(None);
                    }
                }
                Err(e) => return Err(format!("nemo-speech socket: {e}")),
            }
        }
    }

    fn drain(&mut self, until: Option<Instant>) {
        loop {
            let e = match self.next(until) {
                Ok(Some(e)) => e,
                Ok(None) => return,
                Err(e) => {
                    self.error.get_or_insert(e);
                    return;
                }
            };
            let t = e["type"].as_str().unwrap_or("");
            #[cfg(test)]
            if std::env::var_os("RTH_NEMO_TRACE").is_some() {

                eprintln!("nemo event: {e}");
            }
            if t.ends_with("input_audio_transcription.delta") {
                if let Some(d) = e["delta"].as_str() {
                    self.heard.push(d, e["audio_processed"].as_f64().map(|a| a as f32));
                }
            } else if t.ends_with("input_audio_transcription.completed") {
                self.done.push_back(e["transcript"].as_str().unwrap_or("").trim().to_string());
                self.heard.next_segment(e["audio_processed"].as_f64().map(|a| a as f32));
                if until.is_some() {
                    return;
                }
            } else if t == "error" {
                self.error.get_or_insert(e["error"]["message"].as_str().or(e["message"].as_str()).unwrap_or("nemo-speech error").to_string());
            }
        }
    }
}

pub fn language(lang: &str) -> Option<&'static str> {
    match lang {
        "pt" => Some("pt-BR"),
        "en" => Some("en-US"),
        _ => None,
    }
}

pub fn pcm16(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|&x| ((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()).collect()
}

impl Recognizer for Stream {
    fn accept(&mut self, samples: &[f32]) {
        if self.error.is_none() {
            if let Err(e) = self.ws.send(tungstenite::Message::binary(pcm16(samples))) {
                self.error = Some(format!("nemo-speech send: {e}"));
            }
        }
        self.drain(None);
    }
    fn text(&mut self) -> String {
        match self.done.front() {
            Some(t) => {
                self.ended = true;
                t.clone()
            }
            None => self.heard.text.trim().to_string(),
        }
    }
    fn heard_after(&mut self, t: f32) -> Option<String> {
        self.heard.timed().then(|| self.heard.after(t))
    }
    fn endpoint(&mut self) -> bool {
        self.ended || !self.done.is_empty()
    }
    fn reset(&mut self) {
        self.done.pop_front();
        self.ended = false;
    }
    fn failed(&mut self) -> Option<String> {
        self.error.clone()
    }
    fn finish(&mut self) -> String {
        let mut said: Vec<String> = self.done.drain(..).collect();
        if self.error.is_none() && self.send_json(json!({"type": "input_audio_buffer.commit"})).is_ok() {
            let until = Instant::now() + FINISH_WITHIN;
            while self.done.is_empty() && self.error.is_none() && Instant::now() < until {
                self.drain(Some(until));
            }
        }
        said.extend(self.done.drain(..));
        if said.iter().all(String::is_empty) && !self.heard.text.trim().is_empty() {

            said.push(self.heard.text.trim().to_string());
        }
        self.heard.next_segment(None);
        said.retain(|t| !t.is_empty());
        said.join(" ")
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.send_json(json!({"type": "response.cancel"}));
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

pub(crate) const PASS_WITHIN: Duration = Duration::from_secs(2);

pub(crate) const LAST_PASS_WITHIN: Duration = Duration::from_secs(30);

pub fn transcribe_words(port: u16, key: &str, lang: &str, samples: &[f32], wait: Duration) -> Result<Vec<crate::dictation_settle::Word>, String> {
    Ok(words_in(&post_audio(port, key, lang, samples, &[("response_format", "verbose_json"), ("mode", "offline")], wait)?))
}

fn words_in(json: &Value) -> Vec<crate::dictation_settle::Word> {
    json["words"]
        .as_array()
        .map(|w| w.iter().filter_map(|w| Some(crate::dictation_settle::Word { text: w["word"].as_str()?.to_string(), start: w["start"].as_f64()? as f32, end: w["end"].as_f64()? as f32 })).collect())
        .unwrap_or_default()
}

pub fn pass_decode(port: u16, key: &str, lang: &str, samples: &[f32], wait: Duration) -> Result<crate::dictation::Decoded, String> {
    let json = post_audio(port, key, lang, samples, &[("response_format", "verbose_json"), ("mode", "offline")], wait)?;
    Ok(crate::dictation::Decoded { words: timed_words(&json)?, text: json["text"].as_str().unwrap_or("").trim().to_string() })
}

fn timed_words(json: &Value) -> Result<Vec<crate::dictation_settle::Word>, String> {
    let words = words_in(json);
    if words.is_empty() && !json["text"].as_str().unwrap_or("").trim().is_empty() {
        return Err("the model gave text with no word times".into());
    }
    Ok(words)
}

pub fn transcribe(port: u16, key: &str, lang: &str, samples: &[f32], wait: Duration) -> Result<String, String> {
    let json = post_audio(port, key, lang, samples, &[("response_format", "json")], wait)?;
    Ok(json["text"].as_str().unwrap_or("").trim().to_string())
}

fn post_audio(port: u16, key: &str, lang: &str, samples: &[f32], fields: &[(&str, &str)], wait: Duration) -> Result<Value, String> {
    let pcm = pcm16(samples);
    let mut wav = Vec::with_capacity(44 + pcm.len());
    let rate = crate::dictation::RATE as u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);
    let b = format!("rth{}", new_key()?.get(..24).unwrap_or("x"));
    let mut body = Vec::new();
    let field = |body: &mut Vec<u8>, name: &str, value: &str| body.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    for (name, value) in fields {
        field(&mut body, name, value);
    }
    if let Some(code) = language(lang) {
        field(&mut body, "language", code);
    }
    body.extend_from_slice(format!("--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"phrase.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    body.extend_from_slice(&wav);
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), wait.min(Duration::from_secs(2))).map_err(|e| format!("nemo-speech: {e}"))?;
    s.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
    let head = format!(
        "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: multipart/form-data; boundary={b}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(&body)).map_err(|e| format!("nemo-speech: {e}"))?;
    let mut r = Vec::new();
    s.read_to_end(&mut r).map_err(|e| format!("nemo-speech: {e}"))?;
    let r = String::from_utf8_lossy(&r);
    let status: u16 = r.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let json: Value = r.split_once("\r\n\r\n").and_then(|x| serde_json::from_str(x.1.trim()).ok()).unwrap_or(Value::Null);
    if status != 200 {
        return Err(format!("nemo-speech transcription: {status} {}", json["error"]["message"].as_str().unwrap_or("")));
    }
    Ok(json)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::dictation::{Event, Session, RATE};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_words_heard_after_a_point_survive_a_wide_space_before_them() {

        for sp in ['\u{a0}', '\u{202f}', '\u{3000}'] {
            let mut h = Heard::default();
            h.push("um", Some(0.5));
            h.push(&format!("{sp}do"), Some(1.0));
            h.push("is", Some(1.2));
            assert_eq!(h.after(1.1), "dois", "{:?}", sp);
            assert_eq!(h.after(0.7), "dois", "{:?}", sp);
        }
    }

    #[derive(Default)]
    struct Seen {
        sent: Mutex<Vec<String>>,

        drop_after: Mutex<Option<usize>>,

        drop_keyed: AtomicUsize,

        final_after: Mutex<Option<Duration>>,

        sessions: Mutex<Vec<Value>>,

        posted: Mutex<Vec<Vec<String>>>,

        chatter: AtomicBool,
    }

    #[test]
    fn a_session_never_confirmed_ends_the_open_at_its_deadline_however_chatty_the_server() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen: Arc<Seen> = Default::default();
        seen.chatter.store(true, Ordering::SeqCst);
        serve_fake(l, "k".into(), seen);
        let (tx, rx) = std::sync::mpsc::channel();
        let t = Instant::now();
        std::thread::spawn(move || drop(tx.send(Stream::open(port, "k", "pt").map(|_| ()))));
        let r = rx.recv_timeout(Duration::from_secs(8));
        assert!(matches!(&r, Ok(Err(e)) if e.contains("no session")), "{r:?} after {:?}", t.elapsed());
        assert!(t.elapsed() < Duration::from_secs(7), "{:?}", t.elapsed());
    }

    fn serve_fake(l: TcpListener, key: String, seen: Arc<Seen>) {
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let (key, seen) = (key.clone(), seen.clone());
                std::thread::spawn(move || one(c, &key, &seen));
            }
        });
    }

    fn one(mut c: TcpStream, key: &str, seen: &Seen) {

        let mut buf = [0u8; 4096];
        let t = Instant::now();
        let n = loop {
            let n = c.peek(&mut buf).unwrap_or(0);
            if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") || n == buf.len() || t.elapsed() > Duration::from_secs(2) {
                break n;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let authed = head.to_ascii_lowercase().contains(&format!("authorization: bearer {}", key.to_ascii_lowercase()));
        if head.starts_with("GET /v1/audio/transcriptions/realtime") {
            if !authed {
                let _ = c.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
                return;
            }
            let Ok(mut ws) = tungstenite::accept(c) else { return };
            let send = |ws: &mut tungstenite::WebSocket<TcpStream>, v: Value| ws.send(tungstenite::Message::text(v.to_string()));
            let _ = send(&mut ws, json!({"type": "session.created"}));
            let (mut bytes, mut frames, mut said) = (0usize, 0usize, 0usize);
            let words = ["hello", " there", " world"];
            while let Ok(m) = ws.read() {
                match m {
                    tungstenite::Message::Text(t) => {
                        let e: Value = serde_json::from_str(t.as_str()).unwrap();
                        let ty = e["type"].as_str().unwrap_or("").to_string();
                        seen.sent.lock().unwrap().push(ty.clone());
                        match ty.as_str() {
                            "session.update" if seen.chatter.load(Ordering::SeqCst) => {
                                for _ in 0..150 {
                                    if send(&mut ws, json!({"type": "rate_limits.updated"})).is_err() {
                                        return;
                                    }
                                    std::thread::sleep(Duration::from_millis(100));
                                }
                            }
                            "session.update" => {
                                seen.sessions.lock().unwrap().push(e["session"].clone());
                                let _ = send(&mut ws, json!({"type": "session.updated", "session": e["session"]}));
                            }
                            "input_audio_buffer.commit" => {
                                if let Some(d) = *seen.final_after.lock().unwrap() {
                                    std::thread::sleep(d);
                                }
                                let _ = send(&mut ws, json!({"type": "conversation.item.input_audio_transcription.completed", "transcript": "Hello there, world."}));
                            }
                            _ => {}
                        }
                    }
                    tungstenite::Message::Binary(b) => {
                        frames += 1;
                        if seen.drop_after.lock().unwrap().is_some_and(|n| frames > n) {
                            return;
                        }
                        bytes += b.len();
                        while said < words.len() && bytes >= (said + 1) * RATE / 10 * 2 {
                            let _ = send(&mut ws, json!({"type": "conversation.item.input_audio_transcription.delta", "delta": words[said]}));
                            said += 1;
                        }
                    }
                    tungstenite::Message::Close(_) => return,
                    _ => {}
                }
            }
            return;
        }
        if head.starts_with("POST /v1/audio/transcriptions") {

            let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
            let at = head.find("\r\n\r\n").unwrap() + 4;
            let mut req = vec![0u8; at + len];
            let _ = c.read_exact(&mut req);
            let body = String::from_utf8_lossy(&req[at..]).to_string();
            let fields: Vec<String> = body
                .split("Content-Disposition: form-data; name=\"")
                .skip(1)
                .filter(|p| !p.starts_with("file\""))
                .filter_map(|p| p.split_once("\"\r\n\r\n").map(|(n, v)| format!("{n}={}", v.split("\r\n").next().unwrap_or(""))))
                .collect();
            seen.posted.lock().unwrap().push(fields);
            let answer = if authed { json!({"text": "olá mundo", "words": [{"word": "olá", "start": 0.0, "end": 0.4, "confidence": 1}, {"word": "mundo", "start": 0.5, "end": 0.9, "confidence": 1}]}).to_string() } else { String::new() };
            let status = if authed { "200 OK" } else { "401 Unauthorized" };
            let _ = c.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).as_bytes());
            return;
        }
        let mut sink = vec![0u8; n];
        let _ = c.read_exact(&mut sink);
        if head.starts_with("GET /v1/models") && authed && seen.drop_keyed.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
            return;
        }
        let reply: &[u8] = if head.starts_with("GET /ready") {
            b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"ready\": true}"
        } else if head.starts_with("GET /v1/models") && authed {
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"
        } else {
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n"
        };
        let _ = c.write_all(reply);
    }

    static HELD: Mutex<std::collections::BTreeMap<u16, TcpListener>> = Mutex::new(std::collections::BTreeMap::new());

    fn hold() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        HELD.lock().unwrap().insert(port, l);
        port
    }

    struct Fake {
        seen: Arc<Seen>,
        spawns: Arc<AtomicUsize>,
        stay: bool,
        broken: bool,

        program: &'static [&'static str],
    }

    const SLEEP: &[&str] = &["/bin/sleep", "30"];

    const DEAF: &[&str] = &["/bin/sh", "-c", "trap '' TERM; /bin/sleep 30 & /bin/sleep 30; wait"];
    const FORKS: &[&str] = &["/bin/sh", "-c", "/bin/sleep 30 & /bin/sh -c '/bin/sleep 30 & wait' & wait"];

    impl Launch for Fake {
        fn spawn(&mut self, port: u16, key: &str) -> Result<Child, String> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            let bound = HELD.lock().unwrap().remove(&port).filter(|_| !self.broken);
            let exits = bound.is_none() && !self.stay;
            if let Some(l) = bound {
                serve_fake(l, key.to_string(), self.seen.clone());
            }
            let prog: &[&str] = if exits { &["/usr/bin/false"] } else { self.program };
            own_group(Command::new(prog[0]).args(&prog[1..])).spawn().map_err(|e| e.to_string())
        }
    }

    fn tmp() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("rth-nemo-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sup(dir: &Path, seen: Arc<Seen>, spawns: Arc<AtomicUsize>, stay: bool, broken: bool) -> Supervisor {
        sup_running(dir, seen, spawns, stay, broken, SLEEP)
    }

    pub(crate) fn sup_running(dir: &Path, seen: Arc<Seen>, spawns: Arc<AtomicUsize>, stay: bool, broken: bool, program: &'static [&'static str]) -> Supervisor {
        let launch = move |_: &str| -> Result<Box<dyn Launch>, String> { Ok(Box::new(Fake { seen: seen.clone(), spawns: spawns.clone(), stay, broken, program })) };
        let mut s = Supervisor::new(dir.join("nemo-speech.ledger"), PathBuf::from(program[0]), Box::new(launch), Box::new(|l| eprintln!("{l}"))).with_ports(Box::new(|| Ok(hold())));
        s.ready_within = Duration::from_secs(3);
        s
    }

    pub(crate) fn deaf_supervisor(dir: &Path) -> (Supervisor, i32) {
        let mut s = sup_running(dir, Default::default(), Default::default(), false, false, DEAF);
        let pid = s.sidecar("m").unwrap().pid() as i32;
        until("its children run", || group_members(pid).len() >= 3);
        (s, pid)
    }

    pub(crate) fn group_with_parents(pgid: i32) -> Vec<(i32, u32)> {
        group_members(pgid).into_iter().map(|p| {
            let mut i: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
            unsafe { libc::proc_pidinfo(p, libc::PROC_PIDTBSDINFO, 0, (&mut i as *mut libc::proc_bsdinfo).cast(), size) };
            (p, i.pbi_ppid)
        }).collect()
    }

    fn left(ledger: &Path, pid: u32, bin: &Path) {
        let start = proc_info(pid).unwrap().start_us;
        std::fs::OpenOptions::new().create(true).append(true).open(ledger).unwrap().write_all(format!("{pid} {start} owner=1:0 {}\n", bin.display()).as_bytes()).unwrap();
    }

    fn orphan_leader() -> u32 {
        let out = Command::new("/usr/bin/perl").args(["-e", "use POSIX; $| = 1; if (fork) { exit 0 } POSIX::setpgid(0, 0); $SIG{TERM} = 'IGNORE'; print \"$$\\n\"; close STDOUT; sleep 30"]).stdin(Stdio::null()).stderr(Stdio::null()).output().unwrap();
        let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
        until("the orphan runs", || proc_info(pid).is_some());
        pid
    }

    impl Supervisor {
        fn running_alive(&mut self) -> bool {
            self.running.as_mut().is_some_and(|r| r.1.alive())
        }
    }

    fn running(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    fn until(what: &str, mut f: impl FnMut() -> bool) {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(5), "timed out: {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn texts(ev: &[Event]) -> Vec<(u64, String, bool)> {
        ev.iter().filter_map(|e| if let Event::Text { seq, text, is_final, .. } = e { Some((*seq, text.clone(), *is_final)) } else { None }).collect()
    }

    #[test]
    fn the_sidecar_starts_on_loopback_with_a_key_and_streams_partials_then_the_final() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let side = s.sidecar("m").unwrap();
        let (port, pid) = (side.port, side.pid());

        let start = proc_info(pid).unwrap().start_us;
        let me = std::process::id();
        let mine = proc_info(me).unwrap().start_us;
        assert_eq!(std::fs::read_to_string(dir.join("nemo-speech.ledger")).unwrap(), format!("{pid} {start} owner={me}:{mine} /bin/sleep\n"));
        assert_eq!(proc_info(pid).unwrap().pgid, pid as i32, "its own process group");

        assert_eq!(http_get(port, "/v1/models", None).unwrap().0, 401);
        assert_eq!(http_get(port, "/v1/models", Some("not-the-key")).unwrap().0, 401);
        let mut session = Session::new(side.stream("pt").unwrap());
        let mut ev = vec![];
        for c in vec![0.01f32; RATE].chunks(RATE / 100) {
            ev.extend(session.feed(c));
        }

        until("partials", || {
            ev.extend(session.feed(&[0.0; 16]));
            texts(&ev).iter().any(|t| t.1 == "hello there world")
        });
        ev.extend(session.stop());
        let t = texts(&ev);
        assert!(t.windows(2).all(|w| w[0].0 < w[1].0), "seq rises: {t:?}");
        assert_eq!(t.iter().filter(|x| !x.2).map(|x| x.1.as_str()).collect::<Vec<_>>(), ["hello", "hello there", "hello there world"]);
        assert_eq!(t.last().unwrap().1, "Hello there, world.");
        assert!(t.last().unwrap().2 && t.iter().filter(|x| x.2).count() == 1);
        assert!(matches!(ev.last(), Some(Event::Stopped)));

        drop(session);
        until("the client's events", || seen.sent.lock().unwrap().contains(&"response.cancel".to_string()));
        assert_eq!(seen.sent.lock().unwrap()[..2], ["session.update".to_string(), "input_audio_buffer.commit".to_string()]);

        s.stop();
        assert!(!running(pid), "stop returns once it has exited");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn while_live_passes_run_an_offline_phrase_ends_on_the_same_request_as_the_passes() {

        let dir = tmp();
        let seen = Arc::new(Seen::default());
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let (port, key) = s.sidecar("m").unwrap().endpoint();
        let mut d = Session::new(crate::dictation::posted(port, key, "pt"));
        let speech: Vec<f32> = (0..RATE * 3).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        let mut ev = vec![];
        for part in [&vec![0.0; RATE / 2], &speech, &vec![0.0; RATE]] {
            for c in part.chunks(160) {
                ev.extend(d.feed(c));
            }
        }
        assert!(ev.iter().any(|e| matches!(e, Event::Text { is_final: false, .. })), "no live words: {ev:?}");
        assert!(ev.iter().any(|e| matches!(e, Event::Text { is_final: true, text, .. } if text == "olá mundo")), "{ev:?}");
        let posted = seen.posted.lock().unwrap().clone();
        assert!(posted.len() >= 2 && posted.iter().all(|f| f[..2] == ["response_format=verbose_json", "mode=offline"]), "{posted:?}");
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn serve_until_hung(hang: Arc<AtomicBool>) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let hang = hang.clone();
                std::thread::spawn(move || {
                    let mut req = Vec::new();
                    let mut buf = [0u8; 65536];
                    let whole = |req: &[u8]| {
                        let head = String::from_utf8_lossy(req);
                        let Some(at) = head.find("\r\n\r\n") else { return false };
                        let len: usize = head[..at].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                        req.len() >= at + 4 + len
                    };
                    while !whole(&req) {
                        match c.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => req.extend_from_slice(&buf[..n]),
                        }
                    }
                    if hang.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_secs(60));
                        return;
                    }
                    let answer = json!({"text": "olá mundo", "words": [{"word": "olá", "start": 0.0, "end": 0.4}, {"word": "mundo", "start": 0.5, "end": 0.9}]}).to_string();
                    let _ = c.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{answer}", answer.len()).as_bytes());
                });
            }
        });
        port
    }

    #[test]
    fn a_hung_final_and_the_post_after_it_share_one_wait() {
        let hang = Arc::new(AtomicBool::new(false));
        let port = serve_until_hung(hang.clone());
        let mut d = Session::new(crate::dictation::posted_within(port, "k".into(), "pt", Duration::from_secs(1)));
        let speech: Vec<f32> = (0..RATE * 3).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        let mut ev = vec![];
        for part in [&vec![0.0; RATE / 2], &speech] {
            for c in part.chunks(160) {
                ev.extend(d.feed(c));
            }
        }
        assert!(ev.iter().any(|e| matches!(e, Event::Text { is_final: false, .. })), "no live words: {ev:?}");
        hang.store(true, Ordering::SeqCst);
        let t = Instant::now();
        for c in vec![0.0; RATE].chunks(160) {
            d.feed(c);
        }
        let took = t.elapsed();
        assert!(d.failed().is_some(), "the phrase ended without the error");
        assert!(took < Duration::from_millis(1800), "the phrase waited {took:?}, more than its one wait");
    }

    #[test]
    fn a_live_pass_with_text_but_no_word_times_fails() {
        assert!(timed_words(&json!({"text": "olá mundo", "words": []})).is_err());
        assert!(timed_words(&json!({"text": "olá mundo"})).is_err());
        assert_eq!(timed_words(&json!({"text": " ", "words": []})).unwrap(), []);
        assert_eq!(timed_words(&json!({"text": "olá", "words": [{"word": "olá", "start": 0.0, "end": 0.4}]})).unwrap().len(), 1);
    }

    #[test]
    fn a_settle_pass_asks_for_the_offline_mode_and_verbose_json_and_reads_the_words() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let (port, key) = s.sidecar("m").unwrap().endpoint();
        let words = transcribe_words(port, &key, "pt", &[0.01; RATE], PASS_WITHIN).unwrap();
        assert_eq!(
            words,
            [
                crate::dictation_settle::Word { text: "olá".into(), start: 0.0, end: 0.4 },
                crate::dictation_settle::Word { text: "mundo".into(), start: 0.5, end: 0.9 },
            ]
        );
        assert_eq!(seen.posted.lock().unwrap()[0], ["response_format=verbose_json", "mode=offline", "language=pt-BR"]);

        assert_eq!(transcribe(port, &key, "", &[0.01; RATE], LAST_PASS_WITHIN).unwrap(), "olá mundo");
        assert_eq!(seen.posted.lock().unwrap()[1], ["response_format=json"]);
        assert!(transcribe_words(port, "not-the-key", "pt", &[0.01; RATE], PASS_WITHIN).is_err());
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_settled_stream_settles_words_from_the_passes_and_ends_on_the_last_pass() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let side = s.sidecar("m").unwrap();
        let mut session = Session::new(side.settled_stream("pt").unwrap());
        let mut ev = vec![];

        for c in vec![0.01f32; 3 * RATE].chunks(RATE / 100) {
            ev.extend(session.feed(c));
        }
        assert_eq!(seen.posted.lock().unwrap().len(), 3);
        let settled: Vec<usize> = ev.iter().filter_map(|e| if let Event::Text { settled, is_final: false, .. } = e { Some(settled.expect("a settling stream says what settled")) } else { None }).collect();
        assert!(settled.windows(2).all(|w| w[0] <= w[1]), "settled only grows: {settled:?}");
        assert_eq!(settled.last(), Some(&2), "olá and mundo: mundo ends 0.9, behind 3 - 1.5");
        ev.extend(session.stop());
        let t = texts(&ev);
        assert_eq!(t.last().unwrap(), &(t.last().unwrap().0, "olá mundo".to_string(), true), "the last pass is the final text");
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crashed_sidecar_is_started_again_on_the_next_dictation() {
        let dir = tmp();
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, Default::default(), spawns.clone(), false, false);
        let first = s.sidecar("m").unwrap().pid();
        unsafe {
            libc::kill(first as libc::pid_t, libc::SIGKILL);
        }
        until("it died", || !s.running_alive());
        let second = s.sidecar("m").unwrap().pid();
        assert_ne!(first, second);
        assert_eq!(spawns.load(Ordering::SeqCst), 2);

        let third = s.sidecar("other").unwrap().pid();
        assert_ne!(second, third);
        until("the old one stopped", || !running(second));
        s.stop();
        until("stopped", || !running(third));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_reply_to_the_key_check_is_retried_not_a_new_start() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        seen.drop_keyed.store(1, Ordering::SeqCst);
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, seen, spawns.clone(), false, false);
        s.sidecar("m").unwrap();
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "the same process, once the key check answered");
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_taken_port_is_skipped_and_a_server_that_refuses_our_key_is_never_used() {
        let dir = tmp();

        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let tp = taken.local_addr().unwrap().port();
        let mut ports = vec![tp, hold()].into_iter();
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, Default::default(), spawns.clone(), false, false).with_ports(Box::new(move || ports.next().ok_or("no port".into())));
        let p = s.sidecar("m").unwrap().port;
        assert_ne!(p, tp);
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
        s.stop();

        let foreign = TcpListener::bind("127.0.0.1:0").unwrap();
        let fp = foreign.local_addr().unwrap().port();
        serve_fake(foreign, "someone else's".into(), Default::default());
        let mut ports = vec![fp, hold()].into_iter();
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, Default::default(), spawns.clone(), true, false).with_ports(Box::new(move || ports.next().ok_or("no port".into())));
        let p = s.sidecar("m").unwrap().port;
        assert_ne!(p, fp, "never the foreign server");
        assert_eq!(spawns.load(Ordering::SeqCst), 2);
        s.stop();
        drop(taken);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_never_starts_fails_and_is_not_retried_for_a_while() {
        let dir = tmp();
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, Default::default(), spawns.clone(), false, true);
        let e = s.sidecar("m").err().unwrap();
        assert!(e.starts_with("nemo-speech did not start"), "{e}");
        assert_eq!(spawns.load(Ordering::SeqCst), TRIES);
        assert!(s.sidecar("m").err().unwrap().contains("a moment ago"));
        assert_eq!(spawns.load(Ordering::SeqCst), TRIES, "no new tries meanwhile");
        assert!(!dir.join("nemo-speech.pid").exists());
        let lines = std::fs::read_to_string(dir.join("nemo-speech.ledger")).unwrap();
        assert_eq!(lines.lines().count(), TRIES, "every start is recorded, none overwritten");
        s.retry_after = Duration::ZERO;
        assert!(s.sidecar("m").is_err());
        assert_eq!(spawns.load(Ordering::SeqCst), 2 * TRIES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_gone_mid_dictation_ends_it_with_an_error() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        *seen.drop_after.lock().unwrap() = Some(5);
        let mut s = sup(&dir, seen, Default::default(), false, false);
        let mut session = Session::new(s.sidecar("m").unwrap().stream("en").unwrap());
        until("the failure shows", || {
            session.feed(&[0.0; RATE / 100]);
            session.failed().is_some()
        });
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_recorded_pid_still_running_our_binary_is_stopped_at_launch() {
        let dir = tmp();
        let ledger = dir.join("nemo-speech.ledger");
        let mut ours = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        left(&ledger, ours.id(), Path::new("/bin/sleep"));

        let mut other = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        left(&ledger, other.id(), Path::new("/usr/bin/false"));
        let mut later = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        std::fs::OpenOptions::new().append(true).open(&ledger).unwrap().write_all(format!("{} 1 /bin/sleep\n", later.id()).as_bytes()).unwrap();
        stop_leftover(&ledger, &|_| {});
        until("the leftover stopped", || matches!(ours.try_wait(), Ok(Some(_))));
        assert!(!ledger.exists() && dir.join("nemo-speech.ledger.prev").exists(), "the ledger is rotated");
        assert!(matches!(other.try_wait(), Ok(None)) && matches!(later.try_wait(), Ok(None)));
        for mut c in [other, later] {
            let _ = c.kill();
            let _ = c.wait();
        }

        let mut old = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let pidfile = dir.join("nemo-speech.pid");
        std::fs::write(&pidfile, format!("{} /bin/sleep\n", old.id())).unwrap();
        stop_leftover(&pidfile, &|_| {});
        until("the 0.4.2 leftover stopped", || matches!(old.try_wait(), Ok(Some(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sidecar_is_its_own_group_and_takes_sigterm_even_from_a_thread_that_blocks_it() {
        let dir = tmp();
        let bin = dir.join("fake-nemo-speech");
        std::fs::write(&bin, "#!/bin/sh\necho \"$@\" > \"$HOME/args\"\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut serve = Serve { bin, model: dir.join("model"), itn: None, home: dir.clone(), warmup: false, sandbox: false };

        let mut child = std::thread::spawn(move || {
            unsafe {
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGTERM);
                libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
            }
            serve.spawn(1, "k").unwrap()
        }).join().unwrap();
        until("it runs", || proc_info(child.id()).is_some());
        assert_eq!(proc_info(child.id()).unwrap().pgid, child.id() as i32, "its own process group, not the app's");

        until("its args", || std::fs::read_to_string(dir.join("args")).is_ok_and(|a| a.ends_with('\n')));
        assert!(std::fs::read_to_string(dir.join("args")).unwrap().split_whitespace().any(|a| a == "--endpointing"));
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        until("SIGTERM ends it", || matches!(child.try_wait(), Ok(Some(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_ignores_sigterm_is_killed_and_waited_for() {
        let dir = tmp();
        let mut s = sup_running(&dir, Default::default(), Default::default(), false, false, DEAF);
        let pid = s.sidecar("m").unwrap().pid();
        until("its children run", || group_members(pid as i32).len() >= 3);
        let t = Instant::now();
        s.stop();
        assert!(t.elapsed() >= TERM_GRACE, "SIGTERM was given its time first");
        assert_eq!(group_with_parents(pid as i32), vec![], "nothing left in its group");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stopping_a_forking_sidecar_ends_its_grandchildren_too() {
        let dir = tmp();
        let mut s = sup_running(&dir, Default::default(), Default::default(), false, false, FORKS);
        let pid = s.sidecar("m").unwrap().pid();
        until("its children and grandchildren run", || group_members(pid as i32).len() >= 4);
        let all = group_members(pid as i32);
        s.stop();
        assert_eq!(group_with_parents(pid as i32), vec![]);
        assert!(all.iter().all(|&p| proc_info(p as u32).is_none()), "every pid of its group is gone: {all:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_that_ignores_sigterm_is_killed_and_only_then_logged_stopped() {
        let dir = tmp();
        let ledger = dir.join("nemo-speech.ledger");
        let pid = orphan_leader();
        left(&ledger, pid, &pid_path(pid).unwrap());
        let lines = Mutex::new(vec![]);
        stop_leftover(&ledger, &|l| lines.lock().unwrap().push(l.to_string()));
        assert!(proc_info(pid).is_none(), "gone when stop_leftover returns");
        let lines = lines.into_inner().unwrap();
        assert!(lines.len() == 1 && lines[0].contains("killed after SIGTERM was ignored"), "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_recorded_orphans_are_both_reaped_at_launch() {
        let dir = tmp();
        let ledger = dir.join("nemo-speech.ledger");
        let (a, b) = (orphan_leader(), orphan_leader());
        left(&ledger, a, &pid_path(a).unwrap());
        left(&ledger, b, &pid_path(b).unwrap());
        stop_leftover(&ledger, &|l| eprintln!("{l}"));
        assert!(proc_info(a).is_none() && proc_info(b).is_none(), "both reaped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_is_sent_as_pcm16() {
        assert_eq!(pcm16(&[0.0, 1.0, -1.0, 2.0]), [0, 0, 0xff, 0x7f, 0x01, 0x80, 0xff, 0x7f]);
    }

    #[test]
    fn a_final_3_s_after_stop_is_kept_and_auto_sends_no_language() {
        let dir = tmp();
        let seen: Arc<Seen> = Default::default();
        *seen.final_after.lock().unwrap() = Some(Duration::from_secs(3));
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let mut session = Session::new(s.sidecar("m").unwrap().stream("auto").unwrap());
        let mut ev = vec![];
        for c in vec![0.01f32; RATE].chunks(RATE / 100) {
            ev.extend(session.feed(c));
        }
        let t = Instant::now();
        ev.extend(session.stop());
        assert!(t.elapsed() >= Duration::from_secs(3));
        let last = texts(&ev).pop().unwrap();
        assert_eq!((last.1.as_str(), last.2), ("Hello there, world.", true), "the late final, not the partial: {ev:?}");
        assert_eq!(session.finals(), 1);

        drop(session);
        drop(s.sidecar("m").unwrap().stream("pt").unwrap());
        let sessions = seen.sessions.lock().unwrap().clone();
        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].get("language").is_none(), "{sessions:?}");
        assert_eq!(sessions[1]["language"], "pt-BR");
        s.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_whose_app_still_runs_is_left_alone_at_launch() {

        let dir = tmp();
        let ledger = dir.join("nemo-speech.ledger");
        let mut theirs = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        record(&ledger, theirs.id(), Path::new("/bin/sleep"));
        stop_leftover(&ledger, &|_| {});
        assert!(matches!(theirs.try_wait(), Ok(None)), "its app still runs");
        let line = std::fs::read_to_string(&ledger).unwrap_or_default();
        assert!(line.starts_with(&format!("{} ", theirs.id())), "its line stays in the ledger: {line:?}");
        stop_leftover(&ledger, &|_| {});
        assert!(matches!(theirs.try_wait(), Ok(None)) && std::fs::read_to_string(&ledger).is_ok_and(|l| l == line), "and at every launch while that app runs");
        let _ = theirs.kill();
        let _ = theirs.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_quit_during_a_sidecar_start_ends_the_start_at_once() {

        let dir = tmp();
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut s = sup(&dir, Default::default(), spawns.clone(), true, true);
        s.ready_within = Duration::from_secs(30);
        let closing = s.closing();
        let s = Arc::new(Mutex::new(s));
        let s2 = s.clone();
        let start = std::thread::spawn(move || s2.lock().unwrap().sidecar("m").err());
        until("the start began", || std::fs::read_to_string(dir.join("nemo-speech.ledger")).is_ok_and(|t| t.ends_with('\n')));
        let pid = std::fs::read_to_string(dir.join("nemo-speech.ledger")).unwrap().split(' ').next().unwrap().parse::<u32>().unwrap();
        let t = Instant::now();
        closing.store(true, Ordering::SeqCst);
        let e = start.join().unwrap().unwrap();
        assert!(e.contains("quitting"), "{e}");
        assert!(t.elapsed() < TERM_GRACE + Duration::from_secs(1), "{:?}", t.elapsed());
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "no other try");
        assert!(proc_info(pid).is_none(), "its child is stopped");
        assert!(s.lock().unwrap().sidecar("m").err().unwrap().contains("quitting"), "and none begins");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sidecar_that_takes_sigterm_stops_at_once_and_says_so() {
        let dir = tmp();
        let mut s = sup(&dir, Default::default(), Default::default(), false, false);
        s.sidecar("m").unwrap();
        let t = Instant::now();
        let how = s.running.take().unwrap().1.stop();
        assert!(t.elapsed() < TERM_GRACE, "{:?}", t.elapsed());
        assert!(how.starts_with("stopped: exited") && how.contains("after SIGTERM"), "{how}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_a_crashed_sidecar_forked_is_stopped_with_it() {
        let dir = tmp();
        let mut s = sup_running(&dir, Default::default(), Default::default(), false, false, FORKS);
        let pid = s.sidecar("m").unwrap().pid();
        until("its children run", || group_members(pid as i32).len() >= 4);

        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        until("the leader exited", || !s.running_alive());
        s.running = None;
        assert_eq!(group_with_parents(pid as i32), vec![], "what it forked is stopped too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conversation_sets_nemo_endpoint_but_ordinary_dictation_keeps_its_default() {
        let dir = tmp();
        let seen = Arc::new(Seen::default());
        let mut s = sup(&dir, seen.clone(), Default::default(), false, false);
        let side = s.sidecar("m").unwrap();
        let ms = (crate::dictation::CONVERSATION_TRAILING_SILENCE_SECS * 1000.0).round() as u32;
        drop(side.settled_stream_with_endpoint("en", Some(ms)).unwrap());
        drop(side.settled_stream("pt").unwrap());
        let sessions = seen.sessions.lock().unwrap();
        assert_eq!(sessions[0]["endpointing_ms"], 900);
        assert_eq!(sessions[0]["language"], "en-US");
        assert!(sessions[1].get("endpointing_ms").is_none());
        drop(sessions);
        s.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn languages_are_the_servers_codes() {
        assert_eq!((language("pt"), language("en"), language("auto"), language("")), (Some("pt-BR"), Some("en-US"), None, None));
        let _ = AtomicBool::new(false);
    }
}
