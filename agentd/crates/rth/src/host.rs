

use std::collections::HashMap;
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rth_protocol::io::read_frame;
use rth_protocol::{CommandStatus, Envelope, Event, MonotonicClock, PROTOCOL_VERSION, ResponseStatus, Role, Source, Stamper};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

use crate::devices::{Capture, Playout};
use crate::voice::{Control, Note, Urgent, VoiceOptions, drive_with};

pub const REPLY_RING: usize = 256;

pub const OPEN_REPLIES: usize = 64;

pub const REPLY_TEXT_BYTES: usize = 16 * 1024;

pub const OPEN_REPLY_BYTES: usize = 256 * 1024;

const AGENT_RESULT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Default)]
struct Observation {
    text: String,

    started: bool,

    link: u64,

    gap: bool,
    truncated: bool,
}

#[derive(Debug, Default)]
pub struct OpenReplies {
    open: Vec<(String, Observation)>,
    bytes: usize,
}

impl OpenReplies {
    fn get_or_start(&mut self, rid: &str, link: u64, started: bool) -> &mut Observation {
        if let Some(i) = self.open.iter().position(|(r, _)| r == rid) {
            return &mut self.open[i].1;
        }
        while self.open.len() >= OPEN_REPLIES {
            let (_, old) = self.open.remove(0);
            self.bytes -= old.text.len();
        }
        self.open.push((rid.to_string(), Observation { started, link, ..Default::default() }));
        &mut self.open.last_mut().unwrap().1
    }

    fn push(&mut self, rid: &str, link: u64, text: &str) {
        let total = self.bytes;
        let o = self.get_or_start(rid, link, false);
        if o.link != link {
            o.gap = true;
        }
        let room = REPLY_TEXT_BYTES.saturating_sub(o.text.len()).min(OPEN_REPLY_BYTES.saturating_sub(total));
        if text.len() > room {
            o.truncated = true;
            return;
        }
        o.text.push_str(text);
        self.bytes += text.len();
    }

    fn replace(&mut self, rid: &str, link: u64, text: &str) {
        let o = self.get_or_start(rid, link, false);
        let old = std::mem::take(&mut o.text).len();
        o.truncated = false;
        self.bytes -= old;
        self.push(rid, link, text);
    }

    fn take(&mut self, rid: &str) -> Option<Observation> {
        let i = self.open.iter().position(|(r, _)| r == rid)?;
        let (_, o) = self.open.remove(i);
        self.bytes -= o.text.len();
        Some(o)
    }

    fn mark_gap(&mut self) {
        self.open.iter_mut().for_each(|(_, o)| o.gap = true);
    }
}

pub const ADMISSION_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Outgoing {
    pub command_id: String,
    pub session_id: String,
    pub expected_generation: u64,
    pub text: String,

    pub status: String,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub decision_id: Option<String>,
    #[serde(default)]
    pub response_id: Option<String>,
    #[serde(default)]
    pub result_text: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_complete: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    pub updated_utc_ms: u64,

    #[serde(default, skip_serializing)]
    pub recovered: bool,
}

impl Outgoing {
    pub fn terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "cancelled" | "failed" | "interrupted" | "rejected")
    }
}

pub fn status_name(s: CommandStatus) -> &'static str {
    match s {
        CommandStatus::Admitted => "admitted",
        CommandStatus::Dispatched => "dispatched",
        CommandStatus::Completed => "completed",
        CommandStatus::Cancelled => "cancelled",
        CommandStatus::Failed => "failed",
        CommandStatus::Interrupted => "interrupted",
    }
}

pub struct Outbox {
    path: PathBuf,
    file: std::fs::File,
    pub entries: Vec<Outgoing>,
    poisoned: Option<String>,

    pub repaired_bytes: u64,
    #[cfg(test)]
    fail_next: Option<bool>,
}

impl Outbox {
    pub fn open(path: &Path) -> Result<Self> {
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;

            if let Some(parent) = dir.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::File::open(parent)?.sync_all()?;
            }
        }
        let existed = path.exists();
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {
            file.sync_all()?;
            std::fs::File::open(dir)?.sync_all()?;
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes)?;

        let keep = match bytes.last() {
            None | Some(b'\n') => bytes.len(),
            Some(_) => bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1),
        };
        let repaired_bytes = (bytes.len() - keep) as u64;
        if repaired_bytes > 0 {
            file.set_len(keep as u64)?;
            file.sync_all()?;
        }
        let mut entries: Vec<Outgoing> = Vec::new();
        for (n, line) in String::from_utf8_lossy(&bytes[..keep]).lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let mut o: Outgoing = serde_json::from_str(line).with_context(|| format!("{}: malformed record at line {}", path.display(), n + 1))?;
            o.recovered = !o.terminal();
            match entries.iter_mut().find(|e| e.command_id == o.command_id) {
                Some(e) => *e = o,
                None => entries.push(o),
            }
        }
        Ok(Outbox {
            path: path.to_path_buf(),
            file,
            entries,
            poisoned: None,
            repaired_bytes,
            #[cfg(test)]
            fail_next: None,
        })
    }

    pub fn poisoned(&self) -> Option<&str> {
        self.poisoned.as_deref()
    }

    fn append_line(&mut self, line: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if let Some(partial) = self.fail_next.take() {
            if partial {
                self.file.write_all(&line[..line.len() / 2])?;
                return Err(std::io::Error::other("injected: partial write"));
            }
            self.file.write_all(line)?;
            return Err(std::io::Error::other("injected: fsync failed"));
        }
        self.file.write_all(line)?;
        self.file.sync_data()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, id: &str) -> Option<&Outgoing> {
        self.entries.iter().find(|e| e.command_id == id)
    }

    pub fn by_response(&self, response_id: &str) -> Option<&Outgoing> {
        self.entries.iter().find(|e| e.response_id.as_deref() == Some(response_id))
    }

    pub fn put(&mut self, mut o: Outgoing) -> Result<()> {
        if let Some(why) = &self.poisoned {
            bail!("host outbox is poisoned ({why}); restart the host owner to recover it");
        }
        o.updated_utc_ms = rth_protocol::utc_ms();
        let mut line = serde_json::to_string(&o)?;
        line.push('\n');
        if let Err(e) = self.append_line(line.as_bytes()) {
            let why = format!("an append or fsync failed ({e}); whether the record landed is unknown");
            self.poisoned = Some(why.clone());
            bail!("{why}");
        }
        o.recovered = false;
        match self.entries.iter_mut().find(|e| e.command_id == o.command_id) {
            Some(e) => *e = o,
            None => self.entries.push(o),
        }
        Ok(())
    }

    pub fn update(&mut self, id: &str, f: impl FnOnce(&mut Outgoing)) -> Result<()> {
        let Some(mut o) = self.get(id).cloned() else { return Ok(()) };
        let before = o.clone();
        f(&mut o);
        if o != before { self.put(o) } else { Ok(()) }
    }
}

mod tasks;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Expect {

    pub backend: Option<String>,

    pub boot: Option<String>,
}

