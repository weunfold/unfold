

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rth_protocol::deploy::{
    AGENT_FILE, CONTEXT_NONE, DeploymentIdentity, ENV_DEPLOYMENT_ID, ENV_MANIFEST_SHA256, ENV_STATE, InstanceIdentity,
    MANIFEST_FILE, MARK_LEN, build_mark,
};
use rth_protocol::{PROTOCOL_VERSION, utc_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const STATE_SCHEMA: u32 = 1;

pub static MARK: [u8; MARK_LEN] = build_mark(STATE_SCHEMA as u16, PROTOCOL_VERSION);

pub const DEFAULT_STATE: &str = "/rth/instance.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InstanceState {
    pub state_schema: u32,
    pub instance_id: String,
    pub session_id: String,
    pub created_utc_ms: u64,

    #[serde(default)]
    pub written_by: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<serde_json::Value>,
}

pub struct Launch {
    pub state: PathBuf,

    pub managed: bool,
    pub manifest_sha256: Option<String>,
    pub deployment_id: Option<String>,
    pub exe: PathBuf,
}

impl Launch {
    pub fn from_env() -> Self {
        let state = std::env::var_os(ENV_STATE);
        Launch {
            managed: state.is_some(),
            state: state.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_STATE)),
            manifest_sha256: std::env::var(ENV_MANIFEST_SHA256).ok().filter(|s| !s.is_empty()),
            deployment_id: std::env::var(ENV_DEPLOYMENT_ID).ok().filter(|s| !s.is_empty()),
            exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/proc/self/exe")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Boot {
    pub deployment: Option<DeploymentIdentity>,
    pub instance: Option<InstanceIdentity>,

    pub notes: Vec<String>,

