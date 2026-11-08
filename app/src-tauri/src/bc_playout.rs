
use std::sync::atomic::{fence, AtomicU64, Ordering};
pub const RATE: u64 = 48_000;
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub samples: u64,
    pub onset: u64,
    pub stop: u64,
    pub pause: u64,
    pub speaking: bool,
    pub filler: bool,
}
impl Snapshot {
    pub fn advance(&mut self, frames: usize, rendered: usize, reply: bool) {
        let speaking = rendered > 0 && reply;
        if speaking && !self.speaking {
            self.onset = self.samples;
        }
        if !speaking && self.speaking {
            self.stop = self.samples;
        }
        self.speaking = speaking;
        self.filler = rendered > 0 && !reply;
        self.samples += frames as u64;
        if !speaking && self.samples.saturating_sub(self.stop) >= RATE / 5 {
            self.pause = self.stop;
        }
    }
    pub fn frame(&self, at: u64) -> [f32; 8] {
        let age = |event: u64| at.saturating_sub(event).min(RATE * 5) as f32 / (RATE * 5) as f32;
        let speaking = if self.speaking {
            at >= self.onset
        } else {
            at >= self.onset && at < self.stop
        };
        [
            if speaking { 1. } else { 0. },
            age(self.onset),
            age(self.pause),
            age(self.stop),
            if self.filler { 1. } else { 0. },
            0.,
            0.,
            0.,
        ]
    }
}
static REV: AtomicU64 = AtomicU64::new(0);
static SAMPLE: AtomicU64 = AtomicU64::new(0);
static ONSET: AtomicU64 = AtomicU64::new(0);
static STOP: AtomicU64 = AtomicU64::new(0);
static PAUSE: AtomicU64 = AtomicU64::new(0);
static FLAGS: AtomicU64 = AtomicU64::new(0);
fn read() -> Snapshot {
    let flags = FLAGS.load(Ordering::Relaxed);
    Snapshot {
        samples: SAMPLE.load(Ordering::Relaxed),
        onset: ONSET.load(Ordering::Relaxed),
        stop: STOP.load(Ordering::Relaxed),
        pause: PAUSE.load(Ordering::Relaxed),
        speaking: flags & 1 != 0,
        filler: flags & 2 != 0,
    }
}

pub fn rendered(frames: usize, real: usize, reply: bool) {
    REV.fetch_add(1, Ordering::AcqRel);
    fence(Ordering::Release);
    let mut state = read();
    state.advance(frames, real, reply);
    SAMPLE.store(state.samples, Ordering::Relaxed);
    ONSET.store(state.onset, Ordering::Relaxed);
    STOP.store(state.stop, Ordering::Relaxed);
    PAUSE.store(state.pause, Ordering::Relaxed);
    FLAGS.store(
        u64::from(state.speaking) | u64::from(state.filler) << 1,
        Ordering::Relaxed,
    );
    REV.fetch_add(1, Ordering::Release);
}

pub fn snapshot() -> Option<Snapshot> {
    for _ in 0..2 {
        let before = REV.load(Ordering::Acquire);
        if before & 1 != 0 {
            continue;
        }
        let state = read();
        fence(Ordering::Acquire);
        if before == REV.load(Ordering::Acquire) {
            return Some(state);
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_speech_is_not_rendered_speech_and_hold_advances_only_device_clock() {
        let mut s = Snapshot::default();
        s.advance(480, 0, true);
        assert_eq!(s.frame(480)[0], 0.);
        s.advance(480, 480, true);
        assert_eq!(s.frame(960)[0], 1.);
        let cursor = s.samples;
        s.advance(480, 0, true);
        assert_eq!(s.stop, cursor);
        assert_eq!(s.frame(s.samples)[0], 0.);
        for _ in 0..20 {
            s.advance(480, 0, true);
        }
        assert_eq!(s.pause, cursor);
        assert_eq!(s.frame(s.samples)[7], 0.);
        s.advance(480, 480, true);
        assert_eq!(s.onset, s.samples - 480);
    }
    #[test]
    fn frame_ages_are_clipped_and_pause_comes_from_rendered_silence() {
        let mut s = Snapshot::default();
        s.advance(48000, 48000, true);
        assert_eq!(s.frame(48000)[1], 0.2);
        s.advance(480000, 0, true);
        assert_eq!(s.frame(s.samples)[1], 1.);
        assert_eq!(s.pause, 48000);
    }
}
