

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::dsh_ports::Ports;
use crate::{Log, Shared};

pub struct ChildCfg {

    pub pack: PathBuf,

    pub node: PathBuf,

    pub instance_root: PathBuf,
    pub instance_id: String,
    pub ports: Ports,

    pub home: PathBuf,

    pub base_env: Vec<(String, String)>,

    pub sockdir: fn(&str) -> Result<PathBuf, String>,

    pub serve: fn(&Path, &Log) -> Result<Vec<(String, String)>, String>,

    pub ready_within: Duration,

    pub deepseek_key: Option<CarriedKey>,
}

#[derive(Clone, PartialEq)]
pub struct CarriedKey(pub String);

impl std::fmt::Debug for CarriedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CarriedKey(<withheld>)")
    }
}

impl CarriedKey {

    fn fingerprint(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(self.0.as_bytes()).into()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OwnDsh {
    pub pid: u32,

    pub start_us: u64,

    pub port: u16,

    pub web: u16,
    pub endpoint: PathBuf,
}

pub fn dsh_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("dsh")
}

#[derive(Debug, PartialEq)]
pub enum Row {

    Set,

    Unset(&'static str),

    Shared(&'static str),

    Carried(&'static str),
}

pub const ROWS: &[(&str, Row)] = &[
    ("HOME", Row::Shared("the person's home: DSH's agents work with their files, like any DSH")),
    ("DSH_HOME", Row::Set),
    ("AUTOMAGIK_HOME", Row::Set),
    ("AUTOMAGIK_BOTS_HOME", Row::Set),
    ("AUTOMAGIK_HOST", Row::Set),
    ("AUTOMAGIK_PRELOAD", Row::Set),
    ("AUTOMAGIK_PRIVACY_FILE", Row::Unset("the privacy overlay is the only upload control")),
    ("AUTOMAGIK_BUSY_FILE", Row::Set),
    ("DSH_OMC_STATE_DIR", Row::Set),
    ("OMNI_HOME", Row::Set),
    ("OMNI_CONFIG_DIR", Row::Set),
    ("OMNI_TELEMETRY", Row::Set),
    ("OMNI_SKIP_POSTINSTALL_PIN", Row::Set),
    ("UNFOLD_BRIDGE_STATE_DIR", Row::Set),
    ("UNFOLD_BRIDGE_PORT", Row::Set),
    ("UNFOLD_AGENTD_OBSERVER", Row::Set),
    ("UNFOLD_CONTROL_SOCKET", Row::Set),
    ("UNFOLD_CONTROL_TOKEN_FILE", Row::Set),
    ("UNFOLD_HOST_JOURNAL", Row::Unset("reserved: set once the app writes the host journal")),
    ("BUN_INSTALL", Row::Set),
    ("DSH_AGENTS_HOME", Row::Set),
    ("GENIE_HOME", Row::Set),
    ("GENIE_WISHES_DIR", Row::Set),
    ("NODE_OPTIONS", Row::Unset("Node's flags come from the launcher alone")),
    ("PATH", Row::Set),
    ("TMPDIR", Row::Set),
    ("DSH_TELEMETRY_DISABLED", Row::Set),
    ("DO_NOT_TRACK", Row::Set),
    ("DISABLE_TELEMETRY", Row::Set),
    ("OPENUI_TELEMETRY_DISABLED", Row::Set),
    ("npm_config_update_notifier", Row::Set),
    ("DISABLE_ERROR_REPORTING", Row::Set),
    ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", Row::Set),
    ("XDG_STATE_HOME", Row::Unset("it would point the bridge at a shared state path")),
    ("OMNI_CLIENT_ID", Row::Unset("the desktop app's sign-in client")),
    ("DS_BOT_APP", Row::Unset("desktop-only behaviour")),
    ("ELECTRON_RUN_AS_NODE", Row::Unset("DSH runs on Node here, not inside Electron")),
    ("DEEPSEEK_API_KEY", Row::Carried("Unfold's DeepSeek key, when its provider is DeepSeek's own API")),
];

pub const DSHENV_AT_PIN: &[&str] = &[
    "PATH",
    "DSH_HOME",
    "AUTOMAGIK_BOTS_HOME",
    "OMNI_CLIENT_ID",
    "DO_NOT_TRACK",
    "DISABLE_TELEMETRY",
    "OPENUI_TELEMETRY_DISABLED",
    "npm_config_update_notifier",
    "DSH_TELEMETRY_DISABLED",
    "DISABLE_ERROR_REPORTING",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "AUTOMAGIK_PRIVACY_FILE",
    "AUTOMAGIK_BUSY_FILE",
    "AUTOMAGIK_PRELOAD",
    "ELECTRON_RUN_AS_NODE",
];

fn tool_dirs(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".local/bin"), home.join(".claude/local"), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]
}

pub const CLOSED: [&str; 3] = ["genie", "omni", "dsh"];

const CLOSED_DIR: &str = "closed-bin";

fn closed_command(pack: &Path, name: &str) -> String {
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
    let runs = |p: &Path| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    let closed = |what: &str| format!("echo \"{what}: not available to Unfold's own DSH\" >&2\nexit 127\n");
    let run = |runtime: PathBuf, script: PathBuf| (runs(&runtime) && script.is_file()).then(|| format!("exec {} {} \"$@\"\n", q(&runtime), q(&script)));
    let own = match name {
        "genie" => Some(pack.join("genie/bin/genie")).filter(|g| runs(g)).map(|g| format!("exec {} \"$@\"\n", q(&g))),
        "omni" => run(pack.join("omni/bin/bun"), pack.join("omni").join(crate::omni_child::CLI)),

        "dsh" => run(pack.join("bin/node"), pack.join(DSH_BIN)).map(|r| {
            format!("if [ \"$1\" = plugin ]; then\n{}fi\ncase \"$#:$1\" in\n1:-V|1:--version|1:-h|1:--help) {r};;\nesac\necho \"dsh: Unfold's own DSH is already running; it starts from Unfold.\" >&2\nexit 127\n", closed("dsh plugin"))
        }),
        _ => None,
    };
    format!("#!/bin/sh\n{}", own.unwrap_or_else(|| closed(name)))
}

pub fn write_closed(pack: &Path, instance_root: &Path) -> Result<(), String> {
    let dir = dsh_dir(instance_root).join(CLOSED_DIR);
    let e = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dsh_dir(instance_root)).map_err(e)?;
    crate::omni_child::own_dir(&dir)?;
    for f in std::fs::read_dir(&dir).map_err(e)? {
        let f = f.map_err(e)?;
        if !CLOSED.iter().any(|n| f.file_name() == *n) {
            std::fs::remove_file(f.path()).map_err(|e| format!("{}: {e}", f.path().display()))?;
        }
    }
    for name in CLOSED {
        crate::omni_child::write_mode(&dir.join(name), closed_command(pack, name).as_bytes(), 0o700)?;
    }
    Ok(())
}

pub fn instance_path(pack: &Path, instance_root: &Path, home: &Path, base_path: &str) -> String {
    let dsh = dsh_dir(instance_root);
    let mut path: Vec<String> = Vec::new();
    let first = [pack.join("genie/bin"), dsh.join("bin"), dsh.join(CLOSED_DIR)].map(|p| p.display().to_string());
    for d in first.into_iter().chain(base_path.split(':').map(str::to_string)).chain(tool_dirs(home).iter().map(|p| p.display().to_string())) {
        if !d.is_empty() && !path.contains(&d) {
            path.push(d);
        }
    }
    path.join(":")
}

pub fn env(cfg: &ChildCfg, channel: &[(String, String)]) -> BTreeMap<String, String> {
    let dsh = dsh_dir(&cfg.instance_root);
    let omni = cfg.instance_root.join("omni");
    let unset = |k: &str| ROWS.iter().any(|(n, r)| *n == k && matches!(r, Row::Unset(_) | Row::Carried(_)));
    let mut e: BTreeMap<String, String> = cfg.base_env.iter().filter(|(k, _)| !unset(k) && !k.starts_with("RTH_")).cloned().collect();
    let base_path = e.get("PATH").cloned().unwrap_or_default();
    let path = instance_path(&cfg.pack, &cfg.instance_root, &cfg.home, &base_path);
    let p = |x: PathBuf| x.display().to_string();
    for (k, v) in [
        ("HOME", p(cfg.home.clone())),
        ("DSH_HOME", p(dsh.join("home"))),
        ("AUTOMAGIK_HOME", p(dsh.join("automagik"))),
        ("AUTOMAGIK_BOTS_HOME", p(dsh.join("automagik/bots"))),
        ("AUTOMAGIK_HOST", "unfold".into()),
        ("AUTOMAGIK_PRELOAD", "unfold".into()),
        ("AUTOMAGIK_BUSY_FILE", p(dsh.join("automagik/busy"))),
        ("DSH_OMC_STATE_DIR", p(dsh.join("omc"))),
        ("OMNI_HOME", p(omni.clone())),
        ("OMNI_CONFIG_DIR", p(omni.join("sandbox/config"))),
        ("OMNI_TELEMETRY", "false".into()),
        ("OMNI_SKIP_POSTINSTALL_PIN", "1".into()),
        ("UNFOLD_BRIDGE_STATE_DIR", p(dsh.join("bridge"))),
        ("UNFOLD_BRIDGE_PORT", cfg.ports.bridge.to_string()),

        ("BUN_INSTALL", p(omni.join("bun"))),
        ("DSH_AGENTS_HOME", p(dsh.join("agents"))),

        ("GENIE_HOME", p(cfg.instance_root.join("genie"))),
        ("GENIE_WISHES_DIR", p(cfg.instance_root.join("genie/wishes"))),
        ("PATH", path),
        ("TMPDIR", p(dsh.join("tmp"))),
        ("DSH_TELEMETRY_DISABLED", "1".into()),
        ("DO_NOT_TRACK", "1".into()),
        ("DISABLE_TELEMETRY", "1".into()),
        ("OPENUI_TELEMETRY_DISABLED", "1".into()),
        ("npm_config_update_notifier", "false".into()),
        ("DISABLE_ERROR_REPORTING", "1".into()),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".into()),
    ] {
        e.insert(k.into(), v);
    }
    e.extend(channel.iter().cloned());
    if let Some(k) = &cfg.deepseek_key {
        e.insert("DEEPSEEK_API_KEY".into(), k.0.clone());
    }
    e
}

pub const OVERLAY_ROWS: [&str; 5] = ["product-analytics", "desktop-product-telemetry", "session-log-deepseek", "plugin-package-inventory-deepseek", "session-telemetry-otel"];

pub fn overlay() -> String {
    let mut s = String::from("# Written by Unfold at each start: every upload is off. Edits here are replaced.\n");
    for id in OVERLAY_ROWS {
        s.push_str(&format!("- id: {id}\n  disabled: true\n"));
    }
    s
}

pub fn launch_link(line: &str, port: u16) -> Option<String> {
    let at = line.find("dsh web: http://127.0.0.1:")?;
    let link = line[at + "dsh web: ".len()..].split_whitespace().next()?;
    let rest = &link["http://127.0.0.1:".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (digits.parse::<u16>().ok() == Some(port) && rest[digits.len()..].starts_with('/') && link.contains("token=")).then(|| link.to_string())
}

pub fn scrub(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("token=") {
        out.push_str(&rest[..at]);
        out.push_str("(token withheld)");
        let tail = &rest[at + "token=".len()..];
        rest = &tail[tail.find(|c: char| c.is_whitespace() || c == '&' || c == '"' || c == '\'').unwrap_or(tail.len())..];
    }
    out.push_str(rest);
    out
}

pub const GIVE_UP: usize = 6;

pub fn backoff(exits: &[Instant], now: Instant) -> Option<Duration> {
    let recent = exits.iter().filter(|t| now.duration_since(**t) <= Duration::from_secs(600)).count();
    (recent < GIVE_UP).then(|| Duration::from_secs(1 << recent.saturating_sub(1).min(4)))
}

pub fn restart_after_reply(replying: &dyn Fn() -> bool, restart: &dyn Fn(), within: Duration) {
    let end = Instant::now() + within;
    while replying() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    restart();
}

static RECORDS: Mutex<()> = Mutex::new(());

fn record_line(name: &str, id: &crate::tray::Identity) -> Value {
    let me = std::process::id();
    let owner = crate::nemo::proc_info(me).map_or(0, |i| i.start_us);
    json!({"name": name, "pid": id.pid, "pgid": id.pgid, "start_time": id.start_us, "command": id.command, "owner": {"pid": me, "start_time": owner}})
}

fn read_records(file: &Path) -> Vec<Value> {
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

pub fn records_put(file: &Path, rec: Value) -> Result<(), String> {
    let _one = RECORDS.lock().unwrap();
    let mut list = read_records(file);
    list.retain(|r| r["pid"] != rec["pid"]);
    list.push(rec);
    write_private(file, &Value::Array(list).to_string())
}

pub fn records_drop(file: &Path, pid: u32) {
    let _one = RECORDS.lock().unwrap();
    let list = read_records(file);
    if !list.iter().any(|r| r["pid"] == pid) {
        return;
    }
    let left: Vec<Value> = list.into_iter().filter(|r| r["pid"] != pid).collect();
    let _ = if left.is_empty() { std::fs::remove_file(file).map_err(|e| e.to_string()) } else { write_private(file, &Value::Array(left).to_string()) };
}

const CHANGED_IN_LOG: usize = 20;
const CHANGED_NAME_MAX: usize = 256;
const CHANGED_LIST_MAX: usize = 1 << 20;
const CHANGED_LIST: &str = "pack-changed.txt";

fn one_line(name: &str) -> String {
    name.chars().map(|c| if c.is_control() { c.escape_default().to_string() } else { c.to_string() }).collect()
}

fn changed_in_log(changed: &[String], listed: bool) -> String {
    let cut = |n: &String| {
        let l = one_line(n);
        if l.chars().count() > CHANGED_NAME_MAX { l.chars().take(CHANGED_NAME_MAX).collect::<String>() + "…" } else { l }
    };
    let mut s = changed.iter().take(CHANGED_IN_LOG).map(cut).collect::<Vec<_>>().join(", ");
    if changed.len() > CHANGED_IN_LOG {
        s += &format!(" and {} more", changed.len() - CHANGED_IN_LOG);
        if listed {
            s += &format!(" (all in {CHANGED_LIST})");
        }
    }
    s
}

fn changed_list(changed: &[String]) -> String {
    let mut out = String::new();
    for (i, n) in changed.iter().enumerate() {
        let line = one_line(n) + "\n";
        if out.len() + line.len() > CHANGED_LIST_MAX - 32 {
            out += &format!("and {} more\n", changed.len() - i);
            break;
        }
        out += &line;
    }
    out
}

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn stop_leftovers(children: &Path, log: &Log) {
    let _one = RECORDS.lock().unwrap();
    let list = read_records(children);
    if list.is_empty() {
        return;
    }
    let mut kept = Vec::new();
    for c in list {
        let owner_runs = c["owner"]["pid"].as_u64().is_some_and(|p| crate::nemo::proc_info(p as u32).is_some_and(|i| Some(i.start_us) == c["owner"]["start_time"].as_u64()));
        if owner_runs {
            kept.push(c);
            continue;
        }
        let (Some(pid), Some(start)) = (c["pid"].as_u64(), c["start_time"].as_u64()) else { continue };
        let pid = pid as u32;
        if pid <= 1 {
            continue;
        }
        let Some(info) = crate::nemo::proc_info(pid) else { continue };

        if info.start_us != start || c["pgid"].as_i64() != Some(pid as i64) || info.pgid != pid as libc::pid_t {
            continue;
        }
        log.line(&format!("dsh: the last run's {} (pid {pid}) still runs: {}", c["name"].as_str().unwrap_or("child"), crate::nemo::stop_recorded(pid, info)));
    }
    let _ = if kept.is_empty() { std::fs::remove_file(children).map_err(|e| e.to_string()) } else { write_private(children, &Value::Array(kept).to_string()) };
}

static LINKS: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());

pub fn admitted_link(d: &OwnDsh) -> Option<String> {
    LINKS.lock().unwrap().iter().find(|(p, _)| *p == d.pid).map(|(_, l)| l.clone())
}

pub const CHANNEL_VARS: [&str; 4] = ["UNFOLD_AGENTD_OBSERVER", "UNFOLD_CONTROL_SOCKET", "UNFOLD_CONTROL_TOKEN_FILE", "UNFOLD_HOST_JOURNAL"];

const LAUNCHER: &str = r#"exec 2>&1; exec /bin/bash "$1/launch.sh" "$2" "$3""#;

pub const DSH_BIN: &str = "runtime/node_modules/@deepseek-ai/dsh/lib/bin.js";

pub const PACK_FILES: [&str; 5] = ["bin/node", "launch.sh", "install.mjs", "preload.mjs", DSH_BIN];

pub fn pack_layout(pack: &Path) -> Result<(), String> {
    let missing: Vec<&str> = PACK_FILES.iter().copied().filter(|f| !pack.join(f).is_file()).collect();
    match missing.is_empty() {
        true => Ok(()),
        false => Err(format!("{} isn't laid out as this app runs a pack (no {})", pack.display(), missing.join(", "))),
    }
}

#[derive(Debug, PartialEq)]
pub enum StartError {

    PortTaken,

    SockDir(String),

    Cancelled,
    Other(String),
}

pub fn start(cfg: &ChildCfg, owned: &Shared, log: &Log, alive: &dyn Fn() -> bool) -> Result<OwnDsh, StartError> {
    let other = |e: String| StartError::Other(e);
    let dsh = dsh_dir(&cfg.instance_root);
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true).mode(0o700);
    for d in [dsh.clone(), dsh.join("home"), dsh.join("agents"), dsh.join("automagik/bots"), dsh.join("omc"), dsh.join("bridge"), dsh.join("tmp"), dsh.join("bin"), cfg.instance_root.join("omni/sandbox/config")] {
        b.create(&d).map_err(|e| other(format!("{}: {e}", d.display())))?;
    }
    write_closed(&cfg.pack, &cfg.instance_root).map_err(other)?;
    for d in [cfg.instance_root.join("genie"), cfg.instance_root.join("genie/wishes")] {
        crate::omni_child::own_dir(&d).map_err(other)?;
    }
    let sockdir = (cfg.sockdir)(&cfg.instance_id).map_err(StartError::SockDir)?;
    let children = dsh.join("children.json");

    let channel = (cfg.serve)(&sockdir, log).map_err(other)?;
    let endpoint = crate::dsh::own_endpoint_path(&cfg.instance_root);
    let _ = std::fs::remove_file(&endpoint);
    let overlay_file = dsh.join("privacy-overlay.yaml");
    write_private(&overlay_file, &overlay()).map_err(other)?;
    let mut cmd = Command::new("/bin/bash");
    cmd.args(["-c", LAUNCHER, "dsh"]).arg(&cfg.pack).arg(&overlay_file).arg(cfg.ports.web.to_string());
    cmd.env_clear().envs(env(cfg, &channel)).current_dir(&dsh).stdin(Stdio::null()).stdout(Stdio::piped());
    let pid = crate::tray::spawn_recorded(owned, "dsh", &mut cmd).map_err(other)?;
    let stopped = |e: StartError| {
        crate::tray::stop_one(owned, pid, log);
        records_drop(&children, pid);
        e
    };

    match crate::tray::identity(owned, pid) {
        Some(id) => records_put(&children, record_line("dsh", &id)).map_err(|e| stopped(other(e)))?,
        None => return Err(stopped(other("DSH ended at its start".into()))),
    }
    let out = crate::tray::take_stdout(owned, pid).ok_or_else(|| stopped(other("no output".into())))?;
    let dsh_log = std::fs::OpenOptions::new().append(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(dsh.join("dsh.log")).map_err(|e| other(e.to_string()))?;
    let (tx, rx) = mpsc::channel();
    let web = cfg.ports.web;

    let withheld = cfg.deepseek_key.as_ref().map(|k| k.0.clone()).filter(|k| !k.is_empty());
    std::thread::spawn(move || {
        let mut dsh_log = dsh_log;
        let mut tx = Some(tx);
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let line = match &withheld {
                Some(k) if line.contains(k.as_str()) => line.replace(k.as_str(), "<withheld>"),
                _ => line,
            };
            if let Some(link) = launch_link(&line, web) {
                LINKS.lock().unwrap().push((pid, link));
                let _ = writeln!(dsh_log, "dsh web: (token withheld)");
                if let Some(t) = tx.take() {
                    let _ = t.send(Ok(()));
                }
                continue;
            }
            let _ = writeln!(dsh_log, "{}", scrub(&line));
            if line.contains("EADDRINUSE") {
                if let Some(t) = tx.take() {
                    let _ = t.send(Err(StartError::PortTaken));
                }
            }
        }

        if let Some(t) = tx.take() {
            let _ = t.send(Err(StartError::Other("DSH stopped before it was ready".into())));
        }
    });

    let end = Instant::now() + cfg.ready_within;
    let ready = loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(r) => break r,
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err(StartError::Other("DSH stopped before it was ready".into())),
            Err(mpsc::RecvTimeoutError::Timeout) if !alive() => break Err(StartError::Cancelled),
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= end => break Err(StartError::Other(format!("DSH was not ready within {} s", cfg.ready_within.as_secs()))),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    };
    if let Err(e) = ready {
        return Err(stopped(e));
    }

    let id = crate::tray::identity(owned, pid).filter(|i| cfg.node.canonicalize().ok().as_ref() == Some(&i.command)).ok_or_else(|| stopped(other("DSH is not the pack's Node after its start".into())))?;
    records_put(&children, record_line("dsh", &id)).map_err(|e| stopped(other(e)))?;
    log.line(&format!("dsh: Unfold's own DSH runs (pid {pid}) on 127.0.0.1:{web}, launch link kept in memory (token withheld)"));
    Ok(OwnDsh { pid, start_us: id.start_us, port: cfg.ports.bridge, web, endpoint })
}

