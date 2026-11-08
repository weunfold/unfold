

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::durable::{sha256_file, sha256_hex, utc_ms, write_atomic};
use crate::record::RepoSnapshot;
use crate::source::{IMPORT_ROOTS, SourceIdentity, collect, collect_reporting, denied, digest_files};

pub const PIN_FILE: &str = "selfrepo.json";
pub const GUEST_DIR: &str = "/rth/self-repo";
const GITIGNORE: &str = "# Never staged into the self-repo (controller deny rules mirror this).\n\
target/\nnode_modules/\nvendor/\nbrain/\nbench/\nfixtures/\nprivate/\naudit/\n.env*\n*secret*\n*credential*\n\
*.wav\n*.pcm\n*.raw\n*.flac\n*.mp3\n*.ogg\n*.opus\n*.m4a\n*.pem\n*.key\n*.p12\n*.pfx\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPin {
    pub git_path: String,
    pub git_version: String,

    pub repo_dir: String,

    pub git_dir: String,
    pub guest_dir: String,
    pub initial_commit: String,
    pub initial_source: SourceIdentity,
    pub created_utc_ms: u64,
}

pub struct Git {
    path: PathBuf,
    version: String,
}

impl Git {

    pub fn locate() -> Result<Git> {
        let path = std::env::var_os("RTH_GIT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let out = Command::new(&path).arg("--version").env_clear().env("PATH", "/usr/bin:/bin").output().with_context(|| format!("running {}", path.display()))?;
        if !out.status.success() {
            bail!("{} --version failed", path.display());
        }
        Ok(Git { path, version: String::from_utf8_lossy(&out.stdout).trim().to_string() })
    }

    pub fn pinned(pin: &RepoPin) -> Result<Git> {
        let git = Git::locate()?;
        if git.path.display().to_string() != pin.git_path || git.version != pin.git_version {
            bail!("git is {} {:?}, the self-repo is pinned to {} {:?}", git.path.display(), git.version, pin.git_path, pin.git_version);
        }
        Ok(git)
    }

    fn cmd(&self, r: &Repo) -> Command {
        let mut c = Command::new(&self.path);
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &r.git_dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PAGER", "cat")
            .env("GIT_EDITOR", "false")
            .env("GIT_ASKPASS", "false")
            .env("GIT_SSH_COMMAND", "false")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_CEILING_DIRECTORIES", "/")
            .env("LC_ALL", "C")
            .current_dir(&r.git_dir)
            .arg(format!("--git-dir={}", r.git_dir.display()))
            .arg(format!("--work-tree={}", r.work_tree.display()))
            .args(["-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false"])
            .args(["-c", "core.attributesFile=/dev/null", "-c", "core.pager=cat", "-c", "core.editor=false", "-c", "core.sshCommand=false"])
            .args(["-c", "credential.helper=", "-c", "diff.external=", "-c", "gpg.program=false", "-c", "commit.gpgSign=false"])
            .args(["-c", "submodule.recurse=false", "-c", "protocol.allow=never", "-c", "core.symlinks=true", "-c", "core.autocrlf=false"])
            .args(["-c", "user.name=rth-controller", "-c", "user.email=controller@rth.invalid", "-c", "init.defaultBranch=main"]);
        c
    }

    fn run(&self, r: &Repo, args: &[&str]) -> Result<Vec<u8>> {
        let out = self.cmd(r).args(args).output().with_context(|| format!("git {args:?}"))?;
        if !out.status.success() {
            bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(out.stdout)
    }

    fn run_s(&self, r: &Repo, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run(r, args)?).trim().to_string())
    }
}

pub struct Repo {
    pub git_dir: PathBuf,
    pub work_tree: PathBuf,
}

impl RepoPin {
    pub fn repo(&self) -> Repo {
        Repo { git_dir: PathBuf::from(&self.git_dir), work_tree: PathBuf::from(&self.repo_dir) }
    }
}

const TRUSTED_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

fn reset_config(r: &Repo) -> Result<()> {
    write_atomic(&r.git_dir.join("config"), TRUSTED_CONFIG.as_bytes())?;

    let info = r.git_dir.join("info");
    std::fs::create_dir_all(&info)?;
    write_atomic(&info.join("attributes"), b"")?;
    Ok(())
}

