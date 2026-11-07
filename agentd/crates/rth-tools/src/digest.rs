// The tree digest incorporates MIT-licensed OpenAPPA code; see NOTICE.


use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

pub const MAX_ENTRIES: usize = 4096;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
const PREFIX: &str = "sha256:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DigestError {
    Read(PathBuf, String),
    UnportablePath(PathBuf),
    UnsupportedEntry(PathBuf),
    Oversized(PathBuf, String),
}

pub fn spell(bytes: &[u8; 32]) -> String {
    let mut s = String::from(PREFIX);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn is_spelling(text: &str) -> bool {
    text.strip_prefix(PREFIX).is_some_and(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

fn absorb_field(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

pub fn tree(root: &Path) -> Result<String, DigestError> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    let mut total = 0u64;
    for (_, is_file, abs) in &entries {
        if *is_file {
            total = total.saturating_add(fs::metadata(abs).map_err(|e| DigestError::Read(abs.clone(), e.to_string()))?.len());
            if total > MAX_BYTES {
                return Err(DigestError::Oversized(root.to_path_buf(), format!("more than {MAX_BYTES} bytes")));
            }
        }
    }
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut h = Sha256::new();
    for (portable, is_file, abs) in &entries {
        absorb_field(&mut h, portable.as_bytes());
        h.update([if *is_file { b'f' } else { b'd' }]);
        if *is_file {
            absorb_file(&mut h, abs)?;
        } else {
            absorb_field(&mut h, &[]);
        }
    }
    Ok(spell(&h.finalize().into()))
}

pub fn json(v: &serde_json::Value) -> String {
    let mut h = Sha256::new();
    absorb_field(&mut h, serde_json::to_string(v).expect("JSON serializes").as_bytes());
    spell(&h.finalize().into())
}

fn portable(relative: &Path) -> Result<String, DigestError> {
    let mut parts = Vec::new();
    for c in relative.components() {
        match c {
            Component::Normal(p) => {
                let p = p.to_str().ok_or_else(|| DigestError::UnportablePath(relative.to_path_buf()))?;
                if p.contains('\\') {
                    return Err(DigestError::UnportablePath(relative.to_path_buf()));
                }
                parts.push(p);
            }
            _ => return Err(DigestError::UnportablePath(relative.to_path_buf())),
        }
    }
    Ok(parts.join("/"))
}

fn absorb_file(h: &mut Sha256, path: &Path) -> Result<(), DigestError> {
    let read = |e: std::io::Error| DigestError::Read(path.to_path_buf(), e.to_string());
    let mut f = fs::File::open(path).map_err(read)?;
    let len = f.metadata().map_err(read)?.len();
    h.update(len.to_be_bytes());
    let mut left = len;
    let mut buf = [0u8; 64 * 1024];
    while left > 0 {
        let want = left.min(buf.len() as u64) as usize;
        let n = f.read(&mut buf[..want]).map_err(read)?;
        if n == 0 {
            return Err(DigestError::Oversized(path.to_path_buf(), "it changed size while being read".into()));
        }
        h.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(())
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, bool, PathBuf)>) -> Result<(), DigestError> {
    let read = |e: std::io::Error| DigestError::Read(dir.to_path_buf(), e.to_string());
    for e in fs::read_dir(dir).map_err(read)? {
        let e = e.map_err(read)?;
        if out.len() >= MAX_ENTRIES {
            return Err(DigestError::Oversized(root.to_path_buf(), format!("more than {MAX_ENTRIES} entries")));
        }
        let abs = e.path();
        let rel = abs.strip_prefix(root).map_err(|_| DigestError::UnportablePath(abs.clone()))?;
        let p = portable(rel)?;
        let kind = e.file_type().map_err(|err| DigestError::Read(abs.clone(), err.to_string()))?;
        if kind.is_dir() {
            out.push((p, false, abs.clone()));
            collect(root, &abs, out)?;
        } else if kind.is_file() {
            out.push((p, true, abs));
        } else {
            return Err(DigestError::UnsupportedEntry(abs));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TempDir;

    #[test]
    fn the_fixture_tree_gives_openappas_pinned_digest() {
        let d = TempDir::new("digest-pin");
        fs::write(d.path().join("a.txt"), "alpha\n").unwrap();
        fs::create_dir(d.path().join("dir")).unwrap();
        fs::write(d.path().join("dir/b.bin"), [0u8, 1, 2, 255]).unwrap();
        fs::write(d.path().join("z"), "").unwrap();
        assert_eq!(tree(d.path()).unwrap(), "sha256:2619322b5feb343f951e2ed55ab86948a93d9460eea0fc70d46a20fdb9f87dec");
    }

    #[test]
    fn order_does_not_matter_but_any_byte_or_name_does_and_symlinks_are_refused() {
        let (a, b) = (TempDir::new("digest-a"), TempDir::new("digest-b"));
        fs::write(a.path().join("x"), "one").unwrap();
        fs::write(a.path().join("y"), "two").unwrap();
        fs::write(b.path().join("y"), "two").unwrap();
        fs::write(b.path().join("x"), "one").unwrap();
        assert_eq!(tree(a.path()).unwrap(), tree(b.path()).unwrap());
        fs::write(b.path().join("x"), "One").unwrap();
        assert_ne!(tree(a.path()).unwrap(), tree(b.path()).unwrap());
        let c = TempDir::new("digest-c");
        fs::write(c.path().join("ab"), "c").unwrap();
        let e = TempDir::new("digest-e");
        fs::write(e.path().join("a"), "bc").unwrap();
        assert_ne!(tree(c.path()).unwrap(), tree(e.path()).unwrap(), "a path is separated from the bytes beside it");
        std::os::unix::fs::symlink(a.path().join("x"), c.path().join("link")).unwrap();
        assert!(matches!(tree(c.path()), Err(DigestError::UnsupportedEntry(_))));
        assert!(is_spelling(&tree(a.path()).unwrap()) && !is_spelling("sha256:XYZ"));
        assert_ne!(json(&serde_json::json!({"a": 1})), json(&serde_json::json!({"a": 2})));
        assert_eq!(json(&serde_json::json!({"a": 1, "b": 2})), json(&serde_json::json!({"b": 2, "a": 1})), "canonical key order");
    }
}
