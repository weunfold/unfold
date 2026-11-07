

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use speech_markup::{Event, Lang, Mood};

use crate::wire::{Reference, SpeechRequest};

const MANIFEST: &str = "voice.json";

pub const REF_SECONDS: std::ops::RangeInclusive<f64> = 1.0..=30.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "en")]
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {

    Personal,
    Stock,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub file: String,
    #[serde(default)]
    pub transcript: String,
    pub duration_s: f64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceEntry {
    pub id: String,
    pub name: String,
    pub language: Language,
    pub source: Source,

    pub model: String,

    pub temperature: Option<f32>,

    pub pitch_semitones: Option<f32>,
    pub moods: BTreeMap<Mood, Clip>,
    pub events: BTreeMap<Event, Clip>,
    dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CatalogError {
    Io { path: PathBuf, kind: io::ErrorKind },

    InvalidId(String),
    NotFound(String),
    Manifest { voice: String, reason: String },
    MissingClip { voice: String, file: String },
    ShaMismatch { voice: String, file: String, expected: String, actual: String },
    BadClip { voice: String, file: String, reason: String },
    NoReference { voice: String, mood: Mood },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CatalogError::Io { path, kind } => write!(f, "{}: {kind}", path.display()),
            CatalogError::InvalidId(id) => write!(f, "invalid voice id {id:?}"),
            CatalogError::NotFound(id) => write!(f, "no voice {id:?}"),
            CatalogError::Manifest { voice, reason } => write!(f, "voice {voice}: manifest: {reason}"),
            CatalogError::MissingClip { voice, file } => write!(f, "voice {voice}: clip {file} is missing"),
            CatalogError::ShaMismatch { voice, file, expected, actual } => {
                write!(f, "voice {voice}: clip {file} sha256 {actual}, manifest says {expected}")
            }
            CatalogError::BadClip { voice, file, reason } => write!(f, "voice {voice}: clip {file}: {reason}"),
            CatalogError::NoReference { voice, mood } => write!(f, "voice {voice} has no reference for {mood:?}"),
        }
    }
}

impl std::error::Error for CatalogError {}

pub fn mood_name(m: Mood) -> &'static str {
    match m {
        Mood::Neutral => "neutral",
        Mood::Calm => "calm",
        Mood::Joy => "joy",
        Mood::Serious => "serious",
        Mood::Question => "question",
        Mood::Surprised => "surprised",
        Mood::Tired => "tired",
        Mood::Tender => "tender",
    }
}

pub fn event_name(e: Event) -> &'static str {
    match e {
        Event::Laugh => "laugh",
        Event::Giggle => "giggle",
        Event::Sigh => "sigh",
        Event::Gasp => "gasp",
        Event::Breath => "breath",
        Event::Um => "um",
        Event::Pause => "pause",
    }
}

