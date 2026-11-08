

use std::io;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LiveChecks {

    #[serde(default)]
    pub flocks: Vec<PathBuf>,

    #[serde(default)]
    pub ports: Vec<u16>,

    #[serde(default)]
    pub argv_markers: Vec<String>,
}

pub fn live(c: &LiveChecks) -> io::Result<Vec<String>> {
    let mut found = Vec::new();
    for f in &c.flocks {

        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(f) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                found.push(format!("{} cannot be inspected ({e})", f.display()));
                continue;
            }
        };
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            found.push(format!("an agent holds {}", f.display()));
        }

    }
    for p in &c.ports {
        for v6 in [false, true] {
            match port_free(*p, v6) {
                Ok(true) => {}
                Ok(false) => found.push(format!("port {p} is in use{}", if v6 { " (IPv6)" } else { "" })),
                Err(e) => found.push(format!("port {p} cannot be inspected ({e})")),
            }
        }
    }
    if !c.argv_markers.is_empty() {
        found.extend(marked_processes(&c.argv_markers)?);
    }
    Ok(found)
}

fn port_free(port: u16, v6: bool) -> io::Result<bool> {
    unsafe {
        let fd = libc::socket(if v6 { libc::AF_INET6 } else { libc::AF_INET }, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let r = if v6 {
            let one: libc::c_int = 1;
            libc::setsockopt(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, &one as *const _ as *const libc::c_void, std::mem::size_of::<libc::c_int>() as libc::socklen_t);
            let mut a: libc::sockaddr_in6 = std::mem::zeroed();
            a.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
            a.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            a.sin6_port = port.to_be();
            libc::bind(fd, &a as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t)
        } else {
            let mut a: libc::sockaddr_in = std::mem::zeroed();
            a.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            a.sin_family = libc::AF_INET as libc::sa_family_t;
            a.sin_port = port.to_be();
            libc::bind(fd, &a as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t)
        };
        let err = io::Error::last_os_error();
        libc::close(fd);
        match r {
            0 => Ok(true),
            _ if err.raw_os_error() == Some(libc::EADDRINUSE) => Ok(false),

            _ if v6 && err.raw_os_error() == Some(libc::EADDRNOTAVAIL) => Ok(true),
            _ => Err(err),
        }
    }
}

fn bsdinfo(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    (n == size).then_some(info)
}

fn shortinfo(pid: i32) -> Option<libc::proc_bsdshortinfo> {
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDT_SHORTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    (n == size).then_some(info)
}

pub fn is_zombie(pid: i32) -> bool {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid];
    let mut buf = [0u8; 648];
    let mut size: libc::size_t = buf.len();
    let r = unsafe { libc::sysctl(mib.as_mut_ptr(), 4, buf.as_mut_ptr() as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0) };
    r == 0 && size == buf.len() && buf[36] as u32 == libc::SZOMB
}

fn argv(pid: i32) -> Option<Vec<Vec<u8>>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) } != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0) } != 0 || size < 4 {
        return None;
    }
    buf.truncate(size);
    let argc = i32::from_ne_bytes(buf[..4].try_into().ok()?) as usize;
    let mut rest = &buf[4..];

    let end = rest.iter().position(|b| *b == 0)?;
    rest = &rest[end..];
    let start = rest.iter().position(|b| *b != 0)?;
    rest = &rest[start..];
    let mut out = Vec::with_capacity(argc);
    for part in rest.split(|b| *b == 0).take(argc) {
        out.push(part.to_vec());
    }
    Some(out)
}

