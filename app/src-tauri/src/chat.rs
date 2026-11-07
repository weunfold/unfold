

use crate::{free_loopback, http, owned_open, spawn_owned, until, Config, Log, Open, Shared, Slot};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::WebviewWindow;

pub fn server_path(contents: &Path) -> PathBuf {
    contents.join("MacOS/llama-server")
}

pub fn server_args(cat: &Value, m: &Value, port: u16, model: &Path) -> Vec<String> {
    let strs = |v: &Value| v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    let mut a = strs(&cat["server_args"]);
    a.extend(strs(&m["server_args"]));

    if cat["floor_slot"] == 4 && np_of(&a) == 5 &&
        cat["agentd_slots"].as_object().is_some_and(|slots| slots.values().all(|s| s.as_str().and_then(|v| v.parse::<u32>().ok()).is_some_and(|n| n < 4))) {

        let context = ctx_of(&a).map(|n| n / 5 * 4);
        for i in 0..a.len().saturating_sub(1) {
            if a[i] == "-np" || a[i] == "--parallel" { a[i+1] = "4".into(); }
            if a[i] == "-c" || a[i] == "--ctx-size" {
                if let Some(n) = context { a[i+1] = n.to_string(); }
            }
        }
    }

    if !a.iter().any(|x| x == "--cache-ram" || x == "-cram" || x.starts_with("--cache-ram=") || x.starts_with("-cram=")) {
        a.extend(["--cache-ram".into(), "0".into()]);
    }
    a.extend(["--host".into(), "127.0.0.1".into(), "--port".into(), port.to_string(), "--alias".into(), m["id"].as_str().unwrap_or("").into(), "-m".into(), model.display().to_string()]);
    a
}

pub fn ctx_of(args: &[String]) -> Option<u32> {
    args.windows(2).filter(|w| w[0] == "-c" || w[0] == "--ctx-size").last().and_then(|w| w[1].parse().ok())
}

pub fn np_of(args: &[String]) -> u32 {
    args.windows(2).filter(|w| w[0] == "-np" || w[0] == "--parallel").last().and_then(|w| w[1].parse().ok()).filter(|n| *n >= 1).unwrap_or(1)
}

pub fn slot_ctx(args: &[String]) -> Option<u32> {
    ctx_of(args).map(|c| c / np_of(args))
}

pub fn slot_env(cat: &Value, args: &[String]) -> Result<Vec<(String, String)>, String> {
    const KEYS: [&str; 3] = ["RTH_LOCAL_SLOT", "RTH_LOCAL_DECIDE_SLOT", "RTH_LOCAL_CHECK_SLOT"];
    let Some(o) = cat.get("agentd_slots") else { return Ok(Vec::new()) };
    let o = o.as_object().ok_or("agentd_slots is not an object")?;
    let np = np_of(args);
    let mut keys = KEYS.to_vec();
    if o.contains_key("RTH_LOCAL_OPENER_SLOT") { keys.push("RTH_LOCAL_OPENER_SLOT"); }
    if o.len() != keys.len() || !keys.iter().all(|k| o.contains_key(*k)) {
        return Err(format!("agentd_slots needs exactly {}", keys.join(", ")));
    }
    keys.iter()
        .map(|k| {
            let v = o[*k].as_str().filter(|v| !v.is_empty() && v.len() <= 2 && v.bytes().all(|b| b.is_ascii_digit())).ok_or(format!("agentd_slots {k}: not a slot number"))?;
            match v.parse::<u32>() {
                Ok(n) if n < np => Ok((k.to_string(), v.to_string())),
                _ => Err(format!("agentd_slots {k}={v}: the server has {np} slot(s)")),
            }
        })
        .collect()
}

pub fn start_server(contents: &Path, cat: &Value, m: &Value, model: &Path, owned: &Shared, log: &Log, app_dir: &Path) -> Result<String, String> {
    start_server_at(&server_path(contents), cat, m, model, owned, log, app_dir)
}

pub fn start_server_at(server: &Path, cat: &Value, m: &Value, model: &Path, owned: &Shared, log: &Log, app_dir: &Path) -> Result<String, String> {
    let port = free_loopback()?.port();
    let out = owned_open(&app_dir.join("llama-server.log"), Open::Truncate).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(server);
    cmd.args(server_args(cat, m, port, model)).env_clear().env("PATH", "/usr/bin:/bin").env("HOME", crate::home()).current_dir("/").stdin(Stdio::null()).stdout(out).stderr(err);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let (pid, _) = spawn_owned(owned, Slot::Llm, &mut cmd)?;
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().map_err(|e| format!("{e}"))?;
    log.line(&format!("started llama-server (pid {pid}) on {addr} for {}", m["id"]));
    let up = until(owned, Duration::from_secs(180), || {
        if http(addr, "/health", "").is_some_and(|r| r.code == 200) {
            return Some(Ok(()));
        }
        let mut o = owned.lock().unwrap();
        match o.llm.as_mut().map(|c| c.try_wait()) {
            Some(Ok(Some(s))) => Some(Err(format!("llama-server exited while loading ({s}); see {}", app_dir.join("llama-server.log").display()))),
            _ => None,
        }
    })?;
    match up {
        Some(Ok(())) => {
            log.line(&format!("llama-server ready on {addr}"));
            Ok(format!("http://{addr}/v1"))
        }
        Some(Err(e)) => Err(e),
        None => Err("its model did not load within 180 s".into()),
    }
}

#[cfg(feature = "menu_bar")]
static REPLYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "menu_bar")]
pub fn replying() -> bool {
    REPLYING.load(std::sync::atomic::Ordering::SeqCst)
}

static ATTACHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn attached() -> bool {
    ATTACHED.load(std::sync::atomic::Ordering::SeqCst)
}

