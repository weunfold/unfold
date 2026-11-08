

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentIdentity {
    pub deployment_id: String,

    pub artifact_id: String,

    pub manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceIdentity {
    pub instance_id: String,
    pub session_id: String,

    pub state_schema: u32,

    pub journal_bytes_at_boot: u64,

    pub context: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_restore: Option<ContextReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextReport {

    pub status: String,

    pub method: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage_since: Option<u64>,

    pub turns: u64,

    pub tail_records: u64,

    pub truncated_turns: u64,
    pub detail: String,
}

pub const CONTEXT_RESTORED: &str = "restored";
pub const CONTEXT_PARTIAL: &str = "partial";
pub const CONTEXT_NONE: &str = "not_restored";

pub const CONTEXT_STATUSES: &[&str] = &[CONTEXT_RESTORED, CONTEXT_PARTIAL, CONTEXT_NONE, CONTEXT_NOT_RESTORED];

pub const CONTEXT_NOT_RESTORED: &str = "history_retained_model_context_not_restored";

pub const ENV_STATE: &str = "RTH_STATE";
pub const ENV_MANIFEST_SHA256: &str = "RTH_MANIFEST_SHA256";
pub const ENV_DEPLOYMENT_ID: &str = "RTH_DEPLOYMENT_ID";

pub const MANIFEST_FILE: &str = "manifest.json";

pub const AGENT_FILE: &str = "rth-agentd";

pub const MARK_PREFIX: &[u8; 9] = b"RTH-MARK:";
pub const MARK_LEN: usize = 9 + 13 + 4 + 10 + 4 + 1;

pub const fn build_mark(state_schema: u16, protocol: u16) -> [u8; MARK_LEN] {
    let mut out = [0u8; MARK_LEN];
    let parts: [&[u8]; 3] = [b"RTH-MARK:", b"state_schema=", b";protocol="];
    let mut i = 0;
    let mut k = 0;
    while k < parts[0].len() {
        out[i] = parts[0][k];
        i += 1;
        k += 1;
    }
    k = 0;
    while k < parts[1].len() {
        out[i] = parts[1][k];
        i += 1;
        k += 1;
    }
    i = write4(&mut out, i, state_schema);
    k = 0;
    while k < parts[2].len() {
        out[i] = parts[2][k];
        i += 1;
        k += 1;
    }
    i = write4(&mut out, i, protocol);
    out[i] = b';';
    out
}

const fn write4(out: &mut [u8; MARK_LEN], at: usize, v: u16) -> usize {
    let digits = [(v / 1000) % 10, (v / 100) % 10, (v / 10) % 10, v % 10];
    let mut k = 0;
    while k < 4 {
        out[at + k] = b'0' + digits[k] as u8;
        k += 1;
    }
    at + 4
}

pub fn find_marks(bytes: &[u8]) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + MARK_LEN <= bytes.len() {
        if &bytes[i..i + MARK_PREFIX.len()] == MARK_PREFIX
            && let Some(m) = parse_mark(&bytes[i..i + MARK_LEN])
        {
            found.push(m);
        }
        i += 1;
    }
    found.dedup();
    found
}

fn parse_mark(m: &[u8]) -> Option<(u16, u16)> {
    let s = std::str::from_utf8(m).ok()?;
    let rest = s.strip_prefix("RTH-MARK:state_schema=")?;
    let (schema, rest) = rest.split_at(4);
    let rest = rest.strip_prefix(";protocol=")?;
    let (proto, rest) = rest.split_at(4);
    if rest != ";" {
        return None;
    }
    Some((schema.parse().ok()?, proto.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_round_trips_and_is_found_inside_noise() {
        let mark = build_mark(1, 2);
        assert_eq!(&mark, b"RTH-MARK:state_schema=0001;protocol=0002;");
        let mut blob = vec![7u8; 100];
        blob.extend_from_slice(b"RTH-MARK:garbage");
        blob.extend_from_slice(&mark);
        blob.extend_from_slice(&[0u8; 50]);
        assert_eq!(find_marks(&blob), vec![(1, 2)]);
        assert!(find_marks(b"no mark here").is_empty());
    }
}
