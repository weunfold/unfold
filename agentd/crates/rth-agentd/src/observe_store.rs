

use rth_protocol::observe::{self, MemoryReceipt, Observation, Payload, Receipt, Snapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, OpenOptionsExt},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    fn geteuid() -> u32;
}
#[cfg(target_os = "macos")]
const NOFOLLOW: i32 = 0x0100;
#[cfg(not(target_os = "macos"))]
const NOFOLLOW: i32 = 0o400000;
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Entry {
    Binding { session: String },
    Observed { item: Observation },
    Memory { receipt: MemoryReceipt },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Line {
    schema: u32,
    previous: String,
    entry: Entry,
    digest: String,
}
pub struct Receiver {
    file: File,
    session: String,
    history: Vec<Observation>,
    memory: Vec<MemoryReceipt>,
    digest: String,
    poisoned: bool,
}
fn digest(previous: &str, entry: &Entry) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(1u32, previous, entry)).expect("typed record"))
    )
}
impl Receiver {
    pub fn open(path: &Path, session: &str) -> io::Result<Self> {
        if !observe::opaque(session) {
            return Err(observe::invalid());
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .mode(0o600)
            .custom_flags(NOFOLLOW)
            .open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file()
            || meta.nlink() != 1
            || meta.uid() != unsafe { geteuid() }
            || meta.mode() & 0o077 != 0
            || meta.len() > 64 << 20
        {
            return Err(observe::invalid());
        }
        if unsafe { flock(file.as_raw_fd(), 2 | 4) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut receiver = Self {
            file,
            session: String::new(),
            history: vec![],
            memory: vec![],
            digest: String::new(),
            poisoned: false,
        };
        let mut valid = 0;
        for line in bytes.split_inclusive(|c| *c == b'\n') {
            if line.last() != Some(&b'\n') {
                break;
            }
            let row: Line = serde_json::from_slice(line)?;
            if row.schema != 1
                || row.previous != receiver.digest
                || row.digest != digest(&row.previous, &row.entry)
            {
                return Err(observe::invalid());
            }
            receiver.fold(&row.entry)?;
            receiver.digest = row.digest;
            valid += line.len();
        }
        if !receiver.session.is_empty() && receiver.session != session {
            return Err(observe::invalid());
        }
        if valid < bytes.len() {
            receiver.file.set_len(valid as u64)?;
            receiver.file.sync_all()?;
        }
        if receiver.session.is_empty() {
            receiver.append(Entry::Binding {
                session: session.into(),
            })?;
            File::open(path.parent().ok_or_else(observe::invalid)?)?.sync_all()?;
        }
        Ok(receiver)
    }
    fn validate(&self, item: &Observation) -> io::Result<()> {
        item.validate()?;
        if item.session != self.session
            || self.history.len() >= observe::MAX_EVENTS
            || item.version != self.history.len() as u64 + 1
            || item.parent != self.snapshot().tip
        {
            return Err(observe::invalid());
        }
        let prior = self.history.iter().find(|r| r.turn_id == item.turn_id);
        match &item.payload {
            Payload::User { .. } if prior.is_some() => return Err(observe::invalid()),
            Payload::Assistant { .. } => {
                if !prior.is_some_and(|r| {
                    matches!(r.payload, Payload::User { .. }) && r.response_id == item.response_id
                }) || self.history.iter().any(|r| {
                    r.turn_id == item.turn_id && matches!(r.payload, Payload::Assistant { .. })
                }) {
                    return Err(observe::invalid());
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn fold(&mut self, entry: &Entry) -> io::Result<()> {
        match entry {
            Entry::Binding { session } => {
                if !self.session.is_empty() || !observe::opaque(session) {
                    return Err(observe::invalid());
                }
                self.session = session.clone();
            }
            Entry::Observed { item } => {
                self.validate(item)?;
                if self.history.iter().any(|r| r.event_id == item.event_id) {
                    return Err(observe::invalid());
                }
                self.history.push(item.clone());
            }
            Entry::Memory { receipt } => {
                if self.lookup(&receipt.receipt.event_id).as_ref() != Some(&receipt.receipt)
                    || self.memory(&receipt.receipt.event_id).is_some()
                    || receipt.items.len() > 128
                    || receipt
                        .items
                        .iter()
                        .any(|(id, v)| !observe::opaque(id) || *v == 0)
                {
                    return Err(observe::invalid());
                }
                self.memory.push(receipt.clone());
            }
        }
        Ok(())
    }
    fn append(&mut self, entry: Entry) -> io::Result<()> {
        if self.poisoned {
            return Err(observe::invalid());
        }

        let mut probe = Self {
            file: self.file.try_clone()?,
            session: self.session.clone(),
            history: self.history.clone(),
            memory: self.memory.clone(),
            digest: self.digest.clone(),
            poisoned: false,
        };
        probe.fold(&entry)?;
        let next = digest(&self.digest, &entry);
        let mut bytes = serde_json::to_vec(&Line {
            schema: 1,
            previous: self.digest.clone(),
            entry,
            digest: next.clone(),
        })?;
        bytes.push(b'\n');
        if self.file.metadata()?.len() + bytes.len() as u64 > 64 << 20 {
            return Err(observe::invalid());
        }
        self.poisoned = true;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?;
        self.session = probe.session;
        self.history = probe.history;
        self.memory = probe.memory;
        self.digest = next;
        self.poisoned = false;
        Ok(())
    }
    pub fn accept(&mut self, item: Observation) -> io::Result<Receipt> {
        if self.poisoned {
            return Err(observe::invalid());
        }
        if let Some(old) = self.history.iter().find(|r| r.event_id == item.event_id) {
            return if old == &item {
                Ok(old.receipt())
            } else {
                Err(observe::invalid())
            };
        }
        self.validate(&item)?;
        let receipt = item.receipt();
        self.append(Entry::Observed { item })?;
        Ok(receipt)
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            session: self.session.clone(),
            version: self.history.len() as u64,
            tip: self
                .history
                .last()
                .map(|r| r.receipt().digest)
                .unwrap_or_default(),
        }
    }
    pub fn history(&self) -> &[Observation] {
        &self.history
    }
    pub fn lookup(&self, id: &str) -> Option<Receipt> {
        if self.poisoned {
            return None;
        }
        self.history
            .iter()
            .find(|r| r.event_id == id)
            .map(Observation::receipt)
    }
    pub fn next_memory(&self) -> Option<Observation> {
        if self.poisoned {
            return None;
        }
        self.history
            .iter()
            .find(|r| self.memory(&r.event_id).is_none())
            .cloned()
    }
    #[cfg(test)]
    pub fn pending_memory(&self) -> Vec<Observation> {
        if self.poisoned {
            return vec![];
        }
        self.history
            .iter()
            .filter(|r| self.memory(&r.event_id).is_none())
            .cloned()
            .collect()
    }
    pub fn memory(&self, id: &str) -> Option<MemoryReceipt> {
        if self.poisoned {
            return None;
        }
        self.memory
            .iter()
            .find(|r| r.receipt.event_id == id)
            .cloned()
    }
    pub fn record_memory(&mut self, receipt: MemoryReceipt) -> io::Result<()> {
        if self.memory(&receipt.receipt.event_id).as_ref() == Some(&receipt) {
            return Ok(());
        }
        self.append(Entry::Memory { receipt })
    }
}
