

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rth_protocol::{Envelope, Event, MEDIA_REF_FORMAT};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

pub const SEGMENT_BYTES: usize = 32 * 1024;
const QUEUE: usize = 64;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn default_dir(journal: &Path) -> PathBuf {
    match std::env::var_os("RTH_MEDIA") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => journal.parent().unwrap_or(Path::new(".")).join("media"),
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {

    pub fn open(dir: &Path) -> std::io::Result<Self> {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            if let Some(parent) = dir.parent() {
                std::fs::File::open(parent)?.sync_all()?;
            }
        }
        Ok(Store { dir: dir.to_path_buf() })
    }

    pub fn path_of(&self, sha256: &str) -> PathBuf {
        self.dir.join(sha256)
    }

    pub fn put(&self, bytes: &[u8]) -> std::io::Result<String> {
        let sha = hex(&Sha256::digest(bytes));
        let final_path = self.path_of(&sha);
        if verify(&final_path, &sha, bytes.len() as u64) == Resolution::Ok {
            return Ok(sha);
        }

        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = self.dir.join(format!(".tmp-{sha}-{}-{n}", std::process::id()));
        let written = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            std::fs::rename(&tmp, &final_path)
        })();
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written?;
        std::fs::File::open(&self.dir)?.sync_all()?;
        Ok(sha)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ok,
    Missing,

    WrongLength { got: u64 },
    WrongHash { got: String },
}