static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn next_seq() -> u64 {
    SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

pub fn host_frame(seq: u64, kind: &str, body: Value) -> Value {
    let t_ns = T0.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64;
    let mut f = json!({"v": 2, "id": format!("rth-app:{seq}"), "producer": "rth-app", "seq": seq, "t_ns": t_ns, "source": "host", "kind": kind});
    if let (Some(o), Some(b)) = (f.as_object_mut(), body.as_object()) {
        o.extend(b.clone());
    }
    f
}

pub fn reply_for_page(f: &Value, pending: &mut Option<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let kind = f["kind"].as_str().unwrap_or("");
    if kind == "response_diagnostic" && f["outcome_source"] == "local" {
        if pending.take().is_some() {
            let r = f["local_reason"].as_str();
            let mut e = json!({"error": local_reason(r)});

            if let Some(r) = r.filter(|r| FRIENDLY_REASONS.contains(r)) {
                e["reason"] = json!(r);
            }
            out.push(e);
        }
        return out;
    }

    if !matches!(kind, "response_started" | "agent_opener" | "agent_delta" | "agent_replace" | "agent_done") {
        return out;
    }
    if let Some(p) = pending.take() {
        out.push(p);
    }

    let id = |mut v: Value| {
        if let Some(r) = f["response_id"].as_str() {
            v["response_id"] = json!(r);
        }
        v
    };
    match kind {
        "agent_opener" => {
            if let Some(t) = f["text"].as_str().filter(|t| !t.trim().is_empty()) {
                out.push(id(json!({"opener": t})));
            }
        }
        "agent_delta" => {
            if let Some(t) = f["text"].as_str() {
                out.push(id(json!({"delta": t})));
            }
        }
        "agent_replace" => {
            if let Some(t) = f["text"].as_str() {
                out.push(id(json!({"replace": t})));
            }
        }
        "agent_done" => match f["status"].as_str() {
            Some("completed") => out.push(id(json!({"done": true}))),
            Some("cancelled") => *pending = Some(json!({"error": "Stopped."})),
            _ => *pending = Some(json!({"error": "It could not answer."})),
        },
        _ => {}
    }
    out
}

pub fn flush(pending: &mut Option<Value>) -> Option<Value> {
    pending.take()
}

pub const FRIENDLY_REASONS: [&str; 2] = ["local_context_exceeded", "local_reply_empty_at_cap"];

fn local_reason(r: Option<&str>) -> String {
    match r {
        Some("local_context_exceeded") => "That message is too long for the on-device model; try a shorter one or switch to DeepSeek.".into(),
        Some("local_reply_empty_at_cap") => "The on-device model ran out of room before answering; try a shorter message, a higher reply length limit, or DeepSeek.".into(),
        Some("local_model_unavailable") => "Its model is not loaded right now.".into(),
        Some("local_first_token_timeout") => "Its model took too long to start answering.".into(),
        Some("local_server_error") => "Its model server failed while answering.".into(),
        Some(r) => format!("It could not answer ({r})."),
        None => "It could not answer.".into(),
    }
}

pub fn session_of(hello: &Value) -> Option<(String, u64)> {
    Some((hello["instance"]["session_id"].as_str()?.to_string(), hello["command_generation"].as_u64()?))
}

pub fn command_body(command_id: &str, session_id: &str, generation: u64, text: &str) -> Value {
    json!({"command_id": command_id, "session_id": session_id, "expected_generation": generation, "text": text})
}

pub fn with_effort(mut body: Value, effort: &str) -> Value {
    body["effort"] = json!(effort);
    body
}

pub fn with_reply_max_tokens(mut body: Value, limit: Option<u32>) -> Value {
    if let Some(r) = limit {
        body["reply_max_tokens"] = json!(r);
    }
    body
}

pub fn with_reply_voice(mut body: Value, reply_voice: Option<&Value>) -> Value {
    if let Some(v) = reply_voice {
        body["reply_voice"] = v.clone();
    }
    body
}

pub fn admitted(f: &Value) -> Option<(u64, bool, String)> {
    (f["kind"] == "command_admitted").then(|| Some((f["generation"].as_u64()?, f["duplicate"] == true, f["status"].as_str().unwrap_or("").to_string()))).flatten()
}

pub fn stale_generation(reason: &str) -> Option<u64> {
    reason.contains("stale generation").then(|| reason.rsplit("the session is at ").next()?.trim().parse().ok()).flatten()
}

pub fn hhmm(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
}

pub fn memory_wire(f: &Value, ops: &mut std::collections::HashMap<String, String>, source: Option<&str>) -> Option<Value> {
    let kind = f["kind"].as_str()?;
    let set = |w: &mut Value, k: &str, v: &Value| {
        if !v.is_null() {
            w[k] = v.clone();
        }
    };
    let mut w = json!({"wire": kind});
    match kind {
        "memory_act" => {
            let (a, state) = (&f["act"], f["state"].as_str()?);
            let pid = a["proposal_id"].as_str().unwrap_or("").to_string();
            if let Some(op) = a["op"]["op"].as_str() {
                ops.insert(pid.clone(), op.to_string());
            }
            if matches!(state, "accepted" | "submitted") {
                return None;
            }
            w["state"] = json!(state);
            set(&mut w, "act", &ops.get(&pid).map(|o| json!(o)).unwrap_or(Value::Null));
            set(&mut w, "proposal_id", &a["proposal_id"]);
            set(&mut w, "reason", &a["reason"]);
            set(&mut w, "memory_id", &a["outcome"]["memory_id"]);
            set(&mut w, "revision", &a["outcome"]["revision"]);
        }
        "memory_recall" => {
            if f["recall"]["for"] == "session_start" {
                return None;
            }
            set(&mut w, "count", &f["count"]);
            set(&mut w, "status", &f["recall"]["status"]);
            set(&mut w, "for", &f["recall"]["for"]);
            set(&mut w, "items", &f["recall"]["items"]);
            return Some(w);
        }
        "memory_decided" => {
            let r = &f["receipt"];
            set(&mut w, "decision", &f["decision"]);
            set(&mut w, "blocked", &r["blocked"]);
            set(&mut w, "fallback", &r["fallback"]);
            set(&mut w, "decide_ms", &r["decide_ms"]);
            set(&mut w, "warm_wait_ms", &r["warm_wait_ms"]);
            set(&mut w, "proposal_id", &r["proposal_id"]);
        }
        _ => return None,
    }
    if let Some(src) = source {
        w["source"] = json!(src);
    }
    Some(w)
}

pub fn on_turn_frame(f: &Value, l: &crate::Log, w: &Sink) -> bool {
    let pick = |v: &Value, keys: &[&str]| Value::Object(keys.iter().filter_map(|k| v.get(*k).map(|x| (k.to_string(), x.clone()))).collect());
    let each = |v: &Value, keys: &[&str]| Value::Array(v.as_array().map(|a| a.iter().filter(|x| x.is_object()).map(|x| pick(x, keys)).collect()).unwrap_or_default());
    match f["kind"].as_str() {
        Some("turn_summary") => {

            let mut s = pick(f, &["kind", "response_id", "lang"]);
            if f["memory"].is_object() {
                let m = &f["memory"];
                let mut memory = pick(m, &["decision", "sent_to_cloud", "blocked"]);
                for (k, keys) in [("used", &["memory_id", "revision"][..]), ("picked", &["memory_id", "revision"][..]), ("changed", &["memory_id", "revision", "op"][..])] {
                    if m[k].is_array() {
                        memory[k] = each(&m[k], keys);
                    } else if m.get(k).is_some() {
                        memory[k] = Value::Null;
                    }
                }
                s["memory"] = memory;
            }
            if f["shown_text"].is_object() {
                s["shown_text"] = pick(&f["shown_text"], &["history_guard", "markdown_stripped", "sentence_cased", "truncated"]);
            }
            if f["effects"].is_array() {
                s["effects"] = each(&f["effects"], &["effect_id", "effect_kind", "state"]);
            }
            w(json!({"wire": "turn_summary", "summary": s}));
        }
        Some("turn_memory_text") => {
            let items: Vec<Value> = f["items"].as_array().map(|a| a.iter().filter(|i| i["memory_id"].is_string() && i["text"].is_string()).map(|i| json!({"memory_id": i["memory_id"], "text": i["text"]})).collect()).unwrap_or_default();
            l.line(&format!("memory: {} text(s) for {} (page only)", items.len(), f["response_id"].as_str().unwrap_or("?")));
            w(json!({"wire": "turn_memory_text", "response_id": f["response_id"], "items": items}));
        }
        Some("memory_text_revoked") => {
            let Some(id) = f["memory_id"].as_str() else { return true };
            l.line(&format!("memory: text revoked for {id}"));
            w(json!({"wire": "memory_text_revoked", "memory_id": id}));
        }

        Some("reply_spoken") => {
            #[cfg(feature = "voice_pane")]
            crate::settings::reply_frame(&json!({"wire": "reply_spoken", "response_id": f["response_id"]}));
        }
        _ => return false,
    }
    true
}

pub fn memory_line(w: &Value) -> Option<String> {
    let src = w["source"].as_str().map(|s| format!(" ({s})")).unwrap_or_default();
    match w["wire"].as_str()? {
        "memory_act" => Some(match (w["state"].as_str()?, w["act"].as_str()) {
            ("committed", op) => format!("memory: {} {} rev {}{src}", if op == Some("correct") { "updated" } else { "remembered" }, w["memory_id"].as_str().unwrap_or("?"), w["revision"]),
            ("proposed", op) => format!("memory: proposed {} {}", op.unwrap_or("?"), w["proposal_id"].as_str().unwrap_or("?")),
            (state, _) => format!("memory: not saved ({state}{})", w["reason"].as_str().map(|r| format!(": {r}")).unwrap_or_default()),
        }),
        "memory_recall" => {
            let n = w["count"].as_u64()?;
            (n > 0 && w["status"] != "held").then(|| format!("memory: recalled {n} ({})", w["status"].as_str().unwrap_or("?")))
        }
        "memory_decided" => match (w["decision"].as_str()?, w["blocked"].as_str(), w["fallback"].as_str()) {
            (_, Some(b), _) => Some(format!("memory: not saved: {b}")),
            ("none", _, Some(f)) => Some(format!("memory: no decision ({f})")),
            ("no_op", _, _) => None,
            (d, _, _) => Some(format!("memory: decided {d}{}{}", w["decide_ms"].as_u64().map(|m| format!(" in {m} ms")).unwrap_or_default(), w["warm_wait_ms"].as_u64().map(|m| format!(" after {m} ms of warm-up")).unwrap_or_default())),
        },
        _ => None,
    }
}

pub type Sink = std::sync::Arc<dyn Fn(Value) + Send + Sync>;

pub fn page_sink(win: &WebviewWindow) -> Sink {
    let w = win.clone();
    std::sync::Arc::new(move |v: Value| {
        let js = if v["wire"].is_string() {
            format!("window.__rthMemory && window.__rthMemory({v})")
        } else if let Some(line) = v["status"].as_str() {
            format!("window.__rthStatus && window.__rthStatus({})", json!(line))
        } else {
            format!("window.__rthReply && window.__rthReply({v})")
        };
        let _ = w.eval(js);
    })
}

struct Session {
    id: String,
    generation: u64,
    boot: String,
    n: u64,
    last: Option<(String, String, bool, String, Option<Value>)>,
    undos: Vec<(String, String)>,
}

const UNDOS_KEPT: usize = 16;

#[derive(Default)]
struct ReplyFence {
    response: Option<String>,
    cancelling: bool,
    cancelled: std::collections::VecDeque<String>,
}
enum Fenced { Pass, Hide, Cancel(String), End(Value) }
impl ReplyFence {
    fn request(&mut self) -> Option<String> { self.cancelling = true; self.response.clone() }
    fn frame(&mut self, f: &Value) -> Fenced {
        let kind = f["kind"].as_str().unwrap_or("");
        let id = f["response_id"].as_str().unwrap_or("");
        if !id.is_empty() && self.cancelled.iter().any(|r| r == id) { return Fenced::Hide; }
        match kind {
            "response_started" => {
                self.response = Some(id.into());
                if self.cancelling { return Fenced::Cancel(id.into()); }
            }
            "agent_done" if self.cancelling => {
                self.cancelling = false; self.response = None;
                self.cancelled.push_back(id.into());
                if self.cancelled.len() > 16 { self.cancelled.pop_front(); }
                return Fenced::End(json!({"done":true,"interrupted":true,"response_id":id}));
            }
            "agent_done" => self.response = None,
            "agent_opener" | "agent_delta" | "agent_replace" | "reply_spoken" if self.cancelling => return Fenced::Hide,
            _ => {}
        }
        Fenced::Pass
    }
}

struct FrontLegacy {
    link: std::sync::Arc<crate::voice_front::link::Link>,
    conn: std::sync::Arc<std::sync::Mutex<TcpStream>>,
    session: std::sync::Arc<std::sync::Mutex<Session>>,
    awaiting: std::sync::Arc<std::sync::atomic::AtomicBool>,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    input: String, wire: String, reply_voice: Option<Value>, attempted: bool,
    #[cfg(feature = "voice_pane")] order:Option<std::sync::Arc<std::sync::Mutex<crate::voice_front::worker::BackendOrder>>>,
}

trait FrontWriter:std::io::Write {
    fn write_wait(&self)->std::io::Result<Option<Duration>>;
    fn set_write_wait(&self,wait:Option<Duration>)->std::io::Result<()>;
    fn shutdown_attachment(&self);
}
impl FrontWriter for TcpStream {
    fn write_wait(&self)->std::io::Result<Option<Duration>>{self.write_timeout()}
    fn set_write_wait(&self,wait:Option<Duration>)->std::io::Result<()>{self.set_write_timeout(wait)}
    fn shutdown_attachment(&self){let _=self.shutdown(std::net::Shutdown::Both);}
}

fn front_write_locked(stream:&mut impl FrontWriter,frame:&Value,alive:&std::sync::atomic::AtomicBool)->std::io::Result<()> {
    use std::{io,sync::atomic::Ordering};
    if !alive.load(Ordering::SeqCst){return Err(io::Error::new(io::ErrorKind::NotConnected,"controller attachment closed"));}
    let sent=(|| {
        let previous=stream.write_wait()?;stream.set_write_wait(Some(Duration::from_secs(2)))?;
        let sent=writeln!(stream,"{frame}");let restored=stream.set_write_wait(previous);sent.and(restored)
    })();
    if sent.is_err(){alive.store(false,Ordering::SeqCst);stream.shutdown_attachment();}
    sent
}
fn front_write(conn:&std::sync::Mutex<TcpStream>,frame:&Value,alive:&std::sync::atomic::AtomicBool)->std::io::Result<()> {
    let mut stream=conn.try_lock().map_err(|_|std::io::Error::new(std::io::ErrorKind::WouldBlock,"controller writer busy"))?;
    front_write_locked(&mut *stream,frame,alive)
}

fn front_lock<T>(mutex:&std::sync::Mutex<T>,deadline:std::time::Instant)->crate::voice_front::Result<std::sync::MutexGuard<'_,T>> {
    use std::sync::TryLockError;
    loop {
        match mutex.try_lock() {
            Ok(guard)=>return Ok(guard),
            Err(TryLockError::Poisoned(_))=>return Err(crate::voice_front::Error::Closed),
            Err(TryLockError::WouldBlock)=>{}
        }
        let remaining=deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero(){return Err(crate::voice_front::Error::Busy);}
        std::thread::sleep(remaining.min(Duration::from_millis(2)));
    }
}
impl crate::voice_front::runtime::Legacy for FrontLegacy {
    fn send(&mut self,turn:&crate::conversation_log::Turn)->crate::voice_front::Result<String> {
        if !turn.legacy_attempted || turn.owner!=Some(crate::conversation_log::Owner::Legacy) {return Err(crate::voice_front::Error::Cancelled);}
        self.send_admitted(turn,&mut||Ok(turn.clone()))
    }
    fn send_admitted(&mut self,turn:&crate::conversation_log::Turn,fence:&mut dyn FnMut()->crate::voice_front::Result<crate::conversation_log::Turn>)->crate::voice_front::Result<String> {
        use crate::voice_front::Error;
        if self.attempted || turn.input!=self.input || !self.alive.load(std::sync::atomic::Ordering::SeqCst){return Err(Error::Cancelled);}
        let (session,conn)=(self.session.clone(),self.conn.clone());
        let deadline=std::time::Instant::now()+Duration::from_millis(250);
        let mut session=front_lock(&session,deadline)?;
        let mut stream=front_lock(&conn,deadline)?;
        let next=session.n.checked_add(1).ok_or(Error::Context)?;
        let command=format!("rth-app:{}:{}",session.boot,next);
        #[cfg(feature = "voice_pane")]
        if let Some(order)=&self.order {order.lock().unwrap_or_else(|e|e.into_inner()).expect(&command);}
        let body=with_reply_max_tokens(with_effort(with_reply_voice(command_body(&command,&session.id,session.generation,&self.wire),
            self.reply_voice.as_ref()),&crate::settings::turn_effort()),crate::settings::turn_reply_max_tokens());
        let frame=host_frame(next_seq(),"command_submitted",body);let id=frame["id"].as_str().ok_or(Error::Context)?.to_string();
        let (link,awaiting,alive)=(self.link.clone(),self.awaiting.clone(),self.alive.clone());let attempted=&mut self.attempted;
        let mut boundary_error=None;let error=&mut boundary_error;
        let result=link.legacy(&command,&id,move|| {
            let sent=match fence(){Ok(sent)=>sent,Err(e)=>{*error=Some(e);return Err(std::io::Error::other("legacy durable admission refused"));}};
            if !sent.legacy_attempted || sent.owner!=Some(crate::conversation_log::Owner::Legacy){*error=Some(Error::Context);return Err(std::io::Error::other("invalid legacy fence"));}
            *attempted=true;session.n=next;session.last=None;
            awaiting.store(true,std::sync::atomic::Ordering::SeqCst);
            let result=front_write_locked(&mut *stream,&frame,&alive);

            drop(stream);drop(session);result
        });
        if let Some(error)=boundary_error{return Err(error);}result.map_err(|_|Error::Http)
    }
}

pub struct Chat {
    #[cfg(feature = "voice_pane")]
    front:crate::front_native::Attachment,
    observe: std::sync::Arc<crate::voice_front::link::Link>,
    conn: std::sync::Arc<std::sync::Mutex<TcpStream>>,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    session: Option<std::sync::Arc<std::sync::Mutex<Session>>>,

    #[cfg(feature = "menu_bar")]
    bind: SocketAddr,

    awaiting: std::sync::Arc<std::sync::atomic::AtomicBool>,
    fence: std::sync::Arc<std::sync::Mutex<ReplyFence>>,
}

fn front_observe_frame(request: crate::conversation_log::observe::Request) -> Value {
    let seq=next_seq();let mut f=host_frame(seq,"observe_request",json!({"request":request}));
    f["id"]=json!(format!("rth-front-observe-{seq}"));f
}

pub(crate) fn write_frame(conn: &std::sync::Mutex<TcpStream>, kind: &str, body: Value) -> Result<String, String> {
    let f = host_frame(next_seq(), kind, body);
    writeln!(conn.lock().unwrap(), "{f}").map_err(|e| format!("could not send to it: {e}"))?;
    Ok(f["id"].as_str().unwrap_or("").to_string())
}

fn submit(conn: &std::sync::Mutex<TcpStream>, s: &mut Session, text: &str, reply_voice: Option<&Value>, retried: bool, said: &str) -> Result<String, String> {
    s.n += 1;
    let id = format!("rth-app:{}:{}", s.boot, s.n);
    let frame = write_frame(conn, "command_submitted", with_reply_max_tokens(with_effort(with_reply_voice(command_body(&id, &s.id, s.generation, text), reply_voice), &crate::settings::turn_effort()), crate::settings::turn_reply_max_tokens()))?;
    s.last = Some((frame, text.to_string(), retried, said.to_string(), reply_voice.cloned()));
    Ok(id)
}

pub type OnClose = Box<dyn Fn() + Send>;

pub fn closed_for_page(awaiting: bool) -> Option<Value> {
    awaiting.then(|| json!({"error": "It stopped answering."}))
}

impl Chat {

