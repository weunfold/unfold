

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use rth_protocol::Event;
use rth_tasks::ledger::{Exec, Kind, Outcome, Receipt as DeliveryReceipt, TaskState, Update, INTERNAL_EXECUTOR};
use rth_tools::grants::{Grants, Line, Request};
use rth_tools::authority::{Layer, Parts, Standing, UserTap};
use rth_tools::host::{Approvals, Decision, Events, Guard, PartCall, Recall, Task, TaskTools, ToolHost, Verdict};
use rth_tools::parts::{Part, PartSet};
use rth_tools::receipt::{Receipt, Receipts};
use rth_tools::web::BoxFuture;
use rth_tools::worker::{self, End, Model, ModelTurn};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};

use crate::{Agent, effects, local};

pub const COMMAND: &str = "/research";

pub const GRANT: &str = "/grant";

pub const PART: &str = "/part";

pub fn enabled() -> bool {
    std::env::var("RTH_RESEARCH").ok().or_else(|| option_env!("RTH_BUILD_RESEARCH").map(str::to_string)).as_deref() == Some("1")
}

pub fn grants_path(journal: &Path) -> PathBuf {
    journal.parent().unwrap_or(Path::new(".")).join("grants.jsonl")
}

pub fn mcp_path(journal: &Path) -> PathBuf {
    journal.parent().unwrap_or(Path::new(".")).join("mcp.json")
}

pub fn parts_path(journal: &Path) -> PathBuf {
    journal.parent().unwrap_or(Path::new(".")).join("parts.jsonl")
}

pub fn is_command(text: &str) -> bool {
    let t = text.trim_start();
    [COMMAND, GRANT, PART].iter().any(|c| t.strip_prefix(c).is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace)))
}

#[cfg(test)]
pub static TEST_SETUP: Mutex<Option<HashMap<String, Setup>>> = Mutex::new(None);

pub type Setup = (Arc<dyn Model>, Arc<dyn rth_tools::web::Connect>);

pub fn setup(backend: &str, spend: Option<Arc<crate::flash_spend::Ledger>>, journal: &str) -> Option<Setup> {
    #[cfg(test)]
    if let Some(s) = TEST_SETUP.lock().unwrap().as_ref().and_then(|m| m.get(journal).cloned()) {
        return Some(s);
    }
    let _ = journal;
    if !enabled() {
        return None;
    }
    let hosted = || crate::flash::config_from_env().ok().and_then(Result::ok);
    let (cfg, within) = if backend.starts_with("local@") {
        (local::Config::from_env().ok(), Duration::from_secs(120))
    } else {
        match hosted() {
            Some(c) => (Some(c), Duration::from_secs(60)),
            None => (local::Config::from_env().ok(), Duration::from_secs(120)),
        }
    };
    let Some(cfg) = cfg else {
        eprintln!("rth-agentd: research off: no model endpoint for its worker");
        return None;
    };
    let spend = if local::dial(&cfg.authority).is_hosted() { spend } else { None };
    let ca = std::env::var("RTH_CA_FILE").ok().filter(|v| !v.is_empty());
    let connector = match crate::gemini::client_config(ca.as_deref()) {
        Ok(c) => Arc::new(rth_tools::web::TlsConnect::from_config(c)) as Arc<dyn rth_tools::web::Connect>,
        Err(e) => {
            eprintln!("rth-agentd: research off: no TLS roots for web.fetch ({e})");
            return None;
        }
    };
    Some((Arc::new(ChatModel { cfg, spend, within }), connector))
}

pub struct ChatModel {
    pub cfg: local::Config,
    pub spend: Option<Arc<crate::flash_spend::Ledger>>,

    pub within: Duration,
}