impl Expect {
    pub fn check(&self, backend: Option<&str>, boot: &str) -> std::result::Result<(), String> {
        if let Some(want) = &self.backend
            && !backend.is_some_and(|b| b.strip_prefix(want.as_str()).is_some_and(|rest| rest.starts_with('@')))
        {
            return Err(format!("its backend is {backend:?}, not {want}@…"));
        }
        if let Some(want) = &self.boot
            && boot != want
        {
            return Err(format!("its boot is {boot}, not the expected {want} (another agent lifetime)"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LinkState {

    pub state: String,
    pub agent: String,
    pub boot_id: Option<String>,
    pub backend: Option<String>,
    pub deployment: Option<Value>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub context: Option<String>,

    pub command_generation: Option<u64>,

    pub commands_supported: bool,
    pub connects: u64,
    pub last_error: Option<String>,
    pub since_utc_ms: u64,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Admitted(Outgoing, bool),
    Rejected(String),
    LinkDown(String),
}

pub struct Shared {
    pub link: Mutex<LinkState>,
    pub outbox: Mutex<Outbox>,
    waiters: Mutex<HashMap<String, Vec<oneshot::Sender<Outcome>>>>,

    sent: Mutex<HashMap<String, String>>,

    replies: Mutex<OpenReplies>,

    done_replies: Mutex<std::collections::VecDeque<Value>>,
    pub ctl: mpsc::Sender<Control>,
    submit: tokio::sync::Mutex<()>,
    pub log: Mutex<Option<std::fs::File>>,
    started: Instant,
    pub voice: String,

    pub voice_policy: Mutex<Value>,

    pub voice_device: Mutex<Option<Arc<dyn Fn() -> Value + Send + Sync>>>,

    pub feed: Mutex<Option<crate::devices::FeedCapture>>,

    pub call: Mutex<Option<Arc<crate::call::Call>>>,
    pub status_source: Option<StatusSource>,

    pub expect: Mutex<Option<Expect>>,

    pub urgent: Arc<Urgent>,

    later: std::sync::mpsc::Sender<(String, Value)>,

    tasks: Mutex<tasks::TaskRoutes>,
}

#[derive(Clone)]
pub struct StatusSource {
    pub data: PathBuf,
    pub vm: String,
}

impl Shared {
    pub fn new(ctl: mpsc::Sender<Control>, outbox: Outbox, agent: String, voice: String, status_source: Option<StatusSource>, log: Option<std::fs::File>) -> Arc<Self> {
        let (later, later_rx) = std::sync::mpsc::channel::<(String, Value)>();
        let shared = Arc::new(Shared {
            urgent: Arc::new(Urgent::default()),
            later,
            link: Mutex::new(LinkState { state: "connecting".into(), agent, since_utc_ms: rth_protocol::utc_ms(), ..Default::default() }),
            outbox: Mutex::new(outbox),
            waiters: Mutex::new(HashMap::new()),
            sent: Mutex::new(HashMap::new()),
            replies: Mutex::new(OpenReplies::default()),
            done_replies: Mutex::new(std::collections::VecDeque::new()),
            ctl,
            submit: tokio::sync::Mutex::new(()),
            log: Mutex::new(log),
            started: Instant::now(),
            voice,
            voice_policy: Mutex::new(Value::Null),
            voice_device: Mutex::new(None),
            feed: Mutex::new(None),
            call: Mutex::new(None),
            expect: Mutex::new(None),
            status_source,
            tasks: Mutex::new(tasks::TaskRoutes::default()),
        });
        let weak = Arc::downgrade(&shared);
        std::thread::spawn(move || {
            while let Ok((kind, v)) = later_rx.recv() {
                let Some(s) = weak.upgrade() else { return };
                s.note(&kind, v);
            }
        });
        shared
    }

    pub fn native_intent(&self, quit: bool, via: &str) {
        if quit { self.urgent.quit() } else { self.urgent.stop() }
        let _ = self.later.send((if quit { "quit_requested" } else { "stop_requested" }.into(), json!({"via": via})));
    }

    pub fn end_stream_request(&self, mic_wav: bool) -> &'static str {
        let outcome = if !mic_wav {
            "ignored"
        } else {
            match self.ctl.try_send(Control::EndStream) {
                Ok(()) => "admitted",
                Err(_) => "refused",
            }
        };
        let why = match outcome {
            "ignored" => "end-stream is honored only with --mic-wav",
            "refused" => "control channel full or closed; nothing was queued",
            _ => "queued as Control::EndStream",
        };
        let _ = self.later.send(("end_stream_request".into(), json!({"outcome": outcome, "detail": why})));
        outcome
    }

    pub fn talk_request(&self, down: bool, via: &str) -> &'static str {
        let outcome = match self.ctl.try_send(Control::Talk(down)) {
            Ok(()) => "admitted",
            Err(_) => "refused",
        };
        let _ = self.later.send(("talk_request".into(), json!({"down": down, "via": via, "outcome": outcome})));
        outcome
    }

    pub fn mute_request(&self, on: bool, via: &str) -> bool {
        let was = self.urgent.mute.request(on);
        let _ = self.later.send(("mute_request".into(), json!({"muted": on, "was": was, "via": via})));
        was
    }

    pub fn call_request(&self, on: bool, via: &str) -> Value {
        let call = self.call.lock().unwrap().clone();
        match call {
            Some(c) => c.request(on, via),
            None => {
                let _ = self.later.send(("call_request".into(), json!({"request": if on { "start" } else { "end" }, "via": via, "refused": "this owner runs no call"})));
                json!({"refused": "this owner runs no call (an rth-host without the call owner)"})
            }
        }
    }

    pub fn note_later(&self, kind: &str, v: Value) {
        let _ = self.later.send((kind.into(), v));
    }

    pub fn say_request(&self, path: &str) -> Result<(u64, u64), String> {
        let feed = self.feed.lock().unwrap().clone().ok_or("say is honored only with --mic-wav")?;
        let w = rth_voice::wav::read(std::path::Path::new(path))?;
        let range = feed.say(&w.to_i16_at(16_000));
        let _ = self.later.send(("say".into(), json!({"path": path, "start_sample": range.0, "end_sample": range.1})));
        Ok(range)
    }

    pub fn note(&self, kind: &str, mut v: Value) {
        if let Some(f) = self.log.lock().unwrap().as_mut() {
            v["t_ms"] = json!(self.started.elapsed().as_secs_f64() * 1e3);
            v["kind"] = json!(kind);

            let _ = f.write_all(format!("{v}\n").as_bytes());
        }
    }

    fn wake(&self, command_id: &str, outcome: Outcome) {
        if let Some(ws) = self.waiters.lock().unwrap().remove(command_id) {
            for w in ws {
                let _ = w.send(outcome.clone());
            }
        }
    }

    fn wake_all(&self, why: &str) {
        self.tasks_link_down(why);
        let ws: Vec<_> = self.waiters.lock().unwrap().drain().collect();
        for (_, ws) in ws {
            for w in ws {
                let _ = w.send(Outcome::LinkDown(why.into()));
            }
        }
    }

    fn outbox_update(&self, id: &str, f: impl FnOnce(&mut Outgoing)) {
        if let Err(e) = self.outbox.lock().unwrap().update(id, f) {
            self.note("outbox_fault", json!({"command_id": id, "error": e.to_string()}));
        }
    }

    pub fn on_note(&self, n: &Note) {
        match n {
            Note::Out { env, .. } => {
                self.task_written(env);
                if let Event::CommandSubmitted { command_id, .. } = &env.event {
                    self.sent.lock().unwrap().insert(env.id.clone(), command_id.clone());
                    self.note("command_written", json!({"command_id": command_id, "envelope": env.id}));
                }
                if let Event::Cancel { response_id } = &env.event {
                    self.note("cancel_written", json!({"response_id": response_id}));
                }
                if let Event::Playback { response_id, epoch, phase, played_samples, dropped_samples, sample_rate, reason, .. } = &env.event {
                    self.note("playback", json!({"response_id": response_id, "epoch": epoch, "phase": phase, "played_samples": played_samples, "dropped_samples": dropped_samples, "sample_rate": sample_rate, "reason": reason}));
                }
            }
            Note::Cleared { t, trigger, cleared, cancelled } => {
                self.note(
                    "speaker_cleared",
                    json!({"reason": cleared.reason.as_str(), "dropped_samples": cleared.dropped_samples, "clear_us": (*t - *trigger).as_secs_f64() * 1e6, "cancelled": cancelled}),
                );
            }
            Note::In { env, .. } => self.on_agent(env),
            Note::Onset { stream_id, start_sample, detected_sample, audible, action, level_db, echo_db, .. } => {
                self.note("local_vad_onset", json!({"stream_id": stream_id, "start_sample": start_sample, "detected_sample": detected_sample, "audible": audible, "action": action, "level_dbfs": level_db, "echo_est_dbfs": echo_db}));
            }
            Note::NearEnd { near, audible, action, .. } => {
                self.note("echo_gate", json!({"audible": audible, "action": action, "frame_dbfs": near.frame_db, "threshold_dbfs": near.threshold_db, "echo_est_dbfs": near.echo_db, "run_ms": (near.confirmed - near.start).as_secs_f64() * 1e3}));
            }
            Note::EchoProbe { response_id, epoch, phase, rendered_ms, probe } => {
                let p = probe.map(|p| json!({"frames": p.frames, "lag_ms": p.lag_ms, "corr": p.corr, "coupling_db": p.coupling_db, "mic_dbfs_p50": p.mic_db_p50, "rendered_dbfs_p50": p.ref_db_p50}));
                self.note("echo_probe", json!({"response_id": response_id, "epoch": epoch, "phase": phase, "rendered_ms": rendered_ms, "probe": p,
                    "clock": "render taps stamped in the output callback, microphone frames in the capture callback, one host monotonic clock"}));
            }
            Note::Talk { down, applied, .. } => self.note("talk", json!({"down": down, "applied": applied})),
            Note::Mute { muted, call_closed, call_epoch, output_closed, generation, stream_ended, discarded_samples, in_flight, .. } => self.note(
                "mute_applied",
                json!({"muted": muted, "call_input_closed": call_closed, "call_epoch": call_epoch, "call_output_closed": output_closed, "generation": generation, "stream_ended": stream_ended, "partial_samples_discarded": discarded_samples, "in_flight_at_fence": in_flight}),
            ),
            _ => {}
        }
    }

    fn record_reply(&self, reply: Value) {
        let strength = |r: &Value| match (r["text_complete"].as_bool(), r["source"].as_str()) {
            (Some(true), _) => 3,
            (_, Some("command_log")) => 2,
            (_, _) if r["text"].is_string() => 1,
            _ => 0,
        };
        let mut ring = self.done_replies.lock().unwrap();
        if let Some(i) = ring.iter().position(|r| r["response_id"] == reply["response_id"]) {
            if strength(&reply) > strength(&ring[i]) {
                ring[i] = reply;
            }
            return;
        }
        ring.push_back(reply);
        while ring.len() > REPLY_RING {
            ring.pop_front();
        }
    }

    fn link_session(&self) -> u64 {
        self.link.lock().unwrap().connects
    }

    pub fn reply(&self, response_id: &str) -> Option<Value> {
        self.done_replies.lock().unwrap().iter().find(|r| r["response_id"] == response_id).cloned()
    }

    fn on_agent(&self, env: &Envelope) {
        let exits = self.task_from_agent(env);
        if !exits.is_empty() {
            self.reforward_exits(exits);
        }
        match &env.event {
            Event::CommandAdmitted { command_id, generation, status, duplicate, decision_id, response_id, result_text, detail } => {
                {
                    let mut l = self.link.lock().unwrap();
                    l.command_generation = Some(l.command_generation.unwrap_or(0).max(*generation));
                }
                let status = status_name(*status).to_string();
                self.outbox_update(command_id, |o| {

                    if !(o.terminal() && !matches!(status.as_str(), "completed" | "cancelled" | "failed" | "interrupted")) {
                        o.status = status.clone();
                    }
                    o.generation = Some(*generation);
                    o.decision_id = decision_id.clone().or(o.decision_id.take());
                    o.response_id = response_id.clone().or(o.response_id.take());

                    if result_text.is_some() && o.text_complete != Some(true) {
                        let truncated = result_text.as_ref().is_some_and(|t| t.len() >= AGENT_RESULT_BYTES);
                        o.result_text = result_text.clone();
                        o.text_complete = Some(!truncated);
                        o.truncated = Some(truncated);
                        o.source = Some("command_log".into());
                        o.observed = None;
                    }
                    o.detail = detail.clone().or(o.detail.take());
                });
                self.note("command_admitted", json!({"command_id": command_id, "generation": generation, "status": status, "duplicate": duplicate}));
                if let Some(rid) = response_id
                    && matches!(status.as_str(), "completed" | "cancelled" | "failed" | "interrupted")
                {

                    let truncated = result_text.as_ref().is_some_and(|t| t.len() >= AGENT_RESULT_BYTES);
                    let mut reply = json!({"response_id": rid, "status": status, "text": result_text, "source": "command_log",
                        "text_complete": result_text.is_some() && !truncated, "truncated": truncated});
                    if let Some(d) = detail {
                        reply["detail"] = json!(d);
                    }
                    self.record_reply(reply);
                }
                let o = self.outbox.lock().unwrap().get(command_id).cloned();
                if let Some(o) = o {
                    self.wake(command_id, Outcome::Admitted(o, *duplicate));
                }
            }
            Event::Rejected { reason } => {
                let cmd = env.cause.as_ref().and_then(|c| self.sent.lock().unwrap().get(c).cloned());
                if let Some(cmd) = cmd {
                    self.note("command_rejected", json!({"command_id": cmd, "reason": reason}));
                    self.wake(&cmd, Outcome::Rejected(reason.clone()));
                }
            }
            Event::ResponseStarted { response_id, request_id, .. } => {
                self.note("response_started", json!({"response_id": response_id, "request_id": request_id, "cause": env.cause}));
                let known = self.outbox.lock().unwrap().get(request_id).is_some();
                let link = self.link_session();
                self.replies.lock().unwrap().get_or_start(response_id, link, true).started = true;
                if known {
                    self.outbox_update(request_id, |o| {
                        o.response_id = Some(response_id.clone());
                        if !o.terminal() {
                            o.status = "dispatched".into();
                        }
                    });
                }
            }
            Event::AgentDelta { response_id, text } => {
                let link = self.link_session();
                self.replies.lock().unwrap().push(response_id, link, text);
            }
            Event::AgentReplace { response_id, text } => {
                let link = self.link_session();
                self.replies.lock().unwrap().replace(response_id, link, text);
            }

            Event::Log { level, msg } if level == "warn" && msg.contains("lagged") => self.replies.lock().unwrap().mark_gap(),
            Event::InputTranscript { text, .. } => self.note("input_transcript", json!({"text": text})),
            Event::ProviderSignal { response_id, signal } => self.note("provider_signal", json!({"response_id": response_id, "signal": signal})),
            Event::AgentDone { response_id, status } => {
                self.note("response_done", json!({"response_id": response_id, "status": status}));
                let obs = self.replies.lock().unwrap().take(response_id);
                let terminal = match status {
                    ResponseStatus::Completed => "completed",
                    ResponseStatus::Cancelled => "cancelled",
                    ResponseStatus::Failed => "failed",
                };
                let link = self.link_session();
                let (observed, complete, text_seen, truncated) = match &obs {
                    None => ("terminal_only", false, None, false),
                    Some(o) => {
                        let unbroken = o.started && !o.gap && o.link == link && !o.truncated;
                        let kind = match (unbroken, o.started, o.text.is_empty()) {
                            (true, ..) => "complete",
                            (false, false, true) => "terminal_only",
                            (false, false, false) => "suffix",
                            (false, true, _) => "partial",
                        };
                        (kind, unbroken, Some(o.text.clone()), o.truncated)
                    }
                };
                self.record_reply(json!({"response_id": response_id, "status": terminal, "text": text_seen, "source": "observed",
                    "observed": observed, "text_complete": complete, "truncated": truncated}));
                let text = obs.map(|o| o.text);
                let id = self.outbox.lock().unwrap().by_response(response_id).map(|o| o.command_id.clone());
                if let Some(id) = id {
                    let s = match status {
                        ResponseStatus::Completed => "completed",
                        ResponseStatus::Cancelled => "cancelled",
                        ResponseStatus::Failed => "failed",
                    };
                    self.outbox_update(&id, |o| {
                        o.status = s.into();

                        let weaker = o.source.as_deref() == Some("command_log") && !complete;
                        if text.is_some() && !weaker {
                            o.result_text = text.clone();
                            o.text_complete = Some(complete);
                            o.truncated = Some(truncated);
                            o.source = Some("observed".into());
                            o.observed = Some(observed.into());
                        } else if !weaker && o.source.is_none() {
                            o.text_complete = Some(false);
                            o.source = Some("observed".into());
                            o.observed = Some(observed.into());
                        }
                    });
                    self.note("command_finished", json!({"command_id": id, "response_id": response_id, "status": s}));
                } else if let Some(t) = &text {
                    self.note("reply_text", json!({"response_id": response_id, "text": t}));
                }
                if *status == ResponseStatus::Cancelled {
                    self.note("response_cancelled", json!({"response_id": response_id}));
                }
            }
            _ => {}
        }
    }

    pub async fn submit(self: &Arc<Self>, command_id: &str, text: &str) -> (u16, Value) {
        if command_id.is_empty() || command_id.len() > 128 {
            return (400, json!({"error": "command_id must be 1..=128 chars"}));
        }
        let _one = self.submit.lock().await;
        let link = self.link.lock().unwrap().clone();
        if link.state != "up" {
            let known = self.outbox.lock().unwrap().get(command_id).cloned();
            return (503, json!({"error": format!("agent link is {}", link.state), "retry": true, "known": known}));
        }
        if !link.commands_supported {
            return (
                409,
                json!({"rejected": "this agent cannot admit commands (no durable command admission: legacy artifact or no instance session); activate the durable-admission build"}),
            );
        }
        let known = self.outbox.lock().unwrap().get(command_id).cloned();
        let frame = match known {

            Some(o) if o.text == text => o,

            Some(o) => Outgoing { text: text.into(), ..o },
            None => {
                let (Some(session_id), Some(generation)) = (link.session_id.clone(), link.command_generation) else {
                    return (503, json!({"error": "agent did not report its session and generation", "retry": true}));
                };
                let o = Outgoing {
                    command_id: command_id.into(),
                    session_id,
                    expected_generation: generation,
                    text: text.into(),
                    status: "pending".into(),
                    generation: None,
                    decision_id: None,
                    response_id: None,
                    result_text: None,
                    detail: None,
                    text_complete: None,
                    truncated: None,
                    source: None,
                    observed: None,
                    updated_utc_ms: 0,
                    recovered: false,
                };
                let (shared, rec) = (self.clone(), o.clone());
                let put = tokio::task::spawn_blocking(move || shared.outbox.lock().unwrap().put(rec)).await;
                if let Err(e) = put.map_err(anyhow::Error::from).and_then(|r| r) {
                    return (
                        500,
                        json!({"error": format!("host outbox write failed: not sent in this invocation. The record may still have landed; if it did, the restarted host resends it with this same id: {e}"), "sent": false, "recovery": "on host restart, a landed pending record is resent with the same command_id"}),
                    );
                }
                o
            }
        };
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().unwrap().entry(command_id.into()).or_default().push(tx);
        let sent = self
            .ctl
            .send(Control::Submit {
                command_id: frame.command_id.clone(),
                session_id: frame.session_id.clone(),
                expected_generation: frame.expected_generation,
                text: frame.text.clone(),
            })
            .await;
        if sent.is_err() {
            return (503, json!({"error": "host is shutting down", "retry": true}));
        }
        match tokio::time::timeout(ADMISSION_WAIT, rx).await {
            Ok(Ok(Outcome::Admitted(o, duplicate))) => (200, json!({"command": o, "duplicate": duplicate})),
            Ok(Ok(Outcome::Rejected(reason))) => {

                let stored = self.outbox.lock().unwrap().get(command_id).cloned();
                if stored.as_ref().is_some_and(|s| s.text == text && s.status == "pending") {
                    self.outbox_update(command_id, |o| {
                        o.status = "rejected".into();
                        o.detail = Some(reason.clone());
                    });
                }
                (409, json!({"rejected": reason}))
            }
            Ok(Ok(Outcome::LinkDown(why))) => (503, json!({"error": format!("agent link lost before an answer: {why}"), "retry": true})),
            Ok(Err(_)) | Err(_) => (504, json!({"error": "no admission answer in time; admission unknown, retry with the same id", "retry": true})),
        }
    }

    pub async fn resend_unfinished(&self) {
        let open: Vec<Outgoing> = self.outbox.lock().unwrap().entries.iter().filter(|o| !o.terminal()).cloned().collect();
        for o in open {
            self.note("command_resent", json!({"command_id": o.command_id, "last_status": o.status}));
            let _ = self
                .ctl
                .send(Control::Submit { command_id: o.command_id, session_id: o.session_id, expected_generation: o.expected_generation, text: o.text })
                .await;
        }
    }

    pub async fn status(&self) -> Value {
        let link = self.link.lock().unwrap().clone();
        let (count, unfinished) = {
            let ob = self.outbox.lock().unwrap();
            (ob.entries.len(), ob.entries.iter().filter(|o| !o.terminal()).count())
        };
        let controller = match self.status_source.clone() {
            Some(src) => tokio::task::spawn_blocking(move || controller_status(&src)).await.unwrap_or_else(|e| json!({"error": e.to_string()})),
            None => json!({"configured": false, "note": "no --vm/--data: this host was not told which controller box to read"}),
        };
        let device = self.voice_device.lock().unwrap().clone().map(|f| f()).unwrap_or(Value::Null);
        let backend_caps = backend_capabilities(link.backend.as_deref());
        json!({
            "host": {"pid": std::process::id(), "uptime_ms": self.started.elapsed().as_millis() as u64, "voice": self.voice,
                     "voice_policy": self.voice_policy.lock().unwrap().clone(),
                     "voice_device": device,
                     "mic": mic_status(&self.urgent.mute.snapshot()),
                     "call": self.call.lock().unwrap().clone().map(|c| c.status(&self.urgent.mute.snapshot())).unwrap_or(Value::Null),
                     "backend_capabilities": backend_caps,
                     "native_stop": "SIGUSR1 or Ctrl-X in the host terminal stops speech and cancels (a call stays up); SIGINT, SIGTERM or q quits"},
            "link": link,
            "commands": {"known": count, "unfinished": unfinished},
            "deployment": controller["deployment"],
            "self_repo": controller["self_repo"],
            "controller": controller["controller"],
        })
    }
}

pub fn mic_status(f: &crate::voice::FenceState) -> Value {
    let settled = !f.loop_running || f.applied_gen == f.generation;
    let state = match (f.requested, settled) {
        (true, _) if f.effective_muted() => "muted",
        (true, _) => "muting",
        (false, true) => "unmuted",
        (false, false) => "unmuting",
    };
    let ago = |t: Option<Instant>| t.map(|t| t.elapsed().as_millis() as u64);
    json!({
        "state": state,
        "muted": state == "muted",
        "requested": {"muted": f.requested, "generation": f.generation, "ms_ago": ago(f.requested_at)},
        "applied": {"muted": f.applied, "generation": f.applied_gen, "ms_ago": ago(f.applied_at),
                    "by": if f.loop_running { "the voice loop" } else { "no voice loop or writer is running: nothing is sent, and the next loop starts with the fence applied" },
                    "in_flight_at_fence": f.in_flight_at_fence},
        "in_flight_now": f.reserved,
        "writer_abort_unconfirmed": f.writer_abort_unconfirmed,
        "revoked_chunks": f.revoked_chunks,
        "partial_samples_discarded": f.prefix_samples_discarded,
        "while_muted": "no microphone audio is sent (no chunks, no silence frames); input only, the speaker keeps playing",
        "device": "stays open while muted (local VAD and echo probe only; they never cut the speaker while muted)",
        "controls": "stdin `mute on` / `mute off`, terminal key m, the app menu",
    })
}

pub fn controller_status(src: &StatusSource) -> Value {
    let layout = rth_deploy::controller::Layout::new(&src.data);
    let dir = layout.box_dir(&src.vm);
    let deployment = match rth_deploy::record::load_unlocked(&dir) {
        Ok(Some(rec)) => {
            let active = rec.active.as_ref().map(|c| {
                json!({
                    "deployment_id": c.deployment.deployment_id,
                    "kind": c.deployment.kind,
                    "artifact_id": c.deployment.artifact.artifact_id,
                    "manifest_sha256": c.deployment.artifact.manifest_sha256,
                    "attestation": c.deployment.artifact.attestation,
                    "candidate": c.deployment.candidate,
                    "record_generation": c.generation,
                    "receipt": {
                        "sha256": c.receipt.sha256(),
                        "boot_id": c.receipt.boot_id,
                        "pid": c.receipt.pid,
                        "exe_sha256": c.receipt.exe_sha256,
                        "backend": c.receipt.backend,
                        "backend_source": c.receipt.backend_source,
                        "identity_source": c.receipt.identity_source,
                        "instance_id": c.receipt.instance_id,
                        "session_id": c.receipt.session_id,
                        "hello_deployment_id": c.receipt.hello_deployment_id,
                        "checked_utc_ms": c.receipt.checked_utc_ms,
                    },
                })
            });
            json!({"vm": src.vm, "record_generation": rec.generation, "active": active,
                   "pending": rec.pending.as_ref().map(|p| json!({"deployment_id": p.deployment.deployment_id, "phase": p.phase}))})
        }
        Ok(None) => json!({"vm": src.vm, "active": null, "note": "no activation record: this box is not controller-managed"}),
        Err(e) => json!({"vm": src.vm, "error": e.to_string()}),
    };
    let self_repo = match rth_deploy::selfrepo::load_pin(&dir) {
        Ok(Some(pin)) => match rth_deploy::selfrepo::snapshot(&pin) {
            Ok(s) => json!({"head": s.head, "dirty": s.dirty, "dirty_paths": s.dirty_paths, "dirty_sha256": s.dirty_sha256}),
            Err(e) => json!({"error": e.to_string()}),
        },
        Ok(None) => json!({"head": null, "note": "this box has no self-repo"}),
        Err(e) => json!({"error": e.to_string()}),
    };
    let recreate = rth_deploy::controller::pending_recreate(&dir).map(|p| json!({"phase": p["phase"], "target_artifact": p["target_artifact"], "original_deployment": p["original_deployment"]}));
    json!({"deployment": deployment, "self_repo": self_repo, "controller": {"data": src.data, "vm": src.vm, "read": "lock-free (activation.json is replaced atomically)", "recreate_pending": recreate}})
}

pub struct Link {
    pub reader: BufReader<OwnedReadHalf>,
    pub write: OwnedWriteHalf,
    pub stamper: Stamper,
}

pub async fn connect(agent: SocketAddr, label: &str, shared: &Shared) -> Result<Link> {
    let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(agent)).await.context("connect timeout")??;
    let _ = stream.set_nodelay(true);
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let line = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut reader, &mut buf)).await.context("no hello")?.map_err(|e| anyhow!("{e}"))?;
    let hello = Envelope::parse_from(&line, Source::Guest).map_err(|e| anyhow!("hello: {e}"))?;
    let Event::Hello { protocol: PROTOCOL_VERSION, boot_id, backend, deployment, instance, command_generation, .. } = hello.event else {
        bail!("first frame was not a v{PROTOCOL_VERSION} hello")
    };