pub fn stop(d: &OwnDsh, instance_root: &Path, owned: &Shared, log: &Log) {
    crate::tray::stop_one(owned, d.pid, log);
    LINKS.lock().unwrap().retain(|(p, _)| *p != d.pid);
    let _ = std::fs::remove_file(&d.endpoint);
    records_drop(&dsh_dir(instance_root).join("children.json"), d.pid);
    ports_free(&[d.web, d.port], log);
}

pub fn ports_free(ports: &[u16], log: &Log) -> bool {
    let mut free = true;
    for port in ports {
        if std::net::TcpListener::bind(("127.0.0.1", *port)).is_err() {
            log.line(&format!("dsh: port {port} is still in use after the stop"));
            free = false;
        }
    }
    free
}

struct Runtime {
    owned: Shared,
    log: Log,
    data: PathBuf,
    instance_root: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Status {
    on: bool,
    running: bool,
    web: Option<u16>,
    notice: Option<&'static str>,
    omni: Option<&'static str>,

    fetching: bool,

    progress: Option<Progress>,

    changed: Vec<String>,
    changed_n: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Progress {
    phase: &'static str,
    done: u64,
    total: u64,
}

impl Progress {
    fn of(step: crate::dsh_pack::Step) -> Progress {
        use crate::dsh_pack::Step;
        match step {
            Step::Download { done, total } => Progress { phase: "download", done, total },
            Step::Verify => Progress { phase: "verify", done: 0, total: 0 },
            Step::Unpack => Progress { phase: "unpack", done: 0, total: 0 },
            Step::Check => Progress { phase: "check", done: 0, total: 0 },
        }
    }
    const START: Progress = Progress { phase: "start", done: 0, total: 0 };
}

fn progress_json(p: &Option<Progress>) -> Value {
    p.map_or(Value::Null, |p| json!({"phase": p.phase, "done": p.done, "total": p.total}))
}

const PUSH_EVERY: Duration = Duration::from_millis(250);

pub struct Hooks {
    pub start: fn(&ChildCfg, &Shared, &Log, &dyn Fn() -> bool) -> Result<OwnDsh, StartError>,
    pub stop: fn(&OwnDsh, &Path, &Shared, &Log),
    pub exited: fn(&Shared, u32, &Log) -> Option<bool>,

    pub pack: fn(&Path, &Log, &(dyn Fn() -> bool + Sync), &dyn Fn(crate::dsh_pack::Step)) -> Result<(PathBuf, Option<&'static str>), (&'static str, String)>,

    pub healthy: fn(&Path, &Path, &Log),

    pub bad: fn(&Path, &Path, &str, &Log),

    pub backoff: fn(&[Instant], Instant) -> Option<Duration>,
    pub ports: fn(&Path, &str) -> Result<Ports, String>,

    pub manifest: fn(&Path) -> Result<(), Vec<String>>,

    pub refetch: fn(&Path, &Log, &(dyn Fn() -> bool + Sync), &dyn Fn(crate::dsh_pack::Step)) -> Result<(), String>,

    pub can_refetch: fn() -> bool,
    pub leftovers: fn(&Path, &Log),

    pub omni: fn(&'static Own, u64, PathBuf, Ports, mpsc::Sender<()>),
    pub sockdir: fn(&str) -> Result<PathBuf, String>,
    pub serve: fn(&Path, &Log) -> Result<Vec<(String, String)>, String>,

    pub changed: fn(&Log),

    pub refresh: fn(),

    pub address: fn(Option<String>),
    pub ready_within: Duration,

    pub key: fn(&Path, &Log) -> Option<CarriedKey>,
}

pub struct Own {
    hooks: Hooks,
    channel: &'static Channel,
    rt: std::sync::OnceLock<Runtime>,
    status: Mutex<Status>,

    gen: std::sync::atomic::AtomicU64,

    current: Mutex<Option<OwnDsh>>,

    switch: Mutex<()>,

    run_lock: Mutex<()>,

    omni_retry: std::sync::atomic::AtomicBool,

    carried: Mutex<Option<[u8; 32]>>,

    key_restart: std::sync::atomic::AtomicBool,

    pushed: Mutex<Option<Instant>>,
}

const SEQ: std::sync::atomic::Ordering = std::sync::atomic::Ordering::SeqCst;

impl Own {
    pub const fn new(hooks: Hooks, channel: &'static Channel) -> Own {
        Own {
            hooks,
            channel,
            rt: std::sync::OnceLock::new(),
            status: Mutex::new(Status { on: false, running: false, web: None, notice: None, omni: None, fetching: false, progress: None, changed: Vec::new(), changed_n: 0 }),
            gen: std::sync::atomic::AtomicU64::new(0),
            current: Mutex::new(None),
            switch: Mutex::new(()),
            run_lock: Mutex::new(()),
            omni_retry: std::sync::atomic::AtomicBool::new(false),
            carried: Mutex::new(None),
            key_restart: std::sync::atomic::AtomicBool::new(false),
            pushed: Mutex::new(None),
        }
    }

    fn alive(&self, gen: u64) -> bool {
        self.gen.load(SEQ) == gen
    }

    fn view(&self) -> Value {
        let s = self.status.lock().unwrap().clone();
        let address = s.web.filter(|_| s.running).map(|p| format!("http://127.0.0.1:{p}/"));
        json!({"on": s.on, "running": s.running, "port": s.web, "address": address, "signed_in": crate::dsh::signed_in(), "notice": s.notice, "omni": s.omni, "fetching": s.fetching, "progress": progress_json(&s.progress), "changed": s.changed, "changed_n": s.changed_n, "refetch": (self.hooks.can_refetch)()})
    }

    fn main_view(&self) -> Value {
        let s = self.status.lock().unwrap().clone();
        json!({"on": s.on, "running": s.running, "notice": s.notice, "progress": progress_json(&s.progress)})
    }

    fn step(&self, gen: u64, step: crate::dsh_pack::Step) {
        let p = Progress::of(step);
        let mut s = self.status.lock().unwrap();
        if !self.alive(gen) {
            return;
        }
        let same = s.progress.is_some_and(|o| o.phase == p.phase);
        s.progress = Some(p);
        drop(s);
        let now = Instant::now();
        {
            let mut last = self.pushed.lock().unwrap();
            if same && last.is_some_and(|t| now.duration_since(t) < PUSH_EVERY) {
                return;
            }
            *last = Some(now);
        }
        (self.hooks.refresh)();
    }

    fn source(&self) -> crate::dsh::Source {
        use crate::dsh::{Owner, Source};
        match &*self.current.lock().unwrap() {
            Some(d) => Source { path: d.endpoint.clone(), owner: Owner::Child { pid: d.pid, start_us: d.start_us, port: d.port } },
            None => Source { path: PathBuf::new(), owner: Owner::Off },
        }
    }

    fn init(&'static self, owned: Shared, log: Log, data: PathBuf, instance_root: PathBuf, on: bool) {
        let _ = self.rt.set(Runtime { owned, log, data, instance_root });
        if on {
            self.set_on(true);
        }
    }

    fn set_status(&self, gen: u64, f: impl FnOnce(&mut Status)) -> bool {
        let mut s = self.status.lock().unwrap();
        if !self.alive(gen) {
            return false;
        }
        f(&mut s);
        drop(s);
        (self.hooks.refresh)();
        true
    }

    fn set_on(&'static self, on: bool) {
        if self.rt.get().is_none() {
            return;
        }
        let gen = self.gen.fetch_add(1, SEQ) + 1;
        self.omni_retry.store(false, SEQ);
        *self.status.lock().unwrap() = Status { on, ..Status::default() };
        (self.hooks.refresh)();
        std::thread::spawn(move || {
            let rt = self.rt.get().expect("set");

            #[cfg(test)]
            tests::switch::stall(self, gen);
            {
                let _one = self.switch.lock().unwrap();

                if !self.alive(gen) {
                    return;
                }
                let taken = self.current.lock().unwrap().take();
                if let Some(d) = taken {
                    (self.hooks.address)(None);
                    (self.hooks.stop)(&d, &rt.instance_root, &rt.owned, &rt.log);
                    (self.hooks.changed)(&rt.log);
                }
                if !on {
                    self.channel.stop();
                }
            }
            if on {
                self.run(gen);
            }
        });
    }

    fn retry(&'static self) {
        if self.status.lock().unwrap().on {
            self.set_on(true);
        }
    }

    fn another_port(&'static self) -> Result<(), String> {
        let rt = self.rt.get().ok_or("not admitted")?;
        if !self.status.lock().unwrap().on {
            return Ok(());
        }
        let id = instance_id(&rt.instance_root)?;
        let gen = self.gen.load(SEQ);
        std::thread::spawn(move || match crate::dsh_ports::next_web_port(&rt.data, &id) {

            Ok(_) if self.alive(gen) => self.set_on(true),
            Ok(_) => {}
            Err(e) => rt.log.line(&format!("dsh: no other port: {e}")),
        });
        Ok(())
    }

    fn fetch_again(&'static self) {
        if self.rt.get().is_none() || !(self.hooks.can_refetch)() {
            return;
        }
        let gen = {
            let mut s = self.status.lock().unwrap();
            if !s.on || !matches!(s.notice, Some("pack_changed" | "fetch_failed")) {
                return;
            }
            let gen = self.gen.fetch_add(1, SEQ) + 1;
            *s = Status { on: true, fetching: true, ..Status::default() };
            gen
        };
        self.omni_retry.store(false, SEQ);
        (self.hooks.refresh)();
        std::thread::spawn(move || {
            let rt = self.rt.get().expect("set");
            let fetched = {
                let _one = self.run_lock.lock().unwrap();
                if !self.alive(gen) {
                    return;
                }
                (self.hooks.refetch)(&rt.data, &rt.log, &|| self.alive(gen), &|st| self.step(gen, st))
            };
            match fetched {
                Ok(()) if self.set_status(gen, |s| s.fetching = false) => self.run(gen),
                Ok(()) => {}
                Err(e) => {
                    rt.log.line(&format!("dsh: Fetch again: {e}"));
                    self.set_status(gen, |s| {
                        s.fetching = false;
                        s.progress = None;
                        s.notice = Some("fetch_failed");
                    });
                }
            }
        });
    }

    fn key_changed(&self) -> bool {
        let Some(rt) = self.rt.get() else { return false };
        if self.current.lock().unwrap().is_none() {
            return false;
        }
        let now = (self.hooks.key)(&rt.data, &rt.log).map(|k| k.fingerprint());
        now != *self.carried.lock().unwrap()
    }

    fn key_check(&'static self, replying: fn() -> bool) {
        if !self.key_changed() || self.key_restart.swap(true, SEQ) {
            return;
        }
        if let Some(rt) = self.rt.get() {
            rt.log.line("dsh: the DeepSeek key in Unfold changed: Unfold Automagik restarts after the reply in progress");
        }
        std::thread::spawn(move || {
            restart_after_reply(&replying, &|| {
                self.key_restart.store(false, SEQ);
                self.retry();
            }, Duration::from_secs(600))
        });
    }

    fn omni_retry(&self) {
        let s = self.status.lock().unwrap();
        if s.on && s.omni == Some("not_running") {
            self.omni_retry.store(true, SEQ);
        }
    }

    fn run(&'static self, gen: u64) {
        let _one = self.run_lock.lock().unwrap();
        let rt = self.rt.get().expect("set");
        let alive = move || self.alive(gen);
        if !alive() {
            return;
        }
        let dsh = dsh_dir(&rt.instance_root);
        let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dsh);

        (self.hooks.leftovers)(&dsh.join("children.json"), &rt.log);
        let mut exit = RunExit { own: self, omni: None, ports: Vec::new(), log: rt.log.clone() };
        let fail = |notice: &'static str, why: &str| {
            rt.log.line(&format!("dsh: Unfold's own DSH doesn't run ({notice}): {why}"));
            self.set_status(gen, |s| {
                s.running = false;
                s.progress = None;
                s.notice = Some(notice);
            });
        };
        let Ok(id) = instance_id(&rt.instance_root) else { return fail("failed", "no instance") };
        let (pack, kept) = match (self.hooks.pack)(&rt.data, &rt.log, &alive, &|st| self.step(gen, st)) {
            Ok(p) => p,
            Err(_) if !alive() => return,
            Err((notice, e)) => return fail(notice, &e),
        };

        if self.status.lock().unwrap().progress.is_some() {
            self.set_status(gen, |s| s.progress = Some(Progress::START));
        }
        let ports = match (self.hooks.ports)(&rt.data, &id) {
            Ok(p) => p,
            Err(e) => return fail("failed", &e),
        };

        exit.ports = vec![ports.omni_api, ports.omni_db, ports.omni_nats];

        let (first, started) = mpsc::channel();
        let (hook, p2) = (self.hooks.omni, pack.clone());
        exit.omni = Some(std::thread::spawn(move || hook(self, gen, p2, ports, first)));
        while alive() {
            match started.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                _ => break,
            }
        }
        let mut cfg = ChildCfg {
            node: node_of(&pack),
            pack,
            instance_root: rt.instance_root.clone(),
            instance_id: id,
            ports,
            home: crate::home(),
            base_env: std::env::vars().collect(),
            sockdir: self.hooks.sockdir,
            serve: self.hooks.serve,
            ready_within: self.hooks.ready_within,
            deepseek_key: None,
        };
        let mut exits: Vec<Instant> = Vec::new();
        let mut ran_healthy = false;
        loop {
            self.channel.wait(&alive);
            if !alive() {
                return;
            }

            let list = dsh.join(CHANGED_LIST);
            if let Err(changed) = (self.hooks.manifest)(&cfg.pack) {
                let written = write_private(&list, &changed_list(&changed));
                let (n, mut names) = (changed.len(), changed_in_log(&changed, written.is_ok()));
                if let Err(e) = &written {

                    let _ = std::fs::remove_file(&list);
                    names += &format!("; the full list couldn't be written: {e}");
                }
                self.set_status(gen, |s| {
                    s.changed = changed.into_iter().take(3).collect();
                    s.changed_n = n;
                });
                return fail("pack_changed", &format!("{n} of its files differ from the pack's manifest: {names}"));
            }
            let _ = std::fs::remove_file(&list);

            cfg.deepseek_key = (self.hooks.key)(&rt.data, &rt.log);
            rt.log.line(if cfg.deepseek_key.is_some() { "dsh: Unfold's DeepSeek key is shared with Unfold Automagik (its environment only)" } else { "dsh: no DeepSeek key in Unfold to share with Unfold Automagik" });
            let d = match (self.hooks.start)(&cfg, &rt.owned, &rt.log, &alive) {
                Ok(d) => d,
                Err(StartError::Cancelled) => return,
                Err(StartError::PortTaken) => return fail("port_taken", "another process serves its web port"),
                Err(StartError::SockDir(e)) => return fail("socket_dir", &e),
                Err(StartError::Other(e)) => {

                    self.channel.stop();
                    exits.push(Instant::now());
                    match (self.hooks.backoff)(&exits, Instant::now()) {
                        Some(wait) if alive() => {
                            rt.log.line(&format!("dsh: start failed ({e}); trying again in {} s", wait.as_secs()));
                            sleep_while(wait, &alive);
                            continue;
                        }
                        _ if !alive() => return,
                        _ => {

                            if !ran_healthy {
                                (self.hooks.bad)(&rt.data, &cfg.pack, &e, &rt.log);
                            }
                            return fail("crash_loop", &e);
                        }
                    }
                }
            };
            {

                let _switch = self.switch.lock().unwrap();
                if !alive() {
                    (self.hooks.stop)(&d, &rt.instance_root, &rt.owned, &rt.log);
                    return;
                }
                *self.current.lock().unwrap() = Some(d.clone());
                *self.carried.lock().unwrap() = cfg.deepseek_key.as_ref().map(CarriedKey::fingerprint);
            }

            (self.hooks.address)(admitted_link(&d));
            self.set_status(gen, |s| {
                s.running = true;
                s.web = Some(d.web);
                s.notice = kept;
                s.progress = None;
            });
            (self.hooks.changed)(&rt.log);
            if !ran_healthy {
                (self.hooks.healthy)(&rt.data, &cfg.pack, &rt.log);
                ran_healthy = true;
            }

            let ended = loop {
                std::thread::sleep(Duration::from_millis(100));
                if !alive() {
                    break false;
                }
                match (self.hooks.exited)(&rt.owned, d.pid, &rt.log) {
                    Some(false) => continue,
                    Some(true) => break true,
                    None => break false,
                }
            };
            if !ended {
                return;
            }
            self.current.lock().unwrap().take();
            (self.hooks.address)(None);
            LINKS.lock().unwrap().retain(|(p, _)| *p != d.pid);
            let _ = std::fs::remove_file(&d.endpoint);
            records_drop(&dsh.join("children.json"), d.pid);
            exits.push(Instant::now());
            self.set_status(gen, |s| s.running = false);
            match (self.hooks.backoff)(&exits, Instant::now()) {
                Some(wait) if alive() => {
                    rt.log.line(&format!("dsh: Unfold's own DSH stopped; starting again in {} s", wait.as_secs()));
                    sleep_while(wait, &alive);
                }
                _ => return fail("crash_loop", &format!("it stopped {} times within 10 minutes", exits.len())),
            }
        }
    }
}

struct RunExit {
    own: &'static Own,
    omni: Option<std::thread::JoinHandle<()>>,
    ports: Vec<u16>,
    log: Log,
}

impl Drop for RunExit {
    fn drop(&mut self) {
        self.own.channel.stop();
        if let Some(t) = self.omni.take() {
            let _ = t.join();
            ports_free(&self.ports, &self.log);
        }
    }
}

fn sleep_while(d: Duration, alive: &dyn Fn() -> bool) {
    let end = Instant::now() + d;
    while alive() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100).min(end.saturating_duration_since(Instant::now())));
    }
}

fn node_of(pack: &Path) -> PathBuf {
    pack.join("bin/node")
}

fn instance_id(root: &Path) -> Result<String, String> {
    root.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| "no instance id".into())
}

pub type ServeFn = fn(crate::dsh_control::Config, std::sync::Arc<dyn crate::dsh_control::Children>, crate::dsh_control::Submit, &Log) -> Result<crate::dsh_control::Endpoint, String>;

enum Op {

    Serve { dir: PathBuf, journal: PathBuf, children: std::sync::Arc<dyn crate::dsh_control::Children>, submit: crate::dsh_control::Submit, log: Log, done: mpsc::Sender<Result<Vec<(String, String)>, String>> },

    Publish { log: Option<Log> },

    Stop { done: mpsc::Sender<()> },
}

#[derive(Default)]
struct Lease {

    held: Option<(std::net::SocketAddr, Option<String>)>,

    observer: Option<PathBuf>,
}

const CHANNEL_WITHIN: Duration = Duration::from_secs(30);

pub struct Channel {
    serve_fn: ServeFn,

    ops: Mutex<Option<mpsc::Sender<Op>>>,
    lease: Mutex<Lease>,
    seen: std::sync::Condvar,
}

impl Channel {
    pub const fn new(serve_fn: ServeFn) -> Channel {
        Channel { serve_fn, ops: Mutex::new(None), lease: Mutex::new(Lease { held: None, observer: None }), seen: std::sync::Condvar::new() }
    }

    fn send(&'static self, op: Op) -> bool {
        let mut ops = self.ops.lock().unwrap();
        if ops.is_none() {
            let (tx, rx) = mpsc::channel();
            if std::thread::Builder::new().name("dsh-channel".into()).spawn(move || self.run(rx)).is_err() {
                return false;
            }
            *ops = Some(tx);
        }
        ops.as_ref().is_some_and(|tx| tx.send(op).is_ok())
    }

    fn run(&'static self, rx: mpsc::Receiver<Op>) {
        let mut ep: Option<crate::dsh_control::Endpoint> = None;
        for op in rx {
            match op {
                Op::Serve { dir, journal, children, submit, log, done } => {

                    self.halt(&mut ep);
                    let session = self.lease.lock().unwrap().held.as_ref().and_then(|(_, s)| s.clone());
                    let cfg = crate::dsh_control::Config { answer: Some(crate::answer::app()), dir: dir.clone(), session, plugin_host: None, journal };
                    let r = (self.serve_fn)(cfg, children, submit, &log).map(|served| {
                        let env = served.child_env();
                        let mut l = self.lease.lock().unwrap();
                        l.observer = Some(dir.join(crate::dsh_control::OBSERVER_FILE));

                        if let Some(Err(e)) = l.held.as_ref().map(|(b, _)| served.publish_observer(*b)) {
                            log.line(&format!("dsh control: agentd's address not published: {e}"));
                        }
                        drop(l);
                        ep = Some(served);
                        env
                    });
                    let _ = done.send(r);
                }
                Op::Publish { log } => {
                    let l = self.lease.lock().unwrap();
                    if let (Some(served), Some((b, _))) = (ep.as_ref(), l.held.as_ref()) {
                        if let (Err(e), Some(log)) = (served.publish_observer(*b), log) {
                            log.line(&format!("dsh control: agentd's address not published: {e}"));
                        }
                    }
                }
                Op::Stop { done } => {
                    self.halt(&mut ep);
                    let _ = done.send(());
                }
            }
        }
    }

    fn halt(&self, ep: &mut Option<crate::dsh_control::Endpoint>) {
        if let Some(old) = ep.take() {
            self.lease.lock().unwrap().observer = None;
            old.stop();
        }
    }

    fn serve(&'static self, dir: &Path, journal: PathBuf, children: std::sync::Arc<dyn crate::dsh_control::Children>, submit: crate::dsh_control::Submit, log: &Log) -> Result<Vec<(String, String)>, String> {
        let (done, answer) = mpsc::channel();
        if !self.send(Op::Serve { dir: dir.to_path_buf(), journal, children, submit, log: log.clone(), done }) {
            return Err("the control endpoint's thread is gone".into());
        }
        answer.recv_timeout(CHANNEL_WITHIN).unwrap_or_else(|_| Err(format!("the control endpoint wasn't served within {} s", CHANNEL_WITHIN.as_secs())))
    }