impl Model for ChatModel {
    fn turn<'a>(&'a self, messages: &'a [Value], tools: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>> {
        Box::pin(async move {
            let hosted = local::dial(&self.cfg.authority).is_hosted();

            let max_tokens = if hosted { crate::flash::SIDE_MAX_TOKENS } else { self.cfg.max_tokens.unwrap_or(local::DEFAULT_MAX_TOKENS as u32) };
            let mut body = json!({"model": self.cfg.model, "messages": messages, "stream": false, "max_tokens": max_tokens, "temperature": if hosted { 1.0 } else { 0.0 }});
            if tools.as_array().is_some_and(|a| !a.is_empty()) {
                body["tools"] = tools.clone();
                body["tool_choice"] = json!("auto");
            }

            match local::dial(&self.cfg.authority) {
                local::Dial::Hosted(_) => crate::route::Level::Off.deepseek_fields(&mut body),
                local::Dial::Keyed(_) => body["reasoning_effort"] = json!(crate::route::Level::Off.reasoning_effort()),
                local::Dial::Loopback => body["chat_template_kwargs"] = json!({"enable_thinking": false}),
            }
            let reservation = match (&self.spend, hosted) {
                (Some(l), true) => {
                    let (l, bytes) = (l.clone(), body.to_string().len());
                    let attempt = format!("research-{}", rth_tools::utc_ms());
                    match tokio::task::spawn_blocking(move || l.admit(&attempt, bytes)).await {
                        Ok(Ok(r)) => Some(r),
                        Ok(Err(why)) => return Err(format!("spend: {why}")),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                (None, true) => return Err("spend: no ledger for a hosted call".into()),
                _ => None,
            };
            let path = format!("{}/chat/completions", self.cfg.base_path);
            let answer = local::post_json(&self.cfg.authority, &path, &body, self.within).await;
            let parsed = match &answer {
                Ok((200, bytes)) => serde_json::from_slice::<Value>(bytes).map_err(|e| format!("bad JSON from the model: {e}")),
                Ok((status, bytes)) => Err(format!("model answered {status}: {}", String::from_utf8_lossy(&bytes[..bytes.len().min(300)]))),
                Err(f) => Err(f.detail.clone()),
            };
            if let (Some(l), Some(r)) = (&self.spend, reservation) {
                let usage = parsed.as_ref().ok().map(|v| v["usage"].clone());
                let l = l.clone();
                let _ = tokio::task::spawn_blocking(move || l.settle(&r, usage.as_ref())).await;
            }
            parse_turn(&parsed?)
        })
    }
}

pub fn parse_turn(v: &Value) -> Result<ModelTurn, String> {
    let choice = &v["choices"][0];
    if choice.is_null() {
        return Err("the model answered no choice".into());
    }
    let m = &choice["message"];
    let tool_calls = m["tool_calls"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| worker::Call {
                    id: c["id"].as_str().unwrap_or_default().to_string(),
                    name: c["function"]["name"].as_str().unwrap_or_default().to_string(),
                    arguments: match &c["function"]["arguments"] {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    },
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ModelTurn {
        content: m["content"].as_str().unwrap_or_default().to_string(),
        tool_calls,
        finish: choice["finish_reason"].as_str().unwrap_or("stop").to_string(),
        usage: v.get("usage").filter(|u| !u.is_null()).cloned(),
    })
}

pub struct Bridge {
    agent: OnceLock<Weak<Agent>>,
    tasks: Arc<crate::tasks::Tasks>,
    waiters: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
    out: mpsc::UnboundedSender<Event>,
}

impl Bridge {
    fn agent(&self) -> Option<Arc<Agent>> {
        self.agent.get().and_then(Weak::upgrade)
    }

    async fn card(&self, kind: &'static str, rule: &'static str, held: effects::Held, digest: String, summary: String, why: String) -> Decision {
        let Some(agent) = self.agent() else { return Decision::Declined("no_agent".into()) };
        let effect_id = agent.new_id("e");
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().unwrap().insert(effect_id.clone(), tx);
        struct Retire<'a>(&'a Bridge, String);
        impl Drop for Retire<'_> {
            fn drop(&mut self) {
                self.0.waiters.lock().unwrap().remove(&self.1);
                if let Some(agent) = self.0.agent() {
                    agent.effects.lock().unwrap().retire_id(&self.1);
                }
            }
        }
        let _retire = Retire(self, effect_id.clone());
        let (summary, why) = card_texts(&summary, &why);
        let p = agent.effects.lock().unwrap().hold(&effect_id, kind, held, digest.clone(), summary, why, crate::now_utc_ms());
        let event = Event::EffectHeld { effect_id: effect_id.clone(), effect_kind: kind.into(), rule: rule.into(), at: None, args_sha256: digest, token_sha256: effects::sha256_hex(p.token()), expires_utc_ms: p.expires_utc_ms };
        agent.emit(event, None).await;
        crate::send_card(&agent, None, &effect_id, &p).await;
        match tokio::time::timeout(effects::TTL, rx).await {
            Ok(Ok(d)) => d,
            Ok(Err(_)) => Decision::Declined("cancelled".into()),
            Err(_) => Decision::Declined("expired".into()),
        }
    }

