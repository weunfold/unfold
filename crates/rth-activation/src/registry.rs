

use crate::activation::{Cmd, Reconciled};
use crate::product::{BrainRegistry, Pinned};
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn database_name_for(slug: &str) -> String {
    let mut out = String::new();
    for c in slug.to_lowercase().chars() {
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '_' };
        if !(c == '_' && out.ends_with('_')) {
            out.push(c);
        }
    }
    let name = format!("brain_{}", out.trim_matches('_'));
    name.chars().take(63).collect()
}

pub fn tuple_names_ok(tuple: &Value) -> bool {
    let ns = |s: &str| s.starts_with("brain_rth_") && s.len() <= 63 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    let (Some(ctl), Some(mem), Some(name)) = (tuple["control_db"].as_str(), tuple["memory_db"].as_str(), tuple["name"].as_str()) else { return false };
    ns(ctl) && ns(mem) && ctl != mem && mem == database_name_for(name) && name.starts_with("rth-interim-")
}

pub const REGISTRY_LIST_SHA256: &str = "0192c3d169ac0fa4d150bd979a3dfab138b2c4b03e7d9e655761c9ff54921fc1";
pub const CREATE_INTERIM_SHA256: &str = "b0f7966aaec959582300f70e5490090e33544e607350b57a27d6640c42473c89";
pub const INTERIM_DB_SHA256: &str = "7b1e16fc45363445c87aadbc383ab29fc43526b04e9873a2baca2a5cd127b030";

pub struct ListRegistry {
    pub bun: Pinned,

    pub list: Pinned,

    pub brain_root: PathBuf,

    pub create: Option<Pinned>,
    pub interim_db_sha256: String,

    pub socket: PathBuf,
    pub port: u16,

    pub home: PathBuf,
}

impl ListRegistry {
    fn query(&self, tuple: &Value) -> Result<(i32, Vec<Value>), String> {
        let bun = self.bun.check().map_err(|e| e.to_string())?;
        let list = self.list.check().map_err(|e| e.to_string())?;
        if !tuple_names_ok(tuple) {
            return Err("the tuple's database names are not this interim brain's".into());
        }
        let s = |k: &str| tuple[k].as_str().unwrap_or_default().to_string();
        let o = std::process::Command::new(bun)
            .arg(list)
            .args([s("control_db"), "--name".into(), s("name"), "--owner".into(), s("owner"), "--socket".into(), self.socket.display().to_string(), "--port".into(), self.port.to_string()])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("BRAIN_ROOT", &self.brain_root)
            .current_dir("/")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("the registry reader did not start: {e}"))?;
        let lines = String::from_utf8_lossy(&o.stdout).lines().map(|l| serde_json::from_str::<Value>(l).map_err(|_| "the registry reader printed a non-JSON line".to_string())).collect::<Result<Vec<_>, _>>()?;
        Ok((o.status.code().unwrap_or(-1), lines))
    }
}

impl BrainRegistry for ListRegistry {
    fn create(&self, tuple: &Value) -> Cmd {
        let fail = |why: String| Cmd { argv: vec!["/bin/sh".into(), "-c".into(), "printf '%s\\n' \"$1\" >&2; exit 70".into(), "sh".into(), why], env: vec![("PATH".into(), "/usr/bin:/bin".into())], cwd: "/".into() };
        let Some(c) = &self.create else { return fail("no pinned interim-brain creation command".into()) };
        let dir = c.path.parent().map(PathBuf::from).unwrap_or_default();
        let siblings = [Pinned { path: dir.join("interim-db.ts"), sha256: self.interim_db_sha256.clone() }, Pinned { path: dir.join("registry-list.ts"), sha256: self.list.sha256.clone() }];
        if let Some(e) = siblings.iter().find_map(|p| p.check().err()) {
            return fail(e.to_string());
        }
        match c.check() {
            Ok(p) if tuple_names_ok(tuple) => {
                let s = |k: &str| tuple[k].as_str().unwrap_or_default().to_string();
                Cmd {
                    argv: vec![p.display().to_string(), s("control_db"), s("name"), s("owner"), s("home"), "--socket".into(), self.socket.display().to_string(), "--port".into(), self.port.to_string()],
                    env: vec![("PATH".into(), "/usr/bin:/bin".into()), ("HOME".into(), self.home.display().to_string()), ("BRAIN_ROOT".into(), self.brain_root.display().to_string())],
                    cwd: "/".into(),
                }
            }
            Ok(_) => fail("the tuple's database names are not this interim brain's".into()),
            Err(e) => fail(e.to_string()),
        }
    }

