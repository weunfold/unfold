

#![cfg_attr(not(test), allow(dead_code))]

use crate::dsh_ports::Ports;
use crate::Log;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PORTS_FREE_WITHIN: Duration = Duration::from_secs(10);

const CLI_WAIT: Duration = Duration::from_secs(30);
const DELAYS: [u64; 5] = [1, 2, 4, 8, 16];
const WINDOW: Duration = Duration::from_secs(600);

const KEPT: [&str; 3] = ["PATH", "LANG", "TMPDIR"];

pub const CLI: &str = "runtime/node_modules/@automagik/omni/bin/omni";

pub const NOTICE_EN: &str = "Omni isn't running";
pub const NOTICE_PT: &str = "Omni não está em execução";
pub const RETRY_EN: &str = "Retry";
pub const RETRY_PT: &str = "Tentar de novo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub pid: i32,
    pub pgid: i32,
    pub start_time: u64,
    pub command: String,
}

pub trait Children: Send + Sync {
    fn spawn_recorded(&self, name: &'static str, cmd: &mut Command) -> Result<Recorded, String>;
    fn alive(&self, r: &Recorded) -> bool;
    fn stop_recorded(&self, r: &Recorded);
}

pub struct OmniCfg {

    pub instance: PathBuf,

    pub pack: PathBuf,

    pub bun: PathBuf,

    pub version: String,

    pub ports: Ports,

    pub bots_home: PathBuf,

    pub env: Vec<(String, String)>,

    pub healthy_within: Duration,
    pub children: Arc<dyn Children>,
    pub log: Log,
}

pub struct OwnOmni {
    pub pid: i32,
    pub pgid: i32,
    pub api: u16,
    rec: Recorded,
    children: Arc<dyn Children>,
}

pub enum Omni {
    Running(OwnOmni),
    NotRunning,
}

impl Omni {

    pub fn notice(&self, pt: bool) -> Option<(&'static str, &'static str)> {
        match self {
            Omni::Running(_) => None,
            Omni::NotRunning if pt => Some((NOTICE_PT, RETRY_PT)),
            Omni::NotRunning => Some((NOTICE_EN, RETRY_EN)),
        }
    }

    pub fn bots_wait(&self) -> bool {
        matches!(self, Omni::NotRunning)
    }
}

#[derive(Debug)]
pub struct Layout {
    pub sandbox: PathBuf,
    pub home: PathBuf,
}

impl Layout {
    pub fn of(instance: &Path) -> Layout {
        Layout { sandbox: instance.join("omni/sandbox"), home: instance.join("omni/home") }
    }
    fn config_dir(&self) -> PathBuf {
        self.sandbox.join("config")
    }
    fn config(&self) -> PathBuf {
        self.sandbox.join("config/config.json")
    }
    fn password(&self) -> PathBuf {
        self.sandbox.join("config/pg-password")
    }
    fn state(&self) -> PathBuf {
        self.sandbox.join("sandbox.json")
    }
}

pub fn prepare(cfg: &OmniCfg) -> Result<Layout, String> {
    if !cfg.bun.is_absolute() {
        return Err("the pack's bun is not an absolute path".into());
    }
    let inst = fs::symlink_metadata(&cfg.instance).map_err(|e| format!("instance {}: {e}", cfg.instance.display()))?;
    if !inst.is_dir() {
        return Err(format!("instance {} is not a directory", cfg.instance.display()));
    }
    let l = Layout::of(&cfg.instance);

    let previous = read_private(&l.state())?.and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let legacy = match &previous {
        Some(p) => !p["pgPasswordFile"].as_str().is_some_and(|f| !f.is_empty()),
        None => !ran_with_password_file(&l.sandbox.join("data/pg")),
    };
    if legacy && l.sandbox.join("data/pg/PG_VERSION").exists() {
        return Err("its Postgres data was made without a password file, which it can't take now; it is left as it is".into());
    }
    let below = ["omni", "omni/sandbox", "omni/home"]
        .into_iter()
        .map(|d| cfg.instance.join(d))
        .chain(["config", "data", "data/pg", "data/nats", "data/media", "data/packages", "cache", "run", "logs", "bin", "pgserve", "autopg"].map(|d| l.sandbox.join(d)));
    for d in below {
        own_dir(&d)?;
    }

    let bin = l.sandbox.join("pgserve/bin");
    if fs::symlink_metadata(&bin).is_ok_and(|m| m.file_type().is_symlink()) {
        fs::remove_file(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    }
    own_dir(&bin)?;
    let src = cfg.pack.join("omni");
    let autopg = autopg_version(&src.join("autopg"))?;
    link(&src.join("runtime"), &l.sandbox.join("runtime"))?;
    link(&src.join("bin/nats-server"), &l.sandbox.join("bin/nats-server"))?;

    link(&src.join("autopg").join(&autopg), &l.sandbox.join("autopg").join(&autopg))?;
    for e in fs::read_dir(src.join("pgserve/bin")).map_err(|e| format!("the pack's omni/pgserve/bin: {e}"))? {
        let e = e.map_err(|e| e.to_string())?;
        link(&e.path(), &bin.join(e.file_name()))?;
    }
    let key = match read_private(&l.config())? {
        Some(text) => key_of(&text)?,
        None => format!("omni_sk_{}", random_hex(16)?),
    };
    match read_private(&l.password())? {
        Some(p) if p.trim().len() >= 32 => {}
        Some(_) => return Err("config/pg-password is too short; it is left as it is".into()),
        None => write_private(&l.password(), random_hex(32)?.as_bytes())?,
    }
    let created = previous.as_ref().and_then(|v| v["createdAt"].as_str().map(String::from)).unwrap_or_else(now_iso);
    let api_url = format!("http://127.0.0.1:{}", cfg.ports.omni_api);
    let state = json!({
        "schemaVersion": 1,
        "version": cfg.version,
        "packageSpec": cfg.version,
        "bun": cfg.bun,
        "ports": {"api": cfg.ports.omni_api, "pg": cfg.ports.omni_db, "nats": cfg.ports.omni_nats},
        "atLogin": false,
        "autopg": autopg,
        "pgPasswordFile": l.password(),
        "createdAt": created,
        "updatedAt": now_iso(),
    });
    write_json(&l.state(), &state)?;

    let config = json!({
        "apiUrl": api_url,
        "apiKey": key,
        "format": "human",
        "server": {
            "port": cfg.ports.omni_api,
            "databaseUrl": format!("postgresql://postgres@127.0.0.1:{}/omni", cfg.ports.omni_db),
            "dataDir": l.sandbox.join("data"),
            "logLevel": "info",
            "nodeEnv": "production",
            "natsHost": "127.0.0.1",
        },
    });
    write_json(&l.config(), &config)?;
    let rel = cfg.bots_home.strip_prefix(&cfg.instance).map_err(|_| "the Bot team's home is not inside the instance".to_string())?;
    let mut at = cfg.instance.clone();
    for c in rel.components() {
        at.push(c);
        own_dir(&at)?;
    }
    write_json(&cfg.bots_home.join("omni.json"), &json!({"url": api_url, "key": key}))?;
    Ok(l)
}

const PG_HBA_HEADER: &str = "# Written by pgserve at every start (--password-file).";

fn ran_with_password_file(data: &Path) -> bool {
    fs::read_to_string(data.join("pg_hba.conf")).is_ok_and(|t| t.trim().starts_with(&format!("{PG_HBA_HEADER}\n")))
}

fn autopg_version(dir: &Path) -> Result<String, String> {
    let mut found = Vec::new();
    for e in fs::read_dir(dir).map_err(|e| format!("the pack's omni/autopg: {e}"))? {
        let name = e.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            found.push(name);
        }
    }
    let [v] = found.as_slice() else {
        return Err(format!("the pack's omni/autopg holds {} releases, not one", found.len()));
    };
    let m: Value = fs::read(dir.join(v).join("manifest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(|| format!("the pack's autopg {v} has no manifest"))?;
    if m["name"] != "autopg" || m["version"] != v.as_str() {
        return Err(format!("the pack's autopg {v}: its manifest names {} {}", m["name"], m["version"]));
    }
    Ok(v.clone())
}

pub fn start(cfg: &OmniCfg) -> Result<OwnOmni, String> {
    let l = prepare(cfg)?;
    spawn(cfg, &l)
}

fn spawn(cfg: &OmniCfg, l: &Layout) -> Result<OwnOmni, String> {
    let mut cmd = command(cfg, l, "supervisor.log")?;
    cmd.args(["sandbox", "run", "--embedded", "--home"]).arg(&l.sandbox);
    let rec = cfg.children.spawn_recorded("omni", &mut cmd)?;
    if rec.pgid != rec.pid {
        cfg.children.stop_recorded(&rec);
        return Err(format!("omni (pid {}) does not lead its own process group", rec.pid));
    }
    cfg.log.line(&format!("omni: supervisor pid {} (group {}), api 127.0.0.1:{}", rec.pid, rec.pgid, cfg.ports.omni_api));
    Ok(OwnOmni { pid: rec.pid, pgid: rec.pgid, api: cfg.ports.omni_api, rec, children: cfg.children.clone() })
}

pub fn stop(o: &OwnOmni) {
    o.children.stop_recorded(&o.rec);
}

pub fn healthy(api: u16, within: Duration) -> bool {
    let end = Instant::now() + within;
    loop {
        if status(api, "GET", "/api/v2/health", Duration::from_secs(2)) == Some(200) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn start_with_dsh(cfg: &OmniCfg, start_dsh: &mut dyn FnMut()) -> Omni {
    let omni = retry(cfg);
    start_dsh();
    omni
}

pub fn retry(cfg: &OmniCfg) -> Omni {
    match start(cfg) {
        Err(why) => {
            cfg.log.line(&format!("omni: not started ({why})"));
            Omni::NotRunning
        }
        Ok(o) if !healthy(o.api, cfg.healthy_within) => {
            stop(&o);
            cfg.log.line(&format!("omni: not healthy within {} s; stopped", cfg.healthy_within.as_secs()));
            Omni::NotRunning
        }
        Ok(o) => {
            if let Err(why) = claim(cfg) {
                cfg.log.line(&format!("omni: {why}"));
            }
            Omni::Running(o)
        }
    }
}

pub fn claim(cfg: &OmniCfg) -> Result<(), String> {
    run_cli(cfg, &["claim"])
}

pub fn approve(cfg: &OmniCfg, code: &str) -> Result<(), String> {
    let ok = (4..=17).contains(&code.len()) && code.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric()));
    if !ok {
        return Err("omni approve: not a device code".into());
    }
    run_cli(cfg, &["approve", code])
}

fn run_cli(cfg: &OmniCfg, args: &[&str]) -> Result<(), String> {
    let l = Layout::of(&cfg.instance);
    let mut cmd = command(cfg, &l, "cli.log")?;
    let mut child = cmd.args(args).spawn().map_err(|e| format!("omni {}: {e}", args[0]))?;
    let end = Instant::now() + CLI_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => return Ok(()),
            Ok(Some(s)) => return Err(format!("omni {} ended {s}; see the instance's omni logs", args[0])),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            _ => {

                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("omni {} took over {} s; stopped", args[0], CLI_WAIT.as_secs()));
            }
        }
    }
}

