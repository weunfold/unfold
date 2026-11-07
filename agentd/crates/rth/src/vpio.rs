

use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::macos_helpers::{get_default_device_id, get_device_name};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat};
use rth_protocol::MIC_SAMPLE_RATE;
use rth_voice::queue::PlayQueue;
use rth_voice::resample::Resampler;
use serde_json::{Value, json};

use crate::devices::{Capture, MicConvert, MicFrames, Playout};

const K_ENABLE_IO: u32 = 2003;
const K_STREAM_FORMAT: u32 = 8;
const K_BYPASS: u32 = 2100;
const K_AGC: u32 = 2101;
const K_DUCKING: u32 = 2108;

pub const START_DEADLINE: Duration = Duration::from_secs(10);

pub const REAP_WAIT: Duration = Duration::from_secs(2);

pub const CALLBACK_GRACE_MS: u64 = 2000;

#[repr(C)]
#[derive(Clone, Copy)]
struct Ducking {
    advanced: u8,
    level: u32,
}

#[derive(Debug, Clone)]
pub struct VpioOptions {

    pub rate: u32,

    pub agc: bool,

    pub ducking: String,
}

impl Default for VpioOptions {
    fn default() -> Self {
        Self { rate: 48_000, agc: true, ducking: "min".into() }
    }
}

const STARTING: u8 = 0;
const ARMED: u8 = 1;
const CANCELLED: u8 = 2;
const STOPPED: u8 = 3;

pub struct Attempt {
    pub id: u64,
    state: AtomicU8,
}

impl Attempt {
    fn armed(&self) -> bool {
        self.state.load(Ordering::Acquire) == ARMED
    }
}

#[derive(Default)]
struct Slots {
    queue: Option<Arc<PlayQueue>>,
    mic: Option<(MicFrames, Arc<AtomicU64>)>,
}

struct Io {
    slots: Mutex<Slots>,
    epoch: Instant,
    renders: AtomicU64,
    inputs: AtomicU64,
    input_errors: AtomicU64,
    disarmed_callbacks: AtomicU64,

    render_mismatches: AtomicU64,
    input_mismatches: AtomicU64,
    last_render_ms: AtomicU64,
    last_input_ms: AtomicU64,
    armed_at_ms: AtomicU64,
}

impl Io {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64 + 1
    }
}

pub struct RenderPort {
    io: Arc<Io>,
    attempt: Arc<Attempt>,
    mono: Vec<i16>,
}

impl RenderPort {
    pub fn render(&mut self, out: &mut [f32]) {
        if !self.attempt.armed() {
            self.io.disarmed_callbacks.fetch_add(1, Ordering::Relaxed);
            out.fill(0.0);
            return;
        }
        self.io.renders.fetch_add(1, Ordering::Relaxed);
        self.io.last_render_ms.store(self.io.now_ms(), Ordering::Relaxed);
        self.mono.resize(out.len(), 0);
        match self.io.slots.try_lock().ok().and_then(|s| s.queue.clone()) {
            Some(q) => {
                q.pull(&mut self.mono);
            }
            None => self.mono.fill(0),
        }
        for (o, s) in out.iter_mut().zip(&self.mono) {
            *o = *s as f32 / 32768.0;
        }
    }
}

pub struct CapturePort {
    io: Arc<Io>,
    attempt: Arc<Attempt>,
    rate: u32,
    conv: Option<MicConvert>,
}

impl CapturePort {
    pub fn capture(&mut self, input: Option<&[f32]>) {
        if !self.attempt.armed() {
            self.io.disarmed_callbacks.fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.io.inputs.fetch_add(1, Ordering::Relaxed);
        self.io.last_input_ms.store(self.io.now_ms(), Ordering::Relaxed);
        let Some(input) = input else {
            self.io.input_errors.fetch_add(1, Ordering::Relaxed);
            return;
        };
        if self.conv.is_none() {
            let Some((frames, dropped)) = self.io.slots.try_lock().ok().and_then(|s| s.mic.clone()) else { return };
            self.conv = Some(MicConvert { channels: 1, resampler: Resampler::new(self.rate, MIC_SAMPLE_RATE), mono: Vec::new(), out: Vec::new(), frames, dropped });
        }
        if let Some(c) = self.conv.as_mut() {
            c.push(input.iter().copied());
        }
    }
}

pub trait UnitFactory: Send + Sync + 'static {
    fn start(&self, opts: &VpioOptions, render: RenderPort, capture: CapturePort) -> Result<(Box<dyn std::any::Any>, Value)>;

    fn default_devices(&self) -> Value {
        Value::Null
    }
}

struct Worker {
    attempt: Arc<Attempt>,
    stop: Sender<()>,

    done: Receiver<()>,
}

impl Worker {
    fn finished_within(&self, d: Duration) -> bool {
        matches!(self.done.recv_timeout(d), Err(RecvTimeoutError::Disconnected) | Ok(()))
    }
}

pub struct VpioShared {
    opts: VpioOptions,
    factory: Arc<dyn UnitFactory>,
    deadline: Duration,
    io: Arc<Io>,
    unit: Mutex<Option<Worker>>,

