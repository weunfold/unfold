

use std::any::Any;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use rth_protocol::{Event as Played, PlaybackPhase, ResponseStatus};
use rth_voice::owner::{ClearReason, SpeechOwner};
use rth_voice::queue::PlayQueue;
use serde_json::{json, Value};
use speech_markup::{Event, Step};
use unfold_speech::{AudioChunk, Cancel, Client, Report, Sink, StepError, VoiceEntry, Warning};

use crate::pitch::Pitch;
use crate::voice_front::{render::Binding,runtime::{Epoch,Ticket}};
use rth_voice::resample::Resampler;

const QUEUE_SECONDS: f64 = 4.0;

const PIECE_SECONDS: f64 = 0.25;

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

const WAIT_EVERY: Duration = Duration::from_millis(20);

const DRAIN_SLACK: Duration = Duration::from_secs(5);

const MORE_WITHIN: Duration = Duration::from_secs(60);

const CLOSE_AFTER: Duration = Duration::from_millis(500);

const RATE: u32 = unfold_speech::SAMPLE_RATE;

const BUSY_AFTER: u32 = 2;
const BUSY_AFTER_MAX: u32 = 5;

pub fn busy_wait(after: Option<u32>) -> Duration {
    Duration::from_secs(after.unwrap_or(BUSY_AFTER).min(BUSY_AFTER_MAX).into())
}

pub trait Output: Send + Sync {

    fn rate(&self) -> Result<u32, String>;

    fn open(&self, queue: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String>;
    fn open_front(&self, _queue: Arc<PlayQueue>, _binding: Arc<Binding>) -> Result<Box<dyn Any + Send>, String> {
        Err("guarded output unavailable".into())
    }
}

pub enum Job {

    Speak { client: Client, steps: Vec<Step>, voice: VoiceEntry },

    Clip { samples: Vec<i16>, rate: u32 },

    Fleet { url: String, key: Option<String>, body: Value, pitch: Option<f32>, more: Option<std::sync::mpsc::Receiver<String>>, retries: u32 },
}

struct TextSpan { start: u64, end: Option<u64>, text: String }
struct Preview {
    front: Option<Arc<Binding>>,
    request_deadline: Option<Instant>,
    text_spans: Vec<TextSpan>,
    id: String,
    asked: Instant,
    first_audio_ms: Option<u64>,

    shown: Option<Instant>,
    truncated: bool,

    failed: Option<Value>,

    cancel: Cancel,

    stopped: bool,

    retries: u32,

    emit: fn(&Value),
}

struct State {
    tracked: bool,
    owner: Option<SpeechOwner>,
    stream: Option<Box<dyn Any + Send>>,
    open: Vec<Preview>,
}

pub struct Speaker {
    out: &'static dyn Output,
    state: Mutex<State>,

    stops: AtomicU64,
    synthesizing: AtomicU64,
    #[cfg(test)]
    drain_waiter: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    #[cfg(test)]
    request_release: Mutex<Option<RequestRelease>>,
}

#[cfg(test)]
type RequestRelease = (String, std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);

static NEXT: AtomicU64 = AtomicU64::new(1);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Speaker {
    pub const fn new(out: &'static dyn Output) -> Self {
        Speaker { out, state: Mutex::new(State { tracked: false, owner: None, stream: None, open: Vec::new() }), stops: AtomicU64::new(0), synthesizing:AtomicU64::new(0),
            #[cfg(test)] drain_waiter: Mutex::new(None),
            #[cfg(test)] request_release: Mutex::new(None),
        }
    }

    pub fn start(&'static self, job: Job, emit: fn(&Value)) -> String {
        self.start_bound(job,emit,None,None,false).unwrap_or_else(|_|String::new())
    }

    #[allow(dead_code)]
    pub fn start_front(&'static self, job: Job, epoch: Epoch, ticket: Ticket, emit: fn(&Value)) -> Result<String,String> {
        self.start_front_until(job,epoch,ticket,Instant::now()+Duration::from_secs(5),emit)
    }
    pub fn start_front_until(&'static self,job:Job,epoch:Epoch,ticket:Ticket,deadline:Instant,emit:fn(&Value))->Result<String,String> {
        if !matches!(&job,Job::Fleet {retries:0,more:None,..}) {return Err("front requires one bounded speech".into());}
        self.start_bound(job,emit,Some(Arc::new(Binding::new(ticket,epoch))),Some(deadline),false)
    }

    pub fn start_reply(&'static self, job:Job,emit:fn(&Value))->Result<String,String> {
        self.start_bound(job,emit,None,None,true)
    }
    #[allow(dead_code)]
    pub fn stop_front(&self, ticket: &Ticket) {
        let mut s=lock(&self.state);
        let ids=s.open.iter().filter(|p|p.front.as_ref().is_some_and(|b|b.ticket.same(ticket))).map(|p|p.id.clone()).collect::<Vec<_>>();

        s.stop_ids(&ids,if ticket.cancelled(){ClearReason::UserCancel}else{ClearReason::ResponseCancelled});
    }
    fn start_bound(&'static self, job: Job, emit: fn(&Value), front: Option<Arc<Binding>>, request_deadline:Option<Instant>, ordinary_reply:bool) -> Result<String,String> {
        let id = front.as_ref().map(|b|b.ticket.response().to_string()).unwrap_or_else(||format!("p-{}", NEXT.fetch_add(1, Ordering::SeqCst)));
        let cancel = Cancel::new();
        let mut s = lock(&self.state);
        if ordinary_reply && s.open.iter().any(|p|p.front.is_some()) {return Err("newer front turn owns speech".into());}
        if front.as_ref().is_some_and(|b| b.ticket.closed()) {return Err("front turn expired".into());}
        s.stop_all(ClearReason::Superseded);

        if ordinary_reply || front.is_some() {crate::doubletalk::reply(true);}
        s.open.push(Preview { front: front.clone(), request_deadline, text_spans: Vec::new(), id: id.clone(), asked: Instant::now(), first_audio_ms: None, shown: None, truncated: false, failed: None, cancel: cancel.clone(), stopped: false, retries: 0, emit });

        let old_stream=if front.is_some() || s.tracked {s.owner=None;s.stream.take()}else{None};
        s.tracked=front.is_some();
        let opening = s.prepare(self.out).and_then(|queue| {
            if let (Some(b),Some(o))=(&front,s.owner.as_mut()) {
                let rate=o.queue().rate;
                o.audio(&id,0,0,rate,"s16le","");
                if !o.epoch_of(&id).is_some_and(|epoch|b.bind(epoch)) {return Err("front queue binding refused".into());}
            }
            Ok(queue)
        });
        drop(s);
        drop(old_stream);
        let opened = opening.and_then(|q| q.map(|q| match &front {
            Some(b)=>self.out.open_front(q,b.clone()),None=>self.out.open(q)
        }).transpose());
        let mut s = lock(&self.state);
        let open = s.open.iter().any(|p| p.id == id);
        let unused=match opened {
            Err(e) => {s.fail(&id, failed("device_unavailable", &e));None},
            Ok(Some(stream)) if open && s.stream.is_none() => {s.stream=Some(stream);None},
            Ok(stream) => stream,
        };
        let live=s.open.iter().any(|p|p.id==id);
        drop(s);drop(unused);
        if !live {return Ok(id);}
        let worker = id.clone();
        if std::thread::Builder::new().name("voice-preview".into()).spawn(move || {
            if let Some(b)=front {self.supervise(&worker,job,cancel,b)} else {self.run(&worker,job,cancel)}
        }).is_err() {
            lock(&self.state).fail(&id, failed("synthesis", "no worker thread"));
        }
        Ok(id)
    }

    pub fn busy(&self) -> bool {
        !lock(&self.state).open.is_empty()
    }

    pub fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        lock(&self.state).stop_all(ClearReason::UserCancel);
    }

    pub fn reset_output(&self) {
        self.stop();
        let mut s = lock(&self.state);
        let stream=s.stream.take();s.owner=None;
        drop(s);drop(stream);
    }

    pub fn stops(&self) -> u64 {
        self.stops.load(Ordering::SeqCst)
    }