    fn stop(&'static self) {
        #[cfg(test)]
        {
            CHANNEL_STOPS.with(|n| n.set(n.get() + 1));
            ON_CHANNEL_STOP.with(|h| {
                if let Some(h) = h.borrow().as_ref() {
                    h()
                }
            });
        }
        let (done, stopped) = mpsc::channel();
        if self.send(Op::Stop { done }) && stopped.recv_timeout(CHANNEL_WITHIN).is_err() {
            eprintln!("dsh control: the endpoint's stop didn't end within {} s", CHANNEL_WITHIN.as_secs());
        }
    }

    fn held(&'static self, bind: std::net::SocketAddr, session: Option<String>, log: Option<&Log>) {
        self.lease.lock().unwrap().held = Some((bind, session));
        self.seen.notify_all();
        self.send(Op::Publish { log: log.cloned() });
    }

    fn gone(&'static self) {
        let mut l = self.lease.lock().unwrap();
        l.held = None;
        if let Some(observer) = &l.observer {
            let _ = std::fs::remove_file(observer);
        }
    }

    fn wait(&self, alive: &dyn Fn() -> bool) {
        let mut l = self.lease.lock().unwrap();
        while l.held.is_none() && alive() {
            l = self.seen.wait_timeout(l, Duration::from_millis(500)).unwrap().0;
        }
    }
}

static CHANNEL: Channel = Channel::new(crate::dsh_control::serve);

const APP: Hooks = Hooks {
    start,
    stop,
    exited: crate::tray::exited,
    pack: app_pack,
    healthy: app_healthy,
    bad: app_bad,
    backoff,
    ports: crate::dsh_ports::ports_for,
    manifest: app_manifest,
    refetch: app_refetch,
    can_refetch: crate::dsh_pack::start_can_refetch,
    leftovers: stop_leftovers,
    omni: omni_thread,
    sockdir: crate::dsh_sockdir::ensure,
    serve: serve_channel,
    changed: app_changed,
    refresh: app_refresh,
    address: crate::tray::set_address,
    ready_within: Duration::from_secs(300),
    key: app_key,
};

static OWN: Own = Own::new(APP, &CHANNEL);

fn app_pack(data: &Path, log: &Log, alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(crate::dsh_pack::Step)) -> Result<(PathBuf, Option<&'static str>), (&'static str, String)> {
    let p = crate::dsh_pack::for_start(data, &|l| log.line(l), alive, step).map_err(|e| ("no_pack", e))?;
    pack_layout(&p.dir).map_err(|e| ("failed", e))?;
    Ok((p.dir, kept_notice(&p.fell_back)))
}

fn kept_notice(fell_back: &Option<crate::dsh_pack::PackError>) -> Option<&'static str> {
    match fell_back {
        Some(crate::dsh_pack::PackError::Hash) => Some("pack_kept"),
        Some(crate::dsh_pack::PackError::Bad(_)) => Some("pack_rolled_back"),
        _ => None,
    }
}

fn version_of(pack: &Path) -> Result<&str, String> {
    pack.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("dsh-")).ok_or_else(|| format!("{} is not a pack folder", pack.display()))
}

fn app_healthy(data: &Path, pack: &Path, log: &Log) {
    if let Err(e) = version_of(pack).and_then(|v| crate::dsh_pack::mark_healthy(data, v)) {
        log.line(&format!("dsh: the pack ran, but isn't recorded as healthy ({e}); retried at its next start"));
    }
}

fn app_bad(data: &Path, pack: &Path, why: &str, log: &Log) {
    match version_of(pack).and_then(|v| crate::dsh_pack::mark_bad(data, v, why)) {
        Ok(()) => log.line(&format!("dsh: {} never ran healthy ({why}): marked bad and kept; the next start runs the last healthy pack", pack.display())),
        Err(e) => log.line(&format!("dsh: {} never ran healthy ({why}), not marked bad: {e}", pack.display())),
    }
}

fn app_manifest(pack: &Path) -> Result<(), Vec<String>> {
    crate::dsh_pack::check_manifest(pack, &[]).map_err(|changed| changed.iter().map(|p| p.display().to_string()).collect())
}

fn app_refetch(data: &Path, log: &Log, alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(crate::dsh_pack::Step)) -> Result<(), String> {
    crate::dsh_pack::refetch_for_start(data, &|l| log.line(l), alive, step).map(|_| ())
}

fn app_refresh() {
    crate::settings::refresh_open();
    push_main();
}

static MAIN: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

fn push_main() {
    use tauri::Manager;
    if let Some(w) = MAIN.get().and_then(|a| a.get_webview_window("main")) {
        let _ = w.eval(format!("window.__rthOwnDsh && window.__rthOwnDsh({})", OWN.main_view()));
    }
}

pub fn page_loaded(w: &tauri::WebviewWindow) {
    use tauri::Manager;
    if w.label() != "main" {
        return;
    }
    let _ = MAIN.set(w.app_handle().clone());
    push_main();
}

fn app_key(data: &Path, log: &Log) -> Option<CarriedKey> {
    crate::flash::deepseek_key(data, log).map(CarriedKey)
}

fn app_changed(log: &Log) {
    let s = crate::settings::current();
    if s.answers_via_bridge() || s.use_dsh_watch {
        log.line("dsh: Unfold's own DSH changed: agentd restarts after the reply in progress");
        std::thread::spawn(|| restart_after_reply(&crate::chat::replying, &crate::setup::restart_chat, Duration::from_secs(600)));
    }
}

struct InstanceChild(Shared);
impl crate::dsh_control::Children for InstanceChild {
    fn instance_child(&self) -> Option<crate::dsh_control::Recorded> {
        let pid = OWN.current.lock().unwrap().as_ref()?.pid;
        let id = crate::tray::identity(&self.0, pid)?;
        Some(crate::dsh_control::Recorded { pid: id.pid as i32, pgid: id.pgid, start_time: id.start_us, command: id.command.display().to_string() })
    }
}

pub struct OmniKids {
    owned: Shared,
    log: Log,

    record: PathBuf,
}

impl crate::omni_child::Children for OmniKids {
    fn spawn_recorded(&self, name: &'static str, cmd: &mut Command) -> Result<crate::omni_child::Recorded, String> {
        let pid = crate::tray::spawn_recorded(&self.owned, name, cmd)?;
        let Some(id) = crate::tray::identity(&self.owned, pid) else {
            crate::tray::stop_one(&self.owned, pid, &self.log);
            return Err(format!("{name} (pid {pid}) ended at its start"));
        };

        if let Err(e) = records_put(&self.record, record_line(name, &id)) {
            crate::tray::stop_one(&self.owned, pid, &self.log);
            return Err(format!("{name} (pid {pid}) could not be persisted: {e}"));
        }
        Ok(crate::omni_child::Recorded { pid: pid as i32, pgid: id.pgid, start_time: id.start_us, command: id.command.display().to_string() })
    }

    fn alive(&self, r: &crate::omni_child::Recorded) -> bool {
        crate::tray::identity(&self.owned, r.pid as u32).is_some_and(|i| i.start_us == r.start_time && i.pgid == r.pgid)
    }

    fn stop_recorded(&self, r: &crate::omni_child::Recorded) {
        crate::tray::stop_matching(&self.owned, r.pid as u32, r.start_time, &self.log);
        records_drop(&self.record, r.pid as u32);
    }
}

pub const OMNI_HEALTHY_WITHIN: Duration = Duration::from_secs(90);

pub fn omni_cfg(pack: &Path, instance_root: &Path, ports: Ports, owned: Shared, log: Log) -> Result<crate::omni_child::OmniCfg, String> {
    omni_cfg_with(pack, instance_root, ports, owned, log, std::env::vars().collect(), &crate::home())
}

fn omni_cfg_with(pack: &Path, instance_root: &Path, ports: Ports, owned: Shared, log: Log, base: Vec<(String, String)>, home: &Path) -> Result<crate::omni_child::OmniCfg, String> {
    let manifest = pack.join("omni/runtime/node_modules/@automagik/omni/package.json");
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let version = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["version"].as_str().map(str::to_string)).ok_or_else(|| format!("{}: no version", manifest.display()))?;
    write_closed(pack, instance_root)?;
    let path = instance_path(pack, instance_root, home, base.iter().find(|(k, _)| k == "PATH").map_or("", |(_, v)| v.as_str()));
    let env = base.into_iter().filter(|(k, _)| k != "PATH").chain([("PATH".to_string(), path)]).collect();
    Ok(crate::omni_child::OmniCfg {
        instance: instance_root.to_path_buf(),
        pack: pack.to_path_buf(),
        bun: pack.join("omni/bin/bun"),
        version,
        ports,
        bots_home: dsh_dir(instance_root).join("automagik/bots"),
        env,
        healthy_within: OMNI_HEALTHY_WITHIN,
        children: std::sync::Arc::new(OmniKids { owned, log: log.clone(), record: dsh_dir(instance_root).join("children.json") }),
        log,
    })
}

fn omni_thread(own: &'static Own, gen: u64, pack: PathBuf, ports: Ports, first: mpsc::Sender<()>) {
    let Some(rt) = own.rt.get() else { return };
    let alive = || own.alive(gen);
    let mut first = Some(first);
    loop {
        own.set_status(gen, |s| s.omni = None);
        let up = match omni_cfg(&pack, &rt.instance_root, ports, rt.owned.clone(), rt.log.clone()) {
            Ok(cfg) => omni_up(&cfg, &alive).map(|o| (cfg, o)),
            Err(e) => {
                rt.log.line(&format!("omni: not started ({e})"));
                None
            }
        };
        if let Some(t) = first.take() {
            let _ = t.send(());
        }
        if let Some((cfg, o)) = up {
            own.set_status(gen, |s| s.omni = Some("running"));
            if omni_watch(&cfg, o, &alive) {
                return;
            }
        }
        if !alive() {
            return;
        }
        own.set_status(gen, |s| s.omni = Some("not_running"));
        while alive() && !own.omni_retry.swap(false, SEQ) {
            std::thread::sleep(Duration::from_millis(200));
        }
        if !alive() {
            return;
        }
    }
}

fn omni_up(cfg: &crate::omni_child::OmniCfg, alive: &dyn Fn() -> bool) -> Option<crate::omni_child::OwnOmni> {
    let o = match crate::omni_child::start(cfg) {
        Ok(o) => o,
        Err(e) => {
            cfg.log.line(&format!("omni: not started ({e})"));
            return None;
        }
    };
    let end = Instant::now() + cfg.healthy_within;
    while !crate::omni_child::healthy(o.api, Duration::from_millis(200)) {
        if !alive() || Instant::now() >= end {
            crate::omni_child::stop(&o);
            cfg.log.line(&format!("omni: not healthy within {} s; stopped", cfg.healthy_within.as_secs()));
            return None;
        }
    }
    if let Err(why) = crate::omni_child::claim(cfg) {
        cfg.log.line(&format!("omni: {why}"));
    }
    Some(o)
}

fn omni_watch(cfg: &crate::omni_child::OmniCfg, mut o: crate::omni_child::OwnOmni, alive: &dyn Fn() -> bool) -> bool {
    let mut restarts = crate::omni_child::Restarts::default();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if !alive() {
            crate::omni_child::stop(&o);
            return true;
        }
        match crate::omni_child::watch(cfg, &o, &mut restarts, Instant::now(), &mut |d| sleep_while(d, alive)) {
            Ok(None) => {}
            Ok(Some(next)) => o = next,
            Err(e) => {
                cfg.log.line(&format!("omni: {e}"));
                return false;
            }
        }
    }
}

fn serve_channel(dir: &Path, log: &Log) -> Result<Vec<(String, String)>, String> {
    let rt = OWN.rt.get().ok_or("not admitted")?;
    CHANNEL.serve(dir, rt.data.join("app/dsh-control.jsonl"), std::sync::Arc::new(InstanceChild(rt.owned.clone())), std::sync::Arc::new(crate::setup::session_say), log)
}

pub fn stop_channel() {
    CHANNEL.stop();
}

