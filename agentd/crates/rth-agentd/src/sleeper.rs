

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rth_protocol::Event;
use rth_self::Proposal;
use rth_self::consent::{Channel, Grant};
use rth_self::sleep::{self, DraftError, Facts, Line, Material, Night, Plan, Slept};
use rth_self::store as files;
use serde_json::{Value, json};

use crate::context::Turn;
use crate::flash_side::{FlashSide, SideCall, SideError, SideReply};
use crate::flash_spend::{Ledger, Purpose};
use crate::Agent;
use crate::self_store::SelfStore;

pub const HOST_NONE: u8 = 0;

pub const HOST_FLASH_SIDE: u8 = 1;

pub const HOST_NO_FLASH_SIDE: u8 = 2;

pub const TICK: Duration = Duration::from_secs(5 * 60);

pub const MAX_TOKENS: u32 = crate::flash::SIDE_MAX_TOKENS;
pub const DEADLINE: Duration = Duration::from_secs(60);

const CALL_SLACK: Duration = Duration::from_secs(30);

pub const IDLE: Duration = Duration::from_secs(30 * 60);

pub fn fact(host: u8, consents: &[Grant]) -> Value {
    let why_not = if host == HOST_NONE {
        Some("no_host")
    } else if !consents.iter().any(|g| g.channel == Channel::Sleep) {
        Some("not_enabled")
    } else if host != HOST_FLASH_SIDE {
        Some("flash_side_off")
    } else {
        None
    };
    json!({"running": why_not.is_none(), "why_not": why_not})
}

pub trait Presence {
    fn last_active_utc_ms(&self) -> u64;
}

pub struct JournalPresence<'a> {
    pub turns: &'a [Turn],
    pub times: &'a HashMap<String, u64>,
    pub started_utc_ms: u64,
}

impl Presence for JournalPresence<'_> {
    fn last_active_utc_ms(&self) -> u64 {
        self.turns.iter().filter(|t| t.role == "user").filter_map(|t| self.times.get(&t.event).copied()).fold(self.started_utc_ms, u64::max)
    }
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub now_utc_ms: u64,
    pub utc_offset_min: i32,
    pub last_active_utc_ms: u64,
    pub consented: bool,
    pub flash_side: bool,
    pub month_spent_pusd: u64,
}

pub fn facts(s: &Seen) -> Facts {
    let idle = s.now_utc_ms.saturating_sub(s.last_active_utc_ms) >= IDLE.as_millis() as u64;
    Facts { now_utc_ms: s.now_utc_ms, utc_offset_min: s.utc_offset_min, enabled: s.consented, idle, flash_on: s.flash_side, memory_egress: false, month_spent_pusd: s.month_spent_pusd }
}

pub fn chat_lines(turns: &[Turn], times: &HashMap<String, u64>, since_utc_ms: Option<u64>, until_utc_ms: u64) -> Vec<Line> {
    turns
        .iter()
        .filter(|t| times.get(&t.event).is_some_and(|&at| since_utc_ms.is_none_or(|s| at > s) && at <= until_utc_ms))
        .map(|t| Line { id: format!("chat:{}", t.event), text: format!("{}: {}", if t.role == "user" { "person" } else { "assistant" }, t.text) })
        .collect()
}

pub fn repair_torn_tail(path: &Path) -> std::io::Result<bool> {
    let lines = match files::read_lines(path) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if !lines.torn || lines.corrupt {
        return Ok(false);
    }
    let bytes = std::fs::read(path)?;
    let keep = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.set_len(keep as u64)?;
    f.sync_all()?;
    Ok(true)
}

const SYSTEM: &str = "You reflect, once a night, on a day of conversation between a person and their assistant. \
Read the lines you are given. Propose at most three drives: short goals the assistant could hold for the person, \
each grounded in what was said. Reply with JSON only, in the given schema. \
Each text and why is one line of at most 80 characters, with no quotes, colons, backticks, angle brackets or line breaks. \
Each drive cites one to five line ids, exactly as given. \
If nothing stands out, reply with no proposals. \
Never propose contacting anyone, spending money, or changing what is remembered.";

fn alias(i: usize) -> String {
    format!("l{}", i + 1)
}

fn unalias(text: &str, m: &Material) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(text) else { return text.to_string() };
    let ids: HashMap<String, &str> = m.chat.iter().enumerate().map(|(i, l)| (alias(i), l.id.as_str())).collect();
    for p in v.get_mut("proposals").and_then(Value::as_array_mut).into_iter().flatten() {
        for e in p.get_mut("evidence").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(id) = e.as_str().and_then(|a| ids.get(a)) {
                *e = json!(id);
            }
        }
    }
    v.to_string()
}

