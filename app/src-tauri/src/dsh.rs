

use crate::Log;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const ENV_KEY: &str = "RTH_DSH_KEY";
const ENV_URL: &str = "RTH_DSH_URL";
const ENV_MODEL: &str = "RTH_DSH_MODEL";
const ENV_MODEL_NAME: &str = "RTH_DSH_MODEL_NAME";
const ENV_WATCH: &str = "RTH_DSH_WATCH";
const ENV_WATCH_URL: &str = "RTH_DSH_WATCH_URL";
pub const ENV_WATCH_KEY: &str = "RTH_DSH_WATCH_KEY";
const BACKEND_DSH: &str = "dsh";

pub const HELLO: &str = "dsh+";

const FLASH_VARS: [&str; 4] = ["RTH_FLASH_KEY", "RTH_FLASH_URL", "RTH_FLASH_MODEL", "RTH_MEMORY_EGRESS"];

const DEEPSEEK_PROVIDER: &str = "deepseek";

const MODELS_TIMEOUT: Duration = Duration::from_secs(2);
const MODELS_MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, PartialEq)]
pub struct Endpoint {
    pub port: u16,
    key: String,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Endpoint {{ port: {}, key: <redacted> }}", self.port)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: Option<String>,
    pub ready: Option<bool>,
}

impl Model {
    pub fn new(id: &str, name: &str) -> Model {
        Model { id: id.into(), name: name.into(), provider: None, ready: None }
    }
}

pub fn is_deepseek_model(id: &str) -> bool {

    let Some((provider, model)) = id.split_once('/') else { return false };
    let p = provider.to_ascii_lowercase();
    p == DEEPSEEK_PROVIDER || p.starts_with("deepseek-") || model.to_ascii_lowercase().contains("deepseek")
}

pub fn offered(models: &[Model]) -> Vec<Model> {
    models.iter().filter(|m| m.ready != Some(false) && !is_deepseek_model(&m.id)).cloned().collect()
}

pub fn listed() -> Vec<Model> {
    STATE.lock().unwrap().names.clone()
}

pub fn answering_model() -> Option<String> {
    let s = STATE.lock().unwrap();
    matches!(s.route, Some(Route::Dsh)).then(|| s.model.as_ref().map(|m| m.id.clone())).flatten()
}

pub fn bridge_live(src: &Source) -> bool {
    src.owner != Owner::Off && read_from(src).is_ok()
}

#[cfg(feature = "menu_bar")]
pub fn own_endpoint_path(instance_root: &Path) -> PathBuf {
    instance_root.join("dsh").join("bridge").join("endpoint.json")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Owner {

    #[cfg_attr(not(feature = "menu_bar"), allow(dead_code))]
    Child { pid: u32, start_us: u64, port: u16 },

    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub owner: Owner,
}

pub fn source() -> Source {
    #[cfg(feature = "menu_bar")]
    return crate::dsh_child::source();
    #[cfg(not(feature = "menu_bar"))]
    Source { path: PathBuf::new(), owner: Owner::Off }
}

fn key_ok(key: &str) -> bool {
    (8..=512).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic())
}

pub fn endpoint_from(text: &str, alive: impl Fn(i32) -> bool) -> Result<Endpoint, &'static str> {
    endpoint_and_pid(text, alive).map(|(e, _)| e)
}

fn endpoint_and_pid(text: &str, alive: impl Fn(i32) -> bool) -> Result<(Endpoint, i32), &'static str> {
    let v: Value = serde_json::from_str(text).map_err(|_| "not JSON")?;
    if v["v"] != 1 {
        return Err("not version 1");
    }
    let port = v["port"].as_u64().filter(|p| (1..=65535).contains(p)).ok_or("no port")? as u16;
    let key = v["key"].as_str().filter(|k| key_ok(k)).ok_or("no usable key")?;
    let pid = v["pid"].as_i64().filter(|p| *p > 1 && *p <= i32::MAX as i64).ok_or("no pid")? as i32;
    if !alive(pid) {
        return Err("its DSH is not running");
    }
    Ok((Endpoint { port, key: key.to_string() }, pid))
}

fn file_ok(meta: &std::fs::Metadata) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    if !meta.file_type().is_file() {
        return Err("not a regular file");
    }
    if meta.uid() != unsafe { libc::geteuid() } {
        return Err("not this user's");
    }
    if meta.mode() & 0o777 != 0o600 {
        return Err("not mode 0600");
    }
    if meta.len() > 4096 {
        return Err("too large");
    }
    Ok(())
}

pub fn present(path: &Path) -> Result<(), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let not_found = |e: std::io::Error| if e.kind() == std::io::ErrorKind::NotFound { "no bridge" } else { "unreadable" };
    let dir = std::fs::symlink_metadata(path.parent().ok_or("no folder")?).map_err(not_found)?;
    if !dir.file_type().is_dir() || dir.uid() != unsafe { libc::geteuid() } || dir.mode() & 0o777 != 0o700 {
        return Err("its folder is not this user's, mode 0700");
    }
    file_ok(&std::fs::symlink_metadata(path).map_err(not_found)?)
}

pub fn read_endpoint(path: &Path) -> Result<Endpoint, &'static str> {
    read_with_pid(path).map(|(e, _)| e)
}

pub fn read_from(src: &Source) -> Result<Endpoint, &'static str> {
    if src.owner == Owner::Off {
        return Err("Unfold's own DSH is off");
    }
    let (ep, pid) = read_with_pid(&src.path)?;
    match src.owner {
        Owner::Child { pid: child, .. } if pid as u32 != child => Err("not Unfold's own DSH"),
        Owner::Child { port, .. } if ep.port != port => Err("not the registered bridge port"),
        Owner::Child { pid: child, start_us, port } if !listens(child, start_us, port) => Err("Unfold's own DSH doesn't listen on the bridge port"),
        _ => Ok(ep),
    }
}

fn listens(pid: u32, start_us: u64, port: u16) -> bool {
    crate::nemo::proc_info(pid).is_some_and(|i| i.start_us == start_us) && listening_ports(pid).contains(&port)
}

