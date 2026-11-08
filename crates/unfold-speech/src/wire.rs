

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    pub references: Vec<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_new_tokens: Option<u32>,

    pub stream: bool,

    pub stream_format: &'static str,
}

impl std::fmt::Debug for SpeechRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechRequest")
            .field("model", &self.model)
            .field("input_chars", &self.input.chars().count())
            .field("language", &self.language)
            .field("references", &self.references)
            .field("stream_format", &self.stream_format)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Serialize)]
pub struct Reference {
    pub audio_base64: String,
    pub text: String,
}

impl std::fmt::Debug for Reference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reference").field("audio_base64_len", &self.audio_base64.len()).finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpeechEvent {
    Audio(AudioChunk),
    Done(Done),
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Done {
    pub truncated: bool,
    pub error: Option<ServerError>,
    pub frames: u64,
    pub audio_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServerError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Synthesis {
    pub chunks: Vec<AudioChunk>,
    pub done: Done,
    pub frame_budget: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Streamed {
    pub done: Done,
    pub frame_budget: Option<u32>,
}

impl Synthesis {
    pub fn samples(&self) -> u64 {
        self.chunks.iter().map(|c| c.samples.len() as u64).sum()
    }
}

#[derive(Deserialize)]
pub(crate) struct AudioData {
    pub pcm_base64: String,
    pub sample_rate: u32,
    pub format: String,
}