    pub fn retries(&self, id: &str) -> u32 {
        lock(&self.state).open.iter().find(|p| p.id == id).map_or(0, |p| p.retries)
    }

    fn supervise(&self,id:&str,job:Job,cancel:Cancel,b:Arc<Binding>) {
        std::thread::scope(|scope| {
            let (tx,rx)=std::sync::mpsc::sync_channel(1);
            scope.spawn(move || {self.run(id,job,cancel);let _=tx.send(());});
            while rx.recv_timeout(WAIT_EVERY)==Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                let mut s=lock(&self.state);
                if s.open.iter().any(|p|p.id==id && p.front.as_ref().is_some_and(|v|Arc::ptr_eq(v,&b)))
                    && s.owner.as_ref().is_some_and(|o|b.flush_veto(o.queue())) {
                    if b.ticket.cancelled() {s.stop_ids(&[id.into()],ClearReason::UserCancel);}
                    else {s.fail(id,failed("render_guard","render admission refused"));}
                }
            }
        });
    }

    fn synthesis_admission(&self,id:&str,deadline:Option<Instant>)->Result<(),String> {
        loop {
            let s=lock(&self.state);
            if !s.open.iter().any(|p|p.id==id){return Err("preview ended".into());}
            if deadline.is_some_and(|end|Instant::now()>=end){return Err("previous speech did not drain before deadline".into());}
            if deadline.is_none() || self.synthesizing.load(Ordering::SeqCst)==0 {
                self.synthesizing.fetch_add(1,Ordering::SeqCst);return Ok(());
            }
            #[cfg(test)]
            if let Some(waiter) = lock(&self.drain_waiter).take() { let _ = waiter.send(()); }
            drop(s);
            let left=deadline.map(|end|end.saturating_duration_since(Instant::now())).unwrap_or_default();
            std::thread::sleep(left.min(Duration::from_millis(2)));
        }
    }
    fn run(&self, id: &str, job: Job, cancel: Cancel) {
        let deadline=lock(&self.state).open.iter().find(|p|p.id==id).and_then(|p|p.request_deadline);
        if let Err(error)=self.synthesis_admission(id,deadline) {lock(&self.state).fail(id,failed("speech_admission",&error));self.close_when_idle();return;}
        struct InFlight<'a>(&'a AtomicU64);
        impl Drop for InFlight<'_>{fn drop(&mut self){self.0.fetch_sub(1,Ordering::SeqCst);}}
        let in_flight=InFlight(&self.synthesizing);
        let (front,rate)={let s=lock(&self.state);(s.open.iter().find(|p|p.id==id).and_then(|p|p.front.clone()),s.owner.as_ref().map(|o|o.queue().rate).unwrap_or(RATE))};
        let mut feed = Feed { speaker: self, id, chunk: u64::from(front.is_some()), offset: 0, pitch: None,
            resample: front.as_ref().map(|_|Resampler::new(RATE,rate)), output_rate:rate, front, cancel };
        let outcome = match job {

            Job::Speak { .. } | Job::Fleet { .. } if !self.is_open(id) => Ok(false),
            Job::Speak { client, steps, voice } => match Pitch::new(voice.pitch_semitones, RATE) {
                Err(_) => Err(failed("voice", "the voice's pitch is out of range")),
                Ok(pitch) => {
                    feed.pitch = pitch;
                    let o = outcome(&client.speak_plan(&steps, &voice, &mut feed));
                    feed.flush();
                    o
                }
            },
            Job::Clip { samples, rate } => {
                feed.push(&samples, rate);
                Ok(false)
            }
            Job::Fleet { url, key, body, pitch: semitones, more, retries } => match Pitch::new(semitones, RATE) {
                Err(_) => Err(failed("voice", "the voice's pitch is out of range")),
                Ok(pitch) => {
                    feed.pitch = pitch;
                    let mut left = retries;
                    let mut spoken = self.fleet_say(id, &url, key.as_deref(), &body, &mut feed, &mut left);
                    if let Some(more) = more {
                        let (mut quiet, mut tail) = (Instant::now(), true);
                        while spoken.is_ok() && self.is_open(id) && quiet.elapsed() < MORE_WITHIN {

                            if std::mem::take(&mut tail) {
                                feed.flush();
                                feed.pitch = Pitch::new(semitones, RATE).ok().flatten();
                            }
                            match more.recv_timeout(WAIT_EVERY * 10) {
                                Ok(text) => {
                                    let mut b = body.clone();
                                    b["input"] = json!(text);
                                    let cut = spoken == Ok(true);
                                    spoken = self.fleet_say(id, &url, key.as_deref(), &b, &mut feed, &mut left).map(|t| t || cut);
                                    (quiet, tail) = (Instant::now(), true);
                                }

                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => lock(&self.state).pump(),
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                    }
                    feed.flush();
                    spoken.map_err(|e| match e {
                        crate::fleet::Failed::Busy(after) => {
                            let mut f = failed("busy", "voice busy");
                            f["retry_after_s"] = json!(after);
                            f
                        }
                        crate::fleet::Failed::Cancelled | crate::fleet::Failed::Synthesis(_) => failed("synthesis", "synthesis failed"),
                    })
                }
            },
        };
        #[cfg(test)]
        {
            let release = {
                let mut probe = lock(&self.request_release);
                if probe.as_ref().is_some_and(|(held, _, _)| held == id) { probe.take() } else { None }
            };
            if let Some((_, entered, release)) = release {
                let _ = entered.send(());
                release.recv_timeout(Duration::from_secs(5)).expect("bounded request-registration hold");
            }
        }
        drop(in_flight);
        self.finish(id, outcome);
        self.close_when_idle();
    }

    fn fleet_say(&self, id: &str, url: &str, key: Option<&str>, body: &Value, feed: &mut Feed<'_>, left: &mut u32) -> Result<bool, crate::fleet::Failed> {
        if let Some(p)=lock(&self.state).open.iter_mut().find(|p|p.id==id) {
            p.text_spans.push(TextSpan {start:feed.offset,end:None,text:body["input"].as_str().unwrap_or("").into()});
        }
        loop {
            let cancel = feed.cancel.clone();
            match crate::fleet::stream(url, key, body, &cancel, &mut |samples| feed.push(samples, RATE)) {
                Err(crate::fleet::Failed::Busy(after)) if *left > 0 && self.wait_open(id, &cancel, busy_wait(after)) => {
                    *left -= 1;
                    if let Some(p) = lock(&self.state).open.iter_mut().find(|p| p.id == id) {
                        p.retries += 1;
                    }
                }
                spoken => {
                    feed.flush();
                    if let Some(p)=lock(&self.state).open.iter_mut().find(|p|p.id==id) {
                        if p.front.is_none() || spoken==Ok(false) {if let Some(span)=p.text_spans.last_mut(){span.end=Some(feed.offset);}}
                    }
                    return spoken;
                },
            }
        }
    }

    fn wait_open(&self, id: &str, cancel: &Cancel, d: Duration) -> bool {
        let end = Instant::now() + d;
        loop {
            if cancel.is_cancelled() || !self.is_open(id) {
                return false;
            }
            let now = Instant::now();
            if now >= end {
                return true;
            }
            std::thread::sleep((end - now).min(WAIT_EVERY));
        }
    }

    fn finish(&self, id: &str, outcome: Result<bool, Value>) {
        let mut s = lock(&self.state);
        let Some(p) = s.open.iter_mut().find(|p| p.id == id) else { return };
        let truncated = match outcome {
            Err(why) => return s.fail(id, why),
            Ok(t) => t,
        };
        p.truncated = truncated;
        let Some(owner) = s.owner.as_mut() else { return s.end(id, 0.0) };
        let Some(epoch) = owner.epoch_of(id) else { return s.end(id, 0.0) };
        owner.done(id, ResponseStatus::Completed);
        let left = owner.queue().queued_for(epoch) as f64 / owner.queue().rate as f64;
        let mut deadline = Instant::now() + Duration::from_secs_f64(left) + DRAIN_SLACK;
        s.pump();
        drop(s);
        while self.is_open(id) {
            let at = Instant::now();
            std::thread::sleep(WAIT_EVERY);
            if crate::doubletalk::playout_held() { deadline += at.elapsed(); }
            let mut s = lock(&self.state);
            s.pump();
            if Instant::now() > deadline && s.open.iter().any(|p| p.id == id) {
                s.fail(id, failed("device_unavailable", "the output stopped playing"));
            }
        }
    }

    fn close_when_idle(&self) {
        std::thread::sleep(CLOSE_AFTER);
        let mut s = lock(&self.state);
        let stream=if s.open.is_empty() && !s.owner.as_ref().is_some_and(SpeechOwner::audible) {s.stream.take()}else{None};
        drop(s);drop(stream);
    }

    fn is_open(&self, id: &str) -> bool {
        lock(&self.state).open.iter().any(|p| p.id == id)
    }

    #[cfg(test)]
    pub fn queued(&self) -> usize {
        lock(&self.state).owner.as_ref().map_or(0, |o| o.queue().stats().queued)
    }
}

