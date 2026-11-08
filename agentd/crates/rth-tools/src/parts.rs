

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value, json};

use crate::authority::{Layer, PartManifest, PartTool, Parts, authorize};
use crate::web::BoxFuture;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartOutput {
    pub text: String,

    pub is_error: bool,
}

pub trait Part: Send + Sync {

    fn manifest(&self) -> &PartManifest;

    fn digest(&self) -> &str;

    fn schema(&self, tool: &str) -> Option<(String, Value)>;

    fn asks(&self, tool: &str) -> bool;
    fn call<'a>(&'a self, tool: &'a str, args: Map<String, Value>) -> BoxFuture<'a, Result<PartOutput, String>>;
}

pub fn call_sha256(part_id: &str, digest: &str, tool: &str, args: &Value) -> String {
    crate::canonical_sha256(&json!({"part": part_id, "digest": digest, "tool": tool, "args": args}))
}

pub fn wire(part_id: &str, tool: &str) -> Option<String> {
    let w = format!("{}__{tool}", part_id.replace('.', "_"));
    (w.len() <= 64 && w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')).then_some(w)
}

#[derive(Clone)]
pub struct Offered {
    pub part: Arc<dyn Part>,
    pub tool: PartTool,
    pub wire: String,
}

pub struct PartSet {
    ledger: Mutex<Parts>,
    live: Mutex<Vec<Arc<dyn Part>>>,
    session: Mutex<Layer>,
}

impl PartSet {
    pub fn new(ledger: Parts, session: Layer) -> PartSet {
        PartSet { ledger: Mutex::new(ledger), live: Mutex::new(Vec::new()), session: Mutex::new(session) }
    }

    pub fn insert(&self, part: Arc<dyn Part>) -> Option<Arc<dyn Part>> {
        let mut live = self.live.lock().unwrap();
        let old = live.iter().position(|p| p.manifest().id == part.manifest().id).map(|i| live.remove(i));
        live.push(part);
        old
    }

    pub fn remove(&self, part_id: &str) -> Option<Arc<dyn Part>> {
        let mut live = self.live.lock().unwrap();
        live.iter().position(|p| p.manifest().id == part_id).map(|i| live.remove(i))
    }

    pub fn get(&self, part_id: &str) -> Option<Arc<dyn Part>> {
        self.live.lock().unwrap().iter().find(|p| p.manifest().id == part_id).cloned()
    }

    pub fn live(&self) -> Vec<Arc<dyn Part>> {
        self.live.lock().unwrap().clone()
    }

    pub fn ledger(&self) -> MutexGuard<'_, Parts> {
        self.ledger.lock().unwrap()
    }

    pub fn set_session(&self, layer: Layer) {
        *self.session.lock().unwrap() = layer;
    }

    pub fn offered(&self, profile: Option<&Layer>) -> Vec<Offered> {
        let Some(profile) = profile else { return Vec::new() };
        let session = self.session.lock().unwrap().clone();
        let ledger = self.ledger.lock().unwrap();
        let mut out: Vec<Offered> = Vec::new();
        for part in self.live() {
            let Ok(tools) = authorize(&ledger, part.manifest(), part.digest(), profile, &session) else { continue };
            for tool in tools {
                if let Some(w) = wire(&part.manifest().id, &tool.name)
                    && !out.iter().any(|o| o.wire == w)
                {
                    out.push(Offered { part: part.clone(), tool, wire: w });
                }
            }
        }
        out
    }

    pub fn find(&self, profile: Option<&Layer>, wire_name: &str) -> Option<Offered> {
        self.offered(profile).into_iter().find(|o| o.wire == wire_name)
    }

    pub fn part_of(&self, wire_name: &str) -> Option<(String, String)> {
        self.live().iter().find_map(|p| {
            let id = &p.manifest().id;
            let prefix = format!("{}__", id.replace('.', "_"));
            wire_name.strip_prefix(&prefix).map(|tool| (id.clone(), tool.to_string()))
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use crate::profile::Effect;

    pub struct Echo {
        pub manifest: PartManifest,
        pub digest: String,
        pub asks: bool,
        pub calls: Mutex<Vec<(String, Value)>>,
    }

    impl Echo {
        pub fn new(id: &str, digest: &str, tools: &[(&str, Effect)], asks: bool) -> Arc<Echo> {
            let tools = tools.iter().map(|(n, e)| PartTool { name: n.to_string(), effect: *e, description: Some(format!("{n} things")) }).collect();
            Arc::new(Echo { manifest: PartManifest { schema: crate::authority::SCHEMA.into(), id: id.into(), version: "1".into(), title: id.into(), tools }, digest: digest.into(), asks, calls: Mutex::new(Vec::new()) })
        }
    }

    impl Part for Echo {
        fn manifest(&self) -> &PartManifest {
            &self.manifest
        }

        fn digest(&self) -> &str {
            &self.digest
        }

        fn schema(&self, tool: &str) -> Option<(String, Value)> {
            Some((format!("{tool} things"), json!({"type": "object", "properties": {"q": {"type": "string"}}})))
        }

        fn asks(&self, _tool: &str) -> bool {
            self.asks
        }

        fn call<'a>(&'a self, tool: &'a str, args: Map<String, Value>) -> BoxFuture<'a, Result<PartOutput, String>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((tool.to_string(), Value::Object(args.clone())));
                match args.get("q").and_then(Value::as_str) {
                    Some("fail") => Err("connection reset".into()),
                    Some("big") => Ok(PartOutput { text: "x".repeat(30_000), is_error: false }),
                    q => Ok(PartOutput { text: format!("{tool}: {}", q.unwrap_or("")), is_error: false }),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Echo;
    use super::*;
    use crate::profile::Effect;
    use crate::testdir::TempDir;

    const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn tap() -> crate::authority::UserTap {
        crate::authority::UserTap { effect_id: "e-1".into(), args_sha256: "d".into() }
    }

    #[test]
    fn wire_names_fit_the_function_name_rules() {
        assert_eq!(wire("mcp.docs", "search").as_deref(), Some("mcp_docs__search"));
        assert_eq!(wire("mcp.docs", "find page"), None);
        assert_eq!(wire("mcp.docs", &"x".repeat(60)), None);
    }

    #[test]
    fn only_turned_on_parts_offer_tools_and_only_within_every_layer() {
        let d = TempDir::new("partset");
        let set = PartSet::new(Parts::open(&d.path().join("parts.jsonl")).unwrap(), Layer::up_to(Effect::None));
        set.insert(Echo::new("mcp.docs", A, &[("search", Effect::None), ("delete_page", Effect::Irreversible)], false));
        let reads = Layer::up_to(Effect::None);
        assert!(set.offered(Some(&reads)).is_empty(), "off until the person turns it on");
        set.ledger().turn_on("mcp.docs", A, &tap()).unwrap().unwrap();
        let names: Vec<String> = set.offered(Some(&reads)).iter().map(|o| o.wire.clone()).collect();
        assert_eq!(names, ["mcp_docs__search"], "the irreversible tool stays out");
        assert!(set.offered(None).is_empty(), "a profile without parts gets none");
        assert!(set.find(Some(&reads), "mcp_docs__delete_page").is_none());
        assert_eq!(set.part_of("mcp_docs__delete_page"), Some(("mcp.docs".into(), "delete_page".into())));
        set.insert(Echo::new("mcp.docs", B, &[("search", Effect::None)], false));
        assert!(set.offered(Some(&reads)).is_empty(), "a changed digest is blocked");
        set.ledger().turn_on("mcp.docs", B, &tap()).unwrap().unwrap();
        assert_eq!(set.offered(Some(&reads)).len(), 1);
        set.set_session(Layer { allow: vec!["*".into()], deny: vec!["search".into()], max_effect: Effect::None });
        assert!(set.offered(Some(&reads)).is_empty(), "the session's deny wins");
        assert!(set.remove("mcp.docs").is_some() && set.live().is_empty());
    }
}
