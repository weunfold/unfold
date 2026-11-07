

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

#[cfg(test)]
mod child_fixture;
mod capture;
mod chat;

#[allow(dead_code)]
mod conversation_log;
#[allow(dead_code)]
mod voice_front;
#[cfg(feature = "voice_pane")]
mod front_native;
mod dictation;
#[cfg(feature = "voice_pane")]
mod conversation;
#[cfg(all(feature = "voice_pane", feature = "menu_bar"))]
mod conversation_hotkey;
#[cfg(feature = "voice_pane")]
mod conversation_state;
#[cfg(feature = "voice_pane")]
mod conversation_input;
mod onnx_cpu;
mod bc_detector;
mod bc_playout;
mod turn_features;
mod turn_detect;
#[cfg(feature = "voice_pane")]
mod turn_shadow;
#[cfg(feature = "voice_pane")]
mod floor_decide;
#[cfg(feature = "voice_pane")]
mod conversation_overlap;
#[cfg(feature = "voice_pane")]
mod duplex_audio;
#[cfg(feature = "voice_pane")]
mod doubletalk;
#[cfg(feature = "voice_pane")]
mod conversation_text;
#[cfg(test)]
mod dictation_fleurs;
mod dictation_models;
mod dictation_settle;
mod answer;
mod dsh;
mod dsh_control;
mod dsh_pack;
mod dsh_ports;
mod dsh_sockdir;
mod nemo;
mod effects;
mod flash;
mod models;
mod omni_child;
#[cfg(feature = "self_pane")]
mod self_link;
mod settings;
#[cfg(test)]
mod sherpa_lib;
mod setup;
#[cfg(feature = "rehearsal_pack")]
mod scratch_keys;
#[cfg(feature = "menu_bar")]
mod tray;
#[cfg(feature = "menu_bar")]
mod dsh_child;
mod updater;
#[cfg(feature = "voice_pane")]
mod fleet;
#[cfg(feature = "voice_pane")]
mod pitch;
#[cfg(feature = "voice_pane")]
mod speaker;
#[cfg(feature = "voice_pane")]
mod voice;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::webview::{PageLoadEvent, PermissionResponse, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindow, WindowEvent};

fn system_language() -> Option<String> {
    system_languages().into_iter().next()
}

fn system_languages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::ffi::{c_char, c_void, CStr};
        type CFTypeRef = *const c_void;
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFLocaleCopyPreferredLanguages() -> CFTypeRef;
            fn CFArrayGetCount(a: CFTypeRef) -> isize;
            fn CFArrayGetValueAtIndex(a: CFTypeRef, i: isize) -> CFTypeRef;
            fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
            fn CFRelease(r: CFTypeRef);
        }
        let a = CFLocaleCopyPreferredLanguages();
        if a.is_null() {
            return vec![];
        }
        let mut tags = vec![];
        for i in 0..CFArrayGetCount(a) {
            let mut buf = [0 as c_char; 64];
            if CFStringGetCString(CFArrayGetValueAtIndex(a, i), buf.as_mut_ptr(), 64, 0x0800_0100) != 0 {
                if let Ok(t) = CStr::from_ptr(buf.as_ptr()).to_str() {
                    tags.push(t.to_owned());
                }
            }
        }
        CFRelease(a);
        return tags;
    }
    #[allow(unreachable_code)]
    vec![]
}

fn system_hour_cycle() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::ffi::{c_char, c_void, CStr};
        type CFTypeRef = *const c_void;
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            static kCFPreferencesAnyApplication: CFTypeRef;
            static kCFBooleanTrue: CFTypeRef;
            fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
            fn CFPreferencesCopyAppValue(key: CFTypeRef, app: CFTypeRef) -> CFTypeRef;
            fn CFLocaleCopyCurrent() -> CFTypeRef;
            fn CFDateFormatterCreateDateFormatFromTemplate(alloc: CFTypeRef, tmpl: CFTypeRef, options: usize, locale: CFTypeRef) -> CFTypeRef;
            fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
            fn CFRelease(r: CFTypeRef);
        }
        const UTF8: u32 = 0x0800_0100;
        let cf = |s: &CStr| CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), UTF8);
        let switch = |key: &CStr| {
            let k = cf(key);
            let v = CFPreferencesCopyAppValue(k, kCFPreferencesAnyApplication);
            CFRelease(k);
            if v.is_null() {
                return None;
            }
            let on = v == kCFBooleanTrue;
            CFRelease(v);
            Some(on)
        };
        let (force24, force12) = (switch(c"AppleICUForce24HourTime"), switch(c"AppleICUForce12HourTime"));
        let (tmpl, locale) = (cf(c"j"), CFLocaleCopyCurrent());
        let fmt = CFDateFormatterCreateDateFormatFromTemplate(std::ptr::null(), tmpl, 0, locale);
        let mut buf = [0 as c_char; 32];
        let pattern = (!fmt.is_null() && CFStringGetCString(fmt, buf.as_mut_ptr(), 32, UTF8) != 0).then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned());
        for r in [fmt, locale, tmpl] {
            if !r.is_null() {
                CFRelease(r);
            }
        }
        return hour_cycle(force24, force12, pattern.as_deref());
    }
    #[allow(unreachable_code)]
    None
}

fn hour_cycle(force24: Option<bool>, force12: Option<bool>, pattern: Option<&str>) -> Option<&'static str> {
    match (force24, force12) {
        (Some(true), _) => Some("h23"),
        (_, Some(true)) => Some("h12"),
        _ => match pattern {
            Some(p) if p.contains(['H', 'k']) => Some("h23"),
            Some(p) if p.contains(['h', 'K']) => Some("h12"),
            _ => None,
        },
    }
}

fn navigation_allowed(url: &Url) -> bool {
    url.scheme() == "tauri" || url.as_str() == "about:blank"
}

fn clock_js(cycle: Option<&str>) -> String {
    format!("window.__RTH_CLOCK = {:?};", cycle.filter(|c| ["h23", "h12"].contains(c)).unwrap_or_default())
}

fn version_js(v: &str) -> String {
    let t: String = v.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').take(40).collect();
    format!("window.__RTH_VERSION = {t:?};")
}

fn lang_js(tag: Option<String>) -> String {
    let t: String = tag.unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(35).collect();
    format!("window.__RTH_LANG = {t:?};")
}

const NO_MEDIA_JS: &str = r#"(() => { try {
  const deny = () => Promise.reject(new DOMException('Unfold: microphone, camera and screen are native-only', 'NotAllowedError'));
  const P = window.MediaDevices && window.MediaDevices.prototype;
  if (P) for (const k of ['getUserMedia', 'getDisplayMedia']) Object.defineProperty(P, k, { value: deny, configurable: false, writable: false });
  const N = window.Navigator && window.Navigator.prototype;
  if (N && 'getUserMedia' in N) Object.defineProperty(N, 'getUserMedia', { value: () => { throw new DOMException('Unfold: native-only', 'NotAllowedError') }, configurable: false });
} catch (_) {} })();"#;

#[derive(Clone, Debug)]
struct Config {
    data: PathBuf,
    label: String,

    contents: Option<PathBuf>,

    homes: Option<InstanceHomes>,
    probe: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct InstanceHomes {
    instance: PathBuf,
}

fn instance_homes(data: &Path, instance: &str) -> Result<InstanceHomes, String> {
    let ok = !instance.is_empty() && instance.len() <= 64 && !instance.starts_with('.') && instance.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !ok {
        return Err(format!("instance {instance:?}: use 1-64 letters, digits, '.', '_' or '-'"));
    }
    Ok(InstanceHomes { instance: data.join("instances").join(instance) })
}

fn owned_check(state: &Path, p: &Path) -> Result<(), String> {
    let rel = p.strip_prefix(state).map_err(|_| format!("{} is outside the state root {}", p.display(), state.display()))?;
    let mut at = state.to_path_buf();
    for c in rel.components() {
        let std::path::Component::Normal(name) = c else { return Err(format!("{}: not a plain path below the state root", p.display())) };
        at.push(name);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Err(format!("{} is not a directory this app owns (a link or another entry)", at.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("{}: {e}", at.display())),
        }
    }
    Ok(())
}

fn owned_dir(state: &Path, p: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    owned_check(state, p)?;
    let mut at = state.to_path_buf();
    for c in p.strip_prefix(state).unwrap().components() {
        at.push(c);
        match fs::DirBuilder::new().mode(0o700).create(&at) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_dir()) => {}
            Err(e) => return Err(format!("{}: {e}", at.display())),
        }
    }
    Ok(at)
}

fn prepare_instance(cfg: &Config) -> Result<InstanceHomes, String> {
    let h = cfg.homes.clone().ok_or("--prepare-instance: not a packaged app (no instance homes)")?;
    owned_dir(&cfg.data, &h.instance)?;
    Ok(h)
}

fn activate_instance_stand_in(cfg: &Config) -> Result<InstanceHomes, String> {
    let h = prepare_instance(cfg)?;
    owned_dir(&cfg.data, &h.instance.join("agentd"))?;
    Ok(h)
}

fn bundle_contents(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    if macos.file_name()? != "MacOS" || contents.file_name()? != "Contents" {
        return None;
    }
    contents.join("Resources/release.json").is_file().then(|| contents.to_path_buf())
}

const AUTOPG_LABEL: &str = "io.namastex.brain.personal.postgres";

const AUTOPG_VERSION: &str = "v3.3.1";

const AUTOPG_ACCEPTED_EARLIER: &str = "v3.2.2";
const MEMORY_UNAVAILABLE: &str = "Local memory service unavailable — this build needs the existing Brain/autopg service.";

#[derive(Clone, Debug, PartialEq)]
struct AutopgBinding {
    plist: PathBuf,
    program: PathBuf,
    runtime_dir: PathBuf,
    socket_dir: PathBuf,
    port: u16,
    pid: i32,
    autopg_pid: i32,
}

