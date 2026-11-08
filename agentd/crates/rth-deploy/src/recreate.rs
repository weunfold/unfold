

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::controller::{BoxSpec, Controller, GEMINI_KEY_ENV, Layout};
use crate::durable::{sha256_file, sha256_prefix, utc_ms, write_atomic};
use crate::migrate::{clone_tree, remove_vm, stop_vm, tree_digest};
use crate::record::{Kind, RepoSnapshot};

pub const PENDING_FILE: &str = "recreate.pending.json";

pub struct RecreateOpts {
    pub vm: String,
    pub artifact: String,

    pub gemini_secret: Option<bool>,
    pub backend: Option<String>,

    pub image: Option<String>,

    pub web: Option<Option<crate::controller::WebPlacement>>,
    pub candidate: Option<String>,
    pub backup_root: PathBuf,
    pub execute: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Txn {
    pub vm: String,

    pub phase: String,
    pub old_spec: BoxSpec,
    pub new_spec: BoxSpec,
    pub target_artifact: String,
    pub original_deployment: String,
    pub original_artifact: String,
    pub instance_id: String,
    pub session_id: String,
    pub journal_bytes: u64,
    pub journal_sha256: String,
    pub repo: Option<RepoSnapshot>,
    pub backup: Option<PathBuf>,
    pub history: Vec<(String, u64, String)>,
    pub started_utc_ms: u64,
}

impl Txn {
    fn path(c: &Controller) -> PathBuf {
        c.box_dir().join(PENDING_FILE)
    }

    fn save(&mut self, c: &Controller, phase: &str, note: &str) -> Result<()> {
        self.phase = phase.into();
        self.history.push((phase.into(), utc_ms(), note.into()));
        write_atomic(&Self::path(c), &serde_json::to_vec_pretty(self)?).context("writing the recreate transaction record")
    }

