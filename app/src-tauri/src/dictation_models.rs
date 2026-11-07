

use crate::{owned_check, owned_dir, owned_open, owned_replace, Open};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const CATALOG: &str = include_str!("../dictation/catalog.json");
const OPENMDW: &str = include_str!("../dictation/LICENSE-OpenMDW-1.1.txt");
const CC_BY: &str = include_str!("../dictation/LICENSE-CC-BY-4.0.txt");
const NVIDIA_OML: &str = include_str!("../dictation/LICENSE-NVIDIA-Open-Model-License.txt");
const MARKER: &str = ".verified";

pub fn catalog() -> &'static Value {
    static C: OnceLock<Value> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(CATALOG).expect("dictation/catalog.json"))
}

pub fn model<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    cat["models"].as_array()?.iter().find(|m| m["id"] == id)
}

pub fn root(data: &Path) -> PathBuf {
    data.join("dictation")
}

pub fn dir(data: &Path, id: &str) -> PathBuf {
    root(data).join(id)
}

pub fn files(m: &Value) -> impl Iterator<Item = (&str, u64, &str)> {
    m["files"].as_array().into_iter().flatten().map(|f| (f["name"].as_str().unwrap_or(""), f["size"].as_u64().unwrap_or(0), f["sha256"].as_str().unwrap_or("")))
}

pub fn total_size(m: &Value) -> u64 {
    files(m).map(|(_, s, _)| s).sum()
}

fn marker_text(m: &Value) -> String {
    files(m).map(|(n, _, h)| format!("{h}  {n}\n")).collect()
}

pub fn installed(data: &Path, m: &Value) -> bool {
    let d = dir(data, m["id"].as_str().unwrap_or(""));
    std::fs::read_to_string(d.join(MARKER)).is_ok_and(|t| t == marker_text(m)) && files(m).all(|(n, s, _)| std::fs::symlink_metadata(d.join(n)).is_ok_and(|x| x.is_file() && x.len() == s))
}

fn archive(m: &Value) -> Option<(&str, u64, &str, &str)> {
    let a = &m["archive"];
    Some((a["url"].as_str()?, a["size"].as_u64()?, a["sha256"].as_str()?, a["dir"].as_str()?))
}

pub fn download_size(m: &Value) -> u64 {
    archive(m).map_or_else(|| total_size(m), |a| a.1)
}

const ARCHIVE_PART: &str = "archive.tar.bz2.part";

pub fn partial_bytes(data: &Path, m: &Value) -> u64 {
    let p = dir(data, m["id"].as_str().unwrap_or("")).join(".partial");
    let len = |n: &str| std::fs::metadata(p.join(n)).map(|x| x.len()).unwrap_or(0);
    let parts: u64 = files(m).map(|(n, s, _)| len(&format!("{n}.part")).min(s)).sum();
    parts + archive(m).map_or(0, |a| len(ARCHIVE_PART).min(a.1))
}

fn default_path(data: &Path) -> PathBuf {
    root(data).join("default.json")
}

pub fn by_choice(choice: &str) -> Option<&'static Value> {
    catalog()["models"].as_array()?.iter().find(|m| m["choice"] == choice)
}

pub fn suggest(system: Option<&str>) -> &'static str {
    let english = |t: &str| t.eq_ignore_ascii_case("en") || t.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("en-") || p.eq_ignore_ascii_case("en_"));
    if system.is_some_and(english) {
        "en"
    } else {
        "multi"
    }
}

pub fn system_lang() -> Option<String> {
    crate::system_language().or_else(|| {
        let out = std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLanguages"])
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        apple_languages_first(&String::from_utf8_lossy(&out.stdout))
    })
}

fn apple_languages_first(out: &str) -> Option<String> {
    let first = out.trim().strip_prefix('(')?.lines().map(|l| l.trim().trim_end_matches(',').trim_matches('"')).find(|l| !l.is_empty() && *l != ")")?;
    (first.len() <= 35 && first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| first.to_string())
}

fn saved_or_suggested_id(data: &Path) -> String {
    std::fs::read(default_path(data))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["model"].as_str().map(str::to_string))
        .filter(|id| model(catalog(), id).is_some())
        .unwrap_or_else(|| by_choice(suggest(system_lang().as_deref())).and_then(|m| m["id"].as_str()).unwrap_or("").to_string())
}

pub fn set_default(data: &Path, id: &str) -> Result<(), String> {
    model(catalog(), id).ok_or(format!("{id} is not a dictation model"))?;
    owned_dir(data, &root(data))?;
    let p = default_path(data);
    owned_replace(&p, &p.with_extension("json.tmp"), format!("{}\n", json!({ "model": id })).as_bytes()).map_err(|e| format!("{}: {e}", p.display()))
}

pub const PT_MODEL: &str = "parakeet-tdt-0.6b-v3-q8";
const PT_CPU: &str = "parakeet-tdt-0.6b-v3-int8";
fn portuguese(lang: &str) -> bool { lang.eq_ignore_ascii_case("pt") || lang.eq_ignore_ascii_case("pt-BR") }

pub fn default_id_for(data: &Path, lang: &str) -> String {
    if portuguese(lang) { PT_MODEL.into() } else { saved_or_suggested_id(data) }
}
pub fn default_id(data: &Path) -> String { default_id_for(data, crate::dictation::stream_lang()) }
pub fn choice_for(choice: &str, lang: &str) -> Option<&'static Value> {
    if choice == "multi" && portuguese(lang) { model(catalog(), PT_MODEL) } else { by_choice(choice) }
}

pub fn ready_for(data: &Path, lang: &str) -> Option<(&'static Value, PathBuf)> {
    let cat = catalog();
    if portuguese(lang) {
        let pick = [PT_MODEL, PT_CPU].into_iter().filter_map(|id| model(cat, id)).find(|m| installed(data, m))?;
        return Some((pick, dir(data, pick["id"].as_str()?)));
    }
    let def = default_id_for(data, lang);
    let all = cat["models"].as_array()?;
    let pick = all.iter().find(|m| m["id"] == def.as_str() && installed(data, m)).or_else(|| all.iter().find(|m| installed(data, m)))?;
    Some((pick, dir(data, pick["id"].as_str()?)))
}

pub fn ready(data: &Path) -> Option<(&'static Value, PathBuf)> {
    ready_for(data, crate::dictation::stream_lang())
}

pub fn engine(m: &Value) -> &str {
    m["engine"].as_str().unwrap_or("sherpa")
}

pub fn fallback(data: &Path, id: &str) -> Option<(&'static Value, PathBuf)> {
    let m = catalog()["models"].as_array()?.iter().find(|m| m["fallback_for"] == id && installed(data, m))?;
    Some((m, dir(data, m["id"].as_str()?)))
}

pub fn hash_file(p: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn url(m: &Value, name: &str) -> String {
    format!("https://huggingface.co/{}/resolve/{}/{}", m["repo"].as_str().unwrap_or(""), m["revision"].as_str().unwrap_or(""), name)
}

pub trait Fetch {
    fn fetch(&self, url: &str, part: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64), retrying: &dyn Fn(u32, u32)) -> Result<(), String>;
}

pub struct Curl {
    pub https_only: bool,
    pub stall_s: u32,
    pub attempts: u32,
    pub first_wait: std::time::Duration,
}

impl Default for Curl {
    fn default() -> Self {
        Curl { https_only: true, stall_s: 30, attempts: 5, first_wait: std::time::Duration::from_secs(2) }
    }
}

