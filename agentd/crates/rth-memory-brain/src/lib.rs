

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use rth_memory::contract::{Dropped, Failure, Found, Origin, PERSON_KIND, Provenance, Provider, Recalled, Saved};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const INDEX_SCHEMA: &str = "rth-memory-binding/v1";

pub const MAX_TEXT_BYTES: usize = 1000;

pub const ALLOWED: &[&str] = &["brain_remember", "brain_update", "brain_get", "brain_memory_history", "brain_invalidate", "brain_memory_restore"];

pub const NO_NETWORK_PROFILE: &str = "(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote unix-socket))(allow network-outbound (remote ip \"localhost:*\"))";

pub const RECALL_SCAN_MAX: usize = 64;

#[derive(Debug, Clone)]
pub struct Binding {
    pub instance_id: String,
    pub brain_id: String,
    pub binding_rev: u64,

    pub mcp_bin: PathBuf,

    pub principal: String,

    pub runtime_dir: Option<String>,

    pub index: PathBuf,

    pub read_timeout: Duration,

    pub write_timeout: Duration,

    pub no_network: bool,

    pub record: PathBuf,

    pub home: Option<PathBuf>,
    pub database: Option<String>,

    pub seam: Option<SeamConfig>,
}

#[derive(Debug, Clone)]
pub struct SeamConfig {
    pub bun: PathBuf,
    pub script: PathBuf,
    pub brain_src: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingRecord {
    pub schema: String,
    pub instance_id: String,
    pub brain_id: String,
    pub binding_rev: u64,
    pub sentinel_ref: String,
    pub nonce_sha256: String,
    pub sentinel_created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IndexLine {
    schema: String,
    instance_id: String,
    brain_id: String,
    binding_rev: u64,
    memory_id: String,
    brain_ref: String,
    revision: u64,
    text_sha256: String,
    operation_key: String,

    #[serde(default)]
    op_digest: String,
    utc_ms: u64,
}

fn op_digest(kind: &str, target: Option<&str>, expected_revision: Option<u64>, text: &str) -> String {
    sha(&json!({ "kind": kind, "target": target, "expected_revision": expected_revision, "text": text }).to_string())
}

pub const HISTORY_PAGES_MAX: u64 = 100;

#[derive(Default)]
struct Index {

    current: HashMap<String, IndexLine>,

    by_ref: HashMap<String, String>,

    by_key: HashMap<String, IndexLine>,
    items: u64,
}

fn kind_of(g: &Value) -> Option<String> {
    g["metadata"]["rth_kind"].as_str().map(str::to_string)
}

fn speaker_of(g: &Value) -> Option<String> {
    g["metadata"]["rth_speaker"].as_str().map(str::to_string)
}

fn is_person(g: &Value) -> bool {
    g["metadata"]["rth_kind"].as_str() == Some(PERSON_KIND)
}

fn sha(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn local_date(ms: u64) -> String {
    let secs = (ms / 1000) as libc::time_t;

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return utc_date(ms);
    }
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

pub fn utc_offset_min(ms: u64) -> i32 {
    let secs = (ms / 1000) as libc::time_t;

    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return 0;
    }
    (tm.tm_gmtoff / 60) as i32
}

pub fn utc_date(ms: u64) -> String {
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn bad(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

fn open_index(path: &Path, b: &Binding) -> std::io::Result<(std::fs::File, Index)> {
    let existed = match std::fs::metadata(path) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
    if !existed {
        file.sync_all()?;
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        std::fs::File::open(dir)?.sync_all()?;
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut bytes)?;
    let keep = bytes.iter().rposition(|&c| c == b'\n').map(|i| i + 1).unwrap_or(0);
    if keep < bytes.len() {
        file.set_len(keep as u64)?;
        file.sync_all()?;
        bytes.truncate(keep);
    }
    let text = String::from_utf8(bytes).map_err(|e| bad(format!("{}: {e}", path.display())))?;
    let mut idx = Index::default();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let l: IndexLine = serde_json::from_str(line).map_err(|e| bad(format!("{}: line {}: {e}", path.display(), n + 1)))?;
        if l.instance_id != b.instance_id || l.brain_id != b.brain_id {
            return Err(bad(format!("{}: line {}: bound to {}/{}, not {}/{}", path.display(), n + 1, l.instance_id, l.brain_id, b.instance_id, b.brain_id)));
        }
        idx.fold(l);
    }
    Ok((file, idx))
}

impl Index {
    fn fold(&mut self, l: IndexLine) {
        if !self.current.contains_key(&l.memory_id) {
            self.items += 1;
        }
        self.by_ref.insert(l.brain_ref.clone(), l.memory_id.clone());
        if !l.op_digest.is_empty() {
            self.by_key.insert(l.operation_key.clone(), l.clone());
        }

        if self.current.get(&l.memory_id).is_none_or(|c| l.revision >= c.revision) {
            self.current.insert(l.memory_id.clone(), l);
        }
    }

    fn same_text(&self, digest: &str, except: Option<&str>) -> Option<String> {
        self.current.values().find(|l| l.text_sha256 == digest && Some(l.memory_id.as_str()) != except).map(|l| l.memory_id.clone())
    }
}

fn spawn_pinned(b: &Binding, program: &Path, args: &[&str], extra: &[(&str, String)]) -> Result<(Child, ChildStdin, Receiver<String>), String> {
    let mut cmd = if b.no_network {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(NO_NETWORK_PROFILE).arg(program);
        c
    } else {
        Command::new(program)
    };
    cmd.args(args);
    let home = match (&b.home, &b.database) {
        (Some(h), Some(db)) => {

            std::fs::create_dir_all(h.join(".brain")).map_err(|e| e.to_string())?;
            let rec = json!({ "brainId": b.brain_id, "brainSlug": "rth", "brainPath": "", "database": db, "endpoint": "", "pid": std::process::id() });
            std::fs::write(h.join(".brain/config.json"), rec.to_string()).map_err(|e| e.to_string())?;
            h.display().to_string()
        }
        _ => std::env::var("HOME").unwrap_or_default(),
    };
    cmd.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("BRAIN_ID", &b.brain_id)
        .env("BRAIN_PRINCIPAL", &b.principal)
        .env("BRAIN_MEMORY_VECTORS", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(dir) = &b.runtime_dir {
        cmd.env("XDG_RUNTIME_DIR", dir);
    }
    if let (Some(_), Some(db)) = (&b.home, &b.database) {
        cmd.env("BRAIN_CONTROL_DATABASE", db);
    }
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{} did not start: {e}", program.display()))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Ok((child, stdin, rx))
}

pub type Children = std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<u32>>>;

pub fn kill_children(children: &Children) {
    for pid in children.lock().unwrap_or_else(|e| e.into_inner()).iter() {

        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGKILL) };
    }
}

struct Recorded(Children, u32);

impl Recorded {
    fn new(children: &Children, child: &Child) -> Recorded {
        children.lock().unwrap_or_else(|e| e.into_inner()).insert(child.id());
        Recorded(children.clone(), child.id())
    }
    fn reaped(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.1);
    }
}

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next: u64,
    recorded: Recorded,
}

impl Mcp {
    fn spawn(b: &Binding, children: &Children) -> Result<Mcp, String> {
        let (child, stdin, rx) = spawn_pinned(b, &b.mcp_bin, &[], &[])?;
        let recorded = Recorded::new(children, &child);
        let mut m = Mcp { child, stdin, lines: rx, next: 1, recorded };
        m.request("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "rth-memory-brain", "version": "1" } }), b.read_timeout.max(Duration::from_secs(10)))
            .map_err(|e| format!("brain-mcp initialize: {e:?}"))?;
        m.notify("notifications/initialized")?;
        Ok(m)
    }

    fn notify(&mut self, method: &str) -> Result<(), String> {
        writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "method": method })).map_err(|e| e.to_string())
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, bool> {
        let id = self.next;
        self.next += 1;
        if writeln!(self.stdin, "{}", json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).and_then(|()| self.stdin.flush()).is_err() {
            return Err(false);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                    if v.get("id").and_then(Value::as_u64) == Some(id) {
                        return Ok(v);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(true),
                Err(RecvTimeoutError::Disconnected) => return Err(false),
            }
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.recorded.reaped();
        let _ = self.child.wait();
    }
}

struct Seam {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next: u64,
    recorded: Recorded,
}