impl State {

    fn prepare(&mut self, out: &dyn Output) -> Result<Option<Arc<PlayQueue>>, String> {
        if self.stream.is_some() {
            return Ok(None);
        }
        let rate = out.rate()?;
        if self.owner.as_ref().map(|o| o.queue().rate) != Some(rate) {
            let queue=if self.tracked {PlayQueue::new_reserved(rate,(f64::from(rate)*QUEUE_SECONDS) as usize)}else{PlayQueue::with_seconds(rate,QUEUE_SECONDS)};
            self.owner=Some(SpeechOwner::new(Arc::new(queue)));
        }
        self.owner.as_ref().map(|o| Some(o.queue().clone())).ok_or_else(|| "no speech owner".into())
    }

    fn stop_all(&mut self, reason: ClearReason) {
        let ids: Vec<String> = self.open.iter().map(|p| p.id.clone()).collect();
        self.stop_ids(&ids, reason);
    }

    fn stop_ids(&mut self, ids: &[String], reason: ClearReason) {
        if ids.is_empty() {
            return;
        }
        if reason != ClearReason::ResponseCancelled {
            for p in self.open.iter_mut().filter(|p| ids.contains(&p.id)) {
                p.stopped = true;
            }
        }
        if reason==ClearReason::Superseded {
            for p in self.open.iter_mut().filter(|p|ids.contains(&p.id) && p.front.is_some()) {
                p.stopped=false;p.failed=Some(failed("superseded","speech was replaced"));
            }
        }
        for p in self.open.iter().filter(|p|ids.contains(&p.id)) {
            if let Some(b)=&p.front {if matches!(reason,ClearReason::ResponseCancelled|ClearReason::Superseded) {b.ticket.close();}else{b.ticket.fence();}}
        }
        if let Some(o) = self.owner.as_mut() {
            o.clear(reason, ids);
        }
        self.pump();
        for id in ids {
            self.end(id, 0.0);
        }
    }

    fn fail(&mut self, id: &str, why: Value) {
        let Some(p) = self.open.iter_mut().find(|p| p.id == id) else { return };
        p.failed = Some(why);
        self.stop_ids(&[id.to_string()], ClearReason::ResponseCancelled);
    }

    fn end(&mut self, id: &str, played: f64) {
        if let Some(i) = self.open.iter().position(|p| p.id == id) {
            let p = close(&mut self.open, i);
            if p.front.is_some() {crate::doubletalk::reply(false);}
            (p.emit)(&terminal(&p, played));
        }
    }

    fn pump(&mut self) {
        let Some(o) = self.owner.as_mut() else { return };
        let rate = o.queue().rate as f64;
        for e in o.poll() {
            let Played::Playback { response_id, phase, played_samples, .. } = e else { continue };
            let Some(i) = self.open.iter().position(|p| p.id == response_id) else { continue };
            let played = played_samples as f64 / rate;
            match phase {
                PlaybackPhase::Started => {
                    let p = &mut self.open[i];
                    p.first_audio_ms = Some(p.asked.elapsed().as_millis() as u64);
                    p.shown = Some(Instant::now());
                    (p.emit)(&progress(p, played));
                }
                PlaybackPhase::Finished | PlaybackPhase::Cleared => {
                    let p = close(&mut self.open, i);
                    if p.front.is_some() {crate::doubletalk::reply(false);}
                    (p.emit)(&terminal(&p, played));
                }
            }
        }
        for p in self.open.iter_mut().filter(|p| p.shown.is_some_and(|t| t.elapsed() >= PROGRESS_EVERY)) {
            if let Some(epoch) = o.epoch_of(&p.id) {
                p.shown = Some(Instant::now());
                (p.emit)(&progress(p, o.queue().rendered(epoch).samples as f64 / rate));
            }
        }
    }
}

struct Feed<'a> {
    speaker: &'a Speaker,
    id: &'a str,
    chunk: u64,
    offset: u64,
    pitch: Option<Pitch>,
    resample: Option<Resampler>, output_rate:u32, front:Option<Arc<Binding>>,

    cancel: Cancel,
}

impl Feed<'_> {
    fn push(&mut self, samples: &[i16], rate: u32) {
        if !samples.is_empty() {if let Some(b)=&self.front {b.pcm();}}
        match self.pitch.as_mut().map(|p| p.push(samples)) {
            Some(shifted) => self.send(&shifted, rate),
            None => self.send(samples, rate),
        }
    }

    fn flush(&mut self) {
        if let Some(rest) = self.pitch.take().map(|mut pitch| pitch.finish()) {
            self.send(&rest, RATE);
        }
    }

    fn send(&mut self, samples: &[i16], rate: u32) {
        if let Some(resample)=&mut self.resample {
            let mut output=Vec::new();resample.process_i16(samples,&mut output);
            self.send_output(&output,self.output_rate);
        }else{self.send_output(samples,rate);}
    }
    fn send_output(&mut self, samples: &[i16], rate: u32) {
        let piece = ((rate as f64 * PIECE_SECONDS) as usize).max(1);
        for part in samples.chunks(piece) {
            let mut waiting = Instant::now();
            loop {
                let mut s = lock(&self.speaker.state);

                if !s.open.iter().any(|p| p.id == self.id) {
                    return;
                }
                let Some(o) = s.owner.as_mut() else { return };
                let q = o.queue();
                let need = part.len() * q.rate as usize / rate.max(1) as usize;
                if q.stats().queued + need > q.capacity && q.stats().queued > 0 {

                    if waiting.elapsed() > DRAIN_SLACK && !crate::doubletalk::playout_held() {
                        s.fail(self.id, failed("device_unavailable", "the output stopped playing"));
                        return;
                    }
                    s.pump();
                    drop(s);
                    std::thread::sleep(WAIT_EVERY);
                    if crate::doubletalk::playout_held(){waiting+=WAIT_EVERY;}
                    continue;
                }
                let data = base64::engine::general_purpose::STANDARD.encode(rth_voice::le_from_samples(part));
                o.audio(self.id, self.chunk, self.offset, rate, "s16le", &data);
                self.chunk += 1;
                self.offset += part.len() as u64;
                s.pump();
                break;
            }
        }
    }
}

impl Sink for Feed<'_> {
    fn audio(&mut self, _: usize, chunk: &AudioChunk) {
        self.push(&chunk.samples, chunk.sample_rate);
    }
    fn clip(&mut self, _: usize, _: Event, file: Option<&Path>) {
        if let Some(w) = file.and_then(|f| rth_voice::wav::read(f).ok()) {
            self.push(&w.to_i16_at(RATE), RATE);
        }
    }
    fn silence(&mut self, _: usize, ms: u32) {
        self.push(&vec![0; (RATE as u64 * ms as u64 / 1000) as usize], RATE);
    }
    fn cancel(&self) -> Option<Cancel> {
        Some(self.cancel.clone())
    }
}

fn outcome(r: &Report) -> Result<bool, Value> {
    if let Some((_, e)) = r.errors().next() {
        return Err(match e {
            StepError::Busy { retry_after_s } => {
                let mut f = failed("busy", "voice busy");
                f["retry_after_s"] = json!(retry_after_s);
                f
            }
            StepError::Catalog(_) => failed("voice", "the voice has no usable reference"),
            _ => failed("synthesis", "synthesis failed"),
        });
    }
    Ok(r.steps.iter().flat_map(|o| &o.warnings).any(|w| matches!(w, Warning::TruncatedPlayed { .. } | Warning::StillTruncated { .. })))
}

