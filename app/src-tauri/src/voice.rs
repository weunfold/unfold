

use std::any::Any;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use speech_markup::{Event, Mood};
use unfold_speech::{ApiKey, Catalog, Client, Language, Source, VoiceEntry};

use crate::speaker::{Job, Speaker};

pub const ACTIONS: &[&str] = &[
    "voice_probe", "voice_preview", "voice_stop", "voice_choose", "voice_enable", "say_set", "clone_create", "clone_rename", "clone_delete",
    "clone_record_start", "clone_record_stop", "clone_record_cancel", "clone_file_pick", "clone_ref_file", "clone_ref_text", "clone_ref_clear",
    "clone_ref_play", "clone_preview", "voice_import", "voice_key_set", "voice_key_clear", "fleet_preview", "fleet_choose", "voice_on", "listening_set",
];

const LATER: &[&str] = &["clone_file_pick", "clone_ref_file"];

const TAKE_OK: std::ops::RangeInclusive<f64> = 5.0..=9.0;
const TAKE_MAX_SAMPLES: usize = 60 * 192_000;

const IMPORT_FILES: usize = 32;
const IMPORT_BYTES: usize = 20 << 20;

const CLONE_LINES: &str = include_str!("../../../crates/unfold-speech/data/clone_lines_v1.json");

const CLONE_LINES_SHA256: &str = "8d2ae1b5870f6c5d980f052379bf08e84c51afbdb68251d92cc555831a4e4b87";
const SAY_MODES: &[&str] = &["full", "first_sentence", "summary", "off"];
const MOODS: [Mood; 8] = [Mood::Neutral, Mood::Calm, Mood::Joy, Mood::Serious, Mood::Question, Mood::Surprised, Mood::Tired, Mood::Tender];
const EVENTS: [Event; 7] = [Event::Laugh, Event::Giggle, Event::Sigh, Event::Gasp, Event::Breath, Event::Um, Event::Pause];

const SPEECH: &str = "speech_synthesis";
const TIERS: [&str; 3] = ["good_gpu", "gpu_poor", "cpu"];

const LOCAL_GATEWAY: &str = "http://127.0.0.1:8310";

const FLEET_MODEL: &str = "qwen3-tts-1.7b-base";

const FLEET_DEFAULT: &str = "unfold-pt";
const NAME_MAX: usize = 24;

#[derive(Debug, PartialEq)]
pub enum Refusal {

    Later(String),

    NoDevice,

    TranscriptEmpty,

    KeyBad,

    KeyNotSaved(String),

    MicAsk,
    MicDenied,

    MicBusy,
    MicFailed(String),

    NotAVoice(String),

    TooBig,

    VoiceExists,
    Bad(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MicState {
    Granted,
    NotAsked,
    Denied,
}

pub trait Mic: Send + Sync {
    fn state(&self) -> MicState;

    fn ask(&self);

    fn open(&self, sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any + Send>, u32), String>;
}

struct Take {
    voice_id: String,
    mood: Mood,
    rate: u32,
    buf: Arc<Mutex<Vec<f32>>>,
    stream: Box<dyn Any + Send>,
}

#[derive(Default)]
pub struct Studio {
    take: Mutex<Option<Take>>,

    epoch: AtomicU64,
}
pub static STUDIO: Studio = Studio { take: Mutex::new(None), epoch: AtomicU64::new(0) };

impl Studio {
    pub fn recording(&self) -> bool {
        lock(&self.take).is_some()
    }

    fn cancel_take(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        let take = lock(&self.take).take();
        drop(take);
    }

    pub fn cancel(&self) {
        self.cancel_take();
    }
}

pub trait Backend: Send + Sync {
    fn devices(&self, base_url: &str, key: Option<&str>, end: std::time::Instant) -> Result<Value, String>;
    fn voices(&self, base_url: &str, key: Option<&str>, end: std::time::Instant) -> Result<Value, String>;
}

pub trait Keys: Send + Sync {

    fn read(&self) -> Result<Option<String>, String>;

    fn present(&self) -> bool;
    fn write(&self, key: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

pub struct Fleet;

const PROBE_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
const REPLY_MAX: usize = 1 << 20;
impl Backend for Fleet {
    fn devices(&self, base_url: &str, key: Option<&str>, end: std::time::Instant) -> Result<Value, String> {
        get(base_url, "/v1/devices", key, end)
    }
    fn voices(&self, base_url: &str, key: Option<&str>, end: std::time::Instant) -> Result<Value, String> {
        get(base_url, "/v1/voices", key, end)
    }
}

fn get(base_url: &str, path: &str, key: Option<&str>, end: std::time::Instant) -> Result<Value, String> {
    use std::io::{Read, Write};
    let left = || end.checked_duration_since(std::time::Instant::now()).filter(|d| !d.is_zero()).ok_or_else(|| format!("GET {path}: no reply in time"));
    let io = |e: std::io::Error| format!("GET {path}: {e}");
    let rest = base_url.strip_prefix("http://").ok_or("node_url must be http://host:port")?;
    let host = rest.trim_end_matches('/');
    let addr = std::net::ToSocketAddrs::to_socket_addrs(host).map_err(io)?.next().ok_or("no address for node_url")?;
    let mut s = std::net::TcpStream::connect_timeout(&addr, left()?).map_err(io)?;
    s.set_write_timeout(Some(left()?)).map_err(io)?;
    let auth = key.map(|k| format!("Authorization: Bearer {k}\r\n")).unwrap_or_default();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nX-LocalFleet-Client: unfold\r\n{auth}Connection: close\r\n\r\n").map_err(io)?;
    let (mut raw, mut buf) = (Vec::new(), [0u8; 16 * 1024]);
    let body = loop {
        if let Some(body) = whole_reply(&raw, false, path)? {
            break body;
        }
        s.set_read_timeout(Some(left()?)).map_err(io)?;
        let n = s.read(&mut buf).map_err(io)?;
        if n == 0 {
            break whole_reply(&raw, true, path)?.ok_or_else(|| format!("GET {path}: the reply ended early"))?;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > REPLY_MAX + buf.len() {
            return Err(format!("GET {path}: the reply is over 1 MiB"));
        }
    };
    serde_json::from_slice(&raw[body]).map_err(|e| format!("GET {path}: {e}"))
}

fn whole_reply(raw: &[u8], closed: bool, path: &str) -> Result<Option<std::ops::Range<usize>>, String> {
    let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") else { return Ok(None) };
    let head = String::from_utf8_lossy(&raw[..i]).to_ascii_lowercase();
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(format!("GET {path}: HTTP {status}"));
    }
    if head.contains("transfer-encoding: chunked") {
        return Err(format!("GET {path}: chunked replies are not read"));
    }
    let at = i + 4;
    match head.lines().find_map(|l| l.strip_prefix("content-length:")).map(|n| n.trim().parse::<usize>()) {
        Some(Ok(n)) if n > REPLY_MAX => Err(format!("GET {path}: the reply is over 1 MiB")),
        Some(Ok(n)) => Ok((raw.len() >= at + n).then_some(at..at + n)),
        Some(Err(_)) => Err(format!("GET {path}: a bad Content-Length")),
        None => Ok(closed.then_some(at..raw.len())),
    }
}

fn on_this_mac(base_url: &str) -> bool {
    let rest = base_url.strip_prefix("http://").unwrap_or("");
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Prefs {

    #[serde(default)]
    on: bool,
    #[serde(default = "full")]
    say_mode: String,
    #[serde(default)]
    listening_sounds: Option<String>,
    #[serde(default)]
    chosen: Option<Value>,
    #[serde(default)]
    enabled: BTreeMap<String, bool>,
    #[serde(default)]
    node_url: Option<String>,

    #[serde(default)]
    fleet: Option<Value>,
}
fn full() -> String {
    "full".into()
}

#[derive(Debug, Default, Clone)]
pub struct Probed {
    devices: Vec<Value>,
    recommended: Option<Value>,

    voices: Vec<Value>,

    here: bool,
    unreachable: bool,
    done: bool,

    error: Option<String>,

    key_error: Option<String>,
}

pub struct Voice<'a> {
    pub root: PathBuf,
    pub backend: &'a dyn Backend,

    pub env_key: Option<String>,
    pub keys: &'a dyn Keys,
    pub probed: &'a Mutex<Probed>,
    pub mic: &'a dyn Mic,
    pub studio: &'static Studio,

    pub speaker: &'static Speaker,

    pub emit: fn(&Value),
}

pub static PROBED: Mutex<Probed> = Mutex::new(Probed { devices: Vec::new(), recommended: None, voices: Vec::new(), here: false, unreachable: false, done: false, error: None, key_error: None });

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Voice<'_> {

    pub fn act(&self, action: &str, value: &Value) -> Result<Value, Refusal> {
        if LATER.contains(&action) {
            return Err(Refusal::Later(action.into()));
        }
        let cat = Catalog::new(&self.root);
        let mut extra = json!({});
        match action {
            "voice_probe" => self.probe(),

            "voice_key_set" => {
                let key = text(value, "key")?.trim();
                if !(8..=512).contains(&key.len()) || !key.chars().all(|c| c.is_ascii_graphic()) {
                    return Err(Refusal::KeyBad);
                }
                self.keys.write(key).map_err(Refusal::KeyNotSaved)?;
                self.probe();
            }
            "voice_key_clear" => {
                self.keys.clear().map_err(Refusal::KeyNotSaved)?;
                self.probe();
            }
            "voice_stop" => self.speaker.stop(),
            "fleet_preview" => {
                let (v, mood) = self.fleet_voice(value)?;
                let mood_of = MOODS.iter().copied().find(|m| unfold_speech::catalog::mood_name(*m) == mood).unwrap_or(Mood::Neutral);
                let words = line(mood_of, Language::PtBr).unwrap_or_else(|| preview_line(Language::PtBr).into());
                let body = json!({"model": FLEET_MODEL, "input": words, "voice": v["id"], "mood": mood, "stream": true, "stream_format": "events"});
                let url = self.prefs().node_url.unwrap_or_else(|| LOCAL_GATEWAY.into());
                let pitch = v["pitch_semitones"].as_f64().map(|p| p as f32);
                let id = self.speaker.start(Job::Fleet { url, key: self.key(), body, pitch, more: None, retries: BUSY_RETRIES }, self.emit);
                extra = json!({"voice_started": {"preview_id": id, "total_steps": 1}});
            }
            "fleet_choose" => {
                let (v, mood) = self.fleet_voice(value)?;
                self.prefs_edit(|p| p.fleet = Some(json!({"voice_id": v["id"], "mood": mood})))?;
            }

            "voice_on" => {
                let on = value["on"].as_bool().ok_or_else(|| Refusal::Bad("voice_on: on must be true or false".into()))?;
                let listed = lock(self.probed).voices.clone();
                self.prefs_edit(|p| {
                    p.on = on;
                    if on && p.say_mode == "off" {
                        p.say_mode = "summary".into();
                    }
                    if on && fleet_chosen(p, &listed).is_null() {
                        if let Some(v) = listed.first() {
                            p.fleet = Some(json!({"voice_id": v["id"], "mood": v["default_mood"]}));
                        }
                    }
                })?;
                if !on {
                    self.speaker.stop();
                }
            }
            "voice_preview" => {
                let v = cat.load(text(value, "voice_id")?).map_err(bad)?;
                let language = match value.get("lang").and_then(Value::as_str) {
                    Some("pt-BR") => Language::PtBr,
                    Some("en") => Language::En,
                    _ => v.language,
                };
                extra = self.preview(&v, Mood::Neutral, language)?;
            }
            "clone_preview" => {
                let v = self.personal(&cat, value)?;
                extra = self.preview(&v, mood(value)?, v.language)?;
            }
            "clone_ref_play" => {
                let v = self.personal(&cat, value)?;
                extra = self.ref_play(&v, mood(value)?)?;
            }
            "clone_record_cancel" => self.studio.cancel_take(),
            "clone_record_start" => self.record_start(&cat, value)?,
            "clone_record_stop" => extra = self.record_stop(&cat)?,
            "voice_import" => extra = json!({"voice_created": {"voice_id": self.import(&cat, value)?}}),
            "listening_set" => {
                let level = text(value, "level")?;
                if !["off", "low", "moderate"].contains(&level) { return Err(Refusal::Bad("listening_set: expected off, low or moderate".into())); }
                self.prefs_edit(|p| p.listening_sounds = Some(level.into()))?;
            }
            "say_set" => {
                let mode = text(value, "mode")?;
                if !SAY_MODES.contains(&mode) {
                    return Err(Refusal::Bad(format!("say_set: {mode:?} is not a say mode")));
                }

                self.prefs_edit(|p| {
                    p.say_mode = mode.into();
                    p.on &= mode != "off";
                })?;
                if mode == "off" {
                    self.speaker.stop();
                }
            }
            "voice_choose" => {
                let id = text(value, "voice_id")?;
                let v = cat.load(id).map_err(bad)?;
                if v.source == Source::Personal && !v.moods.contains_key(&Mood::Neutral) {
                    return Err(Refusal::Bad(format!("voice_choose: {id} has no Neutral reference")));
                }
                let tier = text(value, "tier")?;
                let device = text(value, "device_id")?;
                if !TIERS.contains(&tier) || device.is_empty() || device.len() > 64 {
                    return Err(Refusal::Bad(format!("voice_choose: tier {tier:?} on device {device:?}")));
                }
                let chosen = json!({"voice_id": id, "tier": tier, "device_id": device});
                self.prefs_edit(|p| {
                    p.chosen = Some(chosen);
                    p.enabled.insert(id.into(), true);
                })?;
            }
            "voice_enable" => {
                let id = text(value, "voice_id")?;
                let on = value["on"].as_bool().ok_or_else(|| Refusal::Bad("voice_enable: on must be true or false".into()))?;
                cat.load(id).map_err(bad)?;
                let p = self.prefs();
                if !on && p.chosen.as_ref().is_some_and(|c| c["voice_id"] == id) {
                    return Err(Refusal::Bad("voice_enable: the chosen voice stays on".into()));
                }
                self.prefs_edit(|p| {
                    p.enabled.insert(id.into(), on);
                })?;
            }
            "clone_create" => {
                let name = self.name(value)?;
                let language = match text(value, "language")? {
                    "pt-BR" => Language::PtBr,
                    "en" => Language::En,
                    l => return Err(Refusal::Bad(format!("clone_create: language {l:?}"))),
                };
                if !lock(self.probed).done {
                    self.probe();
                }
                let model = lock(self.probed).recommended.as_ref().and_then(|r| r["models"][0].as_str().map(String::from)).ok_or(Refusal::NoDevice)?;
                let id = fresh_id(&cat)?;
                let mut v = VoiceEntry::new(&id, &name, language, Source::Personal, &model).map_err(bad)?;
                cat.save(&mut v).map_err(bad)?;
                extra = json!({"voice_created": {"voice_id": id}});
            }
            "clone_rename" => {
                let name = self.name(value)?;
                let mut v = self.personal(&cat, value)?;
                v.name = name;
                cat.save(&mut v).map_err(bad)?;
            }
            "clone_delete" => {
                let v = self.personal(&cat, value)?;
                if lock(&self.studio.take).as_ref().is_some_and(|t| t.voice_id == v.id) {
                    self.studio.cancel_take();
                }
                std::fs::remove_dir_all(v.dir()).map_err(|e| Refusal::Bad(format!("clone_delete: {e}")))?;
                self.prefs_edit(|p| {
                    if p.chosen.as_ref().is_some_and(|c| c["voice_id"] == v.id.as_str()) {
                        p.chosen = None;
                    }
                    p.enabled.remove(&v.id);
                })?;
            }
            "clone_ref_clear" => {
                let mut v = self.personal(&cat, value)?;
                let m = mood(value)?;
                if v.moods.contains_key(&m) {
                    cat.clear_ref(&mut v, m).map_err(bad)?;
                }
            }
            "clone_ref_text" => {
                let mut v = self.personal(&cat, value)?;
                let m = mood(value)?;
                let words = value["text"].as_str().unwrap_or("").trim();
                if words.is_empty() {
                    return Err(Refusal::TranscriptEmpty);
                }
                if words.chars().count() > unfold_speech::MAX_INPUT_CHARS {
                    return Err(Refusal::Bad(format!("clone_ref_text: over {} characters", unfold_speech::MAX_INPUT_CHARS)));
                }
                let clip = v.moods.get_mut(&m).ok_or_else(|| Refusal::Bad(format!("clone_ref_text: {} has no {m:?} reference", v.id)))?;
                clip.transcript = words.into();
                cat.save(&mut v).map_err(bad)?;
            }
            _ => return Err(Refusal::Bad(format!("{action}: not a voice action"))),
        }
        let mut reply = json!({"voice": self.view(&cat)});
        if let (Some(r), Some(x)) = (reply.as_object_mut(), extra.as_object()) {
            r.extend(x.clone());
        }
        Ok(reply)
    }

    fn record_start(&self, cat: &Catalog, value: &Value) -> Result<(), Refusal> {
        let v = self.personal(cat, value)?;
        let mood = mood(value)?;
        if self.studio.recording() {
            return Err(Refusal::MicBusy);
        }

        self.speaker.stop();
        match self.mic.state() {
            MicState::Granted => {}
            MicState::NotAsked => {
                self.mic.ask();
                return Err(Refusal::MicAsk);
            }
            MicState::Denied => return Err(Refusal::MicDenied),
        }
        let buf = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&buf);
        let sink = Box::new(move |chunk: Vec<f32>| {
            let mut b = lock(&into);
            let room = TAKE_MAX_SAMPLES.saturating_sub(b.len());
            b.extend(chunk.into_iter().take(room));
        });
        let (stream, rate) = self.mic.open(sink).map_err(Refusal::MicFailed)?;
        if rate == 0 {
            return Err(Refusal::MicFailed("the microphone reported no rate".into()));
        }
        let epoch = self.studio.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        *lock(&self.studio.take) = Some(Take { voice_id: v.id, mood, rate, buf, stream });
        let (studio, emit) = (self.studio, self.emit);
        std::thread::Builder::new()
            .name("voice-take".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if studio.epoch.load(Ordering::SeqCst) != epoch {
                    return;
                }
                let level = {
                    let t = lock(&studio.take);
                    let Some(t) = t.as_ref() else { return };
                    let b = lock(&t.buf);
                    meter(&b, t.rate)
                };
                emit(&json!({"rec": level}));
            })
            .map_err(|e| {
                self.studio.cancel_take();
                Refusal::MicFailed(e.to_string())
            })?;
        Ok(())
    }

    fn record_stop(&self, cat: &Catalog) -> Result<Value, Refusal> {
        self.studio.epoch.fetch_add(1, Ordering::SeqCst);
        let take = lock(&self.studio.take).take().ok_or_else(|| Refusal::Bad("clone_record_stop: nothing is recording".into()))?;
        let Take { voice_id, mood, rate, buf, stream } = take;
        drop(stream);
        let samples = std::mem::take(&mut *lock(&buf));
        let seconds = (samples.len() as f64 / rate as f64 * 10.0).round() / 10.0;
        let status = if seconds < *TAKE_OK.start() { "too_short" } else if seconds > *TAKE_OK.end() { "too_long" } else { "ok" };
        let mut text = String::new();
        if status == "ok" {
            let mut v = cat.load(&voice_id).map_err(bad)?;
            text = line(mood, v.language).ok_or_else(|| Refusal::Bad(format!("clone_record_stop: no read-along line for {mood:?}")))?;
            cat.set_ref(&mut v, mood, &wav16(&samples, rate), &text).map_err(bad)?;
        }
        Ok(json!({"voice_take": {"seconds": seconds, "status": status, "text": text}}))
    }

    fn import(&self, cat: &Catalog, value: &Value) -> Result<String, Refusal> {
        use base64::Engine;
        let no = |why: &str| Refusal::NotAVoice(why.to_string());

        if value["over_caps"].as_bool() == Some(true) {
            return Err(Refusal::TooBig);
        }
        let files = value["files"].as_array().filter(|f| !f.is_empty()).ok_or_else(|| no("no files"))?;
        if files.len() > IMPORT_FILES {
            return Err(Refusal::TooBig);
        }
        let (mut got, mut total) = (BTreeMap::new(), 0usize);
        for f in files {
            let name = f["name"].as_str().unwrap_or("");

            if name.starts_with('.') && !name.contains(['/', '\\']) {
                continue;
            }
            let plain = !name.is_empty() && name.len() <= 128 && !name.starts_with('.') && !name.contains(['/', '\\']);
            if !plain {
                return Err(no("a file name that isn't plain"));
            }
            let bytes = base64::engine::general_purpose::STANDARD.decode(f["bytes_b64"].as_str().unwrap_or("")).map_err(|_| no("a file that isn't base64"))?;
            total += bytes.len();
            if total > IMPORT_BYTES {
                return Err(Refusal::TooBig);
            }
            if got.insert(name.to_string(), bytes).is_some() {
                return Err(no("a file name twice"));
            }
        }
        let manifest = got.get("voice.json").ok_or_else(|| no("no voice.json"))?;
        let id = serde_json::from_slice::<Value>(manifest).ok().and_then(|m| m["id"].as_str().map(String::from)).ok_or_else(|| no("voice.json has no id"))?;
        match cat.load(&id) {
            Err(unfold_speech::CatalogError::NotFound(_)) => {}
            Ok(_) => return Err(Refusal::VoiceExists),
            Err(e) => return Err(Refusal::NotAVoice(e.to_string())),
        }
        let staging = self.root.join(format!(".import-{}", fresh_id(cat)?));
        let _gone = Removed(staging.clone());
        let dir = staging.join("voices").join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| Refusal::Bad(format!("voice_import: {e}")))?;
        for (name, bytes) in &got {
            std::fs::write(dir.join(name), bytes).map_err(|e| Refusal::Bad(format!("voice_import: {e}")))?;
        }
        let src = Catalog::new(&staging).load(&id).map_err(|e| Refusal::NotAVoice(e.to_string()))?;
        let named: std::collections::BTreeSet<&str> = src.moods.values().chain(src.events.values()).map(|c| c.file.as_str()).chain(["voice.json"]).collect();
        if let Some(extra) = got.keys().find(|n| !named.contains(n.as_str())) {
            return Err(Refusal::NotAVoice(format!("{extra} isn't named in voice.json")));
        }
        let mut v = VoiceEntry::new(&id, &src.name, src.language, Source::Personal, &src.model).map_err(bad)?;
        v.temperature = src.temperature;
        v.pitch_semitones = src.pitch_semitones;
        let made = (|| {
            cat.save(&mut v)?;
            for (m, c) in &src.moods {
                cat.set_ref(&mut v, *m, &got[&c.file], &c.transcript)?;
            }
            for (e, c) in &src.events {
                cat.set_event_clip(&mut v, *e, &got[&c.file])?;
            }
            Ok::<(), unfold_speech::CatalogError>(())
        })();
        if let Err(e) = made {
            let _ = std::fs::remove_dir_all(self.root.join("voices").join(&id));
            return Err(Refusal::NotAVoice(e.to_string()));
        }
        Ok(id)
    }

    pub fn shown(&self) -> Option<Value> {
        let done = lock(self.probed).done;
        done.then(|| self.view(&Catalog::new(&self.root)))
    }

    fn view(&self, cat: &Catalog) -> Value {
        let p = self.prefs();
        let probed = lock(self.probed).clone();
        let voices: Vec<Value> = cat.list().unwrap_or_default().into_iter().filter_map(Result::ok).map(|v| row(&v, &p)).collect();
        json!({
            "devices": probed.devices, "recommended": probed.recommended, "unreachable": probed.unreachable,
            "voices": voices, "lines": lines()["lines"].clone(), "chosen": p.chosen, "on": p.on, "say_mode": p.say_mode, "listening_sounds": p.listening_sounds, "key": self.key_view(),
            "fleet_voices": probed.voices, "fleet_here": probed.here, "fleet_chosen": fleet_chosen(&p, &probed.voices),
        })
    }

    fn probe(&self) {
        let url = self.prefs().node_url.unwrap_or_else(|| LOCAL_GATEWAY.into());
        let key = self.key();
        let end = std::time::Instant::now() + PROBE_WAIT;
        let got = self.backend.devices(&url, key.as_deref(), end);
        let mut fresh = match got {
            Ok(raw) => page_devices(&raw),
            Err(e) => Probed { unreachable: true, done: true, error: Some(e), ..Probed::default() },
        };
        fresh.here = on_this_mac(&url);

        if fresh.error.is_none() {
            match self.backend.voices(&url, key.as_deref(), end) {
                Ok(raw) => fresh.voices = page_voices(&raw),
                Err(e) => fresh.error = Some(e),
            }
        }

        let mut p = lock(self.probed);
        *p = Probed { key_error: p.key_error.take(), ..fresh };
    }

    pub fn take_probe_error(&self) -> Option<String> {
        lock(self.probed).error.take()
    }

    pub fn take_key_error(&self) -> Option<String> {
        lock(self.probed).key_error.take()
    }

    fn key(&self) -> Option<String> {
        let item = self.keys.read().unwrap_or_else(|e| {
            lock(self.probed).key_error = Some(e);
            None
        });
        item.or_else(|| self.env_key.clone())
    }

    fn key_view(&self) -> Value {
        let source = if self.keys.present() { json!("keychain") } else if self.env_key.is_some() { json!("env") } else { Value::Null };
        json!({"present": !source.is_null(), "source": source})
    }

    fn prefs_path(&self) -> PathBuf {
        self.root.join("prefs.json")
    }
    fn prefs(&self) -> Prefs {
        std::fs::read(self.prefs_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Prefs { say_mode: full(), ..Prefs::default() })
    }

    fn prefs_edit(&self, f: impl FnOnce(&mut Prefs)) -> Result<(), Refusal> {
        let path = self.prefs_path();
        if std::fs::read(&path).is_ok_and(|b| serde_json::from_slice::<Prefs>(&b).is_err()) {
            std::fs::rename(&path, path.with_extension("json.bad")).map_err(|e| Refusal::Bad(format!("prefs: {e}")))?;
        }
        let mut p = self.prefs();
        f(&mut p);
        let bytes = serde_json::to_vec_pretty(&p).map_err(|e| Refusal::Bad(e.to_string()))?;
        write_private(&self.prefs_path(), &bytes).map_err(|e| Refusal::Bad(format!("prefs: {e}")))
    }
    fn name(&self, value: &Value) -> Result<String, Refusal> {
        let n = value["name"].as_str().unwrap_or("").trim();
        if n.is_empty() || n.chars().count() > NAME_MAX {
            return Err(Refusal::Bad(format!("a voice name is 1 to {NAME_MAX} characters")));
        }
        Ok(n.into())
    }

    fn preview(&self, v: &VoiceEntry, mood: Mood, language: Language) -> Result<Value, Refusal> {
        let words = line(mood, language).unwrap_or_else(|| preview_line(language).into());
        let url = self.prefs().node_url.unwrap_or_else(|| LOCAL_GATEWAY.into());
        let client = Client::new(&url, self.key().map(ApiKey::new)).map_err(|e| Refusal::Bad(format!("node_url: {e}")))?;
        let steps = vec![speech_markup::Step::Speak { text: words, mood, requested: mood }];
        let total_steps = steps.len();
        let id = self.speaker.start(Job::Speak { client, steps, voice: v.clone() }, self.emit);
        Ok(json!({"voice_started": {"preview_id": id, "total_steps": total_steps}}))
    }

    fn ref_play(&self, v: &VoiceEntry, mood: Mood) -> Result<Value, Refusal> {
        let name = unfold_speech::catalog::mood_name(mood);
        let clip = v.moods.get(&mood).ok_or_else(|| Refusal::Bad(format!("clone_ref_play: {} has no {name} clip", v.id)))?;
        let w = rth_voice::wav::read(&v.clip_path(clip)).map_err(|_| Refusal::Bad(format!("clone_ref_play: {} {name} clip unreadable", v.id)))?;
        let id = self.speaker.start(Job::Clip { samples: w.to_i16_at(w.sample_rate), rate: w.sample_rate }, self.emit);
        Ok(json!({"voice_started": {"preview_id": id, "total_steps": 1}}))
    }