pub(crate) fn listening_ports(pid: u32) -> Vec<u16> {
    const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
    const SOCKET_FDINFO_SIZE: usize = 792;
    const SOI_KIND: usize = 256;
    const LPORT: usize = 268;
    const TCP_STATE: usize = 344;
    const SOCKINFO_TCP: i32 = 2;
    const TSI_S_LISTEN: i32 = 1;
    let fd_size = std::mem::size_of::<libc::proc_fdinfo>();
    let n = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if n <= 0 {
        return Vec::new();
    }
    let mut fds: Vec<libc::proc_fdinfo> = vec![unsafe { std::mem::zeroed() }; n as usize / fd_size + 16];
    let got = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast(), (fds.len() * fd_size) as i32) };
    fds.truncate(got.max(0) as usize / fd_size);
    let int_at = |b: &[u8], at: usize| i32::from_ne_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    let mut ports = Vec::new();
    for fd in fds.iter().filter(|f| f.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32) {
        let mut b = [0u8; SOCKET_FDINFO_SIZE];
        let r = unsafe { libc::proc_pidfdinfo(pid as i32, fd.proc_fd, PROC_PIDFDSOCKETINFO, b.as_mut_ptr().cast(), SOCKET_FDINFO_SIZE as i32) };
        if r as usize == SOCKET_FDINFO_SIZE && int_at(&b, SOI_KIND) == SOCKINFO_TCP && int_at(&b, TCP_STATE) == TSI_S_LISTEN {
            ports.push(u16::from_be(int_at(&b, LPORT) as u16));
        }
    }
    ports
}

fn present_at(src: &Source) -> bool {
    src.owner != Owner::Off && present(&src.path).is_ok()
}

fn read_with_pid(path: &Path) -> Result<(Endpoint, i32), &'static str> {
    use std::os::unix::fs::OpenOptionsExt;
    present(path)?;
    let mut f = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("no bridge"),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err("not a regular file"),
        Err(_) => return Err("unreadable"),
    };
    file_ok(&f.metadata().map_err(|_| "unreadable")?)?;
    let mut text = String::new();
    (&mut f).take(4097).read_to_string(&mut text).map_err(|_| "unreadable")?;
    endpoint_and_pid(&text, |pid| unsafe { libc::kill(pid, 0) } == 0)
}

pub fn model_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && id.bytes().all(|b| b.is_ascii_graphic()) && id.split_once('/').is_some_and(|(p, m)| !p.is_empty() && !m.is_empty())
}

pub fn models_from(body: &str) -> Vec<Model> {
    let v: Value = serde_json::from_str(body).unwrap_or_default();
    let Some(data) = v["data"].as_array() else { return Vec::new() };
    data.iter()
        .filter_map(|m| {
            let id = m["id"].as_str().filter(|id| model_id_ok(id))?;
            let name = m["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 200 && !n.chars().any(char::is_control)).unwrap_or(id);
            let provider = m["provider_name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 200 && !n.chars().any(char::is_control)).map(str::to_string);
            Some(Model { id: id.to_string(), name: name.to_string(), provider, ready: m["ready"].as_bool() })
        })
        .collect()
}

fn fetch_models(ep: &Endpoint) -> Result<Vec<Model>, String> {
    fetch_models_within(ep, MODELS_TIMEOUT)
}

fn fetch_models_within(ep: &Endpoint, total: Duration) -> Result<Vec<Model>, String> {
    let deadline = Instant::now() + total;
    let left = || deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or_else(|| "timed out".to_string());
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, ep.port));
    let mut s = TcpStream::connect_timeout(&addr, left()?).map_err(|e| format!("connect: {}", e.kind()))?;
    s.set_write_timeout(Some(left()?)).map_err(|e| e.kind().to_string())?;
    let req = format!("GET /v1/models HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\n\r\n", ep.port, ep.key);
    s.write_all(req.as_bytes()).map_err(|e| format!("write: {}", e.kind()))?;
    let (mut raw, mut buf) = (Vec::new(), [0u8; 8192]);
    loop {
        s.set_read_timeout(Some(left()?)).map_err(|e| e.kind().to_string())?;
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("read: {}", e.kind())),
        }
        if raw.len() as u64 > MODELS_MAX_BYTES {
            return Err("too large".into());
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("no HTTP response")?;
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(format!("HTTP {status}"));
    }
    Ok(models_from(body))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {

    Dsh,

    Not(&'static str),
}

pub fn with_dsh(mut env: Vec<(String, String)>, on: bool, ep: Option<&Endpoint>, model: Option<&Model>) -> (Vec<(String, String)>, Route) {
    if !on {
        return (env, Route::Not("off"));
    }
    let Some(ep) = ep else { return (env, Route::Not("no_bridge")) };
    let Some(m) = model else { return (env, Route::Not("no_models")) };
    env.retain(|(n, _)| !FLASH_VARS.contains(&n.as_str()) && ![ENV_KEY, ENV_URL, ENV_MODEL, ENV_MODEL_NAME, "RTH_BACKEND"].contains(&n.as_str()));
    env.push(("RTH_BACKEND".into(), BACKEND_DSH.into()));
    env.push((ENV_KEY.into(), ep.key.clone()));
    env.push((ENV_URL.into(), format!("http://127.0.0.1:{}/v1", ep.port)));
    env.push((ENV_MODEL.into(), m.id.clone()));
    env.push((ENV_MODEL_NAME.into(), m.name.clone()));
    (env, Route::Dsh)
}

pub fn with_watch(mut env: Vec<(String, String)>, ep: Option<&Endpoint>) -> Vec<(String, String)> {
    env.retain(|(n, _)| ![ENV_WATCH, ENV_WATCH_URL, ENV_WATCH_KEY].contains(&n.as_str()));
    if let Some(ep) = ep {
        env.push((ENV_WATCH.into(), "1".into()));
        env.push((ENV_WATCH_URL.into(), format!("http://127.0.0.1:{}", ep.port)));
        env.push((ENV_WATCH_KEY.into(), ep.key.clone()));
    }
    env
}

struct State {
    route: Option<Route>,

    model: Option<Model>,

    names: Vec<Model>,
}
static STATE: Mutex<State> = Mutex::new(State { route: None, model: None, names: Vec::new() });

static WATCH: AtomicU64 = AtomicU64::new(0);

static RESTARTS: AtomicU64 = AtomicU64::new(0);

const WATCH_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {

    pub automagik: bool,

    pub answer: &'a str,

    pub watch: bool,
}

impl<'a> Ask<'a> {

    fn want(&self) -> Result<&'a str, &'static str> {
        match self.answer {
            crate::settings::ANSWER_GRANITE => Err("granite"),
            crate::settings::ANSWER_DEEPSEEK => Err("direct"),
            _ if !self.automagik => Err("automagik_off"),
            a => Ok(a),
        }
    }
}

pub struct Prepared {
    pub dsh: Option<Vec<(String, String)>>,
    pub watch: Option<Endpoint>,
}

pub fn prepare(env: &[(String, String)], src: &Source, ask: Ask, log: &Log) -> Prepared {

    let gen = WATCH.fetch_add(1, Ordering::SeqCst) + 1;

    *STATUS.lock().unwrap() = None;
    let (reply, use_watch) = (ask.want().is_ok(), ask.automagik && ask.watch);
    let first = ask.automagik.then(|| read_from(src));
    let seen = first.as_ref().and_then(|r| r.as_ref().ok().cloned());
    let dsh = prepare_with(env, ask.want(), first.clone(), fetch_models, log);
    let watch_ep = watch_pair(use_watch, first.as_ref(), log);
    if reply || use_watch {
        watch(gen, src.clone(), log.clone(), reply.then_some(dsh.is_some()), use_watch, seen);
    }
    Prepared { dsh, watch: watch_ep }
}

pub fn list_now(src: &Source, log: &Log) {
    if src.owner == Owner::Off {
        return;
    }
    let Ok(ep) = read_from(src) else { return };
    match fetch_models(&ep) {
        Ok(models) => STATE.lock().unwrap().names = models,
        Err(e) => log.line(&format!("dsh: the bridge's model list could not be read ({})", scrub(&e, &ep))),
    }
}

pub fn pick<'a>(want: &str, models: &'a [Model]) -> Option<&'a Model> {
    if want == crate::settings::ANSWER_DEEPSEEK {
        return models.iter().find(|m| m.id.split_once('/').is_some_and(|(p, _)| p == DEEPSEEK_PROVIDER));
    }
    models.iter().find(|m| m.id == want)
}