fn failed(code: &str, message: &str) -> Value {
    json!({"state": "failed", "audio_seconds": 0.0, "error": {"code": code, "message": message}})
}

fn seconds(s: f64) -> f64 {
    (s * 1000.0).round() / 1000.0
}

fn rendered_text(p: &Preview, played: f64) -> String {
    let sample = p.front.as_ref().map(|b|b.ticket.frames()).unwrap_or((played * f64::from(RATE)) as u64);
    p.text_spans.iter()
        .filter(|span| span.end.is_some_and(|end| end > span.start && end <= sample))
        .map(|span| span.text.as_str()).collect::<Vec<_>>().join(" ")
}

fn progress(p: &Preview, played: f64) -> Value {
    let sample=p.front.as_ref().map(|b|b.ticket.frames()).unwrap_or((played*RATE as f64) as u64);
    let sentence=p.text_spans.iter().find(|s|s.start <= sample && s.end.is_none_or(|end|sample < end)).map(|s|s.text.as_str()).unwrap_or("");
    let words=p.text_spans.iter().filter(|s|s.end.is_some_and(|end|end <= sample)).map(|s|s.text.as_str()).collect::<Vec<_>>().join(" ");
    json!({"preview_id": p.id, "state": "streaming", "audio_seconds": seconds(played), "first_audio_ms": p.first_audio_ms, "truncated": false, "error": null,"floor_sentence":sentence,"floor_played":words})
}

fn close(open: &mut Vec<Preview>, i: usize) -> Preview {
    let p = open.remove(i);
    p.cancel.cancel();
    p
}

fn terminal(p: &Preview, played: f64) -> Value {
    let mut t = match &p.failed {
        Some(f) => {
            let mut f = f.clone();
            f["preview_id"] = json!(p.id);
            f
        }
        None => json!({"preview_id": p.id, "state": "done", "audio_seconds": seconds(played), "first_audio_ms": p.first_audio_ms, "truncated": p.truncated, "stopped": p.stopped, "error": null}),
    };
    t["stopped"] = json!(p.stopped);
    t["rendered_text"] = json!(rendered_text(p, played));
    if let Some(b)=&p.front {
        let spans=p.text_spans.iter().filter_map(|s|s.end.filter(|end|*end>s.start && *end<=b.ticket.frames())
            .map(|end|json!({"start":s.start,"end":end,"text":s.text}))).collect::<Vec<_>>();
        let (pcm,render)=b.times();
        t["front"]=json!({"epoch":b.epoch,"response":b.ticket.response(),"frames":b.ticket.frames(),"spans":spans,
            "first_pcm_ns":pcm,"first_render_ns":render,"clock":"response-relative-monotonic","notice":"worker-delayed"});
    }
    if p.retries > 0 {
        t["retries"] = json!(p.retries);
    }
    t
}

pub fn pull_playout(queue: &PlayQueue, out: &mut [i16], held: bool) -> usize {
    if held {out.fill(0);0} else {queue.pull(out)}
}

pub struct Cpal;

impl Output for Cpal {
    fn rate(&self) -> Result<u32, String> {
        if crate::conversation::active() { return Ok(crate::duplex_audio::RATE); }
        use cpal::traits::{DeviceTrait, HostTrait};
        let dev = cpal::default_host().default_output_device().ok_or("no output device")?;
        dev.default_output_config().map(|c| c.sample_rate()).map_err(|e| format!("output config: {e}"))
    }

    fn open(&self, queue: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
        if crate::conversation::active() { return crate::duplex_audio::output(queue); }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("voice-out".into())
            .spawn(move || {
                let _ = tx.send(open_default(queue,None));
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(crate::dictation::OPEN_WITHIN).unwrap_or_else(|_| Err("the speaker didn't start in time".into()))
    }
    fn open_front(&self,queue:Arc<PlayQueue>,binding:Arc<Binding>) -> Result<Box<dyn Any+Send>,String> {
        if crate::conversation::active(){return crate::duplex_audio::output_front(queue,binding);}
        let (tx,rx)=std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new().name("voice-front-out".into()).spawn(move || {let _=tx.send(open_default(queue,Some(binding)));}).map_err(|e|e.to_string())?;
        rx.recv_timeout(crate::dictation::OPEN_WITHIN).unwrap_or_else(|_|Err("the speaker didn't start in time".into()))
    }

}

pub(crate) fn cpal_pull(queue: &PlayQueue, mono: &mut Vec<i16>, frames: usize) {
    mono.resize(frames, 0);
    let real=pull_playout(queue, mono, crate::doubletalk::playout_held());
    if queue.rate==crate::bc_playout::RATE as u32 {
        crate::bc_playout::rendered(frames,real,crate::doubletalk::reply_active());
    }
}

pub(crate) fn front_cpal_pull(queue:&PlayQueue,binding:&Binding,mono:&mut Vec<i16>,frames:usize,held:bool)->usize {
    if frames>mono.capacity() {mono.clear();binding.reject();return 0;}
    mono.resize(frames,0);binding.pull(queue,mono,held)
}

fn open_default(queue: Arc<PlayQueue>, binding:Option<Arc<Binding>>) -> Result<Box<dyn Any + Send>, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let dev = cpal::default_host().default_output_device().ok_or("no output device")?;
    let cfg = dev.default_output_config().map_err(|e| format!("output config: {e}"))?;
    let ch = (cfg.channels() as usize).max(1);
    let err = |e: cpal::Error| {

        let _ = std::io::Write::write_all(&mut std::io::stderr(), format!("rth-app: voice output: {e}\n").as_bytes());
    };

    let guarded=binding.is_some();
    let pull = move |queue: &PlayQueue, mono: &mut Vec<i16>, frames: usize| {
        if let Some(b)=&binding {
            let real=front_cpal_pull(queue,b,mono,frames,crate::doubletalk::playout_held());
            if queue.rate==crate::bc_playout::RATE as u32 {crate::bc_playout::rendered(frames,real,crate::doubletalk::reply_active());}
        }else{cpal_pull(queue,mono,frames);}
    };
    let mut mono=if guarded {Vec::with_capacity(8192)}else{Vec::new()};
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            cfg.config(),
            move |d: &mut [f32], _: &_| {
                d.fill(0.0);
                pull(&queue, &mut mono, d.len() / ch);
                for (fr, s) in d.chunks_mut(ch).zip(&mono) {
                    fr.fill(*s as f32 / 32768.0);
                }
            },
            err,
            None,
        ),
        cpal::SampleFormat::I16 => dev.build_output_stream(
            cfg.config(),
            move |d: &mut [i16], _: &_| {
                d.fill(0);
                pull(&queue, &mut mono, d.len() / ch);
                for (fr, s) in d.chunks_mut(ch).zip(&mono) {
                    fr.fill(*s);
                }
            },
            err,
            None,
        ),
        f => return Err(format!("unsupported sample format {f:?}")),
    }
    .map_err(|e| format!("output stream: {e}"))?;
    stream.play().map_err(|e| format!("output play: {e}"))?;
    Ok(Box::new(stream))
}

#[cfg(test)]
mod floor_playout_tests {
    use super::*;
    #[test] fn floor_sentence_flush_consumes_pitch_once() {
        let speaker=Speaker::new(&Cpal);
        let mut feed=Feed {speaker:&speaker,id:"not-admitted",chunk:0,offset:0,resample:None,output_rate:RATE,front:None,pitch:Pitch::new(Some(-3.0),RATE).unwrap(),cancel:Cancel::new()};
        feed.push(&vec![100;2000],RATE);
        feed.flush();
        feed.flush();
        assert!(feed.pitch.is_none());
    }
    #[test] fn yield_holds_samples_and_resume_continues_exactly() {
        let q=PlayQueue::new(RATE,16);q.push(1,vec![11,22,33,44]);let mut out=[0;2];
        pull_playout(&q,&mut out,false);assert_eq!(out,[11,22]);
        pull_playout(&q,&mut out,true);assert_eq!(out,[0,0]);assert_eq!(q.rendered(1).samples,2);assert_eq!(q.stats().queued,2);
        pull_playout(&q,&mut out,false);assert_eq!(out,[33,44]);assert_eq!(q.rendered(1).samples,4);
    }
}

