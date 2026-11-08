

pub struct Resampler {
    from: u32,
    to: u32,

    pos: f64,
    prev: f32,
    primed: bool,

    taps: usize,
    hist: Vec<f32>,
    hist_i: usize,
    hist_sum: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let taps = if from > to { (from as f64 / to as f64).round().max(1.0) as usize } else { 1 };
        Self { from, to, pos: 0.0, prev: 0.0, primed: false, taps, hist: vec![0.0; taps], hist_i: 0, hist_sum: 0.0 }
    }

    pub fn is_identity(&self) -> bool {
        self.from == self.to
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.is_identity() {
            out.extend_from_slice(input);
            return;
        }
        let step = self.from as f64 / self.to as f64;
        for &raw in input {
            let x = self.filter(raw);
            if !self.primed {
                self.prev = x;
                self.primed = true;
                continue;
            }

            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.push(self.prev + (x - self.prev) * t);
                self.pos += step;
            }
            self.pos -= 1.0;
            self.prev = x;
        }
    }

    pub fn process_i16(&mut self, input: &[i16], out: &mut Vec<i16>) {
        if self.is_identity() {
            out.extend_from_slice(input);
            return;
        }
        let f: Vec<f32> = input.iter().map(|&s| s as f32).collect();
        let mut o = Vec::with_capacity(input.len() * self.to as usize / self.from as usize + 2);
        self.process(&f, &mut o);
        out.extend(o.into_iter().map(|v| v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16));
    }

    fn filter(&mut self, x: f32) -> f32 {
        if self.taps == 1 {
            return x;
        }
        self.hist_sum += x - self.hist[self.hist_i];
        self.hist[self.hist_i] = x;
        self.hist_i = (self.hist_i + 1) % self.taps;
        self.hist_sum / self.taps as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_convert_with_the_right_length_in_streaming_pieces() {
        for (from, to) in [(24_000, 48_000), (48_000, 16_000), (44_100, 16_000), (16_000, 16_000), (24_000, 44_100)] {
            let mut r = Resampler::new(from, to);
            let mut out = Vec::new();
            let input: Vec<i16> = (0..from).map(|i| ((i as f32 * 0.01).sin() * 8000.0) as i16).collect();
            for piece in input.chunks(333) {
                r.process_i16(piece, &mut out);
            }
            let expect = to as i64;
            assert!((out.len() as i64 - expect).abs() <= 3, "{from}->{to}: {} samples", out.len());
        }
    }

    #[test]
    fn upsampling_interpolates_between_neighbours() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process_i16(&[0, 100, 200], &mut out);
        assert_eq!(out, vec![0, 50, 100, 150]);
    }
}
