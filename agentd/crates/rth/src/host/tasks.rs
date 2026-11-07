

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rth_protocol::{Envelope, Event};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use super::Shared;
use crate::voice::Control;

#[cfg(not(test))]
pub const TASK_WAIT: Duration = Duration::from_secs(5);
#[cfg(test)]
pub const TASK_WAIT: Duration = Duration::from_millis(300);

#[derive(Default)]
pub struct TaskRoutes {

    ready: bool,
    next: u64,

    unwritten: Vec<(u64, String)>,
    by_envelope: HashMap<String, u64>,
    waiters: HashMap<u64, oneshot::Sender<(u16, Value)>>,
    registry: Option<LaunchRegistry>,
}

pub struct LaunchRegistry {
    path: PathBuf,
    launches: HashMap<String, Launch>,
}

#[derive(Clone)]
struct Launch {
    secret_sha256: String,
    exited: Option<Value>,
}

fn sha_hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

impl LaunchRegistry {

    pub fn open(path: &Path) -> std::io::Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let committed = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let bad = |n: usize, why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("launch registry {}: line {}: {why}", path.display(), n + 1));
        let mut launches: HashMap<String, Launch> = HashMap::new();

        let mut lines: Vec<&[u8]> = bytes[..committed].split(|&b| b == b'\n').collect();
        lines.pop();
        for (n, line) in lines.into_iter().enumerate() {
            let v: Value = serde_json::from_slice(line).map_err(|e| bad(n, &format!("not JSON ({e})")))?;
            let Some(id) = v["launch_id"].as_str() else { return Err(bad(n, "no launch_id")) };
            match v["op"].as_str() {
                Some("register") => {
                    let digest = v["secret_sha256"].as_str().unwrap_or("");
                    match launches.get(id) {
                        None => {
                            launches.insert(id.to_string(), Launch { secret_sha256: digest.to_string(), exited: None });
                        }
                        Some(l) if l.secret_sha256 == digest => {}
                        Some(_) => return Err(bad(n, "a second digest for a registered launch")),
                    }
                }
                Some("exited") => match launches.get_mut(id) {
                    Some(l) => l.exited = Some(v["receipt"].clone()),
                    None => return Err(bad(n, "an exit for an unregistered launch")),
                },
                _ => return Err(bad(n, "unknown op")),
            }
        }
        if committed < bytes.len() {
            let f = std::fs::OpenOptions::new().write(true).open(path)?;
            f.set_len(committed as u64)?;
            f.sync_all()?;
        }
        Ok(LaunchRegistry { path: path.to_path_buf(), launches })
    }

    fn append(&self, v: &Value) -> std::io::Result<()> {
        let existed = self.path.exists();
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&self.path)?;
        let before = f.metadata()?.len();
        if let Err(e) = f.write_all(format!("{v}\n").as_bytes()).and_then(|()| f.sync_data()) {
            let _ = f.set_len(before).and_then(|()| f.sync_all());
            return Err(e);
        }
        if !existed && let Some(dir) = self.path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }

    fn register(&mut self, launch_id: &str, secret_sha256: &str) -> (u16, Value) {
        let valid = !launch_id.is_empty() && launch_id.len() <= 128 && secret_sha256.len() == 64 && secret_sha256.bytes().all(|b| b.is_ascii_hexdigit());
        if !valid {
            return (400, json!({"error": "body must be {launch_id (1..=128 chars), secret_sha256 (64 hex)}"}));
        }
        match self.launches.get(launch_id) {
            Some(l) if l.secret_sha256 == secret_sha256 => (200, json!({"registered": launch_id, "duplicate": true})),
            Some(_) => (409, json!({"rejected": "this launch id is already registered with another secret"})),
            None => {
                if let Err(e) = self.append(&json!({"op": "register", "launch_id": launch_id, "secret_sha256": secret_sha256})) {
                    return (500, json!({"error": format!("launch registry write failed: {e}")}));
                }
                self.launches.insert(launch_id.into(), Launch { secret_sha256: secret_sha256.into(), exited: None });
                (200, json!({"registered": launch_id, "duplicate": false}))
            }
        }
    }

    fn exit(&mut self, b: &Value) -> Result<Value, (u16, Value)> {
        let id = b["launch_id"].as_str().unwrap_or("");
        let proof = b["launch_proof"].as_str().unwrap_or("");
        let Some(l) = self.launches.get(id).cloned() else { return Err((403, json!({"rejected": "unknown launch"}))) };
        if !super::eq_ct(sha_hex(proof.as_bytes()).as_bytes(), l.secret_sha256.as_bytes()) {
            return Err((403, json!({"rejected": "the launch proof does not match its registration"})));
        }
        let receipt = json!({"state": "exited", "launch_id": id, "pid": b["pid"], "code": b["code"], "signal": b["signal"], "proof_ok": true});
        if l.exited.is_none() {
            if let Err(e) = self.append(&json!({"op": "exited", "launch_id": id, "receipt": receipt})) {
                return Err((500, json!({"error": format!("launch registry write failed: {e}")})));
            }
            self.launches.get_mut(id).unwrap().exited = Some(receipt.clone());
        }
        Ok(receipt)
    }

    fn exited(&self) -> Vec<Value> {
        self.launches.values().filter_map(|l| l.exited.clone()).collect()
    }
}

