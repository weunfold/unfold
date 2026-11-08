
use ort::{session::Session, value::Tensor};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {pub endpoint:bool,pub shadow:bool}
impl Default for Mode {fn default()->Self{Self{endpoint:false,shadow:true}}}
impl Mode {pub fn runs(self)->bool{self.endpoint||self.shadow}}
pub const RATE: u64 = 16_000;
const FRAME: usize = 512;
#[derive(Clone, Copy, Debug)]
pub struct Knobs {
    pub silence_ms: u64,
    pub threshold: f32,
    pub max_hold_ms: u64,
    pub retry_ms: u64,
    pub threshold_floor: Option<f32>,
}
impl Default for Knobs {
    fn default() -> Self {
        Self {
            silence_ms: 512,
            threshold: 0.3,
            max_hold_ms: 1200,
            retry_ms: 100,
            threshold_floor: Some(0.2),
        }
    }
}
impl Knobs {
    pub fn validate(self) -> Result<Self, String> {
        if self.silence_ms < 32
            || self.retry_ms < 32
            || self.max_hold_ms < self.silence_ms
            || !self.threshold.is_finite()
            || !(0.0..=1.0).contains(&self.threshold)
            || self
                .threshold_floor
                .is_some_and(|f| !f.is_finite() || f < 0.0 || f > self.threshold)
        {
            return Err("Invalid Smart Turn policy".into());
        }
        Ok(self)
    }
}
#[derive(Clone, Debug)]
pub struct Decision {
    pub probability: f32,
    pub commit: bool,
    pub effective_threshold: f32,
    pub max_hold: bool,
    pub latency_ns: u64,
    pub audio_start_ms: u64,
    pub audio_end_ms: u64,
    pub quiet_ms: u64,
    pub knobs: Knobs,
}
#[derive(Default)]
pub struct Policy {
    heard: u64,
    quiet: u64,
    next: u64,
    speech: bool,
}
impl Policy {
    pub fn frame(&mut self, speech: bool, samples: usize, knobs: Knobs) -> bool {
        self.heard += samples as u64;
        if speech {
            self.speech = true;
            self.quiet = 0;
            self.next = knobs.silence_ms;
        } else {
            self.quiet += samples as u64;
        }
        self.speech && self.quiet * 1000 / RATE >= self.next
    }
    pub fn decide(&mut self, probability: f32, knobs: Knobs) -> Result<Decision, String> {
        if !probability.is_finite() || !(0.0..=1.0).contains(&probability) {
            return Err("Invalid Smart Turn probability".into());
        }
        let quiet_ms = self.quiet * 1000 / RATE;

        let next =
            self.next + (quiet_ms.saturating_sub(self.next) / knobs.retry_ms + 1) * knobs.retry_ms;
        self.next = if quiet_ms < knobs.max_hold_ms {
            next.min(knobs.max_hold_ms)
        } else {
            quiet_ms + knobs.retry_ms
        };
        let max_hold = quiet_ms >= knobs.max_hold_ms;
        let effective_threshold = knobs.threshold_floor.map_or(knobs.threshold, |floor| {
            let progress = (quiet_ms.saturating_sub(knobs.silence_ms) as f32
                / (knobs.max_hold_ms - knobs.silence_ms).max(1) as f32)
                .clamp(0.0, 1.0);
            (knobs.threshold + (floor - knobs.threshold) * progress).max(floor)
        });
        Ok(Decision {
            probability,
            latency_ns: 0,
            commit: probability >= effective_threshold || max_hold,
            effective_threshold,
            max_hold,
            audio_start_ms: self
                .heard
                .saturating_sub(crate::turn_features::SAMPLES as u64)
                * 1000
                / RATE,
            audio_end_ms: self.heard * 1000 / RATE,
            quiet_ms,
            knobs,
        })
    }
}
pub fn model_dir(executable: &Path) -> Result<PathBuf, String> {
    if !executable.is_absolute() {
        return Err("Model executable path must be absolute".into());
    }
    let parent = executable.parent().ok_or("No executable parent")?;
    let bundled = parent.join("../Resources/turn-models");
    if bundled.is_dir() {
        return bundled.canonicalize().map_err(|e| e.to_string());
    }
    #[cfg(debug_assertions)]
    {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("gen/turn-models")
            .canonicalize()
            .map_err(|e| e.to_string());
    }
    #[cfg(not(debug_assertions))]
    Err("Bundled turn models are unavailable".into())
}
pub struct Detector {
    vad: VoiceActivityDetector,
    model: Session,
    pending: Vec<f32>,
    audio: VecDeque<f32>,
    policy: Policy,
    knobs: Knobs,
    shadow_reported: bool,
}
impl Detector {
    pub fn load(knobs: Knobs) -> Result<Self, String> {
        Self::from_dir(
            &model_dir(&std::env::current_exe().map_err(|e| e.to_string())?)?,
            knobs,
        )
    }
    pub fn from_dir(dir: &Path, knobs: Knobs) -> Result<Self, String> {
        let knobs = knobs.validate()?;
        if !dir.is_absolute() {
            return Err("Model directory must be absolute".into());
        }
        let model = crate::onnx_cpu::session(
            &std::fs::read(dir.join("smart-turn-v3.2-cpu.onnx")).map_err(|e| e.to_string())?,
        )?;
        let vad = VoiceActivityDetector::create(
            &VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(dir.join("silero_vad.onnx").to_string_lossy().into_owned()),
                    threshold: 0.5,
                    min_silence_duration: 0.032,
                    min_speech_duration: 0.064,
                    window_size: FRAME as i32,
                    max_speech_duration: 60.0,
                },
                sample_rate: RATE as i32,
                num_threads: 1,
                provider: Some("cpu".into()),
                ..Default::default()
            },
            10.0,
        )
        .ok_or("Silero VAD could not load")?;
        Ok(Self {
            vad,
            model,
            pending: vec![],
            audio: VecDeque::with_capacity(crate::turn_features::SAMPLES),
            policy: Policy::default(),
            knobs,
            shadow_reported: false,
        })
    }
    pub fn configure(&mut self, knobs: Knobs) -> Result<(), String> {
        self.knobs = knobs.validate()?;
        self.reset();
        Ok(())
    }
    pub fn reset(&mut self) {
        self.vad.reset();
        self.pending.clear();
        self.audio.clear();
        self.policy = Policy::default();
        self.shadow_reported = false;
    }
    pub fn infer(model: &mut Session, audio: &[f32]) -> Result<f32, String> {
        let features = crate::turn_features::extract(audio)?;
        let tensor =
            Tensor::from_array(([1_usize, 80, 800], features)).map_err(|e| e.to_string())?;
        let outputs = model
            .run(ort::inputs!["input_features"=>tensor])
            .map_err(|e| e.to_string())?;
        let (_, values) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        if values.len() != 1 {
            return Err("Unexpected Smart Turn output shape".into());
        }
        Ok(values[0])
    }
    pub fn push(&mut self, samples: &[f32]) -> Result<Vec<Decision>, String> {
        self.push_with(samples, Self::infer)
    }

    pub fn push_with(
        &mut self,
        samples: &[f32],
        predict: impl FnMut(&mut Session, &[f32]) -> Result<f32, String>,
    ) -> Result<Vec<Decision>, String> {
        self.push_mode(samples, predict, false)
    }

    pub fn push_shadow(&mut self, samples: &[f32]) -> Result<Vec<Decision>, String> {
        self.push_mode(samples, Self::infer, true)
    }
    fn push_mode(
        &mut self, samples: &[f32],
        mut predict: impl FnMut(&mut Session, &[f32]) -> Result<f32, String>,
        shadow: bool,
    ) -> Result<Vec<Decision>, String> {
        if samples.iter().any(|s| !s.is_finite()) {
            return Err("Non-finite turn audio".into());
        }
        self.pending.extend_from_slice(samples);
        let mut decisions = vec![];
        let mut due = false;
        let used = self.pending.len() / FRAME * FRAME;
        for frame in self.pending[..used].chunks_exact(FRAME) {
            self.vad.accept_waveform(frame);
            let speech = self.vad.detected();
            if speech { self.shadow_reported = false; }

            while !self.vad.is_empty() {
                self.vad.pop();
            }
            self.audio.extend(frame);
            while self.audio.len() > crate::turn_features::SAMPLES {
                self.audio.pop_front();
            }
            due = self.policy.frame(speech, FRAME, self.knobs) && (!shadow || !self.shadow_reported);
        }
        self.pending.drain(..used);
        if due {
            let began = std::time::Instant::now();
            let probability = predict(&mut self.model, self.audio.make_contiguous())?;
            let mut decision = self.policy.decide(probability, self.knobs)?;
            decision.latency_ns = began.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            decisions.push(decision);
            if shadow { self.shadow_reported = true; }
        }
        Ok(decisions)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_probability_waits_past_legacy_timer_then_accepts_continuation() {
        let k = Knobs::default();
        let mut p = Policy::default();
        p.frame(true, 16000, k);
        assert!(p.frame(false, 16000, k));
        assert!(!p.decide(0.1, k).unwrap().commit);
        p.frame(true, 16000, k);
        p.frame(false, 4096, k);
        assert!(p.decide(0.9, k).unwrap().commit);
    }
    #[test]
    fn retry_schedule_cannot_skip_the_max_hold_deadline() {
        let k = Knobs {
            max_hold_ms: 2400,
            ..Knobs::default()
        };
        let mut p = Policy::default();
        p.frame(true, 512, k);
        p.frame(false, 2304 * 16, k);
        assert!(!p.decide(0.1, k).unwrap().commit);
        assert!(p.frame(false, 96 * 16, k));
        let d = p.decide(0.1, k).unwrap();
        assert!(d.max_hold && d.commit);
        assert_eq!(d.quiet_ms, 2400);
    }
    #[test]
    fn decay_reasks_at_100ms_and_never_drops_below_the_floor() {
        let k = Knobs {
            silence_ms: 160,
            threshold: 0.7,
            max_hold_ms: 1200,
            retry_ms: 100,
            threshold_floor: Some(0.2),
        };
        let mut p = Policy::default();
        p.frame(true, 512, k);
        p.frame(false, 160 * 16, k);
        let first = p.decide(0.4, k).unwrap();
        assert!(!first.commit);
        assert_eq!(first.effective_threshold, 0.7);
        assert!(!p.frame(false, 99 * 16, k));
        assert!(p.frame(false, 16, k));
        p.decide(0.4, k).unwrap();
        p.frame(false, 700 * 16, k);
        let later = p.decide(0.4, k).unwrap();
        assert!(later.commit && !later.max_hold);
        p.frame(false, 1000 * 16, k);
        assert!(p.decide(0.1, k).unwrap().effective_threshold >= 0.2);
    }
    #[test]
    fn bounded_hold_is_explicit_and_nonfinite_probability_never_commits() {
        let k = Knobs::default();
        let mut p = Policy::default();
        p.frame(true, 512, k);
        p.frame(false, 16000 * 3, k);
        assert!(p.decide(f32::NAN, k).is_err());
        let d = p.decide(0.1, k).unwrap();
        assert!(d.commit && d.max_hold);
        assert_eq!(d.quiet_ms, 3000);
    }
}