fn watch_pair(on: bool, read: Option<&Result<Endpoint, &'static str>>, log: &Log) -> Option<Endpoint> {
    if !on {
        return None;
    }
    match read {
        Some(Ok(ep)) => {
            log.line("dsh watch: on: agentd follows Unfold Automagik's agents through the bridge");
            Some(ep.clone())
        }
        Some(Err(why)) => {
            log.line(&format!("dsh watch: no bridge ({why}): agentd starts without the watch, and restarts once a bridge runs"));
            None
        }
        None => None,
    }
}

#[derive(Debug, PartialEq)]
enum Next {
    Keep,

    Stop,

    Reconnect,

    Rewatch,
}

fn next_step(dsh: bool, seen: Option<&Endpoint>, now: Option<&Endpoint>) -> Next {
    match (dsh, now) {
        (true, now) if now == seen => Next::Keep,
        (true, _) => Next::Stop,
        (false, Some(e)) if Some(e) != seen => Next::Reconnect,
        (false, _) => Next::Keep,
    }
}

fn next_watch(seen: Option<&Endpoint>, now: Option<&Endpoint>) -> Next {
    match now {
        Some(e) if Some(e) != seen => Next::Rewatch,
        _ => Next::Keep,
    }
}

fn look(reply: Option<bool>, watching: bool, reply_seen: Option<&Endpoint>, watch_seen: Option<&Endpoint>, now: Option<&Endpoint>) -> Next {
    match reply.map(|dsh| next_step(dsh, reply_seen, now)) {
        Some(n) if n != Next::Keep => n,
        _ if watching => next_watch(watch_seen, now),
        _ => Next::Keep,
    }
}

fn watch(gen: u64, src: Source, log: Log, reply: Option<bool>, watching: bool, seen: Option<Endpoint>) {
    let (mut reply_seen, watch_seen) = (seen.clone(), seen);
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH_EVERY);
        if WATCH.load(Ordering::SeqCst) != gen {
            return;
        }
        let now = read_from(&src).ok();
        match look(reply, watching, reply_seen.as_ref(), watch_seen.as_ref(), now.as_ref()) {
            Next::Keep => {
                if reply == Some(false) && now.is_none() {

                    reply_seen = None;
                }
            }
            Next::Stop => {
                if stopped(gen, &log) {
                    crate::settings::refresh_open();
                    restart();
                }
                return;
            }
            n @ (Next::Reconnect | Next::Rewatch) => {
                if WATCH.load(Ordering::SeqCst) == gen {
                    log.line(if n == Next::Reconnect { "dsh: a bridge is running again: agentd restarts to use it" } else { "dsh watch: a different bridge runs now (a new port or key): agentd restarts with it" });
                    restart();
                }
                return;
            }
        }
    });
}

#[cfg(feature = "menu_bar")]
pub fn signed_in() -> Option<bool> {
    match &STATE.lock().unwrap().route {
        Some(Route::Dsh) => Some(true),
        Some(Route::Not("no_models")) => Some(false),
        _ => None,
    }
}

fn restart() {
    RESTARTS.fetch_add(1, Ordering::SeqCst);
    crate::setup::restart_chat();
}

pub fn restarts() -> u64 {
    RESTARTS.load(Ordering::SeqCst)
}

fn stopped(gen: u64, log: &Log) -> bool {
    let mut s = STATE.lock().unwrap();
    if WATCH.load(Ordering::SeqCst) != gen {
        return false;
    }
    s.route = Some(Route::Not("stopped"));
    s.model = None;
    drop(s);
    log.line("dsh: the bridge stopped or restarted: answering locally, agentd restarts");
    true
}

fn prepare_with(env: &[(String, String)], want: Result<&str, &'static str>, read: Option<Result<Endpoint, &'static str>>, list: impl FnOnce(&Endpoint) -> Result<Vec<Model>, String>, log: &Log) -> Option<Vec<(String, String)>> {
    let (ep, models) = match read {
        Some(Ok(ep)) => {
            let models = list(&ep).unwrap_or_else(|e| {
                log.line(&format!("dsh: the bridge's model list could not be read ({})", scrub(&e, &ep)));
                Vec::new()
            });
            (Some(ep), models)
        }
        Some(Err(why)) => {
            log.line(&format!("dsh: no bridge ({why})"));
            (None, Vec::new())
        }
        None => (None, Vec::new()),
    };
    let picked = want.ok().and_then(|w| pick(w, &models)).cloned();
    let (env, route) = match want {
        Err(why) => (env.to_vec(), Route::Not(why)),
        Ok(_) if ep.is_none() => (env.to_vec(), Route::Not("no_bridge")),
        Ok(_) if models.is_empty() => (env.to_vec(), Route::Not("no_models")),
        Ok(_) if picked.is_none() => (env.to_vec(), Route::Not("not_listed")),
        Ok(_) => with_dsh(env.to_vec(), true, ep.as_ref(), picked.as_ref()),
    };
    log.line(&format!("dsh: {}", match (&route, &picked) {
        (Route::Dsh, Some(m)) => format!("on: {} through Unfold Automagik first, the local model as its fallback, no saved memories sent", m.name),
        (Route::Not("direct"), _) => "not used (direct): DeepSeek answers directly".to_string(),
        (Route::Not(why), _) => format!("not used ({why}): Granite answers on this Mac"),
        (Route::Dsh, None) => unreachable!("a DSH route has its model"),
    }));
    let mut s = STATE.lock().unwrap();
    s.names = models;
    let dsh = route == Route::Dsh;
    s.model = picked.filter(|_| dsh);
    s.route = Some(route);
    dsh.then_some(env)
}