pub fn request(m: &Material) -> (String, String, Value) {
    let lines: Vec<Value> = m.chat.iter().enumerate().map(|(i, l)| json!({"id": alias(i), "text": l.text})).collect();
    let schema = json!({"type": "json_schema", "json_schema": {"name": "sleep", "strict": true, "schema": {
        "type": "object",
        "properties": {"proposals": {"type": "array", "items": {
            "type": "object",
            "properties": {"kind": {"type": "string", "enum": ["drive"]}, "text": {"type": "string"}, "why": {"type": "string"}, "evidence": {"type": "array", "items": {"type": "string"}}},
            "required": ["kind", "text", "why", "evidence"],
            "additionalProperties": false,
        }}},
        "required": ["proposals"],
        "additionalProperties": false,
    }}});
    (SYSTEM.to_string(), json!({"lines": lines}).to_string(), schema)
}

pub fn draft(m: &Material, cap_pusd: u64, reserve: &dyn Fn(usize) -> u64, call: &mut dyn FnMut(SideCall) -> Result<SideReply, SideError>) -> Result<String, DraftError> {
    let (system, user, schema) = request(m);
    let c = SideCall { purpose: Purpose::Sleep, system, user, schema, max_tokens: MAX_TOKENS, carries_memory: false, deadline: DEADLINE };
    if reserve(FlashSide::admitted_bytes(&c)) > cap_pusd {
        return Err(DraftError::OverCap);
    }
    match call(c) {
        Ok(r) => Ok(unalias(&r.text, m)),
        Err(SideError::Refused(_)) => Err(DraftError::OverCap),
        Err(e) => Err(DraftError::Failed(
            match e {
                SideError::FlashOff => "flash_off",
                SideError::BreakerOpen => "breaker_open",
                SideError::NoEgressGrant => "no_egress_grant",
                SideError::Timeout => "timeout",
                SideError::Unavailable(_) => "unavailable",
                SideError::Invalid(_) | SideError::Refused(_) => "invalid",
            }
            .into(),
        )),
    }
}

pub struct HostNight<'a> {
    pub log: &'a Path,
    pub now_utc_ms: u64,
    pub chat: &'a dyn Fn(Option<u64>, u64) -> Vec<Line>,
    pub draft: &'a mut dyn FnMut(&Material, u64) -> Result<String, DraftError>,
    pub propose: &'a dyn Fn(&Proposal) -> std::io::Result<()>,
}

impl Night for HostNight<'_> {
    fn material(&mut self, _day: &str, since_utc_ms: Option<u64>) -> Result<Material, String> {
        Ok(Material { chat: (self.chat)(since_utc_ms, self.now_utc_ms), memories: Vec::new() })
    }
    fn draft(&mut self, material: &Material, cap_pusd: u64) -> Result<String, DraftError> {
        (self.draft)(material, cap_pusd)
    }
    fn record(&mut self, line: Value) -> Result<(), String> {
        files::append(self.log, &line).map_err(|e| format!("sleep log not written: {e}"))
    }
    fn propose(&mut self, p: &Proposal) -> Result<(), String> {
        (self.propose)(p).map_err(|e| format!("proposal not written: {e}"))
    }
}

#[derive(Debug, PartialEq)]
pub enum Ticked {
    Waited,
    Skipped(&'static str),
    Ran(Slept),
}

pub fn tick(seen: &Seen, night: &mut HostNight, run_id: &str) -> Result<Ticked, String> {
    repair_torn_tail(night.log).map_err(|e| format!("sleep log not repaired: {e}"))?;
    let f = facts(seen);
    let lines = match files::read_lines(night.log) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => files::Lines::default(),
        Err(e) => return Err(format!("sleep log unreadable: {e}")),
    };
    match sleep::plan(&f, &lines) {
        Plan::Wait => Ok(Ticked::Waited),
        Plan::Skip { day, reason } => {
            night.record(sleep::skip_line(&day, reason, seen.now_utc_ms))?;
            Ok(Ticked::Skipped(reason))
        }
        Plan::Run { day, with_memories, since_utc_ms } => sleep::run(night, &day, with_memories, since_utc_ms, run_id, seen.now_utc_ms).map(Ticked::Ran),
    }
}

pub fn run_id(now_utc_ms: u64) -> String {
    format!("n{now_utc_ms}")
}

pub const DATES_FILE: &str = "sleep-dates.json";

#[derive(Debug, Default)]
pub struct Dates {
    known: HashMap<String, Option<u64>>,
    primed: bool,
    changed: bool,
}

impl Dates {

    pub fn load(path: &Path) -> Dates {
        let kept: HashMap<String, u64> = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Dates { known: kept.into_iter().map(|(id, at)| (id, Some(at))).collect(), ..Dates::default() }
    }