    fn close(&mut self, c: &Controller, phase: &str, note: &str, out: &Value) -> Result<PathBuf> {
        self.save(c, phase, note)?;
        let done = c.box_dir().join(format!("recreate-{}.json", utc_ms()));
        write_atomic(&done, &serde_json::to_vec_pretty(&json!({"txn": self, "result": out}))?)?;
        std::fs::remove_file(Self::path(c))?;
        Ok(done)
    }
}

fn fault(name: &str) -> bool {
    std::env::var("RTH_RECREATE_FAULT").is_ok_and(|v| v.split(',').any(|f| f == name))
}

fn say(execute: bool, n: &str, what: &str) {
    println!("{} [{n}] {what}", if execute { "EXEC" } else { "PLAN" });
}

async fn proven_stop(vm: &str, port: u16, injected: &str) -> Result<()> {
    if fault(injected) {
        bail!("injected: {injected} could not be proven");
    }
    stop_vm(vm, port).await
}

async fn proven_remove(vm: &str, volume: &str, injected: &str) -> Result<()> {
    if fault(injected) {
        bail!("injected: {injected} could not be proven");
    }
    remove_vm(vm, volume).await
}

type Source<'a> = &'a dyn Fn(&Controller) -> Result<Option<RepoSnapshot>>;

pub async fn recreate(layout: &Layout, o: &RecreateOpts, source: Source<'_>) -> Result<Value> {
    let x = o.execute;
    let mut c = Controller::open(layout, &o.vm)?;
    if Txn::path(&c).exists() {
        bail!("a recreate of {} is pending ({}); run `rth-deploy box recreate-recover --vm {}` first", o.vm, Txn::path(&c).display(), o.vm);
    }
    let old = c.spec.clone();
    let active = c.rec.active.clone().ok_or_else(|| anyhow!("{} has no active deployment; nothing to preserve", o.vm))?;
    if c.rec.pending.is_some() {
        bail!("{} has a pending controller transition; run `rth-deploy reconcile --vm {}` first", o.vm, o.vm);
    }
    let mut new = old.clone();
    if let Some(g) = o.gemini_secret {
        new.gemini_secret = g;
    }
    if let Some(b) = &o.backend {
        new.backend = b.clone();
    }
    if let Some(i) = &o.image {
        new.image = i.clone();
    }
    if let Some(w) = &o.web {
        new.web = w.clone();
    }
    let verified = c.preflight(&o.artifact).await.context("preflight of the target artifact")?;
    say(x, "P1", &format!(
        "{}: active {} ({}), instance {}, session {}; target {} passes preflight (schema {}, protocol {})",
        o.vm,
        active.deployment.deployment_id,
        &active.deployment.artifact.artifact_id[..12],
        active.receipt.instance_id,
        active.receipt.session_id,
        &o.artifact[..12.min(o.artifact.len())],
        verified.manifest.state_schema,
        verified.manifest.protocol_version
    ));
    say(x, "P1", &format!("spec: backend {} -> {}, gemini_secret {} -> {}", old.backend, new.backend, old.gemini_secret, new.gemini_secret));
    if old.image != new.image || old.web != new.web {
        say(x, "P1", &format!("spec: image {} -> {}, web {} -> {}", old.image, new.image, serde_json::to_string(&old.web)?, serde_json::to_string(&new.web)?));
    }
    if new.gemini_secret && std::env::var(GEMINI_KEY_ENV).map(|k| k.is_empty()).unwrap_or(true) {
        bail!("the Gemini secret needs {GEMINI_KEY_ENV} in this shell (from bws); nothing was changed");
    }
    let vol = c.volume_path().await?;
    let journal = vol.join("journal.jsonl");
    for (n, what) in [
        ("D1", format!("stop {} and prove it stopped (status + 127.0.0.1:{} refused)", o.vm, old.host_port)),
        ("D2", format!("back up the stopped volume {} and the sandbox dir under {}", vol.display(), o.backup_root.display())),
        ("D3", format!("remove the sandbox {} and prove it is gone (the volume {} stays)", o.vm, old.volume)),
        ("D4", "write the new box spec and create the sandbox from it".into()),
        ("D5", format!("activate {} through the controller (its own receipt)", &o.artifact[..12.min(o.artifact.len())])),
        ("D6", "verify the journal prefix and the self-repo; a failure in D4..D6 restores the original artifact under the old spec".into()),
    ] {
        say(x, n, &what);
    }
    if !x {
        return Ok(json!({"plan": true, "vm": o.vm, "old_spec": old, "new_spec": new, "active": active.deployment.deployment_id}));
    }

    let mut t = Txn {
        vm: o.vm.clone(),
        phase: String::new(),
        old_spec: old.clone(),
        new_spec: new.clone(),
        target_artifact: o.artifact.clone(),
        original_deployment: active.deployment.deployment_id.clone(),
        original_artifact: active.deployment.artifact.artifact_id.clone(),
        instance_id: active.receipt.instance_id.clone(),
        session_id: active.receipt.session_id.clone(),
        journal_bytes: 0,
        journal_sha256: String::new(),
        repo: source(&c)?,
        backup: None,
        history: Vec::new(),
        started_utc_ms: utc_ms(),
    };
    t.save(&c, "planned", "before any destructive step")?;

    c.enter_recreate_scope();
    let t0 = std::time::Instant::now();

    if let Err(e) = proven_stop(&o.vm, old.host_port, "stop").await {
        t.save(&c, "uncertain", &format!("stop not proven: {e:#}; nothing was altered after it"))?;
        bail!("recreate stopped at D1 (stop not proven): {e:#}; transaction kept at {}", Txn::path(&c).display());
    }

    let (jsha, jlen) = sha256_prefix(&journal, u64::MAX)?;
    t.journal_bytes = jlen;
    t.journal_sha256 = jsha.clone();
    t.save(&c, "stopped", "stop proven")?;

    let backup = o.backup_root.join(format!("{}-recreate-{}", o.vm, utc_ms()));
    std::fs::create_dir_all(&backup)?;
    clone_tree(&vol, &backup.join("volume"))?;
    let sandbox_dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".microsandbox/sandboxes").join(&o.vm);
    if sandbox_dir.exists() {
        clone_tree(&sandbox_dir, &backup.join("sandbox"))?;
    }
    write_atomic(&backup.join("box.json"), &serde_json::to_vec_pretty(&old)?)?;
    if sha256_file(&backup.join("volume/journal.jsonl")).unwrap_or_default() != jsha {
        t.save(&c, "stopped", "backup journal differs; nothing removed")?;
        bail!("the backup journal differs from the stopped volume's; nothing was removed. The box is stopped and the recreate is pending: run `rth-deploy box recreate-recover --vm {}`", o.vm);
    }
    let (backup_digest, files) = tree_digest(&backup.join("volume"))?;
    t.backup = Some(backup.clone());
    t.save(&c, "backed_up", &format!("{files} files, volume tree {backup_digest}"))?;

