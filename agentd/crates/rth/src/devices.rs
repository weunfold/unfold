

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use rth_protocol::MIC_SAMPLE_RATE;
use rth_voice::queue::PlayQueue;
use rth_voice::resample::Resampler;
use tokio::sync::mpsc;

use crate::audio::Wav;

#[derive(Debug, Clone)]
pub struct MicFrame {
    pub at: Instant,
    pub samples: Vec<i16>,
}

pub type MicFrames = mpsc::Sender<MicFrame>;

pub trait Capture: Send {

    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()>;
    fn stop(&mut self);
    fn describe(&self) -> String;
}

pub trait Playout: Send {

    fn rate(&self) -> u32;
    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()>;

    fn stop(&mut self) -> Result<String>;
    fn describe(&self) -> String;

    fn unavailable(&self) -> Option<String> {
        None
    }
}

pub(crate) struct DeviceThread {
    stop: Option<std_mpsc::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl DeviceThread {
    pub(crate) fn spawn<F>(name: &str, body: F) -> Result<Self>
    where
        F: FnOnce(std_mpsc::Receiver<()>, std_mpsc::Sender<Result<()>>) + Send + 'static,
    {
        let (stop_tx, stop_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let join = std::thread::Builder::new().name(name.into()).spawn(move || body(stop_rx, ready_tx))?;
        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self { stop: Some(stop_tx), join: Some(join) }),
            Ok(Err(e)) => Err(e),
            Err(_) => {

                let _ = stop_tx.send(());
                let _ = std::thread::Builder::new().name(format!("{name}-reaper")).spawn(move || {
                    let _ = join.join();
                });
                bail!("{name}: device did not start within 10 s (stopped; it will not run late)")
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[derive(Clone)]
pub struct FeedCapture {
    script: Arc<Mutex<Script>>,
    thread: Arc<Mutex<Option<DeviceThread>>>,
}

#[derive(Default)]
struct Script {
    pending: VecDeque<i16>,

    emitted: u64,
}

impl Default for FeedCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedCapture {
    pub const FRAME: usize = (MIC_SAMPLE_RATE / 50) as usize;

    pub fn new() -> Self {
        Self { script: Arc::new(Mutex::new(Script::default())), thread: Arc::new(Mutex::new(None)) }
    }

    pub fn say(&self, samples: &[i16]) -> (u64, u64) {
        let mut s = self.script.lock().unwrap();
        let start = s.emitted + s.pending.len() as u64;
        s.pending.extend(samples);
        (start, start + samples.len() as u64)
    }

    pub fn pending(&self) -> usize {
        self.script.lock().unwrap().pending.len()
    }
}

impl Capture for FeedCapture {
    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
        let script = self.script.clone();
        let t = DeviceThread::spawn("rth-feed-mic", move |stop, ready| {
            let _ = ready.send(Ok(()));
            let period = Duration::from_millis(20);
            let mut next = Instant::now() + period;
            loop {
                match stop.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(()) | Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                }
                next += period;
                let frame: Vec<i16> = {
                    let mut s = script.lock().unwrap();
                    let n = s.pending.len().min(Self::FRAME);
                    let mut f: Vec<i16> = s.pending.drain(..n).collect();
                    f.resize(Self::FRAME, 0);
                    s.emitted += Self::FRAME as u64;
                    f
                };
                if frames.try_send(MicFrame { at: Instant::now(), samples: frame }).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        })?;
        *self.thread.lock().unwrap() = Some(t);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut t) = self.thread.lock().unwrap().take() {
            t.stop();
        }
    }

    fn describe(&self) -> String {
        "scripted microphone (16 kHz, 20 ms frames, real-time paced; silence between utterances)".into()
    }
}

pub struct WavSink {
    path: PathBuf,
    rate: u32,
    period: usize,
    thread: Option<DeviceThread>,
    written: Arc<AtomicU64>,
}

impl WavSink {
    pub fn new(path: PathBuf, rate: u32, period_ms: u32) -> Self {
        Self { path, rate, period: (rate * period_ms / 1000) as usize, thread: None, written: Arc::new(AtomicU64::new(0)) }
    }
}

impl Playout for WavSink {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        let (path, rate, period, written) = (self.path.clone(), self.rate, self.period, self.written.clone());
        self.thread = Some(DeviceThread::spawn("rth-wav-sink", move |stop, ready| {
            let mut wav = match Wav::create(&path, rate) {
                Ok(w) => w,
                Err(e) => {
                    let _ = ready.send(Err(anyhow!("wav sink {}: {e}", path.display())));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            let step = Duration::from_secs_f64(period as f64 / rate as f64);
            let mut next = Instant::now() + step;
            let mut buf = vec![0i16; period];
            loop {
                match stop.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(()) | Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                }
                next += step;
                queue.pull(&mut buf);
                let bytes: Vec<u8> = buf.iter().flat_map(|s| s.to_le_bytes()).collect();
                if wav.append(&bytes).is_ok() {
                    written.fetch_add(buf.len() as u64, Ordering::Relaxed);
                }
            }
            let _ = wav.finish(rate);
        })?);
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        if let Some(mut t) = self.thread.take() {
            t.stop();
        }
        let n = self.written.load(Ordering::Relaxed);
        Ok(format!("wav sink {} · {:.1} s rendered", self.path.display(), n as f64 / self.rate as f64))
    }

    fn describe(&self) -> String {
        format!("WAV sink {} ({} Hz, {} ms pulls, real-time paced)", self.path.display(), self.rate, self.period as u32 * 1000 / self.rate)
    }
}

#[derive(Clone)]
pub struct EchoRoom {
    base: Instant,
    delay: Duration,
    gain: f32,
    line: Arc<Mutex<EchoLine>>,
    mic_thread: Arc<Mutex<Option<DeviceThread>>>,
    spk_thread: Arc<Mutex<Option<DeviceThread>>>,
}

#[derive(Default)]
struct EchoLine {

    echo: Vec<f32>,
    script: VecDeque<i16>,
    emitted: u64,
}

impl EchoRoom {
    pub fn new(delay_ms: u32, gain_db: f32) -> Self {
        Self {
            base: Instant::now(),
            delay: Duration::from_millis(delay_ms as u64),
            gain: 10f32.powf(gain_db / 20.0),
            line: Arc::new(Mutex::new(EchoLine::default())),
            mic_thread: Arc::new(Mutex::new(None)),
            spk_thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn say(&self, samples: &[i16]) -> (u64, u64) {
        let mut l = self.line.lock().unwrap();
        let start = l.emitted + l.script.len() as u64;
        l.script.extend(samples);
        (start, start + samples.len() as u64)
    }

    pub fn say_at(&self, at: Duration, samples: &[i16]) -> (u64, u64) {
        let mut l = self.line.lock().unwrap();
        let target = (at.as_secs_f64() * MIC_SAMPLE_RATE as f64) as u64;
        let now = l.emitted + l.script.len() as u64;
        if target > now {
            l.script.extend(std::iter::repeat_n(0, (target - now) as usize));
        }
        let start = l.emitted + l.script.len() as u64;
        l.script.extend(samples);
        (start, start + samples.len() as u64)
    }

    pub fn origin(&self) -> Instant {
        self.base
    }

    pub fn mic(&self) -> EchoMic {
        EchoMic(self.clone())
    }

    pub fn speaker(&self) -> EchoSpeaker {
        EchoSpeaker(self.clone())
    }
}

pub struct EchoMic(EchoRoom);
pub struct EchoSpeaker(EchoRoom);

impl Capture for EchoMic {
    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
        let room = self.0.clone();
        let t = DeviceThread::spawn("rth-echo-mic", move |stop, ready| {
            let _ = ready.send(Ok(()));
            let mut k = room.line.lock().unwrap().emitted / FeedCapture::FRAME as u64;
            loop {
                let due = room.base + Duration::from_millis(20 * (k + 1));
                match stop.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(()) | Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                }
                let frame: Vec<i16> = {
                    let mut l = room.line.lock().unwrap();
                    let i0 = (k as usize) * FeedCapture::FRAME;
                    let frame = (0..FeedCapture::FRAME)
                        .map(|i| {
                            let near = l.script.pop_front().unwrap_or(0) as f32;
                            let echo = l.echo.get(i0 + i).copied().unwrap_or(0.0) * 32768.0;
                            (near + echo).round().clamp(-32768.0, 32767.0) as i16
                        })
                        .collect();
                    l.emitted += FeedCapture::FRAME as u64;
                    frame
                };
                k += 1;
                if frames.try_send(MicFrame { at: Instant::now(), samples: frame }).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        })?;
        *self.0.mic_thread.lock().unwrap() = Some(t);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut t) = self.0.mic_thread.lock().unwrap().take() {
            t.stop();
        }
    }

    fn describe(&self) -> String {
        format!("echo room microphone (echo after {} ms at {:.1} dB)", self.0.delay.as_millis(), 20.0 * self.0.gain.log10())
    }
}

impl Playout for EchoSpeaker {
    fn rate(&self) -> u32 {
        48_000
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        let room = self.0.clone();
        let t = DeviceThread::spawn("rth-echo-speaker", move |stop, ready| {
            let _ = ready.send(Ok(()));
            let mut buf = vec![0i16; 480];
            let mut next = Instant::now();
            loop {
                next += Duration::from_millis(10);
                match stop.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(()) | Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                }
                let at = Instant::now();
                queue.pull(&mut buf);
                let idx0 = ((at.duration_since(room.base) + room.delay).as_secs_f64() * MIC_SAMPLE_RATE as f64) as usize;
                let mut l = room.line.lock().unwrap();
                if l.echo.len() < idx0 + 160 {
                    l.echo.resize(idx0 + 160, 0.0);
                }
                for (i, tri) in buf.chunks(3).enumerate() {
                    let v = tri.iter().map(|&s| s as f32).sum::<f32>() / 3.0 / 32768.0;
                    l.echo[idx0 + i] += room.gain * v;
                }
            }
        })?;
        *self.0.spk_thread.lock().unwrap() = Some(t);
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        if let Some(mut t) = self.0.spk_thread.lock().unwrap().take() {
            t.stop();
        }
        Ok("echo room speaker".into())
    }

    fn describe(&self) -> String {
        "echo room speaker (48 kHz, 10 ms pulls)".into()
    }
}

#[derive(Default)]
pub struct NullCapture;

impl Capture for NullCapture {
    fn start(&mut self, _frames: MicFrames, _dropped: Arc<AtomicU64>) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) {}

