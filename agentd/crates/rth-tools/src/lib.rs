

pub mod authority;
pub mod confine;
pub mod digest;
pub mod files;
pub mod grants;
pub mod host;
pub mod parts;
pub mod profile;
pub mod receipt;
pub mod web;
pub mod worker;

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn canonical_sha256(v: &serde_json::Value) -> String {
    sha256_hex(serde_json::to_string(v).expect("JSON value serializes").as_bytes())
}

pub fn visible(text: &str) -> String {
    let hidden = |c: char| {
        c.is_control()
            || matches!(c as u32,
                0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180E | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F
                | 0x3164 | 0xFE00..=0xFE0F | 0xFEFF | 0xFFA0 | 0xFFF9..=0xFFFB | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
    };
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) {
            out.push_str(&format!("U+{:04X}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

pub fn utc_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod visible_tests {
    #[test]
    fn invisible_and_control_characters_are_shown_as_code_points() {
        assert_eq!(super::visible("plain text, café, 日本"), "plain text, café, 日本");
        assert_eq!(super::visible("evil\u{202E}txt.exe"), "evilU+202Etxt.exe");
        assert_eq!(super::visible("a\u{200B}b\u{2066}c\u{FEFF}d\u{00AD}e\u{E0041}f"), "aU+200BbU+2066cU+FEFFdU+00ADeU+E0041f");
        assert_eq!(super::visible("line\nbreak\u{7}\u{85}"), "lineU+000AbreakU+0007U+0085");
    }
}

#[cfg(test)]
pub(crate) mod testdir {
    use std::path::{Path, PathBuf};

    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> TempDir {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("rth-tools-{tag}-{}-{n}-{}", std::process::id(), crate::utc_ms()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(std::fs::canonicalize(&p).unwrap())
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn write(&self, rel: &str, text: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