    let expect = shared.expect.lock().unwrap().clone();
    if let Some(x) = expect
        && let Err(why) = x.check(backend.as_deref(), &boot_id)
    {
        shared.link.lock().unwrap().state = "wrong_agent".into();
        bail!("refusing the agent at {agent}: {why} (not attached)");
    }
    let producer = rth_protocol::producer_name("host");
    let mut stamper = Stamper::new(Source::Host, producer, Box::new(MonotonicClock::start()));
    let attach = stamper.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: label.into(), role: Role::Controller }, None);
    write.write_all(attach.to_line().as_bytes()).await?;
    loop {
        let line = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut reader, &mut buf)).await.context("no ready")?.map_err(|e| anyhow!("{e}"))?;
        let Some(env) = Envelope::parse_known(&line, Source::Guest).map_err(|e| anyhow!("{e}"))? else {
            eprintln!("rth-host: skipped an agent event of unknown kind: {}", crate::voice::skipped_kind(&line));
            continue;
        };
        match env.event {
            Event::Ready { attachment_id, role } if attachment_id == label => {
                if role != Role::Controller {
                    let mut l = shared.link.lock().unwrap();
                    l.state = "observer_only".into();
                    bail!("admitted as observer: another process holds the controller lease");
                }
                let mut l = shared.link.lock().unwrap();
                l.state = "up".into();
                l.boot_id = Some(boot_id);
                l.backend = backend;
                l.deployment = deployment.map(|d| serde_json::to_value(d).unwrap_or_default());
                l.instance_id = instance.as_ref().map(|i| i.instance_id.clone());
                l.session_id = instance.as_ref().map(|i| i.session_id.clone());
                l.context = instance.as_ref().map(|i| i.context.clone());
                l.commands_supported = command_generation.is_some() && instance.is_some();
                l.command_generation = command_generation;
                l.connects += 1;
                l.last_error = None;
                l.since_utc_ms = rth_protocol::utc_ms();
                return Ok(Link { reader, write, stamper });
            }
            Event::Rejected { reason } => bail!("attach rejected: {reason}"),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_link(
    shared: Arc<Shared>,
    agent: SocketAddr,
    label: String,
    capture: &mut dyn Capture,
    mut playout: impl FnMut(u64) -> Box<dyn Playout>,
    opts: VoiceOptions,
    mut ctl: mpsc::Receiver<Control>,
) -> Result<()> {
    let mut backoff = Duration::from_millis(250);
    let mut n = 0u64;
    loop {
        let attempt = tokio::select! {
            biased;
            _ = shared.urgent.notified() => {
                if shared.urgent.take() & Urgent::QUIT != 0 {
                    return quit(&shared, "quit while connecting");
                }
                shared.note("stop_without_link", json!({"note": "nothing is playing while the link is down"}));
                continue;
            }
            r = connect(agent, &label, &shared) => r,
            c = ctl.recv() => match c {
                None | Some(Control::Quit) => return quit(&shared, "quit while connecting"),
                Some(other) => { shared.note("control_without_link", json!({"control": control_name(&other)})); continue; }
            },
        };
        let link = match attempt {
            Ok(link) => link,
            Err(e) => {
                {
                    let mut l = shared.link.lock().unwrap();
                    if l.state != "observer_only" && l.state != "wrong_agent" {
                        l.state = "down".into();
                    }
                    l.last_error = Some(e.to_string());
                }
                shared.note("link_down", json!({"error": e.to_string()}));
                let wait = tokio::time::sleep(backoff);
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        biased;
                        _ = shared.urgent.notified() => {
                            if shared.urgent.take() & Urgent::QUIT != 0 {
                                return quit(&shared, "quit while the link was down");
                            }
                            shared.note("stop_without_link", json!({"note": "nothing is playing while the link is down"}));
                        }
                        _ = &mut wait => break,
                        c = ctl.recv() => match c {
                            None | Some(Control::Quit) => return quit(&shared, "quit while the link was down"),
                            Some(other) => shared.note("control_without_link", json!({"control": control_name(&other)})),
                        },
                    }
                }
                backoff = (backoff * 2).min(Duration::from_secs(4));
                continue;
            }
        };
        backoff = Duration::from_millis(250);
        n += 1;
        let l = shared.link.lock().unwrap().clone();
        shared.note("link_up", json!({"boot_id": l.boot_id, "session_id": l.session_id, "command_generation": l.command_generation, "backend": l.backend}));

        let (notes_tx, mut notes_rx) = mpsc::unbounded_channel::<Note>();
        let fan = shared.clone();
        let fan_task = tokio::spawn(async move {
            while let Some(n) = notes_rx.recv().await {
                fan.on_note(&n);
            }
        });
        let resend = shared.clone();
        tokio::spawn(async move { resend.resend_unfinished().await });
        let Link { mut reader, write, stamper } = link;
        let mut speaker = playout(n);
        let r = drive_with(&mut reader, write, stamper, capture, speaker.as_mut(), opts.clone(), &mut ctl, notes_tx, &shared.urgent).await;
        let _ = fan_task.await;
        match r {
            Ok(summary) => {
                shared.note(
                    "quit",
                    json!({"devices_stopped_after_ms": summary.devices_stopped_after.as_secs_f64() * 1e3, "writer_aborted": summary.writer.aborted, "playout": summary.playout}),
                );
                shared.link.lock().unwrap().state = "quit".into();
                shared.wake_all("host quit");
                return Ok(());
            }
            Err(e) => {
                {
                    let mut l = shared.link.lock().unwrap();
                    l.state = "down".into();
                    l.last_error = Some(e.to_string());
                }
                shared.note("link_down", json!({"error": e.to_string()}));
                shared.wake_all(&e.to_string());
            }
        }
    }
}

