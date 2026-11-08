

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use anyhow::Result;
use rth_voice::queue::PlayQueue;
use serde_json::{Value, json};

use crate::devices::{Capture, MicFrames, Playout};
use crate::voice::{FenceState, Urgent};

pub const NO_CALL: &str = "no call is active (Start call opens the microphone and speaker)";

pub const OUTPUT_CLOSE_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Off,
    Starting,
    Active,
    Ending,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Off => "off",
            Phase::Starting => "starting",
            Phase::Active => "active",
            Phase::Ending => "ending",
            Phase::Error => "error",
        }
    }
}

pub type MakeSpeaker = Box<dyn FnMut(u64) -> Box<dyn Playout> + Send>;

pub type CallLog = Box<dyn Fn(&str, Value) + Send + Sync>;

pub struct CallDevices {
    pub capture: Box<dyn Capture>,

    pub playout: MakeSpeaker,

    pub rate: u32,
    pub describe: String,
}

struct Devices {
    capture: Box<dyn Capture>,
    make: MakeSpeaker,

    speaker: Option<Box<dyn Playout>>,
}

#[derive(Default)]
struct Sinks {
    frames: Option<(MicFrames, Arc<AtomicU64>)>,
    queue: Option<Arc<PlayQueue>>,
}

struct State {
    desired: bool,
    phase: Phase,
    reason: String,
    error: Option<String>,

    attempt: u64,

    failed_attempt: u64,

    release_owed: bool,
    since: Instant,
    via: String,
}

pub struct Call {
    state: Mutex<State>,

    devices: Mutex<Option<Devices>>,
    configured: bool,
    sinks: Mutex<Sinks>,

    open: AtomicBool,

    opened: AtomicU64,
    rate: u32,
    describe: String,
    urgent: Arc<Urgent>,
    wake: Sender<()>,
    log: CallLog,
}

impl Call {

    pub fn new(devices: Option<CallDevices>, urgent: Arc<Urgent>, log: CallLog) -> Arc<Self> {
        let (wake, rx) = channel::<()>();
        let (rate, describe, configured) = match &devices {
            Some(d) => (d.rate, d.describe.clone(), true),
            None => (24_000, "no voice devices configured".to_string(), false),
        };
        urgent.mute.close_call(true);
        let call = Arc::new(Call {
            state: Mutex::new(State {
                desired: false,
                phase: Phase::Off,
                reason: "no call yet".into(),
                error: None,
                attempt: 0,
                failed_attempt: 0,
                release_owed: false,
                since: Instant::now(),
                via: "launch".into(),
            }),
            devices: Mutex::new(devices.map(|d| Devices { capture: d.capture, make: d.playout, speaker: None })),
            configured,
            sinks: Mutex::new(Sinks::default()),
            open: AtomicBool::new(false),
            opened: AtomicU64::new(0),
            rate,
            describe,
            urgent,
            wake,
            log,
        });
        let weak: Weak<Call> = Arc::downgrade(&call);
        let _ = std::thread::Builder::new().name("rth-call".into()).spawn(move || {
            while rx.recv().is_ok() {
                while rx.try_recv().is_ok() {}
                let Some(call) = weak.upgrade() else { return };
                call.reconcile();
            }
        });
        call
    }

    fn kick(&self) {
        let _ = self.wake.send(());
    }

    fn set(&self, st: &mut State, phase: Phase, reason: String) {
        if st.phase == phase && st.reason == reason {
            return;
        }
        st.phase = phase;
        st.reason = reason;
        st.since = Instant::now();
        (self.log)("call", json!({"state": phase.as_str(), "desired": if st.desired { "on" } else { "off" }, "reason": st.reason, "attempt": st.attempt, "error": st.error}));
    }

    pub fn request(&self, on: bool, via: &str) -> Value {
        let mut st = self.state.lock().unwrap();
        let was = json!({"desired": if st.desired { "on" } else { "off" }, "state": st.phase.as_str()});
        st.via = via.to_string();
        if on {
            if !st.desired || st.phase == Phase::Error {
                st.attempt += 1;
            }
            st.desired = true;
            if !self.configured {
                st.error = Some("no voice devices configured: this rth-host was started without --voice, --call-ready or the WAV fakes".into());
                let why = st.error.clone().unwrap();
                self.set(&mut st, Phase::Error, why);
            } else {
                match st.phase {
                    Phase::Off | Phase::Error => {
                        st.error = None;
                        self.set(&mut st, Phase::Starting, format!("start requested ({via})"));
                    }

                    Phase::Ending => self.set(&mut st, Phase::Ending, format!("start requested ({via}) while ending: queued until the devices are released, then fresh ones open")),
                    Phase::Starting | Phase::Active => {}
                }
            }
        } else {
            st.desired = false;
            if matches!(st.phase, Phase::Starting | Phase::Active | Phase::Ending) {
                st.release_owed = true;
            }

            self.urgent.mute.close_call(true);
            match st.phase {
                Phase::Active => self.set(&mut st, Phase::Ending, format!("end requested ({via})")),
                Phase::Starting => self.set(&mut st, Phase::Ending, format!("end requested ({via}) while starting: the start completes or times out, then the devices are released")),
                Phase::Error => {
                    st.error = None;
                    self.set(&mut st, Phase::Off, format!("ended ({via})"));
                }
                Phase::Off | Phase::Ending => {}
            }
        }
        let receipt = json!({
            "request": if on { "start" } else { "end" },
            "via": via,
            "was": was,
            "desired": if st.desired { "on" } else { "off" },
            "state": st.phase.as_str(),
            "attempt": st.attempt,
            "note": "a request: what happened is host.call in GET /v1/status (state is read back, never the request)",
        });
        drop(st);
        (self.log)("call_request", receipt.clone());
        self.kick();
        receipt
    }

