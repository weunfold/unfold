

use crate::{owned_dir, owned_open, owned_replace, Log, Open};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const CHECK_EVERY_MS: u64 = 6 * 3600 * 1000;
pub const FIRST_CHECK_AFTER_MS: u64 = 20_000;
pub const RETRY_FIRST_MS: u64 = 5 * 60 * 1000;
pub const IDLE_AFTER_MS: u64 = 15 * 60 * 1000;

pub const GATE_WITHIN: Duration = Duration::from_secs(5 * 60);

pub const MAX_FAILED_ATTEMPTS: u32 = 3;

pub const CRASH_LOOP_STARTS: usize = 5;
pub const CRASH_LOOP_MS: u64 = 2 * 60 * 1000;
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3600);
const NOTES_MAX_LINES: usize = 12;
pub const NOTE_LINE_MAX: usize = 120;

pub const RELAUNCH_ARG: &str = "--relaunched-after-update";

pub fn retry_after_ms(failures: u32) -> u64 {
    match failures {
        0 => CHECK_EVERY_MS,
        n => RETRY_FIRST_MS.saturating_mul(3u64.saturating_pow(n - 1)).min(CHECK_EVERY_MS),
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schedule {
    pub launched_ms: u64,
    pub last_attempt_ms: Option<u64>,
    pub failures: u32,
}

pub fn next_due(s: &Schedule, auto: bool) -> Option<u64> {
    if !auto {
        return None;
    }
    Some(match s.last_attempt_ms {
        None => s.launched_ms + FIRST_CHECK_AFTER_MS,
        Some(t) => t + retry_after_ms(s.failures),
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Activity {
    pub focused: bool,

    pub since_ms: u64,
    pub model_download: bool,
}

pub fn idle(a: &Activity, now_ms: u64) -> bool {
    !a.focused && now_ms.saturating_sub(a.since_ms) >= IDLE_AFTER_MS && !a.model_download
}

pub fn wanted(floor: &semver::Version, release: &semver::Version, bad: &[String]) -> bool {
    release > floor && !bad.iter().filter_map(|b| semver::Version::parse(b.trim_start_matches('v')).ok()).any(|b| &b == release)
}

pub fn note_problem(line: &str) -> Option<&'static str> {
    let l = line.trim();
    if l.chars().count() > NOTE_LINE_MAX {
        return Some("longer than 120 characters");
    }
    let lower = l.to_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return Some("a URL");
    }
    let subject = lower.trim_start_matches(['-', '*', ' ']);
    let kinds = ["feat", "fix", "chore", "refactor", "docs", "test", "tests", "ci", "build", "perf", "style", "revert", "wip"];
    if subject.starts_with("merge branch") || subject.starts_with("merge pull request") || kinds.iter().any(|k| {
        subject.strip_prefix(k).is_some_and(|r| {
            let r = if r.starts_with('(') { r.find(')').map(|i| &r[i + 1..]).unwrap_or(r) } else { r };
            r.trim_start_matches('!').starts_with(':')
        })
    }) {
        return Some("a commit subject");
    }
    if lower.contains("private key") || lower.contains("password=") || lower.contains("token=") || lower.contains("api_key") {
        return Some("a secret");
    }
    for t in l.split(|c: char| c.is_whitespace() || "()[]{},;\"'`".contains(c)) {
        let t = t.trim_end_matches(['.', ':', '!', '?']);
        if t.is_empty() {
            continue;
        }
        if ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "AKIA"].iter().any(|p| t.starts_with(p)) {
            return Some("a secret");
        }
        if t.len() >= 32 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=_-".contains(&b)) {
            return Some("a secret");
        }
        if (7..=40).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_hexdigit()) && t.bytes().any(|b| b.is_ascii_digit()) && t.bytes().any(|b| b.is_ascii_alphabetic()) {
            return Some("a commit sha");
        }
        if t.starts_with('/') || t.starts_with("~/") || t.starts_with("./") || t.contains('\\') || (t.contains('/') && t.contains('.')) {
            return Some("a path");
        }
    }
    None
}

pub fn notes_for<'a>(notes: &'a str, notes_pt: &'a str, pt: bool) -> &'a str {
    if pt && !notes_pt.trim().is_empty() {
        notes_pt
    } else {
        notes
    }
}

pub fn clean_notes(notes: &str) -> String {
    notes.lines().map(str::trim_end).filter(|l| !l.trim().is_empty() && note_problem(l).is_none()).take(NOTES_MAX_LINES).collect::<Vec<_>>().join("\n")
}

pub fn schema_of(instance: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut dirs = vec![(instance.to_path_buf(), 0)];
    while let Some((d, depth)) = dirs.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?.flatten() {
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if m.is_dir() && depth < 1 {
                dirs.push((p, depth + 1));
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !m.is_file() || !matches!(ext, "json" | "jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let v: Value = match ext {
                "json" => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))?,
                _ => match text.lines().next() {
                    Some(l) => serde_json::from_str(l).unwrap_or(Value::Null),
                    None => Value::Null,
                },
            };
            let rel = p.strip_prefix(instance).unwrap_or(&p).display().to_string();
            for (k, x) in v.as_object().into_iter().flatten() {
                if k.to_lowercase().contains("schema") && (x.is_string() || x.is_number()) {
                    out.insert(format!("{rel}#{k}"), x.to_string());
                }
            }
        }
    }
    Ok(out)
}

#[derive(Debug, PartialEq)]
pub enum Verdict {

    Normal,

    Attempt,

