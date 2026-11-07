

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::fixture::{Split, TaskRef};
use crate::registry::Registry;
use crate::scorer::{CaseResult, CaseStatus};
use crate::stats::{Summary, summarize};
use crate::timeline::Rec;
use crate::util::write_new;

pub const SCORECARD_FORMAT: &str = "rth-bench.scorecard/v1";
pub const MANIFEST_FORMAT: &str = "rth-bench.run-manifest/v1";

#[derive(Debug, Clone)]
pub struct Caps {
    pub backend: Option<String>,
    pub flags: BTreeMap<String, Option<bool>>,
}

impl Caps {

    pub fn parse(boot_line: &str) -> Self {
        let msg = serde_json::from_str::<Value>(boot_line).ok().and_then(|v| v.get("msg").and_then(Value::as_str).map(String::from));
        let msg = msg.unwrap_or_default();
        let backend = msg.split("backend ").nth(1).and_then(|s| s.split_whitespace().next()).map(String::from);
        let mut flags: BTreeMap<String, Option<bool>> = BTreeMap::new();
        if let Some(inner) = msg.split('(').nth(1).and_then(|s| s.split(')').next()) {
            for kv in inner.split_whitespace() {
                if let Some((k, v)) = kv.split_once('=') {
                    flags.insert(k.to_string(), v.parse::<bool>().ok());
                }
            }
        }

        let known = backend.is_some();
        flags.insert("text_in".into(), known.then_some(true));
        flags.insert("text_out".into(), known.then_some(true));
        Self { backend, flags }
    }

    pub fn supports(&self, cap: &str) -> Option<bool> {
        self.flags.get(cap).copied().flatten()
    }
}

fn status_str(s: CaseStatus) -> &'static str {
    match s {
        CaseStatus::Pass => "pass",
        CaseStatus::Fail => "fail",
        CaseStatus::Invalid => "invalid",
        CaseStatus::Indeterminate => "indeterminate",
    }
}