#[cfg(test)]
mod shadow_latency_regressions {
    use super::*;
    #[test]
    fn slow_shadow_does_not_move_the_baseline_timer_commit() {
        use std::time::{Duration,Instant};

        let run=|observe:bool| {
            let dir=model_dir(&std::env::current_exe().unwrap()).unwrap();
            let mut detector=Detector::from_dir(&dir,Knobs::default()).unwrap();
            detector.policy.frame(true,512,Knobs::default());
            let mut observer=crate::turn_shadow::Observer::with_processor(move |audio,_| detector.push_mode(audio,|_,_|{std::thread::sleep(Duration::from_millis(300));Ok(0.1)},true), |_|{}).unwrap();
            let mut input=crate::conversation_input::Input::default();
            input.words("a complete request",true,false);
            let began=Instant::now();
            for tick in 1..=16 {
                input.level(0.0);
                if input.ready(false).is_some(){let elapsed=began.elapsed();observer.finish();return (tick,elapsed);}
                if observe {observer.push(1,1,&[0.;800]);}
            }
            panic!("timer did not commit")
        };
        let (baseline_tick,baseline)=run(false);let (shadow_tick,shadow)=run(true);
        eprintln!("timer baseline={baseline:?} shadow={shadow:?} ticks={baseline_tick}/{shadow_tick}");
        assert_eq!(shadow_tick,baseline_tick);
        assert!(shadow.saturating_sub(baseline)<Duration::from_millis(50),"shadow must not block the timer worker");
    }
}

#[cfg(test)]
mod shadow_pause_regressions {
    use super::*;
    #[test]
    fn shadow_emits_once_per_pause_even_when_probability_never_commits() {
        let dir=model_dir(&std::env::current_exe().unwrap()).unwrap();
        let mut d=Detector::from_dir(&dir,Knobs::default()).unwrap();
        d.policy.frame(true,512,Knobs::default());
        let mut rows=vec![];
        for _ in 0..100 {rows.extend(d.push_mode(&[0.;800],|_,_|Ok(0.1),true).unwrap());}
        assert_eq!(rows.len(),1);assert!(!rows[0].commit);
        d.reset();d.policy.frame(true,512,Knobs::default());
        assert_eq!(d.push_mode(&[0.;16000],|_,_|Ok(0.9),true).unwrap().len(),1);
    }
}
