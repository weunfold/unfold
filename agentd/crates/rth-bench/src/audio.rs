

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use base64::Engine;
use rth_protocol::{Envelope, Event, PROTOCOL_VERSION, ResponseStatus};
use sha2::{Digest, Sha256};

use crate::fixture::{Fixture, Op, TaskDef};
use crate::scorer::{AudioAccount, CaseMetrics, CaseResult, CaseStatus, Check, Outcome};
use crate::timeline::{self as tl, Rec};

pub const SCORER_ID: &str = "rth-bench.audio-contract";
pub const SCORER_VERSION: &str = "0.1.0";

pub const CRITICAL: &[&str] = &["protocol_valid", "telemetry_complete", "no_audio_after_terminal", "no_foreign_audio", "flush_effective", "bounded_end"];

pub const MANDATORY: &[&str] =
    &["deadlines_met", "audio_served", "audio_contiguous", "media_reference_consistent", "media_durable", "ingress_integrity", "cancelled", "completed"];

#[derive(Debug, Clone)]
struct Span {
    start: u64,
    end: u64,
    sha256: String,
    bytes: Option<u64>,
    path: Option<String>,
}

fn span_problems(chunks: &[(u64, Vec<u8>)], spans: &[Span]) -> Vec<String> {
    let mut out = Vec::new();
    let mut sorted: Vec<&(u64, Vec<u8>)> = chunks.iter().collect();
    sorted.sort_by_key(|(o, _)| *o);
    for (o, b) in &sorted {
        let end = o + b.len() as u64 / 2;
        let n = spans.iter().filter(|s| s.start <= *o && end <= s.end).count();
        if n != 1 {
            out.push(format!("samples {o}..{end} are covered by {n} references"));
        }
    }
    for s in spans {
        if s.end < s.start {
            out.push(format!("malformed reference interval {}..{}", s.start, s.end));
            continue;
        }
        let mut bytes = Vec::new();
        let mut at = s.start;
        for (o, b) in &sorted {
            if *o >= s.start && *o < s.end {
                if *o != at {
                    out.push(format!("reference {}..{} spans a hole at sample {at}", s.start, s.end));
                    break;
                }
                bytes.extend_from_slice(b);
                at = o + b.len() as u64 / 2;
            }
        }
        if at != s.end {
            out.push(format!("reference {}..{} covers samples the runner never saw (ends at {at})", s.start, s.end));
        } else if hex(&Sha256::digest(&bytes)) != s.sha256 {
            out.push(format!("reference {}..{} hash {} does not match the PCM", s.start, s.end, s.sha256));
        }
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Checks(Vec<Check>);

impl Checks {
    fn new(applies: impl Fn(&str) -> bool) -> Self {
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
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

fn speaker_notes<'a>(recs: &'a [Rec], kind: &str) -> Vec<(u64, &'a str, serde_json::Value)> {
    recs.iter()
        .filter_map(|r| match r {
            Rec::Note { t_ns, conn: Some(c), code, detail } if code == "speaker" => {
                let v: serde_json::Value = serde_json::from_str(detail).ok()?;
                (v["kind"] == kind).then_some((*t_ns, c.as_str(), v))
            }
            _ => None,
        })
        .collect()
}

pub fn score(fx: &Fixture, _task: &TaskDef, recs: &[Rec]) -> CaseResult {
    let e = &fx.expect;
    let mut checks = Checks::new(|id| match id {
        "flush_effective" => e.flush,
        "audio_served" => !e.audio.is_empty(),
        "ingress_integrity" => fx.steps.iter().any(|s| matches!(s.op, Op::UserAudio { .. })),
        "cancelled" => !e.cancelled.is_empty(),
        "completed" => !e.completed.is_empty(),
        _ => true,
    });
    let mut metrics = CaseMetrics::default();
    let mut invalid = Vec::new();
    let (rep, run_tag) = match recs.first() {
        Some(Rec::Header { rep, run_tag, .. }) => (*rep, run_tag.clone()),
        _ => {
            invalid.push("timeline has no header".to_string());
            (0, String::new())
        }
    };
    let request_id = |label: &str| tl::request_id(&run_tag, &fx.scenario_id, rep, label);
    let conns: BTreeSet<&str> = recs.iter().filter_map(Rec::conn).collect();

    for r in recs {
        if let Rec::Note { code, detail, .. } = r {
            match code.as_str() {
                "attach_failed" | "invalid_target" | "isolation_failed" => invalid.push(format!("{code}: {detail}")),
                "trigger_timeout" | "wait_timeout" | "case_deadline" => checks.fail("deadlines_met", format!("{code}: {detail}"), &[]),
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
        match recs.iter().find_map(|r| r.recv_on(c)) {
            None | Some((Envelope { event: Event::Hello { protocol: PROTOCOL_VERSION, .. }, .. }, _)) => {}
            Some((env, _)) => checks.fail("protocol_valid", format!("{c}: first frame is not a v{PROTOCOL_VERSION} Hello"), &[&env.id]),
        }
    }
    for r in recs {
        if let Some((conn, env, _)) = r.recv() {
            match &env.event {

                Event::JournalFault { detail } => checks.fail("telemetry_complete", format!("{conn}: journal fault: {detail}"), &[&env.id]),
                Event::Log { level, msg } if level == "warn" && msg.contains("lagged") => invalid.push(format!("{conn}: runner connection lagged: {msg}")),
                _ => {}
            }
        }
    }

    for c in &conns {
        let mut done: HashSet<&str> = HashSet::new();
        let mut next: HashMap<&str, (u64, u64)> = HashMap::new();
        for r in recs {
            let Some((env, _)) = r.recv_on(c) else { continue };
            match &env.event {
                Event::AgentAudio { response_id, chunk, sample_offset, data, .. } => {
                    if done.contains(response_id.as_str()) {
                        checks.fail("no_audio_after_terminal", format!("{c}: audio chunk {chunk} of {response_id} after its terminal"), &[&env.id]);
                    }
                    let n = base64::engine::general_purpose::STANDARD.decode(data).map(|b| b.len() as u64 / 2).unwrap_or(0);
                    let want = next.entry(response_id).or_insert((0, 0));
                    if (*chunk, *sample_offset) != *want {
                        checks.fail("audio_contiguous", format!("{c}: {response_id} chunk {chunk}@{sample_offset}, expected {}@{}", want.0, want.1), &[&env.id]);
                    }
                    *want = (chunk + 1, sample_offset + n);
                }
                Event::AgentDone { response_id, .. } => {
                    done.insert(response_id);
                }
                _ => {}
            }
        }
    }

    let started: HashSet<&str> = recs
        .iter()
        .filter_map(|r| match r.recv() {
            Some((_, Envelope { event: Event::ResponseStarted { response_id, .. }, .. }, _)) => Some(response_id.as_str()),
            _ => None,
        })
        .collect();
    for r in recs {
        if let Some((conn, env, _)) = r.recv()
            && let Event::AgentAudio { response_id, .. } = &env.event
            && !started.contains(response_id.as_str())
        {
            checks.fail("no_foreign_audio", format!("{conn}: audio for {response_id}, which never started in this case"), &[&env.id]);
        }
    }
    for c in &conns {
        let live = tl::closed_by_peer(recs, c).is_none();
        for (rid, _, _) in tl::started_on(recs, c) {
            if live && tl::first_terminal(recs, c, &rid).is_none() {
                checks.fail("bounded_end", format!("{c}: {rid} never ended on a live connection"), &[]);
            }
        }
    }

    let sent_at = |label: &str| -> Option<(String, u64)> {
        recs.iter().find_map(|r| match r {
            Rec::Sent { label: Some(l), conn, t_ns, .. } if l == label => Some((conn.clone(), *t_ns)),
            _ => None,
        })
    };
    for label in &e.audio {
        let Some((conn, t0)) = sent_at(label) else {
            checks.fail("audio_served", format!("{label}: never sent"), &[]);
            continue;
        };
        let Some(rid) = tl::response_for(recs, &request_id(label)) else {
            checks.fail("audio_served", format!("{label}: no response started"), &[]);
            continue;
        };
        let audio = tl::audio(recs, &conn, &rid);
        match audio.first() {
            Some((t1, ..)) => metrics.first_audio_ms.push(ms(t1.saturating_sub(t0))),
            None => checks.fail("audio_served", format!("{label}: response {rid} carried no audio"), &[]),
        }
        if tl::first_terminal(recs, &conn, &rid).is_none() {
            checks.fail("audio_served", format!("{label}: response {rid} never ended"), &[]);
        }
    }
    for (label, want_status, id) in e.completed.iter().map(|l| (l, ResponseStatus::Completed, "completed")).chain(e.cancelled.iter().map(|l| (l, ResponseStatus::Cancelled, "cancelled"))) {
        let Some((conn, _)) = sent_at(label) else {
            checks.fail(id, format!("{label}: never sent"), &[]);
            continue;
        };
        match tl::response_for(recs, &request_id(label)).and_then(|rid| tl::first_terminal(recs, &conn, &rid)) {
            Some((_, s)) if s == want_status => {}
            Some((_, s)) => checks.fail(id, format!("{label}: ended {s:?}"), &[]),
            None => checks.fail(id, format!("{label}: no terminal"), &[]),
        }
    }

    let mut wire_refs: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut seen_refs = HashSet::new();
    for r in recs {
        if let Some((_, env, _)) = r.recv()
            && let Event::MediaStored { media, response_id: Some(rid), sample_start, sample_end, sha256, .. } = &env.event
            && media == "agent_audio"
            && seen_refs.insert(env.id.clone())
        {
            wire_refs.entry(rid.clone()).or_default().push(Span { start: *sample_start, end: *sample_end, sha256: sha256.clone(), bytes: None, path: None });
        }
    }
    let speaker_summary: HashMap<String, serde_json::Value> = speaker_notes(recs, "summary")
        .into_iter()
        .flat_map(|(_, _, v)| v["responses"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| Some((r["response_id"].as_str()?.to_string(), r)))
        .collect();

    let mut streams: Vec<(String, Vec<(u64, Vec<u8>)>, bool)> = Vec::new();
    let mut accounted = HashSet::new();
    for c in &conns {
        for (rid, _, _) in tl::started_on(recs, c) {
            if !accounted.insert(rid.clone()) {
                continue;
            }
            let chunks: Vec<(u64, Vec<u8>)> = recs
                .iter()
                .filter_map(|r| match r.recv_on(c) {
                    Some((Envelope { event: Event::AgentAudio { response_id, sample_offset, data, .. }, .. }, _)) if *response_id == rid => {
                        Some((*sample_offset, base64::engine::general_purpose::STANDARD.decode(data).ok()?))
                    }
                    _ => None,
                })
                .collect();
            if chunks.is_empty() {
                continue;
            }
            let ended = tl::first_terminal(recs, c, &rid).is_some();
            if ended {
                for p in span_problems(&chunks, wire_refs.get(&rid).map(Vec::as_slice).unwrap_or(&[])) {
                    checks.fail("media_reference_consistent", format!("{rid}: {p}"), &[]);
                }
            }
            streams.push((format!("agent_audio:{rid}"), chunks, ended));
        }
    }
    for r in recs {
        if let Rec::Sent { env: Some(Envelope { event: Event::UserAudio { stream_id, sample_offset, data, .. }, .. }), .. } = r {
            let key = format!("user_audio:{stream_id}");
            let pcm = base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default();
            match streams.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, v, _)) => v.push((*sample_offset, pcm)),
                None => streams.push((key, vec![(*sample_offset, pcm)], true)),
            }
        }
    }

    let receipt = recs.iter().rev().find_map(|r| match r {
        Rec::Note { code, detail, .. } if code == "storage_receipt" => serde_json::from_str::<serde_json::Value>(detail).ok(),
        _ => None,
    });
    let mut stored_refs: BTreeMap<String, Vec<Span>> = BTreeMap::new();
    let mut blobs: HashMap<String, serde_json::Value> = HashMap::new();
    let available = receipt.as_ref().is_some_and(|r| r["available"] == true);
    if let Some(r) = receipt.as_ref().filter(|_| available) {
        for x in r["references"].as_array().into_iter().flatten() {
            let stream = x["response_id"].as_str().or(x["stream_id"].as_str()).unwrap_or_default();
            let key = format!("{}:{stream}", x["media"].as_str().unwrap_or_default());
            stored_refs.entry(key).or_default().push(Span {
                start: x["sample_start"].as_u64().unwrap_or(0),
                end: x["sample_end"].as_u64().unwrap_or(0),
                sha256: x["sha256"].as_str().unwrap_or_default().into(),
                bytes: x["bytes"].as_u64(),
                path: x["path"].as_str().map(String::from),
            });
        }
        for b in r["blobs"].as_array().into_iter().flatten() {
            if let Some(p) = b["path"].as_str() {
                blobs.insert(p.into(), b.clone());
            }
        }
    }
    if streams.iter().any(|(_, _, ended)| *ended) && !available {
        let why = receipt.as_ref().and_then(|r| r["why"].as_str().map(String::from)).unwrap_or_else(|| "no storage receipt".into());
        checks.indeterminate("media_durable", format!("storage not observed: {why}"));
    }
    for (key, chunks, ended) in &streams {
        let spans = stored_refs.get(key).map(Vec::as_slice).unwrap_or(&[]);
        if available && *ended {
            for p in span_problems(chunks, spans) {
                checks.fail("media_durable", format!("{key}: committed journal: {p}"), &[]);
            }
            for sp in spans {
                let path = sp.path.clone().unwrap_or_default();
                match blobs.get(&path) {
                    None => checks.fail("media_durable", format!("{key}: no read-back of {path}"), &[]),
                    Some(b) if b["path_ok"] != true => checks.fail("media_durable", format!("{key}: unsafe blob path {path:?}"), &[]),
                    Some(b) if b["refused"].is_string() => checks.fail("media_durable", format!("{key}: blob {path} refused: {}", b["refused"]), &[]),
                    Some(b) if b["present"] != true => checks.fail("media_durable", format!("{key}: blob {path} is missing"), &[]),
                    Some(b) if b["bytes"].as_u64() != sp.bytes || b["sha256"].as_str() != Some(sp.sha256.as_str()) => checks.fail(
                        "media_durable",
                        format!("{key}: blob {path} is {} B / {}, reference says {:?} B / {}", b["bytes"], b["sha256"], sp.bytes, sp.sha256),
                        &[],
                    ),
                    Some(_) => {}
                }
            }
        }
        if let Some(rid) = key.strip_prefix("agent_audio:") {
            let s = speaker_summary.get(rid);
            let refs = if available { spans } else { wire_refs.get(rid).map(Vec::as_slice).unwrap_or(&[]) };
            metrics.audio.push(AudioAccount {
                response_id: rid.to_string(),
                emitted_samples: Some(refs.iter().map(|r| r.end.saturating_sub(r.start)).sum()),
                received_samples: chunks.iter().map(|(_, b)| b.len() as u64 / 2).sum(),
                queued_samples: s.and_then(|v| v["queued_samples"].as_u64()),
                played_samples: s.and_then(|v| v["played_samples"].as_u64()),
                played_after_clear: s.and_then(|v| v["played_after_clear"].as_u64()),
                evidence: if s.is_some() { "synthetic_device".into() } else { "transport".into() },
            });
        }
    }

    for step in &fx.steps {
        let Op::UserAudio { label, .. } = &step.op else { continue };
        let stream = request_id(label);
        let mut written = Vec::new();
        let mut chunks = 0u64;
        let mut end_id = None;
        for r in recs {
            if let Rec::Sent { env: Some(env), .. } = r {
                match &env.event {
                    Event::UserAudio { stream_id, data, .. } if *stream_id == stream => {
                        written.extend(base64::engine::general_purpose::STANDARD.decode(data).unwrap_or_default());
                        chunks += 1;
                    }
                    Event::UserAudioEnd { stream_id, .. } if *stream_id == stream => end_id = Some(env.id.clone()),
                    _ => {}
                }
            }
        }
        let want_gaps = e.ingress_gaps.get(label).copied().unwrap_or(0);
        let receipt = recs.iter().find_map(|r| match r.recv() {
            Some((_, env @ Envelope { event: Event::AudioIngress { stream_id, .. }, .. }, _)) if *stream_id == stream && env.cause == end_id => Some(env),
            _ => None,
        });
        match receipt.map(|env| (env, &env.event)) {
            Some((env, Event::AudioIngress { chunks: c, samples, bytes, gaps, sha256, .. })) => {
                let sha = hex(&Sha256::digest(&written));
                if *sha256 != sha || *c != chunks || *bytes != written.len() as u64 || *samples != written.len() as u64 / 2 || *gaps != want_gaps {
                    checks.fail(
                        "ingress_integrity",
                        format!("{label}: agent got {c} chunks / {bytes} B / {gaps} gaps / {sha256}; runner wrote {chunks} / {} B / expected {want_gaps} gaps / {sha}", written.len()),
                        &[&env.id],
                    );
                }
            }
            _ => checks.fail("ingress_integrity", format!("{label}: no audio_ingress receipt caused by the stream end"), &[]),
        }
    }

    if e.flush {
        let barge = speaker_notes(recs, "barge_in");
        let summaries = speaker_notes(recs, "summary");
        match barge.first() {
            None => checks.fail("flush_effective", "no barge-in was executed", &[]),
            Some((t_ns, conn, b)) => {
                if let Some(ns) = b["flush_ns"].as_u64() {
                    metrics.flush_us.push(ns as f64 / 1e3);
                }
                let live: Vec<String> = b["live"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
                if live.is_empty() || b["dropped_samples"].as_u64() == Some(0) {
                    checks.indeterminate("flush_effective", "nothing was queued when the barge-in ran (the scenario missed live audio)");
                }
                for (_, _, s) in &summaries {
                    if s["rendered_after_fence"].as_u64().unwrap_or(0) > 0 {
                        checks.fail("flush_effective", format!("{} samples of a fenced epoch were rendered", s["rendered_after_fence"]), &[]);
                    }
                    for r in s["responses"].as_array().into_iter().flatten() {
                        if let Some(n) = r["played_after_clear"].as_u64().filter(|n| *n > 0) {
                            checks.fail("flush_effective", format!("{}: {n} samples rendered after the local clear", r["response_id"]), &[]);
                        }
                    }
                }
                if summaries.is_empty() {
                    checks.fail("flush_effective", "no speaker summary (the playout was not observed)", &[]);
                }
                for rid in &live {
                    if let Some((t, ResponseStatus::Cancelled)) = tl::first_terminal(recs, conn, rid) {
                        metrics.barge_in_to_terminal_ms.push(ms(t.saturating_sub(*t_ns)));
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{Split, TaskRef};
    use rth_protocol::{MEDIA_REF_FORMAT, MonotonicClock, Source, Stamper};

    fn fixture(expect: serde_json::Value, extra_steps: serde_json::Value) -> Fixture {
        let mut steps = vec![
            serde_json::json!({"at_ms": 0, "conn": "c1", "op": "attach", "role": "controller"}),
            serde_json::json!({"at_ms": 50, "conn": "c1", "op": "user_text", "label": "q1", "text": "aloud"}),
        ];
        steps.extend(extra_steps.as_array().cloned().unwrap_or_default());
        serde_json::from_value(serde_json::json!({
            "fixture_format": "rth-bench.fixture/v1", "feed_mode": "paced_causal", "scenario_id": "s", "group_id": "g", "split": "dev",
            "task": {"id": "t", "version": "1"}, "reactive": false, "description": "", "deadline_ms": 1000, "settle_ms": 10,
            "steps": steps, "expect": expect
        }))
        .unwrap()
    }

    fn task() -> TaskDef {
        serde_json::from_value(serde_json::json!({
            "task_format": "rth-bench.task/v1", "task_id": "t", "version": "1", "purpose": "", "family": "", "capabilities": [],
            "input_protocol": {"name": "rth-protocol", "version": 2, "transport": "", "feed_mode": "paced_causal"},
            "provenance": "", "license": "", "fixture_manifest_sha256": "", "fixtures": [],
            "scorer": {"id": SCORER_ID, "version": SCORER_VERSION}, "output_schema": "", "metrics": [], "normalization": "",
            "missing_timeout_error_policy": "", "critical_checks": CRITICAL, "mandatory_checks": MANDATORY, "variant_labels": []
        }))
        .unwrap()
    }

    struct T {
        recs: Vec<Rec>,
        g: Stamper,
        h: Stamper,
    }

    impl T {
        fn new() -> Self {
            let mut t = T {
                recs: vec![Rec::Header { scenario_id: "s".into(), task: TaskRef { id: "t".into(), version: "1".into() }, split: Split::Dev, rep: 1, fixture_sha256: "x".into(), run_tag: "run".into(), origin_ns: 0 }],
                g: Stamper::new(Source::Guest, "guest-a", Box::new(MonotonicClock::start())),
                h: Stamper::new(Source::Host, "host-a", Box::new(MonotonicClock::start())),
            };
            let hello = Event::Hello { protocol: PROTOCOL_VERSION, agent: "a".into(), version: "0".into(), boot_id: "b".into(), utc_ms_at_origin: 0, journal: "j".into(), backend: None, deployment: None, instance: None, command_generation: None };
            t.recv(1, hello);
            t
        }
        fn recv(&mut self, ms: u64, e: Event) -> String {
            let env = self.g.stamp(e, None);
            let id = env.id.clone();
            self.recs.push(Rec::Recv { t_ns: ms * 1_000_000, conn: "c1".into(), env });
            id
        }
        fn sent(&mut self, ms: u64, label: Option<&str>, e: Event) -> String {
            let env = self.h.stamp(e, None);
            let id = env.id.clone();
            self.recs.push(Rec::Sent { t_ns: ms * 1_000_000, conn: "c1".into(), step: Some(1), label: label.map(String::from), env: Some(env), raw: None, raw_bytes: None });
            id
        }
        fn chunk(rid: &str, n: u64) -> Event {
            Event::AgentAudio { response_id: rid.into(), chunk: n, sample_offset: n * 2, sample_rate: 24_000, format: "s16le".into(), data: base64::engine::general_purpose::STANDARD.encode([n as u8, 0, n as u8, 1]) }
        }
        fn media(rid: &str, from: u64, to: u64) -> Event {
            let pcm: Vec<u8> = (from..to).flat_map(|n| [n as u8, 0, n as u8, 1]).collect();
            let sha = hex(&Sha256::digest(&pcm));
            Event::MediaStored {
                ref_format: MEDIA_REF_FORMAT.into(), media: "agent_audio".into(), response_id: Some(rid.into()), stream_id: None,
                sha256: hex(&Sha256::digest(&pcm)), bytes: pcm.len() as u64, encoding: "s16le".into(), sample_rate: 24_000, channels: 1,
                chunk_first: from, chunk_last: to - 1, sample_start: from * 2, sample_end: to * 2, first_event: "x".into(), last_event: "y".into(),
                clock: "guest-a".into(), t_ns_first: 0, t_ns_last: 0, path: format!("media/{sha}"),
            }
        }

        fn clean(&mut self) {
            let q = tl::request_id("run", "s", 1, "q1");
            self.sent(50, Some("q1"), Event::UserText { request_id: q.clone(), text: "aloud".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
            self.recv(60, Event::ResponseStarted { response_id: "r1".into(), request_id: q, utterance_id: None });
            for n in 0..3 {
                self.recv(70 + n, Self::chunk("r1", n));
            }
            self.recv(80, Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Completed });
            self.recv(81, Self::media("r1", 0, 3));
        }
        fn score(&self, fx: &Fixture) -> CaseResult {
            score(fx, &task(), &self.recs)
        }

        fn store(&mut self, name: &str, blob: Blob) {
            let dir = std::env::temp_dir().join(format!("rth-bench-store-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("media")).unwrap();
            let mut journal = String::new();
            for r in &self.recs {
                if let Rec::Recv { env, .. } = r
                    && let Event::MediaStored { path, .. } = &env.event
                {
                    journal.push_str(&env.to_line());
                    let pcm: Vec<u8> = (0..3u64).flat_map(|n| [n as u8, 0, n as u8, 1]).collect();
                    match blob {
                        Blob::Good => std::fs::write(dir.join(path), &pcm).unwrap(),
                        Blob::Corrupt => std::fs::write(dir.join(path), &pcm[..5]).unwrap(),
                        Blob::Absent => {}
                    }
                }
            }
            std::fs::write(dir.join("journal.jsonl"), journal).unwrap();
            let receipt = crate::runner::storage_receipt(Some(&dir), &self.recs);
            self.recs.push(Rec::Note { t_ns: 99_000_000, conn: None, code: "storage_receipt".into(), detail: receipt.to_string() });
        }
    }

    #[derive(Clone, Copy)]
    enum Blob {
        Good,
        Corrupt,
        Absent,
    }

    fn outcome(r: &CaseResult, id: &str) -> Outcome {
        r.checks.iter().find(|c| c.id == id).unwrap().outcome
    }

    #[test]
    fn a_clean_spoken_reply_passes_and_measures_first_audio_on_the_runner_clock() {
        let mut t = T::new();
        t.clean();
        t.store("clean", Blob::Good);
        let r = t.score(&fixture(serde_json::json!({"audio": ["q1"], "completed": ["q1"]}), serde_json::json!([])));
        assert_eq!(r.status, CaseStatus::Pass, "{:?}", r.checks);
        assert_eq!(r.metrics.first_audio_ms, vec![20.0]);
        assert_eq!(r.metrics.audio[0].emitted_samples, Some(6));
        assert_eq!(r.metrics.audio[0].evidence, "transport");
    }

    #[test]
    fn audio_after_the_terminal_is_a_veto() {
        let mut t = T::new();
        t.clean();
        t.recv(90, T::chunk("r1", 3));
        let r = t.score(&fixture(serde_json::json!({"audio": ["q1"]}), serde_json::json!([])));
        assert_eq!(outcome(&r, "no_audio_after_terminal"), Outcome::Fail);
        assert!(r.vetoes().any(|c| c.id == "no_audio_after_terminal"));
    }

    #[test]
    fn a_gap_a_missing_reference_and_a_wrong_hash_fail_their_checks() {

        let mut t = T::new();
        let q = tl::request_id("run", "s", 1, "q1");
        t.sent(50, Some("q1"), Event::UserText { request_id: q.clone(), text: "aloud".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None });
        t.recv(60, Event::ResponseStarted { response_id: "r1".into(), request_id: q, utterance_id: None });
        t.recv(70, T::chunk("r1", 0));
        t.recv(72, T::chunk("r1", 2));
        t.recv(80, Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Completed });
        let fx = fixture(serde_json::json!({"audio": ["q1"]}), serde_json::json!([]));
        t.store("gap", Blob::Good);
        let r = t.score(&fx);
        assert_eq!(outcome(&r, "audio_contiguous"), Outcome::Fail);

        assert_eq!(outcome(&r, "media_reference_consistent"), Outcome::Fail);
        assert_eq!(outcome(&r, "media_durable"), Outcome::Fail);

        let mut t = T::new();
        t.clean();
        if let Some(Rec::Recv { env, .. }) = t.recs.last_mut()
            && let Event::MediaStored { sha256, .. } = &mut env.event
        {
            *sha256 = "0".repeat(64);
        }
        assert_eq!(outcome(&t.score(&fx), "media_reference_consistent"), Outcome::Fail);

        let mut t = T::new();
        t.clean();
        t.recv(85, Event::JournalFault { detail: "media not stored".into() });
        assert_eq!(outcome(&t.score(&fx), "telemetry_complete"), Outcome::Fail);
    }

    #[test]
    fn ingress_must_match_what_the_runner_wrote() {
        let fx: Fixture = serde_json::from_value(serde_json::json!({
            "fixture_format": "rth-bench.fixture/v1", "feed_mode": "paced_causal", "scenario_id": "s", "group_id": "g", "split": "dev",
            "task": {"id": "t", "version": "1"}, "reactive": false, "description": "", "deadline_ms": 1000, "settle_ms": 10,
            "steps": [{"at_ms": 0, "conn": "c1", "op": "attach", "role": "controller"}, {"at_ms": 5, "conn": "c1", "op": "user_audio", "label": "m1", "tone_hz": 0.0, "ms": 80}],
            "expect": {"ingress_gaps": {"m1": 0}}
        }))
        .unwrap();
        let stream = tl::request_id("run", "s", 1, "m1");
        let pcm = [1u8, 2, 3, 4];
        let mk = |t: &mut T, sha: String| {
            t.sent(5, None, Event::UserAudio { stream_id: stream.clone(), chunk: 0, sample_offset: 0, sample_rate: 16_000, format: "s16le".into(), data: base64::engine::general_purpose::STANDARD.encode(pcm), digest: None });
            let end = t.sent(6, Some("m1"), Event::UserAudioEnd { stream_id: stream.clone(), chunks: 1, samples: 2, dropped_chunks: 0, speaker: Default::default() });
            let receipt = t.g.stamp(Event::AudioIngress { stream_id: stream.clone(), chunks: 1, samples: 2, bytes: 4, gaps: 0, sha256: sha }, Some(end));
            t.recs.push(Rec::Recv { t_ns: 7_000_000, conn: "c1".into(), env: receipt });
        };
        let mut good = T::new();
        mk(&mut good, hex(&Sha256::digest(pcm)));
        assert_eq!(outcome(&good.score(&fx), "ingress_integrity"), Outcome::Pass);
        let mut bad = T::new();
        mk(&mut bad, hex(&Sha256::digest([9u8])));
        assert_eq!(outcome(&bad.score(&fx), "ingress_integrity"), Outcome::Fail);
    }

    #[test]
    fn a_flush_that_lets_old_audio_render_is_a_veto_and_an_empty_one_is_indeterminate() {
        let fx = fixture(serde_json::json!({"audio": ["q1"], "flush": true}), serde_json::json!([{"conn": "c1", "op": "speaker"}, {"conn": "c1", "op": "barge_in"}]));
        let note = |t: &mut T, v: serde_json::Value| t.recs.push(Rec::Note { t_ns: 75_000_000, conn: Some("c1".into()), code: "speaker".into(), detail: v.to_string() });
        let summary = |after: u64| serde_json::json!({"kind": "summary", "responses": [{"response_id": "r1", "queued_samples": 6, "played_samples": 4, "played_after_clear": after}], "rendered_after_fence": 0});
        let mut ok = T::new();
        ok.clean();
        note(&mut ok, serde_json::json!({"kind": "barge_in", "t_ns": 75_000_000u64, "live": ["r1"], "dropped_samples": 2, "flush_ns": 3000}));
        note(&mut ok, summary(0));
        let r = ok.score(&fx);
        assert_eq!(outcome(&r, "flush_effective"), Outcome::Pass, "{:?}", r.checks);
        assert_eq!(r.metrics.flush_us, vec![3.0]);
        let mut leak = T::new();
        leak.clean();
        note(&mut leak, serde_json::json!({"kind": "barge_in", "t_ns": 75_000_000u64, "live": ["r1"], "dropped_samples": 2, "flush_ns": 3000}));
        note(&mut leak, summary(240));
        assert!(leak.score(&fx).vetoes().any(|c| c.id == "flush_effective"));
        let mut empty = T::new();
        empty.clean();
        note(&mut empty, serde_json::json!({"kind": "barge_in", "t_ns": 75_000_000u64, "live": [], "dropped_samples": 0, "flush_ns": 100}));
        note(&mut empty, summary(0));
        assert_eq!(outcome(&empty.score(&fx), "flush_effective"), Outcome::Indeterminate);
    }

    #[test]
    fn a_consistent_reference_without_a_good_blob_is_not_durable() {
        let fx = fixture(serde_json::json!({"audio": ["q1"]}), serde_json::json!([]));
        for (name, blob) in [("absent", Blob::Absent), ("corrupt", Blob::Corrupt)] {
            let mut t = T::new();
            t.clean();
            t.store(name, blob);
            let r = t.score(&fx);
            assert_eq!(outcome(&r, "media_reference_consistent"), Outcome::Pass, "{name}");
            assert_eq!(outcome(&r, "media_durable"), Outcome::Fail, "{name}: {:?}", r.checks);
        }

        let mut t = T::new();
        t.clean();
        assert_eq!(outcome(&t.score(&fx), "media_durable"), Outcome::Indeterminate);

        let mut t = T::new();
        t.clean();
        if let Some(Rec::Recv { env, .. }) = t.recs.last_mut()
            && let Event::MediaStored { path, .. } = &mut env.event
        {
            *path = "../../etc/passwd".into();
        }
        t.store("unsafe", Blob::Absent);
        let r = t.score(&fx);
        assert!(r.checks.iter().any(|c| c.id == "media_durable" && c.detail.iter().any(|d| d.contains("unsafe"))), "{:?}", r.checks);
    }

    #[test]
    fn a_reversed_reference_interval_is_malformed_not_a_panic() {
        let mut t = T::new();
        t.clean();
        if let Some(Rec::Recv { env, .. }) = t.recs.last_mut()
            && let Event::MediaStored { sample_start, sample_end, .. } = &mut env.event
        {
            (*sample_start, *sample_end) = (6, 0);
        }
        t.store("reversed", Blob::Good);
        let r = t.score(&fixture(serde_json::json!({"audio": ["q1"]}), serde_json::json!([])));
        assert_eq!(outcome(&r, "media_reference_consistent"), Outcome::Fail);
        assert_eq!(outcome(&r, "media_durable"), Outcome::Fail);
        assert_eq!(r.metrics.audio[0].emitted_samples, Some(0));
    }

    #[test]
    fn a_symlinked_blob_is_refused_and_fails_durability() {
        let mut t = T::new();
        t.clean();
        t.store("symlink", Blob::Good);
        t.recs.pop();
        let dir = std::env::temp_dir().join(format!("rth-bench-store-symlink-{}", std::process::id()));
        let path = match &t.recs.iter().rev().find_map(|r| match r { Rec::Recv { env, .. } => Some(env.event.clone()), _ => None }) {
            Some(Event::MediaStored { path, .. }) => path.clone(),
            _ => panic!(),
        };
        let outside = std::env::temp_dir().join(format!("rth-bench-outside-{}", std::process::id()));
        std::fs::write(&outside, std::fs::read(dir.join(&path)).unwrap()).unwrap();
        std::fs::remove_file(dir.join(&path)).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join(&path)).unwrap();
        let receipt = crate::runner::storage_receipt(Some(&dir), &t.recs);
        t.recs.push(Rec::Note { t_ns: 99_000_000, conn: None, code: "storage_receipt".into(), detail: receipt.to_string() });
        let r = t.score(&fixture(serde_json::json!({"audio": ["q1"]}), serde_json::json!([])));
        assert_eq!(outcome(&r, "media_durable"), Outcome::Fail, "{:?}", r.checks);
    }
}