    pub fn connect(bind: SocketAddr, pid: u32, model_id: &str, sink: Sink, log: &Log, on_close: Option<OnClose>) -> Result<Chat, String> {
        let conn = TcpStream::connect_timeout(&bind, Duration::from_secs(3)).map_err(|e| format!("agent: connect: {e}"))?;
        conn.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e| e.to_string())?;
        let mut r = BufReader::new(conn.try_clone().map_err(|e| e.to_string())?);
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| format!("agent Hello: {e}"))?;
        let prefix = crate::flash::hello_prefix(model_id);
        let boot = crate::setup::hello_boot_for(line.trim_end().as_bytes(), pid, &prefix)?;
        log.line(&format!("chat: connected to agentd boot {boot} ({})", crate::flash::hello_prefix(model_id)));
        if crate::effects::CARDS.lock().unwrap().on_boot(&boot) {
            log.line("effect: agentd restarted, so its held cards are gone");
            sink(json!({"wire": "effects_lost"}));
        }
        let hello: Value = serde_json::from_str(line.trim_end()).unwrap_or_default();
        let session = session_of(&hello).map(|(id, generation)| {
            log.line(&format!("chat: turns go to session {id} as commands (generation {generation})"));
            std::sync::Arc::new(std::sync::Mutex::new(Session { id, generation, boot: boot.clone(), n: 0, last: None, undos: Vec::new() }))
        });
        if session.is_none() {
            log.line("chat: this agent has no instance session: turns go as user_text, without memory");
        }
        let observe = std::sync::Arc::new(crate::voice_front::link::Link::new(
            &session.as_ref().map(|s|s.lock().unwrap().id.clone()).unwrap_or_default(), front_observe_frame));
        let mut chat = Chat {
            #[cfg(feature = "voice_pane")] front:Default::default(),
            observe,
            conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
            alive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            session,
            #[cfg(feature = "menu_bar")]
            bind,
            awaiting: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fence: Default::default(),
        };

        chat.send("attach", json!({"protocol": 2, "attachment_id": "app", "role": "controller"}))?;
        loop {
            line.clear();
            r.read_line(&mut line).map_err(|e| format!("agent attach: {e}"))?;
            if line.is_empty() {
                return Err("agent attach: closed before ready".into());
            }
            let f: Value = serde_json::from_str(line.trim_end()).map_err(|_| "agent attach: not JSON")?;
            match f["kind"].as_str() {
                Some("ready") if f["role"] == "controller" => break,
                Some("ready") => return Err(format!("agent attach: admitted as {}, not the controller", f["role"])),
                Some("heartbeat") => continue,
                Some("error") | Some("refused") => return Err(format!("agent attach refused: {}", line.trim_end())),
                _ => continue,
            }
        }
        log.line("chat: attached as controller");
        ATTACHED.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::settings::SWITCHING.store(false, std::sync::atomic::Ordering::SeqCst);
        #[cfg(feature = "self_pane")]
        crate::self_link::LINK.attach(chat.conn.clone());
        let reader = chat.conn.lock().unwrap().try_clone().map_err(|e| e.to_string())?;
        let (w, l, alive, conn, session, awaiting) = (sink, log.clone(), chat.alive.clone(), chat.conn.clone(), chat.session.clone(), chat.awaiting.clone());
        let fence = chat.fence.clone();
        let observe = chat.observe.clone();
        #[cfg(feature = "voice_pane")]
        chat.front.bind(chat.observe.session());
        #[cfg(feature = "voice_pane")]
        let (native_order,native_route)=(chat.front.order.clone(),chat.front.route());
        std::thread::spawn(move || {
            #[cfg(feature = "voice_pane")]
            let mut native_revision=0;
            let mut pending: Option<Value> = None;
            let mut ops = std::collections::HashMap::new();
            let mut line = String::new();
            loop {

                let _ = reader.set_read_timeout(if pending.is_some() { Some(Duration::from_millis(400)) } else { None });
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let Ok(f) = serde_json::from_str::<Value>(line.trim_end()) else { continue };
                        #[cfg(feature = "voice_pane")]
                        let _delivery=native_route.delivery.lock().unwrap_or_else(|e|e.into_inner());
                        if observe.on_frame(&f) { continue; }

                        #[cfg(feature = "self_pane")]
                        if crate::self_link::LINK.on_frame(&f) { continue; }
                        #[cfg(feature = "voice_pane")]
                        {
                            let mut order=native_order.lock().unwrap_or_else(|e|e.into_inner());
                            if native_revision!=order.revision() {native_revision=order.revision();pending=None;}
                            let allowed=order.allows(&f);drop(order);
                            if !allowed {
                                if f["kind"]=="response_started" {
                                    native_route.order_refused(&l);
                                    if let Some(id)=f["response_id"].as_str().filter(|s|crate::conversation_log::observe::opaque(s)) {
                                        if front_write(&conn,&host_frame(next_seq(),"cancel",json!({"response_id":id})),&alive).is_err(){break;}
                                    }
                                }
                                continue;
                            }
                        }
                        let decision = fence.lock().unwrap().frame(&f);
                        match decision {
                            Fenced::Hide => continue,
                            Fenced::Cancel(id) => {
                                if let Err(e) = write_frame(&conn, "cancel", json!({"response_id":id})) {
                                    l.line(&format!("conversation: deferred cancel failed: {e}"));
                                }
                                continue;
                            }
                            Fenced::End(v) => {
                                awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
                                #[cfg(feature = "menu_bar")]
                                REPLYING.store(false, std::sync::atomic::Ordering::SeqCst);
                                pending = None; w(v); continue;
                            }
                            Fenced::Pass => {}
                        }

                        if let (Some((g, duplicate, status)), Some(s)) = (admitted(&f), &session) {
                            if !duplicate {
                                let mut s = s.lock().unwrap();
                                s.generation = s.generation.max(g);
                            } else if status == "interrupted" {

                                awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
                                *fence.lock().unwrap() = ReplyFence::default();
                                w(json!({"error": "That message wasn't answered (it was interrupted)."}));
                            }
                            continue;
                        }
                        if f["kind"] == "rejected" {
                            let reason = f["reason"].as_str().unwrap_or("").to_string();
                            l.line(&format!("chat: refused: {reason}"));
                            let undo = session.as_ref().and_then(|s| {
                                let mut s = s.lock().unwrap();
                                let i = s.undos.iter().position(|(frame, _)| f["cause"] == frame.as_str())?;
                                Some(s.undos.remove(i).1)
                            });
                            if let Some(id) = undo {
                                w(json!({"wire": "dsh_undo", "change_id": id, "state": "refused"}));
                                continue;
                            }
                            let mine = session.as_ref().and_then(|s| {
                                let mut s = s.lock().unwrap();
                                let (frame, text, retried, said, reply_voice) = s.last.clone()?;
                                (f["cause"] == frame.as_str()).then_some(())?;

                                let real = stale_generation(&reason).filter(|_| !retried)?;
                                s.generation = real;
                                #[cfg(feature = "voice_pane")]
                                native_order.lock().unwrap_or_else(|e|e.into_inner()).expect(&format!("rth-app:{}:{}",s.boot,s.n+1));
                                Some(submit(&conn, &mut s, &text, reply_voice.as_ref(), true, &said))
                            });
                            match mine {
                                Some(Ok(_)) => l.line("chat: resubmitted at the session's generation"),
                                _ => {
                                    awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
                                    *fence.lock().unwrap() = ReplyFence::default();
                                    w(json!({"error": "It could not take that message."}));
                                }
                            }
                            continue;
                        }

                        let card = crate::effects::CARDS.lock().unwrap().on_frame(&f);
                        if let Some(m) = card {
                            let id = m["effect_id"].as_str().unwrap_or("?");
                            l.line(&match m["wire"].as_str() {
                                Some("effect_card") => format!("effect: card {id} ({}) waits for a tap", m["effect_kind"].as_str().unwrap_or("?")),
                                _ => format!("effect: {id} {}{}", m["state"].as_str().unwrap_or("?"), m["reason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default()),
                            });
                            w(m);
                            continue;
                        }

                        if let Some(page) = crate::dsh::on_frame(&f) {
                            if let Some(m) = page {
                                l.line(&match m["wire"].as_str() {
                                    Some("dsh_notice") => format!("dsh watch: a {} notice", m["event_kind"].as_str().unwrap_or("?")),
                                    Some("dsh_draft") => format!("dsh watch: draft {} waits for a click on Send in DSH", m["effect_id"].as_str().unwrap_or("?")),
                                    _ => format!("dsh watch: {} ({} subscriptions, {} drafts)", if m["connected"] == true { "connected" } else { "not connected" }, m["subscriptions"], m["drafts"]),
                                });
                                w(m);
                            }
                            continue;
                        }

                        if on_turn_frame(&f, &l, &w) {
                            continue;
                        }

                        if let Some(page) = crate::flash::on_frame(&f) {
                            if let Some(m) = page {
                                if let Some(why) = m["provenance"]["fallback_reason"].as_str() {
                                    l.line(&format!("flash: answered by the local model ({why})"));
                                }
                                if crate::flash::violation(&m) {
                                    l.line("route: a high-effort reply arrived on the main thread (it should run in the background)");
                                }
                                w(m);
                            }
                            continue;
                        }
                        let said = session.as_ref().and_then(|s| s.lock().unwrap().last.as_ref().map(|l| l.3.clone()));
                        if let Some(m) = memory_wire(&f, &mut ops, said.as_deref()) {
                            if let Some(line) = memory_line(&m) {
                                l.line(&line);
                                w(json!({"status": line}));
                            }
                            w(m);
                            continue;
                        }
                        #[cfg(feature = "menu_bar")]
                        match f["kind"].as_str() {
                            Some("response_started") => REPLYING.store(true, std::sync::atomic::Ordering::SeqCst),
                            Some("agent_done") => REPLYING.store(false, std::sync::atomic::Ordering::SeqCst),
                            _ => {}
                        }
                        if f["kind"] == "agent_done" {
                            awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
                        }
                        for v in reply_for_page(&f, &mut pending) {
                            w(v);
                        }
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        #[cfg(feature = "voice_pane")]
                        let _delivery=native_route.delivery.lock().unwrap_or_else(|e|e.into_inner());
                        #[cfg(feature = "voice_pane")]
                        {let order=native_order.lock().unwrap_or_else(|e|e.into_inner());if native_revision!=order.revision(){native_revision=order.revision();pending=None;}}
                        if let Some(v) = flush(&mut pending) {
                            w(v);
                        }
                    }
                    Err(_) => break,
                }
            }
            #[cfg(feature = "voice_pane")]
            let _delivery=native_route.delivery.lock().unwrap_or_else(|e|e.into_inner());
            #[cfg(feature = "voice_pane")]
            native_route.close();
            #[cfg(feature = "voice_pane")]
            {let order=native_order.lock().unwrap_or_else(|e|e.into_inner());if native_revision!=order.revision(){pending=None;}}
            if let Some(v) = flush(&mut pending) {w(v);}
            observe.close();
            l.line("chat: agentd closed the connection");
            ATTACHED.store(false, std::sync::atomic::Ordering::SeqCst);
            #[cfg(feature = "menu_bar")]
            {

                crate::dsh_child::lease_gone();
                REPLYING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            alive.store(false, std::sync::atomic::Ordering::SeqCst);
            #[cfg(feature = "self_pane")]
            crate::self_link::LINK.close(&conn);
            if let Some(v) = closed_for_page(awaiting.load(std::sync::atomic::Ordering::SeqCst)) {
                w(v);
            }
            if let Some(f) = on_close {
                f();
            }
        });
        Ok(chat)
    }

    #[cfg(feature = "voice_pane")]
    pub fn front_enqueue(&mut self,submission:crate::front_native::Submission<'_>,page:&crate::front_native::Page,log:&Log)->Option<String> {
        let crate::front_native::Submission{text,wire,interruption,reply_voice,epoch}=submission;
        if !crate::settings::current().voice_front_templates || reply_voice.is_none() || !self.alive() || !self.observe.ready() {return None;}
        let selection=crate::settings::front_reply_locale()?;
        let session=self.observe.session().to_string();
        if !self.front.admit_capture(&session,selection) {self.front.show_capture_fallback(selection,page);return None;}
        if self.front.worker.is_none() {
            let transport=self.front_observe().ok()?;
            self.front.install(&session,transport,page.clone(),log.clone(),self.awaiting.clone()).ok()?;
        }
        let nonce=format!("front-{}-{}-{}",self.front.boot(),epoch.capture,epoch.turn);
        let inner=self.front_legacy(text,wire,reply_voice).ok()?;
        let capture=self.front.capture();let revision=capture.revision();
        *self.fence.lock().unwrap()=ReplyFence::default();
        self.awaiting.store(false,std::sync::atomic::Ordering::SeqCst);
        let backend=crate::front_native::Backend {inner:Box::new(inner),interruption,alive:self.alive.clone(),capture,owner:self.front.route().owner.clone(),revision};
        let input=crate::front_native::input(epoch,revision,nonce,text,selection,Box::new(backend));
        let worker=self.front.worker.as_ref()?;
        self.front.accepted(epoch);
        match worker.submit(crate::settings::current().voice_front_templates,&session,input) {
            Ok(nonce)=>Some(nonce),
            Err(_)=>{self.front.refused(epoch);None}
        }
    }

    #[allow(dead_code)]
    pub fn front_legacy(&self, input: &str, wire: &str, reply_voice: Option<Value>) -> std::io::Result<impl crate::voice_front::runtime::Legacy + Send + 'static> {
        if !crate::settings::current().voice_front_templates || !self.alive() || !self.observe.ready() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "voice front unavailable"));
        }
        let session = self.session.clone().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotConnected, "session unavailable"))?;
        Ok(FrontLegacy { #[cfg(feature = "voice_pane")] order:Some(self.front.order.clone()), link:self.observe.clone(),conn:self.conn.clone(),session,awaiting:self.awaiting.clone(),alive:self.alive.clone(),
            input:input.into(),wire:wire.into(),reply_voice,attempted:false })
    }

    #[allow(dead_code)]
    pub fn front_observe(&self) -> std::io::Result<impl crate::conversation_log::mirror::Transport> {
        use std::io;
        if !crate::settings::current().voice_front_templates || !self.alive() || !self.observe.ready() {
            return Err(io::Error::new(io::ErrorKind::NotConnected,"voice front unavailable"));
        }
        let (link,conn,alive)=(self.observe.clone(),self.conn.clone(),self.alive.clone());
        let session=link.session().to_string();
        crate::conversation_log::mirror::RpcTransport::new(crate::conversation_log::observe::SCHEMA,&session,move |request| {
            if !alive.load(std::sync::atomic::Ordering::SeqCst) {return Err(io::Error::new(io::ErrorKind::NotConnected,"controller closed"));}
            link.exchange(request,|frame| {
                front_write(&conn, &frame,&alive)
            })
        })
    }

    pub fn alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(feature = "menu_bar")]
    pub fn lease(&self) -> (SocketAddr, Option<String>) {
        (self.bind, self.session.as_ref().map(|s| s.lock().unwrap().id.clone()))
    }

    fn send(&mut self, kind: &str, body: Value) -> Result<(), String> {
        write_frame(&self.conn, kind, body).map(|_| ())
    }

    #[cfg(feature = "voice_pane")]
    pub fn interrupt(&mut self) -> Result<bool, String> {

        let mut fence = self.fence.lock().unwrap();
        if !self.waiting() { return Ok(false); }
        if let Some(id) = fence.request() { write_frame(&self.conn, "cancel", json!({"response_id":id}))?; }
        Ok(true)
    }

    pub fn session_available(&self) -> Result<(), String> {
        if self.session.is_none() { return Err("the Unfold Session is not ready".into()); }
        Ok(())
    }
    pub fn waiting(&self) -> bool { self.awaiting.load(std::sync::atomic::Ordering::SeqCst) }
    pub fn say_session(&mut self, text: &str, reply_voice: Option<&Value>) -> Result<String, String> {
        self.session_available()?;
        self.say(text, reply_voice)
    }

    pub fn say(&mut self, text: &str, reply_voice: Option<&Value>) -> Result<String, String> {

        #[cfg(feature = "voice_pane")]
        self.front.capture().stop_user(crate::settings::front_stop);

        self.awaiting.store(true, std::sync::atomic::Ordering::SeqCst);
        let r = match &self.session {
            Some(s) => {
                let mut s=s.lock().unwrap();
                #[cfg(feature = "voice_pane")]
                self.front.order.lock().unwrap_or_else(|e|e.into_inner()).expect(&format!("rth-app:{}:{}",s.boot,s.n+1));
                submit(&self.conn,&mut s,text,reply_voice,false,&format!("you said it, {}",hhmm(std::time::SystemTime::now())))
            },
            None => {
                let q = format!("q{}", SEQ.load(std::sync::atomic::Ordering::SeqCst) + 1);
                #[cfg(feature = "voice_pane")]
                self.front.order.lock().unwrap_or_else(|e|e.into_inner()).expect(&q);
                self.send("user_text", with_reply_max_tokens(with_effort(with_reply_voice(json!({"request_id": q, "text": text}), reply_voice), &crate::settings::turn_effort()), crate::settings::turn_reply_max_tokens())).map(|()| q)
            }
        };
        if r.is_err() {
            self.awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        r
    }

    pub fn release(&mut self, effect_id: &str) -> Result<bool, String> {
        let Some(s) = &self.session else { return Err("this agent has no session for a card's tap".into()) };
        let Some(token) = crate::effects::CARDS.lock().unwrap().take(effect_id) else { return Ok(false) };
        let mut s = s.lock().unwrap();
        s.n += 1;
        let id = format!("rth-app:{}:{}", s.boot, s.n);
        write_frame(&self.conn, "command_submitted", command_body(&id, &s.id, s.generation, &crate::effects::release_text(effect_id, &token)))?;
        Ok(true)
    }

    pub fn dsh_undo(&mut self, change_id: &str) -> Result<(), String> {
        let Some(s) = &self.session else { return Err("this agent has no session for an Undo".into()) };
        if !crate::dsh::change_id_ok(change_id) {
            return Err("not a change id".into());
        }
        if !crate::dsh::watching() {
            return Err("the DSH watch is not running in this agent".into());
        }
        let mut s = s.lock().unwrap();
        s.n += 1;
        let id = format!("rth-app:{}:{}", s.boot, s.n);
        let frame = write_frame(&self.conn, "command_submitted", command_body(&id, &s.id, s.generation, &crate::dsh::undo_text(change_id)))?;
        s.undos.push((frame, change_id.to_string()));
        if s.undos.len() > UNDOS_KEPT {
            s.undos.remove(0);
        }
        Ok(())
    }

    pub fn generation(&self) -> Option<u64> {
        self.session.as_ref().map(|s| s.lock().unwrap().generation)
    }
}