    retained: Mutex<Vec<Worker>>,
    state: Arc<Mutex<Value>>,
    next_attempt: AtomicU64,
    late_starts_discarded: Arc<AtomicU64>,

    attempt_failed: std::sync::atomic::AtomicBool,
}

impl VpioShared {
    pub fn new(opts: VpioOptions) -> Arc<Self> {
        Self::with_factory(opts, Arc::new(CoreAudioUnit), START_DEADLINE)
    }

    pub fn with_factory(opts: VpioOptions, factory: Arc<dyn UnitFactory>, deadline: Duration) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(json!({"io": "vpio", "phase": "not_started", "options": {"rate": opts.rate, "agc": opts.agc, "ducking": opts.ducking}}))),
            opts,
            factory,
            deadline,
            io: Arc::new(Io {
                slots: Mutex::new(Slots::default()),
                epoch: Instant::now(),
                renders: AtomicU64::new(0),
                inputs: AtomicU64::new(0),
                input_errors: AtomicU64::new(0),
                disarmed_callbacks: AtomicU64::new(0),
                render_mismatches: AtomicU64::new(0),
                input_mismatches: AtomicU64::new(0),
                last_render_ms: AtomicU64::new(0),
                last_input_ms: AtomicU64::new(0),
                armed_at_ms: AtomicU64::new(0),
            }),
            unit: Mutex::new(None),
            retained: Mutex::new(Vec::new()),
            next_attempt: AtomicU64::new(1),
            late_starts_discarded: Arc::new(AtomicU64::new(0)),
            attempt_failed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn status(&self) -> Value {
        let mut v = self.state.lock().unwrap().clone();
        let io = &self.io;
        let now = io.now_ms();
        let since = |a: &AtomicU64| {
            let t = a.load(Ordering::Relaxed);
            if t == 0 { Value::Null } else { json!(now.saturating_sub(t)) }
        };
        let running = v["phase"] == "running";
        let armed_at = io.armed_at_ms.load(Ordering::Relaxed);

        let stalled = |last: &AtomicU64| running && now.saturating_sub(last.load(Ordering::Relaxed).max(armed_at)) > CALLBACK_GRACE_MS;
        v["callbacks"] = json!({
            "render": io.renders.load(Ordering::Relaxed), "input": io.inputs.load(Ordering::Relaxed),
            "input_render_errors": io.input_errors.load(Ordering::Relaxed),
            "disarmed": io.disarmed_callbacks.load(Ordering::Relaxed),
            "render_buffer_mismatches": io.render_mismatches.load(Ordering::Relaxed),
            "input_buffer_mismatches": io.input_mismatches.load(Ordering::Relaxed),
            "ms_since_render": since(&io.last_render_ms), "ms_since_input": since(&io.last_input_ms),
            "render_stalled": stalled(&io.last_render_ms), "input_stalled": stalled(&io.last_input_ms),
            "grace_ms": CALLBACK_GRACE_MS,
        });
        v["stalled"] = json!(stalled(&io.last_render_ms) || stalled(&io.last_input_ms));
        v["late_starts_discarded"] = json!(self.late_starts_discarded.load(Ordering::Relaxed));
        v["mic_mode"] = mic_mode();
        v
    }

    fn set(&self, f: impl FnOnce(&mut Value)) {
        f(&mut self.state.lock().unwrap());
    }

    fn fail(&self, msg: String) -> bool {
        self.attempt_failed.store(true, Ordering::SeqCst);
        eprintln!("rth-host: Apple voice processing did not start ({msg}); voice is OFF (no raw fallback)");
        self.set(|s| {
            s["phase"] = json!("failed");
            s["error"] = json!(msg);
            s["voice"] = json!("off: the microphone delivers nothing; agent audio is refused (device_unavailable), never played");
        });
        false
    }

    pub fn unavailable(&self) -> Option<String> {
        if self.unit.lock().unwrap().is_some() {
            return None;
        }
        let s = self.state.lock().unwrap();
        match s["phase"].as_str() {
            Some("failed") => Some(s["error"].as_str().unwrap_or("voice processing failed").to_string()),
            Some("running") => None,
            _ => Some("voice processing is not running".into()),
        }
    }

    fn ensure_started(self: &Arc<Self>) -> bool {
        let mut unit = self.unit.lock().unwrap();
        if unit.is_some() {
            return true;
        }
        if self.attempt_failed.load(Ordering::SeqCst) {
            return false;
        }

        {
            let mut kept = self.retained.lock().unwrap();
            kept.retain(|w| !w.finished_within(REAP_WAIT));
            if !kept.is_empty() {
                return self.fail(format!("{} earlier voice-processing start(s) still inside the audio unit; not starting another", kept.len()));
            }
        }
        let attempt = Arc::new(Attempt { id: self.next_attempt.fetch_add(1, Ordering::SeqCst), state: AtomicU8::new(STARTING) });
        let (stop_tx, stop_rx) = channel::<()>();
        let (done_tx, done_rx) = channel::<()>();
        let (ready_tx, ready_rx) = channel::<Result<Value>>();
        let me = self.clone();
        let a = attempt.clone();
        let spawned = std::thread::Builder::new().name(format!("rth-vpio-{}", attempt.id)).spawn(move || {
            let _done = done_tx;
            me.run(a, stop_rx, ready_tx);
        });
        if let Err(e) = spawned {
            return self.fail(format!("spawn: {e}"));
        }
        let worker = Worker { attempt: attempt.clone(), stop: stop_tx, done: done_rx };
        match ready_rx.recv_timeout(self.deadline) {
            Ok(Ok(facts)) => self.arm(&mut unit, worker, facts),
            Ok(Err(e)) => {
                self.retained.lock().unwrap().push(worker);
                self.fail(format!("{e:#}"))
            }
            Err(_) => {

                attempt.state.store(CANCELLED, Ordering::Release);
                let _ = worker.stop.send(());
                self.retained.lock().unwrap().push(worker);
                self.fail(format!("start did not complete within {} ms; the attempt is cancelled and a late unit is stopped, never used", self.deadline.as_millis()))
            }
        }
    }

    fn arm(&self, unit: &mut Option<Worker>, worker: Worker, facts: Value) -> bool {

        if worker.attempt.state.compare_exchange(STARTING, ARMED, Ordering::AcqRel, Ordering::Acquire).is_err() {
            let _ = worker.stop.send(());
            self.retained.lock().unwrap().push(worker);
            return self.fail("start was cancelled".into());
        }
        let io = &self.io;
        io.last_render_ms.store(0, Ordering::Relaxed);
        io.last_input_ms.store(0, Ordering::Relaxed);
        io.armed_at_ms.store(io.now_ms(), Ordering::Relaxed);
        let devices = self.factory.default_devices();
        self.set(|s| {
            s["phase"] = json!("running");
            s["attempt"] = json!(worker.attempt.id);
            s["error"] = Value::Null;
            s["voice"] = Value::Null;
            for (k, v) in facts.as_object().cloned().unwrap_or_default() {
                s[k.as_str()] = v;
            }
            s["default_devices_at_start"] = devices.clone();
            s["default_devices_note"] = json!("the OS default input/output devices when the unit started; not device ids read from the unit");
            s["default_device_changes"] = json!([]);
        });
        *unit = Some(worker);
        true
    }

    fn stop_unit(&self) {
        if let Some(w) = self.unit.lock().unwrap().take() {

            w.attempt.state.store(STOPPED, Ordering::Release);
            let _ = w.stop.send(());
            self.retained.lock().unwrap().push(w);
            self.set(|s| {
                if s["phase"] == "running" {
                    s["phase"] = json!("stopped");
                }
            });
        }
    }

    fn run(self: Arc<Self>, attempt: Arc<Attempt>, stop: Receiver<()>, ready: Sender<Result<Value>>) {
        let render = RenderPort { io: self.io.clone(), attempt: attempt.clone(), mono: Vec::with_capacity(4096) };
        let capture = CapturePort { io: self.io.clone(), attempt: attempt.clone(), rate: self.opts.rate, conv: None };
        let (guard, facts) = match self.factory.start(&self.opts, render, capture) {
            Ok(g) => g,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        if attempt.state.load(Ordering::Acquire) != STARTING || ready.send(Ok(facts)).is_err() {

            self.late_starts_discarded.fetch_add(1, Ordering::Relaxed);
            drop(guard);
            return;
        }

        let mut last = self.factory.default_devices();
        loop {
            match stop.recv_timeout(Duration::from_secs(1)) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if attempt.state.load(Ordering::Acquire) == CANCELLED {
                break;
            }
            let now = self.factory.default_devices();
            if now != last {
                eprintln!("rth-host: OS default audio devices changed under voice processing: {last} -> {now}");
                self.set(|s| {
                    if let Some(a) = s["default_device_changes"].as_array_mut() {
                        a.push(json!({"utc_ms": rth_protocol::utc_ms(), "from": last.clone(), "to": now.clone()}));
                    }
                    s["default_device_note"] = json!("the unit was configured at start; if audio stops after a device change, restart voice");
                });
                last = now;
            }
        }
        if attempt.state.load(Ordering::Acquire) == CANCELLED {
            self.late_starts_discarded.fetch_add(1, Ordering::Relaxed);
        }
        drop(guard);
    }
}