impl Shared {

    pub fn set_launch_registry(&self, path: &Path) -> std::io::Result<()> {
        let r = LaunchRegistry::open(path)?;
        self.tasks.lock().unwrap().registry = Some(r);
        Ok(())
    }

    fn tasks_ready(&self) -> Result<(), (u16, Value)> {
        let link = self.link.lock().unwrap().clone();
        if link.state != "up" {
            return Err((503, json!({"error": format!("agent link is {}", link.state), "retry": true})));
        }
        if !self.tasks.lock().unwrap().ready {
            return Err((409, json!({"rejected": "this agent has no task ledger on this connection (delegation off, or a build without it)"})));
        }
        Ok(())
    }

    async fn forward(&self, ev: Event, wait: bool) -> Result<Option<oneshot::Receiver<(u16, Value)>>, (u16, Value)> {
        let content = serde_json::to_string(&ev).unwrap_or_default();
        let rx = {
            let mut t = self.tasks.lock().unwrap();
            t.next += 1;
            let token = t.next;
            t.unwritten.push((token, content));
            if wait {
                let (tx, rx) = oneshot::channel();
                t.waiters.insert(token, tx);
                Some(rx)
            } else {
                None
            }
        };
        if self.ctl.send(Control::Send(ev)).await.is_err() {
            return Err((503, json!({"error": "host is shutting down", "retry": true})));
        }
        Ok(rx)
    }

    pub async fn task_update(self: &Arc<Self>, task_id: &str, update: Value) -> (u16, Value) {
        if task_id.is_empty() || task_id.len() > 160 || update["task_id"].as_str() != Some(task_id) {
            return (400, json!({"error": "the body's task_id must equal the path's"}));
        }
        if let Err(e) = self.tasks_ready() {
            return e;
        }
        let ev = Event::TaskUpdate { task_id: task_id.into(), update };
        match self.forward(ev, true).await {
            Err(e) => e,
            Ok(Some(rx)) => match tokio::time::timeout(TASK_WAIT, rx).await {
                Ok(Ok(answer)) => answer,
                Ok(Err(_)) | Err(_) => (504, json!({"error": "no committed answer in time; resend the identical update", "retry": true})),
            },
            Ok(None) => unreachable!(),
        }
    }

    pub async fn executor_status(self: &Arc<Self>, mut b: Value) -> (u16, Value) {
        let status = if b["state"].as_str() == Some("exited") {
            let r = {
                let mut t = self.tasks.lock().unwrap();
                match t.registry.as_mut() {
                    Some(reg) => reg.exit(&b),
                    None => Err((503, json!({"error": "this host has no launch registry"}))),
                }
            };
            match r {
                Ok(receipt) => receipt,
                Err(e) => return e,
            }
        } else {
            if let Some(o) = b.as_object_mut() {
                o.remove("launch_proof");
            }
            b
        };
        let exited = status["state"].as_str() == Some("exited");
        if let Err(e) = self.tasks_ready() {

            return if exited { (202, json!({"accepted": true, "forwarded": false, "durable": true})) } else { e };
        }
        match self.forward(Event::ExecutorStatus { status }, false).await {
            Err(e) => e,
            Ok(_) => (202, json!({"accepted": true, "forwarded": true, "durable": exited})),
        }
    }

    pub fn register_launch(&self, b: &Value) -> (u16, Value) {
        let mut t = self.tasks.lock().unwrap();
        match t.registry.as_mut() {
            Some(reg) => reg.register(b["launch_id"].as_str().unwrap_or(""), b["secret_sha256"].as_str().unwrap_or("")),
            None => (503, json!({"error": "this host has no launch registry"})),
        }
    }

