

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rth_protocol::deploy::{AGENT_FILE, MANIFEST_FILE, find_marks};

use crate::durable::{fsync_dir, sha256_hex, utc_ms};
use crate::manifest::{AgentFile, Attestation, BuildInfo, CompatReceipt, MANIFEST_VERSION, Manifest, Verified, verify_dir};
use crate::source::SourceIdentity;

pub struct ArtifactStore {
    root: PathBuf,
}

pub struct PublishOptions {
    pub label: Option<String>,
    pub target: String,
    pub profile: String,
    pub source: Option<SourceIdentity>,
}

impl Default for PublishOptions {
    fn default() -> Self {
        PublishOptions { label: None, target: "aarch64-unknown-linux-musl".into(), profile: "release".into(), source: None }
    }
}

pub struct Published {
    pub verified: Verified,

    pub created: bool,
}

impl ArtifactStore {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root).with_context(|| format!("artifact store {}", root.display()))?;
        Ok(ArtifactStore { root: root.canonicalize()? })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn publish(&self, binary: &Path, opts: PublishOptions, compat: Option<&CompatReceipt>) -> Result<Published> {
        let bytes = std::fs::read(binary).with_context(|| format!("reading {}", binary.display()))?;
        let id = sha256_hex(&bytes);
        let marks = find_marks(&bytes);
        let (state_schema, protocol, attestation) = match (marks.as_slice(), compat) {
            ([(schema, proto)], None) => (*schema as u32, *proto, Attestation::SelfVerified),
            ([_], Some(_)) => bail!("{id} carries its own mark; a compat receipt is only for legacy binaries"),
            ([], Some(r)) => {
                if r.artifact_id != id {
                    bail!("compat receipt is for {}, binary is {id}", r.artifact_id);
                }
                (r.state_schema, r.protocol_version, Attestation::ControllerVerified { compat_receipt_sha256: sha256_hex(&r.to_bytes()) })
            }
            ([], None) => bail!(
                "{} ({id}) has no RTH-MARK: it predates self-verification. Publish it only with a compat receipt for this exact hash",
                binary.display()
            ),
            (many, _) => bail!("{id} carries conflicting marks {many:?}"),
        };
        let dir = self.root.join(&id);
        if dir.exists() {
            let verified = verify_dir(&dir).map_err(|e| anyhow::anyhow!("existing artifact {id} fails verification, refusing to touch it: {e}"))?;
            return Ok(Published { verified, created: false });
        }
        let manifest = Manifest {
            manifest_version: MANIFEST_VERSION,
            artifact_id: id.clone(),
            agent: AgentFile { path: AGENT_FILE.into(), sha256: id.clone(), bytes: bytes.len() as u64 },
            protocol_version: protocol,
            state_schema,
            attestation,
            build: BuildInfo {
                label: opts.label,
                target: opts.target,
                profile: opts.profile,
                workspace_version: env!("CARGO_PKG_VERSION").into(),
                published_utc_ms: utc_ms(),
                source: opts.source,
            },
        };
        manifest.validate().map_err(|e| anyhow::anyhow!(e))?;
        let staging = self.root.join(format!(".staging-{}-{}-{}", &id[..12], std::process::id(), utc_ms()));
        std::fs::create_dir(&staging)?;
        let write = |name: &str, data: &[u8], mode: u32| -> Result<()> {
            let p = staging.join(name);
            std::fs::write(&p, data)?;
            std::fs::File::open(&p)?.sync_all()?;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode))?;
            Ok(())
        };
        write(AGENT_FILE, &bytes, 0o555)?;
        write(MANIFEST_FILE, &manifest.to_bytes(), 0o444)?;
        fsync_dir(&staging)?;
        match std::fs::rename(&staging, &dir) {
            Ok(()) => {}

            Err(_) if dir.exists() => {
                let _ = remove_tree(&staging);
                let verified = verify_dir(&dir).map_err(|e| anyhow::anyhow!(e))?;
                return Ok(Published { verified, created: false });
            }
            Err(e) => return Err(e.into()),
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))?;
        fsync_dir(&self.root)?;
        let verified = verify_dir(&dir).map_err(|e| anyhow::anyhow!(e))?;
        Ok(Published { verified, created: true })
    }

    pub fn get(&self, artifact_id: &str) -> Result<Verified> {
        if !crate::durable::is_sha256_hex(artifact_id) {
            bail!("{artifact_id:?} is not an artifact id");
        }
        verify_dir(&self.root.join(artifact_id)).map_err(|e| anyhow::anyhow!("artifact {artifact_id}: {e}"))
    }

    pub fn list(&self) -> Result<Vec<Result<Verified, String>>> {
        let mut out = Vec::new();
        let mut names: Vec<_> = std::fs::read_dir(&self.root)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        for p in names {
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.')) {
                out.push(verify_dir(&p));
            }
        }
        Ok(out)
    }
}

