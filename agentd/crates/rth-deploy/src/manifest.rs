

use std::path::{Path, PathBuf};

use rth_protocol::PROTOCOL_VERSION;
use rth_protocol::deploy::{AGENT_FILE, MANIFEST_FILE};
use serde::{Deserialize, Serialize};

use crate::durable::{is_sha256_hex, sha256_file, sha256_hex};
use crate::source::SourceIdentity;

pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub artifact_id: String,
    pub agent: AgentFile,
    pub protocol_version: u16,
    pub state_schema: u32,
    pub attestation: Attestation,
    pub build: BuildInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Attestation {

    SelfVerified,

    ControllerVerified { compat_receipt_sha256: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub target: String,
    pub profile: String,
    pub workspace_version: String,
    pub published_utc_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatReceipt {
    pub artifact_id: String,
    pub protocol_version: u16,
    pub state_schema: u32,
    pub reason: String,

    pub unavailable: Vec<String>,
    pub created_utc_ms: u64,
}

impl CompatReceipt {
    pub fn legacy(artifact_id: &str, reason: &str) -> Self {
        CompatReceipt {
            artifact_id: artifact_id.into(),
            protocol_version: PROTOCOL_VERSION,
            state_schema: 1,
            reason: reason.into(),
            unavailable: vec![
                "hello.deployment (self-verified artifact/manifest identity)".into(),
                "hello.instance (instance/session identity, context status)".into(),
                "hello.backend".into(),
                "agent-side downgrade refusal".into(),
            ],
            created_utc_ms: crate::durable::utc_ms(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec_pretty(self).expect("receipt serializes");
        v.push(b'\n');
        v
    }
}

impl Manifest {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec_pretty(self).expect("manifest serializes");
        v.push(b'\n');
        v
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.manifest_version != MANIFEST_VERSION {
            return Err(format!("manifest_version {} is not {MANIFEST_VERSION}", self.manifest_version));
        }
        if !is_sha256_hex(&self.artifact_id) {
            return Err(format!("artifact_id {:?} is not a lowercase sha256", self.artifact_id));
        }
        if self.agent.sha256 != self.artifact_id {
            return Err(format!("agent sha256 {} differs from artifact_id {}", self.agent.sha256, self.artifact_id));
        }
        if self.agent.path != AGENT_FILE {
            return Err(format!("agent path {:?} must be {AGENT_FILE:?}", self.agent.path));
        }
        if self.protocol_version == 0 || self.state_schema == 0 {
            return Err("protocol_version and state_schema must be positive".into());
        }
        if let Attestation::ControllerVerified { compat_receipt_sha256 } = &self.attestation {
            if !is_sha256_hex(compat_receipt_sha256) {
                return Err("controller_verified attestation needs a compat receipt hash".into());
            }
        }
        if self.build.target.is_empty() {
            return Err("build.target is empty".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Verified {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub manifest_sha256: String,
}

pub fn verify_dir(dir: &Path) -> Result<Verified, String> {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let bytes = std::fs::read(dir.join(MANIFEST_FILE)).map_err(|e| format!("{}: {e}", dir.join(MANIFEST_FILE).display()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| format!("manifest does not parse: {e}"))?;
    manifest.validate()?;
    if manifest.artifact_id != name {
        return Err(format!("directory {name} holds manifest for {}", manifest.artifact_id));
    }
    let mut entries: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
        .collect();
    entries.sort();
    let mut expected = vec![MANIFEST_FILE.to_string(), AGENT_FILE.to_string()];
    expected.sort();
    if entries != expected {
        return Err(format!("artifact {name} holds unexpected entries {entries:?}"));
    }
    let agent = dir.join(AGENT_FILE);
    let len = std::fs::metadata(&agent).map_err(|e| e.to_string())?.len();
    if len != manifest.agent.bytes {
        return Err(format!("agent is {len} bytes, manifest says {}", manifest.agent.bytes));
    }
    let h = sha256_file(&agent).map_err(|e| e.to_string())?;
    if h != manifest.artifact_id {
        return Err(format!("agent hashes to {h}, manifest says {}", manifest.artifact_id));
    }
    Ok(Verified { dir: dir.to_path_buf(), manifest_sha256: sha256_hex(&bytes), manifest })
}

pub fn compat(m: &Manifest, state_on_disk: Option<u32>) -> Result<(), String> {
    if m.protocol_version != PROTOCOL_VERSION {
        return Err(format!("artifact speaks protocol {}, controller speaks {PROTOCOL_VERSION}", m.protocol_version));
    }
    if let Some(on_disk) = state_on_disk {
        if on_disk > m.state_schema {
            return Err(format!(
                "artifact {} supports state schema {}, instance state is schema {on_disk}: refusing downgrade",
                &m.artifact_id[..12],
                m.state_schema
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(bin: &[u8]) -> Manifest {
        let id = sha256_hex(bin);
        Manifest {
            manifest_version: MANIFEST_VERSION,
            artifact_id: id.clone(),
            agent: AgentFile { path: AGENT_FILE.into(), sha256: id, bytes: bin.len() as u64 },
            protocol_version: PROTOCOL_VERSION,
            state_schema: 1,
            attestation: Attestation::SelfVerified,
            build: BuildInfo {
                label: None,
                target: "aarch64-unknown-linux-musl".into(),
                profile: "release".into(),
                workspace_version: "0.1.0".into(),
                published_utc_ms: 1,
                source: None,
            },
        }
    }

    #[test]
    fn validation_rejects_each_inconsistency() {
        let ok = sample(b"bin");
        ok.validate().unwrap();
        let cases: Vec<(Box<dyn Fn(&mut Manifest)>, &str)> = vec![
            (Box::new(|m| m.manifest_version = 9), "manifest_version"),
            (Box::new(|m| m.artifact_id = "XYZ".into()), "lowercase sha256"),
            (Box::new(|m| m.agent.sha256 = "0".repeat(64)), "differs from artifact_id"),
            (Box::new(|m| m.agent.path = "../rth-agentd".into()), "agent path"),
            (Box::new(|m| m.state_schema = 0), "positive"),
            (Box::new(|m| m.attestation = Attestation::ControllerVerified { compat_receipt_sha256: "no".into() }), "compat receipt"),
        ];
        for (mutate, want) in cases {
            let mut m = ok.clone();
            mutate(&mut m);
            let err = m.validate().unwrap_err();
            assert!(err.contains(want), "{err} should mention {want}");
        }
    }

    #[test]
    fn compat_refuses_newer_state_and_other_protocols() {
        let m = sample(b"bin");
        compat(&m, None).unwrap();
        compat(&m, Some(1)).unwrap();
        assert!(compat(&m, Some(2)).unwrap_err().contains("refusing downgrade"));
        let mut other = m.clone();
        other.protocol_version = PROTOCOL_VERSION + 1;
        assert!(compat(&other, None).unwrap_err().contains("protocol"));
    }
}
