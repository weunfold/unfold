

use serde_json::{json, Value};
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};

pub const RATE: usize = 16_000;

#[cfg_attr(not(feature = "voice_pane"), allow(dead_code))]
pub const CONVERSATION_TRAILING_SILENCE_SECS: f32 = 0.9;

const LEVEL_BLOCK: usize = RATE / 20;

const TAIL: usize = RATE * 8 / 10;

pub trait Recognizer {

    fn accept(&mut self, samples: &[f32]);

    fn text(&mut self) -> String;

    fn endpoint(&mut self) -> bool;

    fn reset(&mut self);

    fn finish(&mut self) -> String;

    fn failed(&mut self) -> Option<String> {
        None
    }

    fn settled_words(&mut self) -> Option<usize> {
        None
    }

    fn heard_after(&mut self, _t: f32) -> Option<String> {
        None
    }

    fn accept_tail(&mut self, samples: &[f32]) {
        self.accept(samples)
    }

    fn slowest_pass(&mut self) -> Option<std::time::Duration> {
        None
    }

    fn settling_off_at(&mut self) -> Option<std::time::Instant> {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {

    Listening,

    Text { seq: u64, text: String, settled: Option<usize>, is_final: bool },

    Level(f32),

    Tick(u32),

    Stopped,

    Finishing,

    Cancelled,

    Denied,

    NoModel,

    Downloading(u8),

    Error(String),
}

impl Event {
    pub fn js(&self) -> Value {
        match self {
            Event::Listening => json!({"state": "listening"}),
            Event::Text { seq, text, settled: None, is_final } => json!({"seq": seq, "text": text, "is_final": is_final}),
            Event::Text { seq, text, settled: Some(n), is_final } => {
                let words: Vec<&str> = text.split_whitespace().collect();
                let n = (*n).min(words.len());
                json!({"seq": seq, "text": text, "settled": words[..n].join(" "), "tail": words[n..].join(" "), "is_final": is_final})
            }
            Event::Level(v) => json!({"level": (f64::from(*v) * 1000.0).round() / 1000.0}),
            Event::Tick(s) => json!({"tick": s}),
            Event::Stopped => json!({"state": "stopped"}),
            Event::Finishing => json!({"state": "finishing"}),
            Event::Cancelled => json!({"state": "cancelled"}),
            Event::Denied => json!({"state": "denied"}),
            Event::NoModel => json!({"state": "no_model"}),
            Event::Downloading(p) => json!({"state": "downloading", "pct": p}),
            Event::Error(m) => json!({"state": "error", "message": m}),
        }
    }
}

pub fn level_of(sum_sq: f64, n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let rms = (sum_sq / n as f64).sqrt();
    let db = 20.0 * rms.max(1e-9).log10();
    (((db + 60.0) / 50.0).clamp(0.0, 1.0)) as f32
}

pub struct Session<R: Recognizer> {
    rec: R,
    seq: u64,
    partial: String,

    partial_settled: Option<usize>,
    heard: u64,
    block: (f64, usize),
    finals: u32,

    partials: u32,

    second: (f64, usize),
    loudness: Vec<i8>,

    began: std::time::Instant,
    first_words: Option<std::time::Duration>,
    first_final: Option<std::time::Duration>,
}

impl<R: Recognizer> Session<R> {
    pub fn new(rec: R) -> Self {
        Session { rec, seq: 0, partial: String::new(), partial_settled: None, heard: 0, block: (0.0, 0), finals: 0, partials: 0, second: (0.0, 0), loudness: vec![], began: std::time::Instant::now(), first_words: None, first_final: None }
    }

    #[cfg(feature = "voice_pane")]
    fn discard_segment(&mut self) {
        self.rec.reset(); self.partial.clear(); self.partial_settled = None;
    }

    #[cfg(feature = "voice_pane")]
    pub(crate) fn sync_overlap(&mut self, epoch:u64, observed:&mut u64) -> bool {
        let reset=crate::conversation::capture_boundary(epoch);
        if *observed!=reset {self.discard_segment();*observed=reset;true} else {false}
    }

    pub fn timing(&mut self) -> String {
        let at = |d: Option<std::time::Duration>| d.map_or("none".to_string(), |d| format!("{:.1} s", d.as_secs_f64()));
        let slowest = self.rec.slowest_pass().map_or(String::new(), |d| format!(", slowest settle pass {:.1} s", d.as_secs_f64()));
        let off = self.rec.settling_off_at().map_or(String::new(), |t| format!(", settling off after a failed pass at {:.1} s", t.saturating_duration_since(self.began).as_secs_f64()));
        format!("; first words {}, first final {}{slowest}{off}", at(self.first_words), at(self.first_final))
    }

    pub fn finals(&self) -> u32 {
        self.finals
    }

    pub fn partials(&self) -> u32 {
        self.partials
    }

    pub fn loudness_db(&self) -> &[i8] {
        &self.loudness
    }

    pub fn failed(&mut self) -> Option<String> {
        self.rec.failed()
    }

    #[cfg(test)]
    pub fn recognizer(&mut self) -> &mut R {
        &mut self.rec
    }

    fn text_event(&mut self, text: String, settled: Option<usize>, is_final: bool) -> Event {
        let since = Some(self.began.elapsed());
        self.first_words = self.first_words.or(since);
        if is_final {
            self.first_final = self.first_final.or(since);
        }
        self.seq += 1;
        Event::Text { seq: self.seq, text, settled, is_final }
    }

    pub fn feed(&mut self, samples: &[f32]) -> Vec<Event> {
        let mut ev = vec![];
        for &x in samples {
            self.block.0 += f64::from(x) * f64::from(x);
            self.block.1 += 1;
            self.second.0 += f64::from(x) * f64::from(x);
            self.second.1 += 1;
            if self.second.1 == RATE {
                let rms = (self.second.0 / RATE as f64).sqrt();
                self.loudness.push(if rms > 0.0 { (20.0 * rms.log10()).round().clamp(-99.0, 0.0) as i8 } else { -99 });
                self.second = (0.0, 0);
            }
            self.heard += 1;
            if self.block.1 == LEVEL_BLOCK {
                ev.push(Event::Level(level_of(self.block.0, self.block.1)));
                self.block = (0.0, 0);
            }
            if self.heard % RATE as u64 == 0 {
                ev.push(Event::Tick((self.heard / RATE as u64) as u32));
            }
        }
        self.rec.accept(samples);
        let ended = self.rec.endpoint();
        let text = self.rec.text();
        if ended {
            if !text.is_empty() {
                self.finals += 1;
                let n = self.rec.settled_words().map(|_| text.split_whitespace().count());
                ev.push(self.text_event(text, n, true));
            }
            self.partial.clear();
            self.partial_settled = None;
            self.rec.reset();
        } else {
            let settled = self.rec.settled_words();
            if text != self.partial || settled != self.partial_settled {
                self.partial = text.clone();
                self.partial_settled = settled;
                self.partials += 1;
                ev.push(self.text_event(text, settled, false));
            }
        }
        ev
    }

    pub fn stop(&mut self) -> Vec<Event> {
        self.rec.accept_tail(&[0.0; TAIL]);
        let text = match self.rec.finish() {
            t if t.is_empty() => std::mem::take(&mut self.partial),
            t => t,
        };
        let mut ev = vec![];
        if !text.is_empty() {
            self.finals += 1;
            let n = self.rec.settled_words().map(|_| text.split_whitespace().count());
            ev.push(self.text_event(text, n, true));
        }
        ev.push(Event::Stopped);
        ev
    }

    pub fn cancel(&mut self) -> Vec<Event> {
        vec![Event::Cancelled]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Permission {
    Granted,
    NotAsked,
    Denied,
}

#[derive(Debug)]
pub enum MicError {
    Denied,
    Other(String),

    Stuck(String),
}

pub trait Mic {
    fn permission(&self) -> Permission;

    fn ask(&self, answer: Box<dyn FnOnce(bool) + Send>);

    fn open(&mut self, sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError>;

    fn opener(&self) -> Option<Opener> {
        None
    }
}

pub type Opener = Box<dyn FnOnce(Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any + Send>, u32, String), MicError> + Send>;

pub const OPEN_WITHIN: std::time::Duration = std::time::Duration::from_secs(5);

fn open_within(open: Opener, sink: Box<dyn FnMut(Vec<f32>) + Send>, within: std::time::Duration, opening: &Arc<AtomicBool>) -> Result<(Box<dyn Any>, u32, String), MicError> {
    if opening.swap(true, Ordering::SeqCst) {
        return Err(MicError::Stuck("the last try is still starting".into()));
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);

    struct Opening(Arc<AtomicBool>);
    impl Drop for Opening {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let flag = Opening(Arc::clone(opening));
    let spawned = std::thread::Builder::new().name("dictation-mic".into()).spawn(move || {
        let _flag = flag;

        let _ = tx.send(open(sink));
    });
    if let Err(e) = spawned {
        opening.store(false, Ordering::SeqCst);
        return Err(MicError::Other(format!("no thread for the mic: {e}")));
    }
    match rx.recv_timeout(within) {
        Ok(r) => r.map(|(guard, rate, device)| (guard as Box<dyn Any>, rate, device)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(MicError::Stuck("CoreAudio still starting it".into())),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(MicError::Other("its open ended without an answer".into())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModelState {
    Ready,
    Missing,
    Downloading(u8),
}

pub trait Models {
    type R: Recognizer;
    fn state(&self) -> ModelState;

    fn state_for(&self, _lang: &str) -> ModelState { self.state() }

    fn load(&mut self, lang: &str) -> Result<Self::R, String>;

    fn unload(&mut self) {}
}

pub enum Msg {
    Start {
        lang: String,
    },
    Stop,
    Cancel,

    #[cfg(test)]
    Checkpoint(Box<dyn FnOnce() + Send>),

    Audio(Vec<f32>),
    #[cfg(feature = "voice_pane")]
    ConversationStart {lang:String,epoch:u64},
    #[cfg(feature = "voice_pane")]
    ConversationAudio { samples: Vec<f32>, epoch: u64, playout:Option<(crate::bc_playout::Snapshot,bool)> },

    Answer(bool),
}

pub trait Resample {
    fn to_16k(&mut self, samples: &[f32]) -> Vec<f32>;
}

struct Live<R: Recognizer> {
    session: Session<R>,
    #[cfg(feature = "voice_pane")]
    epoch: Option<u64>,
    #[cfg(feature = "voice_pane")]
    turn_detect: Option<Result<crate::turn_detect::Detector,String>>,
    #[cfg(feature = "voice_pane")]
    turn_input:Option<u64>,
    #[cfg(feature = "voice_pane")]
    turn_warned:bool,
    #[cfg(feature = "voice_pane")]
    overlap_reset: u64,
    #[cfg(feature = "voice_pane")]
    shadow:Option<crate::turn_shadow::Observer>,

    rate: u32,
    resample: Option<Box<dyn Resample>>,

    mic: Option<Box<dyn Any>>,

    dropped: Arc<AtomicU64>,
}

#[derive(Default)]
struct Early {
    ready: bool,
    audio: Vec<Vec<f32>>,
    samples: usize,
    dropped: u64,

    cap: usize,
}

const EARLY_SECS: usize = 30;

impl Early {

    fn keep(&mut self, b: Vec<f32>) -> Option<Vec<f32>> {
        if self.ready {
            return Some(b);
        }
        let cap = if self.cap == 0 { EARLY_SECS * 48_000 } else { self.cap };
        if self.samples + b.len() <= cap {
            self.samples += b.len();
            self.audio.push(b);
        } else {
            self.dropped += 1;
        }
        None
    }

    fn for_rate(&mut self, rate: u32) {
        self.cap = EARLY_SECS * rate as usize;
    }

    fn take(&mut self) -> Early {
        let kept = std::mem::take(self);
        self.ready = true;
        kept
    }
}

type Said = (&'static str, &'static str);
const MODEL_FAILED: Said = ("The dictation model didn’t load. Try again, or reinstall it in Settings.", "O modelo de ditado não carregou. Tente de novo, ou instale de novo nos Ajustes.");
const ENGINE_STOPPED: Said = ("Dictation stopped. Press the mic to go on.", "O ditado parou. Toque no microfone para continuar.");
const NO_MIC: Said = ("No microphone is available.", "Nenhum microfone disponível.");
const MIC_STUCK: Said = ("The microphone didn’t start. Try again in a moment.", "O microfone não ligou. Tente de novo daqui a pouco.");
const BUSY: Said = ("Dictation is already listening.", "O ditado já está ouvindo.");
const CHOOSE_FAILED: Said = ("Couldn’t set up dictation. You can install it later in Settings.", "Não deu para preparar o ditado. Dá para instalar depois nos Ajustes.");
const UNAVAILABLE: Said = ("Dictation isn’t available in this run.", "O ditado não está disponível nesta execução.");
#[cfg(feature = "voice_pane")]
const VOICE_TAKE: Said = ("The microphone is recording a voice sample in Settings.", "O microfone está gravando uma amostra de voz nos Ajustes.");

#[cfg(feature = "voice_pane")]
pub fn mic_busy() -> bool {
    WORKER.get().is_some_and(|(_, a)| a.load(Ordering::SeqCst)) || STARTING.load(Ordering::SeqCst)
}

fn said(s: Said) -> String {
    crate::settings::t(s.0, s.1).to_string()
}

pub type Active = Arc<AtomicBool>;

pub struct Worker<M: Mic, L: Models> {
    mic: M,
    models: L,
    emit: Box<dyn Fn(&Event)>,
    tx: SyncSender<Msg>,
    active: Active,
    live: Option<Live<L::R>>,
    #[cfg(feature="voice_pane")]
    overlap:crate::bc_detector::Prepared,
    asking: Option<String>,
    resampler: Box<dyn Fn(u32) -> Option<Box<dyn Resample>>>,
    pub log: Box<dyn Fn(&str)>,

    pub open_within: std::time::Duration,

    opening: Arc<AtomicBool>,

    pub starting: Option<&'static AtomicBool>,

    pub phase: Option<&'static AtomicU8>,
}

pub const IDLE_PHASE: u8 = 0;
pub const LOADING: u8 = 1;
pub const ABORTED: u8 = 2;

pub const STOPPED: u8 = 3;

pub fn abort_load(phase: &AtomicU8, active: &AtomicBool, keep: bool) -> bool {
    if phase.compare_exchange(LOADING, if keep { STOPPED } else { ABORTED }, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return false;
    }
    active.store(false, Ordering::SeqCst);
    true
}

static PHASE: AtomicU8 = AtomicU8::new(IDLE_PHASE);

impl<M: Mic, L: Models> Worker<M, L> {
    pub fn new(mic: M, models: L, emit: Box<dyn Fn(&Event)>, tx: SyncSender<Msg>, active: Active, resampler: Box<dyn Fn(u32) -> Option<Box<dyn Resample>>>) -> Self {
        Worker { mic, models, emit, tx, active, live: None, #[cfg(feature="voice_pane")] overlap:Default::default(), asking: None, resampler, log: Box::new(|_| {}), open_within: OPEN_WITHIN, opening: Arc::default(), starting: None, phase: None }
    }

    #[cfg(test)]
    pub fn listening(&self) -> bool {
        self.live.is_some()
    }

    fn send(&self, events: Vec<Event>) {
        for e in &events {
            (self.emit)(e);
        }
    }

    fn end(&mut self, e: Event) {
        self.not_starting();
        self.asking = None;
        self.active.store(false, Ordering::SeqCst);
        (self.emit)(&e);
    }

    pub fn handle(&mut self, msg: Msg) {
        match msg {
            #[cfg(test)]
            Msg::Checkpoint(done) => done(),
            #[cfg(feature="voice_pane")]
            Msg::ConversationStart {lang,epoch}=>{
                if !crate::conversation::accepts(epoch) {self.end(Event::Cancelled);return;}

                if self.overlap.prepare(epoch).is_err() && self.overlap.notify_failure(epoch) {
                    (self.log)("conversation: overlap unavailable; degraded mode: manual stop only");
                    crate::conversation::overlap_unavailable(epoch);
                }
                if crate::conversation::accepts(epoch) {self.handle(Msg::Start{lang});}else{self.end(Event::Cancelled);}
            }
            Msg::Start { lang } => {
                if self.live.is_some() || self.asking.is_some() {
                    return;
                }
                match self.models.state_for(&lang) {
                    ModelState::Missing => return self.end(Event::NoModel),
                    ModelState::Downloading(p) => return self.end(Event::Downloading(p)),
                    ModelState::Ready => {}
                }
                match self.mic.permission() {
                    Permission::Denied => self.end(Event::Denied),
                    Permission::NotAsked => {
                        self.asking = Some(lang);
                        let tx = self.tx.clone();
                        self.mic.ask(Box::new(move |granted| {
                            let _ = tx.send(Msg::Answer(granted));
                        }));
                    }
                    Permission::Granted => self.begin(lang),
                }
            }
            Msg::Answer(granted) => {
                let Some(lang) = self.asking.take() else { return };
                if granted {
                    self.begin(lang)
                } else {
                    self.end(Event::Denied)
                }
            }
            #[cfg(feature = "voice_pane")]
            Msg::ConversationAudio { samples, epoch, playout } => {
                if !crate::conversation::accepts(epoch) { return; }
                let Some(l) = self.live.as_mut() else { return };

                if l.epoch != Some(epoch) {
                    l.session.discard_segment();
                    l.epoch = Some(epoch);
                    if let Some(Ok(detector))=l.turn_detect.as_mut() {detector.reset();}
                }
                if crate::conversation::accepts(epoch) { self.audio_stamped(samples, Some(epoch),playout); }
            }
            Msg::Audio(a) => {
                #[cfg(feature = "voice_pane")]
                if crate::conversation::active() { return; }
                self.audio(a);
            }
            Msg::Stop | Msg::Cancel => {
                let cancel = matches!(msg, Msg::Cancel);
                if let Some(mut l) = self.live.take() {
                    l.mic = None;
                    let secs = l.session.heard / RATE as u64;
                    let partials = l.session.partials();
                    let db: Vec<String> = l.session.loudness_db().iter().map(i8::to_string).collect();
                    let ev = if cancel { l.session.cancel() } else { l.session.stop() };
                    self.active.store(false, Ordering::SeqCst);

                    let timing = l.session.timing();
                    (self.log)(&format!("dictation: {} after {secs} s, {} segment(s), {partials} partial(s), {} Hz in, {} mic buffer(s) dropped, dBFS per s [{}]{timing}", if cancel { "cancelled" } else { "stopped" }, l.session.finals(), l.rate, l.dropped.load(Ordering::Relaxed), db.join(" ")));
                    self.send(ev);
                } else if self.asking.is_some() {
                    self.end(if cancel { Event::Cancelled } else { Event::Stopped });
                }
            }
        }
    }

    fn audio(&mut self, a: Vec<f32>) { self.audio_for(a, None); }

    fn audio_for(&mut self, a: Vec<f32>, epoch: Option<u64>) {
        self.audio_stamped(a,epoch,None);
    }
    fn audio_stamped(&mut self,a:Vec<f32>,epoch:Option<u64>,#[cfg_attr(not(feature="voice_pane"),allow(unused_variables))] playout:Option<(crate::bc_playout::Snapshot,bool)>) {
                let Some(l) = self.live.as_mut() else { return };
                let a = match l.resample.as_mut() {
                    Some(r) => r.to_16k(&a),
                    None => a,
                };
                #[cfg(feature="voice_pane")]
                if let Some(epoch)=epoch {
                    if l.session.sync_overlap(epoch,&mut l.overlap_reset) {
                        self.overlap.reset_segment();
                        if let Some(Ok(detector))=l.turn_detect.as_mut() {detector.reset();}
                    }
                }
                #[cfg(feature="voice_pane")]
                if let Some((epoch,turn))=epoch.and_then(crate::conversation::overlap_token) {
                    if let Some((rendered,held))=playout {
                        match self.overlap.push(epoch,turn,&a,rendered,held) {
                            Ok(decisions)=>for d in decisions {
                                if !crate::conversation::overlap_decision(epoch,turn,&d) && d.action==crate::bc_detector::Action::Yield {self.overlap.reject_yield();}
                            },
                            Err(_)=>if self.overlap.notify_failure(epoch) {
                                (self.log)("conversation: overlap unavailable; degraded mode: manual stop only");
                                crate::conversation::overlap_unavailable(epoch);
                            }
                        }
                    }
                }
                #[cfg(feature = "voice_pane")]
                let input_turn=crate::conversation::input_turn();
                #[cfg(feature = "voice_pane")]
                if l.turn_input!=Some(input_turn){
                    if let Some(Ok(d))=l.turn_detect.as_mut(){d.reset();}
                    l.turn_input=Some(input_turn);
                }
                #[cfg(feature="voice_pane")]
                if let Some(epoch)=epoch {
                    if l.session.sync_overlap(epoch,&mut l.overlap_reset) {
                        self.overlap.reset_segment();
                        if let Some(Ok(detector))=l.turn_detect.as_mut() {detector.reset();}
                    }
                }
                let ev = l.session.feed(&a);
                let failed = l.session.failed();
                for e in ev {
                    #[cfg(feature = "voice_pane")]
                    if epoch.is_some_and(|epoch| !crate::conversation::accepts(epoch)) { break; }
                    (self.emit)(&e);
                }
                #[cfg(feature = "voice_pane")]
                if let Some(epoch)=epoch.filter(|e|crate::conversation::accepts(*e)) {
                    let mode=crate::settings::turn_mode();
                    if mode.endpoint {
                        l.shadow=None;
                        let detector=l.turn_detect.get_or_insert_with(||crate::turn_detect::Detector::load(Default::default()));
                        let result=match detector {Ok(d)=>d.push(&a),Err(e)=>Err(e.clone())};
                        match result {
                            Ok(decisions)=>for decision in decisions {
                                if crate::conversation::model_decision_in_mode(epoch,input_turn,&decision,mode) {
                                    if let Ok(d)=detector {d.reset();}
                                    break;
                                }
                            },
                            Err(_)=>if !l.turn_warned {l.turn_warned=true;(self.log)("conversation: CPU turn detector unavailable");
                                (self.emit)(&Event::Error(said(("The turn detector could not start. Try Conversation again.","O detector de turnos não pôde iniciar. Tente a Conversa novamente."))));}
                        }
                    } else {
                        l.turn_detect=None;
                        if mode.shadow {
                            if l.shadow.is_none() && !l.turn_warned {
                                let mut warned=None;
                                match crate::turn_shadow::Observer::spawn(move |o|match o.result {
                                    Ok(d)=>{crate::conversation::shadow_decision(o.epoch,o.input_turn,&d,o.stats);},
                                    Err(_)=>if warned!=Some((o.epoch,o.input_turn)) {
                                        warned=Some((o.epoch,o.input_turn));
                                        crate::settings::conversation_log("conversation: CPU turn observer unavailable; timer remains active".into());
                                    }
                                }) {
                                    Ok(observer)=>l.shadow=Some(observer),
                                    Err(_)=>{l.turn_warned=true;(self.log)("conversation: CPU turn observer thread unavailable; timer remains active");}
                                }
                            }
                            if let Some(observer)=l.shadow.as_mut(){observer.push(epoch,input_turn,&a);}
                        } else {l.shadow=None;l.turn_warned=false;}
                    }
                }
                #[cfg(not(feature = "voice_pane"))]
                let _ = epoch;
                if let Some(e) = failed {

                    self.live = None;
                    (self.log)(&format!("dictation: the engine stopped: {e}"));
                    self.end(Event::Error(said(ENGINE_STOPPED)));
                }
    }

    fn begin(&mut self, lang: String) {
        let tx = self.tx.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let early: Arc<Mutex<Early>> = Arc::default();
        let (d2, e2, phase) = (Arc::clone(&dropped), Arc::clone(&early), self.phase);
        let sink = Box::new(move |b: Vec<f32>| {

            if phase.is_some_and(|p| matches!(p.load(Ordering::SeqCst), ABORTED | STOPPED)) {
                return;
            }
            #[cfg(feature = "voice_pane")]
            let (conversation, epoch, listening) = crate::conversation::snapshot();
            #[cfg(feature = "voice_pane")]
            if conversation && !listening { return; }
            let Some(b) = e2.lock().unwrap().keep(b) else { return };
            #[cfg(feature = "voice_pane")]
            if conversation {
                let playout=crate::bc_playout::snapshot().map(|p|(p,crate::doubletalk::playout_held()));
                if tx.try_send(Msg::ConversationAudio { samples: b, epoch, playout }).is_err() { d2.fetch_add(1, Ordering::Relaxed); }
                return;
            }

            if tx.try_send(Msg::Audio(b)).is_err() {
                d2.fetch_add(1, Ordering::Relaxed);
            }
        });
        let t0 = std::time::Instant::now();
        let opened = match self.mic.opener() {
            Some(open) => open_within(open, sink, self.open_within, &self.opening),
            None => self.mic.open(sink),
        };
        let (guard, rate, device) = match opened {
            Ok(o) => o,
            Err(MicError::Denied) => return self.end(Event::Denied),
            Err(MicError::Stuck(e)) => {
                (self.log)(&format!("dictation: the mic did not open in {} s: {e}", self.open_within.as_secs_f32()));
                return self.end(Event::Error(said(MIC_STUCK)));
            }
            Err(MicError::Other(e)) => {
                (self.log)(&format!("dictation: the mic did not open: {e}"));
                return self.end(Event::Error(said(NO_MIC)));
            }
        };
        let mic_ms = t0.elapsed().as_millis();
        early.lock().unwrap().for_rate(rate);
        let resample = if rate as usize == RATE { None } else { (self.resampler)(rate) };
        if rate as usize != RATE && resample.is_none() {
            (self.log)(&format!("dictation: no resampler for {rate} Hz"));
            return self.end(Event::Error(said(NO_MIC)));
        }
        (self.log)(&format!("dictation: mic opened in {mic_ms} ms, before the model ({device}, {rate} Hz)"));
        self.not_starting();
        (self.emit)(&Event::Listening);
        let t1 = std::time::Instant::now();
        if let Some(p) = self.phase {
            p.store(LOADING, Ordering::SeqCst);
        }
        let loaded = self.models.load(&lang);

        match self.phase.map(|p| p.swap(IDLE_PHASE, Ordering::SeqCst)) {
            Some(ABORTED) => {
                drop(guard);
                (self.log)(&format!("dictation: cancelled during the model load ({} ms after the mic); nothing transcribed", t1.elapsed().as_millis()));
                return;
            }
            Some(STOPPED) => {
                drop(guard);
                return self.transcribe_late(loaded, &early, resample, rate, t1.elapsed().as_millis());
            }
            _ => {}
        }
        let rec = match loaded {
            Ok(r) => r,
            Err(e) => {
                drop(guard);
                (self.log)(&format!("dictation: the model did not load: {e}"));
                return self.end(Event::Error(said(MODEL_FAILED)));
            }
        };
        let kept = early.lock().unwrap().take();
        dropped.fetch_add(kept.dropped, Ordering::Relaxed);
        self.live = Some(Live { session: Session::new(rec), #[cfg(feature = "voice_pane")] epoch: None, #[cfg(feature = "voice_pane")] turn_detect: None, #[cfg(feature = "voice_pane")] turn_input:None, #[cfg(feature = "voice_pane")] turn_warned:false,
                        #[cfg(feature="voice_pane")] overlap_reset:0, #[cfg(feature = "voice_pane")] shadow:None, rate, resample, mic: Some(guard), dropped });
        (self.log)(&format!("dictation: listening ({lang}, {rate} Hz, {device}); the model was ready {} ms after the mic, {:.1} s heard meanwhile", t1.elapsed().as_millis(), kept.samples as f64 / rate.max(1) as f64));
        for b in kept.audio {
            if self.live.is_none() {
                break;
            }
            #[cfg(feature = "voice_pane")]
            if crate::conversation::active() {
                let (_, epoch, _) = crate::conversation::snapshot();
                self.handle(Msg::ConversationAudio { samples: b, epoch,playout:None });
                continue;
            }
            self.handle(Msg::Audio(b));
        }
    }

    fn transcribe_late(&mut self, loaded: Result<L::R, String>, early: &Mutex<Early>, mut resample: Option<Box<dyn Resample>>, rate: u32, load_ms: u128) {
        let rec = match loaded {
            Ok(r) => r,
            Err(e) => {
                (self.log)(&format!("dictation: stop during the model load; the model did not load: {e}"));
                return (self.emit)(&Event::Error(said(MODEL_FAILED)));
            }
        };
        let kept = early.lock().unwrap().take();
        let mut session = Session::new(rec);
        let mut ev = vec![];
        for b in kept.audio {
            let a = match resample.as_mut() {
                Some(r) => r.to_16k(&b),
                None => b,
            };
            ev.extend(session.feed(&a));
            if session.failed().is_some() {
                break;
            }
        }
        ev.extend(session.stop());
        (self.log)(&format!("dictation: stop during the model load: {:.1} s heard before it, transcribed after the load ({load_ms} ms), {} segment(s)", kept.samples as f64 / rate.max(1) as f64, session.finals()));
        self.send(ev);
    }

    fn not_starting(&self) {
        if let Some(s) = self.starting {
            s.store(false, Ordering::SeqCst);
        }
    }

    pub fn run(mut self, rx: Receiver<Msg>, idle: std::time::Duration) {
        loop {
            match rx.recv_timeout(idle) {
                Ok(m) => self.handle(m),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self.live.is_none() && self.asking.is_none() {
                        self.models.unload();
                        #[cfg(feature="voice_pane")] {self.overlap=Default::default();}
                    }
                }
                Err(_) => return,
            }
        }
    }
}

const THREADS: i32 = 4;

fn path_of(dir: &std::path::Path, name: &str) -> Option<String> {
    Some(dir.join(name).to_string_lossy().into_owned())
}

pub fn online_recognizer(dir: &std::path::Path) -> Result<sherpa_onnx::OnlineRecognizer, String> {
    online_recognizer_with(dir, true)
}

pub fn online_recognizer_with(dir: &std::path::Path, endpoints: bool) -> Result<sherpa_onnx::OnlineRecognizer, String> {
    let mut c = sherpa_onnx::OnlineRecognizerConfig::default();
    c.model_config.transducer =
        sherpa_onnx::OnlineTransducerModelConfig { encoder: path_of(dir, "encoder.int8.onnx"), decoder: path_of(dir, "decoder.int8.onnx"), joiner: path_of(dir, "joiner.int8.onnx") };
    c.model_config.tokens = path_of(dir, "tokens.txt");
    c.model_config.num_threads = THREADS;
    c.model_config.provider = Some("cpu".into());
    c.decoding_method = Some("greedy_search".into());
    c.enable_endpoint = endpoints;

    c.rule1_min_trailing_silence = 2.4;
    c.rule2_min_trailing_silence = 1.2;
    #[cfg(feature = "voice_pane")]
    if crate::conversation::active() { c.rule2_min_trailing_silence = CONVERSATION_TRAILING_SILENCE_SECS; }
    c.rule3_min_utterance_length = 20.0;
    sherpa_onnx::OnlineRecognizer::create(&c).ok_or_else(|| format!("{}: sherpa-onnx could not create the recognizer", dir.display()))
}

pub struct Online {
    rec: Arc<sherpa_onnx::OnlineRecognizer>,
    stream: sherpa_onnx::OnlineStream,
}

impl Online {

    pub fn new(rec: Arc<sherpa_onnx::OnlineRecognizer>, lang: &str) -> Self {
        let stream = rec.create_stream();
        if !lang.is_empty() {
            stream.set_option("language", lang);
        }
        Online { rec, stream }
    }

    #[cfg(test)]

    pub fn first_token_s(&self) -> Option<f32> {
        self.rec.get_result(&self.stream)?.timestamps?.first().copied()
    }

    fn drain(&self) {
        while self.rec.is_ready(&self.stream) {
            self.rec.decode(&self.stream);
        }
    }
}

impl Recognizer for Online {
    fn accept(&mut self, samples: &[f32]) {
        self.stream.accept_waveform(RATE as i32, samples);
        self.drain();
    }
    fn text(&mut self) -> String {
        self.rec.get_result(&self.stream).map(|r| r.text.trim().to_string()).unwrap_or_default()
    }
    fn endpoint(&mut self) -> bool {
        self.rec.is_endpoint(&self.stream)
    }
    fn reset(&mut self) {
        self.rec.reset(&self.stream);
    }
    fn finish(&mut self) -> String {
        self.stream.input_finished();
        self.drain();
        self.text()
    }
}

pub fn offline_recognizer(dir: &std::path::Path) -> Result<sherpa_onnx::OfflineRecognizer, String> {
    let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
    c.model_config.transducer =
        sherpa_onnx::OfflineTransducerModelConfig { encoder: path_of(dir, "encoder.int8.onnx"), decoder: path_of(dir, "decoder.int8.onnx"), joiner: path_of(dir, "joiner.int8.onnx") };
    c.model_config.tokens = path_of(dir, "tokens.txt");
    c.model_config.num_threads = THREADS;
    c.model_config.provider = Some("cpu".into());
    c.model_config.model_type = Some("nemo_transducer".into());
    c.decoding_method = Some("greedy_search".into());
    sherpa_onnx::OfflineRecognizer::create(&c).ok_or_else(|| format!("{}: sherpa-onnx could not create the recognizer", dir.display()))
}

pub fn offline_ctc_recognizer(dir: &std::path::Path) -> Result<sherpa_onnx::OfflineRecognizer, String> {
    let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
    c.model_config.nemo_ctc = sherpa_onnx::OfflineNemoEncDecCtcModelConfig { model: path_of(dir, "model.onnx") };
    c.model_config.tokens = path_of(dir, "tokens.txt");
    c.model_config.num_threads = THREADS;
    c.model_config.provider = Some("cpu".into());
    c.decoding_method = Some("greedy_search".into());
    sherpa_onnx::OfflineRecognizer::create(&c).ok_or_else(|| format!("{}: sherpa-onnx could not create the recognizer", dir.display()))
}

const VOICED_MIN: usize = RATE / 5;
const SPEECH_DB: f64 = -45.0;
const OVER_FLOOR_DB: f64 = 12.0;
const FLOOR_RISE_DB: f64 = 0.05;
const SPEECH_MIN_DB: f64 = -75.0;
const PAUSE: usize = RATE * 7 / 10;
const LONGEST: usize = RATE * 20;
const QUIET_NO_PASS: usize = RATE * 3 / 10;

const PASS_EVERY: usize = RATE;

const FRAME: f32 = 0.08;

pub struct Decoded {
    pub text: String,
    pub words: Vec<crate::dictation_settle::Word>,
}

pub type Pass = Box<dyn FnMut(&[f32], bool) -> Result<Decoded, String>>;

struct LivePasses {
    pass: Pass,
    agreement: crate::dictation_settle::Agreement,

    since: usize,
    voiced: bool,

    last: Option<(std::time::Instant, std::time::Duration)>,
    slowest: Option<std::time::Duration>,

    off_at: Option<std::time::Instant>,

    now: Box<dyn Fn() -> std::time::Instant>,
}

pub struct Phrases<D: FnMut(&[f32]) -> String> {
    decode: D,
    live: Option<LivePasses>,
    buf: Vec<f32>,
    spoke: bool,
    quiet: usize,
    text: String,
    ended: bool,
    floor: Option<f64>,
    voiced: usize,
}

impl<D: FnMut(&[f32]) -> String> Phrases<D> {
    pub fn new(decode: D) -> Self {
        Phrases { decode, live: None, buf: vec![], spoke: false, quiet: 0, text: String::new(), ended: false, floor: None, voiced: 0 }
    }

    pub fn with_passes(mut self, pass: Pass) -> Self {
        self.live = Some(LivePasses { pass, agreement: Default::default(), since: 0, voiced: false, last: None, slowest: None, off_at: None, now: Box::new(std::time::Instant::now) });
        self
    }

    #[cfg(test)]
    pub fn with_clock(mut self, now: Box<dyn Fn() -> std::time::Instant>) -> Self {
        if let Some(l) = &mut self.live {
            l.now = now;
        }
        self
    }

    fn pass(&mut self, n: usize) {
        let Phrases { live: Some(l), buf, quiet, .. } = self else { return };
        l.since += n;
        let resting = l.last.is_some_and(|(end, took)| (l.now)().saturating_duration_since(end) < took);
        if l.off_at.is_some() || l.since < PASS_EVERY || !l.voiced || *quiet >= QUIET_NO_PASS || resting {
            return;
        }
        (l.since, l.voiced) = (0, false);
        let t = (l.now)();
        let decoded = (l.pass)(buf, false);
        let end = (l.now)();
        let took = end.saturating_duration_since(t);
        l.slowest = l.slowest.max(Some(took));
        l.last = Some((end, took));
        match decoded {
            Ok(d) => l.agreement.pass(&d.words, buf.len() as f32 / RATE as f32),
            Err(_) => l.off_at = Some(end),
        }
    }
    fn close(&mut self) {
        if self.spoke && self.voiced >= VOICED_MIN && !self.buf.is_empty() {

            let live = match &mut self.live {
                Some(l) if l.off_at.is_none() => match (l.pass)(&self.buf, true) {
                    Ok(d) => Some(d.text),
                    Err(_) => {
                        l.off_at = Some((l.now)());
                        None
                    }
                },
                _ => None,
            };
            self.text = live.unwrap_or_else(|| (self.decode)(&self.buf)).trim().to_string();
        }
        self.ended = true;
    }
}

impl<D: FnMut(&[f32]) -> String> Recognizer for Phrases<D> {
    fn accept(&mut self, samples: &[f32]) {
        if self.ended {
            return;
        }
        for block in samples.chunks(LEVEL_BLOCK) {
            let sq: f64 = block.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
            let db = 20.0 * (sq / block.len() as f64).sqrt().max(1e-9).log10();
            let floor = self.floor.map_or(db, |f| db.min(f + FLOOR_RISE_DB));

            if db >= -100.0 { self.floor = Some(floor); }
            if db > SPEECH_DB || (db > floor + OVER_FLOOR_DB && db > SPEECH_MIN_DB) {
                self.spoke = true;
                self.quiet = 0;
                self.voiced += block.len();
                if let Some(l) = &mut self.live {
                    l.voiced = true;
                }
            } else {
                self.quiet += block.len();
            }
        }
        self.buf.extend_from_slice(samples);
        if !self.spoke {

            let keep = RATE * 3 / 10;
            if self.buf.len() > keep {
                self.buf.drain(..self.buf.len() - keep);
            }
        }
        if self.spoke && (self.quiet >= PAUSE || self.buf.len() >= LONGEST) {
            self.close();
        } else if self.spoke {
            self.pass(samples.len());
        }
    }
    fn text(&mut self) -> String {
        match &self.live {

            Some(l) if !self.ended && l.agreement.has_pass() => l.agreement.settled().iter().chain(l.agreement.tail()).cloned().collect::<Vec<_>>().join(" "),
            _ => self.text.clone(),
        }
    }
    fn settled_words(&mut self) -> Option<usize> {

        let l = self.live.as_ref().filter(|l| l.off_at.is_none() && (self.ended || l.agreement.has_pass()))?;
        Some(if self.ended { self.text.split_whitespace().count() } else { l.agreement.settled().iter().map(|w| w.split_whitespace().count()).sum() })
    }
    fn slowest_pass(&mut self) -> Option<std::time::Duration> {
        self.live.as_ref().and_then(|l| l.slowest)
    }
    fn settling_off_at(&mut self) -> Option<std::time::Instant> {
        self.live.as_ref().and_then(|l| l.off_at)
    }
    fn endpoint(&mut self) -> bool {
        self.ended
    }
    fn reset(&mut self) {
        self.buf.clear();
        self.text.clear();
        self.spoke = false;
        self.quiet = 0;
        self.voiced = 0;
        self.ended = false;
        if let Some(l) = &mut self.live {
            (l.agreement, l.since, l.voiced) = (Default::default(), 0, false);
        }
    }
    fn finish(&mut self) -> String {
        if !self.ended {
            self.close();
        }
        self.text.clone()
    }
}

impl Recognizer for Box<dyn Recognizer> {
    fn accept(&mut self, s: &[f32]) {
        (**self).accept(s)
    }
    fn text(&mut self) -> String {
        (**self).text()
    }
    fn endpoint(&mut self) -> bool {
        (**self).endpoint()
    }
    fn reset(&mut self) {
        (**self).reset()
    }
    fn finish(&mut self) -> String {
        (**self).finish()
    }
    fn failed(&mut self) -> Option<String> {
        (**self).failed()
    }
    fn settled_words(&mut self) -> Option<usize> {
        (**self).settled_words()
    }
    fn heard_after(&mut self, t: f32) -> Option<String> {
        (**self).heard_after(t)
    }
    fn accept_tail(&mut self, s: &[f32]) {
        (**self).accept_tail(s)
    }
    fn slowest_pass(&mut self) -> Option<std::time::Duration> {
        (**self).slowest_pass()
    }
    fn settling_off_at(&mut self) -> Option<std::time::Instant> {
        (**self).settling_off_at()
    }
}

pub fn words_of(r: &sherpa_onnx::OfflineRecognizerResult) -> Result<Vec<crate::dictation_settle::Word>, String> {
    let times = r.timestamps.as_ref().filter(|t| t.len() == r.tokens.len()).ok_or("the model gives no token times")?;
    let mut words: Vec<crate::dictation_settle::Word> = vec![];
    for (token, &t) in r.tokens.iter().zip(times) {
        let piece = token.trim_start_matches(['▁', ' ']);
        match words.last_mut() {
            Some(w) if piece.len() == token.len() => w.text.push_str(piece),
            last => {
                if let Some(w) = last {
                    w.end = t;
                }
                words.push(crate::dictation_settle::Word { text: piece.to_string(), start: t, end: t });
            }
        }
    }
    if let (Some(w), Some(&t)) = (words.last_mut(), times.last()) {
        w.end = t + FRAME;
    }
    words.retain(|w| !w.text.is_empty());
    Ok(words)
}

enum Loaded {
    Online(Arc<sherpa_onnx::OnlineRecognizer>),
    Offline(Arc<sherpa_onnx::OfflineRecognizer>),
}

impl Loaded {

    fn of(m: &Value, dir: &std::path::Path) -> Result<Loaded, String> {
        Ok(match m["kind"].as_str() {
            Some("offline_transducer") => Loaded::Offline(Arc::new(offline_recognizer(dir)?)),
            Some("offline_ctc") => Loaded::Offline(Arc::new(offline_ctc_recognizer(dir)?)),
            _ => Loaded::Online(Arc::new(online_recognizer(dir)?)),
        })
    }

    fn session(&self, lang: &str) -> Box<dyn Recognizer> {
        match self {
            Loaded::Online(r) => Box::new(Online::new(r.clone(), lang)),
            Loaded::Offline(r) => {
                let (r, r2) = (r.clone(), r.clone());
                let decode = move |r: &sherpa_onnx::OfflineRecognizer, audio: &[f32]| {
                    let s = r.create_stream();
                    s.accept_waveform(RATE as i32, audio);
                    r.decode(&s);
                    s.get_result()
                };
                Box::new(
                    Phrases::new(move |audio: &[f32]| decode(&r, audio).map(|x| x.text).unwrap_or_default())
                        .with_passes(Box::new(move |audio: &[f32], _last: bool| {
                            let x = decode(&r2, audio).ok_or("no result")?;
                            Ok(Decoded { words: words_of(&x)?, text: x.text })
                        })),
                )
            }
        }
    }
}

pub struct Installed {
    data: std::path::PathBuf,
    loaded: Option<(String, Loaded)>,
    pub log: Box<dyn Fn(&str) + Send>,
}

impl Installed {
    pub fn new(data: std::path::PathBuf) -> Self {
        Installed { data, loaded: None, log: Box::new(|_| {}) }
    }
}

fn nemo_dir() -> Option<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(d) = std::env::var_os("RTH_DICTATION_NEMO") {
        return Some(d.into());
    }
    crate::settings::CTX.get()?.contents.as_ref().map(|c| c.join("Resources/nemo-speech"))
}

static SIDECAR: OnceLock<(Mutex<crate::nemo::Supervisor>, Arc<AtomicBool>)> = OnceLock::new();

fn supervisor(data: &std::path::Path) -> Result<&'static Mutex<crate::nemo::Supervisor>, String> {
    if let Some(s) = SIDECAR.get() {
        return Ok(&s.0);
    }
    let nemo = nemo_dir().ok_or("this build has no nemo-speech")?;
    let bin = nemo.join("bin/nemo-speech");
    let root = crate::dictation_models::root(data);
    let home = root.join(".nemo-home");
    std::fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let (b2, d2) = (bin.clone(), data.to_path_buf());
    let launch = move |id: &str| -> Result<Box<dyn crate::nemo::Launch>, String> {
        let m = crate::dictation_models::model(crate::dictation_models::catalog(), id).ok_or("not a dictation model")?;
        let (file, _, _) = crate::dictation_models::files(m).next().ok_or("no model file")?;
        Ok(Box::new(crate::nemo::Serve {
            bin: b2.clone(),
            model: crate::dictation_models::dir(&d2, id).join(file),
            itn: (m["itn"] == true).then(|| nemo.join("itn")),
            home: home.clone(),
            warmup: false,
            sandbox: true,
        }))
    };
    let log = |l: &str| {
        if let Some(c) = crate::settings::CTX.get() {
            c.log.line(l)
        }
    };
    let entry = SIDECAR.get_or_init(|| shared(crate::nemo::Supervisor::new(root.join(LEDGER), bin, Box::new(launch), Box::new(log)), QUITTING.load(Ordering::SeqCst)));
    catch_up(&entry.1, QUITTING.load(Ordering::SeqCst));
    Ok(&entry.0)
}

fn catch_up(closing: &AtomicBool, quitting: bool) {
    closing.fetch_or(quitting, Ordering::SeqCst);
}

static QUITTING: AtomicBool = AtomicBool::new(false);

fn shared(s: crate::nemo::Supervisor, quitting: bool) -> (Mutex<crate::nemo::Supervisor>, Arc<AtomicBool>) {
    let closing = s.closing();
    closing.fetch_or(quitting, Ordering::SeqCst);
    (Mutex::new(s), closing)
}

#[cfg(test)]
pub fn install_sidecar_for_test(s: crate::nemo::Supervisor) {
    assert!(SIDECAR.set(shared(s, false)).is_ok(), "one sidecar per test process");
}

const LEDGER: &str = "nemo-speech.ledger";
const OLD_PIDFILE: &str = "nemo-speech.pid";

pub fn quit_sidecar() {
    QUITTING.store(true, Ordering::SeqCst);
    if let Some((_, closing)) = SIDECAR.get() {
        closing.store(true, Ordering::SeqCst);
    }
    stop_sidecar();
}

pub fn stop_sidecar() {
    let Some((s, _)) = SIDECAR.get() else { return };
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match s.try_lock() {
            Ok(mut g) => return g.stop(),
            Err(std::sync::TryLockError::Poisoned(p)) => return p.into_inner().stop(),
            Err(std::sync::TryLockError::WouldBlock) if std::time::Instant::now() < end => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(std::sync::TryLockError::WouldBlock) => return s.lock().unwrap_or_else(|p| p.into_inner()).stop(),
        }
    }
}

pub fn stop_leftover_sidecar(data: &std::path::Path) {
    let root = crate::dictation_models::root(data);
    for f in [OLD_PIDFILE, LEDGER] {
        crate::nemo::stop_leftover(&root.join(f), &|l| {
            if let Some(c) = crate::settings::CTX.get() {
                c.log.line(l)
            }
        });
    }
}

pub fn prepare(data: &std::path::Path, m: &Value) {
    let listening = || WORKER.get().is_some_and(|(_, a)| a.load(Ordering::SeqCst));
    if listening() {
        return;
    }
    let t = std::time::Instant::now();
    let r = nemo_session(data, m, &crate::dictation_models::dir(data, m["id"].as_str().unwrap_or("")), "").map(|mut rec| {
        rec.accept(&[0.0; RATE / 2]);
        rec.finish();
    });
    if let Some(c) = crate::settings::CTX.get() {
        c.log.line(&format!("dictation: GPU engine prepared in {:.1} s{}", t.elapsed().as_secs_f64(), r.err().map(|e| format!(" (failed: {e})")).unwrap_or_default()));
    }
    if !listening() {
        stop_sidecar();
    }
}

fn nemo_session(data: &std::path::Path, m: &Value, _dir: &std::path::Path, lang: &str) -> Result<Box<dyn Recognizer>, String> {
    let id = m["id"].as_str().unwrap_or("");
    let lang = if m["lang_option"] == true { lang } else { "" };
    let mut sup = supervisor(data)?.lock().unwrap();
    let side = sup.sidecar(id)?;
    if m["kind"] == "offline" {
        let (port, key) = side.endpoint();
        return Ok(posted(port, key, lang));
    }

    #[cfg(feature = "voice_pane")]
    if crate::conversation::active() {
        let ms = (CONVERSATION_TRAILING_SILENCE_SECS * 1000.0).round() as u32;
        return Ok(Box::new(side.settled_stream_with_endpoint(lang, Some(ms))?));
    }
    Ok(Box::new(side.settled_stream(lang)?))
}

pub(crate) fn posted(port: u16, key: String, lang: &str) -> Box<dyn Recognizer> {
    posted_within(port, key, lang, crate::nemo::LAST_PASS_WITHIN)
}

pub(crate) fn posted_within(port: u16, key: String, lang: &str, within: std::time::Duration) -> Box<dyn Recognizer> {
    let (lang, err) = (lang.to_string(), Arc::new(Mutex::new(None::<String>)));
    let e2 = err.clone();
    let (pass_key, pass_lang) = (key.clone(), lang.clone());

    let ends = Arc::new(Mutex::new(None::<std::time::Instant>));
    let ends2 = ends.clone();
    let phrases = Phrases::new(move |audio: &[f32]| {
        let wait = ends.lock().unwrap().take().map_or(within, |at| at.saturating_duration_since(std::time::Instant::now()));
        let posted = if wait.is_zero() { Err(format!("nemo-speech: no answer within {} s", within.as_secs())) } else { crate::nemo::transcribe(port, &key, &lang, audio, wait) };
        match posted {
            Ok(t) => t,
            Err(e) => {
                e2.lock().unwrap().get_or_insert(e);
                String::new()
            }
        }
    })

    .with_passes(Box::new(move |audio: &[f32], last: bool| {
        if !last {
            return crate::nemo::pass_decode(port, &pass_key, &pass_lang, audio, crate::nemo::PASS_WITHIN);
        }
        let at = std::time::Instant::now() + within;
        let d = crate::nemo::pass_decode(port, &pass_key, &pass_lang, audio, within);
        *ends2.lock().unwrap() = d.is_err().then_some(at);
        d
    }));
    Box::new(Posted { phrases, err })
}

struct Posted<D: FnMut(&[f32]) -> String> {
    phrases: Phrases<D>,
    err: Arc<Mutex<Option<String>>>,
}

impl<D: FnMut(&[f32]) -> String> Recognizer for Posted<D> {
    fn accept(&mut self, s: &[f32]) {
        self.phrases.accept(s)
    }
    fn text(&mut self) -> String {
        self.phrases.text()
    }
    fn endpoint(&mut self) -> bool {
        self.phrases.endpoint()
    }
    fn reset(&mut self) {
        self.phrases.reset()
    }
    fn finish(&mut self) -> String {
        self.phrases.finish()
    }
    fn failed(&mut self) -> Option<String> {
        self.err.lock().unwrap().clone()
    }
    fn settled_words(&mut self) -> Option<usize> {
        self.phrases.settled_words()
    }
    fn slowest_pass(&mut self) -> Option<std::time::Duration> {
        self.phrases.slowest_pass()
    }
    fn settling_off_at(&mut self) -> Option<std::time::Instant> {
        self.phrases.settling_off_at()
    }
}

impl Models for Installed {
    type R = Box<dyn Recognizer>;

    fn state(&self) -> ModelState { self.state_for(stream_lang()) }

    fn state_for(&self, lang: &str) -> ModelState {
        if crate::dictation_models::ready_for(&self.data, lang).is_some() {
            return ModelState::Ready;
        }
        match crate::dictation_models::installing() {
            Some((id, done, total)) if id == crate::dictation_models::default_id_for(&self.data, lang) => ModelState::Downloading((done * 100 / total.max(1)).min(99) as u8),
            _ => ModelState::Missing,
        }
    }

    fn load(&mut self, lang: &str) -> Result<Box<dyn Recognizer>, String> {
        let (mut m, mut dir) = crate::dictation_models::ready_for(&self.data, lang)
            .ok_or("Install Parakeet v3 in Settings > Dictation for Portuguese, or choose another dictation language.")?;
        if crate::dictation_models::engine(m) == "nemo" {
            let sent = if m["lang_option"] == true { crate::nemo::language(lang) } else { None };
            (self.log)(&format!("dictation: {} on the GPU, language sent: {}", m["id"].as_str().unwrap_or(""), sent.unwrap_or("none (the model detects it)")));
            match nemo_session(&self.data, m, &dir, lang) {
                Ok(r) => return Ok(r),
                Err(e) => {
                    let id = m["id"].as_str().unwrap_or("");
                    let Some(cpu) = crate::dictation_models::fallback(&self.data, id) else { return Err(e) };

                    (self.log)(&format!("dictation: {e}; using {} on the CPU", cpu.0["id"].as_str().unwrap_or("")));
                    (m, dir) = cpu;
                }
            }
        }
        let id = m["id"].as_str().unwrap_or("").to_string();
        #[cfg(feature = "voice_pane")]
        let id = if crate::conversation::active() { format!("{id}:conversation") } else { id };
        if self.loaded.as_ref().is_none_or(|(have, _)| *have != id) {
            self.loaded = None;
            self.loaded = Some((id, Loaded::of(m, &dir)?));
        }

        let sent = if m["lang_option"] == true { lang } else { "" };
        (self.log)(&format!("dictation: {} on the CPU, language sent: {}", m["id"].as_str().unwrap_or(""), if sent.is_empty() { "none (one-language model)" } else { sent }));
        Ok(self.loaded.as_ref().unwrap().1.session(if m["lang_option"] == true { lang } else { "" }))
    }

    fn unload(&mut self) {
        self.loaded = None;
        stop_sidecar();
    }
}

pub struct Linear(sherpa_onnx::LinearResampler);

pub fn resampler(rate: u32) -> Option<Box<dyn Resample>> {
    sherpa_onnx::LinearResampler::create(rate as i32, RATE as i32).map(|r| Box::new(Linear(r)) as Box<dyn Resample>)
}

impl Resample for Linear {
    fn to_16k(&mut self, samples: &[f32]) -> Vec<f32> {
        self.0.resample(samples, false)
    }
}

static WORKER: OnceLock<(SyncSender<Msg>, Active)> = OnceLock::new();

const IDLE: std::time::Duration = std::time::Duration::from_secs(600);

fn worker(app: &tauri::AppHandle, c: &'static crate::settings::Ctx) -> &'static (SyncSender<Msg>, Active) {
    WORKER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(512);
        let active: Active = Arc::new(AtomicBool::new(false));
        let (tx2, a2, app, data, log) = (tx.clone(), active.clone(), app.clone(), c.data.clone(), c.log.clone());
        std::thread::Builder::new()
            .name("dictation".into())
            .spawn(move || {
                let emit = move |e: &Event| {
                    #[cfg(feature = "voice_pane")]
                    let e = {
                        let mut filtered = e.clone();
                        if !crate::conversation::dictation(&mut filtered) { return; }
                        filtered
                    };
                    use tauri::Manager;
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(format!("window.__rthDictationEvent && window.__rthDictationEvent({})", e.js()));
                    }
                };
                let mut models = Installed::new(data);
                let l2 = log.clone();
                models.log = Box::new(move |m| l2.line(m));
                let mut w = Worker::new(crate::capture::Cpal, models, Box::new(emit), tx2, a2, Box::new(resampler));

                w.log = Box::new(move |m| log.line(m));
                w.starting = Some(&STARTING);
                w.phase = Some(&PHASE);
                w.run(rx, IDLE);
            })
            .expect("dictation thread");
        (tx, active)
    })
}

pub fn available_js() -> Option<String> {
    let c = crate::settings::CTX.get()?;
    let m = |choice: &str| crate::dictation_models::choice_for(choice, stream_lang());
    let choice = |k: &str| {
        m(k).map(|x| json!({"size": format!("{:.2} GB", crate::dictation_models::total_size(x) as f64 / 1e9), "download": format!("{:.2} GB", crate::dictation_models::download_size(x) as f64 / 1e9), "installed": crate::dictation_models::installed(&c.data, x)}))
    };
    let lang = crate::dictation_models::system_lang();
    let e = json!({
        "state": "available",
        "conversation": cfg!(feature = "voice_pane"),
        "ready": crate::dictation_models::ready(&c.data).is_some(),
        "suggest": crate::dictation_models::suggest(lang.as_deref()),
        "choices": {"multi": choice("multi"), "en": choice("en")},
    });

    let mut e = e;
    if let Some(l) = lang {
        e["system_lang"] = json!(l);
    }
    Some(format!("window.__rthDictationEvent && window.__rthDictationEvent({e})"))
}

fn choose(c: &crate::settings::Ctx, choice: &str) -> Result<(), String> {
    if choice == "later" {
        return Ok(());
    }
    let m = crate::dictation_models::choice_for(choice, stream_lang()).ok_or("choose: multi, en or later")?;
    let id = m["id"].as_str().unwrap_or("");
    crate::dictation_models::set_default(&c.data, id)?;
    c.log.line(&format!("dictation: chose {choice} ({id})"));
    if crate::dictation_models::installed(&c.data, m) || crate::dictation_models::installing().is_some_and(|(i, _, _)| i == id) {
        return Ok(());
    }
    crate::dictation_models::start_install(c.data.clone(), id, c.log.clone(), models_changed)
}


pub fn pcm16(b: &[u8]) -> Vec<f32> {
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if &b[i..i + 4] == b"data" {
            return b[i + 8..(i + 8 + len).min(b.len())].chunks_exact(2).map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0).collect();
        }
        i += 8 + len + (len & 1);
    }
    vec![]
}

static TEST: Mutex<Option<(String, Value)>> = Mutex::new(None);

pub fn test_result() -> Option<(String, Value)> {
    TEST.lock().unwrap().clone()
}

pub fn run_test(data: &std::path::Path, m: &Value, dir: &std::path::Path, pt: bool) -> Result<Value, String> {
    let pt = pt && m["choice"] != "en";
    let root = std::env::var_os("UNFOLD_DICTATION_TEST_CLIPS").ok_or("Dictation test audio is not included in this source build")?;
    let (filename, name) = if pt { ("fleurs-pt_br-8821974356183732066.wav", "FLEURS pt-BR") } else { ("fleurs-en_us-9055240296608129734.wav", "FLEURS en-US") };
    let clip = std::fs::read(std::path::PathBuf::from(root).join(filename)).map_err(|_| "Dictation test audio is unavailable")?;
    let audio = pcm16(&clip);
    let t0 = std::time::Instant::now();
    let lang = if m["lang_option"] == true { if pt { "pt" } else { "en" } } else { "" };
    let mut engine = crate::dictation_models::engine(m);
    let rec = match engine {
        "nemo" => match nemo_session(data, m, dir, lang) {
            Ok(r) => r,

            Err(e) => {
                let (fm, fdir) = crate::dictation_models::fallback(data, m["id"].as_str().unwrap_or("")).ok_or(e)?;
                engine = "sherpa";
                Loaded::of(fm, &fdir)?.session(if fm["lang_option"] == true { lang } else { "" })
            }
        },
        _ => Loaded::of(m, dir)?.session(lang),
    };
    let load_s = t0.elapsed().as_secs_f64();
    let mut s = Session::new(rec);
    let t1 = std::time::Instant::now();
    let mut ev = vec![];
    for c in audio.chunks(RATE / 100) {
        ev.extend(s.feed(c));
    }
    ev.extend(s.stop());
    let text = ev.iter().filter_map(|e| if let Event::Text { text, is_final: true, .. } = e { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join(" ");
    let r = |x: f64| (x * 100.0).round() / 100.0;
    Ok(json!({"text": text, "seconds": r(t1.elapsed().as_secs_f64()), "load_s": r(load_s), "audio_s": r(audio.len() as f64 / RATE as f64), "clip": name, "engine": if engine == "nemo" { "gpu" } else { "cpu" }}))
}

pub fn start_test(data: std::path::PathBuf, id: &str, pt: bool, log: crate::Log) -> Result<(), String> {
    if WORKER.get().is_some_and(|(_, a)| a.load(Ordering::SeqCst)) {
        return Err(said(BUSY));
    }
    let m = crate::dictation_models::model(crate::dictation_models::catalog(), id).filter(|m| crate::dictation_models::installed(&data, m)).ok_or("not installed")?;
    let id = id.to_string();
    if TEST.lock().unwrap().as_ref().is_some_and(|t| t.1["running"] == true) {
        return Ok(());
    }
    *TEST.lock().unwrap() = Some((id.clone(), json!({"running": true})));
    models_changed();
    std::thread::Builder::new()
        .name("dictation-test".into())
        .spawn(move || {
            let r = run_test(&data, m, &crate::dictation_models::dir(&data, &id), pt);

            log.line(&format!("dictation: test of {id}: {}", if r.is_ok() { "ran" } else { "failed" }));
            *TEST.lock().unwrap() = Some((id, r.unwrap_or_else(|e| json!({"error": e}))));
            models_changed();
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn page_loaded(w: &tauri::WebviewWindow) {
    use tauri::Manager;
    let _ = APP.set(w.app_handle().clone());
    if let Some(js) = available_js() {
        let _ = w.eval(js);
    }
}

pub fn models_changed() {
    crate::settings::refresh_open();
    if let (Some(app), Some(js)) = (APP.get(), available_js()) {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.eval(js);
        }
    }
}

pub fn page_gone() {
    #[cfg(feature = "voice_pane")]
    if crate::conversation::active() { crate::conversation::stop(); }
    if let Some((tx, active)) = WORKER.get() {
        if active.load(Ordering::SeqCst) {
            let _ = tx.send(Msg::Cancel);
        }
    }
}

pub(crate) fn stream_lang() -> &'static str {
    stream_lang_of(&crate::settings::current(), &crate::system_languages())
}

fn stream_lang_of(s: &crate::settings::Settings, preferred: &[String]) -> &'static str {
    match s.dictation_language.as_str() {
        "pt" => return "pt",
        "en" => return "en",
        _ => {}
    }
    let brazil = |t: &str| t.split(['-', '_']).skip(1).any(|r| r.eq_ignore_ascii_case("BR"));
    let first = preferred.first().map(String::as_str);
    if crate::settings::is_pt(first) || (first.is_some_and(brazil) && preferred.iter().any(|t| crate::settings::is_pt(Some(t)))) {
        "pt"
    } else if preferred.first().is_some_and(|t| t.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("en")) && t.get(2..3).is_none_or(|c| c == "-" || c == "_")) {
        "en"
    } else {
        "auto"
    }
}

#[derive(Debug, PartialEq)]
pub enum StartReply {

    Start,

    Cancel,

    Busy,
}

pub fn start_reply(active: &AtomicBool, starting: &AtomicBool) -> StartReply {
    if !active.swap(true, Ordering::SeqCst) {
        starting.store(true, Ordering::SeqCst);
        return StartReply::Start;
    }
    if starting.load(Ordering::SeqCst) {
        StartReply::Cancel
    } else {
        StartReply::Busy
    }
}

static STARTING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "voice_pane")]
pub fn cancel_native() {
    if let Some((tx, active)) = WORKER.get() {
        if !abort_load(&PHASE, active, false) { let _ = tx.try_send(Msg::Cancel); }
    }
}

#[cfg(feature = "voice_pane")]
pub fn conversation_control(app: &tauri::AppHandle, end_active: bool) -> Result<(), String> {
    if end_active && crate::conversation::active() { crate::conversation::stop(); return Ok(()); }
    let c = crate::settings::CTX.get().ok_or_else(|| said(UNAVAILABLE))?;
    if crate::voice::STUDIO.recording() { return Err(said(VOICE_TAKE)); }
    if crate::conversation::toggle(app)? {
        crate::settings::reply_dictation();
        let (tx, active) = worker(app, c);
        match start_reply(active, &STARTING) {
            StartReply::Start => {
                if tx.send(Msg::ConversationStart { lang: stream_lang().into(),epoch:crate::conversation::snapshot().1 }).is_err() {
                    crate::conversation::stop(); return Err(said(UNAVAILABLE));
                }
            }
            _ => { crate::conversation::stop(); return Err(said(BUSY)); }
        }
    }
    Ok(())
}

#[tauri::command(async)]
pub fn rth_dictation(webview: tauri::Webview, action: String, choice: Option<String>) -> Result<(), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if webview.label() != "main" || url.scheme() != "tauri" || url.host_str() != Some("localhost") {
        return Err("dictation is only available from Unfold's own page".into());
    }
    use tauri::Manager;
    let c = crate::settings::CTX.get().ok_or_else(|| said(UNAVAILABLE))?;
    let app = webview.app_handle();
    match action.as_str() {
        #[cfg(feature = "voice_pane")]
        "conversation" => conversation_control(app, false),
        #[cfg(feature = "voice_pane")]
        "conversation_send" => crate::conversation::send_current(choice.as_deref().unwrap_or("")),
        #[cfg(feature = "voice_pane")]
        "conversation_stop" => { crate::conversation::stop(); Ok(()) }
        "start" => {
            #[cfg(feature = "voice_pane")]
            if crate::conversation::active() {
                crate::conversation::barge(); return Ok(());
            }
            #[cfg(feature = "voice_pane")]
            if crate::voice::STUDIO.recording() {
                return Err(said(VOICE_TAKE));
            }

            #[cfg(feature = "voice_pane")]
            crate::settings::reply_dictation();
            let (tx, active) = worker(app, c);
            match start_reply(active, &STARTING) {
                StartReply::Start => tx.send(Msg::Start { lang: stream_lang().into() }).map_err(|_| said(UNAVAILABLE)),

                StartReply::Cancel => tx.send(Msg::Cancel).map_err(|_| said(UNAVAILABLE)),
                StartReply::Busy => Err(said(BUSY)),
            }
        }
        "stop" | "cancel" => {
            #[cfg(feature = "voice_pane")]
            if crate::conversation::active() { crate::conversation::stop(); return Ok(()); }
            if let Some((tx, active)) = WORKER.get() {
                if abort_load(&PHASE, active, action == "stop") {

                    let (e, path) = if action == "stop" { (Event::Finishing, "words kept, transcribed after the load") } else { (Event::Cancelled, "discarded") };
                    let _ = webview.eval(format!("window.__rthDictationEvent && window.__rthDictationEvent({})", e.js()));
                    c.log.line(&format!("dictation: {action} during the model load ({path})"));
                } else {
                    let _ = tx.send(if action == "stop" { Msg::Stop } else { Msg::Cancel });
                }
            }
            Ok(())
        }
        "open_mic_settings" => crate::capture::open_mic_settings().map_err(|e| {
            c.log.line(&format!("dictation: System Settings did not open: {e}"));
            said(UNAVAILABLE)
        }),
        "open_settings" => {
            let h = app.clone();
            app.run_on_main_thread(move || crate::settings::open_at(&h, Some("dictation"))).map_err(|e| e.to_string())
        }
        "choose" => choose(c, choice.as_deref().unwrap_or("")).map_err(|e| {
            c.log.line(&format!("dictation: choose: {e}"));
            said(CHOOSE_FAILED)
        }),
        _ => Err("unknown dictation action".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn a_supervisor_made_after_the_quit_began_starts_nothing() {

        let dir = std::env::temp_dir().join(format!("rth-dictation-after-quit-{}", std::process::id()));
        let s = crate::nemo::Supervisor::new(dir.join("ledger"), "/bin/sleep".into(), Box::new(|_| Err("no launch".into())), Box::new(|_| {}));
        let (m, closing) = shared(s, true);
        assert!(closing.load(Ordering::SeqCst));
        let e = m.lock().unwrap().sidecar("m").err().unwrap();
        assert!(e.contains("quitting"), "{e}");
        let s = crate::nemo::Supervisor::new(dir.join("ledger"), "/bin/sleep".into(), Box::new(|_| Err("no launch".into())), Box::new(|_| {}));
        assert!(!shared(s, false).1.load(Ordering::SeqCst), "not quitting: open");
    }

    #[test]
    fn a_quit_that_lands_while_the_supervisor_is_made_still_closes_it() {

        let dir = std::env::temp_dir().join(format!("rth-dictation-quit-race-{}", std::process::id()));
        let s = crate::nemo::Supervisor::new(dir.join("ledger"), "/bin/sleep".into(), Box::new(|_| Err("no launch".into())), Box::new(|_| {}));
        let (m, closing) = shared(s, false);
        catch_up(&closing, true);
        assert!(closing.load(Ordering::SeqCst));
        assert!(m.lock().unwrap().sidecar("m").err().unwrap().contains("quitting"));
        catch_up(&closing, false);
        assert!(closing.load(Ordering::SeqCst), "never reopened");
    }

    #[test]
    fn the_stream_language_follows_the_preferred_list_unless_the_person_picked_one() {
        let tags = |t: &[&str]| t.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut s = crate::settings::Settings { language: "en".into(), ..Default::default() };

        assert_eq!(stream_lang_of(&s, &tags(&["en-BR", "pt-BR"])), "pt");
        assert_eq!(stream_lang_of(&s, &tags(&["pt-BR", "en-US"])), "pt", "Portuguese first");
        assert_eq!(stream_lang_of(&s, &tags(&["pt"])), "pt");

        assert_eq!(stream_lang_of(&s, &tags(&["en-US", "pt-BR"])), "en");
        assert_eq!(stream_lang_of(&s, &tags(&["en-US", "fr-FR", "pt-PT"])), "en");
        assert_eq!(stream_lang_of(&s, &tags(&["en-BR"])), "en", "Brazil alone is not Portuguese");
        assert_eq!(stream_lang_of(&s, &tags(&["es-BR"])), "auto");
        assert_eq!(stream_lang_of(&s, &tags(&["en-US", "fr-FR"])), "en");
        assert_eq!(stream_lang_of(&s, &tags(&["fr-FR", "en-US"])), "auto");
        assert_eq!(stream_lang_of(&s, &tags(&["eng"])), "auto", "a tag that only starts with en");
        assert_eq!(stream_lang_of(&s, &[]), "auto");
        assert_eq!(crate::nemo::language("auto"), None, "auto sends no language");

        s.dictation_language = "en".into();
        assert_eq!(stream_lang_of(&s, &tags(&["en-BR", "pt-BR"])), "en");
        s.dictation_language = "pt".into();
        assert_eq!(stream_lang_of(&s, &tags(&["fr-FR"])), "pt");
    }

    struct Script {
        steps: Vec<(&'static str, bool)>,
        at: usize,
        cur: String,
        end: bool,
        tail: &'static str,
        fed: Rc<RefCell<usize>>,
    }
    impl Recognizer for Script {
        fn accept(&mut self, s: &[f32]) {
            *self.fed.borrow_mut() += s.len();
            if let Some((t, e)) = self.steps.get(self.at) {
                self.cur = t.to_string();
                self.end = *e;
                self.at += 1;
            }
        }
        fn text(&mut self) -> String {
            self.cur.clone()
        }
        fn endpoint(&mut self) -> bool {
            self.end
        }
        fn reset(&mut self) {
            self.cur.clear();
            self.end = false;
        }
        fn failed(&mut self) -> Option<String> {

            (self.cur == "GONE").then(|| "the sidecar closed the socket".into())
        }
        fn finish(&mut self) -> String {
            if !self.tail.is_empty() {
                self.cur = self.tail.to_string();
            }
            self.cur.clone()
        }
    }
    fn script(steps: Vec<(&'static str, bool)>, tail: &'static str) -> Script {
        Script { steps, at: 0, cur: String::new(), end: false, tail, fed: Rc::default() }
    }
    fn texts(ev: &[Event]) -> Vec<(u64, String, bool)> {
        ev.iter().filter_map(|e| if let Event::Text { seq, text, is_final, .. } = e { Some((*seq, text.clone(), *is_final)) } else { None }).collect()
    }
    const CHUNK: usize = 2560;

    #[test]
    fn partials_then_a_final_at_each_pause_with_seq_growing_by_one() {
        let mut s =
            Session::new(script(vec![("", false), ("hello", false), ("hello", false), ("hello world", false), ("hello world", true), ("how", false), ("how are you", false)], "how are you today"));
        let mut ev = vec![];
        for _ in 0..7 {
            ev.extend(s.feed(&[0.0; CHUNK]));
        }
        ev.extend(s.stop());
        assert_eq!(
            texts(&ev),
            vec![
                (1, "hello".into(), false),
                (2, "hello world".into(), false),
                (3, "hello world".into(), true),
                (4, "how".into(), false),
                (5, "how are you".into(), false),
                (6, "how are you today".into(), true),
            ]
        );

        assert_eq!(ev.last(), Some(&Event::Stopped));
    }

    #[test]
    fn a_partial_never_follows_its_own_final() {

        let mut s = Session::new(script(vec![("one", false), ("one two", true), ("", false), ("", false), ("three", false)], ""));
        let mut ev = vec![];
        for _ in 0..5 {
            ev.extend(s.feed(&[0.0; CHUNK]));
        }
        assert_eq!(texts(&ev), vec![(1, "one".into(), false), (2, "one two".into(), true), (3, "three".into(), false)]);
        ev = s.stop();
        assert_eq!(texts(&ev), vec![(4, "three".into(), true)]);
    }

    #[test]
    fn an_endpoint_in_silence_sends_no_empty_final() {
        let mut s = Session::new(script(vec![("", true), ("", false)], ""));
        let mut ev = s.feed(&[0.0; CHUNK]);
        ev.extend(s.feed(&[0.0; CHUNK]));
        ev.extend(s.stop());
        assert!(texts(&ev).is_empty(), "{ev:?}");
        assert_eq!(ev.last(), Some(&Event::Stopped));
    }

    #[test]
    fn stop_feeds_a_tail_of_silence_before_the_last_decode() {
        let r = script(vec![("a", false)], "a b");
        let fed = r.fed.clone();
        let mut s = Session::new(r);
        s.feed(&[0.0; CHUNK]);
        let ev = s.stop();
        assert_eq!(*fed.borrow(), CHUNK + TAIL);
        assert_eq!(texts(&ev), vec![(2, "a b".into(), true)]);
    }

    #[test]
    fn cancel_discards_the_text_and_sends_only_cancelled() {
        let mut s = Session::new(script(vec![("dictated words", false)], "dictated words here"));
        let first = s.feed(&[0.0; CHUNK]);
        assert_eq!(texts(&first).len(), 1);
        assert_eq!(s.cancel(), vec![Event::Cancelled]);
    }

    #[test]
    fn the_level_comes_every_50_ms_and_the_tick_every_second() {
        let mut s = Session::new(script(vec![], ""));
        let mut ev = vec![];

        let tone: Vec<f32> = (0..RATE * 6 / 5).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        for c in tone.chunks(333) {
            ev.extend(s.feed(c));
        }
        let levels: Vec<f32> = ev.iter().filter_map(|e| if let Event::Level(v) = e { Some(*v) } else { None }).collect();
        assert_eq!(levels.len(), 24);
        assert!(levels.iter().all(|v| (v - 0.8).abs() < 0.01), "{levels:?}");
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::Tick(_))).cloned().collect::<Vec<_>>(), vec![Event::Tick(1)]);
        assert_eq!(level_of(0.0, 800), 0.0);
        assert_eq!(level_of(800.0, 800), 1.0);
    }

    struct FakeMic {
        perm: Permission,
        opened: Rc<RefCell<u32>>,
        open_fails: Option<&'static str>,
        asked: Rc<RefCell<u32>>,

        pushes: usize,
    }
    impl Mic for FakeMic {
        fn permission(&self) -> Permission {
            self.perm
        }
        fn ask(&self, _answer: Box<dyn FnOnce(bool) + Send>) {
            *self.asked.borrow_mut() += 1;
        }
        fn open(&mut self, mut sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError> {
            for _ in 0..self.pushes {
                sink(vec![0.0; 160]);
            }
            match self.open_fails {
                Some("denied") => Err(MicError::Denied),
                Some(e) => Err(MicError::Other(e.into())),
                None => {
                    *self.opened.borrow_mut() += 1;
                    Ok((Box::new(()), 16_000, "Fake mic, 1 ch".into()))
                }
            }
        }
    }
    struct FakeModels {
        state: ModelState,
        loads: Rc<RefCell<Vec<String>>>,
        steps: Vec<(&'static str, bool)>,
    }
    impl Models for FakeModels {
        type R = Script;
        fn state(&self) -> ModelState {
            self.state
        }
        fn load(&mut self, lang: &str) -> Result<Script, String> {
            self.loads.borrow_mut().push(lang.into());
            Ok(script(self.steps.clone(), ""))
        }
    }
    struct Rig {
        w: Worker<FakeMic, FakeModels>,
        ev: Rc<RefCell<Vec<Event>>>,
        opened: Rc<RefCell<u32>>,
        asked: Rc<RefCell<u32>>,
        loads: Rc<RefCell<Vec<String>>>,
        active: Active,
    }
    fn rig(perm: Permission, state: ModelState, open_fails: Option<&'static str>) -> Rig {
        let ev: Rc<RefCell<Vec<Event>>> = Rc::default();
        let (opened, asked, loads): (Rc<RefCell<u32>>, Rc<RefCell<u32>>, Rc<RefCell<Vec<String>>>) = Default::default();
        let (tx, _rx) = std::sync::mpsc::sync_channel(4);
        let active: Active = Arc::new(AtomicBool::new(true));
        let e2 = ev.clone();
        let w = Worker::new(
            FakeMic { perm, opened: opened.clone(), open_fails, asked: asked.clone(), pushes: 0 },
            FakeModels { state, loads: loads.clone(), steps: vec![("hi", false), ("hi there", true)] },
            Box::new(move |e| e2.borrow_mut().push(e.clone())),
            tx,
            active.clone(),
            Box::new(|_| None),
        );

        Rig { w, ev, opened, asked, loads, active }
    }
    fn start(r: &mut Rig) {
        r.w.handle(Msg::Start { lang: "pt".into() });
    }

    struct StuckMic {
        release: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>,
        opens: Arc<std::sync::atomic::AtomicU32>,
        closed: Arc<AtomicBool>,
    }
    struct Closes(Arc<AtomicBool>);
    impl Drop for Closes {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl StuckMic {
        fn open_now(release: Arc<Mutex<Option<std::sync::mpsc::Receiver<()>>>>, opens: Arc<std::sync::atomic::AtomicU32>, closed: Arc<AtomicBool>) -> Result<(Box<dyn Any + Send>, u32, String), MicError> {
            opens.fetch_add(1, Ordering::SeqCst);
            if let Some(rx) = release.lock().unwrap().take() {
                let _ = rx.recv();
            }
            Ok((Box::new(Closes(closed)), 48_000, "Stuck mic, 1 ch".into()))
        }
    }
    impl Mic for StuckMic {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, _sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError> {
            Self::open_now(self.release.clone(), self.opens.clone(), self.closed.clone()).map(|(g, r, d)| (g as Box<dyn Any>, r, d))
        }
        fn opener(&self) -> Option<Opener> {
            let (release, opens, closed) = (self.release.clone(), self.opens.clone(), self.closed.clone());
            Some(Box::new(move |_sink| Self::open_now(release, opens, closed)))
        }
    }

    #[allow(clippy::type_complexity)]
    fn stuck_rig() -> (std::sync::mpsc::Receiver<Event>, SyncSender<Msg>, std::sync::mpsc::Sender<()>, Arc<std::sync::atomic::AtomicU32>, Arc<AtomicBool>, Active) {
        let (etx, erx) = std::sync::mpsc::channel::<Event>();
        let (mtx, mrx) = std::sync::mpsc::sync_channel::<Msg>(16);
        let (rtx, rrx) = std::sync::mpsc::channel::<()>();
        let (opens, closed): (Arc<std::sync::atomic::AtomicU32>, Arc<AtomicBool>) = Default::default();
        let active: Active = Arc::new(AtomicBool::new(true));
        let (o2, c2, a2, m2) = (opens.clone(), closed.clone(), active.clone(), mtx.clone());
        std::thread::spawn(move || {
            let mic = StuckMic { release: Arc::new(Mutex::new(Some(rrx))), opens: o2, closed: c2 };
            let models = FakeModels { state: ModelState::Ready, loads: Rc::default(), steps: vec![] };
            let mut w = Worker::new(mic, models, Box::new(move |e: &Event| drop(etx.send(e.clone()))), m2, a2, Box::new(|_| None));
            w.open_within = std::time::Duration::from_millis(300);
            while let Ok(m) = mrx.recv() {
                w.handle(m);
            }
        });
        (erx, mtx, rtx, opens, closed, active)
    }

    fn within<T>(rx: &std::sync::mpsc::Receiver<T>, secs: u64) -> Option<T> {
        rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
    }

    #[test]
    fn a_mic_that_does_not_start_ends_the_dictation_with_an_error_in_time() {
        let (ev, tx, release, opens, closed, active) = stuck_rig();
        let t = std::time::Instant::now();
        tx.send(Msg::Start { lang: "pt".into() }).unwrap();
        let e = within(&ev, 3);

        let _ = release.send(());
        assert!(matches!(&e, Some(Event::Error(m)) if m == &said(MIC_STUCK)), "no error within 3 s: the worker hung on the mic ({e:?})");
        assert!(t.elapsed() < std::time::Duration::from_secs(2), "{:?}", t.elapsed());
        assert!(!active.load(Ordering::SeqCst), "the next press starts again");

        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !closed.load(Ordering::SeqCst) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(closed.load(Ordering::SeqCst), "the late mic stream was left open");
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn while_a_stuck_mic_open_still_runs_a_new_press_fails_at_once() {
        let (ev, tx, release, opens, _closed, active) = stuck_rig();
        tx.send(Msg::Start { lang: "pt".into() }).unwrap();
        let first = within(&ev, 3);
        active.store(true, Ordering::SeqCst);
        let t = std::time::Instant::now();
        tx.send(Msg::Start { lang: "pt".into() }).unwrap();
        let second = within(&ev, 3);
        let _ = release.send(());
        assert!(matches!(&first, Some(Event::Error(_))), "{first:?}");
        assert!(matches!(&second, Some(Event::Error(m)) if m == &said(MIC_STUCK)), "{second:?}");
        assert!(t.elapsed() < std::time::Duration::from_millis(250), "the second press waited: {:?}", t.elapsed());
        assert_eq!(opens.load(Ordering::SeqCst), 1, "no second CoreAudio open while the first still runs");
    }

    #[test]
    fn an_engine_gone_mid_dictation_ends_it_with_an_error_and_closes_the_mic() {
        let mut r = rig(Permission::Granted, ModelState::Ready, None);
        r.w.models.steps = vec![("hi", false), ("GONE", false), ("never", false)];
        start(&mut r);
        r.w.handle(Msg::Audio(vec![0.0; 160]));
        r.w.handle(Msg::Audio(vec![0.0; 160]));
        r.w.handle(Msg::Audio(vec![0.0; 160]));
        let ev = r.ev.borrow().clone();
        let words: Vec<_> = ev.iter().filter_map(|e| if let Event::Text { text, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(words, ["hi", "GONE"]);
        assert!(matches!(ev.last(), Some(Event::Error(m)) if m.starts_with("Dictation stopped")), "{ev:?}");
        assert!(!r.active.load(Ordering::SeqCst) && !r.w.listening(), "the session ended; the next press starts again");
    }

    #[test]
    fn the_mic_opens_before_the_model_and_what_it_hears_meanwhile_is_kept() {
        struct SeesMic {
            opened: Rc<RefCell<u32>>,
            saw: Rc<RefCell<Option<u32>>>,
            fed: Rc<RefCell<usize>>,
        }
        impl Models for SeesMic {
            type R = Script;
            fn state(&self) -> ModelState {
                ModelState::Ready
            }
            fn load(&mut self, _: &str) -> Result<Script, String> {
                *self.saw.borrow_mut() = Some(*self.opened.borrow());
                let mut s = script(vec![("hi", false)], "");
                s.fed = self.fed.clone();
                Ok(s)
            }
        }
        let ev: Rc<RefCell<Vec<Event>>> = Rc::default();
        let (opened, saw, fed): (Rc<RefCell<u32>>, Rc<RefCell<Option<u32>>>, Rc<RefCell<usize>>) = Default::default();
        let lines: Rc<RefCell<Vec<String>>> = Rc::default();
        let (e2, l2) = (ev.clone(), lines.clone());
        let (tx, _rx) = std::sync::mpsc::sync_channel(4);
        let mic = FakeMic { perm: Permission::Granted, opened: opened.clone(), open_fails: None, asked: Rc::default(), pushes: 3 };
        let mut w = Worker::new(mic, SeesMic { opened: opened.clone(), saw: saw.clone(), fed: fed.clone() }, Box::new(move |e| e2.borrow_mut().push(e.clone())), tx, Arc::new(AtomicBool::new(true)), Box::new(|_| None));
        w.log = Box::new(move |m| l2.borrow_mut().push(m.to_string()));
        w.handle(Msg::Start { lang: "pt".into() });
        assert_eq!(*saw.borrow(), Some(1), "the model loaded before the mic opened");
        assert_eq!(*fed.borrow(), 3 * 160, "the audio heard while the model loaded was lost");
        let got = lines.borrow().clone();
        assert!(got.iter().any(|l| l.starts_with("dictation: mic opened in ") && l.contains(" ms, before the model")), "{got:?}");
        assert!(got.iter().any(|l| l.starts_with("dictation: listening (pt, 16000 Hz, Fake mic, 1 ch); the model was ready ")), "{got:?}");
        assert!(matches!(ev.borrow().first(), Some(Event::Listening)), "{:?}", ev.borrow());
    }

    struct ClosingMic {
        opens: Arc<std::sync::atomic::AtomicU32>,
        closed: Arc<AtomicBool>,
    }
    impl Mic for ClosingMic {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, mut sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            self.closed.store(false, Ordering::SeqCst);
            sink(vec![0.1; 160]);
            Ok((Box::new(Closes(self.closed.clone())), 16_000, "Fake mic, 1 ch".into()))
        }
    }

    struct SlowModels {
        release: std::sync::mpsc::Receiver<()>,
    }
    impl Models for SlowModels {
        type R = Script;
        fn state(&self) -> ModelState {
            ModelState::Ready
        }
        fn load(&mut self, _: &str) -> Result<Script, String> {
            let _ = self.release.recv_timeout(std::time::Duration::from_secs(10));
            Ok(script(vec![("late words", true)], ""))
        }
    }

    #[test]
    fn a_stop_during_the_model_load_marks_it_stopped_and_an_escape_aborted() {
        for (keep, want) in [(true, STOPPED), (false, ABORTED)] {
            let (phase, active) = (AtomicU8::new(LOADING), AtomicBool::new(true));
            assert!(abort_load(&phase, &active, keep));
            assert_eq!((phase.load(Ordering::SeqCst), active.load(Ordering::SeqCst)), (want, false), "keep {keep}");
        }
    }

    #[test]
    fn a_stop_during_the_model_load_still_transcribes_what_was_heard() {
        let phase: &'static AtomicU8 = Box::leak(Box::new(AtomicU8::new(IDLE_PHASE)));
        let (etx, erx) = std::sync::mpsc::channel::<Event>();
        let (mtx, mrx) = std::sync::mpsc::sync_channel::<Msg>(16);
        let (rtx, rrx) = std::sync::mpsc::channel::<()>();
        let (opens, closed): (Arc<std::sync::atomic::AtomicU32>, Arc<AtomicBool>) = Default::default();
        let active: Active = Arc::new(AtomicBool::new(true));
        let (o2, c2, a2, m2) = (opens.clone(), closed.clone(), active.clone(), mtx.clone());
        std::thread::spawn(move || {
            let mut w = Worker::new(ClosingMic { opens: o2, closed: c2 }, SlowModels { release: rrx }, Box::new(move |e: &Event| drop(etx.send(e.clone()))), m2, a2, Box::new(|_| None));
            w.phase = Some(phase);
            while let Ok(m) = mrx.recv() {
                w.handle(m);
            }
        });
        mtx.send(Msg::Start { lang: "pt".into() }).unwrap();
        assert_eq!(within(&erx, 3), Some(Event::Listening));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while phase.load(Ordering::SeqCst) != LOADING && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(abort_load(phase, &active, true));

        active.store(true, Ordering::SeqCst);
        rtx.send(()).unwrap();
        let mut late = vec![];
        while let Some(e) = within(&erx, 3) {
            let stop = e == Event::Stopped;
            late.push(e);
            if stop {
                break;
            }
        }
        let finals: Vec<_> = late.iter().filter_map(|e| if let Event::Text { text, is_final: true, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(finals, ["late words"], "{late:?}");
        assert_eq!(late.last(), Some(&Event::Stopped), "{late:?}");
        assert!(closed.load(Ordering::SeqCst), "the mic stayed open");
        assert!(active.load(Ordering::SeqCst), "the late transcription ended the next dictation");
        assert_eq!(phase.load(Ordering::SeqCst), IDLE_PHASE);
    }

    type Held = Arc<Mutex<Option<Box<dyn FnMut(Vec<f32>) + Send>>>>;
    struct HeldMic(Held);
    impl Mic for HeldMic {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, mut sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError> {
            sink(vec![0.1; 8_000]);
            *self.0.lock().unwrap() = Some(sink);
            Ok((Box::new(()), 16_000, "Fake mic, 1 ch".into()))
        }
    }

    #[test]
    fn a_stop_during_the_model_load_keeps_only_what_was_heard_before_it() {
        let phase: &'static AtomicU8 = Box::leak(Box::new(AtomicU8::new(IDLE_PHASE)));
        let (etx, erx) = std::sync::mpsc::channel::<Event>();
        let (mtx, mrx) = std::sync::mpsc::sync_channel::<Msg>(16);
        let (rtx, rrx) = std::sync::mpsc::channel::<()>();
        let held: Held = Arc::default();
        let lines: Arc<Mutex<Vec<String>>> = Arc::default();
        let active: Active = Arc::new(AtomicBool::new(true));
        let (h2, l2, a2, m2) = (held.clone(), lines.clone(), active.clone(), mtx.clone());
        std::thread::spawn(move || {
            let mut w = Worker::new(HeldMic(h2), SlowModels { release: rrx }, Box::new(move |e: &Event| drop(etx.send(e.clone()))), m2, a2, Box::new(|_| None));
            w.phase = Some(phase);
            w.log = Box::new(move |m| l2.lock().unwrap().push(m.to_string()));
            while let Ok(m) = mrx.recv() {
                w.handle(m);
            }
        });
        mtx.send(Msg::Start { lang: "pt".into() }).unwrap();
        assert_eq!(within(&erx, 3), Some(Event::Listening));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while phase.load(Ordering::SeqCst) != LOADING && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(abort_load(phase, &active, true));

        (held.lock().unwrap().as_mut().unwrap())(vec![0.1; 16_000]);
        rtx.send(()).unwrap();
        while let Some(e) = within(&erx, 3) {
            if e == Event::Stopped {
                break;
            }
        }
        let lines = lines.lock().unwrap().clone();
        assert!(lines.iter().any(|l| l.contains("stop during the model load: 0.5 s heard before it")), "{lines:?}");
    }

    #[test]
    fn an_escape_during_the_model_load_ends_the_dictation_at_once_and_the_late_load_is_dropped() {
        let phase: &'static AtomicU8 = Box::leak(Box::new(AtomicU8::new(IDLE_PHASE)));
        let (etx, erx) = std::sync::mpsc::channel::<Event>();
        let (mtx, mrx) = std::sync::mpsc::sync_channel::<Msg>(16);
        let (rtx, rrx) = std::sync::mpsc::channel::<()>();
        let (opens, closed): (Arc<std::sync::atomic::AtomicU32>, Arc<AtomicBool>) = Default::default();
        let active: Active = Arc::new(AtomicBool::new(true));
        let (o2, c2, a2, m2) = (opens.clone(), closed.clone(), active.clone(), mtx.clone());
        std::thread::spawn(move || {
            let mut w = Worker::new(ClosingMic { opens: o2, closed: c2 }, SlowModels { release: rrx }, Box::new(move |e: &Event| drop(etx.send(e.clone()))), m2, a2, Box::new(|_| None));
            w.phase = Some(phase);
            while let Ok(m) = mrx.recv() {
                w.handle(m);
            }
        });
        mtx.send(Msg::Start { lang: "pt".into() }).unwrap();
        assert_eq!(within(&erx, 3), Some(Event::Listening));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while phase.load(Ordering::SeqCst) != LOADING && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert!(abort_load(phase, &active, false), "no load in progress");
        assert!(!active.load(Ordering::SeqCst), "the next press would be busy");
        rtx.send(()).unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !closed.load(Ordering::SeqCst) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(closed.load(Ordering::SeqCst), "the mic stayed open after the load the page had stopped");

        assert_eq!(within(&erx, 1), None);
        assert_eq!(phase.load(Ordering::SeqCst), IDLE_PHASE);

        active.store(true, Ordering::SeqCst);
        mtx.send(Msg::Start { lang: "pt".into() }).unwrap();
        assert_eq!(within(&erx, 3), Some(Event::Listening));
        assert_eq!(opens.load(Ordering::SeqCst), 2);
        rtx.send(()).unwrap();
    }

    #[test]
    fn a_stop_with_no_load_in_progress_goes_to_the_worker() {
        let (phase, active) = (AtomicU8::new(IDLE_PHASE), AtomicBool::new(true));
        assert!(!abort_load(&phase, &active, true));
        assert!(!abort_load(&phase, &active, false));
        assert!(active.load(Ordering::SeqCst));
        assert_eq!(phase.load(Ordering::SeqCst), IDLE_PHASE);
    }

    #[test]
    fn the_early_buffer_holds_30_s_at_the_device_rate() {
        for rate in [16_000u32, 48_000, 96_000] {
            let mut e = Early::default();
            e.for_rate(rate);
            for _ in 0..31 {
                assert!(e.keep(vec![0.0; rate as usize]).is_none());
            }
            assert_eq!((e.samples, e.dropped), (30 * rate as usize, 1), "{rate} Hz");
        }
    }

    #[test]
    fn a_press_while_starting_cancels_the_start_and_one_while_listening_is_busy() {
        let (active, starting) = (AtomicBool::new(false), AtomicBool::new(false));
        assert_eq!(start_reply(&active, &starting), StartReply::Start);
        assert!(active.load(Ordering::SeqCst) && starting.load(Ordering::SeqCst), "a start is pending");
        assert_eq!(start_reply(&active, &starting), StartReply::Cancel);
        starting.store(false, Ordering::SeqCst);
        assert_eq!(start_reply(&active, &starting), StartReply::Busy);
    }

    #[test]
    fn the_start_stops_pending_once_the_mic_is_open_or_the_start_ends() {
        static STARTING: AtomicBool = AtomicBool::new(false);
        let mut r = rig(Permission::Granted, ModelState::Ready, None);
        r.w.starting = Some(&STARTING);
        STARTING.store(true, Ordering::SeqCst);
        start(&mut r);
        assert!(!STARTING.load(Ordering::SeqCst), "still pending after the mic opened");
        let mut r = rig(Permission::Granted, ModelState::Ready, Some("no device"));
        r.w.starting = Some(&STARTING);
        STARTING.store(true, Ordering::SeqCst);
        start(&mut r);
        assert!(!STARTING.load(Ordering::SeqCst), "still pending after the start failed");
    }

    #[test]
    fn with_no_model_the_mic_never_opens() {
        let mut r = rig(Permission::Granted, ModelState::Missing, None);
        start(&mut r);
        assert_eq!(*r.ev.borrow(), vec![Event::NoModel]);
        assert_eq!(*r.opened.borrow(), 0);
        assert_eq!(*r.asked.borrow(), 0, "no permission prompt without a model");
        assert!(!r.active.load(Ordering::SeqCst) && !r.w.listening());
    }

    #[test]
    fn while_the_model_downloads_the_page_sees_the_percentage() {
        let mut r = rig(Permission::Granted, ModelState::Downloading(42), None);
        start(&mut r);
        assert_eq!(*r.ev.borrow(), vec![Event::Downloading(42)]);
        assert_eq!(*r.opened.borrow(), 0);
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn permission_denied_says_so_and_never_loads_or_opens() {
        let mut r = rig(Permission::Denied, ModelState::Ready, None);
        start(&mut r);
        assert_eq!(*r.ev.borrow(), vec![Event::Denied]);
        assert_eq!((*r.opened.borrow(), r.loads.borrow().len()), (0, 0));
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn a_first_start_asks_and_a_no_is_denied() {
        let mut r = rig(Permission::NotAsked, ModelState::Ready, None);
        start(&mut r);
        assert_eq!(*r.asked.borrow(), 1);
        assert!(r.ev.borrow().is_empty() && r.active.load(Ordering::SeqCst));
        r.w.handle(Msg::Answer(false));
        assert_eq!(*r.ev.borrow(), vec![Event::Denied]);
        assert_eq!(*r.opened.borrow(), 0);
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn a_first_start_asks_and_a_yes_starts_listening() {
        let mut r = rig(Permission::NotAsked, ModelState::Ready, None);
        start(&mut r);
        r.w.handle(Msg::Answer(true));
        assert_eq!(*r.ev.borrow(), vec![Event::Listening]);
        assert_eq!(*r.opened.borrow(), 1);
        assert_eq!(*r.loads.borrow(), vec!["pt".to_string()]);
    }

    #[test]
    fn the_mic_refusing_at_open_is_denied_too() {
        let mut r = rig(Permission::Granted, ModelState::Ready, Some("denied"));
        start(&mut r);
        assert_eq!(*r.ev.borrow(), vec![Event::Denied]);
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn no_input_device_is_an_error_worded_for_the_page() {
        let mut r = rig(Permission::Granted, ModelState::Ready, Some("no input device"));
        start(&mut r);
        let ev = r.ev.borrow();
        assert!(matches!(ev.as_slice(), [Event::Error(m)] if !m.contains("no input device")), "{ev:?}");
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn audio_streams_text_and_stop_keeps_it() {
        let mut r = rig(Permission::Granted, ModelState::Ready, None);
        start(&mut r);
        r.w.handle(Msg::Audio(vec![0.0; CHUNK]));
        r.w.handle(Msg::Audio(vec![0.0; CHUNK]));
        r.w.handle(Msg::Stop);
        let ev = r.ev.borrow();
        assert_eq!(texts(&ev), vec![(1, "hi".into(), false), (2, "hi there".into(), true)]);
        assert_eq!(ev.first(), Some(&Event::Listening));
        assert_eq!(ev.last(), Some(&Event::Stopped));
        assert!(!r.active.load(Ordering::SeqCst) && !r.w.listening());
    }

    #[test]
    fn cancel_discards_and_audio_after_it_is_dropped() {
        let mut r = rig(Permission::Granted, ModelState::Ready, None);
        start(&mut r);
        r.w.handle(Msg::Audio(vec![0.0; CHUNK]));
        r.w.handle(Msg::Cancel);
        r.w.handle(Msg::Audio(vec![0.0; CHUNK]));
        r.w.handle(Msg::Stop);
        let ev = r.ev.borrow();
        assert_eq!(texts(&ev), vec![(1, "hi".into(), false)]);
        assert_eq!(ev.last(), Some(&Event::Cancelled));
        assert!(!r.active.load(Ordering::SeqCst));
    }

    #[test]
    fn cancel_while_the_prompt_is_up_ends_it() {
        let mut r = rig(Permission::NotAsked, ModelState::Ready, None);
        start(&mut r);
        r.w.handle(Msg::Cancel);
        r.w.handle(Msg::Answer(true));
        assert_eq!(*r.ev.borrow(), vec![Event::Cancelled]);
        assert_eq!(*r.opened.borrow(), 0, "a late yes opens nothing");
    }

    #[test]
    fn a_second_start_while_listening_is_ignored() {
        let mut r = rig(Permission::Granted, ModelState::Ready, None);
        start(&mut r);
        start(&mut r);
        assert_eq!(*r.opened.borrow(), 1);
        assert_eq!(r.loads.borrow().len(), 1);
        assert_eq!(*r.ev.borrow(), vec![Event::Listening]);
    }

    #[test]
    fn an_offline_model_gives_each_phrase_at_its_pause_and_no_live_words() {
        let decoded: Rc<RefCell<Vec<usize>>> = Rc::default();
        let d2 = decoded.clone();
        let mut s = Session::new(Phrases::new(move |a: &[f32]| {
            d2.borrow_mut().push(a.len());
            format!("phrase {}", d2.borrow().len())
        }));
        let speech: Vec<f32> = (0..RATE).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect();
        let quiet = vec![0.0f32; RATE];
        let mut ev = vec![];
        for part in [&quiet, &speech, &quiet, &speech] {
            for c in part.chunks(160) {
                ev.extend(s.feed(c));
            }
        }
        ev.extend(s.stop());
        assert_eq!(texts(&ev), vec![(1, "phrase 1".into(), true), (2, "phrase 2".into(), true)]);

        assert_eq!(decoded.borrow()[0], RATE * 3 / 10 + RATE + PAUSE);
    }

    #[test]
    fn a_short_noise_between_phrases_is_not_decoded() {

        let mut s = Session::new(Phrases::new(|a: &[f32]| format!("{} samples", a.len())));
        let room: Vec<f32> = (0..RATE).map(|i| if i % 2 == 0 { 1e-4 } else { -1e-4 }).collect();
        let knock: Vec<f32> = (0..RATE / 10).map(|i| if i % 2 == 0 { 0.05 } else { -0.05 }).collect();
        let mut ev = vec![];
        for part in [&room, &knock, &room] {
            for c in part.chunks(160) {
                ev.extend(s.feed(c));
            }
        }
        ev.extend(s.stop());
        assert!(texts(&ev).iter().all(|t| t.1.is_empty()), "{ev:?}");
    }

    #[test]
    fn a_quiet_speaker_over_a_quiet_room_is_still_heard() {

        let mut s = Session::new(Phrases::new(|a: &[f32]| format!("{} samples", a.len())));
        let room: Vec<f32> = (0..RATE).map(|i| if i % 2 == 0 { 1e-4 } else { -1e-4 }).collect();
        let speech: Vec<f32> = (0..RATE).map(|i| if i % 2 == 0 { 1.8e-3 } else { -1.8e-3 }).collect();
        let mut ev = vec![];
        for part in [&room, &speech, &room] {
            for c in part.chunks(160) {
                ev.extend(s.feed(c));
            }
        }
        assert_eq!(texts(&ev).len(), 1, "{ev:?}");
    }

    #[test]
    fn review_r4_vpio_digital_silence_then_room_speech_room_ends_without_stop() {
        for digital in [0.0f32, 1e-6] {
            let mut s = Session::new(Phrases::new(|_: &[f32]| "real phrase".into()));
            let silence = vec![digital; RATE / 5];
            let room = vec![1e-3; RATE];
            let speech = vec![0.05; RATE];
            let mut ev = Vec::new();
            for part in [&silence, &room, &speech, &room] {
                for chunk in part.chunks(160) { ev.extend(s.feed(chunk)); }
            }
            assert_eq!(texts(&ev), vec![(1, "real phrase".into(), true)], "{digital}: {ev:?}");
        }
    }

    fn halves(a: &[f32]) -> Vec<crate::dictation_settle::Word> {
        (0..a.len() / (RATE / 2)).map(|i| crate::dictation_settle::Word { text: format!("w{}", i + 1), start: i as f32 * 0.5, end: i as f32 * 0.5 + 0.4 }).collect()
    }
    fn halves_text(a: &[f32]) -> String {
        halves(a).iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    type Calls = Rc<RefCell<Vec<(char, usize, usize)>>>;

    fn quiet_tail(a: &[f32]) -> usize {
        a.iter().rev().take_while(|x| x.abs() < 1e-3).count()
    }

    fn live_phrases(calls: &Calls) -> Phrases<impl FnMut(&[f32]) -> String> {
        let (c1, c2) = (calls.clone(), calls.clone());
        let at = std::time::Instant::now();
        Phrases::new(move |a: &[f32]| {
            c1.borrow_mut().push(('j', a.len(), quiet_tail(a)));
            halves_text(a)
        })
        .with_passes(Box::new(move |a: &[f32], last: bool| {
            c2.borrow_mut().push((if last { 'f' } else { 'p' }, a.len(), quiet_tail(a)));
            Ok(Decoded { text: halves_text(a), words: halves(a) })
        }))
        .with_clock(Box::new(move || at))
    }
    fn speech(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize).map(|i| if i % 2 == 0 { 0.1 } else { -0.1 }).collect()
    }
    fn feed_all(s: &mut Session<impl Recognizer>, parts: &[&[f32]]) -> Vec<Event> {
        let mut ev = vec![];
        for part in parts {
            for c in part.chunks(160) {
                ev.extend(s.feed(c));
            }
        }
        ev
    }

    fn settled_runs(ev: &[Event]) -> Vec<Vec<String>> {
        ev.iter()
            .filter_map(|e| match e {
                Event::Text { text, settled: Some(n), is_final: false, .. } => Some(text.split_whitespace().take(*n).map(str::to_string).collect()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_offline_model_shows_live_words_before_the_pause() {
        let calls: Calls = Rc::default();
        let mut s = Session::new(live_phrases(&calls));
        let ev = feed_all(&mut s, &[&vec![0.0; RATE * 3 / 10], &speech(3.0)]);
        let partials: Vec<_> = texts(&ev).into_iter().filter(|t| !t.2 && !t.1.is_empty()).collect();
        assert!(!partials.is_empty(), "no live words in 3 s of speech with no pause: {ev:?}");
        assert!(ev.iter().any(|e| matches!(e, Event::Text { settled: Some(_), is_final: false, .. })), "the live words carry no settled count: {ev:?}");
    }

    #[test]
    fn the_live_settled_words_only_grow_and_the_final_keeps_them() {
        let calls: Calls = Rc::default();
        let mut s = Session::new(live_phrases(&calls));
        let quiet = vec![0.0; RATE];
        let ev = feed_all(&mut s, &[&quiet, &speech(6.0), &quiet]);
        let runs = settled_runs(&ev);
        assert!(runs.iter().any(|r| !r.is_empty()), "nothing settled in 6 s: {ev:?}");
        let key = |w: &String| w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        for w in runs.windows(2) {
            assert!(w[1].len() >= w[0].len() && w[0].iter().zip(&w[1]).all(|(a, b)| key(a) == key(b)), "settled words changed: {:?} then {:?}", w[0], w[1]);
        }
        let last = runs.last().unwrap();
        let fin = texts(&ev).into_iter().find(|t| t.2).expect("the phrase's final").1;
        let fin: Vec<String> = fin.split_whitespace().map(str::to_string).collect();
        assert!(last.iter().zip(&fin).all(|(a, b)| key(a) == key(b)) && fin.len() >= last.len(), "the final dropped settled words: {last:?} vs {fin:?}");
    }

    #[test]
    fn with_live_words_the_finals_are_todays_offline_finals() {
        let quiet = vec![0.0; RATE];
        let (one, two) = (speech(2.4), speech(7.3));
        let parts: [&[f32]; 5] = [&quiet, &one, &quiet, &two, &quiet];
        let calls: Calls = Rc::default();
        let mut live = Session::new(live_phrases(&calls));
        let mut today = Session::new(Phrases::new(halves_text));
        let mut a = feed_all(&mut live, &parts);
        a.extend(live.stop());
        let mut b = feed_all(&mut today, &parts);
        b.extend(today.stop());
        let finals = |ev: &[Event]| texts(ev).into_iter().filter(|t| t.2).map(|t| t.1).collect::<Vec<_>>();
        assert_eq!(finals(&a), finals(&b));
        assert_eq!(finals(&a).len(), 2);
        assert!(calls.borrow().iter().any(|c| c.0 == 'p'), "no live pass ran");
    }

    #[test]
    fn the_pause_runs_only_the_final_decode_and_no_pass_starts_in_a_pause() {
        let calls: Calls = Rc::default();
        let mut s = Session::new(live_phrases(&calls));
        let quiet = vec![0.0f32; RATE];
        let mut closing = vec![];

        for part in [&quiet, &speech(4.6), &quiet] {
            for c in part.chunks(160) {
                let before = calls.borrow().len();
                let ev = s.feed(c);
                if ev.iter().any(|e| matches!(e, Event::Text { is_final: true, .. })) {
                    closing = calls.borrow()[before..].to_vec();
                }
            }
        }
        assert_eq!(closing.iter().map(|c| c.0).collect::<String>(), "f", "the pause ran more than the final decode: {closing:?}");
        assert!(calls.borrow().iter().all(|c| c.0 != 'j'), "live words on, yet the phrase ended on its own decode: {:?}", calls.borrow());
        assert!(calls.borrow().iter().filter(|c| c.0 == 'p').all(|c| c.2 < QUIET_NO_PASS), "a pass started in a pause: {:?}", calls.borrow());
    }

    #[test]
    fn stop_runs_only_the_final_decode() {
        let calls: Calls = Rc::default();
        let mut s = Session::new(live_phrases(&calls));
        feed_all(&mut s, &[&vec![0.0; RATE / 2], &speech(2.5)]);
        let before = calls.borrow().len();
        let ev = s.stop();
        assert_eq!(calls.borrow()[before..].iter().map(|c| c.0).collect::<String>(), "f", "{:?}", calls.borrow());
        assert!(ev.iter().any(|e| matches!(e, Event::Text { is_final: true, .. })));
    }

    #[test]
    fn a_failing_pass_turns_the_live_words_off_and_the_phrase_still_ends_as_today() {
        let finals: Rc<RefCell<u32>> = Rc::default();
        let f2 = finals.clone();
        let p = Phrases::new(move |a: &[f32]| {
            *f2.borrow_mut() += 1;
            halves_text(a)
        })
        .with_passes(Box::new(|_: &[f32], _: bool| Err("the sidecar took too long".to_string())));
        let mut s = Session::new(p);
        let quiet = vec![0.0; RATE];
        let ev = feed_all(&mut s, &[&quiet, &speech(3.0), &quiet]);
        assert_eq!(texts(&ev).iter().filter(|t| t.2).count(), 1, "{ev:?}");
        assert!(!ev.iter().any(|e| matches!(e, Event::Error(_))));
        assert!(!ev.iter().any(|e| matches!(e, Event::Text { is_final: false, .. })), "partials after a failed pass: {ev:?}");
        assert!(s.recognizer().settling_off_at().is_some());
        assert_eq!(*finals.borrow(), 1, "live words off: the phrase ends on its own decode");
    }

    #[test]
    fn a_slow_pass_is_not_followed_by_another_until_as_long_again_has_passed() {

        let base = std::time::Instant::now();
        let clock: Rc<std::cell::Cell<std::time::Duration>> = Rc::default();
        let (c1, c2) = (clock.clone(), clock.clone());
        let passes: Rc<RefCell<u32>> = Rc::default();
        let p2 = passes.clone();
        let p = Phrases::new(halves_text)
            .with_passes(Box::new(move |a: &[f32], _: bool| {
                *p2.borrow_mut() += 1;
                c1.set(c1.get() + std::time::Duration::from_millis(200));
                Ok(Decoded { text: halves_text(a), words: halves(a) })
            }))
            .with_clock(Box::new(move || base + c2.get()));
        let mut s = Session::new(p);
        feed_all(&mut s, &[&vec![0.0; RATE / 2], &speech(5.0)]);
        assert_eq!(*passes.borrow(), 1, "a pass ran before as long as the last one took had gone by");

        clock.set(clock.get() + std::time::Duration::from_millis(200));
        feed_all(&mut s, &[&speech(1.0)]);
        assert_eq!(*passes.borrow(), 2);
    }

    #[test]
    fn the_sidecar_phrases_pass_on_the_settled_count() {
        let calls: Calls = Rc::default();
        let mut s = Session::new(Posted { phrases: live_phrases(&calls), err: Arc::new(Mutex::new(None)) });
        let ev = feed_all(&mut s, &[&vec![0.0; RATE / 2], &speech(3.5)]);
        assert!(ev.iter().any(|e| matches!(e, Event::Text { settled: Some(_), is_final: false, .. })), "{ev:?}");
    }

    #[test]
    fn the_final_corrects_the_words_already_settled() {
        let p = Phrases::new(halves_text).with_passes(Box::new(|a: &[f32], last: bool| {
            let mut words = halves(a);
            if last {
                words[0].text = "x1".into();
            }
            Ok(Decoded { text: words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "), words })
        }));
        let mut s = Session::new(p);
        let quiet = vec![0.0; RATE];
        let ev = feed_all(&mut s, &[&quiet, &speech(6.0), &quiet]);
        let last = settled_runs(&ev).into_iter().filter(|r| !r.is_empty()).last().expect("words settled");
        let fin = texts(&ev).into_iter().find(|t| t.2).expect("a final").1;
        assert_eq!(last.first().map(String::as_str), Some("w1"));
        assert_eq!(fin.split_whitespace().next(), Some("x1"), "the whole-phrase decode corrects the early word");
    }

    #[test]
    fn sherpa_tokens_and_times_make_words() {
        let r = sherpa_onnx::OfflineRecognizerResult {
            text: " hello world again".into(),
            tokens: vec!["▁hel".into(), "lo".into(), "▁world".into(), "▁a".into(), "gain".into()],
            timestamps: Some(vec![0.1, 0.3, 0.6, 1.2, 1.4]),
            durations: None,
        };
        let w = words_of(&r).unwrap();
        assert_eq!(w.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["hello", "world", "again"]);
        assert_eq!((w[0].start, w[0].end, w[1].start, w[1].end, w[2].start), (0.1, 0.6, 0.6, 1.2, 1.2));
        assert!(w[2].end > 1.4);

        let r = sherpa_onnx::OfflineRecognizerResult { text: "a b".into(), tokens: vec![" a".into(), " b".into()], timestamps: Some(vec![0.0, 0.5]), durations: None };
        assert_eq!(words_of(&r).unwrap().len(), 2);
        let r = sherpa_onnx::OfflineRecognizerResult { text: "hello".into(), tokens: vec!["▁hello".into()], timestamps: None, durations: None };
        assert!(words_of(&r).is_err());
    }

    struct Settling {
        steps: Vec<(&'static str, usize)>,
        at: usize,
    }
    impl Recognizer for Settling {
        fn accept(&mut self, _: &[f32]) {
            self.at += 1;
        }
        fn text(&mut self) -> String {
            self.steps[self.at.min(self.steps.len()) - 1].0.into()
        }
        fn settled_words(&mut self) -> Option<usize> {
            Some(self.steps[self.at.min(self.steps.len()) - 1].1)
        }
        fn endpoint(&mut self) -> bool {
            false
        }
        fn reset(&mut self) {}
        fn finish(&mut self) -> String {
            self.text()
        }
    }

    #[test]
    fn a_partial_goes_out_when_words_settle_even_if_the_text_is_the_same() {
        let mut s = Session::new(Settling { steps: vec![("o gabinete", 0), ("o gabinete", 0), ("o gabinete", 1), ("o gabinete do", 1), ("o gabinete do", 1)], at: 0 });
        let ev: Vec<Event> = (0..5).flat_map(|_| s.feed(&[0.0; 10])).filter(|e| matches!(e, Event::Text { .. })).collect();
        assert_eq!(
            ev,
            [
                Event::Text { seq: 1, text: "o gabinete".into(), settled: Some(0), is_final: false },
                Event::Text { seq: 2, text: "o gabinete".into(), settled: Some(1), is_final: false },
                Event::Text { seq: 3, text: "o gabinete do".into(), settled: Some(1), is_final: false },
            ]
        );

        let last = s.stop();
        assert_eq!(last[0], Event::Text { seq: 4, text: "o gabinete do".into(), settled: Some(3), is_final: true });
    }

    #[test]
    fn a_dictation_counts_its_partials_and_its_loudness_each_second() {
        let mut s = Session::new(Settling { steps: vec![("um", 0), ("um dois", 0), ("um dois", 0)], at: 0 });

        for _ in 0..10 {
            s.feed(&[0.1; RATE / 10]);
        }
        for _ in 0..10 {
            s.feed(&[0.001; RATE / 10]);
        }
        s.feed(&[0.0; RATE / 2]);
        assert_eq!(s.partials(), 2, "um, then um dois");
        assert_eq!(s.loudness_db(), [-20, -60], "whole seconds only; the half second waits");
    }

    #[test]
    fn events_serialize_for_the_page() {
        assert_eq!(Event::Text { seq: 3, text: "olá".into(), settled: Some(1), is_final: true }.js(), json!({"seq": 3, "text": "olá", "settled": "olá", "tail": "", "is_final": true}));

        assert_eq!(
            Event::Text { seq: 4, text: "o gabinete  do governo".into(), settled: Some(2), is_final: false }.js(),
            json!({"seq": 4, "text": "o gabinete  do governo", "settled": "o gabinete", "tail": "do governo", "is_final": false})
        );
        assert_eq!(Event::Text { seq: 5, text: "um".into(), settled: Some(9), is_final: false }.js()["tail"], json!(""), "a count past the words settles them all");

        assert_eq!(Event::Text { seq: 6, text: "olá mundo".into(), settled: None, is_final: false }.js(), json!({"seq": 6, "text": "olá mundo", "is_final": false}));
        assert_eq!(Event::Downloading(7).js(), json!({"state": "downloading", "pct": 7}));
        assert_eq!(Event::Level(0.12345).js(), json!({"level": 0.123}));
    }
    #[test]
    fn offline_endpoint_final_uses_full_decode_text_including_corrected_prefix() {
        let clock = std::time::Instant::now();
        let p = Phrases::new(|_: &[f32]| "fallback".into()).with_passes(Box::new(|_: &[f32], last: bool| {
            let text = if last { "Hello Unfold, how's it going?" } else { "Qual on Fold" };
            let words = text.split_whitespace().enumerate().map(|(i, text)| crate::dictation_settle::Word { text: text.into(), start: i as f32 * 0.2, end: i as f32 * 0.2 + 0.1 }).collect();
            Ok(Decoded { text: text.into(), words })
        })).with_clock(Box::new(move || clock));
        let mut s = Session::new(p);
        let ev = feed_all(&mut s, &[&vec![0.0; RATE], &speech(6.0), &vec![0.0; RATE]]);
        assert!(settled_runs(&ev).iter().any(|r| r.first().is_some_and(|w| w == "Qual")));
        let finals: Vec<_> = texts(&ev).into_iter().filter(|t| t.2).map(|t| t.1).collect();
        assert_eq!(finals, ["Hello Unfold, how's it going?"]);
        #[cfg(feature = "voice_pane")]
        {
            let mut input = crate::conversation_input::Input::default();
            let mut submitted = Vec::new();
            for e in &ev {
                let ready = match e {
                    Event::Level(level) => { input.level(*level); input.ready(false) }
                    Event::Text { text, is_final, .. } => input.words(text, *is_final, false),
                    _ => None,
                };
                submitted.extend(ready);
            }
            for _ in 0..24 { input.level(0.0); submitted.extend(input.ready(false)); }
            assert_eq!(submitted, finals, "conversation submits the corrected final once");
        }
    }

}

#[cfg(test)]
mod dictation_locale_tests {
    use super::*;
    #[test]
    fn pt_locale_is_canonical_before_model_admission() {
        let mut settings = crate::settings::Settings::default();
        settings.dictation_language = "pt".into();
        assert_eq!(crate::nemo::language(stream_lang_of(&settings, &["en-US".into()])), Some("pt-BR"));
        settings.dictation_language = "auto".into();
        assert_eq!(crate::nemo::language(stream_lang_of(&settings, &["en-BR".into(), "pt-BR".into()])), Some("pt-BR"));
        assert_eq!(crate::nemo::language(stream_lang_of(&settings, &["pt-PT".into()])), Some("pt-BR"));
        settings.dictation_language = "en".into();
        assert_eq!(stream_lang_of(&settings, &["pt-BR".into()]), "en");
        settings.dictation_language = "auto".into();
        assert_eq!(stream_lang_of(&settings, &["es-ES".into()]), "auto");
    }
}

#[cfg(test)]
mod model_admission_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    struct Denied;
    impl Mic for Denied {
        fn permission(&self)->Permission {Permission::Denied}
        fn ask(&self,_answer:Box<dyn FnOnce(bool)+Send>) {panic!("not an inference test")}
        fn open(&mut self,_sink:Box<dyn FnMut(Vec<f32>)+Send>)->Result<(Box<dyn Any>,u32,String),MicError> {panic!("no real microphone")}
    }
    fn sparse_installed(data:&std::path::Path,id:&str) {

        let m=crate::dictation_models::model(crate::dictation_models::catalog(),id).unwrap();
        let d=crate::dictation_models::dir(data,id);std::fs::create_dir_all(&d).unwrap();
        let mut marker=String::new();
        for (name,size,hash) in crate::dictation_models::files(m) {
            std::fs::File::create(d.join(name)).unwrap().set_len(size).unwrap();
            marker.push_str(&format!("{hash}  {name}\n"));
        }
        std::fs::write(d.join(".verified"),marker).unwrap();
    }
    #[test]
    fn pt_start_rejects_stale_installed_nemotron_or_english_before_permission() {
        for (i,id) in ["nemotron-3.5-streaming-0.6b-q8","parakeet-tdt_ctc-110m-en"].into_iter().enumerate() {
            let data=std::env::temp_dir().join(format!("model-admission-{}-{i}",std::process::id()));
            sparse_installed(&data,id);crate::dictation_models::set_default(&data,id).unwrap();
            let events=Rc::new(RefCell::new(vec![]));let copy=events.clone();
            let (tx,_rx)=std::sync::mpsc::sync_channel(4);
            let mut worker=Worker::new(Denied,Installed::new(data),Box::new(move|e|copy.borrow_mut().push(e.clone())),tx,Arc::new(AtomicBool::new(false)),Box::new(|_|None));
            worker.handle(Msg::Start{lang:"pt".into()});
            assert_eq!(events.borrow().as_slice(),[Event::NoModel],"PT must install Parakeet v3 instead of admitting unrelated installed models (base emits Denied)");
        }
    }
}

#[cfg(test)]
mod model_dispatch_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    struct MicFixture;
    impl Mic for MicFixture {
        fn permission(&self)->Permission {Permission::Granted}
        fn ask(&self,_answer:Box<dyn FnOnce(bool)+Send>) {unreachable!()}
        fn open(&mut self,_sink:Box<dyn FnMut(Vec<f32>)+Send>)->Result<(Box<dyn Any>,u32,String),MicError> {Ok((Box::new(()),16000,"fixture".into()))}
    }
    struct RecognizerFixture;
    impl Recognizer for RecognizerFixture {
        fn accept(&mut self,_samples:&[f32]) {} fn text(&mut self)->String {String::new()}
        fn endpoint(&mut self)->bool {false} fn reset(&mut self) {} fn finish(&mut self)->String {String::new()}
    }
    struct ModelFixture(Rc<RefCell<Vec<String>>>);
    impl Models for ModelFixture {
        type R=RecognizerFixture;
        fn state(&self)->ModelState {panic!("readiness must receive resolved language")}
        fn state_for(&self,lang:&str)->ModelState {self.0.borrow_mut().push(format!("ready:{lang}"));ModelState::Ready}
        fn load(&mut self,lang:&str)->Result<Self::R,String> {self.0.borrow_mut().push(format!("load:{lang}"));Ok(RecognizerFixture)}
    }
    #[test]
    fn same_resolved_pt_language_reaches_readiness_and_load() {
        let calls=Rc::new(RefCell::new(vec![]));let (tx,_rx)=std::sync::mpsc::sync_channel(4);
        let mut worker=Worker::new(MicFixture,ModelFixture(calls.clone()),Box::new(|_|{}),tx,Arc::new(AtomicBool::new(false)),Box::new(|_|None));
        let settings=crate::settings::Settings{dictation_language:"pt".into(),..Default::default()};
        let lang=stream_lang_of(&settings,&["en-US".into()]);worker.handle(Msg::Start{lang:lang.into()});
        assert_eq!(*calls.borrow(),["ready:pt","load:pt"]);
        assert_eq!(crate::nemo::language(lang),Some("pt-BR"));
    }
}