fn quit(shared: &Shared, why: &str) -> Result<()> {
    shared.link.lock().unwrap().state = "quit".into();
    shared.note("quit", json!({"why": why}));
    shared.wake_all(why);
    Ok(())
}

pub fn backend_capabilities(backend: Option<&str>) -> Value {
    match backend {
        Some(b) if b.starts_with("gemini") => json!({
            "backend": b,
            "activity_admission": {"automatic_server_vad": true, "manual_activity_start_end": false,
                                   "note": "realtimeInputConfig.automaticActivityDetection; manual activityStart/activityEnd is supported by the API but not wired"},
            "generation_cancellation": {"per_turn_cancel": false, "note": "local fence only; the provider generates to its own turnComplete"},
            "provider_interrupts_on_speech": true,
            "context_truncation": {"supported": false, "note": "contextWindowCompression slidingWindow only; no client-side truncation to the played sample"},
            "resumable_playout": false,
        }),
        Some(b) => json!({"backend": b, "activity_admission": {"automatic_server_vad": false, "manual_activity_start_end": false},
                          "generation_cancellation": {"per_turn_cancel": true}, "context_truncation": {"supported": false}, "resumable_playout": false}),
        None => Value::Null,
    }
}

fn control_name(c: &Control) -> &'static str {
    match c {
        Control::Talk(_) => "talk",
        Control::Cancel => "cancel",
        Control::EndStream => "end_stream",
        Control::Text(_) => "text",
        Control::Submit { .. } => "submit",
        Control::Send(_) => "send",
        Control::Quit => "quit",
    }
}

