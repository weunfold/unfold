

use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{FromRawFd, OwnedFd, AsRawFd};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {

    BadName,

    Symlink,

    NotRegular,
    Missing,

    OverBound { limit: u64 },
    Io(String),
}

impl Refused {
    pub fn as_str(&self) -> String {
        match self {
            Refused::BadName => "bad name".into(),
            Refused::Symlink => "symlink refused".into(),
            Refused::NotRegular => "not a regular file".into(),
            Refused::Missing => "missing".into(),
            Refused::OverBound { limit } => format!("over the {limit}-byte read-back bound"),
            Refused::Io(e) => format!("io: {e}"),
        }
    }
}

fn errno_refusal() -> Refused {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ENOENT) => Refused::Missing,
        Some(libc::ELOOP) | Some(libc::EMLINK) => Refused::Symlink,
        Some(libc::ENOTDIR) => Refused::NotRegular,
        _ => Refused::Io(e.to_string()),
    }
}

fn component(name: &str) -> Result<CString, Refused> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(Refused::BadName);
    }
    CString::new(name).map_err(|_| Refused::BadName)
}

fn openat(dir: &OwnedFd, name: &str, flags: libc::c_int) -> Result<OwnedFd, Refused> {
    let c = component(name)?;

    let fd = unsafe { libc::openat(dir.as_raw_fd(), c.as_ptr(), flags | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(errno_refusal());
    }

    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(fd: &OwnedFd) -> Result<libc::stat, Refused> {

    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(Refused::Io(std::io::Error::last_os_error().to_string()));
    }
    Ok(st)
}

pub struct Root(OwnedFd);

impl Root {
    pub fn open(path: &Path) -> Result<Self, Refused> {
        let c = CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| Refused::BadName)?;

        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(errno_refusal());
        }
        Ok(Root(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub fn dir(&self, name: &str) -> Result<Root, Refused> {
        let fd = openat(&self.0, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
        if fstat(&fd)?.st_mode & libc::S_IFMT != libc::S_IFDIR {
            return Err(Refused::NotRegular);
        }
        Ok(Root(fd))
    }

    pub fn read(&self, name: &str, limit: u64, tail: bool) -> Result<(u64, Vec<u8>), Refused> {
        let fd = openat(&self.0, name, libc::O_RDONLY | libc::O_NONBLOCK)?;
        let st = fstat(&fd)?;
        if st.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(Refused::NotRegular);
        }
        let size = st.st_size as u64;
        let mut f = File::from(fd);
        if tail {
            f.seek(SeekFrom::Start(size.saturating_sub(limit))).map_err(|e| Refused::Io(e.to_string()))?;
        } else if size > limit {
            return Err(Refused::OverBound { limit });
        }
        let mut out = Vec::new();
        (&mut f).take(limit + 1).read_to_end(&mut out).map_err(|e| Refused::Io(e.to_string()))?;
        if out.len() as u64 > limit {
            if tail {
                out.truncate(limit as usize);
            } else {
                return Err(Refused::OverBound { limit });
            }
        }
        Ok((size, out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rth-bench-contained-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn symlinks_non_regular_files_names_and_bounds_are_refused() {
        let d = dir("refuse");
        let outside = dir("outside");
        std::fs::write(outside.join("secret"), b"outside").unwrap();
        std::fs::create_dir(d.join("media")).unwrap();
        std::fs::write(d.join("media/ok"), b"abcd").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), d.join("media/link")).unwrap();
        std::fs::write(d.join("media/big"), vec![0u8; 100]).unwrap();
        let fifo = CString::new(d.join("media/fifo").as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let root = Root::open(&d).unwrap();
        let media = root.dir("media").unwrap();
        assert_eq!(media.read("ok", 10, false).unwrap(), (4, b"abcd".to_vec()));
        assert_eq!(media.read("link", 10, false).unwrap_err(), Refused::Symlink);
        assert_eq!(media.read("fifo", 10, false).unwrap_err(), Refused::NotRegular, "a FIFO never blocks the runner");
        assert_eq!(media.read("big", 10, false).unwrap_err(), Refused::OverBound { limit: 10 });
        assert_eq!(media.read("big", 10, true).unwrap().1.len(), 10, "a tail read is bounded");
        assert_eq!(media.read("..", 10, false).unwrap_err(), Refused::BadName);
        assert_eq!(media.read("a/b", 10, false).unwrap_err(), Refused::BadName);
        assert_eq!(media.read("none", 10, false).unwrap_err(), Refused::Missing);

        let d2 = dir("linked-media");
        std::os::unix::fs::symlink(&outside, d2.join("media")).unwrap();

        assert!(matches!(Root::open(&d2).unwrap().dir("media").err(), Some(Refused::Symlink | Refused::NotRegular)));
    }
}
