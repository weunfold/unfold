

use std::time::{Duration, Instant};

use rth_protocol::Effort;
use serde_json::{json, Value};

use crate::local;

pub const REPLY_ESTIMATE: u32 = 400;

pub const CHECK_DEADLINE: Duration = Duration::from_millis(150);

pub const MIN_CONFIDENCE: f64 = 0.2;

const TOP: u32 = 20;

const CHECK_CHARS: usize = 2000;

pub const CHECK_PROMPT: &str = "You sort chat messages by how much work their reply needs. Read the user's message and answer with one letter only.\nA) a quick reply: a greeting, thanks, small talk, a yes or no, one fact, a short translation, conversion or rewording\nB) some thought: advice, an explanation, a comparison, a plan or a letter written in a few paragraphs\nC) careful reasoning written as one answer (a proof, multi-step math, an algorithm or code with tests, a hard design)\nD) doing a multi-step job with tools: searching or researching, reading or changing files and folders, running code, several steps\nExamples:\n\"hey, how's it going?\" -> A\n\"obrigado!\" -> A\n\"what's the boiling point of water in Fahrenheit?\" -> A\n\"how do I fix a squeaky door hinge?\" -> B\n\"me dá ideias de jantar rápido pra semana\" -> B\n\"show that the sum of two odd numbers is even\" -> C\n\"implement Dijkstra's algorithm in Python with tests\" -> C\n\"look up the opening hours of the three nearest pharmacies and list them\" -> D\n\"organiza os arquivos da minha pasta Documentos por ano\" -> D";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Off,
    Low,
    High,
    Max,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Low => "low",
            Level::High => "high",
            Level::Max => "max",
        }
    }

    pub fn think_budget(self) -> u32 {
        match self {
            Level::Off => 0,
            Level::Low => 2048,
            Level::High => 4096,
            Level::Max => 8192,
        }
    }

    pub fn reasoning_effort(self) -> &'static str {
        match self {
            Level::Off => "none",
            l => l.as_str(),
        }
    }

    pub fn thinks(self) -> bool {
        self != Level::Off
    }

    pub fn deepseek_fields(self, body: &mut Value) {
        body["thinking"] = json!({"type": if self.thinks() { "enabled" } else { "disabled" }});
        body["reasoning_effort"] = json!(self.reasoning_effort());
    }
}

pub fn max_tokens(reply: Option<u32>, level: Level) -> u32 {
    let limit = crate::flash::REPLY_LIMIT;
    reply.map_or(limit, |r| r.saturating_add(level.think_budget()).min(limit))
}