    pub fn desired(&self) -> bool {
        self.state.lock().unwrap().desired
    }

    fn reconcile(&self) {
        loop {
            let mut guard = self.devices.lock().unwrap();
            let Some(dev) = guard.as_mut() else { return };
            let (desired, attempt, failed, owed) = {
                let st = self.state.lock().unwrap();
                (st.desired, st.attempt, st.failed_attempt == st.attempt && st.attempt > 0, st.release_owed)
            };
            let sinks = {
                let s = self.sinks.lock().unwrap();
                match (&s.frames, &s.queue) {
                    (Some((f, d)), Some(q)) => Some((f.clone(), d.clone(), q.clone())),
                    _ => None,
                }
            };
            let running = dev.speaker.is_some();
            let want = desired && !failed && sinks.is_some();

            if running && owed {
                self.close_devices(dev, "released: an End was requested for these devices");
                self.state.lock().unwrap().release_owed = false;
                continue;
            }
            if want && !running {
                let (frames, dropped, queue) = sinks.expect("checked");

                let owed = self.urgent.mute.snapshot();
                if owed.loop_running && owed.output_closed_epoch < owed.call_epoch {
                    let t = Instant::now();
                    let done = self.urgent.mute.wait_output_closed(OUTPUT_CLOSE_WAIT);
                    (self.log)("call_output_close", json!({"call_epoch": owed.call_epoch, "acknowledged": done, "waited_ms": t.elapsed().as_secs_f64() * 1e3, "queued_samples_after": queue.stats().queued}));
                    if !done {
                        let mut st = self.state.lock().unwrap();
                        let why = format!("the voice loop did not finish the previous call's output close within {} s; no new speaker is opened", OUTPUT_CLOSE_WAIT.as_secs());
                        st.failed_attempt = attempt;
                        st.error = Some(why.clone());
                        self.set(&mut st, Phase::Error, why);
                        continue;
                    }
                }
                {
                    let mut st = self.state.lock().unwrap();

                    st.release_owed = false;
                    self.set(&mut st, Phase::Starting, "opening the devices".into());
                }
                let n = self.opened.load(Ordering::SeqCst) + 1;
                match self.open_devices(dev, n, frames, dropped, queue) {
                    Ok(()) => {
                        self.opened.store(n, Ordering::SeqCst);
                        self.open.store(true, Ordering::SeqCst);
                        let mut st = self.state.lock().unwrap();

                        if st.desired && !st.release_owed {
                            self.urgent.mute.close_call(false);
                            self.set(&mut st, Phase::Active, format!("devices open (call {n}): the microphone is admitted, the speaker plays"));
                        }
                    }
                    Err(why) => {
                        let mut st = self.state.lock().unwrap();

                        st.release_owed = false;
                        st.failed_attempt = attempt;
                        st.error = Some(why.clone());
                        if st.desired {
                            self.set(&mut st, Phase::Error, format!("the devices did not open: {why}"));
                        }
                    }
                }
                continue;
            }
            if running && !want {
                self.close_devices(dev, if desired { "the voice loop ended (link down): the devices are released until it runs again" } else { "released" });
                continue;
            }
            let mut st = self.state.lock().unwrap();
            if !running {
                st.release_owed = false;
            }
            match (running, desired) {
                (false, false) if matches!(st.phase, Phase::Starting | Phase::Ending) => self.set(&mut st, Phase::Off, "the devices are released".into()),
                (false, true) if !failed => self.set(&mut st, Phase::Starting, "waiting for the agent link: the devices open when the voice loop runs".into()),
                _ => {}
            }
            return;
        }
    }

    fn open_devices(&self, dev: &mut Devices, n: u64, frames: MicFrames, dropped: Arc<AtomicU64>, queue: Arc<PlayQueue>) -> Result<(), String> {
        let mut speaker = (dev.make)(n);
        if speaker.rate() != self.rate {
            return Err(format!("the speaker runs at {} Hz, the voice loop's queue at {} Hz (restart rth-host after an output device change)", speaker.rate(), self.rate));
        }
        speaker.start(queue).map_err(|e| format!("speaker: {e:#}"))?;
        if let Err(e) = dev.capture.start(frames, dropped) {
            let _ = speaker.stop();
            return Err(format!("microphone: {e:#}"));
        }

        if let Some(why) = speaker.unavailable() {
            dev.capture.stop();
            let _ = speaker.stop();
            return Err(why);
        }
        (self.log)("call_devices_opened", json!({"call": n, "capture": dev.capture.describe(), "speaker": speaker.describe()}));
        dev.speaker = Some(speaker);
        Ok(())
    }