fn split_tally(cases: &[&CaseResult]) -> Value {
    let count = |s: CaseStatus| cases.iter().filter(|c| c.status == s).count();
    let n = cases.len();
    let (pass, fail, invalid, indeterminate) = (count(CaseStatus::Pass), count(CaseStatus::Fail), count(CaseStatus::Invalid), count(CaseStatus::Indeterminate));
    let status = if n == 0 {
        "missing"
    } else if fail > 0 {
        "fail"
    } else if invalid + indeterminate > 0 {
        "incomplete"
    } else {
        "pass"
    };
    json!({
        "cases": n, "pass": pass, "fail": fail, "invalid": invalid, "indeterminate": indeterminate,

        "score": if n == 0 { 0.0 } else { pass as f64 / n as f64 },
        "status": status,
        "failed_cases": cases.iter().filter(|c| c.status != CaseStatus::Pass).map(|c| json!({
            "scenario_id": c.scenario_id, "rep": c.rep, "status": status_str(c.status),
            "checks": c.checks.iter().filter(|k| matches!(k.outcome, crate::scorer::Outcome::Fail | crate::scorer::Outcome::Indeterminate)).map(|k| &k.id).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn envelope_value(s: &Summary, stat: &str) -> Option<f64> {
    match stat {
        "p50" => s.p50,
        "p95" => s.p95,
        "max" => s.max,
        "mean" => s.mean,
        _ => None,
    }
}

pub fn scorecard(reg: &Registry, cases: &[CaseResult], caps: &Caps, integrity: &[(String, bool, String)], splits_run: &[Split]) -> Value {
    let suite = &reg.suite;
    let selection = suite.aggregate.selection_split;
    let of = |task: &TaskRef, split: Split| -> Vec<&CaseResult> { cases.iter().filter(|c| &c.task == task && c.split == split).collect() };

    let mut tasks = Vec::new();
    let mut agg: BTreeMap<Split, f64> = BTreeMap::new();
    let mut floors_ok = true;
    let mut unsupported = Vec::new();
    for st in &suite.tasks {
        let def = reg.task(&st.task);
        let missing_caps: Vec<&String> = def.capabilities.iter().filter(|c| caps.supports(c) != Some(true)).collect();
        let supported = missing_caps.is_empty();
        if !supported {
            unsupported.push(json!({"task": st.task.to_string(), "missing_or_unknown_capabilities": missing_caps}));
        }
        let mut by_split = serde_json::Map::new();
        for split in [Split::Dev, Split::Promotion] {
            let mut tally = split_tally(&of(&st.task, split));
            if !supported {

                tally["status"] = json!("unsupported");
                tally["score"] = json!(0.0);
            }
            let score = tally["score"].as_f64().unwrap_or(0.0);
            *agg.entry(split).or_default() += st.weight * score;
            if split == selection && score < st.floor {
                floors_ok = false;
            }
            by_split.insert(split.as_str().into(), tally);
        }
        tasks.push(json!({
            "task": st.task.to_string(), "family": def.family, "purpose": def.purpose, "weight": st.weight, "floor": st.floor,
            "capabilities": def.capabilities, "supported": supported, "splits": by_split,
        }));
    }

    let probe_cases: Vec<&CaseResult> = cases.iter().filter(|c| c.split == Split::Probe).collect();
    let scenario_cases: Vec<&CaseResult> = cases.iter().filter(|c| c.split != Split::Probe).collect();
    let collect = |cs: &[&CaseResult], f: fn(&CaseResult) -> &Vec<f64>| -> Vec<f64> { cs.iter().flat_map(|c| f(c).iter().copied()).collect() };
    let probe_cancel = summarize(&collect(&probe_cases, |c| &c.metrics.cancel_to_terminal_ms), "ms");
    let scen_cancel = summarize(&collect(&scenario_cases, |c| &c.metrics.cancel_to_terminal_ms), "ms");
    let first_delta = summarize(&collect(&scenario_cases, |c| &c.metrics.first_delta_ms), "ms");
    let started = summarize(&collect(&scenario_cases, |c| &c.metrics.response_started_ms), "ms");
    let first_audio = summarize(&collect(&scenario_cases, |c| &c.metrics.first_audio_ms), "ms");
    let flush = summarize(&collect(&scenario_cases, |c| &c.metrics.flush_us), "us");
    let barge_terminal = summarize(&collect(&scenario_cases, |c| &c.metrics.barge_in_to_terminal_ms), "ms");

    let mut envelopes_ok = true;
    let envelopes: Vec<Value> = suite
        .envelopes
        .iter()
        .map(|e| {
            let s = match (e.metric.as_str(), e.source.as_str()) {
                ("cancel_to_terminal_ms", "probes") => Some(&probe_cancel),
                ("cancel_to_terminal_ms", "scenarios") => Some(&scen_cancel),
                ("first_delta_ms", "scenarios") => Some(&first_delta),
                ("first_audio_ms", "scenarios") => Some(&first_audio),
                ("flush_us", "scenarios") => Some(&flush),
                ("barge_in_to_terminal_ms", "scenarios") => Some(&barge_terminal),
                _ => None,
            };
            let value = s.and_then(|s| envelope_value(s, &e.stat));
            let pass = value.is_some_and(|v| v <= e.max);
            envelopes_ok &= pass;
            let n = s.map(|s| s.n).unwrap_or(0);
            json!({
                "metric": e.metric, "stat": e.stat, "source": e.source, "max": e.max, "unit": e.unit,
                "value": value, "n": n, "pass": pass,
                "note": if n < 59 { "point estimate only: a distribution-free 95% upper bound on p95 needs n >= 59" } else { "" },
            })
        })
        .collect();

    let mut probes = Vec::new();
    for p in &suite.probes {
        let cs: Vec<&CaseResult> = probe_cases.iter().copied().filter(|c| c.scenario_id == p.scenario_id).collect();
        let mut t = split_tally(&cs);
        t["task"] = json!(p.task.to_string());
        t["scenario_id"] = json!(p.scenario_id);
        t["repeats_required"] = json!(p.repeats);
        probes.push(t);
    }

    let vetoes: Vec<Value> = cases
        .iter()
        .flat_map(|c| {
            c.vetoes().map(move |k| {
                json!({"scenario_id": c.scenario_id, "split": c.split.as_str(), "rep": c.rep, "task": c.task.to_string(),
                       "check": k.id, "detail": k.detail, "evidence": k.evidence})
            })
        })
        .collect();
    let invalid: Vec<String> = cases.iter().filter(|c| c.status == CaseStatus::Invalid).map(|c| format!("{}#{}", c.scenario_id, c.rep)).collect();
    let indeterminate: Vec<String> = cases.iter().filter(|c| c.status == CaseStatus::Indeterminate).map(|c| format!("{}#{}", c.scenario_id, c.rep)).collect();

    let all_splits = [Split::Dev, Split::Promotion, Split::Probe].iter().all(|s| splits_run.contains(s));
    let probes_complete = suite.probes.iter().all(|p| probe_cases.iter().filter(|c| c.scenario_id == p.scenario_id).count() as u32 >= p.repeats);
    let mut rules: Vec<(String, bool, String)> = integrity.to_vec();
    rules.push(("all_splits_and_probes_executed".into(), all_splits && probes_complete, format!("splits run: {:?}", splits_run.iter().map(|s| s.as_str()).collect::<Vec<_>>())));
    rules.push(("no_critical_veto".into(), vetoes.is_empty(), format!("{} veto(es)", vetoes.len())));
    rules.push(("no_invalid_case".into(), invalid.is_empty(), format!("{} invalid", invalid.len())));
    rules.push(("capabilities_supported".into(), unsupported.is_empty(), format!("backend {:?}", caps.backend)));
    rules.push((format!("task_floors_met_on_{}", selection.as_str()), floors_ok, "every task score >= its floor".into()));
    rules.push(("latency_envelopes_met".into(), envelopes_ok, "see envelopes".into()));
    let eligible = rules.iter().all(|(_, ok, _)| *ok);

    json!({
        "scorecard_format": SCORECARD_FORMAT,
        "suite_id": suite.suite_id,
        "suite_epoch": suite.epoch,
        "suite_sha256": reg.suite_sha256,
        "policy_approval_id": suite.policy_approval_id,
        "scorer": {"id": reg.scorer().id, "version": reg.scorer().version},
        "candidate": {"backend": caps.backend, "capabilities": caps.flags},
        "selection_split": selection.as_str(),
        "aggregate": {
            "method": suite.aggregate.method,
            "dev": agg.get(&Split::Dev).copied().unwrap_or(0.0),
            "promotion": agg.get(&Split::Promotion).copied().unwrap_or(0.0),
        },
        "eligible": eligible,

        "activation": if suite.policy_approval_id == "unapproved-draft" { "dry-run recommendation only (policy unapproved)" } else { "policy-approved" },
        "eligibility": rules.iter().map(|(r, ok, d)| json!({"rule": r, "pass": ok, "detail": d})).collect::<Vec<_>>(),
        "vetoes": vetoes,
        "tasks": tasks,
        "probes": probes,
        "latency": {
            "probe_cancel_to_terminal_ms": probe_cancel,
            "scenario_cancel_to_terminal_ms": scen_cancel,
            "first_delta_ms": first_delta,
            "response_started_ms": started,
            "first_audio_ms": first_audio,
            "flush_us": flush,
            "barge_in_to_terminal_ms": barge_terminal,
            "clock": "runner monotonic clock (std::time::Instant); send stamped before write, receive stamped when read_frame returns",
        },
        "envelopes": envelopes,
        "statuses": {"invalid": invalid, "indeterminate": indeterminate, "unsupported": unsupported},
    })
}

pub fn write_jsonl(path: &Path, recs: &[Rec]) -> Result<()> {
    let mut out = String::new();
    for r in recs {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    write_new(path, out.as_bytes())
}

pub fn read_jsonl(path: &Path) -> Result<Vec<Rec>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    text.lines().filter(|l| !l.is_empty()).map(|l| serde_json::from_str(l).with_context(|| format!("{}: bad record", path.display()))).collect()
}

pub fn write_json(path: &Path, v: &impl serde::Serialize) -> Result<()> {
    write_new(path, (serde_json::to_string_pretty(v)? + "\n").as_bytes())
}

pub fn freeze(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            freeze(&path)?;
        } else {
            let mut perm = std::fs::metadata(&path)?.permissions();
            perm.set_readonly(true);
            std::fs::set_permissions(&path, perm)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_parse_the_agent_boot_line() {
        let line = r#"{"v":2,"id":"g:1","producer":"g","seq":1,"t_ns":1,"source":"guest","kind":"log","level":"info","msg":"agentd boot x · backend fake@v1 (cancel=true audio_in=false audio_out=false)"}"#;
        let c = Caps::parse(line);
        assert_eq!(c.backend.as_deref(), Some("fake@v1"));
        assert_eq!((c.supports("cancel"), c.supports("audio_in"), c.supports("text_in")), (Some(true), Some(false), Some(true)));
        let unknown = Caps::parse("");
        assert_eq!((unknown.backend.as_deref(), unknown.supports("text_in")), (None, None));
    }
}
