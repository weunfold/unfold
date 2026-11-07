

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use rth_tasks::ledger::{CallKey, Ledger, Recovered, TaskState, Update};
use serde_json::{Value, json};

pub const SCHEMA: &str = "rth-tasks/v1";

pub fn enabled() -> bool {
    std::env::var("RTH_DELEGATE").ok().or_else(|| option_env!("RTH_BUILD_DELEGATE").map(str::to_string)).as_deref() == Some("1")
}

pub fn tools_knob(value: Option<String>) -> Vec<rth_tasks::schema::Tool> {
    use rth_tasks::schema::Tool;
    if enabled() && value.as_deref() == Some("delegate") { vec![Tool::Delegate, Tool::CancelTask] } else { Vec::new() }
}

pub fn default_path(journal: &Path) -> PathBuf {
    journal.parent().unwrap_or(Path::new(".")).join("tasks.jsonl")
}

pub struct Tasks {
    ledger: Mutex<Ledger>,
    started: Instant,

    calls: Mutex<HashMap<String, String>>,
}

impl Tasks {
    pub fn open(path: &Path, instance_id: &str, boot: &str) -> std::io::Result<(Tasks, Recovered)> {
        let (ledger, rec) = Ledger::open(path, instance_id, boot)?;
        Ok((Tasks { ledger: Mutex::new(ledger), started: Instant::now(), calls: Mutex::new(HashMap::new()) }, rec))
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    pub fn tool_call(&self, instance_id: &str, generation: u64, call_id: &str, name: &str, args: &Value, origin_speaker: Option<&str>) -> std::io::Result<(Value, Option<TaskState>)> {
        let key = CallKey { instance_id: instance_id.into(), provider_generation: generation, call_id: call_id.into() };
        let a = self.ledger.lock().unwrap().tool_call_by(name, key, args, self.now(), origin_speaker)?;
        if let Some(id) = &a.task_id {
            self.calls.lock().unwrap().insert(call_id.into(), id.clone());
        }
        Ok((a.output, a.record))
    }

    pub fn output_sent(&self, call_id: &str) -> std::io::Result<Option<TaskState>> {
        let Some(id) = self.calls.lock().unwrap().get(call_id).cloned() else { return Ok(None) };
        self.ledger.lock().unwrap().output_sent(&id, self.now())
    }

    pub fn update(&self, update: &Value) -> std::io::Result<Result<TaskState, String>> {
        let u: Update = match serde_json::from_value(update.clone()) {
            Ok(u) => u,
            Err(e) => return Ok(Err(format!("malformed task update: {e}"))),
        };
        match self.ledger.lock().unwrap().update(&u, self.now())? {
            Some(rec) => Ok(Ok(rec)),
            None => Ok(Err(format!("unknown task {}", u.task_id))),
        }
    }

    pub fn executor_status(&self, s: &Value) -> std::io::Result<Result<Vec<TaskState>, String>> {
        let mut l = self.ledger.lock().unwrap();
        if s["state"].as_str() == Some("exited") {
            if s["proof_ok"].as_bool() != Some(true) {
                return Ok(Err("an exit receipt the host did not authenticate".into()));
            }
            let code = s["code"].as_i64().map(|c| c as i32);
            let signal = s["signal"].as_str().map(str::to_string);
            return Ok(match l.exit_verified(s["launch_id"].as_str().unwrap_or(""), code, signal)? {
                Ok(recs) => Ok(recs),
                Err(o) => Err(format!("exit receipt {o:?}")),
            });
        }
        let inc = match serde_json::from_value(s["incarnation"].clone()) {
            Ok(i) => i,
            Err(e) => return Ok(Err(format!("malformed executor status: {e}"))),
        };
        let r = l.executor_status(s["executor_id"].as_str().unwrap_or(""), &inc, s["seq"].as_u64().unwrap_or(0), s["available"].as_bool().unwrap_or(false), s["lease_ms"].as_u64().unwrap_or(0), self.now())?;

        let inventory = s["inventory"].as_bool() == Some(true);
        Ok(r.map(|()| if inventory { l.inventory(INVENTORY_RECENT) } else { Vec::new() }).map_err(str::to_string))
    }

    pub fn set_internal(&self, on: bool) {
        self.ledger.lock().unwrap().set_internal(on);
    }

    pub fn internal(&self) -> rth_tasks::ledger::Assignment {
        self.ledger.lock().unwrap().internal()
    }

    pub fn update_typed(&self, u: &Update) -> std::io::Result<Option<TaskState>> {
        self.ledger.lock().unwrap().update(u, self.now())
    }

    pub fn typed_call(&self, instance_id: &str, command_id: &str, name: &str, args: &Value, origin_speaker: Option<&str>) -> std::io::Result<(Value, Vec<TaskState>)> {
        let key = CallKey { instance_id: instance_id.into(), provider_generation: 0, call_id: format!("cmd:{command_id}") };
        let mut l = self.ledger.lock().unwrap();
        let now = self.now();
        let a = l.tool_call_by(name, key, args, now, origin_speaker)?;
        let mut recs: Vec<TaskState> = a.record.into_iter().collect();
        if let Some(id) = &a.task_id
            && let Some(r) = l.output_sent(id, now)?
        {
            recs.push(r);
        }
        Ok((a.output, recs))
    }

    pub fn origin_speaker(&self, task_id: &str) -> Option<Option<String>> {
        self.ledger.lock().unwrap().get(task_id).map(|t| t.origin_speaker.clone())
    }

    pub fn tool_call_receipt(&self, task_id: &str, receipt: &Value) -> std::io::Result<Result<(), &'static str>> {
        self.ledger.lock().unwrap().tool_call_receipt(task_id, receipt)
    }