pub struct CoreAudioUnit;

struct Running {
    au: AudioUnit,
    _input: Box<InputCb>,
    _render: Box<RenderCb>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.au.stop();
    }
}

const K_SET_RENDER_CALLBACK: u32 = 23;
const K_SET_INPUT_CALLBACK: u32 = 2005;
const K_MAX_FRAMES: u32 = 14;

const MIN_INPUT_FRAMES: usize = 4096;

const MAX_ZERO_BUFFERS: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RawBuffer {
    pub channels: u32,
    pub bytes: u32,
    pub data: *mut c_void,
}

#[repr(C)]
pub(crate) struct RawBufferList {
    pub count: u32,
    pub buffers: [RawBuffer; 1],
}

type RenderProc = unsafe extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut RawBufferList) -> i32;

#[repr(C)]
struct CallbackStruct {
    proc_: Option<RenderProc>,
    refcon: *mut c_void,
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioUnitRender(unit: *mut c_void, flags: *mut u32, time: *const c_void, bus: u32, frames: u32, data: *mut RawBufferList) -> i32;
}

pub(crate) fn mono_f32_span(count: u32, first: Option<RawBuffer>, data_ok: bool, frames: u32) -> Result<usize, &'static str> {
    let Some(b) = first.filter(|_| count > 0) else { return Err("no buffer") };
    if count > 1 {
        return Err("more than one buffer (not mono non-interleaved)");
    }
    if !data_ok {
        return Err("no usable data pointer");
    }
    if b.channels != 1 {
        return Err("not one channel");
    }
    let want = frames as usize;
    if (b.bytes as usize) < want * std::mem::size_of::<f32>() {
        return Err("buffer shorter than the frame count");
    }
    Ok(want)
}

