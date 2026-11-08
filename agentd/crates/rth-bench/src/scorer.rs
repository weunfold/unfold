

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rth_protocol::{Envelope, Event, PROTOCOL_VERSION, ResponseStatus, Role};
use serde::{Deserialize, Serialize};

use crate::fixture::{Fixture, Op, Split, TaskDef, TaskRef};
use crate::timeline::{self as tl, Rec};

pub const SCORER_ID: &str = "rth-bench.text-contract";
pub const SCORER_VERSION: &str = "0.1.0";

pub const CRITICAL: &[&str] = &[
    "protocol_valid",
    "telemetry_complete",
    "single_terminal",
    "no_output_after_terminal",
    "no_foreign_output",
    "cancel_effective",
    "observer_cannot_command",
    "bounded_end",
];

pub const MANDATORY: &[&str] = &[
    "deadlines_met",
    "request_served",
    "decision_linked",
    "role_granted",
    "completed",
    "cancelled",
    "response_contract",
    "overlap",
    "rejections",
    "closed_by_agent",
    "no_response",
];

pub const CANCEL_DEADLINE_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Fail,

    Indeterminate,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub critical: bool,
    pub outcome: Outcome,
    pub detail: Vec<String>,

    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseStatus {
    Pass,
    Fail,

    Invalid,
    Indeterminate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseMetrics {

    pub first_delta_ms: Vec<f64>,

    pub response_started_ms: Vec<f64>,

    pub cancel_to_terminal_ms: Vec<f64>,

    pub overlap_observed: Vec<bool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub first_audio_ms: Vec<f64>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flush_us: Vec<f64>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub barge_in_to_terminal_ms: Vec<f64>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<AudioAccount>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AudioAccount {
    pub response_id: String,
    pub emitted_samples: Option<u64>,
    pub received_samples: u64,
    pub queued_samples: Option<u64>,
    pub played_samples: Option<u64>,
    pub played_after_clear: Option<u64>,

    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub scenario_id: String,
    pub task: TaskRef,
    pub split: Split,
    pub rep: u32,
    pub status: CaseStatus,
    pub score: f64,
    pub checks: Vec<Check>,
    pub metrics: CaseMetrics,
    pub invalid_reasons: Vec<String>,
}

impl CaseResult {
    pub fn vetoes(&self) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(|c| c.critical && c.outcome == Outcome::Fail)
    }
}

struct Checks(Vec<Check>);

impl Checks {
    fn new(fx: &Fixture) -> Self {
        let e = &fx.expect;
        let applies = |id: &str| match id {
            "role_granted" => !e.roles.is_empty(),
            "completed" => !e.completed.is_empty(),
            "cancelled" => !e.cancelled.is_empty(),
            "response_contract" => !e.contract.is_empty(),
            "overlap" => !e.overlap.is_empty(),
            "rejections" => !e.rejections.is_empty() || !e.rejected_total.is_empty(),
            "closed_by_agent" => !e.closed_by_agent.is_empty(),
            "no_response" => !e.no_response.is_empty(),
            _ => true,
        };
        let mk = |id: &&str, critical| Check {
            id: id.to_string(),
            critical,
            outcome: if applies(id) { Outcome::Pass } else { Outcome::NotApplicable },
            detail: vec![],
            evidence: vec![],
        };
        Self(CRITICAL.iter().map(|id| mk(id, true)).chain(MANDATORY.iter().map(|id| mk(id, false))).collect())
    }

    fn get(&mut self, id: &str) -> &mut Check {
        self.0.iter_mut().find(|c| c.id == id).expect("known check id")
    }

    fn fail(&mut self, id: &str, detail: impl Into<String>, evidence: &[&str]) {
        let c = self.get(id);
        c.outcome = Outcome::Fail;
        c.detail.push(detail.into());
        c.evidence.extend(evidence.iter().map(|s| s.to_string()));
    }

    fn indeterminate(&mut self, id: &str, detail: impl Into<String>) {
        let c = self.get(id);
        if c.outcome != Outcome::Fail {
            c.outcome = Outcome::Indeterminate;
        }
        c.detail.push(detail.into());
    }

    fn not_applicable(&mut self, id: &str, detail: &str) {
        let c = self.get(id);
        if c.outcome == Outcome::Pass && c.detail.is_empty() {
            c.outcome = Outcome::NotApplicable;
            c.detail.push(detail.into());
        }
    }
}

struct Input<'a> {
    label: &'a str,
    text: &'a str,
    conn: &'a str,
    t_ns: u64,
    env: &'a Envelope,
    request_id: &'a str,
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

pub fn score_any(fx: &Fixture, task: &TaskDef, recs: &[Rec]) -> CaseResult {
    if task.scorer.id == crate::audio::SCORER_ID { crate::audio::score(fx, task, recs) } else { score(fx, task, recs) }
}

pub fn score(fx: &Fixture, task: &TaskDef, recs: &[Rec]) -> CaseResult {
    let mut checks = Checks::new(fx);
    let mut metrics = CaseMetrics::default();
    let mut invalid = Vec::new();

    let rep = match recs.first() {
        Some(Rec::Header { rep, .. }) => *rep,
        _ => {
            invalid.push("timeline has no header".to_string());
            0
        }
    };

    let conns: BTreeSet<&str> = recs.iter().filter_map(Rec::conn).collect();
    let roles: BTreeMap<&str, Option<Role>> = conns.iter().map(|c| (*c, tl::granted_role(recs, c))).collect();
    let is_controller = |c: &str| roles.get(c).copied().flatten() == Some(Role::Controller);

    let mut inputs = Vec::new();
    for step in &fx.steps {
        if let Op::UserText { label, text } = &step.op {
            if let Some((conn, t_ns, Some(env))) = tl::sent_label(recs, label) {
                if let Event::UserText { request_id, .. } = &env.event {
                    inputs.push(Input { label, text, conn, t_ns, env, request_id });
                }
            }
        }
    }
    let input = |label: &str| inputs.iter().find(|i| i.label == label);
    let sent_requests: HashSet<&str> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::Sent { env: Some(Envelope { event: Event::UserText { request_id, .. }, .. }), .. } => Some(request_id.as_str()),
            _ => None,
        })
        .collect();

    let mut cleanup_ns = recs.last().map(Rec::t_ns).unwrap_or(0);
    for r in recs {
        if let Rec::Note { code, detail, t_ns, .. } = r {
            match code.as_str() {
                "attach_failed" | "invalid_target" | "isolation_failed" => invalid.push(format!("{code}: {detail}")),

                "trigger_timeout" | "wait_timeout" | "case_deadline" => checks.fail("deadlines_met", format!("{code}: {detail}"), &[]),
                "cleanup" => cleanup_ns = *t_ns,
                _ => {}
            }
        }
    }

    for r in recs {
        if let Rec::RecvInvalid { conn, line, error, .. } = r {
            let head: String = line.chars().take(120).collect();
            checks.fail("protocol_valid", format!("{conn}: invalid guest frame ({error})"), &[head.as_str()]);
        }
    }
    for c in &conns {
        let first = recs.iter().find_map(|r| r.recv_on(c));
        match first {
            None => {}
            Some((Envelope { event: Event::Hello { protocol, .. }, .. }, _)) if *protocol == PROTOCOL_VERSION => {}
            Some((env, _)) => checks.fail("protocol_valid", format!("{c}: first frame is not a v{PROTOCOL_VERSION} Hello"), &[&env.id]),
        }
    }
    for r in recs {
        if let Some((conn, env, _)) = r.recv() {
            match &env.event {
                Event::JournalFault { detail } => checks.fail("telemetry_complete", format!("{conn}: journal fault: {detail}"), &[&env.id]),
                Event::Log { level, msg } if level == "warn" && msg.contains("lagged") => {
                    invalid.push(format!("{conn}: runner connection lagged, observation incomplete: {msg}"));
                }
                _ => {}
            }
        }
    }

    for c in &conns {
        let mut done: HashSet<&str> = HashSet::new();
        for r in recs {
            let Some((env, _)) = r.recv_on(c) else { continue };
            match &env.event {
                Event::AgentDelta { response_id, .. } if done.contains(response_id.as_str()) => {
                    checks.fail("no_output_after_terminal", format!("{c}: delta for {response_id} after its terminal"), &[&env.id]);
                }
                Event::AgentDone { response_id, status } => {
                    if !done.insert(response_id) {
                        checks.fail("single_terminal", format!("{c}: second terminal ({status:?}) for {response_id}"), &[&env.id]);
                        checks.fail("no_output_after_terminal", format!("{c}: terminal for {response_id} after its terminal"), &[&env.id]);
                    }
                }
                _ => {}
            }
        }
    }

    let mut started_ids: HashSet<&str> = HashSet::new();
    for r in recs {
        if let Some((conn, env, _)) = r.recv() {
            if let Event::ResponseStarted { response_id, request_id, .. } = &env.event {
                started_ids.insert(response_id);
                if !sent_requests.contains(request_id.as_str()) {
                    checks.fail("no_foreign_output", format!("{conn}: response {response_id} for a request this case never sent ({request_id})"), &[&env.id]);
                }
            }
        }
    }
    for r in recs {
        if let Some((conn, env, _)) = r.recv() {
            if let Event::AgentDelta { response_id, .. } | Event::AgentDone { response_id, .. } = &env.event {
                if !started_ids.contains(response_id.as_str()) {
                    checks.fail("no_foreign_output", format!("{conn}: output for {response_id}, which never started in this case"), &[&env.id]);
                }
            }
        }
    }

    let cancels: Vec<(&str, u64, &str, &str)> = recs
        .iter()
        .filter_map(|r| match r {
            Rec::Sent { conn, t_ns, env: Some(env @ Envelope { event: Event::Cancel { response_id }, .. }), .. } => {
                Some((conn.as_str(), *t_ns, response_id.as_str(), env.id.as_str()))
            }
            _ => None,
        })
        .collect();
    let mut effective: HashSet<(&str, &str)> = HashSet::new();
    let mut active_cancels = 0;
    for &(conn, t_c, rid, cancel_id) in &cancels {
        if !is_controller(conn) {
            continue;
        }
        let started = tl::started_time(recs, conn, rid);
        let term = tl::first_terminal(recs, conn, rid);
        let active = started.is_some_and(|s| s < t_c) && term.is_none_or(|(t, _)| t > t_c);
        if !active {
            continue;
        }
        active_cancels += 1;
        let first_for_response = effective.insert((conn, rid));
        let closed_early = recs.iter().any(|r| {
            matches!(r, Rec::Closed { conn: c, t_ns, .. } if c == conn && *t_ns < t_c + CANCEL_DEADLINE_MS * 1_000_000)
        });
        match term {
            None if closed_early => checks.indeterminate("cancel_effective", format!("{conn}: connection closed before {rid}'s terminal")),
            None => checks.fail("cancel_effective", format!("{conn}: no terminal for {rid} within {CANCEL_DEADLINE_MS} ms of cancel"), &[cancel_id]),
            Some((t, ResponseStatus::Cancelled)) => {
                let lat = t - t_c;
                if lat > CANCEL_DEADLINE_MS * 1_000_000 {
                    checks.fail("cancel_effective", format!("{conn}: {rid} cancelled only after {:.1} ms", ms(lat)), &[cancel_id]);
                }
                if first_for_response {
                    metrics.cancel_to_terminal_ms.push(ms(lat));
                }
            }
            Some((_, status)) => {
                checks.fail("cancel_effective", format!("{conn}: {rid} ended {status:?} although cancelled mid-output"), &[cancel_id]);
            }
        }
    }
    if active_cancels == 0 {
        checks.not_applicable("cancel_effective", "no cancel overlapped live output");
    }

    let mut observer_inputs = 0;
    for r in recs {
        let Rec::Sent { conn, env: Some(env), .. } = r else { continue };
        if is_controller(conn) || roles.get(conn.as_str()).copied().flatten().is_none() {
            continue;
        }
        match &env.event {
            Event::UserText { request_id, .. } => {
                observer_inputs += 1;
                for r2 in recs {
                    let Some((c2, e2, _)) = r2.recv() else { continue };
                    match &e2.event {
                        Event::ResponseStarted { request_id: q, .. } if q == request_id => {
                            checks.fail("observer_cannot_command", format!("{c2}: observer {conn}'s input started a response"), &[&e2.id]);
                        }
                        Event::Decision { trigger_event, .. } if *trigger_event == env.id => {
                            checks.fail("observer_cannot_command", format!("{c2}: observer {conn}'s input produced a decision"), &[&e2.id]);
                        }
                        _ => {}
                    }
                }
            }
            Event::Cancel { response_id } => {
                observer_inputs += 1;
                let controller_cancelled = cancels.iter().any(|(c, _, rid, _)| rid == response_id && is_controller(c));
                let cancelled = recs.iter().filter_map(|r| r.recv()).find(|(_, e, _)| {
                    matches!(&e.event, Event::AgentDone { response_id: r, status: ResponseStatus::Cancelled } if r == response_id)
                });
                if let (Some((c2, e2, _)), false) = (cancelled, controller_cancelled) {
                    checks.fail("observer_cannot_command", format!("{c2}: observer {conn}'s cancel stopped {response_id}"), &[&e2.id]);
                }
            }
            _ => {}
        }
    }
    if observer_inputs == 0 {
        checks.not_applicable("observer_cannot_command", "no observer input in this scenario");
    }

    for c in &conns {
        let closed_before_end = recs.iter().any(|r| matches!(r, Rec::Closed { conn, t_ns, .. } if conn == c && *t_ns < cleanup_ns));
        if closed_before_end {
            continue;
        }
        let mut seen: Vec<&str> = Vec::new();
        for r in recs {
            if let Some((env, _)) = r.recv_on(c) {
                if let Event::ResponseStarted { response_id, .. } | Event::AgentDelta { response_id, .. } = &env.event {
                    if !seen.contains(&response_id.as_str()) {
                        seen.push(response_id);
                    }
                }
            }
        }
        for rid in seen {
            if tl::first_terminal(recs, c, rid).is_none() {
                checks.fail("bounded_end", format!("{c}: {rid} never reached a terminal"), &[rid]);
            }
        }
    }

    let e = &fx.expect;
    for i in &inputs {
        if !is_controller(i.conn) || e.no_response.iter().any(|l| l == i.label) {
            continue;
        }
        let unobserved_ok = e.unobserved_ok.iter().any(|l| l == i.label);
        let mut response_ids: Vec<&str> = recs
            .iter()
            .filter_map(|r| match r.recv() {
                Some((_, Envelope { event: Event::ResponseStarted { response_id, request_id, .. }, .. }, _)) if request_id == i.request_id => {
                    Some(response_id.as_str())
                }
                _ => None,
            })
            .collect();
        response_ids.sort();
        response_ids.dedup();
        let started_here = tl::started_on(recs, i.conn).into_iter().find(|(_, q, _)| q == i.request_id);
        match (response_ids.len(), &started_here) {
            (1, Some(_)) => {}
            (0, _) | (1, None) if unobserved_ok => continue,
            (n, _) => {
                checks.fail("request_served", format!("{}: {n} distinct responses, seen on sender: {}", i.label, started_here.is_some()), &[&i.env.id]);
                continue;
            }
        }
        let (rid, _, t_start) = started_here.expect("checked above");
        let decision = recs.iter().find_map(|r| match r.recv_on(i.conn) {
            Some((d @ Envelope { event: Event::Decision { trigger_event, .. }, .. }, t)) if *trigger_event == i.env.id => Some((d, t)),
            _ => None,
        });
        match decision {
            Some((d, t)) if d.cause.as_deref() == Some(i.env.id.as_str()) && t <= t_start => {}
            Some((d, _)) => checks.fail("decision_linked", format!("{}: decision missing cause or after ResponseStarted", i.label), &[&d.id]),
            None => checks.fail("decision_linked", format!("{}: no decision triggered by {}", i.label, i.env.id), &[&i.env.id]),
        }
        metrics.response_started_ms.push(ms(t_start - i.t_ns));
        if let Some((t, _)) = tl::deltas(recs, i.conn, &rid).first() {
            metrics.first_delta_ms.push(ms(t - i.t_ns));
        }
    }

    for (conn, want) in &e.roles {
        match roles.get(conn.as_str()).copied().flatten() {
            Some(got) if got == *want => {}
            got => checks.fail("role_granted", format!("{conn}: wanted {want:?}, granted {got:?}"), &[]),
        }
    }

    let resolve = |label: &str| -> Option<(&Input, String)> {
        let i = input(label)?;
        Some((i, tl::response_for(recs, i.request_id)?))
    };

    for label in &e.completed {
        match resolve(label) {
            None => checks.fail("completed", format!("{label}: never sent or never started"), &[]),
            Some((i, rid)) => match tl::first_terminal(recs, i.conn, &rid) {
                Some((_, ResponseStatus::Completed)) => {}
                other => checks.fail("completed", format!("{label}: terminal on {} is {other:?}", i.conn), &[&rid]),
            },
        }
    }

    for label in &e.cancelled {
        let Some((i, rid)) = resolve(label) else {
            checks.fail("cancelled", format!("{label}: never sent or never started"), &[]);
            continue;
        };
        if !effective.iter().any(|(c, r)| *c == i.conn && *r == rid) {
            checks.indeterminate("cancelled", format!("{label}: no cancel landed during live output"));
            continue;
        }
        match tl::first_terminal(recs, i.conn, &rid) {
            Some((_, ResponseStatus::Cancelled)) => {}
            other => checks.fail("cancelled", format!("{label}: terminal on {} is {other:?}", i.conn), &[&rid]),
        }
    }

    for (label, name) in &e.contract {
        let Some(template) = task.response_contract.get(name) else {
            checks.fail("response_contract", format!("{label}: task declares no contract {name:?}"), &[]);
            continue;
        };
        let Some((i, rid)) = resolve(label) else {
            checks.fail("response_contract", format!("{label}: never sent or never started"), &[]);
            continue;
        };
        let text: String = tl::deltas(recs, i.conn, &rid).into_iter().map(|(_, t)| t).collect();
        let completed = matches!(tl::first_terminal(recs, i.conn, &rid), Some((_, ResponseStatus::Completed)));
        if !completed || !matches_template(template, i.text, &text) {
            checks.fail("response_contract", format!("{label}: {name} contract not met (completed={completed}): {text:?}"), &[&rid]);
        }
    }

    for (a, b) in &e.overlap {
        let (Some((ia, rid_a)), Some((ib, rid_b))) = (resolve(a), resolve(b)) else {
            checks.fail("overlap", format!("{a}/{b}: never sent or never started"), &[]);
            continue;
        };
        let started_a = tl::started_time(recs, ia.conn, &rid_a);
        let done_a = tl::first_terminal(recs, ia.conn, &rid_a).map(|(t, _)| t);
        let started_b = tl::started_time(recs, ib.conn, &rid_b);
        let (Some(sa), Some(da), Some(sb)) = (started_a, done_a, started_b) else {
            checks.fail("overlap", format!("{a}/{b}: lifecycle incomplete"), &[&rid_a, &rid_b]);
            continue;
        };
        let real = sa < ib.t_ns && ib.t_ns < da;
        metrics.overlap_observed.push(real);
        if !real {
            checks.indeterminate("overlap", format!("{b} was not sent during {a}'s output"));
        } else if sb > da {
            checks.fail("overlap", format!("{b} started only after {a} ended: responses were serialized"), &[&rid_b]);
        }
    }

    let mut used: HashSet<(String, u64)> = HashSet::new();
    for want in &e.rejections {
        let cause_id = match &want.cause {
            Some(label) => match tl::sent_label(recs, label) {
                Some((_, _, Some(env))) => Some(env.id.clone()),
                _ => {
                    checks.fail("rejections", format!("{}: cause label {label} was never sent as an envelope", want.conn), &[]);
                    continue;
                }
            },
            None => None,
        };
        let hit = tl::rejected_on(recs, &want.conn).into_iter().find(|(cause, reason, t)| {
            !used.contains(&(want.conn.clone(), *t))
                && cause_id.as_ref().is_none_or(|id| cause.as_ref() == Some(id))
                && want.reason_contains.as_ref().is_none_or(|s| reason.contains(s.as_str()))
        });
        match hit {
            Some((_, _, t)) => {
                used.insert((want.conn.clone(), t));
            }
            None => checks.fail("rejections", format!("{}: no Rejected matching {want:?}", want.conn), &[]),
        }
    }
    for (conn, n) in &e.rejected_total {
        let got = tl::rejected_on(recs, conn).len();
        if got != *n {
            checks.fail("rejections", format!("{conn}: {got} Rejected, expected exactly {n}"), &[]);
        }
    }

    for conn in &e.closed_by_agent {
        if tl::closed_by_peer(recs, conn).is_none() {
            checks.fail("closed_by_agent", format!("{conn}: agent never closed the connection"), &[]);
        }
    }

    for label in &e.no_response {
        let Some(i) = input(label) else {
            checks.fail("no_response", format!("{label}: never sent"), &[]);
            continue;
        };
        for r in recs {
            let Some((c, env, _)) = r.recv() else { continue };
            match &env.event {
                Event::ResponseStarted { request_id, .. } if request_id == i.request_id => {
                    checks.fail("no_response", format!("{c}: {label} started a response"), &[&env.id]);
                }
                Event::Decision { trigger_event, .. } if *trigger_event == i.env.id => {
                    checks.fail("no_response", format!("{c}: {label} produced a decision"), &[&env.id]);
                }
                _ => {}
            }
        }
    }

    let checks = checks.0;
    let any = |o: Outcome, critical: Option<bool>| checks.iter().any(|c| c.outcome == o && critical.is_none_or(|k| c.critical == k));
    let status = if any(Outcome::Fail, Some(true)) {
        CaseStatus::Fail
    } else if !invalid.is_empty() {
        CaseStatus::Invalid
    } else if any(Outcome::Fail, None) {
        CaseStatus::Fail
    } else if any(Outcome::Indeterminate, None) {
        CaseStatus::Indeterminate
    } else {
        CaseStatus::Pass
    };
    CaseResult {
        scenario_id: fx.scenario_id.clone(),
        task: fx.task.clone(),
        split: fx.split,
        rep,
        status,
        score: if status == CaseStatus::Pass { 1.0 } else { 0.0 },
        checks,
        metrics,
        invalid_reasons: invalid,
    }
}

