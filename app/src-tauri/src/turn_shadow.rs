
use crate::turn_detect::{Decision, Detector};
use std::{
    collections::VecDeque,
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Condvar, Mutex},
    time::Instant,
};
const CAPACITY: usize = crate::turn_features::SAMPLES;
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub dropped_requests: u64,
    pub worker_lag_ns: u64,
    pub audio_gap_samples: u64,
}
pub struct Observation {
    pub epoch: u64,
    pub input_turn: u64,
    pub result: Result<Decision, String>,
    pub stats: Stats,
}
struct Window {
    key: (u64, u64),
    end: u64,
    audio: Vec<f32>,
    queued: Instant,
}
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<Window>>,
    wake: Condvar,
    closed: AtomicBool,
    dropped: AtomicU64,
}

pub struct Observer {
    mail: Arc<Mailbox>,
    audio: VecDeque<f32>,
    key: Option<(u64, u64)>,
    end: u64,
    #[cfg(test)]
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Observer {
    pub fn spawn(emit: impl FnMut(Observation) + Send + 'static) -> Result<Self, String> {
        let mut detector = None;
        Self::with_processor(move |samples, reset| {
            let d = detector.get_or_insert_with(|| Detector::load(Default::default()));
            match d {
                Ok(d) => { if reset { d.reset(); } d.push_shadow(samples) }
                Err(e) => Err(e.clone()),
            }
        }, emit)
    }

    pub fn with_processor(
        mut process: impl FnMut(&[f32], bool) -> Result<Vec<Decision>, String> + Send + 'static,
        mut emit: impl FnMut(Observation) + Send + 'static,
    ) -> Result<Self, String> {
        let mail = Arc::new(Mailbox::default());
        let inbox = mail.clone();
        let _thread = std::thread::Builder::new().name("turn-observer".into()).spawn(move || {
            let mut key = None;
            let mut seen = 0;
            let mut origin = 0;
            let mut failed = false;
            loop {
                let window = {
                    let mut slot = inbox.pending.lock().unwrap();
                    while slot.is_none() && !inbox.closed.load(Ordering::Acquire) {
                        slot = inbox.wake.wait_timeout(slot, std::time::Duration::from_secs(1)).unwrap().0;
                    }
                    if inbox.closed.load(Ordering::Acquire) { return; }
                    slot.take().unwrap()
                };
                let start = window.end.saturating_sub(window.audio.len() as u64);
                let changed = key != Some(window.key);
                let reset = changed || start > seen;
                let gap = if changed { start } else { start.saturating_sub(seen) };
                if reset { key = Some(window.key); seen = start; origin = start; failed = false; }
                if failed { continue; }
                let offset = seen.saturating_sub(start).min(window.audio.len() as u64) as usize;
                let stats = Stats {
                    dropped_requests: inbox.dropped.load(Ordering::Relaxed),
                    worker_lag_ns: window.queued.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                    audio_gap_samples: gap,
                };
                let result = process(&window.audio[offset..], reset);
                seen = window.end;
                if inbox.closed.load(Ordering::Acquire) { return; }
                match result {
                    Ok(decisions) => for mut d in decisions {
                        d.audio_start_ms += origin * 1000 / crate::turn_detect::RATE;
                        d.audio_end_ms += origin * 1000 / crate::turn_detect::RATE;
                        emit(Observation { epoch: window.key.0, input_turn: window.key.1, result: Ok(d), stats });
                    },
                    Err(e) => {
                        failed = true;
                        emit(Observation { epoch: window.key.0, input_turn: window.key.1, result: Err(e), stats });
                    }
                }
            }
        }).map_err(|e| e.to_string())?;
        Ok(Self { mail, audio: VecDeque::with_capacity(CAPACITY), key: None, end: 0, #[cfg(test)] thread: Some(_thread) })
    }
    #[cfg(test)]
    pub fn finish(&mut self) {
        self.mail.closed.store(true, Ordering::Release);
        self.mail.wake.notify_one();
        if let Some(thread)=self.thread.take(){thread.join().unwrap();}
    }

    pub fn push(&mut self, epoch: u64, input_turn: u64, samples: &[f32]) {
        let key = (epoch, input_turn);
        if self.key != Some(key) { self.key = Some(key); self.audio.clear(); self.end = 0; }
        self.end += samples.len() as u64;
        let samples=&samples[samples.len().saturating_sub(CAPACITY)..];
        let remove=(self.audio.len()+samples.len()).saturating_sub(CAPACITY);
        self.audio.drain(..remove);
        self.audio.extend(samples.iter().copied());
        let window = Window { key, end: self.end, audio: self.audio.iter().copied().collect(), queued: Instant::now() };
        let replaced = {
            let mut slot = self.mail.pending.lock().unwrap();
            let previous = slot.replace(window);
            if previous.is_some() { self.mail.dropped.fetch_add(1, Ordering::Relaxed); }
            previous
        };
        drop(replaced);
        self.mail.wake.notify_one();
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.mail.closed.store(true, Ordering::Release);
        self.mail.wake.notify_one();

    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    #[test]
    fn busy_worker_keeps_only_latest_window_and_recovers_its_audio_tail() {
        let (entered_tx, entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (done, output) = mpsc::channel();
        let mut calls = 0;
        let mut observer = Observer::with_processor(move |audio, reset| {
            calls += 1;
            done.send((audio.to_vec(), reset)).unwrap();
            if calls == 1 { entered_tx.send(()).unwrap(); released.recv().unwrap(); }
            Ok(vec![])
        }, |_| {}).unwrap();
        observer.push(1, 1, &[1.]);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        for sample in [2., 3., 4.] { observer.push(1, 1, &[sample]); }
        assert_eq!(observer.mail.dropped.load(Ordering::Relaxed), 2);
        release.send(()).unwrap();
        assert_eq!(output.recv_timeout(Duration::from_secs(2)).unwrap(), (vec![1.], true));
        assert_eq!(output.recv_timeout(Duration::from_secs(2)).unwrap(), (vec![2.,3.,4.], false));
        observer.push(2, 1, &[5.]);
        assert_eq!(output.recv_timeout(Duration::from_secs(2)).unwrap(), (vec![5.], true));
    }
    #[test]
    fn a_dropped_observer_never_delivers_its_inflight_result() {
        let (entered_tx, entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (sent, output) = mpsc::channel();
        let observer = Observer::with_processor(move |_, _| {
            entered_tx.send(()).unwrap(); released.recv().unwrap(); Err("injected failure".into())
        }, move |o| { sent.send(o.result.is_err()).unwrap(); }).unwrap();
        let mut observer=observer;observer.push(1,1,&[0.]);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(observer);release.send(()).unwrap();
        assert!(output.recv_timeout(Duration::from_secs(2)).is_err());
    }
    #[test]
    fn contended_mailbox_preserves_the_latest_request_and_bounds_audio() {
        let (processed,ready)=mpsc::channel();
        let observer=Observer::with_processor(move |samples,_|{processed.send(samples.len()).unwrap();Ok(vec![])}, |_|{}).unwrap();
        let mail=observer.mail.clone();let slot=mail.pending.lock().unwrap();
        let (started,publishing)=mpsc::channel();let (returned,done)=mpsc::channel();
        let publisher=std::thread::spawn(move || {
            let mut observer=observer;started.send(()).unwrap();
            observer.push(1,1,&vec![0.;CAPACITY+800]);assert!(returned.send(observer).is_ok());
        });
        publishing.recv_timeout(Duration::from_secs(2)).unwrap();

        let early=done.recv_timeout(Duration::from_millis(50));drop(slot);
        let mut observer=early.unwrap_or_else(|_|done.recv_timeout(Duration::from_secs(2)).unwrap());
        publisher.join().unwrap();
        assert_eq!(mail.dropped.load(Ordering::Relaxed),0,"the sole latest request must survive brief mailbox contention");
        assert_eq!(ready.recv_timeout(Duration::from_secs(2)).unwrap(),CAPACITY);
        assert_eq!(observer.audio.len(),CAPACITY);assert_eq!(observer.audio.capacity(),CAPACITY);observer.finish();
    }
    #[test]
    fn paced_timer_matches_baseline_while_a_slow_predictor_is_inflight() {
        let run=|shadow:bool|{
            let (entered_tx,entered)=mpsc::channel();
            let mut first=true;
            let mut observer=Observer::with_processor(move |_,_|{
                if first {entered_tx.send(()).unwrap();first=false;}
                std::thread::sleep(Duration::from_millis(300));Ok(vec![])
            }, |_|{}).unwrap();
            if shadow {observer.push(1,1,&[0.;800]);entered.recv_timeout(Duration::from_secs(2)).unwrap();}
            let mut input=crate::conversation_input::Input::default();input.words("a complete request",true,false);
            let began=Instant::now();let mut commit=None;
            for tick in 1..=16 {
                std::thread::sleep((began+Duration::from_millis(tick*50)).saturating_duration_since(Instant::now()));
                input.level(0.);if input.ready(false).is_some(){commit=Some(began.elapsed());break;}
                if shadow {observer.push(1,1,&[0.;800]);}
            }
            let elapsed=commit.unwrap();observer.finish();elapsed
        };
        let baseline=run(false);let shadow=run(true);
        eprintln!("paced timer baseline_ms={} shadow_ms={} delta_ms={}",baseline.as_secs_f64()*1000.,shadow.as_secs_f64()*1000.,shadow.as_secs_f64()*1000.-baseline.as_secs_f64()*1000.);
        assert!(shadow.abs_diff(baseline)<Duration::from_millis(25));
    }

}
