

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use microsandbox::{Sandbox, Volume};
use rth_protocol::AGENT_PORT;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::controller::{BoxSpec, Controller, Layout, create_sandbox, guest_agents, save_compat, sh, volume_path};
use crate::durable::{sha256_file, sha256_hex, sha256_prefix, utc_ms, write_atomic};
use crate::manifest::CompatReceipt;
use crate::record::{Kind, RECORD_FILE};
use crate::store::{ArtifactStore, PublishOptions};

pub const LEGACY_MOUNT: &str = "/opt/rth";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyConfig {
    pub vm: String,
    pub image: String,
    pub cpus: u8,
    pub memory_mib: u32,
    pub host_port: u16,
    pub volume: String,
    pub legacy_dir: String,
}

pub fn parse_legacy(vm: &str, config_json: &str) -> Result<LegacyConfig> {
    let c: Value = serde_json::from_str(config_json).context("persisted config is not JSON")?;
    let image = c["image"]["Oci"]["reference"].as_str().ok_or_else(|| anyhow!("config has no OCI image"))?.to_string();
    let mounts = c["mounts"].as_array().ok_or_else(|| anyhow!("config has no mounts"))?;
    let by_guest = |g: &str| mounts.iter().find(|m| m["guest"] == g);
    if by_guest(crate::controller::ARTIFACT_MOUNT).is_some() {
        bail!("{vm} already mounts {}: already migrated", crate::controller::ARTIFACT_MOUNT);
    }
    let state = by_guest("/rth").ok_or_else(|| anyhow!("{vm} has no /rth mount"))?;
    if state["type"] != "Named" {
        bail!("{vm}'s /rth is not a named volume");
    }
    let legacy = by_guest(LEGACY_MOUNT).ok_or_else(|| anyhow!("{vm} has no {LEGACY_MOUNT} mount: not the legacy layout"))?;
    if legacy["type"] != "Bind" || legacy["options"]["readonly"] != true {
        bail!("{vm}'s {LEGACY_MOUNT} is not a read-only bind");
    }
    if mounts.len() != 2 {
        bail!("{vm} has {} mounts; the migration only knows /rth and {LEGACY_MOUNT}", mounts.len());
    }
    let port = c["network"]["ports"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["guest_port"] == AGENT_PORT && p["host_bind"] == "127.0.0.1"))
        .ok_or_else(|| anyhow!("{vm} has no loopback bind for guest port {AGENT_PORT}"))?;
    Ok(LegacyConfig {
        vm: vm.into(),
        image,
        cpus: c["resources"]["cpus"].as_u64().ok_or_else(|| anyhow!("no cpus"))? as u8,
        memory_mib: c["resources"]["memory_mib"].as_u64().ok_or_else(|| anyhow!("no memory_mib"))? as u32,
        host_port: port["host_port"].as_u64().ok_or_else(|| anyhow!("no host_port"))? as u16,
        volume: state["name"].as_str().ok_or_else(|| anyhow!("no volume name"))?.into(),
        legacy_dir: legacy["host"].as_str().ok_or_else(|| anyhow!("no bind host"))?.into(),
    })
}

#[derive(Debug, Clone)]
pub struct MigrateOpts {
    pub vm: String,
    pub expect_sha: String,
    pub backend: String,
    pub data: PathBuf,
    pub backup_root: PathBuf,
    pub execute: bool,
}

pub fn instance_from_journal(journal: &[u8], migrated_by: &str) -> Value {
    let first = journal.split(|b| *b == b'\n').next().unwrap_or_default();
    let first_env: Value = serde_json::from_slice(first).unwrap_or(Value::Null);
    let boots: std::collections::BTreeSet<String> = journal
        .split(|b| *b == b'\n')
        .filter_map(|l| std::str::from_utf8(l).ok())
        .filter(|l| l.contains("\"agentd boot "))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["producer"].as_str().map(String::from))
        .collect();
    let anchor = sha256_hex(first);
    json!({
        "state_schema": 1,
        "instance_id": format!("inst-{}", &anchor[..32]),
        "session_id": format!("sess-{}", &sha256_hex(format!("session:{anchor}").as_bytes())[..32]),
        "created_utc_ms": utc_ms(),
        "written_by": migrated_by,
        "origin": {
            "derived_from": "sha256 of the journal's first record",
            "first_record_id": first_env["id"],
            "first_producer": first_env["producer"],
            "journal_bytes_at_migration": journal.len(),
            "journal_sha256_at_migration": sha256_hex(journal),
            "boots_seen": boots.len(),
        },
    })
}

