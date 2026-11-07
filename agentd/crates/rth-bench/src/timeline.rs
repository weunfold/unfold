

use rth_protocol::{Envelope, Event, ResponseStatus, Role};
use serde::{Deserialize, Serialize};

use crate::fixture::{Split, TaskRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosedBy {

    Peer,

    Runner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "rec", rename_all = "snake_case")]
pub enum Rec {
    Header {
        scenario_id: String,
        task: TaskRef,
        split: Split,
        rep: u32,
        fixture_sha256: String,
        run_tag: String,

        origin_ns: u64,
    },

    Sent {
        t_ns: u64,
        conn: String,
        step: Option<usize>,
        label: Option<String>,
        env: Option<Envelope>,

        raw: Option<String>,
        raw_bytes: Option<usize>,
    },

    Recv { t_ns: u64, conn: String, env: Envelope },
    RecvInvalid { t_ns: u64, conn: String, line: String, error: String },
    Closed { t_ns: u64, conn: String, by: ClosedBy, detail: String },

    Step { t_ns: u64, step: usize, op: String, conn: String, sched_ns: u64 },

    Note { t_ns: u64, conn: Option<String>, code: String, detail: String },
}

impl Rec {
    pub fn t_ns(&self) -> u64 {
        match self {
            Rec::Header { origin_ns, .. } => *origin_ns,
            Rec::Sent { t_ns, .. }
            | Rec::Recv { t_ns, .. }
            | Rec::RecvInvalid { t_ns, .. }
            | Rec::Closed { t_ns, .. }
            | Rec::Step { t_ns, .. }
            | Rec::Note { t_ns, .. } => *t_ns,
        }
    }

    pub fn conn(&self) -> Option<&str> {
        match self {
            Rec::Sent { conn, .. }
            | Rec::Recv { conn, .. }
            | Rec::RecvInvalid { conn, .. }
            | Rec::Closed { conn, .. }
            | Rec::Step { conn, .. } => Some(conn),
            Rec::Note { conn, .. } => conn.as_deref(),
            Rec::Header { .. } => None,
        }
    }

    pub fn recv_on<'a>(&'a self, on: &str) -> Option<(&'a Envelope, u64)> {
        match self {
            Rec::Recv { conn, env, t_ns } if conn == on => Some((env, *t_ns)),
            _ => None,
        }
    }

    pub fn recv(&self) -> Option<(&str, &Envelope, u64)> {
        match self {
            Rec::Recv { conn, env, t_ns } => Some((conn, env, *t_ns)),
            _ => None,
        }
    }
}

pub fn request_id(run_tag: &str, scenario_id: &str, rep: u32, label: &str) -> String {
    format!("{run_tag}-{scenario_id}-r{rep}-{label}")
}

pub fn response_for(recs: &[Rec], request_id: &str) -> Option<String> {
    recs.iter().find_map(|r| match r.recv() {
        Some((_, Envelope { event: Event::ResponseStarted { response_id, request_id: q, .. }, .. }, _)) if q == request_id => {
            Some(response_id.clone())
        }
        _ => None,
    })
}

pub fn started_on(recs: &[Rec], conn: &str) -> Vec<(String, String, u64)> {
    recs.iter()
        .filter_map(|r| match r.recv_on(conn) {
            Some((Envelope { event: Event::ResponseStarted { response_id, request_id, .. }, .. }, t)) => {
                Some((response_id.clone(), request_id.clone(), t))
            }
            _ => None,
        })
        .collect()
}

pub fn started_time(recs: &[Rec], conn: &str, response_id: &str) -> Option<u64> {
    started_on(recs, conn).into_iter().find(|(r, _, _)| r == response_id).map(|(_, _, t)| t)
}

pub fn deltas(recs: &[Rec], conn: &str, response_id: &str) -> Vec<(u64, String)> {
    recs.iter()
        .filter_map(|r| match r.recv_on(conn) {
            Some((Envelope { event: Event::AgentDelta { response_id: id, text }, .. }, t)) if id == response_id => {
                Some((t, text.clone()))
            }
            _ => None,
        })
        .collect()
}

pub fn audio(recs: &[Rec], conn: &str, response_id: &str) -> Vec<(u64, u64, u64, u64)> {
    use base64::Engine;
    recs.iter()
        .filter_map(|r| match r.recv_on(conn) {
            Some((Envelope { event: Event::AgentAudio { response_id: rid, chunk, sample_offset, data, .. }, .. }, t)) if rid == response_id => {
                let n = base64::engine::general_purpose::STANDARD.decode(data).map(|b| b.len() as u64 / 2).unwrap_or(0);
                Some((t, *chunk, *sample_offset, n))
            }
            _ => None,
        })
        .collect()
}

pub fn terminals(recs: &[Rec], conn: &str, response_id: &str) -> Vec<(u64, ResponseStatus, String)> {
    recs.iter()
        .filter_map(|r| match r.recv_on(conn) {
            Some((Envelope { id, event: Event::AgentDone { response_id: rid, status }, .. }, t)) if rid == response_id => {
                Some((t, *status, id.clone()))
            }
            _ => None,
        })
        .collect()
}

pub fn first_terminal(recs: &[Rec], conn: &str, response_id: &str) -> Option<(u64, ResponseStatus)> {
    terminals(recs, conn, response_id).first().map(|(t, s, _)| (*t, *s))
}

pub fn rejected_on(recs: &[Rec], conn: &str) -> Vec<(Option<String>, String, u64)> {
    recs.iter()
        .filter_map(|r| match r.recv_on(conn) {
            Some((Envelope { event: Event::Rejected { reason }, cause, .. }, t)) => Some((cause.clone(), reason.clone(), t)),
            _ => None,
        })
        .collect()
}

pub fn closed_by_peer(recs: &[Rec], conn: &str) -> Option<u64> {
    recs.iter().find_map(|r| match r {
        Rec::Closed { conn: c, by: ClosedBy::Peer, t_ns, .. } if c == conn => Some(*t_ns),
        _ => None,
    })
}

pub fn attachment_ids(recs: &[Rec], conn: &str) -> Vec<String> {
    recs.iter()
        .filter_map(|r| match r {
            Rec::Sent { conn: c, env: Some(Envelope { event: Event::Attach { attachment_id, .. }, .. }), .. } if c == conn => {
                Some(attachment_id.clone())
            }
            _ => None,
        })
        .collect()
}

pub fn granted_role(recs: &[Rec], conn: &str) -> Option<Role> {
    let ids = attachment_ids(recs, conn);
    recs.iter().find_map(|r| match r.recv_on(conn) {
        Some((Envelope { event: Event::Ready { attachment_id, role }, .. }, _)) if ids.contains(attachment_id) => Some(*role),
        _ => None,
    })
}

pub fn sent_label<'a>(recs: &'a [Rec], label: &str) -> Option<(&'a str, u64, Option<&'a Envelope>)> {
    recs.iter().find_map(|r| match r {
        Rec::Sent { conn, t_ns, label: Some(l), env, .. } if l == label => Some((conn.as_str(), *t_ns, env.as_ref())),
        _ => None,
    })
}

pub fn sort(recs: &mut [Rec]) {
    recs.sort_by_key(|r| match r {
        Rec::Header { .. } => (0, 0),
        other => (1, other.t_ns()),
    });
}