unsafe fn zero_fill(list: *mut RawBufferList) {
    unsafe {

        let count = ((*list).count as usize).min(MAX_ZERO_BUFFERS);
        let first = std::ptr::addr_of_mut!((*list).buffers) as *mut RawBuffer;
        for i in 0..count {
            let b = *first.add(i);
            if !b.data.is_null() {
                std::ptr::write_bytes(b.data as *mut u8, 0, b.bytes as usize);
            }
        }
    }
}

pub(crate) struct RenderCb {
    pub port: RenderPort,
}

pub(crate) struct InputCb {
    pub port: CapturePort,
    pub unit: *mut c_void,
    pub buf: Vec<f32>,
    pub list: RawBufferList,

    pub render: unsafe extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut RawBufferList) -> i32,
}

impl InputCb {
    pub(crate) fn new(port: CapturePort, frames: usize) -> Box<Self> {
        Box::new(Self {
            port,
            unit: std::ptr::null_mut(),
            buf: vec![0.0; frames.max(MIN_INPUT_FRAMES)],
            list: RawBufferList { count: 0, buffers: [RawBuffer { channels: 0, bytes: 0, data: std::ptr::null_mut() }] },
            render: AudioUnitRender,
        })
    }
}

pub(crate) unsafe extern "C" fn render_proc(refcon: *mut c_void, _flags: *mut u32, _time: *const c_void, _bus: u32, frames: u32, data: *mut RawBufferList) -> i32 {
    unsafe {
        let st = &mut *(refcon as *mut RenderCb);
        if data.is_null() {
            st.port.io.render_mismatches.fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        let count = (*data).count;
        let first = (count > 0).then(|| (*data).buffers[0]);
        let data_ok = first.is_some_and(|b| !b.data.is_null());
        match mono_f32_span(count, first, data_ok, frames) {
            Ok(n) => st.port.render(std::slice::from_raw_parts_mut(first.unwrap().data as *mut f32, n)),
            Err(_) => {
                st.port.io.render_mismatches.fetch_add(1, Ordering::Relaxed);
                zero_fill(data);
            }
        }
        0
    }
}

pub(crate) unsafe extern "C" fn input_proc(refcon: *mut c_void, flags: *mut u32, time: *const c_void, bus: u32, frames: u32, _data: *mut RawBufferList) -> i32 {
    unsafe {
        let st = &mut *(refcon as *mut InputCb);
        if frames as usize > st.buf.len() {
            st.port.io.input_mismatches.fetch_add(1, Ordering::Relaxed);
            return 0;
        }
        let ours = st.buf.as_mut_ptr() as *mut c_void;
        st.list.count = 1;
        st.list.buffers[0] = RawBuffer { channels: 1, bytes: frames * 4, data: ours };
        let status = (st.render)(st.unit, flags, time, bus, frames, &mut st.list);
        if status != 0 {
            st.port.capture(None);
            return status;
        }
        let first = (st.list.count > 0).then(|| st.list.buffers[0]);
        match mono_f32_span(st.list.count, first, first.is_some_and(|b| b.data == ours), frames) {
            Ok(n) => st.port.capture(Some(&st.buf[..n])),
            Err(_) => {
                st.port.io.input_mismatches.fetch_add(1, Ordering::Relaxed);
            }
        }
        0
    }
}

impl UnitFactory for CoreAudioUnit {
    fn default_devices(&self) -> Value {
        let dev = |input: bool| match get_default_device_id(input) {
            Some(id) => json!({"id": id, "name": get_device_name(id).unwrap_or_else(|_| "?".into())}),
            None => Value::Null,
        };
        json!({"input": dev(true), "output": dev(false)})
    }

    fn start(&self, opts: &VpioOptions, render: RenderPort, capture: CapturePort) -> Result<(Box<dyn std::any::Any>, Value)> {

        let mut render_cb = Box::new(RenderCb { port: render });
        let mut input_cb = InputCb::new(capture, MIN_INPUT_FRAMES);
        let mut au = AudioUnit::new_uninitialized(IOType::VoiceProcessingIO).map_err(|e| anyhow!("create VoiceProcessingIO: {e}"))?;
        au.set_property(K_ENABLE_IO, Scope::Input, Element::Input, Some(&1u32)).map_err(|e| anyhow!("enable input bus: {e}"))?;
        au.set_property(K_ENABLE_IO, Scope::Output, Element::Output, Some(&1u32)).map_err(|e| anyhow!("enable output bus: {e}"))?;
        let fmt = StreamFormat {
            sample_rate: opts.rate as f64,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: 1,
        };

        au.set_property(K_STREAM_FORMAT, Scope::Output, Element::Input, Some(&fmt.to_asbd())).map_err(|e| anyhow!("capture format: {e}"))?;
        au.set_property(K_STREAM_FORMAT, Scope::Input, Element::Output, Some(&fmt.to_asbd())).map_err(|e| anyhow!("render format: {e}"))?;
        au.set_property(K_BYPASS, Scope::Global, Element::Input, Some(&0u32)).map_err(|e| anyhow!("voice processing on: {e}"))?;
        let agc_set = au.set_property(K_AGC, Scope::Global, Element::Input, Some(&(opts.agc as u32))).map_err(|e| e.to_string());
        let level = match opts.ducking.as_str() {
            "default" => 0,
            "mid" => 20,
            "max" => 30,
            _ => 10,
        };
        let duck_set = au.set_property(K_DUCKING, Scope::Global, Element::Output, Some(&Ducking { advanced: 0, level })).map_err(|e| e.to_string());

        let max_frames = au.get_property::<u32>(K_MAX_FRAMES, Scope::Global, Element::Output).unwrap_or(0) as usize;
        if max_frames > input_cb.buf.len() {
            input_cb.buf = vec![0.0; max_frames];
        }

        render_cb.port.mono.reserve(input_cb.buf.len());
        let input_frames = input_cb.buf.len();
        input_cb.unit = *au.as_ref() as *mut c_void;
        let render_refcon = &mut *render_cb as *mut RenderCb as *mut c_void;
        let input_refcon = &mut *input_cb as *mut InputCb as *mut c_void;
        au.set_property(K_SET_RENDER_CALLBACK, Scope::Input, Element::Output, Some(&CallbackStruct { proc_: Some(render_proc), refcon: render_refcon }))
            .map_err(|e| anyhow!("render callback: {e}"))?;
        au.set_property(K_SET_INPUT_CALLBACK, Scope::Global, Element::Output, Some(&CallbackStruct { proc_: Some(input_proc), refcon: input_refcon }))
            .map_err(|e| anyhow!("input callback: {e}"))?;
        au.initialize().map_err(|e| anyhow!("initialize VoiceProcessingIO: {e}"))?;
        let read_u32 = |id: u32, el: Element| au.get_property::<u32>(id, Scope::Global, el).map(|v| v != 0).map_err(|e| e.to_string());
        let bypass = read_u32(K_BYPASS, Element::Input);
        let agc = read_u32(K_AGC, Element::Input);
        let f = |scope, el| au.stream_format(scope, el).map(|f| json!({"rate": f.sample_rate, "channels": f.channels, "format": format!("{:?}", f.sample_format)})).unwrap_or(Value::Null);
        let formats = json!({"capture_client": f(Scope::Output, Element::Input), "render_client": f(Scope::Input, Element::Output),
                             "input_hw_side": f(Scope::Input, Element::Input), "output_hw_side": f(Scope::Output, Element::Output), "read_back": true});
        au.start().map_err(|e| anyhow!("start VoiceProcessingIO: {e}"))?;
        let facts = json!({
            "voice_processing": match &bypass {
                Ok(b) => json!({"enabled": !b, "bypass": b, "read_back": true}),
                Err(e) => json!({"enabled": "unknown", "bypass": "unknown", "read_back": false, "read_error": e}),
            },
            "agc": match &agc {
                Ok(a) => json!({"enabled": a, "read_back": true, "set_call": if agc_set.is_ok() { "ok" } else { "failed" }}),
                Err(e) => json!({"enabled": "unknown", "read_back": false, "read_error": e}),
            },
            "other_audio_ducking": match &duck_set {
                Ok(()) => json!({"requested_level": opts.ducking, "set_call": "ok", "read_back": false}),
                Err(e) => json!({"requested_level": opts.ducking, "set_call": "failed", "error": e, "read_back": false, "note": "macOS 14+ only; the system default applies"}),
            },
            "formats": formats,
            "input_buffer_frames": input_frames,
        });
        Ok((Box::new(Running { au, _input: input_cb, _render: render_cb }), facts))
    }
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

pub fn mic_mode() -> Value {
    use objc2::runtime::AnyClass;
    use objc2::{msg_send, sel};
    let name = |m: isize| match m {
        0 => "standard",
        1 => "wide_spectrum",
        2 => "voice_isolation",
        _ => "unknown",
    };
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
        return json!({"active": "unknown", "preferred": "unknown", "note": "AVCaptureDevice unavailable"});
    };
    let readable: bool = unsafe { msg_send![cls, respondsToSelector: sel!(activeMicrophoneMode)] };
    if !readable {
        return json!({"active": "unknown", "preferred": "unknown", "note": "this macOS has no microphone modes"});
    }
    let active: isize = unsafe { msg_send![cls, activeMicrophoneMode] };
    let preferred: isize = unsafe { msg_send![cls, preferredMicrophoneMode] };
    json!({"active": name(active), "preferred": name(preferred),
           "note": "chosen by the user in Control Center (Mic Mode) while voice processing runs; RTH reads it, never sets it"})
}

pub fn show_mic_modes() -> bool {
    use objc2::runtime::AnyClass;
    use objc2::{msg_send, sel};
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return false };
    let ok: bool = unsafe { msg_send![cls, respondsToSelector: sel!(showSystemUserInterface:)] };
    if ok {
        let _: () = unsafe { msg_send![cls, showSystemUserInterface: 2isize] };
    }
    ok
}