fn read_service_record(path: &Path) -> Result<Value, String> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !m.file_type().is_file() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o022 != 0 {
        return Err(format!("{}: not a regular file owned and writable only by this user", path.display()));
    }
    let out = Command::new("/usr/bin/plutil").args(["-convert", "json", "-o", "-"]).arg(path).env_clear().stdin(Stdio::null()).stderr(Stdio::null()).output().map_err(|e| format!("plutil: {e}"))?;
    if !out.status.success() {
        return Err(format!("{}: not a readable property list", path.display()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{}: {e}", path.display()))
}

fn autopg_service(record: &Value, home: &Path) -> Result<(PathBuf, PathBuf, PathBuf, u16), String> {
    if record.get("Label").and_then(Value::as_str) != Some(AUTOPG_LABEL) {
        return Err(format!("the service record is not {AUTOPG_LABEL}"));
    }
    let args: Vec<&str> = record.get("ProgramArguments").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let want = home.join(".local/share/autopg").join(AUTOPG_VERSION).join("autopg");
    if args.first().map(PathBuf::from) != Some(want.clone()) || args.get(1) != Some(&"postmaster") {
        return Err(format!("the service does not run autopg {AUTOPG_VERSION} postmaster (supported: {})", want.display()));
    }
    let after = |flag: &str| args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1)).copied();
    let port: u16 = after("--port").and_then(|p| p.parse().ok()).ok_or("the service record names no --port")?;
    let socket_dir = after("--socket-dir").ok_or("the service record names no --socket-dir")?;
    let rt = record.pointer("/EnvironmentVariables/XDG_RUNTIME_DIR").and_then(Value::as_str).filter(|r| r.starts_with('/')).ok_or("the service record has no absolute XDG_RUNTIME_DIR")?;
    if Path::new(socket_dir) != Path::new(rt).join("pgserve") {
        return Err("the service's socket dir is not its runtime dir's pgserve/".into());
    }
    let canon = |p: &Path| p.canonicalize().map_err(|e| format!("{}: {e}", p.display()));
    let program = canon(&want)?;
    if autopg_release(&program) != Some(AUTOPG_VERSION) {
        return Err(format!("{}: the autopg program resolves outside its version dir", want.display()));
    }
    Ok((program, canon(Path::new(rt))?, canon(Path::new(socket_dir))?, port))
}

fn autopg_release(program: &Path) -> Option<&str> {
    let dir = program.parent()?;
    (program.is_absolute() && program.file_name()? == "autopg" && dir.parent()?.ends_with(".local/share/autopg")).then(|| dir.file_name()?.to_str())?
}

fn pid_path(pid: i32) -> Option<PathBuf> {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned()))
}

fn autopg_runtime(plist: &Path, program: PathBuf, runtime_dir: PathBuf, socket_dir: PathBuf, port: u16) -> Result<AutopgBinding, String> {
    let rec: Value = fs::read(runtime_dir.join("pgserve/runtime.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or("no readable pgserve/runtime.json")?;
    if rec.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err("pgserve/runtime.json: unsupported schemaVersion".into());
    }
    let rec_dir = rec.get("socketDir").and_then(Value::as_str).and_then(|d| Path::new(d).canonicalize().ok());
    if rec_dir.as_ref() != Some(&socket_dir) || rec.get("port").and_then(Value::as_u64) != Some(port as u64) {
        return Err("pgserve/runtime.json disagrees with the service record (socket dir or port)".into());
    }
    let pid_of = |k: &str| rec.get(k).and_then(Value::as_i64).filter(|p| *p > 1 && *p <= i32::MAX as i64).map(|p| p as i32).ok_or(format!("pgserve/runtime.json: no {k}"));
    let (pid, autopg_pid) = (pid_of("pid")?, pid_of("autopgPid")?);
    if pid_path(autopg_pid).and_then(|p| p.canonicalize().ok()).as_ref() != Some(&program) {
        return Err("the recorded autopg process is not the service's program".into());
    }
    let sock = socket_dir.join(format!(".s.PGSQL.{port}"));
    let lock_pid = fs::read_to_string(socket_dir.join(format!(".s.PGSQL.{port}.lock"))).ok().and_then(|l| l.lines().next().and_then(|p| p.trim().parse::<i32>().ok()));
    if lock_pid != Some(pid) || unsafe { libc::kill(pid, 0) } != 0 {
        return Err("the recorded postmaster is not running on the service's socket".into());
    }
    std::os::unix::net::UnixStream::connect(&sock).map_err(|e| format!("{}: {e}", sock.display()))?;
    Ok(AutopgBinding { plist: plist.to_path_buf(), program, runtime_dir, socket_dir, port, pid, autopg_pid })
}

fn discover_autopg(home: &Path) -> Result<AutopgBinding, String> {
    let plist = home.join("Library/LaunchAgents").join(format!("{AUTOPG_LABEL}.plist"));
    let (program, rt, socket_dir, port) = autopg_service(&read_service_record(&plist)?, home)?;
    autopg_runtime(&plist, program, rt, socket_dir, port)
}

fn accept_binding(state: &Path, record: &Path, b: &AutopgBinding) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let now = binding_json(b);
    owned_check(state, record.parent().unwrap())?;
    match fs::symlink_metadata(record) {
        Ok(m) if m.file_type().is_file() => {
            let was: Value = fs::read(record).ok().and_then(|t| serde_json::from_slice(&t).ok()).ok_or(format!("{}: unreadable; it needs review", record.display()))?;
            if same_service(&was, &now) {
                Ok(())
            } else {
                Err(format!("the memory service changed since this instance accepted it ({}); its new record needs review", record.display()))
            }
        }
        Ok(_) => Err(format!("{} is not a regular file", record.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            owned_dir(state, record.parent().unwrap())?;
            let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(record).map_err(|e| format!("{}: {e}", record.display()))?;
            f.write_all(format!("{now:#}\n").as_bytes()).map_err(|e| format!("{}: {e}", record.display()))
        }
        Err(e) => Err(format!("{}: {e}", record.display())),
    }
}

fn binding_json(b: &AutopgBinding) -> Value {
    serde_json::json!({
        "label": AUTOPG_LABEL, "version": AUTOPG_VERSION, "plist": b.plist, "program": b.program,
        "runtime_dir": b.runtime_dir, "socket_dir": b.socket_dir, "port": b.port, "pid": b.pid, "autopg_pid": b.autopg_pid,
    })
}

fn same_service(accepted: &Value, fresh: &Value) -> bool {
    let all = |v: &Value| ["label", "version", "plist", "program", "runtime_dir", "socket_dir", "port"].map(|k| v.get(k).cloned());
    let rest = |v: &Value| ["label", "plist", "runtime_dir", "socket_dir", "port"].map(|k| v.get(k).cloned());
    let root_at = |v: &Value, release: &str| -> Option<PathBuf> {
        let program = Path::new(v.get("program")?.as_str()?);
        (v.get("version")?.as_str()? == release && autopg_release(program)? == release).then(|| program.parent()?.parent().map(Path::to_path_buf))?
    };
    let earlier = root_at(accepted, AUTOPG_ACCEPTED_EARLIER);
    all(accepted) == all(fresh) || (rest(accepted) == rest(fresh) && earlier.is_some() && earlier == root_at(fresh, AUTOPG_VERSION))
}

#[derive(Clone, Debug, PartialEq)]
enum WriteMode {
    Dev,
    Packaged(Option<PathBuf>),
}
static WRITE_MODE: std::sync::OnceLock<WriteMode> = std::sync::OnceLock::new();
fn write_mode() -> WriteMode {
    WRITE_MODE.get().cloned().unwrap_or(WriteMode::Dev)
}

fn startup_mode(bundled: bool, cfg: &Result<Config, String>) -> WriteMode {
    match (bundled, cfg) {
        (false, _) => WriteMode::Dev,
        (true, Ok(c)) if c.homes.is_some() => WriteMode::Packaged(Some(c.data.clone())),
        (true, Ok(_)) => WriteMode::Dev,
        (true, Err(_)) => WriteMode::Packaged(None),
    }
}

#[derive(Clone, Copy)]
enum Open {
    Append,
    Truncate,
}

fn owned_open_in(state: &Path, path: &Path, how: Open) -> Result<File, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let parent = path.parent().ok_or_else(|| format!("{}: no parent", path.display()))?;
    owned_dir(state, parent)?;
    match fs::symlink_metadata(path) {
        Ok(m) if !m.file_type().is_file() => return Err(format!("{} is not a regular file (a link, FIFO or other special file)", path.display())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let mut o = OpenOptions::new();
    o.write(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    if let Open::Append = how {
        o.append(true);
    }
    let f = o.open(path).map_err(|e| format!("{}: {e} (a link is never followed, a special file never waited on)", path.display()))?;
    let m = f.metadata().map_err(|e| format!("{}: {e}", path.display()))?;
    if !m.file_type().is_file() || m.nlink() != 1 {
        return Err(format!("{} is not a regular file this app owns (another link to it exists)", path.display()));
    }
    if let Open::Truncate = how {
        f.set_len(0).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(f)
}

fn owned_open_as(mode: &WriteMode, path: &Path, how: Open) -> std::io::Result<File> {
    match mode {
        WriteMode::Packaged(Some(root)) if path.starts_with(root) => owned_open_in(root, path, how).map_err(std::io::Error::other),
        WriteMode::Packaged(Some(root)) => Err(std::io::Error::other(format!("{} is outside the admitted state root {}: refused", path.display(), root.display()))),
        WriteMode::Packaged(None) => Err(std::io::Error::other(format!("{}: this packaged app has no admitted state root, so it writes no file", path.display()))),
        WriteMode::Dev => {
            if let Some(p) = path.parent() {
                fs::create_dir_all(p)?;
            }
            let mut o = OpenOptions::new();
            o.create(true);
            match how {
                Open::Append => o.append(true),
                Open::Truncate => o.write(true).truncate(true),
            };
            o.open(path)
        }
    }
}

fn owned_open(path: &Path, how: Open) -> std::io::Result<File> {
    owned_open_as(&write_mode(), path, how)
}

fn owned_replace(file: &Path, tmp: &Path, bytes: &[u8]) -> std::io::Result<()> {
    owned_open(tmp, Open::Truncate)?.write_all(bytes)?;
    fs::rename(tmp, file)
}

const MAIN_SIZE: (f64, f64) = (1360.0, 880.0);

pub fn fit_size(want: (f64, f64), work: Option<(f64, f64)>) -> (f64, f64) {
    match work {
        Some((w, h)) if w > 0.0 && h > 0.0 => (want.0.min((0.85 * w).floor()), want.1.min((0.85 * h).floor())),
        _ => want,
    }
}

pub fn fit_window(w: &tauri::WebviewWindow, want: (f64, f64)) {
    let m = w.current_monitor().ok().flatten().or_else(|| w.primary_monitor().ok().flatten());
    let work = m.as_ref().map(|m| {
        let (a, s) = (m.work_area(), m.scale_factor());
        (a.size.width as f64 / s, a.size.height as f64 / s)
    });
    let (fw, fh) = fit_size(want, work);
    let _ = w.set_size(tauri::LogicalSize::new(fw, fh));
    let _ = w.center();
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub(crate) fn keychain_call() -> Result<(), String> {
    #[cfg(test)]
    {
        KEYCHAIN_CALLS.fetch_add(1, Ordering::SeqCst);
        if KEYCHAIN_FENCED.load(Ordering::SeqCst) {
            return Err("the Keychain is fenced off in this test".into());
        }
    }
    Ok(())
}

#[cfg(test)]
static KEYCHAIN_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static KEYCHAIN_FENCED: AtomicBool = AtomicBool::new(false);
#[cfg(all(test, feature = "rehearsal_pack"))]
static KEYCHAIN_FENCE: Mutex<()> = Mutex::new(());

#[cfg(all(test, feature = "rehearsal_pack"))]
pub(crate) struct KeychainFence(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);
#[cfg(all(test, feature = "rehearsal_pack"))]
impl KeychainFence {
    pub(crate) fn up() -> KeychainFence {
        let held = KEYCHAIN_FENCE.lock().unwrap_or_else(|e| e.into_inner());
        KEYCHAIN_CALLS.store(0, Ordering::SeqCst);
        KEYCHAIN_FENCED.store(true, Ordering::SeqCst);
        KeychainFence(held)
    }
    pub(crate) fn calls() -> usize {
        KEYCHAIN_CALLS.load(Ordering::SeqCst)
    }
}
#[cfg(all(test, feature = "rehearsal_pack"))]
impl Drop for KeychainFence {
    fn drop(&mut self) {
        KEYCHAIN_FENCED.store(false, Ordering::SeqCst);
    }
}

fn config() -> Result<Config, String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| argv.iter().position(|a| a == &format!("--{f}")).and_then(|i| argv.get(i + 1)).cloned();
    let data_default = std::env::var_os("RTH_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share/rth"));
    let data_dir = flag("data").map(PathBuf::from).unwrap_or_else(|| data_default.clone());
    let file: Value = fs::read_to_string(data_dir.join("app.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);

    let get = |f: &str| -> Option<String> {
        flag(f)
            .or_else(|| std::env::var(format!("RTH_APP_{}", f.to_uppercase().replace('-', "_"))).ok())
            .or_else(|| file.get(f).and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string()))))
    };
    let contents = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).and_then(|e| bundle_contents(&e));
    let probe = match std::env::var("RTH_APP_PROBE") {
        Ok(p) => Some(fs::read_to_string(&p).map_err(|e| format!("RTH_APP_PROBE {p}: {e}"))?),
        Err(_) => None,
    };

    let data_dir = match contents {
        Some(_) => fs::create_dir_all(&data_dir).and_then(|_| data_dir.canonicalize()).map_err(|e| format!("state root {}: {e}", data_dir.display()))?,
        None => data_dir,
    };
    let homes = match &contents {

        Some(contents) => {
            let instance = match get("instance") {
                Some(i) => i,
                None => setup::plan_instance_dir(contents)?,
            };
            Some(instance_homes(&data_dir, &instance)?)
        }
        None => None,
    };

    let label = get("label").unwrap_or_else(|| "host-0".into());
    if homes.is_some() && (label.is_empty() || label.len() > 64 || label.starts_with('.') || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))) {
        return Err(format!("label {label:?}: use 1-64 letters, digits, '.', '_' or '-'"));
    }
    Ok(Config { data: data_dir, label, contents, homes, probe })
}

