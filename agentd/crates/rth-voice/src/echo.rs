

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::queue::RenderTap;

pub const FRAME_MS: u32 = 20;
const FRAME: usize = (16_000 / 1000 * FRAME_MS) as usize;
const REF_CAP: usize = 4096;
const MIC_CAP: usize = 1500;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoConfig {

    pub delay_ms: f32,

    pub spread_ms: f32,

    pub coupling_db: f32,

    pub margin_db: f32,

    pub tail_ms: f32,

    pub hold_ms: u32,

    pub gap_frames: u32,

    pub floor_dbfs: f32,

    pub onset_frames: u32,

    pub hang_frames: u32,
}

impl Default for EchoConfig {
    fn default() -> Self {
        Self {
            delay_ms: 120.0,
            spread_ms: 40.0,
            coupling_db: -10.0,
            margin_db: 6.0,
            tail_ms: 250.0,
            hold_ms: 250,
            gap_frames: 2,
            floor_dbfs: -38.0,
            onset_frames: 3,
            hang_frames: 20,
        }
    }
}

impl EchoConfig {
    pub fn describe(&self) -> String {
        format!(
            "delay {:.0}±{:.0} ms, coupling {:+.1} dB, margin {:.1} dB, tail {:.0} ms, hold {} ms (gaps {}), floor {:.1} dBFS",
            self.delay_ms, self.spread_ms, self.coupling_db, self.margin_db, self.tail_ms, self.hold_ms, self.gap_frames, self.floor_dbfs
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearEnd {

    pub start: Instant,

    pub confirmed: Instant,
    pub frame_db: f32,
    pub threshold_db: f32,

    pub echo_db: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probe {
    pub frames: usize,
    pub lag_ms: u32,
    pub corr: f32,

    pub coupling_db: Option<f32>,
    pub mic_db_p50: f32,
    pub ref_db_p50: f32,
}

fn db(rms: f64) -> f32 {
    if rms > 1e-9 { (20.0 * rms.log10()) as f32 } else { -120.0 }
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(v[v.len() / 2])
}

pub struct EchoMonitor {
    cfg: EchoConfig,
    refs: VecDeque<(Instant, f32)>,
    mics: VecDeque<(Instant, f32)>,

    sum_sq: f64,
    in_frame: usize,

    run: u32,
    gaps: u32,
    run_start: Option<Instant>,
    confirmed: bool,
    quiet: u32,
}

impl EchoMonitor {
    pub fn new(cfg: EchoConfig) -> Self {
        Self { cfg, refs: VecDeque::new(), mics: VecDeque::new(), sum_sq: 0.0, in_frame: 0, run: 0, gaps: 0, run_start: None, confirmed: false, quiet: 0 }
    }

    pub fn config(&self) -> EchoConfig {
        self.cfg
    }

    pub fn push_render(&mut self, taps: &[RenderTap]) {
        for t in taps {
            if self.refs.len() == REF_CAP {
                self.refs.pop_front();
            }
            self.refs.push_back((t.at, db(t.rms())));
        }
    }

    pub fn echo_db(&self, end: Instant) -> Option<f32> {
        let c = &self.cfg;
        let ms = |v: f32| Duration::from_secs_f32(v.max(0.0) / 1000.0);
        let hi = end.checked_sub(ms(c.delay_ms - c.spread_ms))?;
        let lo = end.checked_sub(ms(c.delay_ms + c.spread_ms + FRAME_MS as f32))?;
        let tail_lo = lo.checked_sub(ms(c.tail_ms)).unwrap_or(lo);
        let mut best: Option<f32> = None;
        for &(at, level) in self.refs.iter().rev() {
            if at > hi {
                continue;
            }
            if at < tail_lo {
                break;
            }
            let l = if at >= lo {
                level
            } else if c.tail_ms > 0.0 {
                level - 60.0 * (lo - at).as_secs_f32() * 1000.0 / c.tail_ms
            } else {
                continue;
            };
            if l > -90.0 {
                best = Some(best.map_or(l, |b: f32| b.max(l)));
            }
        }
        best.map(|b| b + c.coupling_db)
    }

    pub fn push_mic(&mut self, at: Instant, samples: &[i16]) -> Vec<NearEnd> {
        let mut out = Vec::new();
        let n = samples.len();
        for (i, &s) in samples.iter().enumerate() {
            let x = s as f64 / 32768.0;
            self.sum_sq += x * x;
            self.in_frame += 1;
            if self.in_frame == FRAME {
                let end = at.checked_sub(Duration::from_secs_f64((n - 1 - i) as f64 / 16_000.0)).unwrap_or(at);
                let level = db((self.sum_sq / FRAME as f64).sqrt());
                self.sum_sq = 0.0;
                self.in_frame = 0;
                if self.mics.len() == MIC_CAP {
                    self.mics.pop_front();
                }
                self.mics.push_back((end, level));
                if let Some(e) = self.frame(end, level) {
                    out.push(e);
                }
            }
        }
        out
    }

    fn frame(&mut self, end: Instant, level: f32) -> Option<NearEnd> {
        let c = self.cfg;
        let echo = self.echo_db(end);
        let threshold = match echo {
            Some(e) => c.floor_dbfs.max(e + c.margin_db),
            None => c.floor_dbfs,
        };
        let need = match echo {
            Some(_) => (c.hold_ms / FRAME_MS).max(1),
            None => c.onset_frames.max(1),
        };
        if level >= threshold {
            self.quiet = 0;
            if self.run == 0 {
                self.run_start = Some(end);
            }
            self.run += 1;
            if !self.confirmed && self.run >= need {
                self.confirmed = true;
                return Some(NearEnd { start: self.run_start.unwrap_or(end), confirmed: end, frame_db: level, threshold_db: threshold, echo_db: echo });
            }
        } else {
            self.quiet += 1;
            if self.run > 0 && !self.confirmed && self.gaps < c.gap_frames {
                self.gaps += 1;
            } else if !self.confirmed {
                self.run = 0;
                self.gaps = 0;
            }
            if self.confirmed && self.quiet >= c.hang_frames {
                self.confirmed = false;
                self.run = 0;
                self.gaps = 0;
            }
        }
        None
    }

    pub fn probe(&self, from: Instant, to: Instant) -> Option<Probe> {
        let frames: Vec<(Instant, f32)> = self.mics.iter().copied().filter(|(t, _)| *t >= from && *t <= to).collect();
        if frames.len() < 10 {
            return None;
        }
        let max_lag = 500u32;
        let base = from.checked_sub(Duration::from_millis((max_lag + FRAME_MS + 20) as u64))?;
        let slot = |t: Instant| -> Option<usize> { t.checked_duration_since(base).map(|d| (d.as_millis() / 10) as usize) };
        let slots = slot(to)? + 2;
        let mut grid = vec![-120f32; slots];
        for &(at, level) in &self.refs {
            if let Some(i) = slot(at)
                && i < slots
            {
                grid[i] = grid[i].max(level);
            }
        }
        let ref_at = |end: Instant, lag: u32| -> f32 {
            let hi = end.checked_sub(Duration::from_millis(lag as u64));
            let lo = hi.and_then(|h| h.checked_sub(Duration::from_millis(FRAME_MS as u64)));
            match (lo.and_then(slot), hi.and_then(slot)) {
                (Some(a), Some(b)) => grid[a.min(slots - 1)..=b.min(slots - 1)].iter().copied().fold(-120.0, f32::max),
                _ => -120.0,
            }
        };
        let lin = |d: f32| 10f64.powf(d as f64 / 20.0);
        let mic: Vec<f64> = frames.iter().map(|(_, l)| lin(*l)).collect();
        let (mut best_lag, mut best_corr) = (0u32, f64::MIN);
        for lag in (0..=max_lag).step_by(10) {
            let r: Vec<f64> = frames.iter().map(|(t, _)| lin(ref_at(*t, lag))).collect();
            let c = pearson(&mic, &r);
            if c > best_corr {
                best_corr = c;
                best_lag = lag;
            }
        }
        let refs: Vec<f32> = frames.iter().map(|(t, _)| ref_at(*t, best_lag)).collect();
        let coupling = median(frames.iter().zip(&refs).filter(|(_, r)| **r > -50.0).map(|((_, m), r)| m - r).collect());
        Some(Probe {
            frames: frames.len(),
            lag_ms: best_lag,
            corr: best_corr as f32,
            coupling_db: coupling,
            mic_db_p50: median(frames.iter().map(|(_, l)| *l).collect()).unwrap_or(-120.0),
            ref_db_p50: median(refs).unwrap_or(-120.0),
        })
    }
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma).powi(2);
        sbb += (y - mb).powi(2);
    }
    if saa <= 0.0 || sbb <= 0.0 { 0.0 } else { sab / (saa * sbb).sqrt() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(m: &mut EchoMonitor, t0: Instant, ms: u64, rms: f64) {
        let taps: Vec<RenderTap> = (0..ms / 10)
            .map(|i| RenderTap { at: t0 + Duration::from_millis(i * 10), samples: 480, real: 480, sum_sq: rms * rms * 480.0, epoch: Some(1), epoch_sample: i * 480 })
            .collect();
        m.push_render(&taps);
    }

    fn level(dbfs: f32, ms: u32) -> Vec<i16> {

        let a = (32768.0 * 10f32.powf(dbfs / 20.0)) as i16;
        (0..16 * ms as usize).map(|i| if (i / 8) % 2 == 0 { a } else { -a }).collect()
    }

    #[test]
    fn the_echo_estimate_follows_the_rendered_level_at_the_delay_and_decays_in_the_tail() {
        let mut m = EchoMonitor::new(EchoConfig { delay_ms: 100.0, spread_ms: 20.0, coupling_db: -10.0, tail_ms: 200.0, ..Default::default() });
        let t0 = Instant::now();
        render(&mut m, t0, 1000, 0.1);
        assert_eq!(m.echo_db(t0 + Duration::from_millis(50)), None, "before the delay nothing has come back");
        let mid = m.echo_db(t0 + Duration::from_millis(500)).unwrap();
        assert!((mid - -30.0).abs() < 0.5, "{mid}");
        let tail = m.echo_db(t0 + Duration::from_millis(1000 + 100 + 100)).unwrap();
        assert!(tail < mid - 10.0 && tail > mid - 40.0, "{tail}");
        assert_eq!(m.echo_db(t0 + Duration::from_millis(1000 + 100 + 600)), None, "the tail is over");
    }

    #[test]
    fn the_gate_needs_the_hold_over_the_echo_and_only_the_onset_without_reference() {
        let cfg = EchoConfig { delay_ms: 100.0, spread_ms: 20.0, coupling_db: -10.0, margin_db: 6.0, tail_ms: 0.0, hold_ms: 240, ..Default::default() };
        let t0 = Instant::now();

        let mut m = EchoMonitor::new(cfg);
        let ev = m.push_mic(t0 + Duration::from_millis(100), &level(-20.0, 100));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].echo_db, None);

        let mut m = EchoMonitor::new(cfg);
        render(&mut m, t0, 3000, 0.1);
        let at = t0 + Duration::from_millis(1500);
        assert!(m.push_mic(at, &level(-26.0, 1000)).is_empty());
        let start = at + Duration::from_millis(200);
        let ev = m.push_mic(start + Duration::from_millis(400), &level(-18.0, 400));
        assert_eq!(ev.len(), 1, "{ev:?}");
        let took = ev[0].confirmed - ev[0].start;
        assert!((took.as_millis() as i64 - 220).abs() <= 20, "{took:?}");
        assert!((ev[0].threshold_db - -24.0).abs() < 0.5);
    }

    #[test]
    fn the_probe_finds_the_delay_and_the_coupling_of_a_modulated_echo() {
        let mut m = EchoMonitor::new(EchoConfig::default());
        let t0 = Instant::now();

        let on = |ms: u64| {
            let mut x = (ms / 40).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5555;
            x ^= x >> 29;
            x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x ^= x >> 32;
            !x.is_multiple_of(3)
        };
        let taps: Vec<RenderTap> = (0..300u64)
            .map(|i| {
                let rms = if on(i * 10) { 0.1 } else { 0.0 };
                RenderTap { at: t0 + Duration::from_millis(i * 10), samples: 480, real: 480, sum_sq: rms * rms * 480.0, epoch: Some(1), epoch_sample: i * 480 }
            })
            .collect();
        m.push_render(&taps);

        for f in 0..150u64 {
            let end = t0 + Duration::from_millis((f + 1) * 20);
            let src = (f * 20).checked_sub(150);
            let loud = src.is_some_and(on);
            m.push_mic(end, &level(if loud { -32.0 } else { -70.0 }, 20));
        }
        let p = m.probe(t0 + Duration::from_millis(300), t0 + Duration::from_millis(2900)).unwrap();
        assert!((p.lag_ms as i64 - 150).abs() <= 20, "{p:?}");
        assert!(p.corr > 0.6, "{p:?}");
        assert!((p.coupling_db.unwrap() - -12.0).abs() < 1.5, "{p:?}");
    }
}