pub struct VpioCapture(pub Arc<VpioShared>);

impl Capture for VpioCapture {
    fn start(&mut self, frames: MicFrames, dropped: Arc<AtomicU64>) -> Result<()> {
        self.0.io.slots.lock().unwrap().mic = Some((frames, dropped));

        self.0.ensure_started();
        Ok(())
    }

    fn stop(&mut self) {
        self.0.stop_unit();
        self.0.io.slots.lock().unwrap().mic = None;
    }

    fn describe(&self) -> String {
        let s = self.0.status();
        format!("Apple voice processing (VoiceProcessingIO) mic: {}", s["phase"].as_str().unwrap_or("?"))
    }
}

pub struct VpioPlayout(pub Arc<VpioShared>);

impl Playout for VpioPlayout {
    fn rate(&self) -> u32 {
        self.0.opts.rate
    }

    fn start(&mut self, queue: Arc<PlayQueue>) -> Result<()> {
        self.0.io.slots.lock().unwrap().queue = Some(queue);

        self.0.ensure_started();
        Ok(())
    }

    fn stop(&mut self) -> Result<String> {
        self.0.stop_unit();
        self.0.io.slots.lock().unwrap().queue = None;

        self.0.attempt_failed.store(false, Ordering::SeqCst);
        let s = self.0.status();
        Ok(format!("voice processing {} · render callbacks {}", s["phase"].as_str().unwrap_or("?"), s["callbacks"]["render"]))
    }

