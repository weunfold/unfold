

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use microsandbox::Sandbox;
use rth_deploy::controller::{BoxSpec, Controller, Layout, guest_agents, sh, volume_path};
use rth_deploy::durable::{sha256_file, sha256_hex, sha256_prefix, utc_ms, write_atomic};
use rth_deploy::record::Kind;
use rth_deploy::store::{ArtifactStore, PublishOptions};
use rth_deploy::{selfrepo, source};
use rth_protocol::io::read_frame;
use rth_protocol::{AGENT_PORT, Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

const LEGACY_SHA: &str = "6de6c27ebda65d41d9efa8b53d0c78d4084318f09780892dfed4912dd343ee10";

struct Run {
    root: PathBuf,
    data: PathBuf,
    out: PathBuf,
    checks: Vec<Value>,
    mode: String,
}

impl Run {
    fn check(&mut self, name: &str, pass: bool, evidence: Value) -> Result<()> {
        println!("{} {name}", if pass { "PASS" } else { "FAIL" });
        self.checks.push(json!({"check": name, "pass": pass, "evidence": evidence}));
        self.save()?;
        ensure!(pass, "check failed: {name}");
        Ok(())
    }
    fn note(&mut self, name: &str, evidence: Value) -> Result<()> {
        println!("NOTE {name}");
        self.checks.push(json!({"note": name, "evidence": evidence}));
        self.save()
    }
    fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.out)?;
        let all = json!({"mode": self.mode, "utc_ms": utc_ms(), "checks": self.checks,
            "passed": self.checks.iter().filter(|c| c["pass"] == true).count(),
            "failed": self.checks.iter().filter(|c| c["pass"] == false).count()});
        write_atomic(&self.out.join(format!("receipts-{}.json", self.mode)), &serde_json::to_vec_pretty(&all)?)?;
        Ok(())
    }
}

fn guard(name: &str) -> Result<()> {
    ensure!(name.starts_with("rth-deploy-test-") || name.starts_with("rth-deploy-mig-"), "refusing to touch {name}");
    Ok(())
}

async fn hello(port: u16) -> Result<Value> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let line = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut reader, &mut buf)).await?.map_err(|e| anyhow!("{e}"))?;
    let env = Envelope::parse_from(&line, Source::Guest).map_err(|e| anyhow!("{e}"))?;
    let v = serde_json::to_value(&env.event)?;
    let mut st = Stamper::new(Source::Host, "accept-observer", Box::new(MonotonicClock::start()));
    let a = st.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: format!("accept-obs-{}", utc_ms()), role: Role::Observer }, None);
    write.write_all(a.to_line().as_bytes()).await?;
    Ok(v)
}

async fn talk(port: u16, texts: &[&str]) -> Result<u64> {
    let stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = Vec::new();
    let _hello = tokio::time::timeout(Duration::from_secs(3), read_frame(&mut reader, &mut buf)).await?.map_err(|e| anyhow!("{e}"))?;
    let name = format!("accept-ctl-{}", utc_ms());
    let mut st = Stamper::new(Source::Host, &name, Box::new(MonotonicClock::start()));
    let a = st.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: name.clone(), role: Role::Controller }, None);
    write.write_all(a.to_line().as_bytes()).await?;
    let mut done = 0;
    let mut pending: Vec<String> = Vec::new();
    let mut ready = false;
    let mut queue: Vec<&str> = texts.to_vec();
    queue.reverse();
    loop {
        if ready && pending.is_empty() {
            let Some(t) = queue.pop() else { break };
            let req = format!("q{}", utc_ms());
            let e = st.stamp(Event::UserText { request_id: req.clone(), text: t.into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None);
            write.write_all(e.to_line().as_bytes()).await?;
            pending.push(req);
        }
        let line = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut reader, &mut buf)).await?.map_err(|e| anyhow!("{e}"))?;
        let Ok(env) = Envelope::parse_from(&line, Source::Guest) else { continue };
        match env.event {
            Event::Ready { role: Role::Controller, .. } => ready = true,
            Event::Ready { role: Role::Observer, .. } => bail!("admitted as observer"),
            Event::AgentDone { .. } => {
                pending.clear();
                done += 1;
            }
            _ => {}
        }
    }
    Ok(done)
}

fn journal_facts(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let boots: std::collections::BTreeSet<String> = text
        .lines()
        .filter(|l| l.contains("\"agentd boot "))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| v["producer"].as_str().map(String::from))
        .collect();
    Ok(json!({
        "bytes": bytes.len(), "sha256": sha256_hex(&bytes), "records": text.lines().count(),
        "boots": boots.len(), "user_text": text.lines().filter(|l| l.contains("\"kind\":\"user_text\"")).count(),
        "first_record": text.lines().next().and_then(|l| serde_json::from_str::<Value>(l).ok()).map(|v| v["id"].clone()),
    }))
}

