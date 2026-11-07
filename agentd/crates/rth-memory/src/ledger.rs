

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "rth-memory/v1";

pub const PRIVATE_SCOPE: &str = "private";

pub const MAX_TEXT_BYTES: usize = 1000;

pub const MAX_ITEMS: usize = 256;

pub const MAX_LEDGER_TEXT_BYTES: usize = 64 * 1024;

pub const RECALL_MAX_ITEMS: usize = 20;

pub const RECALL_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Remember,
    Correct,

    Invalidate,

    Restore,

    Retag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {

    Committed,

    Proposed,

    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub provider_generation: u64,
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acceptance {
    pub accepted_by: String,
    pub acceptance_event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Act {
    pub schema: String,
    pub act_id: String,

    pub operation_key: String,
    pub instance_id: String,
    pub scope: String,
    pub kind: Kind,
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_from_revision: Option<u64>,

    pub text: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_kind: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_speaker: Option<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deduplicated: bool,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<Acceptance>,
    pub boot: String,
    pub utc_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Remember { text: String, kind: Option<String>, speaker: Option<String> },
    Correct { memory_id: String, expected_revision: u64, text: String },
    Invalidate { memory_id: String, expected_revision: u64 },
    Restore { memory_id: String, expected_revision: u64, to_revision: u64 },
    Retag { memory_id: String, expected_revision: u64, speaker: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub act: Act,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    pub revision: u64,
    pub text: String,
    pub act_id: String,
    pub utc_ms: u64,

    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub invalidated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub memory_id: String,
    pub revisions: Vec<Revision>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

impl Item {
    pub fn current(&self) -> &Revision {
        self.revisions.last().expect("an item has at least one revision")
    }

    pub fn is_person(&self) -> bool {
        self.kind.as_deref() == Some(crate::contract::PERSON_KIND)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemView {
    pub memory_id: String,
    pub revision: u64,
    pub text: String,
    pub act_id: String,
    pub saved_utc_ms: u64,
    pub first_saved_utc_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recall {
    pub items: Vec<ItemView>,
    pub total: usize,
    pub omitted: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    pub truncated_bytes: u64,
    pub acts: usize,
    pub items: usize,
}

pub struct Ledger {
    path: PathBuf,
    file: std::fs::File,
    instance_id: String,
    scope: String,
    boot: String,

    poisoned: Option<String>,
    acts: usize,
    remembered: u64,
    by_key: HashMap<String, Act>,
    items: BTreeMap<String, Item>,
    text_bytes: usize,
}

fn bad(msg: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
}

impl Ledger {

    pub fn open(path: &Path, instance_id: &str, scope: &str, boot: &str) -> std::io::Result<(Ledger, Recovered)> {
        let existed = match std::fs::metadata(path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {
            file.sync_all()?;
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()?;
        }
        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_end(&mut bytes)?;
        let keep = bytes.iter().rposition(|&b| b == b'\n').map(|i| i + 1).unwrap_or(0);
        let truncated_bytes = (bytes.len() - keep) as u64;
        if truncated_bytes > 0 {
            file.set_len(keep as u64)?;
            file.sync_all()?;
            bytes.truncate(keep);
        }
        let text = String::from_utf8(bytes).map_err(|e| bad(format!("{}: not UTF-8: {e}", path.display())))?;
        let mut l = Ledger {
            path: path.to_path_buf(),
            file,
            instance_id: instance_id.to_string(),
            scope: scope.to_string(),
            boot: boot.to_string(),
            poisoned: None,
            acts: 0,
            remembered: 0,
            by_key: HashMap::new(),
            items: BTreeMap::new(),
            text_bytes: 0,
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let act: Act = serde_json::from_str(line).map_err(|e| bad(format!("{}: line {}: {e}", path.display(), n + 1)))?;
            if act.instance_id != instance_id || act.scope != scope {
                return Err(bad(format!("{}: line {}: act of instance {:?} scope {:?}", path.display(), n + 1, act.instance_id, act.scope)));
            }
            l.fold(act).map_err(|e| bad(format!("{}: line {}: {e}", path.display(), n + 1)))?;
        }
        let rec = Recovered { truncated_bytes, acts: l.acts, items: l.items.len() };
        Ok((l, rec))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn fold(&mut self, act: Act) -> Result<(), String> {
        if self.by_key.contains_key(&act.operation_key) {
            return Err(format!("operation key {} recorded twice", act.operation_key));
        }
        self.acts += 1;
        if act.state == State::Committed {
            let id = act.memory_id.clone().ok_or("a committed act without memory_id")?;
            let rev = act.revision.ok_or("a committed act without revision")?;
            let r = Revision { revision: rev, text: act.text.clone(), act_id: act.act_id.clone(), utc_ms: act.utc_ms, invalidated: false };
            match act.kind {
                Kind::Remember => {
                    if rev != 1 || self.items.contains_key(&id) {
                        return Err(format!("remember of {id} at revision {rev}"));
                    }
                    self.remembered += 1;
                    self.text_bytes += r.text.len();
                    self.items.insert(id.clone(), Item { memory_id: id, revisions: vec![r], kind: act.item_kind.clone(), speaker: act.item_speaker.clone() });
                }
                Kind::Correct => {
                    let item = self.items.get_mut(&id).ok_or(format!("correction of unknown {id}"))?;
                    if rev != item.current().revision + 1 {
                        return Err(format!("correction of {id} to revision {rev} after {}", item.current().revision));
                    }
                    self.text_bytes = self.text_bytes - item.current().text.len() + r.text.len();
                    item.revisions.push(r);
                }
                Kind::Retag => {
                    let item = self.items.get_mut(&id).ok_or(format!("retag of unknown {id}"))?;
                    if rev != item.current().revision + 1 {
                        return Err(format!("retag of {id} to revision {rev} after {}", item.current().revision));
                    }
                    item.revisions.push(r);
                    item.speaker = act.item_speaker.clone();
                }
                Kind::Invalidate | Kind::Restore => {
                    let item = self.items.get_mut(&id).ok_or(format!("{:?} of unknown {id}", act.kind))?;
                    if rev != item.current().revision + 1 {
                        return Err(format!("{:?} of {id} to revision {rev} after {}", act.kind, item.current().revision));
                    }

                    let invalidated = match act.kind {
                        Kind::Invalidate => true,
                        _ => {
                            let to = act.restored_from_revision.and_then(|t| t.checked_sub(1)).ok_or("a restore without its target revision")?;
                            item.revisions.get(to as usize).ok_or(format!("restore of {id} from a revision it does not have"))?.invalidated
                        }
                    };
                    self.text_bytes = self.text_bytes - item.current().text.len() + r.text.len();
                    item.revisions.push(Revision { invalidated, ..r });
                }
            }
        }
        self.by_key.insert(act.operation_key.clone(), act);
        Ok(())
    }

    pub fn act(&mut self, operation_key: &str, req: Request, source: Source, acceptance: Option<Acceptance>, utc_ms: u64) -> std::io::Result<Outcome> {
        if let Some(p) = &self.poisoned {
            return Err(std::io::Error::other(format!("memory ledger refused: {p}")));
        }
        if let Some(act) = self.by_key.get(operation_key) {
            return Ok(Outcome { act: act.clone(), duplicate: true });
        }
        let (item_kind, item_speaker) = match &req {
            Request::Remember { kind, speaker, .. } => (kind.clone(), speaker.clone()),
            Request::Retag { speaker, .. } => (None, Some(speaker.clone())),
            _ => (None, None),
        };
        let (kind, memory_id, expected_revision, to_revision, text) = match req {
            Request::Remember { text, .. } => (Kind::Remember, None, None, None, text),
            Request::Correct { memory_id, expected_revision, text } => (Kind::Correct, Some(memory_id), Some(expected_revision), None, text),
            Request::Invalidate { memory_id, expected_revision } => (Kind::Invalidate, Some(memory_id), Some(expected_revision), None, String::new()),
            Request::Restore { memory_id, expected_revision, to_revision } => (Kind::Restore, Some(memory_id), Some(expected_revision), Some(to_revision), String::new()),
            Request::Retag { memory_id, expected_revision, .. } => (Kind::Retag, Some(memory_id), Some(expected_revision), None, String::new()),
        };
        let item = memory_id.as_ref().and_then(|id| self.items.get(id));
        let current = item.map(|i| i.current().clone());
        let target = to_revision.and_then(|t| t.checked_sub(1)).and_then(|n| item.and_then(|i| i.revisions.get(n as usize))).cloned();
        let lifecycle = matches!(kind, Kind::Invalidate | Kind::Restore | Kind::Retag);

        let text = match kind {
            Kind::Invalidate | Kind::Retag => current.as_ref().map(|c| c.text.clone()).unwrap_or_default(),
            Kind::Restore => target.as_ref().map(|t| t.text.clone()).unwrap_or_default(),
            _ => text.trim().to_string(),
        };
        let refusal = if lifecycle {
            match &current {
                None => Some("unknown_target"),
                Some(c) if Some(c.revision) != expected_revision => Some("stale_revision"),
                Some(_) if kind == Kind::Restore && target.is_none() => Some("unknown_revision"),
                Some(c) if kind == Kind::Restore && !c.invalidated => Some("not_invalidated"),
                Some(c) if kind == Kind::Retag && c.invalidated => Some("invalidated"),
                Some(c) if self.text_bytes - c.text.len() + text.len() > MAX_LEDGER_TEXT_BYTES => Some("ledger_full"),
                Some(_) => None,
            }
        } else if text.is_empty() {
            Some("empty_text")
        } else if text.len() > MAX_TEXT_BYTES {
            Some("text_too_long")
        } else if kind == Kind::Correct && current.is_none() {
            Some("unknown_target")
        } else if kind == Kind::Correct && current.as_ref().map(|c| c.revision) != expected_revision {
            Some("stale_revision")
        } else if kind == Kind::Remember && (self.items.len() >= MAX_ITEMS || self.text_bytes + text.len() > MAX_LEDGER_TEXT_BYTES) {
            Some("ledger_full")
        } else if kind == Kind::Correct && self.text_bytes - current.as_ref().map(|c| c.text.len()).unwrap_or(0) + text.len() > MAX_LEDGER_TEXT_BYTES {
            Some("ledger_full")
        } else {
            None
        };

        let same = if kind == Kind::Remember { self.items.values().find(|i| i.current().text == text).map(|i| i.memory_id.clone()) } else { None };
        let (refusal, memory_id) = match (refusal, same) {
            (None, Some(id)) => (Some("duplicate_of"), Some(id)),
            (r, _) => (r, memory_id),
        };
        let deduplicated = false;
        let (state, reason, memory_id, revision) = match (refusal, &acceptance) {
            (Some(r), _) => (State::Rejected, Some(r.to_string()), memory_id, None),
            (None, None) => (State::Proposed, Some("not_accepted".to_string()), memory_id, None),
            (None, Some(_)) => match kind {
                Kind::Remember => (State::Committed, None, Some(format!("m-{}", self.remembered + 1)), Some(1)),
                Kind::Correct | Kind::Invalidate | Kind::Restore | Kind::Retag => (State::Committed, None, memory_id, current.map(|c| c.revision + 1)),
            },
        };
        let text = if reason.as_deref() == Some("text_too_long") { String::new() } else { text };
        let act = Act {
            schema: SCHEMA.into(),
            act_id: format!("a-{}", self.acts + 1),
            operation_key: operation_key.into(),
            instance_id: self.instance_id.clone(),
            scope: self.scope.clone(),
            kind,
            state,
            reason,
            memory_id,
            revision,
            expected_revision,
            restored_from_revision: to_revision,
            text,
            item_kind,
            item_speaker,
            deduplicated,
            source,
            acceptance,
            boot: self.boot.clone(),
            utc_ms,
        };
        let mut line = serde_json::to_string(&act).map_err(std::io::Error::other)?;
        line.push('\n');
        if let Err(e) = self.file.write_all(line.as_bytes()).and_then(|()| self.file.sync_data()) {
            self.poisoned = Some(format!("write failed: {e}"));
            return Err(e);
        }
        self.fold(act.clone()).map_err(std::io::Error::other)?;
        Ok(Outcome { act, duplicate: false })
    }

    pub fn newest_first(&self) -> Vec<&Item> {
        let mut all: Vec<&Item> = self.items.values().collect();
        all.sort_by(|a, b| b.current().utc_ms.cmp(&a.current().utc_ms).then_with(|| b.memory_id.cmp(&a.memory_id)));
        all
    }

    pub fn recall(&self) -> Recall {
        let all: Vec<&Item> = self.newest_first().into_iter().filter(|i| !i.current().invalidated && !i.is_person()).collect();
        let mut items = Vec::new();
        let mut bytes = 0;
        for item in &all {
            let c = item.current();
            if items.len() == RECALL_MAX_ITEMS || bytes + c.text.len() > RECALL_MAX_BYTES {
                break;
            }
            bytes += c.text.len();
            items.push(ItemView {
                memory_id: item.memory_id.clone(),
                revision: c.revision,
                text: c.text.clone(),
                act_id: c.act_id.clone(),
                saved_utc_ms: c.utc_ms,
                first_saved_utc_ms: item.revisions[0].utc_ms,
            });
        }
        Recall { total: all.len(), omitted: all.len() - items.len(), items }
    }

    pub fn item(&self, memory_id: &str) -> Option<&Item> {
        self.items.get(memory_id)
    }

    pub fn recorded(&self, operation_key: &str) -> Option<&Act> {
        self.by_key.get(operation_key)
    }
}

pub struct Reference {
    pub ledger: Ledger,
}

impl Reference {
    fn write(&mut self, key: &str, req: Request, from: &crate::contract::Provenance) -> Result<crate::contract::Saved, crate::contract::Failure> {
        use crate::contract::{Failure, Saved};
        if let Some(prior) = self.ledger.recorded(key) {
            let same = match &req {
                Request::Remember { text, kind, speaker } => prior.kind == Kind::Remember && prior.text == text.trim() && prior.item_kind == *kind && prior.item_speaker == *speaker,
                Request::Correct { memory_id, expected_revision, text } => {
                    prior.kind == Kind::Correct && prior.memory_id.as_deref() == Some(memory_id.as_str()) && prior.expected_revision == Some(*expected_revision) && prior.text == text.trim()
                }
                Request::Invalidate { memory_id, expected_revision } => prior.kind == Kind::Invalidate && prior.memory_id.as_deref() == Some(memory_id.as_str()) && prior.expected_revision == Some(*expected_revision),
                Request::Restore { memory_id, expected_revision, to_revision } => {
                    prior.kind == Kind::Restore && prior.memory_id.as_deref() == Some(memory_id.as_str()) && prior.expected_revision == Some(*expected_revision) && prior.restored_from_revision == Some(*to_revision)
                }
                Request::Retag { memory_id, expected_revision, speaker } => {
                    prior.kind == Kind::Retag && prior.memory_id.as_deref() == Some(memory_id.as_str()) && prior.expected_revision == Some(*expected_revision) && prior.item_speaker.as_deref() == Some(speaker.as_str())
                }
            };
            if !same {
                return Err(Failure::rejected("idempotency_conflict"));
            }
        }
        let source = Source { provider_generation: from.provider_generation, call_id: from.call_id.clone(), response_id: from.response_id.clone(), command_id: Some(from.command_id.clone()) };
        let acceptance = Some(Acceptance { accepted_by: "typed_command".into(), acceptance_event_id: from.command_id.clone() });
        let o = self.ledger.act(key, req, source, acceptance, now_ms()).map_err(|e| Failure::Unavailable { detail: e.to_string() })?;
        match o.act.state {
            State::Committed => Ok(Saved { memory_id: o.act.memory_id.unwrap_or_default(), revision: o.act.revision.unwrap_or_default(), replayed: o.duplicate }),
            _ => {
                let reason = o.act.reason.unwrap_or_default();
                let actual = if reason == "stale_revision" { o.act.memory_id.as_deref().and_then(|id| self.ledger.item(id)).map(|i| i.current().revision) } else { None };
                let named = if reason == "duplicate_of" { o.act.memory_id.clone() } else { None };
                Err(Failure::Rejected { reason, actual_revision: actual, memory_id: named })
            }
        }
    }
}

impl Reference {

    fn lifecycle_source(key: &str) -> crate::contract::Provenance {
        crate::contract::Provenance { instance_id: String::new(), provider_generation: 0, call_id: key.into(), response_id: None, command_id: key.into(), origin: Default::default(), speaker: None }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl crate::contract::Provider for Reference {
    fn name(&self) -> &'static str {
        "reference"
    }
    fn remember(&mut self, key: &str, text: &str, from: &crate::contract::Provenance) -> Result<crate::contract::Saved, crate::contract::Failure> {
        let kind = (from.origin == crate::contract::Origin::PeopleFrame).then(|| crate::contract::PERSON_KIND.to_string());
        self.write(key, Request::Remember { text: text.into(), kind, speaker: from.speaker.clone() }, from)
    }
    fn correct(&mut self, key: &str, memory_id: &str, expected_revision: u64, text: &str, from: &crate::contract::Provenance) -> Result<crate::contract::Saved, crate::contract::Failure> {
        self.write(key, Request::Correct { memory_id: memory_id.into(), expected_revision, text: text.into() }, from)
    }
    fn invalidate(&mut self, key: &str, memory_id: &str, revision: u64, _reason: &str) -> Result<crate::contract::Saved, crate::contract::Failure> {
        self.write(key, Request::Invalidate { memory_id: memory_id.into(), expected_revision: revision }, &Self::lifecycle_source(key))
    }
    fn restore(&mut self, key: &str, memory_id: &str, expected_revision: u64, to_revision: u64) -> Result<crate::contract::Saved, crate::contract::Failure> {
        self.write(key, Request::Restore { memory_id: memory_id.into(), expected_revision, to_revision }, &Self::lifecycle_source(key))
    }
    fn retag(&mut self, key: &str, memory_id: &str, expected_revision: u64, speaker: &str) -> Result<crate::contract::Saved, crate::contract::Failure> {
        self.write(key, Request::Retag { memory_id: memory_id.into(), expected_revision, speaker: speaker.into() }, &Self::lifecycle_source(key))
    }
    fn recall(&mut self, _query: &str, limit: usize) -> Result<crate::contract::Recalled, crate::contract::Failure> {
        let r = self.ledger.recall();
        let items: Vec<crate::contract::Found> = r.items.iter().take(limit).map(|i| crate::contract::Found { memory_id: i.memory_id.clone(), revision: i.revision, text: i.text.clone(), saved_utc_ms: self.ledger.item(&i.memory_id).map(|it| it.current().utc_ms), kind: self.ledger.item(&i.memory_id).and_then(|it| it.kind.clone()), speaker: self.ledger.item(&i.memory_id).and_then(|it| it.speaker.clone()) }).collect();

        let dropped = self
            .ledger
            .newest_first()
            .iter()
            .enumerate()
            .filter_map(|(rank, i)| {
                let reason = if i.current().invalidated { "invalidated" } else if i.is_person() { "person_record" } else { return None };
                Some(crate::contract::Dropped { memory_id: Some(i.memory_id.clone()), brain_ref: None, kind: "excluded", reason, rank })
            })
            .collect();
        Ok(crate::contract::Recalled { omitted: r.total - items.len(), total: r.total, items, trace_id: None, ranks: vec![], dropped })
    }
    fn get(&mut self, memory_id: &str) -> Result<crate::contract::Found, crate::contract::Failure> {
        let item = self.ledger.item(memory_id).ok_or(crate::contract::Failure::rejected("unknown_target"))?;
        let c = item.current();
        if c.invalidated {
            return Err(crate::contract::Failure::rejected("invalidated"));
        }
        Ok(crate::contract::Found { memory_id: memory_id.into(), revision: c.revision, text: c.text.clone(), saved_utc_ms: Some(c.utc_ms), kind: item.kind.clone(), speaker: item.speaker.clone() })
    }
    fn history(&mut self, memory_id: &str) -> Result<Vec<crate::contract::Found>, crate::contract::Failure> {
        let item = self.ledger.item(memory_id).ok_or(crate::contract::Failure::rejected("unknown_target"))?;
        Ok(item.revisions.iter().rev().map(|r| crate::contract::Found { memory_id: memory_id.into(), revision: r.revision, text: r.text.clone(), saved_utc_ms: Some(r.utc_ms), kind: item.kind.clone(), speaker: item.speaker.clone() }).collect())
    }
    fn recent(&mut self, limit: usize) -> Result<Vec<crate::contract::Found>, crate::contract::Failure> {
        let mut all: Vec<crate::contract::Found> = self.ledger.recall().items.iter().filter_map(|i| self.ledger.item(&i.memory_id)).map(|it| {
            let c = it.current();
            crate::contract::Found { memory_id: it.memory_id.clone(), revision: c.revision, text: c.text.clone(), saved_utc_ms: Some(c.utc_ms), kind: it.kind.clone(), speaker: it.speaker.clone() }
        }).collect();
        all.sort_by(|a, b| b.saved_utc_ms.cmp(&a.saved_utc_ms));
        all.truncate(limit);
        Ok(all)
    }
}

#[cfg(test)]
mod tests;
