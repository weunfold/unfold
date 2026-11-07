

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const AUTOPG_LABEL: &str = "io.namastex.brain.personal.postgres";

pub const AUTOPG_VERSION: &str = "v3.3.1";

pub const AUTOPG_ACCEPTED_EARLIER: &str = "v3.2.2";
pub const MEMORY_UNAVAILABLE: &str = "Local memory service unavailable — this build needs the existing Brain/autopg service.";

#[derive(Clone, Debug, PartialEq)]
pub struct AutopgBinding {
    pub plist: PathBuf,
    pub program: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket_dir: PathBuf,
    pub port: u16,
    pub pid: i32,
    pub autopg_pid: i32,
}

pub fn read_service_record(path: &Path) -> Result<Value, String> {
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

pub fn autopg_service(record: &Value, home: &Path) -> Result<(PathBuf, PathBuf, PathBuf, u16), String> {
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

pub fn autopg_release(program: &Path) -> Option<&str> {
    let dir = program.parent()?;
    (program.is_absolute() && program.file_name()? == "autopg" && dir.parent()?.ends_with(".local/share/autopg")).then(|| dir.file_name()?.to_str())?
}

pub fn pid_path(pid: i32) -> Option<PathBuf> {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| PathBuf::from(String::from_utf8_lossy(&buf[..n as usize]).into_owned()))
}

pub fn autopg_runtime(plist: &Path, program: PathBuf, runtime_dir: PathBuf, socket_dir: PathBuf, port: u16) -> Result<AutopgBinding, String> {
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

pub fn discover_autopg(home: &Path) -> Result<AutopgBinding, String> {
    let plist = home.join("Library/LaunchAgents").join(format!("{AUTOPG_LABEL}.plist"));
    let (program, rt, socket_dir, port) = autopg_service(&read_service_record(&plist)?, home)?;
    autopg_runtime(&plist, program, rt, socket_dir, port)
}

pub fn binding_json(b: &AutopgBinding) -> Value {
    serde_json::json!({
        "label": AUTOPG_LABEL, "version": AUTOPG_VERSION, "plist": b.plist, "program": b.program,
        "runtime_dir": b.runtime_dir, "socket_dir": b.socket_dir, "port": b.port, "pid": b.pid, "autopg_pid": b.autopg_pid,
    })
}

pub fn same_service(accepted: &Value, fresh: &Value) -> bool {
    let all = |v: &Value| ["label", "version", "plist", "program", "runtime_dir", "socket_dir", "port"].map(|k| v.get(k).cloned());
    let rest = |v: &Value| ["label", "plist", "runtime_dir", "socket_dir", "port"].map(|k| v.get(k).cloned());
    let root_at = |v: &Value, release: &str| -> Option<PathBuf> {
        let program = Path::new(v.get("program")?.as_str()?);
        (v.get("version")?.as_str()? == release && autopg_release(program)? == release).then(|| program.parent()?.parent().map(Path::to_path_buf))?
    };
    let earlier = root_at(accepted, AUTOPG_ACCEPTED_EARLIER);
    all(accepted) == all(fresh) || (rest(accepted) == rest(fresh) && earlier.is_some() && earlier == root_at(fresh, AUTOPG_VERSION))
}

pub fn stable(v: &Value) -> [Option<Value>; 7] {
    ["label", "version", "plist", "program", "runtime_dir", "socket_dir", "port"].map(|k| v.get(k).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rec(version: &str, program: &str) -> Value {
        json!({"label": AUTOPG_LABEL, "version": version, "plist": "/Users/u/Library/LaunchAgents/io.namastex.brain.personal.postgres.plist",
            "program": program, "runtime_dir": "/private/tmp/brain-autopg-501", "socket_dir": "/private/tmp/brain-autopg-501/pgserve", "port": 5432, "pid": 2, "autopg_pid": 3})
    }

    #[test]
    fn only_an_accepted_v3_2_2_moving_to_v3_3_1_with_all_else_equal_is_the_same_service() {
        let a = rec("v3.2.2", "/Users/u/.local/share/autopg/v3.2.2/autopg");
        let up = rec("v3.3.1", "/Users/u/.local/share/autopg/v3.3.1/autopg");
        let with = |k: &str, v: Value| {
            let mut r = up.clone();
            r[k] = v;
            r
        };
        assert!(same_service(&up, &up) && same_service(&a, &up) && same_service(&a, &with("pid", json!(9))));
        for (accepted, fresh, why) in [
            (&a, with("port", json!(5433)), "port"),
            (&a, with("socket_dir", json!("/private/tmp/other/pgserve")), "socket dir"),
            (&a, with("runtime_dir", json!("/private/tmp/other")), "runtime dir"),
            (&a, with("plist", json!("/Users/u/Library/LaunchAgents/other.plist")), "plist"),
            (&a, with("label", json!("other")), "label"),
            (&up, a.clone(), "a downgrade"),
            (&rec("v3.2.1", "/Users/u/.local/share/autopg/v3.2.1/autopg"), up.clone(), "accepted v3.2.1"),
            (&rec("v9.9.9", "/Users/u/.local/share/autopg/v9.9.9/autopg"), up.clone(), "accepted v9.9.9"),
            (&a, rec("v9.9.9", "/Users/u/.local/share/autopg/v9.9.9/autopg"), "live v9.9.9"),
            (&rec("v3.2.2", "/Users/v/.local/share/autopg/v3.2.2/autopg"), up.clone(), "another home"),
            (&rec("v3.2.2", "/opt/autopg/v3.2.2/autopg"), rec("v3.3.1", "/opt/autopg/v3.3.1/autopg"), "outside .local/share/autopg"),
            (&rec("v3.2.2", "/Users/u/.local/share/autopg/v3.2.1/autopg"), up.clone(), "version and program disagree"),
            (&a, rec("v3.3.1", "/Users/u/.local/share/autopg/v3.3.1/postgres"), "not autopg"),
            (&rec("v3.2.2", "Users/u/.local/share/autopg/v3.2.2/autopg"), rec("v3.3.1", "Users/u/.local/share/autopg/v3.3.1/autopg"), "relative"),
        ] {
            assert!(!same_service(accepted, &fresh), "{why}");
        }
        assert_eq!(autopg_release(Path::new("/h/.local/share/autopg/v3.3.1/autopg")), Some("v3.3.1"));
        assert_eq!(autopg_release(Path::new("/h/autopg/v3.3.1/autopg")), None);
    }
}