impl Seam {
    fn spawn(b: &Binding, c: &SeamConfig, children: &Children) -> Result<Seam, String> {
        let script = c.script.display().to_string();
        let (child, stdin, lines) = spawn_pinned(b, &c.bun, &[&script], &[("BRAIN_SRC", c.brain_src.display().to_string())])?;
        let recorded = Recorded::new(children, &child);
        let s = Seam { child, stdin, lines, next: 1, recorded };
        match s.lines.recv_timeout(Duration::from_secs(20)) {
            Ok(l) if l.contains("\"ready\":true") => Ok(s),
            Ok(l) => Err(format!("memory seam: {}", l.chars().take(200).collect::<String>())),
            Err(_) => Err("memory seam did not start".into()),
        }
    }

    fn search(&mut self, query: &str, limit: usize, timeout: Duration) -> Result<Vec<String>, String> {
        let id = self.next;
        self.next += 1;
        writeln!(self.stdin, "{}", json!({ "id": id, "query": query, "limit": limit })).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.lines.recv_timeout(left).map_err(|_| "memory seam: no answer within the deadline".to_string())?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["id"].as_u64() != Some(id) {
                continue;
            }
            if v["ok"].as_bool() != Some(true) {
                return Err(format!("memory seam: {}", v["error"]));
            }
            return Ok(v["hits"].as_array().map(|a| a.iter().filter_map(|h| h["brain_ref"].as_str().map(str::to_string)).collect()).unwrap_or_default());
        }
    }
}

impl Drop for Seam {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.recorded.reaped();
        let _ = self.child.wait();
    }
}

pub const SEAM_SEARCH_MAX: usize = 9;

pub fn seam_limit(limit: usize) -> usize {
    (limit * 3).clamp(1, SEAM_SEARCH_MAX)
}

enum Answer {
    Ok(Value),

    Refused(String, Value),
}

pub struct Brain {
    binding: Binding,

    children: Children,
    mcp: Option<Mcp>,
    seam: Option<Seam>,
    index_file: std::fs::File,
    index: Index,

    attested: Result<(), String>,

    closed: bool,
}

impl Brain {

    pub fn open(binding: Binding) -> std::io::Result<Brain> {
        let (index_file, index) = open_index(&binding.index, &binding)?;
        let children = Children::default();
        let mcp = Mcp::spawn(&binding, &children).ok();
        let mut b = Brain { binding, children, mcp, seam: None, index_file, index, attested: Err("not checked".into()), closed: false };
        b.attested = b.attest();
        Ok(b)
    }

    pub fn attestation(&self) -> Result<(), String> {
        self.attested.clone()
    }

    #[doc(hidden)]
    pub fn drop_transport(&mut self) {
        self.lose_transport("test");
    }

    fn lose_transport(&mut self, why: &str) {
        self.mcp = None;
        self.seam = None;
        self.attested = Err(format!("reattest_required: {why}"));
    }

    fn attest(&mut self) -> Result<(), String> {
        let text = std::fs::read_to_string(&self.binding.record).map_err(|e| format!("attached_unattested: no binding record ({e})"))?;
        let r: BindingRecord = serde_json::from_str(&text).map_err(|e| format!("attached_unattested: binding record unreadable ({e})"))?;
        if r.instance_id != self.binding.instance_id || r.brain_id != self.binding.brain_id || r.binding_rev != self.binding.binding_rev {
            return Err(format!("binding_mismatch: record is {}/{} rev {}", r.instance_id, r.brain_id, r.binding_rev));
        }
        let g = self.get_ref(&r.sentinel_ref).map_err(|f| format!("attached_unattested: sentinel unreadable ({f:?})"))?;
        let ok = g["metadata"]["rth_binding_nonce_sha256"].as_str() == Some(r.nonce_sha256.as_str())
            && g["metadata"]["rth_instance_id"].as_str() == Some(r.instance_id.as_str())
            && g["created_at"].as_str() == Some(r.sentinel_created_at.as_str());
        if ok { Ok(()) } else { Err("binding_mismatch: the sentinel in this brain is not the one born with the binding".into()) }
    }

