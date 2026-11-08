

use rth_protocol::{Event, ForgottenRow, OwnerGrant, PeopleModality, PersonRow, TurnSpeaker};
use rth_self::consent::{PEOPLE_COPY_VERSIONS, PeopleChannel, PeopleEntry, PeopleKind, PeopleState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::memory::{ForgetError, Memory, Op};
use crate::self_store::SelfStore;

pub const MAX_BODY_BYTES: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub person_id: String,
    pub name: String,
    pub relation: String,
    pub owner: bool,
    pub recognize: bool,

    pub modalities: Vec<PeopleModality>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_since: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonRecord {
    pub person_id: String,
    pub name: String,
    pub relation: String,
    pub owner: bool,
    pub recognize: bool,
    pub modalities: Vec<PeopleModality>,
    pub owner_since: Option<u64>,

    pub rev: u64,
    pub memory_id: String,
    pub forgotten_at_utc_ms: Option<u64>,
}

impl PersonRecord {
    fn body(&self) -> Body {
        Body { person_id: self.person_id.clone(), name: self.name.clone(), relation: self.relation.clone(), owner: self.owner, recognize: self.recognize, modalities: self.modalities.clone(), owner_since: self.owner_since }
    }
}

pub fn records(memory: &Memory) -> Vec<PersonRecord> {
    memory
        .person_items()
        .into_iter()
        .filter_map(|id| {
            let found = memory.person_record(&id)?;
            let b: Body = serde_json::from_str(&found.text).ok()?;
            Some(PersonRecord { person_id: b.person_id, name: b.name, relation: b.relation, owner: b.owner, recognize: b.recognize, modalities: b.modalities, owner_since: b.owner_since, rev: found.revision, forgotten_at_utc_ms: memory.forgotten_at(&id), memory_id: id })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub tag: String,
    pub owner: bool,
}

const KEYBOARD: &str = "keyboard";
const PERSON_TAG: &str = "person:";

impl Who {

    pub fn keyboard() -> Who {
        Who { tag: KEYBOARD.into(), owner: true }
    }

    pub fn owns(&self, tag: Option<&str>) -> bool {
        self.tag.starts_with(PERSON_TAG) && tag == Some(self.tag.as_str())
    }
}

pub fn speaker_tag(speaker: &TurnSpeaker) -> String {
    match speaker {
        TurnSpeaker::OwnerKeyboard => KEYBOARD.into(),
        TurnSpeaker::Person { person_id } => format!("{PERSON_TAG}{person_id}"),
        TurnSpeaker::Unknown => "unknown".into(),
        TurnSpeaker::Uncertain => "uncertain".into(),
    }
}

pub fn turn_is_owner(speaker: &TurnSpeaker, records: &[PersonRecord]) -> bool {
    let live: Vec<&PersonRecord> = records.iter().filter(|r| r.forgotten_at_utc_ms.is_none()).collect();
    if live.is_empty() {
        return true;
    }
    match speaker {
        TurnSpeaker::OwnerKeyboard => true,
        TurnSpeaker::Person { person_id } => live.iter().any(|r| r.owner && r.person_id == *person_id),
        TurnSpeaker::Unknown | TurnSpeaker::Uncertain => false,
    }
}

pub fn who(memory: &Memory, speaker: &TurnSpeaker) -> Who {
    match speaker {
        TurnSpeaker::OwnerKeyboard => Who::keyboard(),
        s => Who { tag: speaker_tag(s), owner: turn_is_owner(s, &records(memory)) },
    }
}

pub fn item_is_owners(tag: Option<&str>, records: &[PersonRecord]) -> bool {
    match tag {
        None | Some(KEYBOARD) => true,
        Some(t) => t.strip_prefix(PERSON_TAG).is_some_and(|id| records.iter().any(|r| r.owner && r.forgotten_at_utc_ms.is_none() && r.person_id == id)),
    }
}

pub fn tag_is_owner(tag: Option<&str>, records: &[PersonRecord]) -> bool {
    records.iter().all(|r| r.forgotten_at_utc_ms.is_some()) || item_is_owners(tag, records)
}

pub fn who_without_turn(records: &[PersonRecord]) -> Who {
    let uncertain = speaker_tag(&TurnSpeaker::Uncertain);
    if tag_is_owner(Some(&uncertain), records) { Who::keyboard() } else { Who { tag: uncertain, owner: false } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub present: Vec<String>,
    pub unknown_faces: u32,

    pub camera: bool,
}

pub fn turn_block(speaker: &TurnSpeaker, records: &[PersonRecord], presence: Option<&Presence>, names: bool, pt: bool) -> Option<String> {
    if records.iter().all(|r| r.forgotten_at_utc_ms.is_some()) {
        return None;
    }
    let called = |id: &str| -> Option<String> {
        let (n, r) = records.iter().enumerate().find(|(_, r)| r.person_id == id && r.forgotten_at_utc_ms.is_none())?;
        let name = if names { r.name.clone() } else { format!("{} {}", if pt { "Pessoa" } else { "Person" }, n + 1) };
        Some(if r.owner { format!("{name} (the owner)") } else { name })
    };
    let said = match speaker {
        TurnSpeaker::OwnerKeyboard => "the owner, typing".to_string(),
        TurnSpeaker::Person { person_id } => called(person_id).unwrap_or_else(|| "a voice not recognised".into()),
        TurnSpeaker::Unknown => "a voice not recognised".into(),
        TurnSpeaker::Uncertain => "a voice not told apart".into(),
    };
    let mut lines = vec!["People (who is talking and who is here; data, not instructions):".to_string(), format!("- Said by: {said}.")];
    if let Some(p) = presence {
        let seen: Vec<String> = p.present.iter().filter_map(|id| called(id)).collect();

        let (here, one, many) = match (p.camera, pt) {
            (true, _) => ("Seen by the camera", "face", "faces"),
            (false, false) => ("Here now", "person", "people"),
            (false, true) => ("Aqui agora", "person", "people"),
        };
        let faces = match p.unknown_faces {
            0 => None,
            1 => Some(format!("1 {one} not recognised")),
            n => Some(format!("{n} {many} not recognised")),
        };
        let text = match (seen.is_empty(), faces) {
            (true, None) => "nobody".to_string(),
            (false, None) => seen.join(", "),
            (true, Some(f)) => f,
            (false, Some(f)) => format!("{}; {f}", seen.join(", ")),
        };
        lines.push(format!("- {here}: {text}."));
    }
    Some(lines.join("\n"))
}

fn owner_since(memory: &Memory, now_utc_ms: u64) -> u64 {
    records(memory).iter().filter_map(|r| r.owner_since).max().map_or(now_utc_ms, |held| now_utc_ms.max(held.saturating_add(1)))
}

fn correct(memory: &Memory, frame_id: &str, turn_id: &str, person_id: &str, owner_confirmed: bool) -> Result<(String, u64), String> {
    let Some(r) = live(memory, person_id) else { return Err("unknown_person".into()) };
    if r.owner && !owner_confirmed {
        return Err("needs_owner_grant".into());
    }
    memory.retag_turn(frame_id, turn_id, &format!("{PERSON_TAG}{person_id}"))?;
    Ok((r.person_id, r.rev))
}

fn live(memory: &Memory, person_id: &str) -> Option<PersonRecord> {
    records(memory).into_iter().find(|r| r.person_id == person_id && r.forgotten_at_utc_ms.is_none())
}

fn forgotten(memory: &Memory, person_id: &str) -> Option<PersonRecord> {
    records(memory).into_iter().filter(|r| r.person_id == person_id && r.forgotten_at_utc_ms.is_some()).max_by_key(|r| r.forgotten_at_utc_ms)
}

fn other_owner(memory: &Memory, person_id: &str) -> bool {
    records(memory).iter().any(|r| r.owner && r.forgotten_at_utc_ms.is_none() && r.person_id != person_id)
}

pub fn list(memory: &Memory) -> Event {
    let all = records(memory);
    let people = all.iter().filter(|r| r.forgotten_at_utc_ms.is_none()).map(|r| PersonRow { person_id: r.person_id.clone(), name: r.name.clone(), relation: r.relation.clone(), owner: r.owner, recognize: r.recognize }).collect();
    let forgotten = all.iter().filter_map(|r| r.forgotten_at_utc_ms.map(|at| ForgottenRow { person_id: r.person_id.clone(), name: r.name.clone(), forgotten_at_utc_ms: at })).collect();
    Event::PeopleList { people, forgotten, prompts: Vec::new() }
}

const TRANSIENT: &[&str] = &["unavailable", "unknown_outcome", "stale"];

pub fn handle(frame: &Event, memory: &Memory, store: &SelfStore, now_utc_ms: u64) -> Option<Event> {
    let frame_id = match frame {
        Event::PeopleListRequest { .. } => return Some(list(memory)),
        Event::PeopleEnrol { frame_id, .. } | Event::PeopleUpdate { frame_id, .. } | Event::PeopleForget { frame_id, .. } | Event::PeopleUndo { frame_id, .. } | Event::PeopleCorrect { frame_id, .. } => frame_id,
        _ => return None,
    };
    let digest = Sha256::digest(serde_json::to_string(frame).unwrap_or_default().as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
    if let Some((d, reply)) = memory.people_answer(frame_id) {
        return Some(match serde_json::from_value::<Event>(reply) {
            Ok(first) if d == digest => first,
            _ => Event::PeopleRefused { frame_id: frame_id.clone(), reason: "frame_id_reused".into() },
        });
    }
    let r = match frame {
        Event::PeopleEnrol { frame_id, person_id, name, relation, owner, modality, consent_copy_version, auth } => {
            let body = Body { person_id: person_id.clone(), name: name.trim().to_string(), relation: relation.trim().to_string(), owner: *owner, recognize: true, modalities: vec![*modality], owner_since: None };
            enrol(memory, store, frame_id, body, *modality, *consent_copy_version, auth.as_ref(), now_utc_ms)
        }
        Event::PeopleUpdate { frame_id, person_id, name, relation, owner, recognize, auth } => {
            let change = Change { name: name.as_deref(), relation: relation.as_deref(), owner: *owner, recognize: *recognize };
            update(memory, store, frame_id, person_id, change, auth.as_ref(), now_utc_ms)
        }
        Event::PeopleForget { frame_id, person_id, modality, auth } => forget(memory, store, frame_id, person_id, *modality, auth.as_ref(), now_utc_ms),
        Event::PeopleUndo { frame_id, person_id, .. } => undo(memory, frame_id, person_id),
        Event::PeopleCorrect { frame_id, turn_id, person_id, owner_confirmed } => correct(memory, frame_id, turn_id, person_id, *owner_confirmed),
        _ => Err("unsupported".to_string()),
    };
    let reply = match r {
        Ok((person_id, record_rev)) => Event::PeopleAck { frame_id: frame_id.clone(), person_id, record_rev },
        Err(reason) => Event::PeopleRefused { frame_id: frame_id.clone(), reason: wire_reason(&reason).into() },
    };
    let decided = !matches!(&reply, Event::PeopleRefused { reason, .. } if TRANSIENT.contains(&reason.as_str()));
    if decided {

        if memory.record_people_answer(frame_id, &digest, &serde_json::to_value(&reply).unwrap_or_default()).is_err() {
            return Some(Event::PeopleRefused { frame_id: frame_id.clone(), reason: "unavailable".into() });
        }
    }
    Some(reply)
}

fn wire_reason(reason: &str) -> &str {
    match reason {
        "stale_revision" => "stale",
        "text_too_long" => "body_too_large",
        "unknown" => "unknown_outcome",

        r if !r.is_empty() && r.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => r,
        _ => "rejected",
    }
}

type Applied = Result<(String, u64), String>;

struct Change<'a> {
    name: Option<&'a str>,
    relation: Option<&'a str>,
    owner: Option<bool>,
    recognize: Option<bool>,
}

fn owner_grant(auth: Option<&OwnerGrant>) -> Result<(), String> {
    auth.filter(|g| !g.grant_id.trim().is_empty()).map(|_| ()).ok_or_else(|| "needs_owner_grant".to_string())
}

fn modality_name(m: PeopleModality) -> String {
    serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn text(body: &Body) -> Result<String, String> {
    let t = serde_json::to_string(body).map_err(|_| "malformed".to_string())?;
    if t.len() > MAX_BODY_BYTES {
        return Err("body_too_large".into());
    }
    Ok(t)
}

fn latest_copy() -> u32 {
    PEOPLE_COPY_VERSIONS[PEOPLE_COPY_VERSIONS.len() - 1]
}

struct Consent {
    kind: PeopleKind,
    person_id: String,
    modality: Option<PeopleModality>,
    copy_version: u32,
}

fn consent_state(store: &SelfStore, frame_id: &str, kind: Option<PeopleKind>) -> Result<Option<(PeopleKind, PeopleState)>, String> {
    let entries = store.people_consents().map_err(|_| "unavailable".to_string())?;
    Ok(entries.iter().rev().find(|e| e.frame_id.as_deref() == Some(frame_id) && kind.is_none_or(|k| e.kind == k)).map(|e| (e.kind, e.state)))
}

fn consented(store: &SelfStore, memory: &Memory, frame_id: &str, c: Consent, now_utc_ms: u64, apply: impl FnOnce() -> Applied) -> Applied {
    let state = consent_state(store, frame_id, Some(c.kind))?.map(|(_, s)| s);
    let mut people: Vec<String> = records(memory).into_iter().map(|r| r.person_id).collect();
    if c.kind == PeopleKind::Enrol {

        people.push(c.person_id.clone());
    }
    let entry = |state| PeopleEntry { kind: c.kind, person_id: c.person_id.clone(), modality: c.modality.map(modality_name), model_id: None, copy_version: c.copy_version, at: now_utc_ms, channel: PeopleChannel::App, frame_id: Some(frame_id.into()), state };
    if state.is_none() {
        store.people_consent(&entry(PeopleState::Pending), &people)?;
    }
    let r = apply();
    if state.is_none() || state == Some(PeopleState::Pending) {
        match &r {
            Ok(_) => store.people_consent(&entry(PeopleState::Done), &people)?,

            Err(reason) if TRANSIENT.contains(&wire_reason(reason)) => {}

            Err(_) => {
                let _ = store.people_consent(&entry(PeopleState::Void), &people);
            }
        }
    }
    r
}

fn write(memory: &Memory, op: Op, frame_id: &str) -> Result<u64, String> {
    let (outcome, _) = memory.people_write(op, frame_id).map_err(|_| "unavailable".to_string())?;
    outcome.map(|(_, rev)| rev)
}

fn restore(memory: &Memory, frame_id: &str, r: &PersonRecord) -> Result<PersonRecord, String> {
    if memory.people_frame_restored(frame_id).as_deref() != Some(r.memory_id.as_str()) {
        memory.people_restore(&r.memory_id, frame_id).map_err(forget_reason)?;
    }
    let found = memory.person_record(&r.memory_id).ok_or("unavailable")?;
    Ok(PersonRecord { rev: found.revision, forgotten_at_utc_ms: None, ..r.clone() })
}

#[allow(clippy::too_many_arguments)]
fn enrol(memory: &Memory, store: &SelfStore, frame_id: &str, body: Body, modality: PeopleModality, copy_version: u32, auth: Option<&OwnerGrant>, now_utc_ms: u64) -> Applied {
    owner_grant(auth)?;
    let body = Body { owner_since: auth.filter(|_| body.owner).map(|_| owner_since(memory, now_utc_ms)), ..body };
    if body.person_id.trim().is_empty() || body.name.is_empty() {
        return Err("malformed".into());
    }
    if !PEOPLE_COPY_VERSIONS.contains(&copy_version) {
        return Err("unknown_copy_version".into());
    }
    let person_id = body.person_id.clone();
    let c = Consent { kind: PeopleKind::Enrol, person_id: person_id.clone(), modality: Some(modality), copy_version };
    consented(store, memory, frame_id, c, now_utc_ms, || enrol_record(memory, frame_id, body, modality))
}

fn enrol_record(memory: &Memory, frame_id: &str, body: Body, modality: PeopleModality) -> Applied {
    let person_id = body.person_id.clone();
    let rev = match (memory.people_frame_committed(frame_id), live(memory, &person_id), forgotten(memory, &person_id)) {

        (Some((_, rev)), _, _) => rev,

        (None, Some(r), _) if memory.people_frame_restored(frame_id).as_deref() == Some(r.memory_id.as_str()) => write(memory, Op::Correct { memory_id: r.memory_id, expected_revision: r.rev, text: text(&body)? }, frame_id)?,

        (None, Some(r), _) => {
            if r.modalities.contains(&modality) {
                return Err("already_enrolled".into());
            }
            let mut b = r.body();
            b.modalities.push(modality);
            write(memory, Op::Correct { memory_id: r.memory_id, expected_revision: r.rev, text: text(&b)? }, frame_id)?
        }

        (None, None, Some(r)) => {
            if body.owner && other_owner(memory, &person_id) {
                return Err("owner_exists".into());
            }
            let t = text(&body)?;
            let back = restore(memory, frame_id, &r)?;
            write(memory, Op::Correct { memory_id: back.memory_id, expected_revision: back.rev, text: t }, frame_id)?
        }
        (None, None, None) => {
            if body.owner && other_owner(memory, &person_id) {
                return Err("owner_exists".into());
            }
            write(memory, Op::Remember { text: text(&body)? }, frame_id)?
        }
    };
    Ok((person_id, rev))
}

fn update(memory: &Memory, store: &SelfStore, frame_id: &str, person_id: &str, c: Change, auth: Option<&OwnerGrant>, now_utc_ms: u64) -> Applied {

    let off_only = c.name.is_none() && c.relation.is_none() && c.owner.is_none() && c.recognize == Some(false);
    if !off_only {
        owner_grant(auth)?;
    }

    if let Some((_, rev)) = memory.people_frame_committed(frame_id) {
        let done = || Ok((person_id.to_string(), rev));
        return match consent_state(store, frame_id, None)? {
            Some((kind, PeopleState::Pending)) => consented(store, memory, frame_id, Consent { kind, person_id: person_id.into(), modality: None, copy_version: latest_copy() }, now_utc_ms, done),
            _ => done(),
        };
    }
    let r = live(memory, person_id).ok_or("unknown_person")?;
    let before = r.body();
    let after = Body {
        name: c.name.map(|n| n.trim().to_string()).unwrap_or(before.name.clone()),
        relation: c.relation.map(|n| n.trim().to_string()).unwrap_or(before.relation.clone()),
        owner: c.owner.unwrap_or(before.owner),
        recognize: c.recognize.unwrap_or(before.recognize),

        owner_since: match (before.owner, c.owner.unwrap_or(before.owner)) {
            (false, true) => auth.map(|_| owner_since(memory, now_utc_ms)),
            (true, true) => before.owner_since,
            (_, false) => None,
        },
        ..before.clone()
    };
    if after.name.is_empty() {
        return Err("malformed".into());
    }
    if after == before {
        return Ok((r.person_id, r.rev));
    }
    if after.owner && !before.owner && other_owner(memory, person_id) {
        return Err("owner_exists".into());
    }
    let change = || Ok((person_id.to_string(), write(memory, Op::Correct { memory_id: r.memory_id.clone(), expected_revision: r.rev, text: text(&after)? }, frame_id)?));

    if before.recognize == after.recognize {
        return change();
    }
    let kind = if after.recognize { PeopleKind::Enrol } else { PeopleKind::Revoke };
    consented(store, memory, frame_id, Consent { kind, person_id: person_id.into(), modality: None, copy_version: latest_copy() }, now_utc_ms, change)
}

fn forget(memory: &Memory, store: &SelfStore, frame_id: &str, person_id: &str, modality: Option<PeopleModality>, auth: Option<&OwnerGrant>, now_utc_ms: u64) -> Applied {
    owner_grant(auth)?;
    let c = Consent { kind: PeopleKind::Revoke, person_id: person_id.into(), modality, copy_version: latest_copy() };
    match modality {

        Some(m) => {
            if memory.people_frame_committed(frame_id).is_none() {
                let r = live(memory, person_id).ok_or("unknown_person")?;
                if !r.modalities.contains(&m) {

                    return Ok((r.person_id, r.rev));
                }
            }
            consented(store, memory, frame_id, c, now_utc_ms, || {
                let rev = match memory.people_frame_committed(frame_id) {
                    Some((_, rev)) => rev,
                    None => {
                        let r = live(memory, person_id).ok_or("unknown_person")?;
                        let mut b = r.body();
                        b.modalities.retain(|x| *x != m);
                        write(memory, Op::Correct { memory_id: r.memory_id, expected_revision: r.rev, text: text(&b)? }, frame_id)?
                    }
                };
                Ok((person_id.to_string(), rev))
            })
        }

        None => consented(store, memory, frame_id, c, now_utc_ms, || {
            let memory_id = match memory.people_frame_forgot(frame_id) {
                Some(id) => id,
                None => {
                    let r = live(memory, person_id).ok_or("unknown_person")?;
                    memory.people_forget(&r.memory_id, r.rev, frame_id).map_err(forget_reason)?;
                    r.memory_id
                }
            };
            Ok((person_id.to_string(), memory.person_record(&memory_id).map(|f| f.revision).ok_or("unavailable")?))
        }),
    }
}

fn undo(memory: &Memory, frame_id: &str, person_id: &str) -> Applied {
    if let Some((_, rev)) = memory.people_frame_committed(frame_id) {
        return Ok((person_id.to_string(), rev));
    }
    let restored = memory.people_frame_restored(frame_id);
    let back = match (restored, live(memory, person_id)) {
        (Some(id), Some(r)) if r.memory_id == id => r,
        (_, Some(_)) => return Err("already_enrolled".into()),
        (_, None) => restore(memory, frame_id, &forgotten(memory, person_id).ok_or("unknown_person")?)?,
    };
    let owner = back.owner && !other_owner(memory, person_id);
    let b = Body { modalities: Vec::new(), owner, owner_since: back.owner_since.filter(|_| owner), ..back.body() };
    if b == back.body() {
        return Ok((back.person_id, back.rev));
    }
    Ok((back.person_id, write(memory, Op::Correct { memory_id: back.memory_id, expected_revision: back.rev, text: text(&b)? }, frame_id)?))
}

pub fn reconcile_owners(memory: &Memory) -> Result<usize, String> {
    let owners: Vec<PersonRecord> = records(memory).into_iter().filter(|r| r.owner && r.forgotten_at_utc_ms.is_none()).collect();
    let Some(keep) = owners.iter().enumerate().max_by_key(|(i, r)| (r.owner_since.unwrap_or(0), std::cmp::Reverse(*i))).map(|(_, r)| r.memory_id.clone()) else { return Ok(0) };
    let mut demoted = 0;
    for r in owners.iter().filter(|r| r.memory_id != keep) {
        let b = Body { owner: false, owner_since: None, ..r.body() };

        write(memory, Op::Correct { memory_id: r.memory_id.clone(), expected_revision: r.rev, text: text(&b)? }, &format!("owner-reconcile:{}:{}", r.memory_id, r.rev))?;
        demoted += 1;
    }
    Ok(demoted)
}

fn forget_reason(e: ForgetError) -> String {
    match e {
        ForgetError::Stale(_) => "stale".into(),
        ForgetError::Failed(r) if r == "unknown_person" => r,
        ForgetError::Failed(_) => "unavailable".into(),
        ForgetError::Unknown(_) => "unknown_outcome".into(),
    }
}

#[cfg(test)]
mod pv05_g6;

#[cfg(test)]
mod pv05_g9 {
    use super::*;

    fn rec(person_id: &str, owner: bool, forgotten: bool) -> PersonRecord {
        PersonRecord { person_id: person_id.into(), name: "x".into(), relation: "family".into(), owner, recognize: true, modalities: vec![], owner_since: None, rev: 1, memory_id: format!("m-{person_id}"), forgotten_at_utc_ms: forgotten.then_some(1) }
    }

    fn person(id: &str) -> TurnSpeaker {
        TurnSpeaker::Person { person_id: id.into() }
    }

    #[test]
    fn pv05_g9_the_owner_is_the_keyboard_or_the_live_owner_record() {
        let people = [rec("p-rui", true, false), rec("p-lia", false, false)];
        assert!(turn_is_owner(&TurnSpeaker::OwnerKeyboard, &people));
        assert!(turn_is_owner(&person("p-rui"), &people));
        for s in [person("p-lia"), person("p-new"), TurnSpeaker::Unknown, TurnSpeaker::Uncertain] {
            assert!(!turn_is_owner(&s, &people), "{s:?}");
        }

        let gone = [rec("p-rui", true, true)];
        for s in [person("p-rui"), TurnSpeaker::Unknown, TurnSpeaker::Uncertain] {
            assert!(turn_is_owner(&s, &gone), "{s:?}");
            assert!(turn_is_owner(&s, &[]), "{s:?}");
        }
    }

    #[test]
    fn pv05_g9_the_turn_block_names_people_only_when_memory_may_leave() {
        let named = |id: &str, name: &str, owner: bool, forgotten: bool| PersonRecord { name: name.into(), ..rec(id, owner, forgotten) };
        let people = [named("p-rui", "Rui", true, false), named("p-ana", "Ana", false, true), named("p-lia", "Lia", false, false)];
        let seen = Presence { present: vec!["p-rui".into(), "p-lia".into(), "p-ana".into()], unknown_faces: 1, camera: true };
        let hidden = turn_block(&person("p-lia"), &people, Some(&seen), false, false).unwrap();
        assert!(hidden.contains("Said by: Person 3.") && hidden.contains("Seen by the camera: Person 1 (the owner), Person 3; 1 face not recognised."), "{hidden}");

        let here = Presence { camera: false, ..seen.clone() };
        let told = turn_block(&person("p-lia"), &people, Some(&here), false, false).unwrap();
        assert!(told.contains("Here now: Person 1 (the owner), Person 3; 1 person not recognised.") && !told.contains("camera") && !told.contains("face"), "{told}");
        assert!(turn_block(&person("p-lia"), &people, Some(&here), false, true).unwrap().contains("Aqui agora: Pessoa 1 (the owner), Pessoa 3; 1 person not recognised."));
        assert!(!["Rui", "Lia", "Ana", "p-"].iter().any(|n| hidden.contains(n)), "no name or id leaves: {hidden}");
        assert!(turn_block(&person("p-lia"), &people, None, false, true).unwrap().contains("Said by: Pessoa 3."), "a forgotten person keeps their number");
        let shown = turn_block(&person("p-rui"), &people, Some(&seen), true, false).unwrap();
        assert!(shown.contains("Said by: Rui (the owner).") && shown.contains("Rui (the owner), Lia;"), "{shown}");
        assert!(turn_block(&TurnSpeaker::Unknown, &people, None, true, false).unwrap().contains("Said by: a voice not recognised."));
        assert!(turn_block(&TurnSpeaker::Uncertain, &people, None, true, false).unwrap().contains("Said by: a voice not told apart."));
        assert!(turn_block(&TurnSpeaker::OwnerKeyboard, &people, None, false, false).unwrap().contains("Said by: the owner, typing."));
        let empty = Presence { present: vec![], unknown_faces: 0, camera: false };
        assert!(turn_block(&TurnSpeaker::OwnerKeyboard, &people, Some(&empty), true, false).unwrap().contains("Here now: nobody."));
        assert_eq!(turn_block(&person("p-lia"), &[], Some(&seen), true, false), None, "nobody enrolled: no block (0.4)");
        assert_eq!(turn_block(&person("p-rui"), &[named("p-rui", "Rui", true, true)], None, true, false), None, "nobody live: no block");
    }

    #[test]
    fn pv05_g9_a_task_or_an_unbound_output_is_the_owners_only_while_no_one_is_enrolled() {
        let people = [rec("p-rui", true, false), rec("p-lia", false, false)];
        assert!(tag_is_owner(None, &people), "a task from before 0.5");
        assert!(tag_is_owner(Some("keyboard"), &people) && tag_is_owner(Some("person:p-rui"), &people));
        for t in ["person:p-lia", "unknown", "uncertain"] {
            assert!(!tag_is_owner(Some(t), &people), "{t}");
            assert!(tag_is_owner(Some(t), &[]) && tag_is_owner(Some(t), &[rec("p-rui", true, true)]), "{t}: nobody enrolled is 0.4");
        }
        assert_eq!(who_without_turn(&people), Who { tag: "uncertain".into(), owner: false });
        assert_eq!(who_without_turn(&[]), Who::keyboard());
    }

    #[test]
    fn pv05_g9_an_item_before_0_5_or_at_the_keyboard_is_the_owners() {
        let people = [rec("p-rui", true, false), rec("p-lia", false, false)];
        assert!(item_is_owners(None, &people), "saved before 0.5");
        assert!(item_is_owners(Some("keyboard"), &people));
        assert!(item_is_owners(Some("person:p-rui"), &people));
        for t in ["person:p-lia", "unknown", "uncertain", "person:p-new"] {
            assert!(!item_is_owners(Some(t), &people), "{t}");
        }
        assert_eq!(speaker_tag(&person("p-lia")), "person:p-lia");
        let lia = Who { tag: speaker_tag(&person("p-lia")), owner: false };
        assert!(lia.owns(Some("person:p-lia")) && !lia.owns(None) && !lia.owns(Some("keyboard")));
        let unknown = Who { tag: speaker_tag(&TurnSpeaker::Unknown), owner: false };
        assert!(!unknown.owns(Some("unknown")), "an unknown voice owns nothing");
    }
}