    pub fn answer(&self, effect_id: &str, d: Decision) -> bool {
        match self.waiters.lock().unwrap().remove(effect_id) {
            Some(tx) => tx.send(d).is_ok(),
            None => false,
        }
    }
}

impl Approvals for Bridge {
    fn folder<'a>(&'a self, task_id: &'a str, request: &'a Request) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            let root = request.root.display().to_string();
            let digest = rth_tools::canonical_sha256(&json!({"kind": "folder_grant", "scope": "read", "root": root, "task_id": task_id, "request_id": request.request_id}));
            let held = effects::Held::FolderGrant { request_id: request.request_id.clone(), task_id: task_id.to_string(), root: root.clone() };
            let summary = format!("Let Unfold read the folder {root} and its subfolders?");
            self.card("folder_grant", "folder.ask", held, digest, summary, format!("a task needs it ({})", request.reason)).await
        })
    }

    fn fetch<'a>(&'a self, task_id: &'a str, url: &'a str, why: &'a str) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            let digest = rth_tools::canonical_sha256(&json!({"kind": "web_fetch", "url": url, "task_id": task_id}));
            let held = effects::Held::WebFetch { task_id: task_id.to_string(), url: url.to_string() };
            self.card("web_fetch", "egress.after_private", held, digest, format!("Let Unfold open {url}?"), why.to_string()).await
        })
    }

    fn part<'a>(&'a self, task_id: &'a str, call: &'a PartCall) -> BoxFuture<'a, Decision> {
        self.part_card(task_id, call)
    }
}

impl Bridge {
    fn part_card<'a>(&'a self, task_id: &'a str, call: &'a PartCall) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            let held = effects::Held::PartCall { task_id: task_id.to_string(), part_id: call.part_id.clone(), tool: call.tool.clone(), args: call.args.clone() };
            self.card("part_call", "part.ask", held, call.args_sha256.clone(), part_call_summary(call), call.why.clone()).await
        })
    }
}

pub fn card_texts(summary: &str, why: &str) -> (String, String) {
    (rth_tools::visible(summary), rth_tools::visible(why))
}

pub fn part_call_summary(call: &PartCall) -> String {

    format!("Let Unfold call {} on {} with {}?", call.tool, call.title, call.args)
}

impl Events for Bridge {
    fn grant_event(&self, line: &Line) {
        let record = serde_json::to_value(line).unwrap_or(Value::Null);
        let event = record["event"].as_str().unwrap_or_default().to_string();
        let _ = self.out.send(Event::FolderGrant { event, record });
    }
}

impl Receipts for Bridge {
    fn record(&self, r: &Receipt) -> std::io::Result<()> {
        let v = serde_json::to_value(r).map_err(std::io::Error::other)?;
        match self.tasks.tool_call_receipt(&r.task_id, &v)? {
            Ok(()) => {
                let _ = self.out.send(Event::TaskToolCall { task_id: r.task_id.clone(), receipt: v });
                Ok(())
            }
            Err(why) => Err(std::io::Error::other(format!("receipt refused: {why}"))),
        }
    }
}

struct MemoryRecall(Arc<crate::memory::Memory>, Arc<crate::tasks::Tasks>);

impl Recall for MemoryRecall {
    fn recall<'a>(&'a self, query: &'a str, task_id: &'a str) -> BoxFuture<'a, Result<Value, String>> {
        let (m, tasks, q, t) = (self.0.clone(), self.1.clone(), query.to_string(), task_id.to_string());
        Box::pin(async move {
            let r = tokio::task::spawn_blocking(move || {

                let asked_by = tasks.origin_speaker(&t).unwrap_or(Some(crate::people::speaker_tag(&rth_protocol::TurnSpeaker::Uncertain)));
                crate::people::tag_is_owner(asked_by.as_deref(), &crate::people::records(&m)).then(|| m.recall_for(&q, &t))
            })
            .await
            .map_err(|e| e.to_string())?;
            let Some(r) = r else { return Ok(json!({"status": "skipped", "reason": "not_owner", "memories": []})) };
            let items: Vec<Value> = r.items.iter().map(|i| json!({"memory_id": i.memory_id, "revision": i.revision, "text": i.text})).collect();
            Ok(json!({"status": r.receipt["status"], "memories": items}))
        })
    }
}