#[cfg(test)]
thread_local! {

    static CHANNEL_STOPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    static LEASE_GONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    static ON_CHANNEL_STOP: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

pub fn lease_held(bind: std::net::SocketAddr, session: Option<String>) {
    CHANNEL.held(bind, session, OWN.rt.get().map(|r| &r.log));
}

pub fn lease_gone() {
    #[cfg(test)]
    LEASE_GONES.with(|n| n.set(n.get() + 1));
    CHANNEL.gone();
}

pub fn view() -> Value {
    OWN.view()
}

pub fn source() -> crate::dsh::Source {
    OWN.source()
}

pub fn init(owned: Shared, log: Log, data: PathBuf, instance_root: PathBuf, on: bool) {
    OWN.init(owned, log, data, instance_root, on);
}

pub fn set_on(on: bool) {
    OWN.set_on(on);
}

pub fn key_check() {
    OWN.key_check(crate::chat::replying);
}

pub fn retry() {
    OWN.retry();
}

pub fn another_port() -> Result<(), String> {
    OWN.another_port()
}

pub fn fetch_again() {
    OWN.fetch_again();
}

pub fn omni_retry() -> Result<(), String> {
    OWN.omni_retry();
    Ok(())
}

pub fn open(install: bool) -> Result<(), String> {
    let d = OWN.current.lock().unwrap().clone().ok_or("Unfold's own DSH is not running")?;
    let link = admitted_link(&d).ok_or("no launch link yet")?;
    let url = if install { format!("{link}#install") } else { link };
    Command::new("/usr/bin/open").arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err(format!("open: {s}")) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn node() -> PathBuf {
        let n = std::env::var_os("RTH_TEST_NODE").map_or(PathBuf::from("/opt/homebrew/bin/node"), PathBuf::from);
        assert!(n.is_file(), "these tests run the launcher on a real Node: set RTH_TEST_NODE to one ({} is missing)", n.display());
        n
    }

    const PACK_LAUNCH: &str = r#"#!/bin/sh
# Starts Unfold's own DSH instance: /bin/sh <pack>/launch.sh <overlay> <web port>, in the pack's
# folder, with the environment Unfold built for the instance. It runs the profile steps
# (install.mjs), then execs DSH on the pack's Node, so the process Unfold recorded becomes DSH.
#
# Node gets no NODE_OPTIONS and no argument but these: --disable-sigusr1 (no inspector on
# SIGUSR1), the channel preload, DSH's bin.js and DSH's own flags. The channel's four variables
# reach only the preload, which deletes them; install.mjs runs without them.
set -eu

[ "$#" -eq 2 ] || { echo "launch.sh: usage: launch.sh <overlay> <web port>" >&2; exit 64; }
case $1 in
  /*) ;;
  *) echo "launch.sh: the overlay must be an absolute path" >&2; exit 64 ;;
esac
[ -f "$1" ] || { echo "launch.sh: no overlay at $1" >&2; exit 66; }
case $2 in
  '' | *[!0-9]*) echo "launch.sh: the web port must be a number" >&2; exit 64 ;;
esac
[ "$2" -ge 1024 ] && [ "$2" -le 65535 ] || { echo "launch.sh: the web port must be 1024 to 65535" >&2; exit 64; }

PACK=$(cd "$(dirname "$0")" && pwd -P)
cd "$PACK"
unset NODE_OPTIONS

env -u UNFOLD_AGENTD_OBSERVER -u UNFOLD_CONTROL_SOCKET -u UNFOLD_CONTROL_TOKEN_FILE -u UNFOLD_HOST_JOURNAL \
  "$PACK/bin/node" --disable-sigusr1 "$PACK/install.mjs"

exec "$PACK/bin/node" --disable-sigusr1 --import "$PACK/preload.mjs" \
  "$PACK/runtime/node_modules/@deepseek-ai/dsh/lib/bin.js" \
  --profile omni --patch "$1" --no-open --host 127.0.0.1 --port "$2"
"#;

    const PACK_PRELOAD: &str = r#"// Loaded by launch.sh with --import, in DSH's own process, before DSH's bin.js and so before any
// plugin loads or any child is spawned. It takes the Unfold channel's four paths out of the
// environment (the three endpoint paths and the reserved host journal, which is usually unset), so
// no tool or agent DSH spawns later inherits them (they build their env from the live
// process.env), reads the control token into memory and unlinks its file.
//
// In-process plugins read them from globalThis[Symbol.for('unfold.channel')]:
//   { observerFile, socket, token, hostJournal }   frozen and not enumerable; null for what was
//   not given.
// This keeps the values out of every child's env; it is not a boundary inside DSH's process.
import { closeSync, constants, fstatSync, openSync, readSync, unlinkSync } from 'node:fs'
import { isAbsolute } from 'node:path'

const take = (name) => {
  const value = process.env[name]
  delete process.env[name]
  return typeof value === 'string' && isAbsolute(value) ? value : null
}

const observerFile = take('UNFOLD_AGENTD_OBSERVER')
const socket = take('UNFOLD_CONTROL_SOCKET')
const tokenFile = take('UNFOLD_CONTROL_TOKEN_FILE')
const hostJournal = take('UNFOLD_HOST_JOURNAL')
// launch.sh already starts Node without it; a child must not get one either.
delete process.env.NODE_OPTIONS

function readToken(file) {
  let fd
  try {
    fd = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW)
    const stat = fstatSync(fd)
    if (!stat.isFile() || stat.size > 128) return null
    const bytes = Buffer.alloc(stat.size)
    readSync(fd, bytes, 0, stat.size, 0)
    const token = bytes.toString('utf8').trim()
    return /^[0-9a-f]{64}$/.test(token) ? token : null
  } catch {
    return null
  } finally {
    if (fd !== undefined) closeSync(fd)
  }
}

let token = null
if (tokenFile) {
  token = readToken(tokenFile)
  try {
    unlinkSync(tokenFile)
  } catch {}
}

Object.defineProperty(globalThis, Symbol.for('unfold.channel'), {
  value: Object.freeze({ observerFile, socket, token, hostJournal }),
  enumerable: false,
  writable: false,
  configurable: false,
})
"#;

    const BIN_JS: &str = r#"import net from 'node:net'
import fs from 'node:fs'
if (process.env.FAKE_PID_RECORD) fs.appendFileSync(process.env.FAKE_PID_RECORD, `${process.pid} dsh-fake ${process.env.FAKE_RUN}\n`)
const harness = Number(process.env.FAKE_HARNESS_PID || 0)
if (harness) setInterval(() => { try { process.kill(harness, 0) } catch { process.exit(0) } }, 300)
const connect = net.Socket.prototype.connect
net.Socket.prototype.connect = function (...a) {
  const o = Array.isArray(a[0]) ? a[0][0] : typeof a[0] === 'object' && a[0] !== null ? a[0] : { port: a[0], host: a[1] }
  if ([7700, 3410, 8310].includes(Number(o.port))) {
    const line = `FENCE: dial ${o.host || '127.0.0.1'}:${o.port}`
    console.log(line)
    if (process.env.FAKE_FENCE_LOG) fs.appendFileSync(process.env.FAKE_FENCE_LOG, `${line} ${process.env.FAKE_RUN}\n`)
    process.exit(97)
  }
  return connect.apply(this, a)
}
if (process.env.FAKE_DIAL) net.connect(Number(process.env.FAKE_DIAL), '127.0.0.2')
const at = process.argv.indexOf('--port')
const port = at < 0 ? 0 : Number(process.argv[at + 1])
const listen = (p, then) => { const s = net.createServer(); s.on('error', (e) => { console.log(`Error: listen ${e.code}: address already in use 127.0.0.1:${p}`); process.exit(1) }); s.listen(p, '127.0.0.1', then) }
// started without --port (a second DSH, from a shell): it serves a port of its own and waits, as a server would
if (at < 0) listen(0, () => {})
else listen(port, () => listen(Number(process.env.UNFOLD_BRIDGE_PORT), () => {
  const dir = process.env.UNFOLD_BRIDGE_STATE_DIR
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(`${dir}/endpoint.tmp`, JSON.stringify({ v: 1, port: Number(process.env.UNFOLD_BRIDGE_PORT), key: 'fake-bridge-key-0000', pid: process.pid, started: 0 }), { mode: 0o600 })
  fs.renameSync(`${dir}/endpoint.tmp`, `${dir}/endpoint.json`)
  const ch = globalThis[Symbol.for('unfold.channel')] || {}
  const names = ['UNFOLD_AGENTD_OBSERVER', 'UNFOLD_CONTROL_SOCKET', 'UNFOLD_CONTROL_TOKEN_FILE']
  console.log(`channel: ${JSON.stringify({ env: names.filter((n) => n in process.env), token: (ch.token || '').length, socket: !!ch.socket, observer: !!ch.observerFile, enumerable: !!(Object.getOwnPropertyDescriptor(globalThis, Symbol.for('unfold.channel')) || {}).enumerable, frozen: Object.isFrozen(ch) })}`)
  console.log(`deepseek: ${JSON.stringify({ set: 'DEEPSEEK_API_KEY' in process.env, length: (process.env.DEEPSEEK_API_KEY || '').length, argv: process.argv.some((a) => a.includes(process.env.DEEPSEEK_API_KEY || '\0')) })}`)
  if (process.env.FAKE_ECHO_KEY) console.log(`echoed: DEEPSEEK_API_KEY=${process.env.DEEPSEEK_API_KEY}`)
  console.log(`dsh web: http://127.0.0.1:${port}/?token=fake${process.pid}`)
  console.log(`a later line with token=stray${process.pid}`)
  if (process.env.FAKE_CRASH && fs.existsSync(process.env.FAKE_CRASH)) process.exit(3)
}))
setInterval(() => {}, 1 << 30)
"#;

    const PROBES_COMPLETE: &str = "fixture probes complete";

    fn bin_with_probe(probe: &str) -> String {

        let (imports, body): (Vec<_>, Vec<_>) = probe.lines().partition(|l| l.starts_with("import "));
        let hook = format!("{}\nconsole.log('fixture probes complete')\n", body.join("\n"));
        let entry = BIN_JS.replace("  if (process.env.FAKE_CRASH", &format!("{hook}  if (process.env.FAKE_CRASH"));
        format!("{}\n{entry}", imports.join("\n"))
    }

    fn wait_for_probes(instance: &Path) {
        assert!(eventually(Duration::from_secs(30), || {
            std::fs::read_to_string(dsh_dir(instance).join("dsh.log")).unwrap_or_default().lines().any(|l| l == PROBES_COMPLETE)
        }), "fixture probes did not finish within their separate 30 s budget");
    }

    fn pids_file() -> PathBuf {
        crate::child_fixture::paths().dsh_pids.clone()
    }

    fn fence_file() -> PathBuf {
        crate::child_fixture::paths().dsh_fence.clone()
    }

    fn fixture_env(run: &str) -> Vec<(String, String)> {
        vec![("FAKE_PID_RECORD".into(), pids_file().display().to_string()), ("FAKE_RUN".into(), run.into()), ("FAKE_HARNESS_PID".into(), std::process::id().to_string()), ("FAKE_FENCE_LOG".into(), fence_file().display().to_string())]
    }

    fn recorded_pids(run: &str) -> Vec<u32> {
        std::fs::read_to_string(pids_file()).unwrap_or_default().lines().filter_map(|l| {
            let mut w = l.splitn(3, ' ');
            let pid = w.next()?.parse().ok()?;
            (w.next()? != "" && w.next()? == run).then_some(pid)
        }).collect()
    }

    fn eventually(d: Duration, ok: impl Fn() -> bool) -> bool {
        let end = Instant::now() + d;
        while !ok() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(25));
        }
        ok()
    }

    fn within<T: Send + 'static>(limit: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit).ok()
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    fn fake(tag: &str) -> (PathBuf, ChildCfg) {
        let root = std::env::temp_dir().join(format!("dsh-child-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pack = root.join("pack");
        std::fs::create_dir_all(pack.join("bin")).unwrap();
        std::fs::create_dir_all(pack.join(DSH_BIN).parent().unwrap()).unwrap();

        std::os::unix::fs::symlink(node(), pack.join("bin/node")).unwrap();
        std::fs::write(pack.join("launch.sh"), PACK_LAUNCH).unwrap();
        std::fs::write(pack.join("preload.mjs"), PACK_PRELOAD).unwrap();
        std::fs::write(pack.join("install.mjs"), "console.log(`install: ${JSON.stringify(Object.keys(process.env).filter((k) => k.startsWith('UNFOLD_') && k !== 'UNFOLD_BRIDGE_PORT' && k !== 'UNFOLD_BRIDGE_STATE_DIR'))}`)\n").unwrap();
        std::fs::write(pack.join(DSH_BIN), BIN_JS).unwrap();
        let mut c = cfg(&root);
        c.node = node_of(&pack);
        c.instance_id = format!("i-{tag}-{}", std::process::id());
        c.base_env = vec![("PATH".into(), "/usr/bin:/bin".into()), ("FAKE_CRASH".into(), root.join("crash").display().to_string()), ("NODE_OPTIONS".into(), "--inspect=127.0.0.1:0".into())];
        c.base_env.extend(fixture_env(&root.display().to_string()));
        c.ports.web = free_port();
        c.ports.bridge = free_port();
        (root, c)
    }

    fn logged(root: &Path) -> (Log, impl Fn() -> String) {
        let dir = root.join("app");
        let log = Log::open_as(&crate::WriteMode::Dev, &dir);
        (log, move || std::fs::read_to_string(dir.join("app.log")).unwrap_or_default())
    }

    fn alive(pid: u32) -> bool {
        crate::nemo::proc_info(pid).is_some()
    }

    struct Leftover(u32, u64);
    impl Leftover {
        fn of(pid: u32) -> Leftover {
            Leftover(pid, crate::nemo::proc_info(pid).map_or(0, |i| i.start_us))
        }
    }
    impl Drop for Leftover {
        fn drop(&mut self) {
            if crate::nemo::proc_info(self.0).is_some_and(|i| i.start_us == self.1) {
                unsafe { libc::killpg(self.0 as i32, libc::SIGKILL) };
            }
        }
    }

    #[test]
    fn dsh_child_starts_records_after_the_exec_reads_only_its_own_endpoint_and_stops() {
        let (root, c) = fake("start");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);

        let kids: Value = serde_json::from_str(&std::fs::read_to_string(dsh_dir(&c.instance_root).join("children.json")).unwrap()).unwrap();
        let node = c.node.canonicalize().unwrap().display().to_string();
        assert_eq!((kids[0]["pid"].as_u64(), kids[0]["pgid"].as_u64(), kids[0]["command"].as_str()), (Some(d.pid as u64), Some(d.pid as u64), Some(node.as_str())));
        assert_eq!(kids[0]["start_time"].as_u64(), crate::nemo::proc_info(d.pid).map(|i| i.start_us));

        let argv = String::from_utf8(Command::new("/bin/ps").args(["-ww", "-o", "args=", "-p", &d.pid.to_string()]).output().unwrap().stdout).unwrap();
        let pack = c.pack.canonicalize().unwrap().display().to_string();
        assert!(argv.contains(&format!(" --disable-sigusr1 --import {pack}/preload.mjs {pack}/{DSH_BIN} --profile omni")), "{argv}");
        let mut ports = crate::dsh::listening_ports(d.pid);
        ports.sort();
        assert_eq!(ports, { let mut p = vec![c.ports.web, c.ports.bridge]; p.sort(); p }, "its two ports only: no inspector");

        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        assert!(text.lines().any(|l| l == "install: []"), "the profile's install got none of them: {text}");
        let line = text.lines().find_map(|l| l.strip_prefix("channel: ")).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(serde_json::from_str::<Value>(line).unwrap(), json!({"env": [], "token": 64, "socket": true, "observer": true, "enumerable": false, "frozen": true}));
        assert!(!(c.sockdir)(&c.instance_id).unwrap().join("control.token").exists(), "the token file is unlinked");

        unsafe { libc::kill(d.pid as i32, libc::SIGUSR1) };
        let end = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < end && crate::dsh::listening_ports(d.pid).len() == 2 {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(alive(d.pid), "it still runs");
        assert_eq!(crate::dsh::listening_ports(d.pid).len(), 2, "no inspector after SIGUSR1: {:?}", crate::dsh::listening_ports(d.pid));

        let own = crate::dsh::Source { path: d.endpoint.clone(), owner: crate::dsh::Owner::Child { pid: d.pid, start_us: d.start_us, port: d.port } };
        assert!(crate::dsh::read_from(&own).is_ok());
        assert_eq!(crate::dsh::read_from(&crate::dsh::Source { owner: crate::dsh::Owner::Child { pid: d.pid, start_us: d.start_us, port: d.port + 1 }, ..own.clone() }), Err("not the registered bridge port"));
        assert_eq!(crate::dsh::read_from(&crate::dsh::Source { owner: crate::dsh::Owner::Child { pid: std::process::id(), start_us: d.start_us, port: d.port }, ..own.clone() }), Err("not Unfold's own DSH"));

        assert!(admitted_link(&d).is_some_and(|l| l.starts_with(&format!("http://127.0.0.1:{}/?token=fake", c.ports.web))));
        stop(&d, &c.instance_root, &owned, &log);
        assert!(!alive(d.pid), "stopped");
        assert!(!d.endpoint.exists() && !dsh_dir(&c.instance_root).join("children.json").exists(), "endpoint and record gone");
        assert!(admitted_link(&d).is_none());
        for p in [c.ports.web, c.ports.bridge] {
            assert!(std::net::TcpListener::bind(("127.0.0.1", p)).is_ok(), "port {p} free");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_its_log_never_holds_a_token_across_3_restarts() {
        let (root, c) = fake("log");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        for _ in 0..3 {
            let d = start(&c, &owned, &log, &|| true).unwrap();
            stop(&d, &c.instance_root, &owned, &log);
        }
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        assert_eq!(text.matches("dsh web: (token withheld)").count(), 3, "{text}");
        assert!(!text.contains("token=") && !text.contains("fake") && !text.contains("stray"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_taken_web_port_says_use_another_port_and_leaves_that_process_alone() {
        let (root, c) = fake("port");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let other = std::net::TcpListener::bind(("127.0.0.1", c.ports.web)).unwrap();
        assert_eq!(start(&c, &owned, &log, &|| true), Err(StartError::PortTaken));
        assert!(owned.lock().unwrap().recorded.is_empty(), "the child that failed is stopped and forgotten");
        assert!(other.local_addr().is_ok() && std::net::TcpStream::connect(("127.0.0.1", c.ports.web)).is_ok(), "the other process keeps its port");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn left_behind(c: &ChildCfg) -> u32 {
        let dsh = dsh_dir(&c.instance_root);
        std::fs::create_dir_all(dsh.join("bridge")).unwrap();
        let out = Command::new("/usr/bin/perl")
            .args(["-e", "my $p = fork; if ($p) { print \"$p\\n\"; exit 0 } setpgrp(0, 0); open(STDOUT, '>', '/dev/null'); exec @ARGV"])
            .arg(&c.node).arg(c.pack.join(DSH_BIN)).args(["--port", "0"])

            .env_clear().envs(env(c, &[])).env("UNFOLD_BRIDGE_PORT", "0").stderr(Stdio::null()).output().unwrap();
        let pid: u32 = String::from_utf8(out.stdout).unwrap().trim().parse().unwrap();
        let node = c.node.canonicalize().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while crate::pid_path(pid as i32).as_ref() != Some(&node) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        pid
    }

    #[test]
    fn dsh_child_after_a_crash_only_a_left_child_matching_pid_and_start_time_and_leading_its_group_is_stopped() {
        let (root, c) = fake("left");
        let (log, lines) = logged(&root);
        let children = dsh_dir(&c.instance_root).join("children.json");
        let gone_owner = json!({"pid": 1, "start_time": 1});
        let rec = |pid: u32, pgid: u32, start: u64, command: &str| json!({"name": "dsh", "pid": pid, "pgid": pgid, "start_time": start, "command": command, "owner": gone_owner});
        let (a, b, d) = (left_behind(&c), left_behind(&c), left_behind(&c));
        let node = c.node.canonicalize().unwrap().display().to_string();
        let node = node.as_str();
        let start_of = |p: u32| crate::nemo::proc_info(p).unwrap().start_us;

        std::fs::write(&children, json!([rec(a, a, start_of(a), "/bin/bash"), rec(b, b, start_of(b) + 1, node), rec(d, d + 1, start_of(d), node)]).to_string()).unwrap();
        stop_leftovers(&children, &log);
        let end = Instant::now() + Duration::from_secs(5);
        while alive(a) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(a), "the matching one is stopped: {}", lines());
        assert!(alive(b) && alive(d), "the others are left alone");
        assert!(!children.exists(), "the record is used once");

        std::fs::write(&children, json!([{"name": "dsh", "pid": b, "pgid": b, "start_time": start_of(b), "command": node, "owner": {"pid": std::process::id(), "start_time": start_of(std::process::id())}}]).to_string()).unwrap();
        stop_leftovers(&children, &log);
        assert!(alive(b), "its Unfold still runs");
        for p in [b, d] {
            unsafe { libc::killpg(p as i32, libc::SIGKILL) };
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_while_off_no_endpoint_is_read_and_the_old_shared_file_is_never_opened() {

        let state = sock_root("shared-state");
        std::env::set_var("XDG_STATE_HOME", &state);
        let planted = state.join("unfold-bridge/endpoint.json");
        std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
        let c = std::ffi::CString::new(planted.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let read = within(Duration::from_secs(3), || {
            let src = crate::dsh::source();
            let r = crate::dsh::read_from(&src);
            let on = crate::settings::Settings { use_own_dsh: true, use_dsh_watch: true, ..Default::default() };
            let v = crate::dsh::view(&on, &src);
            let ask = crate::dsh::Ask { automagik: true, answer: crate::settings::ANSWER_DEEPSEEK, watch: true };
            let p = crate::dsh::prepare(&[], &src, ask, &Log::open_as(&crate::WriteMode::Dev, &std::env::temp_dir().join(format!("dsh-ch-shared-log-{}", std::process::id()))));
            (r, v["available"].clone(), p.dsh.is_some())
        });
        assert_eq!(read, Some((Err("Unfold's own DSH is off"), json!(false), false)), "the readers ended without opening the planted file");

        let w = unsafe { libc::open(c.as_ptr(), libc::O_WRONLY | libc::O_NONBLOCK) };
        assert_eq!((w, std::io::Error::last_os_error().raw_os_error()), (-1, Some(libc::ENXIO)), "a reader has the planted file open");
        let _ = std::fs::remove_dir_all(&state);
        let src = source();
        assert_eq!(src.owner, crate::dsh::Owner::Off);
        assert_eq!(crate::dsh::read_from(&src), Err("Unfold's own DSH is off"));
        assert_eq!(crate::dsh::source(), src, "this build reads only Unfold Automagik's endpoint");
        assert!(!src.path.display().to_string().contains(".local/state"));
    }

    #[test]
    fn dsh_child_the_carried_key_reaches_only_the_child_s_environment() {
        let fixture_secret_0 = format!("fixture-{}-{}-0", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        let (root, mut c) = fake("key");
        let key = fixture_secret_0.as_str();
        c.deepseek_key = Some(CarriedKey(key.into()));

        c.base_env.push(("FAKE_ECHO_KEY".into(), "1".into()));
        let (log, lines) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        let line = text.lines().find_map(|l| l.strip_prefix("deepseek: ")).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(serde_json::from_str::<Value>(line).unwrap(), json!({"set": true, "length": key.len(), "argv": false}), "in its environment, not its arguments");
        assert!(text.contains("echoed: DEEPSEEK_API_KEY=<withheld>"), "{text}");
        let argv = String::from_utf8(Command::new("/bin/ps").args(["-ww", "-o", "args=", "-p", &d.pid.to_string()]).output().unwrap().stdout).unwrap();
        assert!(!argv.contains(key), "{argv}");
        let kids = std::fs::read_to_string(dsh_dir(&c.instance_root).join("children.json")).unwrap();

        let mut files = vec![root.clone()];
        let mut holding = Vec::new();
        while let Some(f) = files.pop() {
            match std::fs::symlink_metadata(&f) {
                Ok(m) if m.is_dir() => files.extend(std::fs::read_dir(&f).unwrap().filter_map(|e| e.ok().map(|e| e.path()))),
                Ok(m) if m.is_file() => {
                    if std::fs::read(&f).is_ok_and(|b| b.windows(key.len()).any(|w| w == key.as_bytes())) {
                        holding.push(f);
                    }
                }
                _ => {}
            }
        }
        stop(&d, &c.instance_root, &owned, &log);
        assert!(holding.is_empty() && !kids.contains(key) && !lines().contains(key), "files holding the key: {holding:?}");

        c.deepseek_key = None;
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        let last = text.lines().filter_map(|l| l.strip_prefix("deepseek: ")).last().unwrap();
        assert_eq!(serde_json::from_str::<Value>(last).unwrap()["set"], false);
        stop(&d, &c.instance_root, &owned, &log);
        let _ = std::fs::remove_dir_all(&root);
    }

    struct NoChild;
    impl crate::dsh_control::Children for NoChild {
        fn instance_child(&self) -> Option<crate::dsh_control::Recorded> {
            None
        }
    }

    #[test]
    fn dsh_child_the_channel_publishes_agentds_address_only_while_the_lease_is_held_and_serves_one_endpoint_at_a_time() {
        let root = std::env::temp_dir().join(format!("dsh-ch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&root).unwrap();
        let (log, _) = logged(&root);
        let ch: &'static Channel = Box::leak(Box::new(Channel::new(crate::dsh_control::serve)));
        let serve = || ch.serve(&root, root.join("journal.jsonl"), std::sync::Arc::new(NoChild), std::sync::Arc::new(|_: &str| Err("no chat".to_string())), &log).unwrap();
        let (observer, socket, token) = (root.join("observer"), root.join("control.sock"), root.join("control.token"));
        let addr: std::net::SocketAddr = "127.0.0.1:45678".parse().unwrap();

        let env = serve();
        assert_eq!(env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["UNFOLD_AGENTD_OBSERVER", "UNFOLD_CONTROL_SOCKET", "UNFOLD_CONTROL_TOKEN_FILE"]);
        assert!(socket.exists() && token.exists() && !observer.exists());

        ch.held(addr, Some("sess-1".into()), Some(&log));
        assert!(eventually(Duration::from_secs(2), || std::fs::read_to_string(&observer).ok().as_deref() == Some("127.0.0.1:45678\n")), "published");
        ch.wait(&|| true);
        ch.gone();
        assert!(!observer.exists(), "withdrawn before the lease goes");

        ch.held(addr, Some("sess-1".into()), Some(&log));
        let first = std::fs::read_to_string(&token).unwrap();
        serve();
        assert_ne!(std::fs::read_to_string(&token).unwrap(), first, "a new token at every serve");
        assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok(), "the new endpoint serves: the old one's stop didn't take its socket");
        assert_eq!(std::fs::read_to_string(&observer).unwrap(), "127.0.0.1:45678\n");

        ch.stop();
        assert!(!socket.exists() && !token.exists() && !observer.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_every_dshenv_variable_has_a_row() {
        for k in DSHENV_AT_PIN {
            assert!(ROWS.iter().any(|(n, _)| n == k), "{k} has no row");
        }
        let names: Vec<_> = ROWS.iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), names.iter().collect::<std::collections::BTreeSet<_>>().len(), "one row per variable");

        let carried: Vec<&str> = ROWS.iter().filter(|(_, r)| matches!(r, Row::Carried(_))).map(|(n, _)| *n).collect();
        assert_eq!(carried, ["DEEPSEEK_API_KEY"]);
        assert!(!DSHENV_AT_PIN.contains(&"DEEPSEEK_API_KEY"), "not Automagik's dshEnv: Unfold's addition");
    }

    #[test]
    fn fixture_fix_dsh_ready_precedes_blocked_probe() {
        let (root, mut c) = fake("probe-barrier");
        let release = root.join("probe-release");
        let entered = root.join("probe-entered");
        c.base_env.push(("FAKE_PROBE_RELEASE".into(), release.display().to_string()));
        c.base_env.push(("FAKE_PROBE_ENTERED".into(), entered.display().to_string()));
        let probe = "import { existsSync as probeExists, writeFileSync as probeWrite } from 'node:fs'\nprobeWrite(process.env.FAKE_PROBE_ENTERED, '')\nwhile (!probeExists(process.env.FAKE_PROBE_RELEASE)) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50)\n";
        std::fs::write(c.pack.join(DSH_BIN), bin_with_probe(probe)).unwrap();
        let (log, _) = logged(&root);
        let log = Arc::new(log);
        let c = Arc::new(c);
        let owned: Shared = Arc::default();
        let (tx, rx) = mpsc::channel();
        let (cc, oo, ll) = (c.clone(), owned.clone(), log.clone());
        let worker = std::thread::spawn(move || { let _ = tx.send(start(&cc, &oo, &ll, &|| true)); });
        let reached = eventually(Duration::from_secs(20), || entered.is_file());
        let early = rx.recv_timeout(Duration::from_secs(2));
        let ready_before_release = matches!(&early, Ok(Ok(_)));

        std::fs::write(&release, "").unwrap();
        let result = match early { Ok(r) => Some(r), Err(_) => rx.recv_timeout(Duration::from_secs(15)).ok() };
        worker.join().unwrap();
        if let Some(Ok(d)) = &result {
            wait_for_probes(&c.instance_root);
            stop(d, &c.instance_root, &owned, &log);
        }
        assert!(reached, "fake never reached the controlled probe barrier");
        assert!(ready_before_release, "readiness waited for a blocked fixture probe");
    }

    #[test]
    fn fixture_fix_dsh_ledgers_are_invocation_local() {
        let pids = pids_file();
        let fence = fence_file();
        assert_ne!(pids.parent(), Some(std::env::temp_dir().as_path()), "PID ledger is shared across invocations");
        assert_ne!(fence.parent(), Some(std::env::temp_dir().as_path()), "fence is shared across invocations");
        assert_eq!(pids.parent(), fence.parent());
        assert_ne!(pids, fence);
    }

    fn cfg(root: &Path) -> ChildCfg {
        ChildCfg {
            pack: root.join("pack"),
            node: PathBuf::from("/bin/bash"),
            instance_root: root.join("instances/i1"),
            instance_id: "i1".into(),
            ports: Ports { web: 41700, bridge: 41701, omni_api: 41702, omni_db: 41703, omni_nats: 41704 },
            home: root.join("home"),
            base_env: [("PATH", "/usr/bin:/bin"), ("XDG_STATE_HOME", "/shared"), ("AUTOMAGIK_PRIVACY_FILE", "/p"), ("OMNI_CLIENT_ID", "omni-desktop"), ("RTH_APP_DATA", "/d"), ("UNFOLD_CONTROL_SOCKET", "/s"), ("NODE_OPTIONS", "--inspect"), ("OMNI_SKIP_POSTINSTALL_PIN", "0"), ("LANG", "en_US.UTF-8"), ("UNFOLD_HOST_JOURNAL", "/j"), ("BUN_INSTALL", "/home/.bun")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),

            sockdir: |id| {
                let d = std::env::temp_dir().join(format!("dsh-child-sock-{id}"));
                std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&d).map_err(|e| e.to_string())?;
                Ok(d)
            },
            serve: |dir, _| {
                std::fs::write(dir.join("control.token"), "a".repeat(64)).map_err(|e| e.to_string())?;
                Ok(["observer", "control.sock", "control.token"].iter().zip(["UNFOLD_AGENTD_OBSERVER", "UNFOLD_CONTROL_SOCKET", "UNFOLD_CONTROL_TOKEN_FILE"]).map(|(f, k)| (k.to_string(), dir.join(f).display().to_string())).collect())
            },
            ready_within: Duration::from_secs(10),
            deepseek_key: None,
        }
    }

    #[test]
    fn dsh_child_a_wish_lands_in_the_instance_never_in_the_persons_genie_folder() {
        let (root, c) = fake("wishes");

        let probe = "import { mkdirSync as mkd, writeFileSync as wf } from 'node:fs'\nimport { homedir } from 'node:os'\nimport { join as pj } from 'node:path'\nconst wishes = process.env.GENIE_WISHES_DIR ?? pj(homedir(), '.genie', 'wishes')\nmkd(wishes, { recursive: true })\nwf(pj(wishes, 'a-wish.md'), 'a wish\\n')\nconsole.log(`wishes: ${wishes}`)\n";
        std::fs::write(c.pack.join(DSH_BIN), bin_with_probe(probe)).unwrap();
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        wait_for_probes(&c.instance_root);
        let wishes = c.instance_root.join("genie/wishes");
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        assert!(text.lines().any(|l| l == format!("wishes: {}", wishes.display())), "the child's GENIE_WISHES_DIR is the instance's: {text}");
        assert!(wishes.join("a-wish.md").is_file(), "the wish is in the instance");
        assert!(!c.home.join(".genie").exists(), "nothing in the person's ~/.genie");
        for d in [c.instance_root.join("genie"), wishes.clone()] {
            assert_eq!(std::fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o700, "{}", d.display());
        }
        stop(&d, &c.instance_root, &owned, &log);

        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::remove_dir_all(c.instance_root.join("genie")).unwrap();
        std::os::unix::fs::symlink(&outside, c.instance_root.join("genie")).unwrap();
        let r = start(&c, &owned, &log, &|| true);
        if let Ok(d) = &r {
            stop(d, &c.instance_root, &owned, &log);
        }
        assert!(matches!(&r, Err(StartError::Other(e)) if e.contains("is a link")), "a linked genie folder fails the start");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_the_packs_genie_keeps_its_home_in_the_instance_never_in_the_persons() {
        let (root, c) = fake("genie-home");

        std::fs::create_dir_all(c.pack.join("genie/bin")).unwrap();
        std::fs::write(c.pack.join("genie/bin/genie"), "#!/bin/sh\nh=${GENIE_HOME:-$HOME/.genie}\nmkdir -p \"$h\"\necho ran > \"$h/state\"\necho \"genie-home: $h\"\n").unwrap();
        std::fs::set_permissions(c.pack.join("genie/bin/genie"), std::fs::Permissions::from_mode(0o755)).unwrap();

        let probe = "import { execFileSync as run } from 'node:child_process'\nconsole.log(run('/bin/sh', ['-c', 'genie'], { encoding: 'utf8' }).trim())\n";
        std::fs::write(c.pack.join(DSH_BIN), bin_with_probe(probe)).unwrap();
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        wait_for_probes(&c.instance_root);
        let home = c.instance_root.join("genie");
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        assert!(text.lines().any(|l| l == format!("genie-home: {}", home.display())), "the child's GENIE_HOME is the instance's: {text}");
        assert!(home.join("state").is_file(), "genie's state is in the instance");
        assert!(!c.home.join(".genie").exists(), "nothing in the person's ~/.genie");
        stop(&d, &c.instance_root, &owned, &log);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_shim_written_by_the_packs_install_resolves_on_the_dsh_childs_path() {
        let (root, c) = fake("shim");

        std::fs::write(c.pack.join("install.mjs"), "import { chmodSync, mkdirSync, writeFileSync } from 'node:fs'\nimport { dirname, join } from 'node:path'\nconst dir = join(dirname(process.env.DSH_HOME), 'bin')\nmkdirSync(dir, { recursive: true })\nwriteFileSync(join(dir, 'unfold-shim-probe'), '#!/bin/sh\\necho probe-ran\\n')\nchmodSync(join(dir, 'unfold-shim-probe'), 0o755)\n").unwrap();
        let probe = "import { execFileSync } from 'node:child_process'\nlet found = 'none'\ntry { found = execFileSync('/bin/sh', ['-c', 'command -v unfold-shim-probe && unfold-shim-probe'], { encoding: 'utf8' }).trim().split('\\n').join(' ') } catch {}\nconsole.log(`shim: ${found}`)\n";
        std::fs::write(c.pack.join(DSH_BIN), bin_with_probe(probe)).unwrap();
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        wait_for_probes(&c.instance_root);
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        let shim = dsh_dir(&c.instance_root).join("bin/unfold-shim-probe");
        assert!(text.lines().any(|l| l == format!("shim: {} probe-ran", shim.display())), "the install's shim is the one DSH's PATH finds: {text}");
        stop(&d, &c.instance_root, &owned, &log);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_the_packs_commands_come_first_so_the_machines_genie_omni_and_dsh_are_never_called() {
        let (root, mut c) = fake("closed");

        let (machine, calls) = (root.join("machine-bin"), root.join("calls.log"));
        machine_commands(&machine, &calls);

        pack_commands(&c.pack);
        for (k, v) in c.base_env.iter_mut() {
            if k == "PATH" {
                *v = format!("{}:/usr/bin:/bin", machine.display());
            }
        }

        let probe = "import { execFileSync } from 'node:child_process'\nconst sh = (c) => { try { return execFileSync('/bin/sh', ['-c', c], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() } catch (e) { return `exit ${e.status}` } }\nfor (const n of ['genie', 'omni', 'pnpm', 'node']) console.log(`ran ${n}: ${sh(`${n} --version`)}`)\nconsole.log(`found dsh: ${sh('command -v dsh')}`)\n";
        std::fs::write(c.pack.join(DSH_BIN), bin_with_probe(probe)).unwrap();
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        wait_for_probes(&c.instance_root);
        let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
        let ran: Vec<&str> = text.lines().filter(|l| l.starts_with("ran ") || l.starts_with("found ")).collect();
        let dsh = dsh_dir(&c.instance_root).join("closed-bin/dsh");
        assert_eq!(ran, ["ran genie: pack-genie", "ran omni: pack-omni --version", "ran pnpm: machine-pnpm", "ran node: machine-node", &format!("found dsh: {}", dsh.display())], "the pack's genie and omni, the person's pnpm and node: {text}");
        assert!(std::fs::read_to_string(&dsh).unwrap().contains(&format!("'{}'", c.pack.join(DSH_BIN).display())), "dsh is the pack's");
        assert_eq!(std::fs::read_to_string(&calls).unwrap_or_default(), "pnpm\nnode\n", "of the machine's, only pnpm and node were called");
        stop(&d, &c.instance_root, &owned, &log);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn pack_commands(pack: &Path) {
        for (file, text) in [("genie/bin/genie", "#!/bin/sh\necho pack-genie\n"), ("omni/bin/bun", "#!/bin/sh\nshift\necho pack-omni \"$@\"\n")] {
            std::fs::create_dir_all(pack.join(file).parent().unwrap()).unwrap();
            std::fs::write(pack.join(file), text).unwrap();
            std::fs::set_permissions(pack.join(file), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let cli = pack.join("omni").join(crate::omni_child::CLI);
        std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
        std::fs::write(&cli, "fake\n").unwrap();
    }

    fn machine_commands(dir: &Path, calls: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for n in ["genie", "omni", "dsh", "pnpm", "node"] {
            std::fs::write(dir.join(n), format!("#!/bin/sh\necho {n} >> '{}'\necho machine-{n}\n", calls.display())).unwrap();
            std::fs::set_permissions(dir.join(n), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn dsh_child_closed_bin_runs_a_name_the_pack_ships_and_closes_the_rest() {
        let root = std::env::temp_dir().join(format!("dsh-child-closed-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (bare, full, inst) = (root.join("bare"), root.join("full"), root.join("instances/i1"));
        std::fs::create_dir_all(&bare).unwrap();
        pack_commands(&full);
        for f in ["bin/node", DSH_BIN] {
            std::fs::create_dir_all(full.join(f).parent().unwrap()).unwrap();
            std::fs::write(full.join(f), "fake\n").unwrap();
        }
        std::fs::set_permissions(full.join("bin/node"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let dir = dsh_dir(&inst).join("closed-bin");
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).ok();
        let closed = |n: &str| Some(format!("#!/bin/sh\necho \"{n}: not available to Unfold's own DSH\" >&2\nexit 127\n"));

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pnpm"), closed("pnpm").unwrap()).unwrap();
        std::fs::write(dir.join("node"), "#!/bin/sh\necho planted\n").unwrap();

        write_closed(&bare, &inst).unwrap();
        for n in CLOSED {
            assert_eq!(read(n), closed(n), "{n}");
            assert_eq!(std::fs::metadata(dir.join(n)).unwrap().permissions().mode() & 0o777, 0o700, "{n}");
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["dsh", "genie", "omni"], "only the closed names");
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);

        write_closed(&full, &inst).unwrap();
        let q = |p: PathBuf| format!("'{}'", p.display());
        assert_eq!(read("genie"), Some(format!("#!/bin/sh\nexec {} \"$@\"\n", q(full.join("genie/bin/genie")))), "the pack's Genie CLI");
        assert_eq!(read("omni"), Some(format!("#!/bin/sh\nexec {} {} \"$@\"\n", q(full.join("omni/bin/bun")), q(full.join("omni").join(crate::omni_child::CLI)))));
        assert_eq!(read("dsh"), Some(format!("#!/bin/sh\nif [ \"$1\" = plugin ]; then\necho \"dsh plugin: not available to Unfold's own DSH\" >&2\nexit 127\nfi\ncase \"$#:$1\" in\n1:-V|1:--version|1:-h|1:--help) exec {} {} \"$@\"\n;;\nesac\necho \"dsh: Unfold's own DSH is already running; it starts from Unfold.\" >&2\nexit 127\n", q(full.join("bin/node")), q(full.join(DSH_BIN)))));

        for f in ["genie/bin/genie", "omni/bin/bun", "bin/node"] {
            std::fs::set_permissions(full.join(f), std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        write_closed(&full, &inst).unwrap();
        for n in CLOSED {
            assert_eq!(read(n), closed(n), "{n}: the pack's can't run");
        }
        let (machine, calls) = (root.join("machine-bin"), root.join("calls.log"));
        machine_commands(&machine, &calls);
        let path = instance_path(&full, &inst, &root.join("home"), &format!("{}:/usr/bin:/bin", machine.display()));
        let o = Command::new("/bin/sh").args(["-c", "genie --version"]).env_clear().env("PATH", &path).output().unwrap();
        assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned()), (Some(127), String::new()), "closed, not the machine's genie");
        assert_eq!(std::fs::read_to_string(&calls).unwrap_or_default(), "", "of the machine's, nothing was called");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_closed_bin_replaces_a_planted_link_and_refuses_a_linked_folder_never_writing_through_either() {
        let root = std::env::temp_dir().join(format!("dsh-child-closed-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (pack, inst, outside) = (root.join("pack"), root.join("instances/i1"), root.join("outside"));
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let theirs = outside.join("genie");
        std::fs::write(&theirs, "the person's genie\n").unwrap();
        std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o755)).unwrap();
        let dir = dsh_dir(&inst).join("closed-bin");
        std::fs::create_dir_all(&dir).unwrap();

        std::os::unix::fs::symlink(&theirs, dir.join("genie")).unwrap();
        write_closed(&pack, &inst).unwrap();
        assert!(!std::fs::symlink_metadata(dir.join("genie")).unwrap().file_type().is_symlink(), "the link is replaced");
        assert_eq!(std::fs::read_to_string(dir.join("genie")).unwrap(), "#!/bin/sh\necho \"genie: not available to Unfold's own DSH\" >&2\nexit 127\n");
        assert_eq!(std::fs::read_to_string(&theirs).unwrap(), "the person's genie\n", "never written through");
        assert_eq!(std::fs::metadata(&theirs).unwrap().permissions().mode() & 0o777, 0o755);

        std::fs::create_dir(dir.join("planted")).unwrap();
        let e = write_closed(&pack, &inst).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", dir.join("planted").display())), "{e}");

        std::fs::remove_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&outside, &dir).unwrap();
        let e = write_closed(&pack, &inst).unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        let names: Vec<String> = std::fs::read_dir(&outside).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!((names, std::fs::read_to_string(&theirs).unwrap()), (vec!["genie".to_string()], "the person's genie\n".to_string()), "the folder it named is as it was");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_closed_bin_s_dsh_refuses_dsh_plugin_before_dsh_runs_and_a_projects_pnpm_is_the_persons() {
        let root = std::env::temp_dir().join(format!("dsh-child-closed-plugin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (pack, inst, profile) = (root.join("pack"), root.join("instances/i1"), root.join("profile"));
        let (machine, calls) = (root.join("machine-bin"), root.join("calls.log"));
        machine_commands(&machine, &calls);

        std::fs::create_dir_all(pack.join(DSH_BIN).parent().unwrap()).unwrap();
        std::fs::create_dir_all(pack.join("bin")).unwrap();
        std::fs::write(pack.join(DSH_BIN), "fake\n").unwrap();
        std::fs::write(pack.join("bin/node"), format!("#!/bin/sh\necho pack-dsh >> '{}'\nshift\necho pack-dsh \"$@\"\n", calls.display())).unwrap();
        std::fs::set_permissions(pack.join("bin/node"), std::fs::Permissions::from_mode(0o755)).unwrap();

        std::fs::create_dir_all(profile.join("node_modules/a-plugin")).unwrap();
        std::fs::write(profile.join("package.json"), "{\"dependencies\":{\"a-plugin\":\"1.0.0\"}}\n").unwrap();
        std::fs::write(profile.join("node_modules/a-plugin/package.json"), "{\"name\":\"a-plugin\",\"version\":\"1.0.0\"}\n").unwrap();
        let files = |d: &Path| -> Vec<(String, String)> {
            let mut v = Vec::new();
            let mut todo = vec![d.to_path_buf()];
            while let Some(p) = todo.pop() {
                for e in std::fs::read_dir(&p).unwrap() {
                    let e = e.unwrap().path();
                    if e.is_dir() {
                        todo.push(e);
                    } else {
                        v.push((e.display().to_string(), std::fs::read_to_string(&e).unwrap()));
                    }
                }
            }
            v.sort();
            v
        };
        let before = files(&profile);
        write_closed(&pack, &inst).unwrap();
        let path = instance_path(&pack, &inst, &root.join("home"), &format!("{}:/usr/bin:/bin", machine.display()));
        let sh = |c: &str| Command::new("/bin/sh").args(["-c", c]).env_clear().env("PATH", &path).current_dir(&profile).output().unwrap();

        for c in ["dsh plugin --profile cordis add left-pad", "dsh plugin --profile cordis i", "dsh plugin --profile cordis remove a-plugin", "dsh plugin --profile cordis dlx left-pad", "dsh plugin --profile cordis allow-version a-plugin@1.0.0 --dsh-version 0.5.0 --accept-risk"] {
            let o = sh(c);
            assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).into_owned()), (Some(127), "dsh plugin: not available to Unfold's own DSH\n".to_string()), "{c}");
        }
        assert_eq!(std::fs::read_to_string(&calls).unwrap_or_default(), "", "neither DSH nor the person's pnpm ran");
        assert_eq!(files(&profile), before, "nothing in the profile moved");

        for a in ["--version", "-V", "--help", "-h"] {
            let o = sh(&format!("dsh {a}"));
            assert_eq!(String::from_utf8_lossy(&o.stdout), format!("pack-dsh {a}\n"));
        }
        let o = sh("pnpm --version");
        assert_eq!(String::from_utf8_lossy(&o.stdout), "machine-pnpm\n");
        assert_eq!(std::fs::read_to_string(&calls).unwrap(), "pack-dsh\npack-dsh\npack-dsh\npack-dsh\npnpm\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_closed_bin_s_dsh_refuses_every_server_start_and_the_instance_keeps_its_one_dsh() {
        use std::io::Read;
        let (root, c) = fake("closed-serve");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _left = Leftover::of(d.pid);
        let run = root.display().to_string();
        let servers = || std::fs::read_to_string(pids_file()).unwrap_or_default().lines().filter(|l| l.ends_with(&format!(" dsh-fake {run}"))).count();
        assert_eq!(servers(), 1, "the instance's DSH");

        let shell_env = env(&c, &[]);
        let dsh = |a: &str| {
            let mut ch = Command::new("/bin/sh").args(["-c", &format!("exec dsh {a}")]).env_clear().envs(&shell_env).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
            let t = Instant::now();
            loop {
                if let Some(st) = ch.try_wait().unwrap() {
                    let mut e = String::new();
                    ch.stderr.take().unwrap().read_to_string(&mut e).unwrap();
                    return (st.code(), e);
                }
                if t.elapsed() > Duration::from_secs(5) {
                    let _ = ch.kill();
                    let _ = ch.wait();
                    return (None, "still running after 5 s".to_string());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let forms = ["", "serve", "web", "omni --no-open --host 127.0.0.1 --port 41999", "--profile omni", "--profile=omni", "--patch x.yml --profile omni", "--from-default-profile web rescue", "headless 'run the tests'", "--profile omni --dump-config", "--version --profile omni", "-h web"];
        let got: Vec<_> = forms.iter().map(|a| (*a, dsh(a))).collect();

        assert_eq!(servers(), 1, "no second DSH ran: {got:?}");
        for (a, r) in got {
            assert_eq!(r, (Some(127), "dsh: Unfold's own DSH is already running; it starts from Unfold.\n".to_string()), "dsh {a}");
        }
        assert!(alive(d.pid), "the instance's DSH still runs");
        stop(&d, &c.instance_root, &owned, &log);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_closed_bin_it_can_t_write_fails_the_start_and_omni_s() {
        let (root, c) = fake("closed-fail");
        let outside = root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let dir = dsh_dir(&c.instance_root).join("closed-bin");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &dir).unwrap();
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let r = start(&c, &owned, &log, &|| true);
        if let Ok(d) = &r {
            stop(d, &c.instance_root, &owned, &log);
        }
        assert!(matches!(&r, Err(StartError::Other(e)) if e.contains("is a link")), "DSH doesn't start");
        let omni = c.pack.join("omni/runtime/node_modules/@automagik/omni");
        std::fs::create_dir_all(&omni).unwrap();
        std::fs::write(omni.join("package.json"), r#"{"name":"@automagik/omni","version":"2.9.1"}"#).unwrap();
        let o = omni_cfg_with(&c.pack, &c.instance_root, c.ports, Arc::default(), log, c.base_env.clone(), &c.home);
        assert!(o.as_ref().is_err_and(|e| e.contains("is a link")), "nor does Omni");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing was written through the link");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_the_instances_omni_runs_on_the_same_path_so_the_machines_genie_omni_and_dsh_are_never_called() {
        let root = std::env::temp_dir().join(format!("dsh-child-omni-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (log, _) = logged(&root);

        let pack = root.join("pack");
        pack_commands(&pack);
        std::fs::remove_dir_all(pack.join("genie")).unwrap();
        std::fs::write(pack.join("omni/runtime/node_modules/@automagik/omni/package.json"), r#"{"name":"@automagik/omni","version":"2.9.1"}"#).unwrap();
        let (machine, calls) = (root.join("machine-bin"), root.join("calls.log"));
        machine_commands(&machine, &calls);
        let base = vec![("PATH".to_string(), format!("{}:/usr/bin:/bin", machine.display())), ("LANG".to_string(), "en_US.UTF-8".to_string())];
        let inst = root.join("instances/i1");
        let o = omni_cfg_with(&pack, &inst, cfg(&root).ports, Arc::default(), log, base, &root.join("home")).unwrap();

        let out = Command::new("/bin/sh")
            .args(["-c", "for n in genie omni dsh pnpm node; do r=$($n --version 2>/dev/null) || r=\"exit $?\"; echo \"ran $n: $r\"; done"])
            .env_clear()
            .envs(crate::omni_child::process_env(&o))
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), ["ran genie: exit 127", "ran omni: pack-omni --version", "ran dsh: exit 127", "ran pnpm: machine-pnpm", "ran node: machine-node"], "Omni's processes: the pack's omni, genie and dsh closed, the person's pnpm and node");
        assert_eq!(std::fs::read_to_string(&calls).unwrap_or_default(), "pnpm\nnode\n", "of the machine's, only pnpm and node were called");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_env_sets_every_set_row_and_drops_every_unset_one() {
        let c = cfg(Path::new("/r"));
        let channel: Vec<(String, String)> = [("UNFOLD_AGENTD_OBSERVER", "/t/observer"), ("UNFOLD_CONTROL_SOCKET", "/t/control.sock"), ("UNFOLD_CONTROL_TOKEN_FILE", "/t/control.token")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let e = env(&c, &channel);
        for (k, row) in ROWS {
            match row {
                Row::Set | Row::Shared(_) => assert!(e.contains_key(*k), "{k} missing"),
                Row::Unset(_) | Row::Carried(_) => assert!(!e.contains_key(*k), "{k} should be unset (no key carried)"),
            }
        }
        assert!(!e.keys().any(|k| k.starts_with("RTH_")), "the app's own variables stay out");
        assert_eq!(e["LANG"], "en_US.UTF-8", "the rest of the base is kept");
        assert_eq!(e["UNFOLD_BRIDGE_STATE_DIR"], "/r/instances/i1/dsh/bridge");
        assert_eq!(e["UNFOLD_BRIDGE_PORT"], "41701");
        assert_eq!(e["OMNI_CONFIG_DIR"], "/r/instances/i1/omni/sandbox/config");
        assert_eq!((e["OMNI_TELEMETRY"].as_str(), e["OMNI_SKIP_POSTINSTALL_PIN"].as_str()), ("false", "1"), "no Omni CLI start pins the person's own Omni");
        assert_eq!((e["UNFOLD_CONTROL_SOCKET"].as_str(), e["UNFOLD_AGENTD_OBSERVER"].as_str()), ("/t/control.sock", "/t/observer"), "the channel's paths, not the base's");

        let base: BTreeMap<String, String> = c.base_env.iter().cloned().collect();
        let changed: Vec<&str> = e.iter().filter(|(k, v)| base.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
        let set: Vec<&str> = ROWS.iter().filter(|(k, r)| *r == Row::Set && base.get(*k).map(String::as_str) != Some(e[*k].as_str())).map(|(k, _)| *k).collect();
        assert_eq!(changed.iter().copied().collect::<std::collections::BTreeSet<_>>(), set.iter().copied().chain(["HOME"]).collect(), "only the set rows (and HOME) are changed or added");
        let gone: Vec<&str> = base.keys().filter(|k| !e.contains_key(*k)).map(String::as_str).collect();
        assert_eq!(gone, ["AUTOMAGIK_PRIVACY_FILE", "NODE_OPTIONS", "OMNI_CLIENT_ID", "RTH_APP_DATA", "UNFOLD_HOST_JOURNAL", "XDG_STATE_HOME"]);

        assert_eq!((e["BUN_INSTALL"].as_str(), e["DSH_AGENTS_HOME"].as_str()), ("/r/instances/i1/omni/bun", "/r/instances/i1/dsh/agents"));
        assert_eq!(e["PATH"], "/r/pack/genie/bin:/r/instances/i1/dsh/bin:/r/instances/i1/dsh/closed-bin:/usr/bin:/bin:/r/home/.local/bin:/r/home/.claude/local:/opt/homebrew/bin:/usr/local/bin", "the pack's commands first");
        assert!(e.values().all(|v| !v.contains("/.local/state")), "no shared state path");

        let mut c = cfg(Path::new("/r"));
        c.base_env.push(("DEEPSEEK_API_KEY".into(), "sk-from-the-app-env".into()));
        let base: BTreeMap<String, String> = c.base_env.iter().cloned().collect();
        let e = env(&c, &channel);
        assert!(!e.contains_key("DEEPSEEK_API_KEY"), "no key in Unfold: none in the child, whatever the app's environment had");
        let gone: Vec<&str> = base.keys().filter(|k| !e.contains_key(*k)).map(String::as_str).collect();
        assert_eq!(gone, ["AUTOMAGIK_PRIVACY_FILE", "DEEPSEEK_API_KEY", "NODE_OPTIONS", "OMNI_CLIENT_ID", "RTH_APP_DATA", "UNFOLD_HOST_JOURNAL", "XDG_STATE_HOME"]);
        c.deepseek_key = Some(CarriedKey("sk-fictional-carried-0123".into()));
        let e = env(&c, &channel);
        assert_eq!(e["DEEPSEEK_API_KEY"], "sk-fictional-carried-0123", "Unfold's key, not the app environment's");
        let changed: Vec<&str> = e.iter().filter(|(k, v)| base.get(*k) != Some(*v)).map(|(k, _)| k.as_str()).collect();
        let set: Vec<&str> = ROWS.iter().filter(|(k, r)| *r == Row::Set && base.get(*k).map(String::as_str) != Some(e[*k].as_str())).map(|(k, _)| *k).collect();
        assert_eq!(changed.iter().copied().collect::<std::collections::BTreeSet<_>>(), set.iter().copied().chain(["HOME", "DEEPSEEK_API_KEY"]).collect(), "the set rows, HOME and the carried key");
        assert_eq!(format!("{:?}", c.deepseek_key), "Some(CarriedKey(<withheld>))", "Debug never prints it");
    }

    #[test]
    fn dsh_child_overlay_disables_every_upload_row() {
        let o = overlay();
        for id in ["session-log-deepseek", "plugin-package-inventory-deepseek", "session-telemetry-otel", "product-analytics", "desktop-product-telemetry"] {
            assert!(o.contains(&format!("- id: {id}\n  disabled: true\n")), "{id}");
        }
    }

    #[test]
    fn dsh_child_the_launch_link_is_kept_and_a_token_never_reaches_the_log() {
        let line = "dsh web: http://127.0.0.1:41700/?token=abcd1234 ready";
        assert_eq!(launch_link(line, 41700).as_deref(), Some("http://127.0.0.1:41700/?token=abcd1234"));
        assert_eq!(launch_link(line, 41799), None, "another port is not this child's link");
        assert_eq!(launch_link("dsh web: http://127.0.0.1:41700/", 41700), None, "no token, no link");
        let s = scrub("open http://127.0.0.1:41700/?token=abcd1234&x=1 or token=zz");
        assert!(!s.contains("token=") && !s.contains("abcd1234") && !s.contains("zz"), "{s}");
    }

    #[test]
    fn dsh_child_crashes_back_off_1_2_4_8_16_then_stop_within_10_minutes() {
        let t0 = Instant::now();
        let mut exits = Vec::new();
        let mut waits = Vec::new();
        for i in 0..5 {
            let now = t0 + Duration::from_secs(i * 30);
            exits.push(now);
            waits.push(backoff(&exits, now));
        }
        assert_eq!(waits, [1, 2, 4, 8, 16].map(|s| Some(Duration::from_secs(s))));
        exits.push(t0 + Duration::from_secs(150));
        assert_eq!(backoff(&exits, t0 + Duration::from_secs(150)), None, "the 6th crash in 10 minutes: no more tries");
        assert!(backoff(&exits, t0 + Duration::from_secs(150 + 600)).is_some(), "10 minutes later it may try again");
    }

    #[test]
    fn dsh_child_agentd_restarts_once_after_the_reply_in_progress() {
        let replying = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let restarts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (r, n) = (replying.clone(), restarts.clone());
        let t = std::thread::spawn(move || restart_after_reply(&|| r.load(std::sync::atomic::Ordering::SeqCst), &|| {
            n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }, Duration::from_secs(10)));
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(restarts.load(std::sync::atomic::Ordering::SeqCst), 0, "not while the reply runs");
        replying.store(false, std::sync::atomic::Ordering::SeqCst);
        t.join().unwrap();
        assert_eq!(restarts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    static SEEN: Mutex<Vec<(Option<String>, bool)>> = Mutex::new(Vec::new());

    fn watched_serve(cfg: crate::dsh_control::Config, children: Arc<dyn crate::dsh_control::Children>, submit: crate::dsh_control::Submit, log: &Log) -> Result<crate::dsh_control::Endpoint, String> {
        let answering = std::os::unix::net::UnixStream::connect(cfg.dir.join(crate::dsh_control::SOCKET)).is_ok();
        SEEN.lock().unwrap().push((std::thread::current().name().map(str::to_string), answering));
        crate::dsh_control::serve(cfg, children, submit, log)
    }

    fn slow_serve(cfg: crate::dsh_control::Config, children: Arc<dyn crate::dsh_control::Children>, submit: crate::dsh_control::Submit, log: &Log) -> Result<crate::dsh_control::Endpoint, String> {
        std::thread::sleep(Duration::from_millis(1500));
        crate::dsh_control::serve(cfg, children, submit, log)
    }

    fn sock_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dsh-ch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&root).unwrap();
        root
    }

    #[test]
    fn dsh_child_the_channel_serves_and_stops_each_endpoint_on_its_own_thread_never_two_at_once() {
        let root = sock_root("one");
        let (log, lines) = logged(&root);
        let ch: &'static Channel = Box::leak(Box::new(Channel::new(watched_serve)));
        ch.held("127.0.0.1:45679".parse().unwrap(), Some("sess-1".into()), Some(&log));
        let (r, l) = (root.clone(), log.clone());

        let done = within(Duration::from_secs(30), move || {
            let serve = || ch.serve(&r, r.join("journal.jsonl"), Arc::new(NoChild), Arc::new(|_: &str| Err("no chat".to_string())), &l).is_ok();
            let (first, second) = (serve(), serve());
            ch.stop();
            (first, second)
        });
        assert_eq!(done, Some((true, true)), "two serves and a stop ended within 30 s");
        let seen = SEEN.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.iter().all(|(name, _)| name.as_deref() == Some("dsh-channel")), "every serve ran on the channel's own thread: {seen:?}");
        assert!(seen.iter().all(|(_, answering)| !answering), "the old endpoint was stopped before the new one was served: {seen:?}");

        let journal = std::fs::read_to_string(root.join("journal.jsonl")).unwrap_or_default();
        assert!(journal.lines().all(|l| serde_json::from_str::<Value>(l).unwrap()["refused"] != "peer"), "{journal}");
        assert!(!lines().contains(&format!("refused peer {}", std::process::id())), "{}", lines());
        for f in ["control.sock", "control.token", "observer"] {
            assert!(!root.join(f).exists(), "{f} left after the stop");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_letting_the_lease_go_or_taking_it_never_waits_on_a_serve_in_progress() {
        let root = sock_root("gate");
        let (log, _) = logged(&root);
        let ch: &'static Channel = Box::leak(Box::new(Channel::new(slow_serve)));
        let addr: std::net::SocketAddr = "127.0.0.1:45680".parse().unwrap();
        let observer = root.join("observer");
        ch.held(addr, Some("sess-1".into()), Some(&log));
        let (r, l) = (root.clone(), log.clone());
        let serving = std::thread::spawn(move || ch.serve(&r, r.join("journal.jsonl"), Arc::new(NoChild), Arc::new(|_: &str| Err("no chat".to_string())), &l).is_ok());
        std::thread::sleep(Duration::from_millis(300));

        let t0 = Instant::now();
        let gone = within(Duration::from_secs(5), move || ch.gone());
        assert!(gone.is_some() && t0.elapsed() < Duration::from_millis(400), "lease_gone waited {:?} on a serve in progress", t0.elapsed());
        assert!(serving.join().unwrap(), "served");
        assert!(!observer.exists(), "a lease let go during the serve is not published by it");

        let t0 = Instant::now();
        let l = log.clone();
        assert!(within(Duration::from_secs(5), move || ch.held(addr, Some("sess-2".into()), Some(&l))).is_some() && t0.elapsed() < Duration::from_millis(400), "lease_held waited {:?}", t0.elapsed());
        assert!(eventually(Duration::from_secs(3), || std::fs::read_to_string(&observer).ok().as_deref() == Some("127.0.0.1:45680\n")), "published after the Ready");
        assert!(within(Duration::from_secs(10), move || ch.stop()).is_some(), "stopped");
        assert!(!observer.exists() && !root.join("control.sock").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    struct Member(u32, u64);
    impl Drop for Member {
        fn drop(&mut self) {
            if crate::nemo::proc_info(self.0).is_some_and(|i| i.start_us == self.1) {
                unsafe { libc::kill(self.0 as i32, libc::SIGKILL) };
            }
        }
    }

    #[test]
    fn dsh_child_omni_s_leader_that_exited_is_not_alive_and_its_stop_ends_its_group_then_reaps_it() {
        use crate::omni_child::Children as _;
        let root = sock_root("omni-kids");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let kids = OmniKids { owned: owned.clone(), log: log.clone(), record: root.join("children.json") };
        let run = root.display().to_string();
        let pidf = root.join("member.pid");

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo \"$$ omni-fake $2\" >> \"$1\"; /bin/sleep 30 & echo \"$! omni-member $2\" >> \"$1\"; echo $! > \"$0\"; exec /bin/sleep 0.3"]).arg(&pidf).arg(pids_file()).arg(&run).stdin(Stdio::null()).stdout(Stdio::null());
        let r = kids.spawn_recorded("omni", &mut cmd).unwrap();
        assert_eq!(r.pgid, r.pid, "it leads its own group");
        assert!(eventually(Duration::from_secs(5), || std::fs::read_to_string(&pidf).is_ok_and(|s| s.trim().parse::<u32>().is_ok())));
        let member: u32 = std::fs::read_to_string(&pidf).unwrap().trim().parse().unwrap();
        let _m = Member(member, crate::nemo::proc_info(member).map_or(0, |i| i.start_us));
        assert!(eventually(Duration::from_secs(5), || !kids.alive(&r)), "a leader that exited is not alive, though it isn't reaped yet");
        assert!(alive(member), "its member still runs");

        kids.stop_recorded(&crate::omni_child::Recorded { start_time: r.start_time + 1, ..r.clone() });
        assert!(alive(member), "a record that doesn't match is left alone");
        kids.stop_recorded(&r);
        assert!(eventually(Duration::from_secs(7), || !alive(member)), "what the leader left in its group is stopped");
        assert_eq!(unsafe { libc::waitpid(r.pid, std::ptr::null_mut(), libc::WNOHANG) }, -1, "the leader is reaped (no zombie left)");
        assert!(owned.lock().unwrap().recorded.is_empty(), "taken out of the list");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_pack_in_the_older_layout_doesn_t_start_and_the_current_layout_does() {
        let root = std::env::temp_dir().join(format!("dsh-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let put = |dir: &Path, files: &[&str]| for f in files {
            std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.join(f), "x").unwrap();
        };
        let old = root.join("old");
        put(&old, &["node/bin/node", "install.mjs", "bin.js"]);
        let r = pack_layout(&old);
        assert!(r.as_ref().is_err_and(|e| e.contains("bin/node, launch.sh, preload.mjs") && e.contains(DSH_BIN)), "{r:?}");
        let current = root.join("current");
        put(&current, &PACK_FILES);
        assert_eq!(pack_layout(&current), Ok(()));
        assert_eq!(node_of(&current), current.join("bin/node"), "the recorded Node is the pack's bin/node");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_the_app_starts_this_build_s_pinned_pack_and_the_placeholder_pin_fails_closed() {
        let root = std::env::temp_dir().join(format!("dsh-app-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        let log = Log::open_as(&crate::WriteMode::Dev, &root.join("app"));
        let _env = crate::dsh_pack::TEST_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let unset = crate::dsh_pack::start_pin_unset();
        #[cfg(not(feature = "rehearsal_pack"))]
        if !unset {
            return;
        }
        let r = (APP.pack)(&root.join("data"), &log, &|| true, &|_| {});
        if unset {
            assert!(r.as_ref().is_err_and(|(n, e)| *n == "no_pack" && e.contains("no pack pin")), "{r:?}");
        } else {

            assert!(r.as_ref().is_err_and(|(n, e)| *n == "no_pack" && e.contains("no placed pack 0.5.0-rehearsal.4")), "{r:?}");
        }
        assert!(!root.join("data/runtimes").exists(), "nothing fetched or written");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_omni_starts_from_the_pack_with_90_s_to_be_healthy_and_its_link_in_the_bot_team_s_home() {
        let root = sock_root("omni-cfg");
        let (log, _) = logged(&root);
        let pack = root.join("pack");
        let manifest = pack.join("omni/runtime/node_modules/@automagik/omni");
        std::fs::create_dir_all(&manifest).unwrap();
        std::fs::write(manifest.join("package.json"), r#"{"name":"@automagik/omni","version":"2.9.1"}"#).unwrap();
        let mut c = cfg(&root);
        c.pack = pack.clone();
        let o = omni_cfg(&pack, &c.instance_root, c.ports, Arc::default(), log.clone()).unwrap();
        assert_eq!(o.healthy_within, Duration::from_secs(90), "90 s at a start (unmeasured until a first run on the real pack)");
        assert_eq!((o.bun.clone(), o.version.as_str(), o.ports), (pack.join("omni/bin/bun"), "2.9.1", c.ports));
        assert_eq!(o.bots_home.display().to_string(), env(&c, &[])["AUTOMAGIK_BOTS_HOME"], "Omni's link goes where DSH's Bot team reads it");
        assert!(o.instance == c.instance_root && o.pack == pack);
        assert!(omni_cfg(&root.join("no-omni"), &c.instance_root, c.ports, Arc::default(), log).is_err(), "a pack without Omni: no Omni, DSH starts without it");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_the_install_gets_none_of_the_channel_variables_and_the_preload_names_all_four() {

        assert!(LAUNCHER.contains(r#"exec /bin/bash "$1/launch.sh" "$2" "$3""#) && !LAUNCHER.contains("node"), "{LAUNCHER}");
        for n in CHANNEL_VARS {
            assert!(PACK_LAUNCH.contains(&format!("-u {n} ")), "the profile's install could get {n}");
            assert!(PACK_PRELOAD.contains(&format!("take('{n}')")), "the preload doesn't take {n}");
        }
        assert!(ROWS.iter().any(|(n, r)| *n == "UNFOLD_HOST_JOURNAL" && matches!(r, Row::Unset(_))), "the host journal is reserved and unset until the app writes it");
    }

    #[test]
    fn dsh_child_the_preload_takes_all_four_channel_variables_and_no_tool_child_sees_one() {
        let root = sock_root("preload");
        let token = root.join("control.token");

        std::fs::write(&token, "ab".repeat(32)).unwrap();
        std::fs::write(root.join("preload.mjs"), PACK_PRELOAD).unwrap();

        std::fs::write(root.join("main.mjs"), r#"import { execFileSync } from 'node:child_process'
const ch = globalThis[Symbol.for('unfold.channel')]
const kid = execFileSync(process.execPath, ['-e', "console.log(JSON.stringify(Object.keys(process.env).filter((k) => k.startsWith('UNFOLD_'))))"]).toString().trim()
console.log(JSON.stringify({ kid: JSON.parse(kid), own: Object.keys(process.env).filter((k) => k.startsWith('UNFOLD_')), keys: Object.keys(ch).sort(), hostJournal: ch.hostJournal, token: (ch.token || '').length }))
"#).unwrap();
        let vals = [root.join("observer").display().to_string(), root.join("control.sock").display().to_string(), token.display().to_string(), "/x/app/host.jsonl".to_string()];
        let out = Command::new(node()).args(["--disable-sigusr1", "--import"]).arg(root.join("preload.mjs")).arg(root.join("main.mjs"))
            .env_clear().env("PATH", "/usr/bin:/bin").envs(CHANNEL_VARS.iter().zip(vals.iter())).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let got: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(got, json!({"kid": [], "own": [], "keys": ["hostJournal", "observerFile", "socket", "token"], "hostJournal": "/x/app/host.jsonl", "token": 64}));
        assert!(!token.exists(), "the token file is unlinked");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_fake_that_dials_7700_3410_or_8310_fails_the_run_loudly() {
        for port in ["7700", "3410", "8310"] {
            let (root, mut c) = fake(&format!("fence{port}"));
            let (log, _) = logged(&root);
            let run = format!("drill:{}", root.display());
            c.base_env.retain(|(k, _)| k != "FAKE_RUN");
            c.base_env.extend([("FAKE_RUN".to_string(), run.clone()), ("FAKE_DIAL".to_string(), port.to_string())]);
            let owned: Shared = Arc::default();
            assert_eq!(start(&c, &owned, &log, &|| true), Err(StartError::Other("DSH stopped before it was ready".into())), "the fence ends the fake");
            let text = std::fs::read_to_string(dsh_dir(&c.instance_root).join("dsh.log")).unwrap();
            assert!(text.contains(&format!("FENCE: dial 127.0.0.2:{port}")), "{text}");
            assert!(std::fs::read_to_string(fence_file()).unwrap_or_default().lines().any(|l| l == format!("FENCE: dial 127.0.0.2:{port} {run}")), "recorded outside the test's tree");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn dsh_child_a_fake_ends_when_its_harness_is_killed() {
        let (root, c) = fake("harness");

        let mut h = Command::new("/bin/sh");
        h.args(["-c", "echo \"$$ harness-fake $0\" >> \"$1\"; exec /bin/sleep 30"]).arg(root.display().to_string()).arg(pids_file());
        let mut harness = h.spawn().unwrap();
        let mut env: Vec<(String, String)> = env(&c, &[]).into_iter().filter(|(k, _)| k != "FAKE_HARNESS_PID").collect();
        env.extend([("FAKE_HARNESS_PID".to_string(), harness.id().to_string()), ("UNFOLD_BRIDGE_PORT".to_string(), "0".to_string())]);
        let mut fake = Command::new(&c.node).arg(c.pack.join(DSH_BIN)).args(["--port", "0"]).env_clear().envs(env).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let pid = fake.id();
        assert!(eventually(Duration::from_secs(5), || recorded_pids(&root.display().to_string()).contains(&pid)), "the fake recorded itself");
        assert!(fake.try_wait().unwrap().is_none(), "it runs while its harness does");
        harness.kill().unwrap();
        let _ = harness.wait();
        let end = Instant::now() + Duration::from_secs(3);
        let mut ended = None;
        while ended.is_none() && Instant::now() < end {
            ended = fake.try_wait().unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }
        if ended.is_none() {
            let _ = fake.kill();
            let _ = fake.wait();
        }
        assert!(ended.is_some(), "the fake outlived its harness");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_test_that_fails_after_a_start_leaves_no_fake_alive() {
        let (root, c) = fake("orphan");
        let run = root.display().to_string();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (log, _) = logged(&root);
            let owned: Shared = Arc::default();
            let d = start(&c, &owned, &log, &|| true).unwrap();
            let _left = Leftover::of(d.pid);
            panic!("on purpose, with the fake running");
        }))
        .is_err();
        assert!(panicked);
        let pids = recorded_pids(&run);
        assert!(!pids.is_empty(), "the fake recorded itself before it started");
        assert!(eventually(Duration::from_secs(5), || pids.iter().all(|p| !alive(*p))), "fakes alive after the failed test: {pids:?}");
        let ps = String::from_utf8(Command::new("/bin/ps").args(["-axo", "ppid=,args="]).output().unwrap().stdout).unwrap();
        assert!(!ps.lines().any(|l| l.trim_start().starts_with("1 ") && l.contains(&run)), "an orphan names the fixture");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_a_model_change_s_stop_local_on_the_gate_loop_never_stops_the_endpoint() {
        let root = sock_root("local");
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        use std::os::unix::process::CommandExt as _;
        let mut agent = Command::new("/bin/sleep");
        agent.arg("30").process_group(0);
        let child = agent.spawn().unwrap();
        let pid = child.id();
        std::fs::OpenOptions::new().create(true).append(true).open(pids_file()).and_then(|mut f| writeln!(f, "{pid} agent-fake {}", root.display())).unwrap();
        owned.lock().unwrap().agent = Some(child);
        let (stops, gones) = (CHANNEL_STOPS.with(|n| n.get()), LEASE_GONES.with(|n| n.get()));
        crate::stop_local(&owned, &log);

        assert_eq!(CHANNEL_STOPS.with(|n| n.get()), stops, "stop_local asked the endpoint to stop");
        assert_eq!(LEASE_GONES.with(|n| n.get()), gones + 1, "agentd's address is withdrawn before agentd stops");
        assert!(!alive(pid), "agentd was stopped");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn fake_omni_cmd(port: u16, run: &str) -> Command {
        let js = format!("const fs = require('fs'), http = require('http'), cp = require('child_process')
fs.appendFileSync(process.env.FAKE_PID_RECORD, process.pid + ' omni-fake ' + process.env.FAKE_RUN + '\\n')
const m = cp.spawn('/bin/sleep', ['30'], {{ stdio: 'ignore' }})
fs.appendFileSync(process.env.FAKE_PID_RECORD, m.pid + ' omni-member ' + process.env.FAKE_RUN + '\\n')
setInterval(() => {{ try {{ process.kill(Number(process.env.FAKE_HARNESS_PID), 0) }} catch {{ try {{ m.kill('SIGKILL') }} catch {{}} process.exit(0) }} }}, 300)
http.createServer((q, r) => {{ r.writeHead(q.url === '/api/v2/health' ? 200 : 404); r.end('{{}}') }}).listen({port}, '127.0.0.1')
");
        let mut cmd = Command::new(node());
        cmd.args(["-e", &js]).env_clear().env("PATH", "/usr/bin:/bin").envs(fixture_env(run)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd
    }

    #[test]
    fn dsh_child_a_crash_leaves_omni_and_dsh_recorded_and_the_next_switch_on_stops_them_before_omni_answers_again() {
        use crate::omni_child::Children as _;
        let (root, c) = fake("crash");
        let (log, lines) = logged(&root);
        let run = root.display().to_string();
        let owned: Shared = Arc::default();
        let children = dsh_dir(&c.instance_root).join("children.json");
        std::fs::create_dir_all(dsh_dir(&c.instance_root)).unwrap();

        let kids = OmniKids { owned: owned.clone(), log: log.clone(), record: children.clone() };
        let api = free_port();
        let o = kids.spawn_recorded("omni", &mut fake_omni_cmd(api, &run)).unwrap();
        let _lo = Leftover(o.pid as u32, o.start_time);
        assert!(crate::omni_child::healthy(api, Duration::from_secs(10)), "the fake Omni answers");
        let d = start(&c, &owned, &log, &|| true).unwrap();
        let _ld = Leftover::of(d.pid);
        let recs: Vec<Value> = serde_json::from_str::<Value>(&std::fs::read_to_string(&children).unwrap()).unwrap().as_array().unwrap().clone();
        let named = |n: &str| recs.iter().find(|r| r["name"] == n).cloned().unwrap_or_else(|| panic!("no {n} record in {recs:?}"));
        assert_eq!((named("omni")["pid"].as_i64(), named("omni")["start_time"].as_u64()), (Some(o.pid as i64), Some(o.start_time)), "Omni persisted at its spawn");
        assert_eq!((named("dsh")["pid"].as_u64(), named("dsh")["start_time"].as_u64()), (Some(d.pid as u64), Some(d.start_us)), "DSH persisted beside it");

        let gone: Vec<Value> = recs.into_iter().map(|mut r| {
            r["owner"] = json!({"pid": 1, "start_time": 1});
            r
        }).collect();
        std::fs::write(&children, Value::Array(gone).to_string()).unwrap();
        owned.lock().unwrap().recorded.clear();

        stop_leftovers(&children, &log);
        assert!(!crate::omni_child::healthy(api, Duration::from_millis(800)), "the leftover Omni still answers the health check: {}", lines());
        assert!(eventually(Duration::from_secs(8), || !alive(d.pid) && !alive(o.pid as u32)), "the leftovers are stopped: {}", lines());
        let members: Vec<u32> = std::fs::read_to_string(pids_file()).unwrap().lines().filter(|l| l.contains(" omni-member ") && l.ends_with(&run)).filter_map(|l| l.split(' ').next()?.parse().ok()).collect();
        assert!(!members.is_empty() && eventually(Duration::from_secs(5), || members.iter().all(|m| !alive(*m))), "Omni's group went with it (Postgres, NATS)");
        assert!(!children.exists(), "the records are used once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_is_persisted_at_its_spawn_while_the_profile_installs() {
        let (root, mut c) = fake("spawn");

        std::fs::write(c.pack.join("install.mjs"), "await new Promise((r) => setTimeout(r, 4000))\n").unwrap();
        c.ready_within = Duration::from_secs(30);
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let children = dsh_dir(&c.instance_root).join("children.json");
        let node = c.node.canonicalize().unwrap().display().to_string();
        let (c2, o2, l2) = (Arc::new(c), owned.clone(), log.clone());
        let c3 = c2.clone();
        let started = std::thread::spawn(move || start(&c3, &o2, &l2, &|| true));
        let rec = || std::fs::read_to_string(&children).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_array()?.iter().find(|r| r["name"] == "dsh").cloned());
        assert!(eventually(Duration::from_secs(20), || rec().is_some()), "no record");
        let early = rec().unwrap();
        let pid = early["pid"].as_u64().unwrap() as u32;
        let _left = Leftover::of(pid);

        assert_ne!(early["command"].as_str(), Some(node.as_str()), "first recorded only once it ran: {early}");
        assert!(early["command"].as_str().is_some_and(|c| c.ends_with("sh")), "the launcher: {early}");
        assert_eq!(early["start_time"].as_u64(), crate::nemo::proc_info(pid).map(|i| i.start_us), "the launcher, with its start time");
        let d = started.join().unwrap().unwrap();
        assert_eq!(d.pid, pid, "the exec keeps the PID");
        assert_eq!(rec().unwrap()["command"].as_str(), Some(node.as_str()), "recorded again as the pack's Node once it runs");
        stop(&d, &c2.instance_root, &owned, &log);
        assert!(rec().is_none(), "its record goes with its stop");
        let _ = std::fs::remove_dir_all(&root);
    }

    pub mod switch {
        use super::*;
        use std::sync::atomic::{AtomicU16, AtomicUsize};

        pub static SERIAL: Mutex<()> = Mutex::new(());
        pub static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

        pub static MODES: Mutex<std::collections::VecDeque<(u64, bool)>> = Mutex::new(std::collections::VecDeque::new());
        pub static ACTIVE: AtomicUsize = AtomicUsize::new(0);
        pub static MAX_ACTIVE: AtomicUsize = AtomicUsize::new(0);
        pub static OMNI_ACTIVE: AtomicUsize = AtomicUsize::new(0);
        pub static OMNI_MAX: AtomicUsize = AtomicUsize::new(0);
        pub static STARTS: AtomicUsize = AtomicUsize::new(0);
        pub static OMNI_DELAY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        pub static CHANGED: AtomicUsize = AtomicUsize::new(0);

        pub static CHANGED_NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

        pub static REFETCH_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        pub static REFETCH_FAILS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        pub static PLACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        pub static KEPT: Mutex<Option<&'static str>> = Mutex::new(None);

        pub static PACK_STEPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        pub static DL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

        pub static REFRESHES: AtomicUsize = AtomicUsize::new(0);

        pub static FAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        pub static DEAF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

        pub static KEY: Mutex<Option<String>> = Mutex::new(None);
        pub static TOOK: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());

        pub static STALL: Mutex<Option<u64>> = Mutex::new(None);
        pub static STALL_CV: std::sync::Condvar = std::sync::Condvar::new();

        pub fn stall(own: &Own, gen: u64) {
            if !std::ptr::eq(own, &OWN_T) {
                return;
            }
            let mut s = STALL.lock().unwrap();
            while *s == Some(gen) {
                s = STALL_CV.wait(s).unwrap();
            }
        }
        pub fn release() {
            *STALL.lock().unwrap() = None;
            STALL_CV.notify_all();
        }
        pub static PORTS: [AtomicU16; 5] = [AtomicU16::new(0), AtomicU16::new(0), AtomicU16::new(0), AtomicU16::new(0), AtomicU16::new(0)];
        pub static CH: Channel = Channel::new(crate::dsh_control::serve);
        pub static OWN_T: Own = Own::new(
            Hooks { start: fake_start, stop: fake_stop, exited: crate::tray::exited, pack, healthy, bad, backoff: fake_backoff, ports, manifest, refetch, can_refetch, leftovers, omni: fake_omni, sockdir, serve, changed, refresh, address, ready_within: Duration::from_secs(10), key },
            &CH,
        );

        pub fn ev(e: impl Into<String>) {
            EVENTS.lock().unwrap().push(e.into());
        }
        pub fn root() -> PathBuf {
            std::env::temp_dir().join(format!("dsh-switch-{}", std::process::id()))
        }
        struct Dec(&'static AtomicUsize);
        impl Drop for Dec {
            fn drop(&mut self) {
                self.0.fetch_sub(1, SEQ);
            }
        }
        fn fake_start(cfg: &ChildCfg, owned: &Shared, log: &Log, alive: &dyn Fn() -> bool) -> Result<OwnDsh, StartError> {
            let n = ACTIVE.fetch_add(1, SEQ) + 1;
            MAX_ACTIVE.fetch_max(n, SEQ);
            let _d = Dec(&ACTIVE);
            STARTS.fetch_add(1, SEQ);
            TOOK.lock().unwrap().push(cfg.deepseek_key.as_ref().map(|k| k.0.clone()));
            let (delay, fails) = MODES.lock().unwrap().pop_front().unwrap_or((0, false));
            let dir = (cfg.sockdir)(&cfg.instance_id).map_err(StartError::SockDir)?;
            (cfg.serve)(&dir, log).map_err(StartError::Other)?;
            ev("start:served");
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", "echo \"$$ switch-fake $0\" >> \"$1\"; exec /bin/sleep 30"]).arg(root().display().to_string()).arg(pids_file());
            let pid = crate::tray::spawn_recorded(owned, "dsh", &mut cmd).map_err(StartError::Other)?;
            let end = Instant::now() + Duration::from_millis(delay);
            while Instant::now() < end {
                if !DEAF.load(SEQ) && !alive() {
                    crate::tray::stop_one(owned, pid, log);
                    ev("start:cancelled");
                    return Err(StartError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if fails {
                crate::tray::stop_one(owned, pid, log);
                ev("start:failed");
                return Err(StartError::Other("the fake failed".into()));
            }
            let id = crate::tray::identity(owned, pid).ok_or(StartError::Other("gone".into()))?;
            ev("start:ok");
            Ok(OwnDsh { pid, start_us: id.start_us, port: cfg.ports.bridge, web: cfg.ports.web, endpoint: dir.join("none.json") })
        }
        fn fake_stop(d: &OwnDsh, _root: &Path, owned: &Shared, log: &Log) {
            crate::tray::stop_one(owned, d.pid, log);
            ev("stop");
        }
        fn pack(_: &Path, _: &Log, _: &(dyn Fn() -> bool + Sync), step: &dyn Fn(crate::dsh_pack::Step)) -> Result<(PathBuf, Option<&'static str>), (&'static str, String)> {
            fake_steps(step, PACK_STEPS.load(SEQ));
            Ok((root().join("pack"), *KEPT.lock().unwrap()))
        }

        fn fake_steps(step: &dyn Fn(crate::dsh_pack::Step), n: u64) {
            use crate::dsh_pack::Step;
            if n == 0 {
                return;
            }
            let t0 = Instant::now();
            for i in 0..=n {
                step(Step::Download { done: i * 1000 / n, total: 1000 });
                std::thread::sleep(Duration::from_millis(5));
            }
            DL_MS.store(t0.elapsed().as_millis() as u64, SEQ);
            for s in [Step::Verify, Step::Unpack, Step::Check] {
                step(s);
            }
        }
        fn healthy(_: &Path, _: &Path, _: &Log) {
            ev("healthy");
        }
        fn bad(_: &Path, _: &Path, why: &str, _: &Log) {
            ev(&format!("bad:{why}"));
        }

        fn fake_backoff(exits: &[Instant], now: Instant) -> Option<Duration> {
            match FAST.load(SEQ) {
                true => (exits.len() < GIVE_UP).then_some(Duration::from_millis(20)),
                false => super::backoff(exits, now),
            }
        }
        fn ports(_: &Path, _: &str) -> Result<Ports, String> {
            let p = |i: usize| PORTS[i].load(SEQ);
            Ok(Ports { web: p(0), bridge: p(1), omni_api: p(2), omni_db: p(3), omni_nats: p(4) })
        }
        fn manifest(_: &Path) -> Result<(), Vec<String>> {
            let names = CHANGED_NAMES.lock().unwrap().clone();
            match CHANGED.load(SEQ) {
                0 => Ok(()),
                _ if !names.is_empty() => Err(names),
                n => Err((1..=n).map(|i| format!("app/f{i}.js")).collect()),
            }
        }

        fn refetch(_: &Path, _: &Log, alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(crate::dsh_pack::Step)) -> Result<(), String> {
            ev("refetch");
            fake_steps(step, PACK_STEPS.load(SEQ));
            sleep_while(Duration::from_millis(REFETCH_MS.load(SEQ)), alive);
            if REFETCH_FAILS.load(SEQ) {
                return Err("offline".into());
            }
            CHANGED.store(0, SEQ);
            Ok(())
        }
        fn can_refetch() -> bool {
            !PLACED.load(SEQ)
        }
        fn leftovers(_: &Path, _: &Log) {
            ev("leftovers");
        }
        fn fake_omni(own: &'static Own, gen: u64, _pack: PathBuf, _ports: Ports, first: mpsc::Sender<()>) {
            let n = OMNI_ACTIVE.fetch_add(1, SEQ) + 1;
            OMNI_MAX.fetch_max(n, SEQ);
            let _d = Dec(&OMNI_ACTIVE);
            ev("omni:start");
            sleep_while(Duration::from_millis(OMNI_DELAY.load(SEQ)), &|| own.alive(gen));
            let _ = first.send(());
            ev("omni:first");
            while own.alive(gen) {
                std::thread::sleep(Duration::from_millis(20));
            }
            ev("omni:stop");
        }
        fn sockdir(_: &str) -> Result<PathBuf, String> {
            let d = std::env::temp_dir().join(format!("dsh-switch-sock-{}", std::process::id()));
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&d).map_err(|e| e.to_string())?;
            Ok(d)
        }
        fn serve(dir: &Path, log: &Log) -> Result<Vec<(String, String)>, String> {
            CH.serve(dir, root().join("journal.jsonl"), Arc::new(NoChild), Arc::new(|_: &str| Err("no chat".to_string())), log)
        }
        fn changed(_: &Log) {}
        fn key(_: &Path, _: &Log) -> Option<CarriedKey> {
            KEY.lock().unwrap().clone().map(CarriedKey)
        }

        fn refresh() {
            REFRESHES.fetch_add(1, SEQ);
            let p = OWN_T.status.lock().unwrap().progress.map(|p| p.phase);
            SEEN.lock().unwrap().push(p);
        }
        pub static SEEN: Mutex<Vec<Option<&'static str>>> = Mutex::new(Vec::new());
        fn address(_: Option<String>) {}

        pub fn ready() -> (&'static Own, Shared, impl Fn() -> String) {
            let r = root();
            std::fs::create_dir_all(r.join("instances/i1")).unwrap();
            let log = Log::open_as(&crate::WriteMode::Dev, &r.join("app"));
            let owned: Shared = Arc::default();
            let _ = OWN_T.rt.set(Runtime { owned: owned.clone(), log, data: r.clone(), instance_root: r.join("instances/i1") });
            if OWN_T.status.lock().unwrap().on {
                OWN_T.set_on(false);
            }
            assert!(super::eventually(Duration::from_secs(10), || ACTIVE.load(SEQ) == 0 && OMNI_ACTIVE.load(SEQ) == 0 && OWN_T.current.lock().unwrap().is_none() && !sockdir("").unwrap().join(crate::dsh_control::SOCKET).exists()), "settled off");
            EVENTS.lock().unwrap().clear();
            MODES.lock().unwrap().clear();
            for c in [&MAX_ACTIVE, &OMNI_MAX, &STARTS, &CHANGED] {
                c.store(0, SEQ);
            }
            CHANGED_NAMES.lock().unwrap().clear();
            *KEY.lock().unwrap() = None;
            TOOK.lock().unwrap().clear();
            OMNI_DELAY.store(0, SEQ);
            REFETCH_MS.store(0, SEQ);
            REFETCH_FAILS.store(false, SEQ);
            PLACED.store(false, SEQ);
            PACK_STEPS.store(0, SEQ);
            REFRESHES.store(0, SEQ);
            SEEN.lock().unwrap().clear();
            *KEPT.lock().unwrap() = None;
            FAST.store(false, SEQ);
            DEAF.store(false, SEQ);
            release();
            for p in &PORTS {
                p.store(super::free_port(), SEQ);
            }
            CH.held("127.0.0.1:45690".parse().unwrap(), Some("sess-s".into()), None);
            let owned = OWN_T.rt.get().unwrap().owned.clone();
            (&OWN_T, owned, move || std::fs::read_to_string(r.join("app/app.log")).unwrap_or_default())
        }
        pub fn socket() -> bool {
            sockdir("").unwrap().join(crate::dsh_control::SOCKET).exists()
        }
        pub fn kids(owned: &Shared) -> usize {
            owned.lock().unwrap().recorded.len()
        }
        pub fn running(own: &Own) -> bool {
            own.status.lock().unwrap().running
        }
        pub fn events() -> Vec<String> {
            EVENTS.lock().unwrap().clone()
        }
    }

    #[test]
    fn dsh_child_a_switch_on_stops_leftovers_then_starts_omni_then_dsh() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::OMNI_DELAY.store(400, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "running: {:?}", switch::events());
        let ev = switch::events();
        let at = |e: &str| ev.iter().position(|x| x == e).unwrap_or_else(|| panic!("no {e} in {ev:?}"));
        assert!(at("leftovers") < at("omni:start") && at("omni:first") < at("start:served"), "leftovers, then Omni, then DSH: {ev:?}");
        assert_eq!((switch::kids(&owned), switch::socket()), (1, true));
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket() && switch::events().contains(&"omni:stop".to_string())), "off: no child, no endpoint, Omni stopped: {:?}", switch::events());
        assert!(own.current.lock().unwrap().is_none() && !own.status.lock().unwrap().running);
    }

    #[test]
    fn dsh_child_each_start_takes_the_key_unfold_holds_and_a_new_key_restarts_it_once() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, lines) = switch::ready();
        *switch::KEY.lock().unwrap() = Some("sk-fictional-first-0001".into());
        assert!(!own.key_changed(), "nothing runs: nothing to compare");
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        assert_eq!(*switch::TOOK.lock().unwrap(), [Some("sk-fictional-first-0001".to_string())], "the start took Unfold's key");
        assert!(!own.key_changed(), "the same key: no restart");
        own.key_check(|| false);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(switch::STARTS.load(SEQ), 1, "the same key restarts nothing");

        *switch::KEY.lock().unwrap() = Some("sk-fictional-second-0002".into());
        assert!(own.key_changed());
        static REPLYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        own.key_check(|| REPLYING.load(SEQ));
        own.key_check(|| REPLYING.load(SEQ));
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(switch::STARTS.load(SEQ), 1, "it waits for the reply in progress");
        REPLYING.store(false, SEQ);
        assert!(eventually(Duration::from_secs(10), || switch::STARTS.load(SEQ) == 2 && switch::running(own)), "{:?}", switch::events());
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(switch::STARTS.load(SEQ), 2, "two checks, one restart");
        assert_eq!(switch::TOOK.lock().unwrap().last().cloned().flatten().as_deref(), Some("sk-fictional-second-0002"));
        assert!(!own.key_changed());

        *switch::KEY.lock().unwrap() = None;
        own.key_check(|| false);
        assert!(eventually(Duration::from_secs(10), || switch::STARTS.load(SEQ) == 3 && switch::running(own)), "{:?}", switch::events());
        assert_eq!(switch::TOOK.lock().unwrap().last().cloned(), Some(None));
        let text = lines();
        assert!(text.contains("dsh: the DeepSeek key in Unfold changed") && !text.contains("sk-fictional"), "{text}");
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
        assert!(!own.key_changed(), "off: nothing to compare");
    }

    #[test]
    fn dsh_child_a_switch_on_records_its_pack_healthy_once_dsh_started() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own) && switch::events().contains(&"healthy".to_string())), "{:?}", switch::events());
        let ev = switch::events();
        let at = |e: &str| ev.iter().position(|x| x == e).unwrap_or_else(|| panic!("no {e} in {ev:?}"));
        assert!(at("start:ok") < at("healthy"), "recorded once DSH started: {ev:?}");
        assert_eq!(ev.iter().filter(|e| *e == "healthy").count(), 1, "{ev:?}");
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_an_older_pack_run_for_a_hash_refused_newer_one_shows_the_kept_notice_while_it_runs() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        *switch::KEPT.lock().unwrap() = kept_notice(&Some(crate::dsh_pack::PackError::Hash));
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        assert_eq!((own.view()["running"].as_bool(), own.view()["notice"].as_str()), (Some(true), Some("pack_kept")), "the running state, plus its line");
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
        assert_eq!(kept_notice(&Some(crate::dsh_pack::PackError::Hash)), Some("pack_kept"), "a hash refusal with an older pack to run: not pack_changed, which says it doesn't start");
        assert_eq!(kept_notice(&Some(crate::dsh_pack::PackError::Io("offline".into()))), None, "an offline fetch: the log only");
        assert_eq!(kept_notice(&None), None);
    }

    #[test]
    fn dsh_child_an_older_pack_run_after_a_bad_newer_one_shows_the_rolled_back_notice_while_it_runs() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        *switch::KEPT.lock().unwrap() = kept_notice(&Some(crate::dsh_pack::PackError::Bad("pack 2.0.0 is marked bad".into())));
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        assert_eq!((own.view()["running"].as_bool(), own.view()["notice"].as_str()), (Some(true), Some("pack_rolled_back")), "the running state, plus its line");
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_the_crash_loop_copy_names_the_exits_backoff_gives_up_at() {
        let page = include_str!("../../ui/settings.html");
        let n = |pre: &str, post: &str| -> Vec<usize> { page.match_indices(pre).filter_map(|(i, _)| page[i + pre.len()..].split_once(post).and_then(|(d, _)| d.parse().ok())).collect() };
        assert_eq!(n("crash_loop: 'It stopped ", " times"), [GIVE_UP], "EN");
        assert_eq!(n("crash_loop: 'Unfold Automagik parou ", " vezes"), [GIVE_UP], "PT");
        let now = Instant::now();
        assert!(backoff(&vec![now; GIVE_UP - 1], now).is_some(), "one fewer: it tries again");
        assert_eq!(backoff(&vec![now; GIVE_UP], now), None, "at the limit: it stops");
    }

    #[test]
    fn dsh_child_a_pack_whose_files_changed_is_refused_with_the_changed_notice_and_doesn_t_start() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, _owned, logged) = switch::ready();
        switch::CHANGED.store(7, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?} {:?}", own.view(), switch::events());
        assert_eq!(own.view()["running"].as_bool(), Some(false), "refused: it doesn't run");
        assert!(!switch::events().iter().any(|e| e.starts_with("start:")), "no start: {:?}", switch::events());
        let v = own.view();
        assert_eq!((v["changed"].clone(), v["changed_n"].as_u64(), v["refetch"].as_bool()), (json!(["app/f1.js", "app/f2.js", "app/f3.js"]), Some(7), Some(true)), "it names three of the 7 changed files, and Fetch again is offered");
        let all = (1..=7).map(|i| format!("app/f{i}.js")).collect::<Vec<_>>().join(", ");
        assert!(logged().contains(&format!("7 of its files differ from the pack's manifest: {all}\n")), "the log names them all: {}", logged());
        let list = dsh_dir(&switch::root().join("instances/i1")).join(CHANGED_LIST);
        assert_eq!(std::fs::read_to_string(&list).ok(), Some((1..=7).map(|i| format!("app/f{i}.js\n")).collect::<String>()), "the list file has them all");
        assert_eq!(std::os::unix::fs::MetadataExt::mode(&std::fs::metadata(&list).unwrap()) & 0o777, 0o600, "private");
        assert!(!switch::events().iter().any(|e| e == "refetch"), "nothing fetched until Fetch again");
        own.set_on(false);
    }

    #[test]
    fn dsh_child_a_changed_file_named_with_a_newline_can_t_forge_a_log_line() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, _owned, logged) = switch::ready();
        let forged = "app/x.js\n1 dsh: Unfold's own DSH is running";
        *switch::CHANGED_NAMES.lock().unwrap() = vec![forged.into(), "app/y\r.js".into()];
        switch::CHANGED.store(2, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());
        let text = logged();
        assert!(text.contains(r"2 of its files differ from the pack's manifest: app/x.js\n1 dsh: Unfold's own DSH is running, app/y\r.js"), "escaped, on one line: {text}");
        assert!(!text.lines().any(|l| l.starts_with("1 dsh: Unfold's own DSH is running")), "no forged line: {text}");
        let list = std::fs::read_to_string(dsh_dir(&switch::root().join("instances/i1")).join(CHANGED_LIST)).unwrap();
        assert_eq!(list, "app/x.js\\n1 dsh: Unfold's own DSH is running\napp/y\\r.js\n", "the list file: a name a line, escaped too");
        own.set_on(false);
    }

    #[test]
    fn dsh_child_a_changed_list_it_can_t_write_is_said_in_the_log_line_which_points_at_no_file() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, _owned, logged) = switch::ready();

        let dsh = dsh_dir(&switch::root().join("instances/i1"));
        let list = dsh.join(CHANGED_LIST);

        let _ = std::fs::remove_file(&list);
        std::fs::create_dir_all(list.join("x")).unwrap();
        switch::CHANGED.store(25, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());
        let text = logged();
        let line = text.lines().filter(|l| l.contains("differ from the pack's manifest")).last().unwrap().to_string();
        own.set_on(false);
        let _ = std::fs::remove_dir_all(&list);
        let _ = std::fs::remove_file(dsh.join("pack-changed.tmp"));
        assert!(line.contains("25 of its files differ") && line.contains(" and 5 more; the full list couldn't be written: "), "{line}");
        assert!(!line.contains(CHANGED_LIST), "the line points at no list file: {line}");
    }

    #[test]
    fn dsh_child_a_huge_changed_list_keeps_the_log_line_and_the_list_file_bounded() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, _owned, logged) = switch::ready();
        let n = 200_000;
        *switch::CHANGED_NAMES.lock().unwrap() = (0..n).map(|i| format!("app/{i:06}-{}.js", "x".repeat(if i == 0 { 5000 } else { 20 }))).collect();
        switch::CHANGED.store(n, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(20), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());
        assert_eq!(own.view()["changed_n"].as_u64(), Some(n as u64));
        let text = logged();
        let line = text.lines().filter(|l| l.contains("differ from the pack's manifest")).last().unwrap();
        assert!(line.contains(&format!("{n} of its files differ")) && line.ends_with(&format!(" and {} more (all in {CHANGED_LIST})", n - CHANGED_IN_LOG)), "{}", &line[line.len().saturating_sub(200)..]);
        assert!(line.len() < 20 * 300 + 300, "the line stays short: {} bytes", line.len());
        assert!(line.contains(&format!("app/000000-{}…, app/000001-", "x".repeat(CHANGED_NAME_MAX - 11))), "a long name is cut in the log");
        let list = std::fs::read_to_string(dsh_dir(&switch::root().join("instances/i1")).join(CHANGED_LIST)).unwrap();
        assert!(list.len() <= CHANGED_LIST_MAX, "bounded: {} bytes", list.len());
        let kept = list.lines().count() - 1;
        assert!(kept > 1000 && list.ends_with(&format!("and {} more\n", n - kept)), "the list names what fits and how many it leaves out: {kept}");
        assert!(list.starts_with(&format!("app/000000-{}.js\n", "x".repeat(5000))), "the file keeps whole names");
        own.set_on(false);
    }

    fn at(events: &[String], from: usize, pick: impl Fn(&str) -> bool) -> Option<usize> {
        events.iter().enumerate().skip(from).find(|(_, e)| pick(e)).map(|(i, _)| i)
    }

    fn phases(seen: &[Option<&'static str>]) -> Vec<Option<&'static str>> {
        let mut out: Vec<Option<&'static str>> = Vec::new();
        for p in seen {
            if out.last() != Some(p) {
                out.push(*p);
            }
        }
        out
    }

    #[test]
    fn dsh_child_a_first_start_shows_the_download_then_each_step_then_no_bar_once_it_runs() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::PACK_STEPS.store(80, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        let seen = switch::SEEN.lock().unwrap().clone();
        let shown = phases(&seen);
        let at = |ph: &str| shown.iter().position(|p| *p == Some(ph)).unwrap_or_else(|| panic!("{ph} never shown: {shown:?}"));
        let order = ["download", "verify", "unpack", "check", "start"].map(at);
        assert!(order.windows(2).all(|w| w[0] < w[1]), "in order: {shown:?}");
        assert_eq!(shown.last(), Some(&None), "no bar once it runs: {shown:?}");

        let downloads = seen.iter().filter(|p| **p == Some("download")).count() as u64;
        let most = switch::DL_MS.load(SEQ) / 250 + 1;
        assert!(downloads >= 1 && downloads <= most, "{downloads} download refreshes for 81 ticks in {} ms", switch::DL_MS.load(SEQ));
        assert_eq!((own.view()["progress"].clone(), own.main_view()), (Value::Null, json!({"on": true, "running": true, "notice": null, "progress": null})));

        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
        switch::PACK_STEPS.store(0, SEQ);
        switch::SEEN.lock().unwrap().clear();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        assert!(switch::SEEN.lock().unwrap().iter().all(Option::is_none), "{:?}", switch::SEEN.lock().unwrap());
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_a_fetch_step_shows_its_bytes_of_the_pin_s_size_and_a_switch_off_drops_the_bar() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, _owned, _) = switch::ready();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        let gen = own.gen.load(SEQ);
        own.step(gen, crate::dsh_pack::Step::Download { done: 128_000_000, total: 353_878_514 });
        assert_eq!(own.view()["progress"], json!({"phase": "download", "done": 128_000_000u64, "total": 353_878_514u64}));
        assert_eq!(own.main_view()["progress"]["phase"], "download");

        own.set_on(false);
        own.step(gen, crate::dsh_pack::Step::Unpack);
        assert_eq!(own.view()["progress"], Value::Null, "{:?}", own.view());
    }

    #[test]
    fn dsh_child_fetch_again_stops_the_instance_then_fetches_the_pack_once_then_starts_it() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::CHANGED.store(3, SEQ);
        switch::REFETCH_MS.store(300, SEQ);
        switch::PACK_STEPS.store(10, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());

        own.fetch_again();
        own.fetch_again();
        assert!(eventually(Duration::from_secs(5), || own.view()["fetching"] == true), "{:?}", own.view());
        assert!(eventually(Duration::from_secs(5), || switch::events().iter().any(|e| e == "refetch")), "{:?}", switch::events());
        own.fetch_again();
        assert_eq!((own.view()["running"].as_bool(), own.view()["notice"].clone()), (Some(false), Value::Null), "fetching: not running, no notice");
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        let ev = switch::events();
        let refetch = at(&ev, 0, |e| e == "refetch").expect("fetched");
        assert_eq!(ev.iter().filter(|e| *e == "refetch").count(), 1, "once: {ev:?}");
        assert!(at(&ev, 0, |e| e == "omni:stop").is_some_and(|i| i < refetch), "the refusing run's Omni stopped before the fetch: {ev:?}");
        assert!(at(&ev, refetch, |e| e.starts_with("start:")).is_some(), "then a new start: {ev:?}");
        assert!(!ev[..refetch].iter().any(|e| e.starts_with("start:")), "no start before it: {ev:?}");
        assert_eq!((own.view()["notice"].clone(), own.view()["fetching"].as_bool(), own.view()["changed"].clone(), own.view()["changed_n"].as_u64()), (Value::Null, Some(false), json!([]), Some(0)));
        let shown = phases(&switch::SEEN.lock().unwrap());
        assert!(shown.iter().position(|p| *p == Some("check")).is_some_and(|c| shown.iter().position(|p| *p == Some("start")).is_some_and(|s| c < s)), "Fetch again's steps, then Starting: {shown:?}");
        assert_eq!(own.view()["progress"], Value::Null);
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_fetch_again_does_nothing_without_the_changed_notice_or_for_a_placed_pack() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        own.fetch_again();
        std::thread::sleep(Duration::from_millis(300));
        assert!(switch::running(own) && !switch::events().iter().any(|e| e == "refetch"), "a running DSH is left alone: {:?}", switch::events());
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());

        switch::CHANGED.store(2, SEQ);
        switch::PLACED.store(true, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());
        assert_eq!(own.view()["refetch"].as_bool(), Some(false));
        own.fetch_again();
        std::thread::sleep(Duration::from_millis(300));
        assert!(own.view()["notice"] == "pack_changed" && !switch::events().iter().any(|e| e == "refetch"), "{:?}", switch::events());
        own.set_on(false);
    }

    #[test]
    fn dsh_child_a_failed_fetch_again_says_so_and_starts_nothing_until_the_next_one() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::CHANGED.store(1, SEQ);
        switch::REFETCH_FAILS.store(true, SEQ);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "pack_changed"), "{:?}", switch::events());
        own.fetch_again();
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "fetch_failed"), "{:?}", own.view());
        assert_eq!((own.view()["running"].as_bool(), own.view()["fetching"].as_bool()), (Some(false), Some(false)));
        let ev = switch::events();
        let refetch = at(&ev, 0, |e| e == "refetch").expect("tried");
        assert!(!ev[refetch..].iter().any(|e| e.starts_with("start:")), "nothing started: {ev:?}");

        switch::REFETCH_FAILS.store(false, SEQ);
        own.fetch_again();
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        assert_eq!(switch::events().iter().filter(|e| *e == "refetch").count(), 2);
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_a_pack_that_never_runs_healthy_is_marked_bad_once_its_run_gives_up() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::FAST.store(true, SEQ);
        switch::MODES.lock().unwrap().extend([(0, true); 6]);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || own.view()["notice"] == "crash_loop"), "{:?}", switch::events());
        let ev = switch::events();
        assert_eq!(ev.iter().filter(|e| e.starts_with("bad:")).collect::<Vec<_>>(), [&"bad:the fake failed".to_string()], "marked bad once, with why: {ev:?}");
        assert!(!ev.contains(&"healthy".to_string()));
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()), "{:?}", switch::events());
    }

    #[test]
    fn dsh_child_off_then_on_during_a_slow_start_leaves_one_run_one_child_one_endpoint() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::MODES.lock().unwrap().extend([(1500, false), (0, false)]);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(5), || switch::events().contains(&"start:served".to_string())));
        own.set_on(false);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own) && switch::events().contains(&"start:ok".to_string())), "{:?}", switch::events());
        std::thread::sleep(Duration::from_millis(1800));
        assert_eq!(switch::MAX_ACTIVE.load(SEQ), 1, "two starts ran at once: {:?}", switch::events());
        assert_eq!(switch::OMNI_MAX.load(SEQ), 1, "two Omni starts ran at once: {:?}", switch::events());
        assert_eq!((switch::kids(&owned), switch::socket(), switch::running(own)), (1, true, true), "exactly one child and one endpoint: {:?}", switch::events());
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()));
    }

    #[test]
    fn dsh_child_an_older_run_that_ends_never_stops_the_newer_runs_endpoint() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::MODES.lock().unwrap().extend([(800, true), (0, false)]);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(5), || switch::events().contains(&"start:served".to_string())));

        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());
        std::thread::sleep(Duration::from_millis(2000));
        assert_eq!((switch::kids(&owned), switch::socket(), switch::running(own)), (1, true, true), "the newer run's child and endpoint stay: {:?}", switch::events());
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()));
    }

    #[test]
    fn dsh_child_rapid_switch_changes_end_off_with_no_child_and_no_endpoint() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::MODES.lock().unwrap().extend([(300, false); 8]);
        for i in 0..8u64 {
            own.set_on(true);
            std::thread::sleep(Duration::from_millis(15 * (i % 4)));
            own.set_on(false);
        }
        assert!(eventually(Duration::from_secs(15), || switch::ACTIVE.load(SEQ) == 0 && switch::OMNI_ACTIVE.load(SEQ) == 0), "{:?}", switch::events());
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!((switch::kids(&owned), switch::socket(), own.current.lock().unwrap().is_none()), (0, false, true), "off: no child, no endpoint: {:?}", switch::events());
        assert!(switch::MAX_ACTIVE.load(SEQ) <= 1 && switch::OMNI_MAX.load(SEQ) <= 1, "two starts ran at once: {:?}", switch::events());
    }

    #[test]
    fn dsh_child_a_failed_start_leaves_no_endpoint_while_it_backs_off() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::MODES.lock().unwrap().extend([(0, true), (0, false)]);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(5), || switch::events().contains(&"start:failed".to_string())));

        assert!(eventually(Duration::from_millis(500), || !switch::socket()), "an endpoint stayed served without its child: {:?}", switch::events());
        assert!(eventually(Duration::from_secs(5), || switch::running(own)), "started again after the backoff");
        assert!(switch::socket());
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()));
    }

    #[test]
    fn dsh_child_use_another_port_and_retry_do_nothing_while_the_switch_is_off() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        own.retry();
        assert_eq!(own.another_port(), Ok(()));
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!((switch::STARTS.load(SEQ), own.status.lock().unwrap().on, switch::kids(&owned)), (0, false, 0), "a click while off turned it on: {:?}", switch::events());
    }

    #[test]
    fn dsh_child_switching_off_checks_omnis_ports_too() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, lines) = switch::ready();
        let held = std::net::TcpListener::bind(("127.0.0.1", switch::PORTS[2].load(SEQ))).unwrap();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)));
        own.set_on(false);
        let api = held.local_addr().unwrap().port();
        assert!(eventually(Duration::from_secs(10), || lines().contains(&format!("port {api} is still in use after the stop"))), "Omni's API port isn't checked: {}", lines());
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()));
    }

    #[test]
    fn dsh_child_a_start_that_ends_after_a_switch_off_is_stopped_not_kept() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        switch::DEAF.store(true, SEQ);
        switch::MODES.lock().unwrap().push_back((800, false));
        own.set_on(true);
        assert!(eventually(Duration::from_secs(5), || switch::events().contains(&"start:served".to_string())));
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::ACTIVE.load(SEQ) == 0 && switch::events().contains(&"omni:stop".to_string())), "{:?}", switch::events());
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!((switch::kids(&owned), switch::socket(), own.current.lock().unwrap().is_none()), (0, false, true), "the late child was kept: {:?}", switch::events());
    }

    #[test]
    fn dsh_child_a_switch_off_after_use_another_port_wins() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)));

        let held = crate::dsh_pack::Lock::take(&switch::root().join("dsh-ports.lock"), Duration::from_secs(5)).unwrap();
        assert_eq!(own.another_port(), Ok(()));
        own.set_on(false);
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
        let reg = switch::root().join("dsh-ports.json");
        assert!(eventually(Duration::from_secs(5), || std::fs::read_to_string(&reg).is_ok_and(|s| s.contains("retired"))), "the other port is registered");
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!((own.status.lock().unwrap().on, switch::STARTS.load(SEQ), switch::kids(&owned), switch::socket()), (false, 1, 0, false), "the click turned it back on: {:?}", switch::events());
    }

    #[test]
    fn dsh_child_a_switch_change_during_the_profile_install_cancels_the_start_at_once() {
        let (root, mut c) = fake("cancel");
        std::fs::write(c.pack.join("install.mjs"), "await new Promise((r) => setTimeout(r, 5000))\n").unwrap();
        c.ready_within = Duration::from_secs(20);
        let (log, _) = logged(&root);
        let owned: Shared = Arc::default();
        let children = dsh_dir(&c.instance_root).join("children.json");
        let t0 = Instant::now();
        let r = start(&c, &owned, &log, &|| t0.elapsed() < Duration::from_millis(600));
        let took = t0.elapsed();
        if let Ok(d) = &r {
            stop(d, &c.instance_root, &owned, &log);
        }
        assert!(matches!(r, Err(StartError::Cancelled)), "{r:?}");
        assert!(took < Duration::from_secs(3), "the start waited {took:?} after the switch changed");
        assert!(owned.lock().unwrap().recorded.is_empty(), "the launcher is stopped");
        assert!(read_records(&children).is_empty(), "its record goes with it");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dsh_child_an_older_switch_change_stalled_through_a_newer_start_does_nothing() {
        let _s = switch::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (own, owned, _) = switch::ready();
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::running(own)), "{:?}", switch::events());

        *switch::STALL.lock().unwrap() = Some(own.gen.load(SEQ) + 1);
        own.set_on(false);
        own.set_on(true);
        assert!(eventually(Duration::from_secs(10), || switch::STARTS.load(SEQ) == 2 && switch::running(own)), "the newer start: {:?}", switch::events());
        let newer = own.current.lock().unwrap().as_ref().map(|d| d.pid);

        switch::release();
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!((switch::kids(&owned), switch::socket(), switch::running(own)), (1, true, true), "the older change stopped the newer run: {:?}", switch::events());
        assert_eq!(own.current.lock().unwrap().as_ref().map(|d| d.pid), newer, "the newer child stays current");
        own.set_on(false);
        assert!(eventually(Duration::from_secs(10), || switch::kids(&owned) == 0 && !switch::socket()));
    }

    #[test]
    fn dsh_child_quit_stops_agentd_before_the_endpoint_so_a_submit_in_flight_cant_hold_it() {
        let root = sock_root("quit");
        let (log, _) = logged(&root);
        use std::os::unix::process::CommandExt as _;
        let mut agent = Command::new("/bin/sh");
        agent.args(["-c", "echo \"$$ agent-fake $0\" >> \"$1\"; exec /bin/sleep 30"]).arg(root.display().to_string()).arg(pids_file()).process_group(0);
        let child = agent.spawn().unwrap();
        let pid = child.id();

        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let s2 = seen.clone();
        ON_CHANNEL_STOP.with(|h| *h.borrow_mut() = Some(Box::new(move || {
            s2.set(Some(alive(pid)));
            std::thread::sleep(Duration::from_secs(2));
        })));
        let stops = CHANNEL_STOPS.with(|n| n.get());
        let t0 = Instant::now();
        crate::stop_taken(crate::Taken { agent: Some(child), llm: None, recorded: Vec::new() }, &log);
        ON_CHANNEL_STOP.with(|h| *h.borrow_mut() = None);
        assert_eq!(CHANNEL_STOPS.with(|n| n.get()), stops + 1, "Quit stops the endpoint");
        assert_eq!(seen.get(), Some(false), "agentd still ran when the endpoint's stop began");
        assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
        let _ = std::fs::remove_dir_all(&root);
    }
}
