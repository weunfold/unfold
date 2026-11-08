

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Shared,
    Exclusive,
}

#[derive(Debug)]
pub enum LeaseError {

    Busy,

    Unusable(String),
    Io(io::Error),
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseError::Busy => f.write_str("the RTH operation lock is held by another process"),
            LeaseError::Unusable(why) => write!(f, "the RTH operation lock is unusable: {why}"),
            LeaseError::Io(e) => write!(f, "the RTH operation lock: {e}"),
        }
    }
}

#[derive(Debug)]
pub struct Lease {
    file: File,
    mode: Mode,
    id: (u64, u64),
}

impl Lease {

    pub fn acquire(path: &Path, mode: Mode) -> Result<Lease, LeaseError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| if e.raw_os_error() == Some(libc::ELOOP) { LeaseError::Unusable(format!("{} is a symlink", path.display())) } else { LeaseError::Io(e) })?;
        let m = file.metadata().map_err(LeaseError::Io)?;
        if !m.is_file() {
            return Err(LeaseError::Unusable(format!("{} is not a regular file", path.display())));
        }
        let op = match mode {
            Mode::Shared => libc::LOCK_SH,
            Mode::Exclusive => libc::LOCK_EX,
        } | libc::LOCK_NB;
        if unsafe { libc::flock(file.as_raw_fd(), op) } != 0 {
            let e = io::Error::last_os_error();
            return Err(if e.raw_os_error() == Some(libc::EWOULDBLOCK) { LeaseError::Busy } else { LeaseError::Io(e) });
        }
        Ok(Lease { id: (m.dev(), m.ino()), file, mode })
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    pub fn identity(&self) -> (u64, u64) {
        self.id
    }

    pub fn still_at(&self, path: &Path) -> bool {
        match std::fs::symlink_metadata(path) {
            Ok(m) => m.is_file() && (m.dev(), m.ino()) == self.id,
            Err(_) => false,
        }
    }

    pub fn inherit_into(&self, cmd: &mut Command) {
        let fd = self.fd();
        unsafe {
            cmd.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

pub fn available(path: &Path, mode: Mode) -> Result<bool, LeaseError> {
    match Lease::acquire(path, mode) {
        Ok(_) => Ok(true),
        Err(LeaseError::Busy) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rth-lease-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn shared_leases_coexist_and_exclude_the_mutator_and_the_reverse() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let p = dir("modes").join("rth.op.lock");
        let a = Lease::acquire(&p, Mode::Shared).unwrap();
        let b = Lease::acquire(&p, Mode::Shared).unwrap();
        assert!(matches!(Lease::acquire(&p, Mode::Exclusive), Err(LeaseError::Busy)), "a running RTH excludes the mutator");
        drop(a);
        assert!(matches!(Lease::acquire(&p, Mode::Exclusive), Err(LeaseError::Busy)), "while any runtime holds it");
        drop(b);
        let x = Lease::acquire(&p, Mode::Exclusive).unwrap();
        assert!(matches!(Lease::acquire(&p, Mode::Shared), Err(LeaseError::Busy)), "the mutator excludes a launch");
        drop(x);
        assert!(available(&p, Mode::Exclusive).unwrap());
    }

    #[test]
    fn the_lock_file_is_a_stable_regular_file_never_a_symlink() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let d = dir("ident");
        let p = d.join("rth.op.lock");
        std::fs::write(d.join("elsewhere"), b"").unwrap();
        std::os::unix::fs::symlink(d.join("elsewhere"), &p).unwrap();
        assert!(matches!(Lease::acquire(&p, Mode::Shared), Err(LeaseError::Unusable(_))), "a symlink at the lock path refuses");
        std::fs::remove_file(&p).unwrap();
        let l = Lease::acquire(&p, Mode::Shared).unwrap();
        assert!(l.still_at(&p));

        std::fs::remove_file(&p).unwrap();
        std::fs::write(&p, b"").unwrap();
        assert!(!l.still_at(&p));
    }

    #[test]
    fn an_inherited_descriptor_keeps_the_lease_for_the_childs_life_and_only_that_one() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let p = dir("inherit").join("rth.op.lock");
        let lease = Lease::acquire(&p, Mode::Shared).unwrap();
        let mut held = Command::new("/bin/sleep");
        held.arg("30").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        lease.inherit_into(&mut held);
        let mut child = held.spawn().unwrap();
        let mut plain = Command::new("/bin/sleep").arg("30").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        drop(lease);
        assert!(!available(&p, Mode::Exclusive).unwrap(), "the inheriting child still holds the shared lease");
        child.kill().unwrap();
        child.wait().unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        while !available(&p, Mode::Exclusive).unwrap() {
            assert!(Instant::now() < end, "the lease ends with the child that held it");
            std::thread::sleep(Duration::from_millis(20));
        }

        assert!(plain.try_wait().unwrap().is_none());
        plain.kill().unwrap();
        plain.wait().unwrap();
    }
}