struct FetchGuard;

impl Guard for FetchGuard {
    fn fetch(&self, task_text: &str, data: &[String], url: &str) -> Verdict {
        let prov = crate::effect_guard::Provenance { person: task_text, earlier: &[], data, person_name: None, cause: crate::effect_guard::Cause::Person };
        match crate::effect_guard::check(&crate::effect_guard::Call { tool: "search_web", target: Some(url), content: None }, &prov) {
            crate::effect_guard::Verdict::Allow => Verdict::Allow,
            crate::effect_guard::Verdict::Hold { rule, .. } => Verdict::Hold(rule.to_string()),
            crate::effect_guard::Verdict::Refuse { rule, .. } => Verdict::Refuse(rule.to_string()),
        }
    }
}

pub struct Research {
    pub bridge: Arc<Bridge>,
    pub host: Arc<ToolHost>,
    model: Arc<dyn Model>,
    running: Mutex<HashMap<String, watch::Sender<bool>>>,
    out_rx: Mutex<Option<mpsc::UnboundedReceiver<Event>>>,

    pub parts: Arc<PartSet>,
    mcp_file: PathBuf,

    servers: Mutex<BTreeMap<String, Server>>,
    config_error: Mutex<Option<String>>,
}

#[derive(Debug, Clone)]
struct Server {
    title: String,

    status: &'static str,
    error: Option<String>,
    config: rth_mcp::ServerConfig,
}

pub fn hard_blocks(home: Option<&Path>, state: &[PathBuf]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = state.to_vec();
    if let Some(h) = home {
        for rel in [
            ".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker", ".cargo", ".config/gcloud", ".config/gh", ".local/share/rth", "Library/Accounts", "Library/Preferences", "Library/Keychains", "Library/Cookies", "Library/Application Support",
            "Library/Containers", "Library/Group Containers", "Library/Mail", "Library/Messages", "Library/Safari",
        ] {
            v.push(h.join(rel));
        }
    }
    v
}

impl Research {

    #[allow(clippy::too_many_arguments)]
    pub fn open(journal: &Path, boot_id: &str, tasks: Arc<crate::tasks::Tasks>, memory: Option<Arc<crate::memory::Memory>>, model: Arc<dyn Model>, home: Option<PathBuf>, state: &[PathBuf], connector: Arc<dyn rth_tools::web::Connect>) -> std::io::Result<Research> {
        let (grants, lapsed) = Grants::open(&grants_path(journal), boot_id)?;
        let parts = Arc::new(PartSet::new(Parts::open(&parts_path(journal))?, Layer::up_to(rth_tools::profile::Effect::None)));
        let (out, out_rx) = mpsc::unbounded_channel();
        let bridge = Arc::new(Bridge { agent: OnceLock::new(), tasks: tasks.clone(), waiters: Mutex::new(HashMap::new()), out });
        for l in &lapsed {
            bridge.grant_event(l);
        }
        tasks.set_internal(true);
        let blocked = hard_blocks(home.as_deref(), state);
        let host = Arc::new(ToolHost {
            profile: rth_tools::profile::research(),
            grants: Arc::new(Mutex::new(grants)),
            blocked,
            home,
            resolver: Arc::new(rth_tools::web::SystemResolver),
            connector,
            fetch: rth_tools::web::FetchConfig::default(),
            approvals: bridge.clone(),
            recall: memory.map(|m| Arc::new(MemoryRecall(m, tasks.clone())) as Arc<dyn Recall>),
            receipts: bridge.clone(),
            events: bridge.clone(),
            guard: Some(Arc::new(FetchGuard)),
            parts: Some(parts.clone()),
        });
        Ok(Research { bridge, host, model, running: Mutex::new(HashMap::new()), out_rx: Mutex::new(Some(out_rx)), parts, mcp_file: mcp_path(journal), servers: Mutex::new(BTreeMap::new()), config_error: Mutex::new(None) })
    }

    fn set_status(&self, id: &str, status: &'static str, error: Option<String>) {
        if let Some(s) = self.servers.lock().unwrap().get_mut(id) {
            s.status = status;
            s.error = error;
        }
    }