fn scrub(text: &str, ep: &Endpoint) -> String {
    text.replace(&ep.key, "<redacted>")
}

pub fn local_reason() -> Option<(&'static str, String)> {
    match STATE.lock().unwrap().route {
        Some(Route::Not(w)) if w != "granite" && w != "direct" => Some((w, crate::flash::local_name())),
        _ => None,
    }
}

pub fn active() -> bool {
    matches!(STATE.lock().unwrap().route, Some(Route::Dsh))
}

pub fn unsupported() {
    let mut s = STATE.lock().unwrap();
    if s.route == Some(Route::Dsh) {
        s.route = Some(Route::Not("unsupported"));
        s.model = None;
    }
}

pub fn name_of(id: &str) -> Option<String> {
    STATE.lock().unwrap().names.iter().find(|m| m.id == id).map(|m| m.name.clone())
}

pub fn view(settings: &crate::settings::Settings, src: &Source) -> Value {
    let on = settings.automagik_on() && settings.answers_via_bridge();

    let available = if on { read_from(src).is_ok() } else { present_at(src) };
    let s = STATE.lock().unwrap();
    let (route, why) = match &s.route {
        Some(Route::Dsh) => ("dsh", None),
        Some(Route::Not(w)) => ("not", Some(*w)),
        None => ("not_started", None),
    };
    let model = s.model.as_ref().map(|m| m.name.clone());
    let answering = on && available && route == "dsh" && model.is_some();
    let models: Vec<Value> = offered(&s.names).iter().map(|m| json!({"id": m.id, "name": m.name})).collect();
    let deepseek = pick(crate::settings::ANSWER_DEEPSEEK, &s.names).map(|m| m.id.clone());
    json!({"answer": settings.answer_with, "on": on, "available": available, "route": route, "why": why, "model": model, "answering": answering, "models": models, "deepseek": deepseek})
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WatchStatus {
    connected: bool,
    subscriptions: u64,
    drafts: u64,
}
static STATUS: Mutex<Option<WatchStatus>> = Mutex::new(None);

const NOTICE_KINDS: [&str; 6] = ["turn_done", "error", "approval_waiting", "status", "created", "subscription"];

pub fn on_frame(f: &Value) -> Option<Option<Value>> {
    let text = |k: &str, max: usize| f[k].as_str().map(|t| t.chars().take(max).collect::<String>());
    match f["kind"].as_str()? {
        "dsh_notice" => {
            let kind = f["event_kind"].as_str().filter(|k| NOTICE_KINDS.contains(k));
            let (Some(kind), Some(agent)) = (kind, text("agent_name", 200)) else { return Some(None) };
            let mut w = json!({"wire": "dsh_notice", "event_kind": kind, "agent_name": agent, "summary": text("summary", 500).unwrap_or_default()});
            if let Some(id) = f["change_id"].as_str().filter(|id| change_id_ok(id)) {
                w["change_id"] = json!(id);
            }
            Some(Some(w))
        }
        "dsh_watch_status" => {
            let n = |k: &str| f[k].as_u64().unwrap_or(0);
            let s = WatchStatus { connected: f["connected"] == true, subscriptions: n("subscriptions"), drafts: n("drafts") };
            let changed = STATUS.lock().unwrap().replace(s) != Some(s);
            if changed {
                crate::settings::refresh_open();
            }
            Some(Some(json!({"wire": "dsh_watch_status", "connected": s.connected, "subscriptions": s.subscriptions, "drafts": s.drafts})))
        }
        "effect_held" if f["effect_kind"] == "dsh_prompt" => {
            let id = f["effect_id"].as_str().filter(|id| crate::effects::valid_id(id))?;
            Some(Some(json!({"wire": "dsh_draft", "effect_id": id})))
        }
        _ => None,
    }
}

pub fn watching() -> bool {
    STATUS.lock().unwrap().is_some()
}

pub fn change_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}

pub fn undo_text(change_id: &str) -> String {
    format!("/dsh sub undo {change_id}")
}