impl Fetch for Curl {
    fn fetch(&self, url: &str, part: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64), retrying: &dyn Fn(u32, u32)) -> Result<(), String> {
        let mut wait = self.first_wait;
        let mut last = Err(String::new());
        for attempt in 1..=self.attempts {
            if attempt > 1 {
                retrying(attempt, self.attempts);

                let until = std::time::Instant::now() + wait;
                while std::time::Instant::now() < until {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(CANCELLED.into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                wait = (wait * 2).min(std::time::Duration::from_secs(30));
            }
            last = self.once(url, part, cancel, progress);
            match &last {
                Ok(()) => return Ok(()),
                Err(e) if e == CANCELLED => return last,
                Err(_) => {}
            }
        }
        last
    }
}

impl Curl {
    fn once(&self, url: &str, part: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64)) -> Result<(), String> {
        use std::process::{Command, Stdio};
        let mut cmd = Command::new("/usr/bin/curl");
        cmd.args(["--fail", "--location", "--silent", "--show-error", "--speed-limit", "1024", "--speed-time"]).arg(self.stall_s.to_string());
        if self.https_only {
            cmd.args(["--proto", "=https", "--proto-redir", "=https", "--tlsv1.2"]);
        }
        let mut child = cmd
            .args(["-C", "-", "-o"])
            .arg(part)
            .arg(url)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        loop {
            if cancel.load(Ordering::SeqCst) {

                let _ = child.kill();
                let _ = child.wait();
                return Err(CANCELLED.into());
            }
            if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                if s.success() {
                    return Ok(());
                }
                let mut e = String::new();
                if let Some(mut er) = child.stderr.take() {
                    let _ = er.read_to_string(&mut e);
                }
                return Err(e.trim().to_string());
            }
            progress(std::fs::metadata(part).map(|x| x.len()).unwrap_or(0));
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}

pub const CANCELLED: &str = "cancelled";

#[derive(Debug, PartialEq)]
pub enum Failure {

    Mismatch(String),
    Cancelled,

    Disk(u64, u64),
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {

    Bytes { what: String, done: u64, total: u64 },

    Retrying { attempt: u32, of: u32 },

    Verifying { what: String },

    Unpacking,
}

pub fn install(data: &Path, id: &str, fetch: &dyn Fetch, cancel: &AtomicBool, on: &dyn Fn(Step)) -> Result<PathBuf, Failure> {
    let m = model(catalog(), id).ok_or_else(|| Failure::Other(format!("{id} is not a dictation model")))?;
    install_model(data, m, fetch, cancel, on)
}

const ARCHIVE_NAME: &str = "release archive";

pub fn space_needed(data: &Path, m: &Value) -> u64 {
    let left = download_size(m).saturating_sub(partial_bytes(data, m));
    left + if archive(m).is_some() { total_size(m) } else { 0 } + 200_000_000
}

pub fn free_bytes(p: &Path) -> Option<u64> {
    let mut at = p;
    while !at.exists() {
        at = at.parent()?;
    }
    let c = std::ffi::CString::new(at.as_os_str().as_encoded_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

pub fn install_model(data: &Path, m: &Value, fetch: &dyn Fetch, cancel: &AtomicBool, on: &dyn Fn(Step)) -> Result<PathBuf, Failure> {
    let id = m["id"].as_str().unwrap_or("");
    let d = dir(data, id);
    if installed(data, m) {
        return Ok(d);
    }
    let staging = d.join(".partial");
    owned_dir(data, &staging).map_err(Failure::Other)?;
    let list: Vec<_> = files(m).collect();
    if list.iter().any(|(n, _, _)| n.is_empty() || n.contains('/') || n.starts_with('.')) {
        return Err(Failure::Other(format!("{id}: a bad file name in the catalog")));
    }

    let mut archive_failed = None;
    if let Some(a) = archive(m) {
        let have = |(n, s, _): &(&str, u64, &str)| std::fs::symlink_metadata(staging.join(format!("{n}.part"))).is_ok_and(|x| x.len() == *s);
        if !list.iter().all(have) {
            match from_archive(data, &list, a, &staging, fetch, cancel, on) {
                Ok(()) => {}
                Err(Failure::Cancelled) => return Err(Failure::Cancelled),
                Err(f) => archive_failed = Some(f),
            }
        }
    }
    let total = total_size(m);
    let mut before = 0u64;
    for (name, size, sha) in &list {
        let part = staging.join(format!("{name}.part"));
        let have = std::fs::symlink_metadata(&part).map(|x| x.len()).unwrap_or(0);
        if have > *size {
            let _ = std::fs::remove_file(&part);
        }
        if std::fs::symlink_metadata(&part).map(|x| x.len()).unwrap_or(0) < *size {
            let base = before;
            let progress = |n: u64| on(Step::Bytes { what: name.to_string(), done: base + n.min(*size), total });
            let retrying = |attempt, of| on(Step::Retrying { attempt, of });
            fetch.fetch(&url(m, name), &part, cancel, &progress, &retrying).map_err(|e| match (e, &archive_failed) {
                (e, _) if e == CANCELLED => Failure::Cancelled,

                (_, Some(Failure::Mismatch(w))) => Failure::Mismatch(w.clone()),
                (e, _) => Failure::Other(e),
            })?;
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(Failure::Cancelled);
        }
        on(Step::Verifying { what: name.to_string() });
        let got = hash_file(&part).map_err(Failure::Other)?;
        if got != *sha {
            let _ = std::fs::remove_file(&part);
            return Err(Failure::Mismatch(format!("{name}: sha256 {got}, pinned {sha}")));
        }
        before += size;
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(Failure::Cancelled);
    }
    for (name, _, _) in &list {
        std::fs::rename(staging.join(format!("{name}.part")), d.join(name)).map_err(|e| Failure::Other(format!("{name}: {e}")))?;
    }
    let _ = std::fs::remove_dir_all(&staging);
    let write = |name: &str, text: &str| -> Result<(), Failure> {
        owned_open(&d.join(name), Open::Truncate).and_then(|mut f| f.write_all(text.as_bytes())).map_err(|e| Failure::Other(format!("{name}: {e}")))
    };
    let lic = m["licence_file"].as_str().unwrap_or("");
    write(lic, licence_text(lic).ok_or_else(|| Failure::Other(format!("no licence text for {lic:?}")))?)?;
    write("NOTICE.txt", &format!("{}\n", m["notice"].as_str().unwrap_or("")))?;
    write(MARKER, &marker_text(m))?;
    Ok(d)
}

fn licence_text(file: &str) -> Option<&'static str> {
    match file {
        "LICENSE-OpenMDW-1.1.txt" => Some(OPENMDW),
        "LICENSE-CC-BY-4.0.txt" => Some(CC_BY),
        "LICENSE-NVIDIA-Open-Model-License.txt" => Some(NVIDIA_OML),
        _ => None,
    }
}

fn from_archive(
    data: &Path,
    list: &[(&str, u64, &str)],
    (url, size, sha, top): (&str, u64, &str, &str),
    staging: &Path,
    fetch: &dyn Fetch,
    cancel: &AtomicBool,
    on: &dyn Fn(Step),
) -> Result<(), Failure> {
    let part = staging.join(ARCHIVE_PART);
    if std::fs::symlink_metadata(&part).map(|x| x.len()).unwrap_or(0) > size {
        let _ = std::fs::remove_file(&part);
    }
    if std::fs::symlink_metadata(&part).map(|x| x.len()).unwrap_or(0) < size {
        let progress = |n: u64| on(Step::Bytes { what: ARCHIVE_NAME.into(), done: n.min(size), total: size });
        let retrying = |attempt, of| on(Step::Retrying { attempt, of });
        fetch.fetch(url, &part, cancel, &progress, &retrying).map_err(|e| if e == CANCELLED { Failure::Cancelled } else { Failure::Other(e) })?;
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(Failure::Cancelled);
    }
    on(Step::Verifying { what: ARCHIVE_NAME.into() });
    let got = hash_file(&part).map_err(Failure::Other)?;
    if got != sha {
        let _ = std::fs::remove_file(&part);
        return Err(Failure::Mismatch(format!("archive: sha256 {got}, pinned {sha}")));
    }
    if top.is_empty() || top.contains('/') || top.starts_with('.') {
        return Err(Failure::Other("a bad archive directory in the catalog".into()));
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(Failure::Cancelled);
    }
    on(Step::Unpacking);
    let x = staging.join("x");
    let _ = std::fs::remove_dir_all(&x);
    owned_dir(data, &x).map_err(Failure::Other)?;
    let r = (|| {
        let mut tar = std::process::Command::new("/usr/bin/tar")
            .arg("-xjf")
            .arg(&part)
            .arg("-C")
            .arg(&x)
            .args(list.iter().map(|(n, _, _)| format!("{top}/{n}")))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Failure::Other(format!("tar: {e}")))?;
        let ok = loop {
            if cancel.load(Ordering::SeqCst) {

                let _ = tar.kill();
                let _ = tar.wait();
                return Err(Failure::Cancelled);
            }
            if let Some(s) = tar.try_wait().map_err(|e| Failure::Other(format!("tar: {e}")))? {
                break s.success();
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        if !ok {
            return Err(Failure::Other("tar: the archive did not extract".into()));
        }
        for (name, size, want) in list {
            let f = x.join(top).join(name);
            if !std::fs::symlink_metadata(&f).is_ok_and(|m| m.is_file() && m.len() == *size) {
                return Err(Failure::Mismatch(format!("archive {name}: not a regular file of {size} bytes")));
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(Failure::Cancelled);
            }
            on(Step::Verifying { what: name.to_string() });
            let got = hash_file(&f).map_err(Failure::Other)?;
            if got != *want {
                return Err(Failure::Mismatch(format!("archive {name}: sha256 {got}, pinned {want}")));
            }
        }
        for (name, _, _) in list {
            std::fs::rename(x.join(top).join(name), staging.join(format!("{name}.part"))).map_err(|e| Failure::Other(format!("{name}: {e}")))?;
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&x);

    if matches!(r, Ok(()) | Err(Failure::Mismatch(_))) {
        let _ = std::fs::remove_file(&part);
    }
    r
}

pub fn remove(data: &Path, id: &str) -> Result<(), String> {
    model(catalog(), id).ok_or(format!("{id} is not a dictation model"))?;
    let d = dir(data, id);
    owned_check(data, &d)?;
    match std::fs::symlink_metadata(&d) {
        Ok(x) if x.is_dir() => std::fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display())),
        Ok(_) => Err(format!("{} is not a directory", d.display())),
        Err(_) => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Queued,
    Downloading,
    Retrying(u32, u32),
    Verifying,
    Unpacking,

    Preparing,
    Paused,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub stage: Stage,
    pub what: String,
    pub done: u64,
    pub total: u64,

    pub speed: Option<f64>,
    pub eta_s: Option<u64>,
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
    started: Option<std::time::Instant>,
}

impl Job {
    fn new(id: &str, stage: Stage) -> Self {
        Job { id: id.into(), stage, what: String::new(), done: 0, total: 0, speed: None, eta_s: None, samples: Default::default(), started: None }
    }

    pub fn bytes(&mut self, what: &str, done: u64, total: u64, now: std::time::Instant) {
        if what != self.what || done < self.done {
            self.samples.clear();
        }
        (self.what, self.done, self.total, self.stage) = (what.into(), done, total, Stage::Downloading);
        let started = *self.started.get_or_insert(now);
        self.samples.push_back((now, done));
        while self.samples.len() > 2 && now.duration_since(self.samples[1].0).as_secs_f64() > 60.0 {
            self.samples.pop_front();
        }
        let rate = |window: f64| {
            let (t0, b0) = *self.samples.iter().find(|(t, _)| now.duration_since(*t).as_secs_f64() <= window).unwrap_or(self.samples.front().unwrap());
            let dt = now.duration_since(t0).as_secs_f64();
            (dt >= 0.5).then(|| (done - b0) as f64 / dt)
        };
        self.speed = rate(2.0);
        self.eta_s = rate(60.0).filter(|r| *r > 0.0 && now.duration_since(started).as_secs_f64() >= 10.0).map(|r| ((total.saturating_sub(done)) as f64 / r).ceil() as u64);
    }
}

struct Queue {
    jobs: Vec<Job>,
    running: bool,

    stop: Option<(Arc<AtomicBool>, Arc<AtomicBool>)>,
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue { jobs: vec![], running: false, stop: None });

pub fn jobs() -> Vec<Job> {
    QUEUE.lock().unwrap().jobs.clone()
}

pub fn installing() -> Option<(String, u64, u64)> {
    QUEUE.lock().unwrap().jobs.iter().find(|j| !matches!(j.stage, Stage::Paused | Stage::Failed(_) | Stage::Queued)).map(|j| (j.id.clone(), j.done, j.total))
}

fn intent_path(data: &Path, id: &str) -> PathBuf {
    dir(data, id).join(".partial").join("intent")
}

fn set_intent(data: &Path, id: &str, intent: Option<&str>) {
    let p = intent_path(data, id);
    match intent {
        Some(i) => {
            if owned_dir(data, &dir(data, id).join(".partial")).is_ok() {
                let _ = owned_open(&p, Open::Truncate).and_then(|mut f| f.write_all(i.as_bytes()));
            }
        }
        None => {
            let _ = std::fs::remove_file(&p);
        }
    }
}

const MISMATCH: (&str, &str) = ("The download didn’t match its checksum, so it was deleted. Try again.", "O download não bateu com a soma de verificação e foi apagado. Tente de novo.");
const STOPPED: (&str, &str) = ("The download stopped. Retry to continue where it left off.", "O download parou. Tente de novo para continuar de onde parou.");

pub fn disk_said(need: u64, free: u64, pt: bool) -> String {
    if pt {
        format!("Precisa de {}; há {} livres.", gb(need), gb(free))
    } else {
        format!("Needs {}; {} free.", gb(need), gb(free))
    }
}

pub fn start_install(data: PathBuf, id: &str, log: crate::Log, changed: fn()) -> Result<(), String> {
    start_install_with(data, id, log, changed, Arc::new(Env { fetch: Box::new(Curl::default()), free: free_bytes, prepare: true }))
}

pub struct Env {
    pub fetch: Box<dyn Fetch + Send + Sync>,
    pub free: fn(&Path) -> Option<u64>,

    pub prepare: bool,
}

pub fn start_install_with(data: PathBuf, id: &str, log: crate::Log, changed: fn(), env: Arc<Env>) -> Result<(), String> {
    model(catalog(), id).ok_or(format!("{id} is not a dictation model"))?;
    {
        let mut q = QUEUE.lock().unwrap();
        match q.jobs.iter_mut().find(|j| j.id == id) {
            Some(j) if matches!(j.stage, Stage::Paused | Stage::Failed(_)) => j.stage = Stage::Queued,
            Some(_) => return Ok(()),
            None => q.jobs.push(Job::new(id, Stage::Queued)),
        }
        set_intent(&data, id, Some("install"));
        if q.running {
            drop(q);
            changed();
            return Ok(());
        }
        q.running = true;
    }
    changed();
    std::thread::Builder::new().name("dictation-install".into()).spawn(move || run_queue(data, log, changed, env)).map_err(|e| {
        QUEUE.lock().unwrap().running = false;
        e.to_string()
    })?;
    Ok(())
}

fn run_queue(data: PathBuf, log: crate::Log, changed: fn(), env: Arc<Env>) {
    loop {
        let (id, stop, pause) = {
            let mut q = QUEUE.lock().unwrap();
            let Some(j) = q.jobs.iter().find(|j| j.stage == Stage::Queued) else {
                q.running = false;
                q.stop = None;
                return;
            };
            let (stop, pause) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
            let id = j.id.clone();
            q.stop = Some((stop.clone(), pause.clone()));
            (id, stop, pause)
        };
        let m = model(catalog(), &id).unwrap();
        let pt = crate::settings::pt();
        let set = |stage: Stage| {
            if let Some(j) = QUEUE.lock().unwrap().jobs.iter_mut().find(|j| j.id == id) {
                j.stage = stage;
            }
            changed();
        };
        let (need, free) = (space_needed(&data, m), (env.free)(&data).unwrap_or(u64::MAX));
        let r = if free < need {
            Err(Failure::Disk(need, free))
        } else {
            log.line(&format!("dictation: installing {id}"));
            let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
            install(&data, &id, env.fetch.as_ref(), &stop, &|step| {
                let mut q = QUEUE.lock().unwrap();
                let Some(j) = q.jobs.iter_mut().find(|j| j.id == id) else { return };
                let forced = !matches!(step, Step::Bytes { .. });
                match step {
                    Step::Bytes { what, done, total } => j.bytes(&what, done, total, std::time::Instant::now()),
                    Step::Retrying { attempt, of } => j.stage = Stage::Retrying(attempt, of),
                    Step::Verifying { what } => (j.stage, j.what) = (Stage::Verifying, what),
                    Step::Unpacking => j.stage = Stage::Unpacking,
                }
                drop(q);
                let mut l = last.lock().unwrap();
                if forced || l.elapsed() >= std::time::Duration::from_millis(500) {
                    *l = std::time::Instant::now();
                    changed();
                }
            })
            .map(|_| ())
        };
        match r {
            Ok(()) => {
                log.line(&format!("dictation: {id} installed and verified"));
                if engine(m) == "nemo" && env.prepare {
                    set(Stage::Preparing);
                    crate::dictation::prepare(&data, m);
                }
                QUEUE.lock().unwrap().jobs.retain(|j| j.id != id);
                changed();
            }
            Err(Failure::Cancelled) if pause.load(Ordering::SeqCst) => {
                log.line(&format!("dictation: {id} paused (resumable)"));
                set_intent(&data, &id, Some("paused"));
                set(Stage::Paused);
            }
            Err(Failure::Cancelled) => {
                log.line(&format!("dictation: {id} cancelled; its partial files removed"));
                QUEUE.lock().unwrap().jobs.retain(|j| j.id != id);
                let _ = std::fs::remove_dir_all(dir(&data, &id).join(".partial"));
                changed();
            }
            Err(f) => {
                log.line(&format!("dictation: {id} install failed: {f:?}"));

                set_intent(&data, &id, None);
                let said = match &f {
                    Failure::Disk(need, free) => disk_said(*need, *free, pt),
                    Failure::Mismatch(_) => (if pt { MISMATCH.1 } else { MISMATCH.0 }).to_string(),
                    _ => (if pt { STOPPED.1 } else { STOPPED.0 }).to_string(),
                };
                set(Stage::Failed(said));
            }
        }
    }
}

pub fn pause_install(data: &Path, id: &str) {
    let mut q = QUEUE.lock().unwrap();
    let running = q.jobs.iter().any(|j| j.id == id && !matches!(j.stage, Stage::Queued | Stage::Paused | Stage::Failed(_)));
    if running {
        if let Some((stop, pause)) = &q.stop {
            pause.store(true, Ordering::SeqCst);
            stop.store(true, Ordering::SeqCst);
        }
    } else if let Some(j) = q.jobs.iter_mut().find(|j| j.id == id && j.stage == Stage::Queued) {
        j.stage = Stage::Paused;
        set_intent(data, id, Some("paused"));
    }
}

pub fn cancel_install(data: &Path, id: &str) {
    let mut q = QUEUE.lock().unwrap();
    let running = q.jobs.iter().any(|j| j.id == id && !matches!(j.stage, Stage::Queued | Stage::Paused | Stage::Failed(_)));
    if running {
        if let Some((stop, _)) = &q.stop {
            stop.store(true, Ordering::SeqCst);
        }
    } else {
        q.jobs.retain(|j| j.id != id);
        let _ = std::fs::remove_dir_all(dir(data, id).join(".partial"));
    }
}

pub fn resume_interrupted(data: &Path, log: crate::Log, changed: fn()) {
    resume_interrupted_with(data, log, changed, || Arc::new(Env { fetch: Box::new(Curl::default()), free: free_bytes, prepare: true }))
}

fn resume_interrupted_with(data: &Path, log: crate::Log, changed: fn(), env: impl Fn() -> Arc<Env>) {
    for m in catalog()["models"].as_array().into_iter().flatten() {
        let id = m["id"].as_str().unwrap_or("");
        if installed(data, m) {
            continue;
        }
        match std::fs::read_to_string(intent_path(data, id)).as_deref() {
            Ok("install") => {
                let _ = start_install_with(data.to_path_buf(), id, log.clone(), changed, env());
            }
            Ok("paused") => QUEUE.lock().unwrap().jobs.push(Job::new(id, Stage::Paused)),
            _ => {}
        }
    }
}

fn footprint(data: &Path, m: &Value) -> (u64, Option<u128>) {
    let d = dir(data, m["id"].as_str().unwrap_or(""));
    let bytes = std::fs::read_dir(&d).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).filter(|x| x.is_file()).map(|x| x.len()).sum();
    let at = std::fs::metadata(d.join(MARKER)).and_then(|x| x.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis());
    (bytes, at)
}

fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

fn job_json(j: &Job) -> Value {
    let (stage, extra) = match &j.stage {
        Stage::Queued => ("queued", json!(null)),
        Stage::Downloading => ("downloading", json!(null)),
        Stage::Retrying(a, of) => ("retrying", json!({"attempt": a, "of": of})),
        Stage::Verifying => ("verifying", json!(null)),
        Stage::Unpacking => ("unpacking", json!(null)),
        Stage::Preparing => ("preparing", json!(null)),
        Stage::Paused => ("paused", json!(null)),
        Stage::Failed(why) => ("failed", json!(why)),
    };
    json!({"stage": stage, "retry": if stage == "retrying" { extra.clone() } else { json!(null) }, "error": if stage == "failed" { extra } else { json!(null) },
           "what": j.what, "done": j.done, "total": j.total, "speed_bps": j.speed.map(|s| s.round()), "eta_s": j.eta_s})
}

pub fn view(data: &Path, pt: bool, test: Option<(String, Value)>) -> Value {
    let def = default_id(data);
    let jobs = jobs();
    let models: Vec<Value> = catalog()["models"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let id = m["id"].as_str().unwrap_or("");
            let size = total_size(m);
            let ok = installed(data, m);
            let (disk, at) = if ok { footprint(data, m) } else { (0, None) };
            json!({
                "id": id,
                "name": if pt { &m["name_pt"] } else { &m["name"] },
                "maker": m["maker"],
                "size_bytes": size,
                "size": gb(size),
                "download": gb(download_size(m)),
                "download_bytes": download_size(m),
                "choice": m["choice"],
                "languages": if pt { &m["languages_pt"] } else { &m["languages"] },
                "live": m["live"],
                "engine": engine(m),
                "fallback_for": m["fallback_for"],
                "on_device": true,
                "licence": m["licence"],
                "licence_url": m["licence_url"],

                "attribution": m["attribution"],
                "released": m["released"],
                "installed": ok,
                "disk": ok.then(|| gb(disk)),
                "installed_ms": at.map(|x| x as u64),
                "partial_bytes": partial_bytes(data, m),
                "default": id == def,
                "job": jobs.iter().find(|j| j.id == id).map(job_json),
                "test": test.as_ref().filter(|t| t.0 == id).map(|t| t.1.clone()),
            })
        })
        .collect();
    json!({ "models": models, "dir": root(data), "free": free_bytes(data).map(gb) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn tmp() -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("rth-dict-models-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn sha(b: &[u8]) -> String {
        Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    fn tiny(a: &[u8], b: &[u8]) -> Value {
        json!({"id": "tiny", "licence_file": "LICENSE-OpenMDW-1.1.txt", "notice": "Tiny by Nobody.", "repo": "x/y", "revision": "r1",
               "files": [{"name": "a.onnx", "size": a.len(), "sha256": sha(a)}, {"name": "tokens.txt", "size": b.len(), "sha256": sha(b)}]})
    }

    struct Serve {
        bytes: Vec<(String, Vec<u8>)>,
        cut: RefCell<Option<String>>,
        calls: RefCell<Vec<String>>,
    }
    impl Fetch for Serve {
        fn fetch(&self, url: &str, part: &Path, _c: &AtomicBool, progress: &dyn Fn(u64), _r: &dyn Fn(u32, u32)) -> Result<(), String> {
            let name = url.rsplit('/').next().unwrap().to_string();
            self.calls.borrow_mut().push(name.clone());
            let body = &self.bytes.iter().find(|(n, _)| *n == name).unwrap().1;
            let have = std::fs::metadata(part).map(|m| m.len() as usize).unwrap_or(0);
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(part).unwrap();
            if self.cut.borrow().as_deref() == Some(name.as_str()) {
                *self.cut.borrow_mut() = None;
                f.write_all(&body[have..have + (body.len() - have) / 2]).unwrap();
                return Err("curl: connection reset".into());
            }
            f.write_all(&body[have..]).unwrap();
            progress(body.len() as u64);
            Ok(())
        }
    }

    fn throttled(body: Vec<u8>, plan: Vec<Option<usize>>) -> (String, Arc<AtomicBool>) {
        use std::io::BufRead;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.bin", l.local_addr().unwrap());
        let done = Arc::new(AtomicBool::new(false));
        let d2 = done.clone();
        l.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            let mut k = 0;
            while !d2.load(Ordering::SeqCst) {
                let Ok((mut c, _)) = l.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                c.set_nonblocking(false).unwrap();

                let (body, d3, step) = (body.clone(), d2.clone(), plan[k.min(plan.len() - 1)]);
                k += 1;
                std::thread::spawn(move || {
                    let mut from = 0;
                    let mut r = std::io::BufReader::new(c.try_clone().unwrap());
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            from = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                        }
                    }
                    let rest = &body[from..];
                    let head = if from > 0 {
                        format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {from}-{}/{}\r\n\r\n", rest.len(), body.len() - 1, body.len())
                    } else {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", rest.len())
                    };
                    let _ = c.write_all(head.as_bytes());
                    match step {
                        Some(n) => {
                            let _ = c.write_all(&rest[..n.min(rest.len())]);
                            let _ = c.flush();

                            let t = std::time::Instant::now();
                            while t.elapsed() < std::time::Duration::from_secs(8) && !d3.load(Ordering::SeqCst) {
                                std::thread::sleep(std::time::Duration::from_millis(20));
                            }
                        }
                        None => {
                            let _ = c.write_all(rest);
                        }
                    }
                });
            }
        });
        (url, done)
    }

    fn fast_curl(attempts: u32) -> Curl {
        Curl { https_only: false, stall_s: 1, attempts, first_wait: std::time::Duration::from_millis(50) }
    }

    #[test]
    fn a_stalled_server_is_retried_and_the_download_resumes_where_it_stopped() {
        let data = tmp();
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();

        let (url, done) = throttled(body.clone(), vec![Some(100_000), Some(50_000), None]);
        let part = data.join("model.bin.part");
        let retries = RefCell::new(vec![]);
        let r = fast_curl(4).fetch(&url, &part, &AtomicBool::new(false), &|_| {}, &|a, of| retries.borrow_mut().push((a, of)));
        done.store(true, Ordering::SeqCst);
        assert_eq!(r, Ok(()));
        assert_eq!(std::fs::read(&part).unwrap(), body, "resumed, never rewound");
        assert_eq!(*retries.borrow(), vec![(2, 4), (3, 4)]);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_server_that_keeps_stalling_fails_after_its_attempts_and_keeps_the_bytes() {
        let data = tmp();
        let body: Vec<u8> = vec![7; 200_000];
        let (url, done) = throttled(body, vec![Some(10_000)]);
        let part = data.join("model.bin.part");
        let r = fast_curl(2).fetch(&url, &part, &AtomicBool::new(false), &|_| {}, &|_, _| {});
        done.store(true, Ordering::SeqCst);
        assert!(matches!(&r, Err(e) if e.contains("(28)")), "curl's stall error: {r:?}");
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 20_000, "both attempts' bytes kept for the next resume");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn speed_comes_from_the_last_two_seconds_and_the_eta_waits_ten() {
        let t0 = std::time::Instant::now();
        let s = std::time::Duration::from_secs;
        let mut j = Job::new("x", Stage::Queued);
        j.bytes("release archive", 0, 100_000_000, t0);
        j.bytes("release archive", 4_000_000, 100_000_000, t0 + s(1));
        assert_eq!((j.stage.clone(), j.speed.map(|v| v as u64), j.eta_s), (Stage::Downloading, Some(4_000_000), None));
        for k in 2..=12 {
            j.bytes("release archive", 4_000_000 * k, 100_000_000, t0 + s(k));
        }

        assert_eq!((j.speed.map(|v| v as u64), j.eta_s), (Some(4_000_000), Some(13)));
        assert_eq!(disk_said(1_200_000_000, 800_000_000, false), "Needs 1.20 GB; 0.80 GB free.");
        assert_eq!(disk_said(1_200_000_000, 800_000_000, true), "Precisa de 1.20 GB; há 0.80 GB livres.");
    }

    struct Trickle;
    impl Fetch for Trickle {
        fn fetch(&self, _url: &str, part: &Path, cancel: &AtomicBool, progress: &dyn Fn(u64), _r: &dyn Fn(u32, u32)) -> Result<(), String> {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(part).unwrap();
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err(CANCELLED.into());
                }
                f.write_all(b"x").unwrap();
                progress(std::fs::metadata(part).unwrap().len());
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }

    fn until(what: &str, f: impl Fn() -> bool) {
        let t = std::time::Instant::now();
        while !f() {
            assert!(t.elapsed() < std::time::Duration::from_secs(10), "timed out waiting for {what}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn the_install_queue_pauses_resumes_cancels_and_checks_the_disk() {
        let data = tmp();
        let id = "parakeet-tdt-0.6b-v3-int8";
        let m = model(catalog(), id).unwrap();
        let log = crate::Log(Arc::new(Mutex::new(None)));
        let env = || Arc::new(Env { fetch: Box::new(Trickle), free: |_| Some(u64::MAX), prepare: false });
        let stage = || jobs().iter().find(|j| j.id == id).map(|j| j.stage.clone());
        start_install_with(data.clone(), id, log.clone(), || {}, env()).unwrap();
        until("downloading", || stage() == Some(Stage::Downloading) && partial_bytes(&data, m) > 3);
        pause_install(&data, id);
        until("paused", || stage() == Some(Stage::Paused));
        let kept = partial_bytes(&data, m);
        assert!(kept > 3);
        assert_eq!(std::fs::read_to_string(intent_path(&data, id)).unwrap(), "paused");

        QUEUE.lock().unwrap().jobs.clear();
        resume_interrupted_with(&data, log.clone(), || {}, env);
        assert_eq!(stage(), Some(Stage::Paused));

        start_install_with(data.clone(), id, log.clone(), || {}, env()).unwrap();
        until("resumed", || partial_bytes(&data, m) > kept + 3);
        assert_eq!(std::fs::read_to_string(intent_path(&data, id)).unwrap(), "install");
        cancel_install(&data, id);
        until("cancelled", || stage().is_none());
        until("its files removed", || !dir(&data, id).join(".partial").exists());

        set_intent(&data, id, Some("install"));
        resume_interrupted_with(&data, log.clone(), || {}, env);
        until("restarted", || stage() == Some(Stage::Downloading));
        cancel_install(&data, id);
        until("cancelled again", || stage().is_none() && !QUEUE.lock().unwrap().running);

        start_install_with(data.clone(), id, log, || {}, Arc::new(Env { fetch: Box::new(Trickle), free: |_| Some(300_000_000), prepare: false })).unwrap();
        until("failed", || matches!(stage(), Some(Stage::Failed(_))));
        let Some(Stage::Failed(why)) = stage() else { unreachable!() };
        assert!(why.starts_with("Needs 1.36 GB; 0.30 GB free.") || why.starts_with("Precisa de 1.36 GB"), "{why}");
        assert_eq!(partial_bytes(&data, m), 0);
        QUEUE.lock().unwrap().jobs.clear();
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_catalog_pins_every_file_and_names_its_licence() {
        let cat = catalog();
        let hex = |h: &str| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit());
        assert!(by_choice("multi").is_some() && by_choice("en").is_some());
        for m in cat["models"].as_array().unwrap() {
            assert!(m["revision"].as_str().unwrap().len() == 40, "{}", m["id"]);
            for (n, s, h) in files(m) {
                assert!(s > 0 && hex(h), "{}: {n}", m["id"]);
            }
            let lic = m["licence_file"].as_str().unwrap();
            assert!(["LICENSE-OpenMDW-1.1.txt", "LICENSE-CC-BY-4.0.txt", "LICENSE-NVIDIA-Open-Model-License.txt"].contains(&lic));
            let notice = m["notice"].as_str().unwrap();
            if m["engine"] == "nemo" {

                assert!(m["repo"].as_str().unwrap().starts_with("nvidia/") && archive(m).is_none(), "{}", m["id"]);
                assert!(files(m).all(|(n, _, _)| n.ends_with(".gguf")) && notice.contains("NVIDIA's own GGUF build"), "{}", m["id"]);
                assert!(["online", "offline"].contains(&m["kind"].as_str().unwrap()));
            } else {

                let (url, size, sha, top) = archive(m).unwrap();
                assert!(url.starts_with("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/") && url.ends_with(&format!("{top}.tar.bz2")) && size > 0 && hex(sha), "{}", m["id"]);
                assert!(notice.contains("ONNX conversion") && notice.contains("sherpa-onnx project"), "{}", m["id"]);
                assert!(["online_transducer", "offline_transducer", "offline_ctc"].contains(&m["kind"].as_str().unwrap()));
            }
            assert!(notice.starts_with("NVIDIA "), "{}", m["id"]);

            if let Some(of) = m["fallback_for"].as_str() {
                assert!(model(cat, of).is_some_and(|g| g["engine"] == "nemo" && m["engine"] == "sherpa" && g["live"] == m["live"]), "{}", m["id"]);
                assert!(m["choice"].is_null());
            }
        }

        assert_eq!(by_choice("multi").unwrap()["id"], "nemotron-3.5-streaming-0.6b-q8");
        assert_eq!(by_choice("en").unwrap()["id"], "parakeet-tdt_ctc-110m-en");
        assert!(OPENMDW.starts_with("OpenMDW License Agreement, version 1.1") && CC_BY.starts_with("Attribution 4.0 International"));
    }

    #[test]
    fn parakeet_unified_en_is_a_pinned_live_cpu_entry_under_the_nvidia_open_model_license() {
        const ATTRIBUTION: &str = "Licensed by NVIDIA Corporation under the NVIDIA Open Model License";
        let cat = catalog();
        let m = model(cat, "parakeet-unified-en-0.6b-int8").expect("the entry");
        assert_eq!((m["engine"].as_str(), m["kind"].as_str(), m["live"].as_bool()), (Some("sherpa"), Some("online_transducer"), Some(true)));
        assert!(m["choice"].is_null() && m["fallback_for"].is_null() && m["lang_option"].is_null(), "an option in Settings, not an onboarding pick or a stand-in");
        assert!(cat["models"].as_array().unwrap().iter().all(|g| g["engine"] != "nemo" || !g["id"].as_str().unwrap().contains("unified")), "no GPU entry");

        assert_eq!(m["source"], json!({"repo": "nvidia/parakeet-unified-en-0.6b", "revision": "fe53cd885760c96b6a5f51a0bfd362cb4584a98b"}));
        assert_eq!(m["repo"], "csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-1120ms");
        assert_eq!(m["revision"], "615bf8c5ad39692dc96cf1105afde73ec3eb2a59");
        let names: Vec<&str> = files(m).map(|(n, _, _)| n).collect();
        assert_eq!(names, ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"]);
        assert_eq!(archive(m).map(|a| a.3), Some("sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-1120ms"));

        assert_eq!(m["licence"], "NVIDIA Open Model License");
        assert_eq!(m["licence_url"], "https://www.nvidia.com/en-us/agreements/enterprise-software/nvidia-open-model-license/");
        assert_eq!(m["licence_file"], "LICENSE-NVIDIA-Open-Model-License.txt");
        assert!(licence_text("LICENSE-NVIDIA-Open-Model-License.txt").unwrap().starts_with("NVIDIA Open Model License Agreement"));
        assert!(licence_text("LICENSE-NVIDIA-Open-Model-License.txt").unwrap().contains(ATTRIBUTION));
        assert_eq!(m["attribution"], ATTRIBUTION);
        assert!(m["notice"].as_str().unwrap().contains(ATTRIBUTION) && m["notice"].as_str().unwrap().contains("fe53cd885760c96b6a5f51a0bfd362cb4584a98b"));
        let data = std::env::temp_dir().join(format!("rth-dict-unified-{}", std::process::id()));
        let row = view(&data, false, None)["models"].as_array().unwrap().iter().find(|r| r["id"] == "parakeet-unified-en-0.6b-int8").cloned().unwrap();
        assert_eq!((row["attribution"].as_str(), row["licence"].as_str(), row["installed"].as_bool()), (Some(ATTRIBUTION), Some("NVIDIA Open Model License"), Some(false)));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn every_licence_file_gets_its_own_text() {
        for (file, title) in [("LICENSE-OpenMDW-1.1.txt", "OpenMDW License Agreement"), ("LICENSE-CC-BY-4.0.txt", "Attribution 4.0 International"), ("LICENSE-NVIDIA-Open-Model-License.txt", "NVIDIA Open Model License Agreement")] {
            assert!(licence_text(file).unwrap().starts_with(title), "{file}");
        }

        assert_eq!(licence_text("LICENSE-Unknown.txt"), None);
    }

    fn tiny_archive(data: &Path, files: &[(&str, &[u8])], pins: &[(&str, &[u8])]) -> (Value, Vec<u8>) {
        let src = data.join("src");
        std::fs::create_dir_all(src.join("tiny-1")).unwrap();
        for (n, b) in files {
            std::fs::write(src.join("tiny-1").join(n), b).unwrap();
        }
        std::fs::write(src.join("tiny-1/README.md"), b"not wanted").unwrap();
        let tb = data.join("tiny-1.tar.bz2");
        assert!(std::process::Command::new("/usr/bin/tar").arg("-cjf").arg(&tb).arg("-C").arg(&src).arg("tiny-1").status().unwrap().success());
        let bytes = std::fs::read(&tb).unwrap();
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_file(&tb);
        let mut m = tiny(pins[0].1, pins[1].1);
        m["archive"] = json!({"url": "https://example.invalid/asr-models/tiny-1.tar.bz2", "size": bytes.len(), "sha256": sha(&bytes), "dir": "tiny-1"});
        (m, bytes)
    }

    #[test]
    fn the_release_archive_is_the_first_source_and_only_its_pinned_files_come_out() {
        let data = tmp();
        let (a, b): (&[u8], &[u8]) = (b"weights", b"tok\n");
        let (m, tar) = tiny_archive(&data, &[("a.onnx", a), ("tokens.txt", b)], &[("a.onnx", a), ("tokens.txt", b)]);
        let serve = Serve { bytes: vec![("tiny-1.tar.bz2".into(), tar)], cut: RefCell::new(None), calls: RefCell::default() };
        let d = install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {}).unwrap();
        assert!(installed(&data, &m));
        assert_eq!(serve.calls.borrow().as_slice(), ["tiny-1.tar.bz2"], "no per-file download");
        assert_eq!(std::fs::read(d.join("a.onnx")).unwrap(), a);
        assert!(!d.join("README.md").exists() && !d.join(".partial").exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn an_archive_that_is_not_the_pinned_one_is_deleted_and_the_files_come_from_the_repo() {
        let data = tmp();
        let (a, b): (&[u8], &[u8]) = (b"weights", b"tok\n");
        let (m, mut tar) = tiny_archive(&data, &[("a.onnx", a), ("tokens.txt", b)], &[("a.onnx", a), ("tokens.txt", b)]);
        let last = tar.len() - 1;
        tar[last] ^= 0xff;
        let serve = Serve { bytes: vec![("tiny-1.tar.bz2".into(), tar), ("a.onnx".into(), a.to_vec()), ("tokens.txt".into(), b.to_vec())], cut: RefCell::new(None), calls: RefCell::default() };
        install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {}).unwrap();
        assert!(installed(&data, &m));
        assert_eq!(serve.calls.borrow().as_slice(), ["tiny-1.tar.bz2", "a.onnx", "tokens.txt"]);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_pinned_archive_whose_file_differs_from_its_pin_falls_back_and_fails_as_a_mismatch() {
        let data = tmp();

        let (m, tar) = tiny_archive(&data, &[("a.onnx", b"WEIGHTS"), ("tokens.txt", b"tok\n")], &[("a.onnx", b"weights"), ("tokens.txt", b"tok\n")]);
        let serve =
            Serve { bytes: vec![("tiny-1.tar.bz2".into(), tar), ("a.onnx".into(), b"WEIGHTS".to_vec()), ("tokens.txt".into(), b"tok\n".to_vec())], cut: RefCell::new(None), calls: RefCell::default() };
        let r = install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {});
        assert!(matches!(r, Err(Failure::Mismatch(_))), "{r:?}");
        assert!(!installed(&data, &m));
        let p = dir(&data, "tiny").join(".partial");
        assert!(!p.join("x").exists() && !p.join(ARCHIVE_PART).exists() && !p.join("a.onnx.part").exists(), "nothing unverified is kept");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn an_install_verifies_then_moves_in_with_the_licence_and_notice() {
        let data = tmp();
        let (a, b) = (b"weights".to_vec(), b"tok\n".to_vec());
        let m = tiny(&a, &b);
        let serve = Serve { bytes: vec![("a.onnx".into(), a.clone()), ("tokens.txt".into(), b.clone())], cut: RefCell::new(None), calls: RefCell::default() };
        let steps = RefCell::new(vec![]);
        let d = install_model(&data, &m, &serve, &AtomicBool::new(false), &|x| steps.borrow_mut().push(x)).unwrap();
        assert!(installed(&data, &m));
        assert_eq!(std::fs::read(d.join("a.onnx")).unwrap(), a);
        assert!(std::fs::read_to_string(d.join("LICENSE-OpenMDW-1.1.txt")).unwrap().starts_with("OpenMDW"));
        assert_eq!(std::fs::read_to_string(d.join("NOTICE.txt")).unwrap(), "Tiny by Nobody.\n");
        assert!(!d.join(".partial").exists());

        assert_eq!(steps.borrow()[0], Step::Bytes { what: "a.onnx".into(), done: 7, total: 11 });
        assert!(steps.borrow().contains(&Step::Verifying { what: "tokens.txt".into() }));
        assert_eq!(serve.calls.borrow().as_slice(), ["a.onnx", "tokens.txt"]);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_cancel_while_verifying_or_unpacking_installs_nothing() {

        let data = tmp();
        let (a, b) = (b"weights".to_vec(), b"tok\n".to_vec());
        let m = tiny(&a, &b);
        let serve = Serve { bytes: vec![("a.onnx".into(), a.clone()), ("tokens.txt".into(), b.clone())], cut: RefCell::new(None), calls: RefCell::default() };
        let cancel = AtomicBool::new(false);
        let r = install_model(&data, &m, &serve, &cancel, &|x| {
            if matches!(x, Step::Verifying { .. }) {
                cancel.store(true, Ordering::SeqCst)
            }
        });
        assert_eq!(r, Err(Failure::Cancelled));
        assert!(!installed(&data, &m) && !dir(&data, "tiny").join("a.onnx").exists() && !dir(&data, "tiny").join(MARKER).exists());
        let _ = std::fs::remove_dir_all(&data);

        let data = tmp();
        let (a, b): (&[u8], &[u8]) = (b"weights", b"tok\n");
        let (m, tar) = tiny_archive(&data, &[("a.onnx", a), ("tokens.txt", b)], &[("a.onnx", a), ("tokens.txt", b)]);
        let serve = Serve { bytes: vec![("tiny-1.tar.bz2".into(), tar)], cut: RefCell::new(None), calls: RefCell::default() };
        let cancel = AtomicBool::new(false);
        let r = install_model(&data, &m, &serve, &cancel, &|x| {
            if x == Step::Unpacking {
                cancel.store(true, Ordering::SeqCst)
            }
        });
        assert_eq!(r, Err(Failure::Cancelled));
        assert!(!installed(&data, &m) && !dir(&data, "tiny").join("a.onnx").exists());
        assert_eq!(serve.calls.borrow().len(), 1, "no per-file fallback after a cancel");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_sha256_mismatch_deletes_the_part_and_installs_nothing() {
        let data = tmp();
        let m = tiny(b"weights", b"tok\n");

        let serve = Serve { bytes: vec![("a.onnx".into(), b"WEIGHTS".to_vec()), ("tokens.txt".into(), b"tok\n".to_vec())], cut: RefCell::new(None), calls: RefCell::default() };
        let r = install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {});
        assert!(matches!(r, Err(Failure::Mismatch(ref e)) if e.starts_with("a.onnx: sha256 ")), "{r:?}");
        let d = dir(&data, "tiny");
        assert!(!installed(&data, &m));
        assert!(!d.join(".partial/a.onnx.part").exists(), "the bad part is gone");
        assert!(!d.join("a.onnx").exists() && !d.join(MARKER).exists());
        assert_eq!(partial_bytes(&data, &m), 0);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_dropped_download_resumes_from_its_part() {
        let data = tmp();
        let a = b"0123456789abcdef".to_vec();
        let m = tiny(&a, b"tok\n");
        let serve = Serve { bytes: vec![("a.onnx".into(), a.clone()), ("tokens.txt".into(), b"tok\n".to_vec())], cut: RefCell::new(Some("a.onnx".into())), calls: RefCell::default() };
        assert!(matches!(install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {}), Err(Failure::Other(_))));
        assert_eq!(partial_bytes(&data, &m), 8, "half the weights kept");
        install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(std::fs::read(dir(&data, "tiny").join("a.onnx")).unwrap(), a);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_tampered_marker_or_a_short_file_is_not_installed() {
        let data = tmp();
        let m = tiny(b"weights", b"tok\n");
        let serve = Serve { bytes: vec![("a.onnx".into(), b"weights".to_vec()), ("tokens.txt".into(), b"tok\n".to_vec())], cut: RefCell::new(None), calls: RefCell::default() };
        let d = install_model(&data, &m, &serve, &AtomicBool::new(false), &|_| {}).unwrap();
        std::fs::write(d.join("a.onnx"), b"weigh").unwrap();
        assert!(!installed(&data, &m));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn with_no_model_installed_nothing_is_ready_and_the_view_says_so() {
        let data = tmp();
        assert!(ready(&data).is_none());
        let v = view(&data, false, None);
        let ms = v["models"].as_array().unwrap();
        assert_eq!(ms.len(), 6);
        assert!(ms.iter().all(|m| m["installed"] == false && m["on_device"] == true));
        assert_eq!(ms.iter().filter(|m| m["default"] == true).count(), 1);
        let col = |k: &str| ms.iter().map(|m| m[k].clone()).collect::<Vec<_>>();
        assert_eq!(col("live"), [true, false, false, true, false, true]);
        assert_eq!(col("engine"), ["nemo", "nemo", "sherpa", "sherpa", "sherpa", "sherpa"]);
        assert_eq!(col("size"), ["0.74 GB", "0.71 GB", "0.46 GB", "0.68 GB", "0.67 GB", "0.66 GB"]);
        assert_eq!(col("download"), ["0.74 GB", "0.71 GB", "0.43 GB", "0.48 GB", "0.49 GB", "0.50 GB"]);
        assert_eq!(view(&data, true, None)["models"][0]["name"], "Nemotron Streaming (ao vivo)");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn the_default_follows_the_macs_first_language() {

        for (tag, want) in [("en-US", "en"), ("en-GB", "en"), ("en", "en"), ("pt-BR", "multi"), ("es-ES", "multi"), ("eu-ES", "multi")] {
            assert_eq!(suggest(Some(tag)), want, "{tag}");
        }
        assert_eq!(suggest(None), "multi");

        assert_eq!(apple_languages_first("(\n    \"pt-BR\",\n    \"en-US\"\n)\n").as_deref(), Some("pt-BR"));
        assert_eq!(apple_languages_first("(\n    \"en-GB\"\n)\n").as_deref(), Some("en-GB"));
        assert_eq!(apple_languages_first("(\n)\n"), None);
        assert_eq!(apple_languages_first("The domain/default pair of (kCFPreferencesAnyApplication, AppleLanguages) does not exist\n"), None);
        assert_eq!(apple_languages_first("(\n    \"en-US; rm -rf\"\n)"), None, "only a plain language tag");
    }

    #[test]
    fn the_default_is_chosen_from_the_catalog_and_remove_takes_everything() {
        let data = tmp();
        assert_eq!(suggest(Some("pt-BR")), "multi");
        assert_eq!(suggest(Some("en-US")), "en");
        assert_eq!(suggest(None), "multi");
        assert_eq!(default_id_for(&data, "auto"), by_choice(suggest(system_lang().as_deref())).unwrap()["id"]);
        set_default(&data, "parakeet-tdt-0.6b-v3-int8").unwrap();
        assert_eq!(default_id_for(&data, "auto"), "parakeet-tdt-0.6b-v3-int8");
        assert!(set_default(&data, "whisper").is_err());
        let d = dir(&data, "parakeet-tdt-0.6b-v3-int8");
        std::fs::create_dir_all(d.join(".partial")).unwrap();
        std::fs::write(d.join(".partial/encoder.int8.onnx.part"), b"x").unwrap();
        remove(&data, "parakeet-tdt-0.6b-v3-int8").unwrap();
        assert!(!d.exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn urls_are_the_pinned_revision() {
        let m = model(catalog(), "nemotron-3.5-streaming-0.6b-int8").unwrap();
        assert_eq!(
            url(m, "tokens.txt"),
            "https://huggingface.co/csukuangfj2/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-160ms-int8-2026-06-11/resolve/b3a4dbde84fba1a13cb4270e6730b525ac6a2db6/tokens.txt"
        );
    }
}

#[cfg(test)]
mod t16_model_choice {
    use super::*;
    use crate::dictation::Models;
    #[test]
    fn t16_pt_choice_overrides_saved_default_and_exposes_install_route() {
        let data=std::env::temp_dir().join(format!("T16-choice-{}",std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        for stale in ["nemotron-3.5-streaming-0.6b-q8","parakeet-tdt_ctc-110m-en"] {
            set_default(&data,stale).unwrap();
            assert_eq!(default_id_for(&data,"pt"),PT_MODEL);
            assert_eq!(choice_for("multi","pt").unwrap()["id"],PT_MODEL);
            assert!(ready_for(&data,"pt").is_none());
            assert_eq!(default_id_for(&data,"en"),stale,"non-PT saved compatible choice remains available");
        }
        std::fs::write(default_path(&data),b"{\"model\":\"removed-stale-model\"}").unwrap();
        assert_eq!(default_id_for(&data,"pt-BR"),PT_MODEL);
        assert_eq!(crate::dictation::Installed::new(data.clone()).state_for("pt"),crate::dictation::ModelState::Missing);
    }
    #[test]
    fn t16_pt_downloading_state_uses_required_model_not_saved_default() {
        use crate::dictation::Models;

        if std::env::var_os("T16_DOWNLOAD_CHILD").is_none() {
            let out=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","dictation_models::t16_model_choice::t16_pt_downloading_state_uses_required_model_not_saved_default","--test-threads","4"]).env("T16_DOWNLOAD_CHILD","1").output().unwrap();
            assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stdout));return;
        }
        let data=std::env::temp_dir().join(format!("T16-download-{}",std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        set_default(&data,"nemotron-3.5-streaming-0.6b-q8").unwrap();
        let mut job=Job::new(PT_MODEL,Stage::Downloading);job.done=25;job.total=100;
        QUEUE.lock().unwrap().jobs.push(job);
        let installed=crate::dictation::Installed::new(data);
        assert_eq!(installed.state_for("pt"),crate::dictation::ModelState::Downloading(25));
        assert_eq!(crate::nemo::language("pt"),Some("pt-BR"));
    }
}
