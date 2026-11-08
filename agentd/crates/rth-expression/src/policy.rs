

use rth_self::ExpressionPrefs;
use rth_self::consent::{Channel, Grant};
use serde::{Deserialize, Serialize};

use crate::reason::Reason;

pub const AWAY_IDLE_S: u64 = 300;

pub const QUIET_GAP_MS: u64 = 10 * 60 * 1000;

pub const DAY_START_MIN: i64 = 4 * 60;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    NameWish,
    Overnight,
    Curiosity,
    OpenThreads,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::NameWish, Kind::Overnight, Kind::Curiosity, Kind::OpenThreads];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::NameWish => "name_wish",
            Kind::Overnight => "overnight",
            Kind::Curiosity => "curiosity",
            Kind::OpenThreads => "open_threads",
        }
    }

    fn on(self, prefs: &ExpressionPrefs) -> bool {
        let k = &prefs.kinds;
        match self {
            Kind::NameWish => k.name_wish,
            Kind::Overnight => k.overnight,
            Kind::Curiosity => k.curiosity,
            Kind::OpenThreads => k.open_threads,
        }
    }

    pub fn window_ms(self) -> Option<u64> {
        match self {
            Kind::NameWish | Kind::Overnight => None,
            Kind::OpenThreads => Some(3 * DAY_MS),
            Kind::Curiosity => Some(7 * DAY_MS),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Egress {
    None,
    Memory,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: Kind,
    pub subject_id: String,
    pub egress: Egress,

    pub held: bool,

    pub reason_days: u32,

    pub data: Vec<Datum>,

    pub memory_refs: Vec<String>,
}

impl Candidate {
    pub fn reason(&self) -> Reason {
        Reason::of(self.kind, self.reason_days)
    }
}

impl std::fmt::Debug for Candidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Candidate")
            .field("kind", &self.kind)
            .field("subject_id", &self.subject_id)
            .field("egress", &self.egress)
            .field("held", &self.held)
            .field("data", &self.data)
            .field("memory_refs", &self.memory_refs.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Label {

    Name,

    Why,
    Thread,
    Motivation,
    Aspiration,
    Routine,
    Memory,

    Reflection,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Datum {
    pub label: Label,
    pub text: String,
}

impl std::fmt::Debug for Datum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}({} bytes)", self.label, self.text.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub locked: bool,
    pub display_asleep: bool,
    pub idle_s: u64,
    pub output_busy: bool,
    pub last_origin: String,
    pub frontmost: bool,
    pub notify_ok: bool,
}

impl Presence {
    pub fn away(&self) -> bool {
        self.locked || self.display_asleep || self.idle_s > AWAY_IDLE_S || self.last_origin == "omni"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub kind: Kind,
    pub subject_id: String,
    pub utc_ms: u64,

    pub local_day: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnState {
    pub streaming: bool,
    pub last_person_utc_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub now_utc_ms: u64,

    pub utc_offset_min: i32,
    pub prefs: &'a ExpressionPrefs,

    pub presence: Option<&'a Presence>,
    pub history: &'a [Attempt],
    pub turn: TurnState,

    pub consents: &'a [Grant],

    pub omni_recipient: Option<&'a str>,

    pub omni_egress_granted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Chat,

    Notification,
    Omni,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    NoPresence,
    TurnRunning,
    QuietGap,
    QuietHours,
    Away,
    NoDestination,
    NoRecipient,
    RecipientMismatch,
    NoEgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    KindOff,
    Duplicate,
    Cap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Send(Destination),
    Hold(HoldReason),
    Drop(DropReason),
}

pub fn local_day(utc_ms: u64, utc_offset_min: i32) -> i64 {
    ((utc_ms / 60_000) as i64 + i64::from(utc_offset_min) - DAY_START_MIN).div_euclid(1440)
}

fn minutes(hhmm: &str) -> i64 {
    let (h, m) = hhmm.split_once(':').unwrap_or(("0", "0"));
    h.parse::<i64>().unwrap_or(0) * 60 + m.parse::<i64>().unwrap_or(0)
}

fn quiet(f: &Facts) -> bool {
    let now = ((f.now_utc_ms / 60_000) as i64 + i64::from(f.utc_offset_min)).rem_euclid(1440);
    let (start, end) = (minutes(f.prefs.quiet_hours.start.as_str()), minutes(f.prefs.quiet_hours.end.as_str()));
    if start < end { start <= now && now < end } else { now >= start || now < end }
}

pub fn decide(f: &Facts, c: &Candidate) -> Verdict {
    if !c.kind.on(f.prefs) {
        return Verdict::Drop(DropReason::KindOff);
    }
    let Some(p) = f.presence else {
        return Verdict::Hold(HoldReason::NoPresence);
    };
    if !c.held {
        let offered = |a: &&Attempt| a.kind == c.kind && a.subject_id == c.subject_id;
        let recent = |a: &&Attempt| c.kind.window_ms().is_none_or(|w| f.now_utc_ms.saturating_sub(a.utc_ms) < w);
        if f.history.iter().filter(offered).any(|a| recent(&a)) {
            return Verdict::Drop(DropReason::Duplicate);
        }
        let today = local_day(f.now_utc_ms, f.utc_offset_min);
        if f.history.iter().filter(|a| a.local_day == today).count() >= usize::from(f.prefs.daily_cap.get()) {
            return Verdict::Drop(DropReason::Cap);
        }
    }
    if f.turn.streaming {
        return Verdict::Hold(HoldReason::TurnRunning);
    }
    if f.turn.last_person_utc_ms.is_some_and(|t| f.now_utc_ms < t.saturating_add(QUIET_GAP_MS)) {
        return Verdict::Hold(HoldReason::QuietGap);
    }
    if quiet(f) {
        return Verdict::Hold(HoldReason::QuietHours);
    }
    let d = &f.prefs.destinations;
    let consented = |ch: Channel| f.consents.iter().find(|g| g.channel == ch);
    if p.away() {
        let Some(grant) = consented(Channel::Omni).filter(|_| d.omni) else {
            return Verdict::Hold(HoldReason::Away);
        };
        return match f.omni_recipient {
            None => Verdict::Hold(HoldReason::NoRecipient),
            Some(r) if grant.recipient.as_deref() != Some(r) => Verdict::Hold(HoldReason::RecipientMismatch),
            Some(_) if c.egress == Egress::Memory && !f.omni_egress_granted => Verdict::Hold(HoldReason::NoEgress),
            Some(_) => Verdict::Send(Destination::Omni),
        };
    }
    let notification = d.notification && p.notify_ok && consented(Channel::Notification).is_some();
    if p.frontmost && d.chat {
        Verdict::Send(Destination::Chat)
    } else if !p.frontmost && notification {
        Verdict::Send(Destination::Notification)
    } else if d.chat {
        Verdict::Send(Destination::Chat)
    } else {
        Verdict::Hold(HoldReason::NoDestination)
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
