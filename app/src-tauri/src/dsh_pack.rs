

#![cfg_attr(not(test), allow(dead_code))]

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Download { done: u64, total: u64 },
    Verify,
    Unpack,
    Check,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackPin { pub version: &'static str, pub url: &'static str, pub sha256: &'static str, pub size: u64 }

impl PackPin {

    pub const UNSET: PackPin = PackPin { version: "", url: "", sha256: "", size: 0 };

    pub fn parse(text: &'static str) -> Result<PackPin, String> {
        let hex = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        match text.split_whitespace().collect::<Vec<_>>()[..] {
            ["unset"] => Ok(PackPin::UNSET),
            [version, url, sha256, size] if hex(sha256) => match size.parse::<u64>() {
                Ok(size) if size > 0 => Ok(PackPin { version, url, sha256, size }),
                _ => Err(format!("pack.pin: size {size:?}")),
            },
            _ => Err(format!("pack.pin is neither `<version> <url> <sha256> <size>` nor `unset`: {:?}", text.trim())),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PackError { Unset, Bad(String), Disk { need: u64, free: u64 }, Hash, Unsafe(String), Signature(String), Io(String) }

pub const MANIFEST: &str = ".unfold-pack-manifest.json";
const HEALTHY: &str = "healthy.json";
const BAD: &str = "bad.json";
const TMP: &str = ".tmp-";
const ASIDE: &str = ".aside-";

const PACK_WAIT: Duration = Duration::from_secs(3600);

const BUSY_WAIT: Duration = Duration::from_secs(10);

const MAX_HOPS: usize = 40;

const FLOOR: u64 = 30 << 30;

pub struct Lock {
    _held: File,
}

impl Lock {
    pub fn take(path: &Path, wait: Duration) -> Result<Lock, String> {
        let f = fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let until = Instant::now() + wait;
        loop {
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(Lock { _held: f });
            }
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::EWOULDBLOCK) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                Some(libc::EWOULDBLOCK) => return Err(format!("{}: still held by another fetch, prune or port change after {} s", path.display(), wait.as_secs_f32())),
                Some(libc::EINTR) => {}
                _ => return Err(format!("{}: {e}", path.display())),
            }
        }
    }
}

fn io(e: impl std::fmt::Display) -> PackError {
    PackError::Io(e.to_string())
}

pub fn ensure(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>) -> Result<PathBuf, PackError> {
    ensure_with(data, pin, fetch, &free_bytes, &codesign_verify)
}

pub fn ensure_with(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>, free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>) -> Result<PathBuf, PackError> {
    ensure_steps(data, pin, fetch, free, verify, &|_| {})
}

fn ensure_steps(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>, free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>, step: &dyn Fn(Step)) -> Result<PathBuf, PackError> {
    let (runtimes, _lock) = open(data, pin)?;
    let dir = runtimes.join(format!("dsh-{}", pin.version));
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() && dir.join(MANIFEST).is_file() => return Ok(dir),
        Ok(_) => return Err(PackError::Unsafe(format!("{} is there without its manifest (fetch it again)", dir.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(format!("{}: {e}", dir.display()))),
    }
    unpack(&runtimes, pin, fetch, free, verify, step)
}

pub fn refetch(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>) -> Result<PathBuf, PackError> {
    refetch_with(data, pin, fetch, &free_bytes, &codesign_verify)
}

pub fn refetch_with(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>, free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>) -> Result<PathBuf, PackError> {
    refetch_steps(data, pin, fetch, free, verify, &|_| {})
}

fn refetch_steps(data: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>, free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>, step: &dyn Fn(Step)) -> Result<PathBuf, PackError> {
    let (runtimes, _lock) = open(data, pin)?;
    let dir = runtimes.join(format!("dsh-{}", pin.version));
    let aside = runtimes.join(format!("{ASIDE}dsh-{}", pin.version));
    let had = fs::symlink_metadata(&dir).is_ok();
    if had {
        fs::rename(&dir, &aside).map_err(io)?;
    }
    match unpack(&runtimes, pin, fetch, free, verify, step) {
        Ok(new) => {

            let _ = remove(&aside);
            Ok(new)
        }
        Err(e) if had => match fs::rename(&aside, &dir) {
            Ok(()) => Err(e),
            Err(back) => Err(io(format!("{e:?}; the old pack stays at {} until the next fetch puts it back: {back}", aside.display()))),
        },
        Err(e) => Err(e),
    }
}

fn open(data: &Path, pin: &PackPin) -> Result<(PathBuf, Lock), PackError> {
    if pin.sha256.is_empty() {
        return Err(PackError::Unset);
    }
    if pin.version.is_empty() || !pin.version.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) || pin.version.starts_with('.') {
        return Err(PackError::Unsafe(format!("pack version {:?}", pin.version)));
    }
    let runtimes = runtimes_dir(data).map_err(io)?;
    let lock = Lock::take(&runtimes.join(".lock"), PACK_WAIT).map_err(io)?;
    sweep(&runtimes).map_err(io)?;
    Ok((runtimes, lock))
}

fn unpack(runtimes: &Path, pin: &PackPin, fetch: &dyn Fn(&str, &Path) -> Result<(), String>, free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>, step: &dyn Fn(Step)) -> Result<PathBuf, PackError> {
    let dir = runtimes.join(format!("dsh-{}", pin.version));
    let need = pin.size.saturating_mul(3).saturating_add(FLOOR);
    let have = free(runtimes).map_err(io)?;
    if have < need {
        return Err(PackError::Disk { need, free: have });
    }
    let made = Made(vec![runtimes.join(format!("{TMP}{}-{}.tar.gz", pin.version, std::process::id())), runtimes.join(format!("{TMP}{}-{}.d", pin.version, std::process::id()))]);
    let (archive, staging) = (&made.0[0], &made.0[1]);
    step(Step::Download { done: 0, total: pin.size });
    fetch(pin.url, archive).map_err(PackError::Io)?;
    step(Step::Verify);
    let f = File::open(archive).map_err(io)?;
    f.sync_all().map_err(io)?;
    if f.metadata().map_err(io)?.len() != pin.size || sha256_of(f).map_err(io)? != pin.sha256.to_ascii_lowercase() {
        return Err(PackError::Hash);
    }
    fs::DirBuilder::new().mode(0o700).create(staging).map_err(io)?;
    step(Step::Unpack);
    extract(archive, staging)?;
    step(Step::Check);
    for (rel, kind) in walk(staging).map_err(io)? {
        if kind == Kind::File && is_macho(&staging.join(&rel)) {
            verify(&staging.join(&rel)).map_err(|e| PackError::Signature(format!("{}: {e}", rel.display())))?;
        }
    }
    write_manifest(staging).map_err(io)?;
    fs::rename(staging, &dir).map_err(io)?;
    File::open(runtimes).and_then(|d| d.sync_all()).map_err(io)?;
    drop(made);

    let mut bad = read_bad(runtimes);
    if bad.remove(pin.version).is_some() {
        write_bad(runtimes, &bad).map_err(io)?;
    }
    Ok(dir)
}

struct Made(Vec<PathBuf>);

impl Drop for Made {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = remove(p);
        }
    }
}

fn remove(p: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn runtimes_dir(data: &Path) -> Result<PathBuf, String> {
    let d = data.join("runtimes");
    match fs::DirBuilder::new().mode(0o700).create(&d) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(format!("{}: {e}", d.display())),
    }
    match fs::symlink_metadata(&d) {
        Ok(m) if m.is_dir() => Ok(d),
        _ => Err(format!("{} is not a directory", d.display())),
    }
}

fn sweep(runtimes: &Path) -> std::io::Result<()> {
    for e in fs::read_dir(runtimes)? {
        let name = e?.file_name().to_string_lossy().into_owned();
        if name.starts_with(TMP) {
            remove(&runtimes.join(&name))?;
        } else if let Some(pack) = name.strip_prefix(ASIDE) {
            if fs::symlink_metadata(runtimes.join(pack)).is_ok() {
                remove(&runtimes.join(&name))?;
            } else {
                fs::rename(runtimes.join(&name), runtimes.join(pack))?;
            }
        }
    }
    Ok(())
}

fn free_bytes(p: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(format!("{}: {}", p.display(), std::io::Error::last_os_error()));
    }
    Ok(s.f_bavail as u64 * s.f_frsize as u64)
}

fn sha256_of(mut r: impl Read) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn inside(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn resolve(link: &Path, target: &Path) -> Option<PathBuf> {
    if target.is_absolute() {
        return None;
    }
    let mut at: Vec<_> = link.parent().map(|p| p.components().collect()).unwrap_or_default();
    for c in target.components() {
        match c {
            Component::Normal(_) => at.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                at.pop()?;
            }
            _ => return None,
        }
    }
    Some(at.iter().collect())
}

fn parents(root: &Path, rel: &Path) -> Result<(), PackError> {
    let mut at = root.to_path_buf();
    for c in rel.parent().into_iter().flat_map(|p| p.components()) {
        at.push(c);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(PackError::Unsafe(format!("{}: written through a link or file", rel.display()))),
            Err(_) => made_dir(&at)?,
        }
    }
    Ok(())
}

fn made_dir(at: &Path) -> Result<(), PackError> {
    fs::DirBuilder::new().mode(0o755).create(at).map_err(io)?;
    fs::set_permissions(at, fs::Permissions::from_mode(0o755)).map_err(io)
}

