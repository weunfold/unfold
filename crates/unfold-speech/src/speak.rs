

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use speech_markup::{Event, Mood, Step};

use crate::catalog::{Language, VoiceEntry};
use crate::client::{Cancel, Client, Playback, StepError};
use crate::normalize::normalize_pt;
use crate::split::{split_for_cap, split_half};
use crate::wire::{AudioChunk, Done, Reference, SpeechRequest};
use crate::{MAX_RESPLIT_DEPTH, SAMPLES_PER_FRAME, SAMPLE_RATE};

pub trait Sink {

    fn audio(&mut self, step: usize, chunk: &AudioChunk);

    fn clip(&mut self, step: usize, event: Event, file: Option<&Path>);
    fn silence(&mut self, step: usize, ms: u32);

    fn cancel(&self) -> Option<Cancel> {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Warning {

    SplitForCap { pieces: usize },

    StillTruncated { depth: u8, chars: usize },

    TruncatedPlayed { step: usize, audio_seconds: f64 },

    DoneMismatch { samples: u64, frames: u64, audio_seconds: f64, frame_budget: Option<u32> },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spoken {
    pub requests: u32,
    pub frames: u64,
    pub audio_seconds: f64,
    pub first_audio: Option<std::time::Duration>,

    pub frame_budget: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StepOutcome {
    pub step: usize,
    pub result: Result<Spoken, StepError>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub steps: Vec<StepOutcome>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = (usize, &StepError)> {
        self.steps.iter().filter_map(|o| o.result.as_ref().err().map(|e| (o.step, e)))
    }
}

impl Client {

    pub fn speak_plan<S: Sink + ?Sized>(&self, steps: &[Step], voice: &VoiceEntry, sink: &mut S) -> Report {
        let mut refs: BTreeMap<Mood, Reference> = BTreeMap::new();
        let mut report = Report::default();
        let cancel = sink.cancel();
        for (i, step) in steps.iter().enumerate() {
            let mut warnings = Vec::new();
            let result = match step {
                _ if cancel.as_ref().is_some_and(Cancel::is_cancelled) => Err(StepError::Cancelled),
                Step::Clip(e) => {
                    let file = voice.events.get(e).map(|c| voice.clip_path(c));
                    sink.clip(i, *e, file.as_deref());
                    Ok(Spoken::default())
                }
                Step::Silence { ms } => {
                    sink.silence(i, *ms);
                    Ok(Spoken::default())
                }
                Step::Speak { text, mood, .. } => {
                    let reference = match refs.get(mood) {
                        Some(r) => Ok(r.clone()),
                        None => voice.reference(*mood).map(|r| refs.entry(*mood).or_insert(r).clone()),
                    };
                    match reference {
                        Ok(r) => self.speak_text(i, text, voice, &r, sink, &mut warnings),
                        Err(e) => Err(e.into()),
                    }
                }
            };
            let stop = matches!(result, Err(StepError::Cancelled));
            report.steps.push(StepOutcome { step: i, result, warnings });
            if stop {
                break;
            }
        }
        report
    }

    fn speak_text<S: Sink + ?Sized>(
        &self,
        step: usize,
        text: &str,
        voice: &VoiceEntry,
        reference: &Reference,
        sink: &mut S,
        warnings: &mut Vec<Warning>,
    ) -> Result<Spoken, StepError> {

        let text = match voice.language {
            Language::PtBr => normalize_pt(text),
            Language::En => text.to_string(),
        };
        let pieces = split_for_cap(&text, self.max_input_chars);
        if pieces.len() > 1 {
            warnings.push(Warning::SplitForCap { pieces: pieces.len() });
        }
        let cancel = sink.cancel();
        let mut play = Play { step, sink, cancel, started: Instant::now(), spoken: Spoken::default(), warnings };
        for piece in &pieces {
            match self.playback {
                Playback::Live => self.speak_live(piece, voice, reference, &mut play)?,
                Playback::HoldUntilDone => self.speak_held(piece, 0, voice, reference, &mut play)?,
            }
        }
        Ok(play.spoken)
    }

    fn request(&self, voice: &VoiceEntry, reference: &Reference, text: &str) -> SpeechRequest {
        let mut request = voice.request_with(reference.clone(), text);
        request.language = self.language.as_ref().map(|l| l.to_lowercase());
        request
    }

    fn speak_live<S: Sink + ?Sized>(
        &self,
        text: &str,
        voice: &VoiceEntry,
        reference: &Reference,
        play: &mut Play<'_, S>,
    ) -> Result<(), StepError> {
        let mut samples = 0u64;
        let cancel = play.cancel.clone();
        let done = self.stream_until(&self.request(voice, reference, text), cancel.as_ref(), |chunk| {
            samples += chunk.samples.len() as u64;
            play.audio(&chunk);
        });
        play.spoken.requests += 1;
        let done = done?;
        play.check(samples, &done.done, done.frame_budget);
        let done = done.done;
        if done.truncated {
            play.warnings.push(Warning::TruncatedPlayed { step: play.step, audio_seconds: done.audio_seconds });
        }
        play.played(&done);
        Ok(())
    }

    fn speak_held<S: Sink + ?Sized>(
        &self,
        text: &str,
        depth: u8,
        voice: &VoiceEntry,
        reference: &Reference,
        play: &mut Play<'_, S>,
    ) -> Result<(), StepError> {
        let synthesis = self.synthesize_until(&self.request(voice, reference, text), play.cancel.as_ref());
        play.spoken.requests += 1;
        let synthesis = synthesis?;
        play.check(synthesis.samples(), &synthesis.done, synthesis.frame_budget);
        if synthesis.done.truncated {
            if depth < MAX_RESPLIT_DEPTH {
                if let Some((a, b)) = split_half(text) {
                    self.speak_held(&a, depth + 1, voice, reference, play)?;
                    return self.speak_held(&b, depth + 1, voice, reference, play);
                }
            }
            play.warnings.push(Warning::StillTruncated { depth, chars: text.chars().count() });
        }
        for chunk in &synthesis.chunks {
            play.audio(chunk);
        }
        play.played(&synthesis.done);
        Ok(())
    }
}

struct Play<'a, S: ?Sized> {
    step: usize,
    sink: &'a mut S,
    cancel: Option<Cancel>,
    started: Instant,
    spoken: Spoken,
    warnings: &'a mut Vec<Warning>,
}

impl<S: Sink + ?Sized> Play<'_, S> {
    fn audio(&mut self, chunk: &AudioChunk) {
        if self.spoken.first_audio.is_none() {
            self.spoken.first_audio = Some(self.started.elapsed());
        }
        self.sink.audio(self.step, chunk);
    }

    fn played(&mut self, done: &Done) {
        self.spoken.frames += done.frames;
        self.spoken.audio_seconds = ((self.spoken.audio_seconds + done.audio_seconds) * 1000.0).round() / 1000.0;
    }

    fn check(&mut self, samples: u64, done: &Done, frame_budget: Option<u32>) {
        if let Some(b) = frame_budget {
            self.spoken.frame_budget = Some(self.spoken.frame_budget.map_or(b, |least| least.min(b)));
        }
        let seconds = samples as f64 / SAMPLE_RATE as f64;
        let over_budget = frame_budget.is_some_and(|b| done.frames > u64::from(b));
        if over_budget || done.frames != samples / SAMPLES_PER_FRAME || (done.audio_seconds - seconds).abs() > 0.01 {
            self.warnings.push(Warning::DoneMismatch { samples, frames: done.frames, audio_seconds: done.audio_seconds, frame_budget });
        }
    }
}