    Rollback(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub from: String,
    pub to: String,
    pub app: PathBuf,
    pub previous: Option<PathBuf>,
    pub snapshot: Option<PathBuf>,

    pub schema: Option<BTreeMap<String, String>>,
    pub notes: String,

    pub notes_pt: String,
    pub at_ms: u64,

    pub starts: Vec<u64>,

    pub failed: u32,

    pub open: bool,
    pub promoted: bool,
    pub notice_shown: bool,
    pub rolled_back: bool,
}

impl Installed {
    pub fn new(from: &str, to: &str, app: &Path, notes: &str) -> Installed {
        Installed { from: from.into(), to: to.into(), app: app.into(), previous: None, snapshot: None, schema: None, notes: notes.into(), notes_pt: String::new(), at_ms: now_ms(), starts: vec![], failed: 0, open: false, promoted: false, notice_shown: false, rolled_back: false }
    }
    pub fn to_json(&self) -> Value {
        json!({"from": self.from, "to": self.to, "app": self.app, "previous": self.previous, "snapshot": self.snapshot, "schema": self.schema, "notes": self.notes, "notes_pt": self.notes_pt,
               "at_ms": self.at_ms, "starts": self.starts, "failed": self.failed, "open": self.open, "promoted": self.promoted, "notice_shown": self.notice_shown, "rolled_back": self.rolled_back})
    }
    pub fn from_json(v: &Value) -> Option<Installed> {
        Some(Installed {
            from: v["from"].as_str()?.into(),
            to: v["to"].as_str()?.into(),
            app: PathBuf::from(v["app"].as_str()?),
            previous: v["previous"].as_str().map(PathBuf::from),
            snapshot: v["snapshot"].as_str().map(PathBuf::from),
            schema: serde_json::from_value(v["schema"].clone()).ok(),
            notes: v["notes"].as_str().unwrap_or("").into(),
            notes_pt: v["notes_pt"].as_str().unwrap_or("").into(),
            at_ms: v["at_ms"].as_u64().unwrap_or(0),
            starts: v["starts"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default(),
            failed: v["failed"].as_u64().unwrap_or(0) as u32,
            open: v["open"].as_bool().unwrap_or(false),
            promoted: v["promoted"].as_bool().unwrap_or(false),
            notice_shown: v["notice_shown"].as_bool().unwrap_or(false),
            rolled_back: v["rolled_back"].as_bool().unwrap_or(false),
        })
    }
    fn watched(&self, running: &str) -> bool {
        self.to == running && !self.promoted && !self.rolled_back
    }
}

pub fn start_attempt(rec: &Installed, running: &str, now: u64) -> (Verdict, Installed) {
    let mut r = rec.clone();
    if !r.watched(running) {
        return (Verdict::Normal, r);
    }
    if r.open {
        r.failed += 1;
        r.open = false;
    }
    r.starts.push(now);
    let recent = r.starts.iter().filter(|t| now.saturating_sub(**t) <= CRASH_LOOP_MS).count();
    let v = if recent >= CRASH_LOOP_STARTS {
        Verdict::Rollback("a crash loop")
    } else if r.failed >= MAX_FAILED_ATTEMPTS {
        Verdict::Rollback("three failed health checks")
    } else {
        r.open = true;
        Verdict::Attempt
    };
    if r.previous.is_none() && matches!(v, Verdict::Rollback(_)) {
        r.open = true;
        return (Verdict::Attempt, r);
    }
    (v, r)
}

pub fn notice_js(version: &str, notes: &str) -> String {
    let v = json!({"version": version, "notes": clean_notes(notes)});
    format!(
        "(()=>{{try{{const v={v};if(typeof window.__rthUpdated==='function'){{window.__rthUpdated(v);return}}\
const pt=String(window.__RTH_LANG||'').toLowerCase().startsWith('pt');\
const d=document.createElement('div');d.setAttribute('role','status');\
d.style.cssText='position:fixed;left:50%;bottom:22px;transform:translateX(-50%);z-index:2147483647;width:max-content;max-width:min(520px,calc(100vw - 32px));padding:12px 14px 12px 16px;border-radius:12px;background:rgba(30,30,32,.94);color:#f5f5f7;font:13px/1.45 -apple-system,BlinkMacSystemFont,system-ui,sans-serif;box-shadow:0 10px 30px rgba(0,0,0,.28);display:flex;gap:14px;align-items:flex-start';\
const c=document.createElement('div');const t=document.createElement('div');t.style.fontWeight='600';t.textContent=(pt?'Atualizado para ':'Updated to ')+v.version;c.appendChild(t);\
if(v.notes){{const n=document.createElement('div');n.style.cssText='margin-top:4px;opacity:.82;white-space:pre-line;max-height:40vh;overflow:auto';n.textContent=v.notes;c.appendChild(n)}}\
const b=document.createElement('button');b.type='button';b.textContent='OK';b.style.cssText='flex:none;font:inherit;font-weight:500;color:#fff;background:rgba(255,255,255,.16);border:0;border-radius:6px;padding:3px 12px;cursor:default';b.onclick=()=>d.remove();\
d.appendChild(c);d.appendChild(b);(document.body||document.documentElement).appendChild(d)}}catch(e){{}}}})()"
    )
}

pub struct Txn<'a> {
    name: &'static str,
    log: &'a Log,
    undo: Vec<(&'static str, Box<dyn FnOnce() -> Result<(), String> + 'a>)>,
}

impl<'a> Txn<'a> {
    pub fn new(name: &'static str, log: &'a Log) -> Txn<'a> {
        Txn { name, log, undo: Vec::new() }
    }
    pub fn step<T>(&mut self, what: &'static str, run: impl FnOnce() -> Result<T, String>, undo: impl FnOnce() -> Result<(), String> + 'a) -> Result<T, String> {
        let v = run().map_err(|e| format!("{what}: {e}"))?;
        self.undo.push((what, Box::new(undo)));
        Ok(v)
    }

    pub fn abort(self, why: &str) -> Result<(), String> {
        self.log.line(&format!("update: {} failed ({why}); undoing {} step(s)", self.name, self.undo.len()));
        let mut stuck = Vec::new();
        for (what, u) in self.undo.into_iter().rev() {
            match u() {
                Ok(()) => self.log.line(&format!("update: undid: {what}")),
                Err(e) => {
                    self.log.line(&format!("update: could not undo {what}: {e}"));
                    stuck.push(format!("{what}: {e}"));
                }
            }
        }
        if stuck.is_empty() { Ok(()) } else { Err(stuck.join("; ")) }
    }
    pub fn commit(self) {}
}