    fn fleet_voice(&self, value: &Value) -> Result<(Value, String), Refusal> {
        let id = text(value, "voice_id")?;
        let v = lock(self.probed).voices.iter().find(|v| v["id"] == id).cloned().ok_or_else(|| Refusal::Bad("fleet: that voice isn't listed".into()))?;
        let moods: Vec<&str> = v["moods"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let mood = match value["mood"].as_str() {
            Some(m) if moods.contains(&m) => m.to_string(),
            Some(_) => return Err(Refusal::Bad("fleet: that voice has no such mood".into())),
            None => v["default_mood"].as_str().or(moods.first().copied()).unwrap_or("neutral").to_string(),
        };
        Ok((v, mood))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn speak_reply(&self, reply: &str, first_only: bool, pt: bool, turns: &Mutex<()>, live: &dyn Fn() -> bool, emit: fn(&Value), more: Option<std::sync::mpsc::Receiver<String>>) -> Result<String, &'static str> {
        let p = self.prefs();
        if !p.on {
            return Err("voice off");
        }
        if p.say_mode == "off" {
            return Err("never speak");
        }
        let mut parts = if crate::conversation::speaks_full() { dialog_parts(reply) } else { spoken_parts(reply, &p.say_mode, reply_language(reply, pt) == "pt") };
        if first_only {
            parts.truncate(1);
        }
        if parts.is_empty() {
            return Err("nothing to say");
        }
        let stale = {
            let s = lock(self.probed);
            !s.done || s.unreachable || fleet_chosen(&p, &s.voices).is_null()
        };
        if stale {
            self.probe();
        }
        let voices = lock(self.probed).voices.clone();
        let chosen = fleet_chosen(&p, &voices);
        let v = voices.iter().find(|v| v["id"] == chosen["voice_id"]).ok_or("no voice")?;
        let mood = chosen["mood"].as_str().or_else(|| v["moods"][0].as_str()).unwrap_or("neutral");
        let body = json!({"model": FLEET_MODEL, "input": parts.join(" "), "voice": v["id"], "mood": mood, "stream": true, "stream_format": "events"});
        let url = p.node_url.unwrap_or_else(|| LOCAL_GATEWAY.into());
        let pitch = v["pitch_semitones"].as_f64().map(|p| p as f32);

        let key = self.key();
        let _turn = lock(turns);
        if !live() {
            return Err("new turn");
        }

        let now = self.prefs();
        if now.say_mode == "off" {
            return Err("never speak");
        }
        if !now.on {
            return Err("voice off");
        }

        self.speaker.start_reply(Job::Fleet {url,key,body,pitch,more,retries:BUSY_RETRIES},emit).map_err(|_|"new turn")
    }

    #[allow(dead_code)]
    pub fn speak_front(&self, request:crate::voice_front::runtime::SpeechRequest, epoch:crate::voice_front::runtime::Epoch,
        turns:&Mutex<()>,live:&dyn Fn()->bool,emit:fn(&Value)) -> Result<String,&'static str> {
        let p=self.prefs();if !p.on || p.say_mode=="off" {return Err("voice off");}
        if request.text.is_empty() {return Err("nothing to say");}
        let stale={let s=lock(self.probed);!s.done || s.unreachable || fleet_chosen(&p,&s.voices).is_null()};
        if stale {self.probe();}
        let voices=lock(self.probed).voices.clone();let chosen=fleet_chosen(&p,&voices);
        let v=voices.iter().find(|v|v["id"]==chosen["voice_id"]).ok_or("no voice")?;
        let mood=chosen["mood"].as_str().or_else(||v["moods"][0].as_str()).unwrap_or("neutral");
        let body=json!({"model":FLEET_MODEL,"input":request.text,"voice":v["id"],"mood":mood,"stream":true,"stream_format":"events"});
        let key=self.key();let _turn=lock(turns);if !live(){return Err("new turn");}
        let now=self.prefs();if !now.on || now.say_mode=="off" {return Err("voice off");}
        self.speaker.start_front_until(Job::Fleet {url:p.node_url.unwrap_or_else(||LOCAL_GATEWAY.into()),key,body,
            pitch:v["pitch_semitones"].as_f64().map(|p|p as f32),more:None,retries:0},epoch,request.ticket,request.deadline,emit).map_err(|_|"front unavailable")
    }

    fn clip_request(&self, text: &str) -> Option<(PathBuf, String, Value, Option<f32>)> {
        use sha2::{Digest, Sha256};
        let p = self.prefs();
        if !p.on || p.say_mode == "off" { return None; }
        let voices = lock(self.probed).voices.clone();
        let chosen = fleet_chosen(&p, &voices);
        let v = voices.iter().find(|v| v["id"] == chosen["voice_id"])?;
        let body = json!({"model": FLEET_MODEL, "input": text,
            "voice": v["id"], "mood": chosen["mood"], "stream": true, "stream_format": "events", "max_new_tokens": 96});
        let url = p.node_url.unwrap_or_else(|| LOCAL_GATEWAY.into());
        let identity = json!([1, url, body, v["pitch_semitones"]]);
        let hash = format!("{:x}", Sha256::digest(identity.to_string().as_bytes()));
        Some((self.root.join("ack-cache").join(format!("{hash}.pcm")), url, body, v["pitch_semitones"].as_f64().map(|s| s as f32)))
    }
    fn cached_clip(&self, text: &str) -> Option<Vec<i16>> {
        Self::read_clip(&self.clip_request(text)?.0)
    }
    fn read_clip(path: &Path) -> Option<Vec<i16>> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.len() == 0 || metadata.len() > 480_000 || metadata.len() % 2 != 0 { return None; }
        let bytes = std::fs::read(path).ok()?;
        Some(bytes.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]])).collect())
    }

    pub fn say_mode(&self) -> String {
        self.prefs().say_mode
    }

    pub fn on(&self) -> bool {
        self.prefs().on
    }

    pub fn reply_voice(&self, dictated: bool) -> Option<Value> {
        let p = self.prefs();
        if !p.on || p.say_mode == "off" || fleet_chosen(&p, &lock(self.probed).voices).is_null() {
            return None;
        }
        Some(json!({"dictated": dictated, "spoken": if p.say_mode == "full" { "full" } else { "first_sentence" }}))
    }

    fn warm_up_request(&self, pt: bool) -> Option<(String, Option<String>, Value)> {
        let p = self.prefs();
        if p.say_mode == "off" {
            return None;
        }
        let voices = lock(self.probed).voices.clone();
        let chosen = fleet_chosen(&p, &voices);
        let v = voices.iter().find(|v| v["id"] == chosen["voice_id"])?;
        let mood = v["default_mood"].as_str().or_else(|| v["moods"][0].as_str()).unwrap_or("neutral");
        let body = json!({"model": FLEET_MODEL, "input": if pt { "Oi." } else { "Hi." }, "voice": v["id"], "mood": mood, "max_new_tokens": WARM_TOKENS, "stream": false});
        Some((p.node_url.unwrap_or_else(|| LOCAL_GATEWAY.into()), self.key(), body))
    }

    pub fn warm(&self) {
        let stale = {
            let s = lock(self.probed);
            !s.done || s.unreachable || fleet_chosen(&self.prefs(), &s.voices).is_null()
        };
        let p = self.prefs();
        if stale && p.on && p.say_mode != "off" {
            self.probe();
        }
    }

    fn personal(&self, cat: &Catalog, value: &Value) -> Result<VoiceEntry, Refusal> {
        let v = cat.load(text(value, "voice_id")?).map_err(bad)?;
        if v.source != Source::Personal {
            return Err(Refusal::Bad(format!("{} is not one of the person's voices", v.id)));
        }
        Ok(v)
    }
}

fn fleet_chosen(p: &Prefs, listed: &[Value]) -> Value {
    let find = |id: &str| listed.iter().find(|v| v["id"] == id);
    if let Some(c) = &p.fleet {
        if let Some(v) = c["voice_id"].as_str().and_then(find) {
            let mood = c["mood"].as_str().filter(|m| v["moods"].as_array().is_some_and(|ms| ms.iter().any(|x| x == m)));
            return json!({"voice_id": v["id"], "mood": mood.map_or(v["default_mood"].clone(), |m| json!(m)), "default": false});
        }
    }
    find(FLEET_DEFAULT).map_or(Value::Null, |v| json!({"voice_id": v["id"], "mood": v["default_mood"], "default": true}))
}

const WARM_TOKENS: u32 = 13;

const WARM_AFTER: std::time::Duration = std::time::Duration::from_secs(300);

const WARM_JOIN: std::time::Duration = std::time::Duration::from_secs(10);

const JOIN_EVERY: std::time::Duration = std::time::Duration::from_millis(50);

const BUSY_RETRIES: u32 = 3;

const ON_SCREEN: [&str; 2] = ["It’s on screen.", "Deixei na tela."];

const SPOKEN_MAX: usize = 64 << 10;

const CUT_MARKERS: [&str; 2] = [" (cut short at the length limit; say or send \"continue\" to go on)", " (cortado no limite de tamanho; diga ou envie \"continue\" para seguir)"];

fn without_cut_marker(reply: &str) -> &str {
    let r = reply.trim_end();
    CUT_MARKERS.iter().find_map(|m| r.strip_suffix(m)).unwrap_or(reply)
}

fn without_stream_cut_marker(reply: &str) -> &str {
    let reply = without_cut_marker(reply);
    if let Some(i) = reply.rfind(" (") {
        if CUT_MARKERS.iter().any(|marker| marker.starts_with(&reply[i..])) { return &reply[..i]; }
    }
    reply
}

pub fn spoken(reply: &str, mode: &str, pt: bool) -> Option<String> {
    let parts = spoken_parts(reply, mode, pt);
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn spoken_parts(reply: &str, mode: &str, pt: bool) -> Vec<String> {
    if mode == "off" {
        return Vec::new();
    }
    let sentences = sentences(without_cut_marker(reply));
    let Some(first) = sentences.first() else { return Vec::new() };
    if on_screen(first) {
        return vec![ON_SCREEN[usize::from(pt)].into()];
    }
    let n = if mode == "full" { sentences.iter().take_while(|s| !on_screen(s)).count() } else { 1 };
    sentences[..n].iter().map(|s| plain(s)).filter(|s| !s.is_empty()).collect()
}

fn sentences(reply: &str) -> Vec<String> {
    speech_markup::parse(reply).sentences.iter().map(|s| s.text()).filter(|t| on_screen(t) || !plain(t).is_empty()).collect()
}

fn first_clause(text: &str) -> Option<usize> {
    let mut words = 0;
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if in_word { words += 1; in_word = false; }
            let before = text[..i].trim_end();
            if !before.is_empty() && (before.ends_with([',', ';', ':', '.', '!', '?', '—', '–']) || words >= 8) {
                return Some(before.len());
            }
        } else { in_word = true; }
    }
    None
}

#[cfg(test)]
fn after_first(reply: &str, mode: &str, pt: bool) -> Option<String> {
    let parts = spoken_parts(reply, mode, pt);
    (parts.len() > 1).then(|| parts[1..].join(" "))
}

fn on_screen(sentence: &str) -> bool {
    use speech_markup::say_gate::{content, Content, Trigger};
    matches!(content(sentence), Content::Visual(t) if t.iter().any(|t| matches!(t, Trigger::Code | Trigger::UrlOrPath | Trigger::Table)))
}

fn dialog_parts(reply: &str) -> Vec<String> { crate::conversation_text::parts(without_cut_marker(reply)) }
fn dialog_complete(reply: &str) -> Vec<String> { crate::conversation_text::complete(without_cut_marker(reply)) }

fn dialogue_parts(reply: &str) -> Vec<String> {
    let text = crate::conversation_text::clean(without_cut_marker(reply));
    if let Some(end) = first_clause(&text) {
        let mut parts = vec![text[..end].trim().to_string()];
        parts.extend(dialog_parts(text[end..].trim()));
        parts
    } else { dialog_parts(&text) }
}
fn dialogue_ready(reply: &str) -> Vec<String> {
    let text = crate::conversation_text::clean(without_stream_cut_marker(reply));
    if let Some(end) = first_clause(&text) {
        let mut parts = vec![text[..end].trim().to_string()];
        parts.extend(dialog_complete(text[end..].trim()));
        parts
    } else { dialog_complete(&text) }
}

fn plain(sentence: &str) -> String {
    let t = sentence.trim_start_matches(|c: char| matches!(c, '#' | '>' | '-' | '*' | '+') || c.is_whitespace());
    t.replace("**", "").replace("__", "").replace('*', "").trim().to_string()
}

pub fn reply_language(reply: &str, pt: bool) -> &'static str {
    const PT: [&str; 16] = ["não", "você", "que", "para", "uma", "um", "com", "é", "está", "isso", "como", "mais", "por", "seu", "sua", "também"];
    const EN: [&str; 16] = ["the", "you", "is", "and", "to", "of", "it", "that", "this", "with", "for", "are", "not", "can", "what", "your"];
    let mut fence: Option<&str> = None;
    let mut prose = String::new();
    for line in reply.lines() {
        let t = line.trim_start();
        match (fence, ["```", "~~~"].into_iter().find(|f| t.starts_with(f))) {
            (None, Some(f)) => fence = Some(f),
            (Some(open), Some(f)) if f == open => fence = None,
            (None, None) => {

                prose.extend(line.split('`').step_by(2));
                prose.push('\n');
            }
            _ => {}
        }
    }
    let prose = speech_markup::parse(&prose).display;
    let words: Vec<String> = prose.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let n = |list: &[&str]| words.iter().filter(|w| list.contains(&w.as_str())).count();
    match n(&PT).cmp(&n(&EN)) {
        std::cmp::Ordering::Greater => "pt",
        std::cmp::Ordering::Less => "en",
        std::cmp::Ordering::Equal if pt => "pt",
        std::cmp::Ordering::Equal => "en",
    }
}

pub struct ReplyText(Mutex<Option<Gathered>>, Mutex<Vec<String>>);

const ENDED_KEPT: usize = 16;
struct Gathered {
    text: String,

    response: Option<String>,

    full: bool,

    first: bool,

    whole_first: bool,
    stream_all: bool,
    sent: usize,
    opener: String,
}

#[derive(Debug, PartialEq)]
pub enum Gather {

    Chunk(Vec<String>),
    StreamDone(Vec<String>),

    First(String),

    Done(String),

    Dropped(&'static str),

    Restart(Option<String>),
}
impl ReplyText {
    pub const fn new() -> Self {
        ReplyText(Mutex::new(None), Mutex::new(Vec::new()))
    }

    pub fn turn(&self, gather: bool) -> bool { self.turn_stream(gather, false) }
    fn turn_stream(&self, gather: bool, stream_all: bool) -> bool {
        let was = std::mem::replace(&mut *lock(&self.0), gather.then(|| Gathered { text: String::new(), response: None, full: false, first: false, whole_first: false, stream_all, sent: 0, opener: String::new() }));
        if let Some(id) = was.as_ref().and_then(|g| g.response.clone()) {
            self.ended(id);
        }
        was.is_some()
    }
    fn ended(&self, id: String) {
        let mut e = lock(&self.1);
        e.push(id);
        let over = e.len().saturating_sub(ENDED_KEPT);
        e.drain(..over);
    }

    pub fn mine(&self, id: &str) -> bool {
        if lock(&self.1).iter().any(|e| e == id) {
            return false;
        }
        let mut g = lock(&self.0);
        let Some(r) = g.as_mut() else { return false };
        match &r.response {
            Some(mine) => mine == id,
            None => {
                r.response = Some(id.to_string());
                true
            }
        }
    }

    pub fn frame(&self, f: &Value) -> Option<Gather> {
        if f["wire"].is_string() || f["status"].is_string() {
            return None;
        }
        if let Some(id) = f["response_id"].as_str() {
            if !self.mine(id) {
                return None;
            }
        }
        let mut g = lock(&self.0);
        let r = g.as_mut()?;
        if let Some(t) = f["opener"].as_str().filter(|t| !t.trim().is_empty()) {
            if !r.opener.is_empty() || !r.text.is_empty() { return None; }
            r.opener = t.trim().to_string();
            return Some(Gather::Chunk(vec![r.opener.clone()]));
        }
        if let Some(t) = f["replace"].as_str() {
            let mut end = t.len().min(SPOKEN_MAX);
            while !t.is_char_boundary(end) {
                end -= 1;
            }
            r.full = end < t.len();
            r.text = t[..end].to_string();
            r.sent = 0;
            r.whole_first = false;
            r.first = if r.stream_all { !dialog_complete(&r.text).is_empty() } else { sentences(&r.text).len() > 1 };
            if !r.stream_all { r.first = first_clause(&r.text).is_some(); }
            return Some(Gather::Restart(r.first.then(|| r.text.clone())));
        }
        if let Some(t) = f["delta"].as_str() {
            r.full = r.full || r.text.len() + t.len() > SPOKEN_MAX;
            if !r.full {
                r.text.push_str(t);
            }
            if r.stream_all {
                let parts = dialogue_ready(&r.text);
                let next = parts.iter().skip(r.sent).cloned().collect::<Vec<_>>();
                r.sent = parts.len();
                if !next.is_empty() { r.first = true; return Some(Gather::Chunk(next)); }
                return None;
            }
            if r.first && !r.whole_first && sentences(&r.text).len() > 1 {
                r.whole_first = true;
                return Some(Gather::First(r.text.clone()));
            }
            if !r.first {
                if let Some(end) = first_clause(without_stream_cut_marker(&r.text)) {

                    let prefix = &r.text[..end];
                    if !on_screen(prefix) {
                        r.first = true;
                        return Some(Gather::First(prefix.to_string()));
                    }
                }
            }
        } else if f["done"] == true || !f["error"].is_null() {
            let r = g.take()?;
            drop(g);
            if let Some(id) = r.response {
                self.ended(id);
            }
            return Some(match (f["done"] == true, r.text.trim().is_empty()) {
                (false, _) => Gather::Dropped("no reply"),
                (true, true) => Gather::Dropped("nothing gathered"),
                (true, false) if r.stream_all => Gather::StreamDone(dialogue_parts(&r.text).into_iter().skip(r.sent).collect()),
                (true, false) => Gather::Done(r.text),
            });
        }
        None
    }
}

struct Pending {
    timing: TurnTiming,
    first_text: String,
    conversation: Option<u64>,
    voice: bool,
    dictated: bool,

    armed: bool,
    at: std::time::Instant,

    mode: String,

    first: bool,
    more: Option<std::sync::mpsc::Sender<String>>,

    speaking: bool,
    preview: Option<String>,
    asked_ms: Option<u128>,
    heard: bool,

    joined_at: Option<std::time::Instant>,
    waited_ms: Option<u128>,

    retries: u32,
}

#[derive(Default)]
struct TurnTiming {
    endpoint_ms: Option<u64>,
    floor_take_ms: Option<u64>,
    opener_ms: Option<u128>,
    first_text_ms: Option<u128>,
    first_reply_audio_ms: Option<u128>,
}
impl TurnTiming {
    fn line(&self) -> String {
        let ms = |n: Option<u128>| n.map_or_else(|| "na".into(), |n| n.to_string());
        format!("turn timing: endpoint_ms={}, floor_take_ms={}, commit->opener_ms={}, commit->first_text_ms={}, first_reply_audio_ms={}",
            ms(self.endpoint_ms.map(u128::from)), ms(self.floor_take_ms.map(u128::from)), ms(self.opener_ms), ms(self.first_text_ms), ms(self.first_reply_audio_ms))
    }
}

struct ListeningBank {
    level: Option<String>,
    clips: BTreeMap<(bool, usize), Vec<i16>>,
}
pub struct Replies {
    listening: Mutex<Option<ListeningBank>>,
    listening_generation: AtomicU64,
    timing: Mutex<Vec<String>>,
    text: ReplyText,

    turn: AtomicU64,

    turns: Mutex<()>,
    pending: Mutex<Option<Pending>>,

    spoke: Mutex<Option<std::time::Instant>>,

    warming: Mutex<bool>,
    warmed: std::sync::Condvar,

    stale: Mutex<Vec<String>>,
}

fn decision(p: &Pending, outcome: &str, mut notes: Vec<String>) -> String {
    let trigger = if p.dictated { "dictated" } else if p.armed { "reply_spoken" } else { "none" };
    if let Some(at) = p.joined_at {
        notes.push(format!("waited warm-up {} ms", p.waited_ms.unwrap_or_else(|| at.elapsed().as_millis())));
        notes.push(format!("retries {}", p.retries));
    }
    let notes = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join(", ")) };
    format!("voice: turn trigger {trigger}, voice chosen {} -> {outcome}{notes}", if p.voice { "yes" } else { "no" })
}

impl Replies {
    pub const fn new() -> Self {
        Replies { listening: Mutex::new(None), listening_generation: AtomicU64::new(0), timing: Mutex::new(Vec::new()), text: ReplyText::new(), turn: AtomicU64::new(0), turns: Mutex::new(()), pending: Mutex::new(None), spoke: Mutex::new(None), warming: Mutex::new(false), warmed: std::sync::Condvar::new(), stale: Mutex::new(Vec::new()) }
    }

    pub fn take_timing(&self) -> Vec<String> { std::mem::take(&mut *lock(&self.timing)) }

    fn render_clip(&self, v: &Voice, path: &Path, url: &str, body: &Value, shift: Option<f32>) {
        let key = v.key();
        for attempt in 0..=BUSY_RETRIES {
            if lock(&self.pending).is_some() || !v.on() { return; }
            let cancel = unfold_speech::Cancel::new();
            let mut samples = Vec::new();
            let result = crate::fleet::stream(url, key.as_deref(), body, &cancel, &mut |piece| {
                if lock(&self.pending).is_some() || samples.len() + piece.len() > 240_000 { cancel.cancel(); }
                else { samples.extend_from_slice(piece); }
            });
            *lock(&self.spoke) = Some(std::time::Instant::now());
            if let Err(crate::fleet::Failed::Busy(after)) = result {
                if attempt == BUSY_RETRIES { return; }
                let end = std::time::Instant::now() + crate::speaker::busy_wait(after);
                while std::time::Instant::now() < end {
                    if lock(&self.pending).is_some() || !v.on() { return; }
                    std::thread::sleep(JOIN_EVERY.min(end.saturating_duration_since(std::time::Instant::now())));
                }
                continue;
            }
            if result != Ok(false) || samples.is_empty() || cancel.is_cancelled() { return; }
            match crate::pitch::Pitch::new(shift, 24_000) {
                Ok(Some(mut pitch)) => { let mut shifted = pitch.push(&samples); shifted.extend(pitch.finish()); samples = shifted; }
                Ok(None) => {}
                Err(_) => return,
            }
            let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            let _ = write_private(path, &bytes);
            return;
        }
    }

    pub fn invalidate_listening(&self) {
        let _turn = lock(&self.turns);
        let mut bank = lock(&self.listening);
        self.listening_generation.fetch_add(1, Ordering::SeqCst);
        *bank = None;
    }
    pub fn listening_level(&self, pt: bool) -> crate::conversation_overlap::Level {
        use crate::conversation_overlap::Level;
        let bank = lock(&self.listening);
        let Some(bank) = bank.as_ref().filter(|b| (0..4).all(|n| b.clips.contains_key(&(pt, n)))) else { return Level::Off };
        Level::from_pref(bank.level.as_deref(), pt)
    }
    pub fn prepare_listening(&self, v: &Voice) {
        if !v.on() || v.speaker.busy() { return; }
        let generation = self.listening_generation.load(Ordering::SeqCst);
        {
            let warming = lock(&self.warming);
            let (warming, _) = self.warmed.wait_timeout_while(warming, WARM_JOIN, |busy| *busy).unwrap_or_else(|e| e.into_inner());
            if *warming { return; }
            drop(warming);
            let pending = lock(&self.pending);
            let mut warming = lock(&self.warming);
            if pending.is_some() || *warming { return; }
            *warming = true;
        }
        let mut clips = BTreeMap::new();
        for (pt, text) in [(false, crate::conversation_overlap::EN), (true, crate::conversation_overlap::PT)] {
            for (n, text) in text.iter().enumerate() {
                if generation != self.listening_generation.load(Ordering::SeqCst) || lock(&self.pending).is_some() || !v.on() { break; }
                if v.cached_clip(text).is_none() {
                    if let Some((path, url, body, shift)) = v.clip_request(text) { self.render_clip(v, &path, &url, &body, shift); }
                }
                if let Some(samples) = v.cached_clip(text) { clips.insert((pt, n), samples); }
            }
        }

        {
            let mut bank = lock(&self.listening);
            if generation == self.listening_generation.load(Ordering::SeqCst) && v.on() {
                *bank = Some(ListeningBank { level: v.prefs().listening_sounds, clips });
            }
        }
        *lock(&self.warming) = false;
        self.warmed.notify_all();
    }

    pub fn play_listening(&self, pt: bool, clip: usize, v: &Voice, live: impl Fn() -> bool) -> bool {
        let generation = self.listening_generation.load(Ordering::SeqCst);
        let samples = lock(&self.listening).as_ref().and_then(|b| b.clips.get(&(pt, clip))).cloned();
        let Some(samples) = samples else { return false };
        let _turn = lock(&self.turns);
        if self.listening_level(pt) == crate::conversation_overlap::Level::Off ||
            generation != self.listening_generation.load(Ordering::SeqCst) || !live() || v.speaker.busy() || lock(&self.pending).is_some() { return false; }
        let form = if pt { crate::conversation_overlap::PT[clip] } else { crate::conversation_overlap::EN[clip] };
        crate::conversation::listening_clip_started(form);
        v.speaker.start(Job::Clip { samples, rate: 24_000 }, crate::settings::listening_emit);
        true
    }

    pub fn turn(&self, dictated: bool, voice: bool, speaker: &Speaker) -> Vec<String> {
        let mut lines = self.cut("new turn", speaker);
        let conversation = crate::conversation::turn();
        let p = Pending { timing: TurnTiming { floor_take_ms: crate::conversation::take_floor_ms(), endpoint_ms: crate::conversation::take_endpoint_ms(), ..TurnTiming::default() }, first_text: String::new(), conversation, voice, dictated, armed: dictated || conversation.is_some(), at: std::time::Instant::now(), mode: String::new(), first: false, more: None, speaking: false, preview: None, asked_ms: None, heard: false, joined_at: None, waited_ms: None, retries: 0 };
        if dictated || voice {
            *lock(&self.pending) = Some(p);
            self.text.turn_stream(true, crate::conversation::speaks_full());
        } else {
            lock(&self.timing).push(p.timing.line());
            lines.push(decision(&p, "skipped(typed)", Vec::new()));
        }
        lines
    }

    pub fn dictation(&self, speaker: &Speaker) -> Vec<String> {
        self.cut("new dictation", speaker)
    }

    pub fn unsent(&self) -> Option<String> {
        self.text.turn(false);
        lock(&self.pending).take().map(|p| { lock(&self.timing).push(p.timing.line()); decision(&p, "skipped(not sent)", Vec::new()) })
    }

    pub fn front_terminal(&self) -> Option<String> {
        self.text.turn(false);
        lock(&self.pending).take().map(|p| {
            lock(&self.timing).push(p.timing.line());
            decision(&p, "front owner", Vec::new())
        })
    }

    fn cut(&self, why: &str, speaker: &Speaker) -> Vec<String> {
        let _turn = lock(&self.turns);
        self.turn.fetch_add(1, Ordering::SeqCst);

        self.warmed.notify_all();
        self.text.turn(false);

        let taken = lock(&self.pending).take();
        crate::doubletalk::reply(false);
        let line = taken.map(|mut p| {
            lock(&self.timing).push(p.timing.line());
            p.retries = p.preview.as_deref().map_or(0, |id| speaker.retries(id));
            if p.heard { decision(&p, "spoken", vec!["cut short".into()]) } else { decision(&p, &format!("skipped({why})"), Vec::new()) }
        });
        speaker.stop();
        line.into_iter().collect()
    }