fn extract(archive: &Path, to: &Path) -> Result<(), PackError> {
    use tar::EntryType as T;
    let gz = flate2::read::GzDecoder::new(std::io::BufReader::new(File::open(archive).map_err(io)?));
    let mut ar = tar::Archive::new(gz);
    let mut links = Vec::new();
    for e in ar.entries().map_err(io)? {
        let mut e = e.map_err(io)?;
        let kind = e.header().entry_type();
        if matches!(kind, T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink) {
            continue;
        }
        let raw = e.path().map_err(io)?.into_owned();
        let rel = inside(&raw).ok_or_else(|| PackError::Unsafe(format!("{}: outside the tree", raw.display())))?;
        let unsafe_ = |why: &str| PackError::Unsafe(format!("{}: {why}", rel.display()));
        if rel.as_os_str().is_empty() {
            if kind == T::Directory {
                continue;
            }
            return Err(unsafe_("no name"));
        }
        if rel == Path::new(MANIFEST) {
            return Err(unsafe_("the manifest's own name"));
        }
        let mode = e.header().mode().map_err(io)?;
        if mode & 0o6000 != 0 {
            return Err(unsafe_("setuid or setgid"));
        }
        parents(to, &rel)?;
        let at = to.join(&rel);
        match kind {
            T::Directory => match fs::symlink_metadata(&at) {
                Ok(m) if m.is_dir() => {}
                Ok(_) => return Err(unsafe_("a directory over another entry")),
                Err(_) => made_dir(&at)?,
            },
            T::Regular | T::Continuous => {

                let keep = (mode & 0o755) | 0o400;
                let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(keep).custom_flags(libc::O_NOFOLLOW).open(&at).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { unsafe_("a duplicate entry") } else { io(e) })?;
                std::io::copy(&mut e, &mut f).map_err(io)?;
                f.set_permissions(fs::Permissions::from_mode(keep)).map_err(io)?;
            }
            T::Symlink => {
                let target = e.link_name().map_err(io)?.ok_or_else(|| unsafe_("a link without a target"))?.into_owned();
                if resolve(&rel, &target).is_none() {
                    return Err(unsafe_("a link out of the tree"));
                }
                std::os::unix::fs::symlink(&target, &at).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { unsafe_("a duplicate entry") } else { io(e) })?;
                links.push(rel.clone());
            }
            other => return Err(unsafe_(&format!("a {other:?} entry"))),
        }
    }

    for l in links {
        follow(to, &l).map_err(|why| PackError::Unsafe(format!("{}: {why}", l.display())))?;
    }
    Ok(())
}

fn follow(root: &Path, link: &Path) -> Result<PathBuf, &'static str> {
    let mut at: Vec<std::ffi::OsString> = link.parent().into_iter().flat_map(|p| p.components()).map(|c| c.as_os_str().to_owned()).collect();
    let mut todo: std::collections::VecDeque<std::ffi::OsString> = std::collections::VecDeque::new();
    let mut hops = 0;
    let mut next = Some(root.join(link));
    loop {
        if let Some(l) = next.take() {
            hops += 1;
            if hops > MAX_HOPS {
                return Err("too many links");
            }
            let target = fs::read_link(&l).map_err(|_| "a link to nothing")?;
            for c in target.components().rev() {
                match c {
                    Component::Normal(_) | Component::CurDir | Component::ParentDir => todo.push_front(c.as_os_str().to_owned()),
                    _ => return Err("a link out of the tree"),
                }
            }
        }
        let Some(c) = todo.pop_front() else { break };
        match c.to_str() {
            Some(".") => {}
            Some("..") => {
                at.pop().ok_or("a link out of the tree")?;
            }
            _ => {
                let p: PathBuf = std::iter::once(root.as_os_str().to_owned()).chain(at.iter().cloned()).chain(std::iter::once(c.clone())).collect();
                match fs::symlink_metadata(&p) {
                    Ok(m) if m.file_type().is_symlink() => next = Some(p),
                    Ok(_) => at.push(c),
                    Err(_) => return Err("a link to nothing"),
                }
            }
        }
    }
    Ok(at.iter().collect())
}

fn is_macho(p: &Path) -> bool {
    let mut b = [0u8; 8];
    if File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_err() {
        return false;
    }
    let magic = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    match magic {
        0xfeed_face | 0xfeed_facf | 0xcefa_edfe | 0xcffa_edfe => true,
        0xcafe_babe | 0xcafe_babf => u32::from_be_bytes([b[4], b[5], b[6], b[7]]) < 45,
        0xbeba_feca | 0xbfba_feca => u32::from_le_bytes([b[4], b[5], b[6], b[7]]) < 45,
        _ => false,
    }
}

fn codesign_verify(p: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/codesign").args(["--verify", "--strict"]).arg(p).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("codesign: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    Err(said.lines().find(|l| !l.trim().is_empty()).unwrap_or("codesign refused it").trim().to_string())
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Kind {
    File,
    Link,
    Other,
}

fn walk(root: &Path) -> std::io::Result<Vec<(PathBuf, Kind)>> {
    let mut out = Vec::new();
    let mut todo = vec![PathBuf::new()];
    while let Some(rel) = todo.pop() {
        for e in fs::read_dir(root.join(&rel))? {
            let e = e?;
            let r = rel.join(e.file_name());
            let t = e.file_type()?;
            if t.is_dir() {
                todo.push(r);
            } else if r != Path::new(MANIFEST) {
                out.push((r, if t.is_file() { Kind::File } else if t.is_symlink() { Kind::Link } else { Kind::Other }));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn record(root: &Path, entries: &[(PathBuf, Kind)], skip: &(dyn Fn(&Path) -> bool + Sync)) -> std::io::Result<BTreeMap<PathBuf, Value>> {
    let out = Mutex::new(BTreeMap::new());
    let next = AtomicUsize::new(0);
    let failed = Mutex::new(None);
    let threads = hash_threads();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((rel, kind)) = entries.get(i) else { break };
                if skip(rel) {
                    continue;
                }
                let p = root.join(rel);
                let got = match kind {
                    Kind::File => fs::symlink_metadata(&p).and_then(|m| Ok(json!({"sha256": sha256_of(File::open(&p)?)?, "size": m.len(), "mode": m.mode() & 0o7777}))),
                    Kind::Link => fs::read_link(&p).map(|t| json!({"link": t.to_string_lossy()})),
                    Kind::Other => Ok(json!({"other": true})),
                };
                match got {
                    Ok(v) => {
                        out.lock().unwrap().insert(rel.clone(), v);
                    }
                    Err(e) => {
                        *failed.lock().unwrap() = Some(std::io::Error::new(e.kind(), format!("{}: {e}", rel.display())));
                        break;
                    }
                }
            });
        }
    });
    match failed.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(out.into_inner().unwrap()),
    }
}

fn hash_threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).min(8)
}

pub(crate) fn write_manifest(dir: &Path) -> Result<(), String> {
    let entries = walk(dir).map_err(|e| e.to_string())?;
    let files: Map<String, Value> = record(dir, &entries, &|_| false).map_err(|e| e.to_string())?.into_iter().map(|(k, v)| (k.to_string_lossy().into_owned(), v)).collect();
    let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(dir.join(MANIFEST)).map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec(&json!({"v": 1, "files": files})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}

pub fn check_manifest(dir: &Path, kept: &[PathBuf]) -> Result<(), Vec<PathBuf>> {
    let kept: Vec<PathBuf> = kept.iter().map(|k| k.strip_prefix(dir).unwrap_or(k).to_path_buf()).collect();
    let manifest = || vec![PathBuf::from(MANIFEST)];
    let want: Value = fs::read(dir.join(MANIFEST)).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or_else(manifest)?;
    let want = want["files"].as_object().ok_or_else(manifest)?;
    let entries = walk(dir).map_err(|_| manifest())?;

    let differs = |rel: &Path| kept.iter().any(|k| k == rel) || want.get(&*rel.to_string_lossy()).is_some_and(|w| w["size"].as_u64().is_some_and(|s| fs::symlink_metadata(dir.join(rel)).is_ok_and(|m| m.len() != s)));
    let got = record(dir, &entries, &differs).map_err(|_| manifest())?;
    let mut changed: Vec<PathBuf> = entries.iter().map(|(r, _)| r.clone()).filter(|r| got.get(r).map(|v| want.get(&*r.to_string_lossy()) != Some(v)).unwrap_or(true)).collect();
    let present: BTreeSet<&PathBuf> = entries.iter().map(|(r, _)| r).collect();
    changed.extend(want.keys().map(PathBuf::from).filter(|r| !present.contains(r)));
    changed.retain(|r| !kept.contains(r));
    changed.sort();
    changed.dedup();
    if changed.is_empty() {
        Ok(())
    } else {
        Err(changed)
    }
}

fn read_version(data: &Path) -> Option<String> {
    let v: Value = serde_json::from_slice(&fs::read(data.join("runtimes").join(HEALTHY)).ok()?).ok()?;
    v["version"].as_str().map(String::from)
}

pub fn healthy(data: &Path) -> Option<PathBuf> {
    let dir = data.join("runtimes").join(format!("dsh-{}", read_version(data)?));
    dir.join(MANIFEST).is_file().then_some(dir)
}

pub fn mark_healthy(data: &Path, version: &str) -> Result<(), String> {
    let runtimes = runtimes_dir(data)?;
    let _lock = Lock::take(&runtimes.join(".lock"), BUSY_WAIT)?;
    let keep = format!("dsh-{version}");
    if version.is_empty() || version.contains('/') || !runtimes.join(&keep).join(MANIFEST).is_file() {
        return Err(format!("no unpacked pack {version:?}"));
    }
    let tmp = runtimes.join(format!("{TMP}{HEALTHY}"));
    let write = || -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
        f.write_all(json!({"v": 1, "version": version}).to_string().as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, runtimes.join(HEALTHY))
    };
    write().map_err(|e| format!("{}: {e}", runtimes.join(HEALTHY).display()))?;

    let bad = read_bad(&runtimes);
    for e in fs::read_dir(&runtimes).map_err(|e| e.to_string())? {
        let name = e.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned();
        let marked = name.strip_prefix("dsh-").is_some_and(|v| bad.contains_key(v));
        if name.starts_with("dsh-") && name != keep && !marked {
            let gone = runtimes.join(format!("{TMP}prune-{name}"));
            fs::rename(runtimes.join(&name), &gone).and_then(|()| remove(&gone)).map_err(|e| format!("{name}: {e}"))?;
        }
    }
    Ok(())
}

