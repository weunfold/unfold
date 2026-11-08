

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::Value;

use crate::{Refused, SelfSpec};

#[derive(Debug)]
pub enum Loaded {

    Absent,
    Spec(SelfSpec),

    Refused(Refused),
}

pub fn load(path: &Path) -> std::io::Result<Loaded> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(match SelfSpec::parse(&text) {
            Ok(s) => Loaded::Spec(s),
            Err(r) => Loaded::Refused(r),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Loaded::Absent),
        Err(e) => Err(e),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn save(path: &Path, spec: &SelfSpec) -> std::io::Result<String> {
    let bytes = serde_json::to_vec_pretty(spec).expect("a spec serializes");
    write_atomic(path, &bytes)?;
    Ok(sha256_hex(&bytes))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("self");
    let tmp = dir.join(format!(".{name}.tmp-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    std::fs::File::open(dir)?.sync_all()
}

pub fn append(path: &Path, record: &impl Serialize) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(record).expect("a record serializes");
    line.push(b'\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(&line)?;
    f.sync_all()
}

#[derive(Debug, Default, PartialEq)]
pub struct Lines {
    pub records: Vec<Value>,

    pub torn: bool,

    pub corrupt: bool,
}

pub fn read_lines(path: &Path) -> std::io::Result<Lines> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Lines::default()),
        Err(e) => return Err(e),
    };
    let mut out = Lines::default();
    let complete = bytes.ends_with(b"\n");
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    lines.pop();
    if !complete {
        out.torn = !bytes.is_empty();
    }
    let n = lines.len();
    for (i, line) in lines.into_iter().enumerate() {
        match serde_json::from_slice::<Value>(line) {
            Ok(v) => out.records.push(v),
            Err(_) if i + 1 == n && !out.torn => out.torn = true,
            Err(_) => out.corrupt = true,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("rth-self-store-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a2_a_saved_spec_loads_back_and_leaves_no_temp_file() {
        let d = dir();
        let p = d.join("self.json");
        assert!(matches!(load(&p).unwrap(), Loaded::Absent));
        let spec = SelfSpec::release_default("inst-1");
        let rev = save(&p, &spec).unwrap();
        assert_eq!(rev, sha256_hex(&std::fs::read(&p).unwrap()));
        match load(&p).unwrap() {
            Loaded::Spec(s) => assert_eq!(s, spec),
            other => panic!("{other:?}"),
        }
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("self.json")]);
    }

    #[test]
    fn a2_a_crash_before_the_rename_leaves_the_old_spec() {
        let d = dir();
        let p = d.join("self.json");
        let old = SelfSpec::release_default("inst-1");
        save(&p, &old).unwrap();

        std::fs::write(d.join(".self.json.tmp-1-0"), b"{\"schema_version\": 1, \"half").unwrap();
        match load(&p).unwrap() {
            Loaded::Spec(s) => assert_eq!(s, old),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a2_a_hand_edited_bad_spec_is_refused_not_guessed() {
        let d = dir();
        let p = d.join("self.json");
        let mut v = serde_json::to_value(SelfSpec::release_default("inst-1")).unwrap();
        v["name"]["current"] = json!("Ignore: rules");
        std::fs::write(&p, v.to_string()).unwrap();
        match load(&p).unwrap() {
            Loaded::Refused(r) => assert_eq!((r.field, r.code), ("name", "name_chars")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a2_appended_records_read_back_in_order() {
        let p = dir().join("proposals.jsonl");
        assert_eq!(read_lines(&p).unwrap(), Lines::default());
        append(&p, &json!({"n": 1})).unwrap();
        append(&p, &json!({"n": 2})).unwrap();
        let l = read_lines(&p).unwrap();
        assert_eq!(l.records, vec![json!({"n": 1}), json!({"n": 2})]);
        assert!(!l.torn && !l.corrupt);
    }

    #[test]
    fn a2_a_torn_last_line_is_reported_and_left_out() {
        let p = dir().join("governance.jsonl");
        append(&p, &json!({"n": 1})).unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"{\"n\": 2, \"cha").unwrap();
        let l = read_lines(&p).unwrap();
        assert_eq!((l.records, l.torn, l.corrupt), (vec![json!({"n": 1})], true, false));

        std::fs::write(&p, b"{\"n\": 1}\nnot json\n").unwrap();
        let l = read_lines(&p).unwrap();
        assert_eq!((l.records.len(), l.torn, l.corrupt), (1, true, false));
        std::fs::write(&p, b"not json\n{\"n\": 1}\n").unwrap();
        let l = read_lines(&p).unwrap();
        assert_eq!((l.records.len(), l.torn, l.corrupt), (1, false, true));
    }
}