    pub fn parts_json(&self) -> Value {
        let servers: Vec<Value> = self
            .servers
            .lock()
            .unwrap()
            .iter()
            .map(|(id, s)| {
                let live = self.parts.get(id);
                let approved = self.parts.ledger().on().into_iter().any(|(p, _)| p == *id);
                let standing = match live.as_ref().map(|p| self.parts.ledger().standing(id, p.digest())) {
                    Some(Standing::On { .. }) => "on",
                    Some(Standing::Changed { .. }) => "changed",
                    _ if approved && s.status == "changed" => "changed",
                    _ if approved => "on",
                    _ => "off",
                };
                let tools: Vec<Value> = live.as_ref().map(|p| p.manifest().tools.iter().map(|t| json!({"name": t.name, "effect": t.effect})).collect()).unwrap_or_default();
                json!({"part_id": id, "title": s.title, "status": s.status, "standing": standing, "digest": live.as_ref().map(|p| p.digest().to_string()), "tools": tools, "error": s.error})
            })
            .collect();
        json!({"servers": servers, "error": *self.config_error.lock().unwrap()})
    }

    pub fn grants_json(&self) -> Value {
        serde_json::to_value(self.host.grants.lock().unwrap().list()).unwrap_or(json!([]))
    }
}

pub fn attach(agent: &Arc<Agent>) {
    let Some(r) = agent.research.as_ref() else { return };
    let _ = r.bridge.agent.set(Arc::downgrade(agent));
    let Some(mut rx) = r.out_rx.lock().unwrap().take() else { return };
    let weak = Arc::downgrade(agent);
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let Some(agent) = weak.upgrade() else { return };
            let changed = matches!(&event, Event::FolderGrant { event, .. } if event == "granted" || event == "revoked");
            agent.emit(event, None).await;
            if changed {
                publish_grants(&agent, None).await;
            }
        }
    });
    tokio::spawn(start_parts(Arc::downgrade(agent)));
}

async fn start_parts(agent: Weak<Agent>) {
    let Some(a) = agent.upgrade() else { return };
    let Some(r) = a.research.clone() else { return };
    let file = match std::fs::read_to_string(&r.mcp_file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => Err(format!("mcp.json: {e}")),
        Ok(text) => rth_mcp::parse(&text),
    };
    let servers = match file {
        Ok(f) => f.servers,
        Err(e) => {
            *r.config_error.lock().unwrap() = Some(e);
            return publish_parts(&a, None).await;
        }
    };
    let approved: Vec<String> = r.parts.ledger().on().into_iter().map(|(id, _)| id).collect();
    for s in servers {
        let id = s.part_id();
        let start = s.enabled && approved.contains(&id);
        let status = if !s.enabled { "disabled" } else if start { "connecting" } else { "off" };
        r.servers.lock().unwrap().insert(id.clone(), Server { title: s.title.clone().unwrap_or_else(|| s.id.clone()), status, error: None, config: s });
        if start {
            tokio::spawn(start_part(agent.clone(), id, None));
        }
    }
    publish_parts(&a, None).await;
}

async fn start_part(agent: Weak<Agent>, id: String, tap: Option<UserTap>) {
    let Some(a) = agent.upgrade() else { return };
    let Some(r) = a.research.clone() else { return };
    let Some(config) = r.servers.lock().unwrap().get(&id).map(|s| s.config.clone()) else { return };
    r.set_status(&id, "connecting", None);
    publish_parts(&a, None).await;
    let started = rth_mcp::connect(&config, &|k| std::env::var(k).ok()).await;
    let (status, error) = match started {
        Err(e) => ("failed", Some(e)),
        Ok(p) => {
            let digest = p.digest().to_string();
            match tap {
                Some(tap) => match r.parts.ledger().turn_on(&id, &digest, &tap) {
                    Ok(Ok(_)) => {
                        r.parts.insert(Arc::new(p));
                        ("connected", None)
                    }
                    Ok(Err(why)) => ("failed", Some(why.to_string())),
                    Err(e) => ("failed", Some(format!("the parts log write failed: {e}"))),
                },
                None => {
                    let on = r.parts.ledger().on().into_iter().any(|(pid, d)| pid == id && d == digest);
                    if on {
                        r.parts.insert(Arc::new(p));
                        ("connected", None)
                    } else {
                        ("changed", Some("it changed since you turned it on; turn it on again to approve it".to_string()))
                    }
                }
            }
        }
    };
    if let (Some(e), true) = (&error, status == "failed") {
        a.emit(Event::Rejected { reason: format!("part {id}: {e}") }, None).await;
    }
    r.set_status(&id, status, error);
    publish_parts(&a, None).await;
}

