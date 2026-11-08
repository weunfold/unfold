

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use crate::confine::{Confiner, Denied};
use crate::grants::{FolderId, Grants, Line, Request};
use crate::parts::{Offered, PartSet};
use crate::profile::Loaded;
use crate::receipt::{Outcome, Receipt, Receipts};
use crate::web::{BoxFuture, Connect, FetchConfig, Fetched, Resolve};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {

    Approved { effect_id: String, args_sha256: String },

    Declined(String),
}

pub trait Approvals: Send + Sync {
    fn folder<'a>(&'a self, task_id: &'a str, request: &'a Request) -> BoxFuture<'a, Decision>;
    fn fetch<'a>(&'a self, task_id: &'a str, url: &'a str, why: &'a str) -> BoxFuture<'a, Decision>;

    fn part<'a>(&'a self, task_id: &'a str, call: &'a PartCall) -> BoxFuture<'a, Decision> {
        let _ = (task_id, call);
        Box::pin(async { Decision::Declined("unsupported".into()) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartCall {
    pub part_id: String,
    pub title: String,
    pub digest: String,
    pub tool: String,
    pub args: Value,

    pub args_sha256: String,
    pub why: String,
}

pub const PART_MAX_CHARS: usize = 24_000;

pub const PART_CARD_MAX_CHARS: usize = 2_000;

pub trait Recall: Send + Sync {
    fn recall<'a>(&'a self, query: &'a str, task_id: &'a str) -> BoxFuture<'a, Result<Value, String>>;
}

pub trait Events: Send + Sync {
    fn grant_event(&self, line: &Line);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Hold(String),
    Refuse(String),
}

pub trait Guard: Send + Sync {
    fn fetch(&self, task_text: &str, data: &[String], url: &str) -> Verdict;
}

pub struct ToolHost {
    pub profile: Loaded,
    pub grants: Arc<Mutex<Grants>>,

    pub blocked: Vec<PathBuf>,
    pub home: Option<PathBuf>,
    pub resolver: Arc<dyn Resolve>,
    pub connector: Arc<dyn Connect>,
    pub fetch: FetchConfig,
    pub approvals: Arc<dyn Approvals>,
    pub recall: Option<Arc<dyn Recall>>,
    pub receipts: Arc<dyn Receipts>,
    pub events: Arc<dyn Events>,
    pub guard: Option<Arc<dyn Guard>>,

    pub parts: Option<Arc<PartSet>>,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub task_id: String,
    pub text: String,

    pub base: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Answered {
    pub content: Value,
    pub receipt: Receipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptFailed(pub String);

pub struct TaskTools {
    pub host: Arc<ToolHost>,
    pub task: Task,
    seq: AtomicU64,
    private_seen: AtomicBool,

    data: Mutex<Vec<String>>,
    fetched: Mutex<HashMap<String, Fetched>>,
    approved_urls: Mutex<HashSet<String>>,

    open_requests: Mutex<Vec<String>>,
}

const MAX_DATA_CHARS: usize = 64 * 1024;

struct Ran {
    content: Value,
    outcome: Outcome,
    reason: Option<String>,
    grant_id: Option<String>,
    request_id: Option<String>,
    target: Option<String>,
}

impl Ran {
    fn ok(content: Value) -> Ran {
        Ran { content, outcome: Outcome::Ok, reason: None, grant_id: None, request_id: None, target: None }
    }

    fn no(outcome: Outcome, reason: &str, message: &str) -> Ran {
        Ran { content: json!({ "error": reason, "message": message }), outcome, reason: Some(reason.to_string()), grant_id: None, request_id: None, target: None }
    }

    fn refused(reason: &str, message: &str) -> Ran {
        Ran::no(Outcome::Refused, reason, message)
    }

    fn failed(reason: &str, message: &str) -> Ran {
        Ran::no(Outcome::Failed, reason, message)
    }
}

fn describe(name: &str) -> (&'static str, Value) {
    match name {
        "files.read" => (
            "Read a text file in a folder the person approved. Returns one page of up to the page size in characters, with offset, next_offset and total_bytes; call again with offset set to next_offset for the next page. The first read in a new folder asks the person and waits for the answer.",
            json!({"type": "object", "properties": {"path": {"type": "string", "description": "The file: an absolute path, ~/..., or relative to the task's folder."}, "offset": {"type": "integer", "minimum": 0, "description": "Byte offset of the page (next_offset from the previous page). Default 0."}}, "required": ["path"], "additionalProperties": false}),
        ),
        "files.search" => (
            "Find files under a folder the person approved: file names and lines that contain the query (case-insensitive). An empty query lists the files. Results are paged; call again with offset set to next_offset.",
            json!({"type": "object", "properties": {"path": {"type": "string", "description": "The folder to search."}, "query": {"type": "string", "description": "Text to look for in names and lines; empty lists files."}, "offset": {"type": "integer", "minimum": 0}}, "required": ["path", "query"], "additionalProperties": false}),
        ),
        "web.fetch" => (
            "Fetch a public web page with GET (https only) and return its text, paged like files_read. Once a file or a memory was read in this task, each new address waits for the person's approval.",
            json!({"type": "object", "properties": {"url": {"type": "string", "description": "The https address."}, "offset": {"type": "integer", "minimum": 0}}, "required": ["url"], "additionalProperties": false}),
        ),
        _ => (
            "Recall the person's saved memories that match a query.",
            json!({"type": "object", "properties": {"query": {"type": "string", "description": "What to recall (1-500 characters)."}}, "required": ["query"], "additionalProperties": false}),
        ),
    }
}

fn parse_args(arguments: &str, required: &[&str], allowed: &[&str]) -> Result<(serde_json::Map<String, Value>, u64), String> {
    let v: Value = serde_json::from_str(if arguments.trim().is_empty() { "{}" } else { arguments }).map_err(|e| format!("arguments are not JSON: {e}"))?;
    let Value::Object(o) = v else { return Err("arguments are not an object".into()) };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("unknown argument {k:?}"));
    }
    for r in required {
        if !o.get(*r).is_some_and(Value::is_string) {
            return Err(format!("{r} must be a string"));
        }
    }
    let offset = match o.get("offset") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or("offset must be a non-negative integer")?,
    };
    Ok((o, offset))
}

