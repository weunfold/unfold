
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;

pub const SCHEMA: &str = "rth.observe/v1";
pub const MAX_EVENTS: usize = 4096;
pub fn invalid() -> io::Error {
    io::Error::other("observe contract conflict")
}
pub fn opaque(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub first: u64,
    pub end: u64,
    pub text: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Terminal {
    Completed,
    Interrupted,
    Failed,
    PresentationUnknown,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    User {
        input: String,
    },
    Assistant {
        spans: Vec<Span>,
        terminal: Terminal,
    },
}
impl Payload {
    pub fn text(&self) -> String {
        match self {
            Self::User { input } => input.clone(),
            Self::Assistant { spans, .. } => spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        match self {
            Self::User { input } if input.trim().is_empty() || input.len() > 8192 => {
                return Err(invalid());
            }
            Self::Assistant { spans, terminal } => {
                if *terminal == Terminal::PresentationUnknown && !spans.is_empty() {
                    return Err(invalid());
                }
                if spans.len() > 128 || self.text().len() > 8192 {
                    return Err(invalid());
                }
                let mut end = 0;
                for s in spans {
                    if s.first < end || s.first >= s.end || s.text.trim().is_empty() {
                        return Err(invalid());
                    }
                    end = s.end;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub session: String,
    pub event_id: String,
    pub turn_id: String,
    pub response_id: String,
    pub version: u64,
    pub parent: String,
    pub payload: Payload,
}
impl Observation {
    pub fn validate(&self) -> io::Result<()> {
        if ![
            &self.session,
            &self.event_id,
            &self.turn_id,
            &self.response_id,
        ]
        .iter()
        .all(|s| opaque(s))
            || self.version == 0
            || self.version > MAX_EVENTS as u64
            || (self.version == 1 && !self.parent.is_empty())
            || (self.version > 1
                && (self.parent.len() != 64 || !self.parent.bytes().all(|b| b.is_ascii_hexdigit())))
        {
            return Err(invalid());
        }
        self.payload.validate()
    }
    pub fn receipt(&self) -> Receipt {
        let bytes = serde_json::to_vec(&(SCHEMA, self)).expect("typed observation serializes");
        Receipt {
            session: self.session.clone(),
            event_id: self.event_id.clone(),
            version: self.version,
            digest: format!("{:x}", Sha256::digest(bytes)),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub session: String,
    pub event_id: String,
    pub version: u64,
    pub digest: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub session: String,
    pub version: u64,
    pub tip: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryReceipt {
    pub receipt: Receipt,
    pub items: Vec<(String, u64)>,
    pub outcome: MemoryOutcome,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryOutcome {
    Stored,
    Filtered,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Observe { observation: Observation },
    Lookup { session: String, event_id: String },
    Resume { session: String },
    Memory { session: String, event_id: String },
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Accepted { receipt: Receipt },
    Snapshot { snapshot: Snapshot },
    Memory { receipt: MemoryReceipt },
    Pending,
    Conflict,
    Unavailable,
}