pub async fn publish_parts(agent: &Arc<Agent>, to: Option<u64>) {
    let Some(r) = agent.research.as_ref() else { return };
    agent.emit_to(to, Event::PartState { parts: r.parts_json() }, None).await;
}

pub async fn publish_grants(agent: &Arc<Agent>, to: Option<u64>) {
    let Some(r) = agent.research.as_ref() else { return };
    agent.emit_to(to, Event::FolderGrants { grants: r.grants_json() }, None).await;
}

fn is_research(rec: &TaskState) -> bool {
    rec.assignment.executor_id == INTERNAL_EXECUTOR && rec.args["purpose"] == "research"
}

pub fn on_records(agent: &Arc<Agent>, recs: &[TaskState]) {
    let Some(r) = agent.research.as_ref() else { return };
    for rec in recs.iter().filter(|r| r.outcome == Outcome::Applied && is_research(r)) {
        match rec.execution {
            Exec::OutputSent => {
                let mut running = r.running.lock().unwrap();
                if running.contains_key(&rec.task_id) {
                    continue;
                }
                let (tx, rx) = watch::channel(false);
                running.insert(rec.task_id.clone(), tx);
                tokio::spawn(run_task(agent.clone(), rec.clone(), rx));
            }
            Exec::CancelRequested => {
                if let Some(tx) = r.running.lock().unwrap().get(&rec.task_id) {
                    let _ = tx.send(true);
                }
            }
            _ => {}
        }
    }
}

async fn run_task(agent: Arc<Agent>, rec: TaskState, mut cancel: watch::Receiver<bool>) {
    let (Some(r), Some(tasks)) = (agent.research.clone(), agent.tasks.clone()) else { return };
    let me = tasks.internal();
    let id = rec.task_id.clone();
    let seq = std::sync::atomic::AtomicU64::new(0);
    let send = |kind: Kind, owner: bool| {
        let (tasks, agent, id, me) = (tasks.clone(), agent.clone(), id.clone(), me.clone());
        let n = if owner { 1 } else { seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 };
        async move {
            let u = Update { task_id: id, executor_id: me.executor_id, incarnation: me.incarnation, seq: n, pub_epoch: owner.then_some(1), request_id: None, kind };
            match tokio::task::spawn_blocking(move || tasks.update_typed(&u)).await {
                Ok(Ok(Some(rec))) => {
                    crate::publish_task_records(&agent, vec![rec.clone()], None).await;
                    Some(rec)
                }
                Ok(Err(e)) => {
                    agent.emit(Event::Log { level: "error".into(), msg: format!("research task update failed: {e}") }, None).await;
                    None
                }
                _ => None,
            }
        }
    };
    let task_text = rec.args["task"].as_str().unwrap_or_default().to_string();
    let claimed = send(Kind::Claim, false).await;
    if claimed.as_ref().map(|c| (c.outcome, c.execution)) != Some((Outcome::Applied, Exec::Claimed)) {
        r.running.lock().unwrap().remove(&id);
        return;
    }
    let input_sha256 = rth_tools::sha256_hex(task_text.as_bytes());
    send(Kind::Running { child_id: rec.child_id.clone(), input_sha256 }, false).await;
    let base = rec.args["folder"].as_str().map(|f| match (f.strip_prefix("~"), &r.host.home) {
        (Some(rest), Some(h)) => h.join(rest.trim_start_matches('/')),
        _ => PathBuf::from(f),
    });
    let tools = TaskTools::new(r.host.clone(), Task { task_id: id.clone(), text: task_text, base });
    let report = worker::run(&tools, r.model.as_ref(), &mut cancel).await;
    let summary = json!({"turns": report.turns, "tool_calls": report.tool_calls, "invalid_args": report.invalid_args, "loop_steers": report.loop_steers, "max_turn_hit": report.max_turn_hit});
    agent.emit(Event::Log { level: "info".into(), msg: format!("research {id}: {summary}") }, None).await;
    match report.end {
        End::Returned { text } => {
            let digest = rth_tools::sha256_hex(text.as_bytes());
            let returned = send(Kind::Returned { result_sha256: digest.clone(), chars: text.chars().count() as u64 }, false).await;
            if returned.is_some_and(|t| t.execution == Exec::Returned)
                && send(Kind::PublishRequest { result_sha256: digest.clone() }, false).await.is_some_and(|t| t.publication == rth_tasks::ledger::Publication::Committed)
            {
                let env = agent.emit(Event::TaskReport { task_id: id.clone(), text, result_sha256: digest.clone() }, None).await;
                let instance = agent.identity.instance.as_ref().map(|i| i.instance_id.clone()).unwrap_or_default();
                let receipt = DeliveryReceipt { task_id: id.clone(), key: rec.child_id.clone(), seq: 1, result_sha256: digest, session_lineage: format!("agentd-journal:{instance}") };
                send(Kind::Delivered { chat_ref: json!({"journal_event": env.id}), receipt }, true).await;
            }
            send(Kind::ExecutionDone, false).await;
        }
        End::Failed { reason } => {
            send(Kind::Failed { reason }, false).await;
        }
        End::Cancelled => {
            send(Kind::Cancelled, false).await;
        }
    }
    r.running.lock().unwrap().remove(&id);
}

