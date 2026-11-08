

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::profile::Effect;

pub const SCHEMA: &str = "rth-part/v1";

fn rank(e: Effect) -> u8 {
    match e {
        Effect::None => 0,
        Effect::Reversible => 1,
        Effect::Irreversible => 2,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartTool {
    pub name: String,
    pub effect: Effect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartManifest {
    pub schema: String,
    pub id: String,
    pub version: String,
    pub title: String,
    pub tools: Vec<PartTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {

    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    pub max_effect: Effect,
}

impl Layer {

    pub fn up_to(max_effect: Effect) -> Layer {
        Layer { allow: vec!["*".into()], deny: Vec::new(), max_effect }
    }
}

pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => !pattern.is_empty() && pattern == name,
    }
}

pub fn narrow(ceiling: &[PartTool], layers: &[&Layer]) -> Vec<PartTool> {
    ceiling
        .iter()
        .filter(|t| {
            layers.iter().all(|l| l.allow.iter().any(|p| matches(p, &t.name)) && rank(t.effect) <= rank(l.max_effect))
                && !layers.iter().any(|l| l.deny.iter().any(|p| matches(p, &t.name)))
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserTap {
    pub effect_id: String,
    pub args_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Line {
    TurnedOn { part_id: String, digest: String, effect_id: String, args_sha256: String, utc_ms: u64 },
    TurnedOff { part_id: String, by: String, utc_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {

    Off,

    Changed { approved: String, current: String },
    On { digest: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocked {
    Off,
    Changed { approved: String, current: String },
    Invalid(String),
}

pub struct Parts {
    path: PathBuf,
    file: std::fs::File,
    on: BTreeMap<String, String>,
}

impl Parts {
    pub fn open(path: &Path) -> std::io::Result<Parts> {
        let existed = path.exists();
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {
            file.sync_all()?;
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()?;
        }
        let mut text = Self::read_all(&mut file)?;
        let keep = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
        if keep < text.len() {
            file.set_len(keep as u64)?;
            file.sync_all()?;
            text.truncate(keep);
        }
        let mut p = Parts { path: path.to_path_buf(), file, on: BTreeMap::new() };
        for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let l: Line = serde_json::from_str(line).map_err(|e| std::io::Error::other(format!("{}:{}: {e}", path.display(), n + 1)))?;
            p.fold(l);
        }
        Ok(p)
    }

    fn fold(&mut self, l: Line) {
        match l {
            Line::TurnedOn { part_id, digest, .. } => {
                self.on.insert(part_id, digest);
            }
            Line::TurnedOff { part_id, .. } => {
                self.on.remove(&part_id);
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn turn_on(&mut self, part_id: &str, digest: &str, tap: &UserTap) -> std::io::Result<Result<Line, &'static str>> {
        if part_id.is_empty() || part_id.len() > 128 {
            return Ok(Err("bad_part_id"));
        }
        if !crate::digest::is_spelling(digest) {
            return Ok(Err("bad_digest"));
        }
        let line = Line::TurnedOn { part_id: part_id.into(), digest: digest.into(), effect_id: tap.effect_id.clone(), args_sha256: tap.args_sha256.clone(), utc_ms: crate::utc_ms() };
        self.write(&line)?;
        self.fold(line.clone());
        Ok(Ok(line))
    }

    pub fn turn_off(&mut self, part_id: &str, by: &str) -> std::io::Result<Result<Line, &'static str>> {
        if !self.on.contains_key(part_id) {
            return Ok(Err("not_on"));
        }
        let line = Line::TurnedOff { part_id: part_id.into(), by: by.into(), utc_ms: crate::utc_ms() };
        self.write(&line)?;
        self.fold(line.clone());
        Ok(Ok(line))
    }

    pub fn standing(&self, part_id: &str, current: &str) -> Standing {
        match self.on.get(part_id) {
            None => Standing::Off,
            Some(d) if d == current => Standing::On { digest: d.clone() },
            Some(d) => Standing::Changed { approved: d.clone(), current: current.to_string() },
        }
    }

    pub fn on(&self) -> Vec<(String, String)> {
        self.on.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    fn read_all(file: &mut std::fs::File) -> std::io::Result<String> {
        let mut text = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut text)?;
        Ok(text)
    }

    fn write(&mut self, line: &Line) -> std::io::Result<()> {
        let mut t = serde_json::to_string(line).map_err(std::io::Error::other)?;
        t.push('\n');
        self.file.write_all(t.as_bytes())?;
        self.file.sync_data()
    }
}

pub fn authorize(parts: &Parts, manifest: &PartManifest, current_digest: &str, profile: &Layer, session: &Layer) -> Result<Vec<PartTool>, Blocked> {
    if manifest.schema != SCHEMA {
        return Err(Blocked::Invalid(format!("part schema {:?} is not {SCHEMA}", manifest.schema)));
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(t) = manifest.tools.iter().find(|t| !seen.insert(t.name.as_str())) {
        return Err(Blocked::Invalid(format!("part {} declares {} twice", manifest.id, t.name)));
    }
    match parts.standing(&manifest.id, current_digest) {
        Standing::Off => Err(Blocked::Off),
        Standing::Changed { approved, current } => Err(Blocked::Changed { approved, current }),
        Standing::On { .. } => Ok(narrow(&manifest.tools, &[profile, session])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TempDir;

    fn t(name: &str, effect: Effect) -> PartTool {
        PartTool { name: name.into(), effect, description: None }
    }

    fn manifest() -> PartManifest {
        PartManifest {
            schema: SCHEMA.into(),
            id: "mcp.docs".into(),
            version: "1".into(),
            title: "Docs".into(),
            tools: vec![t("search", Effect::None), t("read_page", Effect::None), t("edit_page", Effect::Reversible), t("delete_page", Effect::Irreversible)],
        }
    }

    fn names(v: &[PartTool]) -> Vec<&str> {
        v.iter().map(|t| t.name.as_str()).collect()
    }

    const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn tap() -> UserTap {
        UserTap { effect_id: "e-1".into(), args_sha256: "d".into() }
    }

    #[test]
    fn patterns_match_all_a_prefix_or_one_name() {
        assert!(matches("*", "x") && matches("read*", "read_page") && matches("search", "search"));
        assert!(!matches("read*", "search") && !matches("search", "search_all") && !matches("", "x"));
    }

    #[test]
    fn the_ceiling_is_narrowed_by_every_layer_and_a_deny_anywhere_wins() {
        let m = manifest();
        let all = Layer::up_to(Effect::Irreversible);
        assert_eq!(names(&narrow(&m.tools, &[&all, &all])), ["search", "read_page", "edit_page", "delete_page"]);
        let read_only = Layer::up_to(Effect::None);
        assert_eq!(names(&narrow(&m.tools, &[&all, &read_only])), ["search", "read_page"], "the session's effect cap");
        let profile = Layer { allow: vec!["search".into(), "read*".into(), "made_up".into()], deny: vec![], max_effect: Effect::Irreversible };
        assert_eq!(names(&narrow(&m.tools, &[&profile, &all])), ["search", "read_page"], "no layer adds a tool the manifest lacks");
        let session = Layer { allow: vec!["*".into()], deny: vec!["read_page".into()], max_effect: Effect::Irreversible };
        assert_eq!(names(&narrow(&m.tools, &[&profile, &session])), ["search"], "a deny wins over an allow in another layer");
        let none = Layer { allow: vec![], deny: vec![], max_effect: Effect::Irreversible };
        assert!(narrow(&m.tools, &[&all, &none]).is_empty(), "an empty allow list allows nothing");
        let kept = narrow(&m.tools, &[&all]);
        assert_eq!(kept.iter().find(|t| t.name == "edit_page").unwrap().effect, Effect::Reversible, "effects are the manifest's");
    }

    #[test]
    fn a_part_runs_only_at_the_digest_the_person_approved() {
        let d = TempDir::new("parts");
        let log = d.path().join("parts.jsonl");
        let m = manifest();
        let all = Layer::up_to(Effect::None);
        {
            let mut p = Parts::open(&log).unwrap();
            assert_eq!(authorize(&p, &m, A, &all, &all), Err(Blocked::Off), "off until the person turns it on");
            p.turn_on("mcp.docs", A, &tap()).unwrap().unwrap();
            assert_eq!(names(&authorize(&p, &m, A, &all, &all).unwrap()), ["search", "read_page"]);
            assert_eq!(authorize(&p, &m, B, &all, &all), Err(Blocked::Changed { approved: A.into(), current: B.into() }), "changed contents are blocked");
        }
        let mut p = Parts::open(&log).unwrap();
        assert_eq!(p.standing("mcp.docs", A), Standing::On { digest: A.into() }, "remembered after a restart");
        assert_eq!(p.standing("mcp.docs", B), Standing::Changed { approved: A.into(), current: B.into() });
        p.turn_on("mcp.docs", B, &tap()).unwrap().unwrap();
        assert!(authorize(&p, &m, B, &all, &all).is_ok(), "re-approved");
        p.turn_off("mcp.docs", "settings").unwrap().unwrap();
        assert_eq!(p.standing("mcp.docs", B), Standing::Off);
        assert_eq!(p.turn_off("mcp.docs", "settings").unwrap().unwrap_err(), "not_on");
        let mut wrong = m.clone();
        wrong.id = "mcp.other".into();
        assert!(matches!(authorize(&p, &wrong, B, &all, &all), Err(Blocked::Off)));
        let mut bad = m.clone();
        bad.schema = "x".into();
        assert!(matches!(authorize(&p, &bad, B, &all, &all), Err(Blocked::Invalid(_))));
        assert_eq!(p.turn_on("mcp.docs", "not-a-digest", &tap()).unwrap().unwrap_err(), "bad_digest");
        let events: Vec<String> = std::fs::read_to_string(&log).unwrap().lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(events[..3], ["turned_on", "turned_on", "turned_off"]);
    }
}
