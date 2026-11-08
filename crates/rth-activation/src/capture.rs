

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Root {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sums {
    pub files: Vec<(usize, u64, String)>,
    pub links: usize,
    pub digest: String,
    #[serde(skip)]
    pub names: Vec<String>,
}

fn strict(p: &Path) -> io::Result<Vec<u8>> {
    crate::record::read_bytes_strict(p)?.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{} vanished during the walk", p.display())))
}

pub fn sums(root: &Path) -> io::Result<Sums> {
    let mut entries: Vec<(String, Option<(u64, String)>)> = Vec::new();
    fn walk(base: &Path, d: &Path, out: &mut Vec<(String, Option<(u64, String)>)>) -> io::Result<()> {
        let mut names: Vec<_> = std::fs::read_dir(d)?.collect::<Result<Vec<_>, _>>()?;
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let m = std::fs::symlink_metadata(&p)?;
            if m.file_type().is_symlink() {
                out.push((rel, None));
            } else if m.is_dir() {
                walk(base, &p, out)?;
            } else if m.is_file() {
                let bytes = strict(&p)?;
                out.push((rel, Some((m.len(), hex(&Sha256::digest(&bytes))))));
            }
        }
        Ok(())
    }
    let m = std::fs::symlink_metadata(root)?;
    if m.is_file() {
        let bytes = strict(root)?;
        entries.push((String::new(), Some((m.len(), hex(&Sha256::digest(&bytes))))));
    } else {
        walk(root, root, &mut entries)?;
    }
    let mut h = Sha256::new();
    let mut files = Vec::new();
    let mut links = 0;
    for (i, (name, f)) in entries.iter().enumerate() {
        match f {
            Some((size, sha)) => {
                h.update(format!("f {name} {size} {sha}\n"));
                files.push((i, *size, sha.clone()));
            }
            None => {

                let to = std::fs::read_link(root.join(name))?.to_string_lossy().into_owned();
                h.update(format!("l {name} -> {to}\n"));
                links += 1;
            }
        }
    }
    Ok(Sums { files, links, digest: hex(&h.finalize()), names: entries.into_iter().map(|(n, _)| n).collect() })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Moved {

    Absent { name: String },
    Moved { name: String, from: PathBuf, to: PathBuf, digest: String },
}

#[derive(Debug)]
pub enum CaptureError {

    Occupied(PathBuf),

    CrossDevice(PathBuf),

    Mismatch { name: String, rolled_back: Vec<String> },
    Io(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::Occupied(p) => write!(f, "{} is occupied: it is never overwritten", p.display()),
            CaptureError::CrossDevice(p) => write!(f, "{} is on another filesystem: capture refuses (no copy fallback)", p.display()),
            CaptureError::Mismatch { name, rolled_back } => write!(f, "{name} changed while it was captured; moved back: {rolled_back:?}"),
            CaptureError::Io(e) => f.write_str(e),
        }
    }
}

fn io_err(what: &str, p: &Path, e: io::Error) -> CaptureError {
    CaptureError::Io(format!("{what} {}: {e}", p.display()))
}

pub fn make_backup_dir(dest: &Path) -> Result<(), CaptureError> {
    if std::fs::symlink_metadata(dest).is_ok() {
        return Err(CaptureError::Occupied(dest.to_path_buf()));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err("create", parent, e))?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(|e| io_err("chmod", parent, e))?;
    }
    std::fs::create_dir(dest).map_err(|e| io_err("create", dest, e))?;
    std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o700)).map_err(|e| io_err("chmod", dest, e))?;
    Ok(())
}

