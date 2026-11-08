

use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::net::TcpStream;
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const LATE: Duration = Duration::from_secs(60);

pub const WAIT: Duration = Duration::from_secs(5);

pub const ACTIONS: [&str; 5] = ["self_get", "self_set", "proposal_decide", "horizon_decide", "consent_set"];

#[derive(Debug, PartialEq)]
pub enum Failed {

    Timeout,

    Down,
}

#[derive(Default)]
struct Inner {
    conn: Option<Arc<Mutex<TcpStream>>>,

    waiting: HashMap<String, Vec<Waiter>>,

    timed_out: VecDeque<(String, Instant)>,
    next: u64,
}

struct Waiter {
    token: u64,

    frame_id: String,

    reply: &'static str,
    tx: mpsc::Sender<Value>,
}

#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
}

pub static LINK: LazyLock<Registry> = LazyLock::new(Registry::default);

impl Registry {
    pub fn attach(&self, conn: Arc<Mutex<TcpStream>>) {
        let mut i = self.inner.lock().unwrap();
        i.waiting.clear();
        i.conn = Some(conn);
    }

    pub fn close(&self, conn: &Arc<Mutex<TcpStream>>) {
        let mut i = self.inner.lock().unwrap();
        if i.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, conn)) {
            i.conn = None;
            i.waiting.clear();
        }
    }

    pub fn on_frame(&self, f: &Value) -> bool {
        let mut i = self.inner.lock().unwrap();
        let (kind, id) = (f["kind"].as_str().unwrap_or_default(), f["request_id"].as_str().unwrap_or_default());
        let take: Box<dyn Fn(&Waiter) -> bool> = match kind {
            "self_state" | "self_result" => Box::new(move |w: &Waiter| w.reply == kind),
            "rejected" => {
                let cause = f["cause"].as_str().unwrap_or_default();
                let Some(rid) = i.waiting.iter().find(|(_, ws)| ws.iter().any(|w| w.frame_id == cause)).map(|(r, _)| r.clone()) else {

                    i.timed_out.retain(|(_, at)| at.elapsed() < LATE);
                    let late = i.timed_out.iter().position(|(id, _)| id == cause);
                    return late.map(|n| i.timed_out.remove(n)).is_some();
                };
                return deliver(&mut i, &rid, &|w: &Waiter| w.frame_id == cause, f);
            }
            _ => return false,
        };
        deliver(&mut i, id, &*take, f);
        true
    }

    pub fn request(&self, kind: &str, body: Value, wait: Duration) -> Result<Value, Failed> {
        let id = body["request_id"].as_str().unwrap_or_default().to_string();
        let reply = if kind == "self_get" { "self_state" } else { "self_result" };

        let frame = crate::chat::host_frame(crate::chat::next_seq(), kind, body);
        let frame_id = frame["id"].as_str().unwrap_or_default().to_string();
        let (tx, rx) = mpsc::channel();
        let (conn, token) = {
            let mut i = self.inner.lock().unwrap();
            let conn = i.conn.clone().ok_or(Failed::Down)?;
            i.next += 1;
            let token = i.next;
            i.waiting.entry(id.clone()).or_default().push(Waiter { token, frame_id: frame_id.clone(), reply, tx });
            (conn, token)
        };
        let forget = |timed_out: bool| {
            let mut i = self.inner.lock().unwrap();
            if timed_out {
                if i.timed_out.len() == 64 {
                    i.timed_out.pop_front();
                }
                i.timed_out.push_back((frame_id.clone(), Instant::now()));
            }
            if let Some(w) = i.waiting.get_mut(&id) {
                w.retain(|w| w.token != token);
                if w.is_empty() {
                    i.waiting.remove(&id);
                }
            }
        };
        if writeln!(conn.lock().unwrap(), "{frame}").is_err() {
            forget(false);
            return Err(Failed::Down);
        }
        match rx.recv_timeout(wait) {
            Ok(f) => Ok(f),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                forget(true);
                Err(Failed::Timeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Failed::Down),
        }
    }
}

fn deliver(i: &mut Inner, request_id: &str, take: &dyn Fn(&Waiter) -> bool, f: &Value) -> bool {
    let Some(ws) = i.waiting.get_mut(request_id) else { return true };
    let (hit, keep): (Vec<Waiter>, Vec<Waiter>) = ws.drain(..).partition(|w| take(w));
    *ws = keep;
    if ws.is_empty() {
        i.waiting.remove(request_id);
    }
    for w in hit {
        let _ = w.tx.send(f.clone());
    }
    true
}