fn dir(data: &Path) -> PathBuf {
    data.join("app").join("updater")
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn write_json(data: &Path, name: &str, v: &Value) -> Result<(), String> {
    let d = owned_dir(data, &dir(data))?;
    owned_replace(&d.join(name), &d.join(format!("{name}.tmp")), format!("{v:#}\n").as_bytes()).map_err(|e| format!("{name}: {e}"))
}

pub fn load_installed(data: &Path) -> Option<Installed> {
    Installed::from_json(&read_json(&dir(data).join("installed.json"))?)
}

pub fn save_installed(data: &Path, r: &Installed) -> Result<(), String> {
    write_json(data, "installed.json", &r.to_json())
}

fn load_state(data: &Path) -> Value {
    read_json(&dir(data).join("state.json")).filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn update_state(data: &Path, f: impl FnOnce(&mut Value)) -> Result<(), String> {
    let mut v = load_state(data);
    f(&mut v);
    write_json(data, "state.json", &v)
}

pub fn bad_versions(data: &Path) -> Vec<String> {
    load_state(data)["bad_versions"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

pub fn frozen(data: &Path) -> Option<String> {
    load_state(data)["frozen"].as_str().map(str::to_string)
}

fn freeze(data: &Path, why: &str, said: Value, log: &Log) {
    log.line(&format!("update: frozen, needs owner: {why}"));
    let _ = update_state(data, |s| {
        s["frozen"] = json!(why);
        s["frozen_said"] = said;
    });
}

fn frozen_said(st: &Value, pt: bool) -> Option<String> {
    st["frozen"].as_str()?;
    let w = &st["frozen_said"];
    let (v, b) = (w["version"].as_str().unwrap_or("?"), w["back_to"].as_str().unwrap_or("?"));
    Some(match (w["kind"].as_str(), pt) {
        (Some("install_stuck"), false) => format!("Installing Unfold {v} failed, and Unfold could not fully undo it."),
        (Some("install_stuck"), true) => format!("A instalação da versão {v} falhou, e Unfold não conseguiu desfazê-la por completo."),
        (Some("rollback_stuck"), false) => format!("Going back from Unfold {v} failed, and Unfold could not fully undo it."),
        (Some("rollback_stuck"), true) => format!("A volta da versão {v} falhou, e Unfold não conseguiu desfazê-la por completo."),
        (Some("state_unknown"), false) => format!("Unfold went back from {v} to {b}, but could not tell whether {v} changed the data format of this Mac’s instance."),
        (Some("state_unknown"), true) => format!("Unfold voltou da {v} para a {b}, mas não conseguiu saber se a {v} mudou o formato dos dados deste Mac."),
        (_, false) => "An update step could not finish, so Unfold stopped updating until you look.".into(),
        (_, true) => "Uma etapa da atualização não terminou, então Unfold parou de atualizar até você olhar.".into(),
    })
}

fn error_said(raw: &str, pt: bool) -> &'static str {
    let low = raw.to_lowercase();
    let (en, pt_said) = if low.contains("signature") || low.contains("signed for version") {
        ("The update didn’t pass its signature check, so Unfold left it out.", "A atualização não passou na verificação de assinatura, então Unfold não a usou.")
    } else if raw.starts_with("check: ") {
        ("Couldn’t read the list of new versions on github.com.", "Não consegui ler a lista de versões novas no github.com.")
    } else if raw.starts_with("download of ") {
        ("The download stopped before it finished.", "O download parou antes de terminar.")
    } else {
        ("Something went wrong with the update.", "Algo deu errado com a atualização.")
    };
    if pt {
        pt_said
    } else {
        en
    }
}

pub fn resume(data: &Path) -> Result<(), String> {
    update_state(data, |s| {
        if let Some(o) = s.as_object_mut() {
            o.remove("frozen");
            o.remove("frozen_said");
        }
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn remove(p: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(p).map_err(|e| format!("{}: {e}", p.display())),
        Ok(_) => std::fs::remove_file(p).map_err(|e| format!("{}: {e}", p.display())),
        Err(_) => Ok(()),
    }
}

fn clone_tree(from: &Path, to: &Path) -> Result<(), String> {
    let ok = Command::new("/bin/cp").args(["-c", "-R", "-p"]).arg(from).arg(to).env_clear().stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        let _ = remove(to);
        Err(format!("could not copy {} to {}", from.display(), to.display()))
    }
}

fn move_tree(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let ok = Command::new("/usr/bin/ditto").arg(from).arg(to).env_clear().stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if !ok {
        let _ = remove(to);
        return Err(format!("could not move {} to {}", from.display(), to.display()));
    }
    remove(from)
}

#[derive(Clone)]
pub struct Ready {
    pub update: Update,
    pub file: PathBuf,
    pub version: String,
    pub notes: String,

    pub notes_pt: String,
}

pub struct CheckCfg {

    pub current: semver::Version,

    pub exe: PathBuf,
    pub data: PathBuf,

    pub endpoints: Option<Vec<tauri::Url>>,
}

pub async fn check_and_stage<R: Runtime>(app: &AppHandle<R>, cfg: &CheckCfg, progress: &(dyn Fn(u64, Option<u64>) + Sync)) -> Result<Option<Ready>, String> {
    let bad = bad_versions(&cfg.data);
    let floor = cfg.current.clone();
    let mut b = app.updater_builder().executable_path(&cfg.exe).timeout(CHECK_TIMEOUT).version_comparator(move |_, rel| wanted(&floor, &rel.version, &bad));
    if let Some(e) = &cfg.endpoints {
        b = b.endpoints(e.clone()).map_err(|e| e.to_string())?;
    }
    let Some(mut update) = b.build().map_err(|e| e.to_string())?.check().await.map_err(|e| format!("check: {e}"))? else { return Ok(None) };
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    let done = std::sync::atomic::AtomicU64::new(0);
    let bytes = update
        .download(
            |n, total| {
                let d = done.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed) + n as u64;
                progress(d, total);
            },
            || {},
        )
        .await
        .map_err(|e| format!("download of {}: {e}", update.version))?;
    let staged = owned_dir(&cfg.data, &dir(&cfg.data).join("staged"))?;

    for e in std::fs::read_dir(&staged).map_err(|e| e.to_string())?.flatten() {
        let _ = std::fs::remove_file(e.path());
    }
    let file = staged.join(format!("Unfold-{}.app.tar.gz", update.version));
    owned_open(&file, Open::Truncate).and_then(|mut f| f.write_all(&bytes).and_then(|_| f.sync_all())).map_err(|e| format!("{}: {e}", file.display()))?;
    let notes = clean_notes(update.body.as_deref().unwrap_or(""));
    let notes_pt = clean_notes(update.raw_json["notes_pt"].as_str().unwrap_or(""));
    Ok(Some(Ready { version: update.version.clone(), update, file, notes, notes_pt }))
}

pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension()? == "app").then(|| app.to_path_buf())
}

fn writable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else { return false };
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

const APP_FILES: [&str; 2] = ["settings.json", "models.json"];

pub fn install(ready: &Ready, app: &Path, data: &Path, instance: Option<&Path>, from: &str, log: &Log) -> Result<Installed, String> {
    let parent = app.parent().ok_or("the app has no folder")?;
    if !writable(parent) || !writable(app) || !writable(&app.join("Contents")) {
        return Err(format!("{} is not writable by this user: the update waits (no administrator prompt)", parent.display()));
    }
    let name = app.file_name().ok_or("the app has no name")?.to_owned();
    let prev_dir = owned_dir(data, &dir(data).join("previous"))?;
    let previous = prev_dir.join(&name);
    let aside = prev_dir.join(format!("{}.undo", name.to_string_lossy()));
    let snap = dir(data).join("snapshot");
    let record = dir(data).join("installed.json");
    remove(&previous)?;
    remove(&snap)?;
    let bytes = std::fs::read(&ready.file).map_err(|e| format!("{}: {e}", ready.file.display()))?;
    let mut rec = Installed::new(from, &ready.version, app, &ready.notes);
    rec.notes_pt = ready.notes_pt.clone();
    let mut t = Txn::new("install", log);
    macro_rules! step {
        ($what:expr, $run:expr, $undo:expr) => {
            match t.step($what, $run, $undo) {
                Ok(v) => v,
                Err(e) => {
                    if let Err(stuck) = t.abort(&e) {
                        freeze(data, &format!("the install of {} failed and could not be fully undone: {stuck}", ready.version), json!({"kind": "install_stuck", "version": ready.version}), log);
                    }
                    return Err(e);
                }
            }
        };
    }
    let schema = step!("snapshot the instance state and the app's settings", || {
        owned_dir(data, &snap.join("app"))?;
        let mut schema = None;
        if let Some(i) = instance.filter(|i| i.is_dir()) {
            schema = Some(schema_of(i)?);
            clone_tree(i, &snap.join("instance"))?;
        }
        for f in APP_FILES {
            let p = data.join("app").join(f);
            if p.is_file() {
                clone_tree(&p, &snap.join("app").join(f))?;
            }
        }
        Ok(schema)
    }, || remove(&snap));
    rec.schema = schema;
    rec.snapshot = Some(snap.clone());
    step!("keep the previous bundle", || clone_tree(app, &previous), || remove(&previous));
    rec.previous = Some(previous.clone());
    step!("swap the new bundle in", || ready.update.install(&bytes).map_err(|e| e.to_string()), || {

        remove(&aside)?;
        move_tree(app, &aside)?;
        match clone_tree(&previous, app) {
            Ok(()) => remove(&aside),
            Err(e) => {
                let _ = move_tree(&aside, app);
                Err(e)
            }
        }
    });
    step!("record the install", || save_installed(data, &rec), || remove(&record));
    t.commit();
    let _ = std::fs::remove_file(&ready.file);
    log.line(&format!("update: installed {} over {from} at {} (previous kept, state snapshot taken)", rec.to, app.display()));
    Ok(rec)
}

pub fn rollback(data: &Path, instance: Option<&Path>, rec: &Installed, why: &str, log: &Log) -> Result<(), String> {
    let prev = rec.previous.as_ref().filter(|p| p.join("Contents/Info.plist").is_file()).ok_or("no previous app to roll back to")?;
    let name = rec.app.file_name().ok_or("the app has no name")?;
    let failed_dir = owned_dir(data, &dir(data).join("failed"))?;
    let failed = failed_dir.join(name);
    let state_aside = failed_dir.join("instance");
    remove(&failed)?;
    remove(&state_aside)?;

    let state = match (instance, &rec.schema, &rec.snapshot) {
        (None, _, _) => Ok(false),
        (Some(i), Some(was), Some(_)) => schema_of(i).map(|now| &now != was),
        (Some(_), _, _) => Err("no schema record or snapshot from the install".to_string()),
    };
    let mut done = rec.clone();
    done.rolled_back = true;
    done.open = false;
    done.previous = None;
    let mut t = Txn::new("rollback", log);
    macro_rules! step {
        ($what:expr, $run:expr, $undo:expr) => {
            match t.step($what, $run, $undo) {
                Ok(v) => v,
                Err(e) => {
                    if let Err(stuck) = t.abort(&e) {
                        freeze(data, &format!("the rollback of {} failed and could not be fully undone: {stuck}", rec.to), json!({"kind": "rollback_stuck", "version": rec.to}), log);
                    }
                    return Err(e);
                }
            }
        };
    }
    step!("set the failed bundle aside", || move_tree(&rec.app, &failed), || move_tree(&failed, &rec.app));
    step!("put the previous bundle back", || move_tree(prev, &rec.app), || move_tree(&rec.app, prev));
    let restore = matches!(state, Ok(true));
    if let (true, Some(i), Some(snap)) = (restore, instance, rec.snapshot.as_deref()) {
        step!("set the changed instance state aside", || move_tree(i, &state_aside), || move_tree(&state_aside, i));
        step!("restore the instance state from the snapshot", || clone_tree(&snap.join("instance"), i), || remove(i));
        step!("restore the app's settings", || {
            for f in APP_FILES {
                let s = snap.join("app").join(f);
                if s.is_file() {
                    let d = data.join("app").join(f);
                    remove(&d)?;
                    clone_tree(&s, &d)?;
                }
            }
            Ok(())
        }, || Ok(()));
    }
    step!("record the rollback", || save_installed(data, &done), || save_installed(data, rec));
    step!("mark the version bad", || update_state(data, |s| {
        let mut bad: Vec<Value> = s["bad_versions"].as_array().cloned().unwrap_or_default();
        if !bad.contains(&json!(rec.to)) {
            bad.push(json!(rec.to));
        }
        s["bad_versions"] = json!(bad);
    }), || Ok(()));
    t.commit();
    log.line(&format!("update: rolled {} back to {} ({why}); {} is marked bad; instance state {}", rec.to, rec.from, rec.to, if restore { "restored from the snapshot (its schema changed)" } else { "kept" }));
    if let Err(e) = state {
        freeze(data, &format!("rolled {} back to {}, but could not tell whether {} changed the instance's data format ({e})", rec.to, rec.from, rec.to), json!({"kind": "state_unknown", "version": rec.to, "back_to": rec.from}), log);
    }
    Ok(())
}

pub fn promote(data: &Path, running: &str, why: &str, log: &Log) -> bool {
    let Some(mut r) = load_installed(data).filter(|r| r.watched(running)) else { return false };
    r.promoted = true;
    r.open = false;
    for p in [r.previous.take(), r.snapshot.take(), Some(dir(data).join("failed"))].into_iter().flatten() {
        if let Err(e) = remove(&p) {
            log.line(&format!("update: {e}"));
        }
    }
    if save_installed(data, &r).is_err() {
        return false;
    }
    log.line(&format!("update: {running} passed its health gate ({why}): promoted; the previous bundle and the snapshot are deleted"));
    true
}

pub fn gate_missed(data: &Path, running: &str, log: &Log) -> bool {
    let Some(mut r) = load_installed(data).filter(|r| r.watched(running) && r.open) else { return false };
    r.open = false;
    r.failed += 1;
    let _ = save_installed(data, &r);
    log.line(&format!("update: {running} missed its health gate ({} of {MAX_FAILED_ATTEMPTS})", r.failed));
    r.failed >= MAX_FAILED_ATTEMPTS && r.previous.is_some()
}

pub fn attempt_ended(data: &Path, running: &str) {
    if let Some(mut r) = load_installed(data).filter(|r| r.watched(running) && r.open) {
        r.open = false;
        let _ = save_installed(data, &r);
    }
}

#[derive(Debug, PartialEq)]
pub enum Launch {
    Continue,

    Reopen(PathBuf),
}

pub fn on_launch(data: &Path, instance: Option<&Path>, running: &str, log: &Log) -> Launch {
    if let Ok(entries) = std::fs::read_dir(dir(data).join("staged")) {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let Some(rec) = load_installed(data) else { return Launch::Continue };
    let (v, counted) = start_attempt(&rec, running, now_ms());
    if v == Verdict::Normal {
        return Launch::Continue;
    }
    if let Err(e) = save_installed(data, &counted) {
        log.line(&format!("update: {e}"));
    }
    match v {
        Verdict::Rollback(why) => match rollback(data, instance, &counted, why, log) {
            Ok(()) => Launch::Reopen(counted.app.clone()),
            Err(e) => {
                log.line(&format!("update: the rollback of {running} failed: {e}; continuing on it"));
                Launch::Continue
            }
        },
        _ => {
            log.line(&format!("update: {running} attempt {} (failed so far: {}); the health gate has {} min", counted.starts.len(), counted.failed, GATE_WITHIN.as_secs() / 60));
            Launch::Continue
        }
    }
}

pub fn take_notice(data: &Path, running: &str, pt: bool) -> Option<(String, String)> {
    let mut r = load_installed(data).filter(|r| r.to == running && !r.notice_shown && !r.rolled_back)?;
    r.notice_shown = true;
    save_installed(data, &r).ok()?;
    let notes = notes_for(&r.notes, &r.notes_pt, pt).to_string();
    Some((r.to, notes))
}

fn reopen_when_gone(app: &Path, background: bool) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let open = if background { "exec /usr/bin/open -g \"$2\" --args \"$3\"" } else { "exec /usr/bin/open \"$2\"" };
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("i=0; while kill -0 \"$1\" 2>/dev/null && [ \"$i\" -lt 300 ]; do sleep 0.1; i=$((i+1)); done; {open}"))
        .arg("unfold-reopen")
        .arg(std::process::id().to_string())
        .arg(app)
        .arg(RELAUNCH_ARG)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("reopen: {e}"))
}