impl VoiceEntry {
    pub fn new(id: &str, name: &str, language: Language, source: Source, model: &str) -> Result<Self, CatalogError> {
        check_id(id)?;
        Ok(VoiceEntry {
            id: id.into(),
            name: name.into(),
            language,
            source,
            model: model.into(),
            temperature: None,
            pitch_semitones: None,
            moods: BTreeMap::new(),
            events: BTreeMap::new(),
            dir: PathBuf::new(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn clip_path(&self, clip: &Clip) -> PathBuf {
        self.dir.join(&clip.file)
    }

    pub fn to_markup_voice(&self) -> speech_markup::Voice {
        speech_markup::Voice {
            lang: match self.language {
                Language::PtBr => Lang::Pt,
                Language::En => Lang::En,
            },
            moods: self.moods.keys().copied().collect::<BTreeSet<_>>(),
            clips: self.events.keys().copied().collect::<BTreeSet<_>>(),
        }
    }

    pub fn reference(&self, mood: Mood) -> Result<Reference, CatalogError> {
        let clip = self.moods.get(&mood).ok_or_else(|| CatalogError::NoReference { voice: self.id.clone(), mood })?;
        let bytes = read_clip(&self.id, &self.dir, clip)?;
        Ok(Reference { audio_base64: base64::engine::general_purpose::STANDARD.encode(bytes), text: clip.transcript.clone() })
    }

    pub fn request(&self, mood: Mood, input: &str) -> Result<SpeechRequest, CatalogError> {
        Ok(self.request_with(self.reference(mood)?, input))
    }

    pub(crate) fn request_with(&self, reference: Reference, input: &str) -> SpeechRequest {
        SpeechRequest {
            model: self.model.clone(),
            input: input.to_string(),
            language: None,
            references: vec![reference],
            seed: None,
            temperature: self.temperature,
            top_p: None,
            max_new_tokens: None,
            stream: true,
            stream_format: "events",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    root: PathBuf,
}

impl Catalog {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Catalog { root: root.into() }
    }

    fn voice_dir(&self, id: &str) -> PathBuf {
        self.root.join("voices").join(id)
    }

    pub fn list(&self) -> Result<Vec<Result<VoiceEntry, CatalogError>>, CatalogError> {
        let dir = self.root.join("voices");
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(&dir, e)),
        };
        let mut ids: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        ids.sort();
        Ok(ids.iter().map(|id| self.load(id)).collect())
    }

    pub fn load(&self, id: &str) -> Result<VoiceEntry, CatalogError> {
        check_id(id)?;
        let dir = self.voice_dir(id);
        let path = dir.join(MANIFEST);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(CatalogError::NotFound(id.into())),
            Err(e) => return Err(io_error(&path, e)),
        };
        let bad = |reason: String| CatalogError::Manifest { voice: id.into(), reason };
        let m: Manifest = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        if m.version != 1 {
            return Err(bad(format!("version {} (this client reads 1)", m.version)));
        }
        if m.id != id {
            return Err(bad(format!("id {:?} in the directory of {id:?}", m.id)));
        }
        let mut voice = VoiceEntry::new(id, &m.name, m.language, m.source, &m.model)?;
        voice.temperature = m.temperature;
        voice.pitch_semitones = m.pitch_semitones.map(|p| pitch_ok(p).map_err(|r| bad(r.into()))).transpose()?;
        voice.dir = dir;
        for (name, clip) in m.moods {
            let mood = Mood::parse(&name).ok_or_else(|| bad(format!("unknown mood {name:?}")))?;
            check_ref(id, &clip)?;
            read_clip(id, &voice.dir, &clip)?;
            voice.moods.insert(mood, clip);
        }
        for (name, clip) in m.events {
            let event = Event::parse(&name).ok_or_else(|| bad(format!("unknown event {name:?}")))?;
            read_clip(id, &voice.dir, &clip)?;
            voice.events.insert(event, clip);
        }
        Ok(voice)
    }

    pub fn save(&self, voice: &mut VoiceEntry) -> Result<(), CatalogError> {
        check_id(&voice.id)?;
        if let Some(p) = voice.pitch_semitones {
            pitch_ok(p).map_err(|r| CatalogError::Manifest { voice: voice.id.clone(), reason: r.into() })?;
        }
        let dir = self.voice_dir(&voice.id);
        std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        private(&dir, 0o700);
        let manifest = Manifest {
            version: 1,
            id: voice.id.clone(),
            name: voice.name.clone(),
            language: voice.language,
            source: voice.source,
            model: voice.model.clone(),
            temperature: voice.temperature,
            pitch_semitones: voice.pitch_semitones,
            moods: voice.moods.iter().map(|(m, c)| (mood_name(*m).to_string(), c.clone())).collect(),
            events: voice.events.iter().map(|(e, c)| (event_name(*e).to_string(), c.clone())).collect(),
        };
        let json = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| CatalogError::Manifest { voice: voice.id.clone(), reason: e.to_string() })?;
        write_atomic(&dir.join(MANIFEST), &json)?;
        voice.dir = dir;
        Ok(())
    }

    pub fn set_ref(&self, voice: &mut VoiceEntry, mood: Mood, wav: &[u8], transcript: &str) -> Result<(), CatalogError> {
        let file = format!("mood-{}.wav", mood_name(mood));
        let clip = Clip {
            transcript: transcript.trim().to_string(),
            duration_s: wav_seconds(wav).map_err(|r| bad_clip(voice, &file, r))?,
            sha256: sha256_hex(wav),
            file,
        };
        check_ref(&voice.id, &clip)?;
        self.write_clip(voice, &clip, wav)?;
        voice.moods.insert(mood, clip);
        self.save(voice)
    }

    pub fn clear_ref(&self, voice: &mut VoiceEntry, mood: Mood) -> Result<(), CatalogError> {
        if let Some(clip) = voice.moods.remove(&mood) {
            self.save(voice)?;
            remove(&voice.dir.join(&clip.file))?;
        }
        Ok(())
    }

    pub fn set_event_clip(&self, voice: &mut VoiceEntry, event: Event, wav: &[u8]) -> Result<(), CatalogError> {
        let file = format!("event-{}.wav", event_name(event));
        let clip = Clip {
            transcript: String::new(),
            duration_s: wav_seconds(wav).map_err(|r| bad_clip(voice, &file, r))?,
            sha256: sha256_hex(wav),
            file,
        };
        self.write_clip(voice, &clip, wav)?;
        voice.events.insert(event, clip);
        self.save(voice)
    }

    pub fn clear_event_clip(&self, voice: &mut VoiceEntry, event: Event) -> Result<(), CatalogError> {
        if let Some(clip) = voice.events.remove(&event) {
            self.save(voice)?;
            remove(&voice.dir.join(&clip.file))?;
        }
        Ok(())
    }

    fn write_clip(&self, voice: &mut VoiceEntry, clip: &Clip, bytes: &[u8]) -> Result<(), CatalogError> {
        check_id(&voice.id)?;
        let dir = self.voice_dir(&voice.id);
        std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        private(&dir, 0o700);
        voice.dir = dir;
        write_atomic(&voice.dir.join(&clip.file), bytes)
    }
}

fn pitch_ok(p: f32) -> Result<f32, &'static str> {
    if p.is_finite() && (-12.0..=12.0).contains(&p) { Ok(p) } else { Err("pitch_semitones must be a number in -12..=12") }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    id: String,
    name: String,
    language: Language,
    source: Source,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pitch_semitones: Option<f32>,
    #[serde(default)]
    moods: BTreeMap<String, Clip>,
    #[serde(default)]
    events: BTreeMap<String, Clip>,
}