#[derive(Default)]
pub struct Restarts {
    crashes: Vec<Instant>,
}

impl Restarts {

    pub fn after_crash(&mut self, now: Instant) -> Option<Duration> {
        self.crashes.retain(|t| now.saturating_duration_since(*t) < WINDOW);
        let n = self.crashes.len();
        if n >= DELAYS.len() {
            return None;
        }
        self.crashes.push(now);
        Some(Duration::from_secs(DELAYS[n]))
    }

    pub fn reset(&mut self) {
        self.crashes.clear();
    }
}

pub fn watch(cfg: &OmniCfg, o: &OwnOmni, r: &mut Restarts, now: Instant, wait: &mut dyn FnMut(Duration)) -> Result<Option<OwnOmni>, String> {
    if o.children.alive(&o.rec) {
        return Ok(None);
    }

    stop(o);
    let d = r.after_crash(now).ok_or_else(|| format!("omni stopped {} times in {} min", DELAYS.len() + 1, WINDOW.as_secs() / 60))?;
    cfg.log.line(&format!("omni: supervisor pid {} ended; starting again in {} s", o.pid, d.as_secs()));
    wait(d);
    let p = &cfg.ports;
    if !ports_free(&[p.omni_api, p.omni_nats, p.omni_db], PORTS_FREE_WITHIN) {
        return Err(format!("omni: 127.0.0.1:{}, :{} or :{} still held {} s after the supervisor ended", p.omni_api, p.omni_nats, p.omni_db, PORTS_FREE_WITHIN.as_secs()));
    }
    start(cfg).map(Some)
}

fn ports_free(ports: &[u16], within: Duration) -> bool {
    let end = Instant::now() + within;
    loop {
        if ports.iter().all(|&p| TcpListener::bind((Ipv4Addr::LOCALHOST, p)).is_ok()) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn command(cfg: &OmniCfg, l: &Layout, log: &str) -> Result<Command, String> {
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(l.sandbox.join("logs").join(log))
        .map_err(|e| format!("logs/{log}: {e}"))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&cfg.bun);
    cmd.arg(cfg.pack.join("omni").join(CLI))
        .env_clear()
        .envs(cli_env(cfg, l))
        .current_dir(&l.sandbox)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    Ok(cmd)
}

#[cfg(all(test, feature = "menu_bar"))]
pub fn process_env(cfg: &OmniCfg) -> Vec<(String, String)> {
    cli_env(cfg, &Layout::of(&cfg.instance))
}

fn cli_env(cfg: &OmniCfg, l: &Layout) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = cfg.env.iter().filter(|(k, _)| KEPT.contains(&k.as_str())).cloned().collect();
    env.push(("HOME".into(), l.home.to_string_lossy().into_owned()));
    env.push(("OMNI_CONFIG_DIR".into(), l.config_dir().to_string_lossy().into_owned()));
    env.push(("OMNI_TELEMETRY".into(), "false".into()));
    env.push(("OMNI_SKIP_POSTINSTALL_PIN".into(), "1".into()));
    env
}

fn status(port: u16, method: &str, path: &str, timeout: Duration) -> Option<u16> {
    let mut s = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), timeout).ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    s.set_write_timeout(Some(timeout)).ok()?;
    write!(s, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").ok()?;
    let mut head = [0u8; 32];
    let n = s.read(&mut head).ok()?;
    std::str::from_utf8(&head[..n]).ok()?.split_whitespace().nth(1)?.parse().ok()
}

pub(crate) fn own_dir(p: &Path) -> Result<(), String> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => return Err(format!("{} is a link", p.display())),
        Ok(m) if !m.is_dir() => return Err(format!("{} is not a directory", p.display())),
        Ok(m) if m.uid() != unsafe { libc::geteuid() } => return Err(format!("{} is another user's", p.display())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new().mode(0o700).create(p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        Err(e) => return Err(format!("{}: {e}", p.display())),
    }
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", p.display()))
}

fn link(target: &Path, at: &Path) -> Result<(), String> {
    fs::symlink_metadata(target).map_err(|e| format!("the pack has no {}: {e}", target.display()))?;
    match fs::symlink_metadata(at) {
        Ok(m) if m.file_type().is_symlink() => {
            if fs::read_link(at).ok().as_deref() == Some(target) {
                return Ok(());
            }
            fs::remove_file(at).map_err(|e| format!("{}: {e}", at.display()))?;
        }
        Ok(_) => return Err(format!("{} is not a link into the pack", at.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", at.display())),
    }
    std::os::unix::fs::symlink(target, at).map_err(|e| format!("{}: {e}", at.display()))
}

fn read_private(p: &Path) -> Result<Option<String>, String> {
    let mut f = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(p) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: refused ({e})", p.display())),
    };
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o600 {
        return Err(format!("{}: refused (not this user's 0600 file)", p.display()));
    }
    let mut text = String::new();
    f.read_to_string(&mut text).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(Some(text))
}

fn write_private(p: &Path, bytes: &[u8]) -> Result<(), String> {
    write_mode(p, bytes, 0o600)
}

pub(crate) fn write_mode(p: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let dir = p.parent().ok_or("no parent dir")?;
    let name = p.file_name().ok_or("no file name")?.to_string_lossy();
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let tmp = dir.join(format!(".{name}.tmp-{}-{nanos}", std::process::id()));
    let wrote = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|_| f.sync_all()))
        .and_then(|_| fs::rename(&tmp, p));
    wrote.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", p.display())
    })
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    text.push('\n');
    write_private(p, text.as_bytes())
}

fn key_of(config: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(config).map_err(|_| "config/config.json is not JSON; it is left as it is".to_string())?;
    let key = v["apiKey"].as_str().unwrap_or_default();
    let hex = key.strip_prefix("omni_sk_").unwrap_or_default();
    if hex.len() == 32 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(key.to_string())
    } else {
        Err("config/config.json holds no Omni primary key; it is left as it is".into())
    }
}