    fn decide(&self, outcome: &str, first_audio_ms: Option<u64>) -> Option<String> {
        lock(&self.pending).take().map(|p| {
            lock(&self.timing).push(p.timing.line());
            if let Some(turn) = p.conversation { crate::conversation::speech_ended(turn); }
            let mut timing = Vec::new();
            if outcome == "spoken" {
                timing.extend(p.asked_ms.map(|a| format!("speech asked {a} ms after the turn")));
                timing.extend(first_audio_ms.map(|ms| format!("first audio {ms} ms after that")));
            }
            decision(&p, outcome, timing)
        })
    }

    pub fn frame(&'static self, f: &Value, pt: bool, voice: impl Fn() -> Voice<'static> + Send + 'static, emit: fn(&Value), log: impl Fn(String) + Send + 'static) {

        if f["wire"] == "reply_spoken" {
            if let Some(p) = lock(&self.pending).as_mut().filter(|p| !p.armed) {
                if f["response_id"].as_str().is_some_and(|id| self.text.mine(id)) {
                    p.armed = true;
                }
            }
            return;
        }
        if f["response_id"].as_str().is_none_or(|id| self.text.mine(id)) {
            if let Some(p) = lock(&self.pending).as_mut() {
                if f["opener"].as_str().is_some_and(|t| !t.trim().is_empty()) {
                    p.timing.opener_ms.get_or_insert(p.at.elapsed().as_millis());
                }
                if f["delta"].as_str().is_some_and(|t| !t.trim().is_empty()) {
                    p.timing.first_text_ms.get_or_insert(p.at.elapsed().as_millis());
                }
            }
        }
        let Some(mut g) = self.text.frame(f) else { return };
        if let Gather::Restart(first) = g {

            let stopped = {
                let _turn = lock(&self.turns);
                let mut pending = lock(&self.pending);
                match pending.as_mut().filter(|p| p.speaking) {
                    Some(p) => {
                        self.turn.fetch_add(1, Ordering::SeqCst);
                        self.stale_id(p.preview.take());
                        (p.first, p.speaking, p.more, p.asked_ms, p.heard) = (false, false, None, None, false);
                        true
                    }
                    None => false,
                }
            };
            if stopped {
                crate::doubletalk::reply(false);

                self.warmed.notify_all();
                voice().speaker.stop();
            }
            if let Some(p) = lock(&self.pending).as_mut() { p.first_text.clear(); }
            match first {
                Some(so_far) if crate::conversation::speaks_full() => {
                    let parts = dialogue_ready(&so_far);
                    if let Some(r) = lock(&self.text.0).as_mut() { r.sent = parts.len(); }
                    if parts.is_empty() { return; }
                    g = Gather::Chunk(parts);
                }
                Some(so_far) => {
                    let prefix = first_clause(&so_far).map(|end| so_far[..end].to_string()).unwrap_or(so_far);
                    g = Gather::First(prefix);
                }
                None => return,
            }
        }
        let (text, first_only, more) = {
            let mut pending = lock(&self.pending);
            let Some(p) = pending.as_mut() else { return };
            match g {
                Gather::StreamDone(parts) if p.first => {
                    if let Some(tx) = p.more.take() { for part in parts { let _ = tx.send(part); } }
                    return;
                }
                Gather::StreamDone(parts) if parts.is_empty() => {
                    drop(pending);
                    return self.decide("skipped(nothing gathered)", None).into_iter().for_each(log);
                }
                Gather::StreamDone(parts) => {
                    p.speaking = true;
                    (parts.join(" "), false, None)
                }
                Gather::Chunk(ref parts) if !p.armed || parts.is_empty() => return,
                Gather::Chunk(parts) if p.first => {
                    if let Some(tx) = p.more.as_ref() { for part in parts { let _ = tx.send(part); } }
                    return;
                }
                Gather::Chunk(mut parts) => {
                    p.first = true; p.speaking = true; p.mode = if crate::conversation::speaks_full() { "conversation".into() } else { voice().say_mode() };
                    let (tx, rx) = std::sync::mpsc::channel();
                    let first = parts.remove(0);
                    for part in parts { let _ = tx.send(part); }
                    p.more = Some(tx);
                    (first, true, Some(rx))
                }

                Gather::First(_) if !p.armed => return,
                Gather::Done(_) if !p.armed => {
                    drop(pending);
                    return self.decide("skipped(typed)", None).into_iter().for_each(log);
                }
                Gather::Dropped(why) => {
                    drop(pending);
                    return self.decide(&format!("skipped({why})"), None).into_iter().for_each(log);
                }
                Gather::First(so_far) if p.first => {
                    let pt = reply_language(&so_far, pt) == "pt";
                    let first = spoken(&so_far, "first_sentence", pt).unwrap_or_default();
                    let prefix = spoken(&p.first_text, &p.mode, pt).unwrap_or_default();
                    let rest = first.strip_prefix(&prefix).unwrap_or(&first).trim();
                    if let Some(tx) = p.more.as_ref() {
                        if !rest.is_empty() { let _ = tx.send(rest.to_string()); }
                    }
                    p.first_text = first;
                    return;
                }
                Gather::First(so_far) => {
                    p.first_text = so_far.clone();
                    p.first = true;
                    p.speaking = true;
                    p.mode = voice().say_mode();
                    let (tx, rx) = std::sync::mpsc::channel();
                    p.more = Some(tx);
                    (so_far, true, Some(rx))
                }
                Gather::Done(reply) if p.first => {
                    if let Some(tx) = p.more.take() {
                        let whole = spoken(&reply, &p.mode, reply_language(&reply, pt) == "pt").unwrap_or_default();
                        let prefix = spoken(&p.first_text, &p.mode, reply_language(&reply, pt) == "pt").unwrap_or_default();
                        let rest = whole.strip_prefix(&prefix).unwrap_or(&whole).trim();
                        if !rest.is_empty() { let _ = tx.send(rest.to_string()); }
                    }
                    p.more = None;
                    return;
                }
                Gather::Done(reply) => {
                    p.speaking = true;
                    (reply, false, None)
                }
                Gather::Restart(_) => return,
            }
        };
        if crate::conversation::speaks_full() { crate::conversation::speech_started(); }
        let turn = self.turn.load(Ordering::SeqCst);
        let _ = std::thread::Builder::new().name("reply-speech".into()).spawn(move || {
            let live = || self.turn.load(Ordering::SeqCst) == turn;
            let v = voice();
            if let Some(p) = lock(&self.pending).as_mut().filter(|_| live()) {
                p.joined_at = Some(std::time::Instant::now());
            }
            let joined = self.join_warm_up(&live, v.speaker);
            {
                let mut pending = lock(&self.pending);
                match (joined, pending.as_mut()) {
                    (Ok(ms), Some(p)) if live() => p.waited_ms = Some(ms),
                    (Err(why), Some(_)) if live() => {
                        drop(pending);
                        return self.decide(&format!("skipped({why})"), None).into_iter().for_each(log);
                    }
                    _ => return,
                }
            }
            let r = v.speak_reply(&text, first_only, pt, &self.turns, &live, emit, more);
            let mut pending = lock(&self.pending);
            match (r, pending.as_mut()) {
                (Ok(id), Some(p)) if live() => {
                    *lock(&self.spoke) = Some(std::time::Instant::now());
                    p.preview.get_or_insert(id);
                    p.asked_ms = Some(p.at.elapsed().as_millis());
                }
                (Err(why), Some(_)) if live() => {
                    drop(pending);
                    self.decide(&format!("skipped({why})"), None).into_iter().for_each(log);
                }
                (Ok(id), _) => {
                    drop(pending);
                    self.stale_id(Some(id));
                }
                _ => {}
            }
        });
    }

    fn stale_id(&self, id: Option<String>) {
        let mut s = lock(&self.stale);
        s.extend(id);
        let over = s.len().saturating_sub(ENDED_KEPT);
        s.drain(..over);
    }

    fn join_warm_up(&self, live: &dyn Fn() -> bool, speaker: &Speaker) -> Result<u128, &'static str> {
        let (t, stops) = (std::time::Instant::now(), speaker.stops());
        let mut warming = lock(&self.warming);
        if !*warming {
            return Ok(0);
        }
        while *warming && t.elapsed() < WARM_JOIN {
            if !live() {
                return Err("new turn");
            }
            if speaker.stops() != stops {
                return Err("stopped");
            }
            let wait = JOIN_EVERY.min(WARM_JOIN.saturating_sub(t.elapsed()));
            warming = self.warmed.wait_timeout(warming, wait).unwrap_or_else(|e| e.into_inner()).0;
        }
        Ok(t.elapsed().as_millis())
    }

    pub fn warm_up(&self, v: &Voice, pt: bool) -> String {
        let skipped = |why: &str| format!("voice: warm-up skipped({why})");
        if !v.on() {
            return skipped("voice off");
        }
        if v.speaker.busy() {
            return skipped("speech playing");
        }
        let Some((url, key, body)) = v.warm_up_request(pt) else { return skipped("no voice") };
        {

            let pending = lock(&self.pending);
            if pending.is_some() {
                return skipped("reply due");
            }
            let mut spoke = lock(&self.spoke);
            if spoke.is_some_and(|t| t.elapsed() < WARM_AFTER) {
                return skipped("spoke recently");
            }
            let mut warming = lock(&self.warming);
            if *warming { return skipped("warming"); }
            *spoke = Some(std::time::Instant::now());
            *warming = true;
        }
        let t = std::time::Instant::now();
        let warmed = crate::fleet::warm(&url, key.as_deref(), &body);
        *lock(&self.warming) = false;
        self.warmed.notify_all();
        match warmed {
            Ok(()) => format!("voice: warm-up sent -> ok {} ms", t.elapsed().as_millis()),
            Err(crate::fleet::Failed::Busy(_)) => "voice: warm-up sent -> failed(busy)".into(),
            Err(_) => "voice: warm-up sent -> failed(synthesis)".into(),
        }
    }

    pub fn event(&self, e: &Value) -> Option<String> {
        let id = e["preview_id"].as_str()?;
        if lock(&self.stale).iter().any(|s| s == id) {
            return None;
        }
        {
            let mut pending = lock(&self.pending);
            let p = pending.as_mut()?;
            match &p.preview {
                Some(mine) if mine != id => return None,
                Some(_) => {}
                None if p.speaking => p.preview = Some(id.to_string()),
                None => return None,
            }
            crate::floor_decide::playback(e,false);
            if e["state"]=="streaming" || e["state"]=="done" || e["state"]=="failed" {
                crate::doubletalk::reply(e["state"]=="streaming");
                crate::floor_decide::context().lock().unwrap().echo_until=Some(std::time::Instant::now()+std::time::Duration::from_millis(if e["state"]=="streaming" {5000}else{300}));
            }
            if e["state"] == "streaming" {
                let mut c=crate::floor_decide::context().lock().unwrap();
                c.sentence=e["floor_sentence"].as_str().unwrap_or("").into();c.played=e["floor_played"].as_str().unwrap_or("").into();
                p.heard = true;
                p.timing.first_reply_audio_ms.get_or_insert(p.at.elapsed().as_millis());
                return None;
            }
            p.retries = e["retries"].as_u64().map_or(0, |n| n as u32);
        }
        let outcome = match e["state"].as_str() {
            Some("failed") => format!("failed({})", e["error"]["code"].as_str().unwrap_or("unknown")),
            Some("done") if e["audio_seconds"].as_f64().is_some_and(|s| s > 0.0) => return self.decide("spoken", e["first_audio_ms"].as_u64()),
            Some("done") if e["stopped"] == true => "skipped(stopped)".into(),
            Some("done") => "failed(no_audio)".into(),
            _ => return None,
        };
        self.decide(&outcome, None)
    }
}

fn row(v: &VoiceEntry, p: &Prefs) -> Value {
    let moods: serde_json::Map<String, Value> = MOODS
        .iter()
        .filter_map(|m| v.moods.get(m).map(|c| (unfold_speech::catalog::mood_name(*m).to_string(), json!({"duration_s": c.duration_s, "transcript": c.transcript}))))
        .collect();
    let events: Vec<&str> = EVENTS.iter().filter(|e| v.events.contains_key(e)).map(|e| unfold_speech::catalog::event_name(*e)).collect();
    json!({
        "id": v.id, "name": v.name,
        "source": if v.source == Source::Personal { "personal" } else { "stock" },
        "language": if v.language == Language::PtBr { "pt-BR" } else { "en" },
        "enabled": p.enabled.get(&v.id).copied().unwrap_or(true),
        "moods": moods, "events": events,
    })
}

pub fn page_devices(raw: &Value) -> Probed {
    let mut devices = Vec::new();
    let mut best: Option<((usize, bool, bool), Value)> = None;
    let all = raw["devices"].as_array().cloned().unwrap_or_default();
    for d in all.iter().filter(|d| d["reachable"] == true) {
        let id = d["device"].as_str().unwrap_or("").to_string();
        let acc = &d["accelerator"];
        let apple = acc["vendor"].as_str().is_some_and(|v| v.eq_ignore_ascii_case("apple"));
        let entries = d["capabilities"][SPEECH].as_array().cloned().unwrap_or_default();
        let warm = entries.iter().any(|e| e["state"] == "warm");
        devices.push(json!({
            "id": id, "kind": if apple { "Mac" } else { "PC" }, "chip": acc["name"].as_str().unwrap_or("CPU"),
            "free_gb": d["memory_free_gib"], "warm": warm, "remote": d["local"] != true,
        }));
        let Some(model) = d["recommended"][SPEECH]["model"].as_str() else { continue };
        let Some(e) = entries.iter().find(|e| e["model"] == model) else { continue };
        let Some(tier) = TIERS.iter().position(|t| e["tier"] == *t) else { continue };
        let rank = (tier, e["state"] != "warm", d["local"] != true);
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, json!({"device_id": id, "tier": TIERS[tier], "models": [model]})));
        }
    }
    Probed { unreachable: devices.is_empty(), devices, recommended: best.map(|(_, r)| r), voices: Vec::new(), here: false, done: true, error: None, key_error: None }
}

pub fn page_voices(raw: &Value) -> Vec<Value> {
    let known = |m: &str| MOODS.iter().any(|x| unfold_speech::catalog::mood_name(*x) == m);
    let mut out = Vec::new();
    for v in raw["voices"].as_array().into_iter().flatten() {
        let Some(id) = v["voice_id"].as_str().filter(|i| (1..=64).contains(&i.len()) && i.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')) else { continue };
        let moods: Vec<&str> = MOODS.iter().map(|m| unfold_speech::catalog::mood_name(*m)).filter(|m| v["references"].get(*m).is_some()).collect();
        if moods.is_empty() {
            continue;
        }
        let pitch = match &v["pitch_semitones"] {
            Value::Null => None,
            p => match p.as_f64().filter(|p| p.is_finite() && (-12.0..=12.0).contains(p)) {
                Some(p) => Some(p),
                None => continue,
            },
        };
        let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && n.chars().count() <= 64).unwrap_or(id);
        let default_mood = v["default_mood"].as_str().filter(|m| known(m) && moods.contains(m));
        out.push(json!({"id": id, "name": name, "personal": v["personal"] == true, "moods": moods, "default_mood": default_mood, "pitch_semitones": pitch}));
    }
    out
}

fn text<'v>(value: &'v Value, key: &str) -> Result<&'v str, Refusal> {
    value[key].as_str().ok_or_else(|| Refusal::Bad(format!("{key} is required")))
}
fn mood(value: &Value) -> Result<Mood, Refusal> {
    let m = text(value, "mood")?;
    Mood::parse(m).ok_or_else(|| Refusal::Bad(format!("{m:?} is not a mood")))
}
fn bad(e: unfold_speech::CatalogError) -> Refusal {
    Refusal::Bad(e.to_string())
}

fn fresh_id(cat: &Catalog) -> Result<String, Refusal> {
    let mut n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    for _ in 0..1000 {
        let (mut s, mut x) = (String::new(), n);
        while x > 0 {
            s.insert(0, char::from_digit((x % 36) as u32, 36).unwrap());
            x /= 36;
        }
        let id = format!("v{s}");
        match cat.load(&id) {
            Err(unfold_speech::CatalogError::NotFound(_)) => return Ok(id),
            Ok(_) => n += 1,
            Err(e) => return Err(bad(e)),
        }
    }
    Err(Refusal::Bad("clone_create: no free voice id".into()))
}

pub fn window_closed(studio: &Studio, speaker: &Speaker) {
    studio.cancel();
    speaker.stop();
}

fn preview_line(language: Language) -> &'static str {
    if language == Language::PtBr { "Oi, esta é a minha voz." } else { "Hi, this is my voice." }
}

fn lines() -> &'static Value {
    static LINES: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    LINES.get_or_init(|| serde_json::from_str(CLONE_LINES).unwrap_or(Value::Null))
}

fn line(mood: Mood, language: Language) -> Option<String> {
    let lang = if language == Language::PtBr { "pt-BR" } else { "en" };
    lines()["lines"][unfold_speech::catalog::mood_name(mood)][lang].as_str().map(String::from).filter(|l| !l.trim().is_empty())
}

fn meter(b: &[f32], rate: u32) -> Value {
    let last = &b[b.len().saturating_sub(rate as usize / 4)..];
    let rms = (last.iter().map(|x| x * x).sum::<f32>() / last.len().max(1) as f32).sqrt();
    json!({"seconds": b.len() as f64 / rate as f64, "level": (rms * 4.0).min(1.0), "clipped": last.iter().any(|x| x.abs() >= 0.99)})
}