pub fn matches_template(template: &str, input: &str, text: &str) -> bool {
    let filled = template.replace("{input_trimmed}", input.trim());
    let mut parts = filled.split("{uint}");
    let Some(first) = parts.next() else { return false };
    let Some(mut rest) = text.strip_prefix(first) else { return false };
    for part in parts {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return false;
        }
        let Some(r) = rest[digits..].strip_prefix(part) else { return false };
        rest = r;
    }
    rest.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::ClosedBy;
    use rth_protocol::Source;

    const GREETING: &str = "hi! I'm alive inside the rth-core microVM, up {uint}s. Every event you see is in my journal. Press Ctrl-X while I talk to interrupt me.";
    const ECHO: &str = "I heard \"{input_trimmed}\". My real model isn't wired yet; I'm the fake backend.";

    fn task() -> TaskDef {
        let mut t: TaskDef = serde_json::from_value(serde_json::json!({
            "task_format": "rth-bench.task/v1", "task_id": "t", "version": "1", "purpose": "", "family": "",
            "capabilities": [], "input_protocol": {"name": "rth-protocol", "version": 2, "transport": "tcp", "feed_mode": "paced_causal"},
            "provenance": "", "license": "", "fixture_manifest_sha256": "", "fixtures": [],
            "scorer": {"id": SCORER_ID, "version": SCORER_VERSION}, "output_schema": "", "metrics": [],
            "normalization": "", "missing_timeout_error_policy": "", "critical_checks": [], "mandatory_checks": [],
            "variant_labels": []
        }))
        .unwrap();
        t.response_contract.insert("greeting".into(), GREETING.into());
        t.response_contract.insert("echo".into(), ECHO.into());
        t
    }

    fn fixture(texts: &[(&str, &str)], expect: serde_json::Value) -> Fixture {
        let steps: Vec<_> = texts
            .iter()
            .map(|(l, t)| {
                let conn = if l.starts_with('o') { "c2" } else { "c1" };
                serde_json::json!({"conn": conn, "op": "user_text", "label": l, "text": t})
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "fixture_format": "rth-bench.fixture/v1", "scenario_id": "s", "group_id": "g",
            "task": {"id": "t", "version": "1"}, "split": "dev", "feed_mode": "paced_causal", "reactive": false,
            "description": "", "deadline_ms": 5000, "settle_ms": 400, "steps": steps, "expect": expect
        }))
        .unwrap()
    }

    struct Tl {
        recs: Vec<Rec>,
        seq: u64,
    }

    impl Tl {
        fn new() -> Self {
            let header = Rec::Header {
                scenario_id: "s".into(),
                task: TaskRef { id: "t".into(), version: "1".into() },
                split: Split::Dev,
                rep: 1,
                fixture_sha256: "x".into(),
                run_tag: "run".into(),
                origin_ns: 0,
            };
            Self { recs: vec![header], seq: 0 }
        }

        fn env(&mut self, source: Source, producer: &str, event: Event, cause: Option<String>) -> Envelope {
            self.seq += 1;
            Envelope { v: PROTOCOL_VERSION, id: format!("{producer}:{}", self.seq), producer: producer.into(), seq: self.seq, t_ns: 0, source, cause, received: None, event }
        }

        fn recv(&mut self, conn: &str, ms: u64, event: Event, cause: Option<String>) -> String {
            let env = self.env(Source::Guest, "guest", event, cause);
            let id = env.id.clone();
            self.recs.push(Rec::Recv { t_ns: ms * 1_000_000, conn: conn.into(), env });
            id
        }

        fn sent(&mut self, conn: &str, ms: u64, label: Option<&str>, event: Event) -> String {
            let env = self.env(Source::Host, conn, event, None);
            let id = env.id.clone();
            self.recs.push(Rec::Sent { t_ns: ms * 1_000_000, conn: conn.into(), step: None, label: label.map(String::from), env: Some(env), raw: None, raw_bytes: None });
            id
        }

        fn attach(&mut self, conn: &str, ms: u64, granted: Role) -> &mut Self {
            let hello = Event::Hello { protocol: PROTOCOL_VERSION, agent: "a".into(), version: "0".into(), boot_id: "b".into(), utc_ms_at_origin: 0, journal: "j".into(), backend: None, deployment: None, instance: None, command_generation: None };
            self.recv(conn, ms, hello, None);
            let aid = format!("{conn}-a1");
            self.sent(conn, ms, None, Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: aid.clone(), role: Role::Controller });
            self.recv(conn, ms, Event::Ready { attachment_id: aid, role: granted }, None);
            self
        }

        fn ask(&mut self, conn: &str, ms: u64, label: &str, text: &str) -> String {
            let request_id = tl::request_id("run", "s", 1, label);
            let sent = self.sent(conn, ms, Some(label), Event::UserText { request_id: request_id.clone(), text: text.into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
            let decision = Event::Decision {
                decision_id: "d".into(), episode_id: conn.into(), trigger_event: sent.clone(), context_rev: 1,
                candidate_schema: "x".into(), candidates: vec![], selected: "respond".into(), policy: "p".into(),
                latency_ns: 0, training_eligibility: "unknown".into(), utterance_id: None,
            };
            self.recv(conn, ms + 1, decision, Some(sent));
            let rid = format!("r-{label}");
            self.recv(conn, ms + 1, Event::ResponseStarted { response_id: rid.clone(), request_id, utterance_id: None }, None);
            rid
        }

        fn stream(&mut self, conn: &str, from_ms: u64, rid: &str, text: &str) -> u64 {
            let mut t = from_ms;
            for w in text.split_inclusive(' ') {
                t += 30;
                self.recv(conn, t, Event::AgentDelta { response_id: rid.into(), text: w.into() }, None);
            }
            t
        }

        fn done(&mut self, conn: &str, ms: u64, rid: &str, status: ResponseStatus) -> &mut Self {
            self.recv(conn, ms, Event::AgentDone { response_id: rid.into(), status }, None);
            self
        }

        fn cancel(&mut self, conn: &str, ms: u64, rid: &str) -> &mut Self {
            self.sent(conn, ms, None, Event::Cancel { response_id: rid.into() });
            self
        }

        fn note(&mut self, ms: u64, code: &str) -> &mut Self {
            self.recs.push(Rec::Note { t_ns: ms * 1_000_000, conn: None, code: code.into(), detail: String::new() });
            self
        }

        fn run(&mut self, fx: &Fixture) -> CaseResult {
            tl::sort(&mut self.recs);
            score(fx, &task(), &self.recs)
        }
    }

    fn outcome(r: &CaseResult, id: &str) -> Outcome {
        r.checks.iter().find(|c| c.id == id).unwrap().outcome
    }

    fn greeting_text() -> String {
        GREETING.replace("{uint}", "12")
    }

    #[test]
    fn templates_match_the_declared_fake_contract() {
        assert!(matches_template(GREETING, "hi", &greeting_text()));
        assert!(!matches_template(GREETING, "hi", &GREETING.replace("{uint}", "")));
        assert!(!matches_template(GREETING, "hi", &format!("{} extra", greeting_text())));
        assert!(matches_template(ECHO, "  what time is it? ", "I heard \"what time is it?\". My real model isn't wired yet; I'm the fake backend."));
        assert!(!matches_template(ECHO, "abc", "I heard \"abd\". My real model isn't wired yet; I'm the fake backend."));
    }

    #[test]
    fn clean_greeting_lifecycle_passes() {
        let fx = fixture(&[("q1", "hi")], serde_json::json!({"roles": {"c1": "controller"}, "completed": ["q1"], "contract": {"q1": "greeting"}}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 50, "q1", "hi");
        let end = t.stream("c1", 51, &rid, &greeting_text());
        t.done("c1", end + 1, &rid, ResponseStatus::Completed).note(end + 400, "cleanup");
        let r = t.run(&fx);
        assert_eq!(r.status, CaseStatus::Pass, "{:#?}", r.checks);
        assert_eq!(r.score, 1.0);
        assert_eq!(r.metrics.first_delta_ms, vec![31.0]);
        assert_eq!(outcome(&r, "cancel_effective"), Outcome::NotApplicable);
    }

    #[test]
    fn delta_after_cancelled_terminal_is_a_stale_output_veto() {
        let fx = fixture(&[("q1", "hi")], serde_json::json!({"cancelled": ["q1"]}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 50, "q1", "hi");
        t.stream("c1", 51, &rid, "hi! I'm ");
        t.cancel("c1", 120, &rid).done("c1", 122, &rid, ResponseStatus::Cancelled);
        t.recv("c1", 125, Event::AgentDelta { response_id: rid.clone(), text: "alive ".into() }, None);
        t.note(600, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "no_output_after_terminal"), Outcome::Fail);
        assert_eq!(outcome(&r, "cancelled"), Outcome::Pass);
        assert_eq!(r.status, CaseStatus::Fail);
        assert_eq!(r.vetoes().count(), 1);
        assert_eq!(r.metrics.cancel_to_terminal_ms, vec![2.0]);
    }

    #[test]
    fn second_terminal_after_completion_is_a_veto() {
        let fx = fixture(&[("q1", "ok")], serde_json::json!({}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 10, "q1", "ok");
        t.done("c1", 300, &rid, ResponseStatus::Completed).cancel("c1", 301, &rid).done("c1", 302, &rid, ResponseStatus::Cancelled);
        t.note(700, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "single_terminal"), Outcome::Fail);

        assert_eq!(outcome(&r, "cancel_effective"), Outcome::NotApplicable);
        assert_eq!(r.status, CaseStatus::Fail);
    }

    #[test]
    fn ignored_cancel_is_a_veto() {
        let fx = fixture(&[("q1", "hi")], serde_json::json!({"cancelled": ["q1"]}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 50, "q1", "hi");
        t.cancel("c1", 100, &rid);
        let end = t.stream("c1", 100, &rid, &greeting_text());
        t.done("c1", end + 1, &rid, ResponseStatus::Completed).note(end + 400, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "cancel_effective"), Outcome::Fail);
        assert_eq!(outcome(&r, "cancelled"), Outcome::Fail);
        assert_eq!(r.status, CaseStatus::Fail);
    }

    #[test]
    fn cancel_that_missed_the_output_is_indeterminate_not_failed() {
        let fx = fixture(&[("q1", "ok")], serde_json::json!({"cancelled": ["q1"]}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 10, "q1", "ok");
        t.done("c1", 200, &rid, ResponseStatus::Completed).cancel("c1", 250, &rid).note(700, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "cancelled"), Outcome::Indeterminate);
        assert_eq!(r.status, CaseStatus::Indeterminate);
    }

    #[test]
    fn observer_input_that_starts_a_response_is_a_veto() {
        let fx = fixture(&[("q1", "hi"), ("o1", "let me in")], serde_json::json!({"roles": {"c2": "observer"}, "no_response": ["o1"]}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller).attach("c2", 5, Role::Observer);
        let rid = t.ask("c1", 50, "q1", "hi");
        t.done("c1", 400, &rid, ResponseStatus::Completed);
        let orid = t.ask("c2", 100, "o1", "let me in");
        t.done("c2", 300, &orid, ResponseStatus::Completed).note(800, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "observer_cannot_command"), Outcome::Fail);
        assert_eq!(outcome(&r, "no_response"), Outcome::Fail);
        assert_eq!(outcome(&r, "role_granted"), Outcome::Pass);
    }

    #[test]
    fn observer_rejection_passes_and_is_matched_by_cause() {
        let fx = fixture(&[("q1", "hi"), ("o1", "let me in")], serde_json::json!({
            "roles": {"c1": "controller", "c2": "observer"}, "no_response": ["o1"],
            "rejections": [{"conn": "c2", "cause": "o1"}]
        }));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller).attach("c2", 5, Role::Observer);
        let rid = t.ask("c1", 50, "q1", "hi");
        let request_id = tl::request_id("run", "s", 1, "o1");
        let sent = t.sent("c2", 100, Some("o1"), Event::UserText { request_id, text: "let me in".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });

        t.recv("c1", 101, Event::Rejected { reason: "c2 is an observer".into() }, Some(sent.clone()));
        t.recv("c2", 101, Event::Rejected { reason: "c2 is an observer".into() }, Some(sent));
        t.done("c1", 400, &rid, ResponseStatus::Completed).done("c2", 400, &rid, ResponseStatus::Completed).note(800, "cleanup");
        let r = t.run(&fx);
        assert_eq!(r.status, CaseStatus::Pass, "{:#?}", r.checks);
        assert_eq!(outcome(&r, "observer_cannot_command"), Outcome::Pass);
    }

    #[test]
    fn output_for_an_unknown_response_is_foreign() {
        let fx = fixture(&[], serde_json::json!({}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        t.recv("c1", 20, Event::AgentDelta { response_id: "leftover".into(), text: "x".into() }, None);
        t.note(500, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "no_foreign_output"), Outcome::Fail);
    }

    #[test]
    fn response_without_terminal_on_a_live_connection_is_unbounded() {
        let fx = fixture(&[("q1", "hi")], serde_json::json!({}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 50, "q1", "hi");
        t.stream("c1", 51, &rid, "hi! I'm ");
        t.note(3000, "wait_timeout").note(3400, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "bounded_end"), Outcome::Fail);
        assert_eq!(outcome(&r, "deadlines_met"), Outcome::Fail);

        let mut t2 = Tl::new();
        t2.attach("c1", 0, Role::Controller);
        let rid = t2.ask("c1", 50, "q1", "hi");
        t2.recs.push(Rec::Closed { t_ns: 100_000_000, conn: "c1".into(), by: ClosedBy::Runner, detail: String::new() });
        let _ = rid;
        t2.note(600, "cleanup");
        assert_eq!(outcome(&t2.run(&fx), "bounded_end"), Outcome::Pass);
    }

    #[test]
    fn serialized_second_input_fails_overlap_and_late_one_is_indeterminate() {
        let fx = fixture(&[("q1", "hi"), ("q2", "more")], serde_json::json!({"overlap": [["q1", "q2"]], "completed": ["q1", "q2"]}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let r1 = t.ask("c1", 50, "q1", "hi");

        let request_id = tl::request_id("run", "s", 1, "q2");
        t.sent("c1", 100, Some("q2"), Event::UserText { request_id: request_id.clone(), text: "more".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
        t.done("c1", 700, &r1, ResponseStatus::Completed);
        t.recv("c1", 701, Event::ResponseStarted { response_id: "r-q2".into(), request_id, utterance_id: None }, None);
        t.done("c1", 900, "r-q2", ResponseStatus::Completed).note(1300, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "overlap"), Outcome::Fail);
        assert_eq!(r.metrics.overlap_observed, vec![true]);

        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let r1 = t.ask("c1", 50, "q1", "hi");
        t.done("c1", 90, &r1, ResponseStatus::Completed);
        let r2 = t.ask("c1", 100, "q2", "more");
        t.done("c1", 300, &r2, ResponseStatus::Completed).note(700, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "overlap"), Outcome::Indeterminate);
        assert_eq!(r.metrics.overlap_observed, vec![false]);
    }

    #[test]
    fn wrong_contract_text_fails_the_case_without_a_veto() {
        let fx = fixture(&[("q1", "what time is it")], serde_json::json!({"contract": {"q1": "echo"}}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let rid = t.ask("c1", 50, "q1", "what time is it");
        let end = t.stream("c1", 51, &rid, "something else entirely");
        t.done("c1", end + 1, &rid, ResponseStatus::Completed).note(end + 400, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "response_contract"), Outcome::Fail);
        assert_eq!(r.status, CaseStatus::Fail);
        assert_eq!(r.vetoes().count(), 0);
    }

    #[test]
    fn harness_problems_make_the_case_invalid_not_failed() {
        let fx = fixture(&[], serde_json::json!({}));
        let mut t = Tl::new();
        t.note(10, "invalid_target").note(500, "cleanup");
        let r = t.run(&fx);
        assert_eq!(r.status, CaseStatus::Invalid);
        assert_eq!(r.score, 0.0);
    }

    #[test]
    fn missing_decision_link_fails_decision_linked() {
        let fx = fixture(&[("q1", "ok")], serde_json::json!({}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        let request_id = tl::request_id("run", "s", 1, "q1");
        t.sent("c1", 10, Some("q1"), Event::UserText { request_id: request_id.clone(), text: "ok".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
        t.recv("c1", 11, Event::ResponseStarted { response_id: "r".into(), request_id, utterance_id: None }, None);
        t.done("c1", 200, "r", ResponseStatus::Completed).note(600, "cleanup");
        let r = t.run(&fx);
        assert_eq!(outcome(&r, "decision_linked"), Outcome::Fail);
        assert_eq!(outcome(&r, "request_served"), Outcome::Pass);
    }

    #[test]
    fn invalid_guest_frame_fails_protocol_valid() {
        let fx = fixture(&[], serde_json::json!({}));
        let mut t = Tl::new();
        t.attach("c1", 0, Role::Controller);
        t.recs.push(Rec::RecvInvalid { t_ns: 5_000_000, conn: "c1".into(), line: "{".into(), error: "eof".into() });
        t.note(500, "cleanup");
        assert_eq!(outcome(&t.run(&fx), "protocol_valid"), Outcome::Fail);
    }
}