pub fn init(pin_dir: &Path, repo_dir: &Path, source_root: &Path) -> Result<RepoPin> {
    if repo_dir.exists() && std::fs::read_dir(repo_dir)?.next().is_some() {
        bail!("{} already exists and is not empty", repo_dir.display());
    }
    let git = Git::locate()?;
    let files = collect(source_root)?;
    let digest = digest_files(&files)?;
    std::fs::create_dir_all(repo_dir)?;
    for (rel, abs) in &files {
        let dst = repo_dir.join(rel);
        std::fs::create_dir_all(dst.parent().expect("has parent"))?;
        std::fs::copy(abs, &dst)?;
    }
    std::fs::write(repo_dir.join(".gitignore"), GITIGNORE)?;
    let git_dir = pin_dir.join("self-repo.git");
    if git_dir.exists() {
        bail!("{} already exists", git_dir.display());
    }
    std::fs::create_dir_all(&git_dir)?;
    let r = Repo { git_dir: git_dir.canonicalize()?, work_tree: repo_dir.canonicalize()? };
    git.run(&r, &["init", "-q"])?;
    reset_config(&r)?;
    let mut paths: Vec<String> = files.iter().map(|(r, _)| r.clone()).collect();
    paths.push(".gitignore".into());
    stage(&git, &r, &paths)?;
    let msg = format!(
        "rth self-repo: initial source import\n\nSource-Kind: unversioned-workspace\nSource-Path: {}\nSource-Digest: sha256:{digest}\nSource-Files: {}\n",
        source_root.display(),
        files.len()
    );
    git.run(&r, &["commit", "-q", "-m", &msg])?;
    let head = git.run_s(&r, &["rev-parse", "HEAD"])?;
    let pin = RepoPin {
        git_path: git.path.display().to_string(),
        git_version: git.version.clone(),
        repo_dir: r.work_tree.display().to_string(),
        git_dir: r.git_dir.display().to_string(),
        guest_dir: GUEST_DIR.into(),
        initial_commit: head.clone(),
        initial_source: SourceIdentity {
            kind: "unversioned-workspace".into(),
            path: source_root.display().to_string(),
            digest,
            files: files.len() as u64,
            commit: Some(head),
            note: "allowlisted import (Cargo.toml, Cargo.lock, rust-toolchain.toml, crates/), deny rules applied".into(),
        },
        created_utc_ms: utc_ms(),
    };
    std::fs::create_dir_all(pin_dir)?;
    write_atomic(&pin_dir.join(PIN_FILE), &serde_json::to_vec_pretty(&pin)?)?;
    Ok(pin)
}