    pub fn tick(&self) -> std::io::Result<Vec<TaskState>> {
        let now = self.now();
        self.ledger.lock().unwrap().tick(now)
    }
}

pub const INVENTORY_RECENT: usize = 20;

pub fn record_json(r: &TaskState) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or_else(|e| json!({"error": e.to_string()}));
    if let Some(o) = v.as_object_mut() {
        o.remove("origin_speaker");
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_frames_reach_the_ledger_and_an_unauthenticated_exit_is_refused() {
        let dir = std::env::temp_dir().join(format!("agentd-tasks-{}-{}", std::process::id(), rth_protocol::utc_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let (t, _) = Tasks::open(&dir.join("tasks.jsonl"), "inst", "b1").unwrap();
        let lease = json!({"executor_id": "dsh", "incarnation": {"launch_id": "L1", "pid": 1, "plugin_instance": "p"}, "seq": 1, "available": true, "lease_ms": 15000});
        assert_eq!(t.executor_status(&lease).unwrap(), Ok(vec![]));
        let (out, rec) = t.tool_call("inst", 1, "call-1", "delegate", &json!({"purpose": "report", "task": "relatório"}), None).unwrap();
        assert_eq!(out["status"], "accepted");
        let id = rec.unwrap().task_id;
        assert_eq!(t.output_sent("call-1").unwrap().unwrap().task_id, id);
        assert!(t.executor_status(&json!({"state": "exited", "launch_id": "L1"})).unwrap().is_err(), "proof_ok is required");
        let settled = t.executor_status(&json!({"state": "exited", "launch_id": "L1", "proof_ok": true, "code": 0})).unwrap().unwrap();
        assert_eq!(settled.len(), 1);
        assert!(t.update(&json!({"task_id": "nope", "executor_id": "dsh", "incarnation": {"launch_id": "L1", "pid": 1, "plugin_instance": "p"}, "kind": {"kind": "claim"}})).unwrap().is_err());
        assert!(t.update(&json!({"bad": true})).unwrap().is_err());
    }

    #[test]
    fn a_lease_beat_asking_for_the_inventory_is_answered_with_the_current_records_and_a_plain_beat_with_none() {
        let dir = std::env::temp_dir().join(format!("agentd-tasks-inv-{}-{}", std::process::id(), rth_protocol::utc_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let (t, _) = Tasks::open(&dir.join("tasks.jsonl"), "inst", "b1").unwrap();
        let inc = json!({"launch_id": "L1", "pid": 1, "plugin_instance": "p"});
        assert_eq!(t.executor_status(&json!({"executor_id": "dsh", "incarnation": inc, "seq": 1, "available": true, "lease_ms": 15000})).unwrap(), Ok(vec![]));
        let (_, rec) = t.tool_call("inst", 1, "call-1", "delegate", &json!({"purpose": "report", "task": "relatório"}), None).unwrap();
        let id = rec.unwrap().task_id;
        assert_eq!(t.executor_status(&json!({"executor_id": "dsh", "incarnation": inc, "seq": 2, "available": true, "lease_ms": 15000})).unwrap(), Ok(vec![]), "a plain beat carries nothing");
        let inv = t.executor_status(&json!({"executor_id": "dsh", "incarnation": inc, "seq": 3, "available": true, "lease_ms": 15000, "inventory": true})).unwrap().unwrap();
        assert_eq!(inv.iter().map(|r| r.task_id.clone()).collect::<Vec<_>>(), vec![id]);

        assert!(t.executor_status(&json!({"executor_id": "dsh", "incarnation": inc, "seq": 3, "available": true, "lease_ms": 15000, "inventory": true})).unwrap().is_err());
    }

    #[test]
    fn pv05_g9_a_tasks_speaker_is_kept_and_stays_off_the_wire() {
        let dir = std::env::temp_dir().join(format!("agentd-tasks-g9-{}-{}", std::process::id(), rth_protocol::utc_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let (t, _) = Tasks::open(&dir.join("tasks.jsonl"), "inst", "b1").unwrap();
        t.set_internal(true);
        let (out, recs) = t.typed_call("inst", "c1", "research", &json!({"task": "x"}), Some("person:p-lia")).unwrap();
        assert_eq!(out["status"], "accepted", "{out}");
        let id = recs[0].task_id.clone();
        assert_eq!(t.origin_speaker(&id), Some(Some("person:p-lia".to_string())));
        assert_eq!(t.origin_speaker("b1-t99"), None, "an unknown task");
        assert!(recs.iter().all(|r| record_json(r).get("origin_speaker").is_none()), "the speaker stays off the wire");
    }
}
