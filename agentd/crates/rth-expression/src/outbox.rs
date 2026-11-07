

use std::io;
use std::path::{Path, PathBuf};

use rth_self::store::{append, read_lines, sha256_hex, write_atomic};
use serde::{Deserialize, Serialize};

use crate::policy::{Attempt, Candidate, Destination, Egress, Kind};
use crate::reason::{Message, Reason};

pub const LOG_FILE: &str = "expression.jsonl";
pub const UNSENT_FILE: &str = "expression-unsent.jsonl";

pub const HELD_TTL_MS: u64 = 24 * 60 * 60 * 1000;

pub const MAX_HELD: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Line {
    Attempt { id: String, kind: Kind, subject_id: String, utc_ms: u64, local_day: i64, destination: Option<Destination>, text_sha256: String },
    Surfaced { id: String, utc_ms: u64 },
    Expired { id: String, utc_ms: u64 },
    Forgotten { id: String, utc_ms: u64 },

    Dropped { id: String, utc_ms: u64 },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub id: String,
    pub kind: Kind,
    pub subject_id: String,
    pub title: String,
    pub text: String,

    pub reason_days: u32,
    pub memory_refs: Vec<String>,
    pub egress: Egress,
    pub held_since_utc_ms: u64,
}

impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("title_len", &self.title.len())
            .field("text_len", &self.text.len())
            .field("memory_refs", &self.memory_refs.len())
            .finish()
    }
}

impl Held {
    pub fn message(&self) -> Message {
        Message::new(self.title.clone(), self.text.clone(), Reason::of(self.kind, self.reason_days))
    }

    pub fn candidate(&self) -> Candidate {
        Candidate { kind: self.kind, subject_id: self.subject_id.clone(), egress: self.egress, held: true, reason_days: self.reason_days, data: vec![], memory_refs: self.memory_refs.clone() }
    }
}

#[derive(Debug)]
pub struct Outbox {
    log: PathBuf,
    unsent: PathBuf,
    attempts: Vec<Attempt>,
    held: Vec<Held>,
}