    fn describe(&self) -> String {
        format!("Apple voice processing (VoiceProcessingIO) speaker, client {} Hz mono", self.0.opts.rate)
    }

    fn unavailable(&self) -> Option<String> {
        self.0.unavailable()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::devices::MicFrame;
    use tokio::sync::mpsc;

    pub(crate) struct FakeUnit {
        pub delays: Mutex<Vec<Result<Duration, String>>>,
        pub started: AtomicU64,
        pub running: Arc<AtomicU64>,
    }

    impl FakeUnit {
        pub(crate) fn new(delays: Vec<Result<Duration, String>>) -> Arc<Self> {
            Arc::new(Self { delays: Mutex::new(delays), started: AtomicU64::new(0), running: Arc::new(AtomicU64::new(0)) })
        }
    }

    struct FakeRunning(Arc<std::sync::atomic::AtomicBool>, Option<std::thread::JoinHandle<()>>);
    impl Drop for FakeRunning {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
            if let Some(j) = self.1.take() {
                let _ = j.join();
            }
        }
    }

    impl UnitFactory for FakeUnit {
        fn start(&self, _: &VpioOptions, mut render: RenderPort, mut capture: CapturePort) -> Result<(Box<dyn std::any::Any>, Value)> {
            let next = { let mut d = self.delays.lock().unwrap(); if d.is_empty() { Ok(Duration::ZERO) } else { d.remove(0) } };
            let delay = next.map_err(|e| anyhow!(e))?;
            std::thread::sleep(delay);
            self.started.fetch_add(1, Ordering::SeqCst);
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (s, running) = (stop.clone(), self.running.clone());
            let device = std::thread::spawn(move || {
                running.fetch_add(1, Ordering::SeqCst);
                let mut out = vec![0f32; 480];
                let input = vec![0.25f32; 480];
                while !s.load(Ordering::SeqCst) {
                    render.render(&mut out);
                    capture.capture(Some(&input));
                    std::thread::sleep(Duration::from_millis(10));
                }
                running.fetch_sub(1, Ordering::SeqCst);
            });
            Ok((Box::new(FakeRunning(stop, Some(device))), json!({"voice_processing": {"enabled": true, "bypass": false, "read_back": true}})))
        }
    }

    fn rig(factory: Arc<FakeUnit>, deadline_ms: u64) -> (Arc<VpioShared>, VpioCapture, VpioPlayout, Arc<PlayQueue>, mpsc::Receiver<MicFrame>) {
        let shared = VpioShared::with_factory(VpioOptions::default(), factory, Duration::from_millis(deadline_ms));
        let q = Arc::new(PlayQueue::with_seconds(48_000, 10.0));
        q.push(1, vec![1000; 48_000]);
        let (tx, rx) = mpsc::channel(256);
        let mut cap = VpioCapture(shared.clone());
        let mut play = VpioPlayout(shared.clone());
        play.start(q.clone()).unwrap();
        cap.start(tx, Arc::new(AtomicU64::new(0))).unwrap();
        (shared, cap, play, q, rx)
    }

