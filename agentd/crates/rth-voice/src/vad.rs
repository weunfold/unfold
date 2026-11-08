

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadConfig {
    pub sample_rate: u32,
    pub frame_ms: u32,
    pub threshold_dbfs: f32,
    pub onset_frames: u32,
    pub hang_frames: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self { sample_rate: 16_000, frame_ms: 20, threshold_dbfs: -38.0, onset_frames: 3, hang_frames: 20 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {

    SpeechStart { start_sample: u64, detected_sample: u64 },
    SpeechEnd { end_sample: u64 },
}

pub struct Vad {
    cfg: VadConfig,
    frame_len: usize,
    sum_sq: f64,
    in_frame: usize,

    pos: u64,
    loud_run: u32,
    loud_start: u64,
    quiet_run: u32,
    speaking: bool,
    last_db: f32,
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        let frame_len = (cfg.sample_rate as usize * cfg.frame_ms as usize / 1000).max(1);
        Self { cfg, frame_len, sum_sq: 0.0, in_frame: 0, pos: 0, loud_run: 0, loud_start: 0, quiet_run: 0, speaking: false, last_db: -120.0 }
    }

    pub fn config(&self) -> VadConfig {
        self.cfg
    }

    pub fn speaking(&self) -> bool {
        self.speaking
    }

    pub fn last_dbfs(&self) -> f32 {
        self.last_db
    }

    pub fn push(&mut self, samples: &[i16]) -> Vec<VadEvent> {
        let mut events = Vec::new();
        for &s in samples {
            let x = s as f64 / 32768.0;
            self.sum_sq += x * x;
            self.in_frame += 1;
            self.pos += 1;
            if self.in_frame == self.frame_len {
                let rms = (self.sum_sq / self.frame_len as f64).sqrt();
                let db = if rms > 0.0 { (20.0 * rms.log10()) as f32 } else { -120.0 };
                self.last_db = db;
                self.sum_sq = 0.0;
                self.in_frame = 0;
                self.frame(db >= self.cfg.threshold_dbfs, &mut events);
            }
        }
        events
    }

    fn frame(&mut self, loud: bool, events: &mut Vec<VadEvent>) {
        let frame_start = self.pos - self.frame_len as u64;
        if loud {
            if self.loud_run == 0 {
                self.loud_start = frame_start;
            }
            self.loud_run += 1;
            self.quiet_run = 0;
            if !self.speaking && self.loud_run >= self.cfg.onset_frames {
                self.speaking = true;
                events.push(VadEvent::SpeechStart { start_sample: self.loud_start, detected_sample: self.pos });
            }
        } else {
            self.loud_run = 0;
            if self.speaking {
                self.quiet_run += 1;
                if self.quiet_run >= self.cfg.hang_frames {
                    self.speaking = false;
                    self.quiet_run = 0;
                    events.push(VadEvent::SpeechEnd { end_sample: self.pos - (self.cfg.hang_frames as u64 * self.frame_len as u64) });
                }
            }
        }
    }
}

pub fn tone(freq: f32, dbfs: f32, ms: u32, rate: u32) -> Vec<i16> {
    let amp = 32767.0 * 10f32.powf(dbfs / 20.0);
    let n = rate as usize * ms as usize / 1000;
    (0..n).map(|i| (amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()) as i16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(dbfs: f32, ms: u32) -> Vec<i16> {
        let amp = 32767.0 * 10f32.powf(dbfs / 20.0);
        let mut x: u32 = 0x9E37_79B9;
        (0..16 * ms)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                ((x as f32 / u32::MAX as f32) * 2.0 - 1.0) * amp
            })
            .map(|v| v as i16)
            .collect()
    }

    #[test]
    fn a_tone_triggers_speech_start_after_the_onset_frames() {
        let mut vad = Vad::new(VadConfig::default());
        let mut input = vec![0i16; 16 * 500];
        input.extend(tone(220.0, -12.0, 400, 16_000));
        input.extend(vec![0i16; 16 * 800]);
        let events = vad.push(&input);
        assert_eq!(
            events,
            vec![
                VadEvent::SpeechStart { start_sample: 8000, detected_sample: 8000 + 3 * 320 },
                VadEvent::SpeechEnd { end_sample: 8000 + 6400 },
            ]
        );
    }

    #[test]
    fn silence_and_room_noise_never_trigger() {
        let mut vad = Vad::new(VadConfig::default());
        assert!(vad.push(&vec![0i16; 16 * 2000]).is_empty());
        assert!(vad.push(&noise(-50.0, 3000)).is_empty(), "noise at -50 dBFS peak stays below -38 dBFS RMS");
        assert!(!vad.speaking());
    }

    #[test]
    fn a_click_shorter_than_the_onset_is_ignored() {
        let mut vad = Vad::new(VadConfig::default());
        let mut input = tone(1000.0, -6.0, 40, 16_000);
        input.extend(vec![0i16; 16 * 200]);
        assert!(vad.push(&input).is_empty());
    }

    #[test]
    fn detection_is_independent_of_how_samples_are_split() {
        let mut input = vec![0i16; 16 * 100];
        input.extend(tone(300.0, -10.0, 200, 16_000));
        let whole = Vad::new(VadConfig::default()).push(&input);
        let mut v = Vad::new(VadConfig::default());
        let pieces: Vec<VadEvent> = input.chunks(97).flat_map(|c| v.push(c)).collect();
        assert_eq!(whole, pieces);
    }
}