pub fn start(cfg: &Config, record: &Value, cat: &Value, talk: &str, model: &Path, win: &WebviewWindow, owned: &Shared, log: &Log) -> Result<Chat, String> {
    let c = crate::setup::contents(cfg)?;
    let m = cat["models"].as_array().into_iter().flatten().find(|m| m["id"] == talk).ok_or(format!("{talk} is not in the catalog"))?;
    let app_dir = crate::owned_dir(&cfg.data, &cfg.data.join("app").join(&cfg.label))?;
    #[cfg(feature = "voice_pane")]
    {
        let journal = crate::owned_open(&app_dir.join("rth-host.jsonl"), crate::Open::Append).map_err(|e|e.to_string())?;
        crate::floor_decide::journal_install(journal);

        if crate::doubletalk::telemetry_enabled(env!("UNFOLD_APP_VERSION"),std::env::var("UNFOLD_TRIAL_TELEMETRY").ok().as_deref()) {
            let file=crate::owned_open(&app_dir.join("doubletalk-levels.jsonl"),crate::Open::Append).map_err(|e|e.to_string())?;
            crate::doubletalk::install(file);
        }
    }
    let url = start_server(&c, cat, m, model, owned, log, &app_dir)?;
    let args = server_args(cat, m, 0, model);
    let slots = slot_env(cat, &args).unwrap_or_else(|e| {
        log.line(&format!("chat: {e}: agentd runs without slots (no Auto check)"));
        Vec::new()
    });
    let (bind, pid) = crate::setup::start_local_agent(cfg, record, &url, talk, m["sha256"].as_str(), slot_ctx(&args), slots, owned, log, &app_dir)?;
    let sink = page_sink(win);

    #[cfg(feature = "voice_pane")]
    crate::settings::reply_warm();
    #[cfg(feature = "voice_pane")]
    let sink: Sink = std::sync::Arc::new(move |v: Value| {
        crate::settings::reply_frame(&v);
        sink(v)
    });
    Chat::connect(bind, pid, talk, sink, log, Some(Box::new(crate::setup::poke)))
}

pub fn headless_check(a: &[String]) -> Result<Value, String> {
    let (server, agentd, gguf, id, dir, text, sha) = match a {
        [s, ag, g, i, d, t] => (s, ag, g, i, d, t, None),
        [s, ag, g, i, d, t, h] => (s, ag, g, i, d, t, Some(h.as_str())),
        _ => return Err("usage: --local-chat-check SERVER AGENTD GGUF ID NEWDIR TEXT [SHA256]".into()),
    };

    let abs = |p: &String| std::fs::canonicalize(p).map_err(|e| format!("{p}: {e}"));
    let (server, agentd, gguf) = (abs(server)?, abs(agentd)?, abs(gguf)?);
    let dir = PathBuf::from(dir);
    if dir.exists() {
        return Err(format!("{} exists: give a new scratch dir", dir.display()));
    }
    std::fs::create_dir_all(dir.join("inst/agentd")).map_err(|e| e.to_string())?;
    let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
    let log = crate::Log::open(&dir);
    let owned: Shared = Default::default();
    let cat = json!({"server_args": ["-ngl", "99", "-fa", "on", "--jinja", "--no-webui", "-c", "8192", "-np", "1", "--cache-ram", "0", "--offline"]});
    let m = json!({"id": id});
    let r = (|| {
        let url = start_server_at(&server, &cat, &m, &gguf, &owned, &log, &dir)?;
        let bind = free_loopback()?;
        let i = dir.join("inst");
        let s = |p: &Path| p.display().to_string();
        let env: Vec<(String, String)> = [("PATH", "/usr/bin:/bin".to_string()), ("HOME", s(&dir)), ("RTH_AGENT_MODE", "desktop".into()), ("RTH_INSTANCE_ROOT", s(&i)), ("RTH_STATE", s(&i.join("agentd/state.json"))), ("RTH_JOURNAL", s(&i.join("agentd/journal.jsonl"))), ("RTH_AGENT_BIND", bind.to_string()), ("RTH_BACKEND", "local".into()), ("RTH_LOCAL_LLM_URL", url.clone()), ("RTH_LOCAL_MODEL", id.clone())].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let mut cmd = Command::new(&agentd);
        cmd.env_clear()
            .envs(crate::setup::with_app_version(crate::setup::with_local_ctx(crate::setup::with_model_sha256(env, sha)?, ctx_of(&server_args(&cat, &m, 0, &gguf)))))
            .current_dir(i.join("agentd"))
            .stdin(Stdio::null())
            .stdout(owned_open(&dir.join("agentd.log"), Open::Truncate).map_err(|e| e.to_string())?)
            .stderr(owned_open(&dir.join("agentd.err.log"), Open::Truncate).map_err(|e| e.to_string())?);
        let (pid, _) = spawn_owned(&owned, Slot::Agent, &mut cmd)?;
        until(&owned, Duration::from_secs(30), || crate::port_open(bind).then_some(()))?.ok_or("agentd did not listen")?;
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        let sink: Sink = std::sync::Arc::new(move |v: Value| {
            println!("{v}");
            let _ = tx.send(v);
        });
        let t0 = std::time::Instant::now();
        let mut chat = Chat::connect(bind, pid, id, sink, &log, None)?;
        chat.say(text, None)?;
        let (mut reply, mut first) = (String::new(), None);
        loop {
            let v = rx.recv_timeout(Duration::from_secs(120)).map_err(|_| "no reply within 120 s".to_string())?;
            if let Some(d) = v["delta"].as_str() {
                first.get_or_insert(t0.elapsed().as_millis());
                reply.push_str(d);
            }
            if let Some(r) = v["replace"].as_str() {
                reply = r.to_string();
            }
            if v["done"] == true {
                let journal = std::fs::read_to_string(i.join("agentd/journal.jsonl")).unwrap_or_default();
                let provenance = journal.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["kind"] == "response_provenance");
                return Ok(json!({"ok": true, "reply": reply, "first_delta_ms": first, "total_ms": t0.elapsed().as_millis(), "url": url, "provenance": provenance}));
            }
            if v["error"].is_string() {
                return Err(format!("reply ended in error: {}", v["error"]));
            }
        }
    })();
    crate::stop_owned(&owned, &log);
    r
}