    pub created_utc_ms: Option<u64>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_err() {

        let seed = format!("{}-{}-{:?}", utc_ms(), std::process::id(), std::time::Instant::now());
        return sha256_hex(seed.as_bytes())[..n * 2].to_string();
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!(".{}.tmp-{}-{n}", path.file_name().and_then(|n| n.to_str()).unwrap_or("state"), std::process::id()));
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

pub fn check(launch: &Launch, journal_bytes: u64) -> Result<Boot, String> {
    let mut boot = Boot::default();
    boot.deployment = verify_deployment(launch)?;
    let written_by = boot.deployment.as_ref().map(|d| d.artifact_id.clone()).unwrap_or_else(|| "unmanaged".into());

    let state = match std::fs::read(&launch.state) {
        Ok(bytes) => {
            let mut st: InstanceState = serde_json::from_slice(&bytes)
                .map_err(|e| format!("instance state {} is unreadable ({e}); refusing to guess", launch.state.display()))?;
            if st.state_schema > STATE_SCHEMA {
                return Err(format!(
                    "state schema {} at {} is newer than this agent supports ({STATE_SCHEMA}); refusing to run (downgrade protection)",
                    st.state_schema,
                    launch.state.display()
                ));
            }
            if st.state_schema < STATE_SCHEMA && launch.managed {
                let from = st.state_schema;
                st.state_schema = STATE_SCHEMA;
                st.written_by = written_by.clone();
                write_atomic(&launch.state, &serde_json::to_vec_pretty(&st).expect("state serializes"))
                    .map_err(|e| format!("migrating instance state: {e}"))?;
                boot.notes.push(format!("instance state migrated from schema {from} to {STATE_SCHEMA}"));
            }
            Some(st)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if launch.managed {
                let st = InstanceState {
                    state_schema: STATE_SCHEMA,
                    instance_id: format!("inst-{}", random_hex(16)),
                    session_id: format!("sess-{}", random_hex(16)),
                    created_utc_ms: utc_ms(),
                    written_by,
                    origin: None,
                };
                write_atomic(&launch.state, &serde_json::to_vec_pretty(&st).expect("state serializes"))
                    .map_err(|e| format!("creating instance state: {e}"))?;
                boot.notes.push(format!("instance state created: {} / {}", st.instance_id, st.session_id));
                Some(st)
            } else {
                None
            }
        }
        Err(e) => return Err(format!("reading instance state {}: {e}", launch.state.display())),
    };
    boot.created_utc_ms = state.as_ref().map(|st| st.created_utc_ms);
    boot.instance = state.map(|st| InstanceIdentity {
        instance_id: st.instance_id,
        session_id: st.session_id,
        state_schema: st.state_schema,
        journal_bytes_at_boot: journal_bytes,

        context: CONTEXT_NONE.into(),
        context_restore: None,
    });
    Ok(boot)
}

fn verify_deployment(launch: &Launch) -> Result<Option<DeploymentIdentity>, String> {
    let Some(want) = &launch.manifest_sha256 else {
        return Ok(None);
    };
    let deployment_id = launch.deployment_id.clone().ok_or("a manifest was named without a deployment id")?;
    let dir = launch.exe.parent().ok_or("executable has no directory")?;
    if launch.exe.file_name().and_then(|n| n.to_str()) != Some(AGENT_FILE) {
        return Err(format!("executable {} is not an artifact's {AGENT_FILE}", launch.exe.display()));
    }
    let manifest_path = dir.join(MANIFEST_FILE);
    let bytes = std::fs::read(&manifest_path).map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let got = sha256_hex(&bytes);
    if &got != want {
        return Err(format!("manifest {} hashes to {got}, controller selected {want}", manifest_path.display()));
    }
    let m: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("manifest: {e}"))?;
    let artifact_id = m["artifact_id"].as_str().ok_or("manifest has no artifact_id")?.to_string();
    let exe_hash = sha256_file(&launch.exe).map_err(|e| format!("hashing own executable: {e}"))?;
    if exe_hash != artifact_id {
        return Err(format!("own executable hashes to {exe_hash}, manifest says {artifact_id}"));
    }
    if m["state_schema"].as_u64() != Some(STATE_SCHEMA as u64) || m["protocol_version"].as_u64() != Some(PROTOCOL_VERSION as u64) {
        return Err(format!(
            "manifest declares state_schema {} / protocol {}, this build is {STATE_SCHEMA} / {PROTOCOL_VERSION}",
            m["state_schema"], m["protocol_version"]
        ));
    }
    Ok(Some(DeploymentIdentity { deployment_id, artifact_id, manifest_sha256: got }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-instance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn concurrent_atomic_writes_of_one_file_each_land_whole() {

        let d = tmp("atomic-race");
        let path = d.join("context.json");
        let writers: Vec<_> = (0..8u8)
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || (0..25).map(|_| write_atomic(&path, &[b'a' + w; 4096])).collect::<std::io::Result<Vec<()>>>())
            })
            .collect();
        for w in writers {
            w.join().unwrap().expect("every write lands");
        }
        let last = std::fs::read(&path).unwrap();
        assert!(last.len() == 4096 && last.iter().all(|b| *b == last[0]), "one writer's whole file");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1, "no temp file is left behind");
    }

    fn launch(dir: &Path, managed: bool) -> Launch {
        Launch { state: dir.join("instance.json"), managed, manifest_sha256: None, deployment_id: None, exe: dir.join(AGENT_FILE) }
    }

    #[test]
    fn managed_boot_creates_identity_once_and_keeps_it() {
        let d = tmp("create");
        let first = check(&launch(&d, true), 0).unwrap();
        let a = first.instance.unwrap();
        let second = check(&launch(&d, true), 123).unwrap().instance.unwrap();
        assert_eq!((a.instance_id.clone(), a.session_id.clone()), (second.instance_id, second.session_id));
        assert_eq!(second.journal_bytes_at_boot, 123);
        assert_eq!(second.context, CONTEXT_NONE);
        assert!(a.instance_id.starts_with("inst-") && a.session_id.starts_with("sess-") && a.instance_id != a.session_id);
    }

    #[test]
    fn unmanaged_boot_never_creates_state() {
        let d = tmp("legacy");
        assert!(check(&launch(&d, false), 0).unwrap().instance.is_none());
        assert!(!d.join("instance.json").exists());
    }

    #[test]
    fn newer_state_schema_is_refused_and_left_untouched() {
        let d = tmp("downgrade");
        let newer = format!(r#"{{"state_schema":{},"instance_id":"i","session_id":"s","created_utc_ms":1}}"#, STATE_SCHEMA + 1);
        std::fs::write(d.join("instance.json"), &newer).unwrap();
        for managed in [true, false] {
            let err = check(&launch(&d, managed), 0).unwrap_err();
            assert!(err.contains("downgrade protection"), "{err}");
        }
        assert_eq!(std::fs::read_to_string(d.join("instance.json")).unwrap(), newer);
    }

    #[test]
    fn deployment_is_verified_against_own_executable_and_manifest() {
        let d = tmp("deploy");
        let exe = d.join(AGENT_FILE);
        std::fs::write(&exe, b"pretend binary").unwrap();
        let artifact = sha256_hex(b"pretend binary");
        let manifest = format!(r#"{{"artifact_id":"{artifact}","state_schema":{STATE_SCHEMA},"protocol_version":{PROTOCOL_VERSION}}}"#);
        std::fs::write(d.join(MANIFEST_FILE), &manifest).unwrap();
        let msha = sha256_hex(manifest.as_bytes());
        let mut l = launch(&d, true);
        l.manifest_sha256 = Some(msha.clone());
        l.deployment_id = Some("dep-1".into());
        let dep = check(&l, 0).unwrap().deployment.unwrap();
        assert_eq!((dep.artifact_id.as_str(), dep.manifest_sha256.as_str(), dep.deployment_id.as_str()), (artifact.as_str(), msha.as_str(), "dep-1"));

        l.manifest_sha256 = Some("0".repeat(64));
        assert!(check(&l, 0).unwrap_err().contains("controller selected"));

        l.manifest_sha256 = Some(msha);
        std::fs::write(&exe, b"another binary").unwrap();
        assert!(check(&l, 0).unwrap_err().contains("own executable"));
    }
}
