

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};
use speech_markup::{Event, Mood};
use unfold_speech::{AudioChunk, Catalog, Language, Sink, Source, VoiceEntry};

pub const KEY: &str = "sekrit-KEY-123";

pub enum Reply {

    Events { chunks: Vec<(usize, i16)>, done: Option<Value>, cut: bool },

    Gated { chunks: Vec<(usize, i16)>, done: Value, delay: Duration, gate: Arc<Gate> },

    Budget { chunks: Vec<(usize, i16)>, done: Value, budget: u32 },

    GatewayBreak { chunks: Vec<(usize, i16)>, cut: &'static str, done: Value },

    Wav,
    Json { status: u16, body: Value, headers: Vec<(&'static str, &'static str)> },
}

#[derive(Default)]
pub struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
    pub done_sent: AtomicBool,
}

impl Gate {
    pub fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }
    fn wait(&self) {
        let guard = self.open.lock().unwrap();
        let _ = self.cv.wait_timeout_while(guard, Duration::from_secs(2), |open| !*open).unwrap();
    }
}

pub fn done(samples: usize, truncated: bool, error: Option<(&str, &str)>) -> Value {
    json!({
        "truncated": truncated,
        "error": error.map(|(c, m)| json!({"code": c, "message": m})),
        "frames": samples / 1920,
        "audio_seconds": ((samples as f64 / 24000.0) * 1000.0).round() / 1000.0,

        "mood_used": "joy",
    })
}

pub fn ok(chunks: Vec<(usize, i16)>) -> Reply {
    let total = chunks.iter().map(|c| c.0).sum();
    Reply::Events { chunks, done: Some(done(total, false, None)), cut: false }
}

#[derive(Debug, Clone)]
pub struct Recorded {
    pub head: String,
    pub body: Value,
}

impl Recorded {
    pub fn input(&self) -> String {
        self.body["input"].as_str().unwrap_or_default().to_string()
    }
}

pub struct Fake {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Fake {
    pub fn inputs(&self) -> Vec<String> {
        self.requests.lock().unwrap().iter().map(Recorded::input).collect()
    }
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

pub fn serve(handler: impl Fn(&Recorded) -> Reply + Send + 'static) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let len = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                .unwrap_or(0);
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let rec = Recorded { head, body: serde_json::from_slice(&body).unwrap_or(Value::Null) };
            log.lock().unwrap().push(rec.clone());
            write_reply(&mut conn, handler(&rec));
        }
    });
    Fake { url, requests }
}

fn chunk(w: &mut impl Write, data: &[u8]) {
    let _ = w.write_all(format!("{:x}\r\n", data.len()).as_bytes());
    let _ = w.write_all(data);
    let _ = w.write_all(b"\r\n");
}