pub fn load_pin(pin_dir: &Path) -> Result<Option<RepoPin>> {
    match std::fs::read(pin_dir.join(PIN_FILE)) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn stage(git: &Git, repo: &Repo, paths: &[String]) -> Result<()> {
    for p in paths {
        if let Some(why) = denied(Path::new(p)) {
            bail!("refusing to stage {p}: {why}");
        }
    }
    for chunk in paths.chunks(200) {
        let mut args = vec!["add", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git.run(repo, &args)?;
    }
    let staged = git.run_s(repo, &["diff", "--cached", "--name-only"])?;
    for p in staged.lines() {
        if let Some(why) = denied(Path::new(p)) {
            git.run(repo, &["reset", "-q", "--", p])?;
            bail!("{p} was staged but is denied ({why}); unstaged it");
        }
    }
    Ok(())
}

pub fn commit(pin: &RepoPin, paths: &[String], message: &str) -> Result<String> {
    if paths.is_empty() {
        bail!("name the paths to commit");
    }
    let git = Git::pinned(pin)?;
    let repo = pin.repo();
    reset_config(&repo)?;
    stage(&git, &repo, paths)?;
    git.run(&repo, &["commit", "-q", "-m", message])?;
    git.run_s(&repo, &["rev-parse", "HEAD"])
}

pub fn snapshot(pin: &RepoPin) -> Result<RepoSnapshot> {
    let git = Git::pinned(pin)?;
    let repo = pin.repo();
    reset_config(&repo)?;
    let head = git.run_s(&repo, &["rev-parse", "HEAD"])?;
    let status = git.run(&repo, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignore-submodules=all"])?;
    let diff = git.run(&repo, &["diff", "HEAD", "--binary", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all"])?;
    let mut dirty_paths = Vec::new();
    let mut material = status.clone();
    material.extend_from_slice(&diff);
    for entry in status.split(|b| *b == 0).filter(|e| e.len() > 3) {
        let s = String::from_utf8_lossy(entry);
        let (code, path) = s.split_at(3);
        dirty_paths.push(path.to_string());
        if code.starts_with("??") {

            let p = repo.work_tree.join(path);
            let h = match std::fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() => sha256_hex(std::fs::read_link(&p).unwrap_or_default().to_string_lossy().as_bytes()),
                Ok(m) if m.is_file() => sha256_file(&p).unwrap_or_default(),
                _ => String::new(),
            };
            material.extend_from_slice(format!("\0untracked {path} {h}").as_bytes());
        }
    }
    Ok(RepoSnapshot { head, dirty: !dirty_paths.is_empty(), dirty_paths, dirty_sha256: sha256_hex(&material) })
}

pub fn log(pin: &RepoPin) -> Result<Vec<String>> {
    let git = Git::pinned(pin)?;
    let repo = pin.repo();
    reset_config(&repo)?;
    Ok(git.run_s(&repo, &["log", "--format=%H %s", "--no-show-signature"])?.lines().map(String::from).collect())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Human,
    Agent,
    Harness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: String,
    pub model: Option<String>,
}

impl Actor {

    pub fn parse(s: &str, model: Option<String>) -> Result<Actor> {
        let (kind, name) = s.split_once('/').context("actor is kind/name, e.g. human/ana")?;
        let kind = match kind {
            "human" => ActorKind::Human,
            "agent" => ActorKind::Agent,
            "harness" => ActorKind::Harness,
            k => bail!("actor kind {k:?} is not human, agent or harness"),
        };
        Ok(Actor { kind, name: name.into(), model })
    }

    fn kind_str(&self) -> &'static str {
        match self.kind {
            ActorKind::Human => "human",
            ActorKind::Agent => "agent",
            ActorKind::Harness => "harness",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {

    pub head: Option<String>,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,

    pub skipped_denied: Vec<(String, String)>,
    pub source_digest: String,
    pub files: u64,
}

fn check_token(what: &str, s: &str, slash_ok: bool) -> Result<()> {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c.is_control() || (c == '/' && !slash_ok)) {
        bail!("{what} {s:?} must be one non-empty token{}", if slash_ok { "" } else { " without '/'" });
    }
    Ok(())
}

fn refuse_symlinks(work_tree: &Path, rel: &str, last: bool) -> Result<()> {
    let comps: Vec<&str> = rel.split('/').collect();
    let n = if last { comps.len() } else { comps.len() - 1 };
    let mut p = work_tree.to_path_buf();
    for c in &comps[..n] {
        p.push(c);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => bail!("refusing to import through {}: it is a symlink in the work tree", p.display()),
            Ok(_) => {}

            Err(_) => break,
        }
    }
    Ok(())
}

pub fn import(pin: &RepoPin, source_root: &Path, actor: &Actor, message: &str) -> Result<ImportReport> {
    check_token("actor name", &actor.name, false)?;
    if let Some(m) = &actor.model {
        check_token("actor model", m, true)?;
    }
    if message.trim().is_empty() {
        bail!("an import needs a message");
    }
    let source_path = source_root.display().to_string();
    if source_path.chars().any(char::is_control) {
        bail!("source path {source_path:?} has control characters");
    }
    let git = Git::pinned(pin)?;
    let repo = pin.repo();
    let in_roots = |p: &str| p.split('/').next().is_some_and(|c| IMPORT_ROOTS.contains(&c));

    let dirty: Vec<String> = snapshot(pin)?.dirty_paths.into_iter().filter(|p| in_roots(p)).collect();
    if !dirty.is_empty() {
        bail!("the work tree has uncommitted changes under {IMPORT_ROOTS:?}; land them with `repo commit` first: {dirty:?}");
    }
    if !git.run(&repo, &["diff", "--cached", "--name-only", "-z"])?.is_empty() {
        bail!("the index has staged changes; refusing to attribute them to this import");
    }

    let (files, skipped_denied) = collect_reporting(source_root)?;
    let source_digest = digest_files(&files)?;
    let (mut added, mut changed, mut writes) = (Vec::new(), Vec::new(), Vec::new());
    for (rel, abs) in &files {
        refuse_symlinks(&repo.work_tree, rel, true)?;
        let dst = repo.work_tree.join(rel);
        match std::fs::symlink_metadata(&dst) {
            Ok(m) if m.is_file() => {
                if sha256_file(&dst)? == sha256_file(abs)? {
                    continue;
                }
                changed.push(rel.clone());
            }
            Ok(_) => bail!("{rel} exists in the work tree and is not a regular file"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => added.push(rel.clone()),
            Err(e) => return Err(e).with_context(|| format!("stat {}", dst.display())),
        }
        writes.push((rel.clone(), abs.clone()));
    }
    let sources: HashSet<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
    let mut removed = Vec::new();
    for p in git.run(&repo, &["ls-tree", "-r", "--name-only", "--full-tree", "-z", "HEAD"])?.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let p = String::from_utf8(p.to_vec()).context("a path tracked at HEAD is not UTF-8")?;
        if in_roots(&p) && !sources.contains(p.as_str()) {
            refuse_symlinks(&repo.work_tree, &p, false)?;
            removed.push(p);
        }
    }

    for p in &removed {
        std::fs::remove_file(repo.work_tree.join(p)).with_context(|| format!("removing {p}"))?;
        let mut dir = Path::new(p).parent();
        while let Some(d) = dir.filter(|d| !d.as_os_str().is_empty()) {
            if std::fs::remove_dir(repo.work_tree.join(d)).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    for chunk in removed.chunks(200) {
        let mut args = vec!["rm", "-q", "--cached", "--"];
        args.extend(chunk.iter().map(String::as_str));
        git.run(&repo, &args)?;
    }
    for (rel, abs) in &writes {
        let dst = repo.work_tree.join(rel);
        std::fs::create_dir_all(dst.parent().expect("has parent"))?;
        std::fs::copy(abs, &dst).with_context(|| format!("copying {rel}"))?;
    }
    let paths: Vec<String> = writes.into_iter().map(|(rel, _)| rel).collect();
    stage(&git, &repo, &paths)?;

    let mut report = ImportReport { head: None, added, changed, removed, skipped_denied, source_digest, files: files.len() as u64 };
    if git.run(&repo, &["diff", "--cached", "--name-only", "-z"])?.is_empty() {
        return Ok(report);
    }
    let msg = format!(
        "{}\n\nSource-Kind: unversioned-workspace\nSource-Path: {source_path}\nSource-Digest: sha256:{}\nSource-Files: {}\n\
         Import-Added: {}\nImport-Changed: {}\nImport-Removed: {}\nActor: {}/{}\nActor-Model: {}\n",
        message.trim_end(),
        report.source_digest,
        report.files,
        report.added.len(),
        report.changed.len(),
        report.removed.len(),
        actor.kind_str(),
        actor.name,
        actor.model.as_deref().unwrap_or("none"),
    );
    git.run(&repo, &["commit", "-q", "-m", &msg])?;
    report.head = Some(git.run_s(&repo, &["rev-parse", "HEAD"])?);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_commit_and_dirty_snapshot() {
        let d = std::env::temp_dir().join(format!("rth-selfrepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let src = d.join("src");
        std::fs::create_dir_all(src.join("crates/a/src")).unwrap();
        std::fs::write(src.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(src.join("crates/a/src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(src.join("crates/a/src/take.wav"), "RIFF").unwrap();
        std::fs::write(src.join(".env"), "KEY=1").unwrap();
        let repo = d.join("repo");
        let pin = init(&d.join("pin"), &repo, &src).unwrap();
        assert_eq!(pin.initial_source.files, 2);
        assert!(!repo.join("crates/a/src/take.wav").exists() && !repo.join(".env").exists());
        let clean = snapshot(&pin).unwrap();
        assert!(!clean.dirty);
        assert_eq!(clean.head, pin.initial_commit);

        std::fs::write(repo.join("crates/a/src/lib.rs"), "pub fn a() { /* edited */ }\n").unwrap();
        let c = commit(&pin, &["crates/a/src/lib.rs".into()], "edit a").unwrap();
        std::fs::write(repo.join("crates/a/src/lib.rs"), "pub fn a() { /* dirty */ }\n").unwrap();
        std::fs::write(repo.join("notes.txt"), "untracked\n").unwrap();
        let dirty = snapshot(&pin).unwrap();
        assert_eq!(dirty.head, c);
        assert!(dirty.dirty);
        assert_eq!(dirty.dirty_paths, ["crates/a/src/lib.rs", "notes.txt"]);
        assert_eq!(snapshot(&pin).unwrap(), dirty, "snapshot is deterministic and read-only");
        std::fs::write(repo.join("secret.wav"), "x").unwrap();
        assert!(commit(&pin, &["secret.wav".into()], "nope").is_err());
        assert_eq!(log(&pin).unwrap().len(), 2);
        assert!(!Path::new(&pin.git_dir).starts_with(&pin.repo_dir), "git dir is outside the work tree");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn guest_written_git_config_and_attributes_never_execute_on_the_host() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("rth-selfrepo-untrusted-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let src = d.join("src");
        std::fs::create_dir_all(src.join("crates/a/src")).unwrap();
        std::fs::write(src.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(src.join("crates/a/src/lib.rs"), "pub fn a() {}\n").unwrap();
        let repo = d.join("repo");
        let pin = init(&d.join("pin"), &repo, &src).unwrap();

        let marker = d.join("PWNED");
        let script = d.join("payload.sh");
        std::fs::write(&script, format!("#!/bin/sh\ntouch {}\ncat\n", marker.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let plain_git = |args: &[&str]| {
            std::process::Command::new("/usr/bin/git").env_clear().env("PATH", "/usr/bin:/bin").env("HOME", &d).arg("-C").arg(&repo).args(args).output().unwrap()
        };
        assert!(plain_git(&["init", "-q"]).status.success());
        let s = script.display().to_string();
        for (k, v) in [
            ("core.fsmonitor", s.as_str()),
            ("filter.evil.clean", s.as_str()),
            ("filter.evil.smudge", s.as_str()),
            ("diff.evil.textconv", s.as_str()),
            ("core.pager", s.as_str()),
        ] {
            assert!(plain_git(&["config", k, v]).status.success());
        }
        std::fs::write(repo.join(".gitattributes"), "* filter=evil diff=evil\n").unwrap();
        std::fs::write(repo.join("crates/a/src/lib.rs"), "pub fn a() { /* changed */ }\n").unwrap();

        assert!(plain_git(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]).status.success());
        assert!(marker.exists(), "control: the payload is live when work-tree config is trusted");
        std::fs::remove_file(&marker).unwrap();

        let snap = snapshot(&pin).unwrap();
        assert!(snap.dirty && snap.dirty_paths.contains(&".gitattributes".to_string()));
        let c = commit(&pin, &["crates/a/src/lib.rs".into()], "edit under hostile attributes").unwrap();
        assert_eq!(snapshot(&pin).unwrap().head, c);
        log(&pin).unwrap();
        assert!(!marker.exists(), "snapshot/commit/log executed a guest-defined helper on the host");

        assert!(commit(&pin, &[".gitattributes".into()], "nope").is_err());

        std::fs::write(src.join("crates/a/src/new.rs"), "pub fn n() {}\n").unwrap();
        let r = import(&pin, &src, &sample_actor(), "import under hostile attributes").unwrap();
        assert_eq!((r.added, r.changed), (vec!["crates/a/src/new.rs".to_string()], vec!["crates/a/src/lib.rs".to_string()]));
        assert!(!marker.exists(), "import executed a guest-defined helper on the host");
        assert!(repo.join(".git").is_dir() && repo.join(".gitattributes").is_file());
        assert!(!tracked(&pin).contains(&".gitattributes".to_string()));
        std::fs::remove_dir_all(&d).unwrap();
    }

    fn fixture(tag: &str) -> (PathBuf, PathBuf, RepoPin) {
        let d = std::env::temp_dir().join(format!("rth-selfrepo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let src = d.join("src");
        std::fs::create_dir_all(src.join("crates/a/src")).unwrap();
        std::fs::write(src.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(src.join("crates/a/src/lib.rs"), "pub fn a() {}\n").unwrap();
        let pin = init(&d.join("pin"), &d.join("repo"), &src).unwrap();
        (d, src, pin)
    }

    fn sample_actor() -> Actor {
        Actor { kind: ActorKind::Human, name: "ana".into(), model: None }
    }

    fn tracked(pin: &RepoPin) -> Vec<String> {
        let git = Git::pinned(pin).unwrap();
        git.run_s(&pin.repo(), &["ls-tree", "-r", "--name-only", "--full-tree", "HEAD"]).unwrap().lines().map(String::from).collect()
    }

    #[test]
    fn import_adds_a_new_crate_and_commits_it() {
        let (d, src, pin) = fixture("import-add");
        std::fs::create_dir_all(src.join("crates/b/src")).unwrap();
        std::fs::write(src.join("crates/b/src/lib.rs"), "pub fn b() {}\n").unwrap();
        std::fs::write(src.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\"]\n").unwrap();
        let r = import(&pin, &src, &sample_actor(), "add b").unwrap();
        assert_eq!(r.added, ["crates/b/src/lib.rs"]);
        assert_eq!(r.changed, ["Cargo.toml"]);
        assert!(r.removed.is_empty() && r.files == 3);
        let snap = snapshot(&pin).unwrap();
        assert_eq!((Some(snap.head), snap.dirty), (r.head, false));
        assert!(tracked(&pin).contains(&"crates/b/src/lib.rs".to_string()));
        assert_eq!(std::fs::read_to_string(d.join("repo/crates/b/src/lib.rs")).unwrap(), "pub fn b() {}\n");
        assert_eq!(log(&pin).unwrap().len(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_second_identical_import_is_a_noop() {
        let (d, src, pin) = fixture("import-noop");
        std::fs::write(src.join("crates/a/src/lib.rs"), "pub fn a() { /* v2 */ }\n").unwrap();
        let first = import(&pin, &src, &sample_actor(), "v2").unwrap();
        let second = import(&pin, &src, &sample_actor(), "v2 again").unwrap();
        assert!(second.head.is_none() && second.added.is_empty() && second.changed.is_empty() && second.removed.is_empty());
        assert_eq!(second.source_digest, first.source_digest);
        assert_eq!(Some(snapshot(&pin).unwrap().head), first.head, "HEAD unchanged");
        assert_eq!(log(&pin).unwrap().len(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_crate_deleted_from_the_source_is_removed_and_gitignore_stays() {
        let (d, src, pin) = fixture("import-remove");
        let repo = d.join("repo");
        std::fs::create_dir_all(src.join("crates/b/src")).unwrap();
        std::fs::write(src.join("crates/b/src/lib.rs"), "pub fn b() {}\n").unwrap();
        import(&pin, &src, &sample_actor(), "add b").unwrap();
        std::fs::remove_dir_all(src.join("crates/b")).unwrap();

        std::fs::write(repo.join("notes.txt"), "guest\n").unwrap();
        let r = import(&pin, &src, &sample_actor(), "remove b").unwrap();
        assert_eq!(r.removed, ["crates/b/src/lib.rs"]);
        assert!(r.head.is_some());
        let t = tracked(&pin);
        assert!(!t.contains(&"crates/b/src/lib.rs".to_string()) && t.contains(&".gitignore".to_string()));
        assert!(!repo.join("crates/b").exists() && repo.join(".gitignore").is_file());
        assert_eq!(std::fs::read_to_string(repo.join("notes.txt")).unwrap(), "guest\n");
        assert_eq!(snapshot(&pin).unwrap().dirty_paths, ["notes.txt"]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn denied_source_paths_are_skipped_reported_and_never_staged() {
        let (d, src, pin) = fixture("import-deny");
        std::fs::create_dir_all(src.join("crates/a/target")).unwrap();
        std::fs::write(src.join("crates/a/target/out"), "bin").unwrap();
        std::fs::write(src.join("crates/a/.env"), "KEY=1").unwrap();
        std::fs::write(src.join("crates/a/src/take.wav"), "RIFF").unwrap();
        std::fs::write(src.join("crates/a/src/new.rs"), "pub fn n() {}\n").unwrap();
        let r = import(&pin, &src, &sample_actor(), "with denied neighbours").unwrap();
        let skipped: Vec<&str> = r.skipped_denied.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(skipped, ["crates/a/.env", "crates/a/src/take.wav", "crates/a/target"]);
        assert_eq!(r.added, ["crates/a/src/new.rs"]);
        let t = tracked(&pin);
        for p in skipped {
            assert!(!t.iter().any(|x| x.starts_with(p)), "{p} reached HEAD");
            assert!(!d.join("repo").join(p).exists(), "{p} reached the work tree");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn guest_work_under_the_roots_refuses_the_import() {
        let (d, src, pin) = fixture("import-dirty");
        let repo = d.join("repo");
        std::fs::write(repo.join("crates/a/src/lib.rs"), "pub fn a() { /* guest */ }\n").unwrap();
        std::fs::write(src.join("Cargo.toml"), "[workspace]\n# v2\n").unwrap();
        let err = import(&pin, &src, &sample_actor(), "over guest work").unwrap_err().to_string();
        assert!(err.contains("crates/a/src/lib.rs"), "{err}");
        assert_eq!(std::fs::read_to_string(repo.join("crates/a/src/lib.rs")).unwrap(), "pub fn a() { /* guest */ }\n");
        assert_eq!(std::fs::read_to_string(repo.join("Cargo.toml")).unwrap(), "[workspace]\n", "nothing written");
        assert_eq!(snapshot(&pin).unwrap().head, pin.initial_commit);

        commit(&pin, &["crates/a/src/lib.rs".into()], "guest edit").unwrap();
        std::fs::write(repo.join(".gitattributes"), "* -text\n").unwrap();
        let r = import(&pin, &src, &sample_actor(), "after guest landed").unwrap();
        assert!(r.head.is_some() && r.changed.contains(&"Cargo.toml".to_string()));
        assert_eq!(std::fs::read_to_string(repo.join(".gitattributes")).unwrap(), "* -text\n");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_tracked_symlink_on_the_destination_path_refuses_the_import() {
        let (d, src, pin) = fixture("import-symlink");
        let outside = d.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, d.join("repo/crates/b")).unwrap();
        commit(&pin, &["crates/b".into()], "guest lands a symlink").unwrap();
        std::fs::create_dir_all(src.join("crates/b")).unwrap();
        std::fs::write(src.join("crates/b/x.rs"), "pub fn x() {}\n").unwrap();
        let err = import(&pin, &src, &sample_actor(), "through the link").unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        assert!(!outside.join("x.rs").exists(), "wrote outside the work tree");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_import_commit_carries_parseable_trailers() {
        let (d, src, pin) = fixture("import-trailers");
        std::fs::write(src.join("crates/a/src/new.rs"), "pub fn n() {}\n").unwrap();
        let actor = Actor::parse("agent/claude", Some("claude-opus-5-5".into())).unwrap();
        assert!(Actor::parse("robot/x", None).is_err());
        assert!(import(&pin, &src, &Actor { name: "a\nActor: human/ana".into(), ..actor.clone() }, "forge").is_err());
        let r = import(&pin, &src, &actor, "import with provenance\n\nBody line.").unwrap();
        let git = Git::pinned(&pin).unwrap();
        let out = git.run_s(&pin.repo(), &["log", "-1", "--format=%(trailers:only=true,unfold=true)"]).unwrap();
        let t: std::collections::BTreeMap<&str, &str> = out.lines().filter_map(|l| l.split_once(": ")).collect();
        let digest = format!("sha256:{}", r.source_digest);
        let expect = [
            ("Actor", "agent/claude"),
            ("Actor-Model", "claude-opus-5-5"),
            ("Import-Added", "1"),
            ("Import-Changed", "0"),
            ("Import-Removed", "0"),
            ("Source-Digest", digest.as_str()),
            ("Source-Files", "3"),
            ("Source-Kind", "unversioned-workspace"),
        ];
        for (k, v) in expect {
            assert_eq!(t.get(k), Some(&v), "trailer {k} in {out:?}");
        }
        assert_eq!(t.get("Source-Path").copied(), Some(src.display().to_string().as_str()));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
