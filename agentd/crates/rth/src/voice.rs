

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use rth_protocol::io::read_frame;
use rth_protocol::{Envelope, Event, Source, Stamper};
use rth_voice::mic::{DEFAULT_CHUNK_MS, MicChunker, Tally};
use rth_voice::owner::{ClearReason, Cleared, OwnerStats, SpeechOwner};
use rth_voice::echo::{EchoConfig, EchoMonitor, NearEnd, Probe};
use rth_voice::queue::{PlayQueue, QueueStats};
use rth_voice::vad::{Vad, VadConfig, VadEvent};
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::devices::{Capture, Playout};

pub const MEDIA_QUEUE: usize = 50;

pub const CONTROL_QUEUE: usize = 256;

pub const SHUTDOWN_GRACE: Duration = Duration::from_millis(300);

pub const ABORT_CONFIRM: Duration = Duration::from_millis(500);

pub const SETTLE_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BargeIn {

    Provider,

    Local,

    Gated,

    Off,
}

impl BargeIn {
    pub fn as_str(self) -> &'static str {
        match self {
            BargeIn::Provider => "provider",
            BargeIn::Local => "local",
            BargeIn::Gated => "gated",
            BargeIn::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "provider" => BargeIn::Provider,
            "local" => BargeIn::Local,
            "gated" => BargeIn::Gated,
            "off" => BargeIn::Off,
            _ => bail!("--barge-in provider|local|gated|off, not {s}"),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkMode {

    Open,

    PushToTalk,
}

impl TalkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TalkMode::Open => "open",
            TalkMode::PushToTalk => "push_to_talk",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VoiceOptions {
    pub vad: VadConfig,

    pub echo: EchoConfig,
    pub chunk_ms: u32,
    pub barge_in: BargeIn,
    pub talk: TalkMode,
    pub stream_prefix: String,

    pub queue_seconds: f64,
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            vad: VadConfig::default(),
            echo: EchoConfig::default(),
            chunk_ms: DEFAULT_CHUNK_MS,
            barge_in: BargeIn::Provider,
            talk: TalkMode::Open,
            stream_prefix: "mic".into(),
            queue_seconds: 60.0,
        }
    }
}

impl VoiceOptions {

    pub fn policy_json(&self) -> serde_json::Value {
        let v = self.vad;
        serde_json::json!({
            "barge_in": self.barge_in.as_str(),
            "local_vad_cuts": matches!(self.barge_in, BargeIn::Local),
            "local_gate_cuts": matches!(self.barge_in, BargeIn::Gated),
            "provider_tail_flush": matches!(self.barge_in, BargeIn::Provider | BargeIn::Local | BargeIn::Gated),
            "provider_interrupted_flush": true,
            "talk": self.talk.as_str(),
            "turn_taking": "provider VAD (server automatic activity detection; replaceable baseline)",
            "vad": {"kind": "energy", "threshold_dbfs": v.threshold_dbfs, "frame_ms": v.frame_ms, "onset_frames": v.onset_frames, "hang_frames": v.hang_frames},
            "echo_reference": self.echo.describe(),
            "chunk_ms": self.chunk_ms,
            "queue_seconds": self.queue_seconds,
        })
    }
}

#[derive(Default)]
pub struct Urgent {
    intent: std::sync::atomic::AtomicU8,
    notify: tokio::sync::Notify,
    pub mute: MuteFence,
}

impl Urgent {
    pub const STOP: u8 = 1;
    pub const QUIT: u8 = 2;

