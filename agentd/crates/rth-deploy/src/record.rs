

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::durable::{append_synced, sha256_hex, utc_ms, write_atomic};

pub const RECORD_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Selected,
    Started,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Activate,
    Rollback,

    Adopt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub manifest_sha256: String,

    pub attestation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    pub head: String,
    pub dirty: bool,
    pub dirty_paths: Vec<String>,

    pub dirty_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deployment {
    pub deployment_id: String,
    pub kind: Kind,
    pub artifact: ArtifactRef,
    #[serde(default)]
    pub source: Option<RepoSnapshot>,

    #[serde(default)]
    pub candidate: Option<String>,
    pub selected_utc_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub deployment_id: String,
    pub artifact_id: String,
    pub manifest_sha256: String,

    pub pid: u32,
    pub exe_path: String,
    pub exe_sha256: String,
    pub agent_processes: u32,
    pub protocol: u16,
    pub boot_id: String,

    pub hello_deployment_id: Option<String>,
    pub hello_manifest_sha256: Option<String>,
    pub instance_id: String,
    pub session_id: String,

    pub identity_source: String,
    pub state_schema: Option<u32>,
    pub backend: String,

    pub backend_source: String,
    pub context: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_restore: Option<serde_json::Value>,
    pub journal_bytes_at_check: u64,
    pub checked_utc_ms: u64,
}

impl Receipt {
    pub fn sha256(&self) -> String {
        sha256_hex(&serde_json::to_vec(self).expect("receipt serializes"))
    }