fn request_id(v: &Value) -> Result<String, String> {
    let id = v["request_id"].as_str().ok_or("request_id: required")?;
    let ok = (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then(|| id.to_string()).ok_or_else(|| "request_id: 1 to 64 of A-Z a-z 0-9 _ -".into())
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key].as_str().filter(|s| !s.is_empty()).ok_or_else(|| format!("{key}: a string is required"))
}

fn flag(v: &Value, key: &str) -> Result<bool, String> {
    v[key].as_bool().ok_or_else(|| format!("{key}: true or false"))
}

pub fn frame(action: &str, v: &Value, fresh: &dyn Fn() -> String) -> Result<(&'static str, Value), String> {
    Ok(match action {
        "self_get" => ("self_get", json!({"request_id": if v["request_id"].is_null() { fresh() } else { request_id(v)? }})),
        "self_set" => {
            let spec = v["spec"].as_object().ok_or("spec: the whole spec is required")?;
            ("self_set", json!({"request_id": request_id(v)?, "base_revision": text(v, "base_revision")?, "spec": spec}))
        }
        "proposal_decide" => ("proposal_decide", json!({"request_id": request_id(v)?, "proposal_id": text(v, "proposal_id")?, "approve": flag(v, "approve")?})),
        "horizon_decide" => ("horizon_decide", json!({"request_id": request_id(v)?, "horizon_id": text(v, "horizon_id")?, "approve": flag(v, "approve")?})),
        "consent_set" => {

            let copy_version = v["copy_version"].as_u64().filter(|n| u32::try_from(*n).is_ok()).ok_or("copy_version: an integer is required")?;
            let mut b = json!({"request_id": request_id(v)?, "grant": flag(v, "grant")?, "copy_version": copy_version});
            match text(v, "channel")? {
                "omni" => b["recipient"] = json!(text(v, "recipient")?),
                "notification" | "sleep" if v["recipient"].is_null() => {}
                "notification" | "sleep" => return Err("recipient: omni only".into()),
                _ => return Err("channel: notification, omni or sleep".into()),
            }
            b["channel"] = v["channel"].clone();
            ("consent_set", b)
        }
        _ => return Err(format!("{action}: not a Self action")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::time::Instant;

    fn pair() -> (Registry, BufReader<TcpStream>, Arc<Mutex<TcpStream>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let app = Arc::new(Mutex::new(TcpStream::connect(l.local_addr().unwrap()).unwrap()));
        let (agent, _) = l.accept().unwrap();

        agent.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let r = Registry::default();
        r.attach(app.clone());
        (r, BufReader::new(agent), app)
    }

    fn read(agent: &mut BufReader<TcpStream>) -> Value {
        let mut line = String::new();
        assert!(agent.read_line(&mut line).unwrap() > 0, "no frame");
        serde_json::from_str(line.trim_end()).unwrap()
    }

    const NO_FRESH: &dyn Fn() -> String = &|| "fresh-1".to_string();

    #[test]
    fn each_action_builds_its_typed_frame() {
        let (k, b) = frame("self_get", &json!({}), NO_FRESH).unwrap();
        assert_eq!((k, b), ("self_get", json!({"request_id": "fresh-1"})));
        let (k, b) = frame("self_set", &json!({"request_id": "r_1", "base_revision": "ab12", "spec": {"schema_version": 1}, "extra": 1}), NO_FRESH).unwrap();
        assert_eq!((k, b), ("self_set", json!({"request_id": "r_1", "base_revision": "ab12", "spec": {"schema_version": 1}})));
        let (k, b) = frame("proposal_decide", &json!({"request_id": "p1", "proposal_id": "prop-1", "approve": true}), NO_FRESH).unwrap();
        assert_eq!((k, b), ("proposal_decide", json!({"request_id": "p1", "proposal_id": "prop-1", "approve": true})));
        let (k, b) = frame("horizon_decide", &json!({"request_id": "h1", "horizon_id": "h-1", "approve": false}), NO_FRESH).unwrap();
        assert_eq!((k, b), ("horizon_decide", json!({"request_id": "h1", "horizon_id": "h-1", "approve": false})));
        let (_, b) = frame("consent_set", &json!({"request_id": "c1", "channel": "omni", "recipient": "r@s", "grant": true, "copy_version": 1}), NO_FRESH).unwrap();
        assert_eq!(b, json!({"request_id": "c1", "channel": "omni", "recipient": "r@s", "grant": true, "copy_version": 1}));
        let (_, b) = frame("consent_set", &json!({"request_id": "c2", "channel": "notification", "grant": false, "copy_version": 1}), NO_FRESH).unwrap();
        assert_eq!(b, json!({"request_id": "c2", "channel": "notification", "grant": false, "copy_version": 1}));
        let (_, b) = frame("consent_set", &json!({"request_id": "c3", "channel": "sleep", "grant": true, "copy_version": 2}), NO_FRESH).unwrap();
        assert_eq!(b, json!({"request_id": "c3", "channel": "sleep", "grant": true, "copy_version": 2}), "the nightly sleep's own consent");
    }

    #[test]
    fn malformed_actions_are_refused_before_a_frame() {
        for (a, v) in [
            ("self_set", json!({"base_revision": "ab", "spec": {}})),
            ("self_set", json!({"request_id": "r 1", "base_revision": "ab", "spec": {}})),
            ("self_set", json!({"request_id": "x".repeat(65), "base_revision": "ab", "spec": {}})),
            ("self_set", json!({"request_id": "r1", "spec": {}})),
            ("self_set", json!({"request_id": "r1", "base_revision": "ab", "spec": "name"})),
            ("self_get", json!({"request_id": "bad/id"})),
            ("proposal_decide", json!({"request_id": "p1", "proposal_id": "prop-1", "approve": "yes"})),
            ("proposal_decide", json!({"request_id": "p1", "approve": true})),
            ("horizon_decide", json!({"request_id": "h1", "approve": true})),
            ("consent_set", json!({"request_id": "c1", "channel": "omni", "grant": true, "copy_version": 1})),
            ("consent_set", json!({"request_id": "c1", "channel": "notification", "recipient": "r", "grant": true, "copy_version": 1})),
            ("consent_set", json!({"request_id": "c1", "channel": "sms", "grant": true, "copy_version": 1})),
            ("consent_set", json!({"request_id": "c1", "channel": "sleep", "recipient": "r", "grant": true, "copy_version": 2})),
            ("consent_set", json!({"request_id": "c1", "channel": "notification", "grant": true, "copy_version": "1"})),
            ("consent_set", json!({"request_id": "c1", "channel": "notification", "grant": true})),
            ("consent_set", json!({"request_id": "c1", "channel": "notification", "grant": true, "copy_version": 4_294_967_296u64})),
            ("self_preview", json!({"request_id": "x1", "spec": {}})),
        ] {
            assert!(frame(a, &v, NO_FRESH).is_err(), "{a} {v}");
        }
    }

    #[test]
    fn a_reply_is_matched_by_request_id_and_others_are_ignored() {
        let (r, mut agent, _w) = pair();
        let r = Arc::new(r);
        let r2 = r.clone();
        let t = std::thread::spawn(move || r2.request("self_get", json!({"request_id": "g1"}), WAIT));
        let f = read(&mut agent);
        assert_eq!((f["kind"].as_str(), f["request_id"].as_str(), f["source"].as_str()), (Some("self_get"), Some("g1"), Some("host")));

        assert!(!r.on_frame(&json!({"kind": "agent_delta", "request_id": "g1"})));
        assert!(r.on_frame(&json!({"kind": "self_state", "request_id": "other", "state": {}})));
        assert!(r.on_frame(&json!({"kind": "self_state", "request_id": "g1", "state": {"revision": "ab12"}})));
        let got = t.join().unwrap().unwrap();
        assert_eq!(got["state"]["revision"], "ab12");
    }

    #[test]
    fn a_self_reply_with_an_unknown_id_never_reaches_the_chat() {
        let r = Registry::default();
        assert!(r.on_frame(&json!({"kind": "self_result", "request_id": "nobody", "result": {"ok": {}}})));
        assert!(r.on_frame(&json!({"kind": "self_state", "state": {}})));
    }

    #[test]
    fn no_reply_times_out_and_the_waiter_is_removed() {
        let (r, mut agent, _w) = pair();
        let t0 = Instant::now();
        assert_eq!(r.request("self_set", json!({"request_id": "s1"}), Duration::from_millis(200)), Err(Failed::Timeout));
        assert!(t0.elapsed() >= Duration::from_millis(200));
        assert_eq!(read(&mut agent)["kind"], "self_set");
        assert!(r.inner.lock().unwrap().waiting.is_empty());
    }

    #[test]
    fn a_retry_resends_the_same_request_id_and_both_waiters_hear_the_reply() {
        let (r, mut agent, _w) = pair();
        let r = Arc::new(r);
        let spawn = |r: &Arc<Registry>| {
            let r = r.clone();
            std::thread::spawn(move || r.request("self_set", json!({"request_id": "s1", "base_revision": "ab", "spec": {}}), WAIT))
        };
        let first = spawn(&r);
        assert_eq!(read(&mut agent)["request_id"], "s1");
        let retry = spawn(&r);
        assert_eq!(read(&mut agent)["request_id"], "s1", "the retry carries the first attempt's id");
        assert!(r.on_frame(&json!({"kind": "self_result", "request_id": "s1", "result": {"ok": {"revision": "cd34"}}})));
        for t in [first, retry] {
            assert_eq!(t.join().unwrap().unwrap()["result"]["ok"]["revision"], "cd34");
        }
    }

    #[test]
    fn a_waiting_request_never_holds_the_write_lock() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let conn = Arc::new(Mutex::new(TcpStream::connect(l.local_addr().unwrap()).unwrap()));
        let (agent, _) = l.accept().unwrap();
        agent.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut agent = BufReader::new(agent);
        let r = Arc::new(Registry::default());
        r.attach(conn.clone());
        let r2 = r.clone();
        let t = std::thread::spawn(move || r2.request("self_get", json!({"request_id": "g1"}), WAIT));
        assert_eq!(read(&mut agent)["kind"], "self_get");

        let t0 = Instant::now();
        crate::chat::write_frame(&conn, "user_text", json!({"request_id": "q1", "text": "hi"})).unwrap();
        assert!(t0.elapsed() < Duration::from_secs(1));
        assert_eq!(read(&mut agent)["kind"], "user_text");
        r.on_frame(&json!({"kind": "self_state", "request_id": "g1", "state": {}}));
        assert!(t.join().unwrap().is_ok());
    }

    #[test]
    fn a_rejected_self_frame_goes_to_its_request_and_a_chat_rejection_does_not() {
        let (r, mut agent, _app) = pair();
        let r = Arc::new(r);
        let r2 = r.clone();
        let t0 = Instant::now();
        let t = std::thread::spawn(move || r2.request("self_get", json!({"request_id": "g1"}), WAIT));
        let sent = read(&mut agent);

        assert!(!r.on_frame(&json!({"kind": "rejected", "reason": "stale generation", "cause": "rth-app:999999"})));
        assert!(r.on_frame(&json!({"kind": "rejected", "reason": "no saved Self", "cause": sent["id"]})));
        let got = t.join().unwrap().unwrap();
        assert_eq!(got["kind"], "rejected");
        assert!(t0.elapsed() < Duration::from_secs(2), "answered at once, not at the timeout");
        assert!(r.inner.lock().unwrap().waiting.is_empty());
    }

    #[test]
    fn a_rejection_after_the_timeout_is_swallowed_once() {
        let (r, mut agent, _app) = pair();
        assert_eq!(r.request("self_get", json!({"request_id": "g1"}), Duration::from_millis(100)), Err(Failed::Timeout));
        let sent = read(&mut agent);
        let late = json!({"kind": "rejected", "reason": "no saved Self", "cause": sent["id"]});
        assert!(r.on_frame(&late), "late, and still not the chat's");
        assert!(!r.on_frame(&late), "kept once only");
    }

    #[test]
    fn a_reply_of_the_other_kind_leaves_the_waiter_waiting() {
        let (r, mut agent, _app) = pair();
        let r = Arc::new(r);
        let r2 = r.clone();
        let t = std::thread::spawn(move || r2.request("self_set", json!({"request_id": "s1", "base_revision": "ab", "spec": {}}), WAIT));
        read(&mut agent);
        assert!(r.on_frame(&json!({"kind": "self_state", "request_id": "s1", "state": {}})), "consumed, never the chat's");
        assert_eq!(r.inner.lock().unwrap().waiting.len(), 1, "a self_state doesn't answer a write");
        r.on_frame(&json!({"kind": "self_result", "request_id": "s1", "result": {"ok": {}}}));
        assert_eq!(t.join().unwrap().unwrap()["kind"], "self_result");
    }

    #[test]
    fn a_closed_connection_fails_every_waiter_at_once_and_later_requests_fast() {
        let (r, mut agent, app) = pair();
        let r = Arc::new(r);
        let r2 = r.clone();
        let t0 = Instant::now();
        let t = std::thread::spawn(move || r2.request("self_get", json!({"request_id": "g1"}), WAIT));
        assert_eq!(read(&mut agent)["kind"], "self_get");

        let (_, _, older) = pair();
        r.close(&older);
        assert!(r.inner.lock().unwrap().conn.is_some());
        r.close(&app);
        assert_eq!(t.join().unwrap(), Err(Failed::Down));
        assert_eq!(r.request("self_get", json!({"request_id": "g2"}), WAIT), Err(Failed::Down));
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    }
}
