

use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    sha256_prefix(path, u64::MAX).map(|(h, _)| h)
}

pub fn sha256_prefix(path: &Path, limit: u64) -> std::io::Result<(String, u64)> {
    let f = std::fs::File::open(path)?;
    let mut r = f.take(limit);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut n_total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        n_total += n as u64;
        h.update(&buf[..n]);
    }
    Ok((hex(&h.finalize()), n_total))
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = dir.join(format!(".{name}.tmp-{}-{nanos}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    fsync_dir(dir)
}

pub fn append_synced(path: &Path, line: &str) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    if !line.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    f.sync_data()
}

pub fn utc_ms() -> u64 {
    rth_protocol::utc_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_prefix_hash_is_bounded() {
        let d = std::env::temp_dir().join(format!("rth-durable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two!").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two!");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file left behind");
        let (h, n) = sha256_prefix(&p, 3).unwrap();
        assert_eq!((h, n), (sha256_hex(b"two"), 3));
        assert!(is_sha256_hex(&sha256_hex(b"")) && !is_sha256_hex("ABC"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
