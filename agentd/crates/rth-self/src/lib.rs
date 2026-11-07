

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub mod compile;
pub mod consent;
pub mod copy;
pub mod fork;
pub mod sleep;
pub mod store;

pub const SCHEMA_VERSION: u32 = 1;

pub const RELEASE_DEFAULT_SPEC_VERSION: &str = "048426a169f86ef9f89e3a8af3cf43ecbad5b00d452ae0b71b1a31bdbc897207";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub field: &'static str,
    pub code: &'static str,
    pub reason: String,
    pub need: Option<u64>,
    pub max: Option<u64>,
}

pub const CODES: [&str; 15] = [
    "malformed", "unknown_field", "schema", "name_chars", "text_chars", "lang_tag", "clock", "out_of_range", "drives_max", "quiet_equal", "archetype_unknown", "budget", "consent_missing", "flash_off", "stale",
];

fn no(code: &'static str, reason: impl std::fmt::Display) -> String {
    format!("{code}\u{1f}{reason}")
}

fn refused(field: &'static str, error: &str) -> Refused {
    let (code, reason) = match error.split_once('\u{1f}') {
        Some((c, r)) => (CODES.iter().copied().find(|k| *k == c).unwrap_or("malformed"), r.to_string()),
        None if error.starts_with("unknown field") => ("unknown_field", error.to_string()),
        None if error.starts_with("invalid value") || error.starts_with("invalid type: integer") => ("out_of_range", error.to_string()),
        None => ("malformed", error.to_string()),
    };
    Refused { field, code, reason, need: None, max: None }
}

fn check_name(s: &str) -> Result<(), String> {
    let reason = || "a name is 1 to 24 letters, with single spaces or hyphens between them".to_string();
    let n = s.chars().count();
    let parts_ok = s.split([' ', '-']).all(|w| !w.is_empty() && w.chars().all(char::is_alphabetic));
    if (1..=24).contains(&n) && parts_ok { Ok(()) } else { Err(no("name_chars", reason())) }
}

fn check_bounded(s: &str) -> Result<(), String> {
    let n = s.chars().count();
    let banned = |c: char| matches!(c, '"' | '\'' | ':' | '`' | '<' | '>') || c.is_control();
    if (1..=80).contains(&n) && s.trim() == s && !s.chars().any(banned) {
        Ok(())
    } else {
        Err(no("text_chars", "at most 80 characters on one line, without quotes, colons, backticks or angle brackets"))
    }
}

fn check_lang(s: &str) -> Result<(), String> {
    let (lang, region) = s.split_once('-').map_or((s, None), |(l, r)| (l, Some(r)));
    let lang_ok = (2..=3).contains(&lang.len()) && lang.bytes().all(|b| b.is_ascii_lowercase());
    let region_ok = region.is_none_or(|r| r.len() == 2 && r.bytes().all(|b| b.is_ascii_uppercase()));
    if lang_ok && region_ok { Ok(()) } else { Err(no("lang_tag", "a language is a tag like pt or pt-BR")) }
}

fn check_clock(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let digits = b.len() == 5 && b[2] == b':' && [0, 1, 3, 4].iter().all(|&i| b[i].is_ascii_digit());
    let (h, m) = if digits { ((b[0] - b'0') * 10 + b[1] - b'0', (b[3] - b'0') * 10 + b[4] - b'0') } else { (99, 99) };
    if h < 24 && m < 60 { Ok(()) } else { Err(no("clock", "a time is HH:MM on a 24-hour clock")) }
}