pub fn estimate(reply: Option<u32>, level: Level, max_tokens: u32) -> u32 {
    reply.unwrap_or(REPLY_ESTIMATE).saturating_add(level.think_budget()).min(max_tokens)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {

    #[default]
    Default,

    Person,

    Auto,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::Person => "person",
            Source::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {

    Quick,

    Some,

    Careful,

    MultiStep,
}

impl Class {
    pub fn letter(self) -> &'static str {
        match self {
            Class::Quick => "A",
            Class::Some => "B",
            Class::Careful => "C",
            Class::MultiStep => "D",
        }
    }

    pub fn level(self) -> Level {
        match self {
            Class::Quick => Level::Off,
            Class::Some => Level::Low,
            Class::Careful => Level::High,
            Class::MultiStep => Level::Low,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {

    pub class: Option<Class>,

    pub confidence: Option<f64>,
    pub ms: u64,

    pub reason: &'static str,
}

impl Check {
    fn unknown(reason: &'static str, ms: u64) -> Check {
        Check { class: None, confidence: None, ms, reason }
    }

    pub fn level(&self) -> Level {
        self.class.map_or(Level::Off, Class::level)
    }

    pub fn json(&self) -> Value {
        json!({"class": self.class.map(Class::letter), "confidence": self.confidence.map(|c| (c * 1000.0).round() / 1000.0), "ms": self.ms, "reason": self.reason})
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurnLevel {
    pub level: Level,
    pub source: Source,
    pub check: Option<Check>,
    pub reply_max_tokens: Option<u32>,
}

impl TurnLevel {

    pub fn person(e: Effort) -> TurnLevel {
        let level = match e {
            Effort::Off | Effort::Auto => Level::Off,
            Effort::Low => Level::Low,
            Effort::High => Level::High,
            Effort::Max => Level::Max,
        };
        TurnLevel { level, source: Source::Person, check: None, reply_max_tokens: None }
    }

    pub fn auto(check: Check) -> TurnLevel {
        TurnLevel { level: check.level(), source: Source::Auto, check: Some(check), reply_max_tokens: None }
    }

    pub fn with_reply(self, reply_max_tokens: Option<u32>) -> TurnLevel {
        TurnLevel { reply_max_tokens, ..self }
    }

    pub fn extend(&self, config: &mut Value) {
        config["effort"] = json!(self.level.as_str());
        config["thinking"] = json!(self.level.thinks());
        config["effort_source"] = json!(self.source.as_str());
        config["think_budget"] = json!(self.level.think_budget());
        config["max_tokens"] = json!(max_tokens(self.reply_max_tokens, self.level));
        config["reply_max_tokens"] = json!(self.reply_max_tokens);
        if let Some(c) = &self.check {
            config["auto"] = c.json();
        }
    }
}

pub fn wants_check(effort: Option<Effort>) -> bool {
    effort == Some(Effort::Auto)
}

pub fn without_check(effort: Option<Effort>, why: &'static str) -> TurnLevel {
    match effort {
        None => TurnLevel::default(),
        Some(Effort::Auto) => TurnLevel::auto(Check::unknown(why, 0)),
        Some(e) => TurnLevel::person(e),
    }
}

#[derive(Debug, Clone)]
pub struct Checker {
    pub cfg: local::Config,
    pub slot: u32,
}

impl Checker {

    pub fn new(cfg: local::Config, slot: Option<&str>) -> Result<Checker, String> {
        let Some(s) = slot.filter(|s| !s.is_empty()) else { return Err("RTH_LOCAL_CHECK_SLOT unset".into()) };
        let slot = s.parse::<u32>().ok().filter(|n| *n < 16).ok_or_else(|| format!("RTH_LOCAL_CHECK_SLOT={s:?} is not a slot number 0-15"))?;
        match (cfg.slot, cfg.decide_slot) {
            (Some(t), Some(d)) if t != slot && d != slot => Ok(Checker { cfg, slot }),
            _ => Err(format!("RTH_LOCAL_CHECK_SLOT={slot} needs RTH_LOCAL_SLOT and RTH_LOCAL_DECIDE_SLOT set to other slots")),
        }
    }
}

pub fn body(model: &str, text: &str, slot: u32) -> Value {
    let text: String = text.chars().take(CHECK_CHARS).collect();
    json!({
        "model": model,
        "messages": [{"role": "system", "content": CHECK_PROMPT}, {"role": "user", "content": text}],
        "temperature": 0,
        "max_tokens": 1,
        "stream": false,
        "logprobs": true,
        "top_logprobs": TOP,
        "chat_template_kwargs": {"enable_thinking": false},
        "cache_prompt": true,
        "id_slot": slot,
    })
}

pub fn letters(answer: &Value) -> Option<[f64; 4]> {
    let top = answer.pointer("/choices/0/logprobs/content/0/top_logprobs")?.as_array()?;
    let mut p = [0.0f64; 4];
    for t in top {
        let (Some(tok), Some(lp)) = (t["token"].as_str(), t["logprob"].as_f64()) else { continue };
        let i = match tok.trim().to_ascii_uppercase().as_str() {
            "A" => 0,
            "B" => 1,
            "C" => 2,
            "D" => 3,
            _ => continue,
        };
        p[i] += lp.exp();
    }
    let sum: f64 = p.iter().sum();
    (sum > 0.0).then(|| p.map(|x| x / sum))
}

pub fn classify(p: [f64; 4]) -> (Class, f64) {
    let (i, pmax) = p.iter().copied().enumerate().fold((0, f64::MIN), |a, (i, x)| if x > a.1 { (i, x) } else { a });
    let class = [Class::Quick, Class::Some, Class::Careful, Class::MultiStep][i];
    (class, (4.0 * pmax - 1.0) / 3.0)
}

pub async fn check(c: &Checker, text: &str, deadline: Duration) -> Check {
    let t = Instant::now();
    let ms = |t: Instant| t.elapsed().as_millis() as u64;
    let body = body(&c.cfg.model, text, c.slot);
    let path = format!("{}/chat/completions", c.cfg.base_path);
    let answer = match tokio::time::timeout(deadline, local::post_json(&c.cfg.authority, &path, &body, deadline)).await {
        Err(_) => return Check::unknown("timeout", ms(t)),
        Ok(Err(f)) if f.reason == local::FIRST_TOKEN_TIMEOUT => return Check::unknown("timeout", ms(t)),
        Ok(Err(_)) => return Check::unknown("error", ms(t)),
        Ok(Ok((200, raw))) => match serde_json::from_slice::<Value>(&raw) {
            Ok(v) => v,
            Err(_) => return Check::unknown("error", ms(t)),
        },
        Ok(Ok(_)) => return Check::unknown("error", ms(t)),
    };
    let Some(p) = letters(&answer) else { return Check::unknown("no_letter", ms(t)) };
    let (class, confidence) = classify(p);
    if confidence < MIN_CONFIDENCE {
        return Check { class: None, confidence: Some(confidence), ms: ms(t), reason: "low_confidence" };
    }
    Check { class: Some(class), confidence: Some(confidence), ms: ms(t), reason: "ok" }
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