#[derive(Clone)]
struct Log(Arc<Mutex<Option<File>>>);

static LOG_REFUSED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
impl Log {

    fn open_in(state: Option<&Path>, dir: &Path) -> Log {
        let file = match state {
            Some(root) => match owned_open_in(root, &dir.join("app.log"), Open::Append) {
                Ok(f) => Some(f),
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "rth-app: app log refused, logging to stderr only: {e}");
                    LOG_REFUSED.get_or_init(|| e);
                    None
                }
            },
            None => {
                let _ = fs::create_dir_all(dir);
                OpenOptions::new().create(true).append(true).open(dir.join("app.log")).ok()
            }
        };
        Log(Arc::new(Mutex::new(file)))
    }
    #[cfg(test)]
    fn writes_file(&self) -> bool {
        self.0.lock().unwrap().is_some()
    }

    fn open_as(mode: &WriteMode, dir: &Path) -> Log {
        let refuse = |why: String| {
            let _ = writeln!(std::io::stderr(), "rth-app: app log refused, logging to stderr only: {why}");
            LOG_REFUSED.get_or_init(|| why);
            Log(Arc::new(Mutex::new(None)))
        };
        match mode {
            WriteMode::Packaged(Some(r)) if dir.starts_with(r) => Log::open_in(Some(r), dir),
            WriteMode::Packaged(Some(r)) => refuse(format!("{} is outside the admitted state root {}", dir.display(), r.display())),
            WriteMode::Packaged(None) => refuse("this packaged app has no admitted state root (its configuration failed)".into()),
            WriteMode::Dev => Log::open_in(None, dir),
        }
    }
    fn open(dir: &Path) -> Log {
        Log::open_as(&write_mode(), dir)
    }
    fn line(&self, msg: &str) {
        let msg = redact(msg);
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);

        let _ = writeln!(std::io::stderr(), "rth-app: {msg}");
        if let Some(f) = self.0.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{ms} {msg}");
        }
    }
}

fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for mark in ["token=", "bearer "] {
        let mut done = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.to_ascii_lowercase().find(mark) {
            done.push_str(&rest[..i + mark.len()]);
            done.push_str("<redacted>");
            rest = &rest[i + mark.len()..];
            let end = rest.find(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == ';').unwrap_or(rest.len());
            rest = &rest[end..];
        }
        done.push_str(rest);
        out = done;
    }
    out
}

#[derive(Default)]
struct Owned {
    stopped: bool,

    agent: Option<Child>,

    llm: Option<Child>,

    #[cfg(feature = "menu_bar")]
    recorded: Vec<tray::Recorded>,
}
type Shared = Arc<Mutex<Owned>>;

#[derive(Clone, Copy)]
enum Slot {
    Agent,
    Llm,
}

const CLOSING: &str = "closing: startup cancelled";

fn spawn_owned(owned: &Shared, slot: Slot, cmd: &mut Command) -> Result<(u32, Option<std::process::ChildStdout>), String> {
    let mut o = owned.lock().unwrap();
    unblock_quit_signals(cmd);
    if o.stopped {
        return Err(CLOSING.into());
    }
    let held = match slot {
        Slot::Agent => o.agent.as_ref().map(|c| ("rth-agentd", c.id())),
        Slot::Llm => o.llm.as_ref().map(|c| ("llama-server", c.id())),
    };
    if let Some((name, pid)) = held {
        return Err(format!("{name} (pid {pid}) is still owned: nothing started"));
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn {:?}: {e}", cmd.get_program()))?;
    let pid = child.id();
    let out = child.stdout.take();
    match slot {
        Slot::Agent => o.agent = Some(child),
        Slot::Llm => o.llm = Some(child),
    }
    Ok((pid, out))
}

fn unblock_quit_signals(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            Ok(())
        });
    }
}

fn cancelled(owned: &Shared) -> bool {
    owned.lock().unwrap().stopped
}

