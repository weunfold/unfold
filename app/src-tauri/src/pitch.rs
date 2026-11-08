

use std::ops::RangeInclusive;

pub const RANGE: RangeInclusive<f32> = -12.0..=12.0;

const FRAME_S: f64 = 0.030;
const SEARCH_S: f64 = 0.008;

pub struct Pitch {
    wsola: Wsola,
    cubic: Cubic,

    seen: u64,
    given: u64,
    held: Vec<f32>,
}

impl Pitch {

    pub fn new(semitones: Option<f32>, rate: u32) -> Result<Option<Pitch>, String> {
        let Some(s) = semitones else { return Ok(None) };
        if !s.is_finite() || !RANGE.contains(&s) {
            return Err(format!("pitch_semitones {s} is outside -12..=12"));
        }
        if s == 0.0 {
            return Ok(None);
        }
        let ratio = 2f64.powf(s as f64 / 12.0);
        Ok(Some(Pitch { wsola: Wsola::new(rate, ratio), cubic: Cubic::new(ratio), seen: 0, given: 0, held: Vec::new() }))
    }

    pub fn push(&mut self, input: &[i16]) -> Vec<i16> {
        self.seen += input.len() as u64;
        let x: Vec<f32> = input.iter().map(|&s| s as f32 / 32768.0).collect();
        let scaled = self.wsola.push(&x);
        let out = self.cubic.push(&scaled);
        self.held.extend(out);
        self.give(false)
    }

    pub fn finish(&mut self) -> Vec<i16> {
        let scaled = self.wsola.finish();
        let mut out = self.cubic.push(&scaled);
        out.extend(self.cubic.finish());
        self.held.extend(out);
        self.give(true)
    }

    fn give(&mut self, last: bool) -> Vec<i16> {
        let room = (self.seen - self.given) as usize;
        let n = self.held.len().min(room);
        let mut out: Vec<i16> = self.held.drain(..n).map(|v| (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16).collect();
        if last {
            out.resize(room, 0);
            self.held.clear();
        }
        self.given += out.len() as u64;
        out
    }
}

struct Wsola {
    n: usize,
    hs: usize,
    ha: f64,
    tol: usize,
    win: Vec<f32>,
    x: Vec<f32>,
    xb: usize,
    m: usize,
    prev: usize,
    y: Vec<f32>,
    yb: usize,
    skip: usize,
}

impl Wsola {
    fn new(rate: u32, ratio: f64) -> Self {
        let n = (((rate as f64 * FRAME_S) as usize) / 2 * 2).max(4);
        let hs = n / 2;
        let win = (0..n).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos()) as f32).collect();
        Wsola { n, hs, ha: hs as f64 / ratio, tol: (rate as f64 * SEARCH_S) as usize, win, x: vec![0.0; hs], xb: 0, m: 0, prev: 0, y: Vec::new(), yb: 0, skip: hs }
    }

    fn nominal(&self, m: usize) -> usize {
        (m as f64 * self.ha).round() as usize
    }

    fn ready(&self) -> bool {
        let end = self.xb + self.x.len();
        let hi = self.nominal(self.m) + self.tol;
        hi + self.n <= end && (self.m == 0 || self.prev + self.hs + self.n <= end)
    }

    fn frame(&mut self) {
        let (n, m) = (self.n, self.m);
        let start = if m == 0 {
            0
        } else {
            let nominal = self.nominal(m);
            let natural = self.prev + self.hs;
            let lo = nominal.saturating_sub(self.tol).max(self.xb);
            let r = &self.x[natural - self.xb..][..n];
            let mut best = (f32::MIN, nominal.max(lo));
            for s in lo..=nominal + self.tol {
                let c = &self.x[s - self.xb..][..n];
                let (mut dot, mut e) = (0f32, 1e-9f32);
                for i in 0..n {
                    dot += r[i] * c[i];
                    e += c[i] * c[i];
                }
                let score = dot / e.sqrt();
                if score > best.0 {
                    best = (score, s);
                }
            }
            best.1
        };
        let at = m * self.hs - self.yb;
        if self.y.len() < at + n {
            self.y.resize(at + n, 0.0);
        }
        let seg = &self.x[start - self.xb..][..n];
        for i in 0..n {
            self.y[at + i] += self.win[i] * seg[i];
        }
        self.prev = start;
        self.m += 1;

        let keep = (self.nominal(self.m).saturating_sub(self.tol)).min(self.prev + self.hs);
        if keep > self.xb {
            self.x.drain(..keep - self.xb);
            self.xb = keep;
        }
    }

    fn take(&mut self, upto: usize) -> Vec<f32> {
        let k = upto.saturating_sub(self.yb).min(self.y.len());
        let mut out: Vec<f32> = self.y.drain(..k).collect();
        self.yb += k;
        let drop = self.skip.min(out.len());
        out.drain(..drop);
        self.skip -= drop;
        out
    }

    fn push(&mut self, input: &[f32]) -> Vec<f32> {
        self.x.extend_from_slice(input);
        while self.ready() {
            self.frame();
        }
        self.take(self.m * self.hs)
    }

    fn finish(&mut self) -> Vec<f32> {
        let end = self.xb + self.x.len();
        self.x.resize(self.x.len() + self.n + 2 * self.tol + self.ha.ceil() as usize + self.hs, 0.0);
        while self.nominal(self.m) < end && self.ready() {
            self.frame();
        }
        self.take(usize::MAX)
    }
}