fn read_bad(runtimes: &Path) -> Map<String, Value> {
    let v: Option<Value> = fs::read(runtimes.join(BAD)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    v.and_then(|v| v["packs"].as_object().cloned()).unwrap_or_default()
}

fn write_bad(runtimes: &Path, packs: &Map<String, Value>) -> std::io::Result<()> {
    let tmp = runtimes.join(format!("{TMP}{BAD}"));
    let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
    f.write_all(json!({"v": 1, "packs": packs}).to_string().as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, runtimes.join(BAD))
}

pub fn mark_bad(data: &Path, version: &str, reason: &str) -> Result<(), String> {
    let runtimes = runtimes_dir(data)?;
    let _lock = Lock::take(&runtimes.join(".lock"), BUSY_WAIT)?;
    if version.is_empty() || version.contains('/') || !runtimes.join(format!("dsh-{version}")).join(MANIFEST).is_file() {
        return Err(format!("no unpacked pack {version:?}"));
    }
    if read_version(data).as_deref() == Some(version) {
        return Err(format!("pack {version} has run healthy; it isn't marked bad"));
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut bad = read_bad(&runtimes);
    bad.insert(version.to_string(), json!({"reason": reason, "at": at}));
    write_bad(&runtimes, &bad).map_err(|e| format!("{}: {e}", runtimes.join(BAD).display()))
}

pub fn bad_mark(data: &Path, version: &str) -> Option<Value> {
    read_bad(&data.join("runtimes")).get(version).cloned()
}

const PACK_PIN: &str = include_str!("../pack.pin");

#[derive(Debug, PartialEq)]
pub struct Picked {
    pub dir: PathBuf,
    pub fell_back: Option<PackError>,
}

pub fn for_start(data: &Path, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(Step)) -> Result<Picked, String> {
    let (pin, https_only) = start_pin(log)?;
    pinned(data, &pin, &crate::dictation_models::Curl { https_only, ..Default::default() }, log, alive, step)
}

#[cfg(not(feature = "rehearsal_pack"))]
fn start_pin(_log: &dyn Fn(&str)) -> Result<(PackPin, bool), String> {
    PackPin::parse(PACK_PIN).map(|p| (p, true))
}

#[cfg(feature = "rehearsal_pack")]
fn start_pin(log: &dyn Fn(&str)) -> Result<(PackPin, bool), String> {
    match std::env::var(REHEARSAL_PIN) {
        Ok(line) => {
            let pin = rehearsal_pin(Box::leak(line.into_boxed_str()))?;
            log(&format!("dsh: rehearsal pack pin from {REHEARSAL_PIN}: {} at {}", pin.version, pin.url));
            Ok((pin, false))
        }
        Err(_) => Ok((REHEARSAL_PACK, false)),
    }
}

#[cfg(feature = "rehearsal_pack")]
const REHEARSAL_PACK: PackPin = PackPin { version: "0.5.0-rehearsal.4", url: "", sha256: "4c2984291259dafb140ffe05423715469bb84fdcb48f955e1ec176a0bf17598a", size: 0 };

#[cfg(feature = "rehearsal_pack")]
pub const REHEARSAL_PIN: &str = "UNFOLD_REHEARSAL_PACK_PIN";

#[cfg(feature = "rehearsal_pack")]
fn rehearsal_pin(line: &'static str) -> Result<PackPin, String> {
    let pin = PackPin::parse(line)?;
    let port = pin.url.strip_prefix("http://127.0.0.1:").and_then(|r| r.split_once('/')).map(|(p, _)| p);
    match port.map(str::parse::<u16>) {
        Some(Ok(p)) if p > 0 => Ok(pin),
        _ => Err(format!("rehearsal pack pin: {:?} is not a pack on http://127.0.0.1:<port>/", pin.url)),
    }
}

pub fn pinned(data: &Path, pin: &PackPin, fetch: &dyn crate::dictation_models::Fetch, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(Step)) -> Result<Picked, String> {
    pinned_steps(data, pin, fetch, log, alive, &free_bytes, &codesign_verify, step)
}

fn pinned_with(data: &Path, pin: &PackPin, fetch: &dyn crate::dictation_models::Fetch, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>) -> Result<Picked, String> {
    pinned_steps(data, pin, fetch, log, alive, free, verify, &|_| {})
}

#[allow(clippy::too_many_arguments)]
fn pinned_steps(data: &Path, pin: &PackPin, fetch: &dyn crate::dictation_models::Fetch, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>, step: &dyn Fn(Step)) -> Result<Picked, String> {
    if let Some(mark) = bad_mark(data, pin.version) {
        let why = format!("pack {} is marked bad ({}, at {})", pin.version, mark["reason"].as_str().unwrap_or("?"), mark["at"]);
        return match healthy(data) {
            Some(old) if alive() => {
                log(&format!("dsh: {why}; starting the last healthy pack, {}", old.display()));
                Ok(Picked { dir: old, fell_back: Some(PackError::Bad(why)) })
            }
            _ => Err(format!("{why}; no healthy pack to start instead")),
        };
    }
    if let Some(r) = placed(data, pin) {
        return r.map(|dir| Picked { dir, fell_back: None });
    }
    let get = |url: &str, to: &Path| fetch_while(fetch, pin, url, to, log, alive, step);
    match ensure_steps(data, pin, &get, free, verify, step) {
        Ok(dir) => Ok(Picked { dir, fell_back: None }),
        Err(PackError::Unset) => Err("this build carries no pack pin (unset until its signed pack is published); nothing fetched".into()),
        Err(e) => match healthy(data) {
            Some(old) if alive() => {
                log(&format!("dsh: pack {} not unpacked ({e:?}); starting the last healthy pack, {}", pin.version, old.display()));
                Ok(Picked { dir: old, fell_back: Some(e) })
            }
            _ => Err(format!("pack {} not unpacked: {e:?}", pin.version)),
        },
    }
}

fn fetch_while(fetch: &dyn crate::dictation_models::Fetch, pin: &PackPin, url: &str, to: &Path, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(Step)) -> Result<(), String> {
    log(&format!("dsh: fetching pack {}", pin.version));
    let (cancel, done) = (AtomicBool::new(false), AtomicBool::new(false));
    std::thread::scope(|s| {

        s.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                if !alive() {
                    cancel.store(true, Ordering::SeqCst);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let r = fetch.fetch(url, to, &cancel, &|done| step(Step::Download { done, total: pin.size }), &|n, of| log(&format!("dsh: pack fetch, attempt {n} of {of}")));
        done.store(true, Ordering::SeqCst);
        r
    })
}

#[cfg(feature = "menu_bar")]
pub fn refetch_for_start(data: &Path, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), step: &dyn Fn(Step)) -> Result<PathBuf, String> {
    let (pin, https_only) = start_pin(log)?;
    refetch_pinned_steps(data, &pin, &crate::dictation_models::Curl { https_only, ..Default::default() }, log, alive, &free_bytes, &codesign_verify, step)
}

pub fn start_can_refetch() -> bool {
    start_pin(&|_| {}).is_ok_and(|(pin, _)| !pin.sha256.is_empty() && !pin.url.is_empty())
}

fn refetch_pinned(data: &Path, pin: &PackPin, fetch: &dyn crate::dictation_models::Fetch, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    refetch_pinned_steps(data, pin, fetch, log, alive, free, verify, &|_| {})
}

#[allow(clippy::too_many_arguments)]
fn refetch_pinned_steps(data: &Path, pin: &PackPin, fetch: &dyn crate::dictation_models::Fetch, log: &dyn Fn(&str), alive: &(dyn Fn() -> bool + Sync), free: &dyn Fn(&Path) -> Result<u64, String>, verify: &dyn Fn(&Path) -> Result<(), String>, step: &dyn Fn(Step)) -> Result<PathBuf, String> {
    if pin.url.is_empty() {
        return Err(format!("pack {} is placed, never fetched", pin.version));
    }
    let get = |url: &str, to: &Path| fetch_while(fetch, pin, url, to, log, alive, step);
    refetch_steps(data, pin, &get, free, verify, step).map_err(|e| format!("pack {} not fetched again: {e:?}", pin.version))
}

#[cfg(not(feature = "rehearsal_pack"))]
fn placed(_: &Path, _: &PackPin) -> Option<Result<PathBuf, String>> {
    None
}

#[cfg(feature = "rehearsal_pack")]
fn placed(data: &Path, pin: &PackPin) -> Option<Result<PathBuf, String>> {
    if !pin.url.is_empty() || pin.sha256.is_empty() {
        return None;
    }
    let dir = data.join("runtimes").join(format!("dsh-{}", pin.version));
    Some(match fs::read(dir.join(MANIFEST)) {
        Err(e) => Err(format!("no placed pack {}: {e}", pin.version)),
        Ok(manifest) => match sha256_of(&manifest[..]) {
            Ok(sha) if sha == pin.sha256 => Ok(dir),
            _ => Err(format!("the placed pack {}'s manifest is not the pinned one", pin.version)),
        },
    })
}

#[cfg(test)]
pub(crate) fn start_pin_unset() -> bool {
    start_pin(&|_| {}).is_ok_and(|(p, _)| p.sha256.is_empty())
}

#[cfg(test)]
pub(crate) static TEST_ENV: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::{Duration, Instant};
    use tar::EntryType as T;

    static N: AtomicUsize = AtomicUsize::new(0);

    struct Tmp(PathBuf);
    impl std::ops::Deref for Tmp {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("unfold-pack-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("data")).unwrap();
        Tmp(d)
    }
    fn leak(s: &str) -> &'static str {
        Box::leak(s.to_owned().into_boxed_str())
    }
    fn hex(b: &[u8]) -> String {
        b.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn pin(version: &str, url: &str, bytes: &[u8]) -> PackPin {
        PackPin { version: leak(version), url: leak(url), sha256: leak(&hex(&Sha256::digest(bytes))), size: bytes.len() as u64 }
    }
    fn plenty(_: &Path) -> Result<u64, String> {
        Ok(u64::MAX)
    }
    fn signed(_: &Path) -> Result<(), String> {
        Ok(())
    }

    fn left(data: &Path) -> Vec<String> {
        let mut v: Vec<String> = match fs::read_dir(data.join("runtimes")) {
            Ok(rd) => rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n != ".lock").collect(),
            Err(_) => vec![],
        };
        v.sort();
        v
    }

    enum E<'a> {
        F(&'a str, &'a [u8], u32),
        D(&'a str, u32),
        L(&'a str, &'a str),
        K(&'a str, T, &'a str, u32),
    }

    fn pack(es: &[E]) -> Vec<u8> {
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for e in es {
            let (name, kind, link, mode, data): (&str, T, &str, u32, &[u8]) = match *e {
                E::F(n, d, m) => (n, T::Regular, "", m, d),
                E::D(n, m) => (n, T::Directory, "", m, b""),
                E::L(n, t) => (n, T::Symlink, t, 0o777, b""),
                E::K(n, k, t, m) => (n, k, t, m, b""),
            };
            let mut h = tar::Header::new_gnu();
            h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
            h.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
            h.set_entry_type(kind);
            h.set_mode(mode);
            h.set_size(data.len() as u64);
            h.set_mtime(1_700_000_000);
            h.set_cksum();
            b.append(&h, data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap()
    }

    const MACHO: &[u8] = &[0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01, 0, 0, 0, 0];
    const LIBVIPS: &str = "lib/libvips-cpp.42.dylib";
    fn good() -> Vec<u8> {
        pack(&[
            E::D("bin/", 0o755),
            E::F("bin/node", MACHO, 0o755),
            E::F(LIBVIPS, MACHO, 0o644),
            E::F("app/server.js", b"console.log(1)\n", 0o644),
            E::F("app/package.json", b"{}\n", 0o644),
            E::L("app/node", "../bin/node"),
            E::L("app/bin", "../bin"),
            E::L("app/tool", "bin/node"),
            E::D("app/empty/", 0o755),
        ])
    }

    fn serve(body: Vec<u8>, send: usize) -> (String, std::thread::JoinHandle<()>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/dsh-pack.tar.gz", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();

            let mut req = Vec::new();
            let mut b = [0u8; 256];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&b[..n]),
                }
            }
            let _ = write!(s, "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            let _ = s.write_all(&body[..send]);
        });
        (url, h)
    }

    fn finish(url: &str, h: std::thread::JoinHandle<()>) {
        let host = url.strip_prefix("http://").and_then(|r| r.split_once('/')).unwrap().0;
        let _ = std::net::TcpStream::connect(host);
        h.join().unwrap();
    }

    fn http_get(url: &str, to: &Path) -> Result<(), String> {
        let (host, path) = url.strip_prefix("http://").and_then(|r| r.split_once('/')).ok_or("not an http url")?;
        let mut s = std::net::TcpStream::connect(host).map_err(|e| e.to_string())?;
        s.write_all(format!("GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes()).map_err(|e| e.to_string())?;
        let mut all = Vec::new();
        s.read_to_end(&mut all).map_err(|e| e.to_string())?;
        let at = all.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no header end")? + 4;
        fs::write(to, &all[at..]).map_err(|e| e.to_string())
    }

    #[test]
    fn a_good_pack_from_a_local_file_server_is_unpacked_whole() {
        let base = tmp("good");
        let data = base.join("data");
        let bytes = good();
        let (url, h) = serve(bytes.clone(), bytes.len());
        let dir = ensure_with(&data, &pin("1.0.0", &url, &bytes), &http_get, &plenty, &signed);
        finish(&url, h);
        let dir = dir.unwrap();
        assert_eq!(dir, data.join("runtimes/dsh-1.0.0"));
        assert_eq!(fs::read(dir.join("app/server.js")).unwrap(), b"console.log(1)\n");
        assert_eq!(fs::read_link(dir.join("app/node")).unwrap(), Path::new("../bin/node"));
        assert!(dir.join("app/empty").is_dir() && dir.join(MANIFEST).is_file());
        assert_eq!(fs::metadata(dir.join("bin/node")).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(left(&data), ["dsh-1.0.0"]);
        assert_eq!(check_manifest(&dir, &[]), Ok(()));
    }

    #[test]
    fn a_tampered_pack_is_refused_and_leaves_nothing() {
        let base = tmp("tampered");
        let data = base.join("data");
        let bytes = good();
        let mut bad = bytes.clone();
        let i = bad.len() / 2;
        bad[i] ^= 0xff;
        let r = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bad).map_err(|e| e.to_string()), &plenty, &signed);
        assert_eq!(r, Err(PackError::Hash));
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn a_truncated_download_is_refused_and_leaves_nothing() {
        let base = tmp("truncated");
        let data = base.join("data");
        let bytes = good();
        let (url, h) = serve(bytes.clone(), bytes.len() / 2);
        let r = ensure_with(&data, &pin("1.0.0", &url, &bytes), &http_get, &plenty, &signed);
        finish(&url, h);
        assert_eq!(r, Err(PackError::Hash));
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn unsafe_archives_are_refused_and_leave_nothing() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("absolute path", pack(&[E::F("/unfold-pack-escape", b"x", 0o644)])),
            ("parent dir", pack(&[E::F("../escape", b"x", 0o644)])),
            ("inner parent dir", pack(&[E::D("a/", 0o755), E::F("a/../../../escape", b"x", 0o644)])),
            ("absolute link", pack(&[E::L("etc", "/etc")])),
            ("escaping link", pack(&[E::D("a/", 0o755), E::L("a/up", "../../escape")])),
            ("write through a link", pack(&[E::D("sub/", 0o755), E::L("in", "sub"), E::F("in/f", b"x", 0o644)])),
            ("link chain out", pack(&[E::L("s", "."), E::L("t", "s/s/s/../../..")])),
            ("dangling link out through a link", pack(&[E::D("a/", 0o755), E::L("a/up", ".."), E::L("t", "a/up/../x")])),
            ("deep dangling link out through a link", pack(&[E::D("a/b/c/", 0o755), E::L("a/b/c/up", "../../.."), E::L("a/b/c/t", "up/../../../escape")])),
            ("link to nothing", pack(&[E::L("n", "missing")])),
            ("link loop", pack(&[E::L("l", "m"), E::L("m", "l")])),
            ("link through a file", pack(&[E::F("f", b"x", 0o644), E::L("n", "f/x")])),
            ("setuid file", pack(&[E::F("bin/x", b"x", 0o4755)])),
            ("setgid dir", pack(&[E::D("d/", 0o2755)])),
            ("hard link", pack(&[E::F("a", b"x", 0o644), E::K("b", T::Link, "a", 0o644)])),
            ("char device", pack(&[E::K("tty", T::Char, "", 0o644)])),
            ("block device", pack(&[E::K("disk", T::Block, "", 0o644)])),
            ("fifo", pack(&[E::K("pipe", T::Fifo, "", 0o644)])),
            ("manifest name", pack(&[E::F(MANIFEST, b"{}", 0o644)])),
            ("duplicate entry", pack(&[E::F("a", b"x", 0o644), E::F("a", b"y", 0o644)])),
        ];
        for (what, bytes) in cases {
            let base = tmp("unsafe");
            let data = base.join("data");
            let r = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed);
            assert!(matches!(r, Err(PackError::Unsafe(_))), "{what}: {r:?}");
            assert_eq!(left(&data), Vec::<String>::new(), "{what}");
            assert!(!base.join("escape").exists() && !data.join("escape").exists() && !Path::new("/unfold-pack-escape").exists(), "{what}");
        }
    }

    fn loose_modes() -> Vec<u8> {
        pack(&[
            E::D("open/", 0o777),
            E::F("open/all.js", b"x", 0o777),
            E::F("open/sticky.js", b"x", 0o1777),
            E::D("sticky/", 0o1777),
            E::F("group.js", b"x", 0o664),
            E::F("ro.js", b"x", 0o444),
            E::F("deep/er/made.js", b"x", 0o666),
        ])
    }
    fn unpack_loose_modes(data: &Path) -> PathBuf {
        let bytes = loose_modes();
        ensure_with(data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap()
    }

    fn assert_tight_modes(dir: &Path) {
        let mode = |rel: &str| fs::symlink_metadata(dir.join(rel)).unwrap().permissions().mode() & 0o7777;
        for rel in ["open", "open/all.js", "open/sticky.js", "sticky", "group.js", "ro.js", "deep", "deep/er", "deep/er/made.js"] {
            assert_eq!(mode(rel) & 0o7022, 0, "{rel}: {:o}", mode(rel));
        }
        assert_eq!([mode("open/all.js"), mode("open/sticky.js"), mode("group.js"), mode("ro.js"), mode("deep/er/made.js")], [0o755, 0o755, 0o644, 0o444, 0o644]);
        assert_eq!([mode("open"), mode("sticky"), mode("deep"), mode("deep/er")], [0o755; 4]);
        assert_eq!([mode(""), mode("..")].map(|m| m & 0o7077), [0, 0], "the pack and runtimes/ are the owner's only");
    }

    #[test]
    fn unpacked_files_and_dirs_carry_no_group_or_world_write_and_no_special_bits() {
        let base = tmp("modes");
        assert_tight_modes(&unpack_loose_modes(&base.join("data")));

        for m in [0o4755, 0o2755, 0o6777] {
            let base = tmp("modes-suid");
            let data = base.join("data");
            let bytes = pack(&[E::F("bin/x", b"x", m)]);
            let r = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed);
            assert!(matches!(&r, Err(PackError::Unsafe(w)) if w.contains("setuid or setgid")), "{m:o}: {r:?}");
            assert_eq!(left(&data), Vec::<String>::new());
        }
    }

    const UMASK_ZERO_DATA: &str = "UNFOLD_PACK_UMASK_ZERO_DATA";
    const UMASK_STRICT_DATA: &str = "UNFOLD_PACK_UMASK_STRICT_DATA";

    fn unpack_in_a_child(child: &str, var: &str, data: &Path) -> PathBuf {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([child, "--exact", "--nocapture", "--test-threads=1"])
            .env(var, data)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("1 passed"), "the {child} unpack did not run whole: {said}");
        data.join("runtimes/dsh-1.0.0")
    }

    #[test]
    fn umask_zero_unpack_child() {
        let Some(data) = std::env::var_os(UMASK_ZERO_DATA) else { return };
        unsafe { libc::umask(0) };
        unpack_loose_modes(Path::new(&data));
    }

    #[test]
    fn modes_stay_tight_under_a_zero_umask() {
        let base = tmp("umask0");
        assert_tight_modes(&unpack_in_a_child("dsh_pack::tests::umask_zero_unpack_child", UMASK_ZERO_DATA, &base.join("data")));
    }

    #[test]
    fn umask_strict_unpack_child() {
        let Some(data) = std::env::var_os(UMASK_STRICT_DATA) else { return };
        unsafe { libc::umask(0o077) };
        unpack_loose_modes(Path::new(&data));
    }

    #[test]
    fn modes_stay_whole_under_a_strict_umask() {
        let base = tmp("umask077");
        assert_tight_modes(&unpack_in_a_child("dsh_pack::tests::umask_strict_unpack_child", UMASK_STRICT_DATA, &base.join("data")));
    }

    #[test]
    fn a_full_disk_stops_before_the_download() {
        let base = tmp("disk");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let calls = AtomicUsize::new(0);
        let need = p.size * 3 + FLOOR;
        let r = ensure_with(&data, &p, &|_, _| { calls.fetch_add(1, Ordering::SeqCst); Ok(()) }, &|_| Ok(need - 1), &signed);
        assert_eq!(r, Err(PackError::Disk { need, free: need - 1 }));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn a_failed_download_leaves_nothing() {
        let base = tmp("fetchfail");
        let data = base.join("data");
        let bytes = good();
        let r = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| { fs::write(to, b"partial").unwrap(); Err("connection reset".into()) }, &plenty, &signed);
        assert_eq!(r, Err(PackError::Io("connection reset".into())));
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn every_mach_o_is_verified_and_a_bad_signature_leaves_nothing() {
        let base = tmp("sig");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let fetch = |_: &str, to: &Path| fs::write(to, &bytes).map_err(|e| e.to_string());
        let seen = std::sync::Mutex::new(Vec::new());
        let r = ensure_with(&data, &p, &fetch, &plenty, &|f| {
            seen.lock().unwrap().push(f.file_name().unwrap().to_string_lossy().into_owned());
            if f.ends_with("bin/node") { Err("invalid signature (code or signature have been modified)".into()) } else { Ok(()) }
        });
        assert!(matches!(&r, Err(PackError::Signature(m)) if m.contains("bin/node")), "{r:?}");
        assert_eq!(left(&data), Vec::<String>::new());
        let seen = std::mem::take(&mut *seen.lock().unwrap());
        let r = ensure_with(&data, &p, &fetch, &plenty, &|f| { seen_ok(f); Ok(()) });
        assert!(r.is_ok());
        assert!(seen.iter().all(|n| n == "node" || n == "libvips-cpp.42.dylib"), "{seen:?}");
    }
    fn seen_ok(f: &Path) {
        assert!(f.ends_with("bin/node") || f.ends_with(LIBVIPS), "{} is not a Mach-O", f.display());
    }

    #[test]
    fn two_concurrent_ensures_unpack_once() {
        let base = tmp("race");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let calls = AtomicUsize::new(0);
        let go = Barrier::new(2);
        let fetch = |_: &str, to: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
            fs::write(to, &bytes).map_err(|e| e.to_string())
        };
        let got: Vec<_> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..2).map(|_| s.spawn(|| { go.wait(); ensure_with(&data, &p, &fetch, &plenty, &signed) })).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(got[0], Ok(data.join("runtimes/dsh-1.0.0")));
        assert_eq!(got[0], got[1]);
        assert_eq!(left(&data), ["dsh-1.0.0"]);
    }

    #[test]
    fn stale_temp_files_are_swept_and_a_present_pack_is_not_fetched_again() {
        let base = tmp("sweep");
        let data = base.join("data");
        fs::create_dir_all(data.join("runtimes/.tmp-0.9.0-1.d/deep")).unwrap();
        fs::write(data.join("runtimes/.tmp-0.9.0-1.tar.gz"), b"half").unwrap();
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let calls = AtomicUsize::new(0);
        let fetch = |_: &str, to: &Path| { calls.fetch_add(1, Ordering::SeqCst); fs::write(to, &bytes).map_err(|e| e.to_string()) };
        ensure_with(&data, &p, &fetch, &plenty, &signed).unwrap();
        assert_eq!(left(&data), ["dsh-1.0.0"]);
        fs::write(data.join("runtimes/.tmp-prune-dsh-0.9.0"), b"").unwrap();
        ensure_with(&data, &p, &fetch, &plenty, &signed).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(left(&data), ["dsh-1.0.0"]);
    }

    #[test]
    fn the_old_pack_is_kept_until_the_new_one_was_healthy_once() {
        let base = tmp("health");
        let data = base.join("data");
        let (one, two) = (good(), pack(&[E::F("app/server.js", b"console.log(2)\n", 0o644)]));
        let v1 = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/1", &one), &|_, to| fs::write(to, &one).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        assert_eq!(healthy(&data), None);
        mark_healthy(&data, "1.0.0").unwrap();
        assert_eq!(healthy(&data), Some(v1.clone()));
        let v2 = ensure_with(&data, &pin("2.0.0", "http://127.0.0.1/2", &two), &|_, to| fs::write(to, &two).map_err(|e| e.to_string()), &plenty, &signed).unwrap();

        assert_eq!(left(&data), ["dsh-1.0.0", "dsh-2.0.0", "healthy.json"]);
        assert_eq!(healthy(&data), Some(v1.clone()));
        mark_healthy(&data, "2.0.0").unwrap();
        assert_eq!(healthy(&data), Some(v2));
        assert_eq!(left(&data), ["dsh-2.0.0", "healthy.json"]);
        assert!(mark_healthy(&data, "3.0.0").is_err());
        assert!(!v1.exists());
    }

    #[test]
    fn check_manifest_names_exactly_the_changed_files() {
        let base = tmp("manifest");
        let data = base.join("data");
        let bytes = good();
        let dir = ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        fs::write(dir.join("app/server.js"), b"changed\n").unwrap();
        fs::remove_file(dir.join("app/package.json")).unwrap();
        fs::write(dir.join("app/extra.js"), b"new\n").unwrap();
        fs::set_permissions(dir.join("bin/node"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_file(dir.join("app/node")).unwrap();
        std::os::unix::fs::symlink(format!("../{LIBVIPS}"), dir.join("app/node")).unwrap();
        fs::write(dir.join(LIBVIPS), b"rebuilt libvips").unwrap();
        let want: Vec<PathBuf> = ["app/extra.js", "app/node", "app/package.json", "app/server.js", "bin/node"].map(PathBuf::from).to_vec();
        assert_eq!(check_manifest(&dir, &[PathBuf::from(LIBVIPS)]), Err(want.clone()));
        assert_eq!(check_manifest(&dir, &[dir.join(LIBVIPS)]), Err(want));
        assert!(check_manifest(&dir, &[]).unwrap_err().contains(&PathBuf::from(LIBVIPS)));
        fs::remove_file(dir.join(MANIFEST)).unwrap();
        assert_eq!(check_manifest(&dir, &[]), Err(vec![PathBuf::from(MANIFEST)]));
    }

    #[test]
    fn ensure_checks_the_real_free_space_first() {

        let base = tmp("statvfs");
        let data = base.join("data");
        let p = PackPin { version: "1.0.0", url: "http://127.0.0.1/p", sha256: "00", size: u64::MAX / 4 };
        let calls = AtomicUsize::new(0);
        let r = ensure(&data, &p, &|_, _| { calls.fetch_add(1, Ordering::SeqCst); Err("offline".into()) });
        let need = p.size * 3 + FLOOR;
        assert!(matches!(r, Err(PackError::Disk { need: n, free }) if n == need && free > 0 && free < need), "{r:?}");
        let r = refetch(&data, &p, &|_, _| { calls.fetch_add(1, Ordering::SeqCst); Err("offline".into()) });
        assert!(matches!(r, Err(PackError::Disk { need: n, free }) if n == need && free > 0 && free < need), "{r:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn a_pack_s_autopg_cache_ships_0755_so_autopg_s_chmod_changes_nothing_and_a_changed_postgres_is_refused() {
        let dir = tmp("autopg-cache").join("p");
        let bin = dir.join("omni/pgserve/bin/darwin-arm64/bin");
        fs::create_dir_all(&bin).unwrap();
        let both = |mode: u32| {
            for b in ["initdb", "postgres"] {
                fs::set_permissions(bin.join(b), fs::Permissions::from_mode(mode)).unwrap();
            }
        };
        for b in ["initdb", "postgres"] {
            fs::write(bin.join(b), format!("{b} binary\n")).unwrap();
        }

        let autopg_start = || both(0o755);
        let rel = |b: &str| PathBuf::from("omni/pgserve/bin/darwin-arm64/bin").join(b);

        both(0o755);
        write_manifest(&dir).unwrap();
        autopg_start();
        assert_eq!(check_manifest(&dir, &[]), Ok(()));

        fs::write(bin.join("postgres"), "postgres binarY\n").unwrap();
        autopg_start();
        assert_eq!(check_manifest(&dir, &[]), Err(vec![rel("postgres")]));

        fs::write(bin.join("postgres"), "postgres binary\n").unwrap();
        both(0o555);
        fs::remove_file(dir.join(MANIFEST)).unwrap();
        write_manifest(&dir).unwrap();
        autopg_start();
        assert_eq!(check_manifest(&dir, &[]), Err(vec![rel("initdb"), rel("postgres")]));
    }

    #[test]
    fn refetch_replaces_a_changed_pack_with_a_whole_one() {
        let base = tmp("refetch");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let calls = AtomicUsize::new(0);
        let fetch = |_: &str, to: &Path| { calls.fetch_add(1, Ordering::SeqCst); fs::write(to, &bytes).map_err(|e| e.to_string()) };
        let dir = ensure_with(&data, &p, &fetch, &plenty, &signed).unwrap();
        fs::write(dir.join("app/server.js"), b"changed\n").unwrap();
        assert_eq!(check_manifest(&dir, &[]), Err(vec![PathBuf::from("app/server.js")]));
        assert_eq!(refetch_with(&data, &p, &fetch, &plenty, &signed), Ok(dir.clone()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(check_manifest(&dir, &[]), Ok(()));
        assert_eq!(left(&data), ["dsh-1.0.0"]);

        fs::remove_file(dir.join(MANIFEST)).unwrap();
        assert!(matches!(ensure_with(&data, &p, &fetch, &plenty, &signed), Err(PackError::Unsafe(_))));
        assert_eq!(refetch_with(&data, &p, &fetch, &plenty, &signed), Ok(dir.clone()));
        assert_eq!(check_manifest(&dir, &[]), Ok(()));

        let other = tmp("refetch-first");
        assert_eq!(refetch_with(&other.join("data"), &p, &fetch, &plenty, &signed), Ok(other.join("data/runtimes/dsh-1.0.0")));
    }

    #[test]
    fn a_failed_refetch_puts_the_old_pack_back_as_it_was() {
        let base = tmp("refetch-fail");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let dir = ensure_with(&data, &p, &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        fs::write(dir.join("app/server.js"), b"changed\n").unwrap();
        let mut bad = bytes.clone();
        let i = bad.len() / 2;
        bad[i] ^= 0xff;
        let failures: [(&dyn Fn(&str, &Path) -> Result<(), String>, PackError); 2] = [
            (&|_, to| fs::write(to, &bad).map_err(|e| e.to_string()), PackError::Hash),
            (&|_, _| Err("connection reset".into()), PackError::Io("connection reset".into())),
        ];
        for (fetch, want) in failures {
            assert_eq!(refetch_with(&data, &p, fetch, &plenty, &signed), Err(want));
            assert_eq!(left(&data), ["dsh-1.0.0"]);
            assert_eq!(fs::read(dir.join("app/server.js")).unwrap(), b"changed\n");
            assert_eq!(check_manifest(&dir, &[]), Err(vec![PathBuf::from("app/server.js")]));
        }
        assert_eq!(refetch_with(&data, &p, &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &|_| Ok(0), &signed), Err(PackError::Disk { need: p.size * 3 + FLOOR, free: 0 }));
        assert_eq!(left(&data), ["dsh-1.0.0"]);
    }

    #[test]
    fn two_concurrent_refetches_never_overlap() {
        let base = tmp("refetch-race");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let (calls, active, overlap) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
        let fetch = |_: &str, to: &Path| {
            calls.fetch_add(1, Ordering::SeqCst);
            if active.fetch_add(1, Ordering::SeqCst) > 0 {
                overlap.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(Duration::from_millis(50));
            active.fetch_sub(1, Ordering::SeqCst);
            fs::write(to, &bytes).map_err(|e| e.to_string())
        };
        let dir = ensure_with(&data, &p, &fetch, &plenty, &signed).unwrap();
        let go = Barrier::new(2);
        let got: Vec<_> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..2).map(|_| s.spawn(|| { go.wait(); refetch_with(&data, &p, &fetch, &plenty, &signed) })).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(got, [Ok(dir.clone()), Ok(dir.clone())]);
        assert_eq!((calls.load(Ordering::SeqCst), overlap.load(Ordering::SeqCst)), (3, 0));
        assert_eq!(left(&data), ["dsh-1.0.0"]);
        assert_eq!(check_manifest(&dir, &[]), Ok(()));
    }

    #[test]
    fn a_pack_an_interrupted_refetch_moved_aside_is_put_back() {
        let base = tmp("aside");
        let data = base.join("data");
        let bytes = good();
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let calls = AtomicUsize::new(0);
        let fetch = |_: &str, to: &Path| { calls.fetch_add(1, Ordering::SeqCst); fs::write(to, &bytes).map_err(|e| e.to_string()) };
        let dir = ensure_with(&data, &p, &fetch, &plenty, &signed).unwrap();

        fs::rename(&dir, data.join("runtimes/.aside-dsh-1.0.0")).unwrap();
        assert_eq!(ensure_with(&data, &p, &fetch, &plenty, &signed), Ok(dir.clone()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(left(&data), ["dsh-1.0.0"]);

        fs::create_dir_all(data.join("runtimes/.aside-dsh-1.0.0/app")).unwrap();
        assert_eq!(ensure_with(&data, &p, &fetch, &plenty, &signed), Ok(dir.clone()));
        assert_eq!(left(&data), ["dsh-1.0.0"]);
        assert_eq!(check_manifest(&dir, &[]), Ok(()));
    }

    #[test]
    fn a_held_lock_is_waited_on_only_so_long() {
        let base = tmp("lock");
        let path = base.join("data/.lock");
        let held = Lock::take(&path, Duration::from_secs(1)).unwrap();
        let t = Instant::now();
        let e = Lock::take(&path, Duration::from_millis(200)).err().unwrap();
        assert!(t.elapsed() >= Duration::from_millis(200) && e.contains("still held by another fetch"), "{e}");
        drop(held);
        assert!(Lock::take(&path, Duration::from_millis(200)).is_ok());
    }

    #[test]
    fn codesign_refuses_a_changed_mach_o() {
        let base = tmp("codesign");
        let ok = base.join("signed");
        fs::copy(std::env::current_exe().unwrap(), &ok).unwrap();
        assert!(is_macho(&ok));
        assert_eq!(codesign_verify(&ok), Ok(()));
        let mut b = fs::read(&ok).unwrap();
        b[0x4000 + 100] ^= 0xff;
        let bad = base.join("changed");
        fs::write(&bad, &b).unwrap();
        assert!(codesign_verify(&bad).is_err());
        fs::write(base.join("text"), b"#!/bin/sh\n").unwrap();
        assert!(!is_macho(&base.join("text")));
    }

    #[test]
    #[ignore]
    fn manifest_check_time_on_a_432_mb_tree() {
        let root = std::env::var("UNFOLD_PACK_TIME_TREE").unwrap_or_else(|_| panic!("set UNFOLD_PACK_TIME_TREE to a scratch directory"));
        let dir = PathBuf::from(root).join("dsh-time");
        let _ = fs::remove_dir_all(&dir);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut fill = |p: &Path, n: usize| {
            let mut v = vec![0u8; n];
            for c in v.chunks_mut(8) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                c.copy_from_slice(&seed.to_le_bytes()[..c.len()]);
            }
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, v).unwrap();
        };
        let mut total = 0;
        let mut files = 0;
        for (name, n) in [("bin/node", 110_000_000), ("program/libmerged.dylib", 60_000_000)].into_iter().chain((0..20).map(|i| (leak(&format!("lib/big-{i}.dylib")), 5_000_000))) {
            fill(&dir.join(name), n);
            total += n;
            files += 1;
        }
        for d in 0..300 {
            for f in 0..100 {
                fill(&dir.join(format!("node_modules/p{d}/f{f}.js")), 5_400);
                total += 5_400;
                files += 1;
            }
        }
        let t = Instant::now();
        write_manifest(&dir).unwrap();
        let build_ms = t.elapsed().as_millis();
        let check_ms: Vec<u128> = (0..3).map(|_| { let t = Instant::now(); assert_eq!(check_manifest(&dir, &[]), Ok(())); t.elapsed().as_millis() }).collect();
        println!("manifest-time {{\"bytes\": {total}, \"files\": {files}, \"build_ms\": {build_ms}, \"check_ms\": {check_ms:?}, \"threads\": {}}}", hash_threads());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[derive(Default)]
    struct Get(AtomicUsize);
    impl crate::dictation_models::Fetch for Get {
        fn fetch(&self, url: &str, part: &Path, _: &std::sync::atomic::AtomicBool, _: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            http_get(url, part)
        }
    }

    #[test]
    fn an_unset_pin_is_refused_before_anything_is_fetched_or_written() {
        let base = tmp("unset");
        let data = base.join("data");
        let calls = AtomicUsize::new(0);
        let count = |_: &str, _: &Path| -> Result<(), String> {
            calls.fetch_add(1, Ordering::SeqCst);
            Err("offline".into())
        };
        assert_eq!(ensure_with(&data, &PackPin::UNSET, &count, &plenty, &signed), Err(PackError::Unset));
        assert_eq!(refetch_with(&data, &PackPin::UNSET, &count, &plenty, &signed), Err(PackError::Unset));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing fetched");
        assert!(!data.join("runtimes").exists(), "nothing written");
    }

    #[test]
    fn pack_pin_takes_the_release_line_or_unset_and_nothing_else() {
        let sha = leak(&"ab".repeat(32));
        let line = leak(&format!("0.5.0 https://example.invalid/unfold-dsh-pack-0.5.0.tar.gz {sha} 1234\n"));
        assert_eq!(PackPin::parse(line), Ok(PackPin { version: "0.5.0", url: "https://example.invalid/unfold-dsh-pack-0.5.0.tar.gz", sha256: sha, size: 1234 }));
        assert_eq!(PackPin::parse("unset\n"), Ok(PackPin::UNSET));
        let upper = leak(&format!("0.5.0 https://example.invalid/p.tar.gz {} 1234", sha.to_uppercase()));
        let short = leak(&format!("0.5.0 https://example.invalid/p.tar.gz {} 1234", &sha[1..]));
        let zero = leak(&format!("0.5.0 https://example.invalid/p.tar.gz {sha} 0"));
        let more = leak(&format!("0.5.0 https://example.invalid/p.tar.gz {sha} 1234 x"));
        for bad in ["", "unset now", "0.5.0 https://example.invalid/p.tar.gz", upper, short, zero, more] {
            assert!(PackPin::parse(bad).is_err(), "{bad:?}");
        }
        assert!(PackPin::parse(PACK_PIN).is_ok(), "this build's pack.pin: {PACK_PIN:?}");
    }

    #[test]
    fn the_start_path_on_the_placeholder_pin_fetches_nothing_and_writes_nothing() {
        let _env = TEST_ENV.lock().unwrap_or_else(|e| e.into_inner());
        if !start_pin_unset() {
            return;
        }
        let base = tmp("start-unset");
        let data = base.join("data");
        let r = for_start(&data, &|_| {}, &|| true, &|_| {});
        assert!(r.as_ref().is_err_and(|e| e.contains("no pack pin")), "{r:?}");
        assert!(!data.join("runtimes").exists(), "nothing written");
    }

    #[test]
    fn the_start_path_unpacks_a_good_pack_from_a_local_server_once() {
        let base = tmp("start-good");
        let data = base.join("data");
        let bytes = good();
        let (url, h) = serve(bytes.clone(), bytes.len());
        let get = Get::default();
        let p = pin("1.0.0", &url, &bytes);
        let dir = pinned_with(&data, &p, &get, &|_| {}, &|| true, &plenty, &signed);
        finish(&url, h);
        assert_eq!(dir, Ok(Picked { dir: data.join("runtimes/dsh-1.0.0"), fell_back: None }));
        assert_eq!(check_manifest(&data.join("runtimes/dsh-1.0.0"), &[]), Ok(()));

        assert_eq!(pinned_with(&data, &p, &get, &|_| {}, &|| true, &plenty, &signed), Ok(Picked { dir: data.join("runtimes/dsh-1.0.0"), fell_back: None }));
        assert_eq!(get.0.load(Ordering::SeqCst), 1, "fetched once");
        assert_eq!(left(&data), ["dsh-1.0.0"]);
    }

    #[test]
    fn the_start_path_refuses_a_hash_mismatch_from_a_local_server_and_leaves_nothing() {
        let base = tmp("start-hash");
        let data = base.join("data");
        let bytes = good();
        let mut bad = bytes.clone();
        let i = bad.len() / 2;
        bad[i] ^= 0xff;
        let (url, h) = serve(bad.clone(), bad.len());
        let get = Get::default();
        let r = pinned_with(&data, &pin("1.0.0", &url, &bytes), &get, &|_| {}, &|| true, &plenty, &signed);
        finish(&url, h);
        assert!(r.as_ref().is_err_and(|e| e.contains("Hash")), "{r:?}");
        assert_eq!(get.0.load(Ordering::SeqCst), 1, "the server was asked once");
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[test]
    fn the_start_path_falls_back_to_the_last_healthy_pack_but_never_on_an_unset_pin() {
        let base = tmp("start-healthy");
        let data = base.join("data");
        let bytes = good();
        ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        mark_healthy(&data, "1.0.0").unwrap();
        let offline = |_: &str, _: &Path| -> Result<(), String> { Err("offline".into()) };
        struct Off<'a>(&'a dyn Fn(&str, &Path) -> Result<(), String>);
        impl crate::dictation_models::Fetch for Off<'_> {
            fn fetch(&self, url: &str, part: &Path, _: &std::sync::atomic::AtomicBool, _: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
                (self.0)(url, part)
            }
        }
        let lines = std::sync::Mutex::new(Vec::new());
        let log = |l: &str| lines.lock().unwrap().push(l.to_string());
        let newer = pin("2.0.0", "http://127.0.0.1/p", b"a newer pack");
        assert_eq!(pinned_with(&data, &newer, &Off(&offline), &log, &|| true, &plenty, &signed), Ok(Picked { dir: data.join("runtimes/dsh-1.0.0"), fell_back: Some(PackError::Io("offline".into())) }));
        assert!(lines.lock().unwrap().iter().any(|l| l.contains("starting the last healthy pack")), "{:?}", lines.lock().unwrap());
        let r = pinned_with(&data, &PackPin::UNSET, &Off(&offline), &log, &|| true, &plenty, &signed);
        assert!(r.as_ref().is_err_and(|e| e.contains("no pack pin")), "an unset pin starts nothing: {r:?}");
        assert!(pinned_with(&data, &newer, &Off(&offline), &log, &|| false, &plenty, &signed).is_err(), "a changed switch starts nothing");
        assert_eq!(left(&data), ["dsh-1.0.0", "healthy.json"]);
    }

    #[test]
    fn a_switch_change_cancels_the_pack_fetch_and_leaves_nothing() {
        let base = tmp("start-cancel");
        let data = base.join("data");

        #[derive(Default)]
        struct Slow(AtomicUsize);
        impl crate::dictation_models::Fetch for Slow {
            fn fetch(&self, _: &str, part: &Path, cancel: &std::sync::atomic::AtomicBool, _: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                fs::write(part, b"part").map_err(|e| e.to_string())?;
                let end = Instant::now() + Duration::from_secs(10);
                while !cancel.load(Ordering::SeqCst) && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(crate::dictation_models::CANCELLED.into())
            }
        }
        let slow = Slow::default();
        let off_at = Instant::now() + Duration::from_millis(300);
        let t = Instant::now();
        let r = pinned_with(&data, &pin("1.0.0", "http://127.0.0.1/p", b"x"), &slow, &|_| {}, &|| Instant::now() < off_at, &plenty, &signed);
        assert!(r.is_err() && t.elapsed() < Duration::from_secs(3), "{r:?} after {:?}", t.elapsed());
        assert_eq!(slow.0.load(Ordering::SeqCst), 1, "the fetch had started");
        assert_eq!(left(&data), Vec::<String>::new());
    }

    #[cfg(feature = "rehearsal_pack")]
    #[test]
    fn a_rehearsal_pin_is_taken_only_from_a_127_0_0_1_pack_server() {
        let sha = "cd".repeat(32);
        let ok = leak(&format!("0.5.0-r1 http://127.0.0.1:41234/unfold-dsh-pack.tar.gz {sha} 99"));
        assert_eq!(rehearsal_pin(ok).map(|p| p.url), Ok("http://127.0.0.1:41234/unfold-dsh-pack.tar.gz"));
        for url in ["https://example.invalid/p.tar.gz", "http://localhost:41234/p.tar.gz", "http://127.0.0.10:41234/p.tar.gz", "http://127.0.0.1:41234@example.invalid/p.tar.gz", "http://127.0.0.1/p.tar.gz", "http://127.0.0.1:0/p.tar.gz"] {
            assert!(rehearsal_pin(leak(&format!("0.5.0-r1 {url} {sha} 99"))).is_err(), "{url}");
        }
        assert!(rehearsal_pin("unset").is_err());
    }

    #[cfg(feature = "rehearsal_pack")]
    #[test]
    fn a_rehearsal_build_reads_its_pin_from_the_environment_over_plain_http() {
        let _env = TEST_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let line = format!("0.5.0-r1 http://127.0.0.1:41234/unfold-dsh-pack.tar.gz {} 99", "cd".repeat(32));
        let lines = std::sync::Mutex::new(Vec::new());
        std::env::set_var(REHEARSAL_PIN, &line);
        let r = start_pin(&|l| lines.lock().unwrap().push(l.to_string()));
        std::env::remove_var(REHEARSAL_PIN);
        let (p, https_only) = r.unwrap();
        assert_eq!((p.version, p.url, https_only), ("0.5.0-r1", "http://127.0.0.1:41234/unfold-dsh-pack.tar.gz", false));
        assert!(lines.lock().unwrap().iter().any(|l| l.contains("rehearsal pack pin")), "{:?}", lines.lock().unwrap());
        assert_eq!(start_pin(&|_| {}), Ok((REHEARSAL_PACK, false)), "without it: the rehearsal pack's own pin");
    }

    #[test]
    fn a_hash_refused_newer_pack_starts_the_last_healthy_one_and_says_why() {
        let base = tmp("start-hash-healthy");
        let data = base.join("data");
        let bytes = good();
        ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        mark_healthy(&data, "1.0.0").unwrap();
        let newer = good();
        let mut bad = newer.clone();
        let i = bad.len() / 2;
        bad[i] ^= 0xff;
        let (url, h) = serve(bad.clone(), bad.len());
        let get = Get::default();
        let r = pinned_with(&data, &pin("2.0.0", &url, &newer), &get, &|_| {}, &|| true, &plenty, &signed);
        finish(&url, h);
        assert_eq!(r, Ok(Picked { dir: data.join("runtimes/dsh-1.0.0"), fell_back: Some(PackError::Hash) }));
        assert_eq!(left(&data), ["dsh-1.0.0", "healthy.json"], "nothing of the refused pack is left");
    }

    #[cfg(feature = "rehearsal_pack")]
    #[test]
    fn a_rehearsal_build_starts_its_placed_pack_only_when_the_manifest_is_the_pinned_one() {
        let base = tmp("placed");
        let data = base.join("data");
        let dir = data.join("runtimes/dsh-0.5.0-rehearsal.9");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST), b"{\"v\":1}").unwrap();
        let sha = leak(&hex(&Sha256::digest(b"{\"v\":1}")));
        let get = Get::default();
        let placed_pin = PackPin { version: "0.5.0-rehearsal.9", url: "", sha256: sha, size: 0 };
        assert_eq!(pinned_with(&data, &placed_pin, &get, &|_| {}, &|| true, &plenty, &signed), Ok(Picked { dir: dir.clone(), fell_back: None }));
        let other = PackPin { sha256: leak(&"0".repeat(64)), ..placed_pin };
        assert!(pinned_with(&data, &other, &get, &|_| {}, &|| true, &plenty, &signed).is_err_and(|e| e.contains("not the pinned one")));
        let absent = PackPin { version: "0.5.0-rehearsal.8", ..placed_pin };
        assert!(pinned_with(&data, &absent, &get, &|_| {}, &|| true, &plenty, &signed).is_err_and(|e| e.contains("no placed pack")));
        assert_eq!(get.0.load(Ordering::SeqCst), 0, "a placed pack is never fetched");
        assert_eq!((REHEARSAL_PACK.version, REHEARSAL_PACK.url, REHEARSAL_PACK.sha256.len()), ("0.5.0-rehearsal.4", "", 64), "the rehearsal pack's own pin is a set, placed one");
    }

    #[test]
    fn a_pack_marked_bad_is_kept_and_the_next_start_picks_the_last_healthy_one_until_a_new_pin_or_fetch() {
        let base = tmp("bad");
        let data = base.join("data");
        let (v1, v2, v3) = (good(), pack(&[E::F("app/server.js", b"v2", 0o644)]), pack(&[E::F("app/server.js", b"v3", 0o644)]));
        let put = |v: &str, b: &Vec<u8>| ensure_with(&data, &pin(v, "http://127.0.0.1/p", b), &|_, to| fs::write(to, b).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        put("1.0.0", &v1);
        mark_healthy(&data, "1.0.0").unwrap();

        put("2.0.0", &v2);
        assert!(mark_bad(&data, "1.0.0", "x").is_err_and(|e| e.contains("has run healthy")), "a pack that ran healthy is never marked");
        mark_bad(&data, "2.0.0", "DSH was not ready within 300 s").unwrap();
        let mark = bad_mark(&data, "2.0.0").unwrap();
        assert!(mark["reason"] == "DSH was not ready within 300 s" && mark["at"].as_u64().is_some_and(|t| t > 1_700_000_000), "{mark}");
        let get = Get::default();
        let p2 = pin("2.0.0", "http://127.0.0.1/p", &v2);
        let r = pinned_with(&data, &p2, &get, &|_| {}, &|| true, &plenty, &signed).unwrap();
        assert!(r.dir == data.join("runtimes/dsh-1.0.0") && matches!(&r.fell_back, Some(PackError::Bad(w)) if w.contains("marked bad (DSH was not ready")), "{r:?}");
        assert_eq!(pinned_with(&data, &p2, &get, &|_| {}, &|| true, &plenty, &signed).map(|p| p.dir), Ok(data.join("runtimes/dsh-1.0.0")), "not chosen again");
        assert!(data.join("runtimes/dsh-2.0.0").join(MANIFEST).is_file(), "kept for diagnosis");

        let p3 = pin("3.0.0", "http://127.0.0.1/p", &v3);
        let r = pinned_with(&data, &p3, &Off3(&v3), &|_| {}, &|| true, &plenty, &signed).unwrap();
        assert_eq!((r.dir, r.fell_back), (data.join("runtimes/dsh-3.0.0"), None));
        assert_eq!(get.0.load(Ordering::SeqCst), 0);

        let lone = tmp("bad-lone");
        let ldata = lone.join("data");
        ensure_with(&ldata, &p2, &|_, to| fs::write(to, &v2).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        mark_bad(&ldata, "2.0.0", "it stopped before it was ready").unwrap();
        assert!(pinned_with(&ldata, &p2, &get, &|_| {}, &|| true, &plenty, &signed).is_err_and(|e| e.contains("marked bad") && e.contains("no healthy pack")));
    }

    #[test]
    fn a_fetch_of_a_bad_pack_clears_its_mark() {
        let base = tmp("bad-fetch");
        let data = base.join("data");
        let (v1, v2) = (good(), pack(&[E::F("app/server.js", b"v2", 0o644)]));
        ensure_with(&data, &pin("1.0.0", "http://127.0.0.1/p", &v1), &|_, to| fs::write(to, &v1).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        mark_healthy(&data, "1.0.0").unwrap();
        let p2 = pin("2.0.0", "http://127.0.0.1/p", &v2);
        ensure_with(&data, &p2, &|_, to| fs::write(to, &v2).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        mark_bad(&data, "2.0.0", "DSH stopped before it was ready").unwrap();
        refetch_with(&data, &p2, &|_, to| fs::write(to, &v2).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        assert_eq!(bad_mark(&data, "2.0.0"), None);
        let get = Get::default();
        assert_eq!(pinned_with(&data, &p2, &get, &|_| {}, &|| true, &plenty, &signed), Ok(Picked { dir: data.join("runtimes/dsh-2.0.0"), fell_back: None }));
    }

    struct Count<'a>(&'a [u8], AtomicUsize);
    impl crate::dictation_models::Fetch for Count<'_> {
        fn fetch(&self, _: &str, part: &Path, _: &std::sync::atomic::AtomicBool, _: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            fs::write(part, self.0).map_err(|e| e.to_string())
        }
    }

    #[test]
    fn the_start_path_never_fetches_a_changed_pack_again_and_fetch_again_fetches_it_once() {
        let base = tmp("fetch-again");
        let data = base.join("data");
        let bytes = pack(&[E::F("app/server.js", b"v1", 0o644)]);
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let get = Count(&bytes, AtomicUsize::new(0));
        let fetched = || get.1.load(Ordering::SeqCst);
        let dir = pinned_with(&data, &p, &get, &|_| {}, &|| true, &plenty, &signed).unwrap().dir;
        assert_eq!(fetched(), 1, "the first start fetches it");
        fs::write(dir.join("app/server.js"), b"changed").unwrap();
        assert_eq!(check_manifest(&dir, &[]), Err(vec![PathBuf::from("app/server.js")]));

        for _ in 0..3 {
            assert_eq!(pinned_with(&data, &p, &get, &|_| {}, &|| true, &plenty, &signed).unwrap(), Picked { dir: dir.clone(), fell_back: None });
        }
        assert_eq!(fetched(), 1, "no fetch until Fetch again");
        assert_eq!(refetch_pinned(&data, &p, &get, &|_| {}, &|| true, &plenty, &signed), Ok(dir.clone()));
        assert_eq!(fetched(), 2, "Fetch again fetches it once");
        assert_eq!((check_manifest(&dir, &[]), fs::read(dir.join("app/server.js")).unwrap()), (Ok(()), b"v1".to_vec()), "as the pack has it");

        let placed = PackPin { url: "", ..p };
        assert!(refetch_pinned(&data, &placed, &get, &|_| {}, &|| true, &plenty, &signed).is_err_and(|e| e.contains("placed, never fetched")));
        assert_eq!(fetched(), 2);

        #[cfg(not(feature = "rehearsal_pack"))]
        assert_eq!(start_can_refetch(), !start_pin_unset(), "Fetch again follows this build's pin");
    }

    struct Off3<'a>(&'a [u8]);
    impl crate::dictation_models::Fetch for Off3<'_> {
        fn fetch(&self, _: &str, part: &Path, _: &std::sync::atomic::AtomicBool, _: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
            fs::write(part, self.0).map_err(|e| e.to_string())
        }
    }

    struct Halves<'a>(&'a [u8]);
    impl crate::dictation_models::Fetch for Halves<'_> {
        fn fetch(&self, _: &str, part: &Path, _: &std::sync::atomic::AtomicBool, progress: &dyn Fn(u64), _: &dyn Fn(u32, u32)) -> Result<(), String> {
            let half = self.0.len() / 2;
            fs::write(part, &self.0[..half]).map_err(|e| e.to_string())?;
            progress(half as u64);
            fs::OpenOptions::new().append(true).open(part).and_then(|mut f| f.write_all(&self.0[half..])).map_err(|e| e.to_string())?;
            progress(self.0.len() as u64);
            Ok(())
        }
    }

    #[test]
    fn a_fetch_tells_its_bytes_of_the_pin_s_size_then_each_step_and_a_present_pack_tells_nothing() {
        let base = tmp("steps");
        let data = base.join("data");
        let bytes = pack(&[E::F("app/server.js", b"v1", 0o644), E::F("bin/node", b"node", 0o755)]);
        let p = pin("1.0.0", "http://127.0.0.1/p", &bytes);
        let seen = Mutex::new(Vec::new());
        let step = |s: Step| seen.lock().unwrap().push(s);
        let (n, half) = (bytes.len() as u64, bytes.len() as u64 / 2);
        pinned_steps(&data, &p, &Halves(&bytes), &|_| {}, &|| true, &plenty, &signed, &step).unwrap();
        assert_eq!(*seen.lock().unwrap(), [Step::Download { done: 0, total: n }, Step::Download { done: half, total: n }, Step::Download { done: n, total: n }, Step::Verify, Step::Unpack, Step::Check]);
        seen.lock().unwrap().clear();
        pinned_steps(&data, &p, &Halves(&bytes), &|_| {}, &|| true, &plenty, &signed, &step).unwrap();
        assert!(seen.lock().unwrap().is_empty(), "the pack is there: no step");
        refetch_pinned_steps(&data, &p, &Halves(&bytes), &|_| {}, &|| true, &plenty, &signed, &step).unwrap();
        assert_eq!(seen.lock().unwrap().len(), 6, "Fetch again tells the same steps: {:?}", seen.lock().unwrap());

        seen.lock().unwrap().clear();
        let bad = PackPin { sha256: leak(&"00".repeat(32)), ..pin("2.0.0", "http://127.0.0.1/p", &bytes) };
        assert!(ensure_steps(&data, &bad, &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed, &step).is_err());
        assert_eq!(*seen.lock().unwrap(), [Step::Download { done: 0, total: n }, Step::Verify]);
    }

    #[test]
    fn the_healthy_prune_never_removes_a_bad_pack() {
        let base = tmp("bad-prune");
        let data = base.join("data");
        let put = |v: &str, body: &[u8]| {
            let bytes = pack(&[E::F("app/server.js", body, 0o644)]);
            ensure_with(&data, &pin(v, "http://127.0.0.1/p", &bytes), &|_, to| fs::write(to, &bytes).map_err(|e| e.to_string()), &plenty, &signed).unwrap();
        };
        put("1.0.0", b"v1");
        mark_healthy(&data, "1.0.0").unwrap();
        put("2.0.0", b"v2");
        put("3.0.0", b"v3");
        mark_bad(&data, "2.0.0", "it stopped 5 times within 10 minutes").unwrap();
        mark_healthy(&data, "3.0.0").unwrap();
        assert_eq!(left(&data), ["bad.json", "dsh-2.0.0", "dsh-3.0.0", "healthy.json"], "1.0.0 pruned, the bad 2.0.0 kept");
        assert!(bad_mark(&data, "2.0.0").is_some());
    }
}