async fn converged(layout: &Layout, vm: &str, port: u16) -> Result<Value> {
    let c = Controller::open(layout, vm)?;
    let active = c.rec.active.clone().ok_or_else(|| anyhow!("no active deployment"))?;
    let sb = c.sandbox().await?;
    let procs = guest_agents(&sb).await?;
    let h = hello(port).await?;
    let ok = c.rec.pending.is_none()
        && procs.len() == 1
        && procs[0].sha256 == active.deployment.artifact.artifact_id
        && (active.deployment.artifact.attestation == "controller_verified"
            || h["deployment"]["deployment_id"] == active.deployment.deployment_id.as_str());
    Ok(json!({
        "ok": ok, "pending": c.rec.pending, "active_deployment": active.deployment.deployment_id,
        "active_artifact": active.deployment.artifact.artifact_id, "guest_processes": procs,
        "hello_deployment": h["deployment"], "hello_instance": h["instance"], "generation": c.rec.generation,
    }))
}

fn deploy_bin() -> PathBuf {
    std::env::current_exe().unwrap().with_file_name("rth-deploy")
}

fn cli(data: &Path, args: &[&str], crash_after: Option<&str>) -> Result<(i32, String, String)> {
    let mut c = std::process::Command::new(deploy_bin());
    c.arg("--data").arg(data).args(args);
    match crash_after {
        Some(p) => c.env(rth_deploy::controller::ENV_CRASH_AFTER, p),
        None => c.env_remove(rth_deploy::controller::ENV_CRASH_AFTER),
    };
    let o = c.output().context("running rth-deploy")?;
    Ok((o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into()))
}