pub fn watch_view(on: bool, src: &Source) -> Value {
    let available = if on { read_from(src).is_ok() } else { present_at(src) };
    let s = *STATUS.lock().unwrap();
    let (connected, subscriptions, drafts) = s.map_or((false, 0, 0), |s| (s.connected, s.subscriptions, s.drafts));
    json!({"on": on, "available": available, "connected": on && connected, "subscriptions": subscriptions, "drafts": drafts})
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "dsh-bridge-key-fictional-0123456789";

    fn base() -> Vec<(String, String)> {
        vec![("RTH_BACKEND".into(), "local".into()), ("RTH_LOCAL_MODEL".into(), "granite41-3b-q4km".into())]
    }
    fn has(env: &[(String, String)], k: &str) -> Option<String> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }
    fn ep() -> Endpoint {
        Endpoint { port: 51234, key: KEY.into() }
    }
    fn kimi() -> Model {
        Model { provider: Some("Kimi".into()), ..Model::new("kimi/kimi-k2", "Kimi K2") }
    }

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static ONE: Mutex<()> = Mutex::new(());
        ONE.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn off(path: PathBuf) -> Source {
        Source { path, owner: Owner::Off }
    }
    fn deepseek() -> Model {
        Model::new("deepseek/deepseek-v4-flash", "DeepSeek V4 Flash")
    }

    fn asking(answer: &str) -> crate::settings::Settings {
        #[allow(unused_mut)]
        let mut s = crate::settings::Settings { answer_with: answer.into(), ..Default::default() };
        #[cfg(feature = "menu_bar")]
        {
            s.use_own_dsh = true;
        }
        s
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("unfold-dsh-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn file(text: &str) -> String {
        format!(r#"{{"v": 1, "port": 51234, "key": "{KEY}", "pid": {}, "started": 1792997825000}}"#, std::process::id()) + text
    }

    #[test]
    fn a_dsh_start_sets_the_bridge_s_model_and_takes_no_memory_grant() {
        let (env, route) = with_dsh(base(), true, Some(&ep()), Some(&kimi()));
        assert_eq!(route, Route::Dsh);
        assert_eq!(has(&env, "RTH_BACKEND").as_deref(), Some("dsh"));
        assert_eq!(env.iter().filter(|(n, _)| n == "RTH_BACKEND").count(), 1);
        assert_eq!((has(&env, "RTH_DSH_URL").as_deref(), has(&env, "RTH_DSH_KEY").as_deref()), (Some("http://127.0.0.1:51234/v1"), Some(KEY)), "agentd's exact URL shape");
        assert_eq!((has(&env, "RTH_DSH_MODEL").as_deref(), has(&env, "RTH_DSH_MODEL_NAME").as_deref()), (Some("kimi/kimi-k2"), Some("Kimi K2")));
        let flash: Vec<(String, String)> = base().into_iter().chain(FLASH_VARS.iter().map(|k| (k.to_string(), "fictional".to_string()))).collect();
        let (with_flash_vars, _) = with_dsh(flash, true, Some(&ep()), Some(&kimi()));
        for k in FLASH_VARS {
            assert_eq!((has(&env, k), has(&with_flash_vars, k)), (None, None), "{k}: one provider at a time, and memories stay local");
        }
        assert_eq!(has(&env, "RTH_LOCAL_MODEL").as_deref(), Some("granite41-3b-q4km"), "the local model stays: it is the fallback");
    }

    #[test]
    fn off_no_bridge_or_no_model_leaves_the_start_as_it_was() {
        for (on, e, m, why) in [(false, Some(ep()), Some(kimi()), "off"), (true, None, Some(kimi()), "no_bridge"), (true, Some(ep()), None, "no_models")] {
            let (env, route) = with_dsh(base(), on, e.as_ref(), m.as_ref());
            assert_eq!((env, route), (base(), Route::Not(why)));
        }
    }

    #[test]
    fn the_endpoint_file_is_checked_before_its_key_is_used() {
        let me = std::process::id() as i32;
        let alive = |pid: i32| pid == me;
        let good = endpoint_from(&file(""), alive).unwrap();
        assert_eq!(good, ep());
        for bad in [
            r#"{"v": 2, "port": 51234, "key": "dsh-bridge-key-fictional-0123456789", "pid": 4242}"#.to_string(),
            r#"{"v": 1, "port": 0, "key": "dsh-bridge-key-fictional-0123456789", "pid": 4242}"#.to_string(),
            r#"{"v": 1, "port": 70000, "key": "dsh-bridge-key-fictional-0123456789", "pid": 4242}"#.to_string(),
            r#"{"v": 1, "port": 51234, "key": "short", "pid": 4242}"#.to_string(),
            r#"{"v": 1, "port": 51234, "key": "has a space in it", "pid": 4242}"#.to_string(),
            r#"{"v": 1, "port": 51234, "key": "dsh-bridge-key-fictional-0123456789"}"#.to_string(),
            "not json".to_string(),
        ] {
            assert!(endpoint_from(&bad, |_| true).is_err(), "{bad}");
        }
        assert_eq!(endpoint_from(&file(""), |_| false), Err("its DSH is not running"), "a stale file from a DSH that quit");
        assert!(!format!("{good:?}").contains(KEY), "Debug never prints the key");
    }

    #[test]
    fn only_this_user_s_0600_regular_file_counts() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("file");
        let p = d.join("bridge/endpoint.json");
        assert_eq!(read_endpoint(&p), Err("no bridge"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, file("")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(read_endpoint(&p), Err("its folder is not this user's, mode 0700"), "a folder others can list: refused");
        std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(read_endpoint(&p), Err("not mode 0600"), "readable by others: refused");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_endpoint(&p), Ok(ep()));
        let link = p.parent().unwrap().join("link.json");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert_eq!(read_endpoint(&link), Err("not a regular file"));
        std::fs::write(&p, "not json").unwrap();
        assert_eq!((present(&p), read_endpoint(&p)), (Ok(()), Err("not JSON")), "metadata alone passes; the read checks the text");
        assert_eq!(view(&asking("kimi/kimi-k2"), &off(p.clone()))["available"].as_bool(), Some(false), "Unfold Automagik off: nothing is read");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_watch_stops_dsh_once_the_bridge_is_gone_or_has_a_new_key() {
        let rotated = Endpoint { port: 51234, key: "dsh-bridge-key-fictional-rotated-000".into() };
        assert_eq!(next_step(true, Some(&ep()), Some(&ep())), Next::Keep, "the same bridge: keep going");
        assert_eq!(next_step(true, Some(&ep()), None), Next::Stop, "the file is gone (DSH quit), or its DSH died");
        assert_eq!(next_step(true, Some(&ep()), Some(&rotated)), Next::Stop, "a restart: new key");
        assert_eq!(next_step(true, Some(&ep()), Some(&Endpoint { port: 51235, key: KEY.into() })), Next::Stop, "a new port");
    }

    #[test]
    fn with_the_switch_on_a_bridge_that_comes_back_reconnects() {
        let rotated = Endpoint { port: 51234, key: "dsh-bridge-key-fictional-rotated-000".into() };
        assert_eq!(next_step(false, None, None), Next::Keep, "still no bridge");
        assert_eq!(next_step(false, None, Some(&rotated)), Next::Reconnect, "DSH restarted after the start found none");
        assert_eq!(next_step(false, Some(&ep()), Some(&ep())), Next::Keep, "the bridge it already tried (it listed no models): no restart loop");
        assert_eq!(next_step(false, Some(&ep()), Some(&rotated)), Next::Reconnect, "that bridge restarted");
    }

    #[test]
    fn the_watch_pair_reaches_agentd_beside_every_backend() {
        let (dsh, _) = with_dsh(base(), true, Some(&ep()), Some(&kimi()));
        for (backend, env) in [("local", base()), ("dsh", dsh)] {
            let before = env.clone();
            let env = with_watch(env, Some(&ep()));
            assert_eq!(has(&env, "RTH_BACKEND").as_deref(), Some(backend), "the reply's provider stays");
            assert_eq!((has(&env, "RTH_DSH_WATCH").as_deref(), has(&env, "RTH_DSH_WATCH_URL").as_deref(), has(&env, "RTH_DSH_WATCH_KEY").as_deref()), (Some("1"), Some("http://127.0.0.1:51234"), Some(KEY)), "{backend}: agentd's watch pair");
            assert_eq!(env.len(), before.len() + 3, "{backend}: nothing else changes");
            assert_eq!(with_watch(before.clone(), None), before, "{backend}: off or no bridge: no pair");
            let again = with_watch(with_watch(before.clone(), Some(&Endpoint { port: 1, key: "an-older-key-0000".into() })), Some(&ep()));
            assert_eq!(again.iter().filter(|(n, _)| n.starts_with("RTH_DSH_WATCH")).count(), 3, "{backend}: one pair, the latest bridge's");
            assert_eq!(with_watch(again, None), before, "{backend}: a start without a bridge drops an older pair");
        }
    }

    #[test]
    fn the_watch_alone_restarts_only_for_a_different_bridge() {
        let rotated = Endpoint { port: 51234, key: "dsh-bridge-key-fictional-rotated-000".into() };
        let moved = Endpoint { port: 51235, key: KEY.into() };
        assert_eq!(next_watch(Some(&ep()), Some(&ep())), Next::Keep, "the same bridge");
        assert_eq!(next_watch(Some(&ep()), None), Next::Keep, "the bridge went away: agentd's watch reconnects by itself");
        assert_eq!(next_watch(Some(&ep()), Some(&rotated)), Next::Rewatch, "a new key");
        assert_eq!(next_watch(Some(&ep()), Some(&moved)), Next::Rewatch, "a new port");
        assert_eq!(next_watch(None, None), Next::Keep, "still no bridge");
        assert_eq!(next_watch(None, Some(&ep())), Next::Rewatch, "agentd started without the pair, and a bridge runs now");

        assert_eq!(look(None, true, None, Some(&ep()), None), Next::Keep, "watch only: a vanished bridge restarts nothing");
        assert_eq!(look(Some(true), true, Some(&ep()), Some(&ep()), None), Next::Stop, "DSH answers: its bridge is gone, so the reply goes local");
        assert_eq!(look(Some(false), true, None, Some(&ep()), Some(&ep())), Next::Reconnect, "Answer with still waits for its bridge: one that came back is tried again, as before");
        assert_eq!(look(Some(false), true, Some(&ep()), Some(&ep()), Some(&ep())), Next::Keep, "the bridge both already tried");
        assert_eq!(look(Some(false), false, None, None, Some(&ep())), Next::Reconnect, "Answer with alone, as before");
        assert_eq!(look(None, false, None, None, Some(&rotated)), Next::Keep, "both off");
    }

    #[test]
    fn a_rotated_key_reaches_the_next_start_and_never_the_log() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("rotate");
        let log = crate::Log::open_in(None, &d);
        let p = d.join("bridge/endpoint.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::set_permissions(p.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let write = |key: &str| {
            std::fs::write(&p, format!(r#"{{"v": 1, "port": 51234, "key": "{key}", "pid": {}}}"#, std::process::id())).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        };
        let rotated = "dsh-bridge-key-fictional-rotated-000";
        write(KEY);
        let first = watch_pair(true, Some(&read_endpoint(&p)), &log);
        assert_eq!(has(&with_watch(base(), first.as_ref()), "RTH_DSH_WATCH_KEY").as_deref(), Some(KEY));
        write(rotated);
        let now = read_endpoint(&p).ok();
        assert_eq!(next_watch(first.as_ref(), now.as_ref()), Next::Rewatch, "the watch asks for a restart");
        let next = watch_pair(true, Some(&read_endpoint(&p)), &log);
        assert_eq!(has(&with_watch(base(), next.as_ref()), "RTH_DSH_WATCH_KEY").as_deref(), Some(rotated), "the restart's start reads the new key");
        assert_eq!(watch_pair(false, Some(&read_endpoint(&p)), &log), None, "off: no pair");
        std::fs::remove_file(&p).unwrap();
        assert_eq!(watch_pair(true, Some(&read_endpoint(&p)), &log), None, "no bridge: no pair");
        let text = std::fs::read_to_string(d.join("app.log")).unwrap();
        assert!(text.contains("dsh watch: on") && text.contains("dsh watch: no bridge (no bridge)") && !text.contains(KEY) && !text.contains(rotated), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_watch_s_frames_reach_the_page_field_by_field_and_the_view_holds_no_key() {
        let notice = json!({"kind": "dsh_notice", "event_kind": "turn_done", "agent_name": "builder", "summary": "tests pass", "extra": "dropped", "key": KEY});
        assert_eq!(on_frame(&notice), Some(Some(json!({"wire": "dsh_notice", "event_kind": "turn_done", "agent_name": "builder", "summary": "tests pass"}))));
        let change = json!({"kind": "dsh_notice", "event_kind": "subscription", "agent_name": "other", "summary": "Unfold, at your request: now watching other (turn_done)", "change_id": "c-1:2"});
        assert_eq!(on_frame(&change).unwrap().unwrap()["change_id"], "c-1:2", "Unfold's change carries its Undo");
        assert_eq!(on_frame(&json!({"kind": "dsh_notice", "event_kind": "subscription", "agent_name": "x", "summary": "", "change_id": "c 1; rm"})).unwrap().unwrap().get("change_id"), None, "not agentd's id: no Undo");
        assert_eq!(on_frame(&json!({"kind": "dsh_notice", "event_kind": "future_kind", "agent_name": "x", "summary": ""})), Some(None), "a kind the app does not know: taken, not shown");
        let long = on_frame(&json!({"kind": "dsh_notice", "event_kind": "error", "agent_name": "x", "summary": "y".repeat(900)})).unwrap().unwrap();
        assert_eq!(long["summary"].as_str().map(|s| s.chars().count()), Some(500));
        let draft = json!({"kind": "effect_held", "effect_id": "e-9", "effect_kind": "dsh_prompt", "rule": "dsh_prompt.always_held", "args_sha256": "ab", "token_sha256": "cd", "expires_utc_ms": 1});
        assert_eq!(on_frame(&draft), Some(Some(json!({"wire": "dsh_draft", "effect_id": "e-9"}))), "a draft waits in DSH: no token, no text");
        assert_eq!(on_frame(&json!({"kind": "effect_held", "effect_id": "e-8", "effect_kind": "forget"})), None, "other held effects are the cards'");
        assert_eq!(on_frame(&json!({"kind": "agent_delta", "text": "x"})), None);
        *STATUS.lock().unwrap() = None;
        assert!(!watching());
        let d = tmp("wview");
        let v = watch_view(true, &off(d.join("none.json")));
        assert_eq!(v, json!({"on": true, "available": false, "connected": false, "subscriptions": 0, "drafts": 0}), "nothing reported yet");
        assert_eq!(on_frame(&json!({"kind": "dsh_watch_status", "on": true, "connected": true, "subscriptions": 2, "drafts": 1})), Some(Some(json!({"wire": "dsh_watch_status", "connected": true, "subscriptions": 2, "drafts": 1}))));
        assert!(watching(), "an agentd that reports the watch takes the Undo command");
        let v = watch_view(true, &off(d.join("none.json")));
        assert_eq!(v.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["available", "connected", "drafts", "on", "subscriptions"], "view.dsh_watch's shape");
        assert_eq!((v["connected"].as_bool(), v["subscriptions"].as_u64(), v["drafts"].as_u64()), (Some(true), Some(2), Some(1)));
        assert_eq!(watch_view(false, &off(d.join("none.json")))["connected"], false, "off: not connected, whatever was reported");
        assert!(!v.to_string().contains(KEY) && !v.to_string().contains("51234") && !v.to_string().contains("127.0.0.1"), "no key, port or host in the view: {v}");
        assert_eq!(undo_text("c-1:2"), "/dsh sub undo c-1:2", "agentd's command (dsh_watch.rs UNDO)");
        assert!(change_id_ok("c-1777.2_a:b") && !change_id_ok("") && !change_id_ok("c 1") && !change_id_ok(&"c".repeat(129)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_model_list_keeps_what_agentd_takes_in_the_bridge_s_order() {
        let body = r#"{"object": "list", "data": [
            {"id": "kimi/kimi-k2", "object": "model", "owned_by": "kimi", "name": "Kimi K2", "provider_name": "Kimi"},
            {"id": "no-slash", "name": "Bad"},
            {"id": "has space/m", "name": "Bad"},
            {"id": "openai/gpt-x", "provider_name": "OpenAI"},
            {"id": "glm/glm-5", "name": "  "}
        ]}"#;
        assert_eq!(models_from(body), vec![kimi(), Model { provider: Some("OpenAI".into()), ..Model::new("openai/gpt-x", "openai/gpt-x") }, Model::new("glm/glm-5", "glm/glm-5")], "no name: the id");
        assert_eq!(models_from("{}"), vec![]);
        assert_eq!(models_from("not json"), vec![]);
    }

    #[test]
    fn answer_with_offers_ready_models_and_never_deepseek_through_the_bridge() {
        let body = r#"{"object": "list", "data": [
            {"id": "openai-codex/gpt-5.5", "name": "GPT-5.5", "provider_name": "ChatGPT", "ready": true, "levels": ["none", "low", "high"]},
            {"id": "kimi/kimi-k2", "name": "Kimi K2", "provider_name": "Kimi", "ready": false},
            {"id": "deepseek-official/deepseek-flash", "name": "DeepSeek Flash", "ready": true},
            {"id": "deepseek/deepseek-v4-pro", "name": "DeepSeek V4 Pro", "ready": true},
            {"id": "openrouter/deepseek/deepseek-v4", "name": "DeepSeek via OpenRouter", "ready": true},
            {"id": "glm/glm-5", "name": "GLM 5"}
        ]}"#;
        let listed = models_from(body);
        assert_eq!(listed[0], Model { provider: Some("ChatGPT".into()), ready: Some(true), ..Model::new("openai-codex/gpt-5.5", "GPT-5.5") });
        let ids: Vec<String> = offered(&listed).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["openai-codex/gpt-5.5", "glm/glm-5"], "ready only (no field: an older bridge, kept), no DeepSeek route or model");
        for id in ["deepseek/x", "deepseek-official/deepseek-flash", "DeepSeek-API/x", "openrouter/deepseek/deepseek-v4", "groq/DeepSeek-R1"] {
            assert!(is_deepseek_model(id), "{id}");
        }
        for id in ["deepseekish/x", "kimi/kimi-k2", "openai-codex/gpt-5.5", "deepseek", "granite"] {
            assert!(!is_deepseek_model(id), "{id}");
        }
    }

    #[test]
    fn the_key_reaches_only_the_child_s_environment_never_the_log_or_the_view() {
        let _one = serial();
        let d = tmp("log");
        let log = crate::Log::open_in(None, &d);
        let env = prepare_with(&base(), Ok("kimi/kimi-k2"), Some(Ok(ep())), |e| Err(format!("echoed {}", e.key)), &log);
        assert_eq!(env, None, "no model listed: Granite answers");
        let env = prepare_with(&base(), Ok("kimi/kimi-k2"), Some(Ok(ep())), |_| Ok(vec![kimi()]), &log).expect("a DSH start");
        assert_eq!(has(&env, "RTH_DSH_KEY").as_deref(), Some(KEY));
        assert!(active() && name_of("kimi/kimi-k2").as_deref() == Some("Kimi K2"), "the route nod names the reply's model as the bridge lists it");
        let v = view(&asking("kimi/kimi-k2"), &off(d.join("none.json")));
        assert_eq!((v["route"].as_str(), v["model"].as_str(), v["available"].as_bool(), v["answering"].as_bool()), (Some("dsh"), Some("Kimi K2"), Some(false), Some(false)), "no live bridge now: not answering");
        assert_eq!(v.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["answer", "answering", "available", "deepseek", "model", "models", "on", "route", "why"], "view.dsh's shape (the Self pane reads it)");
        assert_eq!((v["answer"].as_str(), v["models"].clone(), v["deepseek"].clone()), (Some("kimi/kimi-k2"), json!([{"id": "kimi/kimi-k2", "name": "Kimi K2"}]), Value::Null), "the picker's options, no DeepSeek listed");
        assert!(!v.to_string().contains(KEY) && !v.to_string().contains("51234") && !v.to_string().contains("127.0.0.1"), "no key, port or host in the view: {v}");
        assert_eq!(crate::flash::hello_prefix("granite41-3b-q4km"), "dsh+local@granite41-3b-q4km");
        let gen = WATCH.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(!stopped(gen - 1, &log) && active(), "an older start's watch changes nothing");
        assert!(stopped(gen, &log));
        let v = view(&asking("kimi/kimi-k2"), &off(d.join("none.json")));
        assert_eq!((v["route"].as_str(), v["why"].as_str(), v["model"].as_str(), active()), (Some("not"), Some("stopped"), None, false), "a stopped bridge: Settings and Privacy read local");
        let _ = prepare_with(&base(), Ok("kimi/kimi-k2"), Some(Ok(ep())), |_| Ok(vec![kimi()]), &log).expect("a DSH start again");
        assert!(crate::flash::accept("local@granite41-3b-q4km", "dsh+local@granite41-3b-q4km"), "an agentd without DSH: local");
        assert!(!active(), "a local-only agentd: not DSH");
        for why in ["granite", "automagik_off"] {
            assert_eq!(prepare_with(&base(), Err(why), None, |_| panic!("{why}: the bridge is never asked"), &log), None);
            assert_eq!(view(&asking("granite"), &off(d.join("none.json")))["why"], why);
        }
        let text = std::fs::read_to_string(d.join("app.log")).unwrap();
        assert!(text.contains("dsh: on: Kimi K2") && text.contains("not used (automagik_off): Granite answers on this Mac") && !text.contains(KEY), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn answer_with_picks_one_listed_model_else_granite_answers_and_says_why() {
        let _one = serial();
        let d = tmp("pick");
        let log = crate::Log::open_in(None, &d);
        let listed = vec![kimi(), deepseek(), Model::new("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro")];
        assert_eq!(pick("deepseek", &listed), Some(&deepseek()), "deepseek: the first DeepSeek model, wherever the bridge lists it");
        assert_eq!(pick("deepseek/deepseek-v4-pro", &listed).map(|m| m.name.as_str()), Some("DeepSeek V4 Pro"), "a picked id: that model");
        assert_eq!(pick("deepseek", &[kimi(), Model::new("deepseekish/x", "x")]), None, "only the deepseek provider");
        let route = |want, read: Option<Result<Endpoint, &'static str>>, models: Vec<Model>| {
            let env = prepare_with(&base(), want, read, move |_| Ok(models), &log);
            (env.map(|e| has(&e, "RTH_DSH_MODEL")), STATE.lock().unwrap().route.clone())
        };
        assert_eq!(route(Ok("deepseek"), Some(Ok(ep())), listed.clone()), (Some(Some("deepseek/deepseek-v4-flash".into())), Some(Route::Dsh)));
        assert_eq!(route(Ok("kimi/kimi-k2"), Some(Ok(ep())), listed.clone()), (Some(Some("kimi/kimi-k2".into())), Some(Route::Dsh)));
        assert_eq!(route(Ok("glm/glm-5"), Some(Ok(ep())), listed.clone()), (None, Some(Route::Not("not_listed"))), "a picked model the bridge doesn't list now");
        assert_eq!(route(Ok("deepseek"), Some(Ok(ep())), vec![kimi()]), (None, Some(Route::Not("not_listed"))), "DeepSeek isn't signed in there");
        assert_eq!(route(Ok("deepseek"), Some(Ok(ep())), vec![]), (None, Some(Route::Not("no_models"))));
        assert_eq!(route(Ok("deepseek"), Some(Err("no bridge")), listed.clone()), (None, Some(Route::Not("no_bridge"))));

        assert_eq!(route(Err("granite"), Some(Ok(ep())), listed.clone()), (None, Some(Route::Not("granite"))));
        assert_eq!(STATE.lock().unwrap().names, listed, "the picker's options");
        assert!(!active());
        let ask = |automagik, answer| Ask { automagik, answer, watch: false }.want();
        assert_eq!((ask(true, "granite"), ask(false, "granite"), ask(false, "deepseek"), ask(true, "deepseek")), (Err("granite"), Err("granite"), Err("direct"), Err("direct")), "DeepSeek is never the bridge's: Unfold calls it directly");
        assert_eq!((ask(false, "kimi/kimi-k2"), ask(true, "kimi/kimi-k2")), (Err("automagik_off"), Ok("kimi/kimi-k2")));

        assert_eq!(route(Err("direct"), Some(Ok(ep())), listed.clone()), (None, Some(Route::Not("direct"))));
        assert_eq!(local_reason(), None, "DeepSeek direct: the bridge has no reason to give (flash.rs gives DeepSeek's)");
        let v = view(&asking("deepseek"), &off(d.join("none.json")));
        assert_eq!((v["on"].as_bool(), v["why"].as_str(), v["deepseek"].as_str()), (Some(false), Some("direct"), Some("deepseek/deepseek-v4-flash")), "not a bridge answer; the picker still leaves the bridge's DeepSeek out");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_models_call_sends_the_bearer_and_the_host_the_bridge_checks() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = c.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            c.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"object\":\"list\",\"data\":[{\"id\":\"kimi/kimi-k2\",\"name\":\"Kimi K2\"}]}").unwrap();
            req
        });
        let got = fetch_models(&Endpoint { port, key: KEY.into() }).unwrap();
        let req = server.join().unwrap();
        assert_eq!(got, vec![Model::new("kimi/kimi-k2", "Kimi K2")], "a bridge with no provider_name or ready: neither");
        assert!(req.starts_with("GET /v1/models HTTP/1.0\r\n"), "{req}");
        assert!(req.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")) && req.contains(&format!("\r\nAuthorization: Bearer {KEY}\r\n")) && !req.contains("Origin"), "{req}");
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let _ = c.read(&mut [0u8; 2048]);
            c.write_all(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}").unwrap();
        });
        assert_eq!(fetch_models(&Endpoint { port, key: KEY.into() }), Err("HTTP 401".into()));
        server.join().unwrap();

        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let _ = c.read(&mut [0u8; 2048]);
            for _ in 0..40 {
                if c.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let t = Instant::now();
        assert!(fetch_models_within(&Endpoint { port, key: KEY.into() }, Duration::from_millis(400)).is_err());
        assert!(t.elapsed() < Duration::from_millis(1200), "cut at the deadline: {:?}", t.elapsed());
        server.join().unwrap();
    }

    #[test]
    fn the_listening_ports_are_read_from_libproc() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let me = std::process::id();
        assert!(listening_ports(me).contains(&port), "{:?}", listening_ports(me));
        drop(l);
        assert!(!listening_ports(me).contains(&port), "a closed listener is gone");

        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let c = std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap();
        assert!(!listening_ports(me).contains(&c.local_addr().unwrap().port()));
    }

    #[test]
    fn a_rogue_listener_on_the_bridge_port_with_a_forged_endpoint_file_is_refused() {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let d = tmp("rogue");
        let dir = d.join("bridge");
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let path = dir.join("endpoint.json");
        let write = |pid: u32, port: u16| {
            let _ = std::fs::remove_file(&path);
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).unwrap();
            std::io::Write::write_all(&mut f, format!(r#"{{"v": 1, "port": {port}, "key": "{KEY}", "pid": {pid}}}"#).as_bytes()).unwrap();
        };

        let mut child = std::process::Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let (pid, start) = (child.id(), crate::nemo::proc_info(child.id()).unwrap().start_us);
        let rogue = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = rogue.local_addr().unwrap().port();
        write(pid, port);
        let asked = restarts();
        let own = |pid, start_us| Source { path: path.clone(), owner: Owner::Child { pid, start_us, port } };
        assert_eq!(read_from(&own(pid, start)), Err("Unfold's own DSH doesn't listen on the bridge port"));
        assert!(!view(&asking("kimi/kimi-k2"), &own(pid, start))["available"].as_bool().unwrap(), "Settings reads it as no bridge");
        assert_eq!(restarts(), asked, "agentd isn't restarted for it");

        let me = std::process::id();
        let mine = crate::nemo::proc_info(me).unwrap().start_us;
        write(me, port);
        assert!(read_from(&own(me, mine)).is_ok());
        assert_eq!(read_from(&own(me, mine + 1)), Err("Unfold's own DSH doesn't listen on the bridge port"), "a PID reused by another start");
        child.kill().unwrap();
        child.wait().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