    pub fn see(&mut self, turns: &[Turn], now_utc_ms: u64) -> HashMap<String, u64> {
        let at = self.primed.then_some(now_utc_ms);
        let ids: std::collections::HashSet<&String> = turns.iter().map(|t| &t.event).collect();
        let before = self.known.len();
        self.known.retain(|id, _| ids.contains(id));
        self.changed |= self.known.len() != before;
        for t in turns {
            if !self.known.contains_key(&t.event) {
                self.known.insert(t.event.clone(), at);
                self.changed |= at.is_some();
            }
        }
        self.primed = true;
        self.known.iter().filter_map(|(id, at)| Some((id.clone(), (*at)?))).collect()
    }

    pub fn save(&mut self, path: &Path, since_utc_ms: Option<u64>, night_ran: bool) -> std::io::Result<()> {
        if !self.changed && !night_ran {
            return Ok(());
        }
        let kept: HashMap<&String, u64> = self.known.iter().filter_map(|(id, at)| Some((id, (*at)?))).filter(|(_, at)| since_utc_ms.is_none_or(|s| *at > s)).collect();
        files::write_atomic(path, &serde_json::to_vec(&kept).expect("dates serialize"))?;
        self.changed = false;
        Ok(())
    }
}

pub fn kept_dates(store: &SelfStore) -> Dates {
    Dates::load(&store.sleep_log().with_file_name(DATES_FILE))
}

fn last_start(log: &Path) -> Option<u64> {
    files::read_lines(log).ok()?.records.iter().rev().filter(|v| v["kind"] == "sleep" && v["phase"] == "start").find_map(|v| v["utc_ms"].as_u64())
}

pub struct Host {
    pub store: Arc<SelfStore>,
    pub side: Option<Arc<FlashSide>>,
    pub ledger: Option<Arc<Ledger>>,
    pub started_utc_ms: u64,
}

impl Host {

    pub fn tick(&self, turns: &[Turn], times: &HashMap<String, u64>, now_utc_ms: u64, utc_offset_min: i32, handle: &tokio::runtime::Handle) -> Result<Ticked, String> {
        let consents = self.store.consents().map_err(|e| format!("consents unreadable: {e}"))?;
        let seen = Seen {
            now_utc_ms,
            utc_offset_min,
            last_active_utc_ms: JournalPresence { turns, times, started_utc_ms: self.started_utc_ms }.last_active_utc_ms(),
            consented: consents.iter().any(|g| g.channel == Channel::Sleep),
            flash_side: self.side.is_some(),
            month_spent_pusd: self.ledger.as_ref().map_or(0, |l| l.status().spent_pusd),
        };
        let chat = |since: Option<u64>, until: u64| chat_lines(turns, times, since, until);
        let mut draft_on_side = |m: &Material, cap: u64| {
            let (Some(side), Some(ledger)) = (self.side.as_deref(), self.ledger.as_deref()) else { return Err(DraftError::Failed("flash_off".into())) };
            let mut call = |c: SideCall| handle.block_on(async { tokio::time::timeout(DEADLINE + CALL_SLACK, side.call(c)).await.unwrap_or(Err(SideError::Timeout)) });
            draft(m, cap, &|bytes| ledger.reserve_for(bytes), &mut call)
        };
        let propose = |p: &Proposal| self.store.propose_from_night(p);
        let log = self.store.sleep_log();
        let mut night = HostNight { log: &log, now_utc_ms, chat: &chat, draft: &mut draft_on_side, propose: &propose };
        tick(&seen, &mut night, &run_id(now_utc_ms))
    }
}