fn check_slug(s: &str) -> Result<(), String> {
    let ok = (1..=40).contains(&s.len()) && s.split(['-', '_']).all(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
    if ok { Ok(()) } else { Err(no("malformed", "an id is lowercase letters and digits, joined by - or _")) }
}

fn check_max(n: u64, max: u64, what: &str) -> Result<(), String> {
    if n <= max { Ok(()) } else { Err(no("out_of_range", format!("{what} is at most {max}"))) }
}

macro_rules! text_type {
    ($(#[$doc:meta])* $t:ident, $check:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $t(String);

        impl $t {
            pub fn new(s: impl Into<String>) -> Result<$t, String> {
                let s = s.into();
                $check(&s)?;
                Ok($t(s))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $t {
            type Error = String;
            fn try_from(s: String) -> Result<$t, String> {
                $t::new(s)
            }
        }

        impl From<$t> for String {
            fn from(t: $t) -> String {
                t.0
            }
        }
    };
}

text_type!(

    Name, check_name
);
text_type!(

    BoundedText, check_bounded
);
text_type!(

    LangTag, check_lang
);
text_type!(

    ClockTime, check_clock
);
text_type!(

    ArchetypeId, check_slug
);
text_type!(ProposalId, check_slug);
text_type!(HorizonId, check_slug);
text_type!(

    TaskId, check_slug
);

pub mod role;

macro_rules! bounded_int {
    ($(#[$doc:meta])* $t:ident, $max:expr, $what:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "u64", into = "u8")]
        pub struct $t(u8);

        impl $t {
            pub fn new(n: u64) -> Result<$t, String> {
                check_max(n, $max, $what)?;
                Ok($t(n as u8))
            }

            pub fn get(self) -> u8 {
                self.0
            }
        }

        impl TryFrom<u64> for $t {
            type Error = String;
            fn try_from(n: u64) -> Result<$t, String> {
                $t::new(n)
            }
        }

        impl From<$t> for u8 {
            fn from(t: $t) -> u8 {
                t.0
            }
        }
    };
}

bounded_int!(

    Axis, 100, "an axis"
);
bounded_int!(

    DailyCap, 3, "the daily cap"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameOrigin {
    Given,
    Chosen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameState {
    pub current: Name,
    pub origin: NameOrigin,

    pub proposal: Option<ProposalId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axes {
    pub energy: Axis,
    pub mind: Axis,
    pub nature: Axis,
    pub tactics: Axis,
    pub identity: Axis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Personality {
    pub archetype: ArchetypeId,
    pub axes: Axes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Length {
    Brief,
    Balanced,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mirroring {
    Subtle,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Humour {
    None,
    Light,
    Playful,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionRate {
    Rare,
    Sometimes,
    Often,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub length: Length,
    pub register_mirroring: Mirroring,
    pub humour: Humour,
    pub question_rate: QuestionRate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Languages {

    pub reply_hint: Option<LangTag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriveSource {
    Person,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drive {
    pub text: BoundedText,
    pub source: DriveSource,
}

pub const MAX_DRIVES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drives {
    pub motivations: Vec<Drive>,
    pub aspirations: Vec<Drive>,
    pub routine: Vec<Drive>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kinds {
    pub name_wish: bool,
    pub overnight: bool,
    pub curiosity: bool,
    pub open_threads: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destinations {
    pub chat: bool,
    pub notification: bool,
    pub omni: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    pub start: ClockTime,
    pub end: ClockTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionPrefs {
    pub kinds: Kinds,
    pub destinations: Destinations,
    pub quiet_hours: QuietHours,
    pub daily_cap: DailyCap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelfSpec {
    pub schema_version: u32,
    pub instance_id: String,
    pub name: NameState,
    pub personality: Personality,
    pub style: Style,
    pub languages: Languages,
    pub drives: Drives,
    pub expression: ExpressionPrefs,
}

const SECTIONS: [&str; 8] = ["schema_version", "instance_id", "name", "personality", "style", "languages", "drives", "expression"];

fn section<T: DeserializeOwned>(v: &mut serde_json::Map<String, Value>, field: &'static str) -> Result<T, Refused> {
    let value = v.remove(field).unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|e| refused(field, &e.to_string()))
}

impl SelfSpec {
    pub fn release_default(instance_id: &str) -> SelfSpec {
        fn ok<T>(r: Result<T, String>) -> T {
            r.expect("release default is valid")
        }
        let mid = ok(Axis::new(50));
        SelfSpec {
            schema_version: SCHEMA_VERSION,
            instance_id: instance_id.to_string(),
            name: NameState { current: ok(Name::new("Unfold")), origin: NameOrigin::Given, proposal: None },
            personality: Personality { archetype: ok(ArchetypeId::new("steady-guide")), axes: Axes { energy: mid, mind: mid, nature: mid, tactics: mid, identity: mid } },
            style: Style { length: Length::Balanced, register_mirroring: Mirroring::Subtle, humour: Humour::Light, question_rate: QuestionRate::Sometimes },
            languages: Languages { reply_hint: None },
            drives: Drives::default(),
            expression: ExpressionPrefs {
                kinds: Kinds { name_wish: true, overnight: true, curiosity: true, open_threads: true },
                destinations: Destinations { chat: true, notification: false, omni: false },
                quiet_hours: QuietHours { start: ok(ClockTime::new("22:00")), end: ok(ClockTime::new("08:00")) },
                daily_cap: ok(DailyCap::new(3)),
            },
        }
    }

    pub fn parse(text: &str) -> Result<SelfSpec, Refused> {
        let spec = |code, reason: &str| Refused { field: "spec", code, reason: reason.to_string(), need: None, max: None };
        let Ok(Value::Object(mut v)) = serde_json::from_str::<Value>(text) else {
            return Err(spec("malformed", "the Self settings are not a JSON object"));
        };
        if let Some(k) = v.keys().find(|k| !SECTIONS.contains(&k.as_str())) {
            return Err(spec("unknown_field", &format!("unknown setting {k}")));
        }
        let s = SelfSpec {
            schema_version: section(&mut v, "schema_version")?,
            instance_id: section(&mut v, "instance_id")?,
            name: section(&mut v, "name")?,
            personality: section(&mut v, "personality")?,
            style: section(&mut v, "style")?,
            languages: section(&mut v, "languages")?,
            drives: section(&mut v, "drives")?,
            expression: section(&mut v, "expression")?,
        };
        s.check()?;
        Ok(s)
    }

    fn check(&self) -> Result<(), Refused> {
        let at = |field, r: Result<(), String>| r.map_err(|e| refused(field, &e));
        at("schema_version", check_schema(self.schema_version))?;
        let id = self.personality.archetype.as_str();
        at("personality", archetype(id).map(|_| ()).ok_or_else(|| no("archetype_unknown", format!("{id} is not in archetypes.json"))))?;
        let d = &self.drives;
        at("drives", check_drives(d.motivations.len(), d.aspirations.len(), d.routine.len()))?;
        at("expression", check_quiet(&self.expression.quiet_hours))?;
        Ok(())
    }

    pub fn spec_version(&self) -> String {
        #[derive(Serialize)]
        struct Prefix<'a> {
            name: &'a Name,
            personality: &'a Personality,
            style: &'a Style,
            reply_hint: &'a Option<LangTag>,
            drives: &'a Drives,
            destinations: &'a Destinations,
        }
        let p = Prefix {
            name: &self.name.current,
            personality: &self.personality,
            style: &self.style,
            reply_hint: &self.languages.reply_hint,
            drives: &self.drives,
            destinations: &self.expression.destinations,
        };
        let bytes = serde_json::to_vec(&p).expect("prefix fields serialize");
        Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn check_schema(v: u32) -> Result<(), String> {
    if v == SCHEMA_VERSION { Ok(()) } else { Err(no("schema", format!("this build reads Self settings version {SCHEMA_VERSION}, not {v}"))) }
}

fn check_drives(m: usize, a: usize, r: usize) -> Result<(), String> {
    if [m, a, r].iter().all(|&n| n <= MAX_DRIVES) { Ok(()) } else { Err(no("drives_max", format!("at most {MAX_DRIVES} drives of each kind"))) }
}

fn check_quiet(q: &QuietHours) -> Result<(), String> {
    if q.start != q.end { Ok(()) } else { Err(no("quiet_equal", "quiet hours can't start when they end")) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalKind {
    Name,
    Drive,
    Aspiration,
    MemoryEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalStatus {
    Pending,
    Approved,
    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub id: ProposalId,
    pub kind: ProposalKind,
    pub text: BoundedText,
    pub why: BoundedText,
    pub evidence_ids: Vec<String>,
    pub created_utc_ms: u64,
    pub status: ProposalStatus,
    pub decided_utc_ms: Option<u64>,
}

impl Proposal {
    pub fn parse(line: &str) -> Result<Proposal, Refused> {
        let p: Proposal = serde_json::from_str(line).map_err(|e| refused("proposal", &e.to_string()))?;
        if p.kind == ProposalKind::Name {
            check_name(p.text.as_str()).map_err(|e| refused("proposal", &e))?;
        }
        Ok(p)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HorizonLayer {

    Thread,

    Goal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HorizonStatus {

    Suggested,
    Open,
    Done,

    Declined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Horizon {
    pub id: HorizonId,
    pub layer: HorizonLayer,
    pub status: HorizonStatus,
    pub text: BoundedText,
    pub created_utc_ms: u64,
    pub last_touched_utc_ms: u64,
    pub task_id: Option<TaskId>,
}

impl Horizon {
    pub fn parse(line: &str) -> Result<Horizon, Refused> {
        serde_json::from_str(line).map_err(|e| refused("horizon", &e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardText {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archetype {
    pub id: ArchetypeId,
    pub shipped: bool,

    #[serde(default)]
    pub held_reason: Option<String>,
    pub axes: Axes,
    pub en: CardText,
    pub pt: CardText,
    pub ui_accent: String,
}

impl Archetype {

    pub fn personality(&self) -> Personality {
        Personality { archetype: self.id.clone(), axes: self.axes }
    }
}

pub const ARCHETYPE_COUNT: usize = 16;

pub const MIN_SHIPPED: usize = 12;

const NOT_OUR_NAMES: &[&str] = &[
    "architect", "logician", "commander", "debater", "advocate", "mediator", "protagonist", "campaigner", "logistician", "defender", "executive", "consul", "virtuoso", "adventurer", "entrepreneur", "entertainer",
    "arquiteto", "arquiteta", "lógico", "lógica", "logico", "logica", "comandante", "inovador", "inovadora", "advogado", "advogada", "mediador", "mediadora", "protagonista", "ativista", "logístico", "logística", "logistico", "logistica",
    "defensor", "defensora", "executivo", "executiva", "cônsul", "consul", "virtuosa", "aventureiro", "aventureira", "empreendedor", "empreendedora", "animador", "animadora", "mbti", "16personalities", "personalities",
];

fn borrowed_name(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric()).find_map(|w| {
        let singular = w.strip_suffix('s').filter(|s| s.len() == 4).unwrap_or(w);
        let b = singular.as_bytes();
        let mbti = b.len() == 4 && b"ie".contains(&b[0]) && b"sn".contains(&b[1]) && b"tf".contains(&b[2]) && b"jp".contains(&b[3]);
        (mbti || NOT_OUR_NAMES.contains(&w) || NOT_OUR_NAMES.contains(&w.trim_end_matches('s'))).then(|| w.to_string())
    })
}

pub fn parse_cards(text: &str) -> Result<Vec<Archetype>, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct File {
        #[allow(dead_code)]
        note: String,
        archetypes: Vec<Archetype>,
    }
    let cards = serde_json::from_str::<File>(text).map_err(|e| format!("archetypes.json: {e}"))?.archetypes;
    if cards.len() != ARCHETYPE_COUNT {
        return Err(format!("{} cards, not {ARCHETYPE_COUNT}", cards.len()));
    }
    for (i, a) in cards.iter().enumerate() {
        let earlier = &cards[..i];
        let texts = [&a.en.name, &a.en.description, &a.pt.name, &a.pt.description];
        if let Some(w) = texts.iter().find_map(|t| borrowed_name(t)) {
            return Err(format!("{} uses a name that isn't ours: {w}", a.id.as_str()));
        }
        if texts.iter().any(|t| t.trim().is_empty() || t.trim() != t.as_str()) {
            return Err(format!("{} has an empty or padded text", a.id.as_str()));
        }
        if a.held_reason.as_ref().is_some_and(|r| r.trim().is_empty() || r.trim() != r.as_str()) {
            return Err(format!("{} has an empty or padded text", a.id.as_str()));
        }
        if a.shipped == a.held_reason.is_some() {
            return Err(format!("{} must state why it is held back, and only if it is", a.id.as_str()));
        }
        if earlier.iter().any(|b| b.id == a.id) {
            return Err(format!("{} appears twice", a.id.as_str()));
        }
        if earlier.iter().any(|b| b.en.name == a.en.name || b.pt.name == a.pt.name) {
            return Err(format!("{} shares a name with another card", a.id.as_str()));
        }
        if earlier.iter().any(|b| b.axes == a.axes) {
            return Err(format!("{} has another card's preset", a.id.as_str()));
        }
    }
    let shipped = cards.iter().filter(|a| a.shipped).count();
    if shipped < MIN_SHIPPED {
        return Err(format!("{shipped} shipped, fewer than {MIN_SHIPPED}"));
    }
    let default = SelfSpec::release_default("").personality;
    if !cards.iter().any(|a| a.shipped && a.personality() == default) {
        return Err(format!("the release default {} is not a shipped card with its preset", default.archetype.as_str()));
    }
    Ok(cards)
}

pub fn archetype_cards() -> &'static [Archetype] {
    static CARDS: std::sync::OnceLock<Vec<Archetype>> = std::sync::OnceLock::new();
    CARDS.get_or_init(|| parse_cards(include_str!("../archetypes.json")).expect("archetypes.json holds to row 5 (rth-self a9 tests)"))
}

pub fn archetype(id: &str) -> Option<&'static Archetype> {
    archetype_cards().iter().find(|a| a.id.as_str() == id)
}

pub fn choose_archetype(saved: &ArchetypeId, new: &ArchetypeId) -> Result<(), Refused> {
    if new == saved || archetype(new.as_str()).is_some_and(|a| a.shipped) {
        Ok(())
    } else {
        Err(refused("personality", &no("archetype_unknown", format!("{} is not an archetype this build offers", new.as_str()))))
    }
}

pub fn archetypes_held() -> usize {
    archetype_cards().iter().filter(|a| !a.shipped).count()
}

fn card(a: &Archetype) -> Value {
    serde_json::json!({
        "id": a.id,
        "name": {"en": a.en.name, "pt": a.pt.name},
        "description": {"en": a.en.description, "pt": a.pt.description},
        "axes": a.axes,
    })
}

pub fn archetypes() -> Vec<Value> {
    archetype_cards().iter().filter(|a| a.shipped).map(card).collect()
}

pub fn archetype_card(id: &str) -> Option<Value> {
    archetype(id).map(card)
}

#[cfg(test)]
mod tests;
