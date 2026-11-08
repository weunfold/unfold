

use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

#[derive(Debug)]
pub enum RecordError {
    Io(io::Error),

    Unreadable(String),

    Seq { expected: u64, found: u64 },
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordError::Io(e) => write!(f, "{e}"),
            RecordError::Unreadable(why) => write!(f, "unreadable record: {why}"),
            RecordError::Seq { expected, found } => write!(f, "record seq {found}, expected {expected}"),
        }
    }
}

fn fsync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

pub fn write(path: &Path, value: &Value, mode: u32) -> io::Result<()> {
    if value.get("seq").and_then(Value::as_u64).is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a record needs a numeric seq"));
    }
    let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "record path has no directory"))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("record");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut f = OpenOptions::new().write(true).create_new(true).mode(mode).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
        f.write_all(format!("{value:#}\n").as_bytes())?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    fsync_dir(dir)
}

pub fn read_bytes_strict(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io::Error::new(e.kind(), format!("{} cannot be inspected: {e}", path.display()))),
        Ok(m) if !m.file_type().is_file() => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not a regular file", path.display()))),
        Ok(_) => {}
    }
    let mut f = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    if !f.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not a regular file", path.display())));
    }
    let mut b = Vec::new();
    io::Read::read_to_end(&mut f, &mut b).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    Ok(Some(b))
}

pub fn read(path: &Path) -> Result<Option<Value>, RecordError> {
    let bytes = match read_bytes_strict(path) {
        Ok(Some(b)) => b,
        Ok(None) => return Ok(None),
        Err(e) => return Err(RecordError::Io(e)),
    };
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| RecordError::Unreadable(format!("{}: {e}", path.display())))?;
    if !v.is_object() || v.get("seq").and_then(Value::as_u64).is_none() {
        return Err(RecordError::Unreadable(format!("{}: not an object with a seq", path.display())));
    }
    Ok(Some(v))
}

pub fn read_expecting(path: &Path, expected: u64) -> Result<Value, RecordError> {
    let v = read(path)?.ok_or_else(|| RecordError::Unreadable(format!("{}: missing", path.display())))?;
    let found = v["seq"].as_u64().unwrap();
    if found != expected {
        return Err(RecordError::Seq { expected, found });
    }
    Ok(v)
}

pub fn advance(path: &Path, current: &Value, mut next: Value, mode: u32) -> Result<Value, RecordError> {
    let seq = current.get("seq").and_then(Value::as_u64).ok_or_else(|| RecordError::Unreadable("current record has no seq".into()))?;
    let on_disk = read_expecting(path, seq)?;
    if &on_disk != current {
        return Err(RecordError::Unreadable(format!("{}: changed underneath this writer", path.display())));
    }
    next["seq"] = Value::from(seq + 1);
    write(path, &next, mode).map_err(RecordError::Io)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rth-record-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_linked_dangling_non_regular_or_unreadable_record_is_an_error_never_absent() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("strict");
        let good = d.join("good.json");
        write(&good, &json!({"seq": 3, "activation_state": "interim_active"}), 0o600).unwrap();
        let link = d.join("activation.json");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        assert!(read(&link).is_err(), "a link to a valid record is not followed");
        let dangling = d.join("rth.op.json");
        std::os::unix::fs::symlink(d.join("nowhere"), &dangling).unwrap();
        assert!(read(&dangling).is_err(), "a dangling link is not absent");
        let dirent = d.join("dir.json");
        std::fs::create_dir(&dirent).unwrap();
        assert!(read(&dirent).is_err());
        let locked = d.join("locked.json");
        write(&locked, &json!({"seq": 0}), 0o600).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(read(&locked).is_err());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_expecting(&link, 3).is_err() && advance(&link, &json!({"seq": 3, "activation_state": "interim_active"}), json!({}), 0o600).is_err());
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "nothing modified");
        assert!(read(&d.join("absent.json")).unwrap().is_none(), "only ENOENT is absent");
        assert_eq!(read(&good).unwrap().unwrap()["seq"], 3);
    }

    #[test]
    fn versions_advance_atomically_and_a_lost_update_refuses() {
        let p = dir("adv").join("rth.op.json");
        assert!(read(&p).unwrap().is_none());
        write(&p, &json!({"seq": 0, "state": "a"}), 0o600).unwrap();
        let v0 = read_expecting(&p, 0).unwrap();
        let v1 = advance(&p, &v0, json!({"state": "b"}), 0o600).unwrap();
        assert_eq!(v1["seq"], 1);
        assert!(matches!(advance(&p, &v0, json!({"state": "c"}), 0o600), Err(RecordError::Seq { expected: 0, found: 1 })), "a writer holding a stale version refuses");
        assert_eq!(read_expecting(&p, 1).unwrap()["state"], "b");
        assert!(std::fs::read_dir(p.parent().unwrap()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".tmp-")), "no temporary left behind");
    }

    #[test]
    fn a_torn_or_seqless_record_is_unreadable_never_absent() {
        let p = dir("torn").join("activation.json");
        std::fs::write(&p, b"{\"seq\": 3, \"state\"").unwrap();
        assert!(matches!(read(&p), Err(RecordError::Unreadable(_))));
        std::fs::write(&p, b"{\"state\": \"x\"}").unwrap();
        assert!(matches!(read(&p), Err(RecordError::Unreadable(_))));
        assert!(write(&p, &json!({"state": "x"}), 0o600).is_err(), "a record is never written without a seq");
    }
}

pub fn read_json_strict(path: &Path) -> io::Result<Option<Value>> {
    match read_bytes_strict(path)? {
        None => Ok(None),
        Some(b) => serde_json::from_slice(&b).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display()))),
    }
}
