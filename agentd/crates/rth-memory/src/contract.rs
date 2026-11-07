

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Provenance {
    pub instance_id: String,
    pub provider_generation: u64,
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,

    pub command_id: String,

    #[serde(default)]
    pub origin: Origin,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Command,

    PeopleFrame,

    Conversation,
}

pub const PERSON_KIND: &str = "person";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Saved {
    pub memory_id: String,
    pub revision: u64,

    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Found {
    pub memory_id: String,
    pub revision: u64,
    pub text: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_utc_ms: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Recalled {
    pub items: Vec<Found>,

    pub total: usize,
    pub omitted: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranks: Vec<usize>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<Dropped>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Dropped {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brain_ref: Option<String>,

    pub kind: &'static str,
    pub reason: &'static str,
    pub rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "failure", rename_all = "snake_case")]
pub enum Failure {

    Rejected { reason: String, actual_revision: Option<u64>, memory_id: Option<String> },

    Unavailable { detail: String },

    Unknown { detail: String },
}

impl Failure {
    pub fn rejected(reason: &str) -> Failure {
        Failure::Rejected { reason: reason.into(), actual_revision: None, memory_id: None }
    }
}

pub const RECALL_BUDGET_MS: u64 = 800;

pub const WRITE_BUDGET_MS: u64 = 2000;

pub trait Provider: Send {
    fn name(&self) -> &'static str;
    fn remember(&mut self, operation_key: &str, text: &str, from: &Provenance) -> Result<Saved, Failure>;

    fn correct(&mut self, operation_key: &str, memory_id: &str, expected_revision: u64, text: &str, from: &Provenance) -> Result<Saved, Failure>;

    fn recall(&mut self, query: &str, limit: usize) -> Result<Recalled, Failure>;

    fn get(&mut self, memory_id: &str) -> Result<Found, Failure>;

    fn history(&mut self, memory_id: &str) -> Result<Vec<Found>, Failure>;

    fn recent(&mut self, _limit: usize) -> Result<Vec<Found>, Failure> {
        Err(Failure::Unavailable { detail: "this implementation cannot list its items".into() })
    }

    fn invalidate(&mut self, _operation_key: &str, _memory_id: &str, _revision: u64, _reason: &str) -> Result<Saved, Failure> {
        Err(Failure::Unavailable { detail: "this implementation cannot invalidate".into() })
    }

    fn restore(&mut self, _operation_key: &str, _memory_id: &str, _expected_revision: u64, _to_revision: u64) -> Result<Saved, Failure> {
        Err(Failure::Unavailable { detail: "this implementation cannot restore".into() })
    }

    fn retag(&mut self, _operation_key: &str, _memory_id: &str, _expected_revision: u64, _speaker: &str) -> Result<Saved, Failure> {
        Err(Failure::Unavailable { detail: "this implementation cannot retag".into() })
    }

    fn health(&mut self) -> Result<String, String> {
        Ok("ready".into())
    }

    fn close(&mut self) {}

    fn children(&self) -> Option<std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<u32>>>> {
        None
    }
}

pub mod conformance {
    use super::*;

    fn from(call: &str, command: &str) -> Provenance {
        Provenance { instance_id: "conformance".into(), provider_generation: 1, call_id: call.into(), response_id: None, command_id: command.into(), origin: Origin::Command, speaker: None }
    }

    pub fn run(open: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {
        let k = |s: &str| format!("{tag}-{s}");
        let first = format!("The conformance code word for {tag} is lantern-42.");
        let second = format!("The conformance code word for {tag} is harbor-17.");
        let mut p = open();

        let a = p.remember(&k("r1"), &first, &from("c1", "cmd1")).expect("remember");
        assert_eq!((a.revision, a.replayed), (1, false), "remember: a new item at revision 1");
        let got = p.get(&a.memory_id).expect("get after remember");
        assert_eq!((got.revision, got.text.as_str()), (1, first.as_str()), "read-back");

        let again = p.remember(&k("r1"), &first, &from("c1", "cmd1")).expect("retry");
        assert_eq!((again.memory_id.as_str(), again.revision, again.replayed), (a.memory_id.as_str(), 1, true), "the same operation key replays the same outcome");
        assert!(matches!(p.remember(&k("r1"), "something else", &from("c1", "cmd1")), Err(Failure::Rejected { .. })), "the same key with another request is refused");

        match p.remember(&k("r2"), &first, &from("c2", "cmd2")) {
            Err(Failure::Rejected { reason, memory_id, .. }) => assert_eq!((reason.as_str(), memory_id.as_deref()), ("duplicate_of", Some(a.memory_id.as_str()))),
            other => panic!("equal text: {other:?}"),
        }
        drop(p);

        let mut p = open();
        let r = p.recall(&format!("what is the conformance code word for {tag}"), 5).expect("recall after restart");
        let hit = r.items.iter().find(|i| i.memory_id == a.memory_id).expect("the remembered item is recalled after a restart");
        assert_eq!((hit.revision, hit.text.as_str()), (1, first.as_str()));

        let c = p.correct(&k("c1"), &a.memory_id, 1, &second, &from("c3", "cmd3")).expect("correct");
        assert_eq!((c.memory_id.as_str(), c.revision), (a.memory_id.as_str(), 2), "a correction is the next revision of the same item");
        match p.correct(&k("c2"), &a.memory_id, 1, "a stale overwrite", &from("c4", "cmd4")) {
            Err(Failure::Rejected { reason, actual_revision, .. }) => assert_eq!((reason.as_str(), actual_revision), ("stale_revision", Some(2))),
            other => panic!("stale correction: {other:?}"),
        }
        assert!(matches!(p.correct(&k("c3"), "999999", 1, "x", &from("c5", "cmd5")), Err(Failure::Rejected { reason, .. }) if reason == "unknown_target"));
        drop(p);

        let mut p = open();
        let got = p.get(&a.memory_id).expect("get after correction");
        assert_eq!((got.revision, got.text.as_str()), (2, second.as_str()));
        let h = p.history(&a.memory_id).expect("history");
        let texts: Vec<(u64, String)> = h.iter().map(|f| (f.revision, f.text.clone())).collect();
        assert_eq!(texts, vec![(2, second.clone()), (1, first.clone())], "history keeps every revision, newest first");
        let r = p.recall(&format!("conformance code word for {tag}"), 5).expect("recall after correction");
        let hit = r.items.iter().find(|i| i.memory_id == a.memory_id).expect("recalled");
        assert_eq!(hit.revision, 2, "recall shows the current revision");
    }

    pub fn run_invalidate(open: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {
        let k = |s: &str| format!("{tag}-{s}");
        let text = format!("The invalidation code word for {tag} is ember-9.");
        let mut p = open();
        let a = p.remember(&k("i1"), &text, &from("i1", "cmdi1")).expect("remember");
        assert!(matches!(p.invalidate(&k("i2"), &a.memory_id, 7, "test"), Err(Failure::Rejected { reason, actual_revision: Some(1), .. }) if reason == "stale_revision"), "invalidate compares and sets");
        let inv = p.invalidate(&k("i3"), &a.memory_id, 1, "test").expect("invalidate");
        assert_eq!((inv.memory_id.as_str(), inv.revision, inv.replayed), (a.memory_id.as_str(), 2, false), "an invalidation is the next revision");
        let again = p.invalidate(&k("i3"), &a.memory_id, 1, "test").expect("replay");
        assert_eq!((again.revision, again.replayed), (2, true), "the same key replays");
        assert!(matches!(p.invalidate(&k("i3"), &a.memory_id, 2, "test"), Err(Failure::Rejected { reason, .. }) if reason == "idempotency_conflict"));
        drop(p);

        let mut p = open();
        let r = p.recall(&format!("invalidation code word for {tag}"), 5).expect("recall");
        assert!(r.items.iter().all(|i| i.memory_id != a.memory_id), "an invalidated item is not recalled after a restart");
        assert!(matches!(p.get(&a.memory_id), Err(Failure::Rejected { reason, .. }) if reason == "invalidated"));
        assert_eq!(p.history(&a.memory_id).expect("history").last().map(|f| (f.revision, f.text.clone())), Some((1, text.clone())), "history keeps the first revision");
        assert!(matches!(p.restore(&k("r1"), &a.memory_id, 1, 1), Err(Failure::Rejected { reason, .. }) if reason == "stale_revision"));
        assert!(matches!(p.restore(&k("r2"), &a.memory_id, 2, 9), Err(Failure::Rejected { reason, .. }) if reason == "unknown_revision"));
        let back = p.restore(&k("r3"), &a.memory_id, 2, 1).expect("restore");
        assert_eq!((back.memory_id.as_str(), back.revision, back.replayed), (a.memory_id.as_str(), 3, false));
        assert!(p.restore(&k("r3"), &a.memory_id, 2, 1).expect("replay").replayed, "the same key replays");
        assert!(matches!(p.restore(&k("r4"), &a.memory_id, 3, 1), Err(Failure::Rejected { reason, .. }) if reason == "not_invalidated"));
        drop(p);

        let mut p = open();
        let got = p.get(&a.memory_id).expect("get after restore");
        assert_eq!((got.revision, got.text.as_str()), (3, text.as_str()), "restored under the same id with its text");
        let r = p.recall(&format!("invalidation code word for {tag}"), 5).expect("recall after restore");
        assert!(r.items.iter().any(|i| i.memory_id == a.memory_id && i.revision == 3), "recalled again after a restart");
        let revs: Vec<u64> = p.history(&a.memory_id).expect("history").iter().map(|f| f.revision).collect();
        assert_eq!(revs, vec![3, 2, 1], "nothing is rewritten: the invalidation and the restore are revisions");
    }

    pub fn run_person(open: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {
        let k = |s: &str| format!("{tag}-{s}");
        let ordinary = format!("The person test code word for {tag} is willow-31.");
        let person = format!("Person record for {tag}: Ana, sister, person test code word willow-31.");
        let query = format!("person test code word willow-31 {tag}");
        let mut p = open();
        let a = p.remember(&k("p1"), &ordinary, &from("p1", "cmdp1")).expect("remember ordinary");
        let frame = Provenance { origin: Origin::PeopleFrame, ..from("p2", "frame-p2") };
        let b = p.remember(&k("p2"), &person, &frame).expect("remember person");
        let check = |p: &mut Box<dyn Provider>, when: &str| {
            let r = p.recall(&query, 10).expect("recall");
            assert!(r.items.iter().any(|i| i.memory_id == a.memory_id), "{when}: the ordinary item is recalled");
            assert!(r.items.iter().all(|i| i.memory_id != b.memory_id), "{when}: a person item never reaches turn recall");
            assert!(r.dropped.iter().any(|d| d.memory_id.as_deref() == Some(b.memory_id.as_str()) && d.kind == "excluded" && d.reason == "person_record"), "{when}: the receipt names it: {:?}", r.dropped);
            assert!(p.recent(10).expect("recent").iter().all(|i| i.memory_id != b.memory_id), "{when}: nor the saved-memories list");
            assert_eq!(p.get(&b.memory_id).expect("get person").kind.as_deref(), Some(PERSON_KIND), "{when}: read back as a person item");
            assert_eq!(p.get(&a.memory_id).expect("get ordinary").kind, None, "{when}: an ordinary item has no kind");
        };
        check(&mut p, "written");
        drop(p);
        let mut p = open();
        check(&mut p, "after a restart");

        let renamed = format!("Person record for {tag}: Ana Maria, sister, person test code word willow-31.");
        assert_eq!(p.correct(&k("p5"), &b.memory_id, 1, &renamed, &frame).expect("people update").revision, 2);
        drop(p);
        let mut p = open();
        check(&mut p, "after an update and a restart");
        p.invalidate(&k("p3"), &b.memory_id, 2, "people forget").expect("forget");
        p.restore(&k("p4"), &b.memory_id, 3, 2).expect("undo");
        drop(p);
        let mut p = open();
        check(&mut p, "after forget, undo and a restart");
        assert_eq!(p.get(&b.memory_id).expect("get").text, renamed, "undo restores the updated record");
    }

    pub fn run_speaker(open: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {
        let k = |s: &str| format!("{tag}-{s}");
        let untagged = format!("The speaker test code word for {tag} is alder-52, said before people.");
        let theirs = format!("The speaker test code word for {tag} is alder-52, said by a known person.");
        let query = format!("speaker test code word alder-52 {tag}");
        let mut p = open();
        let a = p.remember(&k("s1"), &untagged, &from("s1", "cmds1")).expect("remember untagged");
        let said = Provenance { speaker: Some("person:p-fictional".into()), ..from("s2", "cmds2") };
        let b = p.remember(&k("s2"), &theirs, &said).expect("remember tagged");
        let check = |p: &mut Box<dyn Provider>, when: &str| {
            assert_eq!(p.get(&a.memory_id).expect("get untagged").speaker, None, "{when}: no tag reads back as none");
            assert_eq!(p.get(&b.memory_id).expect("get tagged").speaker.as_deref(), Some("person:p-fictional"), "{when}: get keeps the tag");
            let r = p.recall(&query, 10).expect("recall");
            let got = r.items.iter().find(|i| i.memory_id == b.memory_id).expect("recalled");
            assert_eq!(got.speaker.as_deref(), Some("person:p-fictional"), "{when}: recall keeps the tag");
            let recent = p.recent(10).expect("recent");
            assert_eq!(recent.iter().find(|i| i.memory_id == b.memory_id).and_then(|i| i.speaker.clone()).as_deref(), Some("person:p-fictional"), "{when}: recent keeps the tag");
        };
        check(&mut p, "written");
        drop(p);
        let mut p = open();
        check(&mut p, "after a restart");
        let changed = format!("The speaker test code word for {tag} is alder-53, said by a known person.");
        assert_eq!(p.correct(&k("s3"), &b.memory_id, 1, &changed, &said).expect("correct").revision, 2);
        drop(p);
        let mut p = open();
        assert_eq!(p.get(&b.memory_id).expect("get corrected").speaker.as_deref(), Some("person:p-fictional"), "a correction keeps the tag");
    }

    pub fn run_retag(open: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {

        let k = |s: &str| format!("{tag}-retag-{s}");
        let text = format!("The retag test code word for {tag} is birch-61.");
        let mut p = open();
        let a = p.remember(&k("r1"), &text, &from("r1", "cmdr1")).expect("remember untagged");
        let to = "person:p-fictional";
        let r = p.retag(&k("r2"), &a.memory_id, 1, to).expect("retag");
        assert_eq!((r.memory_id.as_str(), r.revision), (a.memory_id.as_str(), 2), "the same item, one revision on");
        let again = p.retag(&k("r2"), &a.memory_id, 1, to).expect("replay");
        assert_eq!((again.revision, again.replayed), (2, true), "the same key replays");
        match p.retag(&k("r3"), &a.memory_id, 1, "person:p-other") {
            Err(Failure::Rejected { reason, .. }) => assert_eq!(reason, "stale_revision"),
            other => panic!("a stale retag must be refused: {other:?}"),
        }
        drop(p);
        let mut p = open();
        let got = p.get(&a.memory_id).expect("get retagged");
        assert_eq!((got.text.as_str(), got.revision, got.speaker.as_deref()), (text.as_str(), 2, Some(to)), "after a restart");
        let back = p.retag(&k("r4"), &a.memory_id, 2, "keyboard").expect("retag back");
        assert_eq!(back.revision, 3);
        assert_eq!(p.get(&a.memory_id).expect("get").speaker.as_deref(), Some("keyboard"));
    }

    pub fn run_scope(open_a: &mut dyn FnMut() -> Box<dyn Provider>, open_b: &mut dyn FnMut() -> Box<dyn Provider>, tag: &str) {
        let mut a = open_a();
        let mut b = open_b();
        let text = format!("The scope union fact for {tag} belongs to instance B.");
        let saved = b.remember(&format!("{tag}-scope-b"), &text, &from("cb", "cmdb")).expect("remember in B");
        let rb = b.recall(&format!("scope union fact {tag}"), 5).expect("B recalls");
        assert!(rb.items.iter().any(|i| i.memory_id == saved.memory_id), "B sees its own item");
        let ra = a.recall(&format!("scope union fact {tag}"), 5).expect("A recalls");
        assert!(ra.items.iter().all(|i| !i.text.contains(&text)), "A never sees B's item");
    }
}
