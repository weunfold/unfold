

use crate::keychain::KeyStore;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};

pub fn records_status(data: &Path) -> Value {
    let paths = crate::activation::Paths { data: data.into() };

    let mut errors = Vec::new();
    let mut read = |p: std::path::PathBuf| match crate::record::read(&p) {
        Ok(v) => v,
        Err(e) => {
            errors.push(format!("{}: {e}", p.display()));
            None
        }
    };
    let op = read(paths.op_record()).map(|r| {
        let steps: serde_json::Map<String, Value> = r["effects"].as_object().map(|m| m.iter().map(|(k, e)| (k.clone(), json!(if !e["outcome"].is_null() { "done" } else if e["release_intent"] == true { "may_have_run" } else { "recorded" }))).collect()).unwrap_or_default();
        json!({"op_id": r["op_id"], "steps": steps, "rollback": !r["rollback"].is_null()})
    });
    let state = read(paths.activation()).and_then(|a| a["activation_state"].as_str().map(str::to_string));
    let state = match (state, errors.is_empty()) {
        (Some(s), true) => s,
        (None, true) => "test_active".into(),
        (_, false) => "unreadable".into(),
    };
    json!({"activation_state": state, "admitted": state == "interim_active", "operation": op, "errors": errors})
}

pub fn setup_status(data: &Path, keys: &KeyStore) -> Value {
    let mut v = records_status(data);
    v["key_present"] = match keys.present() {
        Ok(p) => json!(p),
        Err(e) => json!({"error": e.to_string()}),
    };
    v
}

pub fn save_key(keys: &KeyStore, key: &str) -> Result<(), String> {
    keys.save(key).map_err(|e| e.to_string())
}

pub const WAITING_FOR_KEYCHAIN: &str = "Waiting for Keychain permission…";

pub fn read_key(keys: &KeyStore, before_prompt: impl FnOnce(&str)) -> Result<Option<String>, String> {
    if crate::keychain::GET_MAY_PROMPT {
        before_prompt(WAITING_FOR_KEYCHAIN);
    }
    keys.get().map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Stored,
    NeedsKey,
}

pub fn key_state(keys: &KeyStore) -> Result<KeyState, String> {
    keys.present().map(|p| if p { KeyState::Stored } else { KeyState::NeedsKey }).map_err(|e| e.to_string())
}

pub fn remove_key(keys: &KeyStore) -> Result<bool, String> {
    keys.remove().map_err(|e| e.to_string())
}

pub fn activate(sidecar: &Path, data: &Path, plan: &Path, product: &Path) -> Value {
    let out = Command::new(sidecar).arg("activate").arg(data).arg(plan).arg(product).env_clear().env("PATH", "/usr/bin:/bin").current_dir("/").stdin(Stdio::null()).stderr(Stdio::null()).output();
    match out {
        Ok(o) => serde_json::from_slice(&o.stdout).unwrap_or_else(|_| json!({"error": format!("the activation exited {:?} without a result", o.status.code())})),
        Err(e) => json!({"error": format!("the activation could not start: {e}")}),
    }
}

pub struct Admission {
    pub lease: crate::lease::Lease,
    pub record: Value,
}

pub fn admit(data: &Path, fresh_service: &Value) -> Result<Admission, String> {
    let paths = crate::activation::Paths { data: data.into() };
    let lease = crate::lease::Lease::acquire(&paths.op_lock(), crate::lease::Mode::Shared).map_err(|e| format!("not admitted: {e}"))?;
    let read = |p: std::path::PathBuf| crate::record::read(&p).map_err(|e| format!("not admitted: {}: {e}", p.display()));
    let a = read(paths.activation())?.filter(|a| a["activation_state"] == "interim_active").ok_or("not admitted: the activation is not interim_active")?;
    if let Some(op) = read(paths.op_record())?
        && op["op_id"] != a["op_id"]
    {
        return Err("not admitted: another operation is recorded and unfinished".into());
    }
    let root = std::path::PathBuf::from(a["instance_root"].as_str().ok_or("not admitted: no instance_root")?);
    let id = a["instance_id"].as_str().ok_or("not admitted: no instance_id")?;
    if crate::agentd_env::minted_instance(&root).map_err(|e| format!("not admitted: {e}"))?.as_deref() != Some(id) {
        return Err("not admitted: the instance's state is not the recorded instance (recovery)".into());
    }
    let strict = |p: std::path::PathBuf, what: &str| crate::record::read_json_strict(&p).map_err(|e| format!("not admitted: {e} (recovery)"))?.ok_or(format!("not admitted: no {what} (recovery)"));
    let rec = strict(crate::agentd_env::state_dir(&root).join("memory-binding-record.json"), "binding record")?;
    if rec["instance_id"] != json!(id) || rec["brain_id"] != a["brain"]["brain_id"] || rec["sentinel_ref"].is_null() || rec["sentinel_ref"] != a["binding"]["record"]["sentinel_ref"] {
        return Err("not admitted: the binding record is not the recorded one (recovery)".into());
    }
    let accepted = strict(root.join("memory-service.json"), "accepted memory service")?;
    if !crate::service::same_service(&accepted, fresh_service) {
        return Err("not admitted: the memory service changed since this instance accepted it; its new record needs review".into());
    }
    Ok(Admission { lease, record: a })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_save_and_remove_on_a_scratch_keychain_never_carry_key_bytes() {
        let d = std::env::temp_dir().join(format!("rth-seam-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let keys = KeyStore::file(&d.join("scratch.keychain-db"), "fixture-password-not-a-secret").unwrap();
        let key = "sk-test-fictional-seam-000000000000004";
        let s = setup_status(&d, &keys);
        assert_eq!((s["activation_state"].clone(), s["admitted"].clone(), s["key_present"].clone()), (json!("test_active"), json!(false), json!(false)));
        let bad = save_key(&keys, "sk test with spaces 00000000000").unwrap_err();
        assert!(!bad.contains("sk test"), "{bad}");
        for empty in ["", "   ", "\n\t "] {
            assert!(save_key(&keys, empty).is_err(), "an empty or whitespace-only key is refused: {empty:?}");
        }
        assert_eq!(key_state(&keys).unwrap(), KeyState::NeedsKey, "nothing was stored");
        save_key(&keys, key).unwrap();
        assert_eq!(key_state(&keys).unwrap(), KeyState::Stored);
        let s = setup_status(&d, &keys);
        assert_eq!(s["key_present"], true);
        assert!(!s.to_string().contains(key));
        let mut shown = None;
        assert_eq!(read_key(&keys, |m| shown = Some(m.to_string())).unwrap().as_deref(), Some(key));
        assert_eq!(shown.as_deref(), Some(WAITING_FOR_KEYCHAIN), "the waiting state is announced before the read");
        assert!(remove_key(&keys).unwrap());
        assert!(!remove_key(&keys).unwrap());
        assert_eq!(setup_status(&d, &keys)["key_present"], false);
    }
}