    fn close_devices(&self, dev: &mut Devices, why: &str) {
        self.open.store(false, Ordering::SeqCst);
        {
            let mut st = self.state.lock().unwrap();
            self.urgent.mute.close_call(true);
            if !st.desired {
                self.set(&mut st, Phase::Ending, "releasing the devices".into());
            }
        }
        let t = Instant::now();
        dev.capture.stop();
        let line = dev.speaker.take().map(|mut s| s.stop().unwrap_or_else(|e| format!("speaker stop: {e}")));
        (self.log)("call_devices_released", json!({"why": why, "speaker": line, "release_ms": t.elapsed().as_secs_f64() * 1e3}));
    }

    fn release_now(&self) {
        if let Ok(mut g) = self.devices.try_lock()
            && let Some(dev) = g.as_mut()
            && dev.speaker.is_some()
        {
            let desired = self.desired();
            self.close_devices(dev, if desired { "the voice loop ended (link down): the devices are released until it runs again" } else { "released" });
        }
        self.kick();
    }

    pub fn status(&self, f: &FenceState) -> Value {
        let st = self.state.lock().unwrap();
        let open = self.open.load(Ordering::SeqCst);
        let (input_open, input_closed) = (f.effective_call_open(), f.effective_call_closed());
        let (shown, reason) = match st.phase {
            Phase::Active if !input_open => ("starting", "devices open; the voice loop has not applied the open call input yet".to_string()),
            Phase::Off if !input_closed => ("ending", "devices released; the voice loop has not applied the closed call input yet".to_string()),
            p => (p.as_str(), st.reason.clone()),
        };
        json!({
            "state": shown,
            "desired": if st.desired { "on" } else { "off" },
            "reason": reason,
            "error": st.error,
            "attempt": st.attempt,
            "ms_in_state": st.since.elapsed().as_millis() as u64,
            "via": st.via,
            "configured": self.configured,
            "devices": self.describe,
            "device_open": open,
            "calls_opened": self.opened.load(Ordering::SeqCst),
            "input": {"call_input": if input_open { "open" } else if input_closed { "closed" } else { "changing" },
                      "requested_closed": f.call_closed, "applied_closed": f.applied_call_closed},
            "output": if open { "the call's speaker plays agent audio" } else { "no call speaker: agent audio is refused (device_unavailable), never claimed as played" },
            "output_close": {"call_epoch": f.call_epoch, "done_through": f.output_closed_epoch,
                              "note": "each End owes one output close (open responses cancelled, queued audio cleared), done by the voice loop before any new call's speaker opens"},
            "end_is_not_mute": "End call fences input and output and releases the devices; the agent, its lease and the conversation keep running. Mute is a separate input and is never changed by a call.",
            "controls": "POST /v1/call {\"call\": \"start\"|\"end\"}, stdin `call start` / `call end`, terminal key v, the app menu and tray, the composer's call button",
        })
    }
}

pub struct CallCapture(pub Arc<Call>);

impl Capture for CallCapture {
    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
        self.0.sinks.lock().unwrap().frames = Some((frames, dropped));
        self.0.kick();
        Ok(())
    }

    fn stop(&mut self) {
        self.0.sinks.lock().unwrap().frames = None;
        self.0.release_now();
    }

    fn describe(&self) -> String {
        format!("call microphone ({}; open: {})", self.0.describe, self.0.open.load(Ordering::SeqCst))
    }
}

pub struct CallPlayout(pub Arc<Call>);

impl Playout for CallPlayout {
    fn rate(&self) -> u32 {
        self.0.rate
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        self.0.sinks.lock().unwrap().queue = Some(queue);
        self.0.kick();
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        self.0.sinks.lock().unwrap().queue = None;
        self.0.release_now();
        Ok(format!("call speaker ({}) · calls opened {}", self.0.describe, self.0.opened.load(Ordering::SeqCst)))
    }

    fn describe(&self) -> String {
        format!("call speaker ({})", self.0.describe)
    }

    fn unavailable(&self) -> Option<String> {
        (!self.0.open.load(Ordering::SeqCst)).then(|| NO_CALL.to_string())
    }
}

pub struct NoSpeaker {
    pub rate: u32,
    pub why: String,
}