#[cfg(test)]
mod h1_regressions {
    use super::*;
    #[test]
    fn h1_interrupted_fixture_exposes_only_rendered_text() {
        let p = Preview {
            front:None, request_deadline:None,text_spans: vec![
                TextSpan { start: 0, end: Some(24000), text: "The first sentence.".into() },
                TextSpan { start: 24000, end: Some(48000), text: "Never rendered tail.".into() },
            ],
            id: "fixture".into(), asked: Instant::now(), first_audio_ms: Some(0),
            shown: None, truncated: false, failed: None, cancel: Cancel::new(),
            stopped: true, retries: 0, emit: |_| {},
        };
        let e = terminal(&p, 1.0);
        assert_eq!(e["rendered_text"], "The first sentence.");
        assert_eq!(terminal(&p, 0.0)["rendered_text"], "");
        assert_eq!(terminal(&p, 0.5)["rendered_text"], "", "partial span has no word alignment");
        assert_eq!(terminal(&p, 1.99)["rendered_text"], "The first sentence.");
        assert_eq!(terminal(&p, 2.0)["rendered_text"], "The first sentence. Never rendered tail.");
        assert!(!e["rendered_text"].as_str().unwrap().contains("tail"));
    }
}

#[cfg(test)]
mod front_tests {
    use super::*;
    #[test] fn incomplete_spans_never_use_seconds_to_guess_words() {
        let epoch=Epoch {capture:3,turn:5};let ticket=Ticket::new(epoch,"exact".into());
        let b=Arc::new(Binding::new(ticket.clone(),epoch));
        let p=Preview {front:Some(b),request_deadline:None,text_spans:vec![TextSpan {start:0,end:Some(4),text:"Complete.".into()},
            TextSpan {start:4,end:Some(10),text:"Complete but not yet heard.".into()},TextSpan {start:10,end:None,text:"Uncertified tail.".into()}],id:"exact".into(),asked:Instant::now(),
            first_audio_ms:None,shown:None,truncated:false,failed:None,cancel:Cancel::new(),stopped:true,retries:0,emit:|_|{}};
        assert!(ticket.render(epoch,"exact",6));let end=terminal(&p,9999.0);
        assert_eq!(end["rendered_text"],"Complete.");assert_eq!(end["front"]["spans"],json!([{"start":0,"end":4,"text":"Complete."}]));
        assert!(ticket.fence());assert!(!ticket.render(epoch,"exact",1));
    }
    #[test] fn front_refuses_retry_continuation_and_unguarded_output() {
        let speaker=Box::leak(Box::new(Speaker::new(&Unguarded)));let epoch=Epoch {capture:1,turn:2};
        let ticket=Ticket::new(epoch,"bounded".into());
        assert!(speaker.start_front(Job::Clip {samples:vec![1],rate:RATE},epoch,ticket.clone(),|_|{}).is_err());
        let job=|retries,more|Job::Fleet {url:"http://127.0.0.1:1".into(),key:None,body:json!({"input":"Listen."}),pitch:None,retries,more};
        assert!(speaker.start_front(job(1,None),epoch,ticket.clone(),|_|{}).is_err());
        let (_tx,rx)=std::sync::mpsc::channel();assert!(speaker.start_front(job(0,Some(rx)),epoch,ticket,|_|{}).is_err());
        struct Unguarded;impl Output for Unguarded {
            fn rate(&self)->Result<u32,String>{Ok(RATE)}
            fn open(&self,_:Arc<PlayQueue>)->Result<Box<dyn Any+Send>,String>{panic!("ordinary output must never be used")}
        }
        let (e,t)=(Epoch{capture:1,turn:2},Ticket::new(epoch,"unsupported".into()));
        assert!(Unguarded.open_front(Arc::new(PlayQueue::new(RATE,16)),Arc::new(Binding::new(t,e))).is_err());
    }
}