fn until<T>(owned: &Shared, within: Duration, mut f: impl FnMut() -> Option<T>) -> Result<Option<T>, String> {
    let end = Instant::now() + within;
    loop {
        if cancelled(owned) {
            return Err(CLOSING.into());
        }
        if let Some(v) = f() {
            return Ok(Some(v));
        }
        if Instant::now() > end {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn stop_local(owned: &Shared, log: &Log) {
    #[cfg(feature = "voice_pane")]
    front_native::close();

    #[cfg(feature = "menu_bar")]
    dsh_child::lease_gone();
    let (agent, llm) = {
        let mut o = owned.lock().unwrap();
        (o.agent.take(), o.llm.take())
    };

    if agent.is_some() || llm.is_some() {
        stop_pair(agent, llm, log);
    }
}

fn stop_owned(owned: &Shared, log: &Log) {
    if let Some(taken) = take_owned(owned) {
        stop_taken(taken, log);
    }
}

struct Taken {
    agent: Option<Child>,
    llm: Option<Child>,
    #[cfg(feature = "menu_bar")]
    recorded: Vec<tray::Recorded>,
}

fn take_owned(owned: &Shared) -> Option<Taken> {
    let mut o = owned.lock().unwrap();
    if o.stopped {
        return None;
    }
    o.stopped = true;
    Some(Taken { agent: o.agent.take(), llm: o.llm.take(), #[cfg(feature = "menu_bar")] recorded: std::mem::take(&mut o.recorded) })
}

fn stop_taken(t: Taken, log: &Log) {
    #[cfg(feature = "menu_bar")]
    {
        tray::stop_recorded(t.recorded, log);

        dsh_child::lease_gone();
    }

    stop_pair(t.agent, t.llm, log);

    #[cfg(feature = "menu_bar")]
    dsh_child::stop_channel();
}

fn stop_pair(agent: Option<Child>, llm: Option<Child>, log: &Log) {

    let live = |c: Option<Child>, name: &str| -> Option<Child> {
        let mut c = c?;
        match c.try_wait() {
            Ok(None) => Some(c),
            Ok(Some(s)) => {
                log.line(&format!("{name} (pid {}) had already exited ({s}); not signalled", c.id()));
                None
            }
            Err(e) => {
                log.line(&format!("{name} (pid {}): liveness unknown ({e}); not signalled", c.id()));
                None
            }
        }
    };
    if let Some(a) = live(agent, "rth-agentd") {
        unsafe { libc::kill(a.id() as i32, libc::SIGTERM) };
        log.line(&format!("close: SIGTERM -> owned rth-agentd pid {}", a.id()));
        reap(a, "rth-agentd", OWNED_GRACE, log);
    }
    if let Some(l) = live(llm, "llama-server") {
        unsafe { libc::kill(l.id() as i32, libc::SIGTERM) };
        log.line(&format!("close: SIGTERM -> owned llama-server pid {}", l.id()));
        reap(l, "llama-server", OWNED_GRACE, log);
    }
}

const OWNED_GRACE: Duration = Duration::from_secs(5);

const EXIT_CAP: Duration = Duration::from_secs(20);

fn reap(mut c: Child, name: &str, grace: Duration, log: &Log) {
    let end = Instant::now() + grace;
    loop {
        match c.try_wait() {
            Ok(Some(s)) => return log.line(&format!("{name} (pid {}) exited: {s}", c.id())),
            Ok(None) if Instant::now() > end => {
                unsafe { libc::kill(-(c.id() as i32), libc::SIGKILL) };
                let _ = c.wait();
                return log.line(&format!("{name} (pid {}) killed after {} s", c.id(), grace.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return log.line(&format!("{name} (pid {}): liveness unknown ({e}); left alone", c.id())),
        }
    }
}

struct Resp {
    code: u16,
}

fn http(addr: SocketAddr, path: &str, extra: &str) -> Option<Resp> {
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: {addr}\r\n{extra}Connection: close\r\n\r\n").ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let buf = String::from_utf8_lossy(&buf).into_owned();
    Some(Resp { code: buf.split_whitespace().nth(1)?.parse().ok()? })
}

fn port_open(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

fn status(win: &WebviewWindow, log: &Log, msg: &str) {
    log.line(msg);
    let js = format!("window.__rthStatus && window.__rthStatus({})", serde_json::to_string(&redact(msg)).unwrap());
    let _ = win.eval(js);
}

const PACKAGED_AGENT_BACKEND: &str = "fake";

fn agentd_env(contents: &Path, h: &InstanceHomes, b: &AutopgBinding, bind: SocketAddr, provider_key: Option<(&str, &str)>, memory_egress: Option<&str>) -> Vec<(String, String)> {
    let p = |x: &Path| x.display().to_string();
    let agent_dir = h.instance.join("agentd");
    let mut env = vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), p(&home())),
        ("RTH_INSTANCE_ROOT".into(), p(&h.instance)),
        ("RTH_STATE".into(), p(&agent_dir.join("state.json"))),
        ("RTH_JOURNAL".into(), p(&agent_dir.join("journal.jsonl"))),
        ("RTH_AGENT_BIND".into(), bind.to_string()),
        ("RTH_BACKEND".into(), PACKAGED_AGENT_BACKEND.into()),
        ("RTH_MEMORY_BRAIN_RUNTIME".into(), p(&b.runtime_dir)),
        ("RTH_MEMORY_BRAIN_MCP".into(), p(&contents.join("MacOS/brain-mcp"))),
        ("RTH_MEMORY_BUN".into(), p(&contents.join("MacOS/bun"))),
        ("RTH_MEMORY_BRAIN_SRC".into(), p(&contents.join("Resources/brain-seam/src"))),
        ("RTH_MEMORY_BRAIN_SEAM".into(), p(&contents.join("Resources/brain-seam/seam/memory-search.ts"))),
    ];
    if let Some((k, v)) = provider_key {
        env.push((k.into(), v.into()));
    }
    if let Some(e) = memory_egress {
        env.push(("RTH_MEMORY_EGRESS".into(), e.into()));
    }
    crate::setup::with_app_version(env)
}

fn free_loopback() -> Result<SocketAddr, String> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no free loopback port: {e}"))?;
    l.local_addr().map_err(|e| e.to_string())
}

fn start_agent(cfg: &Config, h: &InstanceHomes, b: &AutopgBinding, owned: &Shared, log: &Log, app_dir: &Path) -> Result<SocketAddr, String> {
    let contents = cfg.contents.clone().ok_or("no bundle Contents")?;
    let agentd = contents.join("MacOS/rth-agentd");

    let dir = h.instance.join("agentd");
    owned_check(&cfg.data, &dir)?;
    if !fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir()) {
        return Err(format!("this instance's agent directory {} is missing: recovery needed (it is never recreated at launch)", dir.display()));
    }
    for f in ["state.json", "journal.jsonl"] {
        match fs::symlink_metadata(dir.join(f)) {
            Ok(m) if !m.file_type().is_file() => return Err(format!("{} is not a regular file", dir.join(f).display())),
            _ => {}
        }
    }
    let bind = free_loopback()?;
    let out = owned_open(&app_dir.join("agentd.log"), Open::Truncate).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&agentd);
    cmd.env_clear().envs(agentd_env(&contents, h, b, bind, None, None)).current_dir(&dir).stdin(Stdio::null()).stdout(out).stderr(err).process_group(0);
    let (pid, _) = spawn_owned(owned, Slot::Agent, &mut cmd)?;
    log.line(&format!("started rth-agentd (pid {pid}) on {bind}, backend {PACKAGED_AGENT_BACKEND}"));
    let up = until(owned, Duration::from_secs(15), || {
        if port_open(bind) {
            return Some(Ok(()));
        }
        let mut o = owned.lock().unwrap();
        match o.agent.as_mut().map(|c| c.try_wait()) {
            Some(Ok(Some(s))) => Some(Err(format!("rth-agentd exited before listening ({s}); see {}", app_dir.join("agentd.log").display()))),
            _ => None,
        }
    })?;
    match up {
        Some(r) => r.map(|()| bind),
        None => Err(format!("rth-agentd did not listen on {bind} within 15 s")),
    }
}

fn start(cfg: Config, win: WebviewWindow, owned: Shared, log: Log) -> Result<(), String> {
    let h = cfg.homes.clone().ok_or("Unfold runs from its installed app (Unfold.app); this binary is not in an app bundle")?;
    if let Some(why) = LOG_REFUSED.get() {
        return Err(format!("the app log could not be admitted below {}: {why}", cfg.data.display()));
    }
    setup::gate(&cfg, &h, &win, &owned, &log)
}

const GOODBYE: Duration = Duration::from_millis(500);

const GOODBYE_JS: &str = "(()=>{try{const n=typeof window.__rthGoodbye==='function'?window.__rthGoodbye(500):0;document.title='RTH goodbye n='+n;const s=document.documentElement.style;s.transition=n?'opacity 250ms ease-in 250ms':'opacity 450ms ease-in';s.opacity='0'}catch(e){}})()";
const UNLOAD_WITHIN: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Quit {
    Idle,
    Running,
    Done,
}
type QuitCell = (Mutex<Quit>, Condvar);
static QUIT: QuitCell = (Mutex::new(Quit::Idle), Condvar::new());

fn claim(q: &QuitCell) -> bool {
    let mut s = q.0.lock().unwrap();
    if *s != Quit::Idle {
        return false;
    }
    *s = Quit::Running;
    true
}

fn finish(q: &QuitCell) {
    *q.0.lock().unwrap() = Quit::Done;
    q.1.notify_all();
}

fn terminate_reply(state: Quit, system: bool) -> (usize, bool, bool) {
    match (state, system) {
        (Quit::Done, _) => (objc::NS_TERMINATE_NOW, false, false),
        (_, true) => (objc::NS_TERMINATE_LATER, true, true),
        (_, false) => (objc::NS_TERMINATE_CANCEL, false, true),
    }
}

#[derive(Debug, PartialEq)]
enum ExitWait {
    AlreadyDone,

    Waited(bool),

    Stopped,
}

fn exit_wait(q: &QuitCell, within: Duration, stop: impl FnOnce()) -> ExitWait {
    let mut s = q.0.lock().unwrap();
    match *s {
        Quit::Done => ExitWait::AlreadyDone,
        Quit::Running => {
            let (s, _) = q.1.wait_timeout_while(s, within, |s| *s == Quit::Running).unwrap();
            ExitWait::Waited(*s != Quit::Running)
        }
        Quit::Idle => {
            *s = Quit::Running;
            drop(s);
            stop();
            finish(q);
            ExitWait::Stopped
        }
    }
}

static TERMINATE_PENDING: AtomicBool = AtomicBool::new(false);

static UNLOADED: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

fn begin_quit(app: &AppHandle, owned: &Shared, log: &Log, why: &str) {
    if !claim(&QUIT) {
        return;
    }
    log.line(&format!("quit: {why}: goodbye, then stopping only what this app started"));

    let taken = take_owned(owned);
    let win = app.get_webview_window("main");
    if let Some(w) = &win {
        let _ = w.eval(GOODBYE_JS);
    }
    let (app, log) = (app.clone(), log.clone());
    std::thread::spawn(move || {
        std::thread::sleep(GOODBYE);
        if let Some(w) = &win {
            let _ = w.hide();
            log.line("quit: window hidden");
            let (tx, rx) = mpsc::channel();
            *UNLOADED.lock().unwrap() = Some(tx);
            let _ = w.navigate("about:blank".parse().unwrap());
            let unloaded = rx.recv_timeout(UNLOAD_WITHIN).is_ok();
            log.line(if unloaded { "quit: page unloaded" } else { "quit: page unload not confirmed within 300 ms; continuing" });
        }
        if let Some(t) = taken {
            stop_taken(t, &log);
        }

        dictation::quit_sidecar();

        updater::on_quit(TERMINATE_PENDING.load(Ordering::SeqCst));
        finish(&QUIT);
        log.line("quit: everything this app started has exited");
        if TERMINATE_PENDING.swap(false, Ordering::SeqCst) {
            let _ = app.run_on_main_thread(|| unsafe { objc::reply_terminate(true) });
        } else {
            app.exit(0);
        }
    });
}

fn exit_quit(owned: &Shared, log: &Log) {
    if *QUIT.0.lock().unwrap() == Quit::Running {
        log.line("exit: waiting for the quit in flight");
    }
    let r = exit_wait(&QUIT, EXIT_CAP, || {
        log.line("close: stopping only what this app started");
        stop_owned(owned, log);
        dictation::quit_sidecar();
    });
    if r == ExitWait::Waited(false) {
        log.line("exit: the quit in flight did not finish within 20 s");
    }
}

mod objc {
    use std::ffi::{c_char, c_void};
    use std::sync::OnceLock;

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn object_getClass(obj: *mut c_void) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn class_addMethod(cls: *mut c_void, sel: *mut c_void, imp: *const c_void, types: *const c_char) -> bool;
        fn objc_msgSend();
    }
    pub const NS_TERMINATE_CANCEL: usize = 0;
    pub const NS_TERMINATE_NOW: usize = 1;
    pub const NS_TERMINATE_LATER: usize = 2;

    const QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");

    static ON_TERMINATE: OnceLock<Box<dyn Fn(bool) -> usize + Send + Sync>> = OnceLock::new();

    unsafe fn ns_app() -> *mut c_void {
        let send: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
        send(objc_getClass(c"NSApplication".as_ptr()), sel_registerName(c"sharedApplication".as_ptr()))
    }

    unsafe fn system_quit() -> bool {
        let send: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
        let attr: extern "C" fn(*mut c_void, *mut c_void, u32) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
        let manager = send(objc_getClass(c"NSAppleEventManager".as_ptr()), sel_registerName(c"sharedAppleEventManager".as_ptr()));
        let event = send(manager, sel_registerName(c"currentAppleEvent".as_ptr()));
        !event.is_null() && !attr(event, sel_registerName(c"attributeDescriptorForKeyword:".as_ptr()), QUIT_REASON).is_null()
    }

    extern "C" fn should_terminate(_this: *mut c_void, _sel: *mut c_void, _sender: *mut c_void) -> usize {
        match ON_TERMINATE.get() {
            Some(f) => f(unsafe { system_quit() }),
            None => NS_TERMINATE_NOW,
        }
    }

    pub fn install(on_terminate: Box<dyn Fn(bool) -> usize + Send + Sync>) -> Result<bool, String> {
        unsafe {
            let app = ns_app();
            let send: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void = std::mem::transmute(objc_msgSend as *const c_void);
            let delegate = send(app, sel_registerName(c"delegate".as_ptr()));
            if delegate.is_null() {
                return Err("NSApp has no delegate".into());
            }
            let _ = ON_TERMINATE.set(on_terminate);
            let sel = sel_registerName(c"applicationShouldTerminate:".as_ptr());
            Ok(class_addMethod(object_getClass(delegate), sel, should_terminate as *const c_void, c"Q@:@".as_ptr()))
        }
    }

    pub unsafe fn reply_terminate(yes: bool) {
        let send: extern "C" fn(*mut c_void, *mut c_void, bool) = std::mem::transmute(objc_msgSend as *const c_void);
        send(ns_app(), sel_registerName(c"replyToApplicationShouldTerminate:".as_ptr()), yes);
    }
}

fn block_quit_signals() -> libc::sigset_t {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGUSR1] {
            libc::sigaddset(&mut set, s);
        }
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        set
    }
}