pub fn reopen(app: &Path) -> Result<(), String> {
    reopen_when_gone(app, false)
}

struct Rt {
    data: PathBuf,
    instance: Option<PathBuf>,
    app: PathBuf,
    version: String,
    log: Log,
    schedule: Schedule,
    activity: Activity,
    phase: &'static str,
    progress: Option<(u64, Option<u64>)>,
    ready: Option<Ready>,
    error: Option<String>,
    force: bool,

    page_loaded: bool,
    agent_ready: bool,

    needs_agent: bool,

    rollback: bool,

    reopen: bool,
}

static RT: Mutex<Option<Rt>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();

static FOCUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

static OFF_REASON: Mutex<Option<Off>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Off {
    NotInstalled,
    NotRelease,
    PathUnknown,
    NoStateFolder,
}
impl Off {
    fn said(self, pt: bool) -> &'static str {
        match (self, pt) {
            (Off::NotInstalled, false) => "Updates apply to the installed app.",
            (Off::NotInstalled, true) => "As atualizações valem para o app instalado.",
            (Off::NotRelease, false) => "This build's version is not a release version.",
            (Off::NotRelease, true) => "A versão desta build não é uma versão de lançamento.",
            (Off::PathUnknown, false) => "Unfold can’t tell where this app is, so it does not update.",
            (Off::PathUnknown, true) => "Unfold não sabe onde este app está, então não atualiza.",
            (Off::NoStateFolder, false) => "This run has no state folder of its own, so it does not update.",
            (Off::NoStateFolder, true) => "Esta execução não tem uma pasta de estado própria, então não atualiza.",
        }
    }
}
type Quit = Arc<dyn Fn() + Send + Sync>;
static QUIT: Mutex<Option<Quit>> = Mutex::new(None);

pub fn unavailable(why: Off) {
    *OFF_REASON.lock().unwrap() = Some(why);
}

pub fn status() -> Value {
    let auto = crate::settings::current().auto_update;
    let pt = crate::settings::pt();
    let g = RT.lock().unwrap();
    let Some(rt) = g.as_ref() else {
        return json!({"supported": false, "auto": auto, "reason": OFF_REASON.lock().unwrap().unwrap_or(Off::NotInstalled).said(pt)});
    };
    let st = load_state(&rt.data);
    let rec = load_installed(&rt.data);
    let installed = rec.as_ref().filter(|r| r.to == rt.version && !r.rolled_back).map(|r| json!({"version": r.to, "from": r.from, "notes": notes_for(&r.notes, &r.notes_pt, pt), "at_ms": r.at_ms, "promoted": r.promoted, "failed": r.failed}));
    let rolled_back = rec.as_ref().filter(|r| r.rolled_back).map(|r| json!({"version": r.to, "back_to": r.from}));
    json!({
        "supported": true,
        "auto": auto,
        "phase": if st["frozen"].is_string() { "needs_owner" } else { rt.phase },
        "needs_owner": frozen_said(&st, pt),
        "progress": rt.progress.map(|(d, t)| json!({"done": d, "total": t})),
        "available": rt.ready.as_ref().map(|r| json!({"version": r.version, "notes": notes_for(&r.notes, &r.notes_pt, pt)})),
        "error": rt.error.as_deref().map(|e| error_said(e, pt)),
        "last_checked_ms": st["last_ok_ms"],
        "next_ms": next_due(&rt.schedule, auto),
        "installed": installed,
        "rolled_back": rolled_back,
    })
}

pub fn focus(label: &str, focused: bool) {
    let any = {
        let mut f = FOCUSED.lock().unwrap();
        f.retain(|l| l != label);
        if focused {
            f.push(label.into());
        }
        !f.is_empty()
    };
    if let Some(rt) = RT.lock().unwrap().as_mut() {
        if rt.activity.focused != any {
            rt.activity = Activity { focused: any, since_ms: now_ms(), ..rt.activity };
        }
    }
}

pub fn wake() {
    WAKE.notify_all();
}

pub fn check_now() {
    if let Some(rt) = RT.lock().unwrap().as_mut() {
        rt.force = true;
    }
    WAKE.notify_all();
}

pub fn resume_updates() -> Result<(), String> {
    let data = RT.lock().unwrap().as_ref().map(|rt| rt.data.clone()).ok_or("this run does not update")?;
    resume(&data)?;
    if let Some(rt) = RT.lock().unwrap().as_ref() {
        rt.log.line("update: resumed by the owner");
    }
    check_now();
    Ok(())
}

fn gate_check() {
    let pass = {
        let g = RT.lock().unwrap();
        let Some(rt) = g.as_ref() else { return };
        (rt.page_loaded && (rt.agent_ready || !rt.needs_agent)).then(|| (rt.data.clone(), rt.version.clone(), rt.log.clone(), rt.needs_agent))
    };
    if let Some((data, version, log, agent)) = pass {
        promote(&data, &version, if agent { "the page loaded, agentd answered and the chat connected" } else { "the page loaded; no models chosen yet" }, &log);
    }
}

