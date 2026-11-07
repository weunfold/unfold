

use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub pid: i32,
    pub pgid: i32,
    pub start_us: u64,
}

pub fn start_us(pid: i32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

pub fn group_alive(pgid: i32) -> io::Result<bool> {
    if pgid <= 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "no such process group"));
    }
    if unsafe { libc::killpg(pgid, 0) } == 0 {
        return Ok(true);
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(false),

        _ => Ok(true),
    }
}

pub fn group_members(pgid: i32) -> Vec<i32> {
    let mut buf = vec![0 as libc::pid_t; 4096];
    let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as i32;
    let n = unsafe { libc::proc_listpgrppids(pgid, buf.as_mut_ptr().cast(), bytes) };
    if n <= 0 {
        return Vec::new();
    }

    buf.retain(|&p| p > 0);
    buf
}

pub const QUIESCE_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);
const QUIESCE_POLL: std::time::Duration = std::time::Duration::from_millis(25);

pub fn quiesce_within(fence: &Path, pgid: Option<i32>, within: std::time::Duration) -> io::Result<Quiescence> {
    let end = std::time::Instant::now() + within;
    loop {
        match quiesce(fence, pgid)? {
            Quiescence::Quiet(g) => return Ok(Quiescence::Quiet(g)),
            q if std::time::Instant::now() >= end => return Ok(q),
            _ => std::thread::sleep(QUIESCE_POLL),
        }
    }
}

pub fn reap_own_group(grace: std::time::Duration, term: std::time::Duration, kill: std::time::Duration) -> Vec<i32> {
    let me = std::process::id() as i32;
    if unsafe { libc::getsid(0) } != me || unsafe { libc::getpgrp() } != me {
        return Vec::new();
    }
    let others = || group_members(me).into_iter().filter(|&p| p != me).filter_map(|p| start_us(p).map(|s| (p, s))).collect::<Vec<_>>();
    for (signal, wait) in [(None, grace), (Some(libc::SIGTERM), term), (Some(libc::SIGKILL), kill)] {
        let left = others();
        if left.is_empty() {
            return Vec::new();
        }
        if let Some(sig) = signal {
            for (p, s) in &left {

                if start_us(*p) == Some(*s) {
                    unsafe { libc::kill(*p, sig) };
                }
            }
        }
        let end = std::time::Instant::now() + wait;
        while std::time::Instant::now() < end && !others().is_empty() {
            std::thread::sleep(QUIESCE_POLL);
        }
    }
    others().into_iter().map(|(p, _)| p).collect()
}

pub struct Pending {
    child: Child,
    stdin: ChildStdin,
    pub identity: Identity,
}

pub fn start(exe: &Path, fence: &Path, command: &[String], env: &[(String, String)], cwd: &Path, stderr: Stdio) -> io::Result<Pending> {
    let mut cmd = Command::new(exe);
    cmd.arg("--effect-helper").arg(fence).arg("--").args(command).env_clear().envs(env.iter().map(|(k, v)| (k, v))).current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(stderr);
    unsafe {
        cmd.pre_exec(|| if libc::setsid() < 0 { Err(io::Error::last_os_error()) } else { Ok(()) });
    }
    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line)?;
    let identity: Identity = serde_json::from_str(line.trim()).map_err(|e| io::Error::other(format!("the effect helper did not report its identity ({e}): {line:?}")))?;
    if identity.pid != child.id() as i32 || identity.pgid != identity.pid {
        return Err(io::Error::other(format!("the effect helper reported a foreign identity {identity:?}")));
    }
    Ok(Pending { child, stdin, identity })
}

impl Pending {

    pub fn go(mut self) -> io::Result<Child> {
        self.stdin.write_all(b"g")?;
        self.stdin.flush()?;
        drop(self.stdin);
        Ok(self.child)
    }

    pub fn abandon(self) -> io::Result<ExitStatus> {
        drop(self.stdin);
        let mut child = self.child;
        child.wait()
    }
}

pub const EXIT_NO_GO: i32 = 70;
pub const EXIT_BAD_GO: i32 = 71;
pub const EXIT_FENCE: i32 = 72;
pub const EXIT_EXEC: i32 = 73;

pub fn helper_main(args: &[String]) -> ! {
    let fail = |code: i32, why: &str| -> ! {
        let _ = writeln!(io::stderr(), "rth-activate effect helper: {why}");
        std::process::exit(code)
    };
    let (fence, command) = match args {
        [fence, dash, rest @ ..] if dash == "--" && !rest.is_empty() => (fence.clone(), rest.to_vec()),
        _ => fail(64, "usage: --effect-helper <fence> -- <command…>"),
    };
    let pid = std::process::id() as i32;
    let id = Identity { pid, pgid: unsafe { libc::getpgrp() }, start_us: start_us(pid).unwrap_or(0) };
    {
        let mut out = io::stdout().lock();
        if writeln!(out, "{}", serde_json::to_string(&id).unwrap()).and_then(|_| out.flush()).is_err() {
            fail(EXIT_NO_GO, "cannot report the identity");
        }
    }
    let mut go = [0u8; 1];
    match io::stdin().read(&mut go) {
        Ok(0) | Err(_) => std::process::exit(EXIT_NO_GO),
        Ok(_) if go[0] != b'g' => fail(EXIT_BAD_GO, "not a go byte"),
        Ok(_) => {}
    }

    let c = CString::new(fence.as_str()).unwrap_or_else(|_| fail(EXIT_FENCE, "fence path"));
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_NOFOLLOW, 0o600) };
    if fd < 0 || unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
        fail(EXIT_FENCE, &format!("cannot take the effect fence {fence}: {}", io::Error::last_os_error()));
    }

    let null = std::fs::OpenOptions::new().read(true).write(true).open("/dev/null").unwrap_or_else(|_| fail(EXIT_EXEC, "/dev/null"));
    unsafe {
        libc::dup2(null.as_raw_fd(), 0);
        libc::dup2(null.as_raw_fd(), 1);
    }
    let argv: Vec<CString> = command.iter().map(|a| CString::new(a.as_str()).unwrap_or_else(|_| fail(EXIT_EXEC, "argv"))).collect();
    let mut ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    unsafe { libc::execv(ptrs[0], ptrs.as_ptr()) };
    fail(EXIT_EXEC, &format!("exec {}: {}", command[0], io::Error::last_os_error()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {

    NotAuthorized,

    NotRun,

    MayHaveRun,
}

pub fn classify(intent: bool, release_intent: bool) -> Classification {
    match (intent, release_intent) {
        (false, _) => Classification::NotAuthorized,
        (true, false) => Classification::NotRun,
        (true, true) => Classification::MayHaveRun,
    }
}

pub enum Quiescence {

    Quiet(crate::lease::Lease),
    FenceBusy,
    GroupAlive,
}

pub fn quiesce(fence: &Path, pgid: Option<i32>) -> io::Result<Quiescence> {
    let guard = match crate::lease::Lease::acquire(fence, crate::lease::Mode::Exclusive) {
        Ok(g) => g,
        Err(crate::lease::LeaseError::Busy) => return Ok(Quiescence::FenceBusy),
        Err(e) => return Err(io::Error::other(e.to_string())),
    };
    if let Some(pgid) = pgid
        && group_alive(pgid)?
    {
        return Ok(Quiescence::GroupAlive);
    }
    Ok(Quiescence::Quiet(guard))
}
