

use std::time::{Duration, Instant};

use crate::echo::{EchoConfig, EchoMonitor};
use crate::queue::RenderTap;
use crate::vad::{Vad, VadConfig, VadEvent};

pub const RATE: u32 = 16_000;
const PERIOD: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoPath {
    pub delay_ms: f32,
    pub gain_db: f32,
    pub tail_ms: f32,

    pub erle_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Policy {

    LocalEnergy(VadConfig),

    Gated(EchoConfig),

    ProviderOnly(VadConfig),
}

impl Policy {
    pub fn name(&self) -> &'static str {
        match self {
            Policy::LocalEnergy(_) => "local_energy",
            Policy::Gated(_) => "gated",
            Policy::ProviderOnly(_) => "provider_only",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Near {
    pub at_ms: u32,
    pub samples: Vec<i16>,
}

#[derive(Debug, Clone)]
pub struct Scenario {

    pub far: Option<Vec<i16>>,
    pub far_at_ms: u32,
    pub echo: EchoPath,
    pub near: Vec<Near>,

    pub noise_dbfs: Option<f32>,
    pub total_ms: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {

    pub cut_ms: Option<u32>,

    pub false_cut: bool,

    pub playback_ms: u32,

    pub interruptions: Vec<Option<(u32, u32)>>,

    pub local_onsets: u32,

    pub local_onsets_unexplained: u32,
}

pub fn normalize(x: &[i16], dbfs: f32) -> Vec<i16> {
    let mut acc = 0.0f64;
    let mut n = 0usize;
    for f in x.chunks(320) {
        let e: f64 = f.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum::<f64>() / f.len() as f64;
        if e > 1e-5 {
            acc += e * f.len() as f64;
            n += f.len();
        }
    }
    if n == 0 {
        return x.to_vec();
    }
    let rms = (acc / n as f64).sqrt();
    let g = 10f64.powf(dbfs as f64 / 20.0) / rms;
    x.iter().map(|&s| (s as f64 * g).round().clamp(-32768.0, 32767.0) as i16).collect()
}

pub fn noise(dbfs: f32, n: usize, seed: u32) -> Vec<f32> {
    let amp = 10f32.powf(dbfs / 20.0) * 3f32.sqrt();
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * amp
        })
        .collect()
}

pub fn run(s: &Scenario, policy: Policy) -> Outcome {
    let total = (s.total_ms as usize * RATE as usize) / 1000;
    let far = s.far.clone().unwrap_or_default();
    let far_at = s.far_at_ms as usize * RATE as usize / 1000;
    let delay = (s.echo.delay_ms * RATE as f32 / 1000.0).round() as usize;
    let gain = 10f32.powf((s.echo.gain_db - s.echo.erle_db) / 20.0);

    let loop_n = (0.037 * RATE as f32) as usize;
    let fb = if s.echo.tail_ms > 0.0 { 10f32.powf(-3.0 * 37.0 / s.echo.tail_ms) } else { 0.0 };
    let mut near = vec![0f32; total];
    let mut labels: Vec<(usize, usize)> = Vec::new();
    for e in &s.near {
        let at = e.at_ms as usize * RATE as usize / 1000;
        let mut last = at;
        for (i, &v) in e.samples.iter().enumerate() {
            if at + i < total {
                near[at + i] += v as f32 / 32768.0;
                if v.unsigned_abs() > 300 {
                    last = at + i;
                }
            }
        }
        let first = e.samples.iter().position(|v| v.unsigned_abs() > 300).map_or(at, |p| at + p);
        labels.push((first, last));
    }
    let nz = s.noise_dbfs.map(|d| noise(d, total, 0xA5A5_1234)).unwrap_or_else(|| vec![0.0; total]);

    let base = Instant::now();
    let at = |n: usize| base + Duration::from_secs_f64(n as f64 / RATE as f64);
    let mut rendered = vec![0f32; total];
    let mut cut: Option<usize> = None;
    let mut stop_render: Option<usize> = None;
    let (vad_cfg, gate_cfg) = match policy {
        Policy::LocalEnergy(v) | Policy::ProviderOnly(v) => (v, EchoConfig::default()),
        Policy::Gated(g) => (VadConfig::default(), g),
    };
    let mut vad = Vad::new(vad_cfg);
    let mut mon = EchoMonitor::new(gate_cfg);
    let mut comb = vec![0f32; total];
    let mut mic = vec![0i16; total];
    let mut out = Outcome::default();
    let far_end = far_at + far.len();
    let audible = |n: usize, stop: Option<usize>| n >= far_at && n < far_end && stop.is_none_or(|s| n < s);
    let explained = |n: usize| labels.iter().any(|&(a, b)| n >= a && n <= b + RATE as usize / 2);

    let mut n = 0;
    while n < total {

        let p_end = (n + PERIOD).min(total);
        let mut sum_sq = 0.0;
        for k in n..p_end {
            let v = if audible(k, stop_render) { far[k - far_at] as f32 / 32768.0 } else { 0.0 };
            rendered[k] = v;
            sum_sq += (v as f64).powi(2);
        }
        mon.push_render(&[RenderTap { at: at(n), samples: (p_end - n) as u32, real: (p_end - n) as u32, sum_sq, epoch: Some(1), epoch_sample: n as u64 }]);
        for k in n..p_end {
            let direct = if k >= delay { rendered[k - delay] } else { 0.0 };
            let c = direct + if k >= loop_n { fb * comb[k - loop_n] } else { 0.0 };
            comb[k] = c;
            let x = gain * c + near[k] + nz[k];
            mic[k] = (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        }

        if (p_end.is_multiple_of(320) || p_end == total) && p_end >= 320 {
            let f0 = p_end - 320.min(p_end);
            let frame = &mic[f0..p_end];
            let now = p_end;
            let playing = audible(now.saturating_sub(1), stop_render);
            let mut want_cut = false;
            for ev in vad.push(frame) {
                if let VadEvent::SpeechStart { .. } = ev {
                    out.local_onsets += 1;
                    if playing && !explained(now) {
                        out.local_onsets_unexplained += 1;
                    }
                    if matches!(policy, Policy::LocalEnergy(_)) {
                        want_cut = true;
                    }
                }
            }
            let gated = mon.push_mic(at(p_end - 1), frame);
            if matches!(policy, Policy::Gated(_)) && !gated.is_empty() {
                want_cut = true;
            }
            if want_cut && playing && cut.is_none() {
                cut = Some(now);

                stop_render = Some(now);
            }
        }
        n = p_end;
    }
    let played_end = stop_render.unwrap_or(far_end).min(far_end).max(far_at);
    out.playback_ms = if s.far.is_some() { ((played_end - far_at) * 1000 / RATE as usize) as u32 } else { 0 };
    out.cut_ms = cut.map(|c| (c * 1000 / RATE as usize) as u32);
    out.false_cut = cut.is_some_and(|c| !explained(c));
    for &(a, b) in &labels {
        if !(a >= far_at && a < far_end) {
            continue;
        }
        let hit = cut.filter(|&c| c >= a && c <= b + RATE as usize / 2);
        let cut_before = cut.is_some_and(|c| c < a);
        if cut_before {
            continue;
        }
        out.interruptions.push(hit.map(|c| {
            let decision = ((c - a) * 1000 / RATE as usize) as u32;
            (decision, decision + 10)
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_speech(ms: u32, dbfs: f32) -> Vec<i16> {

        let t = crate::vad::tone(180.0, dbfs + 3.0, ms, RATE);
        t.iter().enumerate().map(|(i, &v)| if (i / 16) % 250 < 150 { v } else { 0 }).collect()
    }

    #[test]
    fn a_raw_echo_cuts_the_reply_under_the_energy_vad_but_not_under_an_aec_model() {
        let far = tone_speech(6000, -20.0);
        let s = |erle| Scenario { far: Some(far.clone()), far_at_ms: 500, echo: EchoPath { delay_ms: 150.0, gain_db: -12.0, tail_ms: 150.0, erle_db: erle }, near: vec![], noise_dbfs: Some(-60.0), total_ms: 7000 };
        let raw = run(&s(0.0), Policy::LocalEnergy(VadConfig::default()));
        assert!(raw.false_cut, "{raw:?}");
        let c = raw.cut_ms.unwrap();
        assert!((650..=800).contains(&c), "cut ~150 ms delay + 60 ms onset after the start: {c}");
        let aec = run(&s(25.0), Policy::LocalEnergy(VadConfig::default()));
        assert_eq!(aec.cut_ms, None, "{aec:?}");
        assert_eq!(aec.playback_ms, 6000);
    }
}

pub mod matrix {
    use super::*;
    use std::path::Path;

    pub struct Fixtures {
        pub far: Vec<i16>,
        pub barges: Vec<(&'static str, Vec<i16>)>,
    }

    pub fn load(dir: &Path) -> Result<Fixtures, String> {
        let read = |n: &str| crate::wav::read(&dir.join(format!("{n}.wav"))).map(|w| w.to_i16_at(RATE));
        Ok(Fixtures {
            far: normalize(&read("agent-reply")?, -20.0),
            barges: vec![("short", read("barge-short")?), ("long", read("barge-long")?), ("word", read("barge-word")?)],
        })
    }

    pub const DELAYS: [f32; 3] = [100.0, 150.0, 200.0];
    pub const GAINS: [f32; 3] = [-6.0, -12.0, -20.0];
    pub const TAILS: [f32; 3] = [0.0, 150.0, 400.0];

    #[derive(Debug, Clone, Default)]
    pub struct Row {
        pub policy: String,
        pub mic: String,
        pub echo_runs: usize,
        pub false_cuts: usize,
        pub playback_min: f64,
        pub first_false_cut_ms: Vec<u32>,
        pub detected: usize,
        pub missed: usize,

        pub by_level: Vec<(f32, usize, usize)>,
        pub decision_ms: Vec<u32>,
        pub to_last_sample_ms: Vec<u32>,
        pub local_onsets_unexplained: usize,
        pub noise_false_cuts: usize,
        pub noise_playback_min: f64,
    }

    impl Row {
        pub fn false_cuts_per_min(&self) -> f64 {
            let m = self.playback_min + self.noise_playback_min;
            if m > 0.0 { (self.false_cuts + self.noise_false_cuts) as f64 / m } else { 0.0 }
        }
    }

    pub fn pct(v: &[u32], p: f64) -> Option<u32> {
        let mut v = v.to_vec();
        v.sort();
        (!v.is_empty()).then(|| v[((v.len() - 1) as f64 * p).round() as usize])
    }

    pub fn row(f: &Fixtures, policy: Policy, erle_db: f32, levels: &[f32], oracle: bool) -> Row {
        let name = if oracle { format!("{}_measured", policy.name()) } else { policy.name().into() };
        let mut r = Row { policy: name, mic: if erle_db > 0.0 { format!("aec-model {erle_db:.0} dB") } else { "raw".into() }, ..Default::default() };
        let base_policy = policy;
        let far_ms = (f.far.len() as u64 * 1000 / RATE as u64) as u32;
        for &delay_ms in &DELAYS {
            for &gain_db in &GAINS {
                for &tail_ms in &TAILS {
                    let echo = EchoPath { delay_ms, gain_db, tail_ms, erle_db };
                    let policy = match base_policy {
                        Policy::Gated(mut g) if oracle => {
                            g.delay_ms = delay_ms;
                            g.coupling_db = gain_db - erle_db;
                            g.tail_ms = tail_ms.max(50.0);
                            Policy::Gated(g)
                        }
                        p => p,
                    };

                    let o = run(&Scenario { far: Some(f.far.clone()), far_at_ms: 500, echo, near: vec![], noise_dbfs: Some(-60.0), total_ms: far_ms + 1500 }, policy);
                    r.echo_runs += 1;
                    r.playback_min += o.playback_ms as f64 / 60_000.0;
                    r.local_onsets_unexplained += o.local_onsets_unexplained as usize;
                    if o.false_cut {
                        r.false_cuts += 1;
                        r.first_false_cut_ms.push(o.cut_ms.unwrap() - 500);
                    }

                    for &level in levels {
                        let mut det = 0;
                        let mut tot = 0;
                        for (_, b) in &f.barges {
                            let near = vec![Near { at_ms: 6500, samples: normalize(b, level) }];
                            let o = run(&Scenario { far: Some(f.far.clone()), far_at_ms: 500, echo, near, noise_dbfs: Some(-60.0), total_ms: far_ms + 1500 }, policy);
                            for i in &o.interruptions {
                                tot += 1;
                                match i {
                                    Some((d, l)) => {
                                        det += 1;
                                        r.detected += 1;
                                        r.decision_ms.push(*d);
                                        r.to_last_sample_ms.push(*l);
                                    }
                                    None => r.missed += 1,
                                }
                            }
                        }
                        match r.by_level.iter_mut().find(|(l, _, _)| *l == level) {
                            Some(e) => {
                                e.1 += det;
                                e.2 += tot;
                            }
                            None => r.by_level.push((level, det, tot)),
                        }
                    }
                }
            }
        }

        for nz in [-45.0, -35.0] {
            let echo = EchoPath { delay_ms: 150.0, gain_db: -12.0, tail_ms: 150.0, erle_db };
            let policy = match base_policy {
                Policy::Gated(mut g) if oracle => {
                    g.delay_ms = 150.0;
                    g.coupling_db = -12.0 - erle_db;
                    g.tail_ms = 150.0;
                    Policy::Gated(g)
                }
                p => p,
            };
            let o = run(&Scenario { far: Some(f.far.clone()), far_at_ms: 500, echo, near: vec![], noise_dbfs: Some(nz), total_ms: far_ms + 1500 }, policy);
            r.noise_playback_min += o.playback_ms as f64 / 60_000.0;
            r.noise_false_cuts += o.false_cut as usize;
        }
        r
    }

    pub fn near_only(f: &Fixtures, level: f32) -> (usize, usize) {
        let mut seen = 0;
        for (_, b) in &f.barges {
            let o = run(
                &Scenario { far: None, far_at_ms: 0, echo: EchoPath { delay_ms: 150.0, gain_db: -12.0, tail_ms: 150.0, erle_db: 0.0 }, near: vec![Near { at_ms: 500, samples: normalize(b, level) }], noise_dbfs: Some(-60.0), total_ms: 6000 },
                Policy::ProviderOnly(VadConfig::default()),
            );
            assert!(o.cut_ms.is_none());
            seen += (o.local_onsets > 0) as usize;
        }
        (seen, f.barges.len())
    }
}
