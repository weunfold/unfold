

#[path = "conversation_observe.rs"]
pub mod observe;
#[path = "conversation_mirror.rs"]
pub mod mirror;
use observe::{Observation, Payload, Receipt, Snapshot, Span, Terminal, MemoryReceipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

const MAX_INPUT: usize = 8192;
const MAX_PENDING: usize = 64;
const MAX_LOG_BYTES: u64 = 64 << 20;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Owner {
    Front,
    Legacy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub id: String,
    pub response_id: String,
    pub observation_id: String,
    pub nonce: String,
    pub input: String,
    pub owner: Option<Owner>,
    pub rendered: bool,

    pub presentation_uncertain: bool,
    pub legacy_attempted: bool,
    pub backend_response: Option<String>,
    pub acknowledged: bool,
    pub presentation: Option<Payload>,
    pub silent_replay: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Projection {
    session: String,
    turns: Vec<Turn>,
    observations: Vec<Observation>,
    receipts: Vec<Receipt>,
    memory: Vec<MemoryReceipt>,
    mirror_blocked: bool,
    mirror_replay: Option<usize>,

    #[serde(skip)]
    by_id: HashMap<String, usize>,
    #[serde(skip)]
    by_nonce: HashMap<String, usize>,
    #[serde(skip)]
    receipted: HashMap<String, usize>,
    #[serde(skip)]
    cursor: usize,
    #[serde(skip)]
    pending: usize,
    #[serde(skip)]
    cancel_waiting: std::collections::HashSet<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Event {
    Session { session: String },
    Snapshot { projection: Projection },
    RecoverSilent { turn_id: String },
    NoRenderCancel { turn_id: String },
    Commit { nonce: String, input: String },
    Select { turn_id: String, owner: Owner },
    FirstRendered { turn_id: String },
    LegacySend { turn_id: String },
    LegacyAccepted { turn_id: String, backend_response: String },
    ObserveAck { turn_id: String },
    Presentation { turn_id: String, payload: Payload },
    MirrorPrepared { item: Observation },
    MirrorAccepted { receipt: Receipt },
    MirrorConflict,
    MirrorResume { snapshot: Snapshot },
    MirrorMemory { receipt: MemoryReceipt },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    sequence: u64,
    previous: String,
    event: Event,
    digest: String,
}

#[derive(Default)]
pub struct SilentWindow {
    owner: Option<std::sync::Arc<()>>,
    next: usize,
}
pub const SILENT_BATCH: usize = 16;
pub struct SilentBatch {
    pub turns: Vec<Turn>,
    pub inspected: usize,

    pub more: bool,
}

pub struct Store {
    replay_owner: std::sync::Arc<()>,
    file: File,
    _lock: File,
    recovered: std::collections::HashSet<String>,
    path: std::path::PathBuf,
    projection: Projection,
    sequence: u64,
    digest: String,
    poisoned: bool,
}

pub enum FrontStore {
    Ready(Box<Store>),

    LegacyOnly { message: &'static str },
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn opaque(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn hash<T: Serialize>(value: &T) -> io::Result<String> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl Projection {

    fn release_cancels_before(&mut self, index: usize) {
        self.cancel_waiting.retain(|id| self.by_id[id] >= index);
    }
    fn index(&self, id: &str) -> io::Result<usize> {
        self.by_id.get(id).copied().ok_or_else(observe::invalid)
    }
    fn pending_turn(&self, turn: &Turn) -> bool {

        if no_render_cancel(turn) { return false; }
        !turn.acknowledged || ((turn.owner == Some(Owner::Front) || turn.silent_replay)
            && !self.receipted.contains_key(&presentation_id(turn)))
    }
    fn complete(&self, turn: &Turn) -> bool {
        (!turn.silent_replay && turn.owner == Some(Owner::Legacy) && turn.backend_response.is_some())
            || (self.receipted.contains_key(&turn.observation_id)
                && self.receipted.contains_key(&presentation_id(turn)))
    }
    fn advance(&mut self) {
        while self.turns.get(self.cursor).is_some_and(|t| self.complete(t)) { self.cursor += 1; }
    }
    fn rebuild(&mut self) -> io::Result<()> {
        self.by_id.clear(); self.by_nonce.clear(); self.receipted.clear();
        for (i, turn) in self.turns.iter().enumerate() {
            if self.by_id.insert(turn.id.clone(), i).is_some()
                || self.by_nonce.insert(turn.nonce.clone(), i).is_some() { return Err(observe::invalid()); }
        }
        if self.receipts.len() > self.observations.len() { return Err(observe::invalid()); }
        for (i, receipt) in self.receipts.iter().enumerate() {
            if self.observations[i].receipt() != *receipt
                || self.receipted.insert(receipt.event_id.clone(), i).is_some() { return Err(observe::invalid()); }
        }

        self.cancel_waiting.clear();
        self.pending = self.turns.iter().filter(|t| self.pending_turn(t)).count();
        self.cursor = 0; self.advance(); Ok(())
    }
}

fn apply(projection: &mut Projection, event: &Event) -> io::Result<()> {
    let index = match event {
        Event::RecoverSilent {turn_id} | Event::NoRenderCancel {turn_id} | Event::Presentation {turn_id, ..}
        | Event::Select {turn_id, ..} | Event::FirstRendered {turn_id}
        | Event::LegacySend {turn_id} | Event::LegacyAccepted {turn_id, ..}
        | Event::ObserveAck {turn_id} => Some(projection.index(turn_id)?),
        Event::MirrorAccepted {..} => projection.observations
            .get(projection.mirror_replay.unwrap_or(projection.receipts.len()))
            .map(|o| projection.index(&o.turn_id)).transpose()?,
        Event::MirrorResume {snapshot} if snapshot.version as usize > projection.receipts.len() =>
            projection.observations.get(snapshot.version.saturating_sub(1) as usize)
                .map(|o| projection.index(&o.turn_id)).transpose()?,
        _ => None,
    };
    let before = index.map(|i| projection.pending_turn(&projection.turns[i]));
    let complete = index.is_some_and(|i| projection.complete(&projection.turns[i]));
    apply_inner(projection, event)?;
    if let Event::Commit {..} = event {
        let i = projection.turns.len()-1;
        let turn = &projection.turns[i];
        projection.by_id.insert(turn.id.clone(), i);
        projection.by_nonce.insert(turn.nonce.clone(), i);
        projection.pending += 1;
    } else if let Some(i) = index {
        let after = projection.pending_turn(&projection.turns[i]);
        match (before, after) {
            (Some(false), true) => projection.pending += 1,
            (Some(true), false) => projection.pending -= 1,
            _ => {},
        }
        if complete && !projection.complete(&projection.turns[i]) { projection.cursor = projection.cursor.min(i); }
    }
    projection.advance();
    Ok(())
}

fn apply_inner(projection: &mut Projection, event: &Event) -> io::Result<()> {
    match event {
        Event::Snapshot { projection: saved } => {
            if !projection.session.is_empty() || !opaque(&saved.session) { return Err(observe::invalid()); }
            let mut next = saved.clone();
            next.rebuild()?;
            *projection = next;
        }
        Event::RecoverSilent { turn_id } => {
            let index = projection.index(turn_id)?;
            let turn = &mut projection.turns[index];

            if turn.legacy_attempted { return Err(observe::invalid()); }
            turn.silent_replay = true;
            if turn.presentation.is_none() {
                turn.presentation = Some(Payload::Assistant { spans: vec![], terminal: Terminal::PresentationUnknown });
            }
        }
        Event::NoRenderCancel { turn_id } => {
            let index = projection.index(turn_id)?;
            let turn = &mut projection.turns[index];
            if turn.rendered || turn.presentation_uncertain
                || turn.presentation.is_some() || turn.legacy_attempted || turn.acknowledged { return Err(observe::invalid()); }
            turn.silent_replay = true;
            turn.presentation = Some(Payload::Assistant { spans: vec![], terminal: Terminal::Interrupted });
            projection.cancel_waiting.insert(turn_id.clone());
        }
        Event::Presentation { turn_id, payload } => {
            payload.validate()?;
            let index = projection.index(turn_id)?;
            let turn = &mut projection.turns[index];
            if turn.owner != Some(Owner::Front) || !turn.rendered || turn.presentation.is_some()
                || !matches!(payload, Payload::Assistant { .. }) { return Err(observe::invalid()); }
            turn.presentation = Some(payload.clone());
            projection.release_cancels_before(index);
        }
        Event::MirrorPrepared { item } => {
            item.validate()?;
            if next_observation(projection)?.as_ref() != Some(item)
                || projection.observations.len() != projection.receipts.len() { return Err(observe::invalid()); }
            projection.observations.push(item.clone());
        }
        Event::MirrorAccepted { receipt } => {
            let index = projection.mirror_replay.unwrap_or(projection.receipts.len());
            let item = projection.observations.get(index).ok_or_else(observe::invalid)?;
            if item.receipt() != *receipt { return Err(observe::invalid()); }
            let turn_index = projection.index(&item.turn_id)?;
            if index == projection.receipts.len() {
                projection.receipted.insert(receipt.event_id.clone(), index);
                projection.receipts.push(receipt.clone());
                if matches!(item.payload, Payload::User { .. }) {
                    projection.turns[turn_index].acknowledged = true;
                }
            }
            if projection.mirror_replay.is_some() {
                projection.mirror_replay = (index + 1 < projection.receipts.len()).then_some(index + 1);
            }
        }
        Event::MirrorConflict => projection.mirror_blocked = true,
        Event::MirrorResume { snapshot } => {
            let version = usize::try_from(snapshot.version).map_err(|_| observe::invalid())?;
            let tip = if version == 0 { Some(String::new()) } else { projection.observations.get(version - 1).map(|r| r.receipt().digest) };
            if snapshot.session != projection.session || tip.as_deref() != Some(snapshot.tip.as_str()) { return Err(observe::invalid()); }
            if version > projection.receipts.len() {
                let item=projection.observations.get(version-1).ok_or_else(observe::invalid)?;
                if version!=projection.receipts.len()+1 { return Err(observe::invalid()); }
                let turn_index = projection.index(&item.turn_id)?;
                let receipt = item.receipt();
                projection.receipted.insert(receipt.event_id.clone(), projection.receipts.len());
                projection.receipts.push(receipt);
                if matches!(item.payload,Payload::User { .. }) {
                    projection.turns[turn_index].acknowledged=true;
                }
            }
            projection.mirror_replay = (version < projection.receipts.len()).then_some(version);
            projection.mirror_blocked = false;
        }
        Event::MirrorMemory { receipt } => {
            if !projection.receipts.contains(&receipt.receipt) || receipt.items.len() > 128
                || receipt.items.iter().any(|(id, rev)| !observe::opaque(id) || *rev == 0)
                || projection.memory.iter().any(|r| r.receipt.event_id == receipt.receipt.event_id) { return Err(observe::invalid()); }
            projection.memory.push(receipt.clone());
        }
        Event::Session { session } => {
            if !projection.session.is_empty() || !opaque(session) {
                return Err(invalid("invalid or duplicate conversation session"));
            }
            projection.session.clone_from(session);
        }
        Event::Commit { nonce, input } => {
            if projection.session.is_empty() || !opaque(nonce) || input.trim().is_empty() || input.len() > MAX_INPUT {
                return Err(invalid("invalid committed input"));
            }
            if projection.by_nonce.contains_key(nonce) {
                return Err(invalid("duplicate commit record"));
            }
            if projection.pending >= MAX_PENDING {
                return Err(invalid("conversation outbox capacity exhausted"));
            }
            let identity = hash(&(&projection.session, nonce))?;
            projection.turns.push(Turn {
                id: format!("turn-{identity}"), response_id: format!("reply-{identity}"),
                observation_id: format!("observe-{identity}"), nonce: nonce.clone(), input: input.clone(),
                owner: None, rendered: false, presentation_uncertain: false, legacy_attempted: false,
                backend_response: None, acknowledged: false, presentation: None, silent_replay: false,
            });
        }
        _ => {
            let turn_id = match event {
                Event::Select { turn_id, .. } | Event::FirstRendered { turn_id }
                | Event::LegacySend { turn_id } | Event::LegacyAccepted { turn_id, .. }
                | Event::ObserveAck { turn_id } => turn_id,
                _ => unreachable!(),
            };
            let index = projection.index(turn_id)?;
            let turn = &mut projection.turns[index];
            match event {
                Event::Select { owner, .. } => {
                    if no_render_cancel(turn) { return Err(observe::invalid()); }
                    if *owner == Owner::Front && projection.mirror_blocked { return Err(observe::invalid()); }

                    let allowed = turn.owner.is_none() || turn.owner == Some(*owner)
                        || (turn.owner == Some(Owner::Front) && *owner == Owner::Legacy
                            && !turn.rendered && !turn.legacy_attempted && !turn.acknowledged
                            && turn.presentation.is_none() && !turn.silent_replay);
                    let allowed = allowed && !(turn.presentation_uncertain && turn.owner != Some(*owner));
                    if !allowed { return Err(invalid("conversation owner already fixed")); }
                    turn.owner = Some(*owner);
                }
                Event::FirstRendered { .. } => {
                    if turn.owner != Some(Owner::Front) || turn.legacy_attempted || turn.presentation_uncertain || turn.presentation.is_some() {
                        return Err(invalid("only the front owner can expose its first sample"));
                    }
                    turn.rendered = true;

                    projection.release_cancels_before(index);
                }
                Event::LegacySend { .. } => {
                    if turn.owner != Some(Owner::Legacy) || turn.rendered || turn.legacy_attempted {
                        return Err(invalid("legacy delivery already attempted or wrong owner"));
                    }
                    turn.legacy_attempted = true;
                }
                Event::LegacyAccepted { backend_response, .. } => {
                    if !turn.legacy_attempted || turn.owner != Some(Owner::Legacy) || !opaque(backend_response) {
                        return Err(invalid("invalid legacy acceptance"));
                    }
                    if turn.backend_response.as_ref().is_some_and(|id| id != backend_response) {
                        return Err(invalid("legacy response identity conflict"));
                    }
                    turn.backend_response = Some(backend_response.clone());
                    turn.acknowledged = true;
                    projection.release_cancels_before(index);
                }
                Event::ObserveAck { .. } => {
                    if turn.owner != Some(Owner::Front) || !turn.rendered {
                        return Err(invalid("observation not eligible"));
                    }
                    turn.acknowledged = true;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

fn no_render_cancel(turn: &Turn) -> bool {
    !turn.rendered && turn.silent_replay
        && matches!(&turn.presentation, Some(Payload::Assistant { spans, terminal: Terminal::Interrupted }) if spans.is_empty())
}
fn presentation_id(turn: &Turn) -> String { format!("presentation-{}", turn.response_id) }

fn next_observation(p: &Projection) -> io::Result<Option<Observation>> {
    if p.mirror_blocked { return Err(observe::invalid()); }
    if let Some(index) = p.mirror_replay { return Ok(p.observations.get(index).cloned()); }
    if p.observations.len() > p.receipts.len() { return Ok(p.observations.last().cloned()); }
    for turn in &p.turns[p.cursor..] {
        if p.cancel_waiting.contains(&turn.id) { return Ok(None); }
        if !turn.silent_replay { match turn.owner {
            Some(Owner::Legacy) if turn.backend_response.is_some() => continue,
            Some(Owner::Front) if turn.rendered => {},
            _ => return Ok(None),
        }}
        let (event_id, payload) = if !p.receipted.contains_key(&turn.observation_id) {
            (turn.observation_id.clone(), Payload::User { input: turn.input.clone() })
        } else if !p.receipted.contains_key(&presentation_id(turn)) {
            let Some(payload) = turn.presentation.clone() else { return Ok(None) };
            (presentation_id(turn), payload)
        } else { continue };
        let item = Observation { session: p.session.clone(), event_id, turn_id: turn.id.clone(),
            response_id: turn.response_id.clone(), version: p.receipts.len() as u64 + 1,
            parent: p.receipts.last().map(|r| r.digest.clone()).unwrap_or_default(), payload };
        item.validate()?;
        return Ok(Some(item));
    }
    Ok(None)
}

impl Store {
    pub fn open(path: &Path, session: &str) -> io::Result<Self> {
        if !opaque(session) { return Err(invalid("invalid conversation session identity")); }
        let lock_deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);

        let lock_path = path.with_extension("lock");
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(lock_path)?;
        let lm = lock.metadata()?;
        if !lm.is_file() || lm.nlink()!=1 || lm.uid()!=unsafe {libc::geteuid()} || lm.mode()&0o077!=0 {
            return Err(observe::invalid());
        }
        lock_exclusive(&lock, lock_deadline)?;
        let mut file = OpenOptions::new().read(true).append(true).create(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0 || metadata.len() > MAX_LOG_BYTES {
            return Err(invalid("conversation log ownership, links, mode or size refused"));
        }

        lock_exclusive(&file, lock_deadline)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut projection = Projection::default();
        let (mut sequence, mut digest, mut valid_bytes) = (0, String::new(), 0);
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            if line.last() != Some(&b'\n') { break; }
            let record: Record = serde_json::from_slice(line).map_err(io::Error::other)?;
            if record.schema != 1 || record.sequence != sequence + 1 || record.previous != digest
                || record.digest != hash(&(record.schema, record.sequence, &record.previous, &record.event))? {
                return Err(invalid("conversation record integrity failed"));
            }
            apply(&mut projection, &record.event)?;
            sequence = record.sequence;
            digest = record.digest;
            valid_bytes += line.len();
        }
        if !projection.session.is_empty() && projection.session != session {
            return Err(invalid("conversation belongs to another session"));
        }
        for turn in &mut projection.turns {
            if turn.owner == Some(Owner::Front) && !turn.rendered && !no_render_cancel(turn) {
                turn.presentation_uncertain = true;
            }
        }

        if valid_bytes < bytes.len() {
            file.set_len(valid_bytes as u64)?;
            file.sync_all()?;
        }

        projection.cancel_waiting.clear();
        let recovered = projection.turns.iter().map(|t|t.id.clone()).collect();
        let mut store = Self { replay_owner: std::sync::Arc::new(()), file, _lock: lock, recovered, path: path.into(), projection, sequence, digest, poisoned: false };
        if store.sequence == 0 {
            store.append(Event::Session { session: session.into() })?;
            File::open(path.parent().ok_or_else(|| invalid("conversation log needs a parent"))?)?.sync_all()?;
        }
        Ok(store)
    }

    fn append(&mut self, event: Event) -> io::Result<()> {
        if self.poisoned { return Err(invalid("conversation write uncertain; reopen and reconcile")); }
        let sequence = self.sequence.checked_add(1).ok_or_else(|| invalid("conversation sequence exhausted"))?;
        let digest = hash(&(1u32, sequence, &self.digest, &event))?;
        let record = Record { schema: 1, sequence, previous: self.digest.clone(), event, digest: digest.clone() };
        let mut bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
        bytes.push(b'\n');
        if self.file.metadata()?.len() + bytes.len() as u64 > MAX_LOG_BYTES {
            return Err(invalid("conversation log full; snapshot migration required"));
        }

        apply(&mut self.projection, &record.event)?;
        self.poisoned = true;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        self.sequence = sequence;
        self.digest = digest;
        self.poisoned = false;
        Ok(())
    }

    pub fn compact(&mut self) -> io::Result<()> {
        if self.poisoned { return Err(observe::invalid()); }
        let event = Event::Snapshot { projection: self.projection.clone() };
        let digest = hash(&(1u32, 1u64, "", &event))?;
        let mut bytes = serde_json::to_vec(&Record { schema:1, sequence:1, previous:String::new(), event, digest:digest.clone() })?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_LOG_BYTES { return Err(observe::invalid()); }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = self.path.with_extension(format!("compact-{}-{n}",std::process::id()));
        self.poisoned = true;
        let mut output = OpenOptions::new().create_new(true).write(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW|libc::O_CLOEXEC).open(&temp)?;
        output.write_all(&bytes)?; output.sync_all()?;
        std::fs::rename(&temp,&self.path)?;
        File::open(self.path.parent().ok_or_else(observe::invalid)?)?.sync_all()?;
        let file = OpenOptions::new().read(true).append(true).custom_flags(libc::O_NOFOLLOW|libc::O_CLOEXEC).open(&self.path)?;
        lock_exclusive(&file, std::time::Instant::now() + std::time::Duration::from_millis(250))?;
        self.file=file; self.sequence=1; self.digest=digest; self.poisoned=false;
        Ok(())
    }
    pub fn reconcile_without_speech(&mut self, id: &str) -> io::Result<()> {
        let turn=self.turn(id)?;
        if !self.recovered.contains(id) { return Err(observe::invalid()); }
        if turn.silent_replay { return Ok(()); }
        self.append(Event::RecoverSilent { turn_id:id.into() })
    }

    pub fn open_persisted(dir: &Path, expected_session: &str) -> io::Result<Self> {
        if !opaque(expected_session) { return Err(observe::invalid()); }
        use std::io::BufRead;
        let path=dir.join("conversation-v2.jsonl");
        let mut existing=OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_CLOEXEC).open(&path);
        let session=match &mut existing {
            Ok(file) => {
                let mut line=Vec::new();
                std::io::BufReader::new(file).take(MAX_LOG_BYTES).read_until(b'\n', &mut line)?;
                if line.last() != Some(&b'\n') { expected_session.to_string() } else {
                    let row:Record=serde_json::from_slice(&line).map_err(io::Error::other)?;
                    match row.event { Event::Session {session}=>session, Event::Snapshot {projection}=>projection.session,
                        _=>return Err(observe::invalid()) }
                }
            }
            Err(e) if e.kind()==io::ErrorKind::NotFound => {
                expected_session.to_string()
            }
            Err(_)=>return Err(observe::invalid()),
        };
        if session!=expected_session { return Err(observe::invalid()); }
        Self::open(&path,&session)
    }
    pub fn session(&self) -> &str { &self.projection.session }

    pub fn cancel_before_render(&mut self, id: &str) -> io::Result<()> {
        if no_render_cancel(self.turn(id)?) { return Ok(()); }
        self.append(Event::NoRenderCancel { turn_id: id.into() })
    }

    pub fn ready_silent_cancels(&self, window: &mut SilentWindow) -> SilentBatch {
        let mut batch = SilentBatch { turns: Vec::new(), inspected: 0, more: false };
        if self.poisoned { return batch; }
        if !window.owner.as_ref().is_some_and(|owner| std::sync::Arc::ptr_eq(owner, &self.replay_owner)) {
            window.owner = Some(self.replay_owner.clone()); window.next = 0;
        }
        while batch.inspected < SILENT_BATCH {
            let Some(turn) = self.turns().get(window.next) else { return batch; };
            batch.inspected += 1;
            if no_render_cancel(turn) {
                if self.projection.cancel_waiting.contains(&turn.id) { return batch; }
                batch.turns.push(turn.clone());
            } else if !turn.rendered && !turn.legacy_attempted && !turn.acknowledged && turn.presentation.is_none() {
                return batch;
            }
            window.next += 1;
        }
        batch.more = window.next < self.turns().len();
        batch
    }
    pub fn finish_rendered(&mut self, id: &str, spans: Vec<Span>, terminal: Terminal) -> io::Result<()> {
        let payload = Payload::Assistant { spans, terminal };
        if self.turn(id)?.presentation.as_ref() == Some(&payload) { return Ok(()); }
        self.append(Event::Presentation { turn_id: id.into(), payload })
    }
    pub fn front_admission_allowed(&self) -> bool {
        !self.poisoned && !self.projection.mirror_blocked
            && self.turns().iter().filter(|t|t.owner==Some(Owner::Front)||t.silent_replay).count()*2+2<=observe::MAX_EVENTS
            && self.file.metadata().is_ok_and(|m|m.len()<32<<20)
            && !self.turns().iter().any(|t| !t.silent_replay && (t.presentation_uncertain
                || (self.recovered.contains(&t.id) && t.presentation.is_none()
                    && (!t.acknowledged || t.owner==Some(Owner::Front)))))
    }
    pub fn for_front(dir: &Path, session: &str) -> FrontStore {
        match Self::open_persisted(dir,session) {
            Ok(store)=>FrontStore::Ready(Box::new(store)),
            Err(_)=>FrontStore::LegacyOnly { message:"Conversation sync unavailable; local front is disabled." },
        }
    }
    pub fn prepare_observation(&mut self) -> io::Result<Option<Observation>> {
        if self.poisoned { return Err(observe::invalid()); }
        let next = next_observation(&self.projection)?;
        if let Some(item) = &next {
            if self.projection.mirror_replay.is_none() && self.projection.observations.len() == self.projection.receipts.len() {
                self.append(Event::MirrorPrepared { item: item.clone() })?;
            }
        }
        Ok(next)
    }
    pub fn mark_mirror_conflict(&mut self) -> io::Result<()> {
        if self.projection.mirror_blocked { return Ok(()); }
        self.append(Event::MirrorConflict)
    }
    pub fn accept_observation(&mut self, receipt: Receipt) -> io::Result<()> {
        if self.poisoned || self.projection.mirror_blocked { return Err(observe::invalid()); }
        if self.projection.mirror_replay.is_none() && self.projection.receipted.get(&receipt.event_id)
            .is_some_and(|i| self.projection.receipts[*i] == receipt) { return Ok(()); }
        if let Err(e) = self.append(Event::MirrorAccepted { receipt }) {
            if !self.poisoned { self.mark_mirror_conflict()?; }
            return Err(e);
        }
        Ok(())
    }
    pub fn reconcile_mirror(&mut self, snapshot: Snapshot) -> io::Result<()> {
        if let Err(e) = self.append(Event::MirrorResume { snapshot }) {
            if !self.poisoned { self.mark_mirror_conflict()?; }
            return Err(e);
        }
        Ok(())
    }
    pub fn accept_memory_receipt(&mut self, receipt: MemoryReceipt) -> io::Result<()> {
        if self.projection.memory.contains(&receipt) { return Ok(()); }
        self.append(Event::MirrorMemory { receipt })
    }
    pub fn memory_outcome(&self, event_id: &str) -> Option<observe::MemoryOutcome> {
        if self.poisoned { return None; }
        self.projection.memory.iter().find(|r| r.receipt.event_id==event_id).map(|r| r.outcome)
    }
    pub fn memory_synced(&self, event_id: &str) -> bool {
        !self.poisoned && self.projection.memory.iter().any(|r| r.receipt.event_id == event_id && r.outcome == observe::MemoryOutcome::Stored)
    }

    pub fn turns(&self) -> &[Turn] { if self.poisoned { &[] } else { &self.projection.turns } }

    fn turn(&self, id: &str) -> io::Result<&Turn> {
        if self.poisoned { return Err(invalid("conversation write uncertain; reopen and reconcile")); }
        self.projection.index(id).map(|i| &self.projection.turns[i])
    }

    pub fn commit(&mut self, nonce: &str, input: &str) -> io::Result<Turn> {
        if self.poisoned { return Err(invalid("conversation write uncertain; reopen and reconcile")); }
        if let Some(index) = self.projection.by_nonce.get(nonce) {
            let turn = &self.projection.turns[*index];
            return if turn.input == input { Ok(turn.clone()) } else { Err(invalid("input nonce conflict")) };
        }
        self.append(Event::Commit { nonce: nonce.into(), input: input.into() })?;
        Ok(self.turns().last().expect("successful commit appended a turn").clone())
    }

    pub fn select_owner(&mut self, id: &str, owner: Owner) -> io::Result<()> {
        if self.turn(id)?.owner == Some(owner) { return Ok(()); }
        if owner == Owner::Front && !self.front_admission_allowed() { return Err(observe::invalid()); }
        self.append(Event::Select { turn_id: id.into(), owner })
    }

    pub fn first_rendered(&mut self, id: &str) -> io::Result<()> {
        if self.turn(id)?.rendered { return Ok(()); }
        self.append(Event::FirstRendered { turn_id: id.into() })
    }

    pub fn pending_observations(&self) -> Vec<Turn> {
        if self.poisoned { return Vec::new(); }
        self.turns().iter().filter(|turn| turn.owner == Some(Owner::Front) && turn.rendered && !turn.acknowledged)
            .cloned().collect()
    }

    pub fn ack_observation(&mut self, id: &str) -> io::Result<()> {
        let turn = self.turn(id)?;
        if turn.owner != Some(Owner::Front) || !turn.rendered { return Err(invalid("observation not eligible")); }
        if turn.acknowledged { return Ok(()); }
        self.append(Event::ObserveAck { turn_id: id.into() })
    }

    pub fn begin_legacy_send(&mut self, id: &str) -> io::Result<bool> {
        let turn = self.turn(id)?;
        if turn.owner != Some(Owner::Legacy) { return Err(invalid("wrong legacy owner")); }
        if turn.legacy_attempted { return Ok(false); }
        if self.recovered.contains(id) || turn.silent_replay { return Err(observe::invalid()); }
        self.append(Event::LegacySend { turn_id: id.into() })?;
        Ok(true)
    }

    pub fn accept_legacy(&mut self, id: &str, backend_response: &str) -> io::Result<()> {
        if self.turn(id)?.backend_response.as_deref() == Some(backend_response) { return Ok(()); }
        self.append(Event::LegacyAccepted { turn_id: id.into(), backend_response: backend_response.into() })
    }
}

#[cfg(test)]
mod io_failure_tests {
    use super::*;
    #[test]
    fn uncertain_append_requires_reopen_even_for_idempotent_mutations() {
        let target = std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string());
        let dir = std::path::PathBuf::from(target).join(format!("h5a-io-failure-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        let mut store = Store::open(&path, "io-test").unwrap();
        let turn = store.commit("stable", "A durable public input.").unwrap();
        store.select_owner(&turn.id, Owner::Front).unwrap();
        store.first_rendered(&turn.id).unwrap();

        store.file = File::open(&path).unwrap();
        assert!(store.commit("uncertain", "No success receipt.").is_err());
        assert!(store.select_owner(&turn.id, Owner::Front).is_err());
        assert!(store.first_rendered(&turn.id).is_err());
        assert!(store.ack_observation(&turn.id).is_err());
        assert!(store.pending_observations().is_empty());
        assert!(store.turns().is_empty(), "uncertain in-memory mutations must stay hidden");
        drop(store);
        let recovered = Store::open(&path, "io-test").unwrap();
        assert_eq!(recovered.turns().len(), 1);
        assert_eq!(recovered.pending_observations().len(), 1);
    }
    #[test]
    fn opening_waits_for_a_fork_barrier_to_release_inherited_log_locks() {
        use std::os::fd::FromRawFd;
        use std::sync::mpsc;
        let dir = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
            .join(format!("h5-fork-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); let path = dir.join("log.jsonl");
        let store = Store::open(&path, "fork-test").unwrap();
        let (mut ready, mut release) = ([0; 2], [0; 2]);
        assert_eq!(unsafe { libc::pipe(ready.as_mut_ptr()) }, 0);
        assert_eq!(unsafe { libc::pipe(release.as_mut_ptr()) }, 0);
        let pid = unsafe { libc::fork() }; assert!(pid >= 0);
        if pid == 0 {

            unsafe {
                libc::close(ready[0]); libc::close(release[1]);
                let mark = 1u8; libc::write(ready[1], (&mark as *const u8).cast(), 1);
                let mut mark = 0u8; libc::read(release[0], (&mut mark as *mut u8).cast(), 1);
                libc::_exit(0);
            }
        }
        unsafe { libc::close(ready[1]); libc::close(release[0]); }
        let mut ready = unsafe { File::from_raw_fd(ready[0]) };
        let mut release = unsafe { File::from_raw_fd(release[1]) };
        let mut fd = libc::pollfd { fd:ready.as_raw_fd(), events:libc::POLLIN, revents:0 };
        let child_ready = unsafe {libc::poll(&mut fd,1,2000)} > 0;
        let mut mark = [0u8]; let ready_result = if child_ready { ready.read_exact(&mut mark) } else { Err(invalid("fork readiness deadline")) };
        drop(store);
        let (started, start) = mpsc::sync_channel(1);
        let waiter = std::thread::spawn(move || {
            LOCK_CONTENDED.with(|slot| *slot.borrow_mut() = Some(Box::new(move || { let _ = started.send(()); })));
            Store::open(&path, "fork-test")
        });
        let contention = start.recv_timeout(std::time::Duration::from_secs(2));
        let released = release.write_all(&[1]); drop(release);
        let mut status = 0; let waited = unsafe {libc::waitpid(pid,&mut status,0)};
        let reopened = waiter.join().unwrap();
        assert!(ready_result.is_ok() && contention.is_ok() && released.is_ok());
        assert_eq!(waited,pid); assert_eq!(status,0);
        assert!(reopened.is_ok(), "a transient fork holder must not disable the front");
    }
    #[test]
    fn a_real_second_log_holder_is_refused_within_the_bounded_open_deadline() {
        let dir = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
            .join(format!("h5-held-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap(); let path = dir.join("log.jsonl");
        let holder = Store::open(&path, "holder").unwrap(); let start = std::time::Instant::now();
        let result = Store::open(&path, "holder"); let elapsed = start.elapsed();
        assert!(result.is_err()); assert!(elapsed >= std::time::Duration::from_millis(200));
        assert!(elapsed < std::time::Duration::from_secs(1));
        drop(holder); assert!(Store::open(&path,"holder").is_ok());
    }

    #[test] fn non_contention_lock_error_is_immediate() {
        use std::os::fd::FromRawFd;
        let mut pipe=[0;2];assert_eq!(unsafe{libc::pipe(pipe.as_mut_ptr())},0);
        let file=unsafe{File::from_raw_fd(pipe[0])};let _write=unsafe{File::from_raw_fd(pipe[1])};
        let start=std::time::Instant::now();
        let err=lock_exclusive(&file,start+std::time::Duration::from_millis(250)).unwrap_err();
        assert_ne!(err.kind(),io::ErrorKind::WouldBlock);assert!(start.elapsed()<std::time::Duration::from_millis(100));
    }
    #[test] fn both_locks_share_one_deadline_without_busy_spinning() {
        use std::time::{Instant,Duration};
        fn cpu() -> u128 {let mut t=libc::timespec {tv_sec:0,tv_nsec:0};assert_eq!(unsafe{libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID,&mut t)},0);t.tv_sec as u128*1_000_000_000+t.tv_nsec as u128}
        let dir=std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string())).join(format!("h5-two-locks-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();let path=dir.join("log.jsonl");drop(Store::open(&path,"two-locks").unwrap());
        let side=File::open(path.with_extension("lock")).unwrap();let data=File::open(&path).unwrap();
        lock_exclusive(&side,Instant::now()+Duration::from_millis(250)).unwrap();
        lock_exclusive(&data,Instant::now()+Duration::from_millis(250)).unwrap();
        let (tx,rx)=std::sync::mpsc::sync_channel(1);
        LOCK_CONTENDED.with(|slot|*slot.borrow_mut()=Some(Box::new(move || {tx.send(()).unwrap();})));
        let holder=std::thread::spawn(move || {rx.recv_timeout(Duration::from_secs(2)).unwrap();std::thread::sleep(Duration::from_millis(120));drop(side);});
        let start=Instant::now();let before=cpu();let result=Store::open(&path,"two-locks");let used=cpu()-before;let elapsed=start.elapsed();
        holder.join().unwrap();drop(data);assert!(result.is_err());
        assert!(elapsed>=Duration::from_millis(200));assert!(elapsed<Duration::from_millis(340),"shared deadline: {elapsed:?}");
        assert!(used<80_000_000,"lock wait burned {used}ns of thread CPU");
    }

}

#[cfg(test)]
thread_local! { static LOCK_CONTENDED: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }

fn lock_exclusive(file: &File, deadline: std::time::Instant) -> io::Result<()> {
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 { return Ok(()); }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::WouldBlock { return Err(error); }
        #[cfg(test)]
        LOCK_CONTENDED.with(|slot| { let hook = slot.borrow_mut().take(); if let Some(hook) = hook { hook(); } });
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() { return Err(error); }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(2)));
    }
}