#[cfg(test)]
thread_local! { pub static IN_TESTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

pub async fn tick_async(host: Arc<Host>, dates: Arc<Mutex<Dates>>, turns: Vec<Turn>, now_utc_ms: u64, utc_offset_min: i32) -> Result<(Ticked, Option<String>), String> {
    let handle = tokio::runtime::Handle::current();
    let tick = move || {
        let times = dates.lock().unwrap().see(&turns, now_utc_ms);
        let ticked = host.tick(&turns, &times, now_utc_ms, utc_offset_min, &handle)?;
        let log = host.store.sleep_log();
        let ran = matches!(ticked, Ticked::Ran(_));
        let saved = dates.lock().unwrap().save(&log.with_file_name(DATES_FILE), last_start(&log), ran);

        match saved {
            Err(e) if !ran => Err(format!("chat dates not kept: {e}")),
            Err(e) => Ok((ticked, Some(format!("chat dates not kept after the night: {e}")))),
            Ok(()) => Ok((ticked, None)),
        }
    };
    tokio::task::spawn_blocking(tick).await.unwrap_or_else(|e| Err(format!("the tick stopped: {e}")))
}

pub fn attach(agent: &Arc<Agent>) {
    #[cfg(test)]
    if !IN_TESTS.with(std::cell::Cell::get) {
        return;
    }
    let Some(store) = agent.self_store.clone() else { return };
    store.set_sleep_host(if agent.flash_side.is_some() { HOST_FLASH_SIDE } else { HOST_NO_FLASH_SIDE });
    let host = Arc::new(Host { store, side: agent.flash_side.clone(), ledger: agent.flash_spend.clone(), started_utc_ms: crate::now_utc_ms() });
    let dates = Arc::new(Mutex::new(kept_dates(&host.store)));
    let weak = Arc::downgrade(agent);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(TICK);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_error = None;
        loop {
            every.tick().await;
            let Some(agent) = weak.upgrade() else { return };
            let now = crate::now_utc_ms();
            let ticked = tick_async(host.clone(), dates.clone(), agent.journal.turns(), now, rth_memory_brain::utc_offset_min(now)).await;
            let (level, msg) = match ticked {
                Ok((Ticked::Ran(s), None)) => ("info", format!("sleep: the night ran, {} proposals", s.proposed)),
                Ok((Ticked::Ran(s), Some(why))) => ("warn", format!("sleep: the night ran, {} proposals; {why}", s.proposed)),
                Ok(_) => {
                    last_error = None;
                    continue;
                }
                Err(why) if last_error.as_ref() == Some(&why) => continue,
                Err(why) => {
                    last_error = Some(why.clone());
                    ("warn", format!("sleep: {why}"))
                }
            };
            agent.emit(Event::Log { level: level.into(), msg }, None).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use rth_self::ProposalKind;
    use rth_self::sleep::NIGHT_CAP_PUSD;

    const DAWN: u64 = 1_793_001_600_000;
    const BRT: i32 = -180;
    const HOUR: u64 = 3_600_000;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rth-sleeper-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const IDLE_MS: u64 = 30 * 60 * 1000;

    fn seen(now: u64, away: bool) -> Seen {
        Seen { now_utc_ms: now, utc_offset_min: BRT, last_active_utc_ms: if away { now - IDLE_MS } else { now - 1 }, consented: true, flash_side: true, month_spent_pusd: 0 }
    }

    fn grant(channel: Channel) -> Grant {
        Grant { channel, recipient: None, copy_version: 1, granted_utc_ms: 1 }
    }

    const REPLY: &str = r#"{"proposals": [{"kind": "drive", "text": "help keep the fern watered", "why": "they keep forgetting it", "evidence": ["chat:e-1"]}]}"#;

    struct Fake {
        log: std::path::PathBuf,
        calls: RefCell<Vec<Material>>,
        proposed: RefCell<Vec<Proposal>>,
        reply: Result<String, DraftError>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { log: dir().join(sleep::SLEEP_LOG), calls: RefCell::new(Vec::new()), proposed: RefCell::new(Vec::new()), reply: Ok(REPLY.into()) }
        }
        fn tick(&self, s: &Seen) -> Ticked {
            let chat = |_since: Option<u64>, _until: u64| vec![Line { id: "chat:e-1".into(), text: "person: I keep forgetting to water the fern".into() }];
            let mut draft = |m: &Material, _cap: u64| {
                self.calls.borrow_mut().push(m.clone());
                self.reply.clone()
            };
            let propose = |p: &Proposal| {
                self.proposed.borrow_mut().push(p.clone());
                Ok(())
            };
            let mut night = HostNight { log: &self.log, now_utc_ms: s.now_utc_ms, chat: &chat, draft: &mut draft, propose: &propose };
            tick(s, &mut night, &run_id(s.now_utc_ms)).unwrap()
        }
    }

    #[test]
    fn s1_host_the_sleep_fact_says_why_not_in_order() {
        let on = [grant(Channel::Sleep)];
        assert_eq!(fact(HOST_NONE, &on), json!({"running": false, "why_not": "no_host"}));
        assert_eq!(fact(HOST_FLASH_SIDE, &on), json!({"running": true, "why_not": null}));
        assert_eq!(fact(HOST_NO_FLASH_SIDE, &on), json!({"running": false, "why_not": "flash_side_off"}));
        assert_eq!(fact(HOST_FLASH_SIDE, &[grant(Channel::Notification)]), json!({"running": false, "why_not": "not_enabled"}));
        assert_eq!(fact(HOST_NO_FLASH_SIDE, &[]), json!({"running": false, "why_not": "not_enabled"}), "no consent comes before no Flash side");
        assert_eq!(fact(HOST_NONE, &[]), json!({"running": false, "why_not": "no_host"}), "no host comes first");
    }

    #[test]
    fn s1_host_a_night_runs_once_while_away_in_the_window_and_proposes_drives_only() {
        let f = Fake::new();
        assert_eq!(f.tick(&seen(DAWN, false)), Ticked::Waited, "present: no night");
        assert!(f.calls.borrow().is_empty());
        let Ticked::Ran(s) = f.tick(&seen(DAWN + HOUR, true)) else { panic!("away in the window runs") };
        assert_eq!((s.proposed, f.calls.borrow().len()), (1, 1));
        assert!(f.calls.borrow()[0].memories.is_empty(), "chat only");
        let p = &f.proposed.borrow()[0];
        assert_eq!((p.kind, p.status, p.decided_utc_ms), (ProposalKind::Drive, rth_self::ProposalStatus::Pending, None));
        assert_eq!(f.tick(&seen(DAWN + 2 * HOUR, true)), Ticked::Waited, "once a night");
        assert_eq!(f.calls.borrow().len(), 1);
    }

    #[test]
    fn s1_host_away_outside_the_window_never_starts_a_night() {
        for at in [DAWN + 5 * HOUR, DAWN + 18 * HOUR] {
            let f = Fake::new();
            assert_eq!(f.tick(&seen(at, true)), Ticked::Skipped("missed_window"));
            assert_eq!(f.tick(&seen(at + HOUR, true)), Ticked::Waited);
            assert!(f.calls.borrow().is_empty());
        }
    }

    #[test]
    fn s1_host_without_consent_nothing_is_sent() {
        let f = Fake::new();
        let off = Seen { consented: false, ..seen(DAWN, true) };
        assert_eq!(f.tick(&off), Ticked::Skipped("off"));
        assert_eq!(f.tick(&Seen { now_utc_ms: DAWN + 24 * HOUR, ..off }), Ticked::Waited, "off is written once");
        assert!(f.calls.borrow().is_empty());

        let g = Fake::new();
        assert!(matches!(g.tick(&seen(DAWN, true)), Ticked::Ran(_)));
        assert_eq!(g.tick(&Seen { consented: false, ..seen(DAWN + 24 * HOUR, true) }), Ticked::Skipped("off"));
        assert_eq!(g.calls.borrow().len(), 1);
    }

    #[test]
    fn s1_host_no_flash_side_skips_and_says_why() {
        let f = Fake::new();
        assert_eq!(f.tick(&Seen { flash_side: false, ..seen(DAWN, true) }), Ticked::Skipped("flash_off"));
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn s1_host_a_torn_tail_is_cut_back_and_the_night_goes_on() {
        let f = Fake::new();
        files::append(&f.log, &sleep::skip_line("2026-10-25", "off", 1)).unwrap();
        let mut bytes = std::fs::read(&f.log).unwrap();
        bytes.extend_from_slice(b"{\"kind\": \"sl");
        std::fs::write(&f.log, &bytes).unwrap();
        assert!(matches!(f.tick(&seen(DAWN, true)), Ticked::Ran(_)));
        let l = files::read_lines(&f.log).unwrap();
        assert!(!l.torn && !l.corrupt);
        assert_eq!(l.records[0]["local_date"], "2026-10-25", "the whole lines stay");

        let g = Fake::new();
        std::fs::write(&g.log, "not json\n{\"kind\": \"sleep\"}\n").unwrap();
        assert!(!repair_torn_tail(&g.log).unwrap());
        assert_eq!(g.tick(&seen(DAWN, true)), Ticked::Waited);
        assert!(g.calls.borrow().is_empty());
    }

    #[test]
    fn s1_host_the_call_is_refused_over_the_night_cap_and_sends_no_memory() {
        let m = Material { chat: vec![Line { id: "chat:e-1".into(), text: "person: hi".into() }], memories: Vec::new() };
        let sent: RefCell<Vec<SideCall>> = RefCell::new(Vec::new());
        let mut call = |c: SideCall| {
            sent.borrow_mut().push(c);
            Err(SideError::Timeout)
        };
        assert_eq!(draft(&m, NIGHT_CAP_PUSD, &|_| NIGHT_CAP_PUSD + 1, &mut call), Err(DraftError::OverCap));
        let counted = std::cell::Cell::new(0);
        let _ = draft(&m, NIGHT_CAP_PUSD, &|b| { counted.set(b); NIGHT_CAP_PUSD + 1 }, &mut call);
        let (system, user, _) = request(&m);
        assert_eq!(counted.get(), system.len() + user.len() + 2 * 16, "the bytes admission counts: two messages");
        assert!(sent.borrow().is_empty(), "refused before sending");
        assert_eq!(draft(&m, NIGHT_CAP_PUSD, &|_| NIGHT_CAP_PUSD, &mut call), Err(DraftError::Failed("timeout".into())));
        let c = &sent.borrow()[0];
        assert_eq!((c.purpose, c.carries_memory), (Purpose::Sleep, false));
        assert!(c.user.contains(r#""id":"l1""#) && !c.user.contains("chat:e-1"), "{}", c.user);
        let mut refused = |_c: SideCall| Err(SideError::Refused("background line".into()));
        assert_eq!(draft(&m, NIGHT_CAP_PUSD, &|_| 0, &mut refused), Err(DraftError::OverCap), "admission's refusal is over the cap");
    }

    #[test]
    fn s1_host_the_chat_is_the_dated_turns_since_the_last_night() {
        let turn = |event: &str, role: &str, text: &str| serde_json::from_value::<Turn>(json!({"role": role, "text": text, "kind": "typed", "event": event})).unwrap();
        let turns = vec![turn("h:1", "user", "old"), turn("h:2", "user", "hi"), turn("h:3", "model", "hello"), turn("h:4", "user", "undated"), turn("h:5", "user", "later")];
        let times: HashMap<String, u64> = [("h:1", 10), ("h:2", 20), ("h:3", 21), ("h:5", 99)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let got = chat_lines(&turns, &times, Some(10), 50);
        assert_eq!(got.iter().map(|l| (l.id.as_str(), l.text.as_str())).collect::<Vec<_>>(), vec![("chat:h:2", "person: hi"), ("chat:h:3", "assistant: hello")]);
        assert_eq!(chat_lines(&turns, &times, None, 50).len(), 3, "the first night reads all it can date");
    }

    #[test]
    fn s1_host_facts_are_chat_only_and_idle_after_half_an_hour() {
        let f = facts(&seen(DAWN, true));
        assert_eq!((f.memory_egress, f.idle, f.utc_offset_min), (false, true, BRT));
        assert!(!facts(&Seen { last_active_utc_ms: DAWN - IDLE_MS + 1, ..seen(DAWN, true) }).idle, "a turn 29:59 ago: here");
        assert!(!facts(&Seen { last_active_utc_ms: DAWN + 5, ..seen(DAWN, true) }).idle, "a turn after the tick's clock: here");
        assert!(rth_self::ProposalId::new(format!("p-sleep-{}-3", run_id(u64::MAX))).is_ok(), "the longest run id still names a proposal");
    }

    #[test]
    fn s1_host_the_reply_cites_aliases_and_gets_the_lines_ids_back() {
        let m = Material { chat: vec![Line { id: "chat:a-1".into(), text: "person: hi".into() }, Line { id: "chat:a-2".into(), text: "assistant: hello".into() }], memories: Vec::new() };
        let reply = r#"{"proposals": [{"kind": "drive", "text": "t", "why": "w", "evidence": ["l2", "l1", "l3", "chat:x"]}]}"#;
        let back: Value = serde_json::from_str(&unalias(reply, &m)).unwrap();
        assert_eq!(back["proposals"][0]["evidence"], json!(["chat:a-2", "chat:a-1", "l3", "chat:x"]));
        assert_eq!(unalias("not json", &m), "not json");
    }

    #[test]
    fn s1_host_the_journal_presence_is_the_latest_dated_person_turn_or_the_start() {
        let turn = |event: &str, role: &str| serde_json::from_value::<Turn>(json!({"role": role, "text": "x", "kind": "typed", "event": event})).unwrap();
        let turns = vec![turn("h:1", "user"), turn("h:2", "model"), turn("h:3", "user")];
        let times: HashMap<String, u64> = [("h:1", 50), ("h:2", 90)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let at = |started| JournalPresence { turns: &turns, times: &times, started_utc_ms: started }.last_active_utc_ms();
        assert_eq!(at(10), 50, "the model's later reply and the undated turn don't count");
        assert_eq!(at(70), 70, "a start after the last dated turn counts as activity");
    }

    #[test]
    fn s1_host_turns_are_dated_on_the_wall_clock_as_first_seen() {
        let turn = |event: &str| serde_json::from_value::<Turn>(json!({"role": "user", "text": "x", "kind": "typed", "event": event})).unwrap();
        let mut d = Dates::default();
        assert!(d.see(&[turn("h:1")], 100).is_empty(), "an earlier run's turn stays undated");
        let got = d.see(&[turn("h:1"), turn("h:2")], 200);
        assert_eq!(got, HashMap::from([("h:2".to_string(), 200)]));
        assert_eq!(d.see(&[turn("h:1"), turn("h:2"), turn("h:3")], 500), HashMap::from([("h:2".to_string(), 200), ("h:3".to_string(), 500)]), "a date never moves");
    }

    #[test]
    fn s1_host_the_dates_since_the_last_night_are_kept_for_the_next_run() {
        let turn = |event: &str| serde_json::from_value::<Turn>(json!({"role": "user", "text": "x", "kind": "typed", "event": event})).unwrap();
        let path = dir().join(DATES_FILE);
        let mut d = Dates::default();
        d.see(&[], 100);
        d.see(&[turn("h:1"), turn("h:2")], 200);
        d.see(&[turn("h:1"), turn("h:2"), turn("h:3")], 300);
        d.save(&path, Some(200), false).unwrap();
        let mut back = Dates::load(&path);
        assert_eq!(back.see(&[turn("h:1"), turn("h:2"), turn("h:3")], 400), HashMap::from([("h:3".to_string(), 300)]), "only the date after the last night");
        back.save(&path, Some(300), true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}", "a night that ran drops what it read, with no new turn");
        std::fs::write(&path, "{not json").unwrap();
        assert!(Dates::load(&path).see(&[turn("h:3")], 500).is_empty(), "a broken file keeps none");
    }

    #[test]
    fn s1_host_the_dates_follow_the_journal_window() {
        let turn = |n: usize| serde_json::from_value::<Turn>(json!({"role": "user", "text": "x", "kind": "typed", "event": format!("h:{n}")})).unwrap();
        let path = dir().join(DATES_FILE);
        let mut d = Dates::default();
        d.see(&[], 1);
        for n in 0..200u64 {
            let window: Vec<Turn> = (n.saturating_sub(127)..=n).map(|i| turn(i as usize)).collect();
            d.see(&window, 10 + n);
            d.save(&path, None, false).unwrap();
        }
        let kept: HashMap<String, u64> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(kept.len(), 128);
        assert!(!kept.contains_key("h:0") && kept["h:199"] == 209);

        d.see(&(73..=199).map(turn).collect::<Vec<_>>(), 300);
        d.save(&path, None, false).unwrap();
        let kept: HashMap<String, u64> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(kept.len() == 127 && !kept.contains_key("h:72"), "{}", kept.len());
    }

    async fn host_tick(host: &Arc<Host>, dates: &Arc<Mutex<Dates>>, turns: &[Turn], now: u64) -> Result<Ticked, String> {
        host_tick_unkept(host, dates, turns, now).await.map(|(t, unkept)| {
            assert_eq!(unkept, None, "the dates were kept");
            t
        })
    }

    async fn host_tick_unkept(host: &Arc<Host>, dates: &Arc<Mutex<Dates>>, turns: &[Turn], now: u64) -> Result<(Ticked, Option<String>), String> {
        tick_async(host.clone(), dates.clone(), turns.to_vec(), now, BRT).await
    }

    #[tokio::test]
    async fn s1_host_a_night_on_the_flash_side_proposes_into_the_store() {
        use crate::flash::test_server::{Answer, ca, start};
        tokio::time::timeout(Duration::from_secs(30), async {
            let cited = REPLY.replace("chat:e-1", "l1");
            let body = json!({"model": "m", "choices": [{"message": {"role": "assistant", "content": cited}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 120, "completion_tokens": 20}});
            let server = start(Answer::Json(body.to_string())).await;
            let d = dir();
            let store = Arc::new(SelfStore::open(&d, "inst-test", crate::local::DEFAULT_N_CTX).unwrap());
            let mut l = Ledger::open(&d.join("spend"), crate::flash::SIDE_MAX_TOKENS);
            l.month = Some("2026-10".into());
            l.day = Some("2026-10-26".into());
            let ledger = Arc::new(l);
            let cfg = crate::flash::config("sleep-test-key-0123456789", &server.url, Some(ca())).unwrap();
            let side = FlashSide::for_session("flash+local@granite", Some(cfg), Some(ledger.clone()), "b1", "").map(Arc::new);
            assert!(side.is_some());
            let host = Arc::new(Host { store: store.clone(), side, ledger: Some(ledger.clone()), started_utc_ms: DAWN - 2 * HOUR });
            let dates = Arc::new(Mutex::new(Dates::default()));
            let turn = |event: &str, role: &str, text: &str| serde_json::from_value::<Turn>(json!({"role": role, "text": text, "kind": "typed", "event": event})).unwrap();
            let sent = || server.requests.lock().unwrap().iter().map(|raw| serde_json::from_str::<Value>(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()).collect::<Vec<_>>();
            assert_eq!(host_tick(&host, &dates, &[], DAWN).await, Ok(Ticked::Skipped("off")));
            assert!(store.consent_set("c0", "notification", None, true, 1, DAWN)["ok"].is_object());
            let mut turns = vec![turn("e-1", "user", "I keep forgetting to water the fern"), turn("e-2", "model", "I can remind you")];
            assert_eq!(host_tick(&host, &dates, &turns, DAWN + 20 * HOUR).await, Ok(Ticked::Waited), "01:00, before the window");
            assert_eq!(host_tick(&host, &dates, &turns, DAWN + 24 * HOUR).await, Ok(Ticked::Waited), "still off: another channel's consent is not the night's");
            assert!(sent().is_empty(), "off sends nothing");
            assert!(store.consent_set("c1", "sleep", None, true, 2, DAWN + 24 * HOUR)["ok"].is_object());

            let dates = Arc::new(Mutex::new(kept_dates(&store)));
            let host = Arc::new(Host { store: host.store.clone(), side: host.side.clone(), ledger: host.ledger.clone(), started_utc_ms: DAWN + 25 * HOUR });
            let Ok(Ticked::Ran(s)) = host_tick(&host, &dates, &turns, DAWN + 48 * HOUR).await else { panic!("the next night runs") };
            assert_eq!(s.proposed, 1);
            let b = &sent()[0];
            let user = b["messages"][1]["content"].as_str().unwrap();
            assert!(user.contains(r#"{"id":"l1","text":"person: I keep forgetting to water the fern"}"#) && user.contains("assistant: I can remind you") && !user.contains("chat:"), "lines go out under short ids: {user}");
            assert_eq!((b["response_format"]["json_schema"]["name"].as_str(), b["max_tokens"].as_u64()), (Some("sleep"), Some(crate::flash::SIDE_MAX_TOKENS as u64)));
            assert!(ledger.status().by_purpose.get("sleep").is_some_and(|&p| p > 0), "{:?}", ledger.status());
            let proposals = files::read_lines(&d.join("proposals.jsonl")).unwrap().records;
            assert_eq!((proposals.len(), proposals[0]["kind"].as_str(), proposals[0]["status"].as_str()), (1, Some("drive"), Some("pending")));
            assert!(proposals[0]["evidence_ids"].as_array().unwrap().iter().any(|e| e == "chat:e-1"), "the cited alias is the line's own id again: {:?}", proposals[0]);
            let night = DAWN + 72 * HOUR;
            turns.push(turn("e-3", "user", "still up"));
            assert_eq!(host_tick(&host, &dates, &turns, night - 10 * 60 * 1000).await, Ok(Ticked::Waited), "a person turn just now: here");
            let kept: HashMap<String, u64> = serde_json::from_slice(&std::fs::read(d.join(DATES_FILE)).unwrap()).unwrap();
            assert_eq!(kept, HashMap::from([("e-3".to_string(), night - 10 * 60 * 1000)]), "only the dates since the last night are kept");
            assert_eq!(host_tick(&host, &dates, &turns, night).await, Ok(Ticked::Waited), "a person turn ten minutes ago: here");
            assert_eq!(sent().len(), 1);
            assert!(matches!(host_tick(&host, &dates, &turns, night + HOUR).await, Ok(Ticked::Ran(_))), "an hour later: away");
            let user = sent()[1]["messages"][1]["content"].as_str().unwrap().to_string();
            assert!(user.contains("still up") && !user.contains("fern"), "only the chat since the last night: {user}");
            let runs: Vec<String> = files::read_lines(&store.sleep_log()).unwrap().records.iter().filter(|r| r["phase"] == "start").map(|r| r["run_id"].as_str().unwrap().to_string()).collect();
            assert!(runs.len() == 2 && runs[0] != runs[1], "each night its own run id: {runs:?}");
            assert_eq!(std::fs::read_to_string(d.join(DATES_FILE)).unwrap(), "{}", "after the second night, no kept date is older than its start");

            let night2 = night + 24 * HOUR;
            turns.push(turn("e-4", "user", "late again"));
            std::fs::remove_file(d.join(DATES_FILE)).unwrap();
            std::fs::create_dir(d.join(DATES_FILE)).unwrap();
            let why = host_tick(&host, &dates, &turns, night2 - 10 * 60 * 1000).await.unwrap_err();
            assert!(why.starts_with("chat dates not kept: "), "{why}");
            let (ticked, unkept) = host_tick_unkept(&host, &dates, &turns, night2 + HOUR).await.unwrap();
            assert!(matches!(ticked, Ticked::Ran(_)), "the night runs");
            assert!(unkept.as_deref().is_some_and(|w| w.starts_with("chat dates not kept after the night: ")), "{unkept:?}");
        })
        .await
        .expect("the test ended within 30 s")
    }
}