pub fn agent_ready() {
    if let Some(rt) = RT.lock().unwrap().as_mut() {
        rt.agent_ready = true;
    }
    gate_check();
}

pub fn page_loaded(win: &tauri::WebviewWindow) {
    let (data, version, log) = {
        let mut g = RT.lock().unwrap();
        let Some(rt) = g.as_mut() else { return };
        if std::mem::replace(&mut rt.page_loaded, true) {
            return;
        }
        (rt.data.clone(), rt.version.clone(), rt.log.clone())
    };
    if let Some((v, notes)) = take_notice(&data, &version, crate::settings::pt()) {
        log.line(&format!("update: showing \"Updated to {v}\""));
        let _ = win.eval(notice_js(&v, &notes));
    }
    gate_check();
}

pub fn on_quit(system: bool) {
    let Some((ready, app, data, instance, version, log, rollback_now, reopen)) =
        RT.lock().unwrap().as_mut().map(|rt| (rt.ready.take(), rt.app.clone(), rt.data.clone(), rt.instance.clone(), rt.version.clone(), rt.log.clone(), rt.rollback, rt.reopen))
    else {
        return;
    };
    if rollback_now {
        if let Some(rec) = load_installed(&data).filter(|r| r.to == version && !r.rolled_back) {
            match rollback(&data, instance.as_deref(), &rec, "three failed health checks", &log) {
                Ok(()) if !system => {
                    if let Err(e) = reopen_when_gone(&app, true) {
                        log.line(&format!("update: {e}"));
                    }
                }
                Ok(()) => {}
                Err(e) => log.line(&format!("update: the rollback failed: {e}")),
            }
        }
        return;
    }
    attempt_ended(&data, &version);
    let Some(ready) = ready else { return };
    if system {
        log.line(&format!("update: {} is staged; the system is quitting, so it installs at a later quit", ready.version));
        return;
    }
    if let Some(why) = frozen(&data) {
        log.line(&format!("update: not installed: updates are frozen ({why})"));
        return;
    }
    match install(&ready, &app, &data, instance.as_deref(), &version, &log) {
        Ok(_) if reopen => {
            if let Err(e) = reopen_when_gone(&app, true) {
                log.line(&format!("update: {e}"));
            }
        }
        Ok(_) => {}
        Err(e) => log.line(&format!("update: not installed: {e}")),
    }
}

fn set(f: impl FnOnce(&mut Rt)) {
    if let Some(rt) = RT.lock().unwrap().as_mut() {
        f(rt);
    }
}