fn record_json(layout: &Layout, vm: &str) -> Value {
    std::fs::read(layout.box_dir(vm).join("activation.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

async fn boundary(run: &mut Run, a_bin: &Path, b_bin: &Path, workspace: &Path) -> Result<()> {
    let id = format!("{:x}", utc_ms() & 0xffffff);
    let vm = format!("rth-deploy-test-{id}");
    guard(&vm)?;
    let port = 7760;
    let layout = Layout::new(&run.data);
    let spec = BoxSpec::disposable(&vm, port);
    run.note("box", json!({"vm": vm, "volume": spec.volume, "port": port, "data": run.data}))?;
    let result = boundary_steps(run, &layout, &spec, a_bin, b_bin, workspace).await;
    if let Err(e) = &result {
        run.note("aborted", json!(format!("{e:#}")))?;
    }
    if !run_keep() {
        teardown(run, &layout, &vm).await?;
    }
    result
}

fn run_keep() -> bool {
    std::env::args().any(|a| a == "--keep")
}

async fn teardown(run: &mut Run, layout: &Layout, vm: &str) -> Result<()> {
    guard(vm)?;
    let volume = format!("{vm}-state");
    match Controller::open(layout, vm) {
        Ok(c) => c.remove_box().await?,
        Err(_) => {
            if let Ok(h) = Sandbox::get(vm).await {
                if let Ok(sb) = h.connect().await {
                    let _ = sb.stop_and_wait().await;
                }
            }
            let _ = Sandbox::remove(vm).await;
            let _ = microsandbox::Volume::remove(&volume).await;
        }
    }
    let gone_vm = Sandbox::get(vm).await.is_err();
    let gone_vol = microsandbox::Volume::get(&volume).await.is_err();
    run.check("teardown: disposable box and its volume are removed", gone_vm && gone_vol, json!({"vm": vm, "volume": volume}))
}

async fn boundary_steps(run: &mut Run, layout: &Layout, spec: &BoxSpec, a_bin: &Path, b_bin: &Path, workspace: &Path) -> Result<()> {
    let vm = spec.vm.as_str();
    let port = spec.host_port;
    let store = ArtifactStore::open(&layout.artifacts())?;
    let src = source::workspace_identity(workspace)?;

    let a = store.publish(a_bin, PublishOptions { label: Some("A".into()), source: Some(src.clone()), ..Default::default() }, None)?;
    let a_id = a.verified.manifest.artifact_id.clone();
    Controller::register(layout, spec)?;
    let r_a = {
        let mut c = Controller::open(layout, vm)?;
        c.create_box().await?;
        c.activate(&a_id, Kind::Activate, None, None).await?
    };
    let h = hello(port).await?;
    run.check("1 box created with artifact A; guest exe hash and Hello deployment identity are A", r_a.exe_sha256 == a_id && h["deployment"]["artifact_id"] == a_id.as_str() && h["deployment"]["manifest_sha256"] == a.verified.manifest_sha256.as_str(),
        json!({"artifact_a": a_id, "manifest_a": a.verified.manifest_sha256, "receipt": r_a, "hello": h}))?;
    let instance_id = r_a.instance_id.clone();
    let session_id = r_a.session_id.clone();
    let vol = volume_path(&spec.volume).await?;
    let journal = vol.join("journal.jsonl");
    let j0 = journal_facts(&journal)?;

    let pin = selfrepo::init(&layout.box_dir(vm), &vol.join("self-repo"), workspace)?;
    let c = Controller::open(layout, vm)?;
    let sb = c.sandbox().await?;
    drop(c);

    let (code, _, err) = sh(&sb, "cd /rth/self-repo && echo '// self-edit 1 (committed)' >> crates/rth-agentd/src/journal.rs && echo ok").await?;
    ensure!(code == 0, "guest edit failed: {err}");
    let commit = selfrepo::commit(&pin, &["crates/rth-agentd/src/journal.rs".into()], "self-edit 1: comment in journal.rs")?;
    let (code, _, err) = sh(&sb, "cd /rth/self-repo && echo '// self-edit 2 (uncommitted)' >> crates/rth-protocol/src/lib.rs && mkdir -p notes && echo 'dirty untracked note' > notes/wip.txt && echo ok").await?;
    ensure!(code == 0, "guest dirty edit failed: {err}");
    let snap0 = selfrepo::snapshot(&pin)?;
    run.check("self-repo: one committed guest edit and one uncommitted (tracked + untracked) change", snap0.head == commit && snap0.dirty && snap0.dirty_paths.len() == 2,
        json!({"pin": pin, "commit": commit, "snapshot": snap0}))?;

    let turns = talk(port, &["remember the word kiwi"]).await?;
    let j_after_talk = journal_facts(&journal)?;

    let b = store.publish(b_bin, PublishOptions { label: Some("B".into()), source: Some(src.clone()), ..Default::default() }, None)?;
    let b_id = b.verified.manifest.artifact_id.clone();
    let (_, seen, _) = sh(&sb, &format!("test -f /opt/rth-artifacts/{b_id}/rth-agentd && echo visible")).await?;
    let procs = guest_agents(&sb).await?;
    run.check("2a B published into the mounted store: visible in the guest, not executed", seen.trim() == "visible" && procs.len() == 1 && procs[0].sha256 == a_id,
        json!({"artifact_b": b_id, "guest_sees_b": seen.trim(), "guest_processes": procs}))?;
    let r = Controller::open(layout, vm)?.restart().await?;
    run.check("2b process restart without selection change: guest still runs A, same deployment", r.exe_sha256 == a_id && r.deployment_id == r_a.deployment_id && r.boot_id != r_a.boot_id,
        json!({"receipt": r}))?;
    Controller::open(layout, vm)?.stop_box().await?;

    let b_dir = b.verified.dir.to_string_lossy().to_string();
    let sb2 = Sandbox::builder(vm)
        .image("alpine")
        .volume("/opt/rth", |m| m.bind(&b_dir).readonly())
        .detached(true)
        .connect_or_create()
        .await?;
    let (_, legacy_mount, _) = sh(&sb2, "test -e /opt/rth/rth-agentd && echo present || echo absent").await?;
    let after_boot = guest_agents(&sb2).await?;
    let outcome = Controller::open(layout, vm)?.reconcile().await?;
    let conv = converged(layout, vm, port).await?;
    run.check("2c VM stop/start (connect_or_create with other mounts is ignored); controller relaunches exactly A",
        legacy_mount.trim() == "absent" && after_boot.is_empty() && conv["ok"] == true && conv["active_artifact"] == a_id.as_str() && conv["active_deployment"] == r_a.deployment_id.as_str(),
        json!({"builder_asked_for_/opt/rth": b_dir, "guest_/opt/rth": legacy_mount.trim(), "agents_after_vm_boot": after_boot, "reconcile": outcome, "converged": conv}))?;

    let snap = selfrepo::snapshot(&pin)?;
    let r_b = Controller::open(layout, vm)?.activate(&b_id, Kind::Activate, None, Some(snap.clone())).await?;
    let h = hello(port).await?;
    run.check("3 activate B: guest exe hash is B; Hello reports B's deployment id and manifest; identity unchanged",
        r_b.exe_sha256 == b_id && h["deployment"]["deployment_id"] == r_b.deployment_id.as_str() && h["deployment"]["manifest_sha256"] == b.verified.manifest_sha256.as_str()
            && h["instance"]["instance_id"] == instance_id.as_str() && h["instance"]["session_id"] == session_id.as_str(),
        json!({"receipt": r_b, "hello": h, "four_identities": {
            "active_source": {"repo_head": snap.head, "dirty": snap.dirty, "dirty_sha256": snap.dirty_sha256},
            "artifact": {"artifact_id": b_id, "manifest_sha256": b.verified.manifest_sha256, "build_source_digest": src.digest},
            "benchmark_candidate": null,
            "activation_receipt": {"deployment_id": r_b.deployment_id, "receipt_sha256": r_b.sha256(), "generation": Controller::open(layout, vm)?.rec.generation}}}))?;

    let cases = [
        ("selected", a_id.clone(), b_id.clone()),
        ("launched", a_id.clone(), b_id.clone()),
        ("started", a_id.clone(), a_id.clone()),
        ("health_observed", b_id.clone(), b_id.clone()),
        ("healthy", a_id.clone(), a_id.clone()),
    ];
    for (point, target, expect) in cases {
        let (code, _, err) = cli(&run.data, &["activate", "--vm", vm, "--artifact", &target], Some(point))?;
        let at_crash = record_json(layout, vm);
        let pending_phase = at_crash["pending"]["phase"].clone();
        let guest_at_crash = guest_agents(&Controller::open(layout, vm)?.sandbox().await?).await?;
        let (rc, out, rerr) = cli(&run.data, &["reconcile", "--vm", vm], None)?;
        let outcome: Value = serde_json::from_str(&out).map(|v: Value| v["outcome"].clone()).unwrap_or(json!(rerr));
        let conv = converged(layout, vm, port).await?;
        run.check(&format!("4 crash after {point} → controller restart converges to one active deployment"),
            code == rth_deploy::controller::CRASH_EXIT && rc == 0 && conv["ok"] == true && conv["active_artifact"] == expect.as_str(),
            json!({"target": target, "exit_at_crash": code, "stderr": err.trim(), "pending_phase_at_crash": pending_phase,
                   "guest_at_crash": guest_at_crash, "reconcile": outcome, "expected_active": expect, "converged": conv}))?;
    }
    let r_b2 = Controller::open(layout, vm)?.activate(&b_id, Kind::Activate, None, Some(selfrepo::snapshot(&pin)?)).await?;
    run.note("B active again before the rollback test", json!({"receipt": r_b2}))?;

    let before = journal_facts(&journal)?;
    let before_len = before["bytes"].as_u64().unwrap();
    let snap_before = selfrepo::snapshot(&pin)?;
    let r_rb = Controller::open(layout, vm)?.rollback(Some(snap_before.clone())).await?;
    let (prefix, n) = sha256_prefix(&journal, before_len)?;
    let (p0, n0) = sha256_prefix(&journal, j0["bytes"].as_u64().unwrap())?;
    let after = journal_facts(&journal)?;
    let h = hello(port).await?;
    let snap_after = selfrepo::snapshot(&pin)?;
    let log = selfrepo::log(&pin)?;
    let (_, dirty_now, _) = sh(&Controller::open(layout, vm)?.sandbox().await?, "tail -n 1 /rth/self-repo/crates/rth-protocol/src/lib.rs; cat /rth/self-repo/notes/wip.txt").await?;
    run.check("5a rollback B → A: guest runs exact A; instance_id and session_id unchanged",
        r_rb.exe_sha256 == a_id && h["deployment"]["artifact_id"] == a_id.as_str() && r_rb.instance_id == instance_id && r_rb.session_id == session_id,
        json!({"receipt": r_rb, "hello": h}))?;
    run.check("5b journal is append-only across all activations: prefix hashes match",
        n == before_len && prefix == before["sha256"].as_str().unwrap() && n0 == j0["bytes"].as_u64().unwrap() && p0 == j0["sha256"].as_str().unwrap(),
        json!({"before_rollback": before, "after_rollback": after, "first_activation": j0}))?;
    run.check("5c self-repo: commit and uncommitted work survive activation and rollback",
        snap_after == snap_before && snap_after.head == commit && log.iter().any(|l| l.starts_with(&commit)) && dirty_now.contains("self-edit 2") && dirty_now.contains("dirty untracked note"),
        json!({"before": snap_before, "after": snap_after, "log": log, "guest_view": dirty_now.trim()}))?;

    run.check("6a RECEIPT byte-level persistence: retained", n == before_len && prefix == before["sha256"].as_str().unwrap(),
        json!({"journal_bytes_before": before_len, "journal_bytes_after": after["bytes"], "boots_in_journal": after["boots"], "first_record_unchanged": after["first_record"] == j0["first_record"]}))?;
    let ctx = h["instance"]["context"].as_str().unwrap_or_default().to_string();
    run.check("6b RECEIPT semantic continuity: NOT RESTORED (reported honestly; no command replayed)",
        ctx == rth_protocol::deploy::CONTEXT_NOT_RESTORED && after["user_text"] == j_after_talk["user_text"],
        json!({"status": "not_restored", "hello_context": ctx, "turns_before": turns, "user_text_records_after_talk": j_after_talk["user_text"],
               "user_text_records_after_rollback": after["user_text"], "fact_retained_in_history_bytes": String::from_utf8_lossy(&std::fs::read(&journal)?).contains("remember the word kiwi"),
               "model_context": "the backend session starts empty on every boot; the kiwi fact is in the journal, not in the model"}))?;

    let state_path = vol.join("instance.json");
    let original = std::fs::read(&state_path)?;
    let mut newer: Value = serde_json::from_slice(&original)?;
    newer["state_schema"] = json!(2);
    std::fs::write(&state_path, serde_json::to_vec_pretty(&newer)?)?;
    let gen_before = Controller::open(layout, vm)?.rec.generation;
    let refused = Controller::open(layout, vm)?.activate(&b_id, Kind::Activate, None, None).await;
    let refused_restart = Controller::open(layout, vm)?.restart().await;
    let gen_after = Controller::open(layout, vm)?.rec.generation;
    let procs = guest_agents(&Controller::open(layout, vm)?.sandbox().await?).await?;
    run.check("7a controller refuses to launch an artifact older than the state schema (record untouched)",
        refused.as_ref().err().is_some_and(|e| format!("{e:#}").contains("refusing downgrade")) && refused_restart.is_err() && gen_before == gen_after && procs.len() == 1 && procs[0].sha256 == a_id,
        json!({"activate_error": refused.err().map(|e| format!("{e:#}")), "restart_error": refused_restart.err().map(|e| format!("{e:#}")), "generation": [gen_before, gen_after], "guest": procs}))?;

    let jlen = std::fs::metadata(&journal)?.len();
    let probe = format!(
        "cd /rth && RTH_AGENT_MODE=box RTH_STATE=/rth/instance.json RTH_JOURNAL=/rth/journal.jsonl RTH_MANIFEST_SHA256={} RTH_DEPLOYMENT_ID=probe /opt/rth-artifacts/{a_id}/rth-agentd 2>&1; echo \"exit=$?\"",
        a.verified.manifest_sha256
    );
    let (_, out, _) = sh(&Controller::open(layout, vm)?.sandbox().await?, &probe).await?;
    run.check("7b agentd itself refuses a newer state schema (exit 3), journal and state untouched",
        out.contains("downgrade protection") && out.contains("exit=3") && std::fs::metadata(&journal)?.len() == jlen && std::fs::read(&state_path)? == serde_json::to_vec_pretty(&newer)?,
        json!({"guest_output": out.trim()}))?;
    std::fs::write(&state_path, &original)?;
    let outcome = Controller::open(layout, vm)?.reconcile().await?;
    let conv = converged(layout, vm, port).await?;
    run.check("7c with the state restored, the box converges on A again", conv["ok"] == true && conv["active_artifact"] == a_id.as_str(), json!({"reconcile": outcome, "converged": conv}))?;
    run.note("final record", record_json(layout, vm))?;
    Ok(())
}

async fn migration(run: &mut Run, legacy_bin: &Path, b_bin: &Path) -> Result<()> {
    let legacy_sha = sha256_file(legacy_bin)?;
    ensure!(legacy_sha == LEGACY_SHA, "legacy binary {} is {legacy_sha}, not rth-core's {LEGACY_SHA}; stop", legacy_bin.display());
    let id = format!("{:x}", utc_ms() & 0xffffff);
    let vm = format!("rth-deploy-mig-{id}");
    guard(&vm)?;
    let layout = Layout::new(&run.data);
    let result = migration_steps(run, &layout, &vm, legacy_bin, b_bin).await;
    if let Err(e) = &result {
        run.note("aborted", json!(format!("{e:#}")))?;
    }
    if !run_keep() {
        teardown(run, &layout, &vm).await?;
    }
    result
}

async fn migration_steps(run: &mut Run, layout: &Layout, vm: &str, legacy_bin: &Path, b_bin: &Path) -> Result<()> {
    let port = 7761;
    let volume = format!("{vm}-state");

    let legacy_dir = run.root.join("legacy-bin");
    std::fs::create_dir_all(&legacy_dir)?;
    std::fs::copy(legacy_bin, legacy_dir.join("rth-agentd"))?;
    let legacy_dir_s = legacy_dir.canonicalize()?.to_string_lossy().to_string();
    let _ = microsandbox::Volume::builder(volume.as_str()).create().await;
    let sb = Sandbox::builder(vm)
        .image("alpine")
        .cpus(2)
        .memory(1024)
        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), port, AGENT_PORT)
        .volume("/rth", |m| m.named(volume.as_str()))
        .volume("/opt/rth", |m| m.bind(&legacy_dir_s).readonly())
        .detached(true)
        .create()
        .await?;

    let mut turns = 0;
    for boot in 0..3 {
        sh(&sb, "pkill rth-agentd; sleep 0.2; RTH_JOURNAL=/rth/journal.jsonl nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & echo started").await?;
        let mut tries = 0;
        loop {
            match talk(port, &[&format!("hello from boot {boot}"), "how are you"]).await {
                Ok(n) => {
                    turns += n;
                    break;
                }
                Err(_) if tries < 50 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
    let vol = volume_path(&volume).await?;
    let journal = vol.join("journal.jsonl");
    let j0 = journal_facts(&journal)?;
    let cfg0 = Sandbox::get(vm).await?.config_json().to_string();
    run.check("M1 imitation of rth-core (legacy layout, exact legacy binary) with journal history",
        j0["boots"].as_u64() >= Some(3) && turns >= 6 && guest_agents(&sb).await?.first().is_some_and(|p| p.sha256 == LEGACY_SHA),
        json!({"vm": vm, "volume": volume, "journal": j0, "turns": turns, "config": serde_json::from_str::<Value>(&cfg0)?["mounts"]}))?;

    let (code, out, err) = cli(&run.data, &["migrate-core", "--vm", vm, "--expect-sha", LEGACY_SHA, "--backup-root", &run.root.join("backups").to_string_lossy()], None)?;
    let unchanged = Sandbox::get(vm).await?.config_json() == cfg0 && journal_facts(&journal)?["sha256"] == j0["sha256"] && !layout.box_dir(vm).join("activation.json").exists();
    run.check("M2 dry run prints every step and changes nothing", code == 0 && unchanged && out.contains("PLAN [D5]") && out.contains("PLAN [D1]"),
        json!({"stdout": out, "stderr": err}))?;

    let (code, out, err) = cli(&run.data, &["migrate-core", "--vm", vm, "--expect-sha", LEGACY_SHA, "--backup-root", &run.root.join("backups").to_string_lossy(), "--execute", "--confirm", vm], None)?;
    let json_start = out.find("\n{").map(|i| i + 1).unwrap_or(0);
    let plan: Value = serde_json::from_str(&out[json_start..]).unwrap_or(json!({"unparsed": out}));
    let res = &plan["result"];
    let cfg1: Value = serde_json::from_str(Sandbox::get(vm).await?.config_json())?;
    let mounts: Vec<String> = cfg1["mounts"].as_array().map(|m| m.iter().map(|x| x["guest"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let receipt = &res["receipt"];
    run.check("M3 migration: backup, identity from history, re-created on the store mount, legacy A adopted and healthy, journal prefix matches",
        code == 0 && res["journal_prefix_ok"] == true && receipt["exe_sha256"] == LEGACY_SHA && receipt["hello_deployment_id"].is_null()
            && receipt["identity_source"] == "state_file" && receipt["backend_source"] == "journal_boot_line"
            && mounts.contains(&"/opt/rth-artifacts".to_string()) && !mounts.contains(&"/opt/rth".to_string()),
        json!({"stderr": err, "plan": plan, "mounts_after": mounts}))?;
    run.note("M3 downtime", json!({"downtime_ms": res["downtime_ms"], "stop_ms": res["stop_ms"]}))?;
    let backup = PathBuf::from(plan["backup"].as_str().unwrap_or_default());
    let instance_id = plan["instance"]["instance_id"].clone();

    let store = ArtifactStore::open(&layout.artifacts())?;
    let b = store.publish(b_bin, PublishOptions { label: Some("B".into()), ..Default::default() }, None)?;
    let r_b = Controller::open(layout, vm)?.activate(&b.verified.manifest.artifact_id, Kind::Activate, None, None).await?;
    let h = hello(port).await?;
    run.check("M4 attested B activated on the migrated box: Hello reports the migration-derived instance identity",
        r_b.exe_sha256 == b.verified.manifest.artifact_id && h["instance"]["instance_id"] == instance_id && h["deployment"]["deployment_id"] == r_b.deployment_id.as_str(),
        json!({"receipt": r_b, "hello": h}))?;
    let r_a = Controller::open(layout, vm)?.rollback(None).await?;
    let (p, n) = sha256_prefix(&journal, j0["bytes"].as_u64().unwrap())?;
    run.check("M5 controller rollback B → legacy A (schema enforced by the controller); history intact",
        r_a.exe_sha256 == LEGACY_SHA && r_a.identity_source == "state_file" && n == j0["bytes"].as_u64().unwrap() && p == j0["sha256"].as_str().unwrap(),
        json!({"receipt": r_a}))?;

    let data = run.data.clone();
    let rb = |fault: Option<&str>, restore: bool| -> Result<(i32, String, String)> {
        let mut c = std::process::Command::new(deploy_bin());
        c.arg("--data").arg(&data).args(["migrate-core", "--vm", vm, "--rollback", &backup.to_string_lossy(), "--execute", "--confirm", vm]);
        if restore {
            c.arg("--restore-volume");
        }
        c.env_remove(rth_deploy::controller::ENV_CRASH_AFTER);
        match fault {
            Some(f) => c.env("RTH_DEPLOY_FAULT", f),
            None => c.env_remove("RTH_DEPLOY_FAULT"),
        };
        let o = c.output()?;
        Ok((o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into()))
    };
    let mounts_of = |cfg: &str| -> Vec<String> {
        serde_json::from_str::<Value>(cfg).ok().and_then(|c| c["mounts"].as_array().map(|m| m.iter().map(|x| x["guest"].as_str().unwrap_or("").to_string()).collect())).unwrap_or_default()
    };
    let state_path = vol.join("instance.json");
    let state_ok = std::fs::read(&state_path)?;
    for (label, bytes) in [("schema 2", br#"{"state_schema":2,"instance_id":"i","session_id":"s"}"#.to_vec()), ("malformed", b"{not json".to_vec())] {
        std::fs::write(&state_path, &bytes)?;
        let jsha = journal_facts(&journal)?["sha256"].clone();
        let (code, _, err) = rb(None, false)?;
        let still = mounts_of(Sandbox::get(vm).await?.config_json());
        let procs = guest_agents(&Sandbox::get(vm).await?.connect().await?).await?;
        run.check(&format!("M6-inject {label} state: rollback refuses before any change; box, record and journal untouched"),
            code != 0 && err.contains("refusing a legacy launch") && still.contains(&"/opt/rth-artifacts".to_string()) && layout.box_dir(vm).join("activation.json").exists()
                && procs.len() == 1 && journal_facts(&journal)?["sha256"] == jsha,
            json!({"exit": code, "stderr": err.trim(), "mounts": still, "agents": procs}))?;
    }
    std::fs::write(&state_path, &state_ok)?;

    let bj = backup.join("volume/journal.jsonl");
    let bj_bytes = std::fs::read(&bj)?;
    std::fs::write(&bj, [bj_bytes.as_slice(), b"x"].concat())?;
    let (code, _, err) = rb(None, true)?;
    std::fs::write(&bj, &bj_bytes)?;
    run.check("M6-inject tampered backup: --restore-volume refuses (digest mismatch), box untouched",
        code != 0 && err.contains("differs from the recorded") && mounts_of(Sandbox::get(vm).await?.config_json()).contains(&"/opt/rth-artifacts".to_string()),
        json!({"exit": code, "stderr": err.trim()}))?;

    let rec_path = layout.box_dir(vm).join("activation.json");
    let j_before = std::fs::read(&journal)?;
    let cfg_before = Sandbox::get(vm).await?.config_json().to_string();
    let rec_before = std::fs::read(&rec_path)?;
    let (code, _, err) = rb(Some("stop_unverified"), false)?;
    let exists = Sandbox::get(vm).await.is_ok();
    let cfg_after = Sandbox::get(vm).await?.config_json().to_string();
    let j_after = std::fs::read(&journal)?;
    let rec_after = std::fs::read(&rec_path)?;
    run.check("M6-inject unverifiable stop (a): rollback fails closed; journal, VM config and activation record byte-unchanged",
        code != 0 && err.contains("could not prove the box stopped") && exists && j_after == j_before && cfg_after == cfg_before && rec_after == rec_before,
        json!({"exit": code, "stderr": err.trim(), "journal_sha256": sha256_hex(&j_before), "config_sha256": sha256_hex(cfg_before.as_bytes()), "record_sha256": sha256_hex(&rec_before)}))?;
    let (rc, rout, _) = cli(&run.data, &["reconcile", "--vm", vm], None)?;
    let conv = converged(layout, vm, port).await?;
    let (prefix, n) = sha256_prefix(&journal, j_before.len() as u64)?;
    run.check("M6-inject unverifiable stop (b): reconcile restores service; old journal is an exact byte prefix",
        rc == 0 && conv["ok"] == true && n == j_before.len() as u64 && prefix == sha256_hex(&j_before),
        json!({"reconcile": serde_json::from_str::<Value>(&rout).map(|v| v["outcome"].clone()).unwrap_or(json!(rout)), "converged": conv,
               "old_len": j_before.len(), "new_len": std::fs::metadata(&journal)?.len()}))?;

    let (code, out, err) = rb(Some("agent_hash"), false)?;
    let receipts: Vec<_> = std::fs::read_dir(&backup)?.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().starts_with("rollback-")).collect();
    run.check("M6-inject failed rollback check: exit non-zero, diagnostic receipt written, controller record NOT retired",
        code != 0 && err.contains("rollback checks failed") && out.contains("\"agent_hash_ok\":false") && !receipts.is_empty() && layout.box_dir(vm).join("activation.json").exists(),
        json!({"exit": code, "stderr": err.trim(), "receipts": receipts.len()}))?;

    let (code, out, err) = cli(&run.data, &["migrate-core", "--vm", vm, "--rollback", &backup.to_string_lossy(), "--execute", "--confirm", vm], None)?;
    let cfg2: Value = serde_json::from_str(Sandbox::get(vm).await?.config_json())?;
    let mounts2: Vec<String> = cfg2["mounts"].as_array().map(|m| m.iter().map(|x| x["guest"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let (p, n) = sha256_prefix(&journal, j0["bytes"].as_u64().unwrap())?;
    let sb = Sandbox::get(vm).await?.connect().await?;
    let procs = guest_agents(&sb).await?;
    run.check("M6 migration rollback: legacy config (/opt/rth bind) and legacy binary back, journal prefix intact, controller record retired",
        code == 0 && mounts2.contains(&"/opt/rth".to_string()) && !mounts2.contains(&"/opt/rth-artifacts".to_string()) && procs.len() == 1 && procs[0].sha256 == LEGACY_SHA
            && n == j0["bytes"].as_u64().unwrap() && p == j0["sha256"].as_str().unwrap() && !layout.box_dir(vm).exists(),
        json!({"stdout": out, "stderr": err, "mounts_after": mounts2, "agents": procs}))?;

    let _ = rth_deploy::store::remove_tree(&run.root.join("backups"));
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |n: &str| args.windows(2).find(|w| w[0] == n).map(|w| PathBuf::from(&w[1]));
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let a = value("--a").unwrap_or_else(|| ws.join("target/deploy-guest/out/A/rth-agentd"));
    let b = value("--b").unwrap_or_else(|| ws.join("target/deploy-guest/out/B/rth-agentd"));
    let legacy = value("--legacy").unwrap_or_else(|| ws.join("target/aarch64-unknown-linux-musl/release/rth-agentd"));
    let run_id = format!("{}", utc_ms());
    let root = ws.join("target/deploy-acceptance").join(&run_id);
    let out = ws.join("bench/deploy").join(&run_id);
    let mode = args.first().cloned().unwrap_or_default();
    let modes: Vec<&str> = match mode.as_str() {
        "boundary" => vec!["boundary"],
        "migration" => vec!["migration"],
        "all" => vec!["boundary", "migration"],
        _ => {
            eprintln!("usage: rth-deploy-accept boundary|migration|all [--keep] [--a A] [--b B] [--legacy L]");
            std::process::exit(2);
        }
    };
    let mut failed = false;
    for m in modes {
        let mut run = Run { data: root.join(m).join("data"), root: root.join(m), out: out.clone(), checks: Vec::new(), mode: m.into() };
        std::fs::create_dir_all(&run.data)?;
        run.note("inputs", json!({"a": a, "a_sha256": sha256_file(&a).ok(), "b": b, "b_sha256": sha256_file(&b).ok(), "legacy": legacy, "legacy_sha256": sha256_file(&legacy).ok(), "run": run_id}))?;
        let r = if m == "boundary" { boundary(&mut run, &a, &b, &ws).await } else { migration(&mut run, &legacy, &b).await };
        if let Err(e) = r {
            eprintln!("{m}: {e:#}");
            failed = true;
        }
        println!("{m}: receipts in {}", out.join(format!("receipts-{m}.json")).display());
    }
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
