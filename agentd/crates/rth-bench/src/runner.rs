

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use rth_voice::owner::{Heard, SpeechOwner};
use rth_voice::queue::PlayQueue;
use serde_json::json;

use rth_protocol::io::read_frame;
use rth_protocol::{Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::fixture::{Fixture, Op, Target, Trigger, Until};
use crate::timeline::{self as tl, ClosedBy, Rec};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);
const ATTACH_ATTEMPTS: u32 = 10;
const RETRY_GAP: Duration = Duration::from_millis(100);
const TRIGGER_TIMEOUT_MS: u64 = 3000;
const RAW_KEEP: usize = 512;

#[derive(Clone, Copy)]
pub struct RunClock {
    origin: Instant,
}

impl RunClock {
    pub fn start() -> Self {
        Self { origin: Instant::now() }
    }

    pub fn now_ns(&self) -> u64 {
        self.origin.elapsed().as_nanos() as u64
    }

    fn instant_at(&self, ns: u64) -> tokio::time::Instant {
        (self.origin + Duration::from_nanos(ns)).into()
    }
}

const fn ms(x: u64) -> u64 {
    x * 1_000_000
}

struct Conn {
    write: OwnedWriteHalf,
    reader: JoinHandle<()>,
    stamper: Stamper,

    tap: Tap,
    speaker: Option<Speaker>,
}

type Tap = Arc<Mutex<Option<mpsc::UnboundedSender<Envelope>>>>;

pub const SPEAKER_RATE: u32 = 24_000;

const DEVICE_PERIOD_MS: u64 = 10;

#[derive(Default, Clone)]
struct Acct {
    epoch: Option<u64>,
    received_samples: u64,
    queued_samples: u64,
    fenced_chunks: u64,
    first_rendered_ns: Option<u64>,

    rendered_at_clear: Option<u64>,
}

struct Speaker {
    owner: Arc<Mutex<SpeechOwner>>,
    queue: Arc<PlayQueue>,
    accts: Arc<Mutex<BTreeMap<String, Acct>>>,
    feed: JoinHandle<()>,
    device: JoinHandle<()>,
}