fn invalid(what: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

impl Outbox {

    pub fn open(dir: &Path) -> io::Result<Outbox> {
        let log = dir.join(LOG_FILE);
        let lines = read_lines(&log)?;
        if lines.corrupt {
            return Err(invalid(format!("{LOG_FILE}: an unreadable line before the last")));
        }
        if lines.torn {
            let bytes = std::fs::read(&log)?;
            let keep = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            let f = std::fs::OpenOptions::new().write(true).open(&log)?;
            f.set_len(keep as u64)?;
            f.sync_all()?;
        }
        let (mut attempts, mut open_ids, mut closed) = (vec![], std::collections::HashSet::new(), std::collections::HashSet::new());
        for (i, v) in lines.records.into_iter().enumerate() {
            match serde_json::from_value::<Line>(v).map_err(|e| invalid(format!("{LOG_FILE} line {}: {e}", i + 1)))? {
                Line::Attempt { id, kind, subject_id, utc_ms, local_day, destination, .. } => {
                    if destination.is_none() {
                        open_ids.insert(id);
                    }
                    attempts.push(Attempt { kind, subject_id, utc_ms, local_day });
                }
                Line::Surfaced { id, .. } | Line::Expired { id, .. } | Line::Forgotten { id, .. } | Line::Dropped { id, .. } => {
                    closed.insert(id);
                }
            }
        }
        let unsent = dir.join(UNSENT_FILE);

        let stored: Vec<Held> = read_lines(&unsent)?.records.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
        let n = stored.len();
        let held: Vec<Held> = stored.into_iter().filter(|h| open_ids.contains(&h.id) && !closed.contains(&h.id)).collect();
        let out = Outbox { log, unsent, attempts, held };
        if out.held.len() != n {
            out.rewrite()?;
        }
        Ok(out)
    }

    pub fn history(&self) -> &[Attempt] {
        &self.attempts
    }

    pub fn held(&self) -> &[Held] {
        &self.held
    }

    fn rewrite(&self) -> io::Result<()> {
        let mut bytes = vec![];
        for h in &self.held {
            bytes.extend(serde_json::to_vec(h).expect("a held message serializes"));
            bytes.push(b'\n');
        }
        write_atomic(&self.unsent, &bytes)
    }

    #[allow(clippy::too_many_arguments)]
    fn attempt(&mut self, id: &str, kind: Kind, subject_id: &str, utc_ms: u64, local_day: i64, destination: Option<Destination>, text: &str) -> io::Result<()> {
        let line = Line::Attempt { id: id.into(), kind, subject_id: subject_id.into(), utc_ms, local_day, destination, text_sha256: sha256_hex(text.as_bytes()) };
        append(&self.log, &line)?;
        self.attempts.push(Attempt { kind, subject_id: subject_id.into(), utc_ms, local_day });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_sent(&mut self, id: &str, kind: Kind, subject_id: &str, utc_ms: u64, local_day: i64, destination: Destination, text: &str) -> io::Result<()> {
        self.attempt(id, kind, subject_id, utc_ms, local_day, Some(destination), text)
    }

    pub fn hold(&mut self, h: Held, local_day: i64) -> io::Result<()> {
        self.attempt(&h.id, h.kind, &h.subject_id, h.held_since_utc_ms, local_day, None, &h.text)?;
        let now = h.held_since_utc_ms;
        self.held.push(h);
        while self.held.len() > MAX_HELD {
            let oldest = self.held.remove(0);
            append(&self.log, &Line::Expired { id: oldest.id, utc_ms: now })?;
        }
        self.rewrite()
    }

    fn close(&mut self, keep: impl Fn(&Held) -> bool, line: impl Fn(String) -> Line) -> io::Result<usize> {
        let (gone, kept): (Vec<Held>, Vec<Held>) = std::mem::take(&mut self.held).into_iter().partition(|h| !keep(h));
        self.held = kept;
        for h in &gone {
            append(&self.log, &line(h.id.clone()))?;
        }
        if !gone.is_empty() {
            self.rewrite()?;
        }
        Ok(gone.len())
    }

    pub fn expire(&mut self, now_utc_ms: u64) -> io::Result<usize> {
        self.close(|h| now_utc_ms < h.held_since_utc_ms.saturating_add(HELD_TTL_MS), |id| Line::Expired { id, utc_ms: now_utc_ms })
    }

    pub fn forget(&mut self, memory_id: &str, now_utc_ms: u64) -> io::Result<usize> {
        self.close(|h| !h.memory_refs.iter().any(|m| m == memory_id), |id| Line::Forgotten { id, utc_ms: now_utc_ms })
    }

    pub fn drop_forgotten(&mut self, now_utc_ms: u64, is_forgotten: impl Fn(&str) -> bool) -> io::Result<usize> {
        self.close(|h| !h.memory_refs.iter().any(|m| is_forgotten(m)), |id| Line::Forgotten { id, utc_ms: now_utc_ms })
    }

    pub fn drop_held(&mut self, id: &str, now_utc_ms: u64) -> io::Result<usize> {
        self.close(|h| h.id != id, |id| Line::Dropped { id, utc_ms: now_utc_ms })
    }

    pub fn take_oldest(&mut self, now_utc_ms: u64, is_forgotten: impl Fn(&str) -> bool) -> io::Result<Option<Held>> {
        self.expire(now_utc_ms)?;
        self.drop_forgotten(now_utc_ms, is_forgotten)?;
        if self.held.is_empty() {
            return Ok(None);
        }
        let h = self.held.remove(0);
        append(&self.log, &Line::Surfaced { id: h.id.clone(), utc_ms: now_utc_ms })?;
        self.rewrite()?;
        Ok(Some(h))
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