pub async fn command(agent: &Arc<Agent>, conn: u64, cause: String, command_id: String, text: String, speaker: rth_protocol::TurnSpeaker) {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix(GRANT) {
        return grant_command(agent, conn, cause, rest.trim()).await;
    }
    if let Some(rest) = t.strip_prefix(PART) {
        return part_command(agent, conn, cause, rest.trim()).await;
    }
    let task = t.strip_prefix(COMMAND).unwrap_or_default().trim().to_string();
    let reject = |why: String| Event::Rejected { reason: why };
    let (Some(tasks), Some(instance)) = (agent.tasks.clone(), agent.identity.instance.as_ref().map(|i| i.instance_id.clone())) else {
        agent.emit_to(Some(conn), reject("research is off".into()), Some(cause)).await;
        return;
    };
    if agent.research.is_none() {
        agent.emit_to(Some(conn), reject("research is off".into()), Some(cause)).await;
        return;
    }
    let asked_by = crate::turn_who(agent, &speaker).await.tag;
    let args = json!({"task": task});
    match tokio::task::spawn_blocking(move || tasks.typed_call(&instance, &command_id, "research", &args, Some(&asked_by))).await {
        Ok(Ok((output, recs))) => {
            if recs.is_empty() && output["status"] != "accepted" {
                agent.emit_to(Some(conn), reject(format!("research: {}", output["reason"].as_str().unwrap_or("unavailable"))), Some(cause)).await;
            } else {
                crate::publish_task_records(agent, recs, Some(cause)).await;
            }
        }
        Ok(Err(e)) => agent.emit_to(Some(conn), reject(format!("research: the task ledger write failed: {e}")), Some(cause)).await.id.clear(),
        Err(_) => {}
    }
}

async fn grant_command(agent: &Arc<Agent>, conn: u64, cause: String, rest: &str) {
    let Some(r) = agent.research.clone() else {
        agent.emit_to(Some(conn), Event::Rejected { reason: "research is off".into() }, Some(cause)).await;
        return;
    };
    let mut parts = rest.split_whitespace();
    match (parts.next(), parts.next(), parts.next()) {
        (Some("list"), None, None) => publish_grants(agent, Some(conn)).await,
        (Some("revoke"), Some(id), None) => {
            let out = r.host.grants.lock().unwrap().revoke(id, "settings");
            match out {
                Ok(Ok((_, line))) => r.bridge.grant_event(&line),
                Ok(Err(why)) => agent.emit_to(Some(conn), Event::Rejected { reason: format!("grant revoke: {why}") }, Some(cause)).await.id.clear(),
                Err(e) => agent.emit_to(Some(conn), Event::Rejected { reason: format!("grant revoke: the grant log write failed: {e}") }, Some(cause)).await.id.clear(),
            }
        }
        _ => agent.emit_to(Some(conn), Event::Rejected { reason: "usage: /grant list | /grant revoke <grant_id>".into() }, Some(cause)).await.id.clear(),
    }
}