    pub(super) fn task_written(&self, env: &Envelope) {
        if !matches!(env.event, Event::TaskUpdate { .. } | Event::ExecutorStatus { .. }) {
            return;
        }
        let content = serde_json::to_string(&env.event).unwrap_or_default();
        let mut t = self.tasks.lock().unwrap();
        if let Some(i) = t.unwritten.iter().position(|(_, c)| *c == content) {
            let (token, _) = t.unwritten.remove(i);
            if t.waiters.contains_key(&token) {
                t.by_envelope.insert(env.id.clone(), token);
            }
        }
    }

    pub(super) fn task_from_agent(&self, env: &Envelope) -> Vec<Value> {
        let mut t = self.tasks.lock().unwrap();
        match &env.event {
            Event::TasksReady { .. } => {
                t.ready = true;
                return t.registry.as_ref().map(|r| r.exited()).unwrap_or_default();
            }
            Event::TaskState { record, .. } => {
                if let Some(cause) = &env.cause
                    && let Some(token) = t.by_envelope.remove(cause)
                    && let Some(w) = t.waiters.remove(&token)
                {
                    let _ = w.send((200, json!({"record": record})));
                }
            }
            Event::Rejected { reason } => {
                if let Some(cause) = &env.cause
                    && let Some(token) = t.by_envelope.remove(cause)
                    && let Some(w) = t.waiters.remove(&token)
                {
                    let _ = w.send((409, json!({"rejected": reason})));
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub(super) fn tasks_link_down(&self, why: &str) {
        let mut t = self.tasks.lock().unwrap();
        t.ready = false;
        t.unwritten.clear();
        t.by_envelope.clear();
        for (_, w) in t.waiters.drain() {
            let _ = w.send((503, json!({"error": format!("agent link lost before an answer: {why}"), "retry": true})));
        }
    }

    pub(super) fn reforward_exits(&self, receipts: Vec<Value>) {
        for r in receipts {
            let ev = Event::ExecutorStatus { status: r };
            let content = serde_json::to_string(&ev).unwrap_or_default();
            let mut t = self.tasks.lock().unwrap();
            t.next += 1;
            let token = t.next;
            t.unwritten.push((token, content));
            drop(t);
            let _ = self.ctl.try_send(Control::Send(ev));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use rth_protocol::{MonotonicClock, Source, Stamper};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;

    use super::super::{Outbox, serve};
    use super::*;
    use crate::voice::Note;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-host-tasks-{}-{name}-{}", std::process::id(), rth_protocol::utc_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn post(port: u16, path: &str, token: Option<&str>, body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        s.write_all(format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let text = String::from_utf8(out).unwrap();
        let code = text.split(' ').nth(1).unwrap().parse().unwrap();
        (code, serde_json::from_str(text.split("\r\n\r\n").nth(1).unwrap_or("null")).unwrap_or(Value::Null))
    }

    async fn rig(name: &str, answer: impl Fn(&Envelope) -> Option<Event> + Send + 'static) -> (Arc<Shared>, u16, PathBuf, Arc<Mutex<Vec<Envelope>>>) {
        let (ctl, mut rx) = mpsc::channel::<Control>(16);
        let dir = tmp(name);
        let shared = Shared::new(ctl, Outbox::open(&dir.join("outbox.jsonl")).unwrap(), "a".into(), "off".into(), None, None);
        shared.set_launch_registry(&dir.join("launches.jsonl")).unwrap();
        {
            let mut l = shared.link.lock().unwrap();
            l.state = "up".into();
            l.connects = 1;
        }
        let written = Arc::new(Mutex::new(Vec::new()));
        let (s2, w2) = (shared.clone(), written.clone());
        tokio::spawn(async move {
            let mut host = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
            let mut guest = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
            while let Some(c) = rx.recv().await {
                if let Control::Send(ev) = c {
                    let env = host.stamp(ev, None);
                    s2.on_note(&Note::Out { t: Instant::now(), env: env.clone() });
                    w2.lock().unwrap().push(env.clone());
                    if let Some(reply) = answer(&env) {
                        s2.on_agent(&guest.stamp(reply, Some(env.id.clone())));
                    }
                }
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(shared.clone(), listener, "tok".into()));
        (shared, port, dir, written)
    }

    fn ready(shared: &Shared) {
        let mut g = Stamper::new(Source::Guest, "guest-r", Box::new(MonotonicClock::start()));
        shared.on_agent(&g.stamp(Event::TasksReady { schema: "rth-tasks/v1".into() }, None));
    }

    const UPDATE: &str = r#"{"task_id":"b1-t1","executor_id":"dsh","incarnation":{"launch_id":"L1","pid":1,"plugin_instance":"p"},"seq":1,"kind":{"kind":"claim"}}"#;

    #[tokio::test]
    async fn a_task_update_is_forwarded_and_answered_by_the_committed_record_it_caused() {
        let (shared, port, _, written) = rig("ok", |env| match &env.event {
            Event::TaskUpdate { task_id, .. } => Some(Event::TaskState { task_id: task_id.clone(), record: json!({"task_id": task_id, "execution": "claimed", "outcome": "applied"}) }),
            _ => None,
        })
        .await;
        assert_eq!(post(port, "/v1/tasks/b1-t1", None, UPDATE).await.0, 401, "owner-token auth");
        assert_eq!(post(port, "/v1/tasks/b1-t1", Some("tok"), UPDATE).await.0, 409, "no ledger announced yet: nothing is forwarded");
        assert!(written.lock().unwrap().is_empty());
        ready(&shared);
        assert_eq!(post(port, "/v1/tasks/b1-t2", Some("tok"), UPDATE).await.0, 400, "path and body task ids must match");
        let (code, v) = post(port, "/v1/tasks/b1-t1", Some("tok"), UPDATE).await;
        assert_eq!(code, 200, "{v}");
        assert_eq!(v["record"]["execution"], "claimed");
        let w = written.lock().unwrap();
        assert_eq!(w.len(), 1);
        assert!(matches!(&w[0].event, Event::TaskUpdate { task_id, .. } if task_id == "b1-t1"));
    }

    #[tokio::test]
    async fn a_rejection_a_lost_answer_and_a_dropped_link_each_say_so() {
        let (shared, port, _, _) = rig("fail", |env| match &env.event {
            Event::TaskUpdate { update, .. } if update["seq"] == 1 => Some(Event::Rejected { reason: "unknown task".into() }),
            _ => None,
        })
        .await;
        ready(&shared);
        let (code, v) = post(port, "/v1/tasks/b1-t1", Some("tok"), UPDATE).await;
        assert_eq!((code, v["rejected"].as_str()), (409, Some("unknown task")));
        let silent = UPDATE.replace("\"seq\":1", "\"seq\":2");
        let (code, v) = post(port, "/v1/tasks/b1-t1", Some("tok"), &silent).await;
        assert_eq!((code, v["retry"].as_bool()), (504, Some(true)), "no answer: retry the identical update");

        let s2 = shared.clone();
        let h = tokio::spawn(async move { post(port, "/v1/tasks/b1-t1", Some("tok"), &silent).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        s2.wake_all("test");
        assert_eq!(h.await.unwrap().0, 503);
        assert_eq!(post(port, "/v1/tasks/b1-t1", Some("tok"), UPDATE).await.0, 409);
    }

    #[tokio::test]
    async fn launches_register_once_and_an_exit_receipt_is_authenticated_here_never_forwarded_with_its_proof() {
        let (shared, port, dir, written) = rig("launch", |_| None).await;
        let digest = sha_hex(b"s3cret");
        let reg = format!(r#"{{"launch_id":"L1","secret_sha256":"{digest}"}}"#);
        assert_eq!(post(port, "/v1/launches", Some("tok"), &reg).await.0, 200);
        assert_eq!(post(port, "/v1/launches", Some("tok"), &reg).await.1["duplicate"], true);
        let other = format!(r#"{{"launch_id":"L1","secret_sha256":"{}"}}"#, sha_hex(b"other"));
        assert_eq!(post(port, "/v1/launches", Some("tok"), &other).await.0, 409, "register-once");
        assert_eq!(post(port, "/v1/launches", Some("tok"), r#"{"launch_id":"L2","secret_sha256":"xyz"}"#).await.0, 400);

        assert_eq!(post(port, "/v1/executor", Some("tok"), r#"{"state":"exited","launch_id":"L1","launch_proof":"guess"}"#).await.0, 403);

        let (code, v) = post(port, "/v1/executor", Some("tok"), r#"{"state":"exited","launch_id":"L1","pid":7,"code":0,"launch_proof":"s3cret"}"#).await;
        assert_eq!((code, v["forwarded"].as_bool()), (202, Some(false)));
        assert!(written.lock().unwrap().is_empty());
        ready(&shared);
        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let w = written.lock().unwrap();
            assert_eq!(w.len(), 1, "re-forwarded on tasks_ready");
            let Event::ExecutorStatus { status } = &w[0].event else { panic!() };
            assert_eq!((status["proof_ok"].as_bool(), status["launch_id"].as_str()), (Some(true), Some("L1")));
            assert!(status.get("launch_proof").is_none(), "the proof never crosses the link");
            assert!(!w[0].to_line().contains("s3cret"));
        }
        assert!(!std::fs::read_to_string(dir.join("launches.jsonl")).unwrap().contains("s3cret"), "nor reaches the registry file");

        let (code, _) = post(port, "/v1/executor", Some("tok"), r#"{"executor_id":"dsh","available":true,"lease_ms":15000,"launch_proof":"x"}"#).await;
        assert_eq!(code, 202);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let w = written.lock().unwrap();
        let Event::ExecutorStatus { status } = &w.last().unwrap().event else { panic!() };
        assert!(status.get("launch_proof").is_none());

        let reopened = LaunchRegistry::open(&dir.join("launches.jsonl")).unwrap();
        assert_eq!(reopened.exited().len(), 1);
    }

    const D1: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn proof_digest(p: &str) -> String {
        sha_hex(p.as_bytes())
    }

    #[test]
    fn a_torn_tail_never_swallows_the_next_acknowledged_registration() {
        let path = tmp("torn-reg").join("launches.jsonl");
        std::fs::write(&path, br#"{"op":"#).unwrap();
        let mut r = LaunchRegistry::open(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"", "the torn tail is cut (and synced) before any append");
        assert_eq!(r.register("L2", D1).0, 200);
        let reopened = LaunchRegistry::open(&path).unwrap();
        assert!(reopened.launches.contains_key("L2"), "an acknowledged registration survives reopen");
    }

    #[test]
    fn a_torn_tail_after_committed_lines_keeps_them_and_an_acknowledged_exit_survives_reopen() {
        let path = tmp("torn-exit").join("launches.jsonl");
        let mut r = LaunchRegistry::open(&path).unwrap();
        assert_eq!(r.register("L1", &proof_digest("s1")).0, 200);
        drop(r);

        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"op":"exited","launch_id":"L1","rec"#).unwrap();
        drop(f);
        let mut r = LaunchRegistry::open(&path).unwrap();
        assert!(r.launches.contains_key("L1"), "committed lines are kept");
        let b = json!({"launch_id": "L1", "launch_proof": "s1", "pid": 7, "code": 0});
        assert!(r.exit(&b).is_ok());
        let reopened = LaunchRegistry::open(&path).unwrap();
        assert_eq!(reopened.exited().len(), 1, "an acknowledged exit proof never disappears (takeover needs it)");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n') && text.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()));

        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(format!(r#"{{"op":"register","launch_id":"L9","secret_sha256":"{D1}"}}"#).as_bytes()).unwrap();
        let r = LaunchRegistry::open(&path).unwrap();
        assert!(!r.launches.contains_key("L9"));
        assert_eq!(r.exited().len(), 1);
    }

    #[test]
    fn unexplained_interior_corruption_is_refused_not_skipped() {
        let path = tmp("interior").join("launches.jsonl");
        let good = format!(r#"{{"op":"register","launch_id":"L1","secret_sha256":"{D1}"}}"#);
        for bad in ["garbage", r#"{"op":"register"}"#, r#"{"op":"what","launch_id":"L1"}"#, r#"{"op":"exited","launch_id":"L7","receipt":{}}"#] {
            std::fs::write(&path, format!("{good}\n{bad}\n{good}\n")).unwrap();
            let e = LaunchRegistry::open(&path).err().unwrap_or_else(|| panic!("{bad} must be refused"));
            assert_eq!(e.kind(), std::io::ErrorKind::InvalidData, "{bad}");
        }
    }

    #[test]
    fn a_read_failure_is_an_error_and_a_missing_file_is_empty() {
        let dir = tmp("readfail");
        let as_dir = dir.join("launches.jsonl");
        std::fs::create_dir_all(&as_dir).unwrap();
        assert!(LaunchRegistry::open(&as_dir).is_err(), "never an empty registry");
        let missing = LaunchRegistry::open(&dir.join("absent.jsonl")).unwrap();
        assert!(missing.launches.is_empty());
    }
}
