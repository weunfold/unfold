

use super::*;
use rth_memory::contract::{PERSON_KIND, Provider};
use rth_memory_brain::{Binding, Brain};
use rth_protocol::OwnerGrantMethod;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

const FAKE: &str = include_str!("../../../rth-memory-brain/tests/fake_brain_mcp.py");

#[derive(Clone, Copy)]
enum Kind {
    Reference,
    Brain,
}

struct Rig {
    dir: PathBuf,
    kind: Kind,
    binding: Option<Binding>,
}

impl Rig {
    fn new(name: &str, kind: Kind) -> Rig {
        let tag = match kind {
            Kind::Reference => "ref",
            Kind::Brain => "brain",
        };
        let dir = std::env::temp_dir().join(format!("agentd-pv05-people-{name}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        let binding = matches!(kind, Kind::Brain).then(|| {
            let script = dir.join("brain-mcp.py");
            std::fs::write(&script, FAKE).unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
            let b = Binding { instance_id: "inst".into(), brain_id: "brn-fake".into(), binding_rev: 1, mcp_bin: script, principal: "{}".into(), runtime_dir: None, index: dir.join("data/memory-binding.jsonl"), read_timeout: Duration::from_secs(30), write_timeout: Duration::from_secs(5), no_network: false, record: dir.join("data/memory-binding-record.json"), home: Some(dir.join("brain-home")), database: Some("fake-only".into()), seam: None };
            Brain::birth(&b, "fictional-nonce").unwrap();
            b
        });
        Rig { dir, kind, binding }
    }

    fn data(&self) -> PathBuf {
        self.dir.join("data")
    }

    fn boot(&self, boot: &str) -> (Memory, SelfStore) {
        let p: Box<dyn Provider> = match self.kind {
            Kind::Reference => Box::new(rth_memory::ledger::Reference { ledger: rth_memory::ledger::Ledger::open(&self.data().join("ref.jsonl"), "inst", rth_memory::ledger::PRIVATE_SCOPE, boot).unwrap().0 }),
            Kind::Brain => Box::new(Brain::open(self.binding.clone().unwrap()).unwrap()),
        };
        let m = Memory::open(p, &self.data().join("memory-acts.jsonl"), "inst", boot, "fake").unwrap().0;
        let s = SelfStore::open(&self.data(), "inst", 8192).unwrap();
        (m, s)
    }

    fn governance(&self) -> Vec<serde_json::Value> {
        self.governance_lines().into_iter().filter(|g| g["state"] == "done").collect()
    }

    fn governance_lines(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.data().join("governance.jsonl")).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn states(&self) -> Vec<String> {
        self.governance_lines().iter().map(|g| format!("{}:{}", g["kind"].as_str().unwrap(), g["state"].as_str().unwrap())).collect()
    }
}

fn grant() -> Option<OwnerGrant> {
    Some(OwnerGrant { grant_id: "g-1".into(), method: OwnerGrantMethod::TouchIdOrPassword, at_utc_ms: 1 })
}

fn enrol(frame_id: &str, person_id: &str, name: &str, owner: bool, auth: Option<OwnerGrant>) -> Event {
    Event::PeopleEnrol { frame_id: frame_id.into(), person_id: person_id.into(), name: name.into(), relation: if owner { "self".into() } else { "sister".into() }, owner, modality: PeopleModality::Face, consent_copy_version: 2, auth }
}

fn refused(e: Option<Event>) -> String {
    match e {
        Some(Event::PeopleRefused { reason, .. }) => reason,
        other => panic!("expected people_refused, got {other:?}"),
    }
}

fn acked(e: Option<Event>) -> (String, u64) {
    match e {
        Some(Event::PeopleAck { person_id, record_rev, .. }) => (person_id, record_rev),
        other => panic!("expected people_ack, got {other:?}"),
    }
}

fn listed(m: &Memory) -> (Vec<PersonRow>, Vec<ForgottenRow>) {
    match list(m) {
        Event::PeopleList { people, forgotten, prompts } => {
            assert!(prompts.is_empty());
            (people, forgotten)
        }
        other => panic!("{other:?}"),
    }
}

fn both(test: fn(Kind)) {
    test(Kind::Reference);
    test(Kind::Brain);
}