fn explicit_state_root(c: &Config) -> Result<(), String> {
    let explicit = std::env::var_os("RTH_DATA_DIR").is_some() || std::env::args().any(|a| a == "--data");
    if c.homes.is_some() && !explicit {
        return Err("this proof mode stands in for the activation: give an explicit --data or RTH_DATA_DIR (never the default state root)".into());
    }
    Ok(())
}

fn main() {

    if std::env::args().nth(1).as_deref() == Some("--check-memory-service") {
        let r = discover_autopg(&home());
        println!("{}", match &r { Ok(b) => serde_json::json!({"ok": true, "binding": binding_json(b)}), Err(e) => serde_json::json!({"ok": false, "message": MEMORY_UNAVAILABLE, "reason": e}) });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    if std::env::args().nth(1).as_deref() == Some("--memory-chat-check") {
        let a: Vec<String> = std::env::args().skip(2).collect();
        let r = chat::memory_check(&a);
        println!("{}", match &r { Ok(v) => v.clone(), Err(e) => serde_json::json!({"ok": false, "error": e}) });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }
    if std::env::args().nth(1).as_deref() == Some("--local-chat-check") {
        let a: Vec<String> = std::env::args().skip(2).collect();
        let r = chat::headless_check(&a);
        println!("{}", match &r { Ok(v) => v.clone(), Err(e) => serde_json::json!({"ok": false, "error": e}) });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    if std::env::args().nth(1).as_deref() == Some("--setup-status") {
        let r = config().and_then(|c| {
            let h = c.homes.clone().ok_or("not a packaged app")?;
            let contents = setup::contents(&c)?;
            let inputs: serde_json::Map<String, Value> = ["MacOS/rth-activate", "Resources/activation/plan.json", "Resources/activation/product.json"].iter().map(|p| (p.to_string(), Value::Bool(contents.join(p).is_file()))).collect();
            Ok::<_, String>(serde_json::json!({"data": c.data, "instance": h.instance, "records": rth_activation::seam::records_status(&c.data), "inputs": inputs}))
        });
        println!("{}", match &r { Ok(v) => serde_json::json!({"ok": true, "setup": v}), Err(e) => serde_json::json!({"ok": false, "error": e}) });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    if std::env::args().nth(1).as_deref() == Some("--prepare-instance") {
        let r = config().and_then(|c| {
            if c.homes.is_some() {
                let _ = WRITE_MODE.set(WriteMode::Packaged(Some(c.data.clone())));
            }
            explicit_state_root(&c)?;
            activate_instance_stand_in(&c).map(|h| (c, h))
        });
        println!("{}", match &r {
            Ok((c, h)) => serde_json::json!({"ok": true, "data": c.data, "instance": h.instance, "contents": c.contents}),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    if std::env::args().nth(1).as_deref() == Some("--launch-agent-check") {
        let owned: Shared = Arc::default();
        let r = (|| {
            let c = config()?;
            if c.homes.is_some() {
                let _ = WRITE_MODE.set(WriteMode::Packaged(Some(c.data.clone())));
            }
            explicit_state_root(&c)?;
            let h = prepare_instance(&c)?;
            let b = discover_autopg(&home()).map_err(|e| format!("{MEMORY_UNAVAILABLE}\n{e}"))?;
            accept_binding(&c.data, &h.instance.join("memory-service.json"), &b).map_err(|e| format!("{MEMORY_UNAVAILABLE}\n{e}"))?;
            let app_dir = owned_dir(&c.data, &c.data.join("app").join(&c.label))?;
            let log = Log::open(&app_dir);
            let bind = start_agent(&c, &h, &b, &owned, &log, &app_dir)?;
            let pid = owned.lock().unwrap().agent.as_ref().map(|a| a.id());
            let hello = TcpStream::connect_timeout(&bind, Duration::from_secs(2)).is_ok();
            stop_owned(&owned, &log);
            Ok::<_, String>(serde_json::json!({"bind": bind.to_string(), "pid": pid, "accepted_connection": hello, "instance": h.instance, "agentd_log": app_dir.join("agentd.log")}))
        })();
        println!("{}", match &r { Ok(v) => serde_json::json!({"ok": true, "agent": v}), Err(e) => serde_json::json!({"ok": false, "error": e}) });
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }
    let sigset = block_quit_signals();
    let cfg = config();
    let label = cfg.as_ref().map(|c| c.label.clone()).unwrap_or_else(|_| "unconfigured".into());
    let data = cfg.as_ref().map(|c| c.data.clone()).unwrap_or_else(|_| home().join(".local/share/rth"));

    let bundled = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).and_then(|e| bundle_contents(&e)).is_some();
    let _ = WRITE_MODE.set(startup_mode(bundled, &cfg));
    let log = Log::open(&data.join("app").join(&label));
    let owned: Shared = Arc::default();
    let context = tauri::generate_context!();
    let version = context.package_info().version.to_string();

    let updates = bundled && matches!(write_mode(), WriteMode::Packaged(Some(_)));
    let homes = cfg.as_ref().ok().and_then(|c| c.homes.clone());
    let instance = homes.map(|h| h.instance);
    if updates {
        if let updater::Launch::Reopen(app) = updater::on_launch(&data, instance.as_deref(), &version, &log) {
            if let Err(e) = updater::reopen(&app) {
                log.line(&format!("update: {e}"));
            }
            std::process::exit(0);
        }
    }
    let contents = cfg.as_ref().ok().and_then(|c| c.contents.clone());
    let _ = settings::CTX.set(settings::Ctx { data: data.clone(), contents, instance: instance.clone(), version: version.clone(), log: log.clone() });

    settings::migrate(&data, &log);
    settings::sync_turn_mode(&settings::load(&data));

    dictation_models::resume_interrupted(&data, log.clone(), dictation::models_changed);
    dictation::stop_leftover_sidecar(&data);
    let page_lang = settings::language_tag(&settings::load(&data), system_language());

    let menu_pt = settings::is_pt(page_lang.as_deref());

    let (o1, l1) = (owned.clone(), log.clone());
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![setup::rth_setup, settings::rth_settings, dictation::rth_dictation])
        .setup(move |app| {
            let handle = app.handle().clone();
            let (o, l) = (o1.clone(), l1.clone());
            std::thread::spawn(move || loop {
                let mut sig = 0;
                unsafe { libc::sigwait(&sigset, &mut sig) };
                if sig == libc::SIGUSR1 {
                    l.line("signal SIGUSR1: ignored (no voice host to stop)");
                    continue;
                }
                l.line(&format!("signal {sig}: closing"));

                begin_quit(&handle, &o, &l, &format!("signal {sig}"));
            });

            let w = |en: &'static str, pt: &'static str| if menu_pt { pt } else { en };
            let settings_item = MenuItemBuilder::with_id("unfold-settings", w("Settings…", "Ajustes…")).accelerator("CmdOrCtrl+,").build(app)?;
            let app_menu = SubmenuBuilder::new(app, "Unfold")
                .about_with_text(w("About Unfold", "Sobre Unfold"), None)
                .separator()
                .item(&settings_item)
                .separator()
                .hide_with_text(w("Hide Unfold", "Ocultar Unfold"))
                .hide_others_with_text(w("Hide Others", "Ocultar Outros"))
                .separator()
                .quit_with_text(w("Quit Unfold", "Encerrar Unfold"))
                .build()?;
            let edit = SubmenuBuilder::new(app, w("Edit", "Editar"))
                .undo_with_text(w("Undo", "Desfazer"))
                .redo_with_text(w("Redo", "Refazer"))
                .separator()
                .cut_with_text(w("Cut", "Recortar"))
                .copy_with_text(w("Copy", "Copiar"))
                .paste_with_text(w("Paste", "Colar"))
                .select_all_with_text(w("Select All", "Selecionar Tudo"))
                .build()?;
            let view = SubmenuBuilder::new(app, w("Window", "Janela"))
                .minimize_with_text(w("Minimize", "Minimizar"))
                .fullscreen_with_text(w("Toggle Full Screen", "Alternar Tela Cheia"))
                .close_window_with_text(w("Close Window", "Fechar Janela"))
                .build()?;
            app.set_menu(MenuBuilder::new(app).items(&[&app_menu, &edit, &view]).build()?)?;
            app.on_menu_event(move |app, ev| {
                if ev.id().as_ref() == "unfold-settings" {
                    settings::open(app);
                }
            });

            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Unfold")
                .inner_size(MAIN_SIZE.0, MAIN_SIZE.1)
                .min_inner_size(720.0, 480.0)

                .visible(false)

                .on_permission_request(|_, _| PermissionResponse::Deny)
                .initialization_script(NO_MEDIA_JS)
                .initialization_script(lang_js(page_lang.clone()))
                .initialization_script(clock_js(system_hour_cycle()))
                .initialization_script(version_js(&app.package_info().version.to_string()))
                .on_page_load(move |w, p| {
                    let u = p.url();
                    if p.event() == PageLoadEvent::Started {

                        dictation::page_gone();
                    }
                    if p.event() == PageLoadEvent::Finished && u.scheme() == "tauri" {
                        updater::page_loaded(&w);
                        dictation::page_loaded(&w);
                        #[cfg(feature = "menu_bar")]
                        dsh_child::page_loaded(&w);
                    }
                    if p.event() == PageLoadEvent::Finished && u.as_str() == "about:blank" {
                        if let Some(tx) = UNLOADED.lock().unwrap().take() {
                            let _ = tx.send(());
                        }
                    }
                })
                .on_navigation(move |url| {
                    let ok = navigation_allowed(url);
                    if !ok {
                        let _ = writeln!(std::io::stderr(), "rth-app: blocked navigation to {}://{}", url.scheme(), url.host_str().unwrap_or(""));
                    }
                    ok
                });
            if let Ok(Some(probe)) = cfg.as_ref().map(|c| c.probe.clone()) {
                b = b.initialization_script(probe);
            }
            let win = b.build()?;
            fit_window(&win, MAIN_SIZE);
            #[cfg(feature = "menu_bar")]
            {
                let (o, l) = (o1.clone(), l1.clone());
                if let Err(e) = tray::install(app.handle(), menu_pt, move |h| begin_quit(h, &o, &l, "Quit in the menu bar")) {
                    l1.line(&format!("menu bar: no icon ({e})"));
                }

                if let Some(root) = instance.clone() {
                    dsh_child::init(o1.clone(), l1.clone(), data.clone(), root, settings::load(&data).use_own_dsh);
                }
            }

            let _ = win.show();

            if !std::env::args().any(|a| a == updater::RELAUNCH_ARG) {
                let _ = win.set_focus();
            }

            let (h, o, l) = (app.handle().clone(), o1.clone(), l1.clone());
            let installed = objc::install(Box::new(move |system| {
                let state = *QUIT.0.lock().unwrap();
                let (reply, pending, begin) = terminate_reply(state, system);
                if pending {
                    TERMINATE_PENDING.store(true, Ordering::SeqCst);
                }
                if begin {
                    begin_quit(&h, &o, &l, if system { "quit by the system (logout, restart or shutdown)" } else { "quit requested (Cmd+Q, the menu or the Dock)" });
                }
                reply
            }));
            l1.line(&match installed {
                Ok(true) => "quit: applicationShouldTerminate: installed (terminate: runs the goodbye)".to_string(),
                Ok(false) => "quit: the app delegate already answers applicationShouldTerminate:; not installed (terminate: closes synchronously)".to_string(),
                Err(e) => format!("quit: applicationShouldTerminate: not installed ({e}); terminate: closes synchronously"),
            });
            if updates {
                let (h, o, l) = (app.handle().clone(), o1.clone(), l1.clone());
                let quit: Arc<dyn Fn() + Send + Sync> = Arc::new(move || begin_quit(&h, &o, &l, "update: an idle install or a rollback"));
                match std::env::current_exe() {
                    Ok(exe) => updater::start(app.handle().clone(), data.clone(), instance.clone(), exe, version.clone(), l1.clone(), quit),
                    Err(e) => {
                        l1.line(&format!("update: this app's path is unknown: {e}"));
                        updater::unavailable(updater::Off::PathUnknown)
                    }
                }
            } else {
                updater::unavailable(if bundled { updater::Off::NoStateFolder } else { updater::Off::NotInstalled });
            }
            let (o, l) = (o1.clone(), l1.clone());
            match cfg.clone() {
                Ok(c) => {
                    l.line(&format!("open: data {} label {}", c.data.display(), c.label));
                    std::thread::spawn(move || {
                        if let Err(e) = start(c, win.clone(), o, l.clone()) {
                            status(&win, &l, &format!("ERROR: {e}"));
                        }
                    });
                }
                Err(e) => status(&win, &l, &format!("ERROR: {e}")),
            }
            Ok(())
        })
        .on_window_event({
            #[cfg_attr(feature = "menu_bar", allow(unused_variables))]
            let (o, l) = (owned.clone(), log.clone());
            move |w, ev| match ev {

                WindowEvent::CloseRequested { api, .. } if w.label() == "main" => {
                    api.prevent_close();
                    #[cfg(feature = "menu_bar")]
                    tray::main_closed(&l, || w.hide());
                    #[cfg(not(feature = "menu_bar"))]
                    begin_quit(w.app_handle(), &o, &l, "window closed");
                }
                WindowEvent::Focused(f) => updater::focus(w.label(), *f),

                WindowEvent::Destroyed => {
                    #[cfg(feature = "voice_pane")]
                    if w.label() == "settings" {
                        settings::voice_window_closed();
                    }
                    updater::focus(w.label(), false)
                }
                _ => {}
            }
        })
        .build(context)
        .expect("tauri app");

    app.run(move |h, ev| match ev {

        RunEvent::ExitRequested { code: None, api, .. } if *QUIT.0.lock().unwrap() != Quit::Done => {
            api.prevent_exit();
            begin_quit(h, &owned, &log, "exit requested");
        }

        #[cfg(feature = "menu_bar")]
        RunEvent::Reopen { has_visible_windows: false, .. } => tray::show_main(h),
        RunEvent::Exit => {
            exit_quit(&owned, &log);
            log.line("closed");
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_fits_85_percent_of_the_work_area_and_never_grows() {

        assert_eq!(fit_size(MAIN_SIZE, Some((1728.0, 1079.0))), (1360.0, 880.0));
        assert_eq!(fit_size(MAIN_SIZE, Some((2560.0, 1415.0))), (1360.0, 880.0));
        assert_eq!(fit_size((860.0, 640.0), Some((1728.0, 1079.0))), (860.0, 640.0));

        assert_eq!(fit_size(MAIN_SIZE, Some((1280.0, 695.0))), (1088.0, 590.0));
        assert_eq!(fit_size(MAIN_SIZE, Some((1440.0, 875.0))), (1224.0, 743.0));
        assert_eq!(fit_size((860.0, 640.0), Some((1280.0, 695.0))), (860.0, 590.0));

        assert_eq!(fit_size(MAIN_SIZE, None), MAIN_SIZE);
        assert_eq!(fit_size(MAIN_SIZE, Some((0.0, 0.0))), MAIN_SIZE);
    }

    #[test]
    fn the_splash_gets_the_app_version_as_a_plain_string() {
        assert_eq!(version_js("0.4.0"), r#"window.__RTH_VERSION = "0.4.0";"#);
        assert_eq!(version_js("0.4.0-rc.1"), r#"window.__RTH_VERSION = "0.4.0-rc.1";"#);
        assert_eq!(version_js("1\"</script>"), r#"window.__RTH_VERSION = "1script";"#);
        assert_eq!(version_js(env!("CARGO_PKG_VERSION")).is_empty(), false);
    }

    #[test]
    fn the_page_gets_the_system_language_as_a_plain_tag() {
        assert_eq!(lang_js(Some("pt-BR".into())), r#"window.__RTH_LANG = "pt-BR";"#);
        assert_eq!(lang_js(Some("pt\"</script>".into())), r#"window.__RTH_LANG = "ptscript";"#);
        assert_eq!(lang_js(None), r#"window.__RTH_LANG = "";"#);
        assert!(system_language().is_some_and(|t| !t.is_empty()), "this Mac has a preferred language");
    }

    #[test]
    fn the_clock_follows_the_macs_24_hour_switch_then_its_region() {

        assert_eq!(hour_cycle(Some(true), None, Some("h a")), Some("h23"));
        assert_eq!(hour_cycle(None, Some(true), Some("HH")), Some("h12"));
        assert_eq!(hour_cycle(Some(false), None, Some("HH")), Some("h23"), "a false switch says nothing");

        assert_eq!(hour_cycle(None, None, Some("HH")), Some("h23"));
        assert_eq!(hour_cycle(None, None, Some("H")), Some("h23"));
        assert_eq!(hour_cycle(None, None, Some("h a")), Some("h12"));
        assert_eq!(hour_cycle(None, None, Some("a K")), Some("h12"));
        assert_eq!(hour_cycle(None, None, None), None, "nothing known: the page keeps its language's default");
        assert!(system_hour_cycle().is_some(), "this Mac has an hour cycle");
        assert_eq!(clock_js(Some("h23")), r#"window.__RTH_CLOCK = "h23";"#);
        assert_eq!(clock_js(Some("h12\"</script>")), r#"window.__RTH_CLOCK = "";"#);
        assert_eq!(clock_js(None), r#"window.__RTH_CLOCK = "";"#);
    }

    use serde_json::json;

    #[test]
    fn the_installed_bundle_is_found_by_its_release_record_not_the_dsh_tree() {
        let dir = std::env::temp_dir().join(format!("rth-app-bundle-{}", std::process::id()));
        let app = dir.join("Unfold.app/Contents");
        fs::create_dir_all(app.join("MacOS")).unwrap();
        fs::create_dir_all(app.join("Resources")).unwrap();
        let exe = app.join("MacOS/rth-app");
        assert_eq!(bundle_contents(&exe), None, "no release record: not an installed bundle");
        fs::write(app.join("Resources/release.json"), "{}").unwrap();
        assert_eq!(bundle_contents(&exe), Some(app.clone()), "0.3.4 ships no Resources/rth-dsh");

        fs::create_dir_all(dir.join("Resources")).unwrap();
        fs::write(dir.join("Resources/release.json"), "{}").unwrap();
        assert_eq!(bundle_contents(&dir.join("MacOS/rth-app")), None);

        fs::remove_file(app.join("Resources/release.json")).unwrap();
        fs::create_dir_all(app.join("Resources/release.json")).unwrap();
        assert_eq!(bundle_contents(&exe), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_shared_autopg_service_is_discovered_read_only_and_bound_exactly() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ap{}", std::process::id()));
        let home = dir.join("h");
        let prog = home.join(".local/share/autopg/v3.3.1/autopg");
        fs::create_dir_all(prog.parent().unwrap()).unwrap();
        fs::write(&prog, "").unwrap();
        let rt = dir.join("rt");
        fs::create_dir_all(rt.join("pgserve")).unwrap();
        let record = |label: &str, program: &Path, rt: Option<&Path>, sock: &Path| {
            let mut env = serde_json::Map::new();
            if let Some(rt) = rt {
                env.insert("XDG_RUNTIME_DIR".into(), json!(rt));
            }
            json!({"Label": label, "ProgramArguments": [program, "postmaster", "--port", "5999", "--data", "/x", "--socket-dir", sock], "EnvironmentVariables": env})
        };
        let good = record(AUTOPG_LABEL, &prog, Some(&rt), &rt.join("pgserve"));
        let (p, r, s, port) = autopg_service(&good, &home).unwrap();
        assert_eq!((p, r.clone(), s.clone(), port), (prog.canonicalize().unwrap(), rt.canonicalize().unwrap(), rt.join("pgserve").canonicalize().unwrap(), 5999));

        let v322 = home.join(".local/share/autopg/v3.2.2/autopg");
        let v321 = home.join(".local/share/autopg/v3.2.1/autopg");
        let v999 = home.join(".local/share/autopg/v9.9.9/autopg");
        for v in [&v322, &v321, &v999] {
            fs::create_dir_all(v.parent().unwrap()).unwrap();
            fs::write(v, "").unwrap();
        }
        let linked_home = dir.join("lh");
        fs::create_dir_all(linked_home.join(".local/share/autopg")).unwrap();
        std::os::unix::fs::symlink(v322.parent().unwrap(), linked_home.join(".local/share/autopg/v3.3.1")).unwrap();
        let link_rec = record(AUTOPG_LABEL, &linked_home.join(".local/share/autopg/v3.3.1/autopg"), Some(&rt), &rt.join("pgserve"));
        assert!(autopg_service(&link_rec, &linked_home).unwrap_err().contains("resolves outside its version dir"));
        let not_current = "does not run autopg v3.3.1 postmaster (supported: ";
        for (bad, why) in [
            (record("io.namastex.brain.personal.postgres2", &prog, Some(&rt), &rt.join("pgserve")), "not io.namastex"),
            (record(AUTOPG_LABEL, &v322, Some(&rt), &rt.join("pgserve")), not_current),
            (record(AUTOPG_LABEL, &v321, Some(&rt), &rt.join("pgserve")), not_current),
            (record(AUTOPG_LABEL, &v999, Some(&rt), &rt.join("pgserve")), not_current),
            (record(AUTOPG_LABEL, &dir.join("elsewhere/v3.3.1/autopg"), Some(&rt), &rt.join("pgserve")), not_current),
            (record(AUTOPG_LABEL, &prog, None, &rt.join("pgserve")), "no absolute XDG_RUNTIME_DIR"),
            (record(AUTOPG_LABEL, &prog, Some(&rt), &dir.join("elsewhere")), "socket dir is not"),
        ] {
            assert!(autopg_service(&bad, &home).unwrap_err().contains(why), "{why}");
        }

        let me = std::process::id() as i32;
        let exe = std::env::current_exe().unwrap().canonicalize().unwrap();
        let write_rec = |port: u16, autopg: i32| fs::write(r.join("pgserve/runtime.json"), json!({"socketDir": s, "port": port, "pid": me, "autopgPid": autopg, "schemaVersion": 1}).to_string()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(s.join(".s.PGSQL.5999")).unwrap();
        fs::write(s.join(".s.PGSQL.5999.lock"), format!("{me}\n/x\n")).unwrap();
        write_rec(5999, me);
        let b = autopg_runtime(Path::new("/p.plist"), exe.clone(), r.clone(), s.clone(), 5999).unwrap();
        assert_eq!((b.pid, b.autopg_pid, b.runtime_dir.clone()), (me, me, r.clone()));
        write_rec(5998, me);
        assert!(autopg_runtime(Path::new("/p.plist"), exe.clone(), r.clone(), s.clone(), 5999).unwrap_err().contains("disagrees"));
        write_rec(5999, std::os::unix::process::parent_id() as i32);
        assert!(autopg_runtime(Path::new("/p.plist"), exe.clone(), r.clone(), s.clone(), 5999).unwrap_err().contains("not the service's program"));
        write_rec(5999, me);
        fs::write(s.join(".s.PGSQL.5999.lock"), "1\n").unwrap();
        assert!(autopg_runtime(Path::new("/p.plist"), exe.clone(), r.clone(), s.clone(), 5999).unwrap_err().contains("not running on the service's socket"));

        let rec = dir.join("inst/memory-service.json");
        accept_binding(&dir, &rec, &b).unwrap();
        accept_binding(&dir, &rec, &AutopgBinding { pid: 42, ..b.clone() }).unwrap();
        assert!(accept_binding(&dir, &rec, &AutopgBinding { port: 5433, ..b.clone() }).unwrap_err().contains("changed"));

        let b331 = AutopgBinding { program: prog.canonicalize().unwrap(), ..b.clone() };
        assert_eq!(binding_json(&b331)["version"], "v3.3.1", "the bound version");
        let rec2 = dir.join("inst3/memory-service.json");
        accept_binding(&dir, &rec2, &b331).unwrap();
        let accepted_on = |release: &str, program: &Path| {
            let mut v = binding_json(&AutopgBinding { pid: 7, autopg_pid: 8, ..b331.clone() });
            v["version"] = json!(release);
            v["program"] = json!(program);
            fs::write(&rec2, format!("{v:#}\n")).unwrap();
            fs::read(&rec2).unwrap()
        };
        let was = accepted_on("v3.2.2", &v322.canonicalize().unwrap());
        accept_binding(&dir, &rec2, &b331).unwrap();
        accept_binding(&dir, &rec2, &AutopgBinding { pid: 42, autopg_pid: 43, ..b331.clone() }).unwrap();
        assert_eq!(fs::read(&rec2).unwrap(), was, "the accepted record is never rewritten");
        for bad in [
            AutopgBinding { port: 5433, ..b331.clone() },
            AutopgBinding { socket_dir: dir.join("elsewhere"), ..b331.clone() },
            AutopgBinding { runtime_dir: dir.join("rt2"), ..b331.clone() },
            AutopgBinding { plist: "/other.plist".into(), ..b331.clone() },
            AutopgBinding { program: dir.join("elsewhere/.local/share/autopg/v3.3.1/autopg"), ..b331.clone() },
        ] {
            assert!(accept_binding(&dir, &rec2, &bad).unwrap_err().contains("changed since this instance accepted it"), "{bad:?}");
        }
        assert_eq!(fs::read(&rec2).unwrap(), was);
        for (release, program) in [("v3.2.1", v321.clone()), ("v9.9.9", v999.clone()), ("v3.2.2", dir.join("elsewhere/.local/share/autopg/v3.2.2/autopg")), ("v3.2.2", v321.clone())] {
            accepted_on(release, &program);
            assert!(accept_binding(&dir, &rec2, &b331).unwrap_err().contains("changed since this instance accepted it"), "accepted {release} at {}", program.display());
        }

        let linked = dir.join("inst2/memory-service.json");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere.json"), &linked).unwrap();
        assert!(accept_binding(&dir, &linked, &b).is_err());
        assert!(!dir.join("elsewhere.json").exists());

        let plist = dir.join("svc.plist");
        fs::write(&plist, r#"<?xml version="1.0"?><plist version="1.0"><dict><key>Label</key><string>x</string></dict></plist>"#).unwrap();
        fs::set_permissions(&plist, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_service_record(&plist).unwrap()["Label"], "x");
        fs::set_permissions(&plist, fs::Permissions::from_mode(0o620)).unwrap();
        assert!(read_service_record(&plist).unwrap_err().contains("writable only by this user"));
        fs::remove_dir_all(&dir).unwrap();
    }

    fn m12_fixture(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("m12-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let state = dir.join("state");
        fs::create_dir_all(state.join("instances")).unwrap();
        (dir, state)
    }

    #[test]
    fn m12_r1_owned_dirs_refuse_a_symlinked_component_and_create_real_dirs() {
        let (dir, state) = m12_fixture("owned");
        let foreign = dir.join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        std::os::unix::fs::symlink(&foreign, state.join("instances/j")).unwrap();
        assert!(owned_dir(&state, &state.join("instances/j/agentd")).is_err());
        assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
        assert!(owned_dir(&state, &dir.join("elsewhere")).is_err(), "outside the state root");
        let made = owned_dir(&state, &state.join("instances/k/agentd")).unwrap();
        assert!(fs::symlink_metadata(&made).unwrap().is_dir());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_packaged_agentd_environment_is_exactly_the_launch_contract() {
        let h = instance_homes(Path::new("/d"), "i").unwrap();
        let b = AutopgBinding { plist: "/p".into(), program: "/a".into(), runtime_dir: "/rt".into(), socket_dir: "/rt/pgserve".into(), port: 5432, pid: 2, autopg_pid: 3 };
        let bind = free_loopback().unwrap();
        assert!(bind.ip().is_loopback() && bind.port() != 0);
        let env: std::collections::BTreeMap<_, _> = agentd_env(Path::new("/A.app/Contents"), &h, &b, bind, None, None).into_iter().collect();
        let keys: Vec<&str> = env.keys().map(String::as_str).collect();
        assert_eq!(keys, ["HOME", "PATH", "RTH_AGENT_BIND", "RTH_APP_VERSION", "RTH_BACKEND", "RTH_INSTANCE_ROOT", "RTH_JOURNAL", "RTH_MEMORY_BRAIN_MCP", "RTH_MEMORY_BRAIN_RUNTIME", "RTH_MEMORY_BRAIN_SEAM", "RTH_MEMORY_BRAIN_SRC", "RTH_MEMORY_BUN", "RTH_STATE"]);
        assert_eq!(env["RTH_INSTANCE_ROOT"], "/d/instances/i");
        assert_eq!((env["RTH_STATE"].as_str(), env["RTH_JOURNAL"].as_str()), ("/d/instances/i/agentd/state.json", "/d/instances/i/agentd/journal.jsonl"));
        assert_eq!(env["RTH_AGENT_BIND"], bind.to_string());
        assert_eq!(env["RTH_BACKEND"], "fake");
        assert_eq!(env["RTH_MEMORY_BRAIN_RUNTIME"], "/rt");
        assert_eq!(env["RTH_MEMORY_BRAIN_MCP"], "/A.app/Contents/MacOS/brain-mcp");
        assert!(!env.contains_key("RTH_AGENT_MODE"));
        let with_key: Vec<_> = agentd_env(Path::new("/A.app/Contents"), &h, &b, bind, Some(("OPENAI_API_KEY", "k")), Some("allow:openai-realtime")).into_iter().filter(|(k, _)| k.contains("KEY") || k.contains("EGRESS")).collect();
        assert_eq!(with_key, [("OPENAI_API_KEY".to_string(), "k".to_string()), ("RTH_MEMORY_EGRESS".to_string(), "allow:openai-realtime".to_string())]);
        assert!(!env.contains_key("RTH_MEMORY_EGRESS"), "RTH_MEMORY_EGRESS only when the admitted launch passes it");
    }

    #[test]
    fn the_instance_home_lives_under_the_state_root() {
        assert_eq!(instance_homes(Path::new("/d"), "i-1").unwrap(), InstanceHomes { instance: "/d/instances/i-1".into() });
        for bad in ["", "..", ".hidden", "a/b", "a b", &"x".repeat(65)] {
            assert!(instance_homes(Path::new("/d"), bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn m12b2_r1_the_write_mode_is_packaged_even_when_configuration_fails() {
        let missing_instance: Result<Config, String> = Err("this app has no instance yet (\"instance\" in <data>/app.json)".into());
        let invalid: Result<Config, String> = Err("--host-listen: invalid socket address syntax".into());
        assert_eq!(startup_mode(true, &missing_instance), WriteMode::Packaged(None));
        assert_eq!(startup_mode(true, &invalid), WriteMode::Packaged(None));
        assert_eq!(startup_mode(false, &invalid), WriteMode::Dev);
    }

    #[test]
    fn m12b2_r1_failed_packaged_configuration_logs_to_stderr_only_and_writes_nothing() {
        let (dir, state, foreign) = m12b_fixture("cfgfail");
        std::os::unix::fs::symlink(&foreign, state.join("app")).unwrap();
        let log = Log::open_as(&WriteMode::Packaged(None), &state.join("app/unconfigured"));
        log.line("FICTIONAL_LOG_CANARY");
        assert!(!log.writes_file());
        assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0, "nothing created through the link");
        assert!(owned_open_as(&WriteMode::Packaged(None), &state.join("x.log"), Open::Append).is_err());
        assert!(!state.join("x.log").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m12b2_r1_an_admitted_root_refuses_outside_paths_and_never_truncates_them() {
        let (dir, state, foreign) = m12b_fixture("outside");
        fs::write(foreign.join("victim.txt"), "UNCHANGED").unwrap();
        let mode = WriteMode::Packaged(Some(state.clone()));
        assert!(owned_open_as(&mode, &foreign.join("victim.txt"), Open::Truncate).is_err());
        assert_eq!(fs::read_to_string(foreign.join("victim.txt")).unwrap(), "UNCHANGED");
        let log = Log::open_as(&mode, &foreign.join("app"));
        assert!(!log.writes_file());
        assert!(!foreign.join("app/app.log").exists());

        assert!(owned_open_as(&mode, &state.join("app/host-1/agentd.log"), Open::Truncate).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m12b2_r2_a_fifo_leaf_with_no_reader_is_refused_promptly() {
        let (dir, state, _foreign) = m12b_fixture("fifo");
        fs::create_dir_all(state.join("app/host-1")).unwrap();
        let fifo = state.join("app/host-1/app.log");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let (s2, f2) = (state.clone(), fifo.clone());
        std::thread::spawn(move || {
            let _ = tx.send(owned_open_in(&s2, &f2, Open::Append).is_err());
        });
        let refused = rx.recv_timeout(Duration::from_millis(1500));
        if refused.is_err() {

            use std::os::unix::fs::OpenOptionsExt;
            let _ = OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&fifo);
        }
        assert_eq!(refused, Ok(true), "a FIFO is refused within 1.5 s, without blocking");
        let mut f = owned_open_in(&state, &state.join("app/host-1/normal.log"), Open::Append).unwrap();
        f.write_all(b"ok").unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    fn m12b_fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("m12b-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let state = dir.join("state");
        let foreign = dir.join("foreign");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&foreign).unwrap();
        (dir, state, foreign)
    }

    #[test]
    fn m12b_r1_a_linked_log_parent_is_refused_and_nothing_is_created_through_it() {
        let (dir, state, foreign) = m12b_fixture("parent");
        std::os::unix::fs::symlink(&foreign, state.join("app")).unwrap();
        let log = Log::open_in(Some(&state), &state.join("app/host-1"));
        log.line("FICTIONAL_LOG_CANARY");
        assert!(!log.writes_file(), "a refused log writes to stderr only");
        assert!(!foreign.join("host-1/app.log").exists(), "nothing created through the link");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m12b_r1_a_linked_log_leaf_is_refused_and_the_foreign_file_is_unchanged() {
        let (dir, state, foreign) = m12b_fixture("leaf");
        fs::create_dir_all(state.join("app/host-1")).unwrap();
        fs::write(foreign.join("victim.txt"), "UNCHANGED").unwrap();
        std::os::unix::fs::symlink(foreign.join("victim.txt"), state.join("app/host-1/app.log")).unwrap();
        let log = Log::open_in(Some(&state), &state.join("app/host-1"));
        log.line("FICTIONAL_LOG_CANARY");
        assert!(!log.writes_file());
        assert_eq!(fs::read_to_string(foreign.join("victim.txt")).unwrap(), "UNCHANGED");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m12b_r1_a_hard_linked_leaf_is_refused_and_never_truncated() {
        let (dir, state, foreign) = m12b_fixture("hard");
        fs::create_dir_all(state.join("app/host-1")).unwrap();
        fs::write(foreign.join("victim.txt"), "UNCHANGED").unwrap();
        fs::hard_link(foreign.join("victim.txt"), state.join("app/host-1/llama-server.log")).unwrap();
        assert!(owned_open_in(&state, &state.join("app/host-1/llama-server.log"), Open::Truncate).is_err());
        assert_eq!(fs::read_to_string(foreign.join("victim.txt")).unwrap(), "UNCHANGED");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn m12b_r1_normal_logging_writes_its_own_file() {
        let (dir, state, _foreign) = m12b_fixture("normal");
        let log = Log::open_in(Some(&state), &state.join("app/host-1"));
        log.line("hello");
        assert!(log.writes_file());
        assert!(fs::read_to_string(state.join("app/host-1/app.log")).unwrap().contains("hello"));
        let mut f = owned_open_in(&state, &state.join("app/host-1/agentd.log"), Open::Truncate).unwrap();
        f.write_all(b"x").unwrap();
        assert_eq!(fs::read_to_string(state.join("app/host-1/agentd.log")).unwrap(), "x");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn cell() -> QuitCell {
        (Mutex::new(Quit::Idle), Condvar::new())
    }

    #[test]
    fn a_repeated_quit_runs_the_goodbye_once() {
        let q = cell();
        assert!(claim(&q), "the first request claims the quit");
        assert!(!claim(&q), "a second Cmd+Q, close or signal during the goodbye is a no-op");
        finish(&q);
        assert!(!claim(&q), "after the quit, nothing restarts it");
        assert_eq!(*q.0.lock().unwrap(), Quit::Done);
    }

    #[test]
    fn a_users_quit_is_cancelled_and_self_exits_a_systems_waits_for_the_reply() {
        use objc::{NS_TERMINATE_CANCEL as CANCEL, NS_TERMINATE_LATER as LATER, NS_TERMINATE_NOW as NOW};

        assert_eq!(terminate_reply(Quit::Idle, false), (CANCEL, false, true), "Cmd+Q, the menu, the Dock");
        assert_eq!(terminate_reply(Quit::Idle, true), (LATER, true, true), "logout, restart, shutdown");
        assert_eq!(terminate_reply(Quit::Running, false), (CANCEL, false, true), "a second Cmd+Q mid-goodbye: begin_quit is a no-op");
        assert_eq!(terminate_reply(Quit::Running, true), (LATER, true, true), "logout mid-goodbye: the running quit replies");
        assert_eq!(terminate_reply(Quit::Done, false), (NOW, false, false));
        assert_eq!(terminate_reply(Quit::Done, true), (NOW, false, false));
    }

    #[test]
    fn exit_waits_for_a_quit_in_flight_or_stops_the_children_itself() {
        let q = Arc::new(cell());
        assert!(claim(&q));
        let q2 = q.clone();
        let finisher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            finish(&q2);
        });
        let t = Instant::now();
        let mut stopped = false;
        assert_eq!(exit_wait(&q, Duration::from_secs(5), || stopped = true), ExitWait::Waited(true));
        assert!(t.elapsed() >= Duration::from_millis(140), "Exit returned only after the quit in flight finished");
        assert!(!stopped, "Exit does not stop again what the quit in flight is stopping");
        finisher.join().unwrap();
        assert_eq!(exit_wait(&q, Duration::from_secs(5), || panic!("nothing to stop")), ExitWait::AlreadyDone);

        let idle = cell();
        let mut ran = false;
        assert_eq!(exit_wait(&idle, Duration::from_secs(5), || ran = true), ExitWait::Stopped);
        assert!(ran);
        assert_eq!(*idle.0.lock().unwrap(), Quit::Done);

        let stuck = cell();
        assert!(claim(&stuck));
        let t = Instant::now();
        assert_eq!(exit_wait(&stuck, Duration::from_millis(80), || panic!("not while one is running")), ExitWait::Waited(false));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn quitting_stops_the_dictation_sidecar_and_all_it_started_even_when_it_ignores_sigterm() {
        let dir = std::env::temp_dir().join(format!("rth-app-sidecar-quit-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (s, pid) = crate::nemo::tests::deaf_supervisor(&dir);
        dictation::install_sidecar_for_test(s);
        let log = Log::open(&dir);
        exit_quit(&Arc::default(), &log);

        assert_eq!(crate::nemo::tests::group_with_parents(pid), vec![]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_line_never_carries_a_bearer_key_or_a_token() {
        assert_eq!(redact("GET /ready Authorization: Bearer 0a1b2c3d\r\n"), "GET /ready Authorization: Bearer <redacted>\r\n");
        assert_eq!(redact("authorization: BEARER k1; then token=abc&x=1"), "authorization: BEARER <redacted>; then token=<redacted>&x=1");
        assert_eq!(redact(r#"{"Authorization":"Bearer abc123"}"#), r#"{"Authorization":"Bearer <redacted>"}"#);
        assert_eq!(redact("no secrets here"), "no secrets here");
    }

    #[test]
    fn a_quit_during_startup_stops_spawning_and_stops_what_was_started() {
        let dir = std::env::temp_dir().join(format!("rth-app-quit-test-{}", std::process::id()));
        let log = Log::open(&dir);
        let owned: Shared = Arc::default();

        let (agent_pid, _) = spawn_owned(&owned, Slot::Agent, Command::new("/bin/sleep").arg("30").process_group(0)).unwrap();
        let (llm_pid, _) = spawn_owned(&owned, Slot::Llm, &mut Command::new("/usr/bin/true")).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        assert!(spawn_owned(&owned, Slot::Agent, &mut Command::new("/usr/bin/true")).unwrap_err().contains("still owned"));
        let taken = take_owned(&owned).expect("the first take marks closing");

        assert_eq!(spawn_owned(&owned, Slot::Llm, &mut Command::new("/usr/bin/true")).unwrap_err(), CLOSING);
        assert!(take_owned(&owned).is_none());
        assert!(cancelled(&owned), "readiness waits see closing at once");
        stop_taken(taken, &log);
        let text = fs::read_to_string(dir.join("app.log")).unwrap();
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("missing {needle:?} in\n{text}"));
        let term = at(&format!("close: SIGTERM -> owned rth-agentd pid {agent_pid}"));
        let exited = at(&format!("rth-agentd (pid {agent_pid}) exited"));
        let llm_line = at(&format!("llama-server (pid {llm_pid}) had already exited"));
        assert!(term < exited && exited < llm_line, "the live agent is signalled, then reaped; the exited model server is reaped, never signalled:\n{text}");
        assert!(!text.contains(&format!("SIGTERM -> owned llama-server pid {llm_pid}")), "an already-exited child is never signalled");
        assert_eq!(unsafe { libc::kill(agent_pid as i32, 0) }, -1, "the agent stand-in is gone");
        let _ = fs::remove_dir_all(&dir);
    }

}