    fn describe(&self) -> String {
        "no microphone (voice off)".into()
    }
}

pub struct NullPlayout {
    rate: u32,
    thread: Option<DeviceThread>,
}

impl Default for NullPlayout {
    fn default() -> Self {
        Self { rate: 24_000, thread: None }
    }
}

impl Playout for NullPlayout {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        let rate = self.rate;
        self.thread = Some(DeviceThread::spawn("rth-null-speaker", move |stop, ready| {
            let _ = ready.send(Ok(()));
            let period = (rate / 100) as usize;
            let step = Duration::from_millis(10);
            let mut next = Instant::now() + step;
            let mut buf = vec![0i16; period];
            loop {
                match stop.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(()) | Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                }
                next += step;
                queue.pull(&mut buf);
            }
        })?);
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        if let Some(mut t) = self.thread.take() {
            t.stop();
        }
        Ok("no speaker (voice off)".into())
    }

    fn describe(&self) -> String {
        "no speaker (voice off; the queue drains silently)".into()
    }
}

pub struct CpalCapture {
    thread: Option<DeviceThread>,
    desc: Arc<Mutex<String>>,
}

impl CpalCapture {

    pub fn new() -> Self {
        Self { thread: None, desc: Arc::new(Mutex::new("default input device (not started)".into())) }
    }
}