fn owner_gate(kind: Kind) {
    let rig = Rig::new("gate", kind);
    let (m, s) = rig.boot("b1");
    assert_eq!(refused(handle(&enrol("f1", "p-owner", "Rui", true, None), &m, &s, 10)), "needs_owner_grant", "an owner-only frame without auth");
    assert!(records(&m).is_empty() && rig.governance().is_empty(), "nothing changed");

    assert_eq!(acked(handle(&enrol("f2", "p-owner", "Rui", true, grant()), &m, &s, 11)), ("p-owner".to_string(), 1));
    let upd = Event::PeopleUpdate { frame_id: "f3".into(), person_id: "p-owner".into(), name: Some("Rui M.".into()), relation: None, owner: None, recognize: None, auth: None };
    assert_eq!(refused(handle(&upd, &m, &s, 12)), "needs_owner_grant");
    let forget = Event::PeopleForget { frame_id: "f4".into(), person_id: "p-owner".into(), modality: None, auth: None };
    assert_eq!(refused(handle(&forget, &m, &s, 13)), "needs_owner_grant");
    assert_eq!(records(&m)[0].rev, 1, "still revision 1");
}

#[test]
fn an_owner_only_frame_without_auth_is_refused_and_a_first_login_enrol_with_auth_is_accepted() {
    both(owner_gate);
}