    pub fn stop(&self) {
        self.intent.fetch_or(Self::STOP, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn quit(&self) {
        self.intent.fetch_or(Self::QUIT, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_one();
    }

    pub fn take(&self) -> u8 {
        self.intent.swap(0, std::sync::atomic::Ordering::SeqCst)
    }

    pub fn pending(&self) -> u8 {
        self.intent.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn notified(&self) {
        self.notify.notified().await
    }
}

#[derive(Default)]
pub struct MuteFence {
    shared: Arc<FenceShared>,
    changed: tokio::sync::Notify,
}

#[derive(Default)]
struct FenceShared {
    state: Mutex<FenceState>,

    acked: std::sync::Condvar,

    unsettled: Mutex<Option<tokio::task::JoinHandle<Result<()>>>>,
    #[cfg(test)]
    pause: Mutex<Option<Arc<tests::WriterPause>>>,
}

impl FenceShared {

    fn reserve(&self, generation: u64, what: String) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.generation != generation {
            s.revoked_chunks += 1;
            return false;
        }
        s.reserved = Some(what);
        true
    }

    fn release(&self) {
        self.state.lock().unwrap().reserved = None;
    }

    #[cfg(test)]
    async fn pause(&self, point: &'static str) {
        let hook = self.pause.lock().unwrap().clone();
        if let Some(h) = hook {
            h.at(point).await;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FenceState {
    pub requested: bool,
    pub generation: u64,
    pub requested_at: Option<Instant>,

    pub open_since: Option<Instant>,

    pub call_closed: bool,
    pub call_changed_at: Option<Instant>,

    pub applied_call_closed: bool,

    pub call_epoch: u64,

    pub output_closed_epoch: u64,
    pub applied: bool,
    pub applied_gen: u64,
    pub applied_at: Option<Instant>,

    pub loop_running: bool,

    pub writer_abort_unconfirmed: bool,

    pub reserved: Option<String>,

    pub in_flight_at_fence: Option<String>,

    pub prefix_samples_discarded: u64,

    pub revoked_chunks: u64,
}

impl FenceState {

    pub fn effective_muted(&self) -> bool {
        self.requested && !self.writer_abort_unconfirmed && (!self.loop_running || (self.applied && self.applied_gen == self.generation))
    }

    pub fn effective_call_closed(&self) -> bool {
        self.call_closed && !self.writer_abort_unconfirmed && (!self.loop_running || (self.applied_call_closed && self.applied_gen == self.generation))
    }

    pub fn effective_call_open(&self) -> bool {
        !self.call_closed && self.loop_running && !self.applied_call_closed && self.applied_gen == self.generation
    }
}

impl MuteFence {

    pub fn request(&self, muted: bool) -> bool {
        let mut s = self.shared.state.lock().unwrap();
        let was = s.requested;
        if was != muted {
            let now = Instant::now();
            s.requested = muted;
            s.generation += 1;
            s.requested_at = Some(now);
            if !muted {
                s.open_since = Some(now);
            }
            drop(s);
            self.changed.notify_one();
        }
        was
    }

    pub fn requested(&self) -> bool {
        self.shared.state.lock().unwrap().requested
    }

    pub fn close_call(&self, closed: bool) -> bool {
        let mut s = self.shared.state.lock().unwrap();
        let was = s.call_closed;
        if was != closed {
            let now = Instant::now();
            s.call_closed = closed;
            s.generation += 1;
            s.call_changed_at = Some(now);
            if closed {
                s.call_epoch += 1;
            }
            if !closed {
                s.open_since = Some(now);
            }
            drop(s);
            self.changed.notify_one();
        }
        was
    }

    pub fn snapshot(&self) -> FenceState {
        self.shared.state.lock().unwrap().clone()
    }

    pub fn wait_output_closed(&self, within: Duration) -> bool {
        let s = self.shared.state.lock().unwrap();
        let (s, _) = self.shared.acked.wait_timeout_while(s, within, |s| s.loop_running && s.output_closed_epoch < s.call_epoch).unwrap();
        !s.loop_running || s.output_closed_epoch >= s.call_epoch
    }

    fn admits(&self, at: Instant, samples: usize) -> bool {
        let s = self.shared.state.lock().unwrap();
        let span = Duration::from_nanos(samples as u64 * 1_000_000_000 / rth_protocol::MIC_SAMPLE_RATE as u64);
        !s.requested && !s.call_closed && s.open_since.is_none_or(|o| at.checked_sub(span).is_some_and(|start| start >= o))
    }

    fn loop_started(&self) -> (u64, bool, bool, u64) {
        let mut s = self.shared.state.lock().unwrap();
        debug_assert!(!s.writer_abort_unconfirmed, "settle_previous runs first");
        s.loop_running = true;
        s.reserved = None;
        (s.applied, s.applied_gen, s.applied_at, s.in_flight_at_fence) = (s.requested, s.generation, Some(Instant::now()), None);
        s.applied_call_closed = s.call_closed;

        s.output_closed_epoch = s.call_epoch;
        (s.generation, s.requested, s.call_closed, s.call_epoch)
    }

    fn writer_abort_unconfirmed(&self, writer: tokio::task::JoinHandle<Result<()>>) {
        self.shared.state.lock().unwrap().writer_abort_unconfirmed = true;
        *self.shared.unsettled.lock().unwrap() = Some(writer);
    }

    async fn settle_previous(&self, wait: Duration) -> bool {
        let Some(mut old) = self.shared.unsettled.lock().unwrap().take() else { return true };
        if tokio::time::timeout(wait, &mut old).await.is_err() {
            *self.shared.unsettled.lock().unwrap() = Some(old);
            return false;
        }
        let mut s = self.shared.state.lock().unwrap();
        (s.writer_abort_unconfirmed, s.reserved, s.loop_running) = (false, None, false);
        self.shared.acked.notify_all();
        true
    }

    fn loop_ended(&self) {
        let mut s = self.shared.state.lock().unwrap();
        s.loop_running = false;
        s.reserved = None;
        self.shared.acked.notify_all();
    }

    fn pending(&self, applied_gen: u64) -> Option<(u64, bool, bool, u64)> {
        let s = self.shared.state.lock().unwrap();
        (s.generation != applied_gen).then_some((s.generation, s.requested, s.call_closed, s.call_epoch))
    }

    fn applied(&self, generation: u64, muted: bool, call_closed: bool, output_closed_epoch: u64, discarded: u64) -> Option<String> {
        let mut s = self.shared.state.lock().unwrap();
        s.applied = muted;
        s.applied_call_closed = call_closed;
        s.output_closed_epoch = output_closed_epoch;
        self.shared.acked.notify_all();
        s.applied_gen = generation;
        s.applied_at = Some(Instant::now());
        s.in_flight_at_fence = s.reserved.clone();
        s.prefix_samples_discarded += discarded;
        s.in_flight_at_fence.clone()
    }
}

pub enum Control {

    Cancel,

    EndStream,
    Text(String),

    Talk(bool),

    Submit { command_id: String, session_id: String, expected_generation: u64, text: String },

    Send(Event),
    Quit,
}

pub enum Note {
    In { t: Instant, env: Envelope },

    Out { t: Instant, env: Envelope },

    Onset { t: Instant, stream_id: String, start_sample: u64, detected_sample: u64, audible: bool, action: &'static str, level_db: f32, echo_db: Option<f32> },

    NearEnd { t: Instant, near: NearEnd, audible: bool, action: &'static str },

    EchoProbe { response_id: String, epoch: u64, phase: &'static str, rendered_ms: f64, probe: Option<Probe> },

    Talk { t: Instant, down: bool, applied: bool },

    Mute { t: Instant, muted: bool, call_closed: bool, call_epoch: u64, output_closed: bool, generation: u64, stream_ended: bool, discarded_samples: u64, in_flight: Option<String> },
    SpeechEnd { t: Instant, end_sample: u64 },

    Cleared { t: Instant, trigger: Instant, cleared: Cleared, cancelled: Vec<String> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelCounts {
    pub accepted: u64,
    pub refused: u64,
    pub written: u64,

    pub revoked: u64,
}

impl ChannelCounts {

    pub fn unresolved(&self) -> u64 {
        self.accepted.saturating_sub(self.written + self.revoked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EndStatus {

    #[default]
    NotSent,

    Refused,

    QueuedUnwritten,

    InFlight,
    Written,
}

impl EndStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EndStatus::NotSent => "not_sent",
            EndStatus::Refused => "refused",
            EndStatus::QueuedUnwritten => "queued_unwritten",
            EndStatus::InFlight => "in_flight_uncertain",
            EndStatus::Written => "written",
        }
    }
}

#[derive(Clone, Default)]
pub struct StreamOutcome {
    pub stream_id: String,

    pub chunks: ChannelCounts,

    pub written: Tally,

    pub revoked: u64,
    pub end: EndStatus,
}

#[derive(Clone, Default)]
pub struct WriterStats {
    pub control: ChannelCounts,
    pub media: ChannelCounts,

    pub in_flight: Option<String>,

    pub aborted: bool,

    pub streams: Vec<StreamOutcome>,
}

impl WriterStats {
    fn stream(&mut self, id: &str) -> &mut StreamOutcome {
        let i = match self.streams.iter().position(|s| s.stream_id == id) {
            Some(i) => i,
            None => {
                self.streams.push(StreamOutcome { stream_id: id.into(), ..Default::default() });
                self.streams.len() - 1
            }
        };
        &mut self.streams[i]
    }
}

pub struct VoiceSummary {
    pub owner: OwnerStats,
    pub queue: QueueStats,

    pub writer: WriterStats,

    pub mic_frames_dropped: u64,
    pub onsets: u64,
    pub capture: String,
    pub playout: String,

    pub devices_stopped_after: Duration,
}

struct Outbox {
    control: mpsc::Sender<Event>,

    media: mpsc::Sender<(Event, Option<u64>)>,
    stats: Arc<Mutex<WriterStats>>,
}

impl Outbox {
    fn control(&self, e: Event) {
        let ok = self.control.try_send(e).is_ok();
        let mut st = self.stats.lock().unwrap();
        if ok { st.control.accepted += 1 } else { st.control.refused += 1 }
    }

    fn media(&self, e: Event, generation: Option<u64>) -> bool {
        let (stream, is_end) = match &e {
            Event::UserAudio { stream_id, .. } => (stream_id.clone(), false),
            Event::UserAudioEnd { stream_id, .. } => (stream_id.clone(), true),
            _ => (String::new(), false),
        };
        let mut st = self.stats.lock().unwrap();
        st.stream(&stream);
        let ok = matches!(self.media.try_send((e, generation)), Ok(()));
        if ok { st.media.accepted += 1 } else { st.media.refused += 1 }
        let s = st.stream(&stream);
        match (is_end, ok) {
            (false, true) => s.chunks.accepted += 1,
            (false, false) => s.chunks.refused += 1,
            (true, true) => {
                if s.end != EndStatus::Written {
                    s.end = EndStatus::QueuedUnwritten
                }
            }
            (true, false) => s.end = EndStatus::Refused,
        }
        ok
    }
}

fn describe(e: &Event) -> String {
    match e {
        Event::UserAudio { stream_id, chunk, .. } => format!("user_audio {stream_id} #{chunk}"),
        Event::UserAudioEnd { stream_id, .. } => format!("user_audio_end {stream_id}"),
        Event::Cancel { response_id } => format!("cancel {response_id}"),
        Event::UserText { request_id, .. } => format!("user_text {request_id}"),
        Event::CommandSubmitted { command_id, .. } => format!("command_submitted {command_id}"),
        Event::Playback { response_id, phase, .. } => format!("playback {response_id} {phase:?}"),
        _ => "other".into(),
    }
}

async fn writer<W: AsyncWrite + Unpin>(
    mut write: W,
    mut stamper: Stamper,
    mut control: mpsc::Receiver<Event>,
    mut media: mpsc::Receiver<(Event, Option<u64>)>,
    stats: Arc<Mutex<WriterStats>>,
    notes: mpsc::UnboundedSender<Note>,
    fence: Arc<FenceShared>,
) -> Result<()> {
    loop {
        let (mut event, is_media) = tokio::select! {
            biased;
            Some(e) = control.recv() => (e, false),
            Some((e, generation)) = media.recv() => {
                #[cfg(test)]
                fence.pause("before_reserve").await;

                if let (Event::UserAudio { stream_id, .. }, Some(g)) = (&e, generation) {
                    if !fence.reserve(g, describe(&e)) {
                        let mut st = stats.lock().unwrap();
                        st.media.revoked += 1;
                        st.stream(stream_id).revoked += 1;
                        continue;
                    }
                    #[cfg(test)]
                    fence.pause("after_reserve").await;
                }
                (e, true)
            }
            else => return Ok(()),
        };

        if let Event::UserAudioEnd { stream_id, dropped_chunks, .. } = &mut event {
            *dropped_chunks += stats.lock().unwrap().stream(stream_id).revoked;
        }
        stats.lock().unwrap().in_flight = Some(describe(&event));
        let env = stamper.stamp(event, None);
        write.write_all(env.to_line().as_bytes()).await?;
        if matches!(env.event, Event::UserAudio { .. }) {
            fence.release();
        }
        let t = Instant::now();
        {
            let mut st = stats.lock().unwrap();
            st.in_flight = None;
            if is_media { st.media.written += 1 } else { st.control.written += 1 }
            match &env.event {
                Event::UserAudio { stream_id, .. } => st.stream(stream_id).written.sent(&env.event),
                Event::UserAudioEnd { stream_id, .. } => st.stream(stream_id).end = EndStatus::Written,
                _ => {}
            }
        }
        let _ = notes.send(Note::Out { t, env });
    }
}

struct Mic {
    prefix: String,
    chunk_ms: u32,
    n: u64,

    generation: u64,
    chunker: MicChunker,

    dropped: u64,
}

impl Mic {
    fn new(prefix: &str, chunk_ms: u32) -> Self {
        Self {
            prefix: prefix.into(),
            chunk_ms,
            n: 1,
            generation: 0,
            chunker: MicChunker::new(format!("{prefix}-1"), chunk_ms),
            dropped: 0,
        }
    }

    fn push(&mut self, frame: &[i16], out: &Outbox, generation: u64) {
        for e in self.chunker.push(frame) {
            if !out.media(e, Some(generation)) {
                self.dropped += 1;
            }
        }
    }

    fn fence(&mut self, out: &Outbox) -> (u64, bool) {
        let discarded = self.chunker.discard() as u64;
        let ended = self.chunker.samples_emitted() > 0 || self.dropped > 0;
        if ended {
            self.end(out);
        }
        (discarded, ended)
    }

    fn end(&mut self, out: &Outbox) {

        if self.chunker.samples_emitted() == 0 && self.dropped == 0 && self.chunker.buffered() == 0 {
            return;
        }
        let full = self.chunker.samples_emitted() / self.chunker.chunk_samples() as u64;
        let (last, _) = self.chunker.finish(&Tally::default());
        let chunks = full + u64::from(last.is_some());
        if let Some(e) = last
            && !out.media(e, Some(self.generation))
        {
            self.dropped += 1;
        }
        let stream_id = self.chunker.stream_id.clone();
        let marker = Event::UserAudioEnd { stream_id, chunks, samples: self.chunker.samples_emitted(), dropped_chunks: self.dropped, speaker: Default::default() };

        let _ = out.media(marker, None);
        self.n += 1;
        self.chunker = MicChunker::new(format!("{}-{}", self.prefix, self.n), self.chunk_ms);
        self.dropped = 0;
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn drive<R, W>(
    reader: &mut R,
    write: W,
    stamper: Stamper,
    capture: &mut dyn Capture,
    playout: &mut dyn Playout,
    opts: VoiceOptions,
    ctl: &mut mpsc::Receiver<Control>,
    notes: mpsc::UnboundedSender<Note>,
) -> Result<VoiceSummary>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    drive_with(reader, write, stamper, capture, playout, opts, ctl, notes, &Urgent::default()).await
}

#[allow(clippy::too_many_arguments)]
pub async fn drive_with<R, W>(
    reader: &mut R,
    write: W,
    stamper: Stamper,
    capture: &mut dyn Capture,
    playout: &mut dyn Playout,
    opts: VoiceOptions,
    ctl: &mut mpsc::Receiver<Control>,
    notes: mpsc::UnboundedSender<Note>,
    urgent: &Urgent,
) -> Result<VoiceSummary>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    if opts.vad.sample_rate != rth_protocol::MIC_SAMPLE_RATE {
        bail!("the VAD runs on {} Hz microphone frames", rth_protocol::MIC_SAMPLE_RATE);
    }

    if !urgent.mute.settle_previous(SETTLE_WAIT).await {
        bail!("the previous link writer was aborted but has not been observed to finish; not starting a new one");
    }
    let queue = Arc::new(PlayQueue::with_seconds(playout.rate(), opts.queue_seconds));
    let mut owner = SpeechOwner::new(queue.clone());
    playout.start(queue.clone())?;
    let (mic_tx, mut mic_rx) = mpsc::channel::<crate::devices::MicFrame>(256);
    let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
    if let Err(e) = capture.start(mic_tx, dropped.clone()) {
        let _ = playout.stop();
        return Err(e);
    }
    let stats = Arc::new(Mutex::new(WriterStats::default()));
    let (control_tx, control_rx) = mpsc::channel(CONTROL_QUEUE);
    let (media_tx, media_rx) = mpsc::channel(MEDIA_QUEUE);
    let mut writer_task = tokio::spawn(writer(write, stamper, control_rx, media_rx, stats.clone(), notes.clone(), urgent.mute.shared.clone()));
    let out = Outbox { control: control_tx, media: media_tx, stats: stats.clone() };

    let mut vad = Vad::new(opts.vad);
    let mut echo = EchoMonitor::new(opts.echo);

    let mut talking = opts.talk == TalkMode::Open;

    let fence = &urgent.mute;
    let (mut applied_gen, mut muted, mut call_closed, mut output_epoch) = fence.loop_started();
    let mut last_onset: Option<Instant> = None;
    let mut mic = Mic::new(&opts.stream_prefix, opts.chunk_ms);
    mic.generation = applied_gen;
    let mut open: Vec<String> = Vec::new();
    let mut onsets = 0u64;
    let mut buf = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(10));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut text_n = 0u64;

    macro_rules! user_cancel {
        () => {{
            let trigger = Instant::now();
            let targets = union(&owner.live(), &open);
            let cleared = owner.clear(ClearReason::UserCancel, &targets);
            for r in &open {
                out.control(Event::Cancel { response_id: r.clone() });
            }
            if let Some(cleared) = cleared {
                let _ = notes.send(Note::Cleared { t: Instant::now(), trigger, cleared, cancelled: open.clone() });
            }
        }};
    }

    macro_rules! local_clear {
        ($reason:expr, $trigger:expr) => {{
            let targets = union(&owner.live(), &open);
            let cleared = owner.clear($reason, &targets);
            for r in &open {
                out.control(Event::Cancel { response_id: r.clone() });
            }
            if let Some(cleared) = cleared {
                let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: $trigger, cleared, cancelled: open.clone() });
            }
        }};
    }

    macro_rules! apply_fence {
        () => {{
            if let Some((generation, m, c, epoch)) = fence.pending(applied_gen) {
                let (discarded, ended) = mic.fence(&out);

                let output_owed = epoch != output_epoch;
                if output_owed {
                    user_cancel!();
                }
                (applied_gen, muted, call_closed, output_epoch, mic.generation) = (generation, m, c, epoch, generation);
                let in_flight = fence.applied(generation, m, c, epoch, discarded);
                let _ = notes.send(Note::Mute { t: Instant::now(), muted: m, call_closed: c, call_epoch: epoch, output_closed: output_owed, generation, stream_ended: ended, discarded_samples: discarded, in_flight });
            }
        }};
    }

    let result: Result<()> = loop {
        tokio::select! {
            biased;
            _ = fence.changed.notified() => apply_fence!(),
            _ = urgent.notified() => {
                let intent = urgent.take();
                if intent & Urgent::QUIT != 0 {
                    break Ok(());
                }
                if intent & Urgent::STOP != 0 {
                    user_cancel!();
                }
            }
            c = ctl.recv() => match c {
                None | Some(Control::Quit) => break Ok(()),
                Some(Control::Cancel) => user_cancel!(),
                Some(Control::EndStream) => mic.end(&out),
                Some(Control::Talk(down)) => {
                    let t = Instant::now();
                    let applied = opts.talk == TalkMode::PushToTalk && down != talking;
                    if applied {
                        talking = down;
                        if down {

                            if owner.audible() || !open.is_empty() {
                                local_clear!(ClearReason::PushToTalk, t);
                            }
                        } else {
                            mic.end(&out);
                        }
                    }
                    let _ = notes.send(Note::Talk { t, down, applied });
                }
                Some(Control::Submit { command_id, session_id, expected_generation, text }) => {
                    out.control(Event::CommandSubmitted { command_id, session_id, expected_generation, text, speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
                }
                Some(Control::Send(ev)) => out.control(ev),
                Some(Control::Text(text)) => {
                    text_n += 1;
                    out.control(Event::UserText { request_id: format!("{}-t{text_n}", opts.stream_prefix), text, speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
                }
            },
            Some(frame) = mic_rx.recv() => {

                apply_fence!();

                echo.push_render(&queue.drain_tap());
                let gate = echo.push_mic(frame.at, &frame.samples);
                for ev in vad.push(&frame.samples) {
                    let t = Instant::now();
                    match ev {
                        VadEvent::SpeechStart { start_sample, detected_sample } => {
                            onsets += 1;

                            if !muted && !call_closed {
                                last_onset = Some(frame.at);
                            }
                            let audible = owner.audible() || !open.is_empty();
                            let action = if !audible {
                                "nothing_audible"
                            } else if muted {
                                "muted"
                            } else if call_closed {
                                "no_call"
                            } else if !talking {
                                "talk_released"
                            } else if opts.barge_in == BargeIn::Local && opts.talk == TalkMode::Open {
                                "cleared"
                            } else {
                                "observed"
                            };
                            let (level_db, echo_db) = (vad.last_dbfs(), echo.echo_db(frame.at));
                            let _ = notes.send(Note::Onset { t, stream_id: mic.chunker.stream_id.clone(), start_sample, detected_sample, audible, action, level_db, echo_db });
                            if action == "cleared" {
                                local_clear!(ClearReason::LocalVad, t);
                            }
                        }
                        VadEvent::SpeechEnd { end_sample } => {
                            let _ = notes.send(Note::SpeechEnd { t, end_sample });
                        }
                    }
                }
                for near in gate {
                    let t = Instant::now();
                    let audible = owner.audible() || !open.is_empty();
                    let action = if !audible {
                        "nothing_audible"
                    } else if muted {
                        "muted"
                    } else if call_closed {
                        "no_call"
                    } else if !talking {
                        "talk_released"
                    } else if opts.barge_in == BargeIn::Gated && opts.talk == TalkMode::Open {
                        "cleared"
                    } else {
                        "observed"
                    };
                    let _ = notes.send(Note::NearEnd { t, near, audible, action });
                    if action == "cleared" {
                        local_clear!(ClearReason::LocalGate, t);
                    }
                }

                if talking && !muted && !call_closed && fence.admits(frame.at, frame.samples.len()) {
                    mic.push(&frame.samples, &out, applied_gen);
                }
            }
            frame = read_frame(reader, &mut buf) => {
                let line = match frame {
                    Ok(l) => l,
                    Err(e) => break Err(anyhow!("link: {e}")),
                };
                let t = Instant::now();

                let env = match Envelope::parse_known(&line, Source::Guest) {
                    Ok(Some(env)) => env,
                    Ok(None) => {
                        eprintln!("rth: skipped an agent event of unknown kind: {}", skipped_kind(&line));
                        continue;
                    }
                    Err(e) => break Err(anyhow!("bad agent frame: {e}")),
                };
                let tail_flush = opts.barge_in != BargeIn::Off;
                match &env.event {
                    Event::ResponseStarted { response_id, .. } => open.push(response_id.clone()),

                    Event::AgentAudio { response_id, .. } if tail_flush && open.contains(response_id) && owner.epoch_of(response_id).is_none() => {
                        let tails: Vec<String> = owner.tails().into_iter().filter(|r| r != response_id).collect();
                        if !tails.is_empty()
                            && let Some(cleared) = owner.clear(ClearReason::Superseded, &tails)
                        {
                            let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: t, cleared, cancelled: Vec::new() });
                        }
                    }
                    Event::AgentDone { response_id, .. } => open.retain(|r| r != response_id),

                    Event::ProviderSignal { signal, .. } if signal == "voice_activity_start" && tail_flush && talking && !muted && !call_closed && open.is_empty() => {
                        let tails = owner.tails();
                        if !tails.is_empty()
                            && let Some(cleared) = owner.clear(ClearReason::ProviderTail, &tails)
                        {
                            let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: t, cleared, cancelled: Vec::new() });
                        }
                    }

                    Event::InputTranscript { .. } if tail_flush && talking && !muted && !call_closed && open.is_empty() => {

                        let tails = owner.tails();
                        let during = |r: &String| match (owner.first_rendered(r), last_onset) {
                            (Some(first), Some(onset)) => onset >= first && t.duration_since(onset) < Duration::from_secs(4),
                            _ => false,
                        };
                        let heard: Vec<String> = tails.into_iter().filter(during).collect();
                        if !heard.is_empty()
                            && let Some(cleared) = owner.clear(ClearReason::ProviderTail, &heard)
                        {
                            let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: last_onset.unwrap_or(t), cleared, cancelled: Vec::new() });
                        }
                    }
                    _ => {}
                }

                let refused = match &env.event {
                    Event::AgentAudio { response_id, sample_rate, data, .. } if playout.unavailable().is_some() => Some(owner.refuse(response_id, *sample_rate, data)),
                    _ => None,
                };
                match refused {
                    Some(Some(cleared)) => {
                        let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: t, cleared, cancelled: Vec::new() });
                    }
                    Some(None) => {}
                    None => {
                        if let Some(cleared) = owner.on_envelope(&env) {
                            let _ = notes.send(Note::Cleared { t: Instant::now(), trigger: t, cleared, cancelled: Vec::new() });
                        }
                    }
                }
                if !matches!(env.event, Event::Heartbeat { .. }) {
                    let _ = notes.send(Note::In { t, env });
                }
            }
            _ = tick.tick() => {
                echo.push_render(&queue.drain_tap());
                for e in owner.poll() {
                    if let Event::Playback { response_id, epoch, phase, played_samples, sample_rate, .. } = &e
                        && matches!(phase, rth_protocol::PlaybackPhase::Finished | rth_protocol::PlaybackPhase::Cleared)
                    {
                        let probe = queue.rendered(*epoch).first_at.and_then(|first| echo.probe(first, Instant::now()));
                        let _ = notes.send(Note::EchoProbe {
                            response_id: response_id.clone(),
                            epoch: *epoch,
                            phase: if *phase == rth_protocol::PlaybackPhase::Finished { "finished" } else { "cleared" },
                            rendered_ms: *played_samples as f64 * 1e3 / (*sample_rate).max(1) as f64,
                            probe,
                        });
                    }
                    out.control(e);
                }
            }
            r = &mut writer_task => break Err(match r {
                Ok(Ok(())) => anyhow!("link writer ended"),
                Ok(Err(e)) => anyhow!("link write: {e}"),
                Err(e) => anyhow!("link writer: {e}"),
            }),
        }
    };

    let shutdown = Instant::now();
    capture.stop();
    let playout_line = playout.stop().unwrap_or_else(|e| format!("playout stop: {e}"));
    let _ = queue.flush_through(u64::MAX - 1);
    let devices_stopped_after = shutdown.elapsed();

    if result.is_ok() {
        mic.end(&out);
    }
    drop(out);
    let (aborted, settled) = if writer_task.is_finished() {
        (false, true)
    } else {
        match tokio::time::timeout(SHUTDOWN_GRACE, &mut writer_task).await {
            Ok(_) => (false, true),
            Err(_) => {

                writer_task.abort();
                (true, tokio::time::timeout(ABORT_CONFIRM, &mut writer_task).await.is_ok())
            }
        }
    };

    if settled {
        fence.loop_ended();
    } else {
        fence.writer_abort_unconfirmed(writer_task);
    }
    let mut writer_stats = stats.lock().unwrap().clone();
    writer_stats.aborted = aborted;
    if !aborted {
        writer_stats.in_flight = None;
    }

    let in_flight = writer_stats.in_flight.clone();
    for s in writer_stats.streams.iter_mut() {
        if s.end == EndStatus::QueuedUnwritten && in_flight.as_deref() == Some(format!("user_audio_end {}", s.stream_id).as_str()) {
            s.end = EndStatus::InFlight;
        }
    }
    result?;
    Ok(VoiceSummary {
        owner: owner.stats,
        queue: queue.stats(),
        writer: writer_stats,
        mic_frames_dropped: dropped.load(std::sync::atomic::Ordering::Relaxed),
        onsets,
        capture: capture.describe(),
        playout: playout_line,
        devices_stopped_after,
    })
}

fn union(a: &[String], b: &[String]) -> Vec<String> {
    let mut v = a.to_vec();
    for x in b {
        if !v.contains(x) {
            v.push(x.clone());
        }
    }
    v
}

pub fn skipped_kind(line: &str) -> String {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(|k| k.chars().take(64).collect()))
        .unwrap_or_else(|| "?".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::FeedCapture;
    use base64::Engine;
    use rth_protocol::{MonotonicClock, PlaybackPhase, ResponseStatus};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncWriteExt, BufReader, DuplexStream, ReadHalf};

    #[derive(Clone, Default)]
    struct Hold {
        queue: Arc<Mutex<Option<Arc<PlayQueue>>>>,
        stopped: Arc<Mutex<Option<Instant>>>,
    }
    impl Hold {
        fn queued(&self) -> usize {
            self.queue.lock().unwrap().as_ref().map_or(0, |q| q.stats().queued)
        }
    }
    impl Playout for Hold {
        fn rate(&self) -> u32 {
            24_000
        }
        fn start(&mut self, q: Arc<PlayQueue>) -> Result<()> {
            *self.queue.lock().unwrap() = Some(q);
            Ok(())
        }
        fn stop(&mut self) -> Result<String> {
            *self.stopped.lock().unwrap() = Some(Instant::now());
            Ok("hold".into())
        }
        fn describe(&self) -> String {
            "hold".into()
        }
    }

    struct Stalled;
    impl AsyncWrite for Stalled {
        fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, _: &[u8]) -> Poll<std::io::Result<usize>> {
            Poll::Pending
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }

    struct Agent {
        g: Stamper,
        w: tokio::io::WriteHalf<DuplexStream>,
    }
    impl Agent {
        async fn speak(&mut self, rid: &str, chunks: u64) {
            let mut lines = self.g.stamp(Event::ResponseStarted { response_id: rid.into(), request_id: "mic-1".into(), utterance_id: None }, None).to_line();
            for c in 0..chunks {
                let data = base64::engine::general_purpose::STANDARD.encode(vec![0x20u8; 9600]);
                let ev = Event::AgentAudio { response_id: rid.into(), chunk: c, sample_offset: c * 4800, sample_rate: 24_000, format: "s16le".into(), data };
                lines.push_str(&self.g.stamp(ev, None).to_line());
            }
            self.w.write_all(lines.as_bytes()).await.unwrap();
        }
        async fn send(&mut self, ev: Event) {
            self.w.write_all(self.g.stamp(ev, None).to_line().as_bytes()).await.unwrap();
        }
    }

    fn legacy() -> VoiceOptions {
        VoiceOptions { barge_in: BargeIn::Local, ..Default::default() }
    }

    type Rig = (BufReader<ReadHalf<DuplexStream>>, Agent, BufReader<ReadHalf<DuplexStream>>, tokio::io::WriteHalf<DuplexStream>);

    fn rig() -> Rig {
        let (host_side, agent_side) = tokio::io::duplex(1 << 20);
        let (hr, hw) = tokio::io::split(host_side);
        let (ar, aw) = tokio::io::split(agent_side);
        let agent = Agent { g: Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start())), w: aw };
        (BufReader::new(hr), agent, BufReader::new(ar), hw)
    }

    async fn audio_in(notes: &mut mpsc::UnboundedReceiver<Note>, n: usize) {
        let mut seen = 0;
        while seen < n {
            if let Some(Note::In { env: Envelope { event: Event::AgentAudio { .. }, .. }, .. }) = notes.recv().await {
                seen += 1;
            }
        }
    }

    async fn next_clear(notes: &mut mpsc::UnboundedReceiver<Note>) -> (Instant, Instant, Cleared, Vec<String>) {
        loop {
            if let Note::Cleared { t, trigger, cleared, cancelled } = notes.recv().await.expect("notes open") {
                return (t, trigger, cleared, cancelled);
            }
        }
    }

    #[tokio::test]
    async fn local_speech_onset_clears_the_speaker_and_cancels_the_open_response() {
        let (mut reader, mut agent, mut agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let feed = mic.clone();
        let hold = Hold::default();
        let mut out = hold.clone();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let driver = tokio::spawn(async move { drive(&mut reader, hw, stamper, &mut mic, &mut out, legacy(), &mut ctl_rx, notes_tx).await });

        agent.speak("v1", 10).await;
        audio_in(&mut notes, 10).await;
        assert_eq!(hold.queued(), 48_000);

        feed.say(&rth_voice::vad::tone(250.0, -12.0, 300, 16_000));
        let (t, trigger, cleared, cancelled) = next_clear(&mut notes).await;
        assert_eq!(cleared.reason, ClearReason::LocalVad);
        assert_eq!(cancelled, vec!["v1".to_string()]);
        assert_eq!(cleared.dropped_samples, 48_000, "the whole queued reply, none of it played");
        assert_eq!(hold.queued(), 0);
        eprintln!("local VAD onset → queue clear {:.1} µs", (t - trigger).as_secs_f64() * 1e6);

        let mut abuf = Vec::new();
        let (mut cancel, mut record) = (false, false);
        while !(cancel && record) {
            let line = read_frame(&mut agent_reader, &mut abuf).await.unwrap();
            match Envelope::parse_from(&line, Source::Host).unwrap().event {
                Event::Cancel { response_id } => cancel = response_id == "v1",
                Event::Playback { reason, phase, .. } => record = phase == PlaybackPhase::Cleared && reason.as_deref() == Some("local_vad"),
                Event::UserAudio { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }

        let late = Event::AgentAudio { response_id: "v1".into(), chunk: 10, sample_offset: 48_000, sample_rate: 24_000, format: "s16le".into(), data: "AAAA".into() };
        agent.send(late).await;
        agent.send(Event::AgentDone { response_id: "v1".into(), status: ResponseStatus::Cancelled }).await;
        loop {
            if let Some(Note::In { env: Envelope { event: Event::AgentDone { .. }, .. }, .. }) = notes.recv().await {
                break;
            }
        }
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = driver.await.unwrap().unwrap();
        assert_eq!(summary.owner.late_chunks_dropped, 1);
        assert_eq!(summary.queue.rendered_after_fence, 0);
        assert_eq!(summary.onsets, 1);
        let w = &summary.writer;
        assert!(!w.aborted && w.in_flight.is_none());
        assert_eq!((w.media.unresolved(), w.control.unresolved(), w.media.refused), (0, 0, 0));
        assert_eq!(w.streams.len(), 1);
        assert_eq!(w.streams[0].end, EndStatus::Written, "a healthy link gets the stream end");
        assert_eq!(w.streams[0].written.chunks, w.streams[0].chunks.accepted);
    }

    #[tokio::test]
    async fn an_unknown_agent_kind_is_skipped_but_a_wrong_sender_still_drops_the_link() {
        let (mut reader, mut agent, _agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let mut out = Hold::default();
        let (_ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let driver = tokio::spawn(async move { drive(&mut reader, hw, stamper, &mut mic, &mut out, legacy(), &mut ctl_rx, notes_tx).await });
        let unknown = r#"{"v":2,"id":"guest-t:900","producer":"guest-t","seq":900,"t_ns":1,"source":"guest","kind":"from_the_future","x":1}"#;
        agent.w.write_all(format!("{unknown}\n").as_bytes()).await.unwrap();
        agent.send(Event::Log { level: "info".into(), msg: "after".into() }).await;
        loop {
            if let Some(Note::In { env: Envelope { event: Event::Log { msg, .. }, .. }, .. }) = notes.recv().await
                && msg == "after"
            {
                break;
            }
        }
        assert!(!driver.is_finished(), "the link survived the unknown kind");
        let spoofed = r#"{"v":2,"id":"guest-t:901","producer":"guest-t","seq":901,"t_ns":1,"source":"host","kind":"from_the_future"}"#;
        agent.w.write_all(format!("{spoofed}\n").as_bytes()).await.unwrap();
        let Err(err) = tokio::time::timeout(Duration::from_secs(2), driver).await.expect("link ends").unwrap() else { panic!("the spoofed frame was accepted") };
        assert!(err.to_string().contains("bad agent frame"), "{err}");
    }

    #[tokio::test]
    async fn urgent_stop_and_quit_act_with_the_control_channel_full_and_the_link_wedged() {
        let (mut reader, mut agent, _agent_reader, _hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let hold = Hold::default();
        let mut out = hold.clone();
        let (full_tx, mut ctl_rx) = mpsc::channel::<Control>(1);
        full_tx.try_send(Control::Text("queued".into())).unwrap();

        let (_idle_tx, mut idle_rx) = mpsc::channel::<Control>(1);
        assert!(full_tx.try_send(Control::Cancel).is_err(), "the ordinary channel is full");
        let urgent = Arc::new(Urgent::default());
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let u = urgent.clone();
        let driver = tokio::spawn(async move {
            drive_with(&mut reader, Stalled, stamper, &mut mic, &mut out, legacy(), &mut idle_rx, notes_tx, &u).await
        });
        agent.speak("v1", 10).await;
        audio_in(&mut notes, 10).await;
        assert_eq!(hold.queued(), 48_000);

        let pressed = Instant::now();
        urgent.stop();
        urgent.stop();
        let (t, _, cleared, cancelled) = tokio::time::timeout(Duration::from_secs(1), next_clear(&mut notes)).await.expect("urgent stop clears");
        assert_eq!((cleared.reason, hold.queued()), (ClearReason::UserCancel, 0));
        assert!(cancelled.contains(&"v1".to_string()));
        assert!(t - pressed < Duration::from_millis(50), "urgent clear took {:?}", t - pressed);

        let quit = Instant::now();
        urgent.quit();
        let summary = tokio::time::timeout(Duration::from_secs(2), driver).await.expect("quit returns").unwrap().unwrap();
        let stopped = hold.stopped.lock().unwrap().expect("speaker stopped");
        assert!(stopped - quit < Duration::from_millis(50), "speaker stop took {:?}", stopped - quit);
        assert!(quit.elapsed() < SHUTDOWN_GRACE + Duration::from_millis(200));
        assert_eq!(summary.owner.clears, 1, "two stops, one clear");
        drop(ctl_rx.try_recv());
    }

    #[tokio::test]
    async fn a_stalled_link_never_delays_local_stop_cancel_or_quit() {
        let (mut reader, mut agent, _agent_reader, _hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let feed = mic.clone();
        let hold = Hold::default();
        let mut out = hold.clone();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let driver = tokio::spawn(async move { drive(&mut reader, Stalled, stamper, &mut mic, &mut out, legacy(), &mut ctl_rx, notes_tx).await });

        ctl_tx.send(Control::Text("hello?".into())).await.unwrap();
        agent.speak("v1", 10).await;
        audio_in(&mut notes, 10).await;
        assert_eq!(hold.queued(), 48_000);

        let said = Instant::now();
        feed.say(&rth_voice::vad::tone(250.0, -12.0, 300, 16_000));
        let (_, _, cleared, _) = tokio::time::timeout(Duration::from_secs(2), next_clear(&mut notes)).await.expect("onset clear despite a stalled link");
        assert_eq!((cleared.reason, hold.queued()), (ClearReason::LocalVad, 0));
        assert!(said.elapsed() < Duration::from_millis(500), "onset clear took {:?}", said.elapsed());

        agent.speak("v2", 10).await;
        audio_in(&mut notes, 10).await;
        assert_eq!(hold.queued(), 48_000);
        let pressed = Instant::now();
        ctl_tx.send(Control::Cancel).await.unwrap();
        let (t, _, cleared, cancelled) = tokio::time::timeout(Duration::from_secs(1), next_clear(&mut notes)).await.expect("cancel clear despite a stalled link");
        assert_eq!((cleared.reason, hold.queued()), (ClearReason::UserCancel, 0));
        assert!(cancelled.contains(&"v2".to_string()));
        assert!(t - pressed < Duration::from_millis(50), "cancel clear took {:?}", t - pressed);

        let quit = Instant::now();
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = tokio::time::timeout(Duration::from_secs(2), driver).await.expect("quit returns").unwrap().unwrap();
        let stopped = hold.stopped.lock().unwrap().expect("speaker stopped");
        eprintln!("stalled link: quit → speaker stopped {:.2} ms, drive returned {:.0} ms", (stopped - quit).as_secs_f64() * 1e3, quit.elapsed().as_secs_f64() * 1e3);
        assert!(stopped - quit < Duration::from_millis(50), "speaker stop waited on the network: {:?}", stopped - quit);
        assert!(summary.devices_stopped_after < Duration::from_millis(50));
        assert!(quit.elapsed() < SHUTDOWN_GRACE + Duration::from_millis(200));
        let w = &summary.writer;
        assert!(w.aborted);
        assert_eq!(w.in_flight.as_deref(), Some("user_text mic-t1"), "the wedged write is uncertain, not confirmed");
        assert_eq!((w.control.written, w.media.written), (0, 0), "nothing reached the socket");
        assert!(w.media.unresolved() > 0 && w.control.unresolved() > 0, "accepted but unwritten work is reported");
        assert_eq!(w.streams.len(), 1, "a stream with no completed write is still listed");
        assert_eq!(w.streams[0].written.chunks, 0);
        assert!(matches!(w.streams[0].end, EndStatus::QueuedUnwritten | EndStatus::Refused), "{:?}", w.streams[0].end);
        assert_eq!(summary.queue.queued, 0);
    }

    #[test]
    fn media_past_the_bound_is_dropped_and_counted_while_control_has_its_own_room() {
        let (control, _crx) = mpsc::channel(4);
        let (media, _mrx) = mpsc::channel(2);
        let out = Outbox { control, media, stats: Arc::new(Mutex::new(WriterStats::default())) };
        let mut mic = Mic::new("m", 40);
        mic.push(&vec![1i16; 640 * 5], &out, 0);
        out.control(Event::Cancel { response_id: "r".into() });
        let st = out.stats.lock().unwrap().clone();
        assert_eq!((st.media.accepted, st.media.refused, st.control.accepted, st.control.refused, mic.dropped), (2, 3, 1, 0, 3));
        assert_eq!((st.streams[0].chunks.accepted, st.streams[0].chunks.refused), (2, 3));
    }

    struct Gate(Arc<std::sync::atomic::AtomicBool>);
    impl AsyncWrite for Gate {
        fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, b: &[u8]) -> Poll<std::io::Result<usize>> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) { Poll::Ready(Ok(b.len())) } else { Poll::Pending }
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn per_stream_outcomes_survive_a_link_that_wedges_between_streams() {
        let (mut reader, _agent, _agent_reader, _hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let mut out = Hold::default();
        let open = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let gate = Gate(open.clone());
        let driver = tokio::spawn(async move { drive(&mut reader, gate, stamper, &mut mic, &mut out, legacy(), &mut ctl_rx, notes_tx).await });

        let mut chunks = 0;
        while chunks < 3 {
            if let Some(Note::Out { env: Envelope { event: Event::UserAudio { .. }, .. }, .. }) = notes.recv().await {
                chunks += 1;
            }
        }
        ctl_tx.send(Control::EndStream).await.unwrap();
        loop {
            if let Some(Note::Out { env: Envelope { event: Event::UserAudioEnd { stream_id, .. }, .. }, .. }) = notes.recv().await {
                assert_eq!(stream_id, "mic-1");
                break;
            }
        }

        open.store(false, std::sync::atomic::Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(300)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = tokio::time::timeout(Duration::from_secs(2), driver).await.unwrap().unwrap().unwrap();
        let w = &summary.writer;
        assert!(w.aborted);
        assert!(w.in_flight.as_deref().is_some_and(|f| f.starts_with("user_audio mic-2")), "{:?}", w.in_flight);
        let ids: Vec<&str> = w.streams.iter().map(|s| s.stream_id.as_str()).collect();
        assert_eq!(ids, vec!["mic-1", "mic-2"], "every stream is listed");
        let (s1, s2) = (&w.streams[0], &w.streams[1]);
        assert_eq!(s1.end, EndStatus::Written);
        assert_eq!(s1.written.chunks, s1.chunks.accepted, "stream 1 fully written");
        assert_eq!(s2.end, EndStatus::QueuedUnwritten, "the final end is not hidden by stream 1's");
        assert_eq!(s2.written.chunks, 0);
        assert!(s2.chunks.accepted >= 5, "accepted-but-unwritten media is reported: {}", s2.chunks.accepted);
        assert_eq!(w.media.unresolved(), s2.chunks.accepted + 1, "stream 2's chunks and its end marker");
    }

    fn fixture(name: &str, rate: u32, dbfs: f32) -> Vec<i16> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/fixtures/dev/voice-trial-fixes");
        let w = rth_voice::wav::read(&dir.join(format!("{name}.wav"))).expect("sh bench/fixtures/dev/voice-trial-fixes/make.sh");
        let v = rth_voice::sim::normalize(&w.to_i16_at(16_000), dbfs);
        if rate == 16_000 {
            return v;
        }
        let mut out = Vec::new();
        rth_voice::resample::Resampler::new(16_000, rate).process_i16(&v, &mut out);
        out
    }

    impl Agent {

        async fn reply(&mut self, rid: &str, pcm: &[i16], ms: usize, done: bool) {
            let mut lines = self.g.stamp(Event::ResponseStarted { response_id: rid.into(), request_id: "mic-1".into(), utterance_id: None }, None).to_line();
            for (c, chunk) in pcm[..(24 * ms).min(pcm.len())].chunks(4800).enumerate() {
                let data = base64::engine::general_purpose::STANDARD.encode(rth_voice::le_from_samples(chunk));
                let ev = Event::AgentAudio { response_id: rid.into(), chunk: c as u64, sample_offset: c as u64 * 4800, sample_rate: 24_000, format: "s16le".into(), data };
                lines.push_str(&self.g.stamp(ev, None).to_line());
            }
            if done {
                lines.push_str(&self.g.stamp(Event::AgentDone { response_id: rid.into(), status: ResponseStatus::Completed }, None).to_line());
            }
            self.w.write_all(lines.as_bytes()).await.unwrap();
        }
    }

    #[derive(Debug, PartialEq)]
    enum Fate {
        Cleared(ClearReason, u64),
        Finished(u64),
    }

    async fn fate(notes: &mut mpsc::UnboundedReceiver<Note>, rid: &str, onsets: &mut Vec<(&'static str, f32)>) -> (Fate, Instant) {
        let mut reason = None;
        loop {
            match notes.recv().await.expect("notes") {
                Note::Cleared { cleared, .. } if cleared.responses.iter().any(|(r, _, _)| r == rid) => reason = Some(cleared.reason),
                Note::Onset { action, level_db, .. } => onsets.push((action, level_db)),
                Note::Out { t, env: Envelope { event: Event::Playback { response_id, phase, played_samples, .. }, .. } } if response_id == rid => match phase {
                    PlaybackPhase::Finished => return (Fate::Finished(played_samples), t),
                    PlaybackPhase::Cleared => return (Fate::Cleared(reason.expect("a clear note first"), played_samples), t),
                    PlaybackPhase::Started => {}
                },
                _ => {}
            }
        }
    }

    async fn drain_agent(mut r: BufReader<ReadHalf<DuplexStream>>) -> Vec<Event> {
        let mut buf = Vec::new();
        let mut got = Vec::new();
        while let Ok(line) = read_frame(&mut r, &mut buf).await {
            if let Ok(env) = Envelope::parse_from(&line, Source::Host) {
                got.push(env.event);
            }
        }
        got
    }

    async fn run_room(opts: VoiceOptions, room: &crate::devices::EchoRoom, body: impl AsyncFnOnce(&mut Agent, &mut mpsc::UnboundedReceiver<Note>)) -> Vec<Event> {
        let (mut reader, mut agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let (mut mic, mut spk) = (room.mic(), room.speaker());
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let driver = tokio::spawn(async move { drive(&mut reader, hw, stamper, &mut mic, &mut spk, opts, &mut ctl_rx, notes_tx).await });
        body(&mut agent, &mut notes).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        wire.await.unwrap()
    }

    #[tokio::test]
    async fn the_echo_fixture_reproduces_the_self_barge_in_under_the_legacy_local_vad() {
        let room = crate::devices::EchoRoom::new(150, -6.0);
        let pcm = fixture("agent-reply", 24_000, -20.0);
        run_room(legacy(), &room, async |agent, notes| {
            agent.reply("v1", &pcm, 4000, false).await;
            let (f, _) = fate(notes, "v1", &mut Vec::new()).await;
            let Fate::Cleared(ClearReason::LocalVad, played) = f else { panic!("{f:?}") };
            eprintln!("echo fixture, legacy local VAD: self-cut after {played} samples played at 48 kHz ({:.0} ms)", played as f64 / 48.0);
            assert!(played < 48 * 700, "cut early, as in the trial (6,144-9,000 samples): {played}");
        })
        .await;
    }

    #[tokio::test]
    async fn under_provider_mode_the_same_echo_never_cuts_and_provider_interrupted_still_flushes() {
        let room = crate::devices::EchoRoom::new(150, -6.0);
        let pcm = fixture("agent-reply", 24_000, -20.0);
        let wire = run_room(VoiceOptions::default(), &room, async |agent, notes| {
            agent.reply("v1", &pcm, 2000, true).await;
            let mut onsets = Vec::new();
            let (f, _) = fate(notes, "v1", &mut onsets).await;
            assert!(matches!(f, Fate::Finished(p) if p >= 48 * 1990), "{f:?}");
            assert!(onsets.iter().any(|(a, _)| *a == "observed"), "the echo onset was seen and logged, not acted on: {onsets:?}");
            agent.reply("v2", &pcm, 3000, false).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            agent.send(Event::ProviderSignal { response_id: Some("v2".into()), signal: "interrupted".into() }).await;
            let (f, _) = fate(notes, "v2", &mut onsets).await;
            assert!(matches!(f, Fate::Cleared(ClearReason::ProviderInterrupted, _)), "{f:?}");
        })
        .await;
        assert!(!wire.iter().any(|e| matches!(e, Event::Cancel { .. })), "no local energy onset sent a Cancel");
    }

    #[tokio::test]
    async fn on_a_processed_microphone_the_reply_survives_its_echo_and_real_speech_cuts_it() {
        let room = crate::devices::EchoRoom::new(150, -6.0 - 25.0);
        let pcm = fixture("agent-reply", 24_000, -20.0);
        let barge = fixture("barge-short", 16_000, -25.0);
        run_room(legacy(), &room, async |agent, notes| {
            agent.reply("v1", &pcm, 3000, true).await;
            let mut onsets = Vec::new();
            let (f, _) = fate(notes, "v1", &mut onsets).await;
            assert!(matches!(f, Fate::Finished(p) if p >= 48 * 2990), "{f:?} {onsets:?}");
            assert!(onsets.is_empty(), "{onsets:?}");
            agent.reply("v2", &pcm, 6000, false).await;
            let lead = barge.iter().position(|v| v.unsigned_abs() > 300).unwrap_or(0) as f64 / 16.0;
            let at = room.origin().elapsed() + Duration::from_millis(1500);
            room.say_at(at, &barge);
            let onset = room.origin() + at + Duration::from_secs_f64(lead / 1e3);
            let (f, t) = fate(notes, "v2", &mut onsets).await;
            assert!(matches!(f, Fate::Cleared(ClearReason::LocalVad, _)), "{f:?}");
            let ms = t.duration_since(onset).as_secs_f64() * 1e3;
            eprintln!("processed mic, real speech over the reply: onset -> playback cleared record {ms:.0} ms");
            assert!(ms < 250.0, "{ms}");
        })
        .await;
    }

    #[tokio::test]
    async fn speech_during_a_finished_replys_tail_flushes_it_on_the_providers_transcript() {
        let room = crate::devices::EchoRoom::new(150, -6.0 - 25.0);
        let pcm = fixture("agent-reply", 24_000, -20.0);
        let barge = fixture("barge-long", 16_000, -25.0);
        run_room(VoiceOptions::default(), &room, async |agent, notes| {
            agent.reply("v1", &pcm, 8000, true).await;
            let at = room.origin().elapsed() + Duration::from_millis(1500);
            room.say_at(at, &barge);
            let mut onsets = Vec::new();

            let onset_t = loop {
                if let Note::Onset { action, t, .. } = notes.recv().await.unwrap() {
                    assert_eq!(action, "observed");
                    break t;
                }
            };
            tokio::time::sleep(Duration::from_millis(400)).await;
            agent.send(Event::InputTranscript { text: "não".into(), utterance_id: None, utterance_seq: None, is_final: None, response_id: None, association: Some("unknown".into()), boundary: None }).await;
            let (f, t) = fate(notes, "v1", &mut onsets).await;
            let Fate::Cleared(ClearReason::ProviderTail, played) = f else { panic!("{f:?}") };
            eprintln!("tail interruption: local onset -> tail flushed {:.0} ms (400 ms modelled provider delay); {:.1} s of 8 s played", t.duration_since(onset_t).as_secs_f64() * 1e3, played as f64 / 48_000.0);
            assert!(played < 48 * 4000);

            agent.reply("v1s", &pcm, 8000, true).await;
            let at = room.origin().elapsed() + Duration::from_millis(1500);
            room.say_at(at, &barge);
            let onset_s = loop {
                if let Note::Onset { t, .. } = notes.recv().await.unwrap() {
                    break t;
                }
            };
            tokio::time::sleep(Duration::from_millis(1500)).await;
            agent.send(Event::ProviderSignal { response_id: None, signal: "voice_activity_start".into() }).await;
            let (f, t) = fate(notes, "v1s", &mut onsets).await;
            assert!(matches!(f, Fate::Cleared(ClearReason::ProviderTail, _)), "{f:?}");
            eprintln!("tail interruption, slow network: local onset -> tail flushed {:.0} ms (1500 ms modelled provider delay; the flush itself is local)", t.duration_since(onset_s).as_secs_f64() * 1e3);

            agent.reply("v1b", &pcm, 6000, true).await;
            tokio::time::sleep(Duration::from_millis(400)).await;
            agent.send(Event::ProviderSignal { response_id: None, signal: "voice_activity_start".into() }).await;
            let (f, _) = fate(notes, "v1b", &mut onsets).await;
            assert!(matches!(f, Fate::Cleared(ClearReason::ProviderTail, _)), "{f:?}");

            agent.reply("v2", &pcm, 6000, true).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            agent.reply("vsilent", &pcm, 0, true).await;
            tokio::time::sleep(Duration::from_millis(300)).await;
            let before_v3 = Instant::now();
            agent.reply("v3", &pcm, 1000, true).await;
            let (f, t) = fate(notes, "v2", &mut onsets).await;
            assert!(matches!(f, Fate::Cleared(ClearReason::Superseded, _)), "{f:?}");
            assert!(t >= before_v3, "the silent turn did not cut v2; v3's audio did");
        })
        .await;
    }

    #[tokio::test]
    async fn push_to_talk_gates_the_provider_and_release_ends_the_stream() {
        let (mut reader, mut agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let feed = mic.clone();
        let hold = Hold::default();
        let mut out = hold.clone();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let opts = VoiceOptions { talk: TalkMode::PushToTalk, ..Default::default() };
        let driver = tokio::spawn(async move { drive(&mut reader, hw, stamper, &mut mic, &mut out, opts, &mut ctl_rx, notes_tx).await });

        agent.speak("v1", 10).await;
        audio_in(&mut notes, 10).await;
        feed.say(&fixture("s07-phone-during-reply", 16_000, -25.0));
        let mut released_onset = None;
        while released_onset.is_none() {
            if let Some(Note::Onset { action, .. }) = notes.recv().await {
                released_onset = Some(action);
            }
        }
        assert_eq!(released_onset, Some("talk_released"));
        assert_eq!(hold.queued(), 48_000, "side talk did not stop the agent");

        ctl_tx.send(Control::Talk(true)).await.unwrap();
        let (_, _, cleared, cancelled) = next_clear(&mut notes).await;
        assert_eq!((cleared.reason, cancelled), (ClearReason::PushToTalk, vec!["v1".to_string()]));
        feed.say(&fixture("s03-user-to-rth", 16_000, -25.0));
        tokio::time::sleep(Duration::from_millis(600)).await;
        ctl_tx.send(Control::Talk(false)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        let first_audio = wire.iter().position(|e| matches!(e, Event::UserAudio { .. })).expect("audio after the press");
        let cancel = wire.iter().position(|e| matches!(e, Event::Cancel { .. })).unwrap();
        assert!(cancel < first_audio, "no microphone audio reached the provider before the press");
        let end = wire.iter().position(|e| matches!(e, Event::UserAudioEnd { .. })).expect("release ends the stream");
        let audio: Vec<_> = wire.iter().filter_map(|e| if let Event::UserAudio { chunk, .. } = e { Some(*chunk) } else { None }).collect();
        assert!(audio.len() >= 10 && audio.len() <= 30, "only the held ~0.6-0.8 s: {} chunks", audio.len());
        assert!(wire[end + 1..].iter().all(|e| !matches!(e, Event::UserAudio { .. })), "nothing after release");
    }

    #[derive(Clone, Default)]
    struct Manual(Arc<Mutex<Option<crate::devices::MicFrames>>>);
    impl Capture for Manual {
        fn start(&mut self, frames: crate::devices::MicFrames, _dropped: Arc<std::sync::atomic::AtomicU64>) -> Result<()> {
            *self.0.lock().unwrap() = Some(frames);
            Ok(())
        }
        fn stop(&mut self) {
            self.0.lock().unwrap().take();
        }
        fn describe(&self) -> String {
            "manual".into()
        }
    }

    const BLOCK: Duration = Duration::from_millis(1000 * FeedCapture::FRAME as u64 / rth_protocol::MIC_SAMPLE_RATE as u64);

    impl Manual {

        async fn frames(&self, v: i16, n: usize, last: Instant) {
            let tx = self.0.lock().unwrap().clone().expect("capture started");
            for i in 0..n {
                let at = last - BLOCK * (n - 1 - i) as u32;
                tx.send(crate::devices::MicFrame { at, samples: vec![v; FeedCapture::FRAME] }).await.unwrap();
            }
        }

        async fn frames_after(&self, v: i16, n: usize, open: Instant) {
            self.frames(v, n, open + BLOCK * n as u32).await
        }
    }

    fn reopened(urgent: &Urgent) -> Instant {
        urgent.mute.snapshot().open_since.expect("an unmute or call open set the cutoff")
    }

    async fn within<T>(what: &str, f: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(5), f).await.unwrap_or_else(|_| panic!("timed out after 5 s waiting for {what}"))
    }

    fn written_pcm(wire: &[Event]) -> Vec<(String, u64, Vec<i16>)> {
        wire.iter()
            .filter_map(|e| match e {
                Event::UserAudio { stream_id, sample_offset, data, .. } => {
                    Some((stream_id.clone(), *sample_offset, rth_voice::samples_from_le(&base64::engine::general_purpose::STANDARD.decode(data).unwrap())))
                }
                _ => None,
            })
            .collect()
    }

    fn values(pcm: &[(String, u64, Vec<i16>)], stream: &str) -> std::collections::BTreeSet<i16> {
        pcm.iter().filter(|(s, _, _)| s == stream).flat_map(|(_, _, v)| v.iter().copied()).collect()
    }

    async fn mute_applied(notes: &mut mpsc::UnboundedReceiver<Note>, want: bool) -> (u64, bool, u64, Option<String>) {
        within("the mute fence to apply", async {
            loop {
                match notes.recv().await.expect("notes") {
                    Note::Mute { muted, generation, stream_ended, discarded_samples, in_flight, .. } if muted == want => return (generation, stream_ended, discarded_samples, in_flight),
                    Note::Cleared { .. } => panic!("the mute fence touched the speaker"),
                    _ => {}
                }
            }
        })
        .await
    }

    #[tokio::test]
    async fn mute_sends_nothing_never_cancels_playback_and_unmute_restores_input() {
        let (mut reader, mut agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut mic = FeedCapture::new();
        let feed = mic.clone();
        let hold = Hold::default();
        let mut out = hold.clone();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, legacy(), &mut ctl_rx, notes_tx, &u).await });
        within("open-mic audio before the mute", async {
            let mut chunks = 0;
            while chunks < 3 {
                if let Some(Note::Out { env: Envelope { event: Event::UserAudio { .. }, .. }, .. }) = notes.recv().await {
                    chunks += 1;
                }
            }
        })
        .await;
        agent.speak("v1", 10).await;
        within("the reply's audio", audio_in(&mut notes, 10)).await;
        assert_eq!(hold.queued(), 48_000);
        assert!(!urgent.mute.request(true));
        let (_, ended, _, _) = mute_applied(&mut notes, true).await;
        assert!(ended, "muting closed the stream that had sent audio");
        assert!(urgent.mute.snapshot().effective_muted());
        feed.say(&fixture("s07-phone-during-reply", 16_000, -25.0));
        within("the muted onset", async {
            loop {
                match notes.recv().await.expect("notes") {
                    Note::Onset { action, .. } => break assert_eq!(action, "muted"),
                    Note::Cleared { .. } => panic!("speech while muted cut the reply"),
                    Note::Out { env: Envelope { event: Event::UserAudio { .. }, .. }, .. } => panic!("audio written while muted"),
                    _ => {}
                }
            }
        })
        .await;
        assert_eq!(hold.queued(), 48_000, "the reply was not touched");
        assert!(urgent.mute.request(false));
        mute_applied(&mut notes, false).await;
        let restored = within("audio after the unmute", async {
            loop {
                match notes.recv().await.expect("notes") {
                    Note::Out { env: Envelope { event: Event::UserAudio { stream_id, .. }, .. }, .. } => break stream_id,
                    Note::Cleared { .. } => panic!("unmute cleared the speaker"),
                    _ => {}
                }
            }
        })
        .await;
        assert_eq!(restored, "mic-2");
        assert_eq!(hold.queued(), 48_000);
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        assert!(!wire.iter().any(|e| matches!(e, Event::Cancel { .. })), "mute cancelled nothing");
        assert!(!wire.iter().any(|e| matches!(e, Event::Playback { phase: PlaybackPhase::Cleared, .. })), "no playback was cleared");
    }

    #[tokio::test]
    async fn the_fence_revokes_a_blocked_writers_backlog() {

        let (host_side, agent_side) = tokio::io::duplex(4096);
        let (hr, hw) = tokio::io::split(host_side);
        let (ar, aw) = tokio::io::split(agent_side);
        let mut reader = BufReader::new(hr);
        let _agent_w = aw;
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cap.frames(1111, 40, Instant::now()).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        urgent.mute.request(true);
        let (_, ended, _, in_flight) = mute_applied(&mut notes, true).await;
        assert!(ended);
        assert!(in_flight.as_deref().is_some_and(|f| f.starts_with("user_audio mic-1")), "the blocked write is reported as handed off: {in_flight:?}");
        let wire = tokio::spawn(drain_agent(BufReader::new(ar)));
        urgent.mute.request(false);
        mute_applied(&mut notes, false).await;
        cap.frames_after(3333, 4, reopened(&urgent)).await;
        within("mic-2 audio after the unmute", async {
            loop {
                if let Some(Note::Out { env: Envelope { event: Event::UserAudio { stream_id, .. }, .. }, .. }) = notes.recv().await
                    && stream_id == "mic-2"
                {
                    break;
                }
            }
        })
        .await;
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = driver.await.unwrap().unwrap();
        drop(_agent_w);
        let wire = wire.await.unwrap();
        let pcm = written_pcm(&wire);
        let s1 = summary.writer.streams.iter().find(|s| s.stream_id == "mic-1").unwrap();
        let written1 = pcm.iter().filter(|(s, _, _)| s == "mic-1").count() as u64;
        assert_eq!(s1.chunks.accepted, 20);
        assert!(s1.revoked >= 15, "the backlog was revoked, not sent: {} revoked", s1.revoked);
        assert_eq!(written1 + s1.revoked, 20, "every pre-fence chunk is written (before or in flight at the fence) or revoked");
        assert!(written1 <= 3, "at most the two buffered and the one in flight: {written1}");
        assert_eq!(values(&pcm, "mic-1"), [1111].into(), "mic-1 holds only pre-mute capture");
        assert_eq!(values(&pcm, "mic-2"), [3333].into(), "mic-2 holds only post-unmute capture");
        let end = wire.iter().find_map(|e| match e { Event::UserAudioEnd { stream_id, chunks, dropped_chunks, .. } if stream_id == "mic-1" => Some((*chunks, *dropped_chunks)), _ => None });
        assert_eq!(end, Some((20, s1.revoked)), "the end marker counts revoked chunks as dropped");
        assert_eq!(summary.writer.media.unresolved(), 0);
        assert_eq!(urgent.mute.snapshot().revoked_chunks, s1.revoked);
    }

    #[tokio::test]
    async fn a_partial_chunk_never_survives_the_fence() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cap.frames(1111, 1, Instant::now()).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        urgent.mute.request(true);
        let (_, ended, discarded, _) = mute_applied(&mut notes, true).await;
        assert_eq!((ended, discarded), (false, FeedCapture::FRAME as u64), "nothing had been sent; the prefix was dropped");
        urgent.mute.request(false);
        mute_applied(&mut notes, false).await;
        cap.frames_after(3333, 2, reopened(&urgent)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let pcm = written_pcm(&wire.await.unwrap());
        assert_eq!(pcm.len(), 1, "{pcm:?}");
        assert_eq!((pcm[0].1, pcm[0].2.len()), (0, 640), "the first chunk starts the stream fresh");
        assert_eq!(values(&pcm, "mic-1"), [3333].into(), "no pre-mute sample merged into post-unmute input");
    }

    #[tokio::test]
    async fn capture_backlogged_across_the_fence_is_never_admitted() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cap.frames(1111, 4, Instant::now()).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let before_mute = Instant::now();
        urgent.mute.request(true);
        mute_applied(&mut notes, true).await;
        cap.frames(1112, 4, before_mute).await;
        let during_mute = Instant::now();
        urgent.mute.request(false);
        mute_applied(&mut notes, false).await;
        cap.frames(2222, 4, during_mute).await;
        cap.frames_after(3333, 4, reopened(&urgent)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let pcm = written_pcm(&wire.await.unwrap());
        assert_eq!(values(&pcm, "mic-1"), [1111].into());
        assert_eq!(values(&pcm, "mic-2"), [3333].into(), "only frames captured after the unmute");
        assert_eq!(pcm.iter().filter(|(s, _, _)| s == "mic-2").map(|(_, _, v)| v.len()).sum::<usize>(), 4 * FeedCapture::FRAME);
    }

    #[tokio::test]
    async fn a_block_captured_partly_or_wholly_while_muted_is_never_admitted_after_unmute() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        urgent.mute.request(true);
        mute_applied(&mut notes, true).await;
        urgent.mute.request(false);
        mute_applied(&mut notes, false).await;
        let open = reopened(&urgent);
        cap.frames(2221, 1, open).await;
        cap.frames(2222, 1, open + BLOCK / 4).await;
        cap.frames(3333, 1, open + BLOCK).await;
        cap.frames(3334, 1, open + BLOCK * 2).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let pcm = written_pcm(&wire.await.unwrap());
        let all: std::collections::BTreeSet<i16> = pcm.iter().flat_map(|(_, _, v)| v.iter().copied()).collect();
        assert_eq!(all, [3333, 3334].into(), "no sample captured while muted reaches the provider");
    }

    #[tokio::test]
    async fn residual_a_late_delivered_block_is_judged_by_its_delivery_stamp() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        urgent.mute.request(true);
        mute_applied(&mut notes, true).await;
        urgent.mute.request(false);
        mute_applied(&mut notes, false).await;
        let open = reopened(&urgent);

        cap.frames(4444, 1, open + BLOCK * 2).await;
        cap.frames(4445, 1, open + BLOCK * 3).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let pcm = written_pcm(&wire.await.unwrap());
        let all: std::collections::BTreeSet<i16> = pcm.iter().flat_map(|(_, _, v)| v.iter().copied()).collect();
        assert_eq!(all, [4444, 4445].into(), "the delivery stamp, not physical capture time, decides admission");
    }

    #[tokio::test]
    async fn the_fence_applies_with_no_capture_callback_after_the_request() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        cap.frames(1111, 3, Instant::now()).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        urgent.mute.request(true);
        let applied = tokio::time::timeout(Duration::from_secs(1), mute_applied(&mut notes, true)).await.expect("applied without a capture frame");
        assert_eq!((applied.1, applied.2), (true, FeedCapture::FRAME as u64));
        let f = urgent.mute.snapshot();
        assert!(f.effective_muted() && f.applied && f.applied_gen == f.generation && f.loop_running);
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        let pcm = written_pcm(&wire);
        assert_eq!(pcm.len(), 1);
        assert_eq!(values(&pcm, "mic-1"), [1111].into());
        let end = wire.iter().find_map(|e| match e { Event::UserAudioEnd { chunks, samples, dropped_chunks, .. } => Some((*chunks, *samples, *dropped_chunks)), _ => None });
        assert_eq!(end, Some((1, 640, 0)), "the prefix is not in the stream's counts");
        assert!(!urgent.mute.snapshot().loop_running);
    }

    #[derive(Default)]
    pub(super) struct WriterPause {
        point: Mutex<Option<&'static str>>,
        arrived: tokio::sync::Notify,
        release: tokio::sync::Notify,

        block: Option<Duration>,
    }
    impl WriterPause {
        pub(super) async fn at(&self, point: &'static str) {
            let hit = {
                let mut p = self.point.lock().unwrap();
                if *p == Some(point) { p.take().is_some() } else { false }
            };
            if hit {
                self.arrived.notify_one();
                match self.block {
                    Some(d) => std::thread::sleep(d),
                    None => self.release.notified().await,
                }
            }
        }
    }

    fn arm(urgent: &Urgent, point: &'static str) -> Arc<WriterPause> {
        let pause = Arc::new(WriterPause { point: Mutex::new(Some(point)), ..Default::default() });
        *urgent.mute.shared.pause.lock().unwrap() = Some(pause.clone());
        pause
    }

    #[allow(clippy::type_complexity)]
    fn race_rig(point: &'static str) -> (Manual, Arc<Urgent>, Arc<WriterPause>, mpsc::Sender<Control>, mpsc::UnboundedReceiver<Note>, tokio::task::JoinHandle<Result<VoiceSummary>>, tokio::task::JoinHandle<Vec<Event>>, Agent) {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let pause = arm(&urgent, point);
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        (cap, urgent, pause, ctl_tx, notes, driver, wire, agent)
    }

    #[tokio::test]
    async fn a_chunk_that_loses_the_race_to_the_fence_is_revoked_never_written() {
        let (cap, urgent, pause, ctl_tx, mut notes, driver, wire, agent) = race_rig("before_reserve");
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 2, Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified()).await.expect("writer reached the barrier");
        urgent.mute.request(true);
        let (_, ended, _, in_flight) = mute_applied(&mut notes, true).await;
        assert!(ended);
        assert_eq!(in_flight, None, "nothing was reserved when the fence was applied");
        assert!(urgent.mute.snapshot().effective_muted());
        pause.release.notify_one();
        tokio::time::sleep(Duration::from_millis(50)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        assert!(written_pcm(&wire).is_empty(), "no pre-fence PCM after the applied fence: {:?}", written_pcm(&wire).len());
        assert_eq!(summary.writer.media.revoked, 1);
        assert_eq!(urgent.mute.snapshot().revoked_chunks, 1);
        let end = wire.iter().find_map(|e| match e { Event::UserAudioEnd { chunks, dropped_chunks, .. } => Some((*chunks, *dropped_chunks)), _ => None });
        assert_eq!(end, Some((1, 1)), "the revoked chunk is counted as dropped");
    }

    #[tokio::test]
    async fn a_chunk_reserved_before_the_fence_is_reported_and_is_the_only_one_written() {
        let (cap, urgent, pause, ctl_tx, mut notes, driver, wire, agent) = race_rig("after_reserve");
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 4, Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified()).await.expect("writer reached the barrier");
        tokio::time::sleep(Duration::from_millis(20)).await;
        urgent.mute.request(true);
        let (_, _, _, in_flight) = mute_applied(&mut notes, true).await;
        assert_eq!(in_flight.as_deref(), Some("user_audio mic-1 #0"), "the fence names the prior reservation");
        assert_eq!(urgent.mute.snapshot().reserved.as_deref(), Some("user_audio mic-1 #0"));
        pause.release.notify_one();
        tokio::time::sleep(Duration::from_millis(50)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        let chunks: Vec<u64> = wire.iter().filter_map(|e| if let Event::UserAudio { chunk, .. } = e { Some(*chunk) } else { None }).collect();
        assert_eq!(chunks, vec![0], "only the reserved chunk");
        assert_eq!(values(&written_pcm(&wire), "mic-1"), [1111].into());
        assert_eq!(summary.writer.media.revoked, 1, "the queued chunk behind it was revoked");
        assert_eq!(urgent.mute.snapshot().reserved, None, "the reservation ended with its write");
    }

    #[tokio::test]
    async fn a_mute_during_shutdown_is_not_effective_until_the_writer_settles() {
        let (cap, urgent, pause, ctl_tx, _notes, driver, wire, agent) = race_rig("after_reserve");
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 2, Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified()).await.expect("writer reached the barrier");
        ctl_tx.send(Control::Quit).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        urgent.mute.request(true);
        let f = urgent.mute.snapshot();
        assert!(f.loop_running, "the writer has not settled: the loop still counts as running");
        assert_eq!(f.reserved.as_deref(), Some("user_audio mic-1 #0"));
        assert!(!f.effective_muted(), "requested, not effective: a reserved write is still live");
        pause.release.notify_one();
        let summary = driver.await.unwrap().unwrap();
        assert!(!summary.writer.aborted);
        let f = urgent.mute.snapshot();
        assert!(!f.loop_running && f.reserved.is_none() && f.effective_muted(), "{f:?}");
        drop(agent);
        let wire = wire.await.unwrap();
        assert_eq!(written_pcm(&wire).len(), 1, "the write reserved before the request completed, and nothing else");
    }

    #[tokio::test]
    async fn an_aborted_writer_is_awaited_before_the_loop_counts_as_ended() {
        let (cap, urgent, pause, ctl_tx, _notes, driver, wire, agent) = race_rig("after_reserve");
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 2, Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified()).await.expect("writer reached the barrier");
        let quit = Instant::now();
        ctl_tx.send(Control::Quit).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        urgent.mute.request(true);
        let f = urgent.mute.snapshot();
        assert!(f.loop_running && f.reserved.is_some() && !f.effective_muted(), "{f:?}");

        let summary = tokio::time::timeout(Duration::from_secs(2), driver).await.expect("drive returns").unwrap().unwrap();
        assert!(summary.writer.aborted, "the abort path was taken");
        assert!(quit.elapsed() >= SHUTDOWN_GRACE);
        let f = urgent.mute.snapshot();
        assert!(!f.loop_running && f.reserved.is_none() && f.effective_muted(), "ended only after the aborted writer finished: {f:?}");
        drop(agent);
        let wire = wire.await.unwrap();
        assert!(written_pcm(&wire).is_empty(), "the reserved write never completed");
    }

    #[tokio::test]
    async fn a_writer_blocked_mid_send_is_aborted_and_observed_before_the_loop_ends() {
        let (mut reader, agent, _agent_reader, _hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, _notes) = mpsc::unbounded_channel();
        let urgent = Arc::new(Urgent::default());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, Stalled, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 2, Instant::now()).await;

        let reserved = loop {
            if let Some(r) = urgent.mute.snapshot().reserved {
                break r;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(reserved, "user_audio mic-1 #0");
        ctl_tx.send(Control::Quit).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        urgent.mute.request(true);
        assert!(!urgent.mute.snapshot().effective_muted(), "a write is still in flight");
        let summary = tokio::time::timeout(Duration::from_secs(2), driver).await.unwrap().unwrap().unwrap();
        assert!(summary.writer.aborted);
        assert_eq!(summary.writer.in_flight.as_deref(), Some("user_audio mic-1 #0"));
        assert_eq!(summary.writer.media.written, 0, "no PCM reached the link");
        let f = urgent.mute.snapshot();
        assert!(!f.loop_running && !f.writer_abort_unconfirmed && f.effective_muted(), "{f:?}");
        drop(agent);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unobserved_abort_gates_reconnect_until_the_old_writer_is_seen_to_end() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, _notes) = mpsc::unbounded_channel();
        let old_wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let stuck = SHUTDOWN_GRACE + ABORT_CONFIRM + SETTLE_WAIT + Duration::from_millis(700);
        let pause = Arc::new(WriterPause { point: Mutex::new(Some("after_reserve")), block: Some(stuck), ..Default::default() });
        *urgent.mute.shared.pause.lock().unwrap() = Some(pause.clone());
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        cap.frames(1111, 2, Instant::now()).await;
        tokio::time::timeout(Duration::from_secs(2), pause.arrived.notified()).await.expect("writer reached the barrier");
        let stuck_since = Instant::now();
        ctl_tx.send(Control::Quit).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        urgent.mute.request(true);
        let summary = tokio::time::timeout(Duration::from_secs(3), driver).await.unwrap().unwrap().unwrap();
        assert!(summary.writer.aborted);
        let f = urgent.mute.snapshot();
        assert!(f.writer_abort_unconfirmed && f.loop_running && !f.effective_muted(), "{f:?}");

        let attach = |urgent: Arc<Urgent>| {
            let (mut reader, agent, agent_reader, hw) = rig();
            let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
            let cap = Manual::default();
            let mut mic = cap.clone();
            let (ctl_tx, mut ctl_rx) = mpsc::channel::<Control>(8);
            let (notes_tx, notes) = mpsc::unbounded_channel();
            let wire = tokio::spawn(drain_agent(agent_reader));
            let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut Hold::default(), VoiceOptions::default(), &mut ctl_rx, notes_tx, &urgent).await });
            (cap, ctl_tx, notes, driver, wire, agent)
        };
        let (cap2, _ctl2, _notes2, driver2, wire2, agent2) = attach(urgent.clone());
        let refused = tokio::time::timeout(Duration::from_secs(3), driver2).await.unwrap().unwrap();
        assert!(refused.as_ref().is_err_and(|e| e.to_string().contains("not been observed to finish")), "{:?}", refused.err());
        assert!(cap2.0.lock().unwrap().is_none(), "no capture was started for the refused attach");
        let f = urgent.mute.snapshot();
        assert!(f.writer_abort_unconfirmed && !f.effective_muted(), "still uncertain after a new attach: {f:?}");
        drop(agent2);
        assert!(written_pcm(&wire2.await.unwrap()).is_empty(), "the refused attach admitted no PCM");

        tokio::time::sleep((stuck_since + stuck).saturating_duration_since(Instant::now()) + Duration::from_millis(50)).await;
        let (cap3, ctl3, mut notes3, driver3, wire3, agent3) = attach(urgent.clone());
        tokio::time::sleep(Duration::from_millis(100)).await;
        let f = urgent.mute.snapshot();
        assert!(!f.writer_abort_unconfirmed && f.loop_running && f.effective_muted(), "settled, then fenced afresh: {f:?}");
        urgent.mute.request(false);
        mute_applied(&mut notes3, false).await;
        cap3.frames_after(3333, 2, reopened(&urgent)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl3.send(Control::Quit).await.unwrap();
        driver3.await.unwrap().unwrap();
        drop(agent3);
        assert_eq!(values(&written_pcm(&wire3.await.unwrap()), "mic-1"), [3333].into(), "admission proceeds after the old writer is seen to end");
        drop(agent);

        let late = written_pcm(&old_wire.await.unwrap());
        assert!(late.iter().all(|(_, _, v)| v.iter().all(|x| *x == 1111)), "only the chunk it had reserved");
    }

    #[tokio::test]
    async fn a_loop_started_muted_sends_no_microphone_audio() {
        let (mut reader, agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let cap = Manual::default();
        let mut mic = cap.clone();
        let mut out = Hold::default();
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, _notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        urgent.mute.request(true);
        let u = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut out, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(urgent.mute.snapshot().effective_muted());
        cap.frames(1111, 10, Instant::now()).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        assert!(!wire.iter().any(|e| matches!(e, Event::UserAudio { .. } | Event::UserAudioEnd { .. })), "{wire:?}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn with_the_device_unavailable_audio_is_refused_never_reported_as_played() {
        use crate::vpio::{VpioCapture, VpioOptions, VpioPlayout, VpioShared, tests::FakeUnit};
        let unit = FakeUnit::new(vec![Err("injected: VoiceProcessingIO unavailable".into())]);
        let shared = VpioShared::with_factory(VpioOptions::default(), unit, Duration::from_secs(1));
        let (mut reader, mut agent, agent_reader, hw) = rig();
        let stamper = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let (mut mic, mut spk) = (VpioCapture(shared.clone()), VpioPlayout(shared.clone()));
        let (ctl_tx, mut ctl_rx) = mpsc::channel(8);
        let (notes_tx, mut notes) = mpsc::unbounded_channel();
        let wire = tokio::spawn(drain_agent(agent_reader));
        let urgent = Arc::new(Urgent::default());
        let u2 = urgent.clone();
        let driver = tokio::spawn(async move { drive_with(&mut reader, hw, stamper, &mut mic, &mut spk, VoiceOptions::default(), &mut ctl_rx, notes_tx, &u2).await });
        let pcm = fixture("agent-reply", 24_000, -20.0);
        agent.reply("v1", &pcm, 3000, true).await;
        agent.send(Event::AgentDelta { response_id: "v1".into(), text: "texto".into() }).await;
        let mut delta = false;
        let mut cleared = None;
        while !(delta && cleared.is_some()) {
            match notes.recv().await.unwrap() {
                Note::In { env: Envelope { event: Event::AgentDelta { .. }, .. }, .. } => delta = true,
                Note::Cleared { cleared: c, .. } => cleared = Some(c),
                _ => {}
            }
        }
        let c = cleared.unwrap();
        assert_eq!(c.reason, ClearReason::DeviceUnavailable);
        assert_eq!(c.dropped_samples, 9_600, "the first 200 ms chunk at 48 kHz; later chunks are counted as late drops");
        tokio::time::sleep(Duration::from_millis(300)).await;
        urgent.stop();
        tokio::time::sleep(Duration::from_millis(50)).await;
        ctl_tx.send(Control::Quit).await.unwrap();
        let summary = driver.await.unwrap().unwrap();
        drop(agent);
        let wire = wire.await.unwrap();
        let records: Vec<_> = wire.iter().filter_map(|e| if let Event::Playback { phase, played_samples, reason, .. } = e { Some((*phase, *played_samples, reason.clone())) } else { None }).collect();
        assert_eq!(records, vec![(PlaybackPhase::Cleared, 0, Some("device_unavailable".to_string()))], "one terminal drop, no delivery claim");
        assert_eq!((summary.queue.queued, summary.queue.pulls, summary.queue.rendered_after_fence), (0, 0, 0), "nothing queued, nothing pulled");
        assert_eq!(summary.owner.late_chunks_dropped, 14, "the other 14 chunks of the 3 s reply");
        assert!(!wire.iter().any(|e| matches!(e, Event::Cancel { .. })), "the response's text is not cancelled");
        assert_eq!(shared.status()["phase"], "failed");
    }
}