impl Default for CpalCapture {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct MicConvert {
    pub(crate) channels: usize,
    pub(crate) resampler: Resampler,
    pub(crate) mono: Vec<f32>,
    pub(crate) out: Vec<f32>,
    pub(crate) frames: MicFrames,
    pub(crate) dropped: Arc<AtomicU64>,
}

impl MicConvert {
    pub(crate) fn push(&mut self, interleaved: impl Iterator<Item = f32>) {
        self.mono.clear();
        let mut acc = 0.0;
        for (i, x) in interleaved.enumerate() {
            acc += x;
            if (i + 1) % self.channels == 0 {
                self.mono.push(acc / self.channels as f32);
                acc = 0.0;
            }
        }
        self.out.clear();
        self.resampler.process(&self.mono, &mut self.out);
        let frame: Vec<i16> = self.out.iter().map(|v| (v * 32767.0).round().clamp(-32768.0, 32767.0) as i16).collect();
        if !frame.is_empty() && self.frames.try_send(MicFrame { at: Instant::now(), samples: frame }).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Capture for CpalCapture {
    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let desc = self.desc.clone();
        self.thread = Some(DeviceThread::spawn("rth-cpal-mic", move |stop, ready| {
            let build = || -> Result<cpal::Stream> {
                let host = cpal::default_host();
                let device = host.default_input_device().context("no default input device")?;
                let supported = device.default_input_config().context("default input config")?;
                let config = supported.config();
                let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
                *desc.lock().unwrap() =
                    format!("mic {name}: {} Hz × {} ch {:?} → 16 kHz mono", config.sample_rate, config.channels, supported.sample_format());
                let mut conv = MicConvert {
                    channels: config.channels.max(1) as usize,
                    resampler: Resampler::new(config.sample_rate, MIC_SAMPLE_RATE),
                    mono: Vec::new(),
                    out: Vec::new(),
                    frames,
                    dropped,
                };
                let err = |e: cpal::Error| eprintln!("microphone stream error: {e}");
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::F32 => device.build_input_stream(config, move |d: &[f32], _: &_| conv.push(d.iter().copied()), err, None)?,
                    cpal::SampleFormat::I16 => device.build_input_stream(config, move |d: &[i16], _: &_| conv.push(d.iter().map(|&s| s as f32 / 32768.0)), err, None)?,
                    other => bail!("unsupported microphone sample format {other:?}"),
                };
                stream.play().context("starting the microphone")?;
                Ok(stream)
            };
            match build() {
                Ok(stream) => {
                    let _ = ready.send(Ok(()));
                    let _ = stop.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            }
        })?);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut t) = self.thread.take() {
            t.stop();
        }
    }