impl TaskTools {
    pub fn new(host: Arc<ToolHost>, task: Task) -> TaskTools {
        TaskTools {
            host,
            task,
            seq: AtomicU64::new(0),
            private_seen: AtomicBool::new(false),
            data: Mutex::new(Vec::new()),
            fetched: Mutex::new(HashMap::new()),
            approved_urls: Mutex::new(HashSet::new()),
            open_requests: Mutex::new(Vec::new()),
        }
    }

    pub fn declarations(&self) -> Value {
        let mut out: Vec<Value> = self
            .host
            .profile
            .profile
            .tools
            .iter()
            .map(|t| {
                let (description, parameters) = describe(&t.name);
                json!({"type": "function", "function": {"name": crate::profile::wire_name(&t.name), "description": description, "parameters": parameters}})
            })
            .collect();
        for o in self.offered() {
            let (description, parameters) = o.part.schema(&o.tool.name).unwrap_or_else(|| (o.tool.description.clone().unwrap_or_default(), json!({"type": "object"})));
            let description = format!("[{}] {description}", o.part.manifest().title);
            out.push(json!({"type": "function", "function": {"name": o.wire, "description": description, "parameters": parameters}}));
        }
        Value::Array(out)
    }

    fn offered(&self) -> Vec<Offered> {
        match &self.host.parts {
            Some(set) => set.offered(self.host.profile.profile.parts.as_ref()),
            None => Vec::new(),
        }
    }

    fn tool_name(&self, wire_name: &str) -> String {
        if let Some(s) = crate::profile::from_wire(&self.host.profile, wire_name) {
            return s.name.clone();
        }
        match self.host.parts.as_ref().and_then(|set| set.part_of(wire_name)) {
            Some((part, tool)) => format!("{part}/{tool}"),
            None => wire_name.to_string(),
        }
    }

    pub fn private_seen(&self) -> bool {
        self.private_seen.load(Ordering::SeqCst)
    }

    pub async fn call(&self, call_id: &str, wire_name: &str, arguments: &str) -> Result<Answered, ReceiptFailed> {
        let t0 = Instant::now();
        let spec = crate::profile::from_wire(&self.host.profile, wire_name).cloned();
        let tool = self.tool_name(wire_name);
        let ran = match &spec {
            None => self.part(wire_name, arguments).await,
            Some(s) if s.effect != crate::profile::Effect::None => Ran::refused("effect_not_allowed", "This task can only read."),
            Some(s) => self.run(&s.name, arguments).await,
        };
        let mut r = self.receipt(call_id, &tool, arguments, ran.outcome);
        r.reason = ran.reason;
        r.grant_id = ran.grant_id;
        r.request_id = ran.request_id;
        r.target = ran.target;
        r.result_sha256 = Some(crate::canonical_sha256(&ran.content));
        r.result_chars = ran.content.to_string().chars().count() as u64;
        r.ms = t0.elapsed().as_millis() as u64;
        self.host.receipts.record(&r).map_err(|e| ReceiptFailed(e.to_string()))?;
        Ok(Answered { content: ran.content, receipt: r })
    }

    pub fn refuse(&self, call_id: &str, wire_name: &str, arguments: &str, reason: &str, message: &str) -> Result<Answered, ReceiptFailed> {
        let tool = self.tool_name(wire_name);
        let ran = Ran::refused(reason, message);
        let mut r = self.receipt(call_id, &tool, arguments, ran.outcome);
        r.reason = ran.reason;
        r.result_sha256 = Some(crate::canonical_sha256(&ran.content));
        r.result_chars = ran.content.to_string().chars().count() as u64;
        self.host.receipts.record(&r).map_err(|e| ReceiptFailed(e.to_string()))?;
        Ok(Answered { content: ran.content, receipt: r })
    }

    pub fn cancelled(&self, call_id: &str, wire_name: &str, arguments: &str) -> Result<Receipt, ReceiptFailed> {
        let tool = self.tool_name(wire_name);
        let mut r = self.receipt(call_id, &tool, arguments, Outcome::Cancelled);
        r.reason = Some("cancelled".into());
        self.host.receipts.record(&r).map_err(|e| ReceiptFailed(e.to_string()))?;
        Ok(r)
    }

    pub fn end(&self, reason: &str) {
        let open: Vec<String> = std::mem::take(&mut *self.open_requests.lock().unwrap());
        for id in open {
            let line = self.host.grants.lock().unwrap().deny(&id, reason);
            if let Ok(Ok(line)) = line {
                self.host.events.grant_event(&line);
            }
        }
    }

    async fn run(&self, tool: &str, arguments: &str) -> Ran {
        match tool {
            "files.read" => match parse_args(arguments, &["path"], &["path", "offset"]) {
                Err(e) => Ran::refused("invalid_arguments", &e),
                Ok((a, offset)) => self.read(a["path"].as_str().unwrap_or_default(), offset).await,
            },
            "files.search" => match parse_args(arguments, &["path", "query"], &["path", "query", "offset"]) {
                Err(e) => Ran::refused("invalid_arguments", &e),
                Ok((a, offset)) => self.search(a["path"].as_str().unwrap_or_default(), a["query"].as_str().unwrap_or_default(), offset as usize).await,
            },
            "web.fetch" => match parse_args(arguments, &["url"], &["url", "offset"]) {
                Err(e) => Ran::refused("invalid_arguments", &e),
                Ok((a, offset)) => self.fetch(a["url"].as_str().unwrap_or_default(), offset as usize).await,
            },
            "memory.recall" => match parse_args(arguments, &["query"], &["query"]) {
                Err(e) => Ran::refused("invalid_arguments", &e),
                Ok((a, _)) => self.recall(a["query"].as_str().unwrap_or_default()).await,
            },
            other => Ran::refused("unknown_tool", &format!("{other} is not wired in this build.")),
        }
    }

