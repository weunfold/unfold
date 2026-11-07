

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::durable::{sha256_file, sha256_hex};

pub const IMPORT_ROOTS: &[&str] = &["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {

    pub kind: String,
    pub path: String,

    pub digest: String,
    pub files: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub note: String,
}

pub fn denied(rel: &Path) -> Option<&'static str> {
    for c in rel.components() {
        let Component::Normal(name) = c else { return Some("not a plain relative path") };
        let name = name.to_string_lossy().to_ascii_lowercase();
        match name.as_str() {
            "target" => return Some("build output"),
            "node_modules" | "vendor" => return Some("dependency tree"),
            "brain" => return Some("Brain state"),
            "bench" | "fixtures" | "private" | "audit" => return Some("benchmark/promotion/audit fixtures or results"),
            ".git" => return Some("nested repository"),
            ".gitattributes" | ".gitmodules" => return Some("git metadata the controller does not take from a work tree"),
            _ => {}
        }
        if name.starts_with(".env") || name.contains("secret") || name.contains("credential") || name == "one_auth" {
            return Some("secret material");
        }
    }
    let ext = rel.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "wav" | "pcm" | "raw" | "flac" | "mp3" | "ogg" | "opus" | "m4a" => Some("raw media"),
        "pem" | "key" | "p12" | "pfx" | "keychain" => Some("key material"),
        _ => None,
    }
}

pub fn collect(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    Ok(collect_reporting(root)?.0)
}

pub fn collect_reporting(root: &Path) -> Result<(Vec<(String, PathBuf)>, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for top in IMPORT_ROOTS {
        let abs = root.join(top);
        if abs.exists() {
            walk(root, &abs, &mut out, &mut skipped)?;
        }
    }
    out.sort();
    skipped.sort();
    Ok((out, skipped))
}

fn walk(root: &Path, p: &Path, out: &mut Vec<(String, PathBuf)>, skipped: &mut Vec<(String, String)>) -> Result<()> {
    let rel = p.strip_prefix(root).expect("under root").to_path_buf();
    if let Some(why) = denied(&rel) {
        skipped.push((rel.to_string_lossy().replace('\\', "/"), why.to_string()));
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(p).with_context(|| format!("stat {}", p.display()))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in std::fs::read_dir(p)? {
            walk(root, &entry?.path(), out, skipped)?;
        }
    } else if meta.is_file() {
        out.push((rel.to_string_lossy().replace('\\', "/"), p.to_path_buf()));
    }
    Ok(())
}

pub fn digest_files(files: &[(String, PathBuf)]) -> Result<String> {
    let mut lines = String::new();
    for (rel, abs) in files {
        let h = sha256_file(abs).with_context(|| format!("hashing {}", abs.display()))?;
        lines.push_str(&format!("{rel}\0{h}\n"));
    }
    Ok(sha256_hex(lines.as_bytes()))
}

pub fn workspace_identity(root: &Path) -> Result<SourceIdentity> {
    let files = collect(root)?;
    Ok(SourceIdentity {
        kind: "unversioned-workspace".into(),
        path: root.display().to_string(),
        digest: digest_files(&files)?,
        files: files.len() as u64,
        commit: None,
        note: "digest of the allowlisted sources at publish time; not proof that these exact sources produced the binary".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_rules_cover_secrets_media_brain_fixtures_deps_and_builds() {
        for p in [
            "target/release/x",
            "crates/a/target/x",
            "plugins/node_modules/x.js",
            "brain/_index.md",
            "bench/results/r/scorecard.json",
            "crates/x/fixtures/case.json",
            ".env",
            "crates/x/.env.local",
            "crates/x/my_secret.txt",
            "crates/x/q.wav",
            "crates/x/mic.pcm",
            "crates/x/server.pem",
            "../escape.rs",
        ] {
            assert!(denied(Path::new(p)).is_some(), "{p} should be denied");
        }
        for p in ["Cargo.toml", "crates/rth-agentd/src/main.rs", "crates/rth-bench/src/fixture.rs"] {
            assert!(denied(Path::new(p)).is_none(), "{p} should be allowed");
        }
    }

    #[test]
    fn workspace_digest_is_order_independent_and_content_sensitive() {
        let d = std::env::temp_dir().join(format!("rth-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("crates/a/src")).unwrap();
        std::fs::create_dir_all(d.join("crates/a/target")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[workspace]").unwrap();
        std::fs::write(d.join("crates/a/src/lib.rs"), "fn a() {}").unwrap();
        std::fs::write(d.join("crates/a/target/junk"), "x").unwrap();
        std::fs::write(d.join("crates/a/src/clip.wav"), "RIFF").unwrap();
        let one = workspace_identity(&d).unwrap();
        assert_eq!(one.files, 2, "target/ and .wav are excluded");
        assert_eq!(workspace_identity(&d).unwrap().digest, one.digest);
        std::fs::write(d.join("crates/a/src/lib.rs"), "fn b() {}").unwrap();
        assert_ne!(workspace_identity(&d).unwrap().digest, one.digest);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
