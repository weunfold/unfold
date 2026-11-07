

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::spend::write_once;

pub const CAP_PUSD: u64 = 50_000_000_000_000;
pub const STOP_PUSD: u64 = 49_000_000_000_000;

pub const PRICE_IN: u64 = 600_000;
pub const PRICE_CACHED: u64 = 12_000;
pub const PRICE_OUT: u64 = 2_400_000;

pub const BACKGROUND_LINE_PUSD: u64 = 40_000_000_000_000;
pub const REACHOUT_DAY_PUSD: u64 = 20_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Chat,
    Reachout,
    Sleep,
    Preview,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Purpose::Chat => "chat",
            Purpose::Reachout => "reachout",
            Purpose::Sleep => "sleep",
            Purpose::Preview => "preview",
        }
    }

    fn parse(s: &str) -> Option<Purpose> {
        [Purpose::Chat, Purpose::Reachout, Purpose::Sleep, Purpose::Preview].into_iter().find(|p| p.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub month: String,
    pub spent_pusd: u64,
    pub capped: bool,

    pub by_purpose: std::collections::BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    pub month: String,
    pub n: u64,
    pub attempt_id: String,
    pub reserve_pusd: u64,

    pub request_bytes: u64,

    pub max_output_tokens: Option<u64>,
}

pub const STREAM_SLACK_TOKENS: u64 = 64;

#[derive(Debug)]
pub struct Ledger {
    root: PathBuf,
    max_tokens: u64,

    #[cfg(test)]
    pub month: Option<String>,

    #[cfg(test)]
    pub day: Option<String>,
}

pub fn month_of(unix_s: u64) -> String {
    let (y, m, _) = civil(unix_s);
    format!("{y:04}-{m:02}")
}

pub fn day_of(unix_s: u64) -> String {
    let (y, m, d) = civil(unix_s);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil(unix_s: u64) -> (i64, i64, i64) {
    let z = (unix_s / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, doy - (153 * mp + 2) / 5 + 1)
}

pub fn cost(u: &Value) -> Option<u64> {
    let n = |p: &str| u.pointer(p).and_then(Value::as_u64);
    let (prompt, completion) = (n("/prompt_tokens")?, n("/completion_tokens")?);
    let cached = n("/prompt_tokens_details/cached_tokens").or(n("/prompt_cache_hit_tokens")).unwrap_or(0).min(prompt);
    (prompt - cached).checked_mul(PRICE_IN)?.checked_add(cached.checked_mul(PRICE_CACHED)?)?.checked_add(completion.checked_mul(PRICE_OUT)?)
}

struct Month {
    attempts: u64,
    committed: u64,
    stopped: bool,
    by_purpose: std::collections::BTreeMap<String, u64>,

    reachout_days: std::collections::BTreeMap<String, u64>,

    overrun: u64,
}

impl Ledger {

    pub fn open(root: &Path, max_tokens: u32) -> Ledger {
        Ledger {
            root: root.to_path_buf(),
            max_tokens: u64::from(max_tokens),
            #[cfg(test)]
            month: None,
            #[cfg(test)]
            day: None,
        }
    }

    fn month_now(&self) -> String {
        self.now().0
    }

    fn now(&self) -> (String, String) {
        let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        #[cfg(test)]
        if let Some(m) = &self.month {
            return (m.clone(), self.day.clone().unwrap_or(format!("{m}-01")));
        }
        (month_of(s), day_of(s))
    }

    pub fn reserve_for(&self, request_bytes: usize) -> u64 {
        (request_bytes as u64).saturating_mul(PRICE_IN).saturating_add(self.max_tokens.saturating_mul(PRICE_OUT))
    }

    fn read(&self, month: &str) -> Result<Month, String> {
        let dir = self.root.join(month);
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Month { attempts: 0, committed: 0, stopped: false, by_purpose: Default::default(), reachout_days: Default::default(), overrun: 0 }),
            Err(e) => return Err(format!("flash spend: {month} unreadable ({e})")),
        };
        let (mut attempts, mut settles) = (std::collections::BTreeMap::new(), std::collections::BTreeMap::new());
        let mut stopped = false;
        for e in entries {
            let name = e.map_err(|e| format!("flash spend: {month} unreadable ({e})"))?.file_name().to_string_lossy().to_string();
            if name.contains(".tmp-") {
                continue;
            }
            if name == "stop.json" {
                stopped = true;
                continue;
            }
            let number = |s: &str| -> Option<u64> {
                let d = s.strip_prefix("call-")?;
                if d.len() == 6 && d.bytes().all(|b| b.is_ascii_digit()) { d.parse().ok() } else { None }
            };
            let v: Value = fs::read(dir.join(&name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(format!("flash spend: {month}/{name} does not parse"))?;
            if let Some(n) = name.strip_suffix(".settle.json").and_then(number) {
                settles.insert(n, v);
            } else if let Some(n) = name.strip_suffix(".json").and_then(number) {
                attempts.insert(n, v);
            } else {
                return Err(format!("flash spend: foreign file {month}/{name}"));
            }
        }
        let (mut committed, mut overrun): (u64, u64) = (0, 0);
        let (mut by_purpose, mut reachout_days) = (std::collections::BTreeMap::<String, u64>::new(), std::collections::BTreeMap::<String, u64>::new());
        for (i, (n, a)) in attempts.iter().enumerate() {
            let reserve = a["reserve_pusd"].as_u64();
            if i as u64 != *n || a["n"] != *n || a["month"] != month || reserve.is_none() || !a["attempt_id"].is_string() {
                return Err(format!("flash spend: reservation {month}/{n} is malformed or out of order"));
            }

            let purpose = match a.get("purpose") {
                None => Purpose::Chat,
                Some(p) => p.as_str().and_then(Purpose::parse).ok_or(format!("flash spend: reservation {month}/{n} has an unknown purpose"))?,
            };
            let day = a.get("day").and_then(Value::as_str).filter(|d| d.len() == 10 && d.starts_with(&format!("{month}-")));
            if (purpose != Purpose::Chat || a.get("day").is_some()) && day.is_none() {
                return Err(format!("flash spend: reservation {month}/{n} has no day in its month"));
            }
            let charged = match settles.get(n) {
                None => {
                    overrun = overrun.saturating_add(a["worst_pusd"].as_u64().unwrap_or(0).saturating_sub(reserve.unwrap()));
                    reserve.unwrap()
                }
                Some(s) => {
                    let c = s["cost_pusd"].as_u64().ok_or(format!("flash spend: settlement {month}/{n} has no cost"))?;
                    let priced = if s["usage"].is_null() { reserve } else { cost(&s["usage"]) };
                    if s["attempt_id"] != a["attempt_id"] || priced != Some(c) {
                        return Err(format!("flash spend: settlement {month}/{n} is not its usage at the pinned prices"));
                    }
                    c
                }
            };
            committed = committed.saturating_add(charged);
            let e = by_purpose.entry(purpose.as_str().to_string()).or_default();
            *e = e.saturating_add(charged);
            if let (Purpose::Reachout, Some(d)) = (purpose, day) {
                let e = reachout_days.entry(d.to_string()).or_default();
                *e = e.saturating_add(charged);
            }
        }
        if let Some(k) = settles.keys().find(|k| !attempts.contains_key(*k)) {
            return Err(format!("flash spend: settlement {month}/{k} without its reservation"));
        }
        Ok(Month { attempts: attempts.len() as u64, committed, stopped, by_purpose, reachout_days, overrun })
    }

    pub fn status(&self) -> Status {
        let month = self.month_now();
        match self.read(&month) {
            Ok(m) => Status { capped: m.stopped || m.committed >= STOP_PUSD, spent_pusd: m.committed, month, by_purpose: m.by_purpose },
            Err(_) => Status { month, spent_pusd: CAP_PUSD, capped: true, by_purpose: Default::default() },
        }
    }

    pub fn admit(&self, attempt_id: &str, request_bytes: usize) -> Result<Reservation, String> {
        self.admit_for(attempt_id, request_bytes, Purpose::Chat)
    }

    pub fn admit_for(&self, attempt_id: &str, request_bytes: usize, purpose: Purpose) -> Result<Reservation, String> {
        self.admit_reserve(attempt_id, request_bytes, self.reserve_for(request_bytes), None, purpose)
    }

    pub fn admit_tokens(&self, attempt_id: &str, request_bytes: usize, output_tokens: u32, max_tokens: u32) -> Result<Reservation, String> {
        let input = (request_bytes as u64).saturating_mul(PRICE_IN);
        let estimate = input.saturating_add(u64::from(output_tokens).saturating_mul(PRICE_OUT));
        let worst = input.saturating_add(u64::from(max_tokens.max(output_tokens)).saturating_mul(PRICE_OUT));
        let max_output_tokens = Some(u64::from(max_tokens.max(output_tokens)));
        self.admit_reserve(attempt_id, request_bytes, estimate, Some(worst), Purpose::Chat).map(|r| Reservation { max_output_tokens, ..r })
    }

    fn admit_reserve(&self, attempt_id: &str, request_bytes: usize, estimate: u64, worst: Option<u64>, purpose: Purpose) -> Result<Reservation, String> {
        let (month, day) = self.now();
        for _ in 0..64 {
            let m = self.read(&month)?;
            if m.stopped || m.committed >= STOP_PUSD {
                return Err(format!("flash spend: {month} reached its stop (${:.2} spent)", m.committed as f64 / 1e12));
            }
            let reserve = match worst {
                Some(w) if m.committed.saturating_add(m.overrun).saturating_add(w) > STOP_PUSD => w,
                _ => estimate,
            };
            if m.committed.saturating_add(m.overrun).saturating_add(reserve) > CAP_PUSD {
                return Err(format!("flash spend: this call could pass {month}'s cap"));
            }
            if purpose != Purpose::Chat && (m.committed >= BACKGROUND_LINE_PUSD || m.committed.saturating_add(reserve) > BACKGROUND_LINE_PUSD) {
                return Err(format!("flash spend: {} stops at {month}'s background line ($40)", purpose.as_str()));
            }
            let today = m.reachout_days.get(&day).copied().unwrap_or(0);
            if purpose == Purpose::Reachout && (today >= REACHOUT_DAY_PUSD || today.saturating_add(reserve) > REACHOUT_DAY_PUSD) {
                return Err(format!("flash spend: reachout reached its day ceiling ($0.02 on {day})"));
            }
            fs::create_dir_all(self.root.join(&month)).map_err(|e| format!("flash spend: {e}"))?;
            let mut v = json!({ "n": m.attempts, "month": month, "attempt_id": attempt_id, "reserve_pusd": reserve, "purpose": purpose.as_str(), "day": day });
            if let Some(w) = worst {
                v["worst_pusd"] = json!(w);
            }
            match write_once(&self.root.join(&month).join(format!("call-{:06}.json", m.attempts)), &v) {
                Ok(()) => return Ok(Reservation { month, n: m.attempts, attempt_id: attempt_id.into(), reserve_pusd: reserve, request_bytes: request_bytes as u64, max_output_tokens: None }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("flash spend: reservation not written ({e})")),
            }
        }
        Err("flash spend: no reservation could be claimed".into())
    }

    pub fn settle_streamed(&self, r: &Reservation, usage: Option<&Value>, streamed_bytes: u64) -> Status {
        if usage.and_then(cost).is_some() {
            return self.settle(r, usage);
        }
        let completion = streamed_bytes.saturating_add(STREAM_SLACK_TOKENS);
        let completion = r.max_output_tokens.map_or(completion, |m| completion.min(m));
        let counted = json!({"prompt_tokens": r.request_bytes, "completion_tokens": completion, "counted_from_stream": true});
        match cost(&counted) {
            Some(c) if c > r.reserve_pusd => self.settle(r, Some(&counted)),
            _ => self.settle(r, None),
        }
    }

    pub fn settle(&self, r: &Reservation, usage: Option<&Value>) -> Status {
        let usage = usage.filter(|u| cost(u).is_some());
        let charged = usage.and_then(cost).unwrap_or(r.reserve_pusd);
        let s = json!({ "n": r.n, "attempt_id": r.attempt_id, "usage": usage, "cost_pusd": charged });
        let _ = write_once(&self.root.join(&r.month).join(format!("call-{:06}.settle.json", r.n)), &s);
        let status = match self.read(&r.month) {
            Ok(m) => Status { capped: m.stopped || m.committed >= STOP_PUSD, spent_pusd: m.committed, month: r.month.clone(), by_purpose: m.by_purpose },
            Err(_) => Status { month: r.month.clone(), spent_pusd: CAP_PUSD, capped: true, by_purpose: Default::default() },
        };
        if status.capped {
            let _ = write_once(&self.root.join(&r.month).join("stop.json"), &json!({ "spent_pusd": status.spent_pusd, "stop_pusd": STOP_PUSD }));
        }
        status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger(name: &str) -> Ledger {
        let dir = std::env::temp_dir().join(format!("agentd-flash-spend-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut l = Ledger::open(&dir, 400);
        l.month = Some("2026-10".into());
        l.day = Some("2026-10-26".into());
        l
    }

    fn settle_cost(l: &Ledger, r: &Reservation, pusd: u64) -> Status {
        assert_eq!(pusd % PRICE_OUT, 0, "a cost in whole output tokens");
        l.settle(r, Some(&json!({"prompt_tokens": 0, "completion_tokens": pusd / PRICE_OUT})))
    }

    fn put(l: &Ledger, n: u64, purpose: &str, reserve: u64) {
        fs::create_dir_all(l.root.join("2026-10")).unwrap();
        let v = json!({"n": n, "month": "2026-10", "attempt_id": format!("p-{n}"), "reserve_pusd": reserve, "purpose": purpose, "day": "2026-10-26"});
        fs::write(l.root.join(format!("2026-10/call-{n:06}.json")), v.to_string()).unwrap();
    }

    fn row(l: &Ledger, n: u64) -> Value {
        serde_json::from_slice(&fs::read(l.root.join(format!("2026-10/call-{n:06}.json"))).unwrap()).unwrap()
    }

    #[test]
    fn b2_a_reachout_reservation_names_its_purpose_and_reads_back() {
        let l = ledger("b2-tag");
        let r = l.admit_for("x-1-0", 10, Purpose::Reachout).unwrap();
        assert_eq!(row(&l, r.n)["purpose"], "reachout");
        assert_eq!(row(&l, r.n)["day"], "2026-10-26");
        let c = l.admit("r-1", 10).unwrap();
        assert_eq!(row(&l, c.n)["purpose"], "chat");
        let s = l.status();
        assert!(!s.capped, "a tagged row reads: {s:?}");
        assert_eq!(s.by_purpose.get("reachout"), Some(&r.reserve_pusd));
        assert_eq!(s.by_purpose.get("chat"), Some(&c.reserve_pusd));
    }

    #[test]
    fn b2_a_row_without_a_purpose_counts_as_chat() {
        let l = ledger("b2-untagged");
        fs::create_dir_all(l.root.join("2026-10")).unwrap();
        fs::write(l.root.join("2026-10/call-000000.json"), r#"{"n":0,"month":"2026-10","attempt_id":"r-0","reserve_pusd":7}"#).unwrap();
        let s = l.status();
        assert!(!s.capped && s.spent_pusd == 7, "{s:?}");
        assert_eq!(s.by_purpose, [("chat".to_string(), 7)].into_iter().collect());
    }

    #[test]
    fn b2_background_stops_at_40_dollars_while_chat_goes_on() {
        let l = ledger("b2-line");
        put(&l, 0, "chat", BACKGROUND_LINE_PUSD - 1);

        for p in [Purpose::Reachout, Purpose::Sleep, Purpose::Preview] {
            assert!(l.admit_for("x", 0, p).unwrap_err().contains("background line"), "{p:?}");
        }
        assert!(l.admit("chat", 1).is_ok(), "chat is still admitted");
    }

    #[test]
    fn chat_reserves_its_estimate_settles_its_usage_and_still_stops() {
        let l = ledger("reply-limit-estimate");
        let r = l.admit_tokens("c-1", 100, 400, crate::flash::REPLY_LIMIT).unwrap();
        assert_eq!(r.reserve_pusd, 100 * PRICE_IN + 400 * PRICE_OUT);
        assert_eq!(l.settle(&r, Some(&json!({"prompt_tokens": 10, "completion_tokens": 5000}))).spent_pusd, 10 * PRICE_IN + 5000 * PRICE_OUT);
        put(&l, 1, "chat", STOP_PUSD);
        assert!(l.admit_tokens("c-2", 0, 1, 1).unwrap_err().contains("stop"));
        let worst = u64::from(crate::flash::N_CTX) * PRICE_IN + u64::from(crate::flash::REPLY_LIMIT) * PRICE_OUT;
        assert!(worst < CAP_PUSD - STOP_PUSD, "one reply at the limit stays under the cap: {worst}");
    }

    #[test]
    fn concurrent_chat_calls_never_pass_the_cap() {
        let l = ledger("long-reply-concurrent");
        let limit = crate::flash::REPLY_LIMIT;
        let worst = 1000 * PRICE_IN + u64::from(limit) * PRICE_OUT;
        put(&l, 0, "chat", STOP_PUSD - 3 * worst);
        let mut admitted = Vec::new();
        for i in 1..=6 {
            match l.admit_tokens(&format!("c-{i}"), 1000, 400, limit) {
                Ok(r) => admitted.push(r),
                Err(why) => assert!(why.contains("cap"), "{why}"),
            }
        }
        let estimate = 1000 * PRICE_IN + 400 * PRICE_OUT;
        assert_eq!(admitted[0].reserve_pusd, estimate, "far from the stop: the estimate");
        assert!(admitted.iter().any(|r| r.reserve_pusd == worst), "near it: the worst case");

        let mut spent = 0;
        for r in &admitted {
            spent = l.settle(r, Some(&json!({"prompt_tokens": 1000, "completion_tokens": limit}))).spent_pusd;
        }
        assert!(spent <= CAP_PUSD, "{} calls spent {spent}", admitted.len());
    }

    #[test]
    fn a_call_with_no_usage_settles_at_what_it_streamed() {
        let l = ledger("long-reply-streamed");
        let r = l.admit_tokens("c-1", 100, 400, crate::flash::REPLY_LIMIT).unwrap();
        let s = l.settle_streamed(&r, None, 10_000);
        assert_eq!(s.spent_pusd, 100 * PRICE_IN + (10_000 + STREAM_SLACK_TOKENS) * PRICE_OUT);
        let r = l.admit_tokens("c-2", 100, 400, crate::flash::REPLY_LIMIT).unwrap();
        let before = l.status().spent_pusd - r.reserve_pusd;
        assert_eq!(l.settle_streamed(&r, None, 10).spent_pusd - before, r.reserve_pusd, "a short stream keeps its reservation");
        let r = l.admit_tokens("c-3", 100, 400, crate::flash::REPLY_LIMIT).unwrap();
        let before = l.status().spent_pusd - r.reserve_pusd;
        assert_eq!(l.settle_streamed(&r, Some(&json!({"prompt_tokens": 5, "completion_tokens": 7})), 99_999).spent_pusd - before, 5 * PRICE_IN + 7 * PRICE_OUT, "a usage event wins");
    }

    #[test]
    fn a_stream_counted_settlement_stays_within_the_worst_case() {
        let l = ledger("local-reply-clamp");
        let r = l.admit_tokens("c-1", 100, 400, 1000).unwrap();
        let s = l.settle_streamed(&r, None, 1_000_000);
        assert_eq!(s.spent_pusd, 100 * PRICE_IN + 1000 * PRICE_OUT, "clamped to the 1000 tokens sent");
        let r = l.admit_tokens("c-2", 100, 400, 1000).unwrap();
        let before = l.status().spent_pusd - r.reserve_pusd;
        assert_eq!(l.settle_streamed(&r, None, 500).spent_pusd - before, 100 * PRICE_IN + (500 + STREAM_SLACK_TOKENS) * PRICE_OUT, "under the limit: what streamed");
    }

    #[test]
    fn b2_at_exactly_40_dollars_a_reachout_is_refused() {

        let mut l = ledger("b2-exact");
        put(&l, 0, "chat", BACKGROUND_LINE_PUSD);
        l.max_tokens = 0;
        assert_eq!(l.reserve_for(0), 0);
        assert!(l.admit_for("x", 0, Purpose::Reachout).unwrap_err().contains("background line"));
        assert!(l.admit_for("c", 0, Purpose::Chat).is_ok());
    }

    #[test]
    fn b2_reachout_stops_at_two_cents_a_day_and_the_next_day_admits() {
        let mut l = ledger("b2-day");
        put(&l, 0, "reachout", REACHOUT_DAY_PUSD - 2 * PRICE_OUT);

        assert!(l.admit_for("x-1-1", 0, Purpose::Reachout).unwrap_err().contains("day"));
        l.max_tokens = 2;
        let r = l.admit_for("x-1-2", 0, Purpose::Reachout).unwrap();
        settle_cost(&l, &r, 2 * PRICE_OUT);
        l.max_tokens = 0;
        assert!(l.admit_for("x-1-3", 0, Purpose::Reachout).unwrap_err().contains("day"), "at exactly $0.02");
        assert!(l.admit_for("s", 0, Purpose::Sleep).is_ok(), "the day ceiling is reachout's alone");
        assert!(l.admit("c", 0).is_ok());
        l.day = Some("2026-10-27".into());
        assert!(l.admit_for("x-2-0", 0, Purpose::Reachout).is_ok(), "a new UTC day");
    }

    #[test]
    fn b2_chat_and_reachout_admitted_together_claim_distinct_numbers() {
        let base = ledger("b2-race");
        let (root, month, day) = (base.root.clone(), base.month.clone(), base.day.clone());
        let threads: Vec<_> = (0..8u64)
            .map(|i| {
                let (root, month, day) = (root.clone(), month.clone(), day.clone());
                std::thread::spawn(move || {
                    let mut l = Ledger::open(&root, 1);
                    (l.month, l.day) = (month, day);
                    let p = if i % 2 == 0 { Purpose::Chat } else { Purpose::Reachout };
                    let r = l.admit_for(&format!("a-{i}"), 0, p).unwrap();
                    settle_cost(&l, &r, PRICE_OUT * (i + 1));
                    r.n
                })
            })
            .collect();
        let mut ns: Vec<u64> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        ns.sort();
        assert_eq!(ns, (0..8).collect::<Vec<_>>());
        let s = base.status();
        assert_eq!(s.spent_pusd, PRICE_OUT * (1..=8).sum::<u64>());
        assert_eq!(s.by_purpose.get("chat"), Some(&(PRICE_OUT * (1 + 3 + 5 + 7))));
        assert_eq!(s.by_purpose.get("reachout"), Some(&(PRICE_OUT * (2 + 4 + 6 + 8))));
    }

    #[test]
    fn b2_an_unknown_purpose_reads_as_torn() {
        let l = ledger("b2-foreign");
        fs::create_dir_all(l.root.join("2026-10")).unwrap();
        fs::write(l.root.join("2026-10/call-000000.json"), r#"{"n":0,"month":"2026-10","attempt_id":"r-0","reserve_pusd":7,"purpose":"ads"}"#).unwrap();
        assert!(l.status().capped && l.admit("r", 1).is_err());
        let l = ledger("b2-noday");
        fs::create_dir_all(l.root.join("2026-10")).unwrap();
        fs::write(l.root.join("2026-10/call-000000.json"), r#"{"n":0,"month":"2026-10","attempt_id":"x-0","reserve_pusd":7,"purpose":"reachout"}"#).unwrap();
        assert!(l.status().capped, "a background row names its day");
    }

    #[test]
    fn opening_writes_nothing_and_status_reads_an_empty_month() {
        let l = ledger("open");
        assert!(!l.root.exists(), "no directory until a call is admitted");
        assert_eq!(l.status(), Status { month: "2026-10".into(), spent_pusd: 0, capped: false, by_purpose: Default::default() });
        assert!(!l.root.exists());
    }

    #[test]
    fn months_are_utc_calendar_months() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(1_792_918_457), "2026-10", "the smoke's run_ts");
        assert_eq!(month_of(1_793_491_199), "2026-10", "2026-10-31 23:59:59 UTC");
        assert_eq!(month_of(1_793_491_200), "2026-11");
        assert_eq!(month_of(951_782_400), "2000-02", "2000-02-29");
    }

    #[test]
    fn a_call_costs_its_usage_at_the_bench_prices() {

        let u = json!({"prompt_tokens": 1454, "completion_tokens": 11, "prompt_tokens_details": {"cached_tokens": 640}});
        assert_eq!(cost(&u), Some(522_480_000));
        assert_eq!(cost(&json!({"prompt_tokens": 10, "completion_tokens": 1, "prompt_cache_hit_tokens": 10})), Some(10 * PRICE_CACHED + PRICE_OUT));
        assert_eq!(cost(&json!({"completion_tokens": 1})), None);
    }

    #[test]
    fn a_reservation_is_charged_until_its_usage_settles_it() {
        let l = ledger("settle");
        let r = l.admit("r-1", 1000).unwrap();
        assert_eq!(r.reserve_pusd, 1000 * PRICE_IN + 400 * PRICE_OUT);
        assert_eq!((l.status().spent_pusd, l.status().capped), (r.reserve_pusd, false));
        let s = l.settle(&r, Some(&json!({"prompt_tokens": 100, "completion_tokens": 10})));
        assert_eq!(s.spent_pusd, 100 * PRICE_IN + 10 * PRICE_OUT);
        assert_eq!(l.status(), s, "read back from the files");

        let r2 = l.admit("r-2", 1000).unwrap();
        assert_eq!(l.settle(&r2, None).spent_pusd, s.spent_pusd + r2.reserve_pusd);
        assert_eq!(r2.n, 1);
    }

    #[test]
    fn at_49_dollars_the_month_stops_and_the_next_month_starts_fresh() {
        let mut l = ledger("stop");

        let r = l.admit("big", 1).unwrap();
        let s = l.settle(&r, Some(&json!({"prompt_tokens": 0, "completion_tokens": (STOP_PUSD - 1) / PRICE_OUT})));
        assert!(!s.capped, "{s:?}");
        let r = l.admit("cross", 1).unwrap();
        let s = l.settle(&r, Some(&json!({"prompt_tokens": 0, "completion_tokens": 2})));
        assert!(s.capped && s.spent_pusd >= STOP_PUSD, "{s:?}");
        assert!(l.admit("after", 1).unwrap_err().contains("reached its stop"));
        assert!(l.root.join("2026-10/stop.json").exists());
        l.month = Some("2026-11".into());
        assert!(l.admit("november", 1).is_ok(), "a new month has its own cap");
    }

    #[test]
    fn a_call_that_could_pass_the_cap_is_refused_before_it_is_made() {
        let l = ledger("cap");
        let near = CAP_PUSD - 1000;
        let r = l.admit("near", 1).unwrap();

        let s = l.settle(&r, Some(&json!({"prompt_tokens": 0, "completion_tokens": (STOP_PUSD - 1) / PRICE_OUT})));
        assert!(!s.capped);
        let huge = (near / PRICE_IN) as usize;
        assert!(l.admit("huge", huge).unwrap_err().contains("could pass"), "its worst case is above the cap");
        assert!(l.admit("small", 100).is_ok());
    }

    #[test]
    fn a_torn_or_foreign_ledger_reads_as_capped() {
        let l = ledger("torn");
        l.admit("r-1", 10).unwrap();
        fs::write(l.root.join("2026-10/call-000000.settle.json"), r#"{"n":0,"attempt_id":"r-1","usage":null,"cost_pusd":1}"#).unwrap();
        assert!(l.status().capped, "a settlement that is not its reservation's");
        assert!(l.admit("r-2", 10).is_err());
        let l = ledger("foreign");
        fs::create_dir_all(l.root.join("2026-10")).unwrap();
        fs::write(l.root.join("2026-10/notes.txt"), "x").unwrap();
        assert!(l.status().capped && l.admit("r", 1).is_err());
    }
}