pub fn start(app: AppHandle, data: PathBuf, instance: Option<PathBuf>, exe: PathBuf, version: String, log: Log, quit: Quit) {
    let Some(bundle) = bundle_of(&exe) else {
        unavailable(Off::NotInstalled);
        return;
    };
    let Ok(current) = semver::Version::parse(&version) else {
        unavailable(Off::NotRelease);
        return;
    };
    let now = now_ms();
    let attempt = load_installed(&data).is_some_and(|r| r.watched(&version) && r.open);
    *RT.lock().unwrap() = Some(Rt {
        data: data.clone(),
        instance,
        app: bundle,
        version: version.clone(),
        log: log.clone(),
        schedule: Schedule { launched_ms: now, ..Default::default() },
        activity: Activity { focused: false, since_ms: now, model_download: false },
        phase: "idle",
        progress: None,
        ready: None,
        error: None,
        force: false,
        page_loaded: false,
        agent_ready: false,
        needs_agent: crate::models::load_choice(&data).is_some(),
        rollback: false,
        reopen: false,
    });
    *QUIT.lock().unwrap() = Some(quit.clone());
    if attempt {
        let (d, v, l, q) = (data.clone(), version.clone(), log.clone(), quit.clone());
        std::thread::spawn(move || {
            std::thread::sleep(GATE_WITHIN);
            if gate_missed(&d, &v, &l) {
                l.line(&format!("update: {v} failed its health gate {MAX_FAILED_ATTEMPTS} times: quitting to roll back"));
                set(|rt| {
                    rt.rollback = true;
                    rt.reopen = true;
                    rt.phase = "rolling_back";
                });
                q();
            }
        });
    }
    std::thread::spawn(move || loop {

        let (force, staged) = {
            let auto = crate::settings::current().auto_update;
            let mut g = RT.lock().unwrap();
            let rt = g.as_mut().unwrap();
            if rt.rollback {
                return;
            }
            let now = now_ms();
            let due = if frozen(&data).is_some() { None } else { next_due(&rt.schedule, auto) };
            let until_due = due.map(|d| d.saturating_sub(now)).unwrap_or(3_600_000);
            let wait = if rt.force { 0 } else if rt.ready.is_some() { until_due.min(60_000) } else { until_due };
            if wait > 0 {
                let _ = WAKE.wait_timeout(g, Duration::from_millis(wait)).unwrap();
                continue;
            }
            let force = std::mem::replace(&mut rt.force, false);
            let check = force || due.is_some_and(|d| d <= now);
            (force, rt.ready.as_ref().map(|r| r.version.clone()).filter(|_| !check))
        };
        if let Some(v) = staged {

            let auto = crate::settings::current().auto_update;
            let mut a = RT.lock().unwrap().as_ref().unwrap().activity;
            a.model_download = std::fs::read_dir(data.join("models")).map(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "part"))).unwrap_or(false);
            if auto && frozen(&data).is_none() && idle(&a, now_ms()) {
                log.line(&format!("update: idle for {} min: quitting to install {v}, then reopening", IDLE_AFTER_MS / 60_000));
                set(|rt| {
                    rt.reopen = true;
                    rt.phase = "installing";
                });
                if let Some(q) = QUIT.lock().unwrap().clone() {
                    q();
                }
                return;
            }
            continue;
        }
        if let Some(why) = frozen(&data) {
            if force {
                log.line(&format!("update: check skipped: frozen ({why})"));
            }
            crate::settings::refresh(&app);
            continue;
        }
        let floor = RT.lock().unwrap().as_ref().and_then(|rt| rt.ready.as_ref().and_then(|r| semver::Version::parse(&r.version).ok())).filter(|s| s > &current).unwrap_or_else(|| current.clone());
        set(|rt| {
            rt.phase = "checking";
            rt.schedule.last_attempt_ms = Some(now_ms());
        });
        crate::settings::refresh(&app);
        let cfg = CheckCfg { current: floor, exe: exe.clone(), data: data.clone(), endpoints: None };
        let h = app.clone();
        let progress = move |d: u64, t: Option<u64>| {
            set(|rt| {
                rt.phase = "downloading";
                rt.progress = Some((d, t));
            });
            static LAST: Mutex<u64> = Mutex::new(0);
            let mut l = LAST.lock().unwrap();
            if now_ms() > *l + 1000 {
                *l = now_ms();
                crate::settings::refresh(&h);
            }
        };
        let r = tauri::async_runtime::block_on(check_and_stage(&app, &cfg, &progress));
        match r {
            Ok(found) => {
                let _ = update_state(&data, |s| {
                    s["last_ok_ms"] = json!(now_ms());
                    s["last_error"] = Value::Null;
                });
                match &found {
                    Some(r) => log.line(&format!("update: {} downloaded and verified; it installs when Unfold is idle or quits", r.version)),
                    None => log.line("update: up to date"),
                }
                set(|rt| {
                    rt.schedule.failures = 0;
                    rt.error = None;
                    rt.progress = None;
                    if let Some(f) = found {
                        rt.ready = Some(f);
                    }
                    rt.phase = if rt.ready.is_some() { "ready" } else { "up_to_date" };
                });
            }
            Err(e) => {
                let e: String = e.chars().take(300).collect();
                let _ = update_state(&data, |s| s["last_error"] = json!(e));
                set(|rt| {
                    rt.schedule.failures += 1;
                    rt.error = Some(e.clone());
                    rt.progress = None;
                    rt.phase = if rt.ready.is_some() { "ready" } else { "error" };
                    log.line(&format!("update: check failed ({} in a row; next try in {} min): {e}", rt.schedule.failures, retry_after_ms(rt.schedule.failures) / 60_000));
                });
            }
        }
        crate::settings::refresh(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_freeze_is_worded_in_either_language_without_its_detail() {
        let st = json!({"frozen": "rolled 0.3.3 back to 0.3.2, but could not tell whether 0.3.3 changed the instance's data format (verify.json: torn)", "frozen_said": {"kind": "state_unknown", "version": "0.3.3", "back_to": "0.3.2"}});
        let en = frozen_said(&st, false).unwrap();
        let pt = frozen_said(&st, true).unwrap();
        assert!(en.starts_with("Unfold went back from 0.3.3 to 0.3.2") && pt.starts_with("Unfold voltou da 0.3.3 para a 0.3.2"), "{en} / {pt}");
        assert!(!en.contains("torn") && !pt.contains("torn"), "the detail stays in the log");
        for kind in ["install_stuck", "rollback_stuck"] {
            let st = json!({"frozen": "x", "frozen_said": {"kind": kind, "version": "0.3.3"}});
            assert!(frozen_said(&st, false).unwrap().contains("0.3.3") && frozen_said(&st, true).unwrap().contains("0.3.3"), "{kind}");
        }
        let older = json!({"frozen": "the install of 0.3.3 failed and could not be fully undone: x"});
        assert_eq!(frozen_said(&older, true).unwrap(), "Uma etapa da atualização não terminou, então Unfold parou de atualizar até você olhar.");
        assert_eq!(frozen_said(&json!({}), true), None, "not frozen");
    }

    #[test]
    fn an_update_error_reaches_settings_as_a_headline() {
        let said = |raw: &str| (error_said(raw, false), error_said(raw, true));
        for raw in [
            "download of 0.3.3: The signature verification failed",
            "download of 0.3.3: The signature was created with a different key than the one provided",
            "download of 0.3.3: The update was signed for version 0.3.1 but the update endpoint announced version 0.3.3.",
        ] {
            assert_eq!(said(raw).1, "A atualização não passou na verificação de assinatura, então Unfold não a usou.", "{raw}");
        }
        assert_eq!(said("check: error sending request for url (https://github.com/weunfold/unfold/releases/latest/download/latest.json)").0, "Couldn’t read the list of new versions on github.com.");
        assert_eq!(said("download of 0.3.3: connection reset").1, "O download parou antes de terminar.");
        let other = said("/Users/x/Library/staged/Unfold-0.3.3.app.tar.gz: No space left on device");
        assert_eq!(other, ("Something went wrong with the update.", "Algo deu errado com a atualização."), "no path reaches the page");
    }

    #[test]
    fn an_off_updater_says_why_in_either_language() {
        for off in [Off::NotInstalled, Off::NotRelease, Off::PathUnknown, Off::NoStateFolder] {
            assert_ne!(off.said(false), off.said(true), "{off:?}");
        }
    }

    #[test]
    fn checks_at_launch_then_every_6_hours() {
        let s = Schedule { launched_ms: 1_000, ..Default::default() };
        assert_eq!(next_due(&s, true), Some(1_000 + FIRST_CHECK_AFTER_MS), "the first check is at launch");
        let s = Schedule { last_attempt_ms: Some(50_000), ..s };
        assert_eq!(next_due(&s, true), Some(50_000 + 6 * 3600 * 1000));
    }

    #[test]
    fn failures_back_off_and_cap_at_the_interval() {
        let mins: Vec<u64> = (1..=6).map(|n| retry_after_ms(n) / 60_000).collect();
        assert_eq!(mins, vec![5, 15, 45, 135, 360, 360]);
        let s = Schedule { launched_ms: 0, last_attempt_ms: Some(10), failures: 2 };
        assert_eq!(next_due(&s, true), Some(10 + 15 * 60_000));
        assert_eq!(retry_after_ms(u32::MAX), CHECK_EVERY_MS, "no overflow");
    }

    #[test]
    fn automatic_updates_off_schedules_nothing() {
        let s = Schedule { launched_ms: 0, last_attempt_ms: None, failures: 3 };
        assert_eq!(next_due(&s, false), None);
    }

    #[test]
    fn idle_needs_15_unfocused_minutes_and_no_download() {
        let a = Activity { focused: false, since_ms: 0, model_download: false };
        assert!(!idle(&a, IDLE_AFTER_MS - 1));
        assert!(idle(&a, IDLE_AFTER_MS));
        assert!(!idle(&Activity { focused: true, ..a }, IDLE_AFTER_MS * 4), "a focused window is never idle");
        assert!(!idle(&Activity { model_download: true, ..a }, IDLE_AFTER_MS * 4));
    }

    fn rec() -> Installed {
        let mut r = Installed::new("0.3.2", "0.3.3", Path::new("/x/Unfold.app"), "");
        r.previous = Some("/d/previous/Unfold.app".into());
        r
    }

    #[test]
    fn three_failed_attempts_roll_back() {
        let mut r = rec();
        let mut t = 0;
        for n in 1..=3 {
            t += 10 * 60_000;
            let (v, next) = start_attempt(&r, "0.3.3", t);
            assert_eq!(v, Verdict::Attempt, "attempt {n}");
            r = next;
            r.open = false;
            r.failed += 1;
        }
        let (v, _) = start_attempt(&r, "0.3.3", t + 10 * 60_000);
        assert_eq!(v, Verdict::Rollback("three failed health checks"));
    }

    #[test]
    fn a_crash_before_the_gate_counts_as_a_failed_attempt() {
        let (_, r) = start_attempt(&rec(), "0.3.3", 0);
        assert!(r.open);
        let (_, r) = start_attempt(&r, "0.3.3", 10 * 60_000);
        assert_eq!(r.failed, 1, "the first attempt never closed: a crash");
        let mut quit = r.clone();
        quit.open = false;
        let (_, r2) = start_attempt(&quit, "0.3.3", 20 * 60_000);
        assert_eq!(r2.failed, 1);
    }

    #[test]
    fn a_crash_loop_rolls_back_before_three_gate_failures_would() {
        let mut r = rec();
        let mut v = Verdict::Normal;
        for i in 0..CRASH_LOOP_STARTS as u64 {
            let mut c = r.clone();
            c.open = false;
            (v, r) = start_attempt(&c, "0.3.3", i * 20_000);
        }
        assert_eq!(v, Verdict::Rollback("a crash loop"), "5 starts in 80 s");
        assert_eq!(r.failed, 0);
    }

    #[test]
    fn promoted_rolled_back_or_other_versions_are_not_watched_and_no_copy_means_no_rollback() {
        assert_eq!(start_attempt(&Installed { promoted: true, ..rec() }, "0.3.3", 0).0, Verdict::Normal);
        assert_eq!(start_attempt(&Installed { rolled_back: true, ..rec() }, "0.3.3", 0).0, Verdict::Normal);
        assert_eq!(start_attempt(&rec(), "0.3.4", 0).0, Verdict::Normal);
        assert_eq!(start_attempt(&Installed { failed: 9, previous: None, ..rec() }, "0.3.3", 0).0, Verdict::Attempt);
        let r = Installed { failed: 2, starts: vec![1, 2], ..rec() };
        assert_eq!(Installed::from_json(&r.to_json()), Some(r));
    }

    #[test]
    fn only_the_newest_and_never_a_bad_version() {
        let v = |s: &str| semver::Version::parse(s).unwrap();
        assert!(wanted(&v("0.3.2"), &v("0.3.3"), &[]));
        assert!(!wanted(&v("0.3.3"), &v("0.3.3"), &[]));
        assert!(!wanted(&v("0.3.3"), &v("0.3.2"), &[]), "never a downgrade");
        assert!(!wanted(&v("0.3.2"), &v("0.3.3"), &["0.3.3".into()]));
        assert!(wanted(&v("0.3.2"), &v("0.3.4"), &["v0.3.3".into()]), "a later fix is wanted again");
        assert!(!wanted(&v("0.3.4"), &v("0.3.4"), &[]), "with 0.3.4 staged, 0.3.4 again is not news");
        assert!(wanted(&v("0.3.4"), &v("0.3.5"), &[]), "a newer one replaces the staged one");
    }

    #[test]
    fn release_notes_are_human_lines_only() {
        let ok = ["Unfold now keeps itself up to date.", "- A Settings window (⌘,) for updates and privacy", "Replies are faster, and/or shorter."];
        for l in ok {
            assert_eq!(note_problem(l), None, "{l}");
        }
        let bad = [
            ("See https://github.com/weunfold/unfold", "a URL"),
            ("Moved files to /Users/x/models", "a path"),
            ("Edited src-tauri/src/main.rs", "a path"),
            ("fix(updater): handle the race", "a commit subject"),
            ("- feat: settings window", "a commit subject"),
            ("Merge pull request #12 from x", "a commit subject"),
            ("Built from 3f9c2a71d04e", "a commit sha"),
            ("token sk-proj-abc123", "a secret"),
            ("key RWQXRDQQYgeyLxzARz3IxuR86yypfABCDEFGH", "a secret"),
            (&"x".repeat(121), "longer than 120 characters"),
        ];
        for (l, why) in bad {
            assert_eq!(note_problem(l), Some(why), "{l}");
        }
        assert_eq!(clean_notes("Faster replies.\n\nfix: x\nSee www.example.com\nA calmer splash."), "Faster replies.\nA calmer splash.");
    }

    #[test]
    fn schema_markers_are_read_without_following_links_and_a_torn_json_is_doubt() {
        let d = std::env::temp_dir().join(format!("unfold-schema-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(d.join("agentd")).unwrap();
        std::fs::write(d.join("agentd/state.json"), r#"{"state_schema": 1, "x": 2}"#).unwrap();
        std::fs::write(d.join("agentd/memory-acts.jsonl"), "{\"schema\":\"rth-memory-acts/v1\"}\ntorn {\n").unwrap();
        std::os::unix::fs::symlink("/", d.join("link")).unwrap();
        let s = schema_of(&d).unwrap();
        assert_eq!(s.len(), 2, "{s:?}");
        assert_eq!(s["agentd/state.json#state_schema"], "1");
        std::fs::write(d.join("agentd/state.json"), r#"{"state_schema": 2}"#).unwrap();
        assert_ne!(schema_of(&d).unwrap(), s, "a migration shows");
        std::fs::write(d.join("verify.json"), "{torn").unwrap();
        assert!(schema_of(&d).is_err(), "unreadable: in doubt");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_step_undoes_the_done_ones_newest_first() {
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        let log = Log::open_in(None, &std::env::temp_dir().join("unfold-txn-log"));
        let order = std::cell::RefCell::new(Vec::new());
        let mut t = Txn::new("test", &log);
        let r: Result<(), String> = (|| {
            t.step("a", || Ok(()), || { order.borrow_mut().push("undo a"); Ok(()) })?;
            t.step("b", || Ok(()), || { order.borrow_mut().push("undo b"); Ok(()) })?;
            t.step("c", || Err::<(), _>("boom".to_string()), || { order.borrow_mut().push("undo c"); Ok(()) })?;
            Ok(())
        })();
        assert_eq!(r.unwrap_err(), "c: boom");
        assert!(t.abort("c failed").is_ok());
        assert_eq!(*order.borrow(), vec!["undo b", "undo a"], "only done steps, newest first");
        let mut t = Txn::new("test", &log);
        t.step("a", || Ok(()), || Err("stuck".to_string())).unwrap();
        assert_eq!(t.abort("x").unwrap_err(), "a: stuck", "a failed undo needs the owner");
    }

    #[test]
    fn portuguese_notes_show_in_portuguese_and_fall_back_to_english() {
        assert_eq!(notes_for("Faster replies.", "Respostas mais rápidas.", true), "Respostas mais rápidas.");
        assert_eq!(notes_for("Faster replies.", "Respostas mais rápidas.", false), "Faster replies.");
        assert_eq!(notes_for("Faster replies.", "", true), "Faster replies.", "no notes_pt: English");
        assert_eq!(notes_for("Faster replies.", " \n ", true), "Faster replies.", "blank notes_pt: English");
        let mut r = Installed::new("0.3.2", "0.3.3", Path::new("/x/Unfold.app"), "Faster replies.");
        r.notes_pt = "Respostas mais rápidas.".into();
        assert_eq!(Installed::from_json(&r.to_json()).unwrap().notes_pt, r.notes_pt, "kept in installed.json");
        let older = json!({"from": "0.3.1", "to": "0.3.2", "app": "/x/Unfold.app", "notes": "n"});
        assert_eq!(Installed::from_json(&older).unwrap().notes_pt, "", "a 0.3.2 record has none");
        let accented = format!("{} {}", "á".repeat(60), "é".repeat(59));
        assert!(note_problem(&accented).is_none(), "120 characters, not bytes (the publish script counts the same)");
        assert!(note_problem(&format!("{accented}é")).is_some());
    }

    #[test]
    fn the_notice_uses_the_page_hook_and_shows_clean_notes_as_text() {
        let js = notice_js("0.3.3", "<img src=x onerror=alert(1)>\nfix: internal");
        assert!(js.contains("window.__rthUpdated(v)"));
        assert!(js.contains("n.textContent=v.notes") && !js.contains("innerHTML"));
        assert!(js.contains(r#""notes":"<img src=x onerror=alert(1)>""#), "data, inside JSON, commit subjects dropped");
    }

    #[test]
    fn the_shipped_updater_config_is_github_https_signed_versions_and_our_key() {
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let c: tauri_plugin_updater::Config = serde_json::from_value(conf["plugins"]["updater"].clone()).unwrap();
        assert_eq!(c.endpoints.iter().map(|u| u.as_str()).collect::<Vec<_>>(), ["https://github.com/weunfold/unfold/releases/latest/download/latest.json"]);
        assert!(c.require_signed_version && !c.allow_downgrades && !c.dangerous_insecure_transport_protocol && !c.dangerous_accept_invalid_certs);
        let pk = String::from_utf8(base64_decode(&c.pubkey)).unwrap();
        assert!(pk.starts_with("untrusted comment: minisign public key: ") && pk.lines().count() == 2, "a minisign public key, base64 (Tauri's format)");
        assert_eq!(conf["app"]["macOSPrivateApi"], true, "the Settings window's vibrancy needs a transparent window");
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        let t: Vec<u8> = s.bytes().filter(|b| !b"=\n\r".contains(b)).collect();
        let val = |c: u8| match c { b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' => 62, _ => 63 } as u32;
        t.chunks(4).flat_map(|ch| {
            let n = ch.iter().enumerate().fold(0u32, |n, (i, &c)| n | val(c) << (18 - 6 * i));
            (0..ch.len() - 1).map(move |i| (n >> (16 - 8 * i)) as u8)
        }).collect()
    }

    #[test]
    fn the_bundle_is_found_from_its_executable() {
        assert_eq!(bundle_of(Path::new("/Applications/Unfold.app/Contents/MacOS/rth-app")), Some(PathBuf::from("/Applications/Unfold.app")));
        assert_eq!(bundle_of(Path::new("/target/release/rth-app")), None);
    }

    fn mock(pk: &str) -> tauri::App<tauri::test::MockRuntime> {
        let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
        ctx.config_mut().plugins.0.insert("updater".into(), json!({"pubkey": pk, "requireSignedVersion": true, "dangerousInsecureTransportProtocol": true}));
        tauri::test::mock_builder().plugin(tauri_plugin_updater::Builder::new().build()).build(ctx).unwrap()
    }

    #[test]
    #[ignore]
    fn e2e_real_archive_installs_over_a_scratch_bundle() {
        let dir = PathBuf::from(std::env::var("UNFOLD_REAL_DIR").expect("UNFOLD_REAL_DIR"));
        let base = std::env::var("UNFOLD_REAL_BASE").expect("UNFOLD_REAL_BASE");
        let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let log = Log::open_in(None, &dir.join("log"));
        let app_path = dir.join("Applications/Unfold.app");
        let app = mock(conf["plugins"]["updater"]["pubkey"].as_str().unwrap());
        let cfg = CheckCfg { current: semver::Version::parse("0.3.0").unwrap(), exe: app_path.join("Contents/MacOS/rth-app"), data: data.clone(), endpoints: Some(vec![format!("{base}/latest.json").parse().unwrap()]) };
        let t = std::time::Instant::now();
        let ready = tauri::async_runtime::block_on(check_and_stage(app.handle(), &cfg, &|_, _| {})).unwrap().expect("0.3.1 is announced");
        println!("REAL downloaded and verified {} against the shipped public key in {:.1} s", ready.version, t.elapsed().as_secs_f64());
        let t = std::time::Instant::now();
        let rec = install(&ready, &app_path, &data, None, "0.3.0", &log).unwrap();
        println!("REAL installed {} over the placeholder in {:.1} s (previous kept: {})", rec.to, t.elapsed().as_secs_f64(), rec.previous.is_some());
    }

    #[test]
    #[ignore]
    fn e2e_local_update_dry_test() {
        let root = PathBuf::from(std::env::var("UNFOLD_E2E_DIR").expect("UNFOLD_E2E_DIR"));
        let base = std::env::var("UNFOLD_E2E_BASE").expect("UNFOLD_E2E_BASE");
        let pubkey = std::env::var("UNFOLD_E2E_PUBKEY").expect("UNFOLD_E2E_PUBKEY");
        let wrong_pubkey = std::env::var("UNFOLD_E2E_WRONG_PUBKEY").expect("UNFOLD_E2E_WRONG_PUBKEY");
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        let data = root.join("data");
        let inst = data.join("instances/test");
        std::fs::create_dir_all(inst.join("agentd")).unwrap();
        std::fs::write(inst.join("agentd/state.json"), r#"{"state_schema": 1}"#).unwrap();
        std::fs::create_dir_all(data.join("app")).unwrap();
        std::fs::write(data.join("app/settings.json"), r#"{"auto_update": true}"#).unwrap();
        let log = Log::open_in(None, &root.join("log"));
        let app_path = root.join("Applications/Unfold.app");
        let exe = app_path.join("Contents/MacOS/rth-app");
        let ver = |app: &Path| {
            let out = Command::new("/usr/bin/plutil").args(["-extract", "CFBundleShortVersionString", "raw"]).arg(app.join("Contents/Info.plist")).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let schema = || std::fs::read_to_string(inst.join("agentd/state.json")).unwrap();
        let say = |m: &str| println!("E2E {m}");
        let cfg = |current: &str, endpoint: &str| CheckCfg { current: semver::Version::parse(current).unwrap(), exe: exe.clone(), data: data.clone(), endpoints: Some(vec![format!("{base}/{endpoint}").parse().unwrap()]) };
        let none = |_: u64, _: Option<u64>| {};
        let app = mock(&pubkey);
        let check = |current: &str, endpoint: &str| tauri::async_runtime::block_on(check_and_stage(app.handle(), &cfg(current, endpoint), &none));
        assert_eq!(ver(&app_path), "0.3.2-test");
        say(&format!("installed: {} is {}", app_path.display(), ver(&app_path)));

        let e = check("0.3.2-test", "tampered/latest.json").err().expect("a tampered archive must fail");
        say(&format!("tampered archive refused: {e}"));
        let wrong = mock(&wrong_pubkey);
        let e = tauri::async_runtime::block_on(check_and_stage(wrong.handle(), &cfg("0.3.2-test", "latest.json"), &none)).err().expect("a wrong key must fail");
        say(&format!("wrong public key refused: {e}"));
        let e = check("0.3.2-test", "mislabeled/latest.json").err().expect("a signature for another version must fail");
        say(&format!("signature for another version refused: {e}"));
        assert_eq!(std::fs::read_dir(data.join("app/updater/staged")).map(|d| d.count()).unwrap_or(0), 0, "nothing staged");
        assert!(check("0.3.3-test", "latest.json").unwrap().is_none());
        say("0.3.3-test checking 0.3.3-test: up to date");

        let ready = check("0.3.2-test", "latest.json").unwrap().expect("0.3.3-test is announced");
        say(&format!("found {}, signature verified, notes as shown: {:?}", ready.version, ready.notes));
        assert!(!ready.notes.contains("fix:") && ready.notes.contains("Auto-update test notes"), "the linter dropped the commit subject: {:?}", ready.notes);
        let rec = install(&ready, &app_path, &data, Some(&inst), "0.3.2-test", &log).unwrap();
        say(&format!("installed (transaction): app is {}, previous kept ({}), snapshot {:?}", ver(&app_path), rec.previous.as_deref().map(ver).unwrap_or_default(), rec.schema));
        assert_eq!(ver(&app_path), "0.3.3-test");
        assert!(rec.snapshot.as_ref().unwrap().join("instance/agentd/state.json").is_file() && rec.snapshot.as_ref().unwrap().join("app/settings.json").is_file());
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue);
        say(&format!("Portuguese notes as shown: {:?}", ready.notes_pt));
        assert!(!ready.notes_pt.contains("fix:") && ready.notes_pt.contains("Notas de teste"), "notes_pt is linted too: {:?}", ready.notes_pt);
        assert_eq!(load_installed(&data).unwrap().notes_pt, ready.notes_pt, "the install record keeps both");
        let n = take_notice(&data, "0.3.3-test", true).expect("the notice");
        say(&format!("next launch shows once: \"Updated to {}\", in Portuguese: {:?}", n.0, n.1));
        assert!(n.1.contains("Notas de teste"), "PT shows notes_pt");
        assert_eq!(take_notice(&data, "0.3.3-test", true), None, "only once");
        assert!(promote(&data, "0.3.3-test", "test: the page loaded, agentd answered, the chat connected", &log));
        assert!(!rec.previous.as_ref().unwrap().exists() && !rec.snapshot.as_ref().unwrap().exists(), "promotion deletes the kept bundle and the snapshot");
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue);
        say("promoted: the previous bundle and the snapshot are deleted; later launches are normal");

        let reset_to_0_3_2 = || {

            let _ = remove(&app_path);
            clone_tree(&root.join("src-0.3.2/Unfold.app"), &app_path).unwrap();
            let _ = remove(&dir(&data).join("installed.json"));
            update_state(&data, |s| s["bad_versions"] = json!([])).unwrap();
        };
        reset_to_0_3_2();
        let ready = check("0.3.2-test", "latest.json").unwrap().unwrap();
        install(&ready, &app_path, &data, Some(&inst), "0.3.2-test", &log).unwrap();
        for i in 1..=3 {
            assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue);
            let third = gate_missed(&data, "0.3.3-test", &log);
            assert_eq!(third, i == 3);
        }
        let r = load_installed(&data).unwrap();
        rollback(&data, Some(&inst), &r, "three failed health checks", &log).unwrap();
        say(&format!("three missed gates: rolled back in place; app is {} again, state kept ({}), bad: {:?}", ver(&app_path), schema().trim(), bad_versions(&data)));
        assert_eq!(ver(&app_path), "0.3.2-test");
        assert_eq!(bad_versions(&data), vec!["0.3.3-test"]);
        assert!(check("0.3.2-test", "latest.json").unwrap().is_none(), "a bad version is never retried");
        say("0.3.3-test is never retried");

        reset_to_0_3_2();
        let ready = check("0.3.2-test", "latest.json").unwrap().unwrap();
        install(&ready, &app_path, &data, Some(&inst), "0.3.2-test", &log).unwrap();
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue);
        std::fs::write(inst.join("agentd/state.json"), r#"{"state_schema": 2}"#).unwrap();
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue, "crash 1");
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Continue, "crash 2");
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Reopen(app_path.clone()), "crash 3: rolled back at this start");
        say(&format!("schema changed then 3 crashes: rolled back at start; app {}, state restored: {}", ver(&app_path), schema().trim()));
        assert_eq!(ver(&app_path), "0.3.2-test");
        assert_eq!(schema().trim(), r#"{"state_schema": 1}"#);
        assert_eq!(frozen(&data), None);

        reset_to_0_3_2();
        let ready = check("0.3.2-test", "latest.json").unwrap().unwrap();
        install(&ready, &app_path, &data, Some(&inst), "0.3.2-test", &log).unwrap();
        std::fs::write(inst.join("verify.json"), "{torn").unwrap();
        let mut r = load_installed(&data).unwrap();
        r.failed = 3;
        save_installed(&data, &r).unwrap();
        assert_eq!(on_launch(&data, Some(&inst), "0.3.3-test", &log), Launch::Reopen(app_path.clone()));
        let why = frozen(&data).expect("frozen");
        assert_eq!(load_state(&data)["frozen_said"]["kind"], "state_unknown", "Settings words it");
        say(&format!("in doubt: rolled back, state untouched, needs owner: {why}"));
        assert_eq!(std::fs::read_to_string(inst.join("verify.json")).unwrap(), "{torn");
        resume(&data).unwrap();
        assert_eq!(frozen(&data), None);
        say("the owner resumed updates");
        say("PASS");
    }
}