pub fn memory_check(a: &[String]) -> Result<Value, String> {
    let [agentd, url, id, dir, envf, texts @ ..] = a else { return Err("usage: --memory-chat-check AGENTD URL ID NEWDIR ENV.json TEXT|@restart|@pause MS|@effect tap|tap-again|undo|cancel...".into()) };
    if texts.is_empty() {
        return Err("give at least one turn".into());
    }
    let agentd = std::fs::canonicalize(agentd).map_err(|e| format!("{agentd}: {e}"))?;
    let mut extra: serde_json::Map<String, Value> = serde_json::from_str(&std::fs::read_to_string(envf).map_err(|e| format!("{envf}: {e}"))?).map_err(|e| format!("{envf}: {e}"))?;
    let instance_id = extra.remove("RTH_CHECK_INSTANCE_ID").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| format!("inst-check-{}", std::process::id()));
    let cloud = extra.remove("RTH_CHECK_USE_CLOUD").is_some_and(|v| v == "1");
    let answer = if cloud { crate::settings::ANSWER_DEEPSEEK } else { crate::settings::ANSWER_GRANITE };
    let switches = crate::settings::Settings { answer_with: answer.into(), ..Default::default() };
    let flash_key = extra.remove("RTH_CHECK_FLASH_KEY").and_then(|v| v.as_str().map(str::to_string));
    let dir = PathBuf::from(dir);
    let i = dir.join("inst");
    let state_file = i.join("agentd/state.json");

    if state_file.exists() {
        return Err(format!("{} exists: give a new scratch dir", state_file.display()));
    }
    std::fs::create_dir_all(i.join("agentd")).map_err(|e| e.to_string())?;
    let session = format!("sess-check-{}", std::process::id());
    let state = json!({"state_schema": 1, "instance_id": instance_id, "session_id": session, "created_utc_ms": 0, "written_by": "unmanaged"});
    std::fs::write(&state_file, format!("{state:#}\n")).map_err(|e| e.to_string())?;
    let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
    let log = crate::Log::open(&dir);
    let owned: Shared = Default::default();
    let s = |p: &Path| p.display().to_string();
    let r = (|| {
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        let boot = |n: usize| -> Result<Chat, String> {
            let bind = free_loopback()?;
            let mut env: Vec<(String, String)> = [("PATH", "/usr/bin:/bin".to_string()), ("HOME", s(&dir)), ("RTH_AGENT_MODE", "desktop".into()), ("RTH_INSTANCE_ROOT", s(&i)), ("RTH_STATE", s(&state_file)), ("RTH_JOURNAL", s(&i.join("agentd/journal.jsonl"))), ("RTH_AGENT_BIND", bind.to_string()), ("RTH_BACKEND", "local".into()), ("RTH_LOCAL_LLM_URL", url.clone()), ("RTH_LOCAL_MODEL", id.clone())].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
            env.extend(extra.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string())));
            let flash = |env| crate::flash::prepare_with(env, id, switches.answers_direct(), true, crate::flash::provider(&dir, Some(&log)), |_| Ok(flash_key.clone()), &log);
            let env = crate::setup::with_app_version(crate::setup::with_providers(env, &switches, &crate::dsh::source(), flash, id, &log));
            let mut cmd = Command::new(&agentd);
            cmd.env_clear().envs(env).current_dir(i.join("agentd")).stdin(Stdio::null())
                .stdout(owned_open(&dir.join(format!("agentd-{n}.log")), Open::Truncate).map_err(|e| e.to_string())?)
                .stderr(owned_open(&dir.join(format!("agentd-{n}.err.log")), Open::Truncate).map_err(|e| e.to_string())?);
            let (pid, _) = spawn_owned(&owned, Slot::Agent, &mut cmd)?;
            until(&owned, Duration::from_secs(30), || crate::port_open(bind).then_some(()))?.ok_or("agentd did not listen")?;
            let tx = tx.clone();
            let sink: Sink = std::sync::Arc::new(move |v: Value| {
                println!("{v}");
                let _ = tx.send(v);
            });
            let chat = Chat::connect(bind, pid, id, sink, &log, None)?;
            if chat.generation().is_none() {
                return Err("agentd's Hello named no instance session: turns would not reach memory".into());
            }
            Ok(chat)
        };
        let (mut boots, mut turns) = (1, Vec::new());
        let mut chat = boot(boots)?;
        for text in texts {
            println!("{}", json!({"check_step": text}));

            if let Some(ms) = text.strip_prefix("@listen ").and_then(|m| m.trim().parse::<u64>().ok()) {
                let end = std::time::Instant::now() + Duration::from_millis(ms.min(60_000));
                let mut frames = Vec::new();
                while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
                    if let Ok(v) = rx.recv_timeout(left) {
                        frames.push(v);
                    }
                }
                turns.push(json!({"listen_ms": ms, "frames": frames, "restarts_asked": crate::dsh::restarts()}));
                continue;
            }

            if let Some(n) = text.strip_prefix("@dsh-undo ").and_then(|m| m.trim().parse::<usize>().ok()) {
                let seen: Vec<&Value> = turns.iter().flat_map(|t| t["memory"].as_array().into_iter().chain(t["frames"].as_array()).flatten()).filter(|w| w["wire"] == "dsh_notice" && w["change_id"].is_string()).collect();
                let id = seen.get(n.wrapping_sub(1)).and_then(|w| w["change_id"].as_str()).ok_or(format!("@dsh-undo {n}: no such notice yet"))?.to_string();
                chat.dsh_undo(&id)?;
                turns.push(json!({"dsh_undo": id}));
                continue;
            }

            if text == "@await-restart" {
                let (asked, end) = (crate::dsh::restarts(), std::time::Instant::now() + Duration::from_secs(15));
                let mut frames = Vec::new();
                while crate::dsh::restarts() == asked {
                    let left = end.checked_duration_since(std::time::Instant::now()).ok_or("@await-restart: no restart asked within 15 s")?;
                    if let Ok(v) = rx.recv_timeout(left.min(Duration::from_millis(200))) {
                        frames.push(v);
                    }
                }
                drop(chat);
                crate::stop_local(&owned, &log);
                while rx.try_recv().is_ok() {}
                boots += 1;
                chat = boot(boots)?;
                turns.push(json!({"await_restart": boots, "frames": frames, "restarts_asked": crate::dsh::restarts()}));
                continue;
            }

            if let Some(ms) = text.strip_prefix("@pause ").and_then(|m| m.trim().parse::<u64>().ok()) {
                std::thread::sleep(Duration::from_millis(ms.min(30_000)));
                turns.push(json!({"pause_ms": ms}));
                continue;
            }

            if let Some(step) = text.strip_prefix("@effect ") {
                let kind = if step == "undo" { "restore" } else { "forget" };
                let seen: Vec<&Value> = turns.iter().flat_map(|t| t["memory"].as_array().into_iter().flatten()).collect();
                let Some(card) = seen.iter().rev().find(|w| w["wire"] == "effect_card" && w["effect_kind"] == kind) else { return Err(format!("@effect {step}: no {kind} card yet")) };
                let eid = card["effect_id"].as_str().unwrap_or_default().to_string();
                let sent = match step {
                    "tap" | "tap-again" | "undo" => chat.release(&eid)?,
                    "cancel" => {
                        let had = crate::effects::CARDS.lock().unwrap().cancel(&eid);
                        let after = chat.release(&eid)?;
                        turns.push(json!({"effect": "cancel", "effect_id": eid, "had_token": had, "release_after_cancel_sent": after}));
                        continue;
                    }
                    other => return Err(format!("@effect {other}: tap, tap-again, undo or cancel")),
                };
                let mut frames = Vec::new();
                let mut settled = false;
                loop {
                    match rx.recv_timeout(if settled { Duration::from_millis(1000) } else { Duration::from_secs(if sent { 10 } else { 2 }) }) {
                        Ok(v) => {
                            settled |= v["wire"] == "effect_update" && v["effect_id"] == eid.as_str();
                            frames.push(v);
                        }
                        Err(_) => break,
                    }
                }
                turns.push(json!({"effect": step, "effect_id": eid, "sent": sent, "memory": frames}));
                continue;
            }
            if text == "@restart" {
                drop(chat);
                crate::stop_local(&owned, &log);
                while rx.try_recv().is_ok() {}
                boots += 1;
                chat = boot(boots)?;
                turns.push(json!({"restart": boots}));
                continue;
            }
            chat.say(text, None)?;
            let (mut reply, mut memory, mut status) = (String::new(), Vec::new(), Vec::new());
            let mut end: Option<Value> = None;
            loop {
                let wait = if end.is_some() { Duration::from_millis(800) } else { Duration::from_secs(90) };
                let v = match rx.recv_timeout(wait) {
                    Ok(v) => v,
                    Err(_) if end.is_some() => break,
                    Err(_) => return Err(format!("no end for {text:?} within 90 s")),
                };
                if let Some(d) = v["delta"].as_str() {
                    reply.push_str(d);
                }
                if let Some(r) = v["replace"].as_str() {
                    reply = r.to_string();
                }
                if v["wire"].is_string() {
                    memory.push(v.clone());
                }

                if let (false, Some(l)) = (v["wire"].is_string(), v["status"].as_str()) {
                    status.push(json!(l));
                }
                if end.is_none() && (v["done"] == true || v["error"].is_string()) {
                    end = Some(v["error"].clone());
                }
            }
            turns.push(json!({"text": text, "reply": reply, "memory": memory, "mind": status, "error": end}));
        }
        let generation = chat.generation();
        Ok(json!({"ok": true, "session_id": session, "instance_id": instance_id, "generation": generation, "boots": boots, "restarts_asked": crate::dsh::restarts(), "turns": turns}))
    })();
    crate::stop_owned(&owned, &log);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_connection_tells_the_page_only_about_a_turn_still_waiting() {
        assert_eq!(closed_for_page(true), Some(json!({"error": "It stopped answering."})), "a turn cut by the restart says so");
        assert_eq!(closed_for_page(false), None, "a model switch between turns shows no failed reply");
    }

    #[test]
    fn the_host_prompt_cache_is_off_whatever_the_catalog_says() {

        let m = json!({"id": "granite41-3b-q4km"});
        for cat in [json!({"server_args": ["-ngl", "99", "-c", "8192"]}), json!({"server_args": ["-ngl", "99", "--cache-ram", "0"]})] {
            let a = server_args(&cat, &m, 1, Path::new("/m.gguf"));
            let at: Vec<usize> = a.iter().enumerate().filter(|(_, x)| *x == "--cache-ram").map(|(i, _)| i).collect();
            assert_eq!(at.len(), 1, "exactly once: {a:?}");
            assert_eq!(a[at[0] + 1], "0", "{a:?}");
        }
    }

    #[test]
    fn a_catalog_cache_ram_given_with_an_equals_sign_counts_as_set() {

        let m = json!({"id": "g"});
        for given in ["--cache-ram=512", "-cram=512"] {
            let a = server_args(&json!({"server_args": ["-ngl", "99", given]}), &m, 1, Path::new("/m.gguf"));
            assert_eq!(a.iter().filter(|x| x.starts_with("--cache-ram") || x.starts_with("-cram")).collect::<Vec<_>>(), [given], "{a:?}");
        }
    }

    #[test]
    fn the_server_runs_loopback_with_the_catalog_and_model_args() {
        let cat = json!({"server_args": ["-ngl", "99", "--offline"]});
        let m = json!({"id": "qwen35-4b-q4km", "server_args": ["--chat-template-kwargs", "{\"enable_thinking\":false}"]});
        let a = server_args(&cat, &m, 5555, Path::new("/d/models/qwen35-4b-q4km.gguf"));
        assert_eq!(a, ["-ngl", "99", "--offline", "--chat-template-kwargs", "{\"enable_thinking\":false}", "--cache-ram", "0", "--host", "127.0.0.1", "--port", "5555", "--alias", "qwen35-4b-q4km", "-m", "/d/models/qwen35-4b-q4km.gguf"]);
    }

    #[test]
    fn agentd_gets_the_servers_context_size() {
        let a = server_args(&json!({"server_args": ["-ngl", "99", "-c", "8192", "-np", "1"]}), &json!({"id": "g"}), 1, Path::new("/m.gguf"));
        assert_eq!(ctx_of(&a), Some(8192));
        assert_eq!(ctx_of(&["--ctx-size".into(), "4096".into()]), Some(4096));
        assert_eq!(ctx_of(&["-c".into(), "4096".into(), "-c".into(), "8192".into()]), Some(8192), "the last one wins, as in llama-server");
        assert_eq!(ctx_of(&["-ngl".into(), "99".into()]), None);
        assert_eq!(ctx_of(&["-c".into()]), None);
    }

    #[test]
    fn with_two_slots_agentd_budgets_one_slot_and_gets_its_three_slots() {
        let cat = json!({"server_args": ["-ngl", "99", "-c", "16384", "-np", "2"], "agentd_slots": {"RTH_LOCAL_SLOT": "0", "RTH_LOCAL_DECIDE_SLOT": "0", "RTH_LOCAL_CHECK_SLOT": "1"}});
        let a = server_args(&cat, &json!({"id": "g"}), 1, Path::new("/m.gguf"));
        assert_eq!((ctx_of(&a), np_of(&a), slot_ctx(&a)), (Some(16384), 2, Some(8192)), "each slot gets 8192, as with -np 1 -c 8192");
        assert_eq!(slot_ctx(&["-c".into(), "8192".into()]), Some(8192), "no -np: one slot");
        assert_eq!(slot_ctx(&["-c".into(), "8192".into(), "--parallel".into(), "4".into()]), Some(2048));
        let env = slot_env(&cat, &a).unwrap();
        assert_eq!(env, [("RTH_LOCAL_SLOT".to_string(), "0".to_string()), ("RTH_LOCAL_DECIDE_SLOT".into(), "0".into()), ("RTH_LOCAL_CHECK_SLOT".into(), "1".into())]);
        assert_eq!(slot_env(&json!({"server_args": []}), &a).unwrap(), vec![], "no agentd_slots: none");
        let one = server_args(&json!({"server_args": ["-c", "8192", "-np", "1"]}), &json!({"id": "g"}), 1, Path::new("/m.gguf"));
        for (bad, why) in [
            (json!({"RTH_LOCAL_SLOT": "0", "RTH_LOCAL_DECIDE_SLOT": "0"}), "two of three"),
            (json!({"RTH_LOCAL_SLOT": "0", "RTH_LOCAL_DECIDE_SLOT": "0", "RTH_LOCAL_CHECK_SLOT": "1", "RTH_OTHER": "1"}), "an extra key"),
            (json!({"RTH_LOCAL_SLOT": "0", "RTH_LOCAL_DECIDE_SLOT": "0", "RTH_LOCAL_CHECK_SLOT": 1}), "a number, not text"),
            (json!({"RTH_LOCAL_SLOT": "0", "RTH_LOCAL_DECIDE_SLOT": "0", "RTH_LOCAL_CHECK_SLOT": "-1"}), "not a slot"),
            (json!(["RTH_LOCAL_SLOT"]), "not an object"),
        ] {
            assert!(slot_env(&json!({"agentd_slots": bad}), &a).is_err(), "{why}");
        }
        assert!(slot_env(&cat, &one).is_err(), "slot 1 on a one-slot server");
    }

    #[test]
    fn host_frames_carry_the_v2_envelope() {
        let f = host_frame(3, "user_text", json!({"request_id": "q3", "text": "oi"}));
        assert_eq!((f["v"].clone(), f["kind"].clone(), f["source"].clone(), f["seq"].clone(), f["text"].clone()), (json!(2), json!("user_text"), json!("host"), json!(3), json!("oi")));
        assert_eq!(f["id"], "rth-app:3");
        assert!(f["t_ns"].is_u64());
        let (a, b) = (next_seq(), next_seq());
        assert_eq!(b, a + 1, "one counter for the producer's life");
    }

    #[test]
    fn a_turn_carries_its_thinking_level() {
        let body = with_effort(command_body("c1", "s", 3, "olá"), "high");
        assert_eq!(body, json!({"command_id": "c1", "session_id": "s", "expected_generation": 3, "text": "olá", "effort": "high"}));
        let f = host_frame(4, "user_text", with_effort(json!({"request_id": "q4", "text": "oi"}), "off"));
        assert_eq!((f["kind"].clone(), f["effort"].clone()), (json!("user_text"), json!("off")));
    }

    #[test]
    fn a_turn_carries_its_reply_length_limit_only_while_it_is_on() {
        let body = with_reply_max_tokens(with_effort(command_body("c1", "s", 3, "olá"), "high"), Some(4096));
        assert_eq!(body, json!({"command_id": "c1", "session_id": "s", "expected_generation": 3, "text": "olá", "effort": "high", "reply_max_tokens": 4096}));
        let f = host_frame(4, "user_text", with_reply_max_tokens(json!({"request_id": "q4", "text": "oi"}), Some(1)));
        assert_eq!((f["kind"].clone(), f["reply_max_tokens"].clone()), (json!("user_text"), json!(1)));
        let off = with_reply_max_tokens(command_body("c2", "s", 3, "olá"), None);
        assert!(off.get("reply_max_tokens").is_none(), "off: no key, never null (agentd refuses null)");
        let f = host_frame(5, "user_text", with_reply_max_tokens(json!({"request_id": "q5", "text": "oi"}), None));
        assert!(f.get("reply_max_tokens").is_none(), "off on user_text too");
    }

    #[test]
    fn a_turn_carries_reply_voice_only_when_given() {
        let body = command_body("c1", "s", 3, "olá");
        assert_eq!(with_reply_voice(body.clone(), None).to_string(), body.to_string(), "absent: the body as before");
        let v = json!({"dictated": true, "spoken": "full"});
        assert_eq!(with_reply_voice(body, Some(&v)), json!({"command_id": "c1", "session_id": "s", "expected_generation": 3, "text": "olá", "reply_voice": {"dictated": true, "spoken": "full"}}));
    }

    #[test]
    fn an_agent_replace_becomes_the_pages_replace() {
        let mut p = None;
        assert_eq!(reply_for_page(&json!({"kind": "agent_replace", "response_id": "b-r1", "text": "4"}), &mut p), vec![json!({"replace": "4", "response_id": "b-r1"})]);
        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "failed"}), &mut p), Vec::<Value>::new());
        assert_eq!(reply_for_page(&json!({"kind": "agent_replace", "text": ""}), &mut p), vec![json!({"error": "It could not answer."}), json!({"replace": ""})]);
    }

    #[test]
    fn reply_frames_become_the_pages_three_shapes() {
        let mut p = None;
        let none: Vec<Value> = vec![];
        assert_eq!(reply_for_page(&json!({"kind": "response_started", "response_id": "b-r1"}), &mut p), none);
        assert_eq!(reply_for_page(&json!({"kind": "agent_delta", "response_id": "b-r1", "text": "Oi"}), &mut p), vec![json!({"delta": "Oi", "response_id": "b-r1"})]);
        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "response_id": "b-r1", "status": "completed"}), &mut p), vec![json!({"done": true, "response_id": "b-r1"})]);

        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "failed"}), &mut p), none);
        assert_eq!(reply_for_page(&json!({"kind": "response_diagnostic", "outcome_source": "local", "local_reason": "local_model_unavailable"}), &mut p), vec![json!({"error": "Its model is not loaded right now."})]);

        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "failed"}), &mut p), none);
        assert_eq!(reply_for_page(&json!({"kind": "response_started"}), &mut p), vec![json!({"error": "It could not answer."})]);
        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "cancelled"}), &mut p), none);
        assert_eq!(flush(&mut p), Some(json!({"error": "Stopped."})));
        assert_eq!(flush(&mut p), None);
        assert_eq!(reply_for_page(&json!({"kind": "hello"}), &mut p), none);
    }

    #[test]
    fn turn_frames_reach_the_page_and_memory_texts_never_the_log() {
        let dir = std::env::temp_dir().join(format!("unfold-turn-{}-{}", std::process::id(), next_seq()));
        let log = crate::Log::open_in(None, &dir);
        let got = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let g = got.clone();
        let sink: Sink = std::sync::Arc::new(move |v| g.lock().unwrap().push(v));
        let text = "My dog is called Sol-7f3a";
        assert!(on_turn_frame(&json!({"kind": "turn_memory_text", "response_id": "r1", "items": [{"memory_id": "m-1", "text": text, "revision": 2}, {"memory_id": "m-2", "text": 5}, {"text": "no id"}]}), &log, &sink));
        assert!(on_turn_frame(&json!({"kind": "memory_text_revoked", "memory_id": "m-1", "text": text}), &log, &sink));
        let summary = json!({"kind": "turn_summary", "response_id": "r1", "lang": "pt", "memory": {"decision": "recall", "used": [{"memory_id": "m-1", "revision": 2, "text": text}, {"memory_id": "m-4", "revision": 1}], "picked": [{"memory_id": "m-1", "revision": 2, "text": text}], "sent_to_cloud": true, "changed": [{"memory_id": "m-3", "revision": 1, "op": "remember", "text": text}], "blocked": null},
            "shown_text": {"history_guard": false, "markdown_stripped": true, "sentence_cased": false, "truncated": null}, "effects": [{"effect_id": "e-7", "effect_kind": "forget", "state": "card", "why": text}], "raw_text": text});
        assert!(on_turn_frame(&summary, &log, &sink));

        assert!(on_turn_frame(&json!({"kind": "turn_summary", "response_id": "r2", "memory": {"decision": "remember", "changed": null}}), &log, &sink));
        assert!(on_turn_frame(&json!({"kind": "turn_summary", "response_id": "r3"}), &log, &sink));
        assert!(!on_turn_frame(&json!({"kind": "effect_released", "effect_id": "e-7"}), &log, &sink), "the card's (effects.rs)");
        assert!(!on_turn_frame(&json!({"kind": "agent_delta", "text": "x"}), &log, &sink), "not the turn row's");
        assert!(on_turn_frame(&json!({"kind": "reply_spoken", "response_id": "r4"}), &log, &sink), "the speech's");
        let pages = got.lock().unwrap().clone();
        assert!(!pages.iter().any(|p| p.to_string().contains("reply_spoken")), "the page never sees reply_spoken");
        assert_eq!(pages[0], json!({"wire": "turn_memory_text", "response_id": "r1", "items": [{"memory_id": "m-1", "text": text}]}), "texts as strings, by id, nothing else");
        assert_eq!(pages[1], json!({"wire": "memory_text_revoked", "memory_id": "m-1"}));
        assert_eq!(pages[2], json!({"wire": "turn_summary", "summary": {"kind": "turn_summary", "response_id": "r1", "lang": "pt",
            "memory": {"decision": "recall", "used": [{"memory_id": "m-1", "revision": 2}, {"memory_id": "m-4", "revision": 1}], "picked": [{"memory_id": "m-1", "revision": 2}], "sent_to_cloud": true, "changed": [{"memory_id": "m-3", "revision": 1, "op": "remember"}], "blocked": null},
            "shown_text": {"history_guard": false, "markdown_stripped": true, "sentence_cased": false, "truncated": null}, "effects": [{"effect_id": "e-7", "effect_kind": "forget", "state": "card"}]}}), "a summary is ids only: a text in it never reaches the page; regression's picked (the picks) passes beside used (all carried)");
        assert_eq!(pages[3], json!({"wire": "turn_summary", "summary": {"kind": "turn_summary", "response_id": "r2", "memory": {"decision": "remember", "changed": null}}}));
        assert_eq!(pages[4], json!({"wire": "turn_summary", "summary": {"kind": "turn_summary", "response_id": "r3"}}), "nothing known is not nothing happened");
        assert_eq!(pages.len(), 5);
        let logged = std::fs::read_to_string(dir.join("app.log")).unwrap_or_default();
        assert!(!logged.contains("Sol-7f3a"), "{logged}");
        let files: Vec<String> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        assert!(files.iter().all(|f| f == "app.log"), "nothing persisted beside the log: {files:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frame_kind_the_app_does_not_know_changes_nothing() {

        let summary = json!({"kind": "turn_summary", "response_id": "r1", "lang": "pt", "memory": {"decision": "recall", "used": [{"memory_id": "m-1", "revision": 2}], "sent_to_cloud": true, "changed": [], "blocked": null, "card": null},
            "shown_text": {"history_guard": false, "markdown_stripped": true, "sentence_cased": false, "truncated": null}, "effects": [], "text": "not for the page", "fallback_reason": "offline", "status": "failed"});
        let unknown = [summary, json!({"kind": "future_unknown_kind", "response_id": "r1", "text": "x", "wire": "memory_recall"})];
        let none: Vec<Value> = vec![];
        for f in &unknown {
            assert_eq!(crate::flash::on_frame(f), None, "not Flash's");
            assert_eq!(memory_wire(f, &mut std::collections::HashMap::new(), Some("oi")), None, "not a memory line");
            let mut p = None;
            assert_eq!((reply_for_page(f, &mut p), p), (none.clone(), None), "nothing for the page");
        }
        let log = crate::Log::open_in(None, &std::env::temp_dir().join("unfold-chat-unknown"));
        let sink: Sink = std::sync::Arc::new(|_: Value| panic!("not for the page"));
        assert!(!on_turn_frame(&unknown[1], &log, &sink), "an unknown kind is not the turn row's either");

        let mut p = None;
        assert_eq!(reply_for_page(&json!({"kind": "agent_delta", "text": "Oi"}), &mut p), vec![json!({"delta": "Oi"})]);
        assert_eq!(reply_for_page(&unknown[0], &mut p), none);
        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "completed"}), &mut p), vec![json!({"done": true})]);

        assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "failed"}), &mut p), none);
        assert_eq!(reply_for_page(&unknown[1], &mut p), none);
        assert_eq!(reply_for_page(&json!({"kind": "response_diagnostic", "outcome_source": "local", "local_reason": "local_first_token_timeout"}), &mut p), vec![json!({"error": "Its model took too long to start answering."})]);
    }

    #[test]
    fn a_too_long_message_and_an_empty_reply_at_the_limit_say_what_to_do_never_the_raw_reason() {
        let fail = |reason: &str| {
            let mut p = None;
            assert_eq!(reply_for_page(&json!({"kind": "agent_done", "status": "failed"}), &mut p), Vec::<Value>::new());
            reply_for_page(&json!({"kind": "response_diagnostic", "outcome_source": "local", "local_reason": reason}), &mut p)
        };
        for (reason, said) in [
            ("local_context_exceeded", "That message is too long for the on-device model; try a shorter one or switch to DeepSeek."),
            ("local_reply_empty_at_cap", "The on-device model ran out of room before answering; try a shorter message, a higher reply length limit, or DeepSeek."),
        ] {
            let out = fail(reason);
            assert_eq!(out, vec![json!({"error": said, "reason": reason})], "the page's own line by its reason");
            assert!(!out[0]["error"].as_str().unwrap().contains("local_"), "never the raw reason");
        }
        assert_eq!(fail("local_server_error"), vec![json!({"error": "Its model server failed while answering."})], "other reasons: as before, no reason key");
        assert_eq!(fail("local_other"), vec![json!({"error": "It could not answer (local_other)."})]);
    }

    #[test]
    fn turns_are_commands_on_the_hellos_session() {
        let hello = json!({"kind": "hello", "instance": {"instance_id": "i", "session_id": "sess-1"}, "command_generation": 4});
        assert_eq!(session_of(&hello), Some(("sess-1".into(), 4)));
        assert_eq!(session_of(&json!({"kind": "hello"})), None, "no session: user_text");
        assert_eq!(session_of(&json!({"kind": "hello", "instance": {"session_id": "s"}})), None, "no generation: no admission");
        let f = host_frame(9, "command_submitted", command_body("rth-app:b-1:1", "sess-1", 4, "remember that my favourite café is Lua"));
        assert_eq!((f["kind"].clone(), f["command_id"].clone(), f["session_id"].clone(), f["expected_generation"].clone()), (json!("command_submitted"), json!("rth-app:b-1:1"), json!("sess-1"), json!(4)));
        assert_eq!(admitted(&json!({"kind": "command_admitted", "command_id": "c", "generation": 5, "status": "admitted"})), Some((5, false, "admitted".into())));
        assert_eq!(admitted(&json!({"kind": "command_admitted", "generation": 3, "status": "interrupted", "duplicate": true})), Some((3, true, "interrupted".into())));
        assert_eq!(admitted(&json!({"kind": "agent_done", "generation": 5})), None);
        assert_eq!(stale_generation("command c: stale generation: expected 4, the session is at 6"), Some(6));
        assert_eq!(stale_generation("command c: stale session \"x\": the instance's session is \"y\""), None);
        assert_eq!(hhmm(std::time::SystemTime::now()).len(), 5);
    }

    #[test]
    fn conversation_and_dsh_turns_are_commands_on_the_same_session_never_private_user_text() {
        use std::sync::{Arc, Mutex, atomic::AtomicBool};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let conn = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut c = Chat {
            #[cfg(feature = "voice_pane")] front:Default::default(),
            observe: std::sync::Arc::new(crate::voice_front::link::Link::new("test", front_observe_frame)),
            conn: Arc::new(Mutex::new(conn)), alive: Arc::new(AtomicBool::new(true)),
            session: Some(Arc::new(Mutex::new(Session {id:"shared-session".into(), generation:4, boot:"test".into(), n:0, last:None, undos:vec![]}))),
            #[cfg(feature = "menu_bar")]
            bind: listener.local_addr().unwrap(),
            awaiting: Arc::new(AtomicBool::new(false)),
            fence: Default::default(),
        };
        let mut reader = BufReader::new(peer);
        for (text, dictated) in [("spoken endpoint", true), ("typed in DSH", false)] {
            c.say_session(text, Some(&json!({"dictated":dictated,"spoken":"full"}))).unwrap();
            let mut line = String::new(); reader.read_line(&mut line).unwrap();
            let frame: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(frame["kind"], "command_submitted");
            assert_eq!(frame["session_id"], "shared-session"); assert_eq!(frame["text"], text);
            assert_eq!(frame["reply_voice"], json!({"dictated":dictated,"spoken":"full"}));
            assert!(c.waiting(), "the session sender tracks the in-flight turn");
            c.awaiting.store(false, std::sync::atomic::Ordering::SeqCst);
        }
        #[cfg(feature = "voice_pane")]
        {
            c.awaiting.store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(matches!(c.fence.lock().unwrap().frame(&json!({"kind":"response_started","response_id":"response-old"})), Fenced::Pass));
            assert!(c.interrupt().unwrap());
            let mut line = String::new(); reader.read_line(&mut line).unwrap();
            let frame: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(frame["kind"], "cancel"); assert_eq!(frame["response_id"], "response-old");
        }
        c.session = None;
        assert!(c.say_session("no private fallback", None).is_err());
    }

    #[test]
    fn memory_receipts_are_flattened_for_the_page_and_the_mind_line() {

        let mut ops = std::collections::HashMap::new();
        let src = Some("you said it, 14:02");
        let decided = json!({"kind": "memory_decided", "response_id": "r1", "decision": "remember", "receipt": {"decide_ms": 89, "proposal_id": "p-1", "part": "memory.decide@1/mini-v4"}});
        let w = memory_wire(&decided, &mut ops, src).unwrap();
        assert_eq!(w, json!({"wire": "memory_decided", "decision": "remember", "decide_ms": 89, "proposal_id": "p-1", "source": "you said it, 14:02"}));
        assert_eq!(memory_line(&w).as_deref(), Some("memory: decided remember in 89 ms"));
        let proposed = json!({"kind": "memory_act", "state": "proposed", "act": {"op": {"op": "remember", "text": "my favourite café is Lua"}, "proposal_id": "p-1"}});
        let w = memory_wire(&proposed, &mut ops, src).unwrap();
        assert_eq!((w["act"].clone(), w.get("text")), (json!("remember"), None), "the words never go out");
        assert_eq!(memory_line(&w).as_deref(), Some("memory: proposed remember p-1"));
        for st in ["accepted", "submitted"] {
            assert_eq!(memory_wire(&json!({"kind": "memory_act", "state": st, "act": {"proposal_id": "p-1"}}), &mut ops, src), None);
        }
        let committed = json!({"kind": "memory_act", "state": "committed", "act": {"outcome": {"memory_id": "rm-1", "replayed": false, "revision": 1}, "proposal_id": "p-1"}});
        let w = memory_wire(&committed, &mut ops, src).unwrap();
        assert_eq!(w, json!({"wire": "memory_act", "state": "committed", "act": "remember", "proposal_id": "p-1", "memory_id": "rm-1", "revision": 1, "source": "you said it, 14:02"}));
        assert_eq!(memory_line(&w).as_deref(), Some("memory: remembered rm-1 rev 1 (you said it, 14:02)"));

        memory_wire(&json!({"kind": "memory_act", "state": "proposed", "act": {"op": {"op": "correct", "text": "x"}, "proposal_id": "p-2"}}), &mut ops, src);
        let w = memory_wire(&json!({"kind": "memory_act", "state": "committed", "act": {"outcome": {"memory_id": "rm-1", "revision": 2}, "proposal_id": "p-2"}}), &mut ops, src).unwrap();
        assert_eq!(memory_line(&w).as_deref(), Some("memory: updated rm-1 rev 2 (you said it, 14:02)"));

        let w = memory_wire(&json!({"kind": "memory_decided", "decision": "none", "receipt": {"blocked": "secret.code_or_pin"}}), &mut ops, src).unwrap();
        assert_eq!((w["blocked"].clone(), memory_line(&w)), (json!("secret.code_or_pin"), Some("memory: not saved: secret.code_or_pin".into())));
        assert_eq!(memory_line(&memory_wire(&json!({"kind": "memory_decided", "decision": "no_op", "receipt": {}}), &mut ops, src).unwrap()), None);

        let held = json!({"kind": "memory_recall", "count": 0, "total": 0, "recall": {"for": "command:c", "items": [], "status": "held"}});
        assert_eq!(memory_line(&memory_wire(&held, &mut ops, src).unwrap()), None);
        let r = memory_wire(&json!({"kind": "memory_recall", "count": 1, "total": 1, "recall": {"for": "command:c", "items": [{"memory_id": "rm-1", "revision": 1}], "status": "enqueued"}}), &mut ops, src).unwrap();
        assert_eq!((r["count"].clone(), r["status"].clone(), r.get("source")), (json!(1), json!("enqueued"), None), "a recall names no source");
        assert_eq!(memory_line(&r).as_deref(), Some("memory: recalled 1 (enqueued)"));
        assert_eq!(memory_wire(&json!({"kind": "memory_recall", "count": 1, "recall": {"for": "session_start", "status": "sent"}}), &mut ops, src), None);
        assert_eq!(memory_wire(&json!({"kind": "agent_delta", "text": "x"}), &mut ops, src), None);
    }

    #[test]
    fn the_local_hello_binds_to_the_model_and_the_pid() {
        let ok = br#"{"v":2,"kind":"hello","source":"guest","backend":"local@granite41-3b-q4km","boot_id":"b-1-77"}"#;
        assert_eq!(crate::setup::hello_boot_for(ok, 77, "local@granite41-3b-q4km"), Ok("b-1-77".into()));
        assert!(crate::setup::hello_boot_for(ok, 77, "local@qwen35-4b-q4km").is_err());
        assert!(crate::setup::hello_boot_for(ok, 78, "local@granite41-3b-q4km").is_err());
    }
}