fn say(execute: bool, n: &str, what: &str) {
    println!("{} [{n}] {what}", if execute { "EXEC" } else { "PLAN" });
}

pub(crate) fn clone_tree(from: &Path, to: &Path) -> Result<()> {

    let st = std::process::Command::new("/bin/cp").args(["-c", "-R", "-p"]).arg(from).arg(to).status()?;
    if !st.success() {
        bail!("cp -c -R {} {} failed", from.display(), to.display());
    }
    Ok(())
}

pub fn tree_digest(root: &Path) -> Result<(String, u64)> {
    fn walk(root: &Path, p: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
        let m = std::fs::symlink_metadata(p)?;
        if m.is_dir() {
            for e in std::fs::read_dir(p)? {
                walk(root, &e?.path(), out)?;
            }
        } else if m.is_file() {
            out.push((p.strip_prefix(root).unwrap().display().to_string(), sha256_file(p)?));
        } else if m.file_type().is_symlink() {
            out.push((p.strip_prefix(root).unwrap().display().to_string(), format!("link:{}", std::fs::read_link(p)?.display())));
        }
        Ok(())
    }
    let mut v = Vec::new();
    walk(root, root, &mut v)?;
    v.sort();
    let lines: String = v.iter().map(|(p, h)| format!("{p}\0{h}\n")).collect();
    Ok((sha256_hex(lines.as_bytes()), v.len() as u64))
}

fn fault(name: &str) -> bool {
    std::env::var("RTH_DEPLOY_FAULT").is_ok_and(|v| v == name)
}

pub const LEGACY_STATE_SCHEMA: u64 = 1;

pub fn check_legacy_state(volume_dir: &Path) -> Result<()> {
    match std::fs::read(volume_dir.join("instance.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => bail!("cannot read {}/instance.json ({e}); refusing a legacy launch", volume_dir.display()),
        Ok(bytes) => {
            let v: Value = serde_json::from_slice(&bytes)
                .map_err(|e| anyhow!("{}/instance.json is malformed ({e}); refusing a legacy launch", volume_dir.display()))?;
            match v["state_schema"].as_u64() {
                Some(n) if n <= LEGACY_STATE_SCHEMA => Ok(()),
                Some(n) => bail!("state schema {n} in {} is newer than legacy A supports ({LEGACY_STATE_SCHEMA}); refusing a legacy launch", volume_dir.display()),
                None => bail!("{}/instance.json has no integer state_schema; refusing a legacy launch", volume_dir.display()),
            }
        }
    }
}

pub fn is_quiescent(status: Option<microsandbox::sandbox::SandboxStatus>) -> bool {
    use microsandbox::sandbox::SandboxStatus as S;
    matches!(status, None | Some(S::Stopped) | Some(S::Crashed) | Some(S::Created))
}

async fn vm_status(vm: &str) -> Result<Option<microsandbox::sandbox::SandboxStatus>> {
    match Sandbox::get(vm).await {
        Ok(h) => Ok(Some(h.status_snapshot())),
        Err(microsandbox::MicrosandboxError::SandboxNotFound(_)) => Ok(None),
        Err(e) => Err(anyhow!(e).context(format!("reading the status of {vm}"))),
    }
}

pub(crate) async fn stop_vm(vm: &str, host_port: u16) -> Result<()> {
    if !is_quiescent(vm_status(vm).await?) {
        let h = Sandbox::get(vm).await?;
        let sb = h.connect().await.with_context(|| format!("{vm} is not stopped and cannot be reached to stop it"))?;
        sb.stop_and_wait().await.with_context(|| format!("stopping {vm}"))?;
    }
    let mut status = vm_status(vm).await?;
    if fault("stop_unverified") {
        status = Some(microsandbox::sandbox::SandboxStatus::Running);
    }
    if !is_quiescent(status) {
        bail!("{vm} reports {status:?} after stop; refusing to touch its volume");
    }
    if tokio::time::timeout(std::time::Duration::from_millis(500), tokio::net::TcpStream::connect(("127.0.0.1", host_port))).await.is_ok_and(|r| r.is_ok()) {
        bail!("{vm} reports {status:?} but 127.0.0.1:{host_port} still accepts connections; refusing to touch its volume");
    }
    Ok(())
}

pub(crate) async fn remove_vm(vm: &str, volume: &str) -> Result<()> {
    match Sandbox::remove(vm).await {
        Ok(()) | Err(microsandbox::MicrosandboxError::SandboxNotFound(_)) => {}
        Err(e) => return Err(anyhow!(e).context(format!("removing sandbox {vm}"))),
    }
    if vm_status(vm).await?.is_some() {
        bail!("sandbox {vm} still exists after remove");
    }
    Volume::get(volume).await.with_context(|| format!("volume {volume} vanished with the sandbox"))?;
    Ok(())
}

pub const LEGACY_START: &str = "pidof rth-agentd >/dev/null || { RTH_JOURNAL=/rth/journal.jsonl nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & }; echo ok";

async fn create_legacy(l: &LegacyConfig) -> Result<Sandbox> {
    let dir = l.legacy_dir.clone();
    Sandbox::builder(l.vm.as_str())
        .image(l.image.as_str())
        .cpus(l.cpus)
        .memory(l.memory_mib)
        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), l.host_port, AGENT_PORT)
        .volume("/rth", |m| m.named(l.volume.as_str()))
        .volume(LEGACY_MOUNT, move |m| m.bind(&dir).readonly())
        .detached(true)
        .create()
        .await
        .with_context(|| format!("re-creating legacy {}", l.vm))
}