pub fn verify(path: &Path, sha256: &str, bytes: u64) -> Resolution {
    let Ok(mut f) = std::fs::File::open(path) else { return Resolution::Missing };
    let mut data = Vec::new();
    if f.read_to_end(&mut data).is_err() {
        return Resolution::Missing;
    }
    if data.len() as u64 != bytes {
        return Resolution::WrongLength { got: data.len() as u64 };
    }
    let got = hex(&Sha256::digest(&data));
    if got != sha256 { Resolution::WrongHash { got } } else { Resolution::Ok }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum ChunkPcm {

    Inline(Vec<u8>),

    Stored(Vec<u8>),

    Damaged(Resolution),

    NotDurable,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve(records: &[Envelope], state_dir: &Path) -> Vec<((String, u64), ChunkPcm)> {
    use base64::Engine;

    let mut refs: HashMap<String, Vec<(u64, u64, u64, String, u64, String)>> = HashMap::new();
    for env in records {
        if let Event::MediaStored { media, response_id, stream_id, chunk_first, chunk_last, sample_start, sha256, bytes, path, .. } = &env.event {
            let key = format!("{media}:{}", response_id.as_deref().or(stream_id.as_deref()).unwrap_or(""));
            refs.entry(key).or_default().push((*chunk_first, *chunk_last, *sample_start, sha256.clone(), *bytes, path.clone()));
        }
    }
    let mut blobs: HashMap<String, Result<Vec<u8>, Resolution>> = HashMap::new();
    let mut out = Vec::new();
    for env in records {
        let (key, chunk, offset, data, digest_len) = match &env.event {
            Event::AgentAudio { response_id, chunk, sample_offset, data, .. } => (format!("agent_audio:{response_id}"), *chunk, *sample_offset, data, None),
            Event::UserAudio { stream_id, chunk, sample_offset, data, digest, .. } => {
                (format!("user_audio:{stream_id}"), *chunk, *sample_offset, data, digest.as_ref().map(|d| d.bytes))
            }
            _ => continue,
        };
        if !data.is_empty() {
            let pcm = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).unwrap_or_default();
            out.push(((key, chunk), ChunkPcm::Inline(pcm)));
            continue;
        }
        let found = refs.get(&key).and_then(|v| v.iter().find(|(a, b, ..)| (*a..=*b).contains(&chunk)));
        let pcm = match found {
            None => ChunkPcm::NotDurable,
            Some((_, _, start, sha, bytes, path)) => {
                let blob = blobs.entry(sha.clone()).or_insert_with(|| {
                    let p = state_dir.join(path);
                    match verify(&p, sha, *bytes) {
                        Resolution::Ok => std::fs::read(&p).map_err(|_| Resolution::Missing),
                        bad => Err(bad),
                    }
                });
                match blob {
                    Err(bad) => ChunkPcm::Damaged(bad.clone()),
                    Ok(b) => {
                        let from = ((offset - start) * 2) as usize;
                        let len = digest_len.map(|n| n as usize).unwrap_or_else(|| chunk_len(records, &key, chunk, b.len() - from.min(b.len())));
                        ChunkPcm::Stored(b.get(from..from + len).map(<[u8]>::to_vec).unwrap_or_default())
                    }
                }
            }
        };
        out.push(((key, chunk), pcm));
    }
    out
}

#[cfg_attr(not(test), allow(dead_code))]
fn chunk_len(records: &[Envelope], key: &str, chunk: u64, rest: usize) -> usize {
    let next = records.iter().find_map(|e| match &e.event {
        Event::AgentAudio { response_id, chunk: c, sample_offset, .. } if format!("agent_audio:{response_id}") == key && *c == chunk + 1 => Some(*sample_offset),
        _ => None,
    });
    let this = records.iter().find_map(|e| match &e.event {
        Event::AgentAudio { response_id, chunk: c, sample_offset, .. } if format!("agent_audio:{response_id}") == key && *c == chunk => Some(*sample_offset),
        _ => None,
    });
    match (this, next) {
        (Some(a), Some(b)) => ((b - a) * 2) as usize,
        _ => rest,
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub media: &'static str,
    pub response_id: Option<String>,
    pub stream_id: Option<String>,
    pub sample_rate: u32,
    pub chunk_first: u64,
    pub chunk_last: u64,
    pub sample_start: u64,
    pub sample_end: u64,
    pub first_event: String,
    pub last_event: String,
    pub t_ns_first: u64,
    pub t_ns_last: u64,
    pub pcm: Vec<u8>,
}

impl Segment {
    fn key(&self) -> String {
        format!("{}:{}", self.media, self.response_id.as_deref().or(self.stream_id.as_deref()).unwrap_or(""))
    }
}

#[derive(Default)]
pub struct Segmenter {
    open: HashMap<String, Segment>,
}

pub struct Chunk<'a> {
    pub media: &'static str,
    pub stream: &'a str,
    pub sample_rate: u32,
    pub chunk: u64,
    pub sample_offset: u64,
    pub event: &'a str,
    pub t_ns: u64,
    pub pcm: &'a [u8],
}

impl Segmenter {

    pub fn push(&mut self, c: Chunk<'_>) -> Vec<Segment> {
        let key = format!("{}:{}", c.media, c.stream);
        let mut out = Vec::new();
        if let Some(seg) = self.open.get(&key)
            && (seg.sample_end != c.sample_offset || seg.sample_rate != c.sample_rate || seg.chunk_last + 1 != c.chunk)
        {
            out.push(self.open.remove(&key).unwrap());
        }
        let seg = self.open.entry(key.clone()).or_insert_with(|| Segment {
            media: c.media,
            response_id: (c.media == "agent_audio").then(|| c.stream.to_string()),
            stream_id: (c.media == "user_audio").then(|| c.stream.to_string()),
            sample_rate: c.sample_rate,
            chunk_first: c.chunk,
            chunk_last: c.chunk,
            sample_start: c.sample_offset,
            sample_end: c.sample_offset,
            first_event: c.event.to_string(),
            last_event: c.event.to_string(),
            t_ns_first: c.t_ns,
            t_ns_last: c.t_ns,
            pcm: Vec::new(),
        });
        seg.chunk_last = c.chunk;
        seg.sample_end = c.sample_offset + (c.pcm.len() / 2) as u64;
        seg.last_event = c.event.to_string();
        seg.t_ns_last = c.t_ns;
        seg.pcm.extend_from_slice(c.pcm);
        if seg.pcm.len() >= SEGMENT_BYTES {
            out.push(self.open.remove(&key).unwrap());
        }
        out
    }

    pub fn finish(&mut self, media: &str, stream: &str) -> Option<Segment> {
        self.open.remove(&format!("{media}:{stream}"))
    }
}

pub fn crash_after_blob() -> bool {
    std::env::var("RTH_AGENTD_CRASH_AFTER").is_ok_and(|v| v == "media_blob_written")
}

pub enum Stored {
    Durable { seg: Segment, sha256: String, bytes: u64, path: String },
    Failed { seg_key: String, chunks: (u64, u64), why: String },
}

impl Stored {
    pub fn event(seg: &Segment, sha256: &str, bytes: u64, path: &str, clock: &str) -> Event {
        Event::MediaStored {
            ref_format: MEDIA_REF_FORMAT.into(),
            media: seg.media.into(),
            response_id: seg.response_id.clone(),
            stream_id: seg.stream_id.clone(),
            sha256: sha256.into(),
            bytes,
            encoding: "s16le".into(),
            sample_rate: seg.sample_rate,
            channels: 1,
            chunk_first: seg.chunk_first,
            chunk_last: seg.chunk_last,
            sample_start: seg.sample_start,
            sample_end: seg.sample_end,
            first_event: seg.first_event.clone(),
            last_event: seg.last_event.clone(),
            clock: clock.into(),
            t_ns_first: seg.t_ns_first,
            t_ns_last: seg.t_ns_last,
            path: path.into(),
        }
    }
}

pub fn spawn_writer(store: Store, freeze_after_blob: bool) -> (mpsc::Sender<Segment>, mpsc::UnboundedReceiver<Stored>) {
    let (tx, mut rx) = mpsc::channel::<Segment>(QUEUE);
    let (done_tx, done_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Some(seg) = rx.blocking_recv() {
            let result = match store.put(&seg.pcm) {
                Ok(sha) => {
                    if freeze_after_blob {
                        return;
                    }
                    if crash_after_blob() {
                        eprintln!("rth-agentd: RTH_AGENTD_CRASH_AFTER=media_blob_written: exiting after the durable blob");
                        std::process::exit(86);
                    }
                    let bytes = seg.pcm.len() as u64;
                    let path = format!("media/{sha}");
                    Stored::Durable { seg, sha256: sha, bytes, path }
                }
                Err(e) => Stored::Failed { seg_key: seg.key(), chunks: (seg.chunk_first, seg.chunk_last), why: e.to_string() },
            };
            if done_tx.send(result).is_err() {
                return;
            }
        }
    });
    (tx, done_rx)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn orphans(dir: &Path, records: &[Envelope]) -> Vec<String> {
    let named: std::collections::HashSet<&str> = records
        .iter()
        .filter_map(|e| match &e.event {
            Event::MediaStored { sha256, .. } => Some(sha256.as_str()),
            _ => None,
        })
        .collect();
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()) && !named.contains(n.as_str()))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-media-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parallel_puts_of_one_blob_each_land_whole() {

        let d = dir("put-race");
        let store = std::sync::Arc::new(Store::open(&d.join("media")).unwrap());
        let bytes = vec![7u8; 64 * 1024];
        let sha = hex(&Sha256::digest(&bytes));
        for _ in 0..20 {
            let _ = std::fs::remove_file(store.path_of(&sha));
            let writers: Vec<_> = (0..8)
                .map(|_| {
                    let (store, bytes) = (store.clone(), bytes.clone());
                    std::thread::spawn(move || store.put(&bytes))
                })
                .collect();
            for w in writers {
                assert_eq!(w.join().unwrap().expect("every put lands"), sha);
            }
            assert_eq!(verify(&store.path_of(&sha), &sha, bytes.len() as u64), Resolution::Ok);
        }
        assert_eq!(std::fs::read_dir(d.join("media")).unwrap().count(), 1, "no temp file is left behind");
    }

    #[test]
    fn put_is_content_addressed_idempotent_and_repairs_a_damaged_blob() {
        let d = dir("put");
        let store = Store::open(&d.join("media")).unwrap();
        let sha = store.put(b"abcd").unwrap();
        assert_eq!(sha, hex(&Sha256::digest(b"abcd")));
        assert_eq!(store.put(b"abcd").unwrap(), sha);
        assert_eq!(verify(&store.path_of(&sha), &sha, 4), Resolution::Ok);
        std::fs::write(store.path_of(&sha), b"ab").unwrap();
        assert_eq!(verify(&store.path_of(&sha), &sha, 4), Resolution::WrongLength { got: 2 });
        std::fs::write(store.path_of(&sha), b"abce").unwrap();
        assert!(matches!(verify(&store.path_of(&sha), &sha, 4), Resolution::WrongHash { .. }));
        store.put(b"abcd").unwrap();
        assert_eq!(verify(&store.path_of(&sha), &sha, 4), Resolution::Ok);
        std::fs::remove_file(store.path_of(&sha)).unwrap();
        assert_eq!(verify(&store.path_of(&sha), &sha, 4), Resolution::Missing);

        assert!(std::fs::read_dir(d.join("media")).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".tmp")));
    }

    #[test]
    fn segments_are_contiguous_and_cut_at_size_and_gaps() {
        let mut s = Segmenter::default();
        let pcm = vec![1u8; 9600];
        let mut cut = Vec::new();
        for c in 0..4u64 {
            cut.extend(s.push(Chunk { media: "agent_audio", stream: "r1", sample_rate: 24_000, chunk: c, sample_offset: c * 4800, event: "e", t_ns: c, pcm: &pcm }));
        }
        assert_eq!(cut.len(), 1, "cut at {SEGMENT_BYTES} bytes");
        assert_eq!((cut[0].chunk_first, cut[0].chunk_last, cut[0].sample_start, cut[0].sample_end), (0, 3, 0, 19_200));

        assert!(s.push(Chunk { media: "agent_audio", stream: "r1", sample_rate: 24_000, chunk: 4, sample_offset: 19_200, event: "e", t_ns: 4, pcm: &pcm }).is_empty());
        let g = s.push(Chunk { media: "agent_audio", stream: "r1", sample_rate: 24_000, chunk: 6, sample_offset: 28_800, event: "e", t_ns: 6, pcm: &pcm });
        assert_eq!((g[0].chunk_first, g[0].chunk_last), (4, 4));
        let last = s.finish("agent_audio", "r1").unwrap();
        assert_eq!((last.chunk_first, last.sample_start, last.sample_end), (6, 28_800, 33_600));
    }
}