impl Playout for NoSpeaker {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn start(&mut self, _queue: Arc<PlayQueue>) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        Ok(format!("no speaker ({})", self.why))
    }

    fn describe(&self) -> String {
        format!("no speaker ({})", self.why)
    }

    fn unavailable(&self) -> Option<String> {
        Some(self.why.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::FeedCapture;
    use crate::voice::{Control, Note, VoiceOptions, VoiceSummary, drive_with};
    use base64::Engine;
    use rth_protocol::io::read_frame;
    use rth_protocol::{Envelope, Event, MonotonicClock, PlaybackPhase, Source, Stamper};
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};
    use tokio::sync::mpsc;

    #[derive(Clone, Default)]
    struct Mic {
        feed: FeedCapture,
        starts: Arc<AtomicU64>,
        stops: Arc<AtomicU64>,
        delay: Arc<Mutex<Duration>>,
        fail: Arc<Mutex<Option<String>>>,

        entered: Arc<AtomicU64>,
        gate: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
    }

    impl Mic {

        fn hold_next_start(&self) -> std::sync::mpsc::Sender<()> {
            let (tx, rx) = std::sync::mpsc::channel();
            *self.gate.lock().unwrap() = Some(rx);
            tx
        }
    }

    impl Capture for Mic {
        fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            let gate = self.gate.lock().unwrap().take();
            if let Some(rx) = gate {
                let _ = rx.recv();
            }
            std::thread::sleep(*self.delay.lock().unwrap());
            if let Some(e) = self.fail.lock().unwrap().take() {
                return Err(anyhow::anyhow!(e));
            }
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.feed.start(frames, dropped)
        }
        fn stop(&mut self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
            self.feed.stop();
        }
        fn describe(&self) -> String {
            "test mic".into()
        }
    }

    #[derive(Clone, Default)]
    struct Speakers {
        log: Arc<Mutex<Vec<String>>>,
        queue: Arc<Mutex<Option<Arc<PlayQueue>>>>,

        queued_at_start: Arc<Mutex<Vec<usize>>>,
    }

    impl Speakers {
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
        fn queued(&self) -> usize {
            self.queue.lock().unwrap().as_ref().map_or(0, |q| q.stats().queued)
        }
    }

    struct Speaker(u64, Speakers);

    impl Playout for Speaker {
        fn rate(&self) -> u32 {
            24_000
        }
        fn start(&mut self, q: Arc<PlayQueue>) -> Result<()> {
            self.1.queued_at_start.lock().unwrap().push(q.stats().queued);
            *self.1.queue.lock().unwrap() = Some(q);
            self.1.log.lock().unwrap().push(format!("start {}", self.0));
            Ok(())
        }
        fn stop(&mut self) -> Result<String> {
            self.1.log.lock().unwrap().push(format!("stop {}", self.0));
            Ok(format!("speaker {}", self.0))
        }
        fn describe(&self) -> String {
            format!("speaker {}", self.0)
        }
    }

    struct Rig {
        call: Arc<Call>,
        urgent: Arc<Urgent>,
        mic: Mic,
        speakers: Speakers,
        logs: Arc<Mutex<Vec<(String, Value)>>>,
        ctl: mpsc::Sender<Control>,
        _notes: mpsc::UnboundedReceiver<Note>,
        agent: (Stamper, WriteHalf<DuplexStream>),
        wire: (BufReader<ReadHalf<DuplexStream>>, Vec<u8>),
        driver: tokio::task::JoinHandle<Result<VoiceSummary>>,
    }

    fn rig() -> Rig {
        let urgent = Arc::new(Urgent::default());
        let mic = Mic::default();
        let speakers = Speakers::default();
        let s2 = speakers.clone();
        let devices = CallDevices {
            capture: Box::new(mic.clone()),
            playout: Box::new(move |n| Box::new(Speaker(n, s2.clone())) as Box<dyn Playout>),
            rate: 24_000,
            describe: "test devices".into(),
        };
        let logs = Arc::new(Mutex::new(Vec::new()));
        let l2 = logs.clone();
        let call = Call::new(Some(devices), urgent.clone(), Box::new(move |k: &str, v| l2.lock().unwrap().push((k.to_string(), v))));
        let (host_side, agent_side) = tokio::io::duplex(1 << 22);
        let (hr, hw) = tokio::io::split(host_side);
        let (ar, aw) = tokio::io::split(agent_side);
        let mut reader = BufReader::new(hr);
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let (ctl, mut ctl_rx) = mpsc::channel(16);
        let (notes_tx, notes) = mpsc::unbounded_channel();
        let (mut cap, mut out) = (CallCapture(call.clone()), CallPlayout(call.clone()));
        let u2 = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut cap, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u2).await });
        Rig {
            call,
            urgent,
            mic,
            speakers,
            logs,
            ctl,
            _notes: notes,
            agent: (Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start())), aw),
            wire: (BufReader::new(ar), Vec::new()),
            driver,
        }
    }

    impl Rig {
        fn status(&self) -> Value {
            self.call.status(&self.urgent.mute.snapshot())
        }

        async fn until(&self, want: &str) -> Value {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let st = self.status();
                if st["state"] == want {
                    return st;
                }
                assert!(Instant::now() < deadline, "call never reached {want}: {st}");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }

        async fn next(&mut self) -> Event {
            let (r, buf) = &mut self.wire;
            let line = tokio::time::timeout(Duration::from_secs(5), read_frame(r, buf)).await.expect("a frame within 5 s").unwrap();
            Envelope::parse_from(&line, Source::Host).unwrap().event
        }

        async fn drain_to_marker(&mut self, marker: &str) -> Vec<Event> {
            self.ctl.send(Control::Text(marker.into())).await.unwrap();
            let mut seen = Vec::new();
            loop {
                match self.next().await {
                    Event::UserText { text, .. } if text == marker => return seen,
                    e => seen.push(e),
                }
            }
        }

        async fn speak(&mut self, rid: &str, chunks: u64) {
            let (g, w) = &mut self.agent;
            let mut lines = g.stamp(Event::ResponseStarted { response_id: rid.into(), request_id: "c1".into(), utterance_id: None }, None).to_line();
            for c in 0..chunks {
                let data = base64::engine::general_purpose::STANDARD.encode(vec![0x20u8; 9600]);
                let ev = Event::AgentAudio { response_id: rid.into(), chunk: c, sample_offset: c * 4800, sample_rate: 24_000, format: "s16le".into(), data };
                lines.push_str(&g.stamp(ev, None).to_line());
            }
            w.write_all(lines.as_bytes()).await.unwrap();
        }

        fn states_logged(&self) -> Vec<String> {
            let mut v: Vec<String> = self.logs.lock().unwrap().iter().filter(|(k, _)| k == "call").map(|(_, v)| v["state"].as_str().unwrap().to_string()).collect();
            v.dedup();
            v
        }

        async fn quit(self) -> VoiceSummary {
            self.ctl.send(Control::Quit).await.unwrap();
            self.driver.await.unwrap().unwrap()
        }
    }

    fn user_audio(events: &[Event]) -> usize {
        events.iter().filter(|e| matches!(e, Event::UserAudio { .. })).count()
    }

    #[tokio::test]
    async fn start_opens_the_devices_and_becomes_active_from_read_back() {
        let mut r = rig();
        let st = r.status();
        assert_eq!((st["state"].as_str(), st["desired"].as_str(), st["device_open"].as_bool()), (Some("off"), Some("off"), Some(false)));
        assert_eq!(CallPlayout(r.call.clone()).unavailable().as_deref(), Some(NO_CALL), "no call: the speaker refuses agent audio");
        let receipt = r.call.request(true, "test");
        assert_eq!((receipt["request"].as_str(), receipt["desired"].as_str()), (Some("start"), Some("on")), "a request receipt, not the outcome");
        let st = r.until("active").await;
        assert_eq!((st["device_open"].as_bool(), st["input"]["call_input"].as_str(), st["calls_opened"].as_u64()), (Some(true), Some("open"), Some(1)));
        assert_eq!(r.mic.starts.load(Ordering::SeqCst), 1);
        assert_eq!(r.speakers.log(), vec!["start 1"]);
        assert!(CallPlayout(r.call.clone()).unavailable().is_none());

        loop {
            if let Event::UserAudio { .. } = r.next().await {
                break;
            }
        }
        assert_eq!(r.states_logged(), vec!["starting", "active"]);
        r.quit().await;
    }

    #[tokio::test]
    async fn end_releases_the_devices_fences_the_mic_and_keeps_the_agent_link() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.call.request(false, "test");
        let st = r.until("off").await;
        assert_eq!((st["desired"].as_str(), st["device_open"].as_bool(), st["input"]["call_input"].as_str()), (Some("off"), Some(false), Some("closed")));
        assert_eq!((r.mic.starts.load(Ordering::SeqCst), r.mic.stops.load(Ordering::SeqCst)), (1, 1), "the microphone is released");
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1"], "the speaker is released");
        assert_eq!(CallPlayout(r.call.clone()).unavailable().as_deref(), Some(NO_CALL));

        let seen = r.drain_to_marker("after the call").await;
        let end = seen.iter().position(|e| matches!(e, Event::UserAudioEnd { .. })).expect("the stream was ended");
        assert_eq!(user_audio(&seen[end..]), 0, "no microphone audio after the end: {:?}", &seen[end..]);

        assert!(!r.driver.is_finished());
        r.ctl.send(Control::Submit { command_id: "c2".into(), session_id: "s".into(), expected_generation: 1, text: "still here?".into() }).await.unwrap();
        loop {
            if let Event::CommandSubmitted { command_id, .. } = r.next().await {
                assert_eq!(command_id, "c2");
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(user_audio(&r.drain_to_marker("still no call").await), 0);
        assert_eq!(r.states_logged(), vec!["starting", "active", "ending", "off"]);
        r.quit().await;
    }

    #[tokio::test]
    async fn start_after_end_opens_fresh_devices() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        r.call.request(false, "test");
        r.until("off").await;
        let _ = r.drain_to_marker("between calls").await;
        let receipt = r.call.request(true, "test");
        assert_eq!(receipt["attempt"].as_u64(), Some(2));
        let st = r.until("active").await;
        assert_eq!(st["calls_opened"].as_u64(), Some(2));
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1", "start 2"], "a new speaker per call");
        assert_eq!(r.mic.starts.load(Ordering::SeqCst), 2);
        let mut streams = std::collections::BTreeSet::new();
        while streams.is_empty() {
            if let Event::UserAudio { stream_id, .. } = r.next().await {
                streams.insert(stream_id);
            }
        }
        r.quit().await;
    }

    #[tokio::test]
    async fn end_while_starting_waits_for_the_start_then_releases_it_and_never_admits() {
        let mut r = rig();
        *r.mic.delay.lock().unwrap() = Duration::from_millis(300);
        r.call.request(true, "test");
        let deadline = Instant::now() + Duration::from_secs(3);
        while r.status()["reason"] != "opening the devices" {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let t = Instant::now();
        let receipt = r.call.request(false, "test");
        assert!(t.elapsed() < Duration::from_millis(50), "End during Starting reached the owner in {:?}", t.elapsed());
        assert_eq!((receipt["desired"].as_str(), receipt["state"].as_str()), (Some("off"), Some("ending")));
        let st = r.status();
        assert_eq!(st["state"].as_str(), Some("ending"), "the desired state is off, the devices are still starting: {st}");
        let st = r.until("off").await;
        assert_eq!(st["device_open"].as_bool(), Some(false));
        assert_eq!((r.mic.starts.load(Ordering::SeqCst), r.mic.stops.load(Ordering::SeqCst)), (1, 1));
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1"]);
        assert!(!r.states_logged().contains(&"active".to_string()), "never active: {:?}", r.states_logged());
        assert_eq!(user_audio(&r.drain_to_marker("after").await), 0, "the input was never opened");
        r.quit().await;
    }

    fn log_order(r: &Rig) -> Vec<String> {
        r.logs.lock().unwrap().iter().map(|(k, v)| if k == "call" { format!("call:{}", v["state"].as_str().unwrap()) } else { k.clone() }).collect()
    }

    fn assert_released_then_fresh(r: &Rig, superseded: bool) {
        let order = log_order(r);
        let released = order.iter().position(|k| k == "call_devices_released").expect("the first devices were released");
        let opened: Vec<usize> = order.iter().enumerate().filter(|(_, k)| *k == "call_devices_opened").map(|(i, _)| i).collect();
        assert_eq!(opened.len(), 2, "{order:?}");
        assert!(opened[0] < released && released < opened[1], "release before the fresh open: {order:?}");
        let active: Vec<usize> = order.iter().enumerate().filter(|(_, k)| *k == "call:active").map(|(i, _)| i).collect();
        if superseded {
            assert!(active.iter().all(|&a| a > opened[1]), "never active on the superseded devices: {order:?}");
        }
        assert!(!active.iter().any(|&a| a > released && a < opened[1]), "not active between the release and the fresh open: {order:?}");
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1", "start 2"], "never two device owners");
        assert_eq!((r.mic.stops.load(Ordering::SeqCst), r.call.opened.load(Ordering::SeqCst)), (1, 2));
    }

    #[tokio::test]
    async fn start_end_start_while_opening_releases_the_opening_devices_then_opens_fresh_ones() {
        let mut r = rig();
        let barrier = r.mic.hold_next_start();
        r.call.request(true, "test");
        let deadline = Instant::now() + Duration::from_secs(3);
        while r.mic.entered.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }

        let t = Instant::now();
        r.call.request(false, "test");
        assert!(t.elapsed() < Duration::from_millis(50), "End was accepted in {:?}", t.elapsed());
        assert!(r.urgent.mute.snapshot().call_closed, "the call input is fenced while the start is pending");
        let receipt = r.call.request(true, "test");
        assert_eq!((receipt["attempt"].as_u64(), receipt["desired"].as_str(), receipt["state"].as_str()), (Some(2), Some("on"), Some("ending")));
        assert!(r.status()["reason"].as_str().unwrap().contains("queued until the devices are released"), "{}", r.status());
        barrier.send(()).unwrap();
        let st = r.until("active").await;
        assert_eq!((st["attempt"].as_u64(), st["input"]["call_input"].as_str(), st["calls_opened"].as_u64()), (Some(2), Some("open"), Some(2)), "{st}");
        assert_released_then_fresh(&r, true);
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.quit().await;
    }

    #[tokio::test]
    async fn end_start_before_the_loop_runs_cancels_the_old_reply_before_a_fresh_speaker() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        r.speak("v1", 5).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        while r.speakers.queued() < 24_000 {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }

        r.call.request(false, "end");
        r.call.request(true, "restart");
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1"], "the old devices are released, and NO fresh speaker opens before the output close");
        assert_eq!(r.call.opened.load(Ordering::SeqCst), 1);

        let st = r.until("active").await;
        assert_eq!((st["attempt"].as_u64(), st["calls_opened"].as_u64()), (Some(2), Some(2)));
        assert_released_then_fresh(&r, false);
        assert_eq!(r.speakers.queued_at_start.lock().unwrap().last().copied(), Some(0), "the fresh speaker started on an empty queue: zero old samples to render");
        let seen = r.drain_to_marker("after restart").await;
        assert!(seen.iter().any(|e| matches!(e, Event::Cancel { response_id } if response_id == "v1")), "End cancelled the old reply: {seen:?}");
        assert!(
            seen.iter().any(|e| matches!(e, Event::Playback { response_id, phase: PlaybackPhase::Cleared, reason, .. } if response_id == "v1" && reason.as_deref() == Some("user_cancel"))),
            "the old reply's queued audio was cleared, with its record"
        );
        let close = r.logs.lock().unwrap().iter().find(|(k, _)| k == "call_output_close").map(|(_, v)| v.clone()).expect("the worker waited on the output close");
        assert_eq!((close["acknowledged"].as_bool(), close["queued_samples_after"].as_u64()), (Some(true), Some(0)), "{close}");
        assert_eq!(r.speakers.queued(), 0);

        {
            let (g, w) = &mut r.agent;
            let data = base64::engine::general_purpose::STANDARD.encode(vec![0x20u8; 9600]);
            let late = Event::AgentAudio { response_id: "v1".into(), chunk: 5, sample_offset: 24_000, sample_rate: 24_000, format: "s16le".into(), data };
            w.write_all(g.stamp(late, None).to_line().as_bytes()).await.unwrap();
        }
        let _ = r.drain_to_marker("after the late chunk").await;
        assert_eq!(r.speakers.queued(), 0, "the old reply's late chunk never reaches the fresh speaker");
        let st = r.status();
        assert_eq!((st["output_close"]["call_epoch"].as_u64() > Some(0), st["output_close"]["call_epoch"] == st["output_close"]["done_through"]), (true, true), "{st}");
        let summary = r.quit().await;
        assert_eq!(summary.queue.rendered_after_fence, 0);
    }

    #[tokio::test]
    async fn active_end_start_before_the_worker_runs_still_releases_then_reopens() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        {

            let _held = r.call.devices.lock().unwrap();
            r.call.request(false, "test");
            r.call.request(true, "test");
            assert_eq!(r.status()["state"].as_str(), Some("ending"));
        }
        let st = r.until("active").await;
        assert_eq!((st["attempt"].as_u64(), st["calls_opened"].as_u64()), (Some(2), Some(2)));
        assert_released_then_fresh(&r, false);
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.quit().await;
    }

    #[tokio::test]
    async fn repeated_requests_converge_on_the_latest() {
        let mut r = rig();
        r.call.request(true, "test");
        r.call.request(true, "test");
        r.until("active").await;
        assert_eq!(r.call.request(true, "test")["attempt"].as_u64(), Some(1), "a repeated start is the same attempt");
        {
            let _held = r.call.devices.lock().unwrap();
            for on in [false, false, true, true, false, true] {
                r.call.request(on, "test");
            }
        }
        let st = r.until("active").await;
        assert_eq!((st["desired"].as_str(), st["calls_opened"].as_u64()), (Some("on"), Some(2)), "{st}");
        assert_released_then_fresh(&r, false);
        {
            let _held = r.call.devices.lock().unwrap();
            for on in [false, true, false] {
                r.call.request(on, "test");
            }
        }
        let st = r.until("off").await;
        assert_eq!((st["device_open"].as_bool(), st["input"]["call_input"].as_str()), (Some(false), Some("closed")));
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1", "start 2", "stop 2"]);
        let _ = r.drain_to_marker("ended").await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(user_audio(&r.drain_to_marker("still ended").await), 0, "nothing from the microphone after the end");
        r.quit().await;
    }

    #[tokio::test]
    async fn a_failed_start_is_an_error_with_its_reason_until_the_next_request() {
        let mut r = rig();
        *r.mic.fail.lock().unwrap() = Some("microphone permission denied".into());
        r.call.request(true, "test");
        let st = r.until("error").await;
        assert!(st["error"].as_str().unwrap().contains("microphone permission denied"), "{st}");
        assert_eq!((st["desired"].as_str(), st["device_open"].as_bool()), (Some("on"), Some(false)), "desired and actual apart");
        assert_eq!(r.speakers.log(), vec!["start 1", "stop 1"], "the speaker that did start is released");

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(r.status()["state"].as_str(), Some("error"));
        assert_eq!(user_audio(&r.drain_to_marker("x").await), 0);

        r.call.request(false, "test");
        r.until("off").await;
        r.call.request(true, "test");
        let st = r.until("active").await;
        assert!(st["error"].is_null());
        r.quit().await;
    }

    #[tokio::test]
    async fn without_devices_a_start_is_an_error_and_an_end_is_off() {
        let urgent = Arc::new(Urgent::default());
        let call = Call::new(None, urgent.clone(), Box::new(|_: &str, _| {}));
        call.request(true, "test");
        let st = call.status(&urgent.mute.snapshot());
        assert_eq!((st["state"].as_str(), st["configured"].as_bool()), (Some("error"), Some(false)));
        assert!(st["reason"].as_str().unwrap().contains("no voice devices configured"), "{st}");
        call.request(false, "test");
        assert_eq!(call.status(&urgent.mute.snapshot())["state"].as_str(), Some("off"));
    }

    #[tokio::test]
    async fn mute_during_a_call_is_its_own_input_and_survives_end_and_start() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.urgent.mute.request(true);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !r.urgent.mute.snapshot().effective_muted() {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let st = r.status();
        assert_eq!((st["state"].as_str(), st["device_open"].as_bool()), (Some("active"), Some(true)), "mute is not End call");
        let seen = r.drain_to_marker("muted").await;
        let end = seen.iter().position(|e| matches!(e, Event::UserAudioEnd { .. })).expect("mute ends the stream");
        assert_eq!(user_audio(&seen[end..]), 0);

        r.call.request(false, "test");
        r.until("off").await;
        assert!(r.urgent.mute.requested());
        r.call.request(true, "test");
        r.until("active").await;
        assert!(r.urgent.mute.requested(), "a new call starts muted when the person muted");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(user_audio(&r.drain_to_marker("still muted").await), 0);
        r.urgent.mute.request(false);
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.quit().await;
    }

    #[tokio::test]
    async fn stop_during_a_call_cuts_the_speech_and_the_call_stays_up() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        r.speak("v1", 5).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        while r.speakers.queued() < 24_000 {
            assert!(Instant::now() < deadline, "the reply is queued on the call speaker");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        r.urgent.stop();
        let mut cancelled = false;
        while !cancelled {
            if let Event::Cancel { response_id } = r.next().await {
                cancelled = response_id == "v1";
            }
        }
        assert_eq!(r.speakers.queued(), 0, "the speaker is cleared");
        let st = r.status();
        assert_eq!((st["state"].as_str(), st["device_open"].as_bool()), (Some("active"), Some(true)), "Stop does not end the call");
        while !matches!(r.next().await, Event::UserAudio { .. }) {}
        r.quit().await;
    }

    #[tokio::test]
    async fn end_during_a_reply_clears_the_speaker_and_cancels_it() {
        let mut r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        r.speak("v1", 5).await;
        let deadline = Instant::now() + Duration::from_secs(3);
        while r.speakers.queued() < 24_000 {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        r.call.request(false, "test");
        let (mut cancel, mut cleared) = (false, false);
        while !(cancel && cleared) {
            match r.next().await {
                Event::Cancel { response_id } => cancel = response_id == "v1",
                Event::Playback { phase: PlaybackPhase::Cleared, reason, .. } => cleared = reason.as_deref() == Some("user_cancel"),
                _ => {}
            }
        }
        r.until("off").await;
        assert_eq!(r.speakers.queued(), 0);
        r.quit().await;
    }

    #[tokio::test]
    async fn with_no_call_agent_audio_is_refused_never_claimed_as_played() {
        let mut r = rig();
        r.speak("v0", 2).await;
        loop {
            if let Event::Playback { phase, played_samples, reason, .. } = r.next().await {
                assert_eq!((phase, played_samples, reason.as_deref()), (PlaybackPhase::Cleared, 0, Some("device_unavailable")));
                break;
            }
        }
        assert!(r.speakers.log().is_empty(), "no speaker was opened");
        r.quit().await;
    }

    #[tokio::test]
    async fn a_link_end_releases_the_devices_and_the_next_link_reopens_them() {
        let r = rig();
        r.call.request(true, "test");
        r.until("active").await;
        let (call, urgent, mic, speakers) = (r.call.clone(), r.urgent.clone(), r.mic.clone(), r.speakers.clone());

        drop(r.agent);
        drop(r.wire);
        let _ = r.driver.await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while call.status(&urgent.mute.snapshot())["reason"] != "waiting for the agent link: the devices open when the voice loop runs" {
            assert!(Instant::now() < deadline, "{}", call.status(&urgent.mute.snapshot()));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(call.status(&urgent.mute.snapshot())["state"].as_str(), Some("starting"));
        assert_eq!((mic.stops.load(Ordering::SeqCst), speakers.log()), (1, vec!["start 1".to_string(), "stop 1".to_string()]));

        let (host_side, _agent_side) = tokio::io::duplex(1 << 22);
        let (hr, hw) = tokio::io::split(host_side);
        let mut reader = BufReader::new(hr);
        let (ctl, mut ctl_rx) = mpsc::channel(16);
        let (notes_tx, _notes) = mpsc::unbounded_channel();
        let (mut cap, mut out) = (CallCapture(call.clone()), CallPlayout(call.clone()));
        let u2 = urgent.clone();
        let stamper = Stamper::new(Source::Host, "host-t2", Box::new(MonotonicClock::start()));
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut cap, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u2).await });
        let deadline = Instant::now() + Duration::from_secs(5);
        while call.status(&urgent.mute.snapshot())["state"] != "active" {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(speakers.log().last().map(String::as_str), Some("start 2"));
        ctl.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
    }
}