pub async fn migrate(o: &MigrateOpts) -> Result<Value> {
    let x = o.execute;
    let layout = Layout::new(&o.data);

    say(x, "P1", &format!("read the persisted config of {} (microsandbox DB; no exec, no change)", o.vm));
    let handle = Sandbox::get(&o.vm).await.with_context(|| format!("no sandbox {}", o.vm))?;
    let config_json = handle.config_json().to_string();
    let legacy = parse_legacy(&o.vm, &config_json)?;
    println!("       {legacy:?}");
    let legacy_bin = Path::new(&legacy.legacy_dir).join("rth-agentd");
    let bin_sha = sha256_file(&legacy_bin).with_context(|| format!("hashing {}", legacy_bin.display()))?;
    say(x, "P2", &format!("legacy binary {} sha256 {bin_sha}; required {}", legacy_bin.display(), o.expect_sha));
    if bin_sha != o.expect_sha {
        bail!("incompatible: {} is {bin_sha}, not the expected {}. Stop: the legacy dir no longer holds the running artifact", legacy_bin.display(), o.expect_sha);
    }
    let vol = volume_path(&legacy.volume).await?;
    let journal = vol.join("journal.jsonl");
    let jlen = std::fs::metadata(&journal).map(|m| m.len()).unwrap_or(0);
    say(x, "P3", &format!("state volume {} at {} (journal {jlen} bytes now; hashed after quiesce)", legacy.volume, vol.display()));
    let box_dir = layout.box_dir(&o.vm);
    if box_dir.join(RECORD_FILE).exists() {
        bail!("{} already has an activation record: already migrated", o.vm);
    }
    let spec = BoxSpec {
        vm: o.vm.clone(),
        volume: legacy.volume.clone(),
        host_port: legacy.host_port,
        backend: o.backend.clone(),
        image: legacy.image.clone(),
        cpus: legacy.cpus,
        memory_mib: legacy.memory_mib,

        gemini_secret: false,
        web: None,
    };
    say(x, "P4", &format!("guest check: exactly one rth-agentd whose /proc/<pid>/exe hashes to {} (msb exec, read-only)", &o.expect_sha[..12]));
    if x {
        let sb = handle.connect().await.with_context(|| format!("connecting to {}", o.vm))?;
        let procs = guest_agents(&sb).await?;
        match procs.as_slice() {
            [p] if p.sha256 == o.expect_sha => println!("       pid {} {} ok", p.pid, p.exe),
            [] => println!("       no agent running (it will be started as artifact A)"),
            other => bail!("incompatible: guest runs {other:?}, not exactly one {}", o.expect_sha),
        }
    }
    let store = ArtifactStore::open(&layout.artifacts())?;
    say(x, "P5", &format!(
        "write compat receipt {}/{bin_sha}.json (legacy, state_schema 1, protocol 2) and publish {} as artifact A into {} (a copy; the legacy dir is not touched)",
        layout.compat_dir().display(), legacy_bin.display(), store.root().display()
    ));
    let receipt = CompatReceipt::legacy(&bin_sha, &format!("{} as running before migration to the artifact store", o.vm));
    let (artifact_a, manifest_a) = if x {
        let compat_sha = save_compat(&layout, &receipt)?;
        let p = store.publish(&legacy_bin, PublishOptions { label: Some(format!("{}-legacy-A", o.vm)), ..Default::default() }, Some(&receipt))?;
        println!("       artifact {} manifest {} compat {compat_sha} (created={})", p.verified.manifest.artifact_id, p.verified.manifest_sha256, p.created);
        Controller::register(&layout, &spec)?;
        (p.verified.manifest.artifact_id, p.verified.manifest_sha256)
    } else {
        (bin_sha.clone(), "(computed at publish)".into())
    };
    let backup = o.backup_root.join(format!("{}-{}", o.vm, utc_ms()));
    let sandbox_dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".microsandbox/sandboxes").join(&o.vm);

    say(x, "D1", &format!("DOWNTIME BEGINS: stop {} (msb stop equivalent; quiesces every writer of the volume)", o.vm));
    say(x, "D2", &format!("backup: cp -c -R {} {}/volume ; cp -c -R {} {}/sandbox ; save persisted config to {}/vm-config.json", vol.display(), backup.display(), sandbox_dir.display(), backup.display(), backup.display()));
    say(x, "D3", "write /rth/instance.json (instance/session identity derived from the journal's first record; no journal byte is written)");
    say(x, "D4", &format!("remove sandbox {} (msb rm equivalent; the named volume {} is NOT removed)", o.vm, legacy.volume));
    say(x, "D5", &format!(
        "create {}: image {}, {} cpus, {} MiB, 127.0.0.1:{}→{AGENT_PORT}, /rth ← volume {}, {} ← {} (ro). No {LEGACY_MOUNT}.",
        o.vm, legacy.image, legacy.cpus, legacy.memory_mib, legacy.host_port, legacy.volume, crate::controller::ARTIFACT_MOUNT, store.root().display()
    ));
    say(x, "D6", &format!("controller adopts artifact A {} (launch by path+hash, health: Hello/protocol/backend/one process/exe hash)", &artifact_a[..12]));
    say(x, "D7", "DOWNTIME ENDS. Verify journal prefix hash; on any failure in D4–D6 the legacy config is re-created automatically");
    if !x {
        println!("dry run: nothing changed. Re-run with --execute --confirm {} to perform these steps.", o.vm);
        return Ok(json!({"dry_run": true, "legacy": legacy, "artifact_a": artifact_a}));
    }
    let t0 = Instant::now();
    stop_vm(&o.vm, legacy.host_port).await?;
    let t_stopped = t0.elapsed().as_millis();
    std::fs::create_dir_all(&backup)?;
    clone_tree(&vol, &backup.join("volume"))?;
    if sandbox_dir.exists() {
        clone_tree(&sandbox_dir, &backup.join("sandbox"))?;
    }
    write_atomic(&backup.join("vm-config.json"), config_json.as_bytes())?;
    let journal_bytes = std::fs::read(&journal).unwrap_or_default();
    let backup_journal = std::fs::read(backup.join("volume/journal.jsonl")).unwrap_or_default();
    if journal_bytes != backup_journal {
        bail!("backup journal differs from the stopped volume's journal; refusing to continue (nothing removed yet)");
    }
    let (j_len, j_sha) = (journal_bytes.len() as u64, sha256_hex(&journal_bytes));
    let instance_path = vol.join("instance.json");
    let instance = if instance_path.exists() {
        let v: Value = serde_json::from_slice(&std::fs::read(&instance_path)?)?;
        if v["state_schema"].as_u64().unwrap_or(99) > 1 {
            bail!("instance state is schema {}; legacy A supports 1. Stop (nothing removed yet)", v["state_schema"]);
        }
        v
    } else {
        let v = instance_from_journal(&journal_bytes, "migration");
        write_atomic(&instance_path, &serde_json::to_vec_pretty(&v)?)?;
        v
    };
    let mut plan = json!({
        "vm": o.vm, "legacy": legacy, "backup": backup, "config_sha256": sha256_hex(config_json.as_bytes()),
        "journal_bytes": j_len, "journal_sha256": j_sha, "artifact_a": artifact_a, "manifest_a": manifest_a,
        "expect_sha": o.expect_sha, "instance": instance, "data": o.data,
    });
    write_atomic(&backup.join("migration.json"), &serde_json::to_vec_pretty(&plan)?)?;
    remove_vm(&o.vm, &legacy.volume).await?;
    let adopted = async {
        let _sb = create_sandbox(&spec, store.root(), &[]).await?;
        let mut c = Controller::open(&layout, &o.vm)?;
        c.activate(&artifact_a, Kind::Adopt, None, None).await
    }
    .await;
    let receipt = match adopted {
        Ok(r) => r,
        Err(e) => {
            eprintln!("migration failed after removal: {e:#}; restoring the legacy config");

            if vm_status(&o.vm).await?.is_some() {
                stop_vm(&o.vm, legacy.host_port).await.context("automatic fallback: could not prove the half-made box stopped")?;
                remove_vm(&o.vm, &legacy.volume).await.context("automatic fallback")?;
            }
            check_legacy_state(&vol).context("automatic fallback refused")?;
            let sb = create_legacy(&legacy).await?;
            sh(&sb, LEGACY_START).await?;
            let _ = std::fs::rename(&box_dir, box_dir.with_extension(format!("failed-{}", utc_ms())));
            bail!("migration failed and was reverted to the legacy config: {e:#}");
        }
    };
    let downtime_ms = t0.elapsed().as_millis();
    let (prefix_sha, n) = sha256_prefix(&journal, j_len)?;
    let prefix_ok = n == j_len && prefix_sha == j_sha;
    let (tree, files) = tree_digest(&backup.join("volume"))?;
    let new_config = Sandbox::get(&o.vm).await?.config_json().to_string();
    plan["result"] = json!({
        "downtime_ms": downtime_ms, "stop_ms": t_stopped, "receipt": receipt, "receipt_sha256": receipt.sha256(),
        "journal_prefix_ok": prefix_ok, "journal_bytes_now": std::fs::metadata(&journal)?.len(),
        "backup_volume_tree_sha256": tree, "backup_volume_files": files,
        "new_config_sha256": sha256_hex(new_config.as_bytes()),
    });
    write_atomic(&backup.join("migration.json"), &serde_json::to_vec_pretty(&plan)?)?;
    write_atomic(&box_dir.join("migration.json"), &serde_json::to_vec_pretty(&plan)?)?;
    println!("DOWNTIME ENDED after {downtime_ms} ms. journal prefix {j_len} bytes {}; receipt {}", if prefix_ok { "MATCHES" } else { "DIFFERS" }, receipt.sha256());
    if !prefix_ok {
        bail!("journal prefix hash differs after migration; run rollback with --restore-volume");
    }
    Ok(plan)
}

