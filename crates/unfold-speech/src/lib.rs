

pub mod catalog;
mod client;
mod http;
pub mod normalize;
mod speak;
pub mod split;
mod wire;

pub use catalog::{Catalog, CatalogError, Clip, Language, Source, VoiceEntry};
pub use normalize::normalize_pt;
pub use client::{ApiKey, Cancel, Client, ConfigError, Playback, StepError};
pub use speak::{Report, Sink, Spoken, StepOutcome, Warning};
pub use wire::{AudioChunk, Done, Reference, ServerError, SpeechEvent, SpeechRequest, Streamed, Synthesis};

pub const MAX_INPUT_CHARS: usize = 1000;

pub const SAMPLE_RATE: u32 = 24_000;
pub const SAMPLES_PER_FRAME: u64 = 1920;
pub const FRAMES_PER_SECOND: f64 = 12.5;

pub const MAX_RESPLIT_DEPTH: u8 = 2;
