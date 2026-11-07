

use std::path::Path;

use serde_json::{json, Value};

use crate::store::{self, Lines, Loaded};
use crate::{NameOrigin, Refused, SelfSpec};

pub const FORGET_NOTE: &str = "A memory forgotten in the original after this copy stays in the copy; forget it there too.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentItem {
    pub memory_id: String,
    pub text: String,

    pub saved_utc_ms: Option<u64>,
}

pub trait ChildMemory {
    fn remember_dated(&mut self, operation_key: &str, text: &str, original_utc_ms: Option<u64>) -> Result<String, String>;

    fn forget_by_key(&mut self, operation_key: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForkError {

    ChildNotEmpty,

    ParentSelfRefused(Refused),

    LogDamaged { side: &'static str },
    Io(String),

    Memory { memory_id: String, detail: String },
}

fn io(e: std::io::Error) -> ForkError {
    ForkError::Io(e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forked {
    pub spec: SelfSpec,

    pub copied: Vec<(String, String)>,

    pub left_out_forgotten: usize,

    pub undated: usize,
    pub note: &'static str,
}

fn log_of(dir: &Path, side: &'static str) -> Result<Lines, ForkError> {
    let log = store::read_lines(&dir.join("governance.jsonl")).map_err(io)?;
    if log.torn || log.corrupt {
        return Err(ForkError::LogDamaged { side });
    }
    Ok(log)
}

fn is_line(v: &Value, fork_id: &str, side: &str, stage: Option<&str>) -> bool {
    v["kind"] == "fork" && v["fork_id"] == fork_id && v["side"] == side && stage.is_none_or(|s| v["stage"] == s)
}

fn has_own_state(child_dir: &Path, log: &Lines, ours: &dyn Fn(&Value) -> bool) -> bool {
    child_dir.join("proposals.jsonl").exists() || child_dir.join("horizons.jsonl").exists() || log.records.iter().any(|v| !ours(v))
}

fn child_spec(parent: &SelfSpec, child_instance_id: &str) -> SelfSpec {
    let mut s = parent.clone();
    s.instance_id = child_instance_id.to_string();
    s.name.origin = NameOrigin::Given;
    s.name.proposal = None;
    s.expression.destinations.notification = false;
    s.expression.destinations.omni = false;
    s
}

#[allow(clippy::too_many_arguments)]
pub fn fork(fork_id: &str, parent_dir: &Path, parent_instance_id: &str, child_dir: &Path, child_instance_id: &str, items: &[ParentItem], live: &dyn Fn(&str) -> bool, child: &mut dyn ChildMemory, now_utc_ms: u64) -> Result<Forked, ForkError> {
    let (clog, plog) = (log_of(child_dir, "child")?, log_of(parent_dir, "parent")?);
    let done = clog.records.iter().find(|v| is_line(v, fork_id, "child", Some("done"))).cloned();

    if done.is_none() && (has_own_state(child_dir, &clog, &|v| is_line(v, fork_id, "child", None)) || child_dir.join("self.json").exists()) {
        return Err(ForkError::ChildNotEmpty);
    }
    let parent = match store::load(&parent_dir.join("self.json")).map_err(io)? {
        Loaded::Absent => SelfSpec::release_default(parent_instance_id),
        Loaded::Spec(s) => s,
        Loaded::Refused(r) => return Err(ForkError::ParentSelfRefused(r)),
    };
    let gov = |d: &Path| d.join("governance.jsonl");
    let base = |side: &str, other: &str| json!({"kind": "fork", "side": side, "fork_id": fork_id, "other_instance_id": other, "forget_propagates": false, "utc_ms": now_utc_ms});
    let done = match done {
        Some(d) => d,
        None => {
            let planned: Vec<String> = match clog.records.iter().find(|v| is_line(v, fork_id, "child", Some("started"))) {
                Some(s) => serde_json::from_value(s["planned"].clone()).map_err(|e| ForkError::Io(e.to_string()))?,
                None => {
                    let ids: Vec<String> = items.iter().map(|i| i.memory_id.clone()).collect();
                    let mut started = base("child", parent_instance_id);
                    started["stage"] = json!("started");
                    started["planned"] = json!(ids);
                    store::append(&gov(child_dir), &started).map_err(io)?;
                    ids
                }
            };
            let (mut copied, mut left_out_forgotten, mut undated) = (Vec::new(), 0, 0);
            for memory_id in &planned {
                let key = format!("fork/{fork_id}/{memory_id}");
                let failed = |detail| ForkError::Memory { memory_id: memory_id.clone(), detail };
                let Some(item) = items.iter().find(|i| &i.memory_id == memory_id).filter(|i| live(&i.memory_id)) else {
                    child.forget_by_key(&key).map_err(failed)?;
                    left_out_forgotten += 1;
                    continue;
                };
                let id = child.remember_dated(&key, &item.text, item.saved_utc_ms).map_err(failed)?;
                undated += usize::from(item.saved_utc_ms.is_none());
                copied.push((memory_id.clone(), id));
            }
            let mut d = base("child", parent_instance_id);
            d["stage"] = json!("done");
            d["copied"] = json!(copied.len());
            d["left_out_forgotten"] = json!(left_out_forgotten);
            d["undated"] = json!(undated);
            d["copied_ids"] = json!(copied);
            store::append(&gov(child_dir), &d).map_err(io)?;
            d
        }
    };
    let copied: Vec<(String, String)> = serde_json::from_value(done["copied_ids"].clone()).map_err(|e| ForkError::Io(e.to_string()))?;
    let count = |k: &str| done[k].as_u64().unwrap_or(0) as usize;
    let spec = match store::load(&child_dir.join("self.json")).map_err(io)? {
        Loaded::Spec(s) => s,
        Loaded::Absent => {
            let s = child_spec(&parent, child_instance_id);
            store::save(&child_dir.join("self.json"), &s).map_err(io)?;
            s
        }
        Loaded::Refused(_) => return Err(ForkError::ChildNotEmpty),
    };
    if !plog.records.iter().any(|v| is_line(v, fork_id, "parent", None)) {
        let mut p = base("parent", child_instance_id);
        for k in ["copied", "left_out_forgotten", "undated"] {
            p[k] = done[k].clone();
        }
        store::append(&gov(parent_dir), &p).map_err(io)?;
    }
    Ok(Forked { spec, copied, left_out_forgotten: count("left_out_forgotten"), undated: count("undated"), note: FORGET_NOTE })
}

pub fn blank(child_dir: &Path, child_instance_id: &str, now_utc_ms: u64) -> Result<SelfSpec, ForkError> {
    let log = log_of(child_dir, "child")?;
    let claimed = log.records.iter().any(|v| v["kind"] == "blank");
    if has_own_state(child_dir, &log, &|v| v["kind"] == "blank") || child_dir.join("self.json").exists() {
        return Err(ForkError::ChildNotEmpty);
    }
    if !claimed {
        store::append(&child_dir.join("governance.jsonl"), &json!({"kind": "blank", "utc_ms": now_utc_ms})).map_err(io)?;
    }
    let spec = SelfSpec::release_default(child_instance_id);
    store::save(&child_dir.join("self.json"), &spec).map_err(io)?;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent;
    use serde_json::Value;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn dir() -> std::path::PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("rth-self-fork-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[derive(Default)]
    struct FakeBrain {
        index: Vec<(String, String, String, u64)>,
        fail_on: Option<String>,
    }

    const CHILD_NOW: u64 = 9_000;

    impl ChildMemory for FakeBrain {
        fn remember_dated(&mut self, key: &str, text: &str, original_utc_ms: Option<u64>) -> Result<String, String> {
            if let Some(l) = self.index.iter().find(|l| l.0 == key) {
                return Ok(l.1.clone());
            }
            if self.fail_on.as_deref() == Some(text) {
                self.fail_on = None;
                return Err("timeout".into());
            }
            let id = format!("rm-{}", self.index.len() + 1);
            self.index.push((key.into(), id.clone(), text.into(), original_utc_ms.unwrap_or(CHILD_NOW)));
            Ok(id)
        }

        fn forget_by_key(&mut self, key: &str) -> Result<(), String> {
            self.index.retain(|l| l.0 != key);
            Ok(())
        }
    }

    fn items() -> Vec<ParentItem> {
        let it = |id: &str, text: &str, t: Option<u64>| ParentItem { memory_id: id.into(), text: text.into(), saved_utc_ms: t };
        vec![it("rm-1", "likes green tea", Some(100)), it("rm-2", "the old address", Some(200)), it("rm-3", "has a cat named Miso", Some(300)), it("rm-4", "plays the cello", None)]
    }

    fn live(id: &str) -> bool {
        id != "rm-2"
    }

    fn parent_with_a_chosen_name(d: &Path) -> SelfSpec {
        let mut v = serde_json::to_value(SelfSpec::release_default("parent")).unwrap();
        v["name"] = json!({"current": "Lua", "origin": "chosen", "proposal": "p-7"});
        v["personality"] = json!({"archetype": "workbench", "axes": {"energy": 25, "mind": 20, "nature": 30, "tactics": 75, "identity": 40}});
        v["drives"]["motivations"] = json!([{"text": "learn the plants on the desk", "source": "person"}]);
        v["expression"]["destinations"]["notification"] = json!(true);
        let s = SelfSpec::parse(&v.to_string()).unwrap();
        store::save(&d.join("self.json"), &s).unwrap();
        s
    }

    fn log(d: &Path) -> Vec<Value> {
        store::read_lines(&d.join("governance.jsonl")).unwrap().records
    }

    #[test]
    fn a10_a_fork_copies_only_live_memories_into_the_child_index_with_their_original_dates() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain::default();
        let f = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).unwrap();
        let index: Vec<(&str, u64)> = brain.index.iter().map(|l| (l.2.as_str(), l.3)).collect();
        assert_eq!(index, vec![("likes green tea", 100), ("has a cat named Miso", 300), ("plays the cello", CHILD_NOW)]);
        assert!(!brain.index.iter().any(|l| l.2 == "the old address"), "a forgotten memory never reaches the child");
        assert_eq!((f.left_out_forgotten, f.undated), (1, 1));
        assert_eq!(f.copied, vec![("rm-1".into(), "rm-1".into()), ("rm-3".into(), "rm-2".into()), ("rm-4".into(), "rm-3".into())]);
    }

    #[test]
    fn a10_the_child_keeps_the_parents_self_and_name_until_it_proposes_its_own() {
        let (p, c) = (dir(), dir());
        let parent = parent_with_a_chosen_name(&p);
        let f = fork("f1", &p, "parent", &c, "child", &[], &live, &mut FakeBrain::default(), 5_000).unwrap();
        let Loaded::Spec(saved) = store::load(&c.join("self.json")).unwrap() else { panic!("the child's Self loads") };
        assert_eq!(saved, f.spec);
        assert_eq!((saved.instance_id.as_str(), saved.name.current.as_str(), saved.name.origin, saved.name.proposal.clone()), ("child", "Lua", NameOrigin::Given, None));
        assert_eq!((saved.personality.clone(), saved.drives.clone(), saved.style), (parent.personality, parent.drives, parent.style));
        let d = saved.expression.destinations;
        assert_eq!((d.chat, d.notification, d.omni), (true, false, false), "consent doesn't copy, so the outward destinations start off");

        let Loaded::Spec(still) = store::load(&p.join("self.json")).unwrap() else { panic!() };
        assert_eq!(still.name.origin, NameOrigin::Chosen);
    }

    #[test]
    fn a10_the_fork_is_logged_in_both_and_says_a_later_forget_stays_behind() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain::default();
        let f = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).unwrap();
        let (pl, cl) = (log(&p), log(&c));
        assert_eq!((pl.len(), cl.len()), (1, 2));
        assert_eq!((cl[0]["stage"].as_str(), cl[1]["stage"].as_str(), cl[0]["fork_id"].as_str()), (Some("started"), Some("done"), Some("f1")));
        for (l, side, other) in [(&pl[0], "parent", "child"), (&cl[1], "child", "parent")] {
            assert_eq!((l["kind"].as_str(), l["side"].as_str(), l["other_instance_id"].as_str(), l["fork_id"].as_str()), (Some("fork"), Some(side), Some(other), Some("f1")));
            assert_eq!((l["copied"].as_u64(), l["forget_propagates"].as_bool(), l["utc_ms"].as_u64()), (Some(3), Some(false), Some(5_000)));
            assert!(l.get("channel").is_none() && !l.to_string().contains("green tea"), "ids and counts only: {l}");
        }
        assert_eq!(cl[1]["copied_ids"], json!([["rm-1", "rm-1"], ["rm-3", "rm-2"], ["rm-4", "rm-3"]]));
        assert!(consent::fold(&store::read_lines(&c.join("governance.jsonl")).unwrap()).is_empty());
        assert!(f.note.contains("stays in the copy") && f.note.contains("forget it there too"), "{}", f.note);
    }

    #[test]
    fn a10_a_fork_into_a_child_that_has_a_self_is_refused_and_writes_nothing() {
        let (p, c) = (dir(), dir());
        store::save(&c.join("self.json"), &SelfSpec::release_default("child")).unwrap();
        let mut brain = FakeBrain::default();
        assert_eq!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000), Err(ForkError::ChildNotEmpty));
        assert!(brain.index.is_empty() && log(&p).is_empty() && log(&c).is_empty());
        let (p, c) = (dir(), dir());
        std::fs::write(p.join("self.json"), "{\"schema_version\": 9}").unwrap();
        assert!(matches!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000), Err(ForkError::ParentSelfRefused(_))));
        assert!(!c.join("self.json").exists() && brain.index.is_empty());
    }

    #[test]
    fn a10_a_fork_stopped_by_a_memory_resumes_without_copying_twice() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain { fail_on: Some("has a cat named Miso".into()), ..FakeBrain::default() };
        let r = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000);
        assert!(matches!(r, Err(ForkError::Memory { ref memory_id, .. }) if memory_id == "rm-3"), "{r:?}");
        assert!(!c.join("self.json").exists() && log(&p).is_empty(), "no Self and no parent line yet");
        assert_eq!(log(&c).iter().map(|l| l["stage"].as_str()).collect::<Vec<_>>(), vec![Some("started")], "only the claim");
        fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_001).unwrap();
        let texts: Vec<&str> = brain.index.iter().map(|l| l.2.as_str()).collect();
        assert_eq!(texts, vec!["likes green tea", "has a cat named Miso", "plays the cello"]);
    }

    #[test]
    fn a10_a_retried_fork_finishes_a_missing_log_line_once() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain::default();
        fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).unwrap();
        std::fs::remove_file(p.join("governance.jsonl")).unwrap();
        let again = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_001).unwrap();
        assert_eq!((log(&p).len(), log(&c).len(), brain.index.len(), again.copied.len()), (1, 2, 3, 3));
        assert_eq!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_002).map(|f| f.copied.len()), Ok(3), "a whole retry is a no-op");
        assert_eq!((log(&p).len(), log(&c).len()), (1, 2));
        assert_eq!(fork("f2", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_003), Err(ForkError::ChildNotEmpty));
    }

    #[test]
    fn a10_blank_starts_from_the_release_defaults_once() {
        let c = dir();
        assert_eq!(blank(&c, "child", 7).unwrap(), SelfSpec::release_default("child"));
        let Loaded::Spec(saved) = store::load(&c.join("self.json")).unwrap() else { panic!() };
        assert_eq!((saved, log(&c)), (SelfSpec::release_default("child"), vec![json!({"kind": "blank", "utc_ms": 7})]));
        assert_eq!(blank(&c, "child", 8), Err(ForkError::ChildNotEmpty));
        assert_eq!(fork("f1", &dir(), "parent", &c, "child", &[], &live, &mut FakeBrain::default(), 9), Err(ForkError::ChildNotEmpty));
    }

    fn torn(d: &Path) {
        std::fs::write(d.join("governance.jsonl"), "{\"channel\": \"notification\", \"copy_version\": 1, \"granted_utc_ms\": 1, \"revoked\": false}\n{\"kind\": \"fo").unwrap();
    }

    #[test]
    fn a10_1_a_torn_log_on_either_side_refuses_and_writes_nothing() {
        for side in ["child", "parent"] {
            let (p, c) = (dir(), dir());
            torn(if side == "child" { &c } else { &p });
            let before = (std::fs::read(p.join("governance.jsonl")).ok(), std::fs::read(c.join("governance.jsonl")).ok());
            let mut brain = FakeBrain::default();
            assert_eq!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000), Err(ForkError::LogDamaged { side: if side == "child" { "child" } else { "parent" } }));
            assert_eq!((std::fs::read(p.join("governance.jsonl")).ok(), std::fs::read(c.join("governance.jsonl")).ok()), before, "{side}: nothing appended");
            assert!(brain.index.is_empty() && !c.join("self.json").exists());
        }
        let c = dir();
        torn(&c);
        assert_eq!(blank(&c, "child", 7), Err(ForkError::LogDamaged { side: "child" }));
    }

    #[test]
    fn a10_1_a_memory_forgotten_between_attempts_is_taken_back_and_a_finished_fork_copies_nothing() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain { fail_on: Some("plays the cello".into()), ..FakeBrain::default() };
        assert!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).is_err());
        assert!(brain.index.iter().any(|l| l.2 == "has a cat named Miso"), "attempt 1 copied rm-3");
        let live2 = |id: &str| id != "rm-2" && id != "rm-3";
        let f = fork("f1", &p, "parent", &c, "child", &items(), &live2, &mut brain, 5_001).unwrap();
        let texts: Vec<&str> = brain.index.iter().map(|l| l.2.as_str()).collect();
        assert_eq!(texts, vec!["likes green tea", "plays the cello"], "the forgotten one left the child");
        assert_eq!((f.copied.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), f.left_out_forgotten), (vec!["rm-1", "rm-4"], 2));
        let mut more = items();
        more.push(ParentItem { memory_id: "rm-5".into(), text: "learned to bake bread".into(), saved_utc_ms: Some(600) });
        let again = fork("f1", &p, "parent", &c, "child", &more, &live, &mut brain, 5_002).unwrap();
        assert_eq!((again.copied, again.left_out_forgotten, brain.index.len()), (f.copied, 2, 2), "answered from the done line");
    }

    #[test]
    fn a10_1_a_child_a_fork_stopped_in_is_no_one_elses() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain { fail_on: Some("has a cat named Miso".into()), ..FakeBrain::default() };
        assert!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).is_err());
        assert_eq!(fork("f2", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_001), Err(ForkError::ChildNotEmpty));
        assert_eq!(blank(&c, "child", 5_002), Err(ForkError::ChildNotEmpty));
        assert_eq!(brain.index.len(), 1, "only f1's first copy");
    }

    #[test]
    fn a10_1_an_instance_with_state_of_its_own_is_not_empty() {
        for file in ["governance.jsonl", "proposals.jsonl", "horizons.jsonl"] {
            let (p, c) = (dir(), dir());
            store::append(&c.join(file), &json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 1, "revoked": false})).unwrap();
            let mut brain = FakeBrain::default();
            assert_eq!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000), Err(ForkError::ChildNotEmpty), "{file}");
            assert_eq!(blank(&c, "child", 5_000), Err(ForkError::ChildNotEmpty), "{file}");
            assert!(brain.index.is_empty() && !c.join("self.json").exists() && log(&p).is_empty(), "{file}");
        }
    }

    #[test]
    fn a10_1_a_retry_answers_with_the_saved_child_self_and_the_parents_consent_still_folds() {
        let (p, c) = (dir(), dir());
        parent_with_a_chosen_name(&p);
        store::append(&p.join("governance.jsonl"), &json!({"channel": "notification", "copy_version": 1, "granted_utc_ms": 1, "revoked": false})).unwrap();
        let grants = |d: &Path| consent::fold(&store::read_lines(&d.join("governance.jsonl")).unwrap());
        let before = grants(&p);
        let f = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut FakeBrain::default(), 5_000).unwrap();
        assert_eq!((grants(&p), grants(&c).len()), (before.clone(), 0));
        assert_eq!(before.len(), 1);
        let mut v = serde_json::to_value(store::load(&p.join("self.json")).map(|l| match l { Loaded::Spec(s) => s, _ => panic!() }).unwrap()).unwrap();
        v["name"]["current"] = json!("Sol");
        store::save(&p.join("self.json"), &SelfSpec::parse(&v.to_string()).unwrap()).unwrap();
        std::fs::remove_file(p.join("governance.jsonl")).unwrap();
        let again = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut FakeBrain::default(), 5_001).unwrap();
        assert_eq!(again.spec.name.current.as_str(), "Lua");
        assert_eq!(again.spec, f.spec);
        assert_eq!(log(&p)[0]["copied"].as_u64(), Some(3), "the parent's counts come from the child's done line");
    }

    #[test]
    fn a10_1_a_blank_stopped_after_its_line_is_finished_once() {
        let c = dir();
        store::append(&c.join("governance.jsonl"), &json!({"kind": "blank", "utc_ms": 7})).unwrap();
        assert_eq!(blank(&c, "child", 8).unwrap(), SelfSpec::release_default("child"));
        assert_eq!(log(&c), vec![json!({"kind": "blank", "utc_ms": 7})]);
        assert_eq!(blank(&c, "child", 9), Err(ForkError::ChildNotEmpty));
    }

    #[test]
    fn a10_2_a_planned_memory_gone_from_the_list_leaves_the_child_and_a_new_one_waits() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain { fail_on: Some("plays the cello".into()), ..FakeBrain::default() };
        assert!(fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).is_err());
        assert!(brain.index.iter().any(|l| l.2 == "has a cat named Miso"), "attempt 1 copied rm-3");
        let mut later: Vec<ParentItem> = items().into_iter().filter(|i| i.memory_id != "rm-3").collect();
        later.push(ParentItem { memory_id: "rm-5".into(), text: "learned to bake bread".into(), saved_utc_ms: Some(600) });
        let f = fork("f1", &p, "parent", &c, "child", &later, &live, &mut brain, 5_001).unwrap();
        let texts: Vec<&str> = brain.index.iter().map(|l| l.2.as_str()).collect();
        assert_eq!(texts, vec!["likes green tea", "plays the cello"]);
        assert_eq!((f.copied.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), f.left_out_forgotten), (vec!["rm-1", "rm-4"], 2));
        assert_eq!(log(&c)[0]["planned"], json!(["rm-1", "rm-2", "rm-3", "rm-4"]), "ids only, frozen at the first attempt");
    }

    #[test]
    fn a10_2_a_fork_past_its_done_line_finishes_whatever_the_child_did_since() {
        let (p, c) = (dir(), dir());
        let mut brain = FakeBrain::default();
        let f = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_000).unwrap();

        std::fs::remove_file(c.join("self.json")).unwrap();
        std::fs::remove_file(p.join("governance.jsonl")).unwrap();
        store::append(&c.join("proposals.jsonl"), &json!({"id": "p-1"})).unwrap();
        let again = fork("f1", &p, "parent", &c, "child", &items(), &live, &mut brain, 5_001).unwrap();
        assert_eq!((again.spec, again.copied, brain.index.len()), (f.spec, f.copied, 3));
        assert!(c.join("self.json").exists());
        assert_eq!(log(&p).len(), 1, "the parent's line is written");
    }
}