#[cfg(test)]
mod duplex_tests {
    use super::*;
    #[test]
    fn thinking_cancel_waits_for_response_id_and_late_frames_stay_fenced() {
        let mut f = ReplyFence::default();
        assert_eq!(f.request(), None);
        assert!(matches!(f.frame(&json!({"kind":"response_started","response_id":"old"})), Fenced::Cancel(id) if id == "old"));
        assert!(matches!(f.frame(&json!({"kind":"agent_delta","response_id":"old","text":"late"})), Fenced::Hide));
        assert!(matches!(f.frame(&json!({"kind":"agent_done","response_id":"old","status":"cancelled"})), Fenced::End(v) if v["interrupted"] == true));
        assert!(matches!(f.frame(&json!({"kind":"response_started","response_id":"new"})), Fenced::Pass));
        for kind in ["agent_delta", "agent_replace", "agent_done", "reply_spoken", "response_diagnostic"] {
            assert!(matches!(f.frame(&json!({"kind":kind,"response_id":"old"})), Fenced::Hide));
        }
        assert!(matches!(f.frame(&json!({"kind":"agent_delta","response_id":"new","text":"new"})), Fenced::Pass));
    }
}

#[cfg(test)]
mod slice1_tests {
    use super::*;
    #[test]
    fn slice1_opener_reaches_page_with_response_id() {
        assert_eq!(reply_for_page(&json!({"kind":"agent_opener","response_id":"r1","text":"Let me think."}), &mut None), vec![json!({"opener":"Let me think.","response_id":"r1"})]);
    }
    #[test]
    fn slice1_four_slots_preserve_context_and_export_opener() {
        let cat = json!({"agentd_slots":{"RTH_LOCAL_SLOT":"0","RTH_LOCAL_DECIDE_SLOT":"3","RTH_LOCAL_CHECK_SLOT":"1","RTH_LOCAL_OPENER_SLOT":"2"}});
        let args = ["-c","32768","-np","4"].map(str::to_string);
        assert_eq!(slot_ctx(&args), Some(8192));
        assert!(slot_env(&cat,&args).unwrap().contains(&("RTH_LOCAL_OPENER_SLOT".into(),"2".into())));
    }
    #[test]
    fn slice1_opener_is_fenced_during_and_after_cancellation() {
        let mut fence=ReplyFence::default();
        assert!(matches!(fence.frame(&json!({"kind":"response_started","response_id":"old"})),Fenced::Pass));
        fence.request();
        let f=json!({"kind":"agent_opener","response_id":"old","text":"Let me think."});
        assert!(matches!(fence.frame(&f),Fenced::Hide));
        fence.frame(&json!({"kind":"agent_done","response_id":"old"}));
        assert!(matches!(fence.frame(&f),Fenced::Hide));
    }

}

