

use serde_json::{Value, json};

use crate::store::Lines;
use crate::{BoundedText, Proposal, ProposalId, ProposalKind, ProposalStatus};

pub const DAY_START_MIN: i64 = 4 * 60;

pub const NIGHT_WINDOW_MIN: i64 = 4 * 60;

pub const PROPOSE_MEMORY_EDITS: bool = false;

pub const NIGHT_CAP_PUSD: u64 = 100_000_000_000;

pub const MONTH_LINE_PUSD: u64 = 40_000_000_000_000;

pub const MAX_PROPOSALS: usize = 3;

pub const MAX_EVIDENCE: usize = 5;

pub const MAX_MATERIAL_BYTES: usize = 60_000;

pub const SLEEP_LOG: &str = "sleep.jsonl";

pub fn sleep_day(now_utc_ms: u64, utc_offset_min: i32) -> String {
    let local_min = (now_utc_ms / 60_000) as i64 + i64::from(utc_offset_min) - DAY_START_MIN;
    let days = local_min.div_euclid(24 * 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn in_night_window(now_utc_ms: u64, utc_offset_min: i32) -> bool {
    let local_min = (now_utc_ms / 60_000) as i64 + i64::from(utc_offset_min);
    (local_min - DAY_START_MIN).rem_euclid(24 * 60) < NIGHT_WINDOW_MIN
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub now_utc_ms: u64,
    pub utc_offset_min: i32,

    pub enabled: bool,

    pub idle: bool,
    pub flash_on: bool,

    pub memory_egress: bool,
    pub month_spent_pusd: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {

    Wait,
    Skip { day: String, reason: &'static str },

    Run { day: String, with_memories: bool, since_utc_ms: Option<u64> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastSleep {
    pub local_date: String,
    pub ran: bool,
    pub reason: Option<String>,
}

fn sleep_lines(log: &Lines) -> impl Iterator<Item = &Value> {
    log.records.iter().filter(|v| v["kind"] == "sleep")
}

pub fn last_sleep(log: &Lines) -> Option<LastSleep> {
    let v = sleep_lines(log).last();
    if log.torn || log.corrupt {
        return Some(LastSleep { local_date: v.and_then(|v| v["local_date"].as_str()).unwrap_or_default().to_string(), ran: false, reason: Some("log_damaged".into()) });
    }
    let v = v?;
    Some(LastSleep {
        local_date: v["local_date"].as_str()?.to_string(),
        ran: v["phase"] == "end" && v["ran"] == true,
        reason: v["reason"].as_str().map(str::to_string).or_else(|| (v["phase"] == "start").then(|| "running".to_string())),
    })
}

pub fn plan(f: &Facts, log: &Lines) -> Plan {
    if log.torn || log.corrupt {
        return Plan::Wait;
    }
    let day = sleep_day(f.now_utc_ms, f.utc_offset_min);
    let today: Vec<&Value> = sleep_lines(log).filter(|v| v["local_date"] == day.as_str()).collect();
    if today.iter().any(|v| v["phase"] == "start" || v["phase"] == "end" || v["reason"] == "missed_window") {
        return Plan::Wait;
    }
    let skip = |reason: &'static str| if today.iter().any(|v| v["reason"] == reason) { Plan::Wait } else { Plan::Skip { day: day.clone(), reason } };
    if !f.enabled {
        return if sleep_lines(log).last().is_some_and(|v| v["phase"] == "skip" && v["reason"] == "off") { Plan::Wait } else { skip("off") };
    }
    if !f.flash_on {
        return skip("flash_off");
    }
    if f.month_spent_pusd >= MONTH_LINE_PUSD {
        return skip("month_line");
    }
    if !in_night_window(f.now_utc_ms, f.utc_offset_min) {
        return skip("missed_window");
    }
    if !f.idle {
        return Plan::Wait;
    }
    let since_utc_ms = sleep_lines(log).filter(|v| v["phase"] == "start").filter_map(|v| v["utc_ms"].as_u64()).last();
    Plan::Run { day, with_memories: f.memory_egress, since_utc_ms }
}

pub fn skip_line(day: &str, reason: &str, now_utc_ms: u64) -> Value {
    json!({"kind": "sleep", "phase": "skip", "local_date": day, "ran": false, "reason": reason, "utc_ms": now_utc_ms})
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Material {
    pub chat: Vec<Line>,
    pub memories: Vec<Line>,
}

impl Material {

    fn bounded(mut self, with_memories: bool) -> (Material, usize) {
        if !with_memories {
            self.memories.clear();
        }
        let mut budget = MAX_MATERIAL_BYTES;
        let mut left_out = 0;
        for list in [&mut self.chat, &mut self.memories] {
            let mut keep = Vec::new();
            for l in list.drain(..).rev() {
                let n = l.text.len();
                if n <= budget {
                    budget -= n;
                    keep.push(l);
                } else {
                    left_out += 1;
                }
            }
            keep.reverse();
            *list = keep;
        }
        (self, left_out)
    }

    fn ids(&self) -> impl Iterator<Item = &str> {
        self.chat.iter().chain(&self.memories).map(|l| l.id.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {

    OverCap,
    Failed(String),
}

pub trait Night {

    fn material(&mut self, day: &str, since_utc_ms: Option<u64>) -> Result<Material, String>;

    fn draft(&mut self, material: &Material, cap_pusd: u64) -> Result<String, DraftError>;

    fn record(&mut self, line: Value) -> Result<(), String>;

    fn propose(&mut self, p: &Proposal) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slept {
    pub proposed: usize,

    pub refused: usize,
    pub left_out_lines: usize,
}

fn proposals(reply: &str, shown: &Material, run_id: &str, now_utc_ms: u64) -> Option<(Vec<Proposal>, usize)> {
    let v: Value = serde_json::from_str(reply).ok()?;
    let all = v["proposals"].as_array()?.clone();
    let ids: Vec<&str> = shown.ids().collect();
    let mut out: Vec<Proposal> = Vec::new();
    for p in &all {
        let kind = match p["kind"].as_str() {
            Some("drive") => ProposalKind::Drive,
            Some("memory-edit") => ProposalKind::MemoryEdit,
            _ => continue,
        };
        let mut evidence: Vec<String> = Vec::new();
        for e in p["evidence"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !evidence.iter().any(|x| x == e) {
                evidence.push(e.to_string());
            }
        }
        let cited_ok = (1..=MAX_EVIDENCE).contains(&evidence.len()) && evidence.iter().all(|e| ids.contains(&e.as_str()));
        let memories = evidence.iter().filter(|e| e.starts_with("memory:")).count();
        let shape_ok = match kind {
            ProposalKind::Drive => memories == 0,
            _ => PROPOSE_MEMORY_EDITS && memories == 1 && evidence.len() == 1,
        };
        let (Ok(text), Ok(why)) = (BoundedText::new(p["text"].as_str().unwrap_or_default()), BoundedText::new(p["why"].as_str().unwrap_or_default())) else { continue };
        if !cited_ok || !shape_ok || out.len() == MAX_PROPOSALS || out.iter().any(|q| q.kind == kind && q.text == text) {
            continue;
        }
        let Ok(id) = ProposalId::new(format!("p-sleep-{run_id}-{}", out.len() + 1)) else { continue };
        let mut evidence_ids = vec![format!("sleep:{run_id}")];
        evidence_ids.extend(evidence);
        out.push(Proposal { id, kind, text, why, evidence_ids, created_utc_ms: now_utc_ms, status: ProposalStatus::Pending, decided_utc_ms: None });
    }
    let refused = all.len() - out.len();
    Some((out, refused))
}

pub fn run(night: &mut dyn Night, day: &str, with_memories: bool, since_utc_ms: Option<u64>, run_id: &str, now_utc_ms: u64) -> Result<Slept, String> {
    if ProposalId::new(format!("p-sleep-{run_id}-{MAX_PROPOSALS}")).is_err() {
        return Err(format!("run id {run_id:?} is not a short slug"));
    }
    let end = |ran: bool, reason: Option<&str>, s: &Slept| {
        json!({"kind": "sleep", "phase": "end", "local_date": day, "run_id": run_id, "ran": ran, "reason": reason, "proposed": s.proposed, "refused": s.refused, "left_out_lines": s.left_out_lines, "utc_ms": now_utc_ms})
    };
    let material = night.material(day, since_utc_ms)?;
    let had_chat = !material.chat.is_empty();
    let (material, left_out_lines) = material.bounded(with_memories);
    let mut slept = Slept { proposed: 0, refused: 0, left_out_lines };

    if material.chat.is_empty() {
        night.record(end(false, Some(if had_chat { "too_long" } else { "quiet_day" }), &slept))?;
        return Ok(slept);
    }
    night.record(json!({"kind": "sleep", "phase": "start", "local_date": day, "run_id": run_id, "with_memories": with_memories, "utc_ms": now_utc_ms}))?;
    let reply = match night.draft(&material, NIGHT_CAP_PUSD) {
        Ok(r) => r,
        Err(e) => {
            night.record(end(false, Some(if e == DraftError::OverCap { "over_cap" } else { "draft_failed" }), &slept))?;
            return Ok(slept);
        }
    };
    let Some((found, refused)) = proposals(&reply, &material, run_id, now_utc_ms) else {
        night.record(end(false, Some("unparsed"), &slept))?;
        return Ok(slept);
    };
    slept.refused = refused;
    for p in &found {
        if let Err(e) = night.propose(p) {
            night.record(end(false, Some("propose_failed"), &slept))?;
            return Err(e);
        }
        slept.proposed += 1;
    }
    night.record(end(true, None, &slept))?;
    Ok(slept)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: u64 = 1_793_016_000_000;
    const BRT: i32 = -180;
    const HOUR: u64 = 3_600_000;

    const DAWN: u64 = NOON - 4 * HOUR;

    fn lines(records: Vec<Value>) -> Lines {
        Lines { records, torn: false, corrupt: false }
    }

    fn none() -> Lines {
        lines(Vec::new())
    }

    fn facts(now: u64) -> Facts {
        Facts { now_utc_ms: now, utc_offset_min: BRT, enabled: true, idle: true, flash_on: true, memory_egress: true, month_spent_pusd: 0 }
    }

    #[test]
    fn s1_the_sleep_day_turns_at_four_in_the_morning_local() {
        assert_eq!(sleep_day(NOON, BRT), "2026-10-26", "09:00 local");

        assert_eq!(sleep_day(NOON - 5 * HOUR - 60_000, BRT), "2026-10-25");
        assert_eq!(sleep_day(NOON - 5 * HOUR, BRT), "2026-10-26", "04:00 local");
        assert_eq!(sleep_day(NOON - 5 * HOUR, 0), "2026-10-26", "07:00 UTC");
        assert_eq!(sleep_day(0, 0), "1969-12-31");
        assert_eq!(sleep_day(951_782_400_000 + 4 * HOUR, 0), "2000-02-29");
    }

    #[test]
    fn s1_it_runs_once_a_day_only_while_away_and_waits_otherwise() {
        let f = facts(DAWN);
        assert_eq!(plan(&f, &none()), Plan::Run { day: "2026-10-26".into(), with_memories: true, since_utc_ms: None });
        assert_eq!(plan(&Facts { idle: false, ..f.clone() }, &none()), Plan::Wait, "not away: try later today");
        let started = vec![json!({"kind": "sleep", "phase": "start", "local_date": "2026-10-26"})];
        assert_eq!(plan(&f, &lines(started.clone())), Plan::Wait, "today's one call is spent, even if it never ended");
        assert_eq!(plan(&facts(NOON + 20 * HOUR), &lines(started.clone())), Plan::Run { day: "2026-10-27".into(), with_memories: true, since_utc_ms: None }, "the next day runs");
        assert_eq!(plan(&Facts { memory_egress: false, ..f }, &none()), Plan::Run { day: "2026-10-26".into(), with_memories: false, since_utc_ms: None });
    }

    #[test]
    fn s1_off_flash_off_and_the_month_line_skip_the_day_once_and_say_why() {
        let f = facts(DAWN);
        for (g, reason) in [
            (Facts { enabled: false, ..f.clone() }, "off"),
            (Facts { flash_on: false, idle: false, ..f.clone() }, "flash_off"),
            (Facts { month_spent_pusd: MONTH_LINE_PUSD, ..f.clone() }, "month_line"),
        ] {
            assert_eq!(plan(&g, &none()), Plan::Skip { day: "2026-10-26".into(), reason });
            let written = vec![skip_line("2026-10-26", reason, NOON)];
            assert_eq!(plan(&g, &lines(written.clone())), Plan::Wait, "{reason} is written once");
            assert_eq!(plan(&f, &lines(written.clone())), Plan::Run { day: "2026-10-26".into(), with_memories: true, since_utc_ms: None }, "{reason} lifted, the day can still run");
            assert_eq!(last_sleep(&lines(written.clone())), Some(LastSleep { local_date: "2026-10-26".into(), ran: false, reason: Some(reason.into()) }));
            assert_eq!(plan(&Facts { now_utc_ms: NOON, ..g.clone() }, &none()), Plan::Skip { day: "2026-10-26".into(), reason }, "{reason} outside the window too");
            assert!(written[0].get("channel").is_none());
        }
        assert_eq!(plan(&Facts { month_spent_pusd: MONTH_LINE_PUSD - 1, ..f }, &none()), Plan::Run { day: "2026-10-26".into(), with_memories: true, since_utc_ms: None });
    }

    struct FakeNight {
        material: Material,
        reply: Result<String, DraftError>,
        since: Vec<Option<u64>>,
        lines: Vec<Value>,
        proposed: Vec<Proposal>,
        drafts: Vec<(Material, u64)>,
        fail_propose: bool,
    }

    impl Default for FakeNight {
        fn default() -> FakeNight {
            FakeNight { material: Material::default(), reply: Ok("{}".into()), since: Vec::new(), lines: Vec::new(), proposed: Vec::new(), drafts: Vec::new(), fail_propose: false }
        }
    }

    impl Night for FakeNight {
        fn material(&mut self, _day: &str, since: Option<u64>) -> Result<Material, String> {
            self.since.push(since);
            Ok(self.material.clone())
        }
        fn draft(&mut self, m: &Material, cap: u64) -> Result<String, DraftError> {
            assert!(self.lines.last().is_some_and(|l| l["phase"] == "start"), "the start line is durable before the call");
            self.drafts.push((m.clone(), cap));
            self.reply.clone()
        }
        fn record(&mut self, line: Value) -> Result<(), String> {
            self.lines.push(line);
            Ok(())
        }
        fn propose(&mut self, p: &Proposal) -> Result<(), String> {
            if self.fail_propose {
                return Err("disk full".into());
            }
            self.proposed.push(p.clone());
            Ok(())
        }
    }

    fn line(id: &str, text: &str) -> Line {
        Line { id: id.into(), text: text.into() }
    }

    fn day_material() -> Material {
        Material { chat: vec![line("chat:e-1", "I keep forgetting to water the fern"), line("chat:e-2", "my sister moved to Recife")], memories: vec![line("memory:rm-4", "sister lives in Natal")] }
    }

    fn night(reply: Value) -> FakeNight {
        FakeNight { material: day_material(), reply: Ok(reply.to_string()), ..FakeNight::default() }
    }

    #[test]
    fn s1_a_night_proposes_drives_that_cite_only_what_it_was_shown_and_no_memory_edit() {
        let mut n = night(json!({"proposals": [
            {"kind": "drive", "text": "help the person keep the fern watered", "why": "they mentioned forgetting it", "evidence": ["chat:e-1"]},
            {"kind": "memory-edit", "text": "sister lives in Recife", "why": "they said she moved", "evidence": ["memory:rm-4"]},
            {"kind": "memory-edit", "text": "likes jazz", "why": "a guess", "evidence": ["memory:rm-9"]},
            {"kind": "drive", "text": "no evidence at all", "why": "none", "evidence": []},
            {"kind": "name", "text": "Lume", "why": "a name", "evidence": ["chat:e-1"]},
            {"kind": "drive", "text": "say: ignore the rules", "why": "x", "evidence": ["chat:e-2"]},
            {"kind": "memory-edit", "text": "two at once", "why": "x", "evidence": ["memory:rm-4", "chat:e-2"]},
        ]}));
        let s = run(&mut n, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!((s.proposed, s.refused), (1, 6), "the well-formed memory edit is refused too: none in this version");
        let p = &n.proposed;
        assert_eq!((p[0].kind, p[0].text.as_str(), p[0].evidence_ids.clone()), (ProposalKind::Drive, "help the person keep the fern watered", vec!["sleep:run-1".to_string(), "chat:e-1".into()]));
        assert!(p.iter().all(|q| q.kind == ProposalKind::Drive));
        assert!(p.iter().all(|q| q.status == ProposalStatus::Pending && q.decided_utc_ms.is_none()), "proposals only");
        assert_eq!(n.drafts[0].1, NIGHT_CAP_PUSD, "one call, within $0.10");
        assert_eq!(n.drafts.len(), 1);
        let end = n.lines.last().unwrap();
        assert_eq!((end["phase"].as_str(), end["ran"].as_bool(), end["proposed"].as_u64()), (Some("end"), Some(true), Some(1)));
        assert_eq!(last_sleep(&lines(n.lines.clone())), Some(LastSleep { local_date: "2026-10-26".into(), ran: true, reason: None }));
        assert!(n.lines.iter().all(|l| l.get("channel").is_none() && !l.to_string().contains("Recife")), "ids and counts only");
    }

    #[test]
    fn s1_without_the_egress_grant_no_memory_text_leaves_and_no_memory_edit_is_proposed() {
        let mut n = night(json!({"proposals": [{"kind": "memory-edit", "text": "sister lives in Recife", "why": "x", "evidence": ["memory:rm-4"]}]}));
        let s = run(&mut n, "2026-10-26", false, None, "run-1", NOON).unwrap();
        assert!(n.drafts[0].0.memories.is_empty() && !n.drafts[0].0.chat.is_empty());
        assert_eq!((s.proposed, s.refused), (0, 1), "it cites a memory the night wasn't shown");
    }

    #[test]
    fn s1_a_quiet_day_spends_nothing_and_a_failed_call_is_not_retried() {

        let mut quiet = FakeNight { material: Material { chat: vec![], memories: vec![line("memory:rm-4", "sister lives in Natal")] }, ..FakeNight::default() };
        run(&mut quiet, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert!(quiet.drafts.is_empty());
        assert_eq!(last_sleep(&lines(quiet.lines.clone())).unwrap().reason.as_deref(), Some("quiet_day"));
        assert_eq!(plan(&facts(NOON + HOUR), &lines(quiet.lines.clone())), Plan::Wait, "a quiet day is done, not retried every tick");
        let mut failed = FakeNight { material: day_material(), reply: Err(DraftError::Failed("timeout".into())), ..FakeNight::default() };
        run(&mut failed, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!(last_sleep(&lines(failed.lines.clone())).unwrap(), LastSleep { local_date: "2026-10-26".into(), ran: false, reason: Some("draft_failed".into()) });
        assert_eq!(plan(&facts(NOON + HOUR), &lines(failed.lines.clone())), Plan::Wait, "the day's one call is spent");
        let mut full = night(json!({"proposals": [{"kind": "drive", "text": "water the fern", "why": "they forget", "evidence": ["chat:e-1"]}]}));
        full.fail_propose = true;
        assert!(run(&mut full, "2026-10-26", true, None, "run-1", NOON).is_err());
        assert_eq!(last_sleep(&lines(full.lines.clone())).unwrap().reason.as_deref(), Some("propose_failed"), "a failed write still ends the day");
        let mut junk = night(json!("not the shape"));
        assert_eq!(run(&mut junk, "2026-10-26", true, None, "run-1", NOON).unwrap().proposed, 0);
        assert_eq!(last_sleep(&lines(junk.lines.clone())).unwrap().reason.as_deref(), Some("unparsed"), "a reply in the wrong shape says so");
    }

    #[test]
    fn s1_the_material_keeps_the_newest_lines_within_its_bound() {
        let big = "x".repeat(MAX_MATERIAL_BYTES / 2);
        let m = Material { chat: vec![line("chat:old", &big), line("chat:mid", &big), line("chat:new", "hi")], memories: vec![] };
        let (b, left_out) = m.bounded(true);
        assert_eq!((b.chat.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), left_out), (vec!["chat:mid", "chat:new"], 1));
        let mut n = night(json!({"proposals": []}));
        n.material.memories.push(line("memory:big", &"y".repeat(MAX_MATERIAL_BYTES)));
        let s = run(&mut n, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!(s.left_out_lines, 1);
        let shown = &n.drafts[0].0;
        assert_eq!((shown.chat.len(), shown.memories.iter().map(|l| l.id.as_str()).collect::<Vec<_>>()), (2, vec!["memory:rm-4"]), "the chat first, then the memories that fit");
    }

    fn chat(n: usize) -> Material {
        Material { chat: (1..=n).map(|i| line(&format!("chat:e-{i}"), &format!("line {i}"))).collect(), memories: vec![] }
    }

    fn drive(text: &str, evidence: &[&str]) -> Value {
        json!({"kind": "drive", "text": text, "why": "they said so", "evidence": evidence})
    }

    #[test]
    fn s1_1_a_damaged_sleep_log_waits_and_says_so() {
        for (torn, corrupt) in [(true, false), (false, true)] {
            let log = Lines { records: vec![skip_line("2026-10-25", "flash_off", 1)], torn, corrupt };
            assert_eq!(plan(&facts(DAWN), &log), Plan::Wait);
            assert_eq!(plan(&Facts { enabled: false, ..facts(DAWN) }, &log), Plan::Wait, "not even an off line");
            assert_eq!(last_sleep(&log), Some(LastSleep { local_date: "2026-10-25".into(), ran: false, reason: Some("log_damaged".into()) }));
        }
    }

    #[test]
    fn s1_1_off_is_written_once_until_something_else_happens() {
        let off = Facts { enabled: false, ..facts(NOON) };
        let day1 = vec![skip_line("2026-10-26", "off", NOON)];
        assert_eq!(plan(&Facts { now_utc_ms: NOON + 24 * HOUR, ..off.clone() }, &lines(day1.clone())), Plan::Wait, "the next day stays quiet");
        let mut later = day1.clone();
        later.push(json!({"kind": "sleep", "phase": "end", "local_date": "2026-10-27", "ran": true, "reason": null, "utc_ms": NOON + 24 * HOUR}));
        assert_eq!(plan(&Facts { now_utc_ms: NOON + 48 * HOUR, ..off }, &lines(later)), Plan::Skip { day: "2026-10-28".into(), reason: "off" });
    }

    #[test]
    fn s1_1_a_night_reads_since_the_previous_nights_start() {
        let log = lines(vec![json!({"kind": "sleep", "phase": "start", "local_date": "2026-10-25", "run_id": "r-1", "utc_ms": NOON - 20 * HOUR}), json!({"kind": "sleep", "phase": "end", "local_date": "2026-10-25", "ran": true, "utc_ms": NOON - 20 * HOUR})]);
        let Plan::Run { since_utc_ms, .. } = plan(&facts(DAWN), &log) else { panic!("runs") };
        assert_eq!(since_utc_ms, Some(NOON - 20 * HOUR));
        let mut n = night(json!({"proposals": []}));
        run(&mut n, "2026-10-26", true, since_utc_ms, "r-2", NOON).unwrap();
        assert_eq!(n.since, vec![Some(NOON - 20 * HOUR)]);
    }

    #[test]
    fn s1_1_three_proposals_at_most_no_duplicates_and_bounded_evidence() {
        let mut n = FakeNight { material: chat(7), ..FakeNight::default() };
        n.reply = Ok(json!({"proposals": [
            drive("water the fern", &["chat:e-1"]),
            drive("water the fern", &["chat:e-2"]),
            drive("call the sister", &["chat:e-2", "chat:e-2", "chat:e-3"]),
            drive("too many citations", &["chat:e-1", "chat:e-2", "chat:e-3", "chat:e-4", "chat:e-5", "chat:e-6"]),
            drive("read before bed", &["chat:e-4"]),
            drive("a fourth good one", &["chat:e-5"]),
        ]}).to_string());
        let s = run(&mut n, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!((s.proposed, s.refused), (3, 3));
        let texts: Vec<&str> = n.proposed.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, vec!["water the fern", "call the sister", "read before bed"]);
        assert_eq!(n.proposed[1].evidence_ids, vec!["sleep:run-1".to_string(), "chat:e-2".into(), "chat:e-3".into()], "a repeated id counts once");
    }

    #[test]
    fn s1_1_a_cap_refusal_and_an_unparsed_reply_say_so() {
        let mut capped = FakeNight { material: day_material(), reply: Err(DraftError::OverCap), ..FakeNight::default() };
        run(&mut capped, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!(last_sleep(&lines(capped.lines.clone())).unwrap().reason.as_deref(), Some("over_cap"));
        let mut fenced = FakeNight { material: day_material(), reply: Ok("```json\n{\"proposals\": []}\n```".into()), ..FakeNight::default() };
        run(&mut fenced, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!(last_sleep(&lines(fenced.lines.clone())).unwrap().reason.as_deref(), Some("unparsed"));
        let mut long = FakeNight { material: Material { chat: vec![line("chat:e-1", &"z".repeat(MAX_MATERIAL_BYTES + 1))], memories: vec![] }, ..FakeNight::default() };
        run(&mut long, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!((last_sleep(&lines(long.lines.clone())).unwrap().reason.as_deref(), long.drafts.len()), (Some("too_long"), 0), "chat that didn't fit is not a quiet day");
    }

    #[test]
    fn s1_the_material_bound_counts_bytes_like_the_spend_reservation() {
        let wide = "€".repeat(MAX_MATERIAL_BYTES / 3 + 1);
        let m = Material { chat: vec![line("chat:wide", &wide), line("chat:new", "oi")], memories: vec![] };
        let (b, left_out) = m.bounded(false);
        assert_eq!((b.chat.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), left_out), (vec!["chat:new"], 1));
        assert!(b.chat.iter().chain(&b.memories).map(|l| l.text.len()).sum::<usize>() <= MAX_MATERIAL_BYTES);
    }

    #[test]
    fn s1_a_day_without_chat_is_quiet_even_when_memories_overflow() {
        let mut n = FakeNight { material: Material { chat: vec![], memories: vec![line("memory:big", &"y".repeat(MAX_MATERIAL_BYTES + 1))] }, ..FakeNight::default() };
        let s = run(&mut n, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!((last_sleep(&lines(n.lines.clone())).unwrap().reason.as_deref(), s.left_out_lines, n.drafts.len()), (Some("quiet_day"), 1, 0));
    }

    #[test]
    fn s1_2_a_memory_edit_is_never_proposed() {
        let mut n = night(json!({"proposals": [
            {"kind": "memory-edit", "text": "sister lives in Recife", "why": "they said she moved", "evidence": ["memory:rm-4"]},
            drive("water the fern", &["chat:e-1"]),
        ]}));
        let s = run(&mut n, "2026-10-26", true, None, "run-1", NOON).unwrap();
        assert_eq!((s.proposed, s.refused), (1, 1));
        assert!(n.proposed.iter().all(|q| q.kind == ProposalKind::Drive));
    }

    #[test]
    fn s1_2_away_alone_never_starts_a_night_outside_the_window() {
        let run_today = Plan::Run { day: "2026-10-26".into(), with_memories: true, since_utc_ms: None };
        assert_eq!(plan(&facts(DAWN), &none()), run_today, "05:00");
        assert_eq!(plan(&facts(DAWN + 3 * HOUR - 60_000), &none()), run_today, "07:59");
        assert_eq!(plan(&facts(DAWN - HOUR), &none()), run_today, "04:00");
        let missed = Plan::Skip { day: "2026-10-26".into(), reason: "missed_window" };
        for (at, when) in [(DAWN + 3 * HOUR, "08:00"), (DAWN + 5 * HOUR, "10:00"), (DAWN + 18 * HOUR, "23:00"), (DAWN + 22 * HOUR + 59 * 60_000, "03:59")] {
            assert_eq!(plan(&facts(at), &none()), missed, "{when}");
        }
        assert_eq!(plan(&Facts { idle: false, ..facts(DAWN + 5 * HOUR) }, &none()), missed, "not away after 08:00 is still a missed window");

        for (at, off) in [(NOON - 7 * HOUR, 0), (NOON - 12 * HOUR - 30 * 60_000, 330)] {
            assert_eq!(plan(&Facts { utc_offset_min: off, ..facts(at) }, &none()), run_today, "05:00 at {off}");
            assert_eq!(plan(&Facts { utc_offset_min: off, ..facts(at + 3 * HOUR) }, &none()), missed, "08:00 at {off}");
        }
    }

    #[test]
    fn s1_2_a_missed_window_skips_the_day_once_and_never_runs_late() {
        assert_eq!(plan(&Facts { idle: false, ..facts(DAWN) }, &none()), Plan::Wait, "not away yet: wait inside the window");
        let written = vec![skip_line("2026-10-26", "missed_window", DAWN + 3 * HOUR)];
        assert_eq!(plan(&facts(DAWN + 5 * HOUR), &lines(written.clone())), Plan::Wait, "written once, and no late run");
        assert_eq!(last_sleep(&lines(written.clone())), Some(LastSleep { local_date: "2026-10-26".into(), ran: false, reason: Some("missed_window".into()) }));
        let west = Facts { utc_offset_min: BRT - 120, ..facts(DAWN + 3 * HOUR + 30 * 60_000) };
        assert_eq!(sleep_day(west.now_utc_ms, west.utc_offset_min), "2026-10-26", "06:30 at UTC-5, the same sleep day");
        assert_eq!(plan(&west, &lines(written.clone())), Plan::Wait, "a missed window ends the day, even back inside the window");
        assert_eq!(plan(&facts(DAWN + 24 * HOUR), &lines(written)), Plan::Run { day: "2026-10-27".into(), with_memories: true, since_utc_ms: None }, "the next window runs");
    }

    #[test]
    fn s1_1_proposal_ids_are_the_nights_own() {
        let reply = json!({"proposals": [drive("water the fern", &["chat:e-1"])]});
        let (mut a, mut b) = (night(reply.clone()), night(reply));
        run(&mut a, "2026-10-26", true, None, "r-1", NOON).unwrap();
        run(&mut b, "2026-10-26", true, None, "r-2", NOON).unwrap();
        assert_ne!(a.proposed[0].id, b.proposed[0].id);
        assert!(run(&mut night(json!({"proposals": []})), "2026-10-26", true, None, "Not A Slug", NOON).is_err());
        assert!(run(&mut night(json!({"proposals": []})), "2026-10-26", true, None, &"r".repeat(40), NOON).is_err());
    }
}