fn random_hex(n: usize) -> Result<String, String> {
    let mut b = vec![0u8; n];
    fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("no random bytes: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn now_iso() -> String {
    iso(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

fn iso(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3_600, rem % 3_600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::net::TcpListener;
    use std::os::unix::process::CommandExt;
    use std::process::Child;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const REC: &str = "UNFOLD_FAKE_OMNI_REC";
    const ROLE: &str = "UNFOLD_FAKE_OMNI_ROLE";

    const SHELL_ADDED: [&str; 4] = ["PWD", "SHLVL", "_", "OLDPWD"];
    const CODE: &str = "ABCD-EFGH";

    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn within(d: Duration, mut ok: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + d;
        while !ok() {
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    fn ports_free(p: &Ports) -> bool {
        [p.omni_api, p.omni_db, p.omni_nats].iter().all(|&port| TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok())
    }

    const PIDS: &str = "UNFOLD_FAKE_OMNI_PIDS";
    const RUN: &str = "UNFOLD_FAKE_OMNI_RUN";

    fn pids_file() -> PathBuf {
        crate::child_fixture::paths().omni_pids.clone()
    }

    fn record_pid(file: &Path, pid: i32, pgid: i32, what: &str, run: &str) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(f, "{pid} {pgid} {what} {run}");
        }
    }

    fn recorded(run: &str) -> Vec<(i32, i32, String)> {
        let text = fs::read_to_string(pids_file()).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let mut w = l.splitn(4, ' ');
                let (pid, pgid, what) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?, w.next()?.to_string());
                (w.next()? == run).then_some((pid, pgid, what))
            })
            .collect()
    }

    fn ppid(pid: i32) -> Option<i32> {
        let mut i: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut i as *mut libc::proc_bsdinfo).cast(), size) };
        (n == size).then_some(i.pbi_ppid as i32)
    }

    fn orphans_naming(tree: &Path) -> Vec<(i32, String)> {
        let ps = Command::new("/bin/ps").args(["-axo", "pid=,ppid=,args="]).output().unwrap();
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                let (pid, ppid) = (w.next()?.parse().ok()?, w.next()?);
                let args = w.collect::<Vec<_>>().join(" ");
                (ppid == "1" && args.contains(&*tree.to_string_lossy())).then_some((pid, args))
            })
            .collect()
    }

    #[derive(Default)]
    struct Kids {
        held: Mutex<Vec<(Child, i32)>>,

        started: Option<PathBuf>,

        groups: Mutex<Vec<i32>>,
    }

    impl Kids {
        fn with_start_ack(dir: PathBuf) -> Self {
            Self { held: Mutex::default(), started: Some(dir), groups: Mutex::default() }
        }
    }

    fn leaderless(pgid: i32) -> bool {
        (unsafe { libc::killpg(pgid, 0) } == 0) && !alive(pgid)
    }

    impl Children for Kids {
        fn spawn_recorded(&self, _name: &'static str, cmd: &mut Command) -> Result<Recorded, String> {
            cmd.process_group(0);
            let child = cmd.spawn().map_err(|e| e.to_string())?;
            let pid = child.id() as i32;
            let info = crate::nemo::proc_info(pid as u32).ok_or("not listed")?;
            let rec = Recorded { pid, pgid: info.pgid, start_time: info.start_us, command: cmd.get_program().to_string_lossy().into_owned() };
            self.groups.lock().unwrap().push(info.pgid);
            self.held.lock().unwrap().push((child, info.pgid));
            if let Some(dir) = &self.started {
                let marker = dir.join(format!("{pid}.started"));
                if !within(Duration::from_secs(30), || marker.is_file()) {
                    self.stop_recorded(&rec);
                    return Err(format!("fake startup acknowledgement missing (pid {pid}; 30 s budget)"));
                }
            }
            Ok(rec)
        }
        fn alive(&self, r: &Recorded) -> bool {
            let mut held = self.held.lock().unwrap();
            held.iter_mut().any(|(c, _)| c.id() as i32 == r.pid && matches!(c.try_wait(), Ok(None)))
        }
        fn stop_recorded(&self, r: &Recorded) {
            let mut held = self.held.lock().unwrap();
            let Some(i) = held.iter().position(|(c, _)| c.id() as i32 == r.pid) else { return };
            let (mut child, pgid) = held.remove(i);
            let running = matches!(child.try_wait(), Ok(None));
            let same = running && pgid == r.pgid && crate::nemo::proc_info(r.pid as u32).is_some_and(|p| p.start_us == r.start_time && p.pgid == r.pgid);
            if running && !same {
                let _ = child.wait();
                return;
            }
            if !running && (pgid != r.pgid || !leaderless(r.pgid)) {
                return;
            }
            unsafe { libc::killpg(r.pgid, libc::SIGTERM) };
            let _ = child.wait();
            if !within(Duration::from_secs(5), || unsafe { libc::killpg(r.pgid, 0) } != 0) {
                unsafe { libc::killpg(r.pgid, libc::SIGKILL) };
                within(Duration::from_secs(5), || unsafe { libc::killpg(r.pgid, 0) } != 0);
            }
        }
    }

    impl Drop for Kids {

        fn drop(&mut self) {
            for (c, pgid) in self.held.get_mut().unwrap_or_else(|e| e.into_inner()).iter_mut() {
                if matches!(c.try_wait(), Ok(None)) {
                    unsafe { libc::killpg(*pgid, libc::SIGKILL) };
                }
                let _ = c.wait();
            }
            for &pgid in self.groups.get_mut().unwrap_or_else(|e| e.into_inner()).iter() {
                if leaderless(pgid) {
                    unsafe { libc::killpg(pgid, libc::SIGKILL) };
                }
            }
        }
    }

    struct Held(Vec<Child>);

    impl Held {
        fn spawn(&mut self, cmd: &mut Command, run: &str) -> u32 {
            let c = cmd.process_group(0).spawn().unwrap();
            let pid = c.id();
            record_pid(&pids_file(), pid as i32, pid as i32, "held", run);
            self.0.push(c);
            pid
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            for c in &mut self.0 {
                if matches!(c.try_wait(), Ok(None)) {
                    unsafe { libc::killpg(c.id() as i32, libc::SIGKILL) };
                }
                let _ = c.wait();
            }
        }
    }

    struct Fx {
        root: PathBuf,

        run: String,
        user: PathBuf,
        rec: PathBuf,
        app_log: PathBuf,
        planted: Arc<AtomicUsize>,
        cfg: OmniCfg,
    }

    fn out() -> Option<PathBuf> {
        std::env::var("UNFOLD_OMNI_CHILD_OUT").ok().filter(|d| !d.is_empty()).map(PathBuf::from)
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            if out().is_none_or(|o| !self.root.starts_with(o)) {
                let _ = fs::remove_dir_all(&self.root);
            }
        }
    }

    fn fake_bun(exe: &Path, pids: &Path, run: &str) -> String {
        format!(
            "#!/bin/sh\n# fake bun: records its argv and env, and its pid in the pids file, then runs the fake Omni in the test binary\nd=\"$(dirname \"$0\")/rec\"\nprintf '%s\\n' \"$@\" > \"$d/$$.argv\"\n/usr/bin/env > \"$d/$$.env\"\nprintf '%s %s bun %s\\n' \"$$\" \"$(/bin/ps -o pgid= -p $$ | /usr/bin/tr -d ' ')\" '{run}' >> '{pids}'\nexec /usr/bin/env {REC}=\"$d\" {PIDS}='{pids}' {RUN}='{run}' '{}' omni_child::tests::fake_omni --exact --ignored --nocapture --test-threads=1 -q\n",
            exe.display(),
            pids = pids.display()
        )
    }

    fn plant(user: &Path) -> Arc<AtomicUsize> {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for _ in l.incoming() {
                h.fetch_add(1, Ordering::SeqCst);
            }
        });
        fs::create_dir_all(user.join(".omni/sandbox")).unwrap();
        fs::write(user.join(".omni/config.json"), format!("{{\"apiUrl\":\"http://127.0.0.1:{port}\",\"apiKey\":\"planted\"}}\n")).unwrap();
        fs::write(user.join(".omni/sandbox/sandbox.json"), "{}\n").unwrap();
        fs::create_dir_all(user.join("Library/LaunchAgents")).unwrap();
        fs::write(user.join("Library/LaunchAgents/ai.namastex.omni.sandbox.plist"), "<plist/>\n").unwrap();
        hits
    }

    fn snapshot(dir: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for e in fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                if m.is_dir() {
                    todo.push(p);
                    out.insert(rel, format!("dir {:o}", m.mode()));
                } else {
                    out.insert(rel, format!("{:o} {:x}", m.mode(), Sha256::digest(fs::read(&p).unwrap())));
                }
            }
        }
        out
    }

    fn fx(tag: &str) -> Fx {
        match out() {
            Some(o) => fx_in(&o, &format!("oc-{tag}")),
            None => fx_in(&std::env::temp_dir(), &format!("oc-{tag}-{}", std::process::id())),
        }
    }

    const FAKE_AUTOPG: &str = "3.3.0";
    const FAKE_PG_RUNTIME: &str = "18.3.0-beta.17";

    fn fx_in(base: &Path, name: &str) -> Fx {
        let root = base.join(name);
        let _ = fs::remove_dir_all(&root);
        let omni = root.join("pack/omni");
        fs::create_dir_all(omni.join("runtime/node_modules/@automagik/omni/bin")).unwrap();
        fs::write(omni.join(CLI), "fake\n").unwrap();
        fs::create_dir_all(omni.join("bin")).unwrap();
        fs::write(omni.join("bin/nats-server"), "fake\n").unwrap();

        let release = omni.join("autopg").join(FAKE_AUTOPG);
        fs::create_dir_all(&release).unwrap();
        fs::write(release.join("autopg"), "fake\n").unwrap();
        fs::write(release.join("manifest.json"), format!(r#"{{"name":"autopg","version":"{FAKE_AUTOPG}","files":{{}}}}"#)).unwrap();
        let seed = omni.join("pgserve/bin/darwin-arm64");
        fs::create_dir_all(seed.join("bin")).unwrap();
        fs::write(seed.join(".version"), format!("{FAKE_PG_RUNTIME}\n")).unwrap();
        for b in ["initdb", "postgres"] {
            fs::write(seed.join("bin").join(b), "fake\n").unwrap();
        }
        let rec = root.join("pack/rec");
        fs::create_dir_all(&rec).unwrap();
        let bun = root.join("pack/bun");
        let made = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let run = format!("{}#{made}", root.display());
        fs::write(&bun, fake_bun(&std::env::current_exe().unwrap(), &pids_file(), &run)).unwrap();
        fs::set_permissions(&bun, fs::Permissions::from_mode(0o700)).unwrap();
        let instance = root.join("i");
        fs::create_dir_all(&instance).unwrap();
        let user = root.join("user");
        let planted = plant(&user);
        let port = || crate::nemo::free_port().unwrap();
        let ports = Ports { web: 0, bridge: 0, omni_api: port(), omni_db: port(), omni_nats: port() };
        let app = root.join("app");
        let log = Log::open_as(&crate::WriteMode::Dev, &app);
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let env = [
            ("PATH", "/usr/bin:/bin".to_string()),
            ("LANG", "pt_BR.UTF-8".to_string()),
            ("TMPDIR", s(&std::env::temp_dir())),

            ("HOME", s(&user)),
            ("USER", "someone".to_string()),
            ("OMNI_HOME", s(&user.join(".omni"))),
            ("OMNI_CONFIG_DIR", s(&user.join(".omni"))),
            ("DATABASE_URL", "postgresql://postgres:postgres@localhost:5432/omni".to_string()),
            ("SENTRY_DSN", "http://127.0.0.1:9/1".to_string()),
            ("OMNI_CLIENT_ID", "omni-desktop".to_string()),
        ]
        .map(|(k, v)| (k.to_string(), v))
        .to_vec();
        let cfg = OmniCfg {
            bots_home: instance.join("dsh/automagik/bots"),
            instance,
            pack: root.join("pack"),
            bun,
            version: "2.261025.4".into(),
            ports,
            env,
            healthy_within: Duration::from_secs(20),
            children: Arc::new(Kids::default()),
            log,
        };
        Fx { root, run, user, rec, app_log: app.join("app.log"), planted, cfg }
    }

    fn records(rec: &Path, what: &str) -> Vec<(i32, String)> {
        let mut out: Vec<(i32, String)> = fs::read_dir(rec)
            .unwrap()
            .filter_map(|e| {
                let name = e.ok()?.file_name().to_string_lossy().into_owned();
                let pid = name.strip_suffix(&format!(".{what}"))?.parse().ok()?;
                Some((pid, fs::read_to_string(rec.join(&name)).ok()?))
            })
            .collect();
        out.sort();
        out
    }

    fn env_of(text: &str) -> BTreeMap<String, String> {
        text.lines().filter_map(|l| l.split_once('=')).filter(|(k, _)| !SHELL_ADDED.contains(k)).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn group(rec: &Path, supervisor: i32) -> Vec<(String, i32)> {
        let file = rec.join(format!("{supervisor}.group"));
        assert!(within(Duration::from_secs(10), || file.exists()), "the fake supervisor wrote no group");
        fs::read_to_string(file).unwrap().split_whitespace().filter_map(|w| w.split_once('=')).map(|(k, v)| (k.to_string(), v.parse().unwrap())).collect()
    }

    fn secrets(cfg: &OmniCfg) -> (String, String) {
        let l = Layout::of(&cfg.instance);
        let config: Value = serde_json::from_slice(&fs::read(l.config()).unwrap()).unwrap();
        (config["apiKey"].as_str().unwrap().to_string(), fs::read_to_string(l.password()).unwrap())
    }

    struct FirstRun {
        before: BTreeMap<String, String>,
        dsh_started: bool,
    }

    fn first_run(f: &Fx) -> FirstRun {
        let before = snapshot(&f.user);
        let mut dsh_started = false;
        let omni = start_with_dsh(&f.cfg, &mut || dsh_started = true);
        let Omni::Running(o) = omni else { panic!("not running; app log: {}", fs::read_to_string(&f.app_log).unwrap_or_default()) };
        approve(&f.cfg, CODE).expect("approve");
        let kids: Vec<i32> = group(&f.rec, o.pid).into_iter().map(|(_, p)| p).collect();
        stop(&o);
        assert!(within(Duration::from_secs(10), || !alive(o.pid) && kids.iter().all(|&p| !alive(p))));
        let (key, password) = secrets(&f.cfg);
        let bench = json!({
            "key": key, "password": password, "api": f.cfg.ports.omni_api, "pg": f.cfg.ports.omni_db,
            "planted_before": before, "planted_hits": f.planted.load(Ordering::SeqCst),
        });
        fs::write(f.root.join("bench.json"), serde_json::to_string_pretty(&bench).unwrap()).unwrap();
        FirstRun { before, dsh_started }
    }

    #[test]
    fn off_runs_no_omni_and_on_puts_postgres_nats_and_the_api_in_the_supervisor_s_group_and_stop_leaves_none() {
        let _s = serial();
        let f = fx("group");

        assert!(records(&f.rec, "argv").is_empty());
        assert!(ports_free(&f.cfg.ports));
        let o = start(&f.cfg).expect("start");
        assert!(healthy(o.api, Duration::from_secs(20)), "the fake API never answered");
        assert_eq!((o.pgid, o.api), (o.pid, f.cfg.ports.omni_api), "the supervisor leads its own group");
        let kids = group(&f.rec, o.pid);
        assert_eq!(kids.iter().map(|(r, _)| r.as_str()).collect::<Vec<_>>(), ["pg", "nats", "api"]);
        for (role, pid) in &kids {
            assert_eq!(crate::nemo::proc_info(*pid as u32).map(|i| i.pgid), Some(o.pgid), "{role} is in the supervisor's group");
        }
        stop(&o);
        assert!(within(Duration::from_secs(10), || !alive(o.pid) && kids.iter().all(|(_, p)| !alive(*p))), "stop by group left a process");
        assert!(within(Duration::from_secs(5), || ports_free(&f.cfg.ports)), "a port is still held after stop");
    }

    #[test]
    fn the_fake_refuses_a_start_without_the_instance_s_home_or_a_seeded_sandbox_json() {
        let _s = serial();
        let f = fx("refuse");
        let l = Layout::of(&f.cfg.instance);
        let run = |args: &[&Path], home: &Path| {
            let mut c = Command::new(&f.cfg.bun);
            c.arg(f.cfg.pack.join("omni").join(CLI)).args(args).env_clear().env("PATH", "/usr/bin:/bin").env("HOME", home).stdout(Stdio::null()).stderr(Stdio::null());
            c.status().unwrap().code()
        };
        let p = |s: &'static str| Path::new(s);

        assert_eq!(run(&[p("sandbox"), p("run"), p("--embedded")], &l.home), Some(2));

        assert_eq!(run(&[p("sandbox"), p("run"), p("--embedded"), p("--home"), &f.user.join(".omni/sandbox")], &f.user), Some(2));

        fs::create_dir_all(&l.sandbox).unwrap();
        assert_eq!(run(&[p("sandbox"), p("run"), p("--embedded"), p("--home"), &l.sandbox], &l.home), Some(3));

        prepare(&f.cfg).unwrap();
        let mut state: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
        state.as_object_mut().unwrap().remove("bun");
        fs::write(l.state(), state.to_string()).unwrap();
        assert_eq!(run(&[p("sandbox"), p("run"), p("--embedded"), p("--home"), &l.sandbox], &l.home), Some(3));

        prepare(&f.cfg).unwrap();
        assert_eq!(run(&[p("sandbox"), p("run"), p("--home"), &l.sandbox], &l.home), Some(2));
        let refused = fs::read_to_string(f.rec.join("refused")).unwrap();
        assert_eq!(refused.lines().count(), 5, "{refused}");

        let o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        stop(&o);
        assert_eq!(fs::read_to_string(f.rec.join("refused")).unwrap(), refused);
    }

    #[test]
    fn a_planted_dot_omni_and_launch_agents_stay_byte_for_byte_and_its_api_url_gets_no_request() {
        let _s = serial();
        let f = fx("planted");
        let run = first_run(&f);
        assert_eq!(snapshot(&f.user), run.before, "the planted ~/.omni or ~/Library/LaunchAgents changed");
        assert_eq!(f.planted.load(Ordering::SeqCst), 0, "the planted ~/.omni apiUrl got a request");
        let owner = fs::read_to_string(f.rec.join("owner-requests")).unwrap();
        assert_eq!(owner.lines().collect::<Vec<_>>(), ["claim key-ok", "approve key-ok"], "the instance's API got the claim and the approval");
    }

    #[test]
    fn every_omni_cli_process_gets_exactly_the_cli_env_and_reaches_only_the_instance() {
        let _s = serial();
        let f = fx("env");
        let run = first_run(&f);
        assert!(run.dsh_started);
        let l = Layout::of(&f.cfg.instance);
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let want: BTreeMap<String, String> = [
            ("PATH", "/usr/bin:/bin".to_string()),
            ("LANG", "pt_BR.UTF-8".to_string()),
            ("TMPDIR", s(&std::env::temp_dir())),
            ("HOME", s(&l.home)),
            ("OMNI_CONFIG_DIR", s(&l.sandbox.join("config"))),
            ("OMNI_TELEMETRY", "false".to_string()),
            ("OMNI_SKIP_POSTINSTALL_PIN", "1".to_string()),
        ]
        .map(|(k, v)| (k.to_string(), v))
        .into();
        let envs = records(&f.rec, "env");
        assert_eq!(envs.len(), 3, "the supervisor, claim and approve");
        for (pid, text) in &envs {
            assert_eq!(env_of(text), want, "pid {pid}");
        }
        let cli = s(&f.cfg.pack.join("omni").join(CLI));
        let mut argvs: Vec<Vec<String>> = records(&f.rec, "argv").into_iter().map(|(_, a)| a.lines().map(String::from).collect()).collect();
        argvs.sort();
        let mut want = vec![vec![cli.clone(), "sandbox".into(), "run".into(), "--embedded".into(), "--home".into(), s(&l.sandbox)], vec![cli.clone(), "claim".into()], vec![cli, "approve".into(), CODE.into()]];
        want.sort();
        assert_eq!(argvs, want);
        let api = format!("http://127.0.0.1:{}", f.cfg.ports.omni_api);
        let targets: Vec<String> = records(&f.rec, "target").into_iter().map(|(_, t)| t).collect();
        assert_eq!(targets, [api.clone(), api], "claim and approve reach the instance's API only");
    }

    #[test]
    fn the_key_and_the_password_are_0600_files_and_in_no_log_argv_or_env() {
        let _s = serial();
        let f = fx("secrets");
        first_run(&f);
        let l = Layout::of(&f.cfg.instance);
        let (key, password) = secrets(&f.cfg);
        assert!(key.starts_with("omni_sk_") && key.len() == 40 && key[8..].bytes().all(|b| b.is_ascii_hexdigit()), "{}", key.len());
        assert!(password.len() >= 64 && password.bytes().all(|b| b.is_ascii_hexdigit()));
        for p in [l.config(), l.password(), l.state(), f.cfg.bots_home.join("omni.json")] {
            let m = fs::symlink_metadata(&p).unwrap();
            assert!(m.is_file() && m.mode() & 0o777 == 0o600, "{} is {:o}", p.display(), m.mode());
        }
        for d in [l.sandbox.clone(), l.home.clone(), l.sandbox.join("config"), l.sandbox.join("logs"), f.cfg.bots_home.clone()] {
            assert_eq!(fs::metadata(&d).unwrap().mode() & 0o777, 0o700, "{}", d.display());
        }
        let link: Value = serde_json::from_slice(&fs::read(f.cfg.bots_home.join("omni.json")).unwrap()).unwrap();
        assert_eq!(link, json!({"url": format!("http://127.0.0.1:{}", f.cfg.ports.omni_api), "key": key}));
        let mut seen = vec![fs::read_to_string(&f.app_log).unwrap()];
        for e in fs::read_dir(l.sandbox.join("logs")).unwrap().chain(fs::read_dir(&f.rec).unwrap()) {
            seen.push(fs::read_to_string(e.unwrap().path()).unwrap_or_default());
        }
        assert!(seen[0].contains("omni: supervisor pid"), "the app log was read");
        for text in &seen {
            assert!(!text.contains(&key) && !text.contains(password.trim()), "a secret reached a log, argv or env record");
        }

        let o = start(&f.cfg).unwrap();
        stop(&o);
        assert_eq!(secrets(&f.cfg), (key, password));
    }

    #[test]
    fn the_password_file_is_never_followed_through_a_link_and_a_bad_one_is_refused_not_replaced() {
        let _s = serial();
        let f = fx("pwlink");
        let l = Layout::of(&f.cfg.instance);
        prepare(&f.cfg).unwrap();
        let outside = f.root.join("outside");
        fs::write(&outside, "elsewhere\n").unwrap();
        fs::remove_file(l.password()).unwrap();
        std::os::unix::fs::symlink(&outside, l.password()).unwrap();
        assert!(prepare(&f.cfg).unwrap_err().contains("pg-password"));
        assert_eq!(fs::read_to_string(&outside).unwrap(), "elsewhere\n");
        assert!(fs::symlink_metadata(l.password()).unwrap().file_type().is_symlink(), "left as it is");

        fs::remove_file(l.password()).unwrap();
        fs::write(l.password(), "short").unwrap();
        fs::set_permissions(l.password(), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(prepare(&f.cfg).unwrap_err().contains("too short"));
        fs::write(l.password(), "x".repeat(64)).unwrap();
        fs::set_permissions(l.password(), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(prepare(&f.cfg).unwrap_err().contains("0600"));
        assert_eq!(fs::read_to_string(l.password()).unwrap(), "x".repeat(64));

        let g = fx("sblink");
        fs::create_dir_all(g.cfg.instance.join("omni")).unwrap();
        std::os::unix::fs::symlink(&g.user, g.cfg.instance.join("omni/sandbox")).unwrap();
        assert!(prepare(&g.cfg).unwrap_err().contains("is a link"));
    }

    #[test]
    fn the_fake_supervisor_refuses_a_password_file_that_is_a_link_not_0600_or_short() {
        let _s = serial();
        let f = fx("pwfake");
        let l = Layout::of(&f.cfg.instance);
        let run = |pw: &Path| {
            prepare(&f.cfg).unwrap();
            let mut state: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
            state["pgPasswordFile"] = json!(pw);
            fs::write(l.state(), state.to_string()).unwrap();
            let mut c = Command::new(&f.cfg.bun);
            c.arg(f.cfg.pack.join("omni").join(CLI)).args(["sandbox", "run", "--embedded", "--home"]).arg(&l.sandbox).env_clear().env("PATH", "/usr/bin:/bin").env("HOME", &l.home).stdout(Stdio::null()).stderr(Stdio::null());
            c.status().unwrap().code()
        };
        let good = f.root.join("good");
        fs::write(&good, "y".repeat(40)).unwrap();
        let link = f.root.join("link");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        fs::set_permissions(&good, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(run(&link), Some(4), "a link");
        let open = f.root.join("open");
        fs::write(&open, "y".repeat(40)).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(run(&open), Some(4), "0644");
        let short = f.root.join("short");
        fs::write(&short, "y".repeat(15)).unwrap();
        fs::set_permissions(&short, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(run(&short), Some(4), "15 characters");
        assert_eq!(fs::read_to_string(f.rec.join("refused")).unwrap().lines().count(), 3);
    }

    #[test]
    fn config_json_has_build_sandbox_config_s_fields_and_dials_127_0_0_1_without_the_password() {
        let _s = serial();
        let mut f = fx("config");

        let data = f.root.join("data");
        fs::create_dir_all(&data).unwrap();
        f.cfg.ports = crate::dsh_ports::ports_for(&data, "i").unwrap();
        let cfg = &f.cfg;
        let l = prepare(cfg).unwrap();
        let config: Value = serde_json::from_slice(&fs::read(l.config()).unwrap()).unwrap();
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();

        assert_eq!(keys(&config), ["apiKey", "apiUrl", "format", "server"]);
        assert_eq!(keys(&config["server"]), ["dataDir", "databaseUrl", "logLevel", "natsHost", "nodeEnv", "port"]);
        let p = cfg.ports;
        assert_eq!(config["apiUrl"], format!("http://127.0.0.1:{}", p.omni_api));
        assert_eq!(config["server"]["databaseUrl"], format!("postgresql://postgres@127.0.0.1:{}/omni", p.omni_db));
        assert_eq!(config["server"]["port"], p.omni_api);
        assert_eq!(config["server"]["natsHost"], "127.0.0.1");
        assert_eq!(config["server"]["dataDir"], json!(l.sandbox.join("data")));
        let (_, password) = secrets(cfg);
        assert!(!fs::read_to_string(l.config()).unwrap().contains(password.trim()));

        let state: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
        assert_eq!(keys(&state), ["atLogin", "autopg", "bun", "createdAt", "packageSpec", "pgPasswordFile", "ports", "schemaVersion", "updatedAt", "version"]);
        assert_eq!(state["ports"], json!({"api": p.omni_api, "pg": p.omni_db, "nats": p.omni_nats}));
        assert_eq!((state["schemaVersion"].clone(), state["atLogin"].clone()), (json!(1), json!(false)));
        assert_eq!(state["pgPasswordFile"], json!(l.password()));
        assert_eq!(state["bun"], json!(cfg.bun));
        assert!(!fs::read_to_string(l.state()).unwrap().contains(password.trim()));

        assert_eq!(fs::read_link(l.sandbox.join("runtime")).unwrap(), cfg.pack.join("omni/runtime"));
        assert_eq!(fs::read_link(l.sandbox.join("bin/nats-server")).unwrap(), cfg.pack.join("omni/bin/nats-server"));
        assert_eq!(fs::read_link(l.sandbox.join("pgserve/bin/darwin-arm64")).unwrap(), cfg.pack.join("omni/pgserve/bin/darwin-arm64"));
        assert_eq!(fs::read_link(l.sandbox.join("autopg").join(FAKE_AUTOPG)).unwrap(), cfg.pack.join("omni/autopg").join(FAKE_AUTOPG));
        prepare(cfg).unwrap();
        let again: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
        assert_eq!(again["createdAt"], state["createdAt"]);
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_761_609_600 + 3_661), "2025-10-28T01:01:01.000Z");
    }

    fn pg_sockets(instance: &Path, port: u16) -> Vec<PathBuf> {
        fn walk(d: &Path, depth: u32, hit: &dyn Fn(&str) -> bool, out: &mut Vec<PathBuf>) {
            let Ok(rd) = fs::read_dir(d) else { return };
            for e in rd.flatten() {
                let (p, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
                if hit(&name) {
                    out.push(p);
                } else if depth > 0 && fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir()) {
                    walk(&p, depth - 1, hit, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(instance, 12, &|n| n.starts_with(".s.PGSQL."), &mut out);
        let ours = format!(".s.PGSQL.{port}");
        walk(Path::new("/tmp"), 3, &|n| n.starts_with(&ours), &mut out);
        out
    }

    #[test]
    fn in_password_file_mode_postgres_has_no_unix_socket() {
        let _s = serial();

        let f = fx_in(&std::env::temp_dir(), &format!("oc-s-{}", std::process::id()));
        let port = f.cfg.ports.omni_db;
        let o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        assert_eq!(pg_sockets(&f.cfg.instance, port), Vec::<PathBuf>::new());
        assert!(!f.rec.join("pg-socket-mode").exists());
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok(), "Postgres listens on 127.0.0.1");
        stop(&o);

        let l = prepare(&f.cfg).unwrap();
        let mut state: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
        state.as_object_mut().unwrap().remove("pgPasswordFile");
        write_json(&l.state(), &state).unwrap();
        let socket = l.sandbox.join(format!("run/pg/.s.PGSQL.{port}"));
        assert!(socket.as_os_str().len() < 104, "{}", socket.display());
        let o = spawn(&f.cfg, &l).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        assert!(within(Duration::from_secs(5), || pg_sockets(&f.cfg.instance, port) == [socket.clone()]));
        stop(&o);
        if let Some(out) = out() {
            fs::write(out.join("socket-control.txt"), format!("password-file mode: none; pin mode: {}\n", socket.display())).unwrap();
        }
    }

    #[test]
    fn fixture_fix_omni_spawn_waits_for_start_ack() {
        let _s = serial();
        let f = fx("start-ack");
        let entered = f.rec.join("entered");
        let release = f.rec.join("release");
        let kids = Arc::new(Kids::with_start_ack(f.rec.clone()));
        let k = kids.clone();
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf '' > \"$1/entered\"; while [ ! -f \"$1/release\" ]; do /bin/sleep 0.02; done; printf '' > \"$1/$$.started\"; exec /bin/sleep 60", "fixture"]).arg(&f.rec);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || { let _ = tx.send(k.spawn_recorded("fixture", &mut cmd)); });
        let reached = within(Duration::from_secs(20), || entered.is_file());
        let early = rx.recv_timeout(Duration::from_millis(200));
        let waited = early.is_err();
        fs::write(release, "").unwrap();
        let result = match early { Ok(r) => r, Err(_) => rx.recv_timeout(Duration::from_secs(25)).expect("spawn acknowledgement did not complete") };
        worker.join().unwrap();
        let rec = result.expect("fixture spawn failed");
        kids.stop_recorded(&rec);
        assert!(reached, "fixture never reached its startup barrier");
        assert!(waited, "spawn returned before the fixture acknowledged startup");
    }

    #[test]
    fn fixture_fix_omni_ledger_is_invocation_local() {
        assert_ne!(pids_file().parent(), Some(std::env::temp_dir().as_path()), "Omni ledger is shared across invocations");
    }

    #[test]
    fn omni_not_healthy_within_the_limit_starts_dsh_with_the_notice_and_retry() {
        let _s = serial();
        let mut f = fx("unhealthy");
        f.cfg.healthy_within = Duration::from_millis(1500);
        f.cfg.children = Arc::new(Kids::with_start_ack(f.rec.clone()));
        fs::write(f.rec.join("unhealthy"), "").unwrap();
        let mut dsh_started = false;
        let omni = start_with_dsh(&f.cfg, &mut || dsh_started = true);
        assert!(dsh_started, "DSH starts anyway");
        assert!(omni.bots_wait(), "the Omni and Genie Bots wait");
        assert_eq!(omni.notice(false), Some(("Omni isn't running", "Retry")));
        assert_eq!(omni.notice(true), Some(("Omni não está em execução", "Tentar de novo")));
        let argv = records(&f.rec, "argv");
        assert!(!argv.is_empty(), "unhealthy fixture recorded no argv (records={}; startup_markers={}; unhealthy={})",
            argv.len(), records(&f.rec, "started").len(), f.rec.join("unhealthy").is_file());
        let sup = argv.first().expect("argv assertion above").0;
        assert!(within(Duration::from_secs(10), || !alive(sup)), "the unhealthy supervisor was stopped");
        assert!(within(Duration::from_secs(5), || ports_free(&f.cfg.ports)));
        assert!(fs::read_to_string(&f.app_log).unwrap().contains("omni: not healthy within 1 s; stopped"));

        fs::remove_file(f.rec.join("unhealthy")).unwrap();
        f.cfg.healthy_within = Duration::from_secs(20);
        let omni = retry(&f.cfg);
        let Omni::Running(o) = &omni else { panic!("Retry did not start Omni") };
        assert_eq!((omni.notice(false), omni.bots_wait()), (None, false));
        assert_eq!(fs::read_to_string(f.rec.join("owner-requests")).unwrap(), "claim key-ok\n");
        stop(o);
        if let Some(out) = out() {
            let n = json!({"dsh_started": dsh_started, "en": [NOTICE_EN, RETRY_EN], "pt": [NOTICE_PT, RETRY_PT], "retry_running": true});
            fs::write(out.join("notice.json"), n.to_string()).unwrap();
        }
    }

    #[test]
    fn crashes_restart_after_1_2_4_8_16_s_at_most_5_in_10_min() {
        let t0 = Instant::now();
        let mut r = Restarts::default();
        let got: Vec<Option<u64>> = (0..6).map(|i| r.after_crash(t0 + Duration::from_secs(i * 30)).map(|d| d.as_secs())).collect();
        assert_eq!(got, [Some(1), Some(2), Some(4), Some(8), Some(16), None]);

        assert_eq!(r.after_crash(t0 + Duration::from_secs(601)).map(|d| d.as_secs()), Some(16));
        let mut r = Restarts::default();
        for i in 0..5 {
            r.after_crash(t0 + Duration::from_secs(i));
        }
        assert_eq!(r.after_crash(t0 + Duration::from_secs(10)), None);
        r.reset();
        assert_eq!(r.after_crash(t0 + Duration::from_secs(11)), Some(Duration::from_secs(1)));
    }

    #[test]
    fn a_crashed_supervisor_s_group_is_stopped_and_its_ports_freed_before_it_is_started_again_with_the_backoff() {
        let _s = serial();
        let f = fx("crash");
        let mut o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        let mut r = Restarts::default();
        let mut waits = Vec::new();
        assert!(watch(&f.cfg, &o, &mut r, Instant::now(), &mut |d| waits.push(d)).unwrap().is_none(), "running: nothing to do");
        for _ in 0..5 {
            let left = group(&f.rec, o.pid);

            unsafe { libc::kill(o.pid, libc::SIGKILL) };
            assert!(within(Duration::from_secs(5), || !f.cfg.children.alive(&o.rec)));
            o = watch(&f.cfg, &o, &mut r, Instant::now(), &mut |d| waits.push(d)).unwrap().expect("started again");
            assert!(healthy(o.api, Duration::from_secs(20)), "the new supervisor's children got the ports");
            assert!(left.iter().all(|(_, p)| !alive(*p)), "the health answer came from the crashed supervisor's group, still up");
        }
        assert_eq!(waits.iter().map(|d| d.as_secs()).collect::<Vec<_>>(), [1, 2, 4, 8, 16]);
        let left = group(&f.rec, o.pid);
        unsafe { libc::kill(o.pid, libc::SIGKILL) };
        assert!(within(Duration::from_secs(5), || !f.cfg.children.alive(&o.rec)));
        let err = watch(&f.cfg, &o, &mut r, Instant::now(), &mut |d| waits.push(d)).err().expect("the sixth crash gives the notice");
        assert!(err.contains("6 times in 10 min"), "{err}");
        assert!(left.iter().all(|(_, p)| !alive(*p)), "Postgres, NATS or the API still run after the notice");
    }

    #[test]
    fn a_crash_past_the_limit_stops_what_the_supervisor_left_before_the_notice() {
        let _s = serial();
        let f = fx("limit");
        let t0 = Instant::now();
        let mut r = Restarts::default();
        for _ in 0..DELAYS.len() {
            r.after_crash(t0);
        }
        let o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        let left = group(&f.rec, o.pid);
        unsafe { libc::kill(o.pid, libc::SIGKILL) };
        assert!(within(Duration::from_secs(5), || !f.cfg.children.alive(&o.rec)));
        let err = watch(&f.cfg, &o, &mut r, t0, &mut |_| ()).err().expect("past the limit: the notice");
        assert!(err.contains("6 times in 10 min"), "{err}");
        assert!(left.iter().all(|(_, p)| !alive(*p)), "Postgres, NATS or the API still run after the notice");
        assert!(ports_free(&f.cfg.ports), "a port is still held after the notice");
    }

    #[test]
    fn a_test_that_panics_after_a_crash_leaves_no_fixture_process_alive_or_with_ppid_1() {
        let _s = serial();
        let made: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let f = fx("panic");
            *made.lock().unwrap() = Some((f.root.clone(), f.run.clone()));
            let o = start(&f.cfg).unwrap();
            assert!(healthy(o.api, Duration::from_secs(20)));
            group(&f.rec, o.pid);

            unsafe { libc::kill(o.pid, libc::SIGKILL) };
            assert!(within(Duration::from_secs(5), || !f.cfg.children.alive(&o.rec)));
            panic!("on purpose");
        }))
        .is_err();
        assert!(panicked);
        let (root, run) = made.into_inner().unwrap().expect("the fixture was made");
        let pids = recorded(&run);
        assert!(pids.iter().filter(|(_, _, w)| ["pg", "nats", "api"].contains(&w.as_str())).count() == 3, "the three children are recorded: {pids:?}");
        within(Duration::from_secs(5), || pids.iter().all(|(p, _, _)| !alive(*p)));
        let left: Vec<(i32, Option<i32>)> = pids.iter().filter(|(p, g, _)| alive(*p) && crate::nemo::proc_info(*p as u32).is_some_and(|i| i.pgid == *g)).map(|(p, _, _)| (*p, ppid(*p))).collect();
        let unrecorded: Vec<(i32, String)> = orphans_naming(&root).into_iter().filter(|(p, _)| !pids.iter().any(|(q, _, _)| q == p)).collect();

        for (p, _) in &left {
            unsafe { libc::kill(*p, libc::SIGKILL) };
        }
        assert!(left.is_empty(), "fixture processes alive after the panic (pid, ppid): {left:?}");
        assert!(unrecorded.is_empty(), "unrecorded processes naming the fixture with ppid 1: {unrecorded:?}");
    }

    #[test]
    fn omni_s_leftover_sweep_never_matches_a_process_that_names_another_home() {
        let _s = serial();
        let f = fx("sweep");
        let l = prepare(&f.cfg).unwrap();
        let mut held = Held(Vec::new());
        let sleeper = |home: &Path| {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "/bin/sleep 60; :"]).arg(home.join("run/x"));
            c
        };
        let other = held.spawn(&mut sleeper(&f.user.join(".omni/sandbox")), &f.run);
        let ours = held.spawn(&mut sleeper(&l.sandbox), &f.run);
        fs::write(l.sandbox.join("run/fake-children"), format!("{other}\n{ours}\n")).unwrap();
        let o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)));
        assert!(within(Duration::from_secs(5), || matches!(held.0[1].try_wait(), Ok(Some(_)))), "a leftover naming this home is stopped");
        assert!(matches!(held.0[0].try_wait(), Ok(None)), "a process naming another home is never matched");
        stop(&o);
    }

    #[test]
    fn postgres_is_the_packs_autopg_from_its_binary_cache_at_the_version_its_manifest_names() {
        let _s = serial();
        let f = fx("autopg");
        let (omni, sandbox) = (f.cfg.pack.join("omni"), f.cfg.instance.join("omni/sandbox"));

        fs::create_dir_all(sandbox.join("pgserve")).unwrap();
        std::os::unix::fs::symlink(omni.join("pgserve/bin"), sandbox.join("pgserve/bin")).unwrap();
        let l = prepare(&f.cfg).unwrap();
        let state: Value = serde_json::from_slice(&fs::read(l.state()).unwrap()).unwrap();
        assert_eq!(state["autopg"], FAKE_AUTOPG, "the manifest's version");
        assert_eq!(fs::read_link(sandbox.join("autopg").join(FAKE_AUTOPG)).unwrap(), omni.join("autopg").join(FAKE_AUTOPG));
        assert_eq!(fs::read_link(sandbox.join("pgserve/bin/darwin-arm64")).unwrap(), omni.join("pgserve/bin/darwin-arm64"));
        for d in ["autopg", "pgserve", "pgserve/bin"] {
            let m = fs::symlink_metadata(sandbox.join(d)).unwrap();
            assert!(m.is_dir() && m.mode() & 0o777 == 0o700, "{d}: the instance's own 0700 dir");
        }
        let o = start(&f.cfg).unwrap();
        assert!(healthy(o.api, Duration::from_secs(20)), "{:?}", records(&f.rec, "refused"));
        stop(&o);
        assert!(!f.rec.join("npm-pgserve").exists(), "npm pgserve never ran");

        let before = fs::read(l.state()).unwrap();
        let manifest = omni.join("autopg").join(FAKE_AUTOPG).join("manifest.json");
        fs::write(&manifest, r#"{"name":"autopg","version":"3.2.0","files":{}}"#).unwrap();
        assert!(prepare(&f.cfg).is_err_and(|e| e.contains("its manifest names")));
        fs::write(&manifest, format!(r#"{{"name":"autopg","version":"{FAKE_AUTOPG}","files":{{}}}}"#)).unwrap();
        fs::create_dir_all(omni.join("autopg/3.4.0")).unwrap();
        assert!(prepare(&f.cfg).is_err_and(|e| e.contains("2 releases")));
        assert_eq!(fs::read(l.state()).unwrap(), before);
    }

    #[test]
    fn postgres_data_made_without_the_password_file_is_refused_and_left_as_it_is() {
        let f = fx("legacy");
        let sandbox = f.cfg.instance.join("omni/sandbox");
        let data = sandbox.join("data/pg");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("PG_VERSION"), "18\n").unwrap();
        let hba = "# TYPE  DATABASE  USER  ADDRESS  METHOD\nlocal all all trust\nhost all all 127.0.0.1/32 trust\n";
        fs::write(data.join("pg_hba.conf"), hba).unwrap();

        assert!(prepare(&f.cfg).is_err_and(|e| e.contains("without a password file")));
        assert_eq!(fs::read_to_string(data.join("pg_hba.conf")).unwrap(), hba, "pg_hba.conf as it was");
        for p in ["sandbox.json", "config/config.json", "config/pg-password"] {
            assert!(!sandbox.join(p).exists(), "{p} not written");
        }

        let legacy = format!("{{\"schemaVersion\":1,\"version\":\"2.261025.4\",\"createdAt\":\"{}\"}}\n", now_iso());
        write_private(&sandbox.join("sandbox.json"), legacy.as_bytes()).unwrap();
        assert!(prepare(&f.cfg).is_err_and(|e| e.contains("without a password file")));
        assert_eq!((fs::read_to_string(sandbox.join("sandbox.json")).unwrap(), fs::read_to_string(data.join("pg_hba.conf")).unwrap()), (legacy, hba.to_string()));

        fs::remove_file(sandbox.join("sandbox.json")).unwrap();
        fs::write(data.join("pg_hba.conf"), format!("{PG_HBA_HEADER}\nhost all all 127.0.0.1/32 scram-sha-256\n")).unwrap();
        assert!(prepare(&f.cfg).is_ok());
    }

    #[test]
    fn the_legacy_refusal_changes_no_dir_mode_and_keeps_an_older_build_s_link() {
        let f = fx("legacy-untouched");
        let sandbox = f.cfg.instance.join("omni/sandbox");
        let data = sandbox.join("data/pg");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("PG_VERSION"), "18\n").unwrap();
        fs::write(data.join("pg_hba.conf"), "host all all 127.0.0.1/32 trust\n").unwrap();

        for d in [f.cfg.instance.join("omni"), sandbox.clone(), sandbox.join("data"), data.clone()] {
            fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::create_dir_all(sandbox.join("pgserve")).unwrap();
        std::os::unix::fs::symlink(f.cfg.pack.join("omni/pgserve/bin"), sandbox.join("pgserve/bin")).unwrap();
        let before = tree_sums(&f.cfg.instance);
        assert!(prepare(&f.cfg).is_err_and(|e| e.contains("without a password file")));
        let after = tree_sums(&f.cfg.instance);
        let changed: Vec<_> = before.keys().chain(after.keys()).filter(|k| before.get(*k) != after.get(*k)).collect();
        assert!(changed.is_empty(), "the instance as it was: {changed:?}");
        assert!(fs::symlink_metadata(sandbox.join("pgserve/bin")).unwrap().file_type().is_symlink(), "the old link kept");
    }

    #[test]
    fn two_instances_get_two_keys_and_two_postgres_passwords() {
        let (a, b) = (fx("keys-a"), fx("keys-b"));
        let (la, lb) = (prepare(&a.cfg).unwrap(), prepare(&b.cfg).unwrap());
        let key = |l: &Layout| key_of(&fs::read_to_string(l.config()).unwrap()).unwrap();
        let pw = |l: &Layout| fs::read_to_string(l.password()).unwrap();
        let (ka, kb) = (key(&la), key(&lb));
        assert!(ka.starts_with("omni_sk_") && kb.starts_with("omni_sk_") && ka != kb, "two keys");
        assert!(pw(&la).len() >= 32 && pw(&la) != pw(&lb), "two passwords");

        assert_eq!((key(&prepare(&a.cfg).unwrap()), pw(&la)), (ka, pw(&prepare(&a.cfg).unwrap())));
    }

    fn pg_auth_code(port: u16) -> Option<i32> {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let body = b"user\0postgres\0database\0omni\0\0";
        let mut msg = ((8 + body.len()) as i32).to_be_bytes().to_vec();
        msg.extend(196_608i32.to_be_bytes());
        msg.extend(body);
        s.write_all(&msg).ok()?;
        let mut head = [0u8; 9];
        s.read_exact(&mut head).ok()?;
        if head[0] != b'R' {
            return Some(-(head[0] as i32));
        }
        Some(i32::from_be_bytes(head[5..9].try_into().ok()?))
    }

    fn exe_of(pid: i32) -> Option<PathBuf> {
        let mut buf = vec![0u8; 4096];
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned()))
    }

    fn tree_sums(dir: &Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut todo = vec![dir.to_path_buf()];
        while let Some(d) = todo.pop() {
            for e in fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                let m = fs::symlink_metadata(&p).unwrap();
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                let v = if m.file_type().is_symlink() {
                    format!("link {}", fs::read_link(&p).unwrap().display())
                } else if m.is_dir() {
                    todo.push(p);
                    format!("dir {:o}", m.mode())
                } else {
                    format!("{:o} {:x}", m.mode(), Sha256::digest(fs::read(&p).unwrap()))
                };
                out.insert(rel, v);
            }
        }
        out
    }

    fn shipped(modes: &str, sums: &str) -> BTreeMap<String, String> {
        let rel = |p: &str| p.strip_prefix("omni/").map(str::to_string);
        let sha: BTreeMap<String, String> = sums.lines().filter_map(|l| l.split_once("  ")).filter_map(|(h, p)| Some((rel(p)?, h.to_string()))).collect();
        modes.lines().filter_map(|l| l.split_once(' ')).filter_map(|(m, p)| Some((rel(p)?, m.to_string()))).map(|(p, m)| {
            let v = match sha.get(&p) { Some(h) => format!("{m} {h}"), None => m };
            (p, v)
        }).collect()
    }

    fn as_shipped(t: &BTreeMap<String, String>, skip: &str) -> BTreeMap<String, String> {
        t.iter().filter(|(k, v)| !v.starts_with("link ") && !k.starts_with(skip)).map(|(k, v)| {
            let v = match v.strip_prefix("dir ") { Some(m) => m[m.len() - 3..].to_string(), None => { let (m, h) = v.split_once(' ').unwrap(); format!("{} {h}", &m[m.len() - 3..]) } };
            (k.clone(), v)
        }).collect()
    }

    #[test]
    #[ignore = "the real Omni, autopg and Postgres: UNFOLD_REAL_OMNI=<a pack's omni/>, run where it can't exec outside its temp dir"]
    fn on_the_real_layout_postgres_is_autopg_from_its_cache_in_password_file_mode() {
        let Some(src) = std::env::var("UNFOLD_REAL_OMNI").ok().filter(|s| !s.is_empty()) else { return };
        let root = std::env::temp_dir().join(format!("oc-real-{}", std::process::id()));
        let _ = Command::new("/bin/chmod").args(["-R", "u+w"]).arg(&root).status();
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("pack")).unwrap();
        let omni = root.join("pack/omni");
        assert!(Command::new("/bin/cp").arg("-cRp").arg(&src).arg(&omni).status().unwrap().success(), "clone {src}");
        let read = |k: &str| fs::read_to_string(std::env::var(k).unwrap_or_else(|_| panic!("{k}"))).unwrap();
        let shipped_all = shipped(&read("UNFOLD_REAL_OMNI_MODES"), &read("UNFOLD_REAL_OMNI_SUMS"));

        let modules = omni.join("runtime/node_modules");
        let marker = root.join("npm-pgserve-ran");
        fs::create_dir_all(modules.join("pgserve/bin")).unwrap();
        fs::write(modules.join("pgserve/bin/postgres-server.js"), format!("require('node:fs').writeFileSync({:?}, 'ran\\n')\nprocess.exit(1)\n", marker.display().to_string())).unwrap();
        let poisoned = "runtime/node_modules/pgserve";
        let before = tree_sums(&omni);
        let shipped: BTreeMap<String, String> = shipped_all.into_iter().filter(|(k, _)| !before.get(k).is_some_and(|v| v.starts_with("link "))).collect();
        let clone_is_shipped = as_shipped(&before, poisoned) == shipped;

        let pack = root.join("pack");
        crate::dsh_pack::write_manifest(&pack).unwrap();
        let version: Value = serde_json::from_slice(&fs::read(omni.join("runtime/node_modules/@automagik/omni/package.json")).unwrap()).unwrap();
        let instance = root.join("i");
        fs::create_dir_all(&instance).unwrap();
        let port = || crate::nemo::free_port().unwrap();
        let ports = Ports { web: 0, bridge: 0, omni_api: port(), omni_db: port(), omni_nats: port() };
        let cfg = OmniCfg {
            bots_home: instance.join("dsh/automagik/bots"),
            instance: instance.clone(),
            pack: root.join("pack"),
            bun: omni.join("bin/bun"),
            version: version["version"].as_str().unwrap().to_string(),
            ports,
            env: vec![("PATH".into(), "/usr/bin:/bin".into()), ("LANG".into(), "en_US.UTF-8".into()), ("TMPDIR".into(), std::env::temp_dir().display().to_string())],
            healthy_within: Duration::from_secs(120),
            children: Arc::new(Kids::default()),
            log: Log::open_as(&crate::WriteMode::Dev, &root.join("app")),
        };
        let l = prepare(&cfg).unwrap();
        let t = Instant::now();
        let o = start(&cfg).unwrap();

        let end = Instant::now() + cfg.healthy_within;
        let up = loop {
            if marker.exists() || Instant::now() >= end {
                break false;
            }
            if healthy(o.api, Duration::from_secs(1)) {
                break true;
            }
        };
        let took = t.elapsed();
        let data = l.sandbox.join("data/pg");
        let pid_file = fs::read_to_string(data.join("postmaster.pid")).unwrap_or_default();
        let lines: Vec<&str> = pid_file.lines().collect();
        let postmaster: i32 = lines.first().and_then(|p| p.trim().parse().ok()).unwrap_or(0);
        let exe = exe_of(postmaster);
        let auth = pg_auth_code(ports.omni_db);
        let hba = fs::read_to_string(data.join("pg_hba.conf")).unwrap_or_default();
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        eprintln!("real layout: omni {} healthy {up} after {:.1} s; supervisor pid {} group {}; postmaster {postmaster} {:?}; auth {auth:?}; pid file {:?}", cfg.version, took.as_secs_f64(), o.pid, o.pgid, exe, lines);

        let mut sv = command(&cfg, &l, "stop.log").unwrap();
        let stopped = sv.args(["sandbox", "stop", "--home"]).arg(&l.sandbox).status().map(|s| s.code());
        stop(&o);
        let gone = within(Duration::from_secs(30), || !data.join("postmaster.pid").exists() && TcpListener::bind((Ipv4Addr::LOCALHOST, ports.omni_db)).is_ok());
        let after = tree_sums(&omni);
        let _ = Command::new("/bin/chmod").args(["-R", "u+w"]).arg(&root).status();
        let left = (marker.exists(), [".omni", ".autopg", ".pgserve"].iter().filter(|d| home.join(d).exists()).collect::<Vec<_>>());
        eprintln!("real layout, after: npm pgserve ran {}, in the HOME {:?}, stopped {gone} (Omni's stop {stopped:?}), pack unchanged {}", left.0, left.1, after == before);
        for (k, v) in before.iter().filter(|(k, v)| after.get(*k) != Some(*v)).take(20) {
            eprintln!("real layout, changed: {k}: {v} -> {:?}", after.get(k));
        }
        for k in after.keys().filter(|k| !before.contains_key(*k)).take(20) {
            eprintln!("real layout, new: {k}: {}", after[k]);
        }
        let cache = fs::canonicalize(omni.join("pgserve/bin/darwin-arm64/bin/postgres")).unwrap();

        let next_start = crate::dsh_pack::check_manifest(&pack, &[]);
        let mut b = fs::read(&cache).unwrap();
        *b.last_mut().unwrap() ^= 1;
        fs::write(&cache, &b).unwrap();
        let changed_start = crate::dsh_pack::check_manifest(&pack, &[]);
        if gone && after == before {
            let _ = fs::remove_dir_all(&root);
        }
        assert_eq!(left, (false, Vec::<&&str>::new()), "npm pgserve never ran; no ~/.omni, ~/.autopg or ~/.pgserve in the HOME");
        assert_eq!(exe, Some(cache), "Postgres is the binary in autopg's cache");
        assert!(up, "healthy within 120 s: {}", fs::read_to_string(l.sandbox.join("logs/pg.log")).unwrap_or_default());
        assert!(hba.starts_with(&format!("{PG_HBA_HEADER}\n")), "initdb in password-file mode: {hba}");
        assert!(matches!(auth, Some(c) if c != 0), "a login without the password is not let in: {auth:?}");
        assert_eq!((lines.get(3).copied(), lines.get(5).copied()), (Some(ports.omni_db.to_string().as_str()), Some("127.0.0.1")), "the instance's port, 127.0.0.1 only");

        assert!(clone_is_shipped, "the clone is the shipped layout: {} entries vs {}", as_shipped(&before, poisoned).len(), shipped.len());

        let after_shipped = as_shipped(&after, poisoned);
        for b in ["initdb", "postgres"] {
            let k = format!("pgserve/bin/darwin-arm64/bin/{b}");
            assert!(shipped[&k].starts_with("755 ") && after_shipped.get(&k) == Some(&shipped[&k]), "{k}: {} -> {:?}", shipped[&k], after_shipped.get(&k));
        }

        assert!(after == before, "the pack is as it was");

        assert_eq!(next_start, Ok(()), "the next start's manifest check passes");
        assert_eq!(changed_start, Err(vec![PathBuf::from("omni/pgserve/bin/darwin-arm64/bin/postgres")]), "a byte-changed postgres is refused at the next start");
        assert!(gone, "stopped: Omni's stop {stopped:?}, then the recorded group");
    }

    #[test]
    fn approve_takes_only_a_device_code() {
        let f = fx("code");
        for bad in ["", "-x", "--home", "AB CD", "ABCD-", "ABCD-EFGH-IJKL-MNOP", "a;b"] {
            assert!(approve(&f.cfg, bad).is_err(), "{bad:?}");
        }
        assert!(records(&f.rec, "argv").is_empty(), "nothing ran");
    }

    #[test]
    #[ignore = "the fake Omni: run only as a child of these tests"]
    fn fake_omni() {
        let Ok(rec) = std::env::var(REC) else { return };
        let rec = PathBuf::from(rec);

        fs::write(rec.join(format!("{}.started", std::process::id())), "").unwrap();
        let code = match std::env::var(ROLE) {
            Ok(role) => fake::child(&rec, &role),
            Err(_) => {
                let argv: Vec<String> = fs::read_to_string(rec.join(format!("{}.argv", std::process::id()))).unwrap_or_default().lines().map(String::from).collect();
                fake::omni(&rec, &argv)
            }
        };
        std::process::exit(code);
    }

    mod fake {
        use super::*;
        use std::io::BufRead;
        use std::os::unix::net::UnixListener;

        fn note(rec: &Path, file: &str, line: &str) {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(rec.join(file)) {
                let _ = writeln!(f, "{line}");
            }
        }

        pub fn omni(rec: &Path, argv: &[String]) -> i32 {
            if !argv.first().is_some_and(|c| c.ends_with(CLI) && Path::new(c).is_file()) {
                return 66;
            }
            let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
            match args.as_slice() {
                ["sandbox", "run", flags @ ..] => supervisor(rec, flags),
                ["claim"] => cli(rec, "claim", json!({})),
                ["approve", code] => cli(rec, "approve", json!({"userCode": code})),
                _ => 64,
            }
        }

        fn state_ok(v: &Value) -> bool {
            let port = |k: &str| v["ports"][k].as_u64().is_some_and(|p| (1024..=65535).contains(&p));
            let text = |k: &str| v[k].as_str().is_some_and(|s| !s.is_empty());
            v["schemaVersion"] == 1 && text("version") && text("packageSpec") && v["bun"].as_str().is_some_and(|b| b.starts_with('/')) && port("api") && port("pg") && port("nats") && v["atLogin"] == false && text("createdAt") && text("updatedAt")
        }

        fn password_file_ok(p: &Path) -> Result<(), String> {
            let m = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
            if m.file_type().is_symlink() || !m.is_file() {
                return Err("the password file is not a regular file".into());
            }
            if m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o600 {
                return Err("the password file is not this user's 0600 file".into());
            }
            if fs::read_to_string(p).map_err(|e| e.to_string())?.trim().len() < 16 {
                return Err("the password file is under 16 characters".into());
            }
            Ok(())
        }

        fn supervisor(rec: &Path, flags: &[&str]) -> i32 {
            let refuse = |code: i32, why: &str| {
                note(rec, "refused", why);
                code
            };
            let Some(home) = flags.windows(2).find(|w| w[0] == "--home").map(|w| PathBuf::from(w[1])) else {
                return refuse(2, "no --home: Omni would run ~/.omni/sandbox");
            };
            if !flags.contains(&"--embedded") {
                return refuse(2, "no --embedded");
            }
            let env_home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            if !home.ends_with("sandbox") || !env_home.ends_with("home") || home.parent() != env_home.parent() {
                return refuse(2, "--home is not the instance's sandbox");
            }
            let state: Option<Value> = fs::read(home.join("sandbox.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            let Some(state) = state.filter(state_ok) else {
                return refuse(3, "No sandboxed Omni there (no sandbox.json in the pin's schema)");
            };
            let config: Option<Value> = fs::read(home.join("config/config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
            if !config.is_some_and(|c| c["apiKey"].is_string()) {
                return refuse(3, "No sandboxed Omni there (no API key)");
            }
            let password_file = state["pgPasswordFile"].as_str().map(PathBuf::from);
            if let Some(p) = &password_file {
                if let Err(why) = password_file_ok(p) {
                    return refuse(4, &why);
                }
            }

            let Some(v) = state["autopg"].as_str() else {
                note(rec, "npm-pgserve", "");
                return refuse(5, "no autopg: Omni would run npm pgserve's postgres-server.js");
            };
            let seed = home.join("pgserve/bin/darwin-arm64");
            let ready = v == FAKE_AUTOPG && home.join("autopg").join(v).join("autopg").is_file() && fs::read_to_string(seed.join(".version")).is_ok_and(|t| t.trim() == FAKE_PG_RUNTIME) && seed.join("bin/initdb").is_file() && seed.join("bin/postgres").is_file();
            if !ready {
                return refuse(5, "autopg's release or binary cache is not in place");
            }
            sweep(rec, &home);
            let exe = std::env::current_exe().unwrap();
            let mut pids = Vec::new();
            for role in ["pg", "nats", "api"] {
                let port = state["ports"][role].as_u64().unwrap();
                let log = OpenOptions::new().create(true).append(true).open(home.join(format!("logs/{role}.log"))).unwrap();
                let mut c = Command::new(&exe);

                c.args(["omni_child::tests::fake_omni", "--exact", "--ignored", "--nocapture", "--test-threads=1", "-q"])
                    .arg(&home)
                    .env(ROLE, role)
                    .env("UNFOLD_FAKE_OMNI_PORT", port.to_string())
                    .env("UNFOLD_FAKE_OMNI_HOME", &home)
                    .stdout(log.try_clone().unwrap())
                    .stderr(log);
                if let Some(p) = &password_file {
                    c.env("UNFOLD_FAKE_OMNI_PASSWORD_FILE", p);
                }
                let pid = c.spawn().unwrap().id();
                if let (Ok(file), Ok(run)) = (std::env::var(PIDS), std::env::var(RUN)) {
                    record_pid(Path::new(&file), pid as i32, unsafe { libc::getpgrp() }, role, &run);
                }
                pids.push((role, pid));
            }
            let _ = fs::write(home.join("run/fake-children"), pids.iter().map(|(_, p)| format!("{p}\n")).collect::<String>());
            let _ = fs::write(home.join("run/supervisor.pid"), std::process::id().to_string());
            let _ = fs::write(rec.join(format!("{}.group", std::process::id())), pids.iter().map(|(r, p)| format!("{r}={p} ")).collect::<String>());
            loop {
                std::thread::park();
            }
        }

        fn sweep(rec: &Path, home: &Path) {
            let Ok(text) = fs::read_to_string(home.join("run/fake-children")) else { return };
            for pid in text.lines().filter_map(|l| l.trim().parse::<i32>().ok()) {
                let args = Command::new("/bin/ps").args(["-o", "args=", "-p", &pid.to_string()]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
                if args.contains(&*home.to_string_lossy()) {
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    within(Duration::from_secs(5), || !alive(pid));
                    note(rec, "swept", &format!("{pid} stopped"));
                } else {
                    note(rec, "swept", &format!("{pid} not matched"));
                }
            }
        }

        pub fn child(rec: &Path, role: &str) -> i32 {
            let port: u16 = std::env::var("UNFOLD_FAKE_OMNI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
            let home = PathBuf::from(std::env::var("UNFOLD_FAKE_OMNI_HOME").unwrap_or_default());
            let Ok(l) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)) else { return 70 };
            let mut _socket = None;
            if role == "pg" && std::env::var_os("UNFOLD_FAKE_OMNI_PASSWORD_FILE").is_none() {

                note(rec, "pg-socket-mode", "");
                let dir = home.join("run/pg");
                let _ = fs::create_dir_all(&dir);
                _socket = UnixListener::bind(dir.join(format!(".s.PGSQL.{port}"))).ok();
            }
            for s in l.incoming().flatten() {
                if role == "api" {
                    let (rec, home) = (rec.to_path_buf(), home.clone());
                    std::thread::spawn(move || api(&rec, &home, s));
                }
            }
            0
        }

        fn api(rec: &Path, home: &Path, s: TcpStream) {
            let mut r = std::io::BufReader::new(&s);
            let mut first = String::new();
            let _ = r.read_line(&mut first);
            let mut key = String::new();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap_or_default();
                match k.trim().to_ascii_lowercase().as_str() {
                    "x-api-key" => key = v.trim().to_string(),
                    "content-length" => len = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            let mut body = vec![0u8; len];
            let _ = r.read_exact(&mut body);
            let path = first.split_whitespace().nth(1).unwrap_or_default();
            let want = fs::read(home.join("config/config.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|c| c["apiKey"].as_str().map(String::from));
            let code = match path {
                "/api/v2/health" if rec.join("unhealthy").exists() => 503,
                "/api/v2/health" => 200,
                p if p.starts_with("/api/v2/owner/") => {
                    let ok = want.as_deref() == Some(key.as_str());
                    note(rec, "owner-requests", &format!("{} {}", &p[14..], if ok { "key-ok" } else { "key-bad" }));
                    if ok {
                        200
                    } else {
                        401
                    }
                }
                _ => 404,
            };
            let _ = write!(&s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}");
        }

        fn cli(rec: &Path, path: &str, body: Value) -> i32 {
            let dir = std::env::var("OMNI_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".omni"));
            let config: Value = fs::read(dir.join("config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
            let url = config["apiUrl"].as_str().unwrap_or("http://localhost:8882").to_string();
            let key = config["apiKey"].as_str().unwrap_or_default().to_string();
            let _ = fs::write(rec.join(format!("{}.target", std::process::id())), &url);
            let Some(port) = url.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) else { return 1 };
            let Ok(mut s) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else { return 1 };
            let body = body.to_string();
            let _ = write!(s, "POST /api/v2/owner/{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nx-api-key: {key}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let mut reply = String::new();
            let _ = s.read_to_string(&mut reply);
            if reply.split_whitespace().nth(1) == Some("200") {
                0
            } else {
                1
            }
        }
    }
}
