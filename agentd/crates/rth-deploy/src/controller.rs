

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use microsandbox::{Sandbox, Volume};
use rth_protocol::deploy::{AGENT_FILE, CONTEXT_STATUSES, ENV_DEPLOYMENT_ID, ENV_MANIFEST_SHA256, ENV_STATE, MANIFEST_FILE};
use rth_protocol::io::read_frame;
use rth_protocol::{AGENT_PORT, Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::durable::{sha256_hex, utc_ms, write_atomic};
use crate::manifest::{Attestation, CompatReceipt, Verified, compat};
use crate::record::{ActivationRecord, ArtifactRef, Deployment, Kind, Phase, Receipt, RecordStore, RepoSnapshot};
use crate::store::ArtifactStore;

pub const ARTIFACT_MOUNT: &str = "/opt/rth-artifacts";
pub const STATE_MOUNT: &str = "/rth";
pub const GUEST_JOURNAL: &str = "/rth/journal.jsonl";
pub const GUEST_STATE_FILE: &str = "/rth/instance.json";

pub const RESERVED_VMS: &[&str] = &["rth-core"];
pub const RESERVED_VOLUMES: &[&str] = &["rth-state"];

pub const ENV_CRASH_AFTER: &str = "RTH_DEPLOY_CRASH_AFTER";
pub const CRASH_EXIT: i32 = 86;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxSpec {
    pub vm: String,
    pub volume: String,
    pub host_port: u16,

    pub backend: String,
    pub image: String,
    pub cpus: u8,
    pub memory_mib: u32,

    #[serde(default)]
    pub gemini_secret: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<WebPlacement>,
}

pub const WEB_STORE_MOUNT: &str = "/opt/rth-web";
pub const UI_MOUNT: &str = "/rth-ui";
pub const OWNER_MOUNT: &str = "/run/rth-owner";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPlacement {

    pub port: u16,

    pub ui_volume: String,

    pub web_store: String,

    pub owner_dir: String,

    pub owner_port: u16,
}

pub const GEMINI_KEY_ENV: &str = "RTH_GEMINI_API_KEY";
pub const GEMINI_HOST: &str = "generativelanguage.googleapis.com";

impl BoxSpec {
    pub fn disposable(vm: &str, port: u16) -> Self {
        BoxSpec {
            vm: vm.into(),
            volume: format!("{vm}-state"),
            host_port: port,
            backend: "fake".into(),
            image: "alpine".into(),
            cpus: 2,
            memory_mib: 1024,
            gemini_secret: false,
            web: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub data: PathBuf,
}

impl Layout {
    pub fn new(data: &Path) -> Self {
        Layout { data: data.to_path_buf() }
    }
    pub fn artifacts(&self) -> PathBuf {
        self.data.join("artifacts")
    }
    pub fn compat_dir(&self) -> PathBuf {
        self.data.join("controller/compat")
    }
    pub fn box_dir(&self, vm: &str) -> PathBuf {
        self.data.join("controller/boxes").join(vm)
    }
}

pub fn save_compat(layout: &Layout, r: &CompatReceipt) -> Result<String> {
    let dir = layout.compat_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", r.artifact_id));
    if let Ok(existing) = std::fs::read(&path) {
        return Ok(sha256_hex(&existing));
    }
    let bytes = r.to_bytes();
    write_atomic(&path, &bytes)?;
    Ok(sha256_hex(&bytes))
}

pub fn load_compat(layout: &Layout, artifact_id: &str) -> Result<Option<(CompatReceipt, String)>> {
    match std::fs::read(layout.compat_dir().join(format!("{artifact_id}.json"))) {
        Ok(b) => Ok(Some((serde_json::from_slice(&b)?, sha256_hex(&b)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestProc {
    pub pid: u32,
    pub state: String,
    pub exe: String,
    pub sha256: String,
}

struct Hello {
    protocol: u16,
    boot_id: String,
    backend: Option<String>,
    deployment: Option<rth_protocol::deploy::DeploymentIdentity>,
    instance: Option<rth_protocol::deploy::InstanceIdentity>,
}

pub fn known_context_status(context: &str) -> bool {
    CONTEXT_STATUSES.contains(&context)
}

pub fn check_context_status(context: &str) -> Result<()> {
    if !known_context_status(context) {
        bail!("agent reports context {context:?}, which this controller does not know how to verify");
    }
    Ok(())
}

pub struct Controller {
    pub layout: Layout,
    pub spec: BoxSpec,
    pub store: ArtifactStore,
    records: RecordStore,
    pub rec: ActivationRecord,
    crash_after: Option<String>,

    recreate_scope: bool,
}

pub async fn sh(sb: &Sandbox, script: &str) -> Result<(i32, String, String)> {
    let out = sb.shell(script.to_string()).await.with_context(|| format!("guest shell: {}", script.lines().next().unwrap_or("")))?;
    Ok((out.status().code, out.stdout().unwrap_or_default(), out.stderr().unwrap_or_default()))
}

pub async fn volume_path(volume: &str) -> Result<PathBuf> {
    let h = Volume::get(volume).await.with_context(|| format!("volume {volume}"))?;
    Ok(h.local().ok_or_else(|| anyhow!("volume {volume} is not local"))?.path.clone())
}

pub async fn guest_agents(sb: &Sandbox) -> Result<Vec<GuestProc>> {
    let script = "for p in $(pidof rth-agentd); do s=$(awk '{print $3}' /proc/$p/stat 2>/dev/null); \
                  e=$(readlink /proc/$p/exe 2>/dev/null); h=$(sha256sum /proc/$p/exe 2>/dev/null | cut -d' ' -f1); \
                  echo \"$p ${s:-?} ${e:--} ${h:--}\"; done";
    let (_, out, _) = sh(sb, script).await?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() == 4).then(|| GuestProc { pid: f[0].parse().unwrap_or(0), state: f[1].into(), exe: f[2].into(), sha256: f[3].into() })
        })
        .filter(|p| p.state != "Z" && p.state != "?")
        .collect())
}

const ALIVE_FN: &str = "alive() { for p in $(pidof rth-agentd); do [ \"$(awk '{print $3}' /proc/$p/stat 2>/dev/null)\" != Z ] && return 0; done; return 1; }";

pub fn stop_script() -> String {
    format!(
        "{ALIVE_FN}; P=$(pidof rth-agentd); [ -n \"$P\" ] && kill $P 2>/dev/null; i=0; while alive; do i=$((i+1)); \
         if [ $i -eq 30 ]; then kill -9 $(pidof rth-agentd) 2>/dev/null; fi; if [ $i -gt 60 ]; then echo stuck; exit 43; fi; sleep 0.1; done; true"
    )
}

pub fn launch_script(d: &Deployment, backend: &str) -> String {
    let id = &d.artifact.artifact_id;
    let msha = &d.artifact.manifest_sha256;
    let dir = format!("{ARTIFACT_MOUNT}/{id}");
    format!(
        "{stop}; A={dir}; \
         [ \"$(sha256sum $A/{AGENT_FILE} | cut -d' ' -f1)\" = \"{id}\" ] || {{ echo bad-binary; exit 41; }}; \
         [ \"$(sha256sum $A/{MANIFEST_FILE} | cut -d' ' -f1)\" = \"{msha}\" ] || {{ echo bad-manifest; exit 42; }}; \
         cd {STATE_MOUNT} && RTH_AGENT_MODE=box RTH_BACKEND={backend} RTH_JOURNAL={GUEST_JOURNAL} {ENV_STATE}={GUEST_STATE_FILE} \
         {ENV_MANIFEST_SHA256}={msha} {ENV_DEPLOYMENT_ID}={did} nohup $A/{AGENT_FILE} >>{STATE_MOUNT}/agentd.log 2>&1 & echo \"started $!\"",
        stop = stop_script(),
        did = d.deployment_id,
    )
}

impl Controller {

    pub fn register(layout: &Layout, spec: &BoxSpec) -> Result<()> {
        let dir = layout.box_dir(&spec.vm);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("box.json");
        match std::fs::read(&path) {
            Ok(b) => {
                let existing: BoxSpec = serde_json::from_slice(&b)?;
                if &existing != spec {
                    bail!("box {} is registered with a different spec: {existing:?}", spec.vm);
                }
                Ok(())
            }
            Err(_) => Ok(write_atomic(&path, &serde_json::to_vec_pretty(spec)?)?),
        }
    }

    pub fn open(layout: &Layout, vm: &str) -> Result<Self> {
        let dir = layout.box_dir(vm);
        let spec: BoxSpec = serde_json::from_slice(&std::fs::read(dir.join("box.json")).with_context(|| format!("box {vm} is not registered"))?)?;
        let records = RecordStore::open(&dir)?;
        let rec = records.load()?.unwrap_or_else(|| ActivationRecord::new(vm));
        Ok(Controller {
            store: ArtifactStore::open(&layout.artifacts())?,
            layout: layout.clone(),
            spec,
            records,
            rec,
            crash_after: std::env::var(ENV_CRASH_AFTER).ok().filter(|s| !s.is_empty()),
            recreate_scope: false,
        })
    }

    pub fn box_dir(&self) -> &Path {
        self.records.dir()
    }

    pub fn pending_recreate(&self) -> Option<serde_json::Value> {
        pending_recreate(self.box_dir())
    }

    fn guard(&self, what: &str) -> Result<()> {
        if self.recreate_scope {
            return Ok(());
        }
        if let Some(p) = self.pending_recreate() {
            bail!(
                "refusing {what}: a box recreate of {} is unresolved (phase {}); nothing was started, stopped or changed. \
                 Resolve it with `rth-deploy box recreate-recover --vm {}` (plan) and `--execute --confirm {}`",
                self.spec.vm,
                p["phase"].as_str().unwrap_or("?"),
                self.spec.vm,
                self.spec.vm
            );
        }
        Ok(())
    }

    pub(crate) fn enter_recreate_scope(&mut self) {
        self.recreate_scope = true;
    }

    fn crash_point(&self, point: &str) {
        if self.crash_after.as_deref() == Some(point) {
            eprintln!("rth-deploy: injected crash after {point}");
            std::process::exit(CRASH_EXIT);
        }
    }

    fn commit(&mut self, point: &str) -> Result<()> {
        self.records.commit(&self.rec)?;
        self.crash_point(point);
        Ok(())
    }

    pub async fn create_box(&self) -> Result<Sandbox> {
        self.guard("create")?;
        if RESERVED_VMS.contains(&self.spec.vm.as_str()) || RESERVED_VOLUMES.contains(&self.spec.volume.as_str()) {
            bail!("{} / {} are reserved; use the migration procedure", self.spec.vm, self.spec.volume);
        }
        create_sandbox(&self.spec, self.store.root(), &[]).await
    }

    pub async fn recreate_sandbox(&self) -> Result<Sandbox> {
        if !self.recreate_scope {
            bail!("recreate_sandbox is only for an active box recreate");
        }
        create_sandbox(&self.spec, self.store.root(), &[]).await
    }

    pub fn replace_spec(&mut self, spec: BoxSpec) -> Result<()> {
        if !self.recreate_scope {
            bail!("replace_spec is only for an active box recreate");
        }
        write_atomic(&self.box_dir().join("box.json"), &serde_json::to_vec_pretty(&spec)?)?;
        self.spec = spec;
        Ok(())
    }

    pub async fn sandbox(&self) -> Result<Sandbox> {
        self.guard("starting or connecting to the box")?;
        let sb = Sandbox::get(&self.spec.vm)
            .await
            .with_context(|| format!("box {}", self.spec.vm))?
            .connect_or_start_detached()
            .await
            .with_context(|| format!("starting box {}", self.spec.vm))?;
        let (_, out, _) = sh(&sb, &format!("test -d {ARTIFACT_MOUNT} && echo layout-ok")).await?;
        if out.trim() != "layout-ok" {
            bail!("box {} has no {ARTIFACT_MOUNT} mount: it still has the legacy layout (see `rth-deploy migrate-core`)", self.spec.vm);
        }
        Ok(sb)
    }

    pub async fn volume_path(&self) -> Result<PathBuf> {
        volume_path(&self.spec.volume).await
    }

    pub async fn instance_state(&self) -> Result<Option<serde_json::Value>> {
        match std::fs::read(self.volume_path().await?.join("instance.json")) {
            Ok(b) => Ok(Some(serde_json::from_slice(&b).context("instance.json does not parse")?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn preflight(&self, artifact_id: &str) -> Result<Verified> {
        let v = self.store.get(artifact_id)?;
        let on_disk = self.instance_state().await?.and_then(|s| s["state_schema"].as_u64()).map(|n| n as u32);
        compat(&v.manifest, on_disk).map_err(|e| anyhow!(e))?;
        if let Attestation::ControllerVerified { compat_receipt_sha256 } = &v.manifest.attestation {
            let (r, sha) = load_compat(&self.layout, artifact_id)?.ok_or_else(|| anyhow!("legacy artifact {artifact_id} has no compat receipt"))?;
            if &sha != compat_receipt_sha256 || r.artifact_id != artifact_id {
                bail!("compat receipt for {artifact_id} changed since publish");
            }
        }
        Ok(v)
    }

    fn deployment(&self, v: &Verified, kind: Kind, candidate: Option<String>, source: Option<RepoSnapshot>) -> Deployment {
        Deployment {
            deployment_id: self.rec.next_deployment_id(&v.manifest.artifact_id),
            kind,
            artifact: ArtifactRef {
                artifact_id: v.manifest.artifact_id.clone(),
                manifest_sha256: v.manifest_sha256.clone(),
                attestation: match v.manifest.attestation {
                    Attestation::SelfVerified => "self_verified".into(),
                    Attestation::ControllerVerified { .. } => "controller_verified".into(),
                },
            },
            source,
            candidate,
            selected_utc_ms: utc_ms(),
        }
    }

    async fn launch(&self, sb: &Sandbox, d: &Deployment) -> Result<u32> {
        self.preflight(&d.artifact.artifact_id).await?;
        let (code, out, err) = sh(sb, &launch_script(d, &self.spec.backend)).await?;
        let pid = out.lines().find_map(|l| l.strip_prefix("started ")).and_then(|p| p.trim().parse().ok());
        match pid {
            Some(pid) if code == 0 => Ok(pid),
            _ => bail!("launch of {} failed (exit {code}): {} {}", d.artifact.artifact_id, out.trim(), err.trim()),
        }
    }

    async fn agentd_log_tail(&self, sb: &Sandbox) -> String {
        sh(sb, "tail -n 5 /rth/agentd.log 2>/dev/null").await.map(|(_, o, _)| o.trim().replace('\n', " | ")).unwrap_or_default()
    }

    async fn read_hello(&self) -> Result<Hello> {
        let stream = tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(("127.0.0.1", self.spec.host_port))).await??;
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut buf = Vec::new();
        let line = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut reader, &mut buf)).await?.map_err(|e| anyhow!("{e}"))?;
        let env = Envelope::parse_from(&line, Source::Guest).map_err(|e| anyhow!("first frame: {e}"))?;
        let Event::Hello { protocol, boot_id, backend, deployment, instance, .. } = env.event else { bail!("first frame is not Hello") };

        let name = format!("deploy-health-{}", utc_ms());
        let mut stamper = Stamper::new(Source::Host, &name, Box::new(MonotonicClock::start()));
        let attach = stamper.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: name.clone(), role: Role::Observer }, None);
        write.write_all(attach.to_line().as_bytes()).await?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = tokio::time::timeout(left, read_frame(&mut reader, &mut buf)).await.context("no Ready")?.map_err(|e| anyhow!("{e}"))?;
            let Ok(env) = Envelope::parse_from(&line, Source::Guest) else { continue };
            match env.event {
                Event::Ready { attachment_id, role: Role::Observer } if attachment_id == name => break,
                Event::Rejected { reason } => bail!("agent rejected the health attach: {reason}"),
                _ => {}
            }
        }
        Ok(Hello { protocol, boot_id, backend, deployment, instance })
    }

    async fn journal_backend(&self, boot_id: &str) -> Result<Option<String>> {
        let path = self.volume_path().await?.join("journal.jsonl");
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let start = len.saturating_sub(1 << 20);
        let bytes = {
            use std::io::{Read, Seek, SeekFrom};
            let mut f = std::fs::File::open(&path)?;
            f.seek(SeekFrom::Start(start))?;
            let mut v = Vec::new();
            f.read_to_end(&mut v)?;
            v
        };
        let needle = format!("agentd boot {boot_id} · backend ");
        let text = String::from_utf8_lossy(&bytes);
        Ok(text.lines().rev().find_map(|l| {
            let i = l.find(&needle)?;
            let rest = &l[i + needle.len()..];
            Some(rest.split([' ', '"']).next().unwrap_or("").to_string())
        }))
    }

    pub async fn health(&self, sb: &Sandbox, d: &Deployment) -> Result<Receipt> {
        let legacy = d.artifact.attestation == "controller_verified";
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut attempts = 0u32;
        let hello = loop {
            match self.read_hello().await {
                Ok(h) => break h,
                Err(e) => {
                    attempts += 1;
                    if Instant::now() > deadline {
                        bail!("no healthy Hello within 20 s ({e:#}); agentd.log: {}", self.agentd_log_tail(sb).await);
                    }
                    if attempts % 10 == 0 && guest_agents(sb).await?.is_empty() {
                        bail!("agent exited before it was healthy; agentd.log: {}", self.agentd_log_tail(sb).await);
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        };
        if hello.protocol != PROTOCOL_VERSION {
            bail!("agent speaks protocol {}, expected {PROTOCOL_VERSION}", hello.protocol);
        }
        let (instance_id, session_id, state_schema, context, identity_source) = if legacy {
            if hello.deployment.is_some() || hello.instance.is_some() {
                bail!("a controller-verified (legacy) artifact reported self-verified identity; the attestation is wrong");
            }
            let st = self.instance_state().await?.ok_or_else(|| anyhow!("legacy boot needs the controller-written instance.json"))?;
            let get = |k: &str| st[k].as_str().map(String::from).ok_or_else(|| anyhow!("instance.json lacks {k}"));
            (get("instance_id")?, get("session_id")?, st["state_schema"].as_u64().map(|n| n as u32), "unavailable (legacy artifact does not report it; its code does not restore model context)".to_string(), "state_file")
        } else {
            let dep = hello.deployment.as_ref().ok_or_else(|| anyhow!("agent reported no deployment identity"))?;
            if dep.deployment_id != d.deployment_id || dep.artifact_id != d.artifact.artifact_id || dep.manifest_sha256 != d.artifact.manifest_sha256 {
                bail!("agent reports deployment {} / {} / {}, controller selected {} / {} / {}", dep.deployment_id, dep.artifact_id, dep.manifest_sha256, d.deployment_id, d.artifact.artifact_id, d.artifact.manifest_sha256);
            }
            let inst = hello.instance.as_ref().ok_or_else(|| anyhow!("agent reported no instance identity"))?;
            check_context_status(&inst.context)?;
            (inst.instance_id.clone(), inst.session_id.clone(), Some(inst.state_schema), inst.context.clone(), "hello")
        };

        let mut journal_backend = None;
        for _ in 0..30 {
            journal_backend = self.journal_backend(&hello.boot_id).await?;
            if journal_backend.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let (backend, backend_source) = match (&hello.backend, &journal_backend) {
            (Some(h), Some(j)) if h != j => bail!("Hello says backend {h}, journal boot line says {j}"),
            (Some(h), _) => (h.clone(), "hello+journal_boot_line"),
            (None, Some(j)) => (j.clone(), "journal_boot_line"),
            (None, None) => bail!("no backend identity for boot {}", hello.boot_id),
        };
        if !backend.starts_with(&self.spec.backend) {
            bail!("agent runs backend {backend}, box expects {}", self.spec.backend);
        }
        let procs = guest_agents(sb).await?;
        let [p] = procs.as_slice() else { bail!("{} agent processes in the guest, expected exactly one: {procs:?}", procs.len()) };
        if p.sha256 != d.artifact.artifact_id {
            bail!("guest process {} runs {} ({}), selected {}", p.pid, p.exe, p.sha256, d.artifact.artifact_id);
        }
        let journal_bytes_at_check = std::fs::metadata(self.volume_path().await?.join("journal.jsonl")).map(|m| m.len()).unwrap_or(0);
        Ok(Receipt {
            deployment_id: d.deployment_id.clone(),
            artifact_id: d.artifact.artifact_id.clone(),
            manifest_sha256: d.artifact.manifest_sha256.clone(),
            pid: p.pid,
            exe_path: p.exe.clone(),
            exe_sha256: p.sha256.clone(),
            agent_processes: procs.len() as u32,
            protocol: hello.protocol,
            boot_id: hello.boot_id,
            hello_deployment_id: hello.deployment.as_ref().map(|x| x.deployment_id.clone()),
            hello_manifest_sha256: hello.deployment.as_ref().map(|x| x.manifest_sha256.clone()),
            instance_id,
            session_id,
            identity_source: identity_source.into(),
            state_schema,
            backend,
            backend_source: backend_source.into(),
            context,

            context_restore: match hello.instance.as_ref().and_then(|i| i.context_restore.as_ref()) {
                Some(c) if !legacy => Some(serde_json::to_value(c)?),
                _ => None,
            },
            journal_bytes_at_check,
            checked_utc_ms: utc_ms(),
        })
    }

    pub async fn activate(&mut self, artifact_id: &str, kind: Kind, candidate: Option<String>, source: Option<RepoSnapshot>) -> Result<Receipt> {
        self.guard("activate")?;
        if let Some(p) = &self.rec.pending {
            bail!("deployment {} is pending ({:?}); run reconcile first", p.deployment.deployment_id, p.phase);
        }

        let v = self.preflight(artifact_id).await?;
        let d = self.deployment(&v, kind, candidate, source);
        self.rec.select(d.clone())?;
        self.commit("selected")?;
        let sb = match self.sandbox().await {
            Ok(sb) => sb,
            Err(e) => {
                self.rec.fail(&format!("box unreachable: {e:#}"))?;
                self.commit("failed")?;
                return Err(e.context("box unreachable; the selection is recorded failed, run reconcile"));
            }
        };
        match self.launch(&sb, &d).await {
            Ok(pid) => {
                self.crash_point("launched");
                self.rec.started(pid)?;
                self.commit("started")?;
            }
            Err(e) => return self.fail_and_recover(&sb, &format!("{e:#}")).await,
        }
        match self.health(&sb, &d).await {
            Ok(r) => {
                self.crash_point("health_observed");
                if let Err(e) = self.rec.healthy(r.clone()) {
                    return self.fail_and_recover(&sb, &e.to_string()).await;
                }
                self.commit("healthy")?;
                Ok(r)
            }
            Err(e) => self.fail_and_recover(&sb, &format!("{e:#}")).await,
        }
    }

    async fn fail_and_recover(&mut self, sb: &Sandbox, reason: &str) -> Result<Receipt> {
        let failed = self.rec.pending.as_ref().map(|p| p.deployment.deployment_id.clone()).unwrap_or_default();
        self.rec.fail(reason)?;
        self.commit("failed")?;
        match self.recover(sb).await? {
            Some(r) => Err(anyhow!("deployment {failed} failed: {reason}; recovered previous {} (exe {})", r.deployment_id, r.exe_sha256)),
            None => Err(anyhow!("deployment {failed} failed: {reason}; nothing was active before, nothing is active now")),
        }
    }

    async fn recover(&mut self, sb: &Sandbox) -> Result<Option<Receipt>> {
        let p = self.rec.pending.clone().ok_or_else(|| anyhow!("nothing pending"))?;
        match p.previous {
            Some(prev) => {
                self.launch(sb, &prev.deployment).await.context("recovering the previous deployment")?;
                let r = self.health(sb, &prev.deployment).await.context("recovered deployment is not healthy")?;
                self.rec.recovered(r.clone())?;
                self.commit("recovered")?;
                Ok(Some(r))
            }
            None => {
                let (code, out, _) = sh(sb, &stop_script()).await?;
                if code != 0 {
                    bail!("could not stop the failed agent: {out}");
                }
                self.rec.abandoned()?;
                self.commit("abandoned")?;
                Ok(None)
            }
        }
    }

    pub async fn rollback(&mut self, source: Option<RepoSnapshot>) -> Result<Receipt> {
        self.guard("rollback")?;
        let target = self.rec.rollback_target().ok_or_else(|| anyhow!("no earlier healthy artifact to roll back to"))?;
        self.activate(&target, Kind::Rollback, None, source).await
    }

    pub async fn restart(&mut self) -> Result<Receipt> {
        self.guard("restart")?;
        let a = self.rec.active.clone().ok_or_else(|| anyhow!("nothing active"))?;
        if self.rec.pending.is_some() {
            bail!("a deployment is pending; run reconcile");
        }
        let sb = self.sandbox().await?;
        self.launch(&sb, &a.deployment).await?;
        let r = self.health(&sb, &a.deployment).await?;
        self.rec.restarted(r.clone())?;
        self.commit("restarted")?;
        Ok(r)
    }

    pub async fn reconcile(&mut self) -> Result<String> {
        self.guard("reconcile")?;
        let sb = self.sandbox().await?;
        match self.rec.pending.clone() {
            Some(p) => {
                let id = p.deployment.deployment_id.clone();
                if p.phase == Phase::Started {
                    match self.health(&sb, &p.deployment).await {
                        Ok(r) if self.rec.healthy(r.clone()).is_ok() => {
                            self.commit("healthy")?;
                            return Ok(format!("rolled forward: {id} was started and is healthy"));
                        }
                        Ok(_) => self.rec.fail("interrupted after start; identity check failed")?,
                        Err(e) => self.rec.fail(&format!("interrupted after start; not healthy: {e:#}"))?,
                    }
                    self.commit("failed")?;
                } else if p.phase == Phase::Selected {
                    self.rec.fail("interrupted after selection; launch outcome unknown")?;
                    self.commit("failed")?;
                }
                match self.recover(&sb).await? {
                    Some(r) => Ok(format!("recovered previous {} after interrupted {id}", r.deployment_id)),
                    None => Ok(format!("abandoned interrupted first deployment {id}; nothing active")),
                }
            }
            None => {
                let Some(a) = self.rec.active.clone() else { return Ok("nothing active, nothing pending".into()) };
                let running = guest_agents(&sb).await?;
                if running.len() == 1 && running[0].sha256 == a.deployment.artifact.artifact_id {
                    if let Ok(r) = self.health(&sb, &a.deployment).await {
                        if r.boot_id != a.receipt.boot_id {
                            self.rec.restarted(r)?;
                            self.commit("restarted")?;
                        }
                        return Ok(format!("verified: {} is running", a.deployment.deployment_id));
                    }
                }
                let r = self.restart().await?;
                Ok(format!("relaunched {} (guest had {} agent(s) not matching it); boot {}", r.deployment_id, running.len(), r.boot_id))
            }
        }
    }

    pub async fn stop_box(&self) -> Result<()> {
        self.guard("stop")?;
        if let Ok(h) = Sandbox::get(&self.spec.vm).await {
            if let Ok(sb) = h.connect().await {
                let _ = sb.stop_and_wait().await;
            }
        }
        Ok(())
    }

    pub async fn remove_box(self) -> Result<()> {
        self.guard("remove")?;
        if RESERVED_VMS.contains(&self.spec.vm.as_str()) || RESERVED_VOLUMES.contains(&self.spec.volume.as_str()) {
            bail!("refusing to remove reserved {} / {}", self.spec.vm, self.spec.volume);
        }
        self.stop_box().await?;
        Sandbox::remove(&self.spec.vm).await.with_context(|| format!("removing {}", self.spec.vm))?;
        Volume::remove(&self.spec.volume).await.with_context(|| format!("removing volume {}", self.spec.volume))?;
        let dir = self.box_dir().to_path_buf();
        drop(self.records);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}

pub fn pending_recreate(box_dir: &Path) -> Option<serde_json::Value> {
    match std::fs::read(box_dir.join(crate::recreate::PENDING_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(serde_json::json!({"phase": "unreadable", "error": e.to_string()})),
        Ok(b) => Some(serde_json::from_slice(&b).unwrap_or_else(|e| serde_json::json!({"phase": "unreadable", "error": e.to_string()}))),
    }
}

pub async fn create_sandbox(spec: &BoxSpec, store_root: &Path, extra_ro_binds: &[(String, String)]) -> Result<Sandbox> {
    let _ = Volume::builder(spec.volume.as_str()).create().await;
    let store = store_root.to_string_lossy().to_string();
    let mut b = Sandbox::builder(spec.vm.as_str())
        .image(spec.image.as_str())
        .cpus(spec.cpus)
        .memory(spec.memory_mib)
        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), spec.host_port, AGENT_PORT)
        .volume(STATE_MOUNT, |m| m.named(spec.volume.as_str()))
        .volume(ARTIFACT_MOUNT, |m| m.bind(&store).readonly());
    for (guest, host) in extra_ro_binds {
        let host = host.clone();
        b = b.volume(guest.as_str(), move |m| m.bind(&host).readonly());
    }
    if let Some(w) = &spec.web {
        if spec.gemini_secret {

            bail!("box {}: a web placement together with the Gemini secret is not supported yet", spec.vm);
        }
        let _ = Volume::builder(w.ui_volume.as_str()).create().await;
        let (web, owner) = (w.web_store.clone(), w.owner_dir.clone());
        let policy = web_policy(w.owner_port)?;
        b = b
            .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), w.port, w.port)
            .volume(UI_MOUNT, |m| m.named(w.ui_volume.as_str()))
            .volume(WEB_STORE_MOUNT, move |m| m.bind(&web).readonly())
            .volume(OWNER_MOUNT, move |m| m.bind(&owner).readonly())
            .network(|n| n.policy(policy));
    }
    if spec.gemini_secret {
        if std::env::var(GEMINI_KEY_ENV).map(|k| k.is_empty()).unwrap_or(true) {
            bail!("box {} needs the Gemini secret, but {GEMINI_KEY_ENV} is not set on the host", spec.vm);
        }
        b = b.secret(|s| {
            s.env("GEMINI_API_KEY").source(microsandbox::SecretSource::env(GEMINI_KEY_ENV)).allow(GEMINI_HOST).substitute_in_header_fields(["x-goog-api-key"])
        });
    }
    b.detached(true).create().await.with_context(|| format!("creating box {}", spec.vm))
}

pub fn web_policy(owner_port: u16) -> Result<microsandbox::NetworkPolicy> {
    microsandbox::NetworkPolicy::builder()
        .default_egress(microsandbox::NetworkAction::Deny)
        .egress(|e| e.tcp().port(owner_port).allow_host())
        .build()
        .map_err(|e| anyhow!("web egress policy: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_specs_without_a_web_placement_are_unchanged() {
        let core = r#"{"vm":"rth-core","volume":"rth-state","host_port":7700,"backend":"fake","image":"alpine","cpus":2,"memory_mib":1024}"#;
        let spec: BoxSpec = serde_json::from_str(core).unwrap();
        assert_eq!(spec.web, None);
        assert!(!spec.gemini_secret);
        let out = serde_json::to_string(&spec).unwrap();
        assert!(!out.contains("web"), "{out}");
        assert_eq!(serde_json::from_str::<BoxSpec>(&out).unwrap(), spec);
        assert_eq!(BoxSpec::disposable("x", 1).web, None);
        let mut w = spec.clone();
        w.image = "node:22-bookworm-slim".into();
        w.web = Some(WebPlacement { port: 3080, ui_volume: "rth-ui".into(), web_store: "/s".into(), owner_dir: "/o".into(), owner_port: 7781 });
        let back: BoxSpec = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(back, w);
        assert_ne!(back, spec);
        assert!(web_policy(7781).is_ok());
    }

    #[test]
    fn known_context_statuses_pass_health_and_unknown_ones_fail_closed() {
        assert_eq!(CONTEXT_STATUSES.len(), 4);
        for s in ["restored", "partial", "not_restored", rth_protocol::deploy::CONTEXT_NOT_RESTORED] {
            assert!(check_context_status(s).is_ok(), "{s} must pass");
        }
        for s in ["", "RESTORED", "restored ", "maybe", "history_retained"] {
            let e = check_context_status(s).unwrap_err();
            assert!(format!("{e:#}").contains("does not know how to verify"), "{s:?} must fail closed");
        }
    }

    #[tokio::test]
    async fn an_unresolved_recreate_bars_every_ordinary_lifecycle_command() {
        let data = std::env::temp_dir().join(format!("rth-deploy-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let layout = Layout::new(&data);
        let spec = BoxSpec::disposable("guard-test", 7999);
        Controller::register(&layout, &spec).unwrap();
        std::fs::write(layout.box_dir("guard-test").join(crate::recreate::PENDING_FILE), r#"{"phase":"spec_replaced"}"#).unwrap();
        let mut c = Controller::open(&layout, "guard-test").unwrap();
        assert_eq!(c.pending_recreate().unwrap()["phase"], "spec_replaced");
        let refused = |e: anyhow::Error| format!("{e:#}").contains("a box recreate of guard-test is unresolved");
        assert!(refused(c.activate(&"a".repeat(64), Kind::Activate, None, None).await.unwrap_err()));
        assert!(refused(c.rollback(None).await.unwrap_err()));
        assert!(refused(c.restart().await.unwrap_err()));
        assert!(refused(c.reconcile().await.unwrap_err()));
        assert!(refused(c.stop_box().await.unwrap_err()));
        assert!(refused(c.sandbox().await.err().unwrap()));
        assert!(refused(c.create_box().await.err().unwrap()));
        assert!(c.replace_spec(spec.clone()).is_err() && c.recreate_sandbox().await.is_err(), "spec and sandbox changes are recreate-only");
        assert!(refused(c.remove_box().await.unwrap_err()));

        let marker = layout.box_dir("guard-test").join(crate::recreate::PENDING_FILE);
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        let mut c = Controller::open(&layout, "guard-test").unwrap();
        assert_eq!(c.pending_recreate().unwrap()["phase"], "unreadable");
        assert!(refused(c.activate(&"a".repeat(64), Kind::Activate, None, None).await.unwrap_err()));
        assert!(refused(c.rollback(None).await.unwrap_err()));
        assert!(refused(c.restart().await.unwrap_err()));
        assert!(refused(c.reconcile().await.unwrap_err()));
        assert!(refused(c.stop_box().await.unwrap_err()));
        assert!(refused(c.sandbox().await.err().unwrap()));
        assert!(refused(c.remove_box().await.unwrap_err()));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn launch_script_names_one_exact_executable_and_both_hashes() {
        let d = Deployment {
            deployment_id: "dep-00001-aaaaaaaaaaaa".into(),
            kind: Kind::Activate,
            artifact: ArtifactRef { artifact_id: "a".repeat(64), manifest_sha256: "m".repeat(64), attestation: "self_verified".into() },
            source: None,
            candidate: None,
            selected_utc_ms: 0,
        };
        let s = launch_script(&d, "fake");
        assert!(s.contains(&format!("A={ARTIFACT_MOUNT}/{}", "a".repeat(64))));
        assert!(s.contains(&format!("= \"{}\" ] || {{ echo bad-binary", "a".repeat(64))));
        assert!(s.contains(&format!("{ENV_MANIFEST_SHA256}={}", "m".repeat(64))));
        assert!(s.contains("RTH_DEPLOYMENT_ID=dep-00001-aaaaaaaaaaaa"));

        assert!(s.contains(" RTH_AGENT_MODE=box "), "the box deployment states its mode: {s}");
        assert!(!s.contains("ls ") && !s.contains("sort") && !s.contains("*"), "never picks a newest or a glob");
    }
}