pub fn marked_processes(markers: &[String]) -> io::Result<Vec<String>> {
    let me = std::process::id() as i32;
    let uid = unsafe { libc::getuid() };
    let mut pids = vec![0i32; 8192];
    let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr() as *mut libc::c_void, (pids.len() * std::mem::size_of::<i32>()) as libc::c_int) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n as usize >= pids.len() {
        return Err(io::Error::other("the process table did not fit: incomplete"));
    }
    pids.truncate(n as usize);
    let mut found = Vec::new();
    for pid in pids.into_iter().filter(|p| *p > 0 && *p != me) {

        let mut verdict = classify(&observe(pid, uid), uid, markers);
        for _ in 0..10 {
            if verdict != Inspected::Unknown {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            verdict = classify(&observe(pid, uid), uid, markers);
        }
        match verdict {
            Inspected::Clear => {}
            Inspected::Marked => found.push(format!("process {pid} belongs to the current RTH")),
            Inspected::Unknown => found.push(format!("process {pid} cannot be inspected")),
        }
    }
    Ok(found)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Inspected {

    Clear,
    Marked,

    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signal {
    Alive,

    Gone,

    Failed(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Observation {

    pub uid: Option<libc::uid_t>,

    pub zombie: bool,
    pub signal: Signal,

    pub argv: Option<Vec<Vec<u8>>>,
}

fn observe(pid: i32, my_uid: libc::uid_t) -> Observation {
    let info = bsdinfo(pid).map(|i| (i.pbi_uid, i.pbi_status)).or_else(|| shortinfo(pid).map(|i| (i.pbsi_uid, i.pbsi_status)));
    let zombie = info.is_some_and(|(_, st)| st == libc::SZOMB) || is_zombie(pid);
    let uid = info.map(|(u, _)| u);
    let argv = if uid == Some(my_uid) && !zombie { argv(pid) } else { None };
    let signal = if unsafe { libc::kill(pid, 0) } == 0 {
        Signal::Alive
    } else {
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Signal::Gone,
            e => Signal::Failed(e.unwrap_or(-1)),
        }
    };
    Observation { uid, zombie, signal, argv }
}

pub fn classify(o: &Observation, my_uid: libc::uid_t, markers: &[String]) -> Inspected {
    if o.signal == Signal::Gone || o.zombie {
        return Inspected::Clear;
    }
    match (o.uid, &o.argv) {
        (Some(u), _) if u != my_uid => Inspected::Clear,
        (Some(_), Some(args)) if args.iter().any(|a| markers.iter().any(|m| !m.is_empty() && a.windows(m.len()).any(|w| w == m.as_bytes()))) => Inspected::Marked,
        (Some(_), Some(_)) => Inspected::Clear,
        (Some(_), None) | (None, _) => Inspected::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(uid: Option<libc::uid_t>, zombie: bool, signal: Signal, argv: Option<&[&str]>) -> Observation {
        Observation { uid, zombie, signal, argv: argv.map(|a| a.iter().map(|x| x.as_bytes().to_vec()).collect()) }
    }

    #[test]
    fn only_positive_evidence_clears_a_process_and_a_denied_inspection_is_unknown() {
        let (me, other) = (501, 0);
        let m = vec!["tools/rth-try.mjs".to_string()];
        let eperm = Signal::Failed(libc::EPERM);

        assert_eq!(classify(&obs(None, false, eperm, None), me, &m), Inspected::Unknown, "uid unavailable + EPERM");
        assert_eq!(classify(&obs(Some(me), false, eperm, None), me, &m), Inspected::Unknown, "ours, argv unavailable + EPERM");
        assert_eq!(classify(&obs(Some(me), false, Signal::Failed(libc::EINVAL), None), me, &m), Inspected::Unknown, "ours, argv unavailable + another error");
        assert_eq!(classify(&obs(Some(me), false, Signal::Alive, None), me, &m), Inspected::Unknown, "ours, alive, argv unavailable");
        assert_eq!(classify(&obs(None, false, Signal::Alive, None), me, &m), Inspected::Unknown, "uid unavailable, alive");

        assert_eq!(classify(&obs(Some(other), false, eperm, None), me, &m), Inspected::Clear, "known foreign uid");
        assert_eq!(classify(&obs(None, false, Signal::Gone, None), me, &m), Inspected::Clear, "ESRCH");
        assert_eq!(classify(&obs(None, true, Signal::Alive, None), me, &m), Inspected::Clear, "proven zombie");
        assert_eq!(classify(&obs(Some(me), false, Signal::Alive, Some(&["node", "/x/tools/rth-try.mjs"])), me, &m), Inspected::Marked, "a marked launcher");
        assert_eq!(classify(&obs(Some(me), false, Signal::Alive, Some(&["vim", "notes.md"])), me, &m), Inspected::Clear, "ours, unmarked");
    }

    #[test]
    fn an_unreaped_child_reads_as_a_zombie_and_a_running_one_does_not() {
        let mut child = std::process::Command::new("/bin/sh").args(["-c", "read x"]).stdin(std::process::Stdio::piped()).spawn().unwrap();
        let pid = child.id() as i32;
        assert!(!is_zombie(pid));
        drop(child.stdin.take());

        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !is_zombie(pid) {
            assert!(std::time::Instant::now() < end, "never became a zombie");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        child.wait().unwrap();
        assert!(!is_zombie(pid));
    }

    #[test]
    fn a_held_agent_lock_is_live_absent_or_unheld_lock_files_are_not() {
        let d = std::env::temp_dir().join(format!("rth-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let lock = d.join("agent.flock");
        let none = LiveChecks { flocks: vec![d.join("absent.flock")], ..Default::default() };
        assert!(live(&none).unwrap().is_empty());
        std::fs::write(&lock, b"").unwrap();
        let free = LiveChecks { flocks: vec![lock.clone()], ..Default::default() };
        assert!(live(&free).unwrap().is_empty(), "an unheld lock is not live");
        let holder = std::fs::File::open(&lock).unwrap();
        assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert_eq!(live(&free).unwrap().len(), 1, "a held agent lock is live");
        drop(holder);

        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !live(&free).unwrap().is_empty() {
            assert!(std::time::Instant::now() < end, "released again");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
