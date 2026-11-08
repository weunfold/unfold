

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use base64::Engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Off,
}

impl Playback {

    pub fn from_env() -> (Self, Option<String>) {
        match std::env::var("RTH_AUDIO_PLAYBACK").as_deref() {
            Ok("1") | Ok("on") => (Playback::Off, Some("RTH_AUDIO_PLAYBACK is set, but playback is not built; audio is saved to WAV only".into())),
            _ => (Playback::Off, None),
        }
    }
}

pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RTH_DATA_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/rth")
}

pub struct Wav {
    file: File,
    data_bytes: u64,
}

impl Wav {
    pub fn create(path: &Path, sample_rate: u32) -> std::io::Result<Self> {
        let mut file = File::create(path)?;
        file.write_all(&header(sample_rate, 0))?;
        Ok(Self { file, data_bytes: 0 })
    }

    pub fn append(&mut self, pcm: &[u8]) -> std::io::Result<()> {
        self.file.write_all(pcm)?;
        self.data_bytes += pcm.len() as u64;
        Ok(())
    }

    pub fn finish(mut self, sample_rate: u32) -> std::io::Result<u64> {
        let len = u32::try_from(self.data_bytes).unwrap_or(u32::MAX);
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(sample_rate, len))?;
        self.file.sync_data()?;
        Ok(self.data_bytes)
    }
}

fn header(sample_rate: u32, data_len: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36u32.saturating_add(data_len)).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

struct Track {
    wav: Wav,
    sample_rate: u32,
    next_chunk: u64,
    next_sample: u64,
    gaps: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Chunk {
    Accepted,

    Refused(String),
}

#[derive(Debug, Clone)]
pub struct Saved {
    pub path: PathBuf,
    pub samples: u64,
    pub chunks: u64,
    pub sample_rate: u32,

    pub gaps: u64,
}

pub struct AudioStore {
    dir: PathBuf,
    open: HashMap<String, Track>,
    ended: HashSet<String>,
    pub refused_after_end: u64,
    pub playback: Playback,
}

impl AudioStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, open: HashMap::new(), ended: HashSet::new(), refused_after_end: 0, playback: Playback::Off })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, response_id: &str) -> PathBuf {
        let safe: String = response_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
        self.dir.join(format!("{safe}.wav"))
    }

    pub fn chunk(&mut self, response_id: &str, chunk: u64, sample_offset: u64, sample_rate: u32, format: &str, data: &str) -> Chunk {
        if self.ended.contains(response_id) {
            self.refused_after_end += 1;
            return Chunk::Refused(format!("{response_id} already ended"));
        }
        if format != "s16le" {
            return Chunk::Refused(format!("unsupported format {format}"));
        }
        let pcm = match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(p) if p.len() % 2 == 0 => p,
            Ok(_) => return Chunk::Refused("odd byte count for s16le".into()),
            Err(e) => return Chunk::Refused(format!("bad base64: {e}")),
        };
        if !self.open.contains_key(response_id) {
            let wav = match Wav::create(&self.path_for(response_id), sample_rate) {
                Ok(w) => w,
                Err(e) => return Chunk::Refused(format!("cannot create wav: {e}")),
            };
            self.open.insert(response_id.to_string(), Track { wav, sample_rate, next_chunk: 0, next_sample: 0, gaps: 0 });
        }
        let track = self.open.get_mut(response_id).unwrap();
        if chunk != track.next_chunk || sample_offset != track.next_sample || sample_rate != track.sample_rate {
            track.gaps += 1;
        }
        if let Err(e) = track.wav.append(&pcm) {
            return Chunk::Refused(format!("wav write: {e}"));
        }
        track.next_chunk = chunk + 1;
        track.next_sample = sample_offset + (pcm.len() / 2) as u64;
        Chunk::Accepted
    }

    pub fn end(&mut self, response_id: &str) -> Option<std::io::Result<Saved>> {
        self.ended.insert(response_id.to_string());
        let track = self.open.remove(response_id)?;
        let path = self.path_for(response_id);
        let (chunks, samples, gaps, rate) = (track.next_chunk, track.next_sample, track.gaps, track.sample_rate);
        Some(track.wav.finish(rate).map(|_| Saved { path, samples, chunks, sample_rate: rate, gaps }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(b: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    #[test]
    fn writes_a_valid_wav_and_refuses_chunks_after_the_end() {
        let dir = std::env::temp_dir().join(format!("rth-audio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = AudioStore::new(dir.clone()).unwrap();
        assert_eq!(store.chunk("r1", 0, 0, 24_000, "s16le", &b64(&[1, 0, 2, 0])), Chunk::Accepted);
        assert_eq!(store.chunk("r1", 1, 2, 24_000, "s16le", &b64(&[3, 0])), Chunk::Accepted);
        let saved = store.end("r1").unwrap().unwrap();
        assert_eq!((saved.samples, saved.chunks, saved.gaps), (3, 2, 0));

        let bytes = std::fs::read(&saved.path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        assert_eq!(&bytes[44..], &[1, 0, 2, 0, 3, 0]);

        assert!(matches!(store.chunk("r1", 2, 3, 24_000, "s16le", &b64(&[9, 9])), Chunk::Refused(_)));
        assert_eq!(store.refused_after_end, 1);
        assert_eq!(std::fs::read(&saved.path).unwrap().len(), 50, "a refused chunk is never written");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_gap_is_counted_not_hidden() {
        let dir = std::env::temp_dir().join(format!("rth-audio-gap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = AudioStore::new(dir.clone()).unwrap();
        store.chunk("r", 0, 0, 24_000, "s16le", &b64(&[0, 0]));
        store.chunk("r", 2, 10, 24_000, "s16le", &b64(&[0, 0]));
        assert_eq!(store.end("r").unwrap().unwrap().gaps, 1);
        assert!(store.end("never-spoke").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn playback_stays_off_even_when_asked() {
        assert_eq!(Playback::from_env().0, Playback::Off);
    }
}
