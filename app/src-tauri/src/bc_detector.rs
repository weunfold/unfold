
use ort::{
    session::Session,
    value::{DynValue, Tensor, TensorElementType, ValueType},
};
use sha2::{Digest, Sha256};

pub const RATE: u64 = 16_000;
pub const CHUNK: usize = 640;
pub const MODEL_SHA256: &str = "668506592e7d8e115bcd7b9e6cd6bb17b058b1cbfeac33a2996d0a59e80c6d6c";
pub const STATES: [(&str, &[i64]); 7] = [
    ("audio_cache", &[1, 240]),
    ("stem_cache", &[1, 80, 4]),
    ("block0_cache", &[1, 128, 4]),
    ("block1_cache", &[1, 128, 8]),
    ("block2_cache", &[1, 128, 16]),
    ("block3_cache", &[1, 128, 32]),
    ("gru_state", &[2, 1, 192]),
];

#[derive(Clone, Debug)]
pub struct Cache {
    pub name: &'static str,
    pub shape: &'static [i64],
    pub values: Vec<f32>,
}
#[derive(Clone, Debug)]
pub struct Prediction {
    pub intent: [[f32; 5]; 2],
    pub vad: [f32; 2],
    pub vap: [[f32; 8]; 2],
}
impl Prediction {
    pub fn floor(&self) -> f32 {
        self.intent[1][2] + self.intent[1][3]
    }
    pub fn bc(&self) -> f32 {
        self.intent[1][0] + self.intent[1][1]
    }
}
pub struct Model {
    session: Session,
    caches: Vec<Cache>,
}
fn zeros() -> Vec<Cache> {
    STATES
        .iter()
        .map(|&(name, shape)| Cache {
            name,
            shape,
            values: vec![0.; shape.iter().product::<i64>() as usize],
        })
        .collect()
}
fn tensor(shape: &[i64], data: Vec<f32>) -> Result<DynValue, String> {
    Tensor::from_array((shape.to_vec(), data))
        .map(|v| v.into_dyn())
        .map_err(|e| e.to_string())
}
fn extract(value: &DynValue, shape: &[i64]) -> Result<Vec<f32>, String> {
    let (actual, values) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| e.to_string())?;
    if actual.as_ref() != shape || values.iter().any(|v| !v.is_finite()) {
        return Err("Invalid overlap output shape/value".into());
    }
    Ok(values.to_vec())
}
impl Model {
    pub fn load() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let parent = exe.parent().ok_or("Overlap executable has no directory")?;
        let dir = if cfg!(debug_assertions) {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("gen/turn-models")
        } else {
            parent.join("../Resources/turn-models")
        };
        Self::from_bytes(
            &std::fs::read(dir.join("backchannel-int8.onnx"))
                .map_err(|e| format!("Bundled overlap model unavailable: {e}"))?,
        )
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if format!("{:x}", Sha256::digest(bytes)) != MODEL_SHA256 {
            return Err("Overlap model SHA256 mismatch".into());
        }
        let session = crate::onnx_cpu::session(bytes)?;
        let expected = std::iter::once(("chunk", &[1, 640][..]))
            .chain(std::iter::once(("bot_state", &[1, 2, 8][..])))
            .chain(STATES);
        if session.inputs().len() != 9 || session.outputs().len() != 10 {
            return Err("Overlap model tensor contract changed".into());
        }
        for (name, shape) in expected {
            let input = session
                .inputs()
                .iter()
                .find(|v| v.name() == name)
                .ok_or("Missing overlap model input")?;
            match input.dtype() {
                ValueType::Tensor {
                    ty: TensorElementType::Float32,
                    shape: actual,
                    ..
                } if actual.as_ref() == shape => {}
                _ => return Err(format!("Invalid overlap input {name}")),
            }
        }
        Ok(Self {
            session,
            caches: zeros(),
        })
    }
    pub fn reset(&mut self) {
        for cache in &mut self.caches {
            cache.values.fill(0.);
        }
    }
    pub fn caches(&self) -> &[Cache] {
        &self.caches
    }
    pub fn infer(&mut self, chunk: &[f32], bot: [[f32; 8]; 2]) -> Result<Prediction, String> {
        if chunk.len() != CHUNK
            || chunk.iter().any(|v| !v.is_finite() || v.abs() > 1.)
            || bot
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Invalid overlap PCM/bot_state".into());
        }
        let mut inputs = vec![
            ("chunk", tensor(&[1, 640], chunk.to_vec())?),
            (
                "bot_state",
                tensor(&[1, 2, 8], bot.into_iter().flatten().collect())?,
            ),
        ];
        for cache in &self.caches {
            inputs.push((cache.name, tensor(cache.shape, cache.values.clone())?));
        }
        let output = self.session.run(inputs).map_err(|e| e.to_string())?;
        let intent = extract(&output["intent"], &[1, 2, 5])?;
        if intent.chunks_exact(5).any(|f| {
            f.iter().any(|v| !(0. ..=1.).contains(v)) || (f.iter().sum::<f32>() - 1.).abs() > 0.001
        }) {
            return Err("Invalid overlap intent probabilities".into());
        }
        let vad = extract(&output["vad"], &[1, 2])?;
        let vap = extract(&output["vap"], &[1, 2, 8])?;
        if vad.iter().chain(&vap).any(|v| !(0. ..=1.).contains(v)) {
            return Err("Invalid overlap activity probabilities".into());
        }
        let next: Result<Vec<_>, _> = self
            .caches
            .iter()
            .map(|cache| extract(&output[format!("{}_out", cache.name).as_str()], cache.shape))
            .collect();
        for (cache, values) in self.caches.iter_mut().zip(next?) {
            cache.values = values;
        }
        Ok(Prediction {
            intent: [
                intent[..5].try_into().unwrap(),
                intent[5..].try_into().unwrap(),
            ],
            vad: vad.try_into().unwrap(),
            vap: [vap[..8].try_into().unwrap(), vap[8..].try_into().unwrap()],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Hold,
    Resume,
    Yield,
    Noise,

    Speech,
}
#[derive(Clone, Debug)]
pub struct Decision {
    pub action: Action,
    pub begin_sample: u64,
    pub end_sample: u64,
    pub floor: f32,
    pub bc: f32,
    pub no_speech: bool,
}

pub const MAX_HOLD_SAMPLES: u64 = RATE * 3;
#[derive(Default)]
pub struct Policy {
    clock: u64,
    pause: Option<u64>,
    quiet: u64,
    streak: u8,
    peak_bc: f32,
    terminal: Option<Action>,
    speech_seen: bool,
    near_streak: u8,
    listening_reported: bool,
}
impl Policy {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn reject_yield(&mut self) {
        if self.terminal == Some(Action::Yield) { self.terminal = None; self.streak = 0; }
    }
    pub fn step(&mut self, chunk: &[f32], playing: bool, prediction: &Prediction) -> Vec<Decision> {
        let end = self.clock + CHUNK as u64;
        let voiced = chunk.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / CHUNK as f64
            > 0.005_f64.powi(2);
        self.clock = end;
        let near = prediction.vad[1] >= 0.5;
        self.near_streak = if near { self.near_streak.saturating_add(1) } else { 0 };
        if !near { self.listening_reported = false; }
        self.streak = if prediction.floor()>=0.7 && near {self.streak.saturating_add(1)} else {0};
        if matches!(self.terminal, Some(Action::Resume | Action::Noise)) && !voiced {
            self.pause = None;
            self.quiet = 0;
            self.streak = 0;
            self.peak_bc = 0.;
            self.terminal = None;
            self.speech_seen = false;
        }
        let mut decisions = vec![];
        if !playing && self.near_streak >= 2 && !self.listening_reported && self.terminal != Some(Action::Yield) {
            self.listening_reported = true;
            decisions.push(Decision { action: Action::Speech, begin_sample: end.saturating_sub(2 * CHUNK as u64), end_sample: end,
                floor: prediction.floor(), bc: prediction.bc(), no_speech: false });
        }
        if self.pause.is_none() && playing && self.near_streak >= 2 && self.terminal.is_none() {
            self.pause = Some(end);
            decisions.push(Decision {
                action: Action::Hold,
                begin_sample: end - CHUNK as u64,
                end_sample: end,
                floor: prediction.floor(),
                bc: prediction.bc(),
                no_speech: false,
            });
        }
        if let Some(begin) = self.pause.filter(|_| self.terminal.is_none()) {
            self.quiet = if voiced { 0 } else { self.quiet + CHUNK as u64 };
            self.peak_bc = self.peak_bc.max(prediction.bc());
            self.speech_seen |= prediction.vad[1] >= 0.5 || prediction.floor() >= 0.7;
            let no_speech = self.quiet >= RATE / 5 && !self.speech_seen;
            let action = if self.streak >= 2 {
                Some(Action::Yield)
            } else if self.quiet >= RATE / 5 && (self.peak_bc >= 0.7 || no_speech) {
                Some(Action::Resume)
            } else if end - begin >= MAX_HOLD_SAMPLES {
                Some(Action::Noise)
            } else {
                None
            };
            if let Some(action) = action {
                self.terminal = Some(action);
                decisions.push(Decision {
                    action,
                    begin_sample: begin,
                    end_sample: end,
                    floor: prediction.floor(),
                    bc: prediction.bc(),
                    no_speech,
                });
            }
        }
        decisions
    }
}

pub struct Stream {
    pub model: Model,
    policy: Policy,
    key: Option<(u64, u64)>,
    pcm: Vec<f32>,
    bot: [[f32; 8]; 2],
    eligible: bool,
}
impl Stream {
    pub fn load() -> Result<Self, String> {
        Ok(Self {
            model: Model::load()?,
            policy: Policy::default(),
            key: None,
            pcm: Vec::with_capacity(CHUNK),
            bot: [[0.; 8]; 2],
            eligible: false,
        })
    }
    pub fn configure(&mut self, epoch: u64, turn: u64) {
        if self.key != Some((epoch, turn)) {
            if self.key.is_none_or(|key| key.0 != epoch) {
                self.model.reset();
            }
            self.policy.reset();
            self.pcm.clear();
            self.key = Some((epoch, turn));
        }
    }
    pub fn push(
        &mut self,
        samples: &[f32],
        rendered: crate::bc_playout::Snapshot,
        held: bool,
    ) -> Result<Vec<Decision>, String> {
        let mut out = vec![];
        for (offset, &sample) in samples.iter().enumerate() {
            if !sample.is_finite() {
                return Err("Invalid post-AEC overlap sample".into());
            }
            let at = rendered
                .samples
                .saturating_sub(((samples.len() - offset) as u64) * 3);
            if self.pcm.is_empty() {
                self.bot[0] = rendered.frame(at);
                self.eligible = self.bot[0][0] > 0. || held;
            }
            if self.pcm.len() == CHUNK / 2 {
                self.bot[1] = rendered.frame(at);
            }
            self.pcm.push(sample.clamp(-1., 1.));
            if self.pcm.len() == CHUNK {
                let prediction = self.model.infer(&self.pcm, self.bot)?;
                out.extend(self.policy.step(&self.pcm, self.eligible, &prediction));
                self.pcm.clear();
            }
        }
        Ok(out)
    }
}

#[derive(Default)]
pub struct Prepared {
    epoch: Option<u64>,
    stream: Option<Result<Stream, String>>,
    notified: Option<u64>,
}
impl Prepared {
    pub fn prepare(&mut self, epoch: u64) -> Result<(), String> {
        if self.epoch != Some(epoch) {
            if !matches!(self.stream, Some(Ok(_))) {
                self.stream = Some(Stream::load());
            }
            self.epoch = Some(epoch);
        }
        match self.stream.as_mut().ok_or("Overlap session not prepared")? {
            Ok(s) => {
                s.configure(epoch, 0);
                Ok(())
            }
            Err(e) => Err(e.clone()),
        }
    }
    pub fn push(
        &mut self,
        epoch: u64,
        turn: u64,
        pcm: &[f32],
        rendered: crate::bc_playout::Snapshot,
        held: bool,
    ) -> Result<Vec<Decision>, String> {
        if self.epoch != Some(epoch) {
            if matches!(self.stream, Some(Ok(_))) {
                self.epoch = Some(epoch);
            } else {
                return Err("Overlap session not prepared for this epoch".into());
            }
        }
        let result = match self.stream.as_mut() {
            Some(Ok(s)) => {
                s.configure(epoch, turn);
                s.push(pcm, rendered, held)
            }
            Some(Err(e)) => Err(e.clone()),
            None => Err("Overlap session unavailable".into()),
        };
        if let Err(e) = &result {
            self.stream = Some(Err(e.clone()));
        }
        result
    }

    pub fn reset_segment(&mut self) {
        if let Some(Ok(stream))=self.stream.as_mut() {
            stream.model.reset();stream.policy.reset();stream.pcm.clear();
        }
    }
    pub fn reject_yield(&mut self) {
        if let Some(Ok(stream)) = self.stream.as_mut() { stream.policy.reject_yield(); }
    }

    pub fn notify_failure(&mut self, epoch: u64) -> bool {
        if self.notified == Some(epoch) {
            false
        } else {
            self.notified = Some(epoch);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prediction(bc: f32, floor: f32) -> Prediction {
        Prediction {
            intent: [[bc, 0., 0., floor, 1. - bc - floor]; 2],
            vad: [1.; 2],
            vap: [[0.; 8]; 2],
        }
    }
    #[test]
    fn prepare_precedes_pcm_and_failure_notice_is_once_per_epoch() {
        let mut p = Prepared::default();
        let render = crate::bc_playout::Snapshot::default();
        assert!(
            p.push(1, 1, &[0.; CHUNK], render, false).is_err(),
            "PCM must never cold-load a session"
        );
        p.prepare(1).unwrap();
        assert!(p.push(1, 1, &[0.; CHUNK], render, false).is_ok());
        assert!(p.push(1, 1, &[f32::NAN; CHUNK], render, false).is_err());
        assert!(p.notify_failure(1));
        assert!(!p.notify_failure(1));
        assert!(
            p.push(1, 1, &[0.; CHUNK], render, false).is_err(),
            "failed epoch remains degraded"
        );
        p.prepare(2).unwrap();
        assert!(p.push(2, 1, &[0.; CHUNK], render, false).is_ok());
        assert!(p.notify_failure(2));
        assert!(!p.notify_failure(2));
    }
    #[test]
    fn uncertain_residue_never_enters_a_hold() {
        let mut quiet=prediction(0.1,0.1);quiet.vad=[0.1;2];
        let mut p=Policy::default();
        for _ in 0..100 {assert!(p.step(&[0.1;CHUNK],true,&quiet).is_empty());}
    }
    #[test]
    fn finite_resampler_overshoot_is_clamped_but_nonfinite_still_rejected() {
        let mut stream = Stream::load().unwrap();
        stream.configure(1, 1);
        let render = crate::bc_playout::Snapshot {
            samples: 1920,
            speaking: true,
            ..Default::default()
        };
        assert!(stream.push(&[1.01; CHUNK], render, false).is_ok());
        assert!(stream.push(&[-1.01; CHUNK], render, false).is_ok());
        assert!(stream.push(&[f32::INFINITY; CHUNK], render, false).is_err());
        assert!(stream.push(&[f32::NAN; CHUNK], render, false).is_err());
    }
    #[test]
    fn backchannel_resumes_without_yielding() {
        let mut p = Policy::default();
        let pred = prediction(0.9, 0.05);
        assert!(p.step(&[0.1; CHUNK],true,&pred).is_empty());
        assert_eq!(p.step(&[0.1; CHUNK], true, &pred)[0].action, Action::Hold);
        let mut actions = vec![];
        for _ in 0..5 {
            actions.extend(p.step(&[0.; CHUNK], true, &pred));
        }
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, Action::Resume);
    }
    #[test]
    fn resumed_backchannel_rearms_for_later_content_in_same_turn() {
        let mut p = Policy::default();
        let bc = prediction(0.9, 0.05);
        let mut quiet=bc.clone();quiet.vad=[0.1;2];
        p.step(&[0.1; CHUNK], true, &bc);
        p.step(&[0.1; CHUNK], true, &bc);
        for _ in 0..5 {
            p.step(&[0.; CHUNK], true, &quiet);
        }

        p.step(&[0.; CHUNK], true, &quiet);
        let floor = prediction(0.05, 0.9);
        assert!(p.step(&[0.1; CHUNK],true,&floor).is_empty());
        let actions=p.step(&[0.1;CHUNK],true,&floor).into_iter().map(|d|d.action).collect::<Vec<_>>();
        assert_eq!(actions,[Action::Hold,Action::Yield]);
        assert!(
            p.step(&[0.; CHUNK], true, &quiet).is_empty(),
            "yield stays fenced until a new turn"
        );
    }
    #[test]
    fn content_yields_only_after_two_model_chunks() {
        let mut p = Policy::default();
        let pred = prediction(0.05, 0.9);
        assert!(p.step(&[0.1; CHUNK], true, &pred).is_empty());
        let actions=p.step(&[0.1;CHUNK],true,&pred).into_iter().map(|d|d.action).collect::<Vec<_>>();
        assert_eq!(actions,[Action::Hold,Action::Yield]);
    }
    #[test]
    fn uncertain_noise_resumes_at_bounded_hold() {
        let mut p = Policy::default();
        let pred = prediction(0.1, 0.1);
        p.step(&[0.1; CHUNK], true, &pred);
        p.step(&[0.1; CHUNK], true, &pred);
        let mut actions = vec![];
        for _ in 0..MAX_HOLD_SAMPLES / CHUNK as u64 {
            actions.extend(p.step(&[0.; CHUNK], true, &pred));
        }
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, Action::Noise);
    }
}
