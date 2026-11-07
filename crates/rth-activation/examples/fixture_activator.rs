use rth_activation::activation::{Cmd, Effects, Operation, Paths, Plan, Reconciled};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Fake {
    data: PathBuf,
}
impl Fake {
    fn sh(&self, step: &str, script: String) -> Cmd {
        let count = self.data.join(format!("count-{step}"));
        Cmd { argv: vec!["/bin/sh".into(), "-c".into(), format!("echo ran >> '{}'; {script}", count.display())], env: vec![("PATH".into(), "/usr/bin:/bin".into())], cwd: "/".into() }
    }
}
impl Effects for Fake {
    fn mint(&self, root: &Path) -> Cmd {
        self.sh("mint", format!("printf '{{\"instance_id\":\"inst-0123456789abcdef\"}}' > '{}'", root.join("agentd/state.json").display()))
    }
    fn brain_create(&self, tuple: &Value) -> Cmd {
        let reg = self.data.join("fake-registry.json");
        self.sh("brain", format!("printf '%s' '{}' > '{}'", json!({"brain_id": "brn_fakeint", "tuple": tuple}), reg.display()))
    }
    fn reconcile_brain(&self, tuple: &Value) -> Reconciled {
        match std::fs::read(self.data.join("fake-registry.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            Some(r) if &r["tuple"] == tuple => Reconciled::Done(json!({"brain_id": r["brain_id"]})),
            Some(_) => Reconciled::Unknown("a registration with another tuple".into()),
            None => Reconciled::Unknown("no registration matches the tuple".into()),
        }
    }
    fn bind(&self, root: &Path, brain: &Value, _nonce: &Path) -> Cmd {
        let rec = json!({"instance_id": "inst-0123456789abcdef", "brain_id": brain["brain_id"], "binding_rev": 1, "sentinel_ref": "mem_fakesentinel"});
        self.sh("bind", format!("printf '%s' '{rec}' > '{}'", root.join("agentd/memory-binding-record.json").display()))
    }
    fn verify(&self, root: &Path, brain: &Value, bound: &Value) -> Cmd {
        let v = json!({"bound": bound, "readiness": "attached: brain (binding rev 1)", "brain_id": brain["brain_id"], "instance_id": bound["instance_id"], "sentinel_ref": bound["sentinel_ref"], "agentd_sha256": bound["agentd_sha256"]});

        let linger = if self.data.join("verify-linger").exists() { "sleep 2 & " } else { "" };
        self.sh("verify", format!("{linger}printf '%s' '{v}' > '{}'", root.join("verify.json").display()))
    }
    fn agentd_sha256(&self) -> String {
        "fixture-agentd".into()
    }
    fn service_record(&self) -> Option<Value> {
        Some(json!({"label": "fixture-service", "version": "v3.2.2", "plist": "/fixture.plist", "program": "/fixture/.local/share/autopg/v3.2.2/autopg", "runtime_dir": "/fixture/rt", "socket_dir": "/fixture/rt/pgserve", "port": 5999, "pid": 2, "autopg_pid": 3}))
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let paths = Paths { data: PathBuf::from(&a[2]) };

    let bytes = std::fs::read(&a[3]).unwrap();
    let plan: Plan = match rth_activation::activation::load_plan(&bytes, &rth_activation::boot::sha256_hex(&bytes)) {
        Ok(p) => p,
        Err(e) => {
            println!("{}", json!({"error": e.to_string()}));
            std::process::exit(2)
        }
    };
    let mut op = match Operation::begin(&paths, Path::new(&a[1]), &plan) {
        Ok(op) => op,
        Err(e) => {
            println!("{}", json!({"error": e.to_string()}));
            std::process::exit(2)
        }
    };

    if let Some(ms) = std::fs::read_to_string(paths.data.join("quiesce-within-ms")).ok().and_then(|s| s.trim().parse().ok()) {
        op.quiesce_within = std::time::Duration::from_millis(ms);
    }
    let rollback = a.get(4).map(String::as_str) == Some("rollback");
    let hook = a.get(if rollback { 5 } else { 4 }).cloned();

    match hook.as_deref().and_then(|h| h.strip_prefix("pause:")) {
        Some(p) => op.pause = Some(p.to_string()),
        None => op.fault = hook,
    }
    let r = if rollback { op.rollback() } else { op.run(&Fake { data: paths.data.clone() }) };
    match r {
        Ok(v) => println!("{}", json!({"ok": v})),
        Err(e) => {
            println!("{}", json!({"error": e.to_string()}));
            std::process::exit(1)
        }
    }
}