impl Speaker {
    fn start(conn: &str, clock: RunClock, tx: mpsc::UnboundedSender<Rec>) -> (Self, mpsc::UnboundedSender<Envelope>) {
        let queue = Arc::new(PlayQueue::with_seconds(SPEAKER_RATE, 30.0));
        let owner = Arc::new(Mutex::new(SpeechOwner::new(queue.clone())));
        let accts: Arc<Mutex<BTreeMap<String, Acct>>> = Arc::default();
        let (etx, mut erx) = mpsc::unbounded_channel::<Envelope>();
        let (o, a, c) = (owner.clone(), accts.clone(), conn.to_string());
        let ftx = tx.clone();
        let feed = tokio::spawn(async move {
            while let Some(env) = erx.recv().await {
                let mut owner = o.lock().unwrap();
                match &env.event {
                    Event::AgentAudio { response_id, chunk, sample_offset, sample_rate, format, data } => {
                        let n = base64::engine::general_purpose::STANDARD.decode(data).map(|b| b.len() as u64 / 2).unwrap_or(0);
                        let heard = owner.audio(response_id, *chunk, *sample_offset, *sample_rate, format, data);
                        let epoch = owner.epoch_of(response_id);
                        let mut accts = a.lock().unwrap();
                        let acct = accts.entry(response_id.clone()).or_default();
                        acct.received_samples += n;
                        acct.epoch = acct.epoch.or(epoch);
                        match heard {
                            Heard::Queued { .. } => acct.queued_samples += n,
                            Heard::Fenced => acct.fenced_chunks += 1,
                            _ => {}
                        }
                    }
                    _ => {
                        if let Some(cleared) = owner.on_envelope(&env) {
                            let t_ns = clock.now_ns();
                            let detail = json!({"kind": "cleared", "reason": cleared.reason.as_str(), "dropped_samples": cleared.dropped_samples,
                                "flush_ns": cleared.flushed.took_ns, "responses": cleared.responses.iter().map(|(r, _, _)| r.clone()).collect::<Vec<_>>()});
                            let _ = ftx.send(Rec::Note { t_ns, conn: Some(c.clone()), code: "speaker".into(), detail: detail.to_string() });
                        }
                    }
                }
            }
        });
        let (q, a, o, c) = (queue.clone(), accts.clone(), owner.clone(), conn.to_string());
        let device = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(DEVICE_PERIOD_MS));
            let mut out = vec![0i16; (SPEAKER_RATE as u64 * DEVICE_PERIOD_MS / 1000) as usize];
            loop {
                tick.tick().await;
                q.pull(&mut out);
                let _ = o.lock().unwrap().poll();
                let now = clock.now_ns();
                let mut accts = a.lock().unwrap();
                for (rid, acct) in accts.iter_mut() {
                    if acct.first_rendered_ns.is_none()
                        && let Some(e) = acct.epoch
                        && q.rendered(e).samples > 0
                    {
                        acct.first_rendered_ns = Some(now);
                        let detail = json!({"kind": "first_rendered", "response_id": rid, "t_ns": now});
                        let _ = tx.send(Rec::Note { t_ns: now, conn: Some(c.clone()), code: "speaker".into(), detail: detail.to_string() });
                    }
                }
            }
        });
        (Speaker { owner, queue, accts, feed, device }, etx)
    }

    fn barge_in(&self, clock: RunClock) -> (serde_json::Value, Vec<Event>) {
        let t0 = clock.now_ns();
        let mut owner = self.owner.lock().unwrap();
        let live = owner.live();
        let cleared = owner.barge_in();
        let t1 = clock.now_ns();
        let records = owner.poll();
        drop(owner);
        let mut accts = self.accts.lock().unwrap();
        for r in &live {
            if let Some(a) = accts.get_mut(r)
                && let Some(e) = a.epoch
            {
                a.rendered_at_clear = Some(self.queue.rendered(e).samples);
            }
        }
        let detail = json!({"kind": "barge_in", "t_ns": t0, "clear_wall_ns": t1 - t0, "live": live,
            "dropped_samples": cleared.as_ref().map(|c| c.dropped_samples).unwrap_or(0),
            "flush_ns": cleared.as_ref().map(|c| c.flushed.took_ns)});
        (detail, records)
    }

    fn summary(self) -> serde_json::Value {
        self.feed.abort();
        self.device.abort();
        let stats = self.queue.stats();
        let owner = self.owner.lock().unwrap();
        let accts = self.accts.lock().unwrap();
        let responses: Vec<serde_json::Value> = accts
            .iter()
            .map(|(rid, a)| {
                let played = a.epoch.map(|e| self.queue.rendered(e).samples).unwrap_or(0);
                json!({"response_id": rid, "epoch": a.epoch, "received_samples": a.received_samples, "queued_samples": a.queued_samples,
                    "played_samples": played, "fenced_chunks": a.fenced_chunks, "first_rendered_ns": a.first_rendered_ns,
                    "played_after_clear": a.rendered_at_clear.map(|at| played.saturating_sub(at))})
            })
            .collect();
        json!({"kind": "summary", "evidence": "synthetic_device", "device_rate": SPEAKER_RATE, "device_period_ms": DEVICE_PERIOD_MS,
            "responses": responses, "rendered_after_fence": stats.rendered_after_fence, "underrun_samples": stats.underrun_samples,
            "late_chunks_dropped": owner.stats.late_chunks_dropped, "clears": owner.stats.clears})
    }
}

fn spawn_reader(conn: String, mut reader: BufReader<OwnedReadHalf>, clock: RunClock, tx: mpsc::UnboundedSender<Rec>, tap: Tap) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = Vec::new();
        loop {
            match read_frame(&mut reader, &mut buf).await {
                Ok(line) => {
                    let rec = recv_rec(&conn, clock.now_ns(), &line);
                    if let Rec::Recv { env, .. } = &rec
                        && let Some(t) = tap.lock().unwrap().as_ref()
                    {
                        let _ = t.send(env.clone());
                    }
                    let _ = tx.send(rec);
                }
                Err(e) => {
                    let _ = tx.send(Rec::Closed { t_ns: clock.now_ns(), conn, by: ClosedBy::Peer, detail: e.to_string() });
                    return;
                }
            }
        }
    })
}