struct Cubic {
    step: f64,
    x: Vec<f32>,
    xb: usize,
    t: f64,
}

impl Cubic {
    fn new(step: f64) -> Self {
        Cubic { step, x: Vec::new(), xb: 0, t: 0.0 }
    }

    fn at(&self, i: isize) -> f32 {
        if i < self.xb as isize { 0.0 } else { self.x.get(i as usize - self.xb).copied().unwrap_or(0.0) }
    }

    fn run(&mut self, until: f64) -> Vec<f32> {
        let mut out = Vec::new();
        while self.t < until {
            let i = self.t.floor() as isize;
            let f = (self.t - i as f64) as f32;
            let (p0, p1, p2, p3) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
            out.push(p1 + 0.5 * f * (p2 - p0 + f * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + f * (3.0 * (p1 - p2) + p3 - p0))));
            self.t += self.step;
        }
        let keep = (self.t.floor() as usize).saturating_sub(1);
        if keep > self.xb {
            self.x.drain(..(keep - self.xb).min(self.x.len()));
            self.xb = keep;
        }
        out
    }

    fn push(&mut self, input: &[f32]) -> Vec<f32> {
        self.x.extend_from_slice(input);

        let end = (self.xb + self.x.len()) as f64 - 2.0;
        self.run(end)
    }

    fn finish(&mut self) -> Vec<f32> {
        let end = (self.xb + self.x.len()) as f64;
        self.run(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 24_000;

    fn sine(hz: f64, secs: f64) -> Vec<i16> {
        (0..(RATE as f64 * secs) as usize).map(|i| ((std::f64::consts::TAU * hz * i as f64 / RATE as f64).sin() * 12_000.0) as i16).collect()
    }

    fn hz(x: &[i16], rate: u32) -> f64 {
        let (a, b) = (x.len() / 4, x.len() * 3 / 4);
        let ups: Vec<f64> = (a..b).filter(|&i| x[i - 1] < 0 && x[i] >= 0).map(|i| (i - 1) as f64 + x[i - 1] as f64 / (x[i - 1] as f64 - x[i] as f64)).collect();
        (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]) * rate as f64
    }

    fn shifted(x: &[i16], semitones: f32) -> Vec<i16> {
        let mut p = Pitch::new(Some(semitones), RATE).unwrap().expect("a shift");
        let mut out = Vec::new();
        for c in x.chunks(4800) {
            out.extend(p.push(c));
        }
        out.extend(p.finish());
        out
    }

    #[test]
    fn minus_three_lowers_the_pitch_by_its_ratio_and_keeps_the_length() {
        let want = 2f64.powf(-3.0 / 12.0);
        for f in [110.0, 220.0, 440.0] {
            let x = sine(f, 2.0);
            let y = shifted(&x, -3.0);
            let len = y.len() as f64 / x.len() as f64;
            assert!((len - 1.0).abs() < 0.01, "{f} Hz: length {len}");
            let got = hz(&y, RATE) / hz(&x, RATE);
            assert!((got / want - 1.0).abs() < 0.01, "{f} Hz: pitch ratio {got}, want {want}");
        }
    }

    #[test]
    fn other_shifts_move_by_their_ratio_too() {
        for s in [-12.0f32, -5.0, 3.0, 7.0, 12.0] {
            let x = sine(200.0, 2.0);
            let y = shifted(&x, s);
            assert!((y.len() as f64 / x.len() as f64 - 1.0).abs() < 0.01, "{s}: length");
            let (got, want) = (hz(&y, RATE) / 200.0, 2f64.powf(s as f64 / 12.0));
            assert!((got / want - 1.0).abs() < 0.01, "{s}: pitch ratio {got}, want {want}");
        }
    }

    #[test]
    fn absent_or_zero_shifts_nothing() {
        assert!(Pitch::new(None, RATE).unwrap().is_none());
        assert!(Pitch::new(Some(0.0), RATE).unwrap().is_none());
    }

    #[test]
    fn out_of_range_is_refused() {
        for s in [12.5f32, -13.0, f32::NAN, f32::INFINITY] {
            assert!(Pitch::new(Some(s), RATE).is_err(), "{s}");
        }
        assert!(Pitch::new(Some(12.0), RATE).is_ok() && Pitch::new(Some(-12.0), RATE).is_ok(), "the ends are in");
    }

    #[test]
    fn the_output_never_runs_ahead_of_the_input() {
        let x = sine(220.0, 1.0);
        let mut p = Pitch::new(Some(-3.0), RATE).unwrap().unwrap();
        let (mut seen, mut given) = (0, 0);
        for c in x.chunks(333) {
            seen += c.len();
            given += p.push(c).len();
            assert!(given <= seen);
        }
        assert_eq!(given + p.finish().len(), x.len());
    }
}