    fn lookup(&self, tuple: &Value) -> Reconciled {
        let (code, lines) = match self.query(tuple) {
            Ok(r) => r,
            Err(e) => return Reconciled::Unknown(e),
        };
        let summary = lines.last().filter(|l| l["summary"] == true);
        let rows: Vec<&Value> = lines.iter().filter(|l| l["summary"] != true).collect();
        match (code, summary) {
            (0, Some(sm)) if sm["rows"] == 0 && rows.is_empty() => Reconciled::NotDone,
            (0, Some(sm)) if sm["rows"] == 1 && rows.len() == 1 => {
                let r = rows[0];
                let want = |k: &str| tuple[k].clone();

                let config = tuple["home"].as_str().map(|h| crate::record::read_json_strict(&std::path::Path::new(h).join(".brain/config.json")));
                let home_ok = matches!(config, Some(Ok(Some(c))) if c["database"] == want("control_db"));
                let exact = r["short_name"] == want("name")
                    && r["owner"] == want("owner")
                    && r["lifecycle"] == want("lifecycle")
                    && r["home_path"] == json!("")
                    && home_ok
                    && r["control_db"] == want("control_db")
                    && r["memory_db"] == want("memory_db")
                    && r["memory_db_checked"] == true
                    && r["memory_db_exists"] == true
                    && r["memory_db_holds_id"] == true
                    && r["archived_at"].is_null()
                    && r["id"].as_str().is_some_and(|id| id.starts_with("brn_"));
                if exact {
                    Reconciled::Done(json!({"brain_id": r["id"]}))
                } else {
                    Reconciled::Unknown("the one registration under this tuple differs from the recorded tuple (foreign)".into())
                }
            }
            (0, Some(_)) => Reconciled::Unknown("the registry reader's rows and summary disagree".into()),
            (3, _) => Reconciled::Unknown("the registry reader could not confirm (exit 3): uncertain, never created again".into()),
            (2, _) => Reconciled::Unknown("the registry reader refused its input (exit 2)".into()),
            (c, _) => Reconciled::Unknown(format!("the registry reader exited {c}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_names_match_brains_derivation_and_the_rth_namespace() {
        assert_eq!(database_name_for("rth-interim-inst-cc1d81f5a91ee1d96328cdae5c4eb8ab"), "brain_rth_interim_inst_cc1d81f5a91ee1d96328cdae5c4eb8ab");
        assert_eq!(database_name_for("rth"), "brain_rth", "the default brain uses the bare name");
        assert_eq!(database_name_for("--A..b__c--"), "brain_a_b_c");
        assert_eq!(database_name_for(&"x".repeat(80)).len(), 63);
        let t = |ctl: &str, mem: &str| json!({"name": "rth-interim-inst-ab12", "control_db": ctl, "memory_db": mem});
        assert!(tuple_names_ok(&t("brain_rth_interim_ab12_ctl", "brain_rth_interim_inst_ab12")));
        assert!(!tuple_names_ok(&t("brain_rth_interim_ab12_ctl", "brain_rth")), "the default database");
        assert!(!tuple_names_ok(&t("brain_default", "brain_rth_interim_inst_ab12")));
        assert!(!tuple_names_ok(&t("brain_rth_interim_inst_ab12", "brain_rth_interim_inst_ab12")));
    }
}