async fn part_command(agent: &Arc<Agent>, conn: u64, cause: String, rest: &str) {
    let Some(r) = agent.research.clone() else {
        agent.emit_to(Some(conn), Event::Rejected { reason: "research is off".into() }, Some(cause)).await;
        return;
    };
    let reject = |why: String| Event::Rejected { reason: why };
    let mut words = rest.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (Some("list"), None, None) => publish_parts(agent, Some(conn)).await,
        (Some("on"), Some(id), None) => {
            let Some(server) = r.servers.lock().unwrap().get(id).cloned() else {
                agent.emit_to(Some(conn), reject(format!("part on: {id} is not in mcp.json")), Some(cause)).await;
                return;
            };
            if server.status == "disabled" {
                agent.emit_to(Some(conn), reject(format!("part on: {id} is disabled in mcp.json")), Some(cause)).await;
                return;
            }
            if server.status == "connected" || server.status == "connecting" {
                agent.emit_to(Some(conn), reject(format!("part on: {id} is already on")), Some(cause)).await;
                return;
            }
            let c = &server.config;
            let what = match &c.transport {
                rth_mcp::Transport::Stdio { command, args, .. } => format!("starts {} on this computer", std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")),
                rth_mcp::Transport::StreamableHttp { url, .. } => format!("connects to {url}"),
            };
            let marked: Vec<String> = c.tools.keys().map(|name| format!("{name} ({}{})", serde_json::to_value(c.effect(name)).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), if c.asks(name) { ", asks each time" } else { "" })).collect();
            let marked = if marked.is_empty() { "none".to_string() } else { marked.join(", ") };
            let summary = format!("Turn on {}? It {what}. Tools you marked: {marked}. Every other tool it offers counts as irreversible and asks each time.", server.title);
            let why = if server.status == "changed" { "it changed since you turned it on".to_string() } else { "it is in mcp.json".to_string() };
            let card_digest = rth_tools::canonical_sha256(&json!({"kind": "part_on", "part_id": id, "config": c}));
            let held = effects::Held::PartOn { part_id: id.to_string(), digest: card_digest.clone() };
            let (weak, id) = (Arc::downgrade(agent), id.to_string());
            tokio::spawn(async move {
                let Some(r) = weak.upgrade().and_then(|a| a.research.clone()) else { return };
                if let Decision::Approved { effect_id, args_sha256 } = r.bridge.card("part_on", "part.turn_on", held, card_digest, summary, why).await {
                    start_part(weak, id, Some(UserTap { effect_id, args_sha256 })).await;
                }
            });
        }
        (Some("off"), Some(id), None) => {
            let off = r.parts.ledger().turn_off(id, "person");
            match off {
                Ok(Ok(_)) => {

                    r.parts.remove(id);
                    r.set_status(id, "off", None);
                    publish_parts(agent, None).await
                }
                Ok(Err(why)) => agent.emit_to(Some(conn), reject(format!("part off: {why}")), Some(cause)).await.id.clear(),
                Err(e) => agent.emit_to(Some(conn), reject(format!("part off: the parts log write failed: {e}")), Some(cause)).await.id.clear(),
            }
        }
        _ => agent.emit_to(Some(conn), reject("usage: /part list | /part on <part_id> | /part off <part_id>".into()), Some(cause)).await.id.clear(),
    }
}

pub fn answer_card(agent: &Agent, effect_id: &str, p: &effects::Pending, approved: bool) -> Option<Value> {
    let r = agent.research.as_ref()?;
    let d = if approved { Decision::Approved { effect_id: effect_id.to_string(), args_sha256: p.args_sha256.clone() } } else { Decision::Declined("person".into()) };
    r.bridge.answer(effect_id, d).then(|| match &p.call {
        effects::Held::FolderGrant { request_id, root, .. } => json!({"request_id": request_id, "root": root, "approved": approved}),
        effects::Held::WebFetch { url, .. } => json!({"url": url, "approved": approved}),
        effects::Held::PartOn { part_id, digest } => json!({"part_id": part_id, "digest": digest, "approved": approved}),
        effects::Held::PartCall { part_id, tool, .. } => json!({"part_id": part_id, "tool": tool, "approved": approved}),
        _ => json!({"approved": approved}),
    })
}

#[cfg(test)]
mod tests;