#[cfg(test)]
mod h1_regressions {
    use super::*;
    #[test]
    fn h1_reclaims_only_an_unused_last_floor_slot_and_its_kv_context() {
        let mut cat = json!({"server_args":["--jinja","-c","40960","-np","5"],
            "floor_slot":4, "agentd_slots":{"RTH_LOCAL_SLOT":"0","RTH_LOCAL_DECIDE_SLOT":"3","RTH_LOCAL_CHECK_SLOT":"1","RTH_LOCAL_OPENER_SLOT":"2"}});
        let args = server_args(&cat, &json!({"id":"fixture"}), 1, Path::new("/fixture.gguf"));
        assert_eq!(np_of(&args),4);
        assert_eq!(ctx_of(&args),Some(32768));
        assert_eq!(slot_ctx(&args),Some(8192));
        assert_eq!(slot_env(&cat,&args).unwrap().len(),4);
        cat["agentd_slots"]["RTH_LOCAL_SLOT"] = json!("4");
        let args = server_args(&cat, &json!({"id":"fixture"}), 1, Path::new("/fixture.gguf"));
        assert_eq!(np_of(&args),5,"a used slot is never reclaimed");
        assert_eq!(ctx_of(&args),Some(40960));
    }
    #[test]
    fn front_legacy_sends_once_on_the_existing_socket_and_returns_the_backend_response() {
        use crate::voice_front::runtime::Legacy;
        use std::sync::{Arc, Mutex, atomic::AtomicBool};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let conn = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (peer, _) = listener.accept().unwrap(); peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let link = Arc::new(crate::voice_front::link::Link::new("session", front_observe_frame));
        link.on_frame(&json!({"kind":"observe_ready","schema":crate::conversation_log::observe::SCHEMA,"session":"session"}));
        let session = Arc::new(Mutex::new(Session { id:"session".into(),generation:4,boot:"test".into(),n:0,
            last:Some(("old-frame".into(),"old input".into(),false,"old".into(),None)),undos:vec![] }));
        let awaiting = Arc::new(AtomicBool::new(false)); let writer = Arc::new(Mutex::new(conn));
        let mut transport = FrontLegacy { #[cfg(feature = "voice_pane")] order:None, link:link.clone(), conn:writer.clone(), session:session.clone(),
            awaiting:awaiting.clone(),alive:Arc::new(AtomicBool::new(true)), input:"public input".into(),
            wire:"[explicit interruption observation]\n\npublic input".into(), attempted:false, reply_voice:Some(json!({"dictated":true,"spoken":"full"})) };
        let server = std::thread::spawn(move || {
            let mut reader = BufReader::new(peer); let mut line = String::new();reader.read_line(&mut line).unwrap();
            let f:Value = serde_json::from_str(&line).unwrap();
            assert_eq!(f["kind"],"command_submitted");assert_eq!(f["session_id"],"session");
            assert_eq!(f["expected_generation"],4);assert_eq!(f["text"],"[explicit interruption observation]\n\npublic input");
            assert_eq!(f["reply_voice"],json!({"dictated":true,"spoken":"full"}));
            assert!(awaiting.load(std::sync::atomic::Ordering::SeqCst));
            link.on_frame(&json!({"kind":"response_started","request_id":f["command_id"],"response_id":"actual-backend-response"}));
            f["command_id"].as_str().unwrap().to_string()
        });
        let turn = crate::conversation_log::Turn { id:"turn".into(), response_id:"local-response".into(),
            observation_id:"observation".into(),nonce:"nonce".into(),input:"public input".into(),
            owner:Some(crate::conversation_log::Owner::Legacy),rendered:false,presentation_uncertain:false,
            legacy_attempted:true,backend_response:None,acknowledged:false,presentation:None,silent_replay:false };
        let mut foreign=turn.clone();foreign.input="foreign input".into();
        assert!(transport.send(&foreign).is_err());
        assert_eq!(session.lock().unwrap().n,0,"fresh foreign input must not allocate a command");
        assert!(!transport.attempted,"foreign input must not consume the adapter");
        assert_eq!(transport.send(&turn).unwrap(),"actual-backend-response");
        assert_ne!(server.join().unwrap(),"actual-backend-response");
        assert!(session.lock().unwrap().last.is_none(),"one-shot sends cannot enter stale-generation resubmission");
        assert_eq!(session.lock().unwrap().n,1);
        assert_eq!(writer.lock().unwrap().write_timeout().unwrap(),None);
        assert!(transport.send(&turn).is_err()); assert_eq!(session.lock().unwrap().n,1,"no repeated command on one adapter");
        let mut foreign = turn.clone();foreign.input="foreign input".into();assert!(transport.send(&foreign).is_err());
        assert_eq!(session.lock().unwrap().n,1,"foreign input cannot write a command");
    }