fn wav16(samples: &[f32], rate: u32) -> Vec<u8> {
    let n = samples.len() as u32 * 2;
    let mut b = Vec::with_capacity(44 + n as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&n.to_le_bytes());
    for x in samples {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

struct Removed(PathBuf);
impl Drop for Removed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    std::io::Write::write_all(&mut f, bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Result<Value, String>);
    impl Backend for Fake {
        fn devices(&self, base_url: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            assert_eq!(base_url, LOCAL_GATEWAY, "no private host: the loopback gateway unless the prefs say otherwise");
            self.0.clone()
        }
        fn voices(&self, base_url: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            assert_eq!(base_url, LOCAL_GATEWAY);
            self.0.clone().map(|raw| json!({"voices": raw["voices"]}))
        }
    }

    struct Answers(Result<Value, String>);
    impl Backend for Answers {
        fn devices(&self, _: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            self.0.clone()
        }
        fn voices(&self, _: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            self.0.clone().map(|raw| json!({"voices": raw["voices"]}))
        }
    }
    struct Slow(Duration, Value);
    impl Backend for Slow {
        fn devices(&self, _: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            std::thread::sleep(self.0);
            Ok(self.1.clone())
        }
        fn voices(&self, _: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            Ok(json!({"voices": self.1["voices"]}))
        }
    }
    fn scratch() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("unfold-voice-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn wav(secs: f64) -> Vec<u8> {
        let n = (24_000.0 * secs) as u32 * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24_000u32.to_le_bytes());
        b.extend_from_slice(&48_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&n.to_le_bytes());
        b.resize(b.len() + n as usize, 0);
        b
    }

    fn fleet() -> Value {
        json!({"devices": [
            {"device": "mac", "local": true, "reachable": true, "accelerator": {"vendor": "apple", "name": "Apple M5 Pro", "memory_kind": "unified"},
             "memory_free_gib": 21.4, "capabilities": {"speech_synthesis": [
                {"model": "qwen3-tts-0.6b-base", "state": "warm", "tier": "gpu_poor"},
                {"model": "qwen3-tts-1.7b-base", "state": "cold_fits", "tier": "good_gpu"}]},
             "recommended": {"speech_synthesis": {"model": "qwen3-tts-1.7b-base", "why": "best tier that fits, warm first"}}},
            {"device": "pc", "local": false, "reachable": false}]})
    }

    struct Gone(PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn with(root: &Path, backend: &dyn Backend, f: impl FnOnce(&Voice)) {
        with_mic(root, backend, &FakeMic::granted(0.0), f)
    }
    fn with_mic(root: &Path, backend: &dyn Backend, mic: &dyn Mic, f: impl FnOnce(&Voice)) {
        let _gone = Gone(root.into());
        let probed = Mutex::new(Probed::default());
        let studio: &'static Studio = Box::leak(Box::default());
        f(&Voice { root: root.into(), backend, env_key: None, keys: &NoKeys, probed: &probed, mic, studio, speaker: quiet(), emit: |_| {} });
    }

    struct FakeMic {
        state: MicState,
        secs: f64,
        asked: std::sync::atomic::AtomicUsize,
        open: Arc<std::sync::atomic::AtomicUsize>,
    }
    struct Stream(Arc<std::sync::atomic::AtomicUsize>);
    impl Drop for Stream {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl FakeMic {
        fn granted(secs: f64) -> Self {
            FakeMic { state: MicState::Granted, secs, asked: Default::default(), open: Default::default() }
        }
        fn open_now(&self) -> usize {
            self.open.load(Ordering::SeqCst)
        }
    }
    impl Mic for FakeMic {
        fn state(&self) -> MicState {
            self.state
        }
        fn ask(&self) {
            self.asked.fetch_add(1, Ordering::SeqCst);
        }
        fn open(&self, mut sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any + Send>, u32), String> {
            let n = (48_000.0 * self.secs) as usize;
            sink((0..n).map(|i| (i as f32 * 0.05).sin() * 0.2).collect());
            self.open.fetch_add(1, Ordering::SeqCst);
            Ok((Box::new(Stream(Arc::clone(&self.open))), 48_000))
        }
    }
    fn mine(v: &Voice) -> String {
        v.act("clone_create", &json!({"name": "Studio voice", "language": "pt-BR"})).unwrap()["voice_created"]["voice_id"].as_str().unwrap().to_string()
    }
    fn stock(root: &Path, id: &str, lang: Language) {
        let mut v = VoiceEntry::new(id, id, lang, Source::Stock, "qwen3-tts-1.7b-base").unwrap();
        Catalog::new(root).save(&mut v).unwrap();
    }

    #[test]
    fn probe_maps_localfleet_devices_into_the_page_shape() {
        let root = scratch();
        stock(&root, "aurora", Language::PtBr);
        with(&root, &Fake(Ok(fleet())), |v| {
            let r = v.act("voice_probe", &json!({})).unwrap();
            let p = &r["voice"];
            assert_eq!(p["devices"], json!([{"id": "mac", "kind": "Mac", "chip": "Apple M5 Pro", "free_gb": 21.4, "warm": true, "remote": false}]), "the unreachable peer is not listed");
            assert_eq!(p["recommended"], json!({"device_id": "mac", "tier": "good_gpu", "models": ["qwen3-tts-1.7b-base"]}));
            assert_eq!(p["unreachable"], json!(false));
            assert_eq!(p["voices"][0], json!({"id": "aurora", "name": "aurora", "source": "stock", "language": "pt-BR", "enabled": true, "moods": {}, "events": []}));
            assert_eq!((p["chosen"].clone(), p["say_mode"].clone()), (Value::Null, json!("full")));
            assert_eq!(p["lines"], lines()["lines"], "clone_lines_v1's lines");
            assert!(p["lines"]["neutral"]["pt-BR"].as_str().is_some_and(|l| !l.is_empty()));
        });
    }

    #[test]
    fn every_reply_carries_the_voice_view_once_a_probe_has_run() {

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let root = scratch();
            with(&root, &Fake(Ok(fleet())), |v| {
                assert_eq!(v.shown(), None, "before the first probe the page asks for one");
                v.act("say_set", &json!({"mode": "off"})).unwrap();
                assert_eq!(v.shown(), None);
                v.act("voice_probe", &json!({})).unwrap();
                assert_eq!(v.shown().unwrap()["say_mode"], json!("off"));
            });
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(10)).expect("shown() returns, with every check passed");
    }

    #[test]
    fn a_failed_probe_is_unreachable_and_a_new_voice_then_has_no_model() {
        let root = scratch();
        with(&root, &Fake(Err("connection refused".into())), |v| {
            assert_eq!(v.act("voice_probe", &json!({})).unwrap()["voice"]["unreachable"], json!(true));
            assert_eq!((v.take_probe_error(), v.take_probe_error()), (Some("connection refused".into()), None), "the reason, once, for the log");
            assert_eq!(v.act("clone_create", &json!({"name": "Studio voice", "language": "en"})), Err(Refusal::NoDevice));
        });
    }

    #[test]
    fn no_speech_tier_that_fits_means_no_recommendation() {
        let mut f = fleet();
        f["devices"][0]["recommended"] = json!({"speech_synthesis": {"model": null, "why": "no tier fits on this device now"}});
        let p = page_devices(&f);
        assert_eq!((p.devices.len(), p.recommended, p.unreachable), (1, None, false));
    }

    #[test]
    fn the_file_picks_are_refused_in_this_build() {
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            for a in LATER {
                assert_eq!(v.act(a, &json!({"voice_id": "x", "mood": "neutral"})), Err(Refusal::Later(a.to_string())), "{a}");
            }
            for a in ["voice_stop", "clone_record_cancel"] {
                assert!(v.act(a, &json!({})).is_ok(), "{a}: nothing plays or records, so stopping is ok");
            }
            for a in ["voice_preview", "clone_preview", "clone_ref_play"] {
                assert!(!LATER.contains(&a), "{a} plays in this build");
            }
        });
    }

    #[test]
    fn prefs_say_mode_choose_and_enable() {
        let root = scratch();
        stock(&root, "cora", Language::En);
        with(&root, &Fake(Ok(fleet())), |v| {
            assert!(matches!(v.act("say_set", &json!({"mode": "loud"})), Err(Refusal::Bad(_))));
            assert_eq!(v.act("say_set", &json!({"mode": "first_sentence"})).unwrap()["voice"]["say_mode"], json!("first_sentence"));
            let r = v.act("voice_choose", &json!({"voice_id": "cora", "tier": "good_gpu", "device_id": "mac"})).unwrap();
            assert_eq!(r["voice"]["chosen"], json!({"voice_id": "cora", "tier": "good_gpu", "device_id": "mac"}));
            assert!(matches!(v.act("voice_enable", &json!({"voice_id": "cora", "on": false})), Err(Refusal::Bad(_))), "the chosen voice stays on");
            assert!(matches!(v.act("voice_choose", &json!({"voice_id": "nobody"})), Err(Refusal::Bad(_))));
            let mode = std::fs::metadata(root.join("prefs.json")).unwrap().permissions();
            assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        });
    }

    #[test]
    fn a_personal_voice_is_created_renamed_corrected_cleared_and_deleted() {
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            let r = v.act("clone_create", &json!({"name": " Studio voice ", "language": "pt-BR"})).unwrap();
            let id = r["voice_created"]["voice_id"].as_str().unwrap().to_string();
            assert_eq!(r["voice"]["voices"][0]["name"], json!("Studio voice"));
            let cat = Catalog::new(&root);
            let mut e = cat.load(&id).unwrap();
            assert_eq!((e.source, e.model.as_str()), (Source::Personal, "qwen3-tts-1.7b-base"), "cloned with the recommended model");
            assert!(matches!(v.act("voice_choose", &json!({"voice_id": id})), Err(Refusal::Bad(_))), "no Neutral yet");
            cat.set_ref(&mut e, Mood::Neutral, &wav(6.4), "Estou lendo esta frase.").unwrap();
            assert!(matches!(v.act("clone_rename", &json!({"voice_id": id, "name": "  "})), Err(Refusal::Bad(_))), "an empty name is refused");
            v.act("clone_rename", &json!({"voice_id": id, "name": "Minha voz"})).unwrap();
            assert_eq!(v.act("clone_ref_text", &json!({"voice_id": id, "mood": "neutral", "text": "   "})), Err(Refusal::TranscriptEmpty));
            let r = v.act("clone_ref_text", &json!({"voice_id": id, "mood": "neutral", "text": " Estou lendo esta linha. "})).unwrap();
            let row = &r["voice"]["voices"][0];
            assert_eq!((row["name"].clone(), row["moods"]["neutral"]["transcript"].clone()), (json!("Minha voz"), json!("Estou lendo esta linha.")));
            assert_eq!(row["moods"]["neutral"]["duration_s"].as_f64().map(|s| (s * 10.0).round()), Some(64.0));
            v.act("voice_choose", &json!({"voice_id": id, "tier": "good_gpu", "device_id": "mac"})).unwrap();
            v.act("clone_ref_clear", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            assert!(cat.load(&id).unwrap().moods.is_empty());
            let r = v.act("clone_delete", &json!({"voice_id": id})).unwrap();
            assert_eq!((r["voice"]["voices"].as_array().unwrap().len(), r["voice"]["chosen"].clone()), (0, Value::Null), "deleting the chosen voice clears the choice");
            assert!(!root.join("voices").join(&id).exists());
        });
    }

    #[test]
    fn stock_voices_are_never_edited_or_deleted() {
        let root = scratch();
        stock(&root, "bento", Language::PtBr);
        with(&root, &Fake(Ok(fleet())), |v| {
            for a in ["clone_rename", "clone_delete", "clone_ref_clear", "clone_ref_text"] {
                assert!(matches!(v.act(a, &json!({"voice_id": "bento", "name": "X", "mood": "neutral", "text": "x"})), Err(Refusal::Bad(_))), "{a}");
            }
            assert!(root.join("voices/bento/voice.json").exists());
        });
    }

    fn soon() -> std::time::Instant {
        std::time::Instant::now() + PROBE_WAIT
    }
    fn within(secs: u64, f: impl FnOnce() + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            f();
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(secs)).expect("finished in time, with every check passed");
    }

    #[test]
    fn a_catalogue_that_cant_be_read_refuses_a_new_voice_at_once() {
        within(10, || {
            let root = scratch();
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("voices"), b"not a directory").unwrap();
            with(&root, &Fake(Ok(fleet())), |v| {
                assert!(matches!(v.act("clone_create", &json!({"name": "Studio voice", "language": "en"})), Err(Refusal::Bad(_))));
            });
        });
    }

    #[test]
    fn a_corrupt_prefs_file_is_kept_aside_not_lost() {
        let root = scratch();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("prefs.json"), b"{not json").unwrap();
        with(&root, &Fake(Ok(fleet())), |v| {
            v.act("say_set", &json!({"mode": "off"})).unwrap();
            assert_eq!(std::fs::read(root.join("prefs.json.bad")).unwrap(), b"{not json");
            assert_eq!(v.act("voice_probe", &json!({})).unwrap()["voice"]["say_mode"], json!("off"));
        });
    }

    #[test]
    fn page_values_are_checked_before_they_are_stored() {
        let root = scratch();
        stock(&root, "cora", Language::En);
        with(&root, &Fake(Ok(fleet())), |v| {
            assert!(matches!(v.act("voice_choose", &json!({"voice_id": "cora", "tier": {"x": 1}, "device_id": "mac"})), Err(Refusal::Bad(_))), "a tier is one of TIERS");
            assert!(matches!(v.act("voice_choose", &json!({"voice_id": "cora", "tier": "cpu", "device_id": "m".repeat(65)})), Err(Refusal::Bad(_))), "a device id is short");
            let r = v.act("clone_create", &json!({"name": "Studio voice", "language": "en"})).unwrap();
            let id = r["voice_created"]["voice_id"].as_str().unwrap().to_string();
            let cat = Catalog::new(&root);
            let mut e = cat.load(&id).unwrap();
            cat.set_ref(&mut e, Mood::Neutral, &wav(6.0), "I am reading this line.").unwrap();
            let long = "word ".repeat(300);
            assert!(matches!(v.act("clone_ref_text", &json!({"voice_id": id, "mood": "neutral", "text": long})), Err(Refusal::Bad(_))), "the words fit one request");
        });
    }

    fn serve(reply: impl FnOnce(&mut std::net::TcpStream) + Send + 'static) -> (String, std::thread::JoinHandle<String>) {
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut req = Vec::new();
            let mut b = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut b).unwrap();
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&b[..n]);
            }
            reply(&mut s);
            String::from_utf8_lossy(&req).into_owned()
        });
        (url, h)
    }
    fn answer(s: &mut std::net::TcpStream, head: &str, body: &[u8]) {
        use std::io::Write;
        let _ = write!(s, "{head}\r\nContent-Length: {}\r\n\r\n", body.len());
        let _ = s.write_all(body);
    }

    #[test]
    fn fleet_reads_a_200_and_sends_the_key_only_when_there_is_one() {
        within(20, || {
            let body = serde_json::to_vec(&fleet()).unwrap();
            let (url, h) = serve(move |s| answer(s, "HTTP/1.1 200 OK", &body));
            assert_eq!(Fleet.devices(&url, Some("k-test"), soon()).unwrap(), fleet());
            let req = h.join().unwrap();
            assert!(req.starts_with("GET /v1/devices HTTP/1.1\r\n") && req.contains("\r\nAuthorization: Bearer k-test\r\n") && req.contains("X-LocalFleet-Client: unfold"), "{req}");
            let (url, h) = serve(|s| answer(s, "HTTP/1.1 200 OK", b"{\"devices\": []}"));
            assert_eq!(Fleet.devices(&url, None, soon()).unwrap(), json!({"devices": []}));
            assert!(!h.join().unwrap().contains("Authorization"), "no key, no header");
        });
    }

    #[test]
    fn fleet_refuses_other_statuses_chunked_and_oversized_replies_without_the_key() {
        within(30, || {
            let (url, _h) = serve(|s| answer(s, "HTTP/1.1 401 Unauthorized", b"{}"));
            let e = Fleet.devices(&url, Some("k-secret"), soon()).unwrap_err();
            assert!(e.contains("401") && !e.contains("k-secret"), "{e}");
            let (url, _h) = serve(|s| {
                use std::io::Write;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n");
            });
            assert!(Fleet.devices(&url, None, soon()).unwrap_err().contains("chunked"));
            let (url, _h) = serve(|s| answer(s, "HTTP/1.1 200 OK", &vec![b' '; (1 << 20) + 10]));
            assert!(Fleet.devices(&url, None, soon()).is_err(), "over 1 MiB");
        });
    }

    #[test]
    fn fleet_gives_up_on_a_reply_that_trickles_past_its_deadline() {
        within(20, || {
            let (url, _h) = serve(|s| {
                use std::io::Write;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n");
                for _ in 0..100 {
                    if s.write_all(b" ").is_err() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });
            let t = std::time::Instant::now();
            assert!(Fleet.devices(&url, None, soon()).is_err());
            assert!(t.elapsed() < std::time::Duration::from_secs(12), "{:?}", t.elapsed());
        });
    }

    #[test]
    fn the_read_along_lines_are_pinned_and_cover_every_mood_in_both_languages() {
        use sha2::Digest;
        let sha: String = sha2::Sha256::digest(CLONE_LINES.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(sha, CLONE_LINES_SHA256, "clone_lines_v1.json changed: move the pin with it");
        for m in MOODS {
            for l in [Language::PtBr, Language::En] {
                assert!(line(m, l).is_some_and(|t| t.split_whitespace().count() >= 10), "{m:?} {l:?}");
            }
        }
    }

    #[test]
    fn a_kept_take_is_saved_at_stop_with_its_line_and_the_mic_closes() {
        let root = scratch();
        let mic = FakeMic::granted(6.4);
        with_mic(&root, &Fake(Ok(fleet())), &mic, |v| {
            let id = mine(v);
            v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            assert_eq!(mic.open_now(), 1, "the mic is open while the take records");
            assert_eq!(v.act("clone_record_start", &json!({"voice_id": id, "mood": "joy"})), Err(Refusal::MicBusy), "one take at a time");
            let r = v.act("clone_record_stop", &json!({})).unwrap();
            let read = line(Mood::Neutral, Language::PtBr).unwrap();
            assert_eq!(r["voice_take"], json!({"seconds": 6.4, "status": "ok", "text": read}), "the pt-BR voice's Neutral line is the take's words");
            assert_eq!(mic.open_now(), 0, "Stop closes the mic");
            assert_eq!(r["voice"]["voices"][0]["moods"]["neutral"], json!({"duration_s": 6.4, "transcript": read}));
            assert_eq!(r["voice"]["lines"]["neutral"]["pt-BR"], json!(read), "the page reads the same line");
            let cat = Catalog::new(&root);
            assert_eq!(v.act("clone_ref_text", &json!({"voice_id": id, "mood": "neutral", "text": "  "})), Err(Refusal::TranscriptEmpty));
            v.act("clone_ref_text", &json!({"voice_id": id, "mood": "neutral", "text": "Estou lendo esta frase."})).unwrap();
            let e = cat.load(&id).unwrap();
            assert_eq!((e.moods[&Mood::Neutral].transcript.as_str(), e.moods[&Mood::Neutral].duration_s), ("Estou lendo esta frase.", 6.4), "a misread word is corrected");
            assert!(v.act("voice_choose", &json!({"voice_id": id, "tier": "good_gpu", "device_id": "mac"})).is_ok(), "Neutral now exists");
        });
    }

    #[test]
    fn a_take_too_short_or_too_long_keeps_nothing() {
        for (secs, status) in [(3.0, "too_short"), (12.0, "too_long")] {
            let root = scratch();
            let mic = FakeMic::granted(secs);
            with_mic(&root, &Fake(Ok(fleet())), &mic, |v| {
                let id = mine(v);
                v.act("clone_record_start", &json!({"voice_id": id, "mood": "joy"})).unwrap();
                let r = v.act("clone_record_stop", &json!({})).unwrap();
                assert_eq!((r["voice_take"]["status"].clone(), r["voice"]["voices"][0]["moods"].clone()), (json!(status), json!({})));
                assert_eq!(mic.open_now(), 0);
            });
        }
    }

    #[test]
    fn cancel_closes_the_mic_and_keeps_nothing_and_is_safe_when_idle() {
        let root = scratch();
        let mic = FakeMic::granted(6.0);
        with_mic(&root, &Fake(Ok(fleet())), &mic, |v| {
            let id = mine(v);
            assert!(v.act("clone_record_cancel", &json!({})).is_ok(), "idle: a no-op");
            v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            v.act("clone_record_cancel", &json!({})).unwrap();
            assert_eq!(mic.open_now(), 0);
            assert!(matches!(v.act("clone_record_stop", &json!({})), Err(Refusal::Bad(_))), "nothing is recording");
            assert_eq!(v.act("voice_probe", &json!({})).unwrap()["voice"]["voices"][0]["moods"], json!({}));
            v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            v.studio.cancel();
            assert_eq!(mic.open_now(), 0, "the window's close closes the mic too");
        });
    }

    #[test]
    fn the_mic_asks_first_and_a_denied_mic_refuses() {
        let root = scratch();
        let ask = FakeMic { state: MicState::NotAsked, ..FakeMic::granted(6.0) };
        with_mic(&root, &Fake(Ok(fleet())), &ask, |v| {
            let id = mine(v);
            assert_eq!(v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})), Err(Refusal::MicAsk));
            assert_eq!((ask.asked.load(Ordering::SeqCst), ask.open_now()), (1, 0), "asked once, never opened");
        });
        let root = scratch();
        let denied = FakeMic { state: MicState::Denied, ..FakeMic::granted(6.0) };
        with_mic(&root, &Fake(Ok(fleet())), &denied, |v| {
            let id = mine(v);
            assert_eq!(v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})), Err(Refusal::MicDenied));
        });
    }

    static PUSHED: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    #[test]
    fn a_take_pushes_its_level_until_it_stops() {
        within(10, || {
            let root = scratch();
            let mic = FakeMic::granted(6.0);
            let _gone = Gone(root.clone());
            let probed = Mutex::new(Probed::default());
            let studio: &'static Studio = Box::leak(Box::default());
            let v = Voice { root: root.clone(), backend: &Fake(Ok(fleet())), env_key: None, keys: &NoKeys, probed: &probed, mic: &mic, studio, speaker: quiet(), emit: |e| lock(&PUSHED).push(e.clone()) };
            let id = mine(&v);
            v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            let t = std::time::Instant::now();
            while lock(&PUSHED).is_empty() && t.elapsed() < std::time::Duration::from_secs(3) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let first = lock(&PUSHED).first().cloned().expect("a level push within 3 s");
            assert_eq!(first["rec"]["seconds"], json!(6.0));
            assert!(first["rec"]["level"].as_f64().is_some_and(|l| l > 0.0 && l <= 1.0) && first["rec"]["clipped"] == false, "{first}");
            v.act("clone_record_stop", &json!({})).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(600));
            let n = lock(&PUSHED).len();
            std::thread::sleep(std::time::Duration::from_millis(600));
            assert_eq!(lock(&PUSHED).len(), n, "no push after Stop");
        });
    }

    fn folder(root: &Path, id: &str) -> Value {
        use base64::Engine;
        let dir = root.join("voices").join(id);
        let files: Vec<Value> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).map(|p| json!({"name": p.file_name().unwrap().to_str().unwrap(), "bytes_b64": base64::engine::general_purpose::STANDARD.encode(std::fs::read(&p).unwrap())})).collect();
        json!({"files": files})
    }
    fn staged(root: &Path) -> Value {
        let cat = Catalog::new(root);
        let mut v = VoiceEntry::new("lumen", "Lumen", Language::PtBr, Source::Stock, "qwen3-tts-1.7b-base").unwrap();
        v.temperature = Some(1.0);
        cat.save(&mut v).unwrap();
        cat.set_ref(&mut v, Mood::Neutral, &wav(6.0), "Estou lendo esta frase.").unwrap();
        cat.set_ref(&mut v, Mood::Joy, &wav(5.5), "Que dia bonito!").unwrap();
        cat.set_event_clip(&mut v, Event::Laugh, &wav(1.2)).unwrap();
        folder(root, "lumen")
    }

    #[test]
    fn an_imported_folder_becomes_the_persons_own_voice_through_the_catalogue_checks() {
        let src = scratch();
        let _src = Gone(src.clone());
        let files = staged(&src);
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            let r = v.act("voice_import", &files).unwrap();
            assert_eq!(r["voice_created"], json!({"voice_id": "lumen"}));
            let e = Catalog::new(&root).load("lumen").unwrap();
            assert_eq!((e.source, e.language, e.model.as_str(), e.temperature), (Source::Personal, Language::PtBr, "qwen3-tts-1.7b-base", Some(1.0)));
            assert_eq!((e.moods[&Mood::Neutral].transcript.as_str(), e.moods[&Mood::Joy].duration_s, e.events.contains_key(&Event::Laugh)), ("Estou lendo esta frase.", 5.5, true));
            assert_eq!(r["voice"]["voices"][0]["enabled"], json!(true));
            assert!(std::fs::read_dir(&root).unwrap().all(|d| !d.unwrap().file_name().to_string_lossy().starts_with(".import")), "no staging left");
            assert_eq!(v.act("voice_import", &files), Err(Refusal::VoiceExists), "the same id twice");
        });
    }

    #[test]
    fn an_import_that_isnt_a_whole_valid_voice_is_refused_and_leaves_nothing() {
        let src = scratch();
        let _src = Gone(src.clone());
        let files = staged(&src);
        let edit = |f: &dyn Fn(&mut Vec<Value>)| {
            let mut x = files["files"].as_array().unwrap().clone();
            f(&mut x);
            json!({"files": x})
        };
        let cases = [
            ("no voice.json", edit(&|x| x.retain(|f| f["name"] != "voice.json"))),
            ("a file voice.json doesn't name", edit(&|x| x.push(json!({"name": "extra.wav", "bytes_b64": "UklGRg=="})))),
            ("a path, not a name", edit(&|x| x.push(json!({"name": "../x.wav", "bytes_b64": "UklGRg=="})))),
            ("a changed clip (its sha256)", edit(&|x| {
                use base64::Engine;
                let f = x.iter_mut().find(|f| f["name"] != "voice.json").unwrap();
                let mut b = base64::engine::general_purpose::STANDARD.decode(f["bytes_b64"].as_str().unwrap()).unwrap();
                let last = b.len() - 1;
                b[last] ^= 1;
                f["bytes_b64"] = json!(base64::engine::general_purpose::STANDARD.encode(b));
            })),
            ("not base64", edit(&|x| x[0]["bytes_b64"] = json!("@@"))),
            ("no files", json!({"files": []})),
        ];
        for (why, files) in cases {
            let root = scratch();
            with(&root, &Fake(Ok(fleet())), |v| {
                assert!(matches!(v.act("voice_import", &files), Err(Refusal::NotAVoice(_))), "{why}");
                assert!(!root.join("voices").join("lumen").exists() && std::fs::read_dir(&root).map(|d| d.count()).unwrap_or(0) == 0, "{why}: nothing left");
            });
        }
    }

    fn keyed<'a>(root: &Path, backend: &'a dyn Backend, keys: &'a dyn Keys, env_key: Option<String>, probed: &'a Mutex<Probed>) -> Voice<'a> {
        Voice { root: root.into(), backend, env_key, keys, probed, mic: Box::leak(Box::new(FakeMic::granted(0.0))), studio: Box::leak(Box::default()), speaker: quiet(), emit: |_| {} }
    }

    struct NoKeys;
    impl Keys for NoKeys {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn present(&self) -> bool {
            false
        }
        fn write(&self, _: &str) -> Result<(), String> {
            Err("no Keychain in the tests".into())
        }
        fn clear(&self) -> Result<(), String> {
            Ok(())
        }
    }
    #[derive(Default)]
    struct MemKeys(Mutex<Option<String>>);
    impl Keys for MemKeys {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(lock(&self.0).clone())
        }
        fn present(&self) -> bool {
            lock(&self.0).is_some()
        }
        fn write(&self, key: &str) -> Result<(), String> {
            *lock(&self.0) = Some(key.into());
            Ok(())
        }
        fn clear(&self) -> Result<(), String> {
            *lock(&self.0) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct Sent(Mutex<Vec<Option<String>>>);
    impl Backend for Sent {
        fn devices(&self, _: &str, key: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            lock(&self.0).push(key.map(String::from));
            Ok(fleet())
        }
        fn voices(&self, _: &str, _: Option<&str>, _: std::time::Instant) -> Result<Value, String> {
            Ok(json!({"voices": []}))
        }
    }

    #[test]
    fn the_pasted_key_goes_to_the_keychain_wins_over_the_env_and_never_reaches_the_page() {
        let fixture_secret_0 = format!("fixture-{}-{}-0", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        let root = scratch();
        let _gone = Gone(root.clone());
        let (keys, sent, probed) = (MemKeys::default(), Sent::default(), Mutex::new(Probed::default()));
        let mut v = keyed(&root, &sent, &keys, Some("dev-env-key-1".into()), &probed);
        let r = v.act("voice_probe", &json!({})).unwrap();
        assert_eq!(r["voice"]["key"], json!({"present": true, "source": "env"}));
        for bad in ["", "short", "has a space in it", &"k".repeat(513)] {
            assert_eq!(v.act("voice_key_set", &json!({"key": bad})), Err(Refusal::KeyBad), "{bad:?}");
        }
        let r = v.act("voice_key_set", &json!({"key": format!("  {fixture_secret_0} ").as_str()})).unwrap();
        assert_eq!(lock(&keys.0).as_deref(), Some(fixture_secret_0.as_str()), "trimmed, in the Keychain");
        assert_eq!(r["voice"]["key"], json!({"present": true, "source": "keychain"}));
        assert!(!r.to_string().contains("lf-key-from-the-person"), "the key never reaches the page");
        v.act("voice_key_clear", &json!({})).unwrap();
        v.env_key = None;
        let r = v.act("voice_probe", &json!({})).unwrap();
        assert_eq!(r["voice"]["key"], json!({"present": false, "source": null}));
        assert_eq!(*lock(&sent.0), vec![Some("dev-env-key-1".into()), Some(fixture_secret_0.as_str().into()), Some("dev-env-key-1".into()), None], "the Keychain's key first, else the env's");
    }

    #[test]
    fn an_import_keeps_the_voices_pitch() {
        let src = scratch();
        let _src = Gone(src.clone());
        let cat = Catalog::new(&src);
        let mut s = VoiceEntry::new("lumen", "Lumen", Language::PtBr, Source::Stock, "qwen3-tts-1.7b-base").unwrap();
        s.pitch_semitones = Some(-3.0);
        cat.save(&mut s).unwrap();
        cat.set_ref(&mut s, Mood::Neutral, &wav(6.0), "Estou lendo esta frase.").unwrap();
        let files = folder(&src, "lumen");
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            v.act("voice_import", &files).unwrap();
            assert_eq!(Catalog::new(&root).load("lumen").unwrap().pitch_semitones, Some(-3.0), "the voice plays at its own pitch");
        });
    }

    #[test]
    fn a_folder_over_the_caps_is_too_big_and_nothing_is_kept() {
        let src = scratch();
        let _src = Gone(src.clone());
        let files = staged(&src);
        let mut many = files.clone();
        many["files"].as_array_mut().unwrap().extend((0..40).map(|i| json!({"name": format!("f{i}.wav"), "bytes_b64": ""})));
        let mut heavy = files.clone();
        let big = { use base64::Engine; base64::engine::general_purpose::STANDARD.encode(vec![0u8; IMPORT_BYTES + 1]) };
        heavy["files"].as_array_mut().unwrap().push(json!({"name": "big.wav", "bytes_b64": big}));
        for (why, value) in [("33 files or more", many), ("over 20 MB", heavy), ("the page's own check", json!({"files": [], "over_caps": true}))] {
            let root = scratch();
            with(&root, &Fake(Ok(fleet())), |v| {
                assert_eq!(v.act("voice_import", &value).err(), Some(Refusal::TooBig), "{why}");
                assert!(std::fs::read_dir(&root).map(|d| d.count()).unwrap_or(0) == 0, "{why}: nothing left");
            });
        }
    }

    #[test]
    fn with_no_connection_try_again_looks_again() {
        let page = include_str!("../../ui/settings.html");
        let i = page.find("if (V.unreachable && !(V.key").expect("the no-connection branch");
        let branch = &page[i..i + page[i..].find('\n').unwrap()];
        assert!(branch.contains("T.no_connection") && branch.contains(r#"id="v-retry""#) && branch.contains("T.retry"), "the line and Try Again");
        assert!(page.contains("no_connection: 'No LocalFleet connection yet. Paste its key in Connection above, or try again once LocalFleet is running.'"));
        assert!(page.contains("no_connection: 'Ainda não há conexão com o LocalFleet. Cole a chave em Conexão, acima, ou tente de novo quando o LocalFleet estiver rodando.'"));
    }

    #[test]
    fn an_import_skips_finders_dotfiles() {
        let src = scratch();
        let _src = Gone(src.clone());
        let mut files = staged(&src);
        files["files"].as_array_mut().unwrap().push(json!({"name": ".DS_Store", "bytes_b64": "AAAA"}));
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            assert_eq!(v.act("voice_import", &files).unwrap()["voice_created"], json!({"voice_id": "lumen"}));
            assert!(!root.join("voices/lumen/.DS_Store").exists(), "a skipped file is never written");
        });
    }

    #[test]
    fn the_page_skips_dotfiles_and_subfolders_and_checks_sizes_before_reading_an_import() {
        let page = include_str!("../../ui/settings.html");
        let i = page.find("on('v-import-file'").expect("the import handler");
        let handler = &page[i..i + page[i..].find("\n    })").unwrap()];
        assert!(handler.contains("startsWith('.')") && handler.contains("webkitRelativePath"), "dotfiles and subfolders are skipped");
        let (sized, read) = (handler.find("f.size").expect("sizes are checked"), handler.find("new FileReader").expect("files are read"));
        assert!(sized < read, "the caps are checked before any file is read");
        assert!(handler.contains("over ? { files: [], over_caps: true }"), "over the caps, the page says so and sends no file");
    }

    struct DeniedKeys;
    impl Keys for DeniedKeys {
        fn read(&self) -> Result<Option<String>, String> {
            Err("Keychain error -128".into())
        }
        fn present(&self) -> bool {
            true
        }
        fn write(&self, _: &str) -> Result<(), String> {
            Err("no Keychain in the tests".into())
        }
        fn clear(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_keychain_read_error_goes_to_the_log_by_code_and_the_env_key_still_works() {
        let root = scratch();
        let _gone = Gone(root.clone());
        let (backend, probed) = (Fake(Ok(fleet())), Mutex::new(Probed::default()));
        let v = keyed(&root, &backend, &DeniedKeys, Some("dev-env-key-1".into()), &probed);
        assert_eq!(v.key().as_deref(), Some("dev-env-key-1"));
        assert_eq!(v.take_key_error().as_deref(), Some("Keychain error -128"), "the code, never a key");
        assert_eq!(v.take_key_error(), None, "logged once");
        v.act("voice_probe", &json!({})).unwrap();
        assert_eq!(v.take_key_error().as_deref(), Some("Keychain error -128"), "a probe's key read is logged too");
        let none = keyed(&root, &backend, &NoKeys, None, &probed);
        assert_eq!((none.key(), none.take_key_error()), (None, None), "no item is not an error");
    }

    #[test]
    fn with_no_connection_set_the_pane_says_so_instead_of_unreachable() {
        let page = include_str!("../../ui/settings.html");
        assert_eq!(page.matches("no_connection: '").count(), 2, "an EN and a PT line");
        let i = page.find("if (V.unreachable").expect("the unreachable branch");
        let branch = &page[i..i + page[i..].find('\n').unwrap()];
        assert!(branch.contains("V.key") && branch.contains("T.no_connection"), "no key and nothing found: the no-connection line");
        assert!(page.contains("if (st === 'no-connection')"), "a demo state for it");
    }

    #[test]
    fn a_keychain_that_refuses_the_write_is_said_and_nothing_is_kept() {
        let root = scratch();
        with(&root, &Fake(Ok(fleet())), |v| {
            assert!(matches!(v.act("voice_key_set", &json!({"key": "lf-key-0000000001"})), Err(Refusal::KeyNotSaved(_))));
            assert_eq!(v.act("voice_probe", &json!({})).unwrap()["voice"]["key"]["present"], json!(false));
        });
    }

    use crate::speaker::Output;
    use rth_voice::queue::PlayQueue;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct FakeOut {
        heard: Arc<Mutex<Vec<i16>>>,
        streams: Arc<AtomicUsize>,
        broken: bool,
        open_delay: Duration,
        opens: AtomicUsize,
        stall_after: Option<usize>,
    }
    struct OutStream(Arc<AtomicBool>, Arc<AtomicUsize>);
    impl Drop for OutStream {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
            self.1.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl Output for FakeOut {
        fn rate(&self) -> Result<u32, String> {
            if self.broken { Err("no output device".into()) } else { Ok(48_000) }
        }
        fn open(&self, queue: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {self.open_bound(queue,None)}
        fn open_front(&self, queue:Arc<PlayQueue>, binding:Arc<crate::voice_front::render::Binding>) -> Result<Box<dyn Any+Send>,String> {self.open_bound(queue,Some(binding))}
    }
    impl FakeOut {
        fn open_bound(&self, queue: Arc<PlayQueue>, binding:Option<Arc<crate::voice_front::render::Binding>>) -> Result<Box<dyn Any + Send>, String> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.open_delay);
            let (stop, heard) = (Arc::new(AtomicBool::new(false)), Arc::clone(&self.heard));
            let (halt, stall_after) = (Arc::clone(&stop), self.stall_after.unwrap_or(usize::MAX));
            std::thread::spawn(move || {
                let (mut buf, mut taken) = (vec![0i16; 480], 0);
                while !halt.load(Ordering::SeqCst) {
                    if taken < stall_after {
                        let n = match &binding {Some(b)=>b.pull(&queue,&mut buf,false),None=>queue.pull(&mut buf)};
                        taken += n;
                        lock(&heard).extend_from_slice(&buf[..n]);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            self.streams.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(OutStream(stop, Arc::clone(&self.streams))))
        }
    }

    fn quiet() -> &'static Speaker {
        Box::leak(Box::new(Speaker::new(Box::leak(Box::<FakeOut>::default()))))
    }

    enum Says {
        Tone(f64, i16),
        Truncated(f64,i16),

        Sine(f64, f64),
        Held(f64, i16, Arc<(Mutex<bool>, std::sync::Condvar)>),
        Busy(u32),

        Unavailable(u32),

        Paced(f64, i16),

        Late(u64, f64, i16),
    }

    static HUNG_UP: AtomicUsize = AtomicUsize::new(0);
    struct Lf {
        url: String,
        bodies: Arc<Mutex<Vec<Value>>>,
        served: Arc<AtomicUsize>,
    }

    fn localfleet(says: impl Fn(usize) -> Says + Send + 'static) -> Lf {
        use std::io::{BufRead, Read};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (bodies, served) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicUsize::new(0)));
        let (got, sent) = (Arc::clone(&bodies), Arc::clone(&served));
        std::thread::spawn(move || {
            for (n, conn) in listener.incoming().enumerate() {
                let Ok(mut conn) = conn else { continue };
                let mut r = std::io::BufReader::new(conn.try_clone().unwrap());
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                let _ = r.read_exact(&mut body);
                lock(&got).push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                let (reply, sent) = (says(n), Arc::clone(&sent));
                std::thread::spawn(move || {
                    speak_back(&mut conn, reply);
                    sent.fetch_add(1, Ordering::SeqCst);
                });
            }
        });
        Lf { url, bodies, served }
    }
    fn speak_back(conn: &mut std::net::TcpStream, says: Says) {
        use base64::Engine;
        use std::io::Write;
        let chunk = |conn: &mut std::net::TcpStream, data: &str| {
            let _ = write!(conn, "{:x}\r\n{data}\r\n", data.len());
        };
        let truncated=matches!(says,Says::Truncated(..));
        let (secs, value) = match says {
            Says::Busy(after) => {
                let body = r#"{"error":{"code":"admission_busy","message":"busy"}}"#;
                let _ = write!(conn, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: {after}\r\n\r\n{body}", body.len());
                return;
            }
            Says::Unavailable(after) => {
                let body = r#"{"error":{"code":"accelerator_busy","message":"busy"}}"#;
                let _ = write!(conn, "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: {after}\r\n\r\n{body}", body.len());
                return;
            }
            Says::Tone(secs, value) | Says::Truncated(secs,value) => (secs, value),
            Says::Paced(secs, value) => {
                let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                for _ in 0..(secs * 10.0) as usize {
                    let pcm: Vec<u8> = std::iter::repeat_n(value.to_le_bytes(), 2400).flatten().collect();
                    let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
                    let event = format!("event: audio\ndata: {data}\n\n");
                    if write!(conn, "{:x}\r\n{event}\r\n", event.len()).and_then(|_| conn.flush()).is_err() {
                        HUNG_UP.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let n = (2400.0 * secs * 10.0) as usize;
                let done = json!({"truncated": false, "error": null, "frames": n / 1920, "audio_seconds": (n as f64 / 24.0).round() / 1000.0});
                chunk(conn, &format!("event: done\ndata: {done}\n\n"));
                let _ = conn.write_all(b"0\r\n\r\n");
                return;
            }
            Says::Sine(secs, hz) => {
                let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
                let n = (24_000.0 * secs) as usize;
                let wave: Vec<i16> = (0..n).map(|i| ((std::f64::consts::TAU * hz * i as f64 / 24_000.0).sin() * 12_000.0) as i16).collect();
                for part in wave.chunks(4800) {
                    let pcm: Vec<u8> = part.iter().flat_map(|v| v.to_le_bytes()).collect();
                    let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
                    chunk(conn, &format!("event: audio\ndata: {data}\n\n"));
                }
                let done = json!({"truncated": false, "error": null, "frames": n / 1920, "audio_seconds": (n as f64 / 24.0).round() / 1000.0});
                chunk(conn, &format!("event: done\ndata: {done}\n\n"));
                let _ = conn.write_all(b"0\r\n\r\n");
                return;
            }
            Says::Late(ms, secs, value) => {
                std::thread::sleep(Duration::from_millis(ms));
                (secs, value)
            }
            Says::Held(secs, value, gate) => {
                let (open, cv) = &*gate;
                let _ = cv.wait_timeout_while(lock(open), Duration::from_secs(5), |o| !*o);
                (secs, value)
            }
        };
        let _ = conn.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n");
        let n = (24_000.0 * secs) as usize;
        let mut left = n;
        while left > 0 {
            let k = left.min(4800);
            left -= k;
            let pcm: Vec<u8> = std::iter::repeat_n(value.to_le_bytes(), k).flatten().collect();
            let data = json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"});
            chunk(conn, &format!("event: audio\ndata: {data}\n\n"));
        }
        let done = json!({"truncated": truncated, "error": null, "frames": n / 1920, "audio_seconds": (n as f64 / 24.0).round() / 1000.0});
        chunk(conn, &format!("event: done\ndata: {done}\n\n"));
        let _ = conn.write_all(b"0\r\n\r\n");
    }

    static SAID: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    fn said(id: &str) -> Vec<Value> {
        lock(&SAID).iter().filter(|e| e["preview_id"] == id).cloned().collect()
    }
    fn ended(id: &str) -> Option<Value> {
        said(id).into_iter().find(|e| e["state"] != "streaming")
    }
    fn until(what: &str, f: impl Fn() -> bool) {
        let t = Instant::now();
        while !f() {
            assert!(t.elapsed() < Duration::from_secs(8), "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn speaker_on(out: &'static FakeOut) -> &'static Speaker {
        Box::leak(Box::new(Speaker::new(out)))
    }
    fn speaking_voice(root: &Path, speaker: &'static Speaker) -> Voice<'static> {
        let (backend, probed) = (Box::leak(Box::new(Fake(Ok(fleet())))), Box::leak(Box::new(Mutex::new(Probed::default()))));
        let (mic, studio) = (Box::leak(Box::new(FakeMic::granted(0.0))), Box::leak(Box::<Studio>::default()));
        Voice { root: root.into(), backend, env_key: None, keys: &NoKeys, probed, mic, studio, speaker, emit: |e| lock(&SAID).push(e.clone()) }
    }

    fn tone_wav(secs: f64, value: i16) -> Vec<u8> {
        let mut b = wav(secs);
        for s in b[44..].chunks_exact_mut(2) {
            s.copy_from_slice(&value.to_le_bytes());
        }
        b
    }

    fn lumen(root: &Path, lf: &Lf) -> &'static str {
        let cat = Catalog::new(root);
        let mut v = VoiceEntry::new("lumen", "Lumen", Language::PtBr, Source::Personal, "qwen3-tts-1.7b-base").unwrap();
        cat.save(&mut v).unwrap();
        cat.set_ref(&mut v, Mood::Neutral, &wav(6.0), "Estou lendo esta frase.").unwrap();
        cat.set_ref(&mut v, Mood::Joy, &tone_wav(5.5, 3000), "Que dia bonito!").unwrap();
        std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": lf.url}).to_string()).unwrap();
        "lumen"
    }
    fn started(r: &Value) -> String {
        r["voice_started"]["preview_id"].as_str().expect("voice_started names the preview").to_string()
    }
    fn near(v: i16, to: i16) -> bool {
        (v - to).abs() <= 100
    }

    #[test]
    fn a_preview_speaks_its_line_through_the_one_speaker_and_done_comes_last() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let id = lumen(&root, &lf);
            let r = v.act("voice_preview", &json!({"voice_id": id, "lang": "pt-BR"})).unwrap();
            assert_eq!(r["voice_started"]["total_steps"], json!(1));
            assert!(r["voice"]["voices"].is_array(), "the reply carries the voice view");
            let p = started(&r);
            until("the preview ends", || ended(&p).is_some());
            let ev = said(&p);
            assert_eq!(ev[0]["state"], json!("streaming"), "{ev:?}");
            assert_eq!(ev.iter().filter(|e| e["state"] != "streaming").count(), 1, "one terminal event: {ev:?}");
            let last = ev.last().unwrap();
            assert_eq!((last["state"].clone(), last["truncated"].clone(), last["error"].clone()), (json!("done"), json!(false), Value::Null));
            assert!((last["audio_seconds"].as_f64().unwrap() - 0.3).abs() < 0.03, "{last}");
            assert!(last["first_audio_ms"].as_u64().is_some(), "{last}");
            assert_eq!(lock(&lf.bodies)[0]["input"], json!(line(Mood::Neutral, Language::PtBr).unwrap()), "clone_lines_v1's Neutral line");
            until("the device got all of it, resampled to 48 kHz", || lock(&out.heard).len() >= 14_000);
            assert!(lock(&out.heard).iter().filter(|s| near(**s, 2000)).count() >= 13_500, "the tone, at the device's rate");
            until("the speaker closes once nothing plays", || out.streams.load(Ordering::SeqCst) == 0);
        });
    }

    #[test]
    fn a_preview_stopped_while_pending_never_plays_its_late_reply() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            let held = Arc::clone(&gate);
            let lf = localfleet(move |_| Says::Held(0.5, 2000, Arc::clone(&held)));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "en"})).unwrap());
            until("the request reached LocalFleet", || lock(&lf.bodies).len() == 1);
            v.act("voice_stop", &json!({})).unwrap();
            let end = ended(&p).expect("a stop ends the preview at once");
            assert_eq!((end["state"].clone(), end["audio_seconds"].clone()), (json!("done"), json!(0.0)));
            *lock(&gate.0) = true;
            gate.1.notify_all();
            until("LocalFleet sent the late reply", || lf.served.load(Ordering::SeqCst) == 1);
            std::thread::sleep(Duration::from_millis(400));
            assert!(lock(&out.heard).is_empty(), "the late reply never plays");
            assert_eq!(said(&p).len(), 1, "nothing after done");
        });
    }

    #[test]
    fn a_stop_while_playing_flushes_at_once() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(3.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let v = speaking_voice(&root, speaker);
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "pt-BR"})).unwrap());
            until("it plays", || !lock(&out.heard).is_empty());
            v.act("voice_stop", &json!({})).unwrap();
            assert_eq!(speaker.queued(), 0, "flushed at once");
            let at_stop = lock(&out.heard).len();
            let end = ended(&p).expect("done at the stop");
            assert_eq!(end["state"], json!("done"));
            assert!(end["audio_seconds"].as_f64().unwrap() < 3.0, "{end}");
            std::thread::sleep(Duration::from_millis(300));
            assert!(lock(&out.heard).len() <= at_stop + 480, "nothing after the stop but the buffer in flight");
            assert_eq!(said(&p).last().unwrap()["state"], json!("done"), "done is last");
        });
    }

    #[test]
    fn one_playback_at_a_time_a_newer_preview_stops_the_playing_one() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|n| if n == 0 { Says::Tone(3.0, 1000) } else { Says::Tone(0.3, 2000) });
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let id = lumen(&root, &lf);
            let a = started(&v.act("voice_preview", &json!({"voice_id": id, "lang": "pt-BR"})).unwrap());
            until("the first plays", || lock(&out.heard).iter().any(|s| near(*s, 1000)));
            let b = started(&v.act("clone_preview", &json!({"voice_id": id, "mood": "joy"})).unwrap());
            let end_a = ended(&a).expect("the newer preview ended the first at once");
            assert_eq!(end_a["state"], json!("done"));
            until("the second ends", || ended(&b).is_some());
            assert_eq!(ended(&b).unwrap()["state"], json!("done"));
            assert_eq!(lock(&lf.bodies)[1]["input"], json!(line(Mood::Joy, Language::PtBr).unwrap()), "the mood's line, in the voice's language");
            let heard = lock(&out.heard).clone();
            let first_b = heard.iter().position(|s| near(*s, 2000)).expect("the second played");
            assert!(!heard[first_b..].iter().any(|s| near(*s, 1000)), "nothing of the first after the second began");
            assert_eq!(said(&a).len(), said(&a).iter().position(|e| e["state"] != "streaming").unwrap() + 1, "done is the first's last");
        });
    }

    #[test]
    fn the_settings_window_closing_stops_the_preview() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(3.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let v = speaking_voice(&root, speaker);
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "pt-BR"})).unwrap());
            until("it plays", || !lock(&out.heard).is_empty());
            window_closed(v.studio, v.speaker);
            assert_eq!(speaker.queued(), 0, "flushed at the close");
            assert_eq!(ended(&p).expect("the close ends it")["state"], json!("done"));
        });
    }

    #[test]
    fn a_busy_localfleet_fails_the_preview_with_its_retry_and_nothing_plays() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Busy(4));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "pt-BR"})).unwrap());
            until("the preview ends", || ended(&p).is_some());
            let ev = said(&p);
            assert_eq!(ev.len(), 1, "{ev:?}");
            assert_eq!((ev[0]["state"].clone(), ev[0]["error"]["code"].clone(), ev[0]["retry_after_s"].clone()), (json!("failed"), json!("busy"), json!(4)));
            assert!(lock(&out.heard).is_empty());
        });
    }

    #[test]
    fn a_reference_plays_from_its_wav_with_no_synthesis() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let r = v.act("clone_ref_play", &json!({"voice_id": lumen(&root, &lf), "mood": "joy"})).unwrap();
            let p = started(&r);
            assert_eq!(r["voice_started"]["total_steps"], json!(1));
            until("the clip plays", || lock(&out.heard).iter().filter(|s| near(**s, 3000)).count() > 4800);
            assert!(lock(&lf.bodies).is_empty(), "nothing went to LocalFleet");
            v.act("voice_stop", &json!({})).unwrap();
            assert_eq!(ended(&p).unwrap()["state"], json!("done"));
            assert!(matches!(v.act("clone_ref_play", &json!({"voice_id": "lumen", "mood": "tired"})), Err(Refusal::Bad(_))), "no clip for that mood");
        });
    }

    fn pitch_hz(x: &[i16], rate: f64) -> f64 {
        let (a, b) = (x.len() / 4, x.len() * 3 / 4);
        let ups: Vec<f64> = (a..b).filter(|&i| x[i - 1] < 0 && x[i] >= 0).map(|i| (i - 1) as f64 + x[i - 1] as f64 / (x[i - 1] as f64 - x[i] as f64)).collect();
        (ups.len() - 1) as f64 / (ups[ups.len() - 1] - ups[0]) * rate
    }

    #[test]
    fn a_voice_tuned_down_three_semitones_plays_lower_at_the_same_length() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Sine(1.0, 300.0));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let id = lumen(&root, &lf);
            let cat = Catalog::new(&root);
            let mut e = cat.load(id).unwrap();
            e.pitch_semitones = Some(-3.0);
            cat.save(&mut e).unwrap();
            let p = started(&v.act("voice_preview", &json!({"voice_id": id, "lang": "pt-BR"})).unwrap());
            until("the preview ends", || ended(&p).is_some());
            assert_eq!(ended(&p).unwrap()["state"], json!("done"));
            until("the device got it all", || lock(&out.heard).len() >= 47_500);
            std::thread::sleep(Duration::from_millis(100));
            let heard = lock(&out.heard).clone();
            assert!((heard.len() as f64 / 48_000.0 - 1.0).abs() < 0.01, "the same length: {} samples at 48 kHz", heard.len());
            let (got, want) = (pitch_hz(&heard, 48_000.0) / 300.0, 2f64.powf(-3.0 / 12.0));
            assert!((got / want - 1.0).abs() < 0.01, "pitch ratio {got}, want {want}");
            let sent = &lock(&lf.bodies)[0];
            assert!(sent.get("pitch_semitones").is_none() && sent.to_string().len() > 100, "the synthesis request is as before");
        });
    }

    #[test]
    fn a_pitch_out_of_range_is_refused_before_anything_plays() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let id = lumen(&root, &lf);
            let cat = Catalog::new(&root);
            let mut e = cat.load(id).unwrap();
            e.pitch_semitones = Some(13.0);
            assert!(cat.save(&mut e).is_err(), "the catalogue refuses to save it");
            let path = root.join("voices").join(id).join("voice.json");
            let mut j: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            j["pitch_semitones"] = json!(13.0);
            std::fs::write(&path, j.to_string()).unwrap();
            assert!(matches!(v.act("voice_preview", &json!({"voice_id": id, "lang": "pt-BR"})), Err(Refusal::Bad(_))), "nor loads it");
            assert!(lock(&lf.bodies).is_empty() && lock(&out.heard).is_empty());
        });
    }

    #[test]
    fn with_no_speaker_the_preview_fails_at_once_and_nothing_is_asked() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::new(FakeOut { broken: true, ..FakeOut::default() }));
            let v = speaking_voice(&root, speaker_on(out));
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "pt-BR"})).unwrap());
            let ev = said(&p);
            assert_eq!(ev.len(), 1, "{ev:?}");
            assert_eq!((ev[0]["state"].clone(), ev[0]["error"]["code"].clone()), (json!("failed"), json!("device_unavailable")));
            std::thread::sleep(Duration::from_millis(100));
            assert!(lock(&lf.bodies).is_empty(), "no request without a speaker");
        });
    }

    #[test]
    fn record_start_silences_a_playing_preview() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(3.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let v = speaking_voice(&root, speaker);
            let id = lumen(&root, &lf);
            let p = started(&v.act("voice_preview", &json!({"voice_id": id, "lang": "pt-BR"})).unwrap());
            until("the preview plays", || said(&p).iter().any(|e| e["state"] == "streaming"));
            v.act("clone_record_start", &json!({"voice_id": id, "mood": "neutral"})).unwrap();
            assert!(ended(&p).is_some(), "the preview ended before the mic opened");
            assert_eq!(speaker.queued(), 0, "nothing of the preview is left to play into the take");
            v.act("clone_record_cancel", &json!({})).unwrap();
        });
    }

    #[test]
    fn a_stop_hangs_up_on_localfleet_so_its_slot_frees_at_once() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Paced(6.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = speaking_voice(&root, speaker_on(out));
            let p = started(&v.act("voice_preview", &json!({"voice_id": lumen(&root, &lf), "lang": "pt-BR"})).unwrap());
            until("the preview plays", || said(&p).iter().any(|e| e["state"] == "streaming"));
            let before = HUNG_UP.load(Ordering::SeqCst);
            let t = Instant::now();
            v.act("voice_stop", &json!({})).unwrap();
            until("LocalFleet saw the hang-up", || HUNG_UP.load(Ordering::SeqCst) > before);
            assert!(t.elapsed() < Duration::from_secs(2), "the request was dropped, not read to its end: {:?}", t.elapsed());
        });
    }

    #[test]
    fn closing_settings_while_the_speaker_opens_does_not_wait_for_it() {
        within(20, || {
            let out: &'static FakeOut = Box::leak(Box::new(FakeOut { open_delay: Duration::from_millis(1500), ..FakeOut::default() }));
            let speaker = speaker_on(out);
            let id = Arc::new(Mutex::new(None));
            let got = Arc::clone(&id);
            std::thread::spawn(move || *lock(&got) = Some(speaker.start(Job::Clip { samples: vec![2000; 4800], rate: 24_000 }, |e| lock(&SAID).push(e.clone()))));
            until("the speaker is opening", || out.opens.load(Ordering::SeqCst) == 1);
            let t = Instant::now();
            speaker.stop();
            assert!(t.elapsed() < Duration::from_millis(300), "the stop didn't wait for the device: {:?}", t.elapsed());
            until("start returned", || lock(&id).is_some());
            let p = lock(&id).clone().unwrap();
            until("the stopped preview ended", || ended(&p).is_some());
            until("a stream opened for a stopped preview is closed", || out.streams.load(Ordering::SeqCst) == 0);
            assert!(lock(&out.heard).is_empty(), "nothing played");
        });
    }

    #[test]
    fn a_device_that_stops_pulling_fails_the_preview_instead_of_done() {
        within(20, || {
            let out: &'static FakeOut = Box::leak(Box::new(FakeOut { stall_after: Some(4800), ..FakeOut::default() }));
            let speaker = speaker_on(out);
            let p = speaker.start(Job::Clip { samples: vec![2000; 12_000], rate: 24_000 }, |e| lock(&SAID).push(e.clone()));
            until("the drain slack runs out", || ended(&p).is_some());
            let end = ended(&p).unwrap();
            assert_eq!((end["state"].clone(), end["error"]["code"].clone()), (json!("failed"), json!("device_unavailable")), "{end}");
        });
    }

    #[test]
    fn a_stalled_device_ends_a_long_preview_still_waiting_for_room() {
        within(20, || {
            let out: &'static FakeOut = Box::leak(Box::new(FakeOut { stall_after: Some(4800), ..FakeOut::default() }));
            let speaker = speaker_on(out);
            let p = speaker.start(Job::Clip { samples: vec![2000; 24_000 * 8], rate: 24_000 }, |e| lock(&SAID).push(e.clone()));
            until("the wait for room gives up", || ended(&p).is_some());
            let end = ended(&p).unwrap();
            assert_eq!((end["state"].clone(), end["error"]["code"].clone()), (json!("failed"), json!("device_unavailable")), "{end}");
            assert_eq!(said(&p).iter().filter(|e| e["state"] != "streaming").count(), 1, "one terminal event");
        });
    }

    #[test]
    fn every_page_action_is_known() {
        let page = include_str!("../../ui/settings.html");
        let i = page.find("const VOICE_ACTIONS = [").expect("the page lists its voice actions");
        let list = &page[i..i + page[i..].find(']').unwrap()];
        let sent: Vec<&str> = list.split('\'').skip(1).step_by(2).collect();
        assert_eq!(sent.len(), ACTIONS.len());
        for a in sent {
            assert!(ACTIONS.contains(&a), "{a}");
        }
    }

    fn stored() -> Value {
        let refs = |ms: &[&str]| Value::Object(ms.iter().map(|m| (m.to_string(), json!({"ref_hash": "h", "seconds": 6.0}))).collect());
        json!({"voices": [
            {"voice_id": "unfold-pt", "name": "Unfold (pt-BR)", "personal": false, "default_mood": "calm", "temperature": null, "references": refs(&["neutral", "calm", "joy"]), "pitch_semitones": -3.0},
            {"voice_id": "studio-b", "name": "Studio B", "personal": true, "default_mood": null, "temperature": 1.0, "references": refs(&["neutral"])},
            {"voice_id": "too-high", "name": "High", "personal": false, "references": refs(&["neutral"]), "pitch_semitones": 13.0},
            {"voice_id": "Bad Id", "name": "Bad", "personal": false, "references": refs(&["neutral"])},
            {"voice_id": "no-clips", "name": "Empty", "personal": true, "references": {}},
            {"voice_id": "whisper-only", "name": "Whisper", "personal": false, "references": refs(&["whisper"])}]})
    }

    #[test]
    fn localfleet_stored_voices_are_listed_with_their_moods_and_pitch() {
        let got = page_voices(&stored());
        assert_eq!(got, vec![
            json!({"id": "unfold-pt", "name": "Unfold (pt-BR)", "personal": false, "moods": ["neutral", "calm", "joy"], "default_mood": "calm", "pitch_semitones": -3.0}),
            json!({"id": "studio-b", "name": "Studio B", "personal": true, "moods": ["neutral"], "default_mood": null, "pitch_semitones": null}),
        ]);
        assert_eq!(page_voices(&json!({"voices": "no"})), Vec::<Value>::new());
    }

    #[test]
    fn with_no_choice_the_voice_is_unfold_pt_and_a_choice_is_saved() {
        let root = scratch();
        let _gone = Gone(root.clone());
        let mut raw = fleet();
        raw["voices"] = stored()["voices"].clone();
        let (backend, probed) = (Box::leak(Box::new(Fake(Ok(raw)))), Box::leak(Box::new(Mutex::new(Probed::default()))));
        let (mic, studio) = (Box::leak(Box::new(FakeMic::granted(0.0))), Box::leak(Box::<Studio>::default()));
        let v = Voice { root: root.clone(), backend, env_key: None, keys: &NoKeys, probed, mic, studio, speaker: quiet(), emit: |_| {} };
        let r = v.act("voice_probe", &json!({})).unwrap();
        assert_eq!(r["voice"]["fleet_voices"].as_array().map(Vec::len), Some(2));
        assert_eq!(r["voice"]["fleet_chosen"], json!({"voice_id": "unfold-pt", "mood": "calm", "default": true}), "unfold-pt at its default mood");
        let r = v.act("fleet_choose", &json!({"voice_id": "studio-b"})).unwrap();
        assert_eq!(r["voice"]["fleet_chosen"], json!({"voice_id": "studio-b", "mood": "neutral", "default": false}));
        let r = v.act("fleet_choose", &json!({"voice_id": "unfold-pt", "mood": "joy"})).unwrap();
        assert_eq!(r["voice"]["fleet_chosen"], json!({"voice_id": "unfold-pt", "mood": "joy", "default": false}));
        let saved: Value = serde_json::from_slice(&std::fs::read(root.join("prefs.json")).unwrap()).unwrap();
        assert_eq!(saved["fleet"], json!({"voice_id": "unfold-pt", "mood": "joy"}), "the choice is on disk");
        assert!(v.act("fleet_choose", &json!({"voice_id": "too-high"})).is_err(), "a voice not listed");
        assert!(v.act("fleet_choose", &json!({"voice_id": "studio-b", "mood": "joy"})).is_err(), "a mood it has no clip for");

        lock(probed).voices.retain(|x| x["id"] != "unfold-pt");
        assert_eq!(v.act("voice_stop", &json!({})).unwrap()["voice"]["fleet_chosen"], Value::Null, "neither listed: none");
    }

    fn fleet_voice(root: &Path, lf: &Lf, speaker: &'static Speaker) -> Voice<'static> {
        let v = speaking_voice(root, speaker);
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": lf.url}).to_string()).unwrap();
        lock(v.probed).voices = page_voices(&stored());
        v
    }

    #[test]
    fn a_stored_voice_preview_asks_localfleet_for_that_voice_and_mood_and_plays_it() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = fleet_voice(&root, &lf, speaker_on(out));
            let p = started(&v.act("fleet_preview", &json!({"voice_id": "studio-b", "mood": "neutral"})).unwrap());
            until("the preview ends", || ended(&p).is_some());
            assert_eq!(ended(&p).unwrap()["state"], json!("done"), "{:?}", said(&p));
            let sent = lock(&lf.bodies)[0].clone();
            assert_eq!(sent, json!({"model": "qwen3-tts-1.7b-base", "input": line(Mood::Neutral, Language::PtBr).unwrap(), "voice": "studio-b", "mood": "neutral", "stream": true, "stream_format": "events"}), "no references, no language: LocalFleet holds the clips and picks the language");
            until("the device got the tone", || lock(&out.heard).iter().filter(|s| near(**s, 2000)).count() >= 13_500);

            let p = started(&v.act("fleet_preview", &json!({"voice_id": "unfold-pt"})).unwrap());
            until("the second preview ends", || ended(&p).is_some());
            let second = lock(&lf.bodies)[1].clone();
            assert_eq!((second["mood"].clone(), second["input"].clone()), (json!("calm"), json!(line(Mood::Calm, Language::PtBr).unwrap())));
            assert!(v.act("fleet_preview", &json!({"voice_id": "no-clips"})).is_err(), "a voice not listed asks nothing");
        });
    }

    #[test]
    fn a_stored_voice_with_minus_three_plays_lower_and_one_without_plays_as_sent() {
        within(30, || {
            for (id, want) in [("unfold-pt", 2f64.powf(-3.0 / 12.0)), ("studio-b", 1.0)] {
                let root = scratch();
                let _gone = Gone(root.clone());
                let lf = localfleet(|_| Says::Sine(1.0, 300.0));
                let out: &'static FakeOut = Box::leak(Box::default());
                let v = fleet_voice(&root, &lf, speaker_on(out));
                let p = started(&v.act("fleet_preview", &json!({"voice_id": id, "mood": "neutral"})).unwrap());
                until("the preview ends", || ended(&p).is_some());
                assert_eq!(ended(&p).unwrap()["state"], json!("done"));
                until("the device got it all", || lock(&out.heard).len() >= 47_500);
                std::thread::sleep(Duration::from_millis(100));
                let heard = lock(&out.heard).clone();
                assert!((heard.len() as f64 / 48_000.0 - 1.0).abs() < 0.01, "{id}: the same length, {} samples at 48 kHz", heard.len());
                let got = pitch_hz(&heard, 48_000.0) / 300.0;
                assert!((got / want - 1.0).abs() < 0.01, "{id}: pitch ratio {got}, want {want}");
                assert!(lock(&lf.bodies)[0].get("pitch_semitones").is_none(), "LocalFleet isn't asked to shift it");
            }
        });
    }

    #[test]
    fn a_busy_localfleet_fails_a_stored_voice_preview_with_its_retry() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Busy(1));
            let v = fleet_voice(&root, &lf, quiet());
            let p = started(&v.act("fleet_preview", &json!({"voice_id": "unfold-pt", "mood": "neutral"})).unwrap());
            until("the preview ends", || ended(&p).is_some());
            let e = ended(&p).unwrap();
            assert_eq!((e["state"].clone(), e["error"]["code"].clone(), e["retry_after_s"].clone()), (json!("failed"), json!("busy"), json!(1)), "{e}");
            assert_eq!(lock(&lf.bodies).len(), 1 + BUSY_RETRIES as usize, "bounded busy retries are exhausted");
            assert_eq!(e["retries"], BUSY_RETRIES);
        });
    }

    #[test]
    fn the_probes_two_gets_share_one_deadline() {
        within(30, || {
            use std::io::{Read, Write};
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", l.local_addr().unwrap());

            std::thread::spawn(move || {
                let mut held = Vec::new();
                for slow in [true, false] {
                    let Ok((mut s, _)) = l.accept() else { return };
                    let _ = s.read(&mut [0u8; 1024]);
                    if slow {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let body = serde_json::to_vec(&fleet()).unwrap();
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                        let _ = s.write_all(&body);
                    }
                    held.push(s);
                }
                std::thread::sleep(std::time::Duration::from_secs(15));
            });
            let root = scratch();
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": url}).to_string()).unwrap();
            with(&root, &Fleet, |v| {
                let t = std::time::Instant::now();
                let r = v.act("voice_probe", &json!({})).unwrap();
                let took = t.elapsed();
                assert!(took < PROBE_WAIT + std::time::Duration::from_millis(1500), "the whole probe in one deadline: {took:?}");
                assert_eq!((r["voice"]["unreachable"].clone(), r["voice"]["fleet_voices"].clone()), (json!(false), json!([])), "{r}");
                assert!(v.take_probe_error().is_some_and(|e| e.starts_with("GET /v1/voices")));
            });
        });
    }

    #[test]
    fn a_personal_voice_is_on_this_mac_only_behind_this_macs_own_gateway() {
        for (url, here) in [(None, true), (Some("http://localhost:8310"), true), (Some("http://[::1]:8310"), true), (Some("http://192.168.1.20:8310"), false), (Some("http://studio.local:8310"), false)] {
            let root = scratch();
            std::fs::create_dir_all(&root).unwrap();
            if let Some(u) = url {
                std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": u}).to_string()).unwrap();
            }
            with(&root, &Sent::default(), |v| {
                assert_eq!(v.act("voice_probe", &json!({})).unwrap()["voice"]["fleet_here"], json!(here), "{url:?}");
            });
        }
    }

    #[test]
    fn a_reply_says_what_its_mode_asks_and_leaves_code_links_and_tables_on_screen() {
        let two = "Olá! Tudo bem com você?\nSegunda linha.";
        assert_eq!(spoken(two, "first_sentence", true).as_deref(), Some("Olá!"));
        assert_eq!(spoken(two, "summary", true).as_deref(), Some("Olá!"), "a summary reads the first sentence for now");
        assert_eq!(spoken(two, "full", true).as_deref(), Some("Olá! Tudo bem com você? Segunda linha."));
        assert_eq!(spoken(two, "off", true), None);

        let code = "Here is the command.\n```sh\nls -la\n```\nThat lists them.";
        assert_eq!(spoken(code, "full", false).as_deref(), Some("Here is the command."));
        for (first, why) in [("Run `ls` to see them. Then pick one.", "code"), ("See https://example.com for more. It helps.", "a link"), ("| a | b |\n|---|---|\n| 1 | 2 |", "a table"), ("```\nx = 1\n```", "a fence")] {
            assert_eq!(spoken(first, "full", false).as_deref(), Some("It’s on screen."), "{why}");
            assert_eq!(spoken(first, "first_sentence", true).as_deref(), Some("Deixei na tela."), "{why}");
        }

        assert_eq!(spoken("# Plan\n**Sure.** Here it is.", "full", false).as_deref(), Some("Plan Sure. Here it is."));
        assert_eq!(spoken("⟨say:speech mood=joy⟩\nOi! Tudo bem?", "first_sentence", true).as_deref(), Some("Oi!"));
        assert_eq!(spoken("  \n ", "full", true), None, "nothing to say");
    }

    #[test]
    fn a_reply_cut_at_its_length_limit_is_said_without_the_continue_marker() {
        let en = format!("A heat pump moves heat. It works backwards too.{}", CUT_MARKERS[0]);
        let pt = format!("Uma bomba de calor move o calor. Funciona ao contrário também.{}", CUT_MARKERS[1]);
        assert_eq!(spoken(&en, "full", false).as_deref(), Some("A heat pump moves heat. It works backwards too."));
        assert_eq!(spoken(&pt, "full", true).as_deref(), Some("Uma bomba de calor move o calor. Funciona ao contrário também."));
        assert_eq!(spoken(&en, "first_sentence", false).as_deref(), Some("A heat pump moves heat."));
        assert_eq!(spoken(&format!("Two words{}", CUT_MARKERS[0]), "full", false).as_deref(), Some("Two words"), "the marker in the first sentence");
        assert_eq!(spoken(&format!("Duas palavras{}\n", CUT_MARKERS[1]), "first_sentence", true).as_deref(), Some("Duas palavras"));

        assert_eq!(after_first(&en, "full", false).as_deref(), Some("It works backwards too."));
        assert_eq!(after_first(&format!("Only one.{}", CUT_MARKERS[0]), "full", false), None);

        let inside = format!("Before{} and after.", CUT_MARKERS[0]);
        assert!(spoken(&inside, "full", false).unwrap().contains("continue"), "not at the end: said as written");
        assert_eq!(spoken("Short (cut short)", "full", false).as_deref(), Some("Short (cut short)"));
    }

    #[test]
    fn a_replys_language_is_read_from_its_words_and_the_app_language_breaks_a_tie() {
        assert_eq!(reply_language("Você não está sozinho, isso é bom.", false), "pt");
        assert_eq!(reply_language("This is what you need for the trip.", true), "en");
        assert_eq!((reply_language("OK.", true), reply_language("OK.", false)), ("pt", "en"));

        assert_eq!(reply_language("Você pode usar isso aqui, é simples:\n```python\nfor x in it:\n    if x is not None and x == the:\n        print(x)\n```", false), "pt");
        assert_eq!(reply_language("<um> <um> <um> This is it.", true), "en");
    }

    #[test]
    fn a_replace_starts_the_gathering_over_on_the_new_text() {
        let g = ReplyText::new();
        g.turn(true);
        assert_eq!(g.frame(&json!({"delta": "Think. Think more"})), Some(Gather::First("Think.".into())));
        assert_eq!(g.frame(&json!({"replace": "4"})), Some(Gather::Restart(None)));
        assert_eq!(g.frame(&json!({"done": true})), Some(Gather::Done("4".into())));
        g.turn(true);
        assert_eq!(g.frame(&json!({"replace": "Four. It is four"})), Some(Gather::Restart(Some("Four. It is four".into()))));
        assert_eq!(g.frame(&json!({"delta": ". Yes."})), Some(Gather::First("Four. It is four. Yes.".into())), "replacement resets the whole-sentence stage");
        assert_eq!(g.frame(&json!({"done": true})), Some(Gather::Done("Four. It is four. Yes.".into())));
    }

    #[test]
    fn only_a_spoken_turns_reply_is_gathered_and_an_error_drops_it() {
        let g = ReplyText::new();
        g.turn(false);
        assert_eq!((g.frame(&json!({"delta": "Typed reply."})), g.frame(&json!({"done": true}))), (None, None), "a typed turn is never spoken");
        g.turn(true);
        assert_eq!(g.frame(&json!({"delta":"Olá! "})), Some(Gather::First("Olá!".into())));
        for f in [json!({"wire": "memory_saved", "error": "x"}), json!({"status": "Thinking"})] {
            assert_eq!(g.frame(&f), None);
        }

        assert_eq!(g.frame(&json!({"delta": "Tudo bem?"})), Some(Gather::First("Olá! Tudo bem?".into())));
        assert_eq!(g.frame(&json!({"delta": " Sim."})), None, "the first sentence once");
        assert_eq!(g.frame(&json!({"done": true})), Some(Gather::Done("Olá! Tudo bem? Sim.".into())));
        assert_eq!(g.frame(&json!({"done": true})), None, "once");
        g.turn(true);
        g.frame(&json!({"delta": "Half"}));
        assert_eq!(g.frame(&json!({"error": "Stopped."})), Some(Gather::Dropped("no reply")));
        assert_eq!(g.frame(&json!({"done": true})), None, "an error ends it with nothing");
        g.turn(true);
        assert_eq!(g.frame(&json!({"done": true})), Some(Gather::Dropped("nothing gathered")));
        g.turn(true);
        g.frame(&json!({"delta": "Old turn."}));
        assert!(g.turn(true), "a turn drops the one being gathered");
        g.frame(&json!({"delta": "New turn."}));
        assert_eq!(g.frame(&json!({"done": true})), Some(Gather::Done("New turn.".into())), "a new turn drops the one being gathered");
    }

    #[test]
    fn review_r3_conversation_streaming_keeps_split_decimals_and_numbered_items_whole() {
        for (first, rest, whole) in [
            ("It is 3.", "5 degrees.", "It is 3.5 degrees."),
            ("Steps:\n1.", " Open it.", "Steps: Open it."),
        ] {
            let g = ReplyText::new(); g.turn_stream(true, true);
            let prefix = if first.starts_with("Steps:") { Some(Gather::Chunk(vec!["Steps:".into()])) } else { None };
            assert_eq!(g.frame(&json!({"delta":first})), prefix, "a clause may stream; a partial number may not: {first}");
            assert_eq!(g.frame(&json!({"delta":rest})), None);
            let tail = whole.strip_prefix("Steps: ").unwrap_or(whole);
            assert_eq!(g.frame(&json!({"done":true})), Some(Gather::StreamDone(vec![tail.into()])));
        }
    }

    #[test]
    fn conversation_streams_sentences_once_and_flushes_the_tail_on_done() {
        let g = ReplyText::new(); g.turn_stream(true, true);
        assert_eq!(g.frame(&json!({"response_id":"dialogue", "delta":"**Hello!** 😊"})), None);
        assert_eq!(g.frame(&json!({"response_id":"other", "delta":"Wrong."})), None);
        assert_eq!(g.frame(&json!({"response_id":"dialogue", "delta":" Next sentence. Final tail"})), Some(Gather::Chunk(vec!["Hello!".into(), "Next sentence.".into()])));
        assert_eq!(g.frame(&json!({"response_id":"dialogue", "done":true})), Some(Gather::StreamDone(vec!["Final tail".into()])));
        assert_eq!(g.frame(&json!({"response_id":"dialogue", "done":true})), None);
    }

    #[test]
    fn conversation_replacement_restarts_gathering_and_discards_old_sentences() {
        let g = ReplyText::new(); g.turn_stream(true, true);
        assert_eq!(g.frame(&json!({"delta":"Old sentence. Tail"})), Some(Gather::Chunk(vec!["Old sentence.".into()])));
        assert_eq!(g.frame(&json!({"replace":"New sentence. Tail"})), Some(Gather::Restart(Some("New sentence. Tail".into()))));

        lock(&g.0).as_mut().unwrap().sent = 1;
        assert_eq!(g.frame(&json!({"delta":" ends."})), None);
        assert_eq!(g.frame(&json!({"done":true})), Some(Gather::StreamDone(vec!["Tail ends.".into()])));
    }

    fn replying_voice(root: &Path, lf: &Lf, speaker: &'static Speaker) -> Voice<'static> {
        let v = fleet_voice(root, lf, speaker);
        lock(v.probed).done = true;
        v
    }
    fn to_said(e: &Value) {
        lock(&SAID).push(e.clone())
    }

    fn front_ticket(id:&str) -> (crate::voice_front::runtime::Epoch,crate::voice_front::runtime::Ticket) {
        let e=crate::voice_front::runtime::Epoch {capture:70,turn:90};
        (e,crate::voice_front::runtime::Ticket::new(e,id.into()))
    }
    #[test]
    fn refused_ordinary_chunk_cannot_leave_front_reply_hold_owned() {
        const TEST: &str = "voice::tests::refused_ordinary_chunk_cannot_leave_front_reply_hold_owned";
        if std::env::var_os("RTH_FRONT_OWNER_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--test-threads", "4"])
                .env("RTH_FRONT_OWNER_CHILD", "1").output().unwrap();
            assert!(child.status.success(), "{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
            return;
        }
        within(20, || {
            let root = scratch(); let _gone = Gone(root.clone());
            let held=Arc::new((Mutex::new(false),std::sync::Condvar::new()));
            let gate=held.clone();
            let lf = localfleet(move |_| Says::Held(0.4, 2000, gate.clone()));
            let speaker = quiet(); let v = replying_voice(&root, &lf, speaker);
            v.act("say_set", &json!({"mode":"full"})).unwrap();
            crate::doubletalk::reset();
            let (e, ticket) = front_ticket("owner-front");
            v.speak_front(crate::voice_front::runtime::SpeechRequest {
                ticket: ticket.clone(), text:"I am listening.".into(),
                deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)
            }, e, &Mutex::new(()), &||true, to_said).unwrap();
            let refused=v.speak_reply("Stale ordinary chunk.", false, false, &Mutex::new(()), &||true, to_said, None);
            speaker.stop_front(&ticket);
            *lock(&held.0)=true; held.1.notify_all();
            assert_eq!(refused,Err("new turn"));
            assert!(!crate::doubletalk::reply_active(), "front terminal must retire its hold despite a refused ordinary chunk");
            assert!(!crate::doubletalk::playout_held());
            crate::doubletalk::reset();
        });
    }
    #[test]
    fn reconnect_with_old_capture_reader_uses_audible_legacy_until_reopen() {
        const TEST: &str="voice::tests::reconnect_with_old_capture_reader_uses_audible_legacy_until_reopen";
        if std::env::var_os("RTH_FRONT_RECONNECT_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact",TEST,"--test-threads","4"])
                .env("RTH_FRONT_RECONNECT_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        within(20,|| {
            let selection=crate::voice_front::policy::settings_locale(Some("en"),crate::voice_front::policy::LocaleSource::Setting).unwrap();
            let mut attached=crate::front_native::Attachment::default();
            attached.bind("before-reconnect");
            let previous=attached.capture();let weak=Arc::downgrade(&previous);
            let reader=previous.reader();reader.activate();drop(previous);
            assert!(attached.admit_capture("before-reconnect",selection));
            attached.bind("after-reconnect");
            let route=attached.capture();
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Sine(0.2,300.0));
            let out=Box::leak(Box::<FakeOut>::default());let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);
            v.act("say_set",&json!({"mode":"full"})).unwrap();

            assert!(!attached.admit_capture("after-reconnect",selection));
            assert_eq!(route.metrics()["legacy_capture_fallbacks"],1);
            let reply=v.speak_reply("The legacy reply is spoken.",false,false,&Mutex::new(()),&||true,to_said,None).unwrap();
            until("legacy reply after reconnect ends",||ended(&reply).is_some());
            assert!(!lock(&out.heard).is_empty(),"reconnect must use legacy speech, never silence");
            assert!(route.retained()==0,"no front ticket published without its new capture reader");
            reader.fence();assert_eq!(route.revision(),0,"old callback cannot fence new session");
            drop(reader);assert!(weak.upgrade().is_none(),"retired arena must not leak per rotation");
            let reopened=route.reader();reopened.activate();
            assert!(attached.admit_capture("after-reconnect",selection));
            assert_eq!(route.metrics()["legacy_capture_fallbacks"],1,"reopen stops counting capture fallbacks");
            speaker.stop();crate::doubletalk::reset();
        });
    }
    #[test]
    fn outer_typed_admission_stops_front_before_busy_refusal_or_ordinary_speech() {
        const TEST:&str="voice::tests::outer_typed_admission_stops_front_before_busy_refusal_or_ordinary_speech";
        if std::env::var_os("RTH_FRONT_TYPED_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",TEST,"--test-threads","4"])
                .env("RTH_FRONT_TYPED_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        within(20,|| crate::conversation::review_busy_front(|epoch| {
            let mut attached=crate::front_native::Attachment::default();attached.bind("typed-front");attached.accepted(epoch);
            let root=scratch();let _gone=Gone(root.clone());
            let lf=localfleet(|_|Says::Sine(1.0,300.0));
            let out=Box::leak(Box::new(FakeOut{stall_after:Some(480),..FakeOut::default()}));
            let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);v.act("say_set",&json!({"mode":"full"})).unwrap();
            let ticket=crate::voice_front::runtime::Ticket::new(epoch,"before-typed".into());
            attached.capture().publish(&ticket,0).unwrap();
            v.speak_front(crate::voice_front::runtime::SpeechRequest{ticket:ticket.clone(),text:"Older fixed line.".into(),deadline:Instant::now()+Duration::from_secs(5)},epoch,&Mutex::new(()),&||true,to_said).unwrap();
            until("front has rendered before typed interrupt",||ticket.frames()>0);
            let typed=crate::conversation::typed("Newer typed input.");
            let closed_at_admission=ticket.closed();
            let replies=Replies::new();replies.turn(false,true,speaker);
            let terminal=ended(ticket.response()).unwrap();
            assert_eq!(typed,Ok(true),"outer typed admission must supersede the native owner before its busy check");
            assert!(closed_at_admission,"outer typed admission fences the old ticket before accepting input");
            let closed=ticket.closed();let next=v.speak_reply("Newer typed answer.",false,false,&Mutex::new(()),&||true,to_said,None);
            speaker.stop();
            assert!(terminal["stopped"]==true && terminal["front"]["frames"].as_u64().unwrap()>0,"rendered old front ends as Interrupted");
            assert!(closed,"typed ownership must stop the playing front first");assert!(next.is_ok(),"typed speech must not wait for the old fixed line");
        }));
    }
    #[test] fn front_selected_voice_pitch_and_mood_have_exact_output_sample_spans() {
        within(20, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Sine(0.4,300.0));
            let out=Box::leak(Box::<FakeOut>::default());let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);
            v.act("fleet_choose",&json!({"voice_id":"unfold-pt","mood":"joy"})).unwrap();
            v.act("say_set",&json!({"mode":"first_sentence"})).unwrap();
            let text="I am listening. Estou ouvindo.";let (e,t)=front_ticket("front-selected");
            assert_eq!(v.speak_front(crate::voice_front::runtime::SpeechRequest {ticket:t.clone(),text:text.into(),deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)},e,&Mutex::new(()),&||true,to_said).unwrap(),t.response());
            until("front complete",||ended(t.response()).is_some());let end=ended(t.response()).unwrap();
            assert_eq!(end["state"],"done");assert_eq!(end["rendered_text"],text);
            let n=lock(&out.heard).len() as u64;assert_eq!(t.frames(),n);assert!((19000..=19300).contains(&n));
            assert_eq!(end["front"]["frames"],n);assert_eq!(end["front"]["epoch"],json!(e));
            assert_eq!(end["front"]["response"],t.response());
            assert_eq!(end["front"]["spans"],json!([{"start":0,"end":n,"text":text}]));
            assert!(end["front"]["first_pcm_ns"].is_u64() && end["front"]["first_render_ns"].is_u64());
            let body=lock(&lf.bodies)[0].clone();assert_eq!(body["input"],text);assert_eq!(body["voice"],"unfold-pt");assert_eq!(body["mood"],"joy");
            assert!(body.get("language").is_none() && body.get("pitch_semitones").is_none());
            let got=pitch_hz(&lock(&out.heard),48000.0)/300.0;assert!((got/2f64.powf(-3.0/12.0)-1.0).abs()<0.03);

            let ordinary=v.speak_reply("Ordinary reply.",false,false,&Mutex::new(()),&||true,to_said,None).unwrap();
            until("ordinary complete",||ended(&ordinary).is_some());assert_eq!(t.frames(),n);assert_eq!(out.opens.load(Ordering::SeqCst),2);
        });
    }
    #[test] fn front_busy_is_one_attempt_and_no_certified_text_or_samples() {
        within(10, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Busy(0));
            let out=Box::leak(Box::<FakeOut>::default());let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);
            let (e,t)=front_ticket("front-busy");
            v.speak_front(crate::voice_front::runtime::SpeechRequest {ticket:t.clone(),text:"Estou ouvindo.".into(),deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)},e,&Mutex::new(()),&||true,to_said).unwrap();
            until("front busy terminal",||ended(t.response()).is_some());let end=ended(t.response()).unwrap();
            assert_eq!(end["error"]["code"],"busy");assert_eq!(lock(&lf.bodies).len(),1);assert_eq!(t.frames(),0);
            assert_eq!(end["rendered_text"],"");assert_eq!(end["front"]["spans"],json!([]));assert!(lock(&out.heard).is_empty());
            assert!(!t.cancelled(),"failure must not become a user cancellation");
            assert!(!t.fence(),"failed front must still permit zero-frame legacy failover");
        });
    }
    #[test] fn front_truncated_audio_certifies_no_complete_text() {
        within(10, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Truncated(0.2,2000));
            let out=Box::leak(Box::<FakeOut>::default());let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);
            let (e,t)=front_ticket("front-truncated");
            v.speak_front(crate::voice_front::runtime::SpeechRequest {ticket:t.clone(),text:"The unheard tail is not complete.".into(),deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)},e,&Mutex::new(()),&||true,to_said).unwrap();
            until("truncated front terminal",||ended(t.response()).is_some());let end=ended(t.response()).unwrap();
            assert!(t.frames()>0);assert_eq!(end["truncated"],true);assert_eq!(end["rendered_text"],"");assert_eq!(end["front"]["spans"],json!([]));
        });
    }
    #[test] fn front_guard_veto_flushes_automatically_and_stop_targets_only_its_ticket() {
        within(15, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Paced(2.0,2000));
            let out=Box::leak(Box::<FakeOut>::default());let speaker=speaker_on(out);let v=replying_voice(&root,&lf,speaker);
            let (e,t)=front_ticket("front-cancel");
            v.speak_front(crate::voice_front::runtime::SpeechRequest {ticket:t.clone(),text:"Only completed text may enter history.".into(),deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)},e,&Mutex::new(()),&||true,to_said).unwrap();
            until("front first real frames",||t.frames()>0);
            let (_,foreign)=front_ticket("foreign");speaker.stop_front(&foreign);assert!(speaker.busy());
            t.fence();let n=t.frames();until("veto worker terminal",||ended(t.response()).is_some());
            assert_eq!(speaker.queued(),0);assert_eq!(t.frames(),n);let end=ended(t.response()).unwrap();
            assert_eq!(end["stopped"],true);assert_eq!(end["rendered_text"],"");assert_eq!(end["front"]["spans"],json!([]));
            assert_eq!(said(t.response()).iter().filter(|v|v["state"]!="streaming").count(),1);
            let (e,next)=front_ticket("front-explicit-stop");
            v.speak_front(crate::voice_front::runtime::SpeechRequest {ticket:next.clone(),text:"I am listening.".into(),deadline:std::time::Instant::now()+std::time::Duration::from_secs(5)},e,&Mutex::new(()),&||true,to_said).unwrap();
            until("next first frames",||next.frames()>0);next.fence();speaker.stop_front(&next);
            assert_eq!(ended(next.response()).unwrap()["stopped"],true);assert_eq!(speaker.queued(),0);
        });
    }
    #[test]
    fn a_spoken_reply_goes_to_localfleet_in_the_chosen_voice_and_mood_naming_no_language() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = replying_voice(&root, &lf, speaker_on(out));

            let p = v.speak_reply("This is the answer. It has two sentences.", false, true, &Mutex::new(()), &|| true, to_said, None).expect("spoken");
            until("the speech ends", || ended(&p).is_some());
            assert_eq!(ended(&p).unwrap()["state"], json!("done"), "{:?}", said(&p));
            assert_eq!(lock(&lf.bodies)[0], json!({"model": "qwen3-tts-1.7b-base", "input": "This is the answer. It has two sentences.", "voice": "unfold-pt", "mood": "calm", "stream": true, "stream_format": "events"}), "no language key: a named one lowers the voice");
            until("the device got it", || !lock(&out.heard).is_empty());

            v.act("fleet_choose", &json!({"voice_id": "studio-b"})).unwrap();
            v.act("say_set", &json!({"mode": "first_sentence"})).unwrap();
            let p = v.speak_reply("Você não está sozinho. Isso é bom.", false, false, &Mutex::new(()), &|| true, to_said, None).expect("spoken");
            until("the second speech ends", || ended(&p).is_some());
            let b = lock(&lf.bodies)[1].clone();
            assert_eq!((b["voice"].clone(), b["mood"].clone(), b["input"].clone(), b.get("language")), (json!("studio-b"), json!("neutral"), json!("Você não está sozinho."), None));

            assert_eq!(v.speak_reply("Olá.", false, true, &Mutex::new(()), &|| false, to_said, None), Err("new turn"));
            assert_eq!(lock(&lf.bodies).len(), 2);

            v.act("say_set", &json!({"mode": "off"})).unwrap();
            assert_eq!(v.speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None), Err("voice off"));
            assert_eq!(lock(&lf.bodies).len(), 2);
        });
    }

    #[test]
    fn with_no_listed_voice_or_a_busy_localfleet_a_reply_is_not_spoken_and_nothing_plays() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Busy(0));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = replying_voice(&root, &lf, speaker_on(out));
            let p = v.speak_reply("Olá! Tudo bem?", false, true, &Mutex::new(()), &|| true, to_said, None).expect("asked");
            until("the speech ends", || ended(&p).is_some());
            let end = ended(&p).unwrap();
            assert_eq!((end["state"].clone(), end["error"]["code"].clone(), end["retries"].clone()), (json!("failed"), json!("busy"), json!(3)), "asked again three times: {end}");
            assert!(lock(&out.heard).is_empty());
            lock(v.probed).voices.clear();
            let v = Voice { backend: Box::leak(Box::new(Answers(Ok(json!({"devices": fleet()["devices"], "voices": []}))))), ..v };
            assert_eq!(v.speak_reply("Olá! Tudo bem?", false, true, &Mutex::new(()), &|| true, to_said, None), Err("no voice"), "LocalFleet lists no voice");
            assert_eq!(lock(&lf.bodies).len(), 4);
        });
    }

    #[test]
    fn a_second_reply_stops_the_speech_still_playing() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(3.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = replying_voice(&root, &lf, speaker_on(out));
            let first = v.speak_reply("The first reply.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("the first plays", || !lock(&out.heard).is_empty());
            let second = v.speak_reply("The second reply.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            assert!(ended(&first).is_some(), "the first ended when the second began");
            until("the second ends", || ended(&second).is_some());
            assert_eq!(ended(&second).unwrap()["state"], json!("done"));
        });
    }

    #[test]
    fn a_second_spoken_turn_after_the_first_ended_is_said_too() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|n| Says::Tone(0.3, if n == 0 { 2000 } else { 3000 }));
            let out: &'static FakeOut = Box::leak(Box::default());
            let v = replying_voice(&root, &lf, speaker_on(out));
            let first = v.speak_reply("The first reply.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("the first ends", || ended(&first).is_some());

            until("the output closes", || out.streams.load(Ordering::SeqCst) == 0);
            let second = v.speak_reply("The second reply.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("the second ends", || ended(&second).is_some());
            assert_eq!(ended(&second).unwrap()["state"], json!("done"), "{:?}", said(&second));
            let heard = lock(&out.heard).iter().filter(|s| near(**s, 3000)).count();
            assert!(heard > 0, "the second reply played nothing: {:?}", said(&second));
        });
    }

    macro_rules! replies {
        () => {{
            static R: Replies = Replies::new();
            static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
            static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
            fn emit(e: &Value) {
                to_said(e);
                lock(&EVENTS).push(e.clone());
                if let Some(l) = R.event(e) {
                    lock(&LINES).push(l);
                }
            }
            fn log(l: String) {
                lock(&LINES).push(l)
            }
            (&R, emit as fn(&Value), log as fn(String), &LINES, &EVENTS)
        }};
    }

    fn voices_of(root: &Path, backend: &'static (dyn Backend + Sync), probed: &'static Mutex<Probed>, speaker: &'static Speaker) -> impl Fn() -> Voice<'static> + Clone + Send + 'static {
        let root = root.to_path_buf();
        let (mic, studio): (&'static FakeMic, &'static Studio) = (Box::leak(Box::new(FakeMic::granted(0.0))), Box::leak(Box::default()));
        move || Voice { root: root.clone(), backend, env_key: None, keys: &NoKeys, probed, mic, studio, speaker, emit: to_said }
    }

    fn listed(root: &Path, lf: &Lf, mode: &str) -> &'static Mutex<Probed> {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": mode, "node_url": lf.url}).to_string()).unwrap();
        Box::leak(Box::new(Mutex::new(Probed { voices: page_voices(&stored()), done: true, ..Probed::default() })))
    }
    fn delta(t: &str) -> Value {
        json!({"delta": t, "response_id": "r1"})
    }
    fn heard(out: &FakeOut, value: i16) -> usize {
        lock(&out.heard).iter().filter(|s| near(**s, value)).count()
    }

    #[test]
    fn a_spoken_reply_starts_at_its_first_sentence_and_its_rest_follows_in_the_same_voice() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());

            let lf = localfleet(|n| if n == 0 { Says::Tone(0.1, 2000) } else { Says::Tone(0.3, 3000) });
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, events) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            assert!(r.turn(true, true, speaker).is_empty());
            r.frame(&delta("Primeira frase aqui. "), true, voice.clone(), emit, log);
            assert!(lock(&lf.bodies).is_empty(), "a sentence still forming isn't said");
            r.frame(&delta("Segunda"), true, voice.clone(), emit, log);

            until("the first sentence plays", || heard(out, 2000) > 0);
            assert_eq!(lock(&lf.bodies)[0]["input"], json!("Primeira frase aqui."));

            until("its progress reaches the page while the rest is awaited", || lock(events).iter().filter(|e| e["state"] == "streaming").count() >= 3);

            until("the whole first sentence plays", || lock(&out.heard).len() >= 4_790);
            r.frame(&delta(" frase. Terceira."), true, voice.clone(), emit, log);
            r.frame(&json!({"done": true, "response_id": "r1"}), true, voice.clone(), emit, log);
            until("the rest plays", || heard(out, 3000) > 0);
            let b = lock(&lf.bodies).clone();
            assert_eq!((b.len(), b[1]["input"].clone(), b[1]["voice"].clone(), b[1].get("language")), (2, json!("Segunda frase. Terceira."), b[0]["voice"].clone(), None));
            until("its line", || !lock(lines).is_empty());
            let l = lock(lines).clone();
            assert!(l.len() == 1 && l[0].starts_with("voice: turn trigger dictated, voice chosen yes -> spoken (speech asked ") && l[0].contains(" ms after the turn, first audio "), "{l:?}");
            assert!(!l[0].contains("frase"), "no words in the log");
        });
    }

    #[test]
    fn a_replaced_reply_stops_what_was_said_of_it_and_says_only_the_new_text() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|n| if n == 0 { Says::Tone(3.0, 2000) } else { Says::Tone(0.3, 3000) });
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _events) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            assert!(r.turn(true, true, speaker).is_empty());
            r.frame(&delta("Ana buys seven pens. So she has "), true, voice.clone(), emit, log);
            until("the reasoning's first sentence plays", || heard(out, 2000) > 0);
            r.frame(&json!({"replace": "4", "response_id": "r1"}), true, voice.clone(), emit, log);
            r.frame(&json!({"done": true, "response_id": "r1"}), true, voice.clone(), emit, log);
            until("the replacement plays", || heard(out, 3000) > 0);
            let b = lock(&lf.bodies).clone();
            assert_eq!((b.len(), b[1]["input"].clone()), (2, json!("4")));
            assert!(heard(out, 2000) < 96_000, "the reasoning's speech stopped before its 3 s");
            until("its line", || !lock(lines).is_empty());
            let l = lock(lines).clone();
            assert!(l.len() == 1 && l[0].starts_with("voice: turn trigger dictated, voice chosen yes -> spoken"), "{l:?}");
        });
    }

    #[test]
    fn a_reply_probes_again_when_the_last_probe_listed_no_voice() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let probed = listed(&root, &lf, "full");
            lock(probed).voices.clear();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Ok(json!({"devices": fleet()["devices"], "voices": stored()["voices"]}))))), probed, speaker_on(out));
            let p = voice().speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None).expect("the probe lists the voice again");
            until("the speech ends", || ended(&p).is_some());
            assert_eq!(lock(&lf.bodies)[0]["voice"], json!("unfold-pt"));
        });
    }

    #[test]
    fn a_reply_with_no_output_device_logs_its_failure() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::new(FakeOut { broken: true, ..FakeOut::default() }));
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            r.turn(true, true, speaker);
            r.frame(&delta("Olá."), false, voice.clone(), emit, log);
            r.frame(&json!({"done": true, "response_id": "r1"}), false, voice, emit, log);

            until("its line", || !lock(lines).is_empty());
            assert_eq!(lock(lines)[0], "voice: turn trigger dictated, voice chosen yes -> failed(device_unavailable) (waited warm-up 0 ms, retries 0)");
        });
    }

    #[test]
    fn a_dictation_drops_the_words_still_gathered() {
        let (r, _, _, _, _) = replies!();
        r.turn(true, true, quiet());
        assert_eq!(r.text.frame(&delta("Still ")), None);
        r.dictation(quiet());
        assert_eq!(r.text.frame(&json!({"done": true, "response_id": "r1"})), None, "nothing kept of it");
    }

    #[test]
    fn a_typed_turn_is_said_only_when_its_reply_spoken_comes() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            let reply = |id: &str, spoken: Option<&str>| {
                if let Some(event) = spoken {
                    r.frame(&json!({"wire": "reply_spoken", "response_id": event}), false, voice.clone(), emit, log);
                }
                r.frame(&json!({"delta": "Esta é a minha voz.", "response_id": id}), false, voice.clone(), emit, log);
                r.frame(&json!({"done": true, "response_id": id}), false, voice.clone(), emit, log);
            };

            r.turn(false, true, speaker).into_iter().for_each(log);
            reply("r1", None);
            until("its line", || lock(lines).len() == 1);
            assert!(lock(&lf.bodies).is_empty());

            r.turn(false, true, speaker).into_iter().for_each(log);
            reply("r2", Some("r2"));
            until("its line", || lock(lines).len() == 2);
            assert_eq!(lock(&lf.bodies)[0]["input"], json!("Esta é a minha voz."));

            r.turn(false, true, speaker).into_iter().for_each(log);
            reply("r3", Some("r2"));
            until("its line", || lock(lines).len() == 3);
            assert_eq!(lock(&lf.bodies).len(), 1);
            let l = lock(lines).clone();
            assert_eq!(l[0], "voice: turn trigger none, voice chosen yes -> skipped(typed)");
            assert!(l[1].starts_with("voice: turn trigger reply_spoken, voice chosen yes -> spoken ("), "{l:?}");
            assert_eq!(l[2], "voice: turn trigger none, voice chosen yes -> skipped(typed)");
        });
    }

    #[test]
    fn a_turn_carries_reply_voice_only_while_a_voice_can_say_its_reply() {
        let root = scratch();
        let _gone = Gone(root.clone());
        let lf = localfleet(|_| Says::Tone(0.1, 2000));
        let probed = listed(&root, &lf, "full");
        let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), probed, quiet());
        assert_eq!(voice().reply_voice(true), Some(json!({"dictated": true, "spoken": "full"})));
        let mode = |m: &str| std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": m, "node_url": lf.url}).to_string()).unwrap();
        mode("summary");
        assert_eq!(voice().reply_voice(false), Some(json!({"dictated": false, "spoken": "first_sentence"})));
        mode("off");
        assert_eq!(voice().reply_voice(true), None, "Never speak");
        mode("full");
        lock(probed).voices.clear();
        assert_eq!(voice().reply_voice(true), None, "no voice listed");
    }

    #[test]
    fn the_switch_is_off_until_turned_on_and_stays_as_set_across_a_restart() {
        let root = scratch();
        let _gone = Gone(root.clone());
        let lf = localfleet(|_| Says::Tone(0.1, 2000));
        let none: &'static Answers = Box::leak(Box::new(Answers(Err("no probe".into()))));
        let voice = voices_of(&root, none, listed(&root, &lf, "full"), quiet());
        let on = |v: &Voice| v.act("voice_stop", &json!({})).unwrap()["voice"]["on"].clone();

        std::fs::remove_file(root.join("prefs.json")).unwrap();
        assert_eq!(on(&voice()), json!(false), "a new install");
        std::fs::write(root.join("prefs.json"), json!({"say_mode": "full", "node_url": lf.url, "fleet": {"voice_id": "studio-b", "mood": "neutral"}}).to_string()).unwrap();
        assert_eq!(on(&voice()), json!(false), "0.4.11's prefs, a voice chosen");

        voice().act("fleet_choose", &json!({"voice_id": "unfold-pt"})).unwrap();
        voice().act("say_set", &json!({"mode": "first_sentence"})).unwrap();
        assert_eq!(on(&voice()), json!(false));

        assert_eq!(voice().act("voice_on", &json!({"on": true})).unwrap()["voice"]["on"], json!(true));
        let relaunched = voices_of(&root, none, Box::leak(Box::default()), quiet());
        assert!(relaunched().on());
        relaunched().act("voice_on", &json!({"on": false})).unwrap();
        let again = voices_of(&root, none, Box::leak(Box::default()), quiet());
        assert!(!again().on());
        assert_eq!(voice().act("voice_on", &json!({})), Err(Refusal::Bad("voice_on: on must be true or false".into())));
    }

    #[test]
    fn turning_it_on_makes_never_speak_the_short_summary_and_picks_the_first_voice_when_none_shows_chosen() {
        let root = scratch();
        let _gone = Gone(root.clone());
        let lf = localfleet(|_| Says::Tone(0.1, 2000));
        let none: &'static Answers = Box::leak(Box::new(Answers(Err("no probe".into()))));

        let probed = listed(&root, &lf, "off");
        std::fs::write(root.join("prefs.json"), json!({"say_mode": "off", "node_url": lf.url}).to_string()).unwrap();
        lock(probed).voices.retain(|v| v["id"] != FLEET_DEFAULT);
        let voice = voices_of(&root, none, probed, quiet());
        assert_eq!(voice().act("voice_stop", &json!({})).unwrap()["voice"]["fleet_chosen"], Value::Null);
        let v = voice().act("voice_on", &json!({"on": true})).unwrap()["voice"].clone();
        let first = lock(probed).voices[0]["id"].clone();
        assert_eq!((v["on"].clone(), v["say_mode"].clone(), v["fleet_chosen"]["voice_id"].clone(), v["fleet_chosen"]["default"].clone()), (json!(true), json!("summary"), first, json!(false)));

        assert_eq!(voice().act("say_set", &json!({"mode": "off"})).unwrap()["voice"]["on"], json!(false));

        let root2 = scratch();
        let _gone2 = Gone(root2.clone());
        let probed2 = listed(&root2, &lf, "full");
        std::fs::write(root2.join("prefs.json"), json!({"say_mode": "full", "node_url": lf.url}).to_string()).unwrap();
        let voice2 = voices_of(&root2, none, probed2, quiet());
        let v = voice2().act("voice_on", &json!({"on": true})).unwrap()["voice"].clone();
        assert_eq!((v["say_mode"].clone(), v["fleet_chosen"].clone()), (json!("full"), json!({"voice_id": "unfold-pt", "mood": "calm", "default": true})));
        let saved: Value = serde_json::from_slice(&std::fs::read(root2.join("prefs.json")).unwrap()).unwrap();
        assert_eq!(saved["fleet"], Value::Null, "the default isn't saved as a choice");
    }

    #[test]
    fn turning_it_off_stops_the_speech_playing() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());

            let lf = localfleet(|_| Says::Paced(20.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            voice().speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("it plays", || heard(out, 2000) > 0);
            let t = std::time::Instant::now();
            voice().act("voice_on", &json!({"on": false})).unwrap();
            until("it stopped", || !speaker.busy());
            assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());

            voice().act("voice_on", &json!({"on": true})).unwrap();
            let before = heard(out, 2000);
            voice().speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("it plays again", || heard(out, 2000) > before);
            let t = std::time::Instant::now();
            voice().act("say_set", &json!({"mode": "off"})).unwrap();
            until("it stopped again", || !speaker.busy());
            assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        });
    }

    #[test]
    fn with_the_switch_off_no_turn_carries_reply_voice_and_nothing_is_said() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let probed = listed(&root, &lf, "full");
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), probed, speaker);
            voice().act("voice_on", &json!({"on": false})).unwrap();
            assert_eq!((voice().reply_voice(true), voice().reply_voice(false)), (None, None));
            assert_eq!(r.warm_up(&voice(), true), "voice: warm-up skipped(voice off)");

            let listed_voices = std::mem::take(&mut lock(probed).voices);
            voice().warm();
            assert!(!lock(probed).unreachable, "no probe while off");
            lock(probed).voices = listed_voices;
            let reply = |id: &str, spoken: Option<&str>| {
                if let Some(event) = spoken {
                    r.frame(&json!({"wire": "reply_spoken", "response_id": event}), false, voice.clone(), emit, log);
                }
                r.frame(&json!({"delta": "Esta é a minha voz.", "response_id": id}), false, voice.clone(), emit, log);
                r.frame(&json!({"done": true, "response_id": id}), false, voice.clone(), emit, log);
            };

            r.turn(true, voice().reply_voice(true).is_some(), speaker).into_iter().for_each(log);
            reply("r1", None);
            until("its line", || lock(lines).len() == 1);
            r.turn(false, voice().reply_voice(false).is_some(), speaker).into_iter().for_each(log);
            reply("r2", Some("r2"));
            until("its line", || lock(lines).len() == 2);
            assert!(lock(&lf.bodies).is_empty(), "nothing went to LocalFleet");
            assert_eq!(heard(out, 2000), 0);
            assert_eq!(lock(lines).clone(), ["voice: turn trigger dictated, voice chosen no -> skipped(voice off) (waited warm-up 0 ms, retries 0)", "voice: turn trigger none, voice chosen no -> skipped(typed)"]);

            voice().act("voice_on", &json!({"on": true})).unwrap();
            r.turn(true, voice().reply_voice(true).is_some(), speaker).into_iter().for_each(log);
            reply("r3", None);
            until("its line", || lock(lines).len() == 3);
            assert_eq!(lock(&lf.bodies).len(), 1);
            assert!(lock(lines)[2].starts_with("voice: turn trigger dictated, voice chosen yes -> spoken ("), "{:?}", lock(lines));
        });
    }

    #[test]
    fn a_warm_up_is_one_short_capped_request_and_never_while_speech_plays() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            let held = Arc::clone(&gate);
            let lf = localfleet(move |n| if n == 0 { Says::Tone(0.1, 2000) } else { Says::Held(0.5, 2000, Arc::clone(&held)) });
            let out: &'static FakeOut = Box::leak(Box::default());
            let probed = listed(&root, &lf, "full");
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), probed, speaker_on(out));
            let (r, _, _, _, _) = replies!();
            let v = voice();
            let line = r.warm_up(&v, true);
            assert!(line.starts_with("voice: warm-up sent -> ok ") && line.ends_with(" ms"), "{line}");
            assert_eq!(lock(&lf.bodies)[0], json!({"model": "qwen3-tts-1.7b-base", "input": "Oi.", "voice": "unfold-pt", "mood": "calm", "max_new_tokens": 13, "stream": false}), "a real word, capped, no language");
            assert_eq!(r.warm_up(&v, true), "voice: warm-up skipped(spoke recently)");
            assert!(heard(out, 2000) == 0, "its audio is dropped");

            let (r2, _, _, _, _) = replies!();
            let p = v.speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
            until("asked", || lock(&lf.bodies).len() == 2);
            assert_eq!(r2.warm_up(&v, true), "voice: warm-up skipped(speech playing)");
            v.speaker.stop();
            until("stopped", || ended(&p).is_some());
            lock(probed).voices.clear();
            assert_eq!(r2.warm_up(&v, true), "voice: warm-up skipped(no voice)");
            assert_eq!(lock(&lf.bodies).len(), 2);

            lock(probed).voices = page_voices(&stored());
            let (r3, _, _, _, _) = replies!();
            r3.turn(true, true, v.speaker);
            assert_eq!(r3.warm_up(&v, true), "voice: warm-up skipped(reply due)");
            assert_eq!(lock(&lf.bodies).len(), 2, "no request beside a reply due");
            *lock(&gate.0) = true;
            gate.1.notify_all();
        });
    }

    fn noted(line: &str, what: &str) -> Option<u64> {
        line.split(what).nth(1)?.trim_start().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
    }

    fn on_fleet(root: &Path, lf: &Lf) -> (impl Fn() -> Voice<'static> + Clone + Send + 'static, &'static Speaker, &'static FakeOut) {
        let out: &'static FakeOut = Box::leak(Box::default());
        let speaker = speaker_on(out);
        (voices_of(root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(root, lf, "full"), speaker), speaker, out)
    }

    fn dictated(r: &'static Replies, voice: &(impl Fn() -> Voice<'static> + Clone + Send + 'static), speaker: &'static Speaker, emit: fn(&Value), log: fn(String)) {
        r.turn(true, true, speaker).into_iter().for_each(log);
        r.frame(&delta("Olá."), true, voice.clone(), emit, log);
        r.frame(&json!({"done": true, "response_id": "r1"}), true, voice.clone(), emit, log);
    }

    #[test]
    fn a_reply_waits_for_the_warm_up_in_flight_and_is_said_without_being_turned_away() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());

            let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            let (held, turned_away) = (Arc::clone(&gate), Arc::new(AtomicUsize::new(0)));
            let away = Arc::clone(&turned_away);
            let lf = localfleet(move |n| match n {
                0 => Says::Held(0.1, 1000, Arc::clone(&held)),
                _ if !*lock(&held.0) => {
                    away.fetch_add(1, Ordering::SeqCst);
                    Says::Busy(1)
                }
                _ => Says::Tone(0.3, 2000),
            });
            let (voice, speaker, out) = on_fleet(&root, &lf);
            let (r, emit, log, lines, _) = replies!();

            let v = voice.clone();
            let warm = std::thread::spawn(move || r.warm_up(&v(), true));
            until("the warm-up asked", || lock(&lf.bodies).len() == 1);

            dictated(r, &voice, speaker, emit, log);
            std::thread::sleep(Duration::from_millis(600));
            assert_eq!(lock(&lf.bodies).len(), 1, "the reply waits for the warm-up");
            *lock(&gate.0) = true;
            gate.1.notify_all();
            let w = warm.join().unwrap();
            assert!(w.starts_with("voice: warm-up sent -> ok "), "{w}");
            until("the turn's line", || !lock(lines).is_empty());
            let l = lock(lines)[0].clone();
            assert!(l.starts_with("voice: turn trigger dictated, voice chosen yes -> spoken (speech asked "), "{l}");
            assert!(noted(&l, "waited warm-up").is_some_and(|ms| ms >= 500) && noted(&l, "retries") == Some(0), "{l}");
            assert_eq!(turned_away.load(Ordering::SeqCst), 0, "LocalFleet never turned it away");
            assert!(heard(out, 2000) > 0, "the reply played");
            assert_eq!(lock(&lf.bodies).len(), 2);
        });
    }

    #[test]
    fn a_reply_waits_ten_seconds_at_most_for_the_warm_up() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());

            let lf = localfleet(|n| if n == 0 { Says::Late(13_000, 0.1, 1000) } else { Says::Tone(0.3, 2000) });
            let (voice, speaker, out) = on_fleet(&root, &lf);
            let (r, emit, log, lines, _) = replies!();
            let v = voice.clone();
            std::thread::spawn(move || r.warm_up(&v(), true));
            until("the warm-up asked", || lock(&lf.bodies).len() == 1);
            let t = Instant::now();
            dictated(r, &voice, speaker, emit, log);
            while lock(&lf.bodies).len() < 2 {
                assert!(t.elapsed() < Duration::from_secs(12), "the reply was asked within the cap");
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(t.elapsed() >= Duration::from_millis(9_900), "asked after {:?}, before the cap", t.elapsed());
            until("the turn's line", || !lock(lines).is_empty());
            let l = lock(lines)[0].clone();
            assert!(l.contains(" -> spoken (") && noted(&l, "waited warm-up").is_some_and(|ms| (9_900..11_000).contains(&ms)), "{l}");
            assert!(heard(out, 2000) > 0, "the reply played");
        });
    }

    #[test]
    fn a_busy_localfleet_is_asked_again_after_its_retry_after_and_the_reply_is_said() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());

            let lf = localfleet(|n| match n {
                0 => Says::Busy(1),
                1 => Says::Unavailable(1),
                _ => Says::Tone(0.3, 2000),
            });
            let (voice, speaker, out) = on_fleet(&root, &lf);
            let (r, emit, log, lines, _) = replies!();
            let t = Instant::now();
            dictated(r, &voice, speaker, emit, log);
            until("asked three times", || lock(&lf.bodies).len() == 3);
            assert!(t.elapsed() >= Duration::from_millis(1_900), "each Retry-After waited: {:?}", t.elapsed());
            until("the turn's line", || !lock(lines).is_empty());
            let l = lock(lines)[0].clone();
            assert!(l.contains(" -> spoken (") && noted(&l, "retries") == Some(2), "{l}");
            assert!(heard(out, 2000) > 0, "the reply played");
            let b = lock(&lf.bodies).clone();
            assert!(b[0] == b[1] && b[1] == b[2], "the same request each time: {b:?}");
        });
    }

    #[test]
    fn a_busy_answer_waits_its_retry_after_two_seconds_without_one_and_five_at_most() {
        use crate::speaker::busy_wait;
        assert_eq!([busy_wait(Some(0)), busy_wait(Some(1)), busy_wait(None), busy_wait(Some(5)), busy_wait(Some(30))], [0, 1, 2, 5, 5].map(Duration::from_secs));
    }

    #[test]
    fn a_localfleet_busy_four_times_ends_failed_busy_after_three_retries() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|n| if n < 4 { Says::Busy(0) } else { Says::Tone(0.3, 2000) });
            let (voice, speaker, out) = on_fleet(&root, &lf);
            let (r, emit, log, lines, _) = replies!();
            dictated(r, &voice, speaker, emit, log);
            until("the turn's line", || !lock(lines).is_empty());
            assert_eq!(lock(lines)[0], "voice: turn trigger dictated, voice chosen yes -> failed(busy) (waited warm-up 0 ms, retries 3)");
            std::thread::sleep(Duration::from_millis(300));
            assert_eq!(lock(&lf.bodies).len(), 4, "asked once, then again three times, and no more");
            assert_eq!(heard(out, 2000), 0);
        });
    }

    #[test]
    fn a_new_dictation_or_stop_ends_the_wait_for_the_warm_up_or_a_retry() {
        within(60, || {
            type R = (&'static Replies, fn(&Value), fn(String), &'static Mutex<Vec<String>>, &'static Mutex<Vec<Value>>);

            let case = |(r, emit, log, lines, _): R, how: &str, warm_up: bool| {
                let root = scratch();
                let _gone = Gone(root.clone());
                let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
                let held = Arc::clone(&gate);
                let lf = localfleet(move |n| if warm_up && n == 0 { Says::Held(0.1, 1000, Arc::clone(&held)) } else { Says::Busy(1) });
                let (voice, speaker, _) = on_fleet(&root, &lf);
                let warm = warm_up.then(|| {
                    let v = voice.clone();
                    std::thread::spawn(move || r.warm_up(&v(), true))
                });
                if warm_up {
                    until("the warm-up asked", || lock(&lf.bodies).len() == 1);
                }
                dictated(r, &voice, speaker, emit, log);
                if warm_up {
                    std::thread::sleep(Duration::from_millis(300));
                } else {
                    until("asked again", || lock(&lf.bodies).len() == 2);
                    std::thread::sleep(Duration::from_millis(100));
                }
                match how {
                    "dictation" => r.dictation(speaker).into_iter().for_each(log),
                    "voice_stop" => drop(voice().act("voice_stop", &json!({})).unwrap()),
                    "voice_on" => drop(voice().act("voice_on", &json!({"on": false})).unwrap()),
                    _ => drop(voice().act("say_set", &json!({"mode": "off"})).unwrap()),
                }
                let (why, retried) = (if how == "dictation" { "new dictation" } else { "stopped" }, if warm_up { 0 } else { 1 });
                until("the turn's line", || !lock(lines).is_empty());
                let l = lock(lines)[0].clone();
                assert!(l.starts_with(&format!("voice: turn trigger dictated, voice chosen yes -> skipped({why}) (waited warm-up ")) && noted(&l, "retries") == Some(retried), "{l}");
                *lock(&gate.0) = true;
                gate.1.notify_all();
                if let Some(w) = warm {
                    w.join().unwrap();
                }
                std::thread::sleep(Duration::from_millis(1_500));
                assert_eq!(lock(&lf.bodies).len(), 1 + retried as usize, "nothing asked once it ended ({how}, warm-up {warm_up})");
                assert_eq!(lock(lines).len(), 1, "{:?}", lock(lines));
            };
            case(replies!(), "dictation", true);
            case(replies!(), "voice_stop", true);
            case(replies!(), "voice_on", true);
            case(replies!(), "dictation", false);
            case(replies!(), "voice_stop", false);
            case(replies!(), "voice_on", false);
            case(replies!(), "say_set", false);
        });
    }

    #[test]
    fn a_new_dictation_while_a_reply_plays_never_deadlocks_with_the_speakers_events() {
        static R: Replies = Replies::new();
        static ARMED: AtomicBool = AtomicBool::new(false);
        static HOLDING: AtomicBool = AtomicBool::new(false);
        static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

        fn emit(e: &Value) {
            if e["state"] == "streaming" && ARMED.swap(false, Ordering::SeqCst) {
                HOLDING.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(300));
            }
            if let Some(l) = R.event(e) {
                lock(&LINES).push(l);
            }
        }
        fn log(l: String) {
            lock(&LINES).push(l)
        }
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(3.0, 2000));
            let (voice, speaker, out) = on_fleet(&root, &lf);
            dictated(&R, &voice, speaker, emit, log);
            until("it plays", || heard(out, 2000) > 0);
            until("the turn heard it", || lock(&R.pending).as_ref().is_some_and(|p| p.heard));
            ARMED.store(true, Ordering::SeqCst);
            until("an event holds the speaker", || HOLDING.load(Ordering::SeqCst));
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || tx.send(R.dictation(speaker)).unwrap());

            let l = rx.recv_timeout(Duration::from_secs(3)).expect("the dictation came back: no deadlock between the turn and the speaker");
            assert!(l.len() == 1 && l[0].starts_with("voice: turn trigger dictated, voice chosen yes -> spoken (cut short, "), "{l:?}");
        });
    }

    #[test]
    fn turning_the_switch_off_or_never_speak_while_a_replys_probe_runs_says_nothing() {
        within(20, || {
            for (action, value, why) in [("voice_on", json!({"on": false}), "voice off"), ("say_set", json!({"mode": "off"}), "never speak")] {
                let root = scratch();
                let _gone = Gone(root.clone());
                let lf = localfleet(|_| Says::Tone(0.3, 2000));
                let out: &'static FakeOut = Box::leak(Box::default());
                let speaker = speaker_on(out);

                let probed = listed(&root, &lf, "full");
                lock(probed).done = false;
                let slow: &'static Slow = Box::leak(Box::new(Slow(Duration::from_millis(800), json!({"devices": fleet()["devices"], "voices": stored()["voices"]}))));
                let voice = voices_of(&root, slow, probed, speaker);
                let v = voice.clone();
                let reply = std::thread::spawn(move || v().speak_reply("Olá.", false, true, &Mutex::new(()), &|| true, to_said, None));
                std::thread::sleep(Duration::from_millis(300));
                voice().act(action, &value).unwrap();
                assert_eq!(reply.join().unwrap(), Err(why), "{action}");
                std::thread::sleep(Duration::from_millis(300));
                assert!(lock(&lf.bodies).is_empty() && heard(out, 2000) == 0, "nothing said once {action} turned it off");
            }
        });
    }

    #[test]
    fn first_sentence_mode_says_it_once_and_nothing_after() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.3, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "first_sentence"), speaker);
            r.turn(true, true, speaker);
            for f in [delta("One. "), delta("Two. Three."), json!({"done": true, "response_id": "r1"})] {
                r.frame(&f, false, voice.clone(), emit, log);
            }
            until("its line", || !lock(lines).is_empty());
            let b = lock(&lf.bodies).clone();
            assert_eq!((b.len(), b[0]["input"].clone()), (1, json!("One.")));
        });
    }

    #[test]
    fn two_dictated_turns_in_a_row_are_both_said_and_each_turn_logs_one_line() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|n| Says::Paced(1.0, [2000, 3000, 4000][n.min(2)]));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Err("no probe".into())))), listed(&root, &lf, "full"), speaker);
            let done = json!({"done": true, "response_id": "r1"});

            r.turn(true, true, speaker);
            for f in [delta("Hello there. "), delta("How are you?"), done.clone()] {
                r.frame(&f, false, voice.clone(), emit, log);
            }
            until("turn 1 plays", || heard(out, 2000) > 0);
            std::thread::sleep(Duration::from_millis(200));
            lock(lines).extend(r.dictation(speaker));

            r.turn(true, true, speaker);
            for f in [json!({"delta": "Yes I can. ", "response_id": "r2"}), json!({"delta": "Ask me.", "response_id": "r2"}), json!({"done": true, "response_id": "r2"})] {
                r.frame(&f, false, voice.clone(), emit, log);
            }
            until("turn 2 plays", || heard(out, 3000) > 0);
            until("turn 2 ends", || lock(lines).len() == 2);

            lock(lines).extend(r.turn(false, false, speaker));
            let l = lock(lines).clone();
            assert_eq!(l[0], "voice: turn trigger dictated, voice chosen yes -> spoken (cut short, waited warm-up 0 ms, retries 0)");
            assert!(l[1].starts_with("voice: turn trigger dictated, voice chosen yes -> spoken ("), "{l:?}");
            assert_eq!(l[2], "voice: turn trigger none, voice chosen no -> skipped(typed)");
            assert_eq!(l.len(), 3, "{l:?}");
        });
    }

    #[test]
    fn a_turn_whose_reply_isnt_said_logs_why() {
        within(30, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            let held = Arc::clone(&gate);

            let lf = localfleet(move |n| if n < 4 { Says::Busy(0) } else { Says::Held(0.5, 2000, Arc::clone(&held)) });
            let stored_voices = page_voices(&stored());
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let (r, emit, log, lines, _) = replies!();
            let probed = listed(&root, &lf, "full");
            let voice = voices_of(&root, Box::leak(Box::new(Answers(Ok(json!({"devices": fleet()["devices"], "voices": []}))))), probed, speaker);

            let n = std::sync::atomic::AtomicUsize::new(0);
            let id = || format!("r{}", n.fetch_add(1, Ordering::SeqCst));
            let said = |r: &'static Replies, words: &str| {
                r.turn(true, true, speaker).into_iter().for_each(log);
                let id = id();
                r.frame(&json!({"delta": words, "response_id": id}), false, voice.clone(), emit, log);
                r.frame(&json!({"done": true, "response_id": id}), false, voice.clone(), emit, log);
            };
            let next = |n: usize| until("the line", || lock(lines).len() == n);

            said(r, "Busy now.");
            next(1);

            said(r, "Held back.");
            until("asked", || lock(&lf.bodies).len() == 5);
            r.dictation(speaker).into_iter().for_each(log);
            next(2);

            r.turn(true, true, speaker).into_iter().for_each(log);
            r.frame(&json!({"error": "It could not answer."}), false, voice.clone(), emit, log);
            r.turn(true, true, speaker).into_iter().for_each(log);
            r.frame(&json!({"done": true, "response_id": id()}), false, voice.clone(), emit, log);
            r.turn(true, true, speaker).into_iter().for_each(log);
            r.unsent().into_iter().for_each(log);

            std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "off", "node_url": lf.url}).to_string()).unwrap();
            said(r, "Not said.");
            next(6);

            std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": lf.url}).to_string()).unwrap();
            lock(probed).voices.clear();
            said(r, "No voice.");
            next(7);

            lock(probed).voices = stored_voices;
            said(r, "Stopped in Settings.");
            until("asked", || lock(&lf.bodies).len() == 6);

            voice().act("voice_stop", &json!({})).unwrap();
            next(8);
            let l = lock(lines).clone();
            let ends: Vec<&str> = l.iter().map(|l| l.rsplit("-> ").next().unwrap()).collect();
            let (w, r0) = ("waited warm-up 0 ms", "retries 0");
            assert_eq!(ends, [format!("failed(busy) ({w}, retries 3)"), format!("skipped(new dictation) ({w}, {r0})"), "skipped(no reply)".into(), "skipped(nothing gathered)".into(), "skipped(not sent)".into(), format!("skipped(never speak) ({w}, {r0})"), format!("skipped(no voice) ({w}, {r0})"), format!("skipped(stopped) ({w}, {r0})")], "{l:?}");
            *lock(&gate.0) = true;
            gate.1.notify_all();
        });
    }

    #[test]
    fn where_a_spoken_replys_time_goes() {

        within(40, || {
            let run = |early: bool| -> u128 {
                let root = scratch();
                let _gone = Gone(root.clone());
                let lf = localfleet(|_| Says::Late(800, 0.5, 2000));
                let out: &'static FakeOut = Box::leak(Box::new(FakeOut { open_delay: Duration::from_millis(100), ..FakeOut::default() }));
                let speaker = speaker_on(out);
                let probed = listed(&root, &lf, "full");
                lock(probed).done = false;

                std::fs::write(root.join("prefs.json"), json!({"on": true, "say_mode": "full", "node_url": lf.url, "fleet": {"voice_id": "studio-b"}}).to_string()).unwrap();
                let slow: &'static Slow = Box::leak(Box::new(Slow(Duration::from_millis(1000), json!({"devices": fleet()["devices"], "voices": stored()["voices"]}))));
                let voice = voices_of(&root, slow, probed, speaker);
                let sentences = ["One sentence here. ", "Two here. ", "Three here. ", "Four here. ", "Five here."];
                let t0 = Instant::now();
                let asked = Arc::clone(&lf.bodies);
                let watch = std::thread::spawn(move || {
                    while lock(&asked).is_empty() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    t0.elapsed().as_millis()
                });
                let first_audio = std::thread::spawn(move || {
                    until("the first audio", || heard(out, 2000) > 0);
                    t0.elapsed().as_millis()
                });
                if early {

                    let warm = voice.clone();
                    std::thread::spawn(move || warm().warm());
                    std::thread::sleep(Duration::from_millis(1500));
                    let (r, emit, log, _, _) = replies!();
                    r.turn(true, true, speaker);
                    for t in sentences {
                        std::thread::sleep(Duration::from_millis(400));
                        r.frame(&delta(t), true, voice.clone(), emit, log);
                    }
                    std::thread::sleep(Duration::from_millis(400));
                    r.frame(&json!({"done": true, "response_id": "r1"}), true, voice.clone(), emit, log);
                } else {

                    std::thread::sleep(Duration::from_millis(1500 + 400 * 5 + 400));
                    voice().speak_reply(&sentences.concat(), false, true, &Mutex::new(()), &|| true, to_said, None).unwrap();
                }
                let first = first_audio.join().unwrap();
                eprintln!("F1 {}: LocalFleet asked at {} ms, first audio at {first} ms", if early { "this build" } else { "0.4.11" }, watch.join().unwrap());
                first
            };
            let done = 1500 + 400 * 5 + 400;
            let (old, new) = (run(false), run(true));
            eprintln!("F1 (from the dictation's start): reply done at {done} ms; first audio at {old} ms with the speech at the reply's end and the probe on its way, at {new} ms with the speech from the first sentence and the probe at the dictation's start");
            assert!(new < done && old > done + 1800, "old {old} ms, new {new} ms");
        });
    }

    #[test]
    fn a_new_turn_right_after_the_last_check_still_silences_the_reply() {
        within(20, || {
            let root = scratch();
            let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(1.0, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let v = replying_voice(&root, &lf, speaker);
            let turns: &'static Mutex<()> = Box::leak(Box::default());

            let live = || {
                std::thread::spawn(move || {
                    let _turn = lock(turns);
                    speaker.stop()
                });
                std::thread::sleep(Duration::from_millis(100));
                true
            };
            let p = v.speak_reply("The answer.", false, true, turns, &live, to_said, None);
            std::thread::sleep(Duration::from_millis(1500));
            let heard = lock(&out.heard).iter().filter(|s| near(**s, 2000)).count();
            assert!(p.is_err() || heard == 0, "said into the new dictation: {heard} samples, LocalFleet asked {} times", lock(&lf.bodies).len());
        });
    }

    #[test]
    fn a_replys_language_leaves_inline_code_and_tilde_fences_out() {
        let inline = "Isso é bom. `the value is the thing that you can set for this and that`";
        assert_eq!(reply_language(inline, true), "pt", "inline code counted");
        let tilde = "Isso é bom.\n~~~\nthe value is the thing that you can set for this and that\n~~~";
        assert_eq!(reply_language(tilde, true), "pt", "a ~~~ fence counted");

        let mixed = "Isso é bom.\n~~~\n```\nthe value is the thing that you can set for this and that\n~~~";
        assert_eq!(reply_language(mixed, true), "pt");
    }

    #[test]
    fn code_first_is_on_screen_in_both_languages() {
        assert_eq!(spoken("```\nlet x = 1;\n```\nThat sets x.", "first_sentence", false).as_deref(), Some("It’s on screen."));
        assert_eq!(spoken("Veja https://example.com/a agora.", "first_sentence", true).as_deref(), Some("Deixei na tela."));
        assert_eq!(spoken("Abra /etc/hosts e edite.", "full", true).as_deref(), Some("Deixei na tela."));
        assert_eq!(spoken("| a | b |\n|---|---|\n| 1 | 2 |", "full", false).as_deref(), Some("It’s on screen."));
        assert_eq!(spoken("Olá.", "off", true), None);
    }

    #[test]
    fn only_the_first_response_after_a_spoken_turn_is_gathered() {
        let g = ReplyText::new();
        g.turn(true);
        g.frame(&json!({"delta": "Mine.", "response_id": "r1"}));
        g.frame(&json!({"delta": "Another.", "response_id": "r2"}));
        assert_eq!(g.frame(&json!({"done": true, "response_id": "r2"})), None, "another response's done ends nothing");
        assert_eq!(g.frame(&json!({"done": true, "response_id": "r1"})), Some(Gather::Done("Mine.".into())));
    }

    #[test]
    fn past_the_limit_the_rest_of_a_reply_is_left_out_with_no_gap() {
        let g = ReplyText::new();
        g.turn(true);
        g.frame(&json!({"delta": "a".repeat(SPOKEN_MAX - 1)}));
        g.frame(&json!({"delta": "bb"}));
        g.frame(&json!({"delta": "c"}));
        let Some(Gather::Done(r)) = g.frame(&json!({"done": true})) else { panic!("no reply") };
        assert_eq!((r.len(), r.contains('c')), (SPOKEN_MAX - 1, false), "a smaller delta after a dropped one isn't appended");
    }
    #[test]
    fn overlap_listening_cache_has_eight_forms_reuses_disk_and_tracks_voice_mood() {
        within(15, || {
            let root=scratch(); let _gone=Gone(root.clone());
            let lf=localfleet(|_| Says::Tone(0.05,2000));
            let out: &'static FakeOut=Box::leak(Box::default()); let speaker=speaker_on(out);
            let v=replying_voice(&root,&lf,speaker); let r=Replies::new();
            r.prepare_listening(&v);
            let forms: Vec<String>=lock(&lf.bodies).iter().map(|b| b["input"].as_str().unwrap().into()).collect();
            assert_eq!(forms,crate::conversation_overlap::EN.into_iter().chain(crate::conversation_overlap::PT).map(String::from).collect::<Vec<_>>());
            assert_eq!(r.listening_level(false),crate::conversation_overlap::Level::Low);
            assert_eq!(r.listening_level(true),crate::conversation_overlap::Level::Off);
            assert_eq!(heard(out,2000),0,"rendering never plays");
            assert!(lock(&r.pending).is_none(),"no agent turn is committed");
            r.prepare_listening(&v); assert_eq!(lock(&lf.bodies).len(),8,"disk reused");
            let old=v.clip_request("yeah").unwrap().0;
            v.act("fleet_choose",&json!({"voice_id":"studio-b"})).unwrap();r.invalidate_listening();
            assert_eq!(r.listening_level(false),crate::conversation_overlap::Level::Off);
            assert_ne!(old,v.clip_request("yeah").unwrap().0);
            r.prepare_listening(&v);assert_eq!(lock(&lf.bodies).len(),16);
            assert!(lock(&lf.bodies)[8..].iter().all(|b|b["voice"]=="studio-b"));
            assert!(v.act("listening_set",&json!({"level":"high"})).is_err());
            for level in ["off","low","moderate"] {
                let saved=v.act("listening_set",&json!({"level":level})).unwrap();
                assert_eq!(saved["voice"]["listening_sounds"],level);
                assert_eq!(v.prefs().listening_sounds.as_deref(),Some(level));
            }
        });
    }

    #[test]
    fn overlap_cached_listening_uses_queue_without_reply_state_and_respects_fences() {
        within(15, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_| Says::Tone(0.05,2000));
            let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);
            let v=replying_voice(&root,&lf,speaker);let r=Replies::new();r.prepare_listening(&v);
            assert!(!r.play_listening(false,0,&v,||false));
            assert!(!r.play_listening(true,0,&v,||true),"PT default off");
            assert!(r.play_listening(false,0,&v,||true));
            until("listening clip audible",||heard(out,2000)>0);
            until("listening clip drains",||!speaker.busy());
            assert!(lock(&r.pending).is_none());assert!(r.take_timing().is_empty());
            assert_eq!(lock(&lf.bodies).len(),8,"cached playback never synthesizes");
            r.turn(true,true,speaker);
            assert!(!r.play_listening(false,1,&v,||true),"a submitted turn retires the pause");
            r.unsent();r.invalidate_listening();
            assert!(!r.play_listening(false,2,&v,||true),"invalidated settings never play");
        });
    }

    #[test]
    fn review_fix_b3_real_listening_playback_filters_its_echo_before_commit() {
        within(15, || {
            let root = scratch(); let _gone = Gone(root.clone());
            let lf = localfleet(|_| Says::Tone(0.2, 2000));
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            let v = replying_voice(&root, &lf, speaker);
            let r = Replies::new(); r.prepare_listening(&v);
            crate::conversation::review_fix_b3_listening_clip_echo(|| {
                assert!(r.play_listening(false, 1, &v, || true));
                until("listening sound audible", || heard(out, 2000) > 0);
            });
            speaker.stop();
        });
    }

    #[test]
    fn overlap_cache_busy_retries_are_bounded_and_never_publish_bad_audio() {
        within(15, || {
            let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_| Says::Busy(0));
            let v=replying_voice(&root,&lf,quiet());let r=Replies::new();
            let (path,url,body,shift)=v.clip_request("yeah").unwrap();
            r.render_clip(&v,&path,&url,&body,shift);
            assert_eq!(lock(&lf.bodies).len(),1+BUSY_RETRIES as usize);
            assert!(!path.exists());
            let lf=localfleet(|n|if n==0 {Says::Busy(0)} else {Says::Tone(0.05,2000)});
            let v=replying_voice(&root,&lf,quiet());
            let (path,url,body,shift)=v.clip_request("yeah").unwrap();
            r.render_clip(&v,&path,&url,&body,shift);
            assert_eq!(lock(&lf.bodies).len(),2);assert!(v.cached_clip("yeah").is_some());
            std::fs::write(&path,[1u8,2,3]).unwrap();assert!(v.cached_clip("yeah").is_none(),"torn cache rejected");
        });
    }

    #[test]
    fn review04_b1_semantic_reply_hold_is_armed_before_first_audio() {
        if std::env::var_os("RTH_VOICE_EARLY_REVIEW04_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","voice::tests::review04_b1_semantic_reply_hold_is_armed_before_first_audio","--test-threads","4"])
                .env("RTH_VOICE_EARLY_REVIEW04_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}",String::from_utf8_lossy(&child.stdout));return;
        }
        let root=scratch();let _gone=Gone(root.clone());let lf=localfleet(|_|Says::Tone(0.5,2000));
        let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);
        let v=replying_voice(&root,&lf,speaker);let turns=Mutex::new(());
        crate::doubletalk::reset();crate::floor_decide::hold(true);
        v.speak_reply("A reply that must stay paused.",false,false,&turns,&||true,to_said,None).unwrap();
        let held=crate::doubletalk::playout_held();
        speaker.stop();crate::doubletalk::reset();crate::floor_decide::hold(false);
        assert!(held,"a pending semantic pause must cover the first reply samples, before streaming progress exists");
    }
    #[test]
    fn review04_b1_reply_marker_tracks_matching_preview_and_retires_on_cut() {
        if std::env::var_os("RTH_VOICE_REVIEW04_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","voice::tests::review04_b1_reply_marker_tracks_matching_preview_and_retires_on_cut","--test-threads","4"])
                .env("RTH_VOICE_REVIEW04_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}",String::from_utf8_lossy(&child.stdout));return;
        }
        let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);let r=Replies::new();
        crate::doubletalk::reset();crate::doubletalk::playing(true);

        let admitted=||{if crate::doubletalk::reply_active() {crate::doubletalk::hold_overlap();}};
        r.turn(true,true,speaker);
        {let mut p=lock(&r.pending);let p=p.as_mut().unwrap();p.speaking=true;p.preview=Some("reply-owned".into());}
        r.event(&json!({"preview_id":"listening-clip","state":"streaming"}));
        admitted();assert!(!crate::doubletalk::held(),"another preview is not reply audio");
        r.event(&json!({"preview_id":"reply-owned","state":"streaming"}));
        admitted();assert!(crate::doubletalk::held(),"the matching reply preview can hold");
        r.event(&json!({"preview_id":"reply-owned","state":"done","audio_seconds":1.0}));
        assert!(!crate::doubletalk::held(),"a reply terminal retires the hold");
        r.turn(true,true,speaker);
        {let mut p=lock(&r.pending);let p=p.as_mut().unwrap();p.speaking=true;p.preview=Some("reply-next".into());}
        r.event(&json!({"preview_id":"reply-next","state":"streaming"}));admitted();assert!(crate::doubletalk::held());
        r.dictation(speaker);admitted();assert!(!crate::doubletalk::held(),"cut removes pending before its terminal arrives");
        crate::doubletalk::reset();
    }
    #[test]
    fn h1_two_word_overlap_flushes_queued_audio() {
        within(10, || {
            use crate::conversation_overlap::{Class, Overlap};
            let out: &'static FakeOut = Box::leak(Box::default());
            let speaker = speaker_on(out);
            speaker.start(Job::Clip { samples: vec![2000; 24_000 * 5], rate: 24_000 }, to_said);
            until("agent is audible", || heard(out, 2000) > 0);
            let mut gate = Overlap::default();
            let d = gate.words("makes sense", false).unwrap();
            assert_eq!(d.class, Class::Barge);
            speaker.stop();
            assert!(!speaker.busy());
            assert_eq!(speaker.queued(), 0);
        });
    }

    #[test]
    fn slice1_opener_and_replacement_preserve_main_tail_without_duplicate_speech() {
        within(10, || {
            let root=scratch(); let _gone=Gone(root.clone());
            let lf=localfleet(|_| Says::Tone(0.1,3000));
            let out: &'static FakeOut=Box::leak(Box::default()); let speaker=speaker_on(out);
            let voice=voices_of(&root,Box::leak(Box::new(Answers(Err("no probe".into())))),listed(&root,&lf,"full"),speaker);
            let (r,emit,log,lines,_)=replies!(); r.turn(true,true,speaker);
            r.frame(&json!({"opener":"Let me think.","response_id":"r1"}),false,voice.clone(),emit,log);
            until("opener synthesized", || lock(&lf.bodies).len()==1);
            r.frame(&delta("Discard this, "),false,voice.clone(),emit,log);
            r.frame(&json!({"replace":"The answer is four.","response_id":"r1"}),false,voice.clone(),emit,log);
            r.frame(&json!({"done":true,"response_id":"r1"}),false,voice.clone(),emit,log);
            until("replacement ends", || !lock(lines).is_empty());
            let bodies=lock(&lf.bodies);
            assert_eq!(bodies.last().unwrap()["input"],"The answer is four.");
            assert_eq!(bodies.iter().filter(|b| b["input"]=="Let me think.").count(),1,"replacement never replays the opener");
        });
    }

    #[test]
    fn slice1_cache_fill_and_reply_are_serialized_at_localfleet_admission() {
        within(10, || {
            let root=scratch(); let _gone=Gone(root.clone());
            let gate=Arc::new((Mutex::new(false),std::sync::Condvar::new())); let held=Arc::clone(&gate);
            let lf=localfleet(move |n| if n==0 { Says::Held(0.1,2000,Arc::clone(&held)) } else { Says::Tone(0.1,3000) });
            let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);
            let voice=voices_of(&root,Box::leak(Box::new(Answers(Err("no probe".into())))),listed(&root,&lf,"full"),speaker);
            let (r,emit,log,lines,_)=replies!();let fill=voice.clone();
            let worker=std::thread::spawn(move || r.prepare_listening(&fill()));
            until("cache request admitted", || lock(&lf.bodies).len()==1);
            r.turn(true,true,speaker);
            r.frame(&delta("Here is the answer, "),false,voice.clone(),emit,log);
            r.frame(&json!({"done":true,"response_id":"r1"}),false,voice.clone(),emit,log);
            until("reply joins cache fill", || lock(&r.pending).as_ref().is_some_and(|p| p.joined_at.is_some()));
            assert_eq!(lock(&lf.bodies).len(),1);
            *lock(&gate.0)=true;gate.1.notify_all();worker.join().unwrap();
            until("reply ends", || !lock(lines).is_empty());
            assert_eq!(lock(&lf.bodies).len(),2,"no second cache language request beside reply");
            assert!(lock(lines)[0].contains("retries 0"));
            assert!(heard(out,3000)>0);assert_eq!(heard(out,2000),0);
        });
    }

    #[test]
    fn review_b1_first_sentence_tail_is_sent_before_done_without_repeating_clause() {
        let g=ReplyText::new();g.turn(true);
        assert_eq!(g.frame(&json!({"delta":"For this question, "})),Some(Gather::First("For this question,".into())));
        assert_eq!(g.frame(&json!({"delta":"the answer is four. Then more"})),Some(Gather::First("For this question, the answer is four. Then more".into())));
        for mode in ["full","first_sentence","summary"] {
            within(10, move || {
                let root=scratch();let _gone=Gone(root.clone());
                let lf=localfleet(|_| Says::Tone(0.1,3000));
                let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);
                let voice=voices_of(&root,Box::leak(Box::new(Answers(Err("no probe".into())))),listed(&root,&lf,mode),speaker);
                let (r,emit,log,lines,_)=replies!();
                r.turn(true,true,speaker);r.frame(&json!({"delta":"For this question, ","response_id":mode}),false,voice.clone(),emit,log);
                until("first clause sent", || lock(&lf.bodies).len()==1);
                r.frame(&json!({"delta":"the answer is four. Then more","response_id":mode}),false,voice.clone(),emit,log);
                until("sentence tail sent before done", || lock(&lf.bodies).len()==2);
                assert_eq!(lock(&lf.bodies)[1]["input"],"the answer is four.");
                r.frame(&json!({"delta":" keeps coming","response_id":mode}),false,voice.clone(),emit,log);
                r.frame(&json!({"done":true,"response_id":mode}),false,voice.clone(),emit,log);
                until("reply ends", || !lock(lines).is_empty());
                let inputs:Vec<String>=lock(&lf.bodies).iter().map(|b| b["input"].as_str().unwrap().into()).collect();
                let expected=if mode=="full" { vec!["For this question,","the answer is four.","Then more keeps coming"] } else { vec!["For this question,","the answer is four."] };
                assert_eq!(inputs,expected);
                r.take_timing();lock(lines).clear();
            });
        }
    }

    #[test]
    fn review_b2_preview_retries_429_during_cache_fill_then_plays_audio() {
        within(10, || {
            let root=scratch();let _gone=Gone(root.clone());
            let gate=Arc::new((Mutex::new(false),std::sync::Condvar::new()));let held=Arc::clone(&gate);
            let lf=localfleet(move |n| if n==0 { Says::Held(0.1,2000,Arc::clone(&held)) } else if !*lock(&held.0) { Says::Busy(1) } else { Says::Tone(0.1,3000) });
            let out: &'static FakeOut=Box::leak(Box::default());let speaker=speaker_on(out);
            let voice=voices_of(&root,Box::leak(Box::new(Answers(Err("no probe".into())))),listed(&root,&lf,"full"),speaker);
            let r=Box::leak(Box::new(Replies::new()));let fill=voice.clone();
            let worker=std::thread::spawn(move || r.prepare_listening(&fill()));
            until("cache fill held", || lock(&lf.bodies).len()==1);
            let preview=voice().act("fleet_preview",&json!({"voice_id":"unfold-pt"})).unwrap();
            let id=preview["voice_started"]["preview_id"].as_str().unwrap();
            until("preview got busy", || lf.served.load(Ordering::SeqCst)>=1);
            *lock(&gate.0)=true;gate.1.notify_all();worker.join().unwrap();
            until("preview ends", || ended(id).is_some());
            assert_eq!(ended(id).unwrap()["state"],"done");
            assert!(heard(out,3000)>0);assert_eq!(heard(out,2000),0);
            assert!(ended(id).unwrap()["retries"].as_u64().unwrap()>0);
        });
    }

    #[test]
    fn slice1_first_clause_is_ready_before_sentence_finishes() {
        let g = ReplyText::new(); g.turn(true);
        assert_eq!(g.frame(&json!({"delta":"For this question, "})), Some(Gather::First("For this question,".into())));
    }
    #[test]
    fn slice1_opener_is_speakable_before_any_delta() {
        let g = ReplyText::new(); g.turn_stream(true,true);
        assert_eq!(g.frame(&json!({"opener":"Let me think.","response_id":"r1"})), Some(Gather::Chunk(vec!["Let me think.".into()])));
        assert_eq!(g.frame(&json!({"delta":"The answer is four.","response_id":"r1"})), None);
        assert_eq!(g.frame(&json!({"done":true,"response_id":"r1"})), Some(Gather::StreamDone(vec!["The answer is four.".into()])));
    }
    #[test]
    fn slice1_clause_word_cap_is_utf8_safe_and_does_not_cut_decimal_tokens() {
        let g=ReplyText::new();g.turn(true);
        assert_eq!(g.frame(&json!({"delta":"Um dois três quatro cinco seis sete oito "})),Some(Gather::First("Um dois três quatro cinco seis sete oito".into())));
        assert_eq!(first_clause("It is 3."),None);
        assert_eq!(first_clause("It is 3.5 and still"),None);
    }

    #[test]
    fn slice1_first_clause_never_speaks_a_partially_streamed_cut_marker() {
        for marker in [" (cut short at the length limit; ", " (cortado no limite de tamanho; "] {
            for all in [false,true] {
                let g=ReplyText::new();g.turn_stream(true,all);
                assert_eq!(g.frame(&json!({"delta":format!("Two words{marker}")})),None);
            }
        }
    }

}

#[cfg(test)]
mod h1_regressions {
    #[test]
    fn h1_no_ack_playback_is_wired_or_timed() {
        let settings = include_str!("settings.rs");
        for retired in ["schedule_ack", "prepare_ack", "ack_event"] {
            assert!(!settings.contains(retired), "retired ack still wired: {retired}");
        }
        let line = super::TurnTiming::default().line();
        assert!(!line.contains("ack_"), "retired ack timing: {line}");
        assert!(line.starts_with("turn timing:"));
    }
}