#[cfg(test)]
mod front_lifecycle_tests {
    use super::*;
    struct DeviceFree;
    impl Output for DeviceFree {
        fn rate(&self) -> Result<u32, String> {
            Ok(RATE)
        }
        fn open(&self, _: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
            Err("fake output opened by stale reply".into())
        }
    }
    static OUT: DeviceFree = DeviceFree;
    fn front(speaker: &Speaker, id: &str) -> Ticket {
        let epoch = Epoch {
            capture: 1,
            turn: 2,
        };
        let t = Ticket::new(epoch, id.into());
        lock(&speaker.state).open.push(Preview {
            front: Some(Arc::new(Binding::new(t.clone(), epoch))),
            request_deadline: None,
            text_spans: vec![],
            id: id.into(),
            asked: Instant::now(),
            first_audio_ms: None,
            shown: None,
            truncated: false,
            failed: None,
            cancel: Cancel::new(),
            stopped: false,
            retries: 0,
            emit: |_| {},
        });
        t
    }
    #[test]
    fn late_legacy_speech_cannot_supersede_a_newer_front_turn() {
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));

        let t = front(speaker, "newer-front-turn2");
        assert!(speaker
            .start_reply(
                Job::Clip {
                    samples: vec![1; 4],
                    rate: RATE
                },
                |_| {}
            )
            .is_err());
        assert!(!t.cancelled());
        assert!(t.render(
            Epoch {
                capture: 1,
                turn: 2
            },
            t.response(),
            1
        ));
        assert!(lock(&speaker.state)
            .open
            .iter()
            .any(|p| p.id == t.response()));
        speaker.stop_front(&t);
    }
    #[test]
    fn non_user_supersession_closes_without_user_cancel_or_silent_terminal() {
        let speaker = Speaker::new(&OUT);
        let t = front(&speaker, "non-user-replacement");
        lock(&speaker.state).stop_all(ClearReason::Superseded);
        assert!(
            !t.cancelled(),
            "a non-user replacement must retain spoken legacy fallback"
        );
        assert!(!t.render(
            Epoch {
                capture: 1,
                turn: 2
            },
            t.response(),
            1
        ));
        assert_eq!(t.frames(), 0);
    }
    #[test]
    fn reset_drops_the_previous_stream_outside_speaker_state_lock() {
        struct Check(&'static Speaker);
        impl Drop for Check {
            fn drop(&mut self) {
                assert!(
                    self.0.state.try_lock().is_ok(),
                    "device drop held Speaker lock"
                );
            }
        }
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        lock(&speaker.state).stream = Some(Box::new(Check(speaker)));
        speaker.reset_output();
    }
    #[test]
    fn previous_synthesis_must_end_before_new_front_request_admission() {
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let _t = front(speaker, "next-request");
        speaker.synthesizing.store(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let t = std::thread::spawn(move || {
            let result = speaker.synthesis_admission(
                "next-request",
                Some(Instant::now() + Duration::from_millis(80)),
            );
            let _ = tx.send(result);
        });
        let received = rx.recv_timeout(Duration::from_millis(500));

        assert!(received.unwrap().is_err());
        t.join().unwrap();
        assert_eq!(speaker.synthesizing.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn drained_synthesis_admits_once_after_release_without_extending_the_deadline() {
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let _t = front(speaker, "drained-request");
        speaker.synthesizing.store(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let t = std::thread::spawn(move || {
            let result = speaker.synthesis_admission(
                "drained-request",
                Some(Instant::now() + Duration::from_secs(1)),
            );
            let _ = tx.send(result);
        });
        let early = rx.recv_timeout(Duration::from_millis(35));
        speaker.synthesizing.store(0, Ordering::SeqCst);
        assert!(
            matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "new request preceded old request drain"
        );
        assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
        t.join().unwrap();
        assert_eq!(speaker.synthesizing.load(Ordering::SeqCst), 1);
    }

    static REPLACED: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    fn replaced(v: &Value) { lock(&REPLACED).push(v.clone()); }
    #[test]
    fn four_step_old_legacy_chunks_new_front_then_non_user_replacement_falls_back_spoken() {
        use crate::{
            conversation_log::{Owner, Store, Turn},
            voice_front::{http::Burst, policy::*, runtime::*, Error},
        };
        use std::sync::atomic::AtomicBool;
        struct SpeechAdapter(&'static Speaker);
        impl Speech for SpeechAdapter {
            fn start(&mut self, r: SpeechRequest) -> crate::voice_front::Result<()> {
                let t = r.ticket;
                let b = Arc::new(Binding::new(
                    t.clone(),
                    Epoch {
                        capture: 1,
                        turn: 2,
                    },
                ));
                lock(&self.0.state).open.push(Preview {
                    front: Some(b),
                    request_deadline: None,
                    text_spans: vec![],
                    id: t.response().into(),
                    asked: Instant::now(),
                    first_audio_ms: None,
                    shown: None,
                    truncated: false,
                    failed: None,
                    cancel: Cancel::new(),
                    stopped: false,
                    retries: 0,
                    emit: replaced,
                });
                Ok(())
            }
            fn stop(&mut self, t: &Ticket) {
                self.0.stop_front(t);
            }
        }
        struct Backend(usize);
        impl Legacy for Backend {
            fn send(&mut self, t: &Turn) -> crate::voice_front::Result<String> {
                assert!(t.legacy_attempted);
                self.0 += 1;
                Ok(format!("backend-{}", self.0))
            }
        }
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let mut audio = SpeechAdapter(speaker);
        let mut backend = Backend(0);
        let dir = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
            .join(format!("four-step-front-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir.join("log"), "four-step").unwrap();
        let mut controller = Controller::new("four-step-clock").unwrap();
        let selection = Selection {
            locale: Locale::En,
            source: LocaleSource::Setting,
            confidence: None,
        };
        let inventory = Inventory::default();
        let cancel = AtomicBool::new(false);
        let submit = |controller: &mut Controller,
                      store: &mut Store,
                      epoch,
                      nonce,
                      candidate,
                      audio: &mut SpeechAdapter,
                      backend: &mut Backend| {
            controller.activate(epoch).unwrap();
            controller
                .submit(
                    store,
                    Submission {
                        epoch,
                        nonce,
                        input: "public input",
                        selection,
                        inventory: &inventory,
                        evidence: Evidence::default(),
                        history: &[] as &[RenderedReply],
                        intent: None,
                        candidate,
                        control: Control {
                            started: Instant::now(),
                            cancel: &cancel,
                        },
                    },
                    audio,
                    backend,
                )
                .unwrap()
        };

        assert_eq!(
            submit(
                &mut controller,
                &mut store,
                Epoch {
                    capture: 1,
                    turn: 1
                },
                "old",
                Candidate::Model(Burst::default()),
                &mut audio,
                &mut backend
            ),
            Outcome::LegacyAccepted
        );

        assert_eq!(
            submit(
                &mut controller,
                &mut store,
                Epoch {
                    capture: 1,
                    turn: 2
                },
                "new",
                Candidate::Controller(ControllerLine::Continue),
                &mut audio,
                &mut backend
            ),
            Outcome::Started
        );
        let ticket = lock(&speaker.state).open[0]
            .front
            .as_ref()
            .unwrap()
            .ticket
            .clone();

        assert!(speaker
            .start_reply(
                Job::Clip {
                    samples: vec![1; 4],
                    rate: RATE
                },
                |_| {}
            )
            .is_err());
        assert!(!ticket.cancelled());

        speaker.start(Job::Clip { samples: vec![], rate: RATE }, |_| {});
        let terminal = lock(&REPLACED).iter().find(|e| e["preview_id"] == ticket.response()).cloned().expect("Speaker terminal");
        assert_eq!(terminal["state"], "failed", "non-user supersession must reach the renderer terminal");
        assert_eq!(terminal["error"]["code"], "superseded");
        assert_ne!(terminal["stopped"], true);
        assert_eq!(
            controller.failed(&mut store, &ticket, &mut audio, &mut backend),
            Ok(Outcome::LegacyAccepted)
        );
        assert_eq!(backend.0, 2);
        assert_eq!(store.turns()[1].owner, Some(Owner::Legacy));
        assert!(store.turns()[1].legacy_attempted);
        assert!(store.turns()[1].presentation.is_none());
        assert!(!store.turns()[1].silent_replay);
        assert!(!ticket.cancelled());
        assert_eq!(
            controller.failed(&mut store, &ticket, &mut audio, &mut backend),
            Ok(Outcome::Existing)
        );
        assert_eq!(backend.0, 2);
        let _ = std::mem::size_of::<Error>();
    }
    static SUPERVISOR_EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    fn supervisor_emit(v: &Value) {
        lock(&SUPERVISOR_EVENTS).push(v.clone());
    }
    #[test]
    fn close_veto_supervisor_fails_without_user_cancel_and_drains_the_real_fake_http_request() {
        use std::{
            io::{BufRead, Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (asked, request) = std::sync::mpsc::sync_channel(1);
        let (closed, eof) = std::sync::mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let start = Instant::now();
            let (mut peer, _) = loop {
                match listener.accept() {
                    Ok(v) => break v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(start.elapsed() < Duration::from_secs(3));
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("fake accept {e}"),
                }
            };
            peer.set_nonblocking(false).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            {
                let mut reader = std::io::BufReader::new(&mut peer);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
            }
            peer.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            asked.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(
                peer.read(&mut byte).unwrap(),
                0,
                "cancel must close the previous HTTP request"
            );
            closed.send(()).unwrap();
        });
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let ticket = front(speaker, "close-veto");
        let e = Epoch {
            capture: 1,
            turn: 2,
        };
        let (binding, cancel) = {
            let mut state = lock(&speaker.state);
            let p = &mut state.open[0];
            p.emit = supervisor_emit;
            (p.front.as_ref().unwrap().clone(), p.cancel.clone())
        };
        assert!(binding.bind(1));
        let q = Arc::new(PlayQueue::new(RATE, 32));
        q.push(1, vec![123; 4]);
        lock(&speaker.state).owner = Some(SpeechOwner::new(q.clone()));
        let b = binding.clone();
        let (done, finished) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            speaker.supervise(
                "close-veto",
                Job::Fleet {
                    url,
                    key: None,
                    body: json!({"input":"public fixed line"}),
                    pitch: None,
                    more: None,
                    retries: 0,
                },
                cancel,
                b,
            );
            done.send(()).unwrap();
        });
        request.recv_timeout(Duration::from_secs(3)).unwrap();
        ticket.close();
        let mut out = [1; 4];
        assert_eq!(binding.pull(&q, &mut out, false), 0);
        finished.recv_timeout(Duration::from_secs(3)).unwrap();
        worker.join().unwrap();
        eof.recv_timeout(Duration::from_secs(1)).unwrap();
        server.join().unwrap();
        assert!(!ticket.cancelled());
        assert!(!ticket.render(e, ticket.response(), 1));
        assert_eq!(q.stats().queued, 0);
        assert_eq!(speaker.synthesizing.load(Ordering::SeqCst), 0);
        let events = lock(&SUPERVISOR_EVENTS);
        let terminal = events
            .iter()
            .find(|v| v["preview_id"] == "close-veto" && v["state"] == "failed")
            .expect("STOP veto ends as failure, never user stop");
        assert_eq!(terminal["error"]["code"], "render_guard");
    }

    #[test]
    fn replacing_a_front_stream_drops_it_outside_the_state_lock() {
        struct Check(&'static Speaker);
        impl Drop for Check {
            fn drop(&mut self) {
                assert!(
                    self.0.state.try_lock().is_ok(),
                    "front replacement held Speaker lock during device drop"
                );
            }
        }
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        {
            let mut state = lock(&speaker.state);
            state.stream = Some(Box::new(Check(speaker)));
            state.tracked = true;
        }
        let e = Epoch {
            capture: 1,
            turn: 8,
        };
        let t = Ticket::new(e, "replacing-output".into());
        speaker
            .start_front(
                Job::Fleet {
                    url: "http://127.0.0.1:1".into(),
                    key: None,
                    body: json!({}),
                    pitch: None,
                    more: None,
                    retries: 0,
                },
                e,
                t,
                |_| {},
            )
            .unwrap();
    }
    #[test]
    fn oversized_front_cpal_callback_is_silent_without_growing_its_preallocated_buffer() {
        let e = Epoch {
            capture: 1,
            turn: 4,
        };
        let t = Ticket::new(e, "bounded-cpal".into());
        let b = Binding::new(t.clone(), e);
        b.bind(1);
        let q = PlayQueue::new_reserved(RATE, 16);
        q.push(1, vec![123; 4]);
        let mut mono = Vec::with_capacity(2);
        assert_eq!(front_cpal_pull(&q, &b, &mut mono, 2, false), 2);
        assert_eq!(mono, [123; 2]);
        let taps = q.drain_tap();
        assert_eq!(taps.len(), 1);
        assert_eq!(front_cpal_pull(&q, &b, &mut mono, 3, false), 0);
        assert!(mono.is_empty());
        assert_eq!(mono.capacity(), 2);
        assert_eq!(t.frames(), 2);
        assert!(!t.cancelled());
        assert_eq!(q.rendered(1).samples, 2);
        assert!(q.drain_tap().is_empty());
        assert!(b.flush_veto(&q));
    }
}

#[cfg(test)]
mod prepared_queue_allocations {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering as CountOrder};
    static COUNT_THREAD: AtomicUsize = AtomicUsize::new(0);
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    struct Count;
    fn note_allocation() {
        let owner = COUNT_THREAD.load(CountOrder::Relaxed);
        if owner != 0 && owner == unsafe { libc::pthread_self() } as usize {
            ALLOCS.fetch_add(1, CountOrder::Relaxed);
        }
    }
    unsafe impl GlobalAlloc for Count {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            note_allocation();
            unsafe { System.alloc(l) }
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            note_allocation();
            unsafe { System.alloc_zeroed(l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            note_allocation();
            unsafe { System.realloc(p, l, n) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
    }
    #[global_allocator]
    static A: Count = Count;
    fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
        let id = unsafe { libc::pthread_self() } as usize;
        assert!(COUNT_THREAD
            .compare_exchange(0, id, CountOrder::SeqCst, CountOrder::SeqCst)
            .is_ok());
        ALLOCS.store(0, CountOrder::Relaxed);
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                COUNT_THREAD.store(0, CountOrder::SeqCst);
            }
        }
        let reset = Reset;
        let r = f();
        let n = ALLOCS.load(CountOrder::Relaxed);
        drop(reset);
        (r, n)
    }

    struct FakeOutput;
    impl Output for FakeOutput {
        fn rate(&self) -> Result<u32, String> {
            Ok(RATE)
        }
        fn open(&self, _: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
            panic!("allocation probe never opens a device")
        }
    }
    #[test]
    fn fresh_prepared_front_queues_allocate_zero_on_six_replies() {
        let mut ns = Vec::with_capacity(6600);
        for turn in 1..=6 {
            let mut state = State {
                tracked: true,
                owner: None,
                stream: None,
                open: vec![],
            };
            let q = state.prepare(&FakeOutput).unwrap().unwrap();
            let e = Epoch { capture: 1, turn };
            let t = Ticket::new(e, format!("prepared-{turn}"));
            let b = Binding::new(t, e);
            b.bind(1);
            let mut mono = Vec::with_capacity(8192);
            let mut allocations = 0;
            for _ in 0..1100 {
                q.push(1, vec![123; 480]);
                let at = Instant::now();
                let (n, a) = counted(|| front_cpal_pull(&q, &b, &mut mono, 480, false));
                ns.push(at.elapsed().as_nanos());
                assert_eq!(n, 480);
                allocations += a;
            }
            println!("prepared_front_reply={turn} callback_allocations={allocations}");
            assert_eq!(
                allocations, 0,
                "production prepare must choose the reserved constructor for each fresh queue"
            );
        }
        let mut onset_ns=Vec::with_capacity(6600);
        for rotation in 1..=6 {
            let capture=Arc::new(crate::voice_front::capture::Capture::<2048>::new());
            let reader=capture.reader();reader.activate();
            let e=Epoch{capture:rotation,turn:1};let t=Ticket::new(e,format!("capture-{rotation}"));capture.publish(&t,0).unwrap();
            for _ in 0..1100 {
                let at=Instant::now();let (_,allocations)=counted(||reader.fence());onset_ns.push(at.elapsed().as_nanos());
                assert_eq!(allocations,0,"the new capture onset fence must not allocate");
            }
        }
        onset_ns.sort_unstable();
        println!("capture_onset_fence_cpu calls={} median_ns={} p90_ns={} p99_ns={} max_ns={}",onset_ns.len(),onset_ns[onset_ns.len()/2],onset_ns[onset_ns.len()*9/10],onset_ns[onset_ns.len()*99/100],onset_ns[onset_ns.len()-1]);
        ns.sort_unstable();
        println!(
            "cold_and_warm_front_callback_cpu pulls={} median_ns={} p90_ns={} p99_ns={} max_ns={}",
            ns.len(),
            ns[ns.len() / 2],
            ns[ns.len() * 9 / 10],
            ns[ns.len() * 99 / 100],
            ns[ns.len() - 1]
        );
    }
}

#[cfg(test)]
mod review_idle_close {
    use super::*;
    struct NoOut;
    impl Output for NoOut {
        fn rate(&self) -> Result<u32, String> { Ok(RATE) }
        fn open(&self, _: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> { Err("no device".into()) }
    }
    static OUT: NoOut = NoOut;

    #[test]
    fn review_stopped_before_worker_still_closes_idle_stream() {
        let speaker: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        lock(&speaker.state).stream = Some(Box::new(()));
        speaker.run("gone", Job::Clip { samples: vec![1; 4], rate: RATE }, Cancel::new());
        assert!(lock(&speaker.state).stream.is_none(), "idle stream left open");
    }
}

#[cfg(test)]
mod admission_boundary_tests {
    use super::*;
    use std::sync::{mpsc, OnceLock};
    struct DropProbe {
        speaker: &'static Speaker,
        result: mpsc::SyncSender<(bool, u64)>,
    }
    impl Drop for DropProbe {
        fn drop(&mut self) {
            let unlocked = self.speaker.state.try_lock().is_ok();
            let _ = self
                .result
                .send((unlocked, self.speaker.synthesizing.load(Ordering::SeqCst)));
        }
    }
    struct NoDevice;
    impl Output for NoDevice {
        fn rate(&self) -> Result<u32, String> {
            Ok(RATE)
        }
        fn open(&self, _: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
            Ok(Box::new(()))
        }
        fn open_front(
            &self,
            q: Arc<PlayQueue>,
            _: Arc<Binding>,
        ) -> Result<Box<dyn Any + Send>, String> {
            self.open(q)
        }
    }
    static OUT: NoDevice = NoDevice;
    static ENDS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    fn ended(v: &Value) {
        lock(&ENDS).push(v.clone());
    }
    #[test]
    fn real_front_start_keeps_the_request_deadline_when_previous_http_is_still_held() {
        let s: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        s.synthesizing.store(1, Ordering::SeqCst);
        let epoch = Epoch {
            capture: 7,
            turn: 8,
        };
        let ticket = Ticket::new(epoch, "deadline-through-real-start".into());
        let at = Instant::now();
        s.start_front_until(
            Job::Fleet {
                url: "http://127.0.0.1:1".into(),
                key: None,
                body: json!({"input":"Hello."}),
                pitch: None,
                more: None,
                retries: 0,
            },
            epoch,
            ticket,
            at + Duration::from_millis(40),
            ended,
        )
        .unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        let terminal = loop {
            if let Some(v) = lock(&ENDS)
                .iter()
                .find(|v| v["preview_id"] == "deadline-through-real-start")
                .cloned()
            {
                break v;
            }
            assert!(Instant::now() < end, "front deadline must finish");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(
            terminal["error"]["code"], "speech_admission",
            "deadline must reach the real front worker"
        );
        assert!(at.elapsed() < Duration::from_millis(400));
        assert_eq!(s.synthesizing.load(Ordering::SeqCst), 1);
        s.synthesizing.store(0, Ordering::SeqCst);
    }
    #[test]
    fn idle_drop_is_unlocked_and_http_registration_ends_before_the_idle_sleep() {
        let s: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let (tx, rx) = mpsc::sync_channel(1);
        lock(&s.state).stream = Some(Box::new(DropProbe {
            speaker: s,
            result: tx,
        }));

        lock(&s.state).open.push(Preview {
            front: None,
            request_deadline: None,
            text_spans: vec![],
            id: "idle-probe".into(),
            asked: Instant::now(),
            first_audio_ms: None,
            shown: None,
            truncated: false,
            failed: None,
            cancel: Cancel::new(),
            stopped: false,
            retries: 0,
            emit: |_| {},
        });
        s.run(
            "idle-probe",
            Job::Clip {
                samples: vec![],
                rate: RATE,
            },
            Cancel::new(),
        );
        let (unlocked, active) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(unlocked, "idle output drop must not hold Speaker lock");
        assert_eq!(
            active, 0,
            "HTTP registration must end before idle sleep/teardown"
        );
    }
    #[test]
    fn unused_output_after_stop_is_dropped_outside_the_state_lock() {
        struct BlockOpen {
            speaker: OnceLock<&'static Speaker>,
            entered: mpsc::SyncSender<()>,
            release: Mutex<mpsc::Receiver<()>>,
            result: mpsc::SyncSender<(bool, u64)>,
        }
        impl Output for BlockOpen {
            fn rate(&self) -> Result<u32, String> {
                Ok(RATE)
            }
            fn open(&self, _: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
                self.entered.send(()).unwrap();
                lock(&self.release)
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap();
                Ok(Box::new(DropProbe {
                    speaker: self.speaker.get().copied().unwrap(),
                    result: self.result.clone(),
                }))
            }
        }
        let (entered, wait) = mpsc::sync_channel(1);
        let (release, blocked) = mpsc::sync_channel(1);
        let (result, dropped) = mpsc::sync_channel(1);
        let out: &'static BlockOpen = Box::leak(Box::new(BlockOpen {
            speaker: OnceLock::new(),
            entered,
            release: Mutex::new(blocked),
            result,
        }));
        let s: &'static Speaker = Box::leak(Box::new(Speaker::new(out)));
        assert!(out.speaker.set(s).is_ok());
        let worker = std::thread::spawn(move || {
            s.start(
                Job::Clip {
                    samples: vec![],
                    rate: RATE,
                },
                |_| {},
            )
        });
        let ready = wait.recv_timeout(Duration::from_secs(1));
        s.stop();
        let _ = release.send(());
        assert!(ready.is_ok());
        worker.join().unwrap();
        assert!(
            dropped.recv_timeout(Duration::from_secs(1)).unwrap().0,
            "unused output drop held Speaker lock"
        );
    }
    #[test]
    fn second_real_front_start_cancels_and_drains_the_previous_http_request() {
        use std::{
            io::{BufRead, Read, Write},
            net::{TcpListener, TcpStream},
            sync::atomic::AtomicBool,
        };
        fn request(peer: &mut TcpStream) {
            peer.set_nonblocking(false).unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut reader = std::io::BufReader::new(peer);
            let mut n = 0;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0, "request ended before headers");
                if line == "\r\n" { break; }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    n = v.trim().parse::<usize>().unwrap();
                }
            }
            reader.read_exact(&mut vec![0; n]).unwrap();
        }
        fn accept(listener: &TcpListener) -> TcpStream {
            let end = Instant::now() + Duration::from_secs(4);
            loop {
                match listener.accept() {
                    Ok((peer, _)) => return peer,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("accept: {e}"),
                }
                assert!(Instant::now() < end, "first request deadline");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::sync_channel(1);
        let (accepted_tx, accepted) = mpsc::channel();
        let released = Arc::new(AtomicBool::new(false));
        let release_seen = released.clone();
        let server = std::thread::spawn(move || {
            let mut old = accept(&listener);
            request(&mut old);
            old.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            tx.send(()).unwrap();
            old.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut eof = false;
            let mut next = loop {
                let mut byte = [0];
                match old.read(&mut byte) {
                    Ok(0) => eof = true,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    other => panic!("old HTTP: {other:?}"),
                }
                match listener.accept() {
                    Ok((peer, _)) => {
                        accepted_tx.send((eof, release_seen.load(Ordering::SeqCst))).unwrap();
                        break peer;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("accept: {e}"),
                }
                assert!(Instant::now() < deadline, "second request deadline");
                std::thread::sleep(Duration::from_millis(1));
            };
            request(&mut next);
            next.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: done\ndata: {\"truncated\":false}\n\n").unwrap();
        });
        let s: &'static Speaker = Box::leak(Box::new(Speaker::new(&OUT)));
        let job = || Job::Fleet {
            url: url.clone(), key: None, body: json!({"input":"Public line."}),
            pitch: None, more: None, retries: 0,
        };
        let e = Epoch { capture: 8, turn: 1 };
        let first = Ticket::new(e, "real-drain-first".into());
        let (entered, held) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        *lock(&s.request_release) = Some((first.response().into(), entered, gate));
        s.start_front_until(job(), e, first.clone(), Instant::now() + Duration::from_secs(4), ended).unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (waiting_tx, waiting) = mpsc::channel();
        *lock(&s.drain_waiter) = Some(waiting_tx);
        let newer = Epoch { turn: 2, ..e };
        let second = Ticket::new(newer, "real-drain-second".into());
        s.start_front_until(job(), newer, second, Instant::now() + Duration::from_secs(4), ended).unwrap();

        let old_held = held.recv_timeout(Duration::from_secs(2));
        let new_waiting = waiting.recv_timeout(Duration::from_secs(1));
        let premature = accepted.try_recv();
        released.store(true, Ordering::SeqCst);
        let _ = release.send(());
        let admission = match premature {
            Ok(value) => Ok(value),
            Err(_) => accepted.recv_timeout(Duration::from_secs(2)),
        };
        let served = server.join();
        s.stop();
        assert!(old_held.is_ok(), "first real HTTP registration did not reach the hold");
        assert!(new_waiting.is_ok(), "second real HTTP worker skipped the drain wait");
        assert!(premature.is_err(), "second socket accepted while first registration was held");
        assert_eq!(admission.unwrap(), (true, true), "second socket preceded cancellation or registration release");
        served.unwrap();
        assert!(!first.cancelled());
        assert!(!first.render(e, first.response(), 1));
    }
}

#[cfg(test)]
mod pre_enable_expired_ticket {
    use super::*;
    struct ForbiddenDevice;
    impl Output for ForbiddenDevice {
        fn rate(&self)->Result<u32,String>{panic!("expired front must not inspect an audio device")}
        fn open(&self,_:Arc<PlayQueue>)->Result<Box<dyn Any+Send>,String>{panic!("expired front must not open an audio device")}
    }
    #[test]
    fn closed_front_is_refused_before_arming_reply_or_touching_device() {
        const NAME:&str="speaker::pre_enable_expired_ticket::closed_front_is_refused_before_arming_reply_or_touching_device";
        if std::env::var_os("RTH_F2D_EXPIRED_CHILD").is_none(){
            let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",NAME,"--test-threads","4"]).env("RTH_F2D_EXPIRED_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        static OUT:ForbiddenDevice=ForbiddenDevice;
        let speaker: &'static Speaker=Box::leak(Box::new(Speaker::new(&OUT)));
        let epoch=Epoch{capture:1,turn:1};let ticket=Ticket::new(epoch,"expired".into());ticket.close();
        crate::doubletalk::reset();let before=crate::doubletalk::reply_active();
        let result=speaker.start_bound(Job::Clip{samples:vec![1;4],rate:RATE},|_|{},Some(Arc::new(Binding::new(ticket,epoch))),None,false);
        assert_eq!(result,Err("front turn expired".into()),"closed front ticket must be rejected at admission");
        assert_eq!(crate::doubletalk::reply_active(),before,"an expired front must not arm the semantic reply hold");
        assert!(!speaker.busy());
    }
}
