

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::schema::{self, Refusal, Tool};

pub const START_MS: u64 = 5_000;

pub const RUN_MS: u64 = 90_000;

pub const TEARDOWN_MS: u64 = 10_000;

pub const RATE_PER_HOUR: usize = 10;

pub const RESEARCH_RUN_MS: u64 = 600_000;

pub const INTERNAL_EXECUTOR: &str = "agentd";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Incarnation {
    pub launch_id: String,
    pub pid: u32,
    pub plugin_instance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub executor_id: String,
    pub incarnation: Incarnation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct CallKey {
    pub instance_id: String,
    pub provider_generation: u64,
    pub call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exec {
    Refused,
    Accepted,
    OutputSent,
    Claimed,
    Running,
    Returned,
    ExecutionDone,
    StartRevoked,
    StartUncertain,
    CancelRequested,
    Cancelled,
    TeardownUnknown,
    Unknown,
    SettledExited,
    SettledFailed,
}

impl Exec {

    pub fn holds_slot(self) -> bool {
        matches!(
            self,
            Exec::Accepted
                | Exec::OutputSent
                | Exec::Claimed
                | Exec::Running
                | Exec::Returned
                | Exec::StartUncertain
                | Exec::CancelRequested
                | Exec::TeardownUnknown
                | Exec::Unknown
        )
    }

    fn settled(self) -> bool {
        !self.holds_slot()
    }

    fn cancellable(self) -> bool {
        matches!(self, Exec::Accepted | Exec::OutputSent | Exec::Claimed | Exec::Running | Exec::Returned | Exec::StartUncertain | Exec::Unknown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Publication {
    None,
    Committed,
    Delivered,
    DeliveryFailed,
    Unknown,
    VisibleUnconfirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    Duplicate,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Update {
    pub task_id: String,
    pub executor_id: String,
    pub incarnation: Incarnation,

    #[serde(default)]
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pub_epoch: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {

    Claim,
    Running { child_id: String, input_sha256: String },
    Returned { result_sha256: String, chars: u64 },
    PublishRequest { result_sha256: String },
    Cancelled,
    TeardownUnknown,
    Failed { reason: String },
    ExecutionDone,

    Delivered { chat_ref: Value, receipt: Receipt },
    DeliveryFailed { reason: String },
    VisibleUnconfirmed { seq: u64 },

    VisibleObserved { seq: u64, result_sha256: String },
    PublicationTakeover { expected_owner: Incarnation, expected_epoch: u64, result_sha256: String },
    CancelIntent { by: String },
    Abandon,
}

impl Kind {
    fn class(&self) -> Class {
        match self {
            Kind::Claim
            | Kind::Running { .. }
            | Kind::Returned { .. }
            | Kind::PublishRequest { .. }
            | Kind::Cancelled
            | Kind::TeardownUnknown
            | Kind::Failed { .. }
            | Kind::ExecutionDone => Class::Exec,
            Kind::Delivered { .. } | Kind::DeliveryFailed { .. } | Kind::VisibleUnconfirmed { .. } => Class::Owner,
            _ => Class::Request,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Kind::Claim => "claim",
            Kind::Running { .. } => "running",
            Kind::Returned { .. } => "returned",
            Kind::PublishRequest { .. } => "publish_request",
            Kind::Cancelled => "cancelled",
            Kind::TeardownUnknown => "teardown_unknown",
            Kind::Failed { .. } => "failed",
            Kind::ExecutionDone => "execution_done",
            Kind::Delivered { .. } => "delivered",
            Kind::DeliveryFailed { .. } => "delivery_failed",
            Kind::VisibleUnconfirmed { .. } => "visible_unconfirmed",
            Kind::VisibleObserved { .. } => "visible_observed",
            Kind::PublicationTakeover { .. } => "publication_takeover",
            Kind::CancelIntent { .. } => "cancel_intent",
            Kind::Abandon => "abandon",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Exec,
    Owner,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub task_id: String,
    pub key: String,
    pub seq: u64,
    pub result_sha256: String,

    pub session_lineage: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskState {
    pub task_id: String,
    pub rev: u64,
    pub execution: Exec,
    pub publication: Publication,
    pub trigger: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub call_key: CallKey,
    pub args_sha256: String,
    pub args: Value,
    pub assignment: Assignment,
    pub child_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pub_owner: Option<Incarnation>,
    #[serde(default)]
    pub pub_epoch: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<String>,
    #[serde(default)]
    pub after_commit: bool,
    #[serde(default)]
    pub abandoned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateRef>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateRef {
    pub kind: String,
    pub incarnation: Incarnation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pub_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rec", rename_all = "snake_case")]
enum Line {
    Task(TaskState),

    Call { call_key: CallKey, args_sha256: String, output: Value, task_id: Option<String> },

    Launch { launch_id: String, secret_sha256: String },

    Exit { launch_id: String, code: Option<i32>, signal: Option<String> },

    Current { incarnation: Incarnation },

    ToolCall { task_id: String, receipt: Value },
}

#[derive(Default)]
struct Timers {
    start_by: Option<u64>,
    running_by: Option<u64>,
    run_deadline: Option<u64>,
    teardown_by: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallAnswer {
    pub output: Value,
    pub task_id: Option<String>,
    pub record: Option<TaskState>,
}

pub struct Lease {
    pub incarnation: Incarnation,
    pub executor_id: String,
    pub available: bool,
    pub expires_at: u64,
    seq: u64,
}

pub struct Ledger {
    path: PathBuf,
    file: std::fs::File,
    boot: String,
    instance_id: String,
    poisoned: Option<String>,
    enabled: bool,
    next_n: u64,
    tasks: BTreeMap<String, TaskState>,

    order: Vec<String>,
    calls: HashMap<CallKey, (String, Value, Option<String>)>,

    seen_seq: HashMap<(String, u64, u64), (String, TaskState)>,

    seen_req: HashMap<(String, Incarnation, String), (String, TaskState)>,
    last_exec_seq: HashMap<String, u64>,
    last_pub_seq: HashMap<(String, u64), u64>,
    launches: HashMap<String, String>,
    exited: HashSet<String>,
    current: Option<Incarnation>,
    superseded: HashSet<Incarnation>,
    lease: Option<Lease>,
    timers: HashMap<String, Timers>,
    accepted_at: Vec<u64>,

    internal_on: bool,

    receipts: HashMap<String, Vec<Value>>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Recovered {
    pub truncated_bytes: u64,
    pub tasks: usize,
    pub revoked: Vec<String>,
    pub unknown: Vec<String>,

    pub interrupted: Vec<String>,
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn canonical_sha256(v: &Value) -> String {
    sha_hex(serde_json::to_string(v).expect("JSON value serializes").as_bytes())
}

impl Ledger {

    pub fn open(path: &Path, instance_id: &str, boot: &str) -> std::io::Result<(Ledger, Recovered)> {
        let existed = path.exists();
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {
            file.sync_all()?;
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()?;
        }
        let mut text = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut text)?;
        let keep = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let truncated_bytes = (text.len() - keep) as u64;
        if truncated_bytes > 0 {
            file.set_len(keep as u64)?;
            file.sync_all()?;
            text.truncate(keep);
        }
        let mut l = Ledger {
            path: path.to_path_buf(),
            file,
            boot: boot.to_string(),
            instance_id: instance_id.to_string(),
            poisoned: None,
            enabled: true,
            next_n: 0,
            tasks: BTreeMap::new(),
            order: Vec::new(),
            calls: HashMap::new(),
            seen_seq: HashMap::new(),
            seen_req: HashMap::new(),
            last_exec_seq: HashMap::new(),
            last_pub_seq: HashMap::new(),
            launches: HashMap::new(),
            exited: HashSet::new(),
            current: None,
            superseded: HashSet::new(),
            lease: None,
            timers: HashMap::new(),
            accepted_at: Vec::new(),
            internal_on: false,
            receipts: HashMap::new(),
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Line = serde_json::from_str(line)
                .map_err(|e| std::io::Error::other(format!("{}: line {}: {e}", path.display(), n + 1)))?;
            l.fold(rec);
        }

        let mut rec = Recovered { truncated_bytes, tasks: l.tasks.len(), ..Default::default() };
        let ids: Vec<String> = l.tasks.keys().cloned().collect();
        let me = l.internal().incarnation;
        for id in ids {
            let t = l.tasks[&id].clone();

            if t.assignment.executor_id == INTERNAL_EXECUTOR && t.assignment.incarnation != me && !t.execution.settled() && t.execution != Exec::Accepted && t.execution != Exec::OutputSent {
                let mut n = t.clone();
                n.execution = Exec::SettledExited;
                n.reason = Some("agentd_restarted".into());
                if n.publication == Publication::Committed {
                    n.publication = Publication::Unknown;
                }
                l.commit(n, "boot", None)?;
                rec.interrupted.push(id);
                continue;
            }
            let (to, reason) = match t.execution {
                Exec::Accepted => (Exec::StartRevoked, "output_unconfirmed"),
                Exec::OutputSent => (Exec::StartRevoked, "agentd_restart"),
                Exec::Claimed | Exec::Running | Exec::StartUncertain | Exec::Returned | Exec::CancelRequested => {
                    (Exec::Unknown, "agentd_restart")
                }
                _ => continue,
            };
            let mut n = t.clone();
            n.execution = to;
            n.reason = Some(reason.to_string());
            l.commit(n, "boot", None)?;
            if to == Exec::StartRevoked { rec.revoked.push(id) } else { rec.unknown.push(id) }
        }
        Ok((l, rec))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_internal(&mut self, on: bool) {
        self.internal_on = on;
    }

    pub fn internal(&self) -> Assignment {
        Assignment {
            executor_id: INTERNAL_EXECUTOR.to_string(),
            incarnation: Incarnation { launch_id: format!("agentd-{}", self.boot), pid: std::process::id(), plugin_instance: "worker".into() },
        }
    }

    pub fn tool_call_receipt(&mut self, task_id: &str, receipt: &Value) -> std::io::Result<Result<(), &'static str>> {
        let Some(t) = self.tasks.get(task_id) else { return Ok(Err("unknown_task")) };
        if receipt["task_id"].as_str() != Some(task_id) {
            return Ok(Err("task_mismatch"));
        }
        if !matches!(t.execution, Exec::Running | Exec::CancelRequested) || t.assignment.executor_id != INTERNAL_EXECUTOR {
            return Ok(Err("not_running"));
        }
        let last = self.receipts.get(task_id).map(|r| r.len() as u64).unwrap_or(0);
        match receipt["call_seq"].as_u64() {
            Some(n) if n <= last => return Ok(Err("stale_call_seq")),
            Some(n) if n > last + 1 => return Ok(Err("call_seq_gap")),
            None => return Ok(Err("call_seq_missing")),
            _ => {}
        }
        let line = Line::ToolCall { task_id: task_id.to_string(), receipt: receipt.clone() };
        self.write(&line)?;
        self.fold(line);
        Ok(Ok(()))
    }

    pub fn receipts(&self, task_id: &str) -> Vec<Value> {
        self.receipts.get(task_id).cloned().unwrap_or_default()
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    pub fn get(&self, task_id: &str) -> Option<&TaskState> {
        self.tasks.get(task_id)
    }

    pub fn inventory(&self, recent: usize) -> Vec<TaskState> {
        let unresolved = |t: &TaskState| t.execution.holds_slot() || matches!(t.publication, Publication::Committed | Publication::Unknown | Publication::VisibleUnconfirmed);
        let mut budget = recent;
        let mut keep = vec![false; self.order.len()];
        for (i, id) in self.order.iter().enumerate().rev() {
            let t = &self.tasks[id];
            if unresolved(t) {
                keep[i] = true;
            } else if budget > 0 {
                keep[i] = true;
                budget -= 1;
            }
        }
        self.order.iter().zip(keep).filter(|(_, k)| *k).map(|(id, _)| self.tasks[id].clone()).collect()
    }

    pub fn slot_holder(&self) -> Option<&TaskState> {
        self.tasks.values().find(|t| t.execution.holds_slot())
    }

    fn fold(&mut self, rec: Line) {
        match rec {
            Line::Task(t) => {
                if let Some(n) = t.task_id.rsplit("-t").next().and_then(|n| n.parse::<u64>().ok())
                    && t.task_id.starts_with(&format!("{}-t", self.boot))
                {
                    self.next_n = self.next_n.max(n);
                }
                if t.outcome == Outcome::Refused {
                    return;
                }
                if let Some(u) = &t.update {
                    self.note_seen(&t, u);
                }
                if !self.tasks.contains_key(&t.task_id) {
                    self.order.push(t.task_id.clone());
                }
                self.tasks.insert(t.task_id.clone(), t);
            }
            Line::Call { call_key, args_sha256, output, task_id } => {
                self.calls.insert(call_key, (args_sha256, output, task_id));
            }
            Line::Launch { launch_id, secret_sha256 } => {
                self.launches.insert(launch_id, secret_sha256);
            }
            Line::Exit { launch_id, .. } => {
                self.exited.insert(launch_id);
            }
            Line::Current { incarnation } => {
                if let Some(old) = self.current.replace(incarnation) {
                    self.superseded.insert(old);
                }
            }
            Line::ToolCall { task_id, receipt } => {
                self.receipts.entry(task_id).or_default().push(receipt);
            }
        }
    }

    fn note_seen(&mut self, t: &TaskState, u: &UpdateRef) {
        if let Some(req) = &u.request_id {
            self.seen_req.insert((t.task_id.clone(), u.incarnation.clone(), req.clone()), (u.payload_sha256.clone(), t.clone()));
        } else if let Some(seq) = u.seq {
            let epoch = u.pub_epoch.unwrap_or(0);
            self.seen_seq.insert((t.task_id.clone(), epoch, seq), (u.payload_sha256.clone(), t.clone()));
            if u.pub_epoch.is_some() {
                let e = self.last_pub_seq.entry((t.task_id.clone(), epoch)).or_default();
                *e = (*e).max(seq);
            } else {
                let e = self.last_exec_seq.entry(t.task_id.clone()).or_default();
                *e = (*e).max(seq);
            }
        }
    }

    fn write(&mut self, line: &Line) -> std::io::Result<()> {
        if let Some(p) = &self.poisoned {
            return Err(std::io::Error::other(format!("task log poisoned: {p}")));
        }
        let mut bytes = serde_json::to_vec(line).expect("record serializes");
        bytes.push(b'\n');
        let r = self.file.write_all(&bytes).and_then(|_| self.file.sync_data());
        if let Err(e) = &r {
            self.poisoned = Some(e.to_string());
        }
        r
    }

    fn commit(&mut self, mut t: TaskState, trigger: &str, update: Option<UpdateRef>) -> std::io::Result<TaskState> {
        t.rev = self.tasks.get(&t.task_id).map(|o| o.rev + 1).unwrap_or(1);
        t.trigger = trigger.to_string();
        t.outcome = Outcome::Applied;
        t.update = update;
        self.write(&Line::Task(t.clone()))?;
        self.fold(Line::Task(t.clone()));
        Ok(t)
    }

    fn refuse(&mut self, task_id: &str, why: &str, update: Option<UpdateRef>) -> std::io::Result<Option<TaskState>> {
        let Some(cur) = self.tasks.get(task_id).cloned() else { return Ok(None) };
        let mut t = cur;
        t.outcome = Outcome::Refused;
        t.reason = Some(why.to_string());
        t.trigger = "update".to_string();
        t.update = update;
        self.write(&Line::Task(t.clone()))?;
        Ok(Some(t))
    }

    pub fn executor_status(&mut self, executor_id: &str, inc: &Incarnation, seq: u64, available: bool, lease_ms: u64, now: u64) -> std::io::Result<Result<(), &'static str>> {
        if self.superseded.contains(inc) {
            return Ok(Err("stale_executor"));
        }
        if self.current.as_ref() != Some(inc) {
            self.write(&Line::Current { incarnation: inc.clone() })?;
            self.fold(Line::Current { incarnation: inc.clone() });
            self.lease = None;
        }
        if let Some(l) = &self.lease
            && seq <= l.seq
        {
            return Ok(Err("stale_seq"));
        }
        self.lease = Some(Lease { incarnation: inc.clone(), executor_id: executor_id.to_string(), available, expires_at: now + lease_ms, seq });
        Ok(Ok(()))
    }

    fn live_lease(&self, now: u64) -> Option<&Lease> {
        self.lease.as_ref().filter(|l| l.available && now < l.expires_at && self.current.as_ref() == Some(&l.incarnation))
    }

    pub fn register_launch(&mut self, launch_id: &str, secret_sha256: &str) -> std::io::Result<Outcome> {
        match self.launches.get(launch_id) {
            Some(d) if d == secret_sha256 => Ok(Outcome::Duplicate),
            Some(_) => Ok(Outcome::Refused),
            None => {
                let l = Line::Launch { launch_id: launch_id.to_string(), secret_sha256: secret_sha256.to_string() };
                self.write(&l)?;
                self.fold(l);
                Ok(Outcome::Applied)
            }
        }
    }

    pub fn exit_receipt(&mut self, launch_id: &str, launch_proof: &str, code: Option<i32>, signal: Option<String>) -> std::io::Result<Result<Vec<TaskState>, Outcome>> {
        let Some(digest) = self.launches.get(launch_id) else { return Ok(Err(Outcome::Refused)) };
        if &sha_hex(launch_proof.as_bytes()) != digest {
            return Ok(Err(Outcome::Refused));
        }
        self.exit_verified(launch_id, code, signal)
    }

    pub fn exit_verified(&mut self, launch_id: &str, code: Option<i32>, signal: Option<String>) -> std::io::Result<Result<Vec<TaskState>, Outcome>> {
        if launch_id.is_empty() {
            return Ok(Err(Outcome::Refused));
        }
        if self.exited.contains(launch_id) {
            return Ok(Err(Outcome::Duplicate));
        }
        let l = Line::Exit { launch_id: launch_id.to_string(), code, signal };
        self.write(&l)?;
        self.fold(l);
        let mut out = Vec::new();
        let ids: Vec<String> = self.tasks.keys().cloned().collect();
        for id in ids {
            let t = self.tasks[&id].clone();
            let mut n = t.clone();
            let mut changed = false;
            if t.assignment.incarnation.launch_id == launch_id && !t.execution.settled() {
                n.execution = Exec::SettledExited;
                n.reason = Some("launcher_exit_receipt".into());
                changed = true;
            }
            if t.publication == Publication::Committed && t.pub_owner.as_ref().is_some_and(|o| o.launch_id == launch_id) {
                n.publication = Publication::Unknown;
                changed = true;
            }
            if changed {
                out.push(self.commit(n, "exit_receipt", None)?);
            }
        }
        Ok(Ok(out))
    }

    pub fn tool_call(&mut self, name: &str, call_key: CallKey, args: &Value, now: u64) -> std::io::Result<CallAnswer> {
        self.tool_call_by(name, call_key, args, now, None)
    }

    pub fn tool_call_by(&mut self, name: &str, call_key: CallKey, args: &Value, now: u64, origin_speaker: Option<&str>) -> std::io::Result<CallAnswer> {
        if call_key.instance_id != self.instance_id {
            return Err(std::io::Error::other(format!("call key instance {} is not this ledger's {}", call_key.instance_id, self.instance_id)));
        }
        let args_sha = canonical_sha256(args);
        if let Some((sha, output, task_id)) = self.calls.get(&call_key) {
            let output = if *sha == args_sha { output.clone() } else { schema::output_unavailable(Refusal::Conflict) };
            return Ok(CallAnswer { output, task_id: task_id.clone(), record: None });
        }
        let (output, task_id, record) = match Tool::from_name(name) {
            Some(Tool::Delegate) => self.delegate(&call_key, &args_sha, args, now, origin_speaker)?,
            Some(Tool::CancelTask) => self.cancel_tool(args)?,
            Some(Tool::Research) => self.research(&call_key, &args_sha, args, now, origin_speaker)?,
            None => (schema::output_unavailable(Refusal::BadArguments), None, None),
        };
        let line = Line::Call { call_key: call_key.clone(), args_sha256: args_sha, output: output.clone(), task_id: task_id.clone() };
        self.write(&line)?;
        self.fold(line);
        Ok(CallAnswer { output, task_id, record })
    }

    fn delegate(&mut self, call_key: &CallKey, args_sha: &str, args: &Value, now: u64, origin_speaker: Option<&str>) -> std::io::Result<(Value, Option<String>, Option<TaskState>)> {
        let refuse = |r| Ok((schema::output_unavailable(r), None, None));
        let Ok(valid) = schema::validate_delegate(args) else { return refuse(Refusal::BadArguments) };
        if !self.enabled {
            return refuse(Refusal::Disabled);
        }
        let Some(lease) = self.live_lease(now) else { return refuse(Refusal::NoExecutor) };
        let assignment = Assignment { executor_id: lease.executor_id.clone(), incarnation: lease.incarnation.clone() };
        if self.slot_holder().is_some() {
            return refuse(Refusal::Busy);
        }
        self.accepted_at.retain(|t| now.saturating_sub(*t) < 3_600_000);
        if self.accepted_at.len() >= RATE_PER_HOUR {
            return refuse(Refusal::Budget);
        }
        self.next_n += 1;
        let task_id = format!("{}-t{}", self.boot, self.next_n);
        let t = TaskState {
            task_id: task_id.clone(),
            rev: 0,
            execution: Exec::Accepted,
            publication: Publication::None,
            trigger: String::new(),
            outcome: Outcome::Applied,
            reason: None,
            call_key: call_key.clone(),
            args_sha256: args_sha.to_string(),
            args: json!({ "purpose": valid.purpose, "task": valid.task, "audience": valid.audience }),
            assignment,
            child_id: format!("rth-task-{task_id}"),
            input_sha256: None,
            result_sha256: None,
            pub_owner: None,
            pub_epoch: 0,
            chat_ref: None,
            cancel: None,
            after_commit: false,
            abandoned: false,
            update: None,
            origin_speaker: origin_speaker.map(str::to_string),
        };
        let rec = self.commit(t, "tool_call", None)?;
        self.accepted_at.push(now);
        Ok((schema::output_accepted(&task_id), Some(task_id), Some(rec)))
    }

    fn research(&mut self, call_key: &CallKey, args_sha: &str, args: &Value, now: u64, origin_speaker: Option<&str>) -> std::io::Result<(Value, Option<String>, Option<TaskState>)> {
        let refuse = |r| Ok((schema::output_unavailable(r), None, None));
        let Ok(valid) = schema::validate_research(args) else { return refuse(Refusal::BadArguments) };
        if !self.enabled || !self.internal_on {
            return refuse(Refusal::Disabled);
        }
        if self.slot_holder().is_some() {
            return refuse(Refusal::Busy);
        }
        self.accepted_at.retain(|t| now.saturating_sub(*t) < 3_600_000);
        if self.accepted_at.len() >= RATE_PER_HOUR {
            return refuse(Refusal::Budget);
        }
        self.next_n += 1;
        let task_id = format!("{}-t{}", self.boot, self.next_n);
        let mut stored = json!({ "purpose": "research", "task": valid.task });
        if let Some(f) = &valid.folder {
            stored["folder"] = json!(f);
        }
        let t = TaskState {
            task_id: task_id.clone(),
            rev: 0,
            execution: Exec::Accepted,
            publication: Publication::None,
            trigger: String::new(),
            outcome: Outcome::Applied,
            reason: None,
            call_key: call_key.clone(),
            args_sha256: args_sha.to_string(),
            args: stored,
            assignment: self.internal(),
            child_id: format!("rth-task-{task_id}"),
            input_sha256: None,
            result_sha256: None,
            pub_owner: None,
            pub_epoch: 0,
            chat_ref: None,
            cancel: None,
            after_commit: false,
            abandoned: false,
            update: None,
            origin_speaker: origin_speaker.map(str::to_string),
        };
        let rec = self.commit(t, "tool_call", None)?;
        self.accepted_at.push(now);
        Ok((schema::output_accepted_research(&task_id), Some(task_id), Some(rec)))
    }

    fn cancel_tool(&mut self, args: &Value) -> std::io::Result<(Value, Option<String>, Option<TaskState>)> {
        let Ok(id) = schema::validate_cancel(args) else { return Ok((schema::output_unavailable(Refusal::BadArguments), None, None)) };
        let target = match id {
            Some(id) => self.tasks.get(&id).cloned(),
            None => {
                let active: Vec<&TaskState> =
                    self.tasks.values().filter(|t| t.execution.cancellable() || t.publication == Publication::Committed).collect();
                if active.len() == 1 { Some(active[0].clone()) } else { None }
            }
        };
        let Some(t) = target else { return Ok((schema::output_unavailable(Refusal::Ambiguous), None, None)) };
        let id = t.task_id.clone();
        match self.cancel(&t, "tool", None)? {
            Some(rec) if rec.after_commit => Ok((schema::output_publication_committed(&id), Some(id), Some(rec))),
            Some(rec) => Ok((schema::output_cancel_requested(&id), Some(id), Some(rec))),
            None => Ok((schema::output_unavailable(Refusal::NotActive), Some(id), None)),
        }
    }

    fn cancel(&mut self, t: &TaskState, by: &str, uref: Option<UpdateRef>) -> std::io::Result<Option<TaskState>> {
        if t.publication != Publication::None {
            if t.after_commit {
                return Ok(Some(t.clone()));
            }
            let mut n = t.clone();
            n.after_commit = true;
            n.cancel = Some(by.to_string());
            return self.commit(n, "cancel_after_commit", uref).map(Some);
        }
        if !t.execution.cancellable() {
            return Ok(None);
        }
        let mut n = t.clone();
        n.execution = Exec::CancelRequested;
        n.cancel = Some(by.to_string());
        n.reason = Some(format!("cancel:{by}"));
        self.commit(n, "cancel", uref).map(Some)
    }

    pub fn output_sent(&mut self, task_id: &str, now: u64) -> std::io::Result<Option<TaskState>> {
        let Some(t) = self.tasks.get(task_id).cloned() else { return Ok(None) };
        if t.execution != Exec::Accepted {
            return Ok(None);
        }
        let mut n = t;
        n.execution = Exec::OutputSent;
        let rec = self.commit(n, "output_sent", None)?;
        self.timers.entry(task_id.to_string()).or_default().start_by = Some(now + START_MS);
        Ok(Some(rec))
    }

    pub fn tick(&mut self, now: u64) -> std::io::Result<Vec<TaskState>> {
        let mut out = Vec::new();
        let lapsed = self.lease.as_ref().is_some_and(|l| now >= l.expires_at);
        let ids: Vec<String> = self.tasks.keys().cloned().collect();
        for id in ids {
            let t = self.tasks[&id].clone();
            let tm = self.timers.entry(id.clone()).or_default();
            let (to, why) = match t.execution {
                Exec::OutputSent if tm.start_by.is_some_and(|d| now > d) => (Exec::StartRevoked, "start_expired"),
                Exec::Claimed if tm.running_by.is_some_and(|d| now > d) => (Exec::StartUncertain, "no_running"),
                Exec::Claimed | Exec::Running | Exec::Returned | Exec::StartUncertain
                    if t.publication == Publication::None && tm.run_deadline.is_some_and(|d| now > d) =>
                {
                    (Exec::CancelRequested, "cancel:deadline")
                }
                Exec::CancelRequested if tm.teardown_by.is_some_and(|d| now > d) => (Exec::TeardownUnknown, "no_teardown_evidence"),
                Exec::Claimed | Exec::Running | Exec::StartUncertain
                    if lapsed && self.lease.as_ref().is_some_and(|l| l.incarnation == t.assignment.incarnation) =>
                {
                    (Exec::Unknown, "lease_lapse")
                }
                _ => continue,
            };
            let mut n = t;
            n.execution = to;
            n.reason = Some(why.to_string());
            if to == Exec::CancelRequested {
                n.cancel = Some("deadline".into());
            }
            out.push(self.commit(n, "tick", None)?);
            if to == Exec::CancelRequested {
                self.timers.entry(id).or_default().teardown_by = Some(now + TEARDOWN_MS);
            }
        }
        Ok(out)
    }

    pub fn update(&mut self, u: &Update, now: u64) -> std::io::Result<Option<TaskState>> {
        let Some(t) = self.tasks.get(&u.task_id).cloned() else { return Ok(None) };
        let payload = canonical_sha256(&serde_json::to_value(&u.kind).expect("kind serializes"));
        let class = u.kind.class();
        let uref = UpdateRef {
            kind: u.kind.name().to_string(),
            incarnation: u.incarnation.clone(),
            seq: (class != Class::Request).then_some(u.seq),
            pub_epoch: (class == Class::Owner).then(|| u.pub_epoch.unwrap_or(0)),
            request_id: (class == Class::Request).then(|| u.request_id.clone().unwrap_or_default()),
            payload_sha256: payload.clone(),
        };

        match class {
            Class::Request => {
                let Some(req) = u.request_id.clone().filter(|r| !r.is_empty()) else {
                    return self.refuse(&u.task_id, "request_id_required", Some(uref));
                };
                if let Some((d, rec)) = self.seen_req.get(&(u.task_id.clone(), u.incarnation.clone(), req)) {
                    if *d == payload {
                        let mut r = rec.clone();
                        r.outcome = Outcome::Duplicate;
                        return Ok(Some(r));
                    }
                    return self.refuse(&u.task_id, "conflict", Some(uref));
                }
            }
            Class::Exec | Class::Owner => {
                let epoch = if class == Class::Owner { u.pub_epoch.unwrap_or(0) } else { 0 };
                if let Some((d, rec)) = self.seen_seq.get(&(u.task_id.clone(), epoch, u.seq)) {
                    if *d == payload {
                        let mut r = rec.clone();
                        r.outcome = Outcome::Duplicate;
                        return Ok(Some(r));
                    }
                    return self.refuse(&u.task_id, "conflict", Some(uref));
                }
                let last = if class == Class::Owner {
                    self.last_pub_seq.get(&(u.task_id.clone(), epoch)).copied().unwrap_or(0)
                } else {
                    self.last_exec_seq.get(&u.task_id).copied().unwrap_or(0)
                };
                if u.seq <= last {
                    return self.refuse(&u.task_id, "stale_seq", Some(uref));
                }
            }
        }

        match class {
            Class::Exec if u.incarnation != t.assignment.incarnation || u.executor_id != t.assignment.executor_id => {
                let why = if self.superseded.contains(&u.incarnation) { "stale_executor" } else { "not_assigned" };
                return self.refuse(&u.task_id, why, Some(uref));
            }
            Class::Owner if t.pub_owner.as_ref() != Some(&u.incarnation) || u.pub_epoch != Some(t.pub_epoch) => {
                return self.refuse(&u.task_id, "not_publication_owner", Some(uref));
            }
            Class::Request if self.superseded.contains(&u.incarnation) => {
                return self.refuse(&u.task_id, "stale_executor", Some(uref));
            }
            _ => {}
        }

        if t.abandoned && !(class == Class::Owner || matches!(u.kind, Kind::PublicationTakeover { .. } | Kind::VisibleObserved { .. })) {
            return self.refuse(&u.task_id, "abandoned", Some(uref));
        }
        if t.abandoned && t.pub_owner.is_none() {
            return self.refuse(&u.task_id, "abandoned", Some(uref));
        }
        let mut n = t.clone();
        n.reason = None;
        let refused = |l: &mut Ledger, why: &str| l.refuse(&u.task_id, why, Some(uref.clone()));
        match &u.kind {
            Kind::Claim => {
                let ok = t.execution == Exec::OutputSent
                    && self.timers.get(&t.task_id).and_then(|tm| tm.start_by).is_some_and(|d| now <= d);
                if !ok {
                    return refused(self, "claim_not_allowed");
                }
                n.execution = Exec::Claimed;
                let rec = self.commit(n, "update", Some(uref))?;
                let tm = self.timers.entry(t.task_id.clone()).or_default();
                tm.running_by = Some(now + START_MS);
                tm.run_deadline = Some(now + if t.args["purpose"] == "research" { RESEARCH_RUN_MS } else { RUN_MS });
                return Ok(Some(rec));
            }
            Kind::Running { child_id, input_sha256 } => {
                if !matches!(t.execution, Exec::Claimed | Exec::StartUncertain | Exec::Unknown) || *child_id != t.child_id {
                    return refused(self, "running_not_allowed");
                }
                n.execution = Exec::Running;
                n.input_sha256 = Some(input_sha256.clone());
            }
            Kind::Returned { result_sha256, .. } => {
                if !matches!(t.execution, Exec::Running | Exec::Unknown) {
                    let why = if t.execution == Exec::CancelRequested { "late_after_cancel" } else { "returned_not_allowed" };
                    return refused(self, why);
                }
                n.execution = Exec::Returned;
                n.result_sha256 = Some(result_sha256.clone());
            }
            Kind::PublishRequest { result_sha256 } => {
                let has_result = t.result_sha256.as_deref() == Some(result_sha256.as_str());
                let ok = has_result
                    && matches!(t.execution, Exec::Returned | Exec::ExecutionDone | Exec::Unknown)
                    && t.publication == Publication::None
                    && t.cancel.is_none()
                    && !t.abandoned;
                if !ok {
                    return refused(self, "publication_not_allowed");
                }
                n.publication = Publication::Committed;
                n.pub_owner = Some(u.incarnation.clone());
                n.pub_epoch = 1;
            }
            Kind::Cancelled => {
                if !matches!(t.execution, Exec::CancelRequested | Exec::TeardownUnknown | Exec::Unknown) {
                    return refused(self, "cancelled_not_allowed");
                }
                n.execution = Exec::Cancelled;
            }
            Kind::TeardownUnknown => {
                if t.execution != Exec::CancelRequested {
                    return refused(self, "teardown_unknown_not_allowed");
                }
                n.execution = Exec::TeardownUnknown;
            }
            Kind::Failed { reason } => {
                if !matches!(t.execution, Exec::Claimed | Exec::Running | Exec::StartUncertain | Exec::Unknown | Exec::CancelRequested) {
                    return refused(self, "failed_not_allowed");
                }
                n.execution = Exec::SettledFailed;
                n.reason = Some(reason.clone());
            }
            Kind::ExecutionDone => {
                if !matches!(t.execution, Exec::Returned | Exec::Unknown) || t.result_sha256.is_none() {
                    return refused(self, "execution_done_not_allowed");
                }
                n.execution = Exec::ExecutionDone;
            }
            Kind::Delivered { chat_ref, receipt } => {
                if !matches!(t.publication, Publication::Committed) {
                    return refused(self, "delivered_not_allowed");
                }
                if let Err(why) = validate_receipt(&t, receipt) {
                    return refused(self, why);
                }
                n.publication = Publication::Delivered;
                n.chat_ref = Some(chat_ref.clone());
            }
            Kind::DeliveryFailed { reason } => {
                if t.publication != Publication::Committed {
                    return refused(self, "delivery_failed_not_allowed");
                }
                n.publication = Publication::DeliveryFailed;
                n.reason = Some(reason.clone());
            }
            Kind::VisibleUnconfirmed { .. } => {
                if t.publication != Publication::Committed {
                    return refused(self, "visible_unconfirmed_not_allowed");
                }
                n.publication = Publication::VisibleUnconfirmed;
            }
            Kind::VisibleObserved { result_sha256, .. } => {

                if t.result_sha256.as_deref() != Some(result_sha256.as_str()) {
                    return refused(self, "digest_mismatch");
                }
            }
            Kind::PublicationTakeover { expected_owner, expected_epoch, result_sha256 } => {
                let owner = t.pub_owner.clone();
                let ok = owner.as_ref() == Some(expected_owner)
                    && *expected_epoch == t.pub_epoch
                    && t.result_sha256.as_deref() == Some(result_sha256.as_str())
                    && matches!(t.publication, Publication::Committed | Publication::Unknown | Publication::VisibleUnconfirmed)
                    && expected_owner.launch_id != u.incarnation.launch_id
                    && self.exited.contains(&expected_owner.launch_id)
                    && self.current.as_ref() == Some(&u.incarnation);
                if !ok {
                    return refused(self, "takeover_not_allowed");
                }
                n.publication = Publication::Committed;
                n.pub_owner = Some(u.incarnation.clone());
                n.pub_epoch = t.pub_epoch + 1;
            }
            Kind::CancelIntent { by } => {
                return match self.cancel(&t, by, Some(uref.clone()))? {
                    Some(rec) => Ok(Some(rec)),
                    None => refused(self, "not_active"),
                };
            }
            Kind::Abandon => {
                if t.publication != Publication::None {
                    n.after_commit = true;
                } else {
                    n.abandoned = true;
                    if t.execution.cancellable() {
                        n.execution = Exec::CancelRequested;
                        n.cancel = Some("abandon".into());
                    }
                }
            }
        }
        Ok(Some(self.commit(n, "update", Some(uref))?))
    }
}

fn validate_receipt(t: &TaskState, r: &Receipt) -> Result<(), &'static str> {
    if r.task_id != t.task_id || r.key != t.child_id {
        return Err("receipt_task_mismatch");
    }
    if t.result_sha256.as_deref() != Some(r.result_sha256.as_str()) {
        return Err("receipt_digest_mismatch");
    }
    if r.session_lineage.is_empty() {
        return Err("receipt_lineage_missing");
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod research_tests;