fn io_error(path: &Path, e: io::Error) -> CatalogError {
    CatalogError::Io { path: path.to_path_buf(), kind: e.kind() }
}

fn bad_clip(voice: &VoiceEntry, file: &str, reason: &str) -> CatalogError {
    CatalogError::BadClip { voice: voice.id.clone(), file: file.into(), reason: reason.into() }
}

fn check_ref(voice: &str, clip: &Clip) -> Result<(), CatalogError> {
    let bad = |reason: &str| CatalogError::BadClip { voice: voice.into(), file: clip.file.clone(), reason: reason.into() };
    if clip.transcript.trim().is_empty() {
        return Err(bad("a mood reference needs its transcript"));
    }
    if !REF_SECONDS.contains(&clip.duration_s) {
        return Err(bad("a mood reference must be 1 to 30 seconds long"));
    }
    Ok(())
}

fn read_clip(voice: &str, dir: &Path, clip: &Clip) -> Result<Vec<u8>, CatalogError> {
    let plain = !clip.file.is_empty() && !clip.file.starts_with('.') && !clip.file.contains(['/', '\\']);
    if !plain {
        return Err(CatalogError::BadClip { voice: voice.into(), file: clip.file.clone(), reason: "not a plain file name".into() });
    }
    let path = dir.join(&clip.file);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(CatalogError::MissingClip { voice: voice.into(), file: clip.file.clone() })
        }
        Err(e) => return Err(io_error(&path, e)),
    };
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(&clip.sha256) {
        return Err(CatalogError::ShaMismatch { voice: voice.into(), file: clip.file.clone(), expected: clip.sha256.clone(), actual });
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn wav_seconds(b: &[u8]) -> Result<f64, &'static str> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a WAV file");
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (mut at, mut fmt, mut data) = (12usize, None, None);
    while at + 8 <= b.len() {
        let size = u32_at(at + 4) as usize;
        let body = at + 8;
        match &b[at..at + 4] {
            b"fmt " if size >= 16 && body + 16 <= b.len() => {
                fmt = Some((u16_at(body), u32_at(body + 4), u16_at(body + 12)));
            }
            b"data" => {
                data = Some(size.min(b.len() - body));
                break;
            }
            _ => {}
        }
        at = body.saturating_add(size).saturating_add(size & 1);
    }
    let (format, rate, block) = fmt.ok_or("WAV without a fmt chunk")?;
    let data = data.ok_or("WAV without a data chunk")?;
    if !(format == 1 || format == 3 || format == 0xFFFE) || rate == 0 || block == 0 {
        return Err("not a PCM WAV");
    }
    Ok(((data / block as usize) as f64 / rate as f64 * 1000.0).round() / 1000.0)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CatalogError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| io_error(&tmp, e))?;
    private(&tmp, 0o600);
    std::fs::rename(&tmp, path).map_err(|e| io_error(path, e))
}

fn remove(path: &Path) -> Result<(), CatalogError> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error(path, e)),
        _ => Ok(()),
    }
}

fn private(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

fn check_id(id: &str) -> Result<(), CatalogError> {
    let ok = (1..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(CatalogError::InvalidId(id.into()))
    }
}