fn recv_rec(conn: &str, t_ns: u64, line: &str) -> Rec {
    match Envelope::parse_from(line, Source::Guest) {
        Ok(env) => Rec::Recv { t_ns, conn: conn.to_string(), env },
        Err(e) => Rec::RecvInvalid { t_ns, conn: conn.to_string(), line: line.chars().take(RAW_KEEP).collect(), error: e.to_string() },
    }
}

enum Attempt {
    Attached(BufReader<OwnedReadHalf>, OwnedWriteHalf, Stamper),

    Retry(String),
    Failed(String),
}

async fn attempt(recs: &mut Vec<Rec>, clock: RunClock, addr: SocketAddr, conn: &str, aid: &str, role: Option<Role>) -> Attempt {
    let stream = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(e) => return Attempt::Retry(format!("connect: {e}")),
    };
    let _ = stream.set_nodelay(true);
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();

    let line = match tokio::time::timeout(HANDSHAKE_TIMEOUT, read_frame(&mut reader, &mut buf)).await {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => return Attempt::Retry(format!("before hello: {e}")),
        Err(_) => return Attempt::Retry("no hello within 2 s".into()),
    };
    let rec = recv_rec(conn, clock.now_ns(), &line);
    let hello_ok = matches!(&rec, Rec::Recv { env: Envelope { event: Event::Hello { protocol: PROTOCOL_VERSION, .. }, .. }, .. });
    let invalid = matches!(rec, Rec::RecvInvalid { .. });
    recs.push(rec);
    if invalid {
        return Attempt::Failed("agent speaks an incompatible protocol".into());
    }
    if !hello_ok {
        return Attempt::Retry("first frame was not a v2 hello".into());
    }
    let mut stamper = Stamper::new(Source::Host, aid, Box::new(MonotonicClock::start()));
    let Some(role) = role else { return Attempt::Attached(reader, write, stamper) };

    let attach = stamper.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: aid.to_string(), role }, None);
    recs.push(Rec::Sent { t_ns: clock.now_ns(), conn: conn.into(), step: None, label: None, env: Some(attach.clone()), raw: None, raw_bytes: None });
    if let Err(e) = write.write_all(attach.to_line().as_bytes()).await {
        return Attempt::Retry(format!("attach write: {e}"));
    }

    let mut rejected = None;
    loop {
        let line = match tokio::time::timeout(HANDSHAKE_TIMEOUT, read_frame(&mut reader, &mut buf)).await {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => return rejected.map(Attempt::Failed).unwrap_or(Attempt::Retry(format!("before ready: {e}"))),
            Err(_) => return rejected.map(Attempt::Failed).unwrap_or(Attempt::Retry("no ready within 2 s".into())),
        };
        let rec = recv_rec(conn, clock.now_ns(), &line);
        let ready = matches!(&rec, Rec::Recv { env: Envelope { event: Event::Ready { attachment_id, .. }, .. }, .. } if attachment_id == aid);
        if let Rec::Recv { env: Envelope { event: Event::Rejected { reason }, .. }, .. } = &rec {
            rejected = Some(format!("agent rejected attach: {reason}"));
        }
        recs.push(rec);
        if ready {
            return Attempt::Attached(reader, write, stamper);
        }
    }
}

struct Case<'a> {
    clock: RunClock,
    addr: SocketAddr,
    run_tag: &'a str,
    fx: &'a Fixture,
    rep: u32,
    recs: Vec<Rec>,
    tx: mpsc::UnboundedSender<Rec>,
    rx: mpsc::UnboundedReceiver<Rec>,
    conns: BTreeMap<String, Conn>,
}