pub fn capture(roots: &[Root], dest: &Path, checkpoint: &mut dyn FnMut(&Moved) -> io::Result<()>) -> Result<Vec<Moved>, CaptureError> {
    let dest_dev = crate::activation::real_dir(dest).map_err(|e| io_err("inspect", dest, e))?.dev();
    let mut done: Vec<Moved> = Vec::new();
    let mut names_file = Vec::new();
    for r in roots {
        let target = dest.join(&r.name);
        if std::fs::symlink_metadata(&target).is_ok() {
            return Err(CaptureError::Occupied(target));
        }
        let m = match std::fs::symlink_metadata(&r.path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let a = Moved::Absent { name: r.name.clone() };
                checkpoint(&a).map_err(|e| io_err("record", dest, e))?;
                done.push(a);
                continue;
            }
            Err(e) => return Err(io_err("inspect", &r.path, e)),
        };
        if m.file_type().is_symlink() {
            return Err(CaptureError::Io(format!("{} is a symlink: the set's roots are real files and directories", r.path.display())));
        }
        if m.dev() != dest_dev {
            return Err(CaptureError::CrossDevice(r.path.clone()));
        }
        let before = sums(&r.path).map_err(|e| io_err("hash", &r.path, e))?;
        if let Err(e) = std::fs::rename(&r.path, &target) {
            if e.raw_os_error() == Some(libc::EXDEV) {
                return Err(CaptureError::CrossDevice(r.path.clone()));
            }
            return Err(io_err("move", &r.path, e));
        }
        let after = sums(&target).map_err(|e| io_err("hash", &target, e))?;
        let mv = Moved::Moved { name: r.name.clone(), from: r.path.clone(), to: target.clone(), digest: after.digest.clone() };
        done.push(mv.clone());
        if after.digest != before.digest {
            let rolled_back = restore_moved(&done);
            return Err(CaptureError::Mismatch { name: r.name.clone(), rolled_back });
        }
        checkpoint(&mv).map_err(|e| io_err("record", dest, e))?;
        names_file.push(serde_json::json!({"root": r.name, "names": before.names}));
    }

    let p = dest.join(".names.json");
    std::fs::write(&p, serde_json::to_vec_pretty(&names_file).unwrap()).map_err(|e| io_err("write", &p, e))?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).map_err(|e| io_err("chmod", &p, e))?;
    Ok(done)
}

fn restore_moved(done: &[Moved]) -> Vec<String> {
    let mut back = Vec::new();
    for m in done.iter().rev() {
        if let Moved::Moved { name, from, to, .. } = m
            && std::fs::symlink_metadata(from).is_err()
            && std::fs::rename(to, from).is_ok()
        {
            back.push(name.clone());
        }
    }
    back
}