    async fn part(&self, wire_name: &str, arguments: &str) -> Ran {
        let unknown = || Ran::refused("unknown_tool", &format!("There is no tool named {wire_name} in this task."));
        let Some(set) = &self.host.parts else { return unknown() };
        let Some(o) = set.find(self.host.profile.profile.parts.as_ref(), wire_name) else {
            let Some((part_id, _)) = set.part_of(wire_name) else { return unknown() };
            let standing = set.get(&part_id).map(|p| set.ledger().standing(&part_id, p.digest()));
            return match standing {
                Some(crate::authority::Standing::Off) => Ran::refused("part_off", &format!("{part_id} is turned off.")),
                Some(crate::authority::Standing::Changed { .. }) => Ran::refused("part_changed", &format!("{part_id} changed since the person approved it; it waits for a new approval.")),
                _ => Ran::refused("not_allowed", &format!("This task may not call {wire_name}.")),
            };
        };
        let (part_id, digest) = (o.part.manifest().id.clone(), o.part.digest().to_string());
        let target = Some(format!("{part_id}@{digest}"));
        let no = |mut r: Ran| {
            r.target = target.clone();
            r
        };
        let args = match serde_json::from_str::<Value>(if arguments.trim().is_empty() { "{}" } else { arguments }) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return no(Ran::refused("invalid_arguments", "arguments are not an object")),
            Err(e) => return no(Ran::refused("invalid_arguments", &format!("arguments are not JSON: {e}"))),
        };
        if o.tool.effect != crate::profile::Effect::None {
            return no(Ran::refused("effect_not_allowed", "This task can only read."));
        }
        let private = self.private_seen();
        if o.part.asks(&o.tool.name) || private {
            let value = Value::Object(args.clone());
            if value.to_string().chars().count() > PART_CARD_MAX_CHARS {
                return no(Ran::refused("arguments_too_long_to_confirm", &format!("These arguments are longer than {PART_CARD_MAX_CHARS} characters, too long to show the person whole; nothing ran. Make the call with shorter arguments.")));
            }
            let want = crate::parts::call_sha256(&part_id, &digest, &o.tool.name, &value);
            let why = if private { "private data is already in this task, and these arguments leave this computer".to_string() } else { format!("{} asks before each call", o.part.manifest().title) };
            let card = PartCall { part_id: part_id.clone(), title: o.part.manifest().title.clone(), digest: digest.clone(), tool: o.tool.name.clone(), args: value, args_sha256: want.clone(), why };
            match self.host.approvals.part(&self.task.task_id, &card).await {
                Decision::Approved { args_sha256, .. } if args_sha256 == want => {}
                Decision::Approved { .. } => return no(Ran::refused("approval_mismatch", "The tap approved a different call; nothing ran.")),
                Decision::Declined(_) => return no(Ran::refused("declined", "The person did not approve this call.")),
            }
        }
        let out = match o.part.call(&o.tool.name, args).await {
            Ok(out) => out,
            Err(e) => return no(Ran::failed("part_failed", &e)),
        };
        let total = out.text.chars().count();
        let text: String = out.text.chars().take(PART_MAX_CHARS).collect();
        self.remember_private(text.clone());
        let mut content = json!({"part": part_id, "tool": o.tool.name, "text": text, "is_error": out.is_error});
        if total > PART_MAX_CHARS {
            content["truncated"] = json!(true);
            content["total_chars"] = json!(total);
        }
        let mut r = Ran::ok(content);
        if out.is_error {
            r.outcome = Outcome::Failed;
            r.reason = Some("tool_error".into());
        }
        no(r)
    }

    fn snapshot(&self) -> Vec<crate::grants::Grant> {
        self.host.grants.lock().unwrap().list()
    }

    fn blocked_now(&self) -> Vec<FolderId> {
        crate::confine::block_list(&self.host.blocked)
    }

    fn confiner<'a>(&'a self, grants: &'a Vec<crate::grants::Grant>, blocked: &'a [FolderId]) -> Confiner<'a> {
        Confiner { grants, blocked, base: self.task.base.as_deref(), home: self.host.home.as_deref() }
    }

    fn remember_private(&self, text: String) {
        self.private_seen.store(true, Ordering::SeqCst);
        let mut d = self.data.lock().unwrap();
        d.push(text);
        let mut total: usize = d.iter().map(|s| s.len()).sum();
        while total > MAX_DATA_CHARS && d.len() > 1 {
            total -= d.remove(0).len();
        }
    }

    async fn granted<T>(&self, tool: &str, raw: &str, ran: &mut Ran, open: impl Fn(&Confiner) -> Result<T, Denied>) -> Result<T, Ran> {
        let first = {
            let (snap, blocked) = (self.snapshot(), self.blocked_now());
            open(&self.confiner(&snap, &blocked))
        };
        let (folder, id) = match first {
            Ok(v) => return Ok(v),
            Err(Denied::NotGranted { folder, id }) => (folder, id),
            Err(d) => return Err(denied(d)),
        };
        let asked = self.host.grants.lock().unwrap().ask(&self.task.task_id, &folder, id, &format!("{tool} {raw}"));
        let (req, line) = match asked {
            Ok(x) => x,
            Err(e) => return Err(Ran::failed("grant_log_failed", &e.to_string())),
        };
        if let Some(l) = &line {
            self.host.events.grant_event(l);
        }
        ran.request_id = Some(req.request_id.clone());
        self.open_requests.lock().unwrap().push(req.request_id.clone());
        let decision = self.host.approvals.folder(&self.task.task_id, &req).await;
        self.open_requests.lock().unwrap().retain(|r| *r != req.request_id);
        match decision {
            Decision::Approved { effect_id, args_sha256 } => {
                let g = self.host.grants.lock().unwrap().grant(&req.request_id, &effect_id, &args_sha256);
                match g {
                    Ok(Ok((_, line))) => self.host.events.grant_event(&line),
                    Ok(Err(why)) => return Err(Ran::refused("not_granted", why)),
                    Err(e) => return Err(Ran::failed("grant_log_failed", &e.to_string())),
                }
            }
            Decision::Declined(why) => {
                if let Ok(Ok(line)) = self.host.grants.lock().unwrap().deny(&req.request_id, &why) {
                    self.host.events.grant_event(&line);
                }
                let msg = if why == "expired" { "The approval card expired before the person answered." } else { "The person did not approve reading this folder." };
                return Err(Ran::refused("not_granted", &format!("{msg} Folder: {}", folder.display())));
            }
        }
        let (snap, blocked) = (self.snapshot(), self.blocked_now());
        open(&self.confiner(&snap, &blocked)).map_err(denied)
    }

    async fn read(&self, raw: &str, offset: u64) -> Ran {
        let mut ran = Ran::ok(Value::Null);
        let mut opened = match self.granted("files.read", raw, &mut ran, |c| c.open_file(raw)).await {
            Ok(o) => o,
            Err(mut r) => {
                r.request_id = ran.request_id;
                return r;
            }
        };
        let page_chars = self.host.profile.profile.limits.page_chars;
        let grant_id = opened.grant.grant_id.clone();
        let target = opened.real.to_string_lossy().into_owned();
        let mut out = match crate::files::read_page(&mut opened, offset, page_chars) {
            Ok(p) => {
                self.remember_private(p.text.clone());
                Ran::ok(serde_json::to_value(&p).unwrap_or(Value::Null))
            }
            Err(e) => Ran::failed(e.reason(), &format!("{e:?}")),
        };
        out.grant_id = Some(grant_id);
        out.request_id = ran.request_id;
        out.target = Some(target);
        out
    }

    async fn search(&self, raw: &str, query: &str, offset: usize) -> Ran {
        let mut ran = Ran::ok(Value::Null);
        let dir = match self.granted("files.search", raw, &mut ran, |c| c.open_dir(raw)).await {
            Ok(d) => d,
            Err(mut r) => {
                r.request_id = ran.request_id;
                return r;
            }
        };
        let (snap, blocked, base, home, q) = (self.snapshot(), self.blocked_now(), self.task.base.clone(), self.host.home.clone(), query.to_string());
        let d2 = dir.clone();
        let page = tokio::task::spawn_blocking(move || {
            let c = Confiner { grants: &snap, blocked: &blocked, base: base.as_deref(), home: home.as_deref() };
            crate::files::search(&c, &d2, &q, offset, crate::files::SearchLimits::default())
        })
        .await;
        let mut out = match page {
            Ok(p) => {
                self.remember_private(p.hits.iter().map(|h| h.snippet.clone()).collect::<Vec<_>>().join("\n"));
                Ran::ok(serde_json::to_value(&p).unwrap_or(Value::Null))
            }
            Err(e) => Ran::failed("search_failed", &e.to_string()),
        };
        out.grant_id = Some(dir.grant.grant_id.clone());
        out.request_id = ran.request_id;
        out.target = Some(dir.real.to_string_lossy().into_owned());
        out
    }

    async fn fetch(&self, raw: &str, offset: usize) -> Ran {
        let url = match crate::web::vet_url(raw) {
            Ok(u) => u.to_string(),
            Err(e) => return Ran::refused(e.reason(), &format!("{e:?}")),
        };
        let page_chars = self.host.profile.profile.limits.page_chars;
        let cached = self.fetched.lock().unwrap().get(&url).cloned();
        let fetched = match cached {
            Some(f) => f,
            None => {
                let verdict = match &self.host.guard {
                    Some(g) => g.fetch(&self.task.text, &self.data.lock().unwrap(), &url),
                    None => Verdict::Allow,
                };
                if let Verdict::Refuse(rule) = &verdict {
                    return Ran::refused("guard_refused", &format!("This address is not fetched ({rule})."));
                }
                let hold_private = self.private_seen() && self.host.profile.profile.egress_after_private == crate::profile::Egress::Hold;
                let approved = self.approved_urls.lock().unwrap().contains(&url);
                if (hold_private || matches!(verdict, Verdict::Hold(_))) && !approved {
                    let why = match &verdict {
                        Verdict::Hold(rule) => rule.clone(),
                        _ => "private data is already in this task".to_string(),
                    };
                    match self.host.approvals.fetch(&self.task.task_id, &url, &why).await {
                        Decision::Approved { .. } => {
                            self.approved_urls.lock().unwrap().insert(url.clone());
                        }
                        Decision::Declined(_) => return Ran::refused("declined", "The person did not approve fetching this address."),
                    }
                }
                match crate::web::fetch(&url, self.host.resolver.as_ref(), self.host.connector.as_ref(), &self.host.fetch).await {
                    Ok(f) => {
                        self.fetched.lock().unwrap().insert(url.clone(), f.clone());
                        f
                    }
                    Err(e) => {
                        let mut r = Ran::failed(e.reason(), &format!("{e:?}"));
                        r.target = Some(url);
                        return r;
                    }
                }
            }
        };
        let (start, text, next) = crate::web::page(&fetched.text, offset, page_chars);
        let mut r = Ran::ok(json!({
            "url": fetched.url, "final_url": fetched.final_url, "status": fetched.status, "content_type": fetched.content_type,
            "offset": start, "next_offset": next, "total_chars": fetched.text.chars().count(), "more": next.is_some(), "complete": fetched.complete, "text": text,
        }));
        r.target = Some(fetched.final_url.clone());
        r
    }

    async fn recall(&self, query: &str) -> Ran {
        let n = query.trim().chars().count();
        if n == 0 || n > 500 {
            return Ran::refused("invalid_arguments", "query must be 1-500 characters");
        }
        let Some(recall) = &self.host.recall else { return Ran::refused("memory_off", "Memory is off for this agent.") };
        match recall.recall(query, &self.task.task_id).await {
            Ok(v) => {
                self.remember_private(v.to_string());
                Ran::ok(v)
            }
            Err(e) => Ran::failed("recall_failed", &e),
        }
    }

    fn receipt(&self, call_id: &str, tool: &str, arguments: &str, outcome: Outcome) -> Receipt {
        Receipt {
            task_id: self.task.task_id.clone(),
            call_seq: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            call_id: call_id.to_string(),
            tool: tool.to_string(),
            args_sha256: crate::sha256_hex(arguments.as_bytes()),
            profile_sha256: self.host.profile.sha256.clone(),
            outcome,
            reason: None,
            grant_id: None,
            request_id: None,
            target: None,
            result_sha256: None,
            result_chars: 0,
            ms: 0,
            utc_ms: crate::utc_ms(),
        }
    }
}