    #[test]
    fn a_late_successful_start_is_discarded_and_the_next_attempt_is_safe() {
        let unit = FakeUnit::new(vec![Ok(Duration::from_millis(400)), Ok(Duration::ZERO)]);
        let (shared, mut cap, mut play, q, mut rx) = rig(unit.clone(), 100);
        assert_eq!(shared.status()["phase"], "failed");
        assert!(shared.status()["error"].as_str().unwrap().contains("did not complete within 100 ms"));
        assert!(play.unavailable().is_some(), "voice is off for this link");
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(unit.started.load(Ordering::SeqCst), 1, "the slow start did complete");
        assert_eq!(unit.running.load(Ordering::SeqCst), 0, "and its device is stopped again");
        assert_eq!(q.rendered(1).samples, 0, "nothing rendered");
        assert!(q.drain_tap().is_empty(), "no render taps (no pull)");
        assert!(rx.try_recv().is_err(), "no capture delivered");
        let st = shared.status();
        assert_eq!((st["phase"].as_str(), st["late_starts_discarded"].as_u64()), (Some("failed"), Some(1)), "{st}");
        assert_eq!(st["callbacks"]["render"], 0);

        cap.stop();
        play.stop().unwrap();
        play.start(q.clone()).unwrap();
        let (tx, mut rx2) = mpsc::channel(256);
        cap.start(tx, Arc::new(AtomicU64::new(0))).unwrap();
        assert_eq!(shared.status()["phase"], "running");
        assert!(play.unavailable().is_none());
        std::thread::sleep(Duration::from_millis(200));
        assert!(q.rendered(1).samples > 0, "the armed unit renders");
        assert!(rx2.try_recv().is_ok(), "and captures");
        cap.stop();
        play.stop().unwrap();
        assert_eq!(shared.status()["phase"], "stopped");
        drop(rx);
    }

    #[test]
    fn a_failed_start_pulls_nothing_and_reports_unavailable() {
        let unit = FakeUnit::new(vec![Err("injected: no device".into())]);
        let (shared, mut cap, mut play, q, mut rx) = rig(unit, 1000);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(shared.status()["phase"], "failed");
        assert!(play.unavailable().unwrap().contains("injected"));
        assert_eq!((q.rendered(1).samples, q.stats().pulls), (0, 0));
        assert!(rx.try_recv().is_err());
        cap.stop();
        play.stop().unwrap();
    }

    fn armed_ports() -> (Arc<VpioShared>, RenderPort, CapturePort) {
        let shared = VpioShared::with_factory(VpioOptions::default(), FakeUnit::new(vec![]), Duration::from_secs(1));
        let attempt = Arc::new(Attempt { id: 99, state: AtomicU8::new(ARMED) });
        let render = RenderPort { io: shared.io.clone(), attempt: attempt.clone(), mono: Vec::with_capacity(4096) };
        let capture = CapturePort { io: shared.io.clone(), attempt, rate: 48_000, conv: None };
        (shared, render, capture)
    }

    const SENTINEL: f32 = 7.0;
    fn list(data: *mut f32, channels: u32, bytes: u32, count: u32) -> RawBufferList {
        RawBufferList { count, buffers: [RawBuffer { channels, bytes, data: data as *mut c_void }] }
    }

    #[test]
    fn the_span_check_refuses_every_buffer_that_does_not_fit() {
        let b = |channels, bytes| Some(RawBuffer { channels, bytes, data: std::ptr::null_mut() });
        assert_eq!(mono_f32_span(1, b(1, 480 * 4), true, 480), Ok(480));
        assert_eq!(mono_f32_span(1, b(1, 481 * 4), true, 480), Ok(480), "a larger buffer is fine");
        assert!(mono_f32_span(1, b(1, 400 * 4), true, 480).is_err(), "shorter than the frame count");
        assert!(mono_f32_span(1, b(1, 480 * 4 - 1), true, 480).is_err(), "one byte short");
        assert!(mono_f32_span(0, None, true, 480).is_err(), "no buffer");
        assert!(mono_f32_span(2, b(1, 480 * 4), true, 480).is_err(), "two buffers");
        assert!(mono_f32_span(1, b(2, 480 * 8), true, 480).is_err(), "two channels");
        assert!(mono_f32_span(1, b(1, 480 * 4), false, 480).is_err(), "no usable data");
        assert_eq!(mono_f32_span(1, b(1, 0), true, 0), Ok(0), "zero frames, zero bytes");
    }