pub fn remove_tree(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        return Ok(());
    }
    for entry in walk(p) {
        let _ = std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::remove_dir_all(p)
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut out = vec![p.to_path_buf()];
    if p.is_dir() {
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                out.extend(walk(&e.path()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::deploy::build_mark;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-store-{name}-{}", std::process::id()));
        let _ = remove_tree(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_binary(dir: &Path, name: &str, body: &[u8], mark: bool) -> PathBuf {
        let mut bytes = body.to_vec();
        if mark {
            bytes.extend_from_slice(&build_mark(1, rth_protocol::PROTOCOL_VERSION));
        }
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn publish_is_content_addressed_immutable_and_idempotent() {
        let d = tmp("publish");
        let store = ArtifactStore::open(&d.join("store")).unwrap();
        let a = fake_binary(&d, "a", b"agent A", true);
        let first = store.publish(&a, PublishOptions { label: Some("A".into()), ..Default::default() }, None).unwrap();
        assert!(first.created);
        let m = &first.verified.manifest;
        assert_eq!((m.state_schema, m.protocol_version, &m.attestation), (1, rth_protocol::PROTOCOL_VERSION, &Attestation::SelfVerified));
        let manifest_before = std::fs::read(first.verified.dir.join(MANIFEST_FILE)).unwrap();

        let again = store.publish(&a, PublishOptions { label: Some("other".into()), ..Default::default() }, None).unwrap();
        assert!(!again.created);
        assert_eq!(std::fs::read(again.verified.dir.join(MANIFEST_FILE)).unwrap(), manifest_before);
        assert_eq!(again.verified.manifest_sha256, first.verified.manifest_sha256);

        let b = fake_binary(&d, "b", b"agent B", true);
        let pb = store.publish(&b, PublishOptions::default(), None).unwrap();
        assert_ne!(pb.verified.manifest.artifact_id, m.artifact_id);
        assert_eq!(store.list().unwrap().iter().filter(|r| r.is_ok()).count(), 2);

        let mode = std::fs::metadata(first.verified.dir.join(AGENT_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o222, 0);
        remove_tree(&d).unwrap();
    }

    #[test]
    fn tampering_is_detected_and_never_repaired() {
        let d = tmp("tamper");
        let store = ArtifactStore::open(&d.join("store")).unwrap();
        let a = fake_binary(&d, "a", b"agent A", true);
        let p = store.publish(&a, PublishOptions::default(), None).unwrap();
        let agent = p.verified.dir.join(AGENT_FILE);
        std::fs::set_permissions(&p.verified.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut bytes = std::fs::read(&agent).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&agent, &bytes).unwrap();
        let id = p.verified.manifest.artifact_id.clone();
        assert!(format!("{:#}", store.get(&id).unwrap_err()).contains("hashes to"));
        assert!(format!("{:#}", store.publish(&a, PublishOptions::default(), None).err().unwrap()).contains("refusing to touch"));
        std::fs::write(p.verified.dir.join("extra"), b"x").unwrap();
        assert!(format!("{:#}", store.get(&id).unwrap_err()).contains("unexpected entries"));
        remove_tree(&d).unwrap();
    }

    #[test]
    fn legacy_binaries_need_a_receipt_for_their_exact_hash() {
        let d = tmp("legacy");
        let store = ArtifactStore::open(&d.join("store")).unwrap();
        let old = fake_binary(&d, "old", b"legacy agent", false);
        let err = store.publish(&old, PublishOptions::default(), None).err().unwrap();
        assert!(format!("{err:#}").contains("no RTH-MARK"));
        let wrong = CompatReceipt::legacy(&"0".repeat(64), "test");
        assert!(format!("{:#}", store.publish(&old, PublishOptions::default(), Some(&wrong)).err().unwrap()).contains("receipt is for"));
        let id = sha256_hex(&std::fs::read(&old).unwrap());
        let receipt = CompatReceipt::legacy(&id, "test");
        let p = store.publish(&old, PublishOptions::default(), Some(&receipt)).unwrap();
        assert_eq!(
            p.verified.manifest.attestation,
            Attestation::ControllerVerified { compat_receipt_sha256: sha256_hex(&receipt.to_bytes()) }
        );

        let new = fake_binary(&d, "new", b"new agent", true);
        let nid = sha256_hex(&std::fs::read(&new).unwrap());
        assert!(store.publish(&new, PublishOptions::default(), Some(&CompatReceipt::legacy(&nid, "x"))).is_err());
        remove_tree(&d).unwrap();
    }
}
