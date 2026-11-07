

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "rth-profile/v1";

pub const KNOWN_TOOLS: &[&str] = &["files.read", "files.search", "web.fetch", "memory.recall"];

pub const RESEARCH_JSON: &str = include_str!("../profiles/research/profile.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {

    None,

    Reversible,

    Irreversible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {

    Folder,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub name: String,
    pub effect: Effect,
    pub needs: Needs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ask {
    PerFolder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderGrants {
    pub scope: Scope,
    pub ask: Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantSpec {
    pub folders: FolderGrants,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {

    Session,

    Local,

    Paid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spend {
    pub tier: Tier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub run_ms: u64,
    pub page_chars: usize,
    pub max_context_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Egress {

    Hold,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,

    pub notes: String,
    pub purposes: Vec<String>,
    pub tools: Vec<ToolSpec>,
    pub grants: GrantSpec,
    pub spend: Spend,
    pub limits: Limits,
    pub egress_after_private: Egress,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parts: Option<crate::authority::Layer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub profile: Profile,
    pub sha256: String,
}

pub fn load(bytes: &[u8]) -> Result<Loaded, String> {
    let profile: Profile = serde_json::from_slice(bytes).map_err(|e| format!("profile: {e}"))?;
    if profile.schema != SCHEMA {
        return Err(format!("profile schema {:?} is not {SCHEMA}", profile.schema));
    }
    let id_ok = |s: &str| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok(&profile.id) {
        return Err(format!("profile id {:?} must be 1-64 of [a-z0-9-]", profile.id));
    }
    if profile.version.is_empty() || profile.title.is_empty() || profile.notes.trim().is_empty() {
        return Err("profile version, title and notes are required".into());
    }
    let mut seen = std::collections::HashSet::new();
    for t in &profile.tools {
        if !KNOWN_TOOLS.contains(&t.name.as_str()) {
            return Err(format!("profile tool {:?} does not exist in this build", t.name));
        }
        if !seen.insert(t.name.as_str()) {
            return Err(format!("profile names tool {:?} twice", t.name));
        }
        if t.effect != Effect::None {
            return Err(format!("profile tool {:?} has effect {:?}; this build runs read tools only (effects need S3's confirm cards)", t.name, t.effect));
        }
        let folder = t.name.starts_with("files.");
        if folder != (t.needs == Needs::Folder) {
            return Err(format!("profile tool {:?}: file tools need a folder grant and only file tools do", t.name));
        }
    }
    if profile.parts.as_ref().is_some_and(|p| p.max_effect != Effect::None) {
        return Err("profile parts: this build runs read tools only (max_effect none)".into());
    }
    let l = &profile.limits;
    if l.max_turns == 0 || l.max_tool_calls == 0 || l.run_ms == 0 || l.page_chars == 0 || l.max_context_chars < l.page_chars {
        return Err("profile limits must be positive, with max_context_chars at least one page".into());
    }
    Ok(Loaded { profile, sha256: crate::sha256_hex(bytes) })
}

pub fn research() -> Loaded {
    load(RESEARCH_JSON.as_bytes()).expect("the shipped research profile loads")
}

impl Loaded {
    pub fn tool(&self, name: &str) -> Option<&ToolSpec> {
        self.profile.tools.iter().find(|t| t.name == name)
    }
}

pub fn wire_name(name: &str) -> String {
    name.replace('.', "_")
}

pub fn from_wire<'a>(p: &'a Loaded, wire: &str) -> Option<&'a ToolSpec> {
    p.profile.tools.iter().find(|t| wire_name(&t.name) == wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(edit: impl FnOnce(&mut serde_json::Value)) -> Result<Loaded, String> {
        let mut v: serde_json::Value = serde_json::from_str(RESEARCH_JSON).unwrap();
        edit(&mut v);
        load(serde_json::to_string(&v).unwrap().as_bytes())
    }

    #[test]
    fn the_shipped_research_profile_loads_with_read_tools_only_and_its_digest() {
        let p = research();
        assert_eq!(p.profile.id, "research");
        assert_eq!(p.sha256, crate::sha256_hex(RESEARCH_JSON.as_bytes()));
        let names: Vec<&str> = p.profile.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["files.read", "files.search", "web.fetch", "memory.recall"]);
        assert!(p.profile.tools.iter().all(|t| t.effect == Effect::None));
        assert_eq!(p.tool("files.read").unwrap().needs, Needs::Folder);
        assert_eq!(p.profile.egress_after_private, Egress::Hold);
        assert!(p.profile.limits.page_chars > 0 && p.profile.limits.page_chars < 24_000, "a page is well under the old 24k cut");
        assert_eq!(p.profile.parts, Some(crate::authority::Layer::up_to(Effect::None)), "read tools of turned-on parts");
    }

    #[test]
    fn unknown_keys_tools_schemas_and_effects_are_refused() {
        assert!(with(|v| v["extra"] = "x".into()).unwrap_err().contains("extra"));
        assert!(with(|v| v["tools"][0]["extra"] = "x".into()).is_err());
        assert!(with(|v| v["schema"] = "rth-profile/v0".into()).unwrap_err().contains("schema"));
        assert!(with(|v| v["tools"][0]["name"] = "shell.run".into()).unwrap_err().contains("shell.run"));
        let e = with(|v| v["tools"][0]["effect"] = "irreversible".into()).unwrap_err();
        assert!(e.contains("effect") && e.contains("files.read"), "{e}");
        assert!(with(|v| v["tools"][1]["name"] = "files.read".into()).unwrap_err().contains("twice"));
        assert!(with(|v| v["tools"][0]["needs"] = "none".into()).unwrap_err().contains("folder"), "file tools always need a folder grant");
        assert!(with(|v| v["limits"]["page_chars"] = 0.into()).is_err());
        assert!(with(|v| v["parts"]["max_effect"] = "reversible".into()).unwrap_err().contains("parts"));
        assert!(with(|v| {
            v.as_object_mut().unwrap().remove("parts");
        }).unwrap().profile.parts.is_none());
        assert!(with(|v| v["id"] = "".into()).is_err());
        assert!(load(b"not json").is_err());
    }

    #[test]
    fn wire_names_round_trip() {
        let p = research();
        assert_eq!(wire_name("files.read"), "files_read");
        assert_eq!(from_wire(&p, "web_fetch").unwrap().name, "web.fetch");
        assert!(from_wire(&p, "files.read").is_none(), "only the wire form is a model name");
        assert!(from_wire(&p, "shell_run").is_none());
    }
}