    #[test] fn front_factories_refuse_with_the_setting_off_on_a_ready_live_attachment() {
        use std::sync::{Arc,Mutex,atomic::AtomicBool};
        assert!(!crate::settings::current().voice_front_templates);
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let conn=TcpStream::connect(listener.local_addr().unwrap()).unwrap();let (mut peer,_)=listener.accept().unwrap();
        let observe=Arc::new(crate::voice_front::link::Link::new("session",front_observe_frame));
        observe.on_frame(&json!({"kind":"observe_ready","schema":crate::conversation_log::observe::SCHEMA,"session":"session"}));
        let chat=Chat {
            #[cfg(feature = "voice_pane")] front:Default::default(),observe,conn:Arc::new(Mutex::new(conn)),alive:Arc::new(AtomicBool::new(true)),
            session:Some(Arc::new(Mutex::new(Session {id:"session".into(),generation:1,boot:"test".into(),n:0,last:None,undos:vec![]}))),
            #[cfg(feature="menu_bar")] bind:listener.local_addr().unwrap(),
            awaiting:Arc::new(AtomicBool::new(false)),fence:Arc::new(Mutex::new(ReplyFence::default()))};
        assert!(chat.alive() && chat.observe.ready());
        assert!(chat.front_legacy("public input","public input",None).is_err());assert!(chat.front_observe().is_err());
        peer.set_nonblocking(true).unwrap();let mut byte=[0];
        assert_eq!(std::io::Read::read(&mut peer,&mut byte).unwrap_err().kind(),std::io::ErrorKind::WouldBlock);
    }

    #[test] fn front_session_and_writer_contention_happen_before_the_durable_fence() {
        use crate::voice_front::{runtime::Legacy,Error};
        use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
        for session_held in [true,false] {
            let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let conn=Arc::new(Mutex::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap()));
            let (peer,_)=listener.accept().unwrap();peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let link=Arc::new(crate::voice_front::link::Link::new("session",front_observe_frame));
            link.on_frame(&json!({"kind":"observe_ready","schema":crate::conversation_log::observe::SCHEMA,"session":"session"}));
            let session=Arc::new(Mutex::new(Session{id:"session".into(),generation:1,boot:"test".into(),n:0,last:None,undos:vec![]}));
            let transport=FrontLegacy {#[cfg(feature = "voice_pane")] order:None,link:link.clone(),conn:conn.clone(),session:session.clone(),awaiting:Arc::new(AtomicBool::new(false)),
                alive:Arc::new(AtomicBool::new(true)),input:"public input".into(),wire:"public input".into(),reply_voice:None,attempted:false};
            let turn=crate::conversation_log::Turn {id:"turn".into(),response_id:"local-response".into(),observation_id:"observation".into(),
                nonce:"nonce".into(),input:"public input".into(),owner:Some(crate::conversation_log::Owner::Legacy),rendered:false,
                presentation_uncertain:false,legacy_attempted:false,backend_response:None,acknowledged:false,presentation:None,silent_replay:false};
            let fences=Arc::new(AtomicUsize::new(0));let calls=fences.clone();let original=turn.clone();
            let session_hold=session_held.then(||session.lock().unwrap());let conn_hold=(!session_held).then(||conn.lock().unwrap());
            let (tx,rx)=std::sync::mpsc::sync_channel(1);
            let thread=std::thread::spawn(move || {let mut t=transport;let result=t.send_admitted(&original,&mut||{
                calls.fetch_add(1,Ordering::SeqCst);let mut turn=original.clone();turn.legacy_attempted=true;Ok(turn)
            });let _=tx.send((t,result));});
            let received=rx.recv_timeout(Duration::from_secs(1));

            drop(session_hold);drop(conn_hold);
            let (mut transport,result)=received.expect("bounded pre-fence lock admission");thread.join().unwrap();
            assert!(matches!(result,Err(Error::Busy)));assert_eq!(fences.load(Ordering::SeqCst),0,"an unsent collision cannot cross the durable fence");
            assert!(!transport.attempted);assert_eq!(session.lock().unwrap().n,0);
            let ready=session.clone();let writer=conn.clone();let server=std::thread::spawn(move||{
                let mut reader=BufReader::new(peer);let mut line=String::new();reader.read_line(&mut line).unwrap();let f:Value=serde_json::from_str(&line).unwrap();

                let at=std::time::Instant::now();while ready.try_lock().is_err() || writer.try_lock().is_err(){assert!(at.elapsed()<Duration::from_secs(1));std::thread::sleep(Duration::from_millis(2));}
                link.on_frame(&json!({"kind":"response_started","request_id":f["command_id"],"response_id":"actual-response"}));
            });
            assert_eq!(transport.send_admitted(&turn,&mut||{fences.fetch_add(1,Ordering::SeqCst);let mut t=turn.clone();t.legacy_attempted=true;Ok(t)}).unwrap(),"actual-response");
            server.join().unwrap();assert_eq!(fences.load(Ordering::SeqCst),1);assert_eq!(session.lock().unwrap().n,1);
        }
    }

    #[test] fn torn_front_frame_closes_the_attachment_before_any_next_frame() {
        use std::{cell::Cell,io::{self,Write},sync::atomic::{AtomicBool,Ordering}};
        struct Torn {bytes:Vec<u8>,remaining:usize,closed:Cell<usize>,wait:Cell<Option<Duration>>,restore_fails:bool}
        impl Write for Torn {
            fn write(&mut self,bytes:&[u8])->io::Result<usize>{
                if self.remaining==0{return Err(io::Error::new(io::ErrorKind::TimedOut,"injected partial frame"));}
                let n=bytes.len().min(self.remaining);self.bytes.extend_from_slice(&bytes[..n]);self.remaining-=n;Ok(n)
            }
            fn flush(&mut self)->io::Result<()>{Ok(())}
        }
        impl FrontWriter for Torn {
            fn write_wait(&self)->io::Result<Option<Duration>>{Ok(self.wait.get())}
            fn set_write_wait(&self,wait:Option<Duration>)->io::Result<()>{
                if self.restore_fails && wait.is_none(){return Err(io::Error::other("injected restore failure"));}self.wait.set(wait);Ok(())
            }
            fn shutdown_attachment(&self){self.closed.set(self.closed.get()+1);}
        }
        for restore_fails in [false,true] {
            let alive=AtomicBool::new(true);let mut stream=Torn {bytes:vec![],remaining:if restore_fails{usize::MAX}else{3},closed:Cell::new(0),wait:Cell::new(None),restore_fails};
            assert!(front_write_locked(&mut stream,&json!({"kind":"observe_request"}),&alive).is_err());
            assert!(!alive.load(Ordering::SeqCst));assert_eq!(stream.closed.get(),1);
            assert!(!stream.bytes.is_empty());if !restore_fails{assert_eq!(stream.bytes.len(),3);}
            let bytes=stream.bytes.clone();stream.remaining=usize::MAX;stream.restore_fails=false;
            assert!(front_write_locked(&mut stream,&json!({"kind":"command_submitted"}),&alive).is_err());assert_eq!(stream.bytes,bytes,"never glue a next frame to the torn bytes");
        }
    }

}

#[cfg(all(test,feature="voice_pane"))]
mod native_hook_review {
    use super::*;
    use crate::voice_front::{policy::{Locale,LocaleSource,Selection},runtime::{Epoch,Legacy,Outcome,Speech,SpeechRequest},worker::{Input,Update,Worker},Error};
    use std::sync::{Arc,Mutex,mpsc,atomic::AtomicBool};
    struct Mirror;
    impl crate::conversation_log::mirror::Transport for Mirror {
        fn resume(&mut self,s:&str)->std::io::Result<crate::conversation_log::observe::Snapshot>{Ok(crate::conversation_log::observe::Snapshot{session:s.into(),version:0,tip:String::new()})}
        fn observe(&mut self,o:&crate::conversation_log::observe::Observation)->std::io::Result<crate::conversation_log::observe::Receipt>{Ok(o.receipt())}
    }
    struct Silent;
    impl Speech for Silent {fn start(&mut self,_:SpeechRequest)->crate::voice_front::Result<()>{Ok(())}fn stop(&mut self,_:&crate::voice_front::runtime::Ticket){}}
    struct RetryGate {inner:FrontLegacy,first:bool,entered:mpsc::SyncSender<()>,release:Option<mpsc::Receiver<()>>}
    impl Legacy for RetryGate {
        fn send(&mut self,t:&crate::conversation_log::Turn)->crate::voice_front::Result<String>{self.inner.send(t)}
        fn send_admitted(&mut self,t:&crate::conversation_log::Turn,f:&mut dyn FnMut()->crate::voice_front::Result<crate::conversation_log::Turn>)->crate::voice_front::Result<String>{
            if !self.first {if let Some(held)=self.release.take(){let _=self.entered.send(());held.recv_timeout(Duration::from_secs(3)).map_err(|_|Error::Deadline)?;}}
            self.first=false;self.inner.send_admitted(t,f)
        }
    }
    fn event(rx:&mpsc::Receiver<Update>,wanted:impl Fn(&Update)->bool){let end=std::time::Instant::now()+Duration::from_secs(4);loop{let e=rx.recv_timeout(end.saturating_duration_since(std::time::Instant::now())).expect("bounded native worker event");if wanted(&e){return;}}}
    #[test]
    fn newer_typed_send_supersedes_a_not_sent_voice_retry_before_its_later_admission() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();let socket=TcpStream::connect(listener.local_addr().unwrap()).unwrap();let (peer,_)=listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let link=Arc::new(crate::voice_front::link::Link::new("session",front_observe_frame));
        link.on_frame(&json!({"kind":"observe_ready","schema":crate::conversation_log::observe::SCHEMA,"session":"session"}));
        let session=Arc::new(Mutex::new(Session{id:"session".into(),generation:1,boot:"test".into(),n:0,last:None,undos:vec![]}));
        let mut chat=Chat{front:Default::default(),observe:link.clone(),conn:Arc::new(Mutex::new(socket)),alive:Arc::new(AtomicBool::new(true)),session:Some(session.clone()),
            #[cfg(feature="menu_bar")] bind:listener.local_addr().unwrap(),awaiting:Arc::new(AtomicBool::new(false)),fence:Default::default()};
        chat.front.bind("session");let capture=chat.front.capture();
        let root=std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string())).join(format!("typed-wins-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();let saved=root.clone();let (tx,rx)=mpsc::channel();
        let h=Worker::start("session".into(),capture.clone(),move||crate::conversation_log::Store::for_front(&root,"session"),Silent,Mirror,move|u|{let _=tx.send(u);}).unwrap();event(&rx,|u|matches!(u,Update::Status("Ready")));
        let inner=FrontLegacy{order:Some(chat.front.order.clone()),link:link.clone(),conn:chat.conn.clone(),session:session.clone(),awaiting:chat.awaiting.clone(),alive:chat.alive.clone(),
            input:"Older voice input.".into(),wire:"Older voice input.".into(),reply_voice:Some(json!({"dictated":true,"spoken":"full"})),attempted:false};
        let (at,entered)=mpsc::sync_channel(1);let (release,held)=mpsc::sync_channel(1);
        let busy=session.lock().unwrap();
        assert!(h.submit(true,"session",Input{epoch:Epoch{capture:1,turn:1},revision:0,nonce:"older".into(),original:"Older voice input.".into(),started:std::time::Instant::now(),
            selection:Selection{locale:Locale::En,source:LocaleSource::Setting,confidence:None},backend:Box::new(RetryGate{inner,first:true,entered:at,release:Some(held)})}).is_ok());
        event(&rx,|u|matches!(u,Update::Outcome(_,_,Outcome::NotSent)));entered.recv_timeout(Duration::from_secs(2)).unwrap();drop(busy);
        let server=std::thread::spawn(move||{let mut reader=BufReader::new(peer);let mut texts=vec![];loop{let mut row=String::new();if reader.read_line(&mut row).unwrap_or(0)==0{break;}let frame:Value=serde_json::from_str(&row).unwrap();texts.push(frame["text"].as_str().unwrap_or("").to_string());
            link.on_frame(&json!({"kind":"response_started","request_id":frame["command_id"],"response_id":"accepted"}));}texts});
        let typed=chat.say_session("Newer typed input.",Some(&json!({"dictated":false,"spoken":"full"})));
        let revision=capture.revision();let _=release.send(());
        let outcome=loop{let u=rx.recv_timeout(Duration::from_secs(3)).unwrap();if let Update::Outcome(_,_,o)=u{break o;}};
        h.close();event(&rx,|u|matches!(u,Update::Status("Closed")));let texts=server.join().unwrap();
        assert!(typed.is_ok());assert_ne!(revision,0,"ordinary typed send must invalidate the older voice retry");
        assert_eq!(outcome,Outcome::Cancelled,"a newer typed send supersedes an older retry");assert_eq!(texts,vec!["Newer typed input."]);
        let store=crate::conversation_log::Store::open_persisted(&saved,"session").unwrap();assert!(store.turns()[0].silent_replay);assert!(!store.turns()[0].legacy_attempted);
    }
}

#[cfg(all(test,feature="voice_pane"))]
#[path="front_pre_enable_tests.rs"]
mod pre_enable_tests;