fn write_reply(conn: &mut std::net::TcpStream, reply: Reply) {
    match reply {
        Reply::Events { chunks, done, cut } => {
            let _ = conn.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n",
            );
            for (n, v) in chunks {
                let pcm: Vec<u8> = std::iter::repeat_n(v.to_le_bytes(), n).flatten().collect();
                let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
                chunk(conn, format!("event: audio\ndata: {data}\n\n").as_bytes());
            }
            if cut {
                let _ = conn.write_all(b"1f4\r\nevent: au");
                return;
            }
            if let Some(d) = done {
                chunk(conn, format!("event: done\ndata: {d}\n\n").as_bytes());
            }
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        Reply::Gated { chunks, done, delay, gate } => {
            let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
            let _ = conn.flush();
            std::thread::sleep(delay);
            for (n, v) in chunks {
                let pcm: Vec<u8> = std::iter::repeat_n(v.to_le_bytes(), n).flatten().collect();
                let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
                chunk(conn, format!("event: audio\ndata: {data}\n\n").as_bytes());
                let _ = conn.flush();
            }
            gate.wait();
            gate.done_sent.store(true, Ordering::SeqCst);
            chunk(conn, format!("event: done\ndata: {done}\n\n").as_bytes());
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        Reply::Budget { chunks, done, budget } => {
            let _ = conn.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nX-Granite-Frame-Budget: {budget}\r\n\r\n").as_bytes(),
            );
            for (n, v) in chunks {
                chunk(conn, audio_event(n, v).as_bytes());
            }
            chunk(conn, format!("event: done\ndata: {done}\n\n").as_bytes());
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        Reply::GatewayBreak { chunks, cut, done } => {
            let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
            for (n, v) in chunks {
                chunk(conn, audio_event(n, v).as_bytes());
            }
            chunk(conn, cut.as_bytes());
            chunk(conn, format!("\n\nevent: done\ndata: {done}\n\n").as_bytes());
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        Reply::Wav => {
            let _ = conn.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nTransfer-Encoding: chunked\r\nTrailer: X-Granite-Truncated\r\n\r\n",
            );
            chunk(conn, &wav(1.0, 3)[..44]);
            chunk(conn, &[0u8; 3840]);
            let _ = conn.write_all(b"0\r\n\r\n");
        }
        Reply::Json { status, body, headers } => {
            let body = body.to_string();
            let mut head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            let _ = conn.write_all(format!("{head}\r\n{body}").as_bytes());
        }
    }
    let _ = conn.flush();
}

fn audio_event(n: usize, v: i16) -> String {
    let pcm: Vec<u8> = std::iter::repeat_n(v.to_le_bytes(), n).flatten().collect();
    let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
    format!("event: audio\ndata: {data}\n\n")
}

pub fn wav(seconds: f64, value: i16) -> Vec<u8> {
    let n = (seconds * 24000.0) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + n * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24000u32.to_le_bytes());
    out.extend_from_slice(&48000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(n * 2).to_le_bytes());
    for _ in 0..n {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

static N: AtomicUsize = AtomicUsize::new(0);

pub fn root(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn transcript(m: Mood) -> String {
    format!("Referência {m:?} da Sample.")
}

pub fn voice(name: &str, moods: &[Mood]) -> (Catalog, VoiceEntry) {
    let catalog = Catalog::new(root(name));
    let mut v = VoiceEntry::new("sample", "Sample", Language::PtBr, Source::Personal, "qwen3-tts-1.7b-base").unwrap();
    v.temperature = Some(1.0);
    catalog.save(&mut v).unwrap();
    for (i, m) in moods.iter().enumerate() {
        catalog.set_ref(&mut v, *m, &wav(1.5, i as i16 + 1), &transcript(*m)).unwrap();
    }
    catalog.set_event_clip(&mut v, Event::Laugh, &wav(1.2, 9)).unwrap();
    (catalog, v)
}

#[derive(Default, Debug)]
pub struct Recorder {
    pub audio: Vec<(usize, Vec<i16>)>,
    pub clips: Vec<(usize, Event, Option<PathBuf>)>,
    pub silences: Vec<(usize, u32)>,

    pub order: Vec<String>,
}

impl Recorder {
    pub fn samples(&self) -> usize {
        self.audio.iter().map(|(_, s)| s.len()).sum()
    }
    pub fn has_value(&self, v: i16) -> bool {
        self.audio.iter().any(|(_, s)| s.contains(&v))
    }
}

impl Sink for Recorder {
    fn audio(&mut self, step: usize, chunk: &AudioChunk) {
        assert_eq!(chunk.sample_rate, 24000);
        self.audio.push((step, chunk.samples.clone()));
        self.order.push(format!("a{step}"));
    }
    fn clip(&mut self, step: usize, event: Event, file: Option<&Path>) {
        self.clips.push((step, event, file.map(Path::to_path_buf)));
        self.order.push(format!("c{step}"));
    }
    fn silence(&mut self, step: usize, ms: u32) {
        self.silences.push((step, ms));
        self.order.push(format!("s{step}"));
    }
}