    pub fn birth(binding: &Binding, nonce: &str) -> Result<BindingRecord, Failure> {
        if binding.record.exists() {
            return Err(Failure::rejected("binding_record_exists"));
        }
        let (index_file, index) = open_index(&binding.index, binding).map_err(|e| Failure::Unavailable { detail: e.to_string() })?;
        let mut b = Brain { binding: binding.clone(), children: Children::default(), mcp: None, seam: None, index_file, index, attested: Err("birth".into()), closed: false };
        let nonce_sha256 = sha(nonce);
        let args = json!({
            "content": format!("RTH memory binding sentinel for instance {} (binding rev {}).", binding.instance_id, binding.binding_rev),
            "operation_key": format!("birth/{}/{}", binding.instance_id, &nonce_sha256[..16]),
            "scope": "global",
            "source": format!("rth:{}", binding.instance_id),
            "veracity": "tool",
            "importance": 0.0,
            "actor": format!("rth-host:{}", binding.instance_id),
            "reason": "binding birth",
            "extract": false,
            "extract_entities": false,
            "metadata": { "rth_binding_sentinel": true, "rth_instance_id": binding.instance_id, "rth_binding_rev": binding.binding_rev, "rth_binding_nonce_sha256": nonce_sha256 },
        });
        let v = match b.call("brain_remember", args, true)? {
            Answer::Ok(v) => v,

            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("birth brain_remember {code}: {v}") }),
        };
        let sentinel_ref = v["memory_id"].as_str().map(str::to_string).ok_or(Failure::Unknown { detail: "birth: no memory_id".into() })?;
        let g = b.get_ref(&sentinel_ref)?;
        let rec = BindingRecord {
            schema: "rth-memory-binding-record/v1".into(),
            instance_id: binding.instance_id.clone(),
            brain_id: binding.brain_id.clone(),
            binding_rev: binding.binding_rev,
            sentinel_ref,
            nonce_sha256,
            sentinel_created_at: g["created_at"].as_str().unwrap_or_default().to_string(),
        };

        let tmp = binding.record.with_extension(format!("tmp-{}-{}", std::process::id(), &rec.nonce_sha256[..12]));
        let body = serde_json::to_string_pretty(&rec).map_err(|e| Failure::Unknown { detail: e.to_string() })?;
        let write = || -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            f.write_all(body.as_bytes())?;
            f.sync_all()?;
            let linked = std::fs::hard_link(&tmp, &binding.record);
            let _ = std::fs::remove_file(&tmp);
            linked?;
            let dir = binding.record.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()
        };
        write().map_err(|e| Failure::Unknown { detail: format!("binding record: {e}") })?;
        Ok(rec)
    }

    fn writable(&mut self) -> Result<(), Failure> {
        if self.attested.is_err() {
            self.attested = self.attest();
        }
        self.attested.clone().map_err(|detail| Failure::Unavailable { detail })
    }

    fn ours(&self, g: &Value, memory_id: &str) -> bool {
        g["metadata"]["rth_memory_id"].as_str() == Some(memory_id)
            && g["metadata"]["rth_instance_id"].as_str() == Some(self.binding.instance_id.as_str())
            && g["metadata"]["rth_binding_rev"].as_u64() == Some(self.binding.binding_rev)
            && g["invalidated_at"].is_null()
    }

    fn ours_history(&self, r: &Value, memory_id: &str) -> bool {
        r["metadata"]["rth_memory_id"].as_str() == Some(memory_id)
            && r["metadata"]["rth_instance_id"].as_str() == Some(self.binding.instance_id.as_str())
            && r["metadata"]["rth_binding_rev"].as_u64() == Some(self.binding.binding_rev)
    }

    fn revision_by_key(&mut self, brain_ref: &str, key: &str) -> Result<Option<Value>, Failure> {
        for page in 0..HISTORY_PAGES_MAX {
            let h = match self.call("brain_memory_history", json!({ "memory_id": brain_ref, "limit": 100, "offset": page * 100 }), false)? {
                Answer::Ok(h) => h,
                Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("replay check: brain_memory_history {code}: {v}") }),
            };
            let rows = h["revisions"].as_array().cloned().unwrap_or_default();
            if let Some(r) = rows.iter().find(|r| r["operation_key"].as_str() == Some(key)) {
                return Ok(Some(r.clone()));
            }
            if rows.len() < 100 {
                return Ok(None);
            }
        }
        Err(Failure::Unknown { detail: format!("replay check: {brain_ref} has more than {} history pages", HISTORY_PAGES_MAX) })
    }

    fn reconcile_head(&mut self, brain_ref: &str, memory_id: &str) -> Result<(), Failure> {
        let g = self.get_ref(brain_ref)?;
        if !self.ours(&g, memory_id) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {brain_ref} is not {memory_id} of this binding") });
        }
        let rev = g["revision"].as_u64().unwrap_or(0);
        if self.index.current.get(memory_id).is_some_and(|c| c.revision >= rev) {
            return Ok(());
        }
        let text = g["content"].as_str().unwrap_or_default().to_string();
        let l = self.line(memory_id, brain_ref, rev, &text, &format!("reconcile/{brain_ref}/{rev}"), "");
        self.record(l)
    }

    fn call(&mut self, tool: &str, mut args: Value, write: bool) -> Result<Answer, Failure> {
        if !ALLOWED.contains(&tool) {
            return Err(Failure::Unavailable { detail: format!("{tool} is outside the memory-core allowlist") });
        }
        if self.closed {
            return Err(Failure::Unavailable { detail: "brain closed: agentd is stopping".into() });
        }
        if self.mcp.is_none() {
            self.mcp = Some(Mcp::spawn(&self.binding, &self.children).map_err(|detail| Failure::Unavailable { detail })?);
        }
        args["brain_id"] = json!(self.binding.brain_id);
        let timeout = if write { self.binding.write_timeout } else { self.binding.read_timeout };
        let mcp = self.mcp.as_mut().expect("spawned above");
        let reply = match mcp.request("tools/call", json!({ "name": tool, "arguments": args }), timeout) {
            Ok(v) => v,
            Err(sent) => {
                self.lose_transport("transport lost");
                let detail = format!("{tool}: {}", if sent { "no answer within the deadline" } else { "brain-mcp connection lost" });
                return Err(if write && sent { Failure::Unknown { detail } } else { Failure::Unavailable { detail } });
            }
        };

        let sent_failure = |detail: String| if write { Failure::Unknown { detail } } else { Failure::Unavailable { detail } };
        if let Some(e) = reply.get("error") {
            return Err(sent_failure(format!("{tool}: {e}")));
        }
        let r = &reply["result"];
        let text: String = r["content"].as_array().map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect()).unwrap_or_default();
        if r["isError"].as_bool() == Some(true) {

            let parsed = text.find('{').and_then(|i| serde_json::from_str::<Value>(&text[i..]).ok()).unwrap_or(Value::Null);
            let code = parsed["code"].as_str().unwrap_or("").to_string();
            if code.is_empty() {
                return Err(sent_failure(format!("{tool}: {}", text.chars().take(300).collect::<String>())));
            }
            return Ok(Answer::Refused(code, parsed));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| sent_failure(format!("{tool}: unparseable answer: {e}")))?;

        if let Some(id) = v.get("brain_id").and_then(Value::as_str)
            && id != self.binding.brain_id
        {
            self.lose_transport("answer for another brain");
            return Err(sent_failure(format!("binding_mismatch: {tool} answered for {id}")));
        }
        Ok(Answer::Ok(v))
    }

    fn get_ref(&mut self, brain_ref: &str) -> Result<Value, Failure> {
        match self.call("brain_get", json!({ "memory_id": brain_ref }), false)? {
            Answer::Ok(v) => Ok(v),
            Answer::Refused(code, _) if code == "MEMORY_NOT_FOUND" => Err(Failure::rejected("unknown_target")),
            Answer::Refused(code, v) => Err(Failure::Unavailable { detail: format!("brain_get {code}: {v}") }),
        }
    }

    fn record(&mut self, l: IndexLine) -> Result<(), Failure> {
        let mut line = serde_json::to_string(&l).map_err(|e| Failure::Unknown { detail: e.to_string() })?;
        line.push('\n');

        self.index_file.write_all(line.as_bytes()).and_then(|()| self.index_file.sync_data()).map_err(|e| Failure::Unknown { detail: format!("binding index: {e}") })?;
        self.index.fold(l);
        Ok(())
    }

    fn line(&self, memory_id: &str, brain_ref: &str, revision: u64, text: &str, key: &str, op_digest: &str) -> IndexLine {
        IndexLine {
            schema: INDEX_SCHEMA.into(),
            instance_id: self.binding.instance_id.clone(),
            brain_id: self.binding.brain_id.clone(),
            binding_rev: self.binding.binding_rev,
            memory_id: memory_id.into(),
            brain_ref: brain_ref.into(),
            revision,
            text_sha256: sha(text),
            operation_key: key.into(),
            op_digest: op_digest.into(),
            utc_ms: now_ms(),
        }
    }

    fn confirm(&mut self, brain_ref: &str, memory_id: &str, revision: u64, text: &str) -> Result<(), Failure> {
        let v = self.get_ref(brain_ref).map_err(|f| Failure::Unknown { detail: format!("read-back: {f:?}") })?;
        let ok = v["content"].as_str() == Some(text) && v["revision"].as_u64() == Some(revision) && self.ours(&v, memory_id);
        if ok { Ok(()) } else { Err(Failure::Unknown { detail: format!("read-back of {memory_id} ({brain_ref}) did not match revision {revision}") }) }
    }

    fn replay(&self, key: &str, digest: &str) -> Option<Result<Saved, Failure>> {
        let l = self.index.by_key.get(key)?;
        Some(if l.op_digest == digest { Ok(Saved { memory_id: l.memory_id.clone(), revision: l.revision, replayed: true }) } else { Err(Failure::rejected("idempotency_conflict")) })
    }

    fn recall_native(&mut self, cfg: &SeamConfig, query: &str, limit: usize) -> Result<Recalled, Failure> {
        if self.closed {
            return Err(Failure::Unavailable { detail: "brain closed: agentd is stopping".into() });
        }
        if self.seam.is_none() {
            self.seam = Some(Seam::spawn(&self.binding, cfg, &self.children).map_err(|detail| Failure::Unavailable { detail })?);
        }
        let refs = match self.seam.as_mut().expect("spawned above").search(query, seam_limit(limit), self.binding.read_timeout) {
            Ok(r) => r,
            Err(detail) => {
                self.lose_transport("memory seam lost");
                return Err(Failure::Unavailable { detail });
            }
        };
        let mut items = Vec::new();
        let mut ranks = Vec::new();
        let mut dropped = Vec::new();
        for (rank, r) in refs.iter().enumerate() {

            let drop = |memory_id: Option<&String>, kind, reason| Dropped { memory_id: memory_id.cloned(), brain_ref: Some(r.clone()), kind, reason, rank };
            let Some(id) = self.index.by_ref.get(r).cloned() else {
                dropped.push(drop(None, "fault", "unmapped"));
                continue;
            };
            if self.index.current.values().filter(|c| c.brain_ref == *r).count() > 1 {
                dropped.push(drop(Some(&id), "fault", "ambiguous"));
                continue;
            }
            let g = self.get_ref(r)?;
            let watermark = self.index.current.get(&id).map(|c| c.revision).unwrap_or(0);
            if !self.ours(&g, &id) {

                let reason = if self.ours_history(&g, &id) { "invalidated" } else { "unknown_provenance" };
                dropped.push(drop(Some(&id), "excluded", reason));
                continue;
            }
            if g["revision"].as_u64().unwrap_or(0) < watermark {
                dropped.push(drop(Some(&id), "excluded", "superseded"));
                continue;
            }
            if is_person(&g) {
                dropped.push(drop(Some(&id), "excluded", "person_record"));
                continue;
            }
            let saved_utc_ms = self.index.current.get(&id).map(|c| c.utc_ms);
            items.push(Found { memory_id: id, revision: g["revision"].as_u64().unwrap_or(0), text: g["content"].as_str().unwrap_or_default().to_string(), saved_utc_ms, kind: kind_of(&g), speaker: speaker_of(&g) });
            ranks.push(rank);
        }
        let total = items.len();
        items.truncate(limit);
        ranks.truncate(limit);
        Ok(Recalled { omitted: total - items.len(), total, items, trace_id: Some(format!("brain:searchMemories:{}", refs.len())), ranks, dropped })
    }

    fn recall_scan(&mut self, query: &str, limit: usize) -> Result<Recalled, Failure> {
        let words = content_words(query);
        let mut lines: Vec<IndexLine> = self.index.current.values().cloned().collect();
        lines.sort_by(|a, b| b.utc_ms.cmp(&a.utc_ms));
        let scanned = lines.len().min(RECALL_SCAN_MAX);
        let mut scored = Vec::new();
        let mut dropped = Vec::new();
        for (rank, l) in lines.into_iter().take(RECALL_SCAN_MAX).enumerate() {
            let g = self.get_ref(&l.brain_ref)?;
            if !self.ours(&g, &l.memory_id) || g["revision"].as_u64().unwrap_or(0) < l.revision {
                continue;
            }
            let text = g["content"].as_str().unwrap_or_default().to_string();
            let have = content_words(&text);
            let score = words.iter().filter(|w| have.contains(*w)).count();
            if score > 0 && is_person(&g) {

                dropped.push(Dropped { memory_id: Some(l.memory_id), brain_ref: Some(l.brain_ref), kind: "excluded", reason: "person_record", rank });
            } else if score > 0 {
                scored.push((score, Found { memory_id: l.memory_id, revision: g["revision"].as_u64().unwrap_or(0), text, saved_utc_ms: Some(l.utc_ms), kind: kind_of(&g), speaker: speaker_of(&g) }));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let total = scored.len();
        let items: Vec<Found> = scored.into_iter().take(limit).map(|(_, f)| f).collect();
        Ok(Recalled { omitted: total - items.len(), total, items, trace_id: Some(format!("rth-memory-brain:scan:{scanned}")), ranks: vec![], dropped })
    }
}

const STOP: &[&str] = &["the", "and", "for", "you", "your", "what", "was", "are", "did", "that", "this", "with", "from", "have", "has", "who", "how", "que", "qual", "uma", "para", "com", "não", "sim", "asked", "keep", "tell", "about", "remember"];

fn content_words(s: &str) -> std::collections::HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric()).map(str::to_lowercase).filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str())).collect()
}