fn recognition_off_without_grant(kind: Kind) {
    let rig = Rig::new("off-no-grant", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-lia", "Lia", false, grant()), &m, &s, 20));
    let upd = |frame: &str, name: Option<&str>, owner: Option<bool>, recognize: bool| Event::PeopleUpdate { frame_id: frame.into(), person_id: "p-lia".into(), name: name.map(Into::into), relation: None, owner, recognize: Some(recognize), auth: None };

    assert_eq!(refused(handle(&upd("f2", Some("Lia M."), None, false), &m, &s, 21)), "needs_owner_grant", "off plus a rename");
    assert_eq!(refused(handle(&upd("f3", None, Some(false), false), &m, &s, 22)), "needs_owner_grant", "off plus an owner field");
    assert_eq!(records(&m)[0].rev, 1, "still revision 1");

    assert_eq!(acked(handle(&upd("f4", None, None, false), &m, &s, 23)).1, 2);
    assert!(!body_of(&m, "p-lia").recognize);
    assert_eq!(kinds(&rig), ["enrol:face", "revoke:-"]);

    assert_eq!(refused(handle(&upd("f5", None, None, true), &m, &s, 24)), "needs_owner_grant");
    assert!(!body_of(&m, "p-lia").recognize, "still off");
}

#[test]
fn turning_recognition_off_alone_needs_no_owner_grant_and_turning_it_on_does() {
    both(recognition_off_without_grant);
}

fn copy_version_2_only(kind: Kind) {
    let rig = Rig::new("copy-v2", kind);
    let (m, s) = rig.boot("b1");
    let at = |frame: &str, v| Event::PeopleEnrol { frame_id: frame.into(), person_id: "p-rui".into(), name: "Rui".into(), relation: "self".into(), owner: true, modality: PeopleModality::ReadAlong, consent_copy_version: v, auth: grant() };

    assert_eq!(refused(handle(&at("f1", 1), &m, &s, 30)), "unknown_copy_version");
    assert!(records(&m).is_empty() && rig.governance().is_empty(), "nothing changed");
    assert_eq!(acked(handle(&at("f2", 2), &m, &s, 31)), ("p-rui".to_string(), 1));
    assert_eq!(rig.governance().last().unwrap()["copy_version"], 2);
}

#[test]
fn people_consent_is_copy_version_2_only() {
    both(copy_version_2_only);
}

fn enrol_reads_as_person(kind: Kind) {
    let rig = Rig::new("enrol", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-ana", "Ana", false, grant()), &m, &s, 20));
    let ids = m.person_items();
    assert_eq!(ids.len(), 1);
    let f = m.person_record(&ids[0]).unwrap();
    assert_eq!(f.kind.as_deref(), Some(PERSON_KIND), "the item reads back as rth_kind: person");
    let body: Body = serde_json::from_str(&f.text).unwrap();
    assert_eq!((body.person_id.as_str(), body.name.as_str(), body.owner, body.recognize, body.modalities.as_slice()), ("p-ana", "Ana", false, true, &[PeopleModality::Face][..]));
    if matches!(kind, Kind::Brain) {
        let calls = std::fs::read_to_string(rig.dir.join("brain-home/fake-brain-calls.jsonl")).unwrap();
        let w = calls.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()).find(|c| c["tool"] == "brain_remember" && c["arguments"]["content"].as_str().is_some_and(|t| t.contains("p-ana"))).unwrap();
        assert_eq!(w["arguments"]["metadata"]["rth_kind"], "person");
    }

    let tr = m.recall_for("Ana sister", "test");
    assert!(tr.items.is_empty(), "never recalled: {:?}", tr.items);
    assert!(m.recent_on_this_mac(10).unwrap().is_empty());
    assert_eq!(refused(handle(&enrol("f2", "p-ana", "Ana", false, grant()), &m, &s, 21)), "already_enrolled");
    let gov = rig.governance();
    assert_eq!((gov.len(), gov[0]["kind"].as_str(), gov[0]["channel"].as_str(), gov[0]["modality"].as_str()), (1, Some("enrol"), Some("app"), Some("face")));
    assert!(s.consents().unwrap().is_empty(), "a People entry is never an outward consent");
    drop(m);

    let (m, _) = rig.boot("b2");
    let (people, forgotten) = listed(&m);
    assert_eq!((people.len(), people[0].name.as_str(), people[0].recognize, forgotten.len()), (1, "Ana", true, 0));
}

#[test]
fn people_enrol_creates_a_person_item_outside_recall() {
    both(enrol_reads_as_person);
}

fn forget_and_undo(kind: Kind) {
    let rig = Rig::new("forget", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-bia", "Bia", false, grant()), &m, &s, 30));
    let forget = Event::PeopleForget { frame_id: "f2".into(), person_id: "p-bia".into(), modality: None, auth: grant() };
    let (_, rev) = acked(handle(&forget, &m, &s, 31));
    assert_eq!(rev, 2, "the forget is the record's next revision");
    let (people, forgotten) = listed(&m);
    assert!(people.is_empty());
    assert_eq!((forgotten.len(), forgotten[0].person_id.as_str(), forgotten[0].name.as_str()), (1, "p-bia", "Bia"));
    let gov = rig.governance();
    assert_eq!((gov.last().unwrap()["kind"].as_str(), gov.last().unwrap()["person_id"].as_str()), (Some("revoke"), Some("p-bia")), "revoke is in the consent log");
    assert_eq!(refused(handle(&Event::PeopleUpdate { frame_id: "f3".into(), person_id: "p-bia".into(), name: Some("B".into()), relation: None, owner: None, recognize: None, auth: grant() }, &m, &s, 32)), "unknown_person");
    drop(m);
    let (m, s) = rig.boot("b2");
    assert_eq!(listed(&m).1.len(), 1, "still forgotten after a restart");
    let (_, rev) = acked(handle(&Event::PeopleUndo { frame_id: "f4".into(), person_id: "p-bia".into(), auth: None }, &m, &s, 33));
    assert_eq!(rev, 4, "Undo restores the record (revision 3), then clears its modalities (4)");
    let (people, forgotten) = listed(&m);
    assert_eq!((people.len(), people[0].name.as_str(), forgotten.len()), (1, "Bia", 0));
    assert_eq!(m.person_record(&m.person_items()[0]).unwrap().kind.as_deref(), Some(PERSON_KIND));
    assert_eq!(refused(handle(&Event::PeopleUndo { frame_id: "f5".into(), person_id: "p-bia".into(), auth: None }, &m, &s, 34)), "already_enrolled");
}

#[test]
fn people_forget_then_people_undo_restores_the_record_and_revoke_is_logged() {
    both(forget_and_undo);
}

fn update_and_refusals(kind: Kind) {
    let rig = Rig::new("update", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-caio", "Caio", false, grant()), &m, &s, 40));
    let off = Event::PeopleUpdate { frame_id: "f2".into(), person_id: "p-caio".into(), name: Some("Caio R.".into()), relation: None, owner: None, recognize: Some(false), auth: grant() };
    assert_eq!(acked(handle(&off, &m, &s, 41)).1, 2);
    let (people, _) = listed(&m);
    assert_eq!((people[0].name.as_str(), people[0].recognize), ("Caio R.", false));
    assert_eq!(rig.governance().last().unwrap()["kind"], "revoke", "recognition off revokes its consent");
    assert_eq!(acked(handle(&off, &m, &s, 42)).1, 2, "an update that changes nothing writes nothing");
    let long = Event::PeopleUpdate { frame_id: "f3".into(), person_id: "p-caio".into(), name: Some("x".repeat(1200)), relation: None, owner: None, recognize: None, auth: grant() };
    assert_eq!(refused(handle(&long, &m, &s, 43)), "body_too_large");
    assert_eq!(refused(handle(&Event::PeopleForget { frame_id: "f4".into(), person_id: "p-nobody".into(), modality: None, auth: grant() }, &m, &s, 44)), "unknown_person");
    let copy1 = Event::PeopleEnrol { frame_id: "f5".into(), person_id: "p-dani".into(), name: "Dani".into(), relation: "friend".into(), owner: false, modality: PeopleModality::ReadAlong, consent_copy_version: 1, auth: grant() };
    assert_eq!(refused(handle(&copy1, &m, &s, 45)), "unknown_copy_version");

    let id = m.person_items()[0].clone();
    let (rec, _) = m.typed(crate::memory::Typed::Correct { memory_id: id.clone(), revision: 2, text: "Caio is a stranger.".into() }, "cmd-1", &crate::people::Who::keyboard()).unwrap();
    assert_eq!(rec.acts.last().and_then(|l| l.outcome.as_ref()).and_then(|o| o["reason"].as_str()), Some("person_item_needs_people_frame"));
    assert!(matches!(m.forget(&id, 2, "e1"), Err(ForgetError::Failed(r)) if r == "person_item_needs_people_frame"));
    acked(handle(&Event::PeopleForget { frame_id: "f6".into(), person_id: "p-caio".into(), modality: None, auth: grant() }, &m, &s, 46));
    assert!(matches!(m.restore(&id, "f6", "e2"), Err(ForgetError::Failed(r)) if r == "person_item_needs_people_frame"), "the memory card's Undo cannot restore a person");

    let _ = m.typed(crate::memory::Typed::Remember("The fictional café is Lua.".into()), "cmd-2", &crate::people::Who::keyboard()).unwrap();
    let ordinary = m.recent_on_this_mac(5).unwrap()[0].memory_id.clone();
    let (outcome, _) = m.people_write(Op::Correct { memory_id: ordinary, expected_revision: 1, text: "{}".into() }, "f7").unwrap();
    assert_eq!(outcome, Err("unknown_person".to_string()));
}

#[test]
fn people_update_forget_and_the_memory_paths_refuse_what_they_must() {
    both(update_and_refusals);
}

fn body_of(m: &Memory, person_id: &str) -> Body {
    let r = records(m).into_iter().find(|r| r.person_id == person_id && r.forgotten_at_utc_ms.is_none()).unwrap();
    Body { person_id: r.person_id, name: r.name, relation: r.relation, owner: r.owner, recognize: r.recognize, modalities: r.modalities, owner_since: r.owner_since }
}

fn kinds(rig: &Rig) -> Vec<String> {
    rig.governance().iter().map(|g| format!("{}:{}", g["kind"].as_str().unwrap(), g["modality"].as_str().unwrap_or("-"))).collect()
}

fn modalities_come_back(kind: Kind) {
    let rig = Rig::new("modalities", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-ana", "Ana", false, grant()), &m, &s, 50));
    let read_along = Event::PeopleEnrol { frame_id: "f2".into(), person_id: "p-ana".into(), name: "Ana".into(), relation: "sister".into(), owner: false, modality: PeopleModality::ReadAlong, consent_copy_version: 2, auth: grant() };
    acked(handle(&read_along, &m, &s, 51));
    assert_eq!(body_of(&m, "p-ana").modalities, vec![PeopleModality::Face, PeopleModality::ReadAlong], "a second modality is added");
    acked(handle(&Event::PeopleForget { frame_id: "f3".into(), person_id: "p-ana".into(), modality: Some(PeopleModality::Face), auth: grant() }, &m, &s, 52));
    assert_eq!(body_of(&m, "p-ana").modalities, vec![PeopleModality::ReadAlong]);
    acked(handle(&enrol("f4", "p-ana", "Ana", false, grant()), &m, &s, 53));
    assert_eq!(body_of(&m, "p-ana").modalities, vec![PeopleModality::ReadAlong, PeopleModality::Face], "the face is set up again");

    acked(handle(&Event::PeopleForget { frame_id: "f5".into(), person_id: "p-ana".into(), modality: None, auth: grant() }, &m, &s, 54));
    acked(handle(&Event::PeopleUndo { frame_id: "f6".into(), person_id: "p-ana".into(), auth: None }, &m, &s, 55));
    assert!(body_of(&m, "p-ana").modalities.is_empty(), "Undo restores no modality");
    acked(handle(&enrol("f7", "p-ana", "Ana", false, grant()), &m, &s, 56));
    assert_eq!(body_of(&m, "p-ana").modalities, vec![PeopleModality::Face]);

    let rec = |on: bool, f: &str| Event::PeopleUpdate { frame_id: f.into(), person_id: "p-ana".into(), name: None, relation: None, owner: None, recognize: Some(on), auth: grant() };
    acked(handle(&rec(false, "f8"), &m, &s, 57));
    acked(handle(&rec(true, "f9"), &m, &s, 58));
    assert_eq!(kinds(&rig), ["enrol:face", "enrol:read_along", "revoke:face", "enrol:face", "revoke:-", "enrol:face", "revoke:-", "enrol:-"]);

    acked(handle(&Event::PeopleForget { frame_id: "f10".into(), person_id: "p-ana".into(), modality: None, auth: grant() }, &m, &s, 59));
    acked(handle(&enrol("f11", "p-ana", "Ana", false, grant()), &m, &s, 60));
    let (people, forgotten) = listed(&m);
    assert_eq!((people.len(), forgotten.len(), m.person_items().len()), (1, 0, 1), "the same record, not a second one");
    assert_eq!(kinds(&rig).last().map(String::as_str), Some("enrol:face"));
}

#[test]
fn a_modality_can_be_added_back_and_undo_restores_no_templates() {
    both(modalities_come_back);
}

fn resends(kind: Kind) {
    let rig = Rig::new("resend", kind);
    let (m, s) = rig.boot("b1");
    let e = enrol("f1", "p-rui", "Rui", true, grant());
    let first = handle(&e, &m, &s, 70);
    let j = |e: &Option<Event>| serde_json::to_string(e).unwrap();
    assert_eq!(j(&handle(&e, &m, &s, 71)), j(&first), "an identical resend gets the first answer");
    let f = Event::PeopleForget { frame_id: "f2".into(), person_id: "p-rui".into(), modality: None, auth: grant() };
    let first = handle(&f, &m, &s, 72);
    acked(first.clone());
    drop(m);
    let (m, s) = rig.boot("b2");
    assert_eq!(j(&handle(&f, &m, &s, 73)), j(&first), "after a restart too");
    assert_eq!(kinds(&rig), ["enrol:face", "revoke:-"], "a resend writes no second consent entry");
    assert_eq!(refused(handle(&enrol("f2", "p-rui", "Rui", true, grant()), &m, &s, 74)), "frame_id_reused");
}

#[test]
fn an_identical_resend_is_answered_as_the_first_time() {
    both(resends);
}

fn one_owner(kind: Kind) {
    let rig = Rig::new("owner", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-rui", "Rui", true, grant()), &m, &s, 80));
    assert_eq!(refused(handle(&enrol("f2", "p-eva", "Eva", true, grant()), &m, &s, 81)), "owner_exists");
    acked(handle(&enrol("f3", "p-eva", "Eva", false, grant()), &m, &s, 82));
    let owner = |f: &str, person: &str, on: bool| Event::PeopleUpdate { frame_id: f.into(), person_id: person.into(), name: None, relation: None, owner: Some(on), recognize: None, auth: grant() };
    assert_eq!(refused(handle(&owner("f4", "p-eva", true), &m, &s, 83)), "owner_exists");

    acked(handle(&owner("f5", "p-rui", false), &m, &s, 84));
    acked(handle(&owner("f6", "p-eva", true), &m, &s, 85));
    assert_eq!(listed(&m).0.iter().filter(|p| p.owner).map(|p| p.person_id.as_str()).collect::<Vec<_>>(), ["p-eva"]);
}

#[test]
fn at_most_one_live_owner() {
    both(one_owner);
}

#[test]
fn provider_refusals_map_onto_the_documented_reasons() {
    for (from, to) in [("stale_revision", "stale"), ("text_too_long", "body_too_large"), ("unknown", "unknown_outcome"), ("duplicate_of", "duplicate_of"), ("ledger_full", "ledger_full"), ("rejected", "rejected"), ("unavailable", "unavailable"), ("a free-text detail: Ana", "rejected"), ("needs_owner_grant", "needs_owner_grant")] {
        assert_eq!(wire_reason(from), to, "{from}");
    }
}

#[test]
fn a_people_frame_is_journaled_without_the_name_relation_or_grant() {
    let mut env = rth_protocol::Stamper::new(rth_protocol::Source::Host, "app", Box::new(rth_protocol::MonotonicClock::start())).stamp(enrol("f1", "p-ana", "Ana", false, grant()), None);
    let original = crate::redact_people(&mut env).expect("a People frame");
    let line = env.to_line();
    assert!(!line.contains("Ana") && !line.contains("sister") && !line.contains("g-1") && !line.contains("p-ana"), "{line}");
    assert!(line.contains("f1"), "the frame id stays");
    assert_eq!(serde_json::to_string(&original).unwrap(), serde_json::to_string(&enrol("f1", "p-ana", "Ana", false, grant())).unwrap(), "the handler gets the frame as sent");
}

fn host_env(event: Event) -> rth_protocol::Envelope {
    rth_protocol::Stamper::new(rth_protocol::Source::Host, "app", Box::new(rth_protocol::MonotonicClock::start())).stamp(event, None)
}

#[test]
fn a_turn_speaker_and_presence_are_journaled_without_person_ids() {
    use rth_protocol::TurnSpeaker;
    let ana = || TurnSpeaker::Person { person_id: "p-ana".into() };
    for sent in [
        Event::UserText { request_id: "q1".into(), text: "oi".into(), speaker: ana(), reply_voice: None, effort: None, reply_max_tokens: None },
        Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "oi".into(), speaker: ana(), reply_voice: None, effort: None, reply_max_tokens: None },
        Event::UserAudioEnd { stream_id: "mic-1".into(), chunks: 4, samples: 2560, dropped_chunks: 0, speaker: ana() },
        Event::PeoplePresence { present: vec!["p-ana".into(), "p-bia".into()], unknown_faces: 1, at_utc_ms: 7, source: None },
    ] {
        let mut env = host_env(sent.clone());
        let original = crate::redact_people(&mut env).expect("a frame that names a person");
        let line = env.to_line();
        assert!(!line.contains("p-ana") && !line.contains("p-bia"), "{line}");
        assert_eq!(serde_json::to_string(&original).unwrap(), serde_json::to_string(&sent).unwrap(), "the handler gets the frame as sent");
    }
    let mut env = host_env(Event::PeoplePresence { present: vec!["p-ana".into(), "p-bia".into()], unknown_faces: 1, at_utc_ms: 7, source: None });
    crate::redact_people(&mut env);
    let Event::PeoplePresence { present, unknown_faces, .. } = &env.event else { unreachable!() };
    assert_eq!((present.len(), *unknown_faces), (2, 1), "the journal keeps how many were there");
}

#[test]
fn a_turn_that_names_no_one_is_journaled_as_sent() {
    use rth_protocol::TurnSpeaker;
    for speaker in [TurnSpeaker::OwnerKeyboard, TurnSpeaker::Unknown, TurnSpeaker::Uncertain] {
        let sent = Event::UserText { request_id: "q1".into(), text: "oi".into(), speaker, reply_voice: None, effort: None, reply_max_tokens: None };
        let mut env = host_env(sent.clone());
        assert!(crate::redact_people(&mut env).is_none());
        assert_eq!(serde_json::to_string(&env.event).unwrap(), serde_json::to_string(&sent).unwrap());
    }
}

fn undo_keeps_one_owner(kind: Kind) {
    let rig = Rig::new("undo-owner", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-rui", "Rui", true, grant()), &m, &s, 90));
    acked(handle(&Event::PeopleForget { frame_id: "f2".into(), person_id: "p-rui".into(), modality: None, auth: grant() }, &m, &s, 91));
    acked(handle(&enrol("f3", "p-lia", "Lia", true, grant()), &m, &s, 92));
    acked(handle(&Event::PeopleUndo { frame_id: "f4".into(), person_id: "p-rui".into(), auth: None }, &m, &s, 93));
    let (people, _) = listed(&m);
    assert_eq!(people.iter().filter(|p| p.owner).map(|p| p.person_id.as_str()).collect::<Vec<_>>(), ["p-lia"], "Undo, which needs no grant, never makes a second owner");
    assert!(people.iter().any(|p| p.person_id == "p-rui" && !p.owner));
}

#[test]
fn undo_never_makes_a_second_owner() {
    both(undo_keeps_one_owner);
}

#[derive(Clone, Copy, Debug)]
enum Crash {

    AfterPending,

    AfterRecord,

    AfterDone,
}

fn update_crash_keeps_its_consent(kind: Kind) {
    for crash in [Crash::AfterPending, Crash::AfterRecord, Crash::AfterDone] {
        let rig = Rig::new(&format!("update-crash-{crash:?}"), kind);
        let (m, s) = rig.boot("b1");
        acked(handle(&enrol("f1", "p-caio", "Caio", false, grant()), &m, &s, 100));
        let f2 = Event::PeopleUpdate { frame_id: "f2".into(), person_id: "p-caio".into(), name: None, relation: None, owner: None, recognize: Some(false), auth: grant() };
        let r = records(&m).pop().unwrap();
        let entry = |state| PeopleEntry { kind: PeopleKind::Revoke, person_id: "p-caio".into(), modality: None, model_id: None, copy_version: 2, at: 101, channel: PeopleChannel::App, frame_id: Some("f2".into()), state };
        let people = ["p-caio".to_string()];
        match crash {
            Crash::AfterDone => {
                acked(handle(&f2, &m, &s, 101));
                std::fs::remove_file(rig.data().join("people-answers.jsonl")).unwrap();
            }
            _ => {
                s.people_consent(&entry(PeopleState::Pending), &people).unwrap();
                if matches!(crash, Crash::AfterRecord) {
                    let off = Body { recognize: false, ..r.body() };
                    m.people_write(Op::Correct { memory_id: r.memory_id.clone(), expected_revision: r.rev, text: serde_json::to_string(&off).unwrap() }, "f2").unwrap().0.unwrap();
                }
            }
        }
        drop(m);
        let (m, s) = rig.boot("b2");
        assert_eq!(acked(handle(&f2, &m, &s, 102)).1, 2, "{crash:?}: the resend gets the one revision");
        assert_eq!(rig.states(), ["enrol:pending", "enrol:done", "revoke:pending", "revoke:done"], "{crash:?}: the entry is settled once");
        assert!(!body_of(&m, "p-caio").recognize);
    }
}

#[test]
fn a_crash_in_update_before_its_consent_still_writes_it_on_resend() {
    both(update_crash_keeps_its_consent);
}

#[test]
fn a_torn_consent_log_is_cut_at_open_so_a_forget_is_never_blocked() {
    let rig = Rig::new("torn", Kind::Reference);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-dani", "Dani", false, grant()), &m, &s, 110));
    let gov = rig.data().join("governance.jsonl");
    let mut torn = std::fs::read_to_string(&gov).unwrap();
    torn.push_str("{\"kind\":\"revo");
    std::fs::write(&gov, torn).unwrap();
    let off = Event::PeopleUpdate { frame_id: "f2".into(), person_id: "p-dani".into(), name: None, relation: None, owner: None, recognize: Some(false), auth: grant() };
    assert_eq!(refused(handle(&off, &m, &s, 111)), "unavailable", "transient until the restart");
    assert_eq!(std::fs::read_to_string(&gov).unwrap().matches("\"kind\":\"revoke\",").count(), 0, "no entry written over a torn log");
    drop((m, s));
    let (m, s) = rig.boot("b2");
    let forget = Event::PeopleForget { frame_id: "f3".into(), person_id: "p-dani".into(), modality: None, auth: grant() };
    acked(handle(&off, &m, &s, 112));
    acked(handle(&forget, &m, &s, 113));
    assert_eq!(kinds(&rig), ["enrol:face", "revoke:-", "revoke:-"], "both settle after the cut");
    assert_eq!(s.governance_torn(), 13, "the torn bytes are counted");
}

#[test]
fn a_torn_outward_consent_line_is_repaired_and_people_writes_work() {
    let rig = Rig::new("torn-outward", Kind::Reference);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-eva", "Eva", false, grant()), &m, &s, 150));
    assert!(s.consent_set("c1", "notification", None, true, 1, 151)["ok"].is_object());
    drop((m, s));
    let gov = rig.data().join("governance.jsonl");
    let torn = r#"{"channel":"notification","copy_version":1,"granted_utc_ms":152,"revo"#;
    let whole = std::fs::read_to_string(&gov).unwrap();
    std::fs::write(&gov, format!("{whole}{torn}")).unwrap();
    let (m, s) = rig.boot("b2");
    assert_eq!((s.governance_torn(), s.governance_outward_reset()), (torn.len() as u64, true));
    assert!(s.consents().unwrap().is_empty(), "the grant before the torn revoke is not back");
    let forget = Event::PeopleForget { frame_id: "f2".into(), person_id: "p-eva".into(), modality: None, auth: grant() };
    acked(handle(&forget, &m, &s, 153));
    assert_eq!(kinds(&rig), ["enrol:face", "revoke:-"], "the forget's consent is written");
}

#[test]
fn a_refused_frame_voids_its_pending_consent_entry() {
    let rig = Rig::new("void", Kind::Reference);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-ivo", "Ivo", false, grant()), &m, &s, 120));
    assert_eq!(refused(handle(&enrol("f2", "p-ivo", "Ivo", false, grant()), &m, &s, 121)), "already_enrolled");
    assert_eq!(rig.states(), ["enrol:pending", "enrol:done", "enrol:pending", "enrol:void"]);
}

fn grant_at(at_utc_ms: u64) -> Option<OwnerGrant> {
    Some(OwnerGrant { grant_id: format!("g-{at_utc_ms}"), method: OwnerGrantMethod::TouchIdOrPassword, at_utc_ms })
}

fn undo_crash_reconciles_to_one_owner(kind: Kind) {
    let rig = Rig::new("undo-crash-owner", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-rui", "Rui", true, grant_at(10)), &m, &s, 130));
    acked(handle(&Event::PeopleForget { frame_id: "f2".into(), person_id: "p-rui".into(), modality: None, auth: grant_at(11) }, &m, &s, 131));
    acked(handle(&enrol("f3", "p-lia", "Lia", true, grant_at(12)), &m, &s, 132));
    let rui = records(&m).into_iter().find(|r| r.person_id == "p-rui").unwrap();
    m.people_restore(&rui.memory_id, "f4").unwrap();
    let owners = |m: &Memory| { let (p, _) = listed(m); p.into_iter().filter(|r| r.owner).map(|r| r.person_id).collect::<Vec<_>>() };
    assert_eq!(owners(&m).len(), 2, "the crash window");
    drop((m, s));
    let (m, s) = rig.boot("b2");
    assert_eq!(reconcile_owners(&m), Ok(1), "one owner demoted");
    assert_eq!(owners(&m), ["p-lia"], "the most recent grant wins");
    assert_eq!(reconcile_owners(&m), Ok(0), "settled");
    acked(handle(&Event::PeopleUndo { frame_id: "f4".into(), person_id: "p-rui".into(), auth: None }, &m, &s, 133));
    assert_eq!(owners(&m), ["p-lia"], "the resent Undo agrees");
    drop((m, s));
    let (m, _s) = rig.boot("b3");
    assert_eq!((reconcile_owners(&m), owners(&m)), (Ok(0), vec!["p-lia".to_string()]), "and stays so across a restart");
}

#[test]
fn a_crash_in_undo_is_reconciled_to_one_owner_at_start() {
    both(undo_crash_reconciles_to_one_owner);
}

fn owner_since_is_the_receive_time(kind: Kind) {
    let rig = Rig::new("owner-since-receive", kind);
    let (m, s) = rig.boot("b1");
    acked(handle(&enrol("f1", "p-rui", "Rui", true, grant_at(90)), &m, &s, 140));
    assert_eq!(body_of(&m, "p-rui").owner_since, Some(140), "agentd's clock, not the grant's 90");
    acked(handle(&Event::PeopleForget { frame_id: "f2".into(), person_id: "p-rui".into(), modality: None, auth: grant_at(91) }, &m, &s, 141));
    acked(handle(&enrol("f3", "p-lia", "Lia", false, grant_at(50)), &m, &s, 142));
    let made_owner = Event::PeopleUpdate { frame_id: "f4".into(), person_id: "p-lia".into(), name: None, relation: None, owner: Some(true), recognize: None, auth: grant_at(12) };
    acked(handle(&made_owner, &m, &s, 143));
    assert_eq!(body_of(&m, "p-lia").owner_since, Some(143), "an update that makes the owner: agentd's clock too");
    let rui = records(&m).into_iter().find(|r| r.person_id == "p-rui").unwrap();
    m.people_restore(&rui.memory_id, "f5").unwrap();
    drop((m, s));
    let (m, _s) = rig.boot("b2");
    assert_eq!(reconcile_owners(&m), Ok(1));
    let (p, _) = listed(&m);
    assert_eq!(p.into_iter().filter(|r| r.owner).map(|r| r.person_id).collect::<Vec<_>>(), ["p-lia"], "received last, so the most recent owner");
}

#[test]
fn owner_since_is_agentds_receive_time_not_the_grants() {
    both(owner_since_is_the_receive_time);
}

fn owner_since_after_a_clock_step_back(kind: Kind) {
    for by_update in [false, true] {
        let rig = Rig::new(if by_update { "owner-since-step-back-update" } else { "owner-since-step-back-enrol" }, kind);
        let (m, s) = rig.boot("b1");
        acked(handle(&enrol("f1", "p-rui", "Rui", true, grant_at(1)), &m, &s, 500));
        acked(handle(&Event::PeopleForget { frame_id: "f2".into(), person_id: "p-rui".into(), modality: None, auth: grant_at(2) }, &m, &s, 501));
        if by_update {
            acked(handle(&enrol("f3", "p-lia", "Lia", false, grant_at(3)), &m, &s, 300));
            acked(handle(&Event::PeopleUpdate { frame_id: "f4".into(), person_id: "p-lia".into(), name: None, relation: None, owner: Some(true), recognize: None, auth: grant_at(4) }, &m, &s, 301));
        } else {
            acked(handle(&enrol("f3", "p-lia", "Lia", true, grant_at(3)), &m, &s, 300));
        }
        assert_eq!(body_of(&m, "p-lia").owner_since, Some(501), "one past Rui's 500, not the stepped-back clock");
        let rui = records(&m).into_iter().find(|r| r.person_id == "p-rui").unwrap();
        m.people_restore(&rui.memory_id, "f5").unwrap();
        drop((m, s));
        let (m, _s) = rig.boot("b2");
        assert_eq!(reconcile_owners(&m), Ok(1));
        let (p, _) = listed(&m);
        assert_eq!(p.into_iter().filter(|r| r.owner).map(|r| r.person_id).collect::<Vec<_>>(), ["p-lia"], "received last, so the owner kept");
    }
}

#[test]
fn owner_since_stays_ordered_when_the_clock_steps_back() {
    both(owner_since_after_a_clock_step_back);
}