impl Case<'_> {
    fn now(&self) -> u64 {
        self.clock.now_ns()
    }

    fn note(&mut self, conn: Option<&str>, code: &str, detail: impl Into<String>) {
        let t_ns = self.now();
        self.recs.push(Rec::Note { t_ns, conn: conn.map(String::from), code: code.into(), detail: detail.into() });
    }

    fn drain(&mut self) {
        while let Ok(r) = self.rx.try_recv() {
            self.recs.push(r);
        }
    }

    async fn observe_until(&mut self, deadline_ns: u64) {
        loop {
            self.drain();
            if self.now() >= deadline_ns {
                return;
            }
            tokio::select! {
                r = self.rx.recv() => if let Some(r) = r { self.recs.push(r) },
                _ = tokio::time::sleep_until(self.clock.instant_at(deadline_ns)) => {}
            }
        }
    }

    fn request_id(&self, label: &str) -> String {
        tl::request_id(self.run_tag, &self.fx.scenario_id, self.rep, label)
    }

    async fn open(&mut self, conn: &str, role: Option<Role>) {
        for n in 1..=ATTACH_ATTEMPTS {
            let aid = format!("{}-{}-r{}-{conn}-a{n}", self.run_tag, self.fx.scenario_id, self.rep);
            match attempt(&mut self.recs, self.clock, self.addr, conn, &aid, role).await {
                Attempt::Attached(reader, write, stamper) => {
                    let tap: Tap = Arc::default();
                    let reader = spawn_reader(conn.to_string(), reader, self.clock, self.tx.clone(), tap.clone());
                    self.conns.insert(conn.to_string(), Conn { write, reader, stamper, tap, speaker: None });
                    return;
                }
                Attempt::Retry(why) => {
                    self.note(Some(conn), "attach_retry", format!("attempt {n}: {why}"));
                    let until = self.now() + RETRY_GAP.as_nanos() as u64;
                    self.observe_until(until).await;
                }
                Attempt::Failed(why) => {
                    self.note(Some(conn), "attach_failed", why);
                    return;
                }
            }
        }
        self.note(Some(conn), "attach_failed", format!("no ready after {ATTACH_ATTEMPTS} attempts"));
    }

    async fn send_event(&mut self, conn: &str, step: usize, label: Option<&str>, event: Event) {
        let Some(c) = self.conns.get_mut(conn) else {
            self.note(Some(conn), "send_failed", format!("step {step}: connection is not open"));
            return;
        };
        let env = c.stamper.stamp(event, None);
        let line = env.to_line();
        let t_ns = self.clock.now_ns();
        let res = c.write.write_all(line.as_bytes()).await;
        self.recs.push(Rec::Sent { t_ns, conn: conn.into(), step: Some(step), label: label.map(String::from), env: Some(env), raw: None, raw_bytes: None });
        if let Err(e) = res {
            self.note(Some(conn), "send_failed", format!("step {step}: {e}"));
        }
    }

    async fn send_raw(&mut self, conn: &str, step: usize, label: Option<&str>, mut bytes: Vec<u8>) {
        let Some(c) = self.conns.get_mut(conn) else {
            self.note(Some(conn), "send_failed", format!("step {step}: connection is not open"));
            return;
        };
        let raw_bytes = bytes.len();
        bytes.push(b'\n');
        let t_ns = self.clock.now_ns();
        let res = c.write.write_all(&bytes).await;
        let raw = Some(String::from_utf8_lossy(&bytes[..raw_bytes.min(RAW_KEEP)]).into_owned());
        self.recs.push(Rec::Sent { t_ns, conn: conn.into(), step: Some(step), label: label.map(String::from), env: None, raw, raw_bytes: Some(raw_bytes) });
        if let Err(e) = res {
            self.note(Some(conn), "send_failed", format!("step {step}: {e}"));
        }
    }

    fn close(&mut self, conn: &str) {
        if let Some(mut c) = self.conns.remove(conn) {
            if let Some(sp) = c.speaker.take() {
                let t_ns = self.now();
                self.recs.push(Rec::Note { t_ns, conn: Some(conn.into()), code: "speaker".into(), detail: sp.summary().to_string() });
            }
            c.reader.abort();
            drop(c.write);
            let t_ns = self.now();
            self.recs.push(Rec::Closed { t_ns, conn: conn.into(), by: ClosedBy::Runner, detail: "runner dropped the connection".into() });
        }
    }

    fn trigger_time(&self, trig: &Trigger, step_conn: &str) -> Option<u64> {
        let (request, conn) = match trig {
            Trigger::ResponseStarted { request, conn } | Trigger::Delta { request, conn, .. } | Trigger::Terminal { request, conn } | Trigger::Audio { request, conn, .. } => {
                (request, conn.as_deref().unwrap_or(step_conn))
            }
        };
        let rid = tl::response_for(&self.recs, &self.request_id(request))?;
        match trig {
            Trigger::ResponseStarted { .. } => tl::started_time(&self.recs, conn, &rid),
            Trigger::Delta { count, .. } => tl::deltas(&self.recs, conn, &rid).get(count.saturating_sub(1)).map(|(t, _)| *t),
            Trigger::Terminal { .. } => tl::first_terminal(&self.recs, conn, &rid).map(|(t, _)| t),
            Trigger::Audio { count, .. } => tl::audio(&self.recs, conn, &rid).get(count.saturating_sub(1)).map(|(t, ..)| *t),
        }
    }

    fn until_met(&self, until: &Until, conn: &str, recs: &[Rec]) -> bool {
        match until {
            Until::Terminal { requests } => requests.iter().all(|l| {
                tl::response_for(recs, &self.request_id(l)).is_some_and(|rid| tl::first_terminal(recs, conn, &rid).is_some())
            }),
            Until::Closed {} => tl::closed_by_peer(recs, conn).is_some(),
            Until::Rejected { count } => tl::rejected_on(recs, conn).len() >= *count,
            Until::ForMs { .. } => false,
        }
    }
}