pub async fn rollback(backup: &Path, restore_volume: bool, execute: bool) -> Result<Value> {
    let x = execute;
    let plan: Value = serde_json::from_slice(&std::fs::read(backup.join("migration.json")).context("no migration.json in the backup dir")?)?;
    let legacy: LegacyConfig = serde_json::from_value(plan["legacy"].clone())?;
    let expect = plan["expect_sha"].as_str().unwrap_or_default().to_string();
    let data = PathBuf::from(plan["data"].as_str().unwrap_or_default());
    let legacy_bin = Path::new(&legacy.legacy_dir).join("rth-agentd");
    let sha = sha256_file(&legacy_bin)?;
    say(x, "R1", &format!("legacy binary {} is {sha}; migration ran {expect}", legacy_bin.display()));
    if sha != expect {
        bail!("{} changed since the migration ({sha}); rolling back would run a different binary. Restore that file first", legacy_bin.display());
    }
    let vol = volume_path(&legacy.volume).await?;
    let backup_vol = backup.join("volume");

    let selected = if restore_volume { backup_vol.clone() } else { vol.clone() };
    say(x, "R2", &format!("check {} holds legacy-compatible state (instance.json absent or state_schema <= {LEGACY_STATE_SCHEMA})", selected.display()));
    check_legacy_state(&selected)?;
    if restore_volume {
        let want = plan["result"]["backup_volume_tree_sha256"].as_str().unwrap_or_default();
        let (got, _) = tree_digest(&backup_vol)?;
        say(x, "R2b", &format!("verify the backup volume tree digest {got} against the recorded {want}"));
        if got != want {
            bail!("backup volume digest {got} differs from the recorded {want}; refusing to restore it");
        }
    }
    let journal = vol.join("journal.jsonl");
    let j_len = plan["journal_bytes"].as_u64().unwrap_or(0);
    let j_sha = plan["journal_sha256"].as_str().unwrap_or_default().to_string();
    say(x, "R3", &format!("DOWNTIME BEGINS: stop {} and prove it stopped (status + agent port closed), then remove the sandbox (volume {} kept)", legacy.vm, legacy.volume));
    if restore_volume {
        say(x, "R4", &format!("move {} aside to {}/volume-after-rollback-<ts> and clone {} into its place", vol.display(), backup.display(), backup_vol.display()));
    } else {
        say(x, "R4", "keep the current volume (history appended since the migration stays); instance.json stays, legacy A ignores it");
    }
    say(x, "R5", "re-check the selected volume's state now that nothing writes it; on refusal restart the migrated box and stop");
    say(x, "R6", &format!("create {} with the pre-migration config: {legacy:?}", legacy.vm));
    say(x, "R7", &format!("start the legacy agent exactly as `rth` does: {LEGACY_START}"));
    say(x, "R8", "DOWNTIME ENDS. Verify one agent with the legacy hash and the journal prefix hash; only then retire the controller record");
    if !x {
        println!("dry run: nothing changed.");
        return Ok(json!({"dry_run": true}));
    }
    let t0 = Instant::now();
    stop_vm(&legacy.vm, legacy.host_port).await.context("rollback: could not prove the box stopped; nothing was changed")?;

    if let Err(e) = check_legacy_state(&selected) {
        if let Ok(h) = Sandbox::get(&legacy.vm).await {
            let _ = h.start_detached().await;
            if let Ok(mut c) = Controller::open(&Layout::new(&data), &legacy.vm) {
                let _ = c.reconcile().await;
            }
        }
        return Err(e.context("rollback refused after stop; the migrated box was restarted, controller record kept"));
    }
    remove_vm(&legacy.vm, &legacy.volume).await?;
    if restore_volume {
        let aside = backup.join(format!("volume-after-rollback-{}", utc_ms()));
        std::fs::rename(&vol, &aside)?;
        clone_tree(&backup_vol, &vol)?;
        check_legacy_state(&vol)?;
    }
    let sb = create_legacy(&legacy).await?;
    sh(&sb, LEGACY_START).await?;
    let mut procs = Vec::new();
    for _ in 0..50 {
        procs = guest_agents(&sb).await?;
        if !procs.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let downtime_ms = t0.elapsed().as_millis();
    let (prefix, n) = sha256_prefix(&journal, j_len)?;
    let mut out = json!({
        "downtime_ms": downtime_ms,
        "agents": procs,
        "agent_hash_ok": procs.len() == 1 && procs[0].sha256 == expect && !fault("agent_hash"),
        "journal_prefix_ok": n == j_len && prefix == j_sha,
        "restored_volume": restore_volume,
    });
    let verdict = rollback_verdict(&out);
    let layout = Layout::new(&data);
    let box_dir = layout.box_dir(&legacy.vm);
    if verdict.is_ok() && box_dir.exists() {
        let retired = box_dir.with_extension(format!("retired-{}", utc_ms()));
        std::fs::rename(&box_dir, &retired)?;
        out["controller_record_retired_to"] = json!(retired);
    }
    write_atomic(&backup.join(format!("rollback-{}.json", utc_ms())), &serde_json::to_vec_pretty(&out)?)?;
    println!("rollback finished in {downtime_ms} ms: {out}");
    verdict?;
    Ok(out)
}

pub fn rollback_verdict(out: &Value) -> Result<()> {
    let failed: Vec<&str> = ["agent_hash_ok", "journal_prefix_ok"].into_iter().filter(|k| out[*k] != true).collect();
    if failed.is_empty() { Ok(()) } else { bail!("rollback checks failed: {failed:?}; controller record kept; see the rollback receipt") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY_JSON: &str = r#"{"image":{"Oci":{"reference":"alpine"}},"mounts":[
        {"guest":"/rth","name":"rth-state","type":"Named","options":{"readonly":false}},
        {"guest":"/opt/rth","host":"/w/target/aarch64-unknown-linux-musl/release","type":"Bind","options":{"readonly":true}}],
        "network":{"ports":[{"guest_port":7700,"host_bind":"127.0.0.1","host_port":7700,"protocol":"tcp"}]},
        "resources":{"cpus":2,"memory_mib":1024}}"#;

    #[test]
    fn parses_the_legacy_config_and_refuses_anything_else() {
        let l = parse_legacy("rth-core", LEGACY_JSON).unwrap();
        assert_eq!(
            l,
            LegacyConfig {
                vm: "rth-core".into(),
                image: "alpine".into(),
                cpus: 2,
                memory_mib: 1024,
                host_port: 7700,
                volume: "rth-state".into(),
                legacy_dir: "/w/target/aarch64-unknown-linux-musl/release".into()
            }
        );
        let migrated = LEGACY_JSON.replace("\"/opt/rth\"", "\"/opt/rth-artifacts\"");
        assert!(parse_legacy("x", &migrated).unwrap_err().to_string().contains("already migrated"));
        let writable = LEGACY_JSON.replace("\"readonly\":true", "\"readonly\":false");
        assert!(parse_legacy("x", &writable).is_err());
    }

    #[test]
    fn legacy_launch_refuses_newer_malformed_or_untyped_state() {
        let d = std::env::temp_dir().join(format!("rth-migrate-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        check_legacy_state(&d).expect("absent instance.json is the pre-migration layout");
        let f = d.join("instance.json");
        std::fs::write(&f, r#"{"state_schema":1,"instance_id":"i","session_id":"s"}"#).unwrap();
        check_legacy_state(&d).unwrap();
        for (bytes, want) in [
            (r#"{"state_schema":2}"#, "newer than legacy A"),
            (r#"{"state_schema":"1"}"#, "no integer state_schema"),
            (r#"{"instance_id":"i"}"#, "no integer state_schema"),
            ("{not json", "malformed"),
        ] {
            std::fs::write(&f, bytes).unwrap();
            let err = check_legacy_state(&d).unwrap_err().to_string();
            assert!(err.contains(want), "{bytes}: {err}");
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn only_a_stopped_or_absent_box_counts_as_quiescent() {
        use microsandbox::sandbox::SandboxStatus as S;
        for s in [S::Running, S::Starting, S::Draining, S::Paused] {
            assert!(!is_quiescent(Some(s)), "{s:?} must not count as stopped");
        }
        for s in [None, Some(S::Stopped), Some(S::Crashed), Some(S::Created)] {
            assert!(is_quiescent(s));
        }
    }

    #[test]
    fn rollback_returns_err_when_any_check_failed() {
        assert!(rollback_verdict(&json!({"agent_hash_ok": true, "journal_prefix_ok": true})).is_ok());
        for bad in [json!({"agent_hash_ok": false, "journal_prefix_ok": true}), json!({"agent_hash_ok": true, "journal_prefix_ok": false}), json!({})] {
            assert!(rollback_verdict(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn instance_identity_is_derived_from_the_first_record_and_stable() {
        let j = b"{\"id\":\"guest-1:1\",\"producer\":\"guest-1\",\"msg\":\"agentd boot 1 \"}\n{\"id\":\"x\",\"producer\":\"guest-2\",\"msg\":\"agentd boot 2 \"}\n";
        let a = instance_from_journal(j, "m");
        let more = [j.as_slice(), b"{\"id\":\"y\"}\n"].concat();
        let b = instance_from_journal(&more, "m");
        assert_eq!(a["instance_id"], b["instance_id"]);
        assert_eq!(a["session_id"], b["session_id"]);
        assert_ne!(a["instance_id"], a["session_id"]);
        assert_eq!(a["origin"]["first_record_id"], "guest-1:1");
        assert_eq!(a["origin"]["boots_seen"], 2);
    }
}
