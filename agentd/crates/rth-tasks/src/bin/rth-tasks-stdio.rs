

use std::io::{BufRead, Write};

use rth_tasks::ledger::{CallKey, Incarnation, Ledger, Update};
use serde_json::{Value, json};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: rth-tasks-stdio <log> <instance> <boot>");
        std::process::exit(2);
    }
    let (mut l, rec) = Ledger::open(std::path::Path::new(&a[1]), &a[2], &a[3]).expect("open ledger");
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", json!({"ready": true, "recovered": {"revoked": rec.revoked, "unknown": rec.unknown, "tasks": rec.tasks}})).unwrap();
    out.flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("stdin");
        if line.trim().is_empty() {
            continue;
        }
        let op: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"error": format!("bad op: {e}")})).unwrap();
                out.flush().unwrap();
                continue;
            }
        };
        let now = op["now"].as_u64().unwrap_or(0);
        let ans = match op["op"].as_str().unwrap_or("") {
            "enabled" => {
                l.set_enabled(op["on"].as_bool().unwrap_or(true));
                json!({"ok": true})
            }
            "executor_status" => {
                let inc: Incarnation = serde_json::from_value(op["incarnation"].clone()).expect("incarnation");
                match l.executor_status(op["executor_id"].as_str().unwrap_or(""), &inc, op["seq"].as_u64().unwrap_or(0), op["available"].as_bool().unwrap_or(false), op["lease_ms"].as_u64().unwrap_or(0), now) {
                    Ok(Ok(())) => json!({"ok": true}),
                    Ok(Err(why)) => json!({"ok": false, "reason": why}),
                    Err(e) => json!({"error": e.to_string()}),
                }
            }
            "tool_call" => {
                let key: CallKey = serde_json::from_value(op["call_key"].clone()).expect("call_key");
                match l.tool_call(op["name"].as_str().unwrap_or(""), key, &op["args"], now) {
                    Ok(a) => json!({"output": a.output, "task_id": a.task_id, "record": a.record}),
                    Err(e) => json!({"error": e.to_string()}),
                }
            }
            "output_sent" => match l.output_sent(op["task_id"].as_str().unwrap_or(""), now) {
                Ok(r) => json!({"record": r}),
                Err(e) => json!({"error": e.to_string()}),
            },
            "update" => {
                let u: Update = match serde_json::from_value(op["update"].clone()) {
                    Ok(u) => u,
                    Err(e) => {
                        writeln!(out, "{}", json!({"error": format!("bad update: {e}")})).unwrap();
                        out.flush().unwrap();
                        continue;
                    }
                };
                match l.update(&u, now) {
                    Ok(r) => json!({"record": r}),
                    Err(e) => json!({"error": e.to_string()}),
                }
            }
            "tick" => match l.tick(now) {
                Ok(r) => json!({"records": r}),
                Err(e) => json!({"error": e.to_string()}),
            },
            "register_launch" => match l.register_launch(op["launch_id"].as_str().unwrap_or(""), op["secret_sha256"].as_str().unwrap_or("")) {
                Ok(o) => json!({"outcome": o}),
                Err(e) => json!({"error": e.to_string()}),
            },
            "exit_receipt" => match l.exit_receipt(op["launch_id"].as_str().unwrap_or(""), op["launch_proof"].as_str().unwrap_or(""), op["code"].as_i64().map(|c| c as i32), op["signal"].as_str().map(str::to_string)) {
                Ok(Ok(r)) => json!({"records": r}),
                Ok(Err(o)) => json!({"outcome": o}),
                Err(e) => json!({"error": e.to_string()}),
            },
            "get" => json!({"record": l.get(op["task_id"].as_str().unwrap_or(""))}),
            other => json!({"error": format!("unknown op {other:?}")}),
        };
        writeln!(out, "{ans}").unwrap();
        out.flush().unwrap();
    }
}