pub async fn run_case(addr: SocketAddr, clock: RunClock, run_tag: &str, fx: &Fixture, fixture_sha256: &str, rep: u32, storage: Option<&std::path::Path>) -> Vec<Rec> {
    let (tx, rx) = mpsc::unbounded_channel();
    let origin = clock.now_ns();
    let header = Rec::Header {
        scenario_id: fx.scenario_id.clone(),
        task: fx.task.clone(),
        split: fx.split,
        rep,
        fixture_sha256: fixture_sha256.to_string(),
        run_tag: run_tag.to_string(),
        origin_ns: origin,
    };
    let mut case = Case { clock, addr, run_tag, fx, rep, recs: vec![header], tx, rx, conns: BTreeMap::new() };
    let deadline = origin + ms(fx.deadline_ms);
    let mut prev_done = origin;

    for (i, step) in fx.steps.iter().enumerate() {
        let fire_at = if let Some(at) = step.at_ms {
            origin + ms(at)
        } else if let Some(after) = step.after_ms {
            prev_done + ms(after)
        } else if let Some(trig) = &step.on {
            let until = (case.now() + ms(TRIGGER_TIMEOUT_MS)).min(deadline);
            if !wait_trigger(&mut case, trig, &step.conn, until).await {
                case.note(Some(&step.conn), "trigger_timeout", format!("step {i}: {trig:?} not observed"));
                break;
            }
            case.trigger_time(trig, &step.conn).expect("observed") + ms(step.delay_ms)
        } else {
            case.now()
        };
        if fire_at > deadline {
            case.note(None, "case_deadline", format!("step {i} due after the {} ms case deadline", fx.deadline_ms));
            break;
        }
        case.observe_until(fire_at).await;
        let t_ns = case.now();
        case.recs.push(Rec::Step { t_ns, step: i, op: step.op.name().into(), conn: step.conn.clone(), sched_ns: fire_at });

        let conn = step.conn.as_str();
        match &step.op {
            Op::Attach { role } => case.open(conn, Some(*role)).await,
            Op::ConnectNoAttach {} => case.open(conn, None).await,
            Op::UserText { label, text } => {
                let event = Event::UserText { request_id: case.request_id(label), text: text.clone(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None };
                case.send_event(conn, i, Some(label), event).await;
            }
            Op::Cancel { label, target } => {
                case.drain();
                let rid = match target {
                    Target::Request(l) => tl::response_for(&case.recs, &case.request_id(l)),
                    Target::Nth(n) => tl::started_on(&case.recs, conn).get(n.saturating_sub(1)).map(|(r, _, _)| r.clone()),
                    Target::UnknownId(s) => Some(format!("{run_tag}-{s}")),
                };
                match rid {
                    Some(response_id) => case.send_event(conn, i, label.as_deref(), Event::Cancel { response_id }).await,

                    None => case.note(Some(conn), "invalid_target", format!("step {i}: {target:?} has no observed response yet")),
                }
            }
            Op::Raw { label, line } => case.send_raw(conn, i, Some(label), line.clone().into_bytes()).await,
            Op::Oversize { bytes } => case.send_raw(conn, i, None, vec![b'x'; *bytes]).await,
            Op::Disconnect {} => case.close(conn),
            Op::UserAudio { label, tone_hz, ms: dur, chunk_ms, skip_chunks } => {
                let stream_id = case.request_id(label);
                let per = (16 * *chunk_ms) as usize;
                let total = (16 * *dur) as usize;
                let pcm = tone_16k(*tone_hz, total);
                let start = case.now();
                let (mut sent, mut n) = (0u64, 0u64);
                for (k, chunk) in pcm.chunks(per).enumerate() {
                    let k = k as u64;
                    n += 1;
                    if !skip_chunks.contains(&k) {
                        let data = base64::engine::general_purpose::STANDARD.encode(chunk.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>());
                        let event = Event::UserAudio { stream_id: stream_id.clone(), chunk: k, sample_offset: k * per as u64, sample_rate: 16_000, format: "s16le".into(), data, digest: None };
                        case.send_event(conn, i, None, event).await;
                        sent += 1;
                    }

                    case.observe_until(start + (k + 1) * ms(*chunk_ms)).await;
                }
                let end = Event::UserAudioEnd { stream_id, chunks: n, samples: total as u64, dropped_chunks: n - sent, speaker: Default::default() };
                case.send_event(conn, i, Some(label), end).await;
            }
            Op::Speaker {} => {
                let tx = case.tx.clone();
                let clock = case.clock;
                match case.conns.get_mut(conn) {
                    Some(c) => {
                        let (sp, etx) = Speaker::start(conn, clock, tx);
                        *c.tap.lock().unwrap() = Some(etx);
                        c.speaker = Some(sp);
                    }
                    None => case.note(Some(conn), "invalid_target", format!("step {i}: speaker on an unopened connection")),
                }
            }
            Op::BargeIn { label } => {
                case.drain();
                let clock = case.clock;
                let Some((detail, records)) = case.conns.get(conn).and_then(|c| c.speaker.as_ref()).map(|sp| sp.barge_in(clock)) else {
                    case.note(Some(conn), "invalid_target", format!("step {i}: barge_in without a speaker"));
                    prev_done = case.now();
                    continue;
                };
                let t_ns = case.now();
                case.recs.push(Rec::Note { t_ns, conn: Some(conn.into()), code: "speaker".into(), detail: detail.to_string() });

                let open: Vec<String> = tl::started_on(&case.recs, conn)
                    .into_iter()
                    .map(|(r, _, _)| r)
                    .filter(|r| tl::first_terminal(&case.recs, conn, r).is_none())
                    .collect();
                for (k, r) in open.into_iter().enumerate() {
                    let l = if k == 0 { label.as_deref() } else { None };
                    case.send_event(conn, i, l, Event::Cancel { response_id: r }).await;
                }
                for e in records {
                    case.send_event(conn, i, None, e).await;
                }
            }
            Op::Wait { until, deadline_ms } => {
                let limit = match until {
                    Until::ForMs { ms: d } => case.now() + ms(*d),
                    _ => (case.now() + ms(*deadline_ms)).min(deadline),
                };
                if !wait_until(&mut case, until, conn, limit).await && !matches!(until, Until::ForMs { .. }) {
                    case.note(Some(conn), "wait_timeout", format!("step {i}: {until:?} not met within {deadline_ms} ms"));
                }
            }
        }
        prev_done = case.now();
    }

    let settle = case.now() + ms(fx.settle_ms);
    case.observe_until(settle).await;
    case.note(None, "cleanup", "settle window over; runner closes remaining connections");
    let open: Vec<String> = case.conns.keys().cloned().collect();
    for c in open {
        case.close(&c);
    }
    case.drain();

    let t_ns = case.now();
    let receipt = storage_receipt(storage, &case.recs);
    case.recs.push(Rec::Note { t_ns, conn: None, code: "storage_receipt".into(), detail: receipt.to_string() });
    tl::sort(&mut case.recs);
    case.recs
}

const READBACK_JOURNAL_BYTES: u64 = 64 << 20;
const READBACK_BLOB_BYTES: u64 = 8 << 20;

pub fn storage_receipt(dir: Option<&std::path::Path>, recs: &[Rec]) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let Some(dir) = dir else { return json!({"kind": "storage_receipt", "available": false, "why": "no state dir given to the runner"}) };
    let responses: std::collections::BTreeSet<String> = recs
        .iter()
        .filter_map(|r| match r.recv() {
            Some((_, Envelope { event: Event::ResponseStarted { response_id, .. }, .. }, _)) => Some(response_id.clone()),
            _ => None,
        })
        .collect();
    let streams: std::collections::BTreeSet<String> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::Sent { env: Some(Envelope { event: Event::UserAudio { stream_id, .. }, .. }), .. } => Some(stream_id.clone()),
            _ => None,
        })
        .collect();
    let root = match crate::contained::Root::open(dir) {
        Ok(r) => r,
        Err(e) => return json!({"kind": "storage_receipt", "available": false, "why": format!("state dir {}: {}", dir.display(), e.as_str())}),
    };
    let (journal_bytes, text) = match root.read("journal.jsonl", READBACK_JOURNAL_BYTES, true) {
        Ok((len, b)) => (len, String::from_utf8_lossy(&b).into_owned()),
        Err(e) => return json!({"kind": "storage_receipt", "available": false, "why": format!("journal.jsonl: {}", e.as_str())}),
    };
    let media_dir = root.dir("media");
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let mut references = Vec::new();
    let mut blobs = std::collections::BTreeMap::new();
    for line in text.lines().filter(|l| l.contains("\"kind\":\"media_stored\"")) {
        let Ok(env) = serde_json::from_str::<Envelope>(line) else { continue };
        let Event::MediaStored { media, response_id, stream_id, sha256, bytes, sample_start, sample_end, path, .. } = &env.event else { continue };
        let ours = response_id.as_ref().is_some_and(|r| responses.contains(r)) || stream_id.as_ref().is_some_and(|s| streams.contains(s));
        if !ours {
            continue;
        }
        references.push(json!({"event_id": env.id, "media": media, "response_id": response_id, "stream_id": stream_id, "sha256": sha256,
            "bytes": bytes, "sample_start": sample_start, "sample_end": sample_end, "path": path}));
        let leaf = path.strip_prefix("media/").filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        blobs.entry(path.clone()).or_insert_with(|| {
            let Some(leaf) = leaf else { return json!({"path": path, "path_ok": false, "present": false, "refused": "unsafe path"}) };
            let media = match &media_dir {
                Ok(m) => m,
                Err(e) => return json!({"path": path, "path_ok": true, "present": false, "refused": format!("media/: {}", e.as_str())}),
            };
            match media.read(leaf, READBACK_BLOB_BYTES, false) {
                Ok((_, b)) => json!({"path": path, "path_ok": true, "present": true, "bytes": b.len(), "sha256": hex(&Sha256::digest(&b))}),
                Err(crate::contained::Refused::Missing) => json!({"path": path, "path_ok": true, "present": false}),
                Err(e) => json!({"path": path, "path_ok": true, "present": false, "refused": e.as_str()}),
            }
        });
    }
    json!({"kind": "storage_receipt", "available": true, "state_dir": dir.display().to_string(), "journal_bytes": journal_bytes,
        "references": references, "blobs": blobs.into_values().collect::<Vec<_>>()})
}