    if let Err(e) = proven_remove(&o.vm, &old.volume, "remove").await {
        t.save(&c, "uncertain", &format!("removal not proven: {e:#}; nothing was created or launched after it"))?;
        bail!("recreate stopped at D3 (removal not proven): {e:#}; transaction kept at {}", Txn::path(&c).display());
    }
    t.save(&c, "removed", "the old sandbox is gone; the volume stays")?;

    let attempt: Result<Value> = async {
        c.replace_spec(new.clone())?;
        t.save(&c, "spec_replaced", "box.json now holds the new spec")?;
        if fault("after_spec") {
            eprintln!("rth-deploy: injected interruption after the spec replacement");
            std::process::exit(86);
        }
        if fault("create") {
            bail!("injected: creating the sandbox failed");
        }
        c.recreate_sandbox().await.context("creating the sandbox from the new spec")?;
        t.save(&c, "created", "the new sandbox exists")?;
        if fault("activate") {
            bail!("injected: activation failed before launch");
        }
        let src = source(&c)?;
        let receipt = c.activate(&o.artifact, Kind::Activate, o.candidate.clone(), src).await?;
        t.save(&c, "activated", &format!("{} healthy", receipt.deployment_id))?;
        if fault("receipt") {
            bail!("injected: writing the final receipt failed");
        }
        let checks = verify(&c, &t, &journal, &o.artifact, &new, source)?;
        Ok(json!({"receipt": receipt, "receipt_sha256": receipt.sha256(), "checks": checks}))
    }
    .await;
    match attempt {
        Ok(mut out) => {
            out["vm"] = json!(o.vm);
            out["downtime_ms"] = json!(t0.elapsed().as_millis());
            out["backup"] = json!(backup);
            out["backup_volume_digest"] = json!(backup_digest);
            out["journal_prefix"] = json!({"bytes": jlen, "sha256": jsha, "kept": true});
            out["old_spec"] = json!(old);
            out["new_spec"] = json!(new);
            out["previous_deployment"] = json!(active.deployment.deployment_id);
            let done = t.close(&c, "verified", "recreated and verified", &out)?;
            out["transaction"] = json!(done);
            Ok(out)
        }
        Err(e) => {
            let detail = format!("{e:#}");
            let restored = restore(&mut c, &mut t, &journal, &detail, source).await?;
            bail!("recreate failed ({detail}); restored the original artifact under the old spec: {restored}");
        }
    }
}

fn verify(c: &Controller, t: &Txn, journal: &Path, artifact: &str, spec: &BoxSpec, source: Source<'_>) -> Result<Value> {
    let a = c.rec.active.as_ref().ok_or_else(|| anyhow!("no active deployment after the operation"))?;
    let (prefix, _) = sha256_prefix(journal, t.journal_bytes)?;
    let repo = source(c)?;
    let checks = json!({
        "artifact": a.deployment.artifact.artifact_id == artifact && a.receipt.exe_sha256 == artifact,
        "backend": a.receipt.backend.starts_with(&spec.backend),
        "spec": &c.spec == spec,
        "identity": a.receipt.instance_id == t.instance_id && a.receipt.session_id == t.session_id,
        "journal_prefix": prefix == t.journal_sha256,
        "self_repo": match (&t.repo, &repo) {
            (Some(b), Some(n)) => b.head == n.head && b.dirty_sha256 == n.dirty_sha256,
            (None, None) => true,
            _ => false,
        },
    });
    if checks.as_object().unwrap().values().any(|v| v != &json!(true)) {
        bail!("verification failed: {checks}");
    }
    Ok(json!({"deployment": a.deployment.deployment_id, "receipt_boot": a.receipt.boot_id, "backend": a.receipt.backend, "checks": checks}))
}