    fn describe(&self) -> String {
        self.desc.lock().unwrap().clone()
    }
}

pub struct CpalPlayout {
    rate: u32,
    channels: u16,
    name: String,
    thread: Option<DeviceThread>,

    period: Arc<AtomicU64>,
}

impl CpalPlayout {

    pub fn new() -> Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let device = host.default_output_device().context("no default output device")?;
        let config = device.default_output_config().context("default output config")?;
        let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
        Ok(Self { rate: config.sample_rate(), channels: config.channels(), name, thread: None, period: Arc::new(AtomicU64::new(0)) })
    }
}

impl Playout for CpalPlayout {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let period = self.period.clone();
        self.thread = Some(DeviceThread::spawn("rth-cpal-speaker", move |stop, ready| {
            let build = || -> Result<cpal::Stream> {
                let host = cpal::default_host();
                let device = host.default_output_device().context("no default output device")?;
                let supported = device.default_output_config().context("default output config")?;
                let config = supported.config();
                let ch = config.channels.max(1) as usize;
                let mut mono: Vec<i16> = Vec::with_capacity(16_384);
                let err = |e: cpal::Error| eprintln!("speaker stream error: {e}");
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        config,
                        move |out: &mut [f32], _: &_| {
                            let frames = out.len() / ch;
                            period.store(frames as u64, Ordering::Relaxed);
                            mono.resize(frames, 0);
                            queue.pull(&mut mono);
                            for (f, s) in out.chunks_mut(ch).zip(&mono) {
                                f.fill(*s as f32 / 32768.0);
                            }
                        },
                        err,
                        None,
                    )?,
                    cpal::SampleFormat::I16 => device.build_output_stream(
                        config,
                        move |out: &mut [i16], _: &_| {
                            let frames = out.len() / ch;
                            period.store(frames as u64, Ordering::Relaxed);
                            mono.resize(frames, 0);
                            queue.pull(&mut mono);
                            for (f, s) in out.chunks_mut(ch).zip(&mono) {
                                f.fill(*s);
                            }
                        },
                        err,
                        None,
                    )?,
                    other => bail!("unsupported speaker sample format {other:?}"),
                };
                stream.play().context("starting the speaker")?;
                Ok(stream)
            };
            match build() {
                Ok(stream) => {
                    let _ = ready.send(Ok(()));
                    let _ = stop.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            }
        })?);
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        if let Some(mut t) = self.thread.take() {
            t.stop();
        }
        Ok(format!("speaker {} · last device period {} frames", self.name, self.period.load(Ordering::Relaxed)))
    }

    fn describe(&self) -> String {
        let p = self.period.load(Ordering::Relaxed);
        let period = if p == 0 { "not observed yet".to_string() } else { format!("{p} frames ≈ {:.1} ms", p as f64 * 1000.0 / self.rate as f64) };
        format!("speaker {}: {} Hz × {} ch, device period {period}", self.name, self.rate, self.channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_scripted_mic_emits_real_time_frames_with_the_utterance_in_place() {
        let mut mic = FeedCapture::new();
        let (tx, mut rx) = mpsc::channel(64);
        let dropped = Arc::new(AtomicU64::new(0));
        let (start, end) = mic.say(&vec![7; 500]);
        assert_eq!((start, end), (0, 500));
        let t0 = Instant::now();
        mic.start(tx, dropped.clone()).unwrap();
        let mut got = Vec::new();
        while got.len() < 3 * FeedCapture::FRAME {
            got.extend(rx.recv().await.unwrap().samples);
        }
        mic.stop();
        assert!(t0.elapsed() >= Duration::from_millis(55), "paced, not dumped: {:?}", t0.elapsed());
        assert!(got[..500].iter().all(|&s| s == 7) && got[500..].iter().all(|&s| s == 0));
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn the_wav_sink_renders_the_queue_at_its_rate() {
        let path = std::env::temp_dir().join(format!("rth-sink-{}.wav", std::process::id()));
        let q = Arc::new(PlayQueue::with_seconds(24_000, 5.0));
        q.push(1, vec![100; 2400]);
        let mut sink = WavSink::new(path.clone(), 24_000, 10);
        sink.start(q.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while q.stats().queued > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(30));
        sink.stop().unwrap();
        let w = rth_voice::wav::read(&path).unwrap();
        assert_eq!(w.sample_rate, 24_000);
        let rendered = w.mono.iter().filter(|&&v| v != 0.0).count();
        assert_eq!(rendered, 2400);
        std::fs::remove_file(&path).unwrap();
    }
}