pub fn tone_16k(hz: f64, samples: usize) -> Vec<i16> {
    (0..samples).map(|i| if hz == 0.0 { 0 } else { ((i as f64 / 16_000.0 * hz * std::f64::consts::TAU).sin() * 8_000.0) as i16 }).collect()
}

async fn wait_trigger(case: &mut Case<'_>, trig: &Trigger, conn: &str, limit: u64) -> bool {
    loop {
        case.drain();
        if case.trigger_time(trig, conn).is_some() {
            return true;
        }
        if case.now() >= limit {
            return false;
        }
        match tokio::time::timeout_at(case.clock.instant_at(limit), case.rx.recv()).await {
            Ok(Some(r)) => case.recs.push(r),
            _ => {
                case.drain();
                return case.trigger_time(trig, conn).is_some();
            }
        }
    }
}

async fn wait_until(case: &mut Case<'_>, until: &Until, conn: &str, limit: u64) -> bool {
    loop {
        case.drain();
        if case.until_met(until, conn, &case.recs) {
            return true;
        }
        if case.now() >= limit {
            return false;
        }
        match tokio::time::timeout_at(case.clock.instant_at(limit), case.rx.recv()).await {
            Ok(Some(r)) => case.recs.push(r),
            _ => {
                case.drain();
                return case.until_met(until, conn, &case.recs);
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HelloInfo {
    pub protocol: u16,
    pub agent: String,
    pub version: String,
    pub boot_id: String,
    pub utc_ms_at_origin: u64,
    pub journal: String,

    pub backend: Option<String>,
    pub artifact_id: Option<String>,
    pub deployment: Option<rth_protocol::deploy::DeploymentIdentity>,
    pub context: Option<String>,
}

pub async fn isolation_check(addr: SocketAddr, clock: RunClock, run_tag: &str, n: usize) -> (Vec<Rec>, Result<HelloInfo, String>) {
    let conn = format!("iso-{n}");
    let mut recs = Vec::new();
    let aid = format!("{run_tag}-iso-{n}");
    let mut parts = None;
    for k in 1..=ATTACH_ATTEMPTS {
        match attempt(&mut recs, clock, addr, &conn, &format!("{aid}-a{k}"), Some(Role::Observer)).await {
            Attempt::Attached(r, w, _) => {
                parts = Some((r, w));
                break;
            }
            Attempt::Retry(why) => {
                recs.push(Rec::Note { t_ns: clock.now_ns(), conn: Some(conn.clone()), code: "attach_retry".into(), detail: why });
                tokio::time::sleep(RETRY_GAP).await;
            }
            Attempt::Failed(why) => return (recs, Err(why)),
        }
    }
    let Some((mut reader, write)) = parts else { return (recs, Err("agent never became ready".into())) };
    let hello = recs.iter().rev().find_map(|r| match r {
        Rec::Recv { env: Envelope { event: Event::Hello { protocol, agent, version, boot_id, utc_ms_at_origin, journal, backend, deployment, instance, .. }, .. }, .. } => {
            Some(HelloInfo {
                protocol: *protocol,
                agent: agent.clone(),
                version: version.clone(),
                boot_id: boot_id.clone(),
                utc_ms_at_origin: *utc_ms_at_origin,
                journal: journal.clone(),
                backend: backend.clone(),
                artifact_id: deployment.as_ref().map(|d| d.artifact_id.clone()),
                deployment: deployment.clone(),
                context: instance.as_ref().map(|i| i.context.clone()),
            })
        }
        _ => None,
    });
    let limit = tokio::time::Instant::now() + Duration::from_secs(6);
    let mut buf = Vec::new();
    let mut quiet = true;
    let verdict = loop {
        let line = match tokio::time::timeout_at(limit, read_frame(&mut reader, &mut buf)).await {
            Ok(Ok(line)) => line,
            Ok(Err(e)) => break Err(format!("isolation link closed: {e}")),
            Err(_) => break Err("no clean heartbeat within 6 s: previous case not drained".into()),
        };
        let rec = recv_rec(&conn, clock.now_ns(), &line);
        let mut clean = false;
        if let Rec::Recv { env, .. } = &rec {
            match &env.event {
                Event::ResponseStarted { .. } | Event::AgentDelta { .. } | Event::AgentDone { .. } => quiet = false,
                Event::Heartbeat { attachments, .. } => {
                    clean = quiet && *attachments == 1;
                    quiet = true;
                }
                _ => {}
            }
        }
        recs.push(rec);
        if clean {
            break hello.clone().ok_or_else(|| "no hello recorded".to_string());
        }
    };
    drop(write);
    drop(reader);
    recs.push(Rec::Closed { t_ns: clock.now_ns(), conn, by: ClosedBy::Runner, detail: "isolation check done".into() });
    (recs, verdict)
}