pub fn restore(moved: &[Moved], checkpoint: &mut dyn FnMut(&str) -> io::Result<()>) -> Result<(), CaptureError> {
    for m in moved {
        if let Moved::Moved { from, to, digest, name } = m {
            if std::fs::symlink_metadata(from).is_ok() {
                return Err(CaptureError::Occupied(from.clone()));
            }
            let now = sums(to).map_err(|e| io_err("hash", to, e))?;
            if &now.digest != digest {
                return Err(CaptureError::Mismatch { name: name.clone(), rolled_back: vec![] });
            }
        }
    }
    for m in moved {
        if let Moved::Moved { from, to, name, .. } = m {
            std::fs::rename(to, from).map_err(|e| io_err("restore", to, e))?;
            checkpoint(name).map_err(|e| io_err("record", from, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, Vec<Root>) {
        let d = std::env::temp_dir().join(format!("rth-capture-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("live/agentd-openai/media")).unwrap();
        std::fs::write(d.join("live/agentd-openai/state.json"), b"{\"instance_id\":\"inst-fictional\"}").unwrap();
        std::fs::write(d.join("live/agentd-openai/journal.jsonl"), b"{\"fictional\":1}\n").unwrap();
        std::fs::create_dir_all(d.join("live/sessions/ws/rth-root")).unwrap();
        std::fs::write(d.join("live/sessions/ws/rth-root/chat.jsonl"), b"{\"text\":\"a fictional chat line\"}\n").unwrap();
        let roots = vec![
            Root { name: "agentd".into(), path: d.join("live/agentd-openai") },
            Root { name: "session".into(), path: d.join("live/sessions/ws/rth-root") },
            Root { name: "delegation".into(), path: d.join("live/rth") },
        ];
        (d, roots)
    }

    #[test]
    fn capture_moves_each_root_once_records_it_and_restore_brings_it_back() {
        let (d, roots) = fixture("ok");
        let before: Vec<_> = roots.iter().map(|r| sums(&r.path).ok().map(|s| s.digest)).collect();
        let dest = d.join("backups/op-1/test");
        make_backup_dir(&dest).unwrap();
        let mut log = Vec::new();
        let moved = capture(&roots, &dest, &mut |m| { log.push(m.clone()); Ok(()) }).unwrap();
        assert_eq!(log, moved, "every step was checkpointed, in order");
        assert!(matches!(&moved[2], Moved::Absent { name } if name == "delegation"));
        assert!(!roots[0].path.exists() && dest.join("agentd/state.json").exists());
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o700);
        let names = std::fs::read_to_string(dest.join(".names.json")).unwrap();
        assert!(names.contains("chat.jsonl"), "the private name list is in the backup only");
        let evidence = serde_json::to_string(&sums(&dest.join("session")).unwrap()).unwrap();
        assert!(!evidence.contains("chat") && !evidence.contains("fictional chat"), "evidence carries indices and sums, no names: {evidence}");
        let mut back = Vec::new();
        restore(&moved, &mut |n| { back.push(n.to_string()); Ok(()) }).unwrap();
        let after: Vec<_> = roots.iter().map(|r| sums(&r.path).ok().map(|s| s.digest)).collect();
        assert_eq!(before, after, "restored byte for byte");
        assert_eq!(back, vec!["agentd", "session"]);
    }

    #[test]
    fn an_occupied_destination_or_origin_is_never_overwritten() {
        let (d, roots) = fixture("occ");
        let dest = d.join("backups/op-2/test");
        make_backup_dir(&dest).unwrap();
        std::fs::create_dir_all(dest.join("agentd")).unwrap();
        assert!(matches!(capture(&roots, &dest, &mut |_| Ok(())), Err(CaptureError::Occupied(_))));
        assert!(roots[0].path.join("state.json").exists(), "nothing moved");
        assert!(matches!(make_backup_dir(&dest), Err(CaptureError::Occupied(_))), "a backup dir is made once");

        std::fs::remove_dir_all(dest.join("agentd")).unwrap();
        let moved = capture(&roots, &dest, &mut |_| Ok(())).unwrap();
        std::fs::create_dir_all(&roots[0].path).unwrap();
        std::fs::write(roots[0].path.join("foreign.txt"), b"someone else's").unwrap();
        assert!(matches!(restore(&moved, &mut |_| Ok(())), Err(CaptureError::Occupied(_))));
        assert_eq!(std::fs::read(roots[0].path.join("foreign.txt")).unwrap(), b"someone else's");
        assert!(dest.join("agentd/state.json").exists(), "the captured tree stays captured");
    }

    #[test]
    fn a_captured_tree_that_changed_is_not_restored() {
        let (d, roots) = fixture("chg");
        let dest = d.join("backups/op-3/test");
        make_backup_dir(&dest).unwrap();
        let moved = capture(&roots, &dest, &mut |_| Ok(())).unwrap();
        std::fs::write(dest.join("agentd/journal.jsonl"), b"tampered\n").unwrap();
        assert!(matches!(restore(&moved, &mut |_| Ok(())), Err(CaptureError::Mismatch { .. })));
        assert!(!roots[0].path.exists(), "nothing was restored");
    }

    #[test]
    fn a_root_on_another_filesystem_refuses_without_a_copy() {

        let (d, roots) = fixture("xdev");
        let dest = d.join("backups/op-4/test");
        make_backup_dir(&dest).unwrap();
        let foreign = vec![Root { name: "devnull".into(), path: PathBuf::from("/dev/null") }];
        assert!(matches!(capture(&foreign, &dest, &mut |_| Ok(())), Err(CaptureError::CrossDevice(_))));
        assert!(Path::new("/dev/null").exists() && roots[0].path.exists());
    }
}