fn check_text(text: &str) -> Result<&str, Failure> {
    let t = text.trim();
    if t.is_empty() {
        return Err(Failure::rejected("empty_text"));
    }
    if t.len() > MAX_TEXT_BYTES {
        return Err(Failure::rejected("text_too_long"));
    }
    Ok(t)
}

impl Provider for Brain {
    fn name(&self) -> &'static str {
        "brain"
    }

    fn close(&mut self) {
        self.closed = true;
        self.lose_transport("closed");
    }
    fn children(&self) -> Option<Children> {
        Some(self.children.clone())
    }

    fn health(&mut self) -> Result<String, String> {
        if self.attested.is_err() {
            self.attested = self.attest();
        }
        self.attested.clone().map(|()| format!("attested binding rev {}", self.binding.binding_rev))
    }

    fn remember(&mut self, key: &str, text: &str, from: &Provenance) -> Result<Saved, Failure> {
        let text = check_text(text)?;

        let person = from.origin == Origin::PeopleFrame;
        let conversation = from.origin == Origin::Conversation;
        let digest = op_digest(if conversation { "observe" } else if person { "remember_person" } else { "remember" }, None, None, text);
        if let Some(r) = self.replay(key, &digest) {
            return r;
        }
        self.writable()?;
        if let Some(id) = self.index.same_text(&sha(text), None) {
            return Err(Failure::Rejected { reason: "duplicate_of".into(), actual_revision: None, memory_id: Some(id) });
        }
        let memory_id = format!("rm-{}", self.index.items + 1);
        let args = json!({
            "content": text,
            "operation_key": key,
            "scope": "global",

            "source": format!("user said, {}, rth:{}, command {}", local_date(now_ms()), self.binding.instance_id, from.command_id),
            "veracity": "stated",
            "importance": 0.5,
            "actor": format!("rth-agentd:{}", self.binding.instance_id),
            "reason": format!("accepted by typed command {}", from.command_id),
            "extract": false,
            "extract_entities": false,
            "metadata": {
                "rth_memory_id": memory_id,
                "rth_instance_id": self.binding.instance_id,
                "rth_binding_rev": self.binding.binding_rev,
                "rth_call_id": from.call_id,
                "rth_command_id": from.command_id,
                "rth_provider_generation": from.provider_generation,
            },
        });
        let mut args = args;
        if conversation {
            args["source"] = json!("quoted conversation evidence; speaker role is recorded in content");
            args["reason"] = json!("durable conversation observation; no extraction or effects");
            args["metadata"]["rth_kind"] = json!("conversation");
        }
        if person {
            args["metadata"]["rth_kind"] = json!(PERSON_KIND);
        }
        if let Some(speaker) = &from.speaker {
            args["metadata"]["rth_speaker"] = json!(speaker);
        }
        let v = match self.call("brain_remember", args, true)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, _) if code == "IDEMPOTENCY_CONFLICT" => return Err(Failure::rejected("idempotency_conflict")),
            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("brain_remember {code}: {v}") }),
        };
        let brain_ref = v["memory_id"].as_str().map(str::to_string).or_else(|| v["memory_id"].as_u64().map(|n| n.to_string())).ok_or(Failure::Unknown { detail: "brain_remember answered no memory_id".into() })?;
        let revision = v["revision"].as_u64().unwrap_or(0);
        if self.index.by_ref.contains_key(&brain_ref) || v["action"].as_str() != Some("inserted") || revision != 1 {

            let first = self.revision_by_key(&brain_ref, key)?.unwrap_or(Value::Null);
            let id = first["metadata"]["rth_memory_id"].as_str().map(str::to_string);
            let ours = first["revision"].as_u64() == Some(1)
                && first["content"].as_str() == Some(text)
                && first["metadata"]["rth_instance_id"].as_str() == Some(self.binding.instance_id.as_str())
                && first["metadata"]["rth_binding_rev"].as_u64() == Some(self.binding.binding_rev)
                && is_person(&first) == person;
            let Some(id) = id.filter(|_| ours) else {
                return Err(Failure::Unknown { detail: format!("unexpected_merge: brain {brain_ref} action {:?}: not this operation's item", v["action"]) });
            };
            let l = self.line(&id, &brain_ref, 1, text, key, &digest);
            self.record(l)?;
            self.reconcile_head(&brain_ref, &id)?;
            return Ok(Saved { memory_id: id, revision: 1, replayed: true });
        }
        self.confirm(&brain_ref, &memory_id, 1, text)?;
        let l = self.line(&memory_id, &brain_ref, 1, text, key, &digest);
        self.record(l)?;
        Ok(Saved { memory_id, revision: 1, replayed: false })
    }

    fn correct(&mut self, key: &str, memory_id: &str, expected_revision: u64, text: &str, from: &Provenance) -> Result<Saved, Failure> {
        let text = check_text(text)?;
        let digest = op_digest("correct", Some(memory_id), Some(expected_revision), text);
        if let Some(r) = self.replay(key, &digest) {
            return r;
        }
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        self.writable()?;

        let target = self.get_ref(&cur.brain_ref)?;
        if !self.ours(&target, memory_id) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} is not {memory_id} of this binding", cur.brain_ref) });
        }
        if let Some(id) = self.index.same_text(&sha(text), Some(memory_id)) {
            return Err(Failure::Rejected { reason: "duplicate_of".into(), actual_revision: None, memory_id: Some(id) });
        }
        let args = json!({
            "memory_id": cur.brain_ref,
            "content": text,
            "expected_revision": expected_revision,
            "operation_key": key,
            "actor": format!("rth-agentd:{}", self.binding.instance_id),
            "reason": format!("correction accepted by typed command {}", from.command_id),
        });
        let v = match self.call("brain_update", args, true)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, v) if code == "REVISION_CONFLICT" => {
                return Err(Failure::Rejected { reason: "stale_revision".into(), actual_revision: v["actual_revision"].as_u64(), memory_id: None });
            }
            Answer::Refused(code, _) if code == "MEMORY_NOT_FOUND" => return Err(Failure::rejected("unknown_target")),
            Answer::Refused(code, _) if code == "IDEMPOTENCY_CONFLICT" => return Err(Failure::rejected("idempotency_conflict")),
            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("brain_update {code}: {v}") }),
        };
        let revision = v["revision"].as_u64().ok_or(Failure::Unknown { detail: "brain_update answered no revision".into() })?;
        let want = expected_revision + 1;
        if revision == want && self.confirm(&cur.brain_ref, memory_id, revision, text).is_ok() {
            let l = self.line(memory_id, &cur.brain_ref, revision, text, key, &digest);
            self.record(l)?;
            return Ok(Saved { memory_id: memory_id.into(), revision, replayed: false });
        }

        let hist = self.revision_by_key(&cur.brain_ref, key)?.unwrap_or(Value::Null);
        if hist["revision"].as_u64() != Some(want) || hist["content"].as_str() != Some(text) || !self.ours_history(&hist, memory_id) {
            return Err(Failure::Unknown { detail: format!("brain_update answered revision {revision}; no revision {want} by this operation in history") });
        }
        let l = self.line(memory_id, &cur.brain_ref, want, text, key, &digest);
        self.record(l)?;
        self.reconcile_head(&cur.brain_ref, memory_id)?;
        Ok(Saved { memory_id: memory_id.into(), revision: want, replayed: true })
    }

    fn retag(&mut self, key: &str, memory_id: &str, expected_revision: u64, speaker: &str) -> Result<Saved, Failure> {
        let digest = op_digest("retag", Some(memory_id), Some(expected_revision), speaker);
        if let Some(r) = self.replay(key, &digest) {
            return r;
        }
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        self.writable()?;
        let head = self.get_ref(&cur.brain_ref)?;
        if !self.ours_history(&head, memory_id) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} is not {memory_id} of this binding", cur.brain_ref) });
        }
        let want = expected_revision + 1;
        let text = head["content"].as_str().unwrap_or_default().to_string();
        if head["revision"].as_u64() != Some(expected_revision) {

            return match self.revision_by_key(&cur.brain_ref, key)? {
                Some(hist) if hist["revision"].as_u64() == Some(want) && hist["metadata"]["rth_speaker"].as_str() == Some(speaker) && self.ours_history(&hist, memory_id) => {
                    let l = self.line(memory_id, &cur.brain_ref, want, hist["content"].as_str().unwrap_or_default(), key, &digest);
                    self.record(l)?;
                    self.reconcile_head(&cur.brain_ref, memory_id)?;
                    Ok(Saved { memory_id: memory_id.into(), revision: want, replayed: true })
                }
                _ => Err(Failure::Rejected { reason: "stale_revision".into(), actual_revision: head["revision"].as_u64(), memory_id: None }),
            };
        }
        if !head["invalidated_at"].is_null() {
            return Err(Failure::rejected("invalidated"));
        }
        let mut metadata = head["metadata"].clone();
        metadata["rth_speaker"] = json!(speaker);
        let args = json!({
            "content": text,
            "operation_key": key,
            "metadata": metadata,
            "actor": format!("rth-agentd:{}", self.binding.instance_id),
            "reason": "people correction of who said it",
        });
        let v = match self.call("brain_remember", args, true)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, _) if code == "IDEMPOTENCY_CONFLICT" => return Err(Failure::rejected("idempotency_conflict")),
            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("brain_remember (retag) {code}: {v}") }),
        };
        let brain_ref = v["memory_id"].as_str().map(str::to_string).or_else(|| v["memory_id"].as_u64().map(|n| n.to_string()));
        if brain_ref.as_deref() != Some(cur.brain_ref.as_str()) || v["action"].as_str() != Some("existing") {
            return Err(Failure::Unknown { detail: format!("unexpected_merge: the retag of {memory_id} answered brain {brain_ref:?} action {:?}", v["action"]) });
        }

        let hist = self.revision_by_key(&cur.brain_ref, key)?.unwrap_or(Value::Null);
        if hist["revision"].as_u64() != Some(want) || hist["content"].as_str() != Some(text.as_str()) || hist["metadata"]["rth_speaker"].as_str() != Some(speaker) || !self.ours_history(&hist, memory_id) {
            return Err(Failure::Unknown { detail: format!("brain_remember (retag) answered {v}; no revision {want} with the new speaker by this operation in history") });
        }
        let l = self.line(memory_id, &cur.brain_ref, want, &text, key, &digest);
        self.record(l)?;
        self.reconcile_head(&cur.brain_ref, memory_id)?;
        Ok(Saved { memory_id: memory_id.into(), revision: want, replayed: false })
    }

    fn invalidate(&mut self, key: &str, memory_id: &str, revision: u64, reason: &str) -> Result<Saved, Failure> {
        let digest = op_digest("invalidate", Some(memory_id), Some(revision), "");
        if let Some(r) = self.replay(key, &digest) {
            return r;
        }
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        self.writable()?;

        let target = self.get_ref(&cur.brain_ref)?;
        if !self.ours_history(&target, memory_id) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} is not {memory_id} of this binding", cur.brain_ref) });
        }

        let args = json!({
            "memory_id": cur.brain_ref,
            "expected_revision": revision,
            "operation_key": key,
            "actor": format!("rth-agentd:{}", self.binding.instance_id),
            "reason": reason,
        });
        let v = match self.call("brain_invalidate", args, true)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, v) if code == "REVISION_CONFLICT" => {
                return Err(Failure::Rejected { reason: "stale_revision".into(), actual_revision: v["actual_revision"].as_u64(), memory_id: None });
            }
            Answer::Refused(code, _) if code == "MEMORY_NOT_FOUND" => return Err(Failure::rejected("unknown_target")),
            Answer::Refused(code, _) if code == "IDEMPOTENCY_CONFLICT" => return Err(Failure::rejected("idempotency_conflict")),
            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("brain_invalidate {code}: {v}") }),
        };
        let want = revision + 1;
        if v["invalidated"].as_bool() != Some(true) || v["revision"].as_u64() != Some(want) {
            return Err(Failure::Unknown { detail: format!("brain_invalidate answered {v}; expected revision {want} invalidated") });
        }

        let hist = self.revision_by_key(&cur.brain_ref, key)?.unwrap_or(Value::Null);
        if hist["revision"].as_u64() != Some(want) || hist["invalidated_at"].is_null() || !self.ours_history(&hist, memory_id) {
            return Err(Failure::Unknown { detail: format!("brain_invalidate: no invalidated revision {want} by this operation in history") });
        }
        let text = hist["content"].as_str().unwrap_or_default().to_string();
        let l = self.line(memory_id, &cur.brain_ref, want, &text, key, &digest);
        self.record(l)?;
        Ok(Saved { memory_id: memory_id.into(), revision: want, replayed: false })
    }

    fn restore(&mut self, key: &str, memory_id: &str, expected_revision: u64, to_revision: u64) -> Result<Saved, Failure> {
        let digest = sha(&json!({ "kind": "restore", "target": memory_id, "expected_revision": expected_revision, "to_revision": to_revision }).to_string());
        if let Some(r) = self.replay(key, &digest) {
            return r;
        }
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        self.writable()?;
        let head = self.get_ref(&cur.brain_ref)?;
        if !self.ours_history(&head, memory_id) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} is not {memory_id} of this binding", cur.brain_ref) });
        }

        if head["invalidated_at"].is_null() && head["revision"].as_u64() == Some(expected_revision) {
            return Err(Failure::rejected("not_invalidated"));
        }

        let args = json!({
            "memory_id": cur.brain_ref,
            "revision": to_revision,
            "expected_revision": expected_revision,
            "operation_key": key,
            "actor": format!("rth-agentd:{}", self.binding.instance_id),
            "reason": "undo of a forget the person asked for",
        });
        let v = match self.call("brain_memory_restore", args, true)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, v) if code == "REVISION_CONFLICT" => {
                return Err(Failure::Rejected { reason: "stale_revision".into(), actual_revision: v["actual_revision"].as_u64(), memory_id: None });
            }
            Answer::Refused(code, _) if code == "MEMORY_NOT_FOUND" => return Err(Failure::rejected("unknown_target")),

            Answer::Refused(code, _) if code == "REVISION_NOT_FOUND" => return Err(Failure::rejected("unknown_revision")),
            Answer::Refused(code, _) if code == "IDEMPOTENCY_CONFLICT" => return Err(Failure::rejected("idempotency_conflict")),
            Answer::Refused(code, v) => return Err(Failure::Unknown { detail: format!("brain_memory_restore {code}: {v}") }),
        };
        let want = expected_revision + 1;
        if v["revision"].as_u64() != Some(want) || v["restored_from_revision"].as_u64() != Some(to_revision) {
            return Err(Failure::Unknown { detail: format!("brain_memory_restore answered revision {} from {}; expected {want} from {to_revision}", v["revision"], v["restored_from_revision"]) });
        }
        let hist = self.revision_by_key(&cur.brain_ref, key)?.unwrap_or(Value::Null);
        if hist["revision"].as_u64() != Some(want) || !hist["invalidated_at"].is_null() || hist["restored_from_revision"].as_u64() != Some(to_revision) || !self.ours_history(&hist, memory_id) {
            return Err(Failure::Unknown { detail: format!("brain_memory_restore: no live revision {want} restored from {to_revision} by this operation in history") });
        }
        let text = hist["content"].as_str().unwrap_or_default().to_string();
        let l = self.line(memory_id, &cur.brain_ref, want, &text, key, &digest);
        self.record(l)?;
        Ok(Saved { memory_id: memory_id.into(), revision: want, replayed: v["replayed"].as_bool().unwrap_or(false) })
    }

    fn recall(&mut self, query: &str, limit: usize) -> Result<Recalled, Failure> {
        match self.binding.seam.clone() {
            Some(cfg) => self.recall_native(&cfg, query, limit),
            None => self.recall_scan(query, limit),
        }
    }

    fn get(&mut self, memory_id: &str) -> Result<Found, Failure> {
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        let g = self.get_ref(&cur.brain_ref)?;
        if !self.ours(&g, memory_id) {
            if self.ours_history(&g, memory_id) {
                return Err(Failure::rejected("invalidated"));
            }
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} is not {memory_id} of this binding", cur.brain_ref) });
        }
        if g["revision"].as_u64().unwrap_or(0) < cur.revision {
            return Err(Failure::Unavailable { detail: format!("rollback: {memory_id} is at revision {} below the recorded {}", g["revision"], cur.revision) });
        }
        Ok(Found { memory_id: memory_id.into(), revision: g["revision"].as_u64().unwrap_or(0), text: g["content"].as_str().unwrap_or_default().to_string(), saved_utc_ms: Some(cur.utc_ms), kind: kind_of(&g), speaker: speaker_of(&g) })
    }

    fn history(&mut self, memory_id: &str) -> Result<Vec<Found>, Failure> {
        let Some(cur) = self.index.current.get(memory_id).cloned() else { return Err(Failure::rejected("unknown_target")) };
        let v = match self.call("brain_memory_history", json!({ "memory_id": cur.brain_ref, "limit": 100 }), false)? {
            Answer::Ok(v) => v,
            Answer::Refused(code, v) => return Err(Failure::Unavailable { detail: format!("brain_memory_history {code}: {v}") }),
        };
        let rows = v["revisions"].as_array().cloned().unwrap_or_default();

        if let Some(bad) = rows.iter().find(|r| !self.ours_history(r, memory_id)) {
            return Err(Failure::Unavailable { detail: format!("binding_mismatch: {} revision {} is not {memory_id} of this binding", cur.brain_ref, bad["revision"]) });
        }
        Ok(rows.iter().map(|r| Found { memory_id: memory_id.into(), revision: r["revision"].as_u64().unwrap_or(0), text: r["content"].as_str().unwrap_or_default().to_string(), saved_utc_ms: None, kind: kind_of(r), speaker: speaker_of(r) }).collect())
    }

    fn recent(&mut self, limit: usize) -> Result<Vec<Found>, Failure> {
        let mut lines: Vec<IndexLine> = self.index.current.values().cloned().collect();
        lines.sort_by(|a, b| b.utc_ms.cmp(&a.utc_ms));
        let mut out = Vec::new();
        for l in lines.into_iter().take(RECALL_SCAN_MAX.max(limit)) {
            if out.len() == limit {
                break;
            }
            let g = self.get_ref(&l.brain_ref)?;
            if !self.ours(&g, &l.memory_id) || g["revision"].as_u64().unwrap_or(0) < l.revision || is_person(&g) {
                continue;
            }
            out.push(Found { memory_id: l.memory_id, revision: g["revision"].as_u64().unwrap_or(0), text: g["content"].as_str().unwrap_or_default().to_string(), saved_utc_ms: Some(l.utc_ms), kind: kind_of(&g), speaker: speaker_of(&g) });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod scope_regression {

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn foreign_scope_rows_are_refused_on_correction_recovery_and_history() {
        let dir = std::env::temp_dir().join(format!("rth-memory-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("mcp.py");
        std::fs::write(&script, r##"#!/usr/bin/python3
import sys,json
meta={'rth_memory_id':'rm-1','rth_instance_id':'FOREIGN','rth_binding_rev':99}
log=open(sys.argv[0]+'.calls','a')
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 if r['method']=='initialize': result={}
 else:
  tool=r['params']['name']; log.write(tool+'\n'); log.flush()
  if tool=='brain_update': v={'brain_id':'brn-test','revision':2}
  elif tool=='brain_get': v={'brain_id':'brn-test','revision':2,'content':'foreign fact','metadata':meta}
  elif tool=='brain_memory_history': v={'revisions':[{'revision':2,'content':'foreign fact','operation_key':'correct-k','metadata':meta}]}
  else: raise Exception(tool)
  result={'content':[{'type':'text','text':json.dumps(v)}]}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"##).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let binding = Binding { instance_id: "OURS".into(), brain_id: "brn-test".into(), binding_rev: 1, mcp_bin: script.clone(), principal: "{}".into(), runtime_dir: None, index: dir.join("index"), read_timeout: Duration::from_secs(3), write_timeout: Duration::from_secs(3), no_network: false, record: dir.join("record"), home: Some(dir.join("home")), database: Some("fake-only".into()), seam: None };
        let (index_file, index) = open_index(&binding.index, &binding).unwrap();

        let mut b = Brain { binding, children: Children::default(), mcp: None, seam: None, index_file, index, attested: Ok(()), closed: false };
        b.record(b.line("rm-1", "brain-ref-1", 1, "old fact", "old-key", &op_digest("remember", None, None, "old fact"))).unwrap();
        let from = Provenance { instance_id: "OURS".into(), provider_generation: 1, call_id: "c".into(), response_id: None, command_id: "cmd".into(), origin: Default::default(), speaker: None };
        assert!(matches!(b.get("rm-1"), Err(Failure::Unavailable { .. })), "get refuses the foreign row");
        assert!(b.correct("correct-k", "rm-1", 1, "foreign fact", &from).is_err(), "a foreign confirmation never becomes a receipt");
        assert!(b.history("rm-1").is_err(), "history never exposes foreign rows");
        let calls = std::fs::read_to_string(format!("{}.calls", script.display())).unwrap_or_default();
        assert!(!calls.contains("brain_update"), "a known foreign target is never mutated first: {calls}");
        drop(b);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod dates {

    const EVENING_IN_SAO_PAULO: u64 = 1_792_886_700_000;

    #[test]
    #[ignore = "run by local_dates_follow_the_system_time_zone, in a child process with TZ set"]
    fn local_date_probe() {
        let want = std::env::var("RTH_PROBE_LOCAL_DATE").expect("set by the parent test");
        assert_eq!(super::local_date(EVENING_IN_SAO_PAULO), want);
    }

    #[test]
    fn local_dates_follow_the_system_time_zone() {
        assert_eq!(super::utc_date(EVENING_IN_SAO_PAULO), "2026-10-25");
        for (tz, want) in [("America/Sao_Paulo", "2026-10-24"), ("UTC", "2026-10-25"), ("Asia/Tokyo", "2026-10-25"), ("America/Los_Angeles", "2026-10-24")] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "dates::local_date_probe", "--ignored", "--test-threads=1"])
                .env("TZ", tz)
                .env("RTH_PROBE_LOCAL_DATE", want)
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success() && text.contains("1 passed"), "TZ={tz}: {text}{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    #[ignore = "run by utc_offsets_follow_the_system_time_zone, in a child process with TZ set"]
    fn utc_offset_probe() {
        let want: i32 = std::env::var("RTH_PROBE_UTC_OFFSET").expect("set by the parent test").parse().unwrap();
        assert_eq!(super::utc_offset_min(EVENING_IN_SAO_PAULO), want);
    }

    #[test]
    fn utc_offsets_follow_the_system_time_zone() {
        for (tz, want) in [("America/Sao_Paulo", -180), ("UTC", 0), ("Asia/Tokyo", 540), ("America/Los_Angeles", -420)] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "dates::utc_offset_probe", "--ignored", "--test-threads=1"])
                .env("TZ", tz)
                .env("RTH_PROBE_UTC_OFFSET", want.to_string())
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(out.status.success() && text.contains("1 passed"), "TZ={tz}: {text}{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    fn the_seam_search_never_asks_for_more_than_the_refill_bound() {
        assert_eq!((crate::seam_limit(0), crate::seam_limit(1), crate::seam_limit(3), crate::seam_limit(4), crate::seam_limit(50)), (1, 3, 9, 9, 9));
    }

    #[test]
    fn utc_dates_are_civil_dates() {
        assert_eq!(super::utc_date(0), "1970-01-01");
        assert_eq!(super::utc_date(951_782_400_000), "2000-02-29");
        assert_eq!(super::utc_date(1_792_800_000_000), "2026-10-24");
    }
}

#[cfg(test)]
mod sent_write {

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn brain(mode: &str) -> (Brain, std::path::PathBuf) {

        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rth-memory-sentwrite-{}-{}-{n}", mode.replace(':', "-"), std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("mcp.py");
        let body = format!(r##"#!/usr/bin/python3
import sys,json
mode='{mode}'
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 if r['method']=='initialize':
  print(json.dumps({{'jsonrpc':'2.0','id':r['id'],'result':{{}}}}),flush=True); continue
 if mode=='error': out={{'jsonrpc':'2.0','id':r['id'],'error':{{'code':-32000,'message':'storage error after commit'}}}}
 elif mode=='iserror': out={{'jsonrpc':'2.0','id':r['id'],'result':{{'isError':True,'content':[{{'type':'text','text':'failed after commit'}}]}}}}
 elif mode=='unparseable': out={{'jsonrpc':'2.0','id':r['id'],'result':{{'content':[{{'type':'text','text':'{{'}}]}}}}
 elif mode.startswith('coded:') and r['params']['name']=='brain_get': out={{'jsonrpc':'2.0','id':r['id'],'result':{{'content':[{{'type':'text','text':json.dumps({{'brain_id':'brn-test','revision':1,'content':'old fact','metadata':{{'rth_memory_id':'rm-1','rth_instance_id':'OURS','rth_binding_rev':1}}}})}}]}}}}
 elif mode.startswith('coded:'): out={{'jsonrpc':'2.0','id':r['id'],'result':{{'isError':True,'content':[{{'type':'text','text':'failed: '+json.dumps({{'code':mode[6:],'actual_revision':2}})}}]}}}}
 else: out={{'jsonrpc':'2.0','id':r['id'],'result':{{'content':[{{'type':'text','text':json.dumps({{'brain_id':'brn-OTHER','revision':1}})}}]}}}}
 print(json.dumps(out),flush=True)
"##);
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let binding = Binding { instance_id: "OURS".into(), brain_id: "brn-test".into(), binding_rev: 1, mcp_bin: script.clone(), principal: "{}".into(), runtime_dir: None, index: dir.join("index"), read_timeout: Duration::from_secs(3), write_timeout: Duration::from_secs(3), no_network: false, record: dir.join("record"), home: Some(dir.join("home")), database: Some("fake-only".into()), seam: None };
        let (index_file, index) = open_index(&binding.index, &binding).unwrap();
        (Brain { binding, children: Children::default(), mcp: None, seam: None, index_file, index, attested: Ok(()), closed: false }, dir)
    }

    fn sent_write(mode: &str) {
        let (mut b, dir) = brain(mode);
        let w = b.call("brain_remember", json!({ "content": "fictional" }), true);
        assert!(matches!(w, Err(Failure::Unknown { .. })), "{mode}: a sent write answered badly is unknown, got {:?}", w.as_ref().err());
        if mode == "binding" {
            assert!(b.mcp.is_none() && b.attested.is_err(), "binding_mismatch keeps lose_transport");
        }
        drop(b);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_jsonrpc_error_is_unknown() {
        sent_write("error");
    }

    #[test]
    fn write_uncoded_is_error_is_unknown() {
        sent_write("iserror");
    }

    #[test]
    fn write_unparseable_answer_is_unknown() {
        sent_write("unparseable");
    }

    #[test]
    fn write_binding_mismatch_is_unknown_and_loses_transport() {
        sent_write("binding");
    }

    fn from() -> Provenance {
        Provenance { instance_id: "OURS".into(), provider_generation: 1, call_id: "c".into(), response_id: None, command_id: "cmd".into(), origin: Default::default(), speaker: None }
    }

    #[test]
    fn unrecognized_coded_errors_after_a_sent_write_are_unknown_in_remember_correct_and_birth() {
        let (mut b, dir) = brain("coded:STORAGE_WOBBLE");
        assert!(matches!(b.remember("k1", "a fictional fact", &from()), Err(Failure::Unknown { .. })), "remember");
        let (mut c, dir2) = brain("coded:STORAGE_WOBBLE");
        c.record(c.line("rm-1", "brain-ref-1", 1, "old fact", "old-key", &op_digest("remember", None, None, "old fact"))).unwrap();
        assert!(matches!(c.correct("k2", "rm-1", 1, "new fact", &from()), Err(Failure::Unknown { .. })), "correct");
        let binding = b.binding.clone();
        drop((b, c));
        assert!(matches!(Brain::birth(&binding, "fictional-nonce"), Err(Failure::Unknown { .. })), "birth");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(dir2);
    }

    #[test]
    fn proven_no_commit_refusals_stay_definitive() {
        let (mut b, dir) = brain("coded:IDEMPOTENCY_CONFLICT");
        assert!(matches!(b.remember("k1", "a fictional fact", &from()), Err(Failure::Rejected { reason, .. }) if reason == "idempotency_conflict"));
        for (code, want) in [("REVISION_CONFLICT", "stale_revision"), ("MEMORY_NOT_FOUND", "unknown_target"), ("IDEMPOTENCY_CONFLICT", "idempotency_conflict")] {
            let (mut c, d) = brain(&format!("coded:{code}"));
            c.record(c.line("rm-1", "brain-ref-1", 1, "old fact", "old-key", &op_digest("remember", None, None, "old fact"))).unwrap();
            let r = c.correct("k2", "rm-1", 1, "new fact", &from());
            assert!(matches!(&r, Err(Failure::Rejected { reason, .. }) if reason == want), "{code}: {:?}", r.as_ref().err());
            drop(c);
            let _ = std::fs::remove_dir_all(d);
        }
        drop(b);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_control_stays_unavailable_for_every_path() {
        for mode in ["error", "iserror", "unparseable", "binding"] {
            let (mut b, dir) = brain(mode);
            let r = b.call("brain_get", json!({ "id": "x" }), false);
            assert!(matches!(r, Err(Failure::Unavailable { .. })), "{mode}: a read stays unavailable, got {:?}", r.as_ref().err());
            drop(b);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod forget_mirror {

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const FAKE: &str = include_str!("../tests/fake_brain_mcp.py");

    fn binding(name: &str) -> (Binding, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rth-memory-forget-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("brain-mcp.py");
        std::fs::write(&script, FAKE).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let b = Binding { instance_id: "OURS".into(), brain_id: "brn-fake".into(), binding_rev: 1, mcp_bin: script, principal: "{}".into(), runtime_dir: None, index: dir.join("index"), read_timeout: Duration::from_secs(5), write_timeout: Duration::from_secs(5), no_network: false, record: dir.join("record"), home: Some(dir.join("home")), database: Some("fake-only".into()), seam: None };
        Brain::birth(&b, "fictional-nonce").expect("birth against the fake");
        (b, dir)
    }

    fn from() -> Provenance {
        Provenance { instance_id: "OURS".into(), provider_generation: 1, call_id: "c".into(), response_id: None, command_id: "cmd".into(), origin: Default::default(), speaker: None }
    }

    fn writes(dir: &Path) -> Vec<Value> {
        let text = std::fs::read_to_string(dir.join("home/fake-brain-calls.jsonl")).unwrap_or_default();
        text.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|c| !["brain_get", "brain_memory_history", "brain_remember"].contains(&c["tool"].as_str().unwrap())).collect()
    }

    fn keys(v: &Value) -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    }

    #[test]
    fn brain_forget_is_outside_the_allowlist_and_never_sent() {
        assert!(!ALLOWED.contains(&"brain_forget"), "a hard delete is never dispatchable");
        let (b, dir) = binding("allowlist");
        let mut brain = Brain::open(b).unwrap();
        let r = brain.call("brain_forget", json!({ "memory_id": "1" }), true);
        assert!(matches!(&r, Err(Failure::Unavailable { detail }) if detail.contains("allowlist")), "{:?}", r.as_ref().err());
        let calls = std::fs::read_to_string(dir.join("home/fake-brain-calls.jsonl")).unwrap_or_default();
        assert!(!calls.contains("brain_forget"), "refused before anything is sent: {calls}");
        drop(brain);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalidate_then_restore_send_brains_own_tools_and_survive_restarts() {
        let (b, dir) = binding("roundtrip");
        let text = "My fictional café is Lua.";
        let mut p = Brain::open(b.clone()).unwrap();
        let a = p.remember("k-seed", text, &from()).unwrap();
        let brain_ref = p.index.current[&a.memory_id].brain_ref.clone();
        let inv = p.invalidate("k-inv", &a.memory_id, 1, "the person asked Unfold to forget this").unwrap();
        assert_eq!((inv.memory_id.as_str(), inv.revision), (a.memory_id.as_str(), 2));
        drop(p);

        let mut p = Brain::open(b.clone()).unwrap();
        assert!(p.recall("fictional café", 5).unwrap().items.is_empty(), "an invalidated item is not recalled");
        assert!(matches!(p.get(&a.memory_id), Err(Failure::Rejected { reason, .. }) if reason == "invalidated"));
        assert!(p.recent(5).unwrap().is_empty());
        let back = p.restore("k-res", &a.memory_id, 2, 1).unwrap();
        assert_eq!((back.memory_id.as_str(), back.revision, back.replayed), (a.memory_id.as_str(), 3, false));
        drop(p);

        let mut p = Brain::open(b.clone()).unwrap();
        let got = p.get(&a.memory_id).unwrap();
        assert_eq!((got.revision, got.text.as_str()), (3, text), "the same id, its text back");
        assert!(p.recall("fictional café", 5).unwrap().items.iter().any(|i| i.memory_id == a.memory_id));
        let h: Vec<(u64, String)> = p.history(&a.memory_id).unwrap().into_iter().map(|f| (f.revision, f.text)).collect();
        assert_eq!(h, vec![(3, text.to_string()), (2, text.to_string()), (1, text.to_string())], "history only grew");

        let sent = writes(&dir).len();
        assert!(p.invalidate("k-inv", &a.memory_id, 1, "x").unwrap().replayed && p.restore("k-res", &a.memory_id, 2, 1).unwrap().replayed);
        assert_eq!(writes(&dir).len(), sent);

        let w = writes(&dir);
        assert_eq!(w.iter().map(|c| c["tool"].as_str().unwrap()).collect::<Vec<_>>(), ["brain_invalidate", "brain_memory_restore"], "exactly Brain's own invalidate, then its restore");
        let (i, r) = (&w[0]["arguments"], &w[1]["arguments"]);
        assert_eq!(keys(i), ["actor", "brain_id", "expected_revision", "memory_id", "operation_key", "reason"], "no replacement_id");
        assert_eq!((i["memory_id"].as_str(), i["expected_revision"].as_u64(), i["operation_key"].as_str(), i["brain_id"].as_str()), (Some(brain_ref.as_str()), Some(1), Some("k-inv"), Some("brn-fake")));
        assert_eq!(keys(r), ["actor", "brain_id", "expected_revision", "memory_id", "operation_key", "reason", "revision"]);
        assert_eq!((r["memory_id"].as_str(), r["revision"].as_u64(), r["expected_revision"].as_u64(), r["operation_key"].as_str()), (Some(brain_ref.as_str()), Some(1), Some(2), Some("k-res")));
        drop(p);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_lost_invalidate_answer_is_unknown_and_its_retry_replays_under_the_same_key() {
        let (b, dir) = binding("lost");
        let mut p = Brain::open(b.clone()).unwrap();
        let a = p.remember("k-seed", "A fictional fact to forget.", &from()).unwrap();
        std::fs::write(dir.join("home/fake-brain-fail"), "brain_invalidate:after\n").unwrap();
        assert!(matches!(p.invalidate("k-inv", &a.memory_id, 1, "forget"), Err(Failure::Unknown { .. })), "a sent write without an answer is unknown");
        std::fs::remove_file(dir.join("home/fake-brain-fail")).unwrap();
        drop(p);
        let mut p = Brain::open(b).unwrap();
        let inv = p.invalidate("k-inv", &a.memory_id, 1, "forget").unwrap();
        assert_eq!(inv.revision, 2, "Brain replayed the committed invalidation");
        assert_eq!(p.history(&a.memory_id).unwrap().len(), 2, "one invalidation, not two");
        let w = writes(&dir);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], w[1], "the retry is the same request under the same key");
        drop(p);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod pv05_g6 {

    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn binding(name: &str, native: bool) -> (Binding, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rth-memory-pv05-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("brain-mcp.py");
        std::fs::write(&script, include_str!("../tests/fake_brain_mcp.py")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let seam = native.then(|| {
            let s = dir.join("seam.py");
            std::fs::write(&s, include_str!("../tests/fake_seam.py")).unwrap();
            SeamConfig { bun: PathBuf::from("/usr/bin/python3"), script: s, brain_src: dir.clone() }
        });
        let b = Binding { instance_id: "OURS".into(), brain_id: "brn-fake".into(), binding_rev: 1, mcp_bin: script, principal: "{}".into(), runtime_dir: None, index: dir.join("index"), read_timeout: Duration::from_secs(5), write_timeout: Duration::from_secs(5), no_network: false, record: dir.join("record"), home: Some(dir.join("home")), database: Some("fake-only".into()), seam };
        Brain::birth(&b, "fictional-nonce").expect("birth against the fake");
        (b, dir)
    }

    fn run(name: &str, native: bool) {
        let (b, dir) = binding(name, native);
        let mut open = || -> Box<dyn Provider> { Box::new(Brain::open(b.clone()).unwrap()) };
        rth_memory::contract::conformance::run_person(&mut open, name);
        let calls = std::fs::read_to_string(dir.join("home/fake-brain-calls.jsonl")).unwrap_or_default();
        let person = calls.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|c| c["tool"] == "brain_remember" && c["arguments"]["content"].as_str().is_some_and(|t| t.starts_with("Person record"))).expect("the person write was sent");
        assert_eq!(person["arguments"]["metadata"]["rth_kind"], "person", "the item is tagged rth_kind: person in Brain");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn run_speaker(name: &str, native: bool) {
        let (b, dir) = binding(name, native);
        let mut open = || -> Box<dyn Provider> { Box::new(Brain::open(b.clone()).unwrap()) };
        rth_memory::contract::conformance::run_speaker(&mut open, name);
        let calls = std::fs::read_to_string(dir.join("home/fake-brain-calls.jsonl")).unwrap_or_default();
        let tagged = calls.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).find(|c| c["tool"] == "brain_remember" && c["arguments"]["content"].as_str().is_some_and(|t| t.ends_with("said by a known person."))).expect("the tagged write was sent");
        assert_eq!(tagged["arguments"]["metadata"]["rth_speaker"], "person:p-fictional", "the speaker is Brain metadata");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pv05_g9_a_people_correction_retags_through_a_remember_dedup() {
        let (b, dir) = binding("pv05-g9-retag", false);
        let mut open = || -> Box<dyn Provider> { Box::new(Brain::open(b.clone()).unwrap()) };
        rth_memory::contract::conformance::run_retag(&mut open, "pv05-g9-retag");
        let calls = std::fs::read_to_string(dir.join("home/fake-brain-calls.jsonl")).unwrap_or_default();
        let calls: Vec<Value> = calls.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|c| c["tool"] == "brain_remember" && c["arguments"]["content"].as_str().is_some_and(|t| t.contains("birch-61"))).collect();
        assert_eq!(calls.len(), 3, "the save and two retags (the replay never reaches Brain)");
        let (saved, retag) = (&calls[0]["arguments"], &calls[1]["arguments"]);
        assert_eq!(retag["metadata"]["rth_speaker"], "person:p-fictional");
        for f in ["rth_memory_id", "rth_instance_id", "rth_binding_rev", "rth_command_id"] {
            assert_eq!(retag["metadata"][f], saved["metadata"][f], "{f} is kept");
        }
        assert!(retag.get("scope").is_none() && retag.get("importance").is_none() && retag.get("source").is_none(), "nothing else the dedup could change is sent: {retag}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pv05_g9_an_item_keeps_its_speaker_on_the_scan_path() {
        run_speaker("pv05-g9-speaker-scan", false);
    }

    #[test]
    fn pv05_g9_an_item_keeps_its_speaker_on_the_native_path() {
        run_speaker("pv05-g9-speaker-native", true);
    }

    #[test]
    fn a_person_item_never_reaches_recall_on_the_scan_path() {
        run("scan", false);
    }

    #[test]
    fn a_person_item_never_reaches_recall_on_the_native_path() {
        run("native", true);
    }
}