fn denied(d: Denied) -> Ran {
    match &d {
        Denied::NotGranted { folder, .. } => Ran::refused("not_granted", &format!("{} is not in an approved folder.", folder.display())),
        Denied::TooBroad { folder } => Ran::refused("too_broad", &format!("{} is too broad to approve; ask for a narrower folder.", folder.display())),
        Denied::Blocked => Ran::refused("blocked", "That location is never readable."),
        Denied::Invalid(why) => Ran::refused("invalid_path", why),
        Denied::NotFound => Ran::failed("not_found", "Nothing is there."),
        Denied::NotRegular => Ran::failed("not_a_file", "That is not a regular file."),
        Denied::NotDirectory => Ran::failed("not_a_folder", "That is not a folder."),
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use crate::receipt;

    #[derive(Default)]
    pub struct Cards {
        pub folder_answers: Mutex<Vec<Decision>>,
        pub fetch_answers: Mutex<Vec<Decision>>,
        pub folders_asked: Mutex<Vec<Request>>,
        pub fetches_asked: Mutex<Vec<String>>,
        pub part_answers: Mutex<Vec<Decision>>,
        pub parts_asked: Mutex<Vec<PartCall>>,

        pub hang: AtomicBool,
        pub dropped: Arc<AtomicU64>,
    }

    struct OnDrop(Arc<AtomicU64>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Approvals for Cards {
        fn folder<'a>(&'a self, _task_id: &'a str, request: &'a Request) -> BoxFuture<'a, Decision> {
            self.folders_asked.lock().unwrap().push(request.clone());
            let guard = OnDrop(self.dropped.clone());
            Box::pin(async move {
                let _g = guard;
                if self.hang.load(Ordering::SeqCst) {
                    std::future::pending::<()>().await;
                }
                let mut a = self.folder_answers.lock().unwrap();
                if a.is_empty() { Decision::Declined("person".into()) } else { a.remove(0) }
            })
        }

        fn fetch<'a>(&'a self, _task_id: &'a str, url: &'a str, _why: &'a str) -> BoxFuture<'a, Decision> {
            self.fetches_asked.lock().unwrap().push(url.to_string());
            Box::pin(async move {
                let mut a = self.fetch_answers.lock().unwrap();
                if a.is_empty() { Decision::Declined("person".into()) } else { a.remove(0) }
            })
        }

        fn part<'a>(&'a self, _task_id: &'a str, call: &'a PartCall) -> BoxFuture<'a, Decision> {
            self.parts_asked.lock().unwrap().push(call.clone());
            Box::pin(async move {
                let mut a = self.part_answers.lock().unwrap();
                if a.is_empty() { Decision::Declined("person".into()) } else { a.remove(0) }
            })
        }
    }

    #[derive(Default)]
    pub struct Published(pub Mutex<Vec<Line>>);
    impl Events for Published {
        fn grant_event(&self, line: &Line) {
            self.0.lock().unwrap().push(line.clone());
        }
    }

    pub struct Recalled;
    impl Recall for Recalled {
        fn recall<'a>(&'a self, query: &'a str, _task_id: &'a str) -> BoxFuture<'a, Result<Value, String>> {
            let q = query.to_string();
            Box::pin(async move { Ok(json!({"memories": [{"memory_id": "m-1", "text": format!("about {q}: the person's name is Ana")}]})) })
        }
    }

    pub struct NoNet;
    impl Resolve for NoNet {
        fn resolve<'a>(&'a self, _host: &'a str, _port: u16) -> BoxFuture<'a, std::io::Result<Vec<std::net::SocketAddr>>> {
            Box::pin(async { Ok(vec!["93.184.215.14:443".parse().unwrap()]) })
        }
    }

    pub struct Canned(pub Vec<u8>);
    impl Connect for Canned {
        fn connect<'a>(&'a self, _addr: std::net::SocketAddr, _host: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn crate::web::Stream>>> {
            let (a, mut b) = tokio::io::duplex(1 << 20);
            let body = self.0.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 4096];
                let _ = b.read(&mut buf).await;
                let _ = b.write_all(&body).await;
                let _ = b.shutdown().await;
            });
            Box::pin(async move { Ok(Box::new(a) as Box<dyn crate::web::Stream>) })
        }
    }

    pub struct Rig {
        pub dir: crate::testdir::TempDir,
        pub cards: Arc<Cards>,
        pub receipts: Arc<receipt::Memory>,
        pub events: Arc<Published>,
        pub host: Arc<ToolHost>,
        pub parts: Arc<PartSet>,
    }

    pub fn rig(tag: &str, profile: Loaded) -> Rig {
        let dir = crate::testdir::TempDir::new(tag);
        dir.write("docs/a.md", "alpha line\n");
        dir.write("docs/big.txt", &"0123456789".repeat(3000));
        dir.write("other/b.md", "beta\n");
        let (grants, _) = Grants::open(&dir.path().join("grants.jsonl"), "b1").unwrap();
        let cards = Arc::new(Cards::default());
        let receipts = Arc::new(receipt::Memory::default());
        let events = Arc::new(Published::default());
        let parts = Arc::new(PartSet::new(crate::authority::Parts::open(&dir.path().join("parts.jsonl")).unwrap(), crate::authority::Layer::up_to(crate::profile::Effect::None)));
        let page = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 11\r\n\r\npublic page".to_vec();
        let host = Arc::new(ToolHost {
            profile,
            grants: Arc::new(Mutex::new(grants)),
            blocked: Vec::new(),
            home: None,
            resolver: Arc::new(NoNet),
            connector: Arc::new(Canned(page)),
            fetch: FetchConfig::default(),
            approvals: cards.clone(),
            recall: Some(Arc::new(Recalled)),
            receipts: receipts.clone(),
            events: events.clone(),
            guard: None,
            parts: Some(parts.clone()),
        });
        Rig { dir, cards, receipts, events, host, parts }
    }

    pub fn approved() -> Decision {
        Decision::Approved { effect_id: "e-1".into(), args_sha256: "digest".into() }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;
    use crate::profile;

    fn tools(r: &Rig) -> TaskTools {
        TaskTools::new(r.host.clone(), Task { task_id: "b1-t1".into(), text: "find the port".into(), base: None })
    }

    fn path(r: &Rig, rel: &str) -> String {
        r.dir.path().join(rel).to_string_lossy().into_owned()
    }

    fn args(v: Value) -> String {
        v.to_string()
    }

    #[test]
    fn declarations_carry_the_profile_tools_by_wire_name() {
        let r = rig("host-decl", profile::research());
        let d = tools(&r).declarations();
        let names: Vec<&str> = d.as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["files_read", "files_search", "web_fetch", "memory_recall"]);
        assert!(d.as_array().unwrap().iter().all(|t| t["type"] == "function" && t["function"]["parameters"]["type"] == "object"));
    }

    #[tokio::test]
    async fn every_call_gets_exactly_one_receipt_in_order_valid_or_not() {
        let r = rig("host-receipts", profile::research());
        let t = tools(&r);
        r.cards.folder_answers.lock().unwrap().push(approved());
        let a = t.call("c1", "files_read", &args(json!({"path": path(&r, "docs/a.md")}))).await.unwrap();
        assert_eq!(a.content["text"], "alpha line\n");
        let bad = t.call("c2", "files_read", "{not json").await.unwrap();
        assert_eq!(bad.content["error"], "invalid_arguments");
        let unknown = t.call("c3", "shell_run", "{}").await.unwrap();
        assert_eq!(unknown.content["error"], "unknown_tool");
        let extra = t.call("c4", "files_read", &args(json!({"path": path(&r, "docs/a.md"), "mode": "w"}))).await.unwrap();
        assert_eq!(extra.content["error"], "invalid_arguments", "unknown keys are refused");
        let all = r.receipts.all();
        let got: Vec<(u64, &str, Outcome, Option<&str>)> = all.iter().map(|x| (x.call_seq, x.call_id.as_str(), x.outcome, x.reason.as_deref())).collect();
        assert_eq!(
            got,
            [(1, "c1", Outcome::Ok, None), (2, "c2", Outcome::Refused, Some("invalid_arguments")), (3, "c3", Outcome::Refused, Some("unknown_tool")), (4, "c4", Outcome::Refused, Some("invalid_arguments"))]
        );
        assert_eq!(all[0].tool, "files.read");
        assert_eq!(all[0].target.as_deref(), Some(path(&r, "docs/a.md").as_str()));
        assert_eq!(all[0].result_sha256.as_deref(), Some(crate::canonical_sha256(&a.content).as_str()), "the receipt names exactly what the model got");
        assert_eq!(all[0].profile_sha256, profile::research().sha256);
        assert!(all[0].grant_id.is_some() && all[0].request_id.is_some());
        assert_eq!(all[2].tool, "shell_run");
    }

    #[tokio::test]
    async fn a_folder_is_asked_once_and_a_declined_card_reads_nothing() {
        let r = rig("host-ask", profile::research());
        let t = tools(&r);
        let no = t.call("c1", "files_read", &args(json!({"path": path(&r, "other/b.md")}))).await.unwrap();
        assert_eq!((no.content["error"].as_str(), no.receipt.outcome), (Some("not_granted"), Outcome::Refused));
        assert!(no.content.get("text").is_none());
        r.cards.folder_answers.lock().unwrap().push(approved());
        t.call("c2", "files_read", &args(json!({"path": path(&r, "docs/a.md")}))).await.unwrap();
        t.call("c3", "files_search", &args(json!({"path": path(&r, "docs"), "query": "alpha"}))).await.unwrap();
        t.call("c4", "files_read", &args(json!({"path": path(&r, "docs/big.txt")}))).await.unwrap();
        let asked: Vec<PathBuf> = r.cards.folders_asked.lock().unwrap().iter().map(|q| q.root.clone()).collect();
        assert_eq!(asked, [r.dir.path().join("other"), r.dir.path().join("docs")], "one card per folder");
        let ev: Vec<&str> = r.events.0.lock().unwrap().iter().map(|l| match l { Line::GrantRequested { .. } => "requested", Line::Granted { .. } => "granted", Line::Denied { .. } => "denied", Line::Revoked { .. } => "revoked", Line::Lapsed { .. } => "lapsed" }).collect();
        assert_eq!(ev, ["requested", "denied", "requested", "granted"]);
    }

    #[tokio::test]
    async fn a_revoke_takes_effect_at_the_next_call_mid_task() {
        let r = rig("host-revoke", profile::research());
        let t = tools(&r);
        r.cards.folder_answers.lock().unwrap().push(approved());
        let p1 = t.call("c1", "files_read", &args(json!({"path": path(&r, "docs/big.txt")}))).await.unwrap();
        assert_eq!(p1.content["more"], true);
        let next = p1.content["next_offset"].as_u64().unwrap();
        let gid = p1.receipt.grant_id.clone().unwrap();
        r.host.grants.lock().unwrap().revoke(&gid, "settings").unwrap().unwrap();
        let p2 = t.call("c2", "files_read", &args(json!({"path": path(&r, "docs/big.txt"), "offset": next}))).await.unwrap();
        assert_eq!(p2.content["error"], "not_granted", "page 2 after the revoke is refused");
        assert_eq!(r.cards.folders_asked.lock().unwrap().len(), 2, "and the folder is asked again");
    }

    #[tokio::test]
    async fn a_fetch_before_private_data_runs_and_after_it_waits_for_a_tap() {
        let r = rig("host-egress", profile::research());
        let t = tools(&r);
        let f1 = t.call("c1", "web_fetch", &args(json!({"url": "https://example.com/a"}))).await.unwrap();
        assert_eq!((f1.content["text"].as_str(), f1.receipt.outcome), (Some("public page"), Outcome::Ok));
        assert!(r.cards.fetches_asked.lock().unwrap().is_empty());
        t.call("c2", "memory_recall", &args(json!({"query": "name"}))).await.unwrap();
        assert!(t.private_seen());
        let f2 = t.call("c3", "web_fetch", &args(json!({"url": "https://example.com/b?q=Ana"}))).await.unwrap();
        assert_eq!((f2.content["error"].as_str(), f2.receipt.reason.as_deref()), (Some("declined"), Some("declined")));
        assert_eq!(r.cards.fetches_asked.lock().unwrap().as_slice(), ["https://example.com/b?q=Ana"], "the card shows the exact address");
        r.cards.fetch_answers.lock().unwrap().push(approved());
        let f3 = t.call("c4", "web_fetch", &args(json!({"url": "https://example.com/b?q=Ana"}))).await.unwrap();
        assert_eq!(f3.receipt.outcome, Outcome::Ok);
        assert_eq!(f3.receipt.target.as_deref(), Some("https://example.com/b?q=Ana"));
        let bad = t.call("c5", "web_fetch", &args(json!({"url": "http://example.com/"}))).await.unwrap();
        assert_eq!(bad.content["error"], "invalid_url");
    }

    #[tokio::test]
    async fn a_profile_without_a_tool_cannot_call_it_and_a_failing_receipt_gives_the_model_nothing() {
        let mut p = profile::research();
        p.profile.tools.retain(|t| t.name != "web.fetch");
        let r = rig("host-profile", p);
        let t = tools(&r);
        let a = t.call("c1", "web_fetch", &args(json!({"url": "https://example.com/"}))).await.unwrap();
        assert_eq!(a.content["error"], "unknown_tool");

        struct Broken;
        impl Receipts for Broken {
            fn record(&self, _r: &Receipt) -> std::io::Result<()> {
                Err(std::io::Error::other("disk full"))
            }
        }
        let r2 = rig("host-broken", profile::research());
        let mut host = Arc::try_unwrap(r2.host).ok().unwrap();
        host.receipts = Arc::new(Broken);
        let t2 = TaskTools::new(Arc::new(host), Task { task_id: "b1-t2".into(), text: "x".into(), base: None });
        assert!(t2.call("c1", "memory_recall", &args(json!({"query": "x"}))).await.is_err());
    }

    const PA: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn turn_on(r: &Rig, part: Arc<crate::parts::fake::Echo>) -> Arc<crate::parts::fake::Echo> {
        r.parts.insert(part.clone());
        r.parts.ledger().turn_on(&part.manifest.id, &part.digest, &crate::authority::UserTap { effect_id: "e-0".into(), args_sha256: "d".into() }).unwrap().unwrap();
        part
    }

    fn echo(asks: bool) -> Arc<crate::parts::fake::Echo> {
        crate::parts::fake::Echo::new("mcp.docs", PA, &[("search", profile::Effect::None), ("delete_page", profile::Effect::Irreversible)], asks)
    }

    #[tokio::test]
    async fn a_turned_on_part_offers_its_read_tools_and_each_call_has_a_receipt_naming_the_part() {
        let r = rig("host-part", profile::research());
        let echo = turn_on(&r, echo(false));
        let t = tools(&r);
        let d = t.declarations();
        let names: Vec<&str> = d.as_array().unwrap().iter().map(|x| x["function"]["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"mcp_docs__search") && names.contains(&"files_read"), "{names:?}");
        assert!(!names.contains(&"mcp_docs__delete_page"), "the irreversible tool is not offered");
        let no = t.call("c1", "mcp_docs__delete_page", &args(json!({"q": "x"}))).await.unwrap();
        assert_eq!((no.content["error"].as_str(), no.receipt.tool.as_str()), (Some("not_allowed"), "mcp.docs/delete_page"));
        let bad = t.call("c2", "mcp_docs__search", "[1]").await.unwrap();
        assert_eq!(bad.content["error"], "invalid_arguments");
        let failed = t.call("c3", "mcp_docs__search", &args(json!({"q": "fail"}))).await.unwrap();
        assert_eq!((failed.receipt.outcome, failed.content["error"].as_str()), (Outcome::Failed, Some("part_failed")));
        assert!(!t.private_seen(), "a call that failed brought nothing in");
        let a = t.call("c4", "mcp_docs__search", &args(json!({"q": "ports"}))).await.unwrap();
        assert_eq!(a.content["text"], "search: ports");
        let target = format!("mcp.docs@{PA}");
        assert_eq!((a.receipt.outcome, a.receipt.tool.as_str(), a.receipt.target.as_deref()), (Outcome::Ok, "mcp.docs/search", Some(target.as_str())));
        assert!(r.cards.parts_asked.lock().unwrap().is_empty(), "a tool marked allow needs no card");
        assert!(t.private_seen(), "a part's answer counts as private data");

        let want = crate::parts::call_sha256("mcp.docs", PA, "search", &json!({"q": "big"}));
        r.cards.part_answers.lock().unwrap().push(Decision::Approved { effect_id: "e-5".into(), args_sha256: want });
        let big = t.call("c5", "mcp_docs__search", &args(json!({"q": "big"}))).await.unwrap();
        assert_eq!(r.cards.parts_asked.lock().unwrap().len(), 1);
        assert_eq!((big.content["truncated"].as_bool(), big.content["total_chars"].as_u64()), (Some(true), Some(30_000)));
        assert_eq!(big.content["text"].as_str().unwrap().chars().count(), PART_MAX_CHARS);
        assert_eq!(echo.calls.lock().unwrap().len(), 3, "refused calls never reach the part");
        assert_eq!(r.receipts.all().iter().map(|x| x.call_seq).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn a_part_call_that_asks_runs_only_on_a_tap_bound_to_its_exact_arguments() {
        let r = rig("host-part-ask", profile::research());
        let echo = turn_on(&r, echo(true));
        let t = tools(&r);
        let declined = t.call("c1", "mcp_docs__search", &args(json!({"q": "one"}))).await.unwrap();
        assert_eq!(declined.content["error"], "declined");
        assert!(echo.calls.lock().unwrap().is_empty());
        let want = crate::parts::call_sha256("mcp.docs", PA, "search", &json!({"q": "two"}));
        r.cards.part_answers.lock().unwrap().push(Decision::Approved { effect_id: "e-2".into(), args_sha256: want.clone() });
        let ok = t.call("c2", "mcp_docs__search", &args(json!({"q": "two"}))).await.unwrap();
        assert_eq!(ok.content["text"], "search: two");
        {
            let asked = r.cards.parts_asked.lock().unwrap();
            assert_eq!(asked.len(), 2);
            assert_eq!((asked[1].args_sha256.as_str(), &asked[1].args, asked[1].digest.as_str()), (want.as_str(), &json!({"q": "two"}), PA));
        }
        r.cards.part_answers.lock().unwrap().push(Decision::Approved { effect_id: "e-3".into(), args_sha256: want });
        let wrong = t.call("c3", "mcp_docs__search", &args(json!({"q": "three"}))).await.unwrap();
        assert_eq!(wrong.content["error"], "approval_mismatch", "a tap for other arguments runs nothing");
        assert_eq!(echo.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn after_private_data_every_part_call_waits_and_a_part_turned_off_mid_task_refuses_the_next_call() {
        let r = rig("host-part-off", profile::research());
        turn_on(&r, echo(false));
        let t = tools(&r);
        t.call("c1", "memory_recall", &args(json!({"query": "name"}))).await.unwrap();
        let held = t.call("c2", "mcp_docs__search", &args(json!({"q": "Ana"}))).await.unwrap();
        assert_eq!(held.content["error"], "declined");
        assert!(r.cards.parts_asked.lock().unwrap()[0].why.contains("private"), "the arguments may carry what was read");
        r.parts.ledger().turn_off("mcp.docs", "settings").unwrap().unwrap();
        let off = t.call("c3", "mcp_docs__search", &args(json!({"q": "x"}))).await.unwrap();
        assert_eq!((off.content["error"].as_str(), off.receipt.tool.as_str()), (Some("part_off"), "mcp.docs/search"));
        r.parts.remove("mcp.docs");
        let gone = t.call("c4", "mcp_docs__search", &args(json!({"q": "x"}))).await.unwrap();
        assert_eq!(gone.content["error"], "unknown_tool");
        let mut p = profile::research();
        p.profile.parts = None;
        let r2 = rig("host-part-none", p);
        turn_on(&r2, echo(false));
        assert!(!tools(&r2).declarations().to_string().contains("mcp_docs"), "a profile without parts gets none");
    }

    #[tokio::test]
    async fn a_part_call_too_long_to_show_whole_on_its_card_is_refused_without_a_card() {
        let r = rig("host-part-long", profile::research());
        let echo = turn_on(&r, echo(true));
        let t = tools(&r);
        let long = "x".repeat(PART_CARD_MAX_CHARS);
        let a = t.call("c1", "mcp_docs__search", &args(json!({"q": long}))).await.unwrap();
        assert_eq!(a.content["error"], "arguments_too_long_to_confirm");
        assert!(r.cards.parts_asked.lock().unwrap().is_empty(), "no card for arguments it could not show");
        assert!(echo.calls.lock().unwrap().is_empty());
        let shown = "y".repeat(1500);
        t.call("c2", "mcp_docs__search", &args(json!({"q": shown}))).await.unwrap();
        assert_eq!(r.cards.parts_asked.lock().unwrap()[0].args, json!({"q": shown}), "the card carries the whole arguments");
    }

    #[tokio::test]
    async fn a_folder_blocked_after_boot_is_refused_at_the_next_read() {
        let r = rig("host-block-late", profile::research());
        let secret = r.dir.path().join("docs/keys");
        let mut host = Arc::try_unwrap(r.host).ok().unwrap();
        host.blocked = vec![secret.clone()];
        let t = TaskTools::new(Arc::new(host), Task { task_id: "b1-t3".into(), text: "x".into(), base: None });
        r.cards.folder_answers.lock().unwrap().push(approved());
        let first = t.call("c1", "files_read", &args(json!({"path": path_in(&r.dir, "docs/a.md")}))).await.unwrap();
        assert_eq!(first.receipt.outcome, Outcome::Ok);
        r.dir.write("docs/keys/k.txt", "secret\n");
        let late = t.call("c2", "files_read", &args(json!({"path": path_in(&r.dir, "docs/keys/k.txt")}))).await.unwrap();
        assert_eq!(late.content["error"], "blocked", "created after boot, blocked all the same");
    }

    fn path_in(d: &crate::testdir::TempDir, rel: &str) -> String {
        d.path().join(rel).to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn a_cancelled_call_has_its_own_receipt() {
        let r = rig("host-cancel", profile::research());
        let t = tools(&r);
        let rec = t.cancelled("c9", "files_read", "{}").unwrap();
        assert_eq!((rec.outcome, rec.call_seq, rec.tool.as_str()), (Outcome::Cancelled, 1, "files.read"));
        assert_eq!(r.receipts.all().len(), 1);
    }
}
