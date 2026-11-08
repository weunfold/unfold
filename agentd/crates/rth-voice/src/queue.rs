

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

struct Segment {
    epoch: u64,
    samples: Vec<i16>,
    pos: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Rendered {
    pub samples: u64,
    pub first_at: Option<Instant>,
    pub last_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    Queued,

    Fenced,

    Overflow { queued: usize, dropped: usize },
}

#[derive(Debug, Clone)]
pub struct Flushed {

    pub dropped: Vec<(u64, u64)>,

    pub took_ns: u64,
    pub at: Instant,
}

impl Flushed {
    pub fn dropped_for(&self, epoch: u64) -> u64 {
        self.dropped.iter().filter(|(e, _)| *e == epoch).map(|(_, n)| n).sum()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderTap {
    pub at: Instant,

    pub samples: u32,

    pub real: u32,
    pub sum_sq: f64,

    pub epoch: Option<u64>,
    pub epoch_sample: u64,
}

impl RenderTap {
    pub fn rms(&self) -> f64 {
        if self.samples == 0 { 0.0 } else { (self.sum_sq / self.samples as f64).sqrt() }
    }
}

pub const TAP_CAP: usize = 1024;

#[derive(Default)]
struct Inner {
    segs: VecDeque<Segment>,
    queued: usize,

    live_from: u64,
    rendered: HashMap<u64, Rendered>,

    rendered_after_fence: u64,
    underrun_samples: u64,
    overflow_samples: u64,
    pulls: u64,
    tap: VecDeque<RenderTap>,
}

pub struct PlayQueue {
    inner: Mutex<Inner>,

    pub rate: u32,
    pub capacity: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct QueueStats {
    pub queued: usize,
    pub underrun_samples: u64,
    pub overflow_samples: u64,
    pub rendered_after_fence: u64,
    pub pulls: u64,
}

impl PlayQueue {

    pub fn new(rate: u32, capacity: usize) -> Self {
        Self { inner: Mutex::new(Inner::default()), rate, capacity }
    }

    pub fn new_reserved(rate: u32, capacity: usize) -> Self {
        let inner = Inner { rendered: HashMap::with_capacity(1),
            tap: VecDeque::with_capacity(TAP_CAP), ..Inner::default() };
        let queue = Self { inner: Mutex::new(inner), rate, capacity };

        drop(queue.inner.lock().unwrap());
        queue
    }

    pub fn with_seconds(rate: u32, seconds: f64) -> Self {
        Self::new(rate, (rate as f64 * seconds) as usize)
    }

    pub fn push(&self, epoch: u64, mut samples: Vec<i16>) -> Push {
        let mut q = self.inner.lock().unwrap();
        if epoch < q.live_from {
            return Push::Fenced;
        }
        let room = self.capacity.saturating_sub(q.queued);
        let dropped = samples.len().saturating_sub(room);
        if dropped > 0 {
            samples.truncate(room);
            q.overflow_samples += dropped as u64;
        }
        let queued = samples.len();
        if queued > 0 {
            q.queued += queued;
            q.segs.push_back(Segment { epoch, samples, pos: 0 });
        }
        if dropped > 0 { Push::Overflow { queued, dropped } } else { Push::Queued }
    }

    pub fn pull(&self, out: &mut [i16]) -> usize {
        let now = Instant::now();
        let mut q = self.inner.lock().unwrap();
        q.pulls += 1;
        let mut n = 0;
        let first = q.segs.front().map(|s| s.epoch);
        let first_cursor = first.map(|e| q.rendered.get(&e).map_or(0, |r| r.samples)).unwrap_or(0);
        while n < out.len() {
            let Some(seg) = q.segs.front_mut() else { break };
            let take = (seg.samples.len() - seg.pos).min(out.len() - n);
            out[n..n + take].copy_from_slice(&seg.samples[seg.pos..seg.pos + take]);
            seg.pos += take;
            let (epoch, done) = (seg.epoch, seg.pos == seg.samples.len());
            n += take;
            q.queued -= take;
            if epoch < q.live_from {
                q.rendered_after_fence += take as u64;
            }
            let r = q.rendered.entry(epoch).or_default();
            r.samples += take as u64;
            r.first_at.get_or_insert(now);
            r.last_at = Some(now);
            if done {
                q.segs.pop_front();
            }
        }
        out[n..].fill(0);
        q.underrun_samples += (out.len() - n) as u64;
        let sum_sq: f64 = out[..n].iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
        if q.tap.len() == TAP_CAP {
            q.tap.pop_front();
        }
        q.tap.push_back(RenderTap { at: now, samples: out.len() as u32, real: n as u32, sum_sq, epoch: first, epoch_sample: first_cursor });
        n
    }

    pub fn pull_guarded(&self, out: &mut [i16], mut admit: impl FnMut(u64, u64, usize) -> bool) -> usize {
        if out.is_empty() { return 0; }
        let now = Instant::now();
        let mut q = self.inner.lock().unwrap();
        let mut n = 0;
        let mut vetoed = false;
        while n < out.len() {
            let Some(seg) = q.segs.front() else { break };
            let epoch = seg.epoch;
            let take = (seg.samples.len() - seg.pos).min(out.len() - n);
            let cursor = q.rendered.get(&epoch).map_or(0, |r| r.samples);
            if !admit(epoch, cursor, take) { vetoed = true; break; }
            let seg = q.segs.front_mut().expect("segment held under lock");
            out[n..n + take].copy_from_slice(&seg.samples[seg.pos..seg.pos + take]);
            seg.pos += take;
            let done = seg.pos == seg.samples.len();
            let sum_sq = out[n..n + take].iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
            n += take;
            q.queued -= take;
            if epoch < q.live_from { q.rendered_after_fence += take as u64; }
            let rendered = q.rendered.entry(epoch).or_default();
            rendered.samples += take as u64;
            rendered.first_at.get_or_insert(now);
            rendered.last_at = Some(now);
            if done { q.segs.pop_front(); }
            if q.tap.len() == TAP_CAP { q.tap.pop_front(); }
            q.tap.push_back(RenderTap { at: now, samples: take as u32, real: take as u32,
                sum_sq, epoch: Some(epoch), epoch_sample: cursor });
        }
        out[n..].fill(0);
        if !vetoed || n > 0 { q.pulls += 1; }
        if !vetoed {
            q.underrun_samples += (out.len() - n) as u64;
            if n == 0 {
                if q.tap.len() == TAP_CAP { q.tap.pop_front(); }
                q.tap.push_back(RenderTap { at: now, samples: out.len() as u32,
                    real: 0, sum_sq: 0.0, epoch: None, epoch_sample: 0 });
            } else if let Some(tap) = q.tap.back_mut() {
                tap.samples += (out.len() - n) as u32;
            }
        }
        n
    }

    pub fn drain_tap(&self) -> Vec<RenderTap> {
        self.inner.lock().unwrap().tap.drain(..).collect()
    }

    pub fn flush_through(&self, through: u64) -> Flushed {
        let at = Instant::now();
        let mut q = self.inner.lock().unwrap();
        q.live_from = q.live_from.max(through + 1);
        let live_from = q.live_from;
        let mut dropped: Vec<(u64, u64)> = Vec::new();
        let mut removed = 0usize;
        q.segs.retain(|s| {
            if s.epoch < live_from {
                let left = (s.samples.len() - s.pos) as u64;
                removed += left as usize;
                match dropped.iter_mut().find(|(e, _)| *e == s.epoch) {
                    Some(d) => d.1 += left,
                    None => dropped.push((s.epoch, left)),
                }
                false
            } else {
                true
            }
        });
        q.queued -= removed;
        drop(q);
        Flushed { dropped, took_ns: at.elapsed().as_nanos() as u64, at }
    }

    pub fn is_fenced(&self, epoch: u64) -> bool {
        epoch < self.inner.lock().unwrap().live_from
    }

    pub fn queued_for(&self, epoch: u64) -> usize {
        self.inner.lock().unwrap().segs.iter().filter(|s| s.epoch == epoch).map(|s| s.samples.len() - s.pos).sum()
    }

    pub fn rendered(&self, epoch: u64) -> Rendered {
        self.inner.lock().unwrap().rendered.get(&epoch).copied().unwrap_or_default()
    }

    pub fn stats(&self) -> QueueStats {
        let q = self.inner.lock().unwrap();
        QueueStats {
            queued: q.queued,
            underrun_samples: q.underrun_samples,
            overflow_samples: q.overflow_samples,
            rendered_after_fence: q.rendered_after_fence,
            pulls: q.pulls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_is_bounded_and_counts_what_it_refuses() {
        let q = PlayQueue::new(24_000, 1000);
        assert_eq!(q.push(1, vec![1; 600]), Push::Queued);
        assert_eq!(q.push(1, vec![2; 600]), Push::Overflow { queued: 400, dropped: 200 });
        assert_eq!(q.push(1, vec![3; 10]), Push::Overflow { queued: 0, dropped: 10 });
        assert_eq!(q.stats().queued, 1000);
        assert_eq!(q.stats().overflow_samples, 210);
        let mut out = vec![0; 700];
        assert_eq!(q.pull(&mut out), 700);
        assert_eq!((out[599], out[600]), (1, 2));
        assert_eq!(q.push(1, vec![4; 500]), Push::Queued, "room again after the device pulled");
    }

    #[test]
    fn a_flush_fences_old_epochs_and_a_late_chunk_is_dropped() {
        let q = PlayQueue::new(48_000, 48_000 * 30);
        q.push(1, vec![1; 4800]);
        q.push(2, vec![2; 9600]);
        let mut out = vec![0; 1000];
        q.pull(&mut out);
        let f = q.flush_through(2);
        assert_eq!(f.dropped, vec![(1, 3800), (2, 9600)]);
        assert_eq!(q.stats().queued, 0);

        assert_eq!(q.push(2, vec![9; 480]), Push::Fenced);
        assert_eq!(q.push(1, vec![9; 480]), Push::Fenced, "older epochs are fenced too");
        assert_eq!(q.push(3, vec![3; 480]), Push::Queued, "the next response plays");
        let mut out = vec![0; 960];
        assert_eq!(q.pull(&mut out), 480);
        assert!(out[..480].iter().all(|&s| s == 3) && out[480..].iter().all(|&s| s == 0));
        assert_eq!(q.stats().rendered_after_fence, 0);
        assert_eq!(q.rendered(1).samples, 1000);
        assert_eq!(q.rendered(2).samples, 0);
    }

    #[test]
    fn flush_latency_under_a_pulling_device() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let q = Arc::new(PlayQueue::with_seconds(48_000, 60.0));
        let stop = Arc::new(AtomicBool::new(false));
        let device = {
            let (q, stop) = (q.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut buf = vec![0i16; 480];
                while !stop.load(Ordering::Relaxed) {
                    q.pull(&mut buf);
                    std::thread::yield_now();
                }
            })
        };
        let mut took = Vec::new();
        for epoch in 1..=200u64 {

            for _ in 0..10 {
                q.push(epoch, vec![epoch as i16; 4800]);
            }
            let f = q.flush_through(epoch);
            assert_eq!(q.queued_for(epoch), 0);
            assert_eq!(q.push(epoch, vec![0; 480]), Push::Fenced);
            took.push(f.took_ns);
        }
        stop.store(true, Ordering::Relaxed);
        device.join().unwrap();
        assert_eq!(q.stats().rendered_after_fence, 0);
        took.sort();
        let (p50, max) = (took[took.len() / 2], took[took.len() - 1]);
        eprintln!("queue flush (n={}) p50 {:.1} µs max {:.1} µs", took.len(), p50 as f64 / 1e3, max as f64 / 1e3);
        assert!(p50 < 1_000_000, "a flush must not take a millisecond: p50 {p50} ns");
    }
}

#[cfg(test)]
#[path = "guarded_tests.rs"]
mod guarded_tests;
