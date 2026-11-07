

use std::fs::File;
use std::path::{Component, Path, PathBuf};

use crate::grants::{FolderId, Grant, Grants};

pub const MAX_PATH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {

    Invalid(String),

    NotFound,

    NotGranted { folder: PathBuf, id: FolderId },

    TooBroad { folder: PathBuf },

    Blocked,

    NotRegular,

    NotDirectory,
}

impl Denied {

    pub fn reason(&self) -> &'static str {
        match self {
            Denied::Invalid(_) => "invalid_path",
            Denied::NotFound => "not_found",
            Denied::NotGranted { .. } => "not_granted",
            Denied::TooBroad { .. } => "too_broad",
            Denied::Blocked => "blocked",
            Denied::NotRegular => "not_a_file",
            Denied::NotDirectory => "not_a_folder",
        }
    }
}

#[derive(Debug)]
pub struct OpenedFile {
    pub file: File,
    pub real: PathBuf,
    pub grant: Grant,
    pub len: u64,
}

#[derive(Debug, Clone)]
pub struct OpenedDir {
    pub real: PathBuf,
    pub grant: Grant,
}

pub trait Covers {
    fn for_folder(&self, id: FolderId) -> Option<&Grant>;
}

impl Covers for Grants {
    fn for_folder(&self, id: FolderId) -> Option<&Grant> {
        Grants::for_folder(self, id)
    }
}

impl Covers for [Grant] {
    fn for_folder(&self, id: FolderId) -> Option<&Grant> {
        self.iter().find(|g| g.id == id)
    }
}

impl Covers for Vec<Grant> {
    fn for_folder(&self, id: FolderId) -> Option<&Grant> {
        self.as_slice().for_folder(id)
    }
}

pub struct Confiner<'a> {
    pub grants: &'a dyn Covers,

    pub blocked: &'a [FolderId],

    pub base: Option<&'a Path>,

    pub home: Option<&'a Path>,
}

#[cfg(target_os = "macos")]
pub fn real_path(file: &File) -> std::io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];

    let r = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) };
    if r == -1 {
        return Err(std::io::Error::last_os_error());
    }
    let n = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..n])))
}

#[cfg(not(target_os = "macos"))]
pub fn real_path(file: &File) -> std::io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

pub fn folder_id(path: &Path) -> std::io::Result<FolderId> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    if !m.is_dir() {
        return Err(std::io::Error::other("not a directory"));
    }
    Ok(FolderId { dev: m.dev(), ino: m.ino() })
}

pub fn block_list(paths: &[PathBuf]) -> Vec<FolderId> {
    paths.iter().filter_map(|p| folder_id(p).ok()).collect()
}

fn open_nonblocking(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC).open(path)
}

impl Confiner<'_> {

    pub fn resolve(&self, raw: &str) -> Result<PathBuf, Denied> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(Denied::Invalid("empty path".into()));
        }
        if raw.len() > MAX_PATH || raw.contains('\0') {
            return Err(Denied::Invalid("a path is at most 4096 bytes with no NUL".into()));
        }
        let p = if raw == "~" || raw.starts_with("~/") {
            let home = self.home.ok_or_else(|| Denied::Invalid("~ has no home folder here".into()))?;
            home.join(raw.trim_start_matches('~').trim_start_matches('/'))
        } else if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            let base = self.base.ok_or_else(|| Denied::Invalid("a relative path needs the task's folder; give an absolute path".into()))?;
            base.join(raw)
        };
        Ok(lexical(&p))
    }

    pub fn open_file(&self, raw: &str) -> Result<OpenedFile, Denied> {
        let path = self.resolve(raw)?;
        let file = match open_nonblocking(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {

                return Err(self.not_found_or_ungranted(&path));
            }
            Err(_) => return Err(Denied::NotFound),
        };
        let meta = file.metadata().map_err(|_| Denied::NotFound)?;
        let real = real_path(&file).map_err(|_| Denied::NotFound)?;
        let folder = if meta.is_dir() { real.clone() } else { real.parent().map(Path::to_path_buf).unwrap_or_else(|| real.clone()) };
        let grant = self.covering(&folder)?;
        if !meta.is_file() {
            return Err(Denied::NotRegular);
        }
        Ok(OpenedFile { file, real, grant, len: meta.len() })
    }

    pub fn open_dir(&self, raw: &str) -> Result<OpenedDir, Denied> {
        let path = self.resolve(raw)?;
        let file = match open_nonblocking(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(self.not_found_or_ungranted(&path)),
            Err(_) => return Err(Denied::NotFound),
        };
        let meta = file.metadata().map_err(|_| Denied::NotFound)?;
        let real = real_path(&file).map_err(|_| Denied::NotFound)?;
        if !meta.is_dir() {
            let parent = real.parent().map(Path::to_path_buf).unwrap_or_else(|| real.clone());
            self.covering(&parent)?;
            return Err(Denied::NotDirectory);
        }
        let grant = self.covering(&real)?;
        Ok(OpenedDir { real, grant })
    }

    pub fn covering(&self, real: &Path) -> Result<Grant, Denied> {
        let mut found: Option<Grant> = None;
        for a in real.ancestors() {
            let Ok(id) = folder_id(a) else { continue };
            if self.blocked.contains(&id) {
                return Err(Denied::Blocked);
            }
            if found.is_none() {
                found = self.grants.for_folder(id).cloned();
            }
        }
        if let Some(g) = found {
            return Ok(g);
        }
        if self.too_broad(real) {
            return Err(Denied::TooBroad { folder: real.to_path_buf() });
        }
        let id = folder_id(real).map_err(|_| Denied::NotFound)?;
        Err(Denied::NotGranted { folder: real.to_path_buf(), id })
    }

    fn too_broad(&self, real: &Path) -> bool {
        if real.parent().is_none() {
            return true;
        }
        let Some(home) = self.home else { return false };
        let (Ok(h), Ok(r)) = (folder_id(home), folder_id(real)) else { return false };
        h == r || home.ancestors().filter_map(|a| folder_id(a).ok()).any(|a| a == r)
    }

    fn not_found_or_ungranted(&self, path: &Path) -> Denied {
        let Some(existing) = path.ancestors().skip(1).find(|a| a.is_dir()) else { return Denied::NotFound };
        let Ok(real) = std::fs::canonicalize(existing) else { return Denied::NotFound };
        match self.covering(&real) {
            Ok(_) => Denied::NotFound,
            Err(d) => d,
        }
    }
}

fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TempDir;

    struct Fixture {
        d: TempDir,
        grants: Grants,
        blocked: Vec<FolderId>,
    }

    impl Fixture {
        fn new(tag: &str) -> Fixture {
            let d = TempDir::new(tag);
            d.write("docs/a.md", "alpha");
            d.write("docs/sub/b.md", "beta");
            d.write("docs2/c.md", "sibling with a shared prefix");
            d.write("outside/secret.txt", "secret");
            d.write("docs/private/key.txt", "blocked inside a grant");
            let (grants, _) = Grants::open(&d.path().join("grants.jsonl"), "b1").unwrap();
            Fixture { d, grants, blocked: Vec::new() }
        }

        fn grant(&mut self, rel: &str) -> Grant {
            let root = std::fs::canonicalize(self.d.path().join(rel)).unwrap();
            let id = folder_id(&root).unwrap();
            let (r, _) = self.grants.ask("t1", &root, id, "test").unwrap();
            self.grants.grant(&r.request_id, "e", "d").unwrap().unwrap().0
        }

        fn c(&self) -> Confiner<'_> {
            Confiner { grants: &self.grants, blocked: &self.blocked, base: None, home: None }
        }

        fn p(&self, rel: &str) -> String {
            self.d.path().join(rel).to_string_lossy().into_owned()
        }
    }

    #[test]
    fn a_file_inside_a_grant_opens_with_its_real_path() {
        let mut f = Fixture::new("conf-ok");
        let g = f.grant("docs");
        let o = f.c().open_file(&f.p("docs/sub/b.md")).unwrap();
        assert_eq!(o.real, f.d.path().join("docs/sub/b.md"));
        assert_eq!((o.grant.grant_id, o.len), (g.grant_id, 4));
        assert!(matches!(f.c().open_file(&f.p("docs/missing.md")), Err(Denied::NotFound)));
    }

    #[test]
    fn nothing_outside_a_grant_opens_whatever_the_path_says() {
        let mut f = Fixture::new("conf-escape");
        f.grant("docs");
        let outside = f.d.path().join("outside/secret.txt");
        std::os::unix::fs::symlink(&outside, f.d.path().join("docs/link.txt")).unwrap();
        std::os::unix::fs::symlink(f.d.path().join("outside"), f.d.path().join("docs/linkdir")).unwrap();
        for raw in [
            f.p("docs/../outside/secret.txt"),
            f.p("docs/sub/../../outside/secret.txt"),
            outside.to_string_lossy().into_owned(),
            f.p("docs/link.txt"),
            f.p("docs/linkdir/secret.txt"),
            f.p("docs2/c.md"),
        ] {
            match f.c().open_file(&raw) {
                Err(Denied::NotGranted { folder, .. }) => assert_ne!(folder, f.d.path().join("docs"), "{raw}"),
                other => panic!("{raw}: {other:?}"),
            }
        }
        assert!(f.c().open_dir(&f.p("docs/linkdir")).is_err(), "a symlinked folder out of the grant is outside it");
    }

    #[test]
    fn a_symlink_that_stays_inside_the_grant_and_a_root_reached_through_a_symlink_both_open() {
        let mut f = Fixture::new("conf-inside");
        f.grant("docs");
        std::os::unix::fs::symlink(f.d.path().join("docs/sub/b.md"), f.d.path().join("docs/inner.md")).unwrap();
        assert_eq!(f.c().open_file(&f.p("docs/inner.md")).unwrap().real, f.d.path().join("docs/sub/b.md"));
        std::os::unix::fs::symlink(f.d.path().join("docs"), f.d.path().join("docs-alias")).unwrap();
        assert_eq!(f.c().open_file(&f.p("docs-alias/a.md")).unwrap().real, f.d.path().join("docs/a.md"));
    }

    #[test]
    fn only_regular_files_are_read_and_only_folders_are_searched() {
        let mut f = Fixture::new("conf-kinds");
        f.grant("docs");
        let fifo = f.d.path().join("docs/pipe");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(f.c().open_file(&f.p("docs/pipe")).unwrap_err(), Denied::NotRegular, "a fifo never blocks the worker");
        assert_eq!(f.c().open_file(&f.p("docs/sub")).unwrap_err(), Denied::NotRegular);
        assert_eq!(f.c().open_dir(&f.p("docs/a.md")).unwrap_err(), Denied::NotDirectory);
        assert_eq!(f.c().open_dir(&f.p("docs/sub")).unwrap().real, f.d.path().join("docs/sub"));
    }

    #[test]
    fn a_hard_block_wins_inside_a_grant_and_a_revoke_or_a_recreated_folder_ends_access() {
        let mut f = Fixture::new("conf-block");
        let g = f.grant("docs");
        f.blocked = block_list(&[f.d.path().join("docs/private"), f.d.path().join("does-not-exist")]);
        assert_eq!(f.blocked.len(), 1);
        assert_eq!(f.c().open_file(&f.p("docs/private/key.txt")).unwrap_err(), Denied::Blocked);
        assert!(f.c().open_file(&f.p("docs/a.md")).is_ok());
        f.grants.revoke(&g.grant_id, "settings").unwrap().unwrap();
        assert!(matches!(f.c().open_file(&f.p("docs/a.md")), Err(Denied::NotGranted { .. })));
        let g2 = f.grant("docs2");
        std::fs::remove_dir_all(f.d.path().join("docs2")).unwrap();
        f.d.write("docs2/c.md", "a new folder with the old name");
        assert!(matches!(f.c().open_file(&f.p("docs2/c.md")), Err(Denied::NotGranted { .. })), "{g2:?} named another folder");
    }

    #[test]
    fn paths_resolve_against_the_base_and_home_and_bad_ones_are_invalid() {
        let mut f = Fixture::new("conf-resolve");
        f.grant("docs");
        let base = f.d.path().join("docs");
        let c = Confiner { grants: &f.grants, blocked: &[], base: Some(&base), home: Some(f.d.path()) };
        assert_eq!(c.open_file("sub/b.md").unwrap().real, f.d.path().join("docs/sub/b.md"));
        assert_eq!(c.open_file("~/docs/a.md").unwrap().real, f.d.path().join("docs/a.md"));
        assert!(matches!(c.open_file("../outside/secret.txt"), Err(Denied::NotGranted { .. })));
        assert!(matches!(f.c().open_file("sub/b.md"), Err(Denied::Invalid(_))), "relative with no base");
        assert!(matches!(f.c().open_file(""), Err(Denied::Invalid(_))));
        assert!(matches!(f.c().open_file("/a\0b"), Err(Denied::Invalid(_))));
        assert!(matches!(f.c().open_file(&"/a".repeat(3000)), Err(Denied::Invalid(_))));
    }

    #[test]
    fn the_disk_root_and_the_home_folder_are_too_broad_to_ask_for() {
        let f = Fixture::new("conf-broad");
        let home = f.d.path().to_path_buf();
        let c = Confiner { grants: &f.grants, blocked: &[], base: None, home: Some(&home) };
        assert!(matches!(c.open_dir("/"), Err(Denied::TooBroad { .. })));
        assert!(matches!(c.open_dir("~"), Err(Denied::TooBroad { .. })));
        f.d.write("loose.txt", "x");
        assert!(matches!(c.open_file(&f.p("loose.txt")), Err(Denied::TooBroad { .. })), "a file directly in home asks for home");
        assert!(matches!(c.open_dir(&f.p("docs")), Err(Denied::NotGranted { .. })));
    }

    #[test]
    fn letter_case_does_not_matter_on_a_case_insensitive_volume() {
        let mut f = Fixture::new("conf-case");
        f.grant("docs");
        let upper = f.p("DOCS/a.md");
        if std::fs::metadata(&upper).is_err() {
            return;
        }
        assert_eq!(f.c().open_file(&upper).unwrap().len, 5);
    }
}