async fn restore(c: &mut Controller, t: &mut Txn, journal: &Path, why: &str, source: Source<'_>) -> Result<Value> {
    t.save(c, "restoring", why)?;
    let cur = c.spec.clone();
    if let Err(e) = proven_stop(&t.vm, cur.host_port, "fallback_stop").await {
        t.save(c, "uncertain", &format!("fallback stop not proven: {e:#}; nothing more was altered"))?;
        bail!("fallback stopped (stop not proven): {e:#}; transaction kept at {}", Txn::path(c).display());
    }
    if let Err(e) = proven_remove(&t.vm, &t.old_spec.volume, "fallback_remove").await {
        t.save(c, "uncertain", &format!("fallback removal not proven: {e:#}; nothing was created or launched"))?;
        bail!("fallback stopped (removal not proven): {e:#}; transaction kept at {}", Txn::path(c).display());
    }
    c.replace_spec(t.old_spec.clone())?;
    c.recreate_sandbox().await.context("re-creating the old sandbox")?;
    t.save(c, "restoring", "old spec and sandbox back; selecting the original artifact")?;

    let settled = if c.rec.pending.is_some() { c.reconcile().await.unwrap_or_else(|e| format!("reconcile: {e:#}")) } else { "no pending transition".into() };
    let running_original = c.rec.active.as_ref().is_some_and(|a| a.deployment.artifact.artifact_id == t.original_artifact);
    let how = if running_original {

        let r = c.restart().await.context("relaunching the original deployment")?;
        format!("{settled}; relaunched {}", r.deployment_id)
    } else {
        let src = source(c)?;
        let r = c.activate(&t.original_artifact, Kind::Rollback, None, src).await.context("selecting the original artifact")?;
        format!("{settled}; selected the original artifact again as {}", r.deployment_id)
    };
    let (orig, old) = (t.original_artifact.clone(), t.old_spec.clone());
    let checks = verify(c, t, journal, &orig, &old, source)?;
    let out = json!({"restored": how, "verify": checks});
    let done = t.close(c, "restored", &how, &out)?;
    Ok(json!({"restored": how, "verify": checks, "transaction": done}))
}

pub async fn recover(layout: &Layout, vm: &str, execute: bool, source: Source<'_>) -> Result<Value> {
    let mut c = Controller::open(layout, vm)?;
    c.enter_recreate_scope();
    let raw = std::fs::read(Txn::path(&c)).with_context(|| format!("no pending recreate for {vm}"))?;
    let mut t: Txn = serde_json::from_slice(&raw)?;
    say(execute, "R0", &format!("pending recreate of {vm} at phase {}; original {} ({})", t.phase, t.original_deployment, &t.original_artifact[..12]));
    if t.phase == "uncertain" {
        bail!("the recreate of {vm} is uncertain ({:?}); inspect the box by hand: nothing is changed automatically", t.history.last());
    }
    if !execute {
        return Ok(json!({"plan": true, "txn": t}));
    }
    let journal = c.volume_path().await?.join("journal.jsonl");
    if t.journal_sha256.is_empty() {

        let out = json!({"recovered": "nothing destructive had happened; the record was only planned"});
        t.close(&c, "abandoned", "interrupted before the stop", &out)?;
        let outcome = c.reconcile().await?;
        return Ok(json!({"recovered": outcome}));
    }
    restore(&mut c, &mut t, &journal, "resumed after an interruption", source).await
}

pub fn default_backup_root(data: &Path) -> PathBuf {
    data.join("backups")
}