pub struct Endpoint {
    pub listen: SocketAddr,
    pub token: String,
}

pub fn token(path: &Path) -> Result<String> {
    if let Ok(t) = std::fs::read_to_string(path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact_sync(&mut bytes)?;
    let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600).open(path)?;
    f.write_all(t.as_bytes())?;
    f.sync_all()?;
    Ok(t)
}

trait ReadExactSync {
    fn read_exact_sync(&mut self, buf: &mut [u8]) -> std::io::Result<()>;
}

impl ReadExactSync for std::fs::File {
    fn read_exact_sync(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        std::io::Read::read_exact(self, buf)
    }
}

fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub async fn serve(shared: Arc<Shared>, listener: TcpListener, token: String) {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let token = Arc::new(token);
    loop {
        let Ok((stream, peer)) = listener.accept().await else { continue };
        if !peer.ip().is_loopback() {
            continue;
        }
        let (shared, token) = (shared.clone(), token.clone());
        tokio::spawn(async move {
            let _ = tokio::time::timeout(ADMISSION_WAIT + Duration::from_secs(5), handle(shared, stream, &token, port)).await;
        });
    }
}

async fn handle(shared: Arc<Shared>, mut stream: TcpStream, token: &str, port: u16) -> Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let head_end = loop {
        let mut chunk = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await??;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 * 1024 {
            return respond(&mut stream, 431, json!({"error": "headers too large"})).await;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut req = lines.next().unwrap_or("").split(' ');
    let (method, path) = (req.next().unwrap_or(""), req.next().unwrap_or(""));
    let headers: HashMap<String, String> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();

    let host_ok = headers.get("host").is_some_and(|h| h == &format!("127.0.0.1:{port}") || h == &format!("localhost:{port}"));
    if !host_ok {
        return respond(&mut stream, 421, json!({"error": "loopback host only"})).await;
    }
    if headers.contains_key("origin") {
        return respond(&mut stream, 403, json!({"error": "browser requests are refused; go through DSH"})).await;
    }
    let auth = headers.get("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
    if !eq_ct(auth.as_bytes(), token.as_bytes()) {
        return respond(&mut stream, 401, json!({"error": "bad or missing bearer token"})).await;
    }
    let len: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    if len > 64 * 1024 {
        return respond(&mut stream, 413, json!({"error": "body too large"})).await;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let mut chunk = vec![0u8; len - body.len()];
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await??;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    let json_body = || serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let (code, v) = match (method, path) {
        ("GET", "/v1/status") => (200, shared.status().await),
        ("GET", "/v1/commands") => (200, json!({"commands": shared.outbox.lock().unwrap().entries.clone()})),
        ("POST", "/v1/commands") => {
            let b = json_body();
            match (b["command_id"].as_str(), b["text"].as_str()) {
                (Some(id), Some(text)) => shared.submit(id, text).await,
                _ => (400, json!({"error": "body must be {command_id, text}"})),
            }
        }
        ("GET", p) if p.starts_with("/v1/replies/") => {
            let id = &p["/v1/replies/".len()..];
            match shared.reply(id) {
                Some(reply) => (200, json!({"reply": reply})),
                None => (404, json!({"error": "no recorded reply with that id (unknown or evicted)"})),
            }
        }
        ("POST", "/v1/cancel") => {
            shared.native_intent(false, "endpoint");
            (200, json!({"ok": true, "note": "local stop requested; the wire cancel is best-effort"}))
        }
        ("POST", "/v1/call") => match json_body()["call"].as_str() {
            Some(a @ ("start" | "end")) => {
                let r = shared.call_request(a == "start", "endpoint");
                (if r.get("refused").is_some() { 409 } else { 202 }, r)
            }
            _ => (400, json!({"error": "body must be {\"call\": \"start\" | \"end\"}"})),
        },

        ("POST", p) if p.starts_with("/v1/tasks/") => shared.task_update(&p["/v1/tasks/".len()..], json_body()).await,
        ("POST", "/v1/executor") => shared.executor_status(json_body()).await,
        ("POST", "/v1/launches") => shared.register_launch(&json_body()),
        ("POST", "/v1/mute") => match json_body()["muted"].as_bool() {
            Some(on) => {
                let was = shared.mute_request(on, "endpoint");
                (202, json!({"request": {"muted": on}, "was_requested": was, "note": "a request: host.mic in GET /v1/status says when it is applied"}))
            }
            None => (400, json!({"error": "body must be {\"muted\": true | false}"})),
        },
        _ => (404, json!({"error": "no such route"})),
    };
    respond(&mut stream, code, v).await
}

async fn respond(stream: &mut TcpStream, code: u16, v: Value) -> Result<()> {
    let body = v.to_string();
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        202 => "Accepted",
        409 => "Conflict",
        413 => "Payload Too Large",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    let _ = stream.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn outbox_is_durable_last_line_wins_and_a_torn_line_is_skipped() {
        let p = tmp("outbox").join("outbox.jsonl");
        let mut ob = Outbox::open(&p).unwrap();
        let o = Outgoing {
            command_id: "c1".into(),
            session_id: "s".into(),
            expected_generation: 0,
            text: "hi".into(),
            status: "pending".into(),
            generation: None,
            decision_id: None,
            response_id: None,
            result_text: None,
            detail: None,
            text_complete: None,
            truncated: None,
            source: None,
            observed: None,
            updated_utc_ms: 0,
            recovered: false,
        };
        ob.put(o).unwrap();
        ob.update("c1", |o| o.status = "admitted".into()).unwrap();
        drop(ob);
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"{\"command_id\":\"c1\",\"sta").unwrap();
        let ob = Outbox::open(&p).unwrap();
        assert_eq!(ob.entries.len(), 1);
        assert_eq!(ob.get("c1").unwrap().status, "admitted");
        assert!(!ob.get("c1").unwrap().terminal());
    }

    fn pending(id: &str) -> Outgoing {
        Outgoing {
            command_id: id.into(),
            session_id: "s".into(),
            expected_generation: 0,
            text: "hi".into(),
            status: "pending".into(),
            generation: None,
            decision_id: None,
            response_id: None,
            result_text: None,
            detail: None,
            text_complete: None,
            truncated: None,
            source: None,
            observed: None,
            updated_utc_ms: 0,
            recovered: false,
        }
    }

    #[test]
    fn a_torn_tail_is_cut_before_appending_and_the_next_record_survives_a_second_reopen() {
        let p = tmp("torn2").join("outbox.jsonl");
        Outbox::open(&p).unwrap().put(pending("a")).unwrap();
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(br#"{"command_id":"#).unwrap();
        let mut ob = Outbox::open(&p).unwrap();
        assert!(ob.repaired_bytes > 0);
        ob.put(pending("b")).unwrap();
        drop(ob);
        let ob = Outbox::open(&p).unwrap();
        assert_eq!(ob.repaired_bytes, 0);
        assert!(ob.get("a").is_some() && ob.get("b").is_some(), "both fsynced records are retained");
        assert!(ob.get("b").unwrap().recovered, "a pending record from an earlier run is marked recovered");
    }

    #[test]
    fn a_malformed_interior_record_refuses_the_open() {
        let p = tmp("interior").join("outbox.jsonl");
        Outbox::open(&p).unwrap().put(pending("a")).unwrap();
        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(b"not json\n").unwrap();
        assert!(Outbox::open(&p).is_err(), "an interior record that does not parse is never skipped");
    }

    #[test]
    fn a_failed_append_poisons_the_outbox_until_reopen_and_recovery_keeps_only_whole_records() {
        for partial in [true, false] {
            let p = tmp(if partial { "poison-partial" } else { "poison-sync" }).join("outbox.jsonl");
            let mut ob = Outbox::open(&p).unwrap();
            ob.fail_next = Some(partial);
            ob.put(pending("a")).unwrap_err();
            assert!(ob.poisoned().is_some());
            let len = std::fs::metadata(&p).unwrap().len();
            ob.put(pending("b")).unwrap_err();
            assert_eq!(std::fs::metadata(&p).unwrap().len(), len, "nothing appended after the failure");
            drop(ob);
            let mut ob = Outbox::open(&p).unwrap();

            assert_eq!(ob.get("a").is_some(), !partial);
            assert_eq!(ob.repaired_bytes > 0, partial);
            ob.put(pending("c")).unwrap();
            drop(ob);
            assert!(Outbox::open(&p).unwrap().get("c").is_some());
        }
    }

    #[test]
    fn native_intent_is_published_before_logging_and_never_through_the_command_channel() {
        let (ctl, _rx) = mpsc::channel::<Control>(1);
        ctl.try_send(Control::Text("fill".into())).unwrap();
        let p = tmp("intent").join("outbox.jsonl");
        let shared = Shared::new(ctl.clone(), Outbox::open(&p).unwrap(), "a".into(), "off".into(), None, None);
        assert!(ctl.try_send(Control::Cancel).is_err(), "the command channel is full");
        let stalled = shared.log.lock().unwrap();
        let t = Instant::now();
        shared.native_intent(false, "test");
        shared.native_intent(false, "test");
        shared.native_intent(true, "test");
        assert!(t.elapsed() < Duration::from_millis(5), "the intent waited: {:?}", t.elapsed());
        assert_eq!(shared.urgent.pending(), Urgent::STOP | Urgent::QUIT, "repeats coalesce; quit is kept");
        drop(stalled);
    }

    #[tokio::test]
    async fn mute_is_never_queued_and_status_shows_it() {
        let (ctl, _rx) = mpsc::channel::<Control>(1);
        ctl.try_send(Control::Text("fill".into())).unwrap();
        let shared = Shared::new(ctl, Outbox::open(&tmp("mute").join("outbox.jsonl")).unwrap(), "a".into(), "off".into(), None, None);
        assert_eq!(shared.status().await["host"]["mic"]["muted"], json!(false), "unmuted after launch");
        let t = Instant::now();
        assert!(!shared.mute_request(true, "test"), "was unmuted");
        assert!(t.elapsed() < Duration::from_millis(5));
        assert_eq!(shared.urgent.pending(), 0, "mute is not a stop or quit intent");
        let st = shared.status().await;

        assert_eq!((st["host"]["mic"]["state"].as_str(), st["host"]["mic"]["muted"].as_bool()), (Some("muted"), Some(true)));
        assert_eq!(st["host"]["mic"]["requested"]["muted"], json!(true));
        assert_eq!(st["host"]["mic"]["applied"]["muted"], json!(false), "requested and applied are reported apart");
        assert!(shared.mute_request(false, "test"), "was muted");
        assert_eq!(shared.status().await["host"]["mic"]["muted"], json!(false));
    }

    #[test]
    fn token_file_is_private_and_stable() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("token").join("token");
        let a = token(&p).unwrap();
        assert_eq!(a.len(), 64);
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(token(&p).unwrap(), a);
        assert!(eq_ct(a.as_bytes(), a.as_bytes()) && !eq_ct(a.as_bytes(), b"x"));
    }

    async fn http_get(port: u16, path: &str, token: Option<&str>) -> (u16, Value) {
        use tokio::io::AsyncWriteExt;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8(out).unwrap();
        let code = text.split(' ').nth(1).unwrap().parse().unwrap();
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("null");
        (code, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    fn reply_rig(name: &str) -> (Arc<Shared>, rth_protocol::Stamper) {
        let (ctl, _rx) = mpsc::channel::<Control>(8);
        let shared = Shared::new(ctl, Outbox::open(&tmp(name).join("outbox.jsonl")).unwrap(), "a".into(), "off".into(), None, None);
        shared.link.lock().unwrap().connects = 1;
        (shared, rth_protocol::Stamper::new(rth_protocol::Source::Guest, "guest-r", Box::new(rth_protocol::MonotonicClock::start())))
    }

    fn feed(shared: &Shared, g: &mut rth_protocol::Stamper, e: Event) {
        shared.on_agent(&g.stamp(e, None));
    }

    fn started(r: &str) -> Event {
        Event::ResponseStarted { response_id: r.into(), request_id: "mic-1".into(), utterance_id: None }
    }

    fn delta(r: &str, t: &str) -> Event {
        Event::AgentDelta { response_id: r.into(), text: t.into() }
    }

    fn done(r: &str) -> Event {
        Event::AgentDone { response_id: r.into(), status: ResponseStatus::Completed }
    }

    #[test]
    fn a_replace_starts_the_observed_text_over() {
        let mut r = OpenReplies::default();
        r.push("r1", 1, "Ana buys 7 pens. So 4.\n</thi");
        r.replace("r1", 1, "4");
        r.push("r1", 1, " euros.");
        assert_eq!(r.bytes, "4 euros.".len());
        let o = r.take("r1").unwrap();
        assert_eq!((o.text.as_str(), o.truncated, o.gap), ("4 euros.", false, false));
        assert_eq!(r.bytes, 0);
    }

    #[tokio::test]
    async fn replies_route_serves_recorded_terminals_only_and_needs_the_token() {
        let (shared, mut g) = reply_rig("replies");

        for e in [started("v1"), delta("v1", "Two plus two "), delta("v1", "is four."), done("v1")] {
            feed(&shared, &mut g, e);
        }

        feed(&shared, &mut g, Event::CommandAdmitted {
            command_id: "c9".into(),
            generation: 9,
            status: CommandStatus::Interrupted,
            duplicate: true,
            decision_id: None,
            response_id: Some("r9".into()),
            result_text: None,
            detail: Some("the effect may have started (unknown)".into()),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(shared.clone(), listener, "tok".into()));
        let (code, v) = http_get(port, "/v1/replies/v1", Some("tok")).await;
        assert_eq!(code, 200);
        assert_eq!(
            v["reply"],
            json!({"response_id": "v1", "status": "completed", "text": "Two plus two is four.", "source": "observed", "observed": "complete", "text_complete": true, "truncated": false})
        );
        let (code, v) = http_get(port, "/v1/replies/r9", Some("tok")).await;
        assert_eq!((code, v["reply"]["status"].as_str(), v["reply"]["detail"].as_str()), (200, Some("interrupted"), Some("the effect may have started (unknown)")));
        assert_eq!(v["reply"]["text_complete"], json!(false), "no recorded text is not complete text");
        assert_eq!(http_get(port, "/v1/replies/nope", Some("tok")).await.0, 404);
        assert_eq!(http_get(port, "/v1/replies/v1", None).await.0, 401);
        assert_eq!(http_get(port, "/v1/replies/v1", Some("wrong")).await.0, 401);

        for i in 0..REPLY_RING {
            shared.record_reply(json!({"response_id": format!("x{i}"), "status": "completed", "text": null, "source": "observed", "text_complete": false}));
        }
        assert_eq!(http_get(port, "/v1/replies/v1", Some("tok")).await.0, 404);
        assert_eq!(http_get(port, &format!("/v1/replies/x{}", REPLY_RING - 1), Some("tok")).await.0, 200);
    }

    #[test]
    fn a_reply_observed_across_a_reconnect_is_never_complete() {
        let (shared, mut g) = reply_rig("reconnect");
        feed(&shared, &mut g, started("r1"));
        feed(&shared, &mut g, delta("r1", "One, "));
        shared.link.lock().unwrap().connects += 1;
        feed(&shared, &mut g, delta("r1", "three."));
        feed(&shared, &mut g, done("r1"));
        let r = shared.reply("r1").unwrap();
        assert_eq!((r["text_complete"].as_bool(), r["observed"].as_str(), r["text"].as_str()), (Some(false), Some("partial"), Some("One, three.")));
        feed(&shared, &mut g, Event::CommandAdmitted {
            command_id: "c1".into(),
            generation: 1,
            status: CommandStatus::Completed,
            duplicate: true,
            decision_id: None,
            response_id: Some("r1".into()),
            result_text: Some("One, two, three.".into()),
            detail: None,
        });
        let r = shared.reply("r1").unwrap();
        assert_eq!((r["text_complete"].as_bool(), r["source"].as_str(), r["text"].as_str()), (Some(true), Some("command_log"), Some("One, two, three.")));

        shared.record_reply(json!({"response_id": "r1", "status": "completed", "text": "three.", "source": "observed", "observed": "suffix", "text_complete": false}));
        assert_eq!(shared.reply("r1").unwrap()["text"], json!("One, two, three."));

        feed(&shared, &mut g, delta("r2", "tail"));
        feed(&shared, &mut g, done("r2"));
        let r = shared.reply("r2").unwrap();
        assert_eq!((r["text_complete"].as_bool(), r["observed"].as_str()), (Some(false), Some("suffix")));

        feed(&shared, &mut g, started("r3"));
        feed(&shared, &mut g, Event::Log { level: "warn".into(), msg: "attachment x lagged; 3 events skipped (journal has them)".into() });
        feed(&shared, &mut g, done("r3"));
        assert_eq!(shared.reply("r3").unwrap()["text_complete"], json!(false));
    }

    #[test]
    fn a_terminal_only_reply_has_no_text() {
        let (shared, mut g) = reply_rig("terminal-only");
        feed(&shared, &mut g, done("r1"));
        let r = shared.reply("r1").unwrap();
        assert_eq!((r["text_complete"].as_bool(), r["observed"].as_str(), r["text"].clone()), (Some(false), Some("terminal_only"), Value::Null));
    }

    #[test]
    fn reply_text_bounds_mark_truncation_and_evict_open_responses() {
        let (shared, mut g) = reply_rig("bounds");
        feed(&shared, &mut g, started("big"));
        let chunk = "x".repeat(1024);
        for _ in 0..(REPLY_TEXT_BYTES / 1024 + 2) {
            feed(&shared, &mut g, delta("big", &chunk));
        }
        feed(&shared, &mut g, done("big"));
        let r = shared.reply("big").unwrap();
        assert_eq!((r["text_complete"].as_bool(), r["truncated"].as_bool()), (Some(false), Some(true)));
        assert!(r["text"].as_str().unwrap().len() <= REPLY_TEXT_BYTES);

        for i in 0..=OPEN_REPLIES {
            feed(&shared, &mut g, started(&format!("o{i}")));
        }
        feed(&shared, &mut g, done("o0"));
        assert_eq!(shared.reply("o0").unwrap()["observed"], json!("terminal_only"));
        feed(&shared, &mut g, done(&format!("o{OPEN_REPLIES}")));
        assert_eq!(shared.reply(&format!("o{OPEN_REPLIES}")).unwrap()["text_complete"], json!(true));
    }

    #[test]
    fn a_typed_reply_across_a_reconnect_is_incomplete_until_the_command_log_says_otherwise() {
        let (shared, mut g) = reply_rig("typed");
        let mut o = pending("c1");
        o.status = "admitted".into();
        shared.outbox.lock().unwrap().put(o).unwrap();
        feed(&shared, &mut g, Event::ResponseStarted { response_id: "r1".into(), request_id: "c1".into(), utterance_id: None });
        feed(&shared, &mut g, delta("r1", "One, "));
        shared.link.lock().unwrap().connects += 1;
        feed(&shared, &mut g, delta("r1", "three."));
        feed(&shared, &mut g, done("r1"));
        let rec = shared.outbox.lock().unwrap().get("c1").cloned().unwrap();
        assert_eq!(rec.result_text.as_deref(), Some("One, three."), "kept for compatibility");
        assert_eq!((rec.text_complete, rec.source.as_deref(), rec.observed.as_deref()), (Some(false), Some("observed"), Some("partial")));
        feed(&shared, &mut g, Event::CommandAdmitted {
            command_id: "c1".into(),
            generation: 1,
            status: CommandStatus::Completed,
            duplicate: true,
            decision_id: None,
            response_id: Some("r1".into()),
            result_text: Some("One, two, three.".into()),
            detail: None,
        });
        let rec = shared.outbox.lock().unwrap().get("c1").cloned().unwrap();
        assert_eq!((rec.result_text.as_deref(), rec.text_complete, rec.source.as_deref()), (Some("One, two, three."), Some(true), Some("command_log")));

        let ob = Outbox::open(&std::env::temp_dir().join(format!("rth-host-typed-{}", std::process::id())).join("outbox.jsonl")).unwrap();
        assert_eq!(ob.get("c1").unwrap().text_complete, Some(true));
    }

    #[test]
    fn end_stream_is_admitted_only_for_a_scripted_mic_and_never_blocks() {
        let (ctl, mut rx) = mpsc::channel::<Control>(1);
        let shared = Shared::new(ctl, Outbox::open(&tmp("endstream").join("outbox.jsonl")).unwrap(), "a".into(), "off".into(), None, None);
        assert_eq!(shared.end_stream_request(false), "ignored");
        assert!(rx.try_recv().is_err(), "nothing queued outside --mic-wav");
        assert_eq!(shared.end_stream_request(true), "admitted");
        let t = Instant::now();
        assert_eq!(shared.end_stream_request(true), "refused", "a full channel refuses instead of waiting");
        assert!(t.elapsed() < Duration::from_millis(5));

        shared.native_intent(true, "test");
        assert_eq!(shared.urgent.pending() & Urgent::QUIT, Urgent::QUIT);
        assert!(matches!(rx.try_recv(), Ok(Control::EndStream)));
    }

    async fn http_post(port: u16, path: &str, token: Option<&str>, body: &str) -> (u16, Value) {
        use tokio::io::AsyncWriteExt;
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        s.write_all(format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8(out).unwrap();
        let code = text.split(' ').nth(1).unwrap().parse().unwrap();
        (code, serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap_or("null")).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn call_and_mute_endpoints_are_requests_and_status_reads_back() {
        let (ctl, _rx) = mpsc::channel::<Control>(8);
        let shared = Shared::new(ctl, Outbox::open(&tmp("call").join("outbox.jsonl")).unwrap(), "a".into(), "test".into(), None, None);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(shared.clone(), listener, "tok".into()));
        assert_eq!(http_post(port, "/v1/call", Some("tok"), r#"{"call":"start"}"#).await.0, 409, "no call owner: refused, never success-shaped");
        let devices = crate::call::CallDevices {
            capture: Box::new(crate::devices::NullCapture),
            playout: Box::new(|_| Box::new(crate::devices::NullPlayout::default()) as Box<dyn crate::devices::Playout>),
            rate: 24_000,
            describe: "null".into(),
        };
        let weak = Arc::downgrade(&shared);
        let call = crate::call::Call::new(Some(devices), shared.urgent.clone(), Box::new(move |k: &str, v| { if let Some(s) = weak.upgrade() { s.note_later(k, v) } }));
        *shared.call.lock().unwrap() = Some(call);
        assert_eq!(http_get(port, "/v1/status", Some("tok")).await.1["host"]["call"]["state"], json!("off"));
        assert_eq!(http_post(port, "/v1/call", None, r#"{"call":"start"}"#).await.0, 401);
        assert_eq!(http_post(port, "/v1/call", Some("tok"), r#"{"call":"go"}"#).await.0, 400);
        let (code, v) = http_post(port, "/v1/call", Some("tok"), r#"{"call":"start"}"#).await;
        assert_eq!((code, v["desired"].as_str()), (202, Some("on")));
        let st = http_get(port, "/v1/status", Some("tok")).await.1;
        assert_eq!((st["host"]["call"]["state"].as_str(), st["host"]["call"]["desired"].as_str()), (Some("starting"), Some("on")), "{}", st["host"]["call"]);
        let (code, v) = http_post(port, "/v1/mute", Some("tok"), r#"{"muted":true}"#).await;
        assert_eq!((code, v["was_requested"].as_bool()), (202, Some(false)));
        assert_eq!(http_get(port, "/v1/status", Some("tok")).await.1["host"]["mic"]["state"], json!("muted"));
        let (code, v) = http_post(port, "/v1/call", Some("tok"), r#"{"call":"end"}"#).await;
        assert_eq!((code, v["desired"].as_str()), (202, Some("off")));
        let deadline = Instant::now() + Duration::from_secs(3);
        let st = loop {
            let st = http_get(port, "/v1/status", Some("tok")).await.1;
            if st["host"]["call"]["state"] == "off" || Instant::now() > deadline {
                break st;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!((st["host"]["call"]["state"].as_str(), st["host"]["mic"]["state"].as_str()), (Some("off"), Some("muted")), "End is not unmute");
        assert_eq!(http_post(port, "/v1/mute", Some("tok"), r#"{"muted":"yes"}"#).await.0, 400);
    }

    async fn mock_agent(hellos: Vec<(Option<&'static str>, &'static str)>) -> (SocketAddr, Arc<Mutex<Vec<bool>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let attached = Arc::new(Mutex::new(Vec::new()));
        let seen = attached.clone();
        tokio::spawn(async move {
            let mut n = 0usize;
            loop {
                let Ok((stream, _)) = listener.accept().await else { return };
                let (backend, boot) = hellos[n.min(hellos.len() - 1)];
                n += 1;
                let (r, mut w) = stream.into_split();
                let mut g = Stamper::new(Source::Guest, "guest-mock", Box::new(MonotonicClock::start()));
                let hello = g.ephemeral(Event::Hello {
                    protocol: PROTOCOL_VERSION,
                    agent: "mock".into(),
                    version: "0".into(),
                    boot_id: boot.into(),
                    utc_ms_at_origin: 0,
                    journal: "x".into(),
                    backend: backend.map(str::to_string),
                    deployment: None,
                    instance: None,
                    command_generation: None,
                });
                let idx = {
                    let mut a = seen.lock().unwrap();
                    a.push(false);
                    a.len() - 1
                };
                let seen = seen.clone();
                tokio::spawn(async move {
                    if w.write_all(hello.to_line().as_bytes()).await.is_err() {
                        return;
                    }
                    let mut reader = BufReader::new(r);
                    let mut buf = Vec::new();
                    while let Ok(line) = read_frame(&mut reader, &mut buf).await {
                        if let Ok(Envelope { event: Event::Attach { attachment_id, .. }, .. }) = Envelope::parse_from(&line, Source::Host) {
                            seen.lock().unwrap()[idx] = true;
                            let ready = g.stamp(Event::Ready { attachment_id, role: Role::Controller }, None);
                            let _ = w.write_all(ready.to_line().as_bytes()).await;
                        }
                    }
                });
            }
        });
        (addr, attached)
    }

    fn expecting(name: &str, backend: Option<&str>, boot: Option<&str>) -> Arc<Shared> {
        let (ctl, _rx) = mpsc::channel::<Control>(8);
        let shared = Shared::new(ctl, Outbox::open(&tmp(name).join("outbox.jsonl")).unwrap(), "a".into(), "test".into(), None, None);
        if backend.is_some() || boot.is_some() {
            *shared.expect.lock().unwrap() = Some(Expect { backend: backend.map(str::to_string), boot: boot.map(str::to_string) });
        }
        shared
    }

    #[test]
    fn expect_matches_the_backend_name_before_at_and_the_exact_boot() {
        let x = Expect { backend: Some("openai-realtime".into()), boot: Some("b-1".into()) };
        assert!(x.check(Some("openai-realtime@v1"), "b-1").is_ok());
        assert!(x.check(Some("openai-realtime-x@v1"), "b-1").is_err(), "a longer name is another backend");
        assert!(x.check(Some("openai-realtime"), "b-1").is_err(), "no version part");
        assert!(x.check(Some("fake@v2"), "b-1").is_err());
        assert!(x.check(None, "b-1").is_err(), "an agent that names no backend");
        assert!(x.check(Some("openai-realtime@v1"), "b-2").is_err());
        assert!(Expect::default().check(None, "anything").is_ok(), "no expectations: nothing checked");
    }

    #[tokio::test]
    async fn an_agent_with_another_backend_is_refused_before_attach() {
        let (addr, attached) = mock_agent(vec![(Some("fake@v2"), "boot-1")]).await;
        let shared = expecting("x-backend", Some("openai-realtime"), Some("boot-1"));
        let e = connect(addr, "t", &shared).await.err().expect("refused").to_string();
        assert!(e.contains("backend") && e.contains("fake@v2"), "{e}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(*attached.lock().unwrap(), vec![false], "no Attach: no lease");
        assert_eq!(shared.link.lock().unwrap().state, "wrong_agent");
    }

    #[tokio::test]
    async fn an_agent_of_another_lifetime_is_refused_before_attach() {
        let (addr, attached) = mock_agent(vec![(Some("openai-realtime@v1"), "boot-2")]).await;
        let shared = expecting("x-boot", Some("openai-realtime"), Some("boot-1"));
        let e = connect(addr, "t", &shared).await.err().expect("refused").to_string();
        assert!(e.contains("boot-2") && e.contains("boot-1"), "{e}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(*attached.lock().unwrap(), vec![false]);
    }

    #[tokio::test]
    async fn the_same_boot_reconnecting_is_accepted_and_a_new_boot_never_is() {
        let (addr, attached) = mock_agent(vec![(Some("openai-realtime@v1"), "boot-1"), (Some("openai-realtime@v1"), "boot-1"), (Some("openai-realtime@v1"), "boot-9")]).await;
        let shared = expecting("x-reconnect", Some("openai-realtime"), Some("boot-1"));
        let first = connect(addr, "t", &shared).await.expect("the expected agent");
        drop(first);
        assert_eq!(shared.link.lock().unwrap().state, "up");
        let again = connect(addr, "t", &shared).await.expect("the same boot reconnecting");
        drop(again);
        let e = connect(addr, "t", &shared).await.err().expect("a new boot is refused").to_string();
        assert!(e.contains("boot-9"), "{e}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(*attached.lock().unwrap(), vec![true, true, false]);
        assert_eq!(shared.link.lock().unwrap().state, "wrong_agent");
    }

    #[tokio::test]
    async fn without_expectations_any_agent_is_attached_as_before() {
        let (addr, attached) = mock_agent(vec![(Some("fake@v2"), "boot-x")]).await;
        let shared = expecting("x-none", None, None);
        connect(addr, "t", &shared).await.expect("no expectations: unchanged behaviour");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(*attached.lock().unwrap(), vec![true]);
    }

    #[tokio::test]
    async fn a_wanted_call_never_opens_devices_against_a_refused_agent() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let (addr, attached) = mock_agent(vec![(Some("fake@v2"), "boot-other")]).await;
        let (ctl_tx, ctl_rx) = mpsc::channel::<Control>(8);
        let shared = Shared::new(ctl_tx.clone(), Outbox::open(&tmp("x-call").join("outbox.jsonl")).unwrap(), addr.to_string(), "test".into(), None, None);
        *shared.expect.lock().unwrap() = Some(Expect { backend: Some("openai-realtime".into()), boot: Some("boot-1".into()) });
        let opens = Arc::new(AtomicU64::new(0));
        let o = opens.clone();
        let devices = crate::call::CallDevices {
            capture: Box::new(crate::devices::NullCapture),
            playout: Box::new(move |_| {
                o.fetch_add(1, Ordering::SeqCst);
                Box::new(crate::devices::NullPlayout::default()) as Box<dyn crate::devices::Playout>
            }),
            rate: 24_000,
            describe: "null".into(),
        };
        let call = crate::call::Call::new(Some(devices), shared.urgent.clone(), Box::new(|_: &str, _| {}));
        *shared.call.lock().unwrap() = Some(call.clone());
        call.request(true, "test");
        let (s2, c2) = (shared.clone(), call.clone());
        let driver = tokio::spawn(async move {
            let mut capture = crate::call::CallCapture(c2.clone());
            let playout = move |_n: u64| -> Box<dyn Playout> { Box::new(crate::call::CallPlayout(c2.clone())) };
            run_link(s2, addr, "t".into(), &mut capture, playout, VoiceOptions::default(), ctl_rx).await
        });
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let st = call.status(&shared.urgent.mute.snapshot());
        assert_eq!(st["state"].as_str(), Some("starting"), "{st}");
        assert_eq!(st["reason"].as_str(), Some("waiting for the agent link: the devices open when the voice loop runs"), "{st}");
        assert_eq!(opens.load(Ordering::SeqCst), 0, "no speaker was opened");
        let attempts = attached.lock().unwrap().clone();
        assert!(!attempts.is_empty() && attempts.iter().all(|a| !a), "refused on every reconnect: {attempts:?}");
        {
            let l = shared.link.lock().unwrap();
            assert_eq!(l.state, "wrong_agent");
            assert!(l.last_error.as_deref().is_some_and(|e| e.contains("fake@v2")), "{:?}", l.last_error);
        }
        ctl_tx.send(Control::Quit).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(3), driver).await;
    }
}