    #[test]
    fn a_short_render_buffer_is_zero_filled_within_its_size_and_counted() {
        let (shared, render, _) = armed_ports();
        let mut cb = RenderCb { port: render };

        let mut mem = vec![SENTINEL; 960];
        let mut l = list(mem.as_mut_ptr(), 1, 100 * 4, 1);
        let r = unsafe { render_proc(&mut cb as *mut RenderCb as *mut c_void, std::ptr::null_mut(), std::ptr::null(), 0, 480, &mut l) };
        assert_eq!(r, 0);
        assert!(mem[..100].iter().all(|&v| v == 0.0), "silence inside the buffer");
        assert!(mem[100..].iter().all(|&v| v == SENTINEL), "nothing written past its byte size");
        assert_eq!(shared.status()["callbacks"]["render_buffer_mismatches"], 1);
        assert_eq!(shared.status()["callbacks"]["render"], 0, "the port never saw the bad buffer");

        let mut l = list(mem.as_mut_ptr(), 1, 480 * 4, 1);
        unsafe { render_proc(&mut cb as *mut RenderCb as *mut c_void, std::ptr::null_mut(), std::ptr::null(), 0, 100, &mut l) };
        assert_eq!(shared.status()["callbacks"]["render"], 1);
        assert_eq!(shared.status()["callbacks"]["render_buffer_mismatches"], 1);

        let mut l = list(mem.as_mut_ptr(), 2, 480 * 8, 1);
        unsafe { render_proc(&mut cb as *mut RenderCb as *mut c_void, std::ptr::null_mut(), std::ptr::null(), 0, 480, &mut l) };
        unsafe { render_proc(&mut cb as *mut RenderCb as *mut c_void, std::ptr::null_mut(), std::ptr::null(), 0, 480, std::ptr::null_mut()) };
        assert_eq!(shared.status()["callbacks"]["render_buffer_mismatches"], 3);
    }

    static FAKE_RENDERS: AtomicU64 = AtomicU64::new(0);
    static FAKE_MODE: AtomicU8 = AtomicU8::new(0);

    unsafe extern "C" fn fake_render(_: *mut c_void, _: *mut u32, _: *const c_void, _: u32, frames: u32, data: *mut RawBufferList) -> i32 {
        FAKE_RENDERS.fetch_add(1, Ordering::SeqCst);
        unsafe {
            let b = &mut (*data).buffers[0];
            match FAKE_MODE.load(Ordering::SeqCst) {
                0 => std::slice::from_raw_parts_mut(b.data as *mut f32, frames as usize).fill(0.25),
                1 => b.bytes = frames * 2,
                2 => b.data = 0x10 as *mut c_void,
                _ => return -50,
            }
        }
        0
    }

    #[test]
    fn the_input_callback_uses_only_its_preallocated_buffer_and_drops_misfits() {
        let (shared, _, capture) = armed_ports();
        let mut cb = InputCb::new(capture, 0);
        cb.render = fake_render;
        assert_eq!(cb.buf.len(), MIN_INPUT_FRAMES, "preallocated before any callback");
        let ptr = cb.buf.as_ptr();
        let call = |cb: &mut InputCb, frames| unsafe { input_proc(cb as *mut InputCb as *mut c_void, std::ptr::null_mut(), std::ptr::null(), 1, frames, std::ptr::null_mut()) };
        let cbs = |k: &str| shared.status()["callbacks"][k].as_u64().unwrap();
        FAKE_MODE.store(0, Ordering::SeqCst);
        assert_eq!(call(&mut cb, 480), 0);
        assert_eq!((cbs("input"), cbs("input_buffer_mismatches")), (1, 0), "a fit is delivered");
        let before = FAKE_RENDERS.load(Ordering::SeqCst);
        call(&mut cb, (MIN_INPUT_FRAMES + 1) as u32);
        assert_eq!(FAKE_RENDERS.load(Ordering::SeqCst), before, "too many frames: never rendered");
        assert_eq!(cbs("input_buffer_mismatches"), 1);
        FAKE_MODE.store(1, Ordering::SeqCst);
        call(&mut cb, 480);
        FAKE_MODE.store(2, Ordering::SeqCst);
        call(&mut cb, 480);
        assert_eq!((cbs("input"), cbs("input_buffer_mismatches")), (1, 3), "short and swapped results are dropped");
        FAKE_MODE.store(3, Ordering::SeqCst);
        assert_eq!(call(&mut cb, 480), -50, "a render error is passed back");
        assert_eq!(cbs("input_render_errors"), 1);
        assert_eq!(cb.buf.as_ptr(), ptr, "the buffer never moved or grew");
        assert_eq!(cb.buf.len(), MIN_INPUT_FRAMES);
    }

    #[test]
    fn a_unit_without_callbacks_is_stalled_after_the_grace() {
        struct Silent;
        impl UnitFactory for Silent {
            fn start(&self, _: &VpioOptions, _: RenderPort, _: CapturePort) -> Result<(Box<dyn std::any::Any>, Value)> {
                Ok((Box::new(()), json!({})))
            }
        }
        let shared = VpioShared::with_factory(VpioOptions::default(), Arc::new(Silent), Duration::from_secs(1));
        let mut play = VpioPlayout(shared.clone());
        play.start(Arc::new(PlayQueue::with_seconds(48_000, 1.0))).unwrap();
        let st = shared.status();
        assert_eq!((st["phase"].as_str(), st["stalled"].as_bool()), (Some("running"), Some(false)), "inside the grace");
        std::thread::sleep(Duration::from_millis(CALLBACK_GRACE_MS + 100));
        let st = shared.status();
        assert_eq!((st["callbacks"]["render_stalled"].as_bool(), st["callbacks"]["input_stalled"].as_bool()), (Some(true), Some(true)), "{st}");
        play.stop().unwrap();
    }
}