    fn matches(&self, d: &Deployment) -> bool {
        self.deployment_id == d.deployment_id
            && self.artifact_id == d.artifact.artifact_id
            && self.manifest_sha256 == d.artifact.manifest_sha256
            && self.exe_sha256 == d.artifact.artifact_id
            && self.agent_processes == 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Committed {
    pub deployment: Deployment,
    pub receipt: Receipt,

    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub deployment: Deployment,
    pub phase: Phase,
    pub previous: Option<Committed>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageEntry {
    pub generation: u64,
    pub deployment_id: String,
    pub artifact_id: String,
    pub event: String,
    pub utc_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationRecord {
    pub record_version: u32,

    pub instance: String,
    pub generation: u64,

    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    pub active: Option<Committed>,
    pub pending: Option<Pending>,
    pub lineage: Vec<LineageEntry>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransitionError {
    AlreadyPending(String),
    NotPending,
    WrongPhase { want: &'static str, have: Phase },
    ReceiptMismatch(String),
    IdentityChanged { field: &'static str, recorded: String, observed: String },
    NoPrevious,
    HasPrevious,
    NothingActive,
    DuplicateDeployment(String),
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyPending(d) => write!(f, "deployment {d} is still pending; reconcile first"),
            Self::NotPending => write!(f, "no pending deployment"),
            Self::WrongPhase { want, have } => write!(f, "pending deployment is {have:?}, expected {want}"),
            Self::ReceiptMismatch(why) => write!(f, "health receipt does not match the selected deployment: {why}"),
            Self::IdentityChanged { field, recorded, observed } => {
                write!(f, "{field} changed: record has {recorded}, box reports {observed}; refusing (wrong state volume?)")
            }
            Self::NoPrevious => write!(f, "nothing to recover: there was no previous deployment"),
            Self::HasPrevious => write!(f, "a previous deployment exists; recover it instead of abandoning"),
            Self::NothingActive => write!(f, "no active deployment"),
            Self::DuplicateDeployment(d) => write!(f, "deployment id {d} was already used"),
        }
    }
}

impl std::error::Error for TransitionError {}

type T = Result<(), TransitionError>;

impl ActivationRecord {
    pub fn new(instance: &str) -> Self {
        ActivationRecord {
            record_version: RECORD_VERSION,
            instance: instance.into(),
            generation: 0,
            instance_id: None,
            session_id: None,
            active: None,
            pending: None,
            lineage: Vec::new(),
        }
    }

    fn bump(&mut self, deployment_id: &str, artifact_id: &str, event: impl Into<String>) {
        self.generation += 1;
        self.lineage.push(LineageEntry {
            generation: self.generation,
            deployment_id: deployment_id.into(),
            artifact_id: artifact_id.into(),
            event: event.into(),
            utc_ms: utc_ms(),
        });
    }

    pub fn next_deployment_id(&self, artifact_id: &str) -> String {
        format!("dep-{:05}-{}", self.generation + 1, &artifact_id[..12.min(artifact_id.len())])
    }

    fn check_identity(&self, r: &Receipt) -> T {
        for (field, recorded, observed) in
            [("instance_id", &self.instance_id, &r.instance_id), ("session_id", &self.session_id, &r.session_id)]
        {
            if let Some(rec) = recorded {
                if rec != observed {
                    return Err(TransitionError::IdentityChanged { field, recorded: rec.clone(), observed: observed.clone() });
                }
            }
        }
        Ok(())
    }

    pub fn select(&mut self, d: Deployment) -> T {
        if let Some(p) = &self.pending {
            return Err(TransitionError::AlreadyPending(p.deployment.deployment_id.clone()));
        }
        if self.lineage.iter().any(|l| l.deployment_id == d.deployment_id && l.event == "selected") {
            return Err(TransitionError::DuplicateDeployment(d.deployment_id));
        }
        self.bump(&d.deployment_id, &d.artifact.artifact_id, "selected");
        self.pending = Some(Pending { deployment: d, phase: Phase::Selected, previous: self.active.clone(), pid: None, failure: None });
        Ok(())
    }

    pub fn started(&mut self, pid: u32) -> T {
        let p = self.pending.as_mut().ok_or(TransitionError::NotPending)?;
        if p.phase != Phase::Selected {
            return Err(TransitionError::WrongPhase { want: "selected", have: p.phase });
        }
        p.phase = Phase::Started;
        p.pid = Some(pid);
        let (d, a) = (p.deployment.deployment_id.clone(), p.deployment.artifact.artifact_id.clone());
        self.bump(&d, &a, format!("started pid {pid}"));
        Ok(())
    }

    pub fn healthy(&mut self, r: Receipt) -> T {
        let p = self.pending.as_ref().ok_or(TransitionError::NotPending)?;
        if p.phase != Phase::Started {
            return Err(TransitionError::WrongPhase { want: "started", have: p.phase });
        }
        if !r.matches(&p.deployment) {
            return Err(TransitionError::ReceiptMismatch(format!(
                "selected {} / {}, observed {} / exe {} ({} processes)",
                p.deployment.deployment_id, p.deployment.artifact.artifact_id, r.deployment_id, r.exe_sha256, r.agent_processes
            )));
        }
        self.check_identity(&r)?;
        let p = self.pending.take().expect("checked");
        self.instance_id = Some(r.instance_id.clone());
        self.session_id = Some(r.session_id.clone());
        self.bump(&p.deployment.deployment_id, &p.deployment.artifact.artifact_id, "healthy");
        self.active = Some(Committed { deployment: p.deployment, receipt: r, generation: self.generation });
        Ok(())
    }

    pub fn fail(&mut self, reason: &str) -> T {
        let p = self.pending.as_mut().ok_or(TransitionError::NotPending)?;
        if p.phase == Phase::Failed {
            return Err(TransitionError::WrongPhase { want: "selected or started", have: p.phase });
        }
        p.phase = Phase::Failed;
        p.failure = Some(reason.into());
        let (d, a) = (p.deployment.deployment_id.clone(), p.deployment.artifact.artifact_id.clone());
        self.bump(&d, &a, format!("failed: {reason}"));
        Ok(())
    }

    pub fn recovered(&mut self, r: Receipt) -> T {
        let p = self.pending.as_ref().ok_or(TransitionError::NotPending)?;
        if p.phase != Phase::Failed {
            return Err(TransitionError::WrongPhase { want: "failed", have: p.phase });
        }
        let prev = p.previous.as_ref().ok_or(TransitionError::NoPrevious)?;
        if !r.matches(&prev.deployment) {
            return Err(TransitionError::ReceiptMismatch(format!(
                "previous {} / {}, observed {} / exe {}",
                prev.deployment.deployment_id, prev.deployment.artifact.artifact_id, r.deployment_id, r.exe_sha256
            )));
        }
        self.check_identity(&r)?;
        let p = self.pending.take().expect("checked");
        let prev = p.previous.expect("checked");
        self.bump(&prev.deployment.deployment_id, &prev.deployment.artifact.artifact_id, "recovered");
        self.active = Some(Committed { deployment: prev.deployment, receipt: r, generation: self.generation });
        Ok(())
    }

    pub fn abandoned(&mut self) -> T {
        let p = self.pending.as_ref().ok_or(TransitionError::NotPending)?;
        if p.phase != Phase::Failed {
            return Err(TransitionError::WrongPhase { want: "failed", have: p.phase });
        }
        if p.previous.is_some() {
            return Err(TransitionError::HasPrevious);
        }
        let p = self.pending.take().expect("checked");
        self.bump(&p.deployment.deployment_id, &p.deployment.artifact.artifact_id, "abandoned");
        self.active = None;
        Ok(())
    }

    pub fn restarted(&mut self, r: Receipt) -> T {
        if let Some(p) = &self.pending {
            return Err(TransitionError::AlreadyPending(p.deployment.deployment_id.clone()));
        }
        let a = self.active.as_ref().ok_or(TransitionError::NothingActive)?;
        if !r.matches(&a.deployment) {
            return Err(TransitionError::ReceiptMismatch(format!("active {}, observed exe {}", a.deployment.artifact.artifact_id, r.exe_sha256)));
        }
        self.check_identity(&r)?;
        let d = a.deployment.clone();
        self.bump(&d.deployment_id, &d.artifact.artifact_id, format!("restarted boot {}", r.boot_id));
        self.active = Some(Committed { deployment: d, receipt: r, generation: self.generation });
        Ok(())
    }

    pub fn rollback_target(&self) -> Option<String> {
        let current = &self.active.as_ref()?.deployment.artifact.artifact_id;
        self.lineage
            .iter()
            .rev()
            .filter(|l| l.event == "healthy" || l.event == "recovered")
            .find(|l| &l.artifact_id != current)
            .map(|l| l.artifact_id.clone())
    }
}

pub struct RecordStore {
    dir: PathBuf,
    _lock: File,
}

pub const RECORD_FILE: &str = "activation.json";
pub const LOG_FILE: &str = "transitions.jsonl";

impl RecordStore {

    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("controller.lock"))?;
        lock.try_lock().map_err(|e| anyhow::anyhow!("another controller holds {}: {e}", dir.display()))?;
        Ok(RecordStore { dir: dir.to_path_buf(), _lock: lock })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn load(&self) -> Result<Option<ActivationRecord>> {
        load_unlocked(&self.dir)
    }
}

pub fn load_unlocked(dir: &Path) -> Result<Option<ActivationRecord>> {
    {
        match std::fs::read(dir.join(RECORD_FILE)) {
            Ok(b) => {
                let r: ActivationRecord = serde_json::from_slice(&b).context("activation record does not parse")?;
                if r.record_version != RECORD_VERSION {
                    bail!("activation record version {} is not {RECORD_VERSION}", r.record_version);
                }
                Ok(Some(r))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl RecordStore {

    pub fn commit(&self, r: &ActivationRecord) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(r)?;
        bytes.push(b'\n');
        write_atomic(&self.dir.join(RECORD_FILE), &bytes)?;
        append_synced(&self.dir.join(LOG_FILE), &serde_json::to_string(r)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dep(id: &str, artifact: &str) -> Deployment {
        Deployment {
            deployment_id: id.into(),
            kind: Kind::Activate,
            artifact: ArtifactRef { artifact_id: artifact.into(), manifest_sha256: format!("m-{artifact}"), attestation: "self_verified".into() },
            source: None,
            candidate: None,
            selected_utc_ms: 0,
        }
    }

    fn receipt(d: &Deployment, instance: &str) -> Receipt {
        Receipt {
            deployment_id: d.deployment_id.clone(),
            artifact_id: d.artifact.artifact_id.clone(),
            manifest_sha256: d.artifact.manifest_sha256.clone(),
            pid: 10,
            exe_path: "/opt/rth-artifacts/x/rth-agentd".into(),
            exe_sha256: d.artifact.artifact_id.clone(),
            agent_processes: 1,
            protocol: 2,
            boot_id: "b".into(),
            hello_deployment_id: Some(d.deployment_id.clone()),
            hello_manifest_sha256: Some(d.artifact.manifest_sha256.clone()),
            instance_id: instance.into(),
            session_id: "s".into(),
            identity_source: "hello".into(),
            state_schema: Some(1),
            backend: "fake@v1".into(),
            backend_source: "hello".into(),
            context: "x".into(),
            context_restore: None,
            journal_bytes_at_check: 0,
            checked_utc_ms: 0,
        }
    }

    #[test]
    fn context_restore_is_additive_and_keeps_old_receipt_hashes() {
        let d = dep("dep-00001-aaaaaaaaaaaa", &"a".repeat(64));
        let r = receipt(&d, "i");
        let old = serde_json::to_vec(&r).unwrap();
        assert!(!String::from_utf8_lossy(&old).contains("context_restore"));
        let back: Receipt = serde_json::from_slice(&old).unwrap();
        assert_eq!(back.sha256(), r.sha256());
        let mut with = r.clone();
        with.context_restore = Some(serde_json::json!({"restored_turns": 3, "watermark": 123}));
        let back: Receipt = serde_json::from_slice(&serde_json::to_vec(&with).unwrap()).unwrap();
        assert_eq!(back.context_restore, with.context_restore);
        assert_ne!(with.sha256(), r.sha256());
    }

    fn activate(r: &mut ActivationRecord, d: &Deployment) {
        r.select(d.clone()).unwrap();
        r.started(1).unwrap();
        r.healthy(receipt(d, "i")).unwrap();
    }

    #[test]
    fn happy_path_commits_exactly_one_active_deployment() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        activate(&mut r, &a);
        assert_eq!(r.active.as_ref().unwrap().deployment, a);
        assert!(r.pending.is_none());
        assert_eq!(r.instance_id.as_deref(), Some("i"));
        let events: Vec<_> = r.lineage.iter().map(|l| l.event.as_str()).collect();
        assert_eq!(events, ["selected", "started pid 1", "healthy"]);
        assert_eq!(r.generation, 3);
    }

    #[test]
    fn transitions_out_of_order_are_refused() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        assert_eq!(r.started(1), Err(TransitionError::NotPending));
        r.select(a.clone()).unwrap();
        assert!(matches!(r.select(dep("d2", "B")), Err(TransitionError::AlreadyPending(_))));
        assert!(matches!(r.healthy(receipt(&a, "i")), Err(TransitionError::WrongPhase { .. })), "healthy needs started");
        r.started(1).unwrap();
        assert!(matches!(r.started(2), Err(TransitionError::WrongPhase { .. })));
        assert!(matches!(r.recovered(receipt(&a, "i")), Err(TransitionError::WrongPhase { .. })));
    }

    #[test]
    fn a_receipt_for_another_artifact_or_two_processes_is_not_healthy() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        r.select(a.clone()).unwrap();
        r.started(1).unwrap();
        let mut wrong = receipt(&a, "i");
        wrong.exe_sha256 = "B".into();
        assert!(matches!(r.healthy(wrong), Err(TransitionError::ReceiptMismatch(_))));
        let mut two = receipt(&a, "i");
        two.agent_processes = 2;
        assert!(matches!(r.healthy(two), Err(TransitionError::ReceiptMismatch(_))));
        assert!(r.active.is_none());
    }

    #[test]
    fn failure_recovers_the_previous_exact_deployment() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        activate(&mut r, &a);
        let b = dep("d2", "B");
        r.select(b.clone()).unwrap();
        r.started(2).unwrap();
        r.fail("no hello").unwrap();
        assert_eq!(r.active.as_ref().unwrap().deployment, a, "active is untouched while B is pending/failed");

        assert!(matches!(r.recovered(receipt(&b, "i")), Err(TransitionError::ReceiptMismatch(_))));
        r.recovered(receipt(&a, "i")).unwrap();
        assert_eq!(r.active.as_ref().unwrap().deployment, a);
        assert!(r.pending.is_none());
        assert_eq!(r.lineage.last().unwrap().event, "recovered");
    }

    #[test]
    fn identity_change_is_refused() {
        let mut r = ActivationRecord::new("box");
        activate(&mut r, &dep("d1", "A"));
        let b = dep("d2", "B");
        r.select(b.clone()).unwrap();
        r.started(1).unwrap();
        assert!(matches!(r.healthy(receipt(&b, "other")), Err(TransitionError::IdentityChanged { field: "instance_id", .. })));
    }

    #[test]
    fn first_activation_failure_abandons_and_rollback_targets_previous_artifact() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        r.select(a.clone()).unwrap();
        r.fail("crash").unwrap();
        assert_eq!(r.recovered(receipt(&a, "i")), Err(TransitionError::NoPrevious));
        r.abandoned().unwrap();
        assert!(r.active.is_none() && r.pending.is_none());
        activate(&mut r, &dep("d2", "A"));
        assert_eq!(r.rollback_target(), None);
        activate(&mut r, &dep("d3", "B"));
        assert_eq!(r.rollback_target().as_deref(), Some("A"));
        activate(&mut r, &dep("d4", "A"));
        assert_eq!(r.rollback_target().as_deref(), Some("B"));
    }

    #[test]
    fn restart_keeps_the_selection() {
        let mut r = ActivationRecord::new("box");
        let a = dep("d1", "A");
        activate(&mut r, &a);
        let mut again = receipt(&a, "i");
        again.boot_id = "b2".into();
        r.restarted(again).unwrap();
        assert_eq!(r.active.as_ref().unwrap().deployment.deployment_id, "d1");
        assert_eq!(r.active.as_ref().unwrap().receipt.boot_id, "b2");
        assert!(matches!(r.restarted(receipt(&dep("d9", "B"), "i")), Err(TransitionError::ReceiptMismatch(_))));
    }

    #[test]
    fn record_store_is_single_writer_and_round_trips() {
        let d = std::env::temp_dir().join(format!("rth-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let store = RecordStore::open(&d).unwrap();
        assert!(RecordStore::open(&d).is_err(), "second controller is locked out");
        assert!(store.load().unwrap().is_none());
        let mut r = ActivationRecord::new("box");
        activate(&mut r, &dep("d1", "A"));
        store.commit(&r).unwrap();
        assert_eq!(store.load().unwrap().unwrap(), r);
        let log = std::fs::read_to_string(d.join(LOG_FILE)).unwrap();
        assert_eq!(log.lines().count(), 1);
        drop(store);

        let reopened = (0..50).any(|_| RecordStore::open(&d).is_ok() || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            false
        });
        assert!(reopened, "lock released with the controller");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
