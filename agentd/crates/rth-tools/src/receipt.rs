

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {

    Ok,

    Refused,

    Failed,

    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub task_id: String,

    pub call_seq: u64,

    pub call_id: String,

    pub tool: String,

    pub args_sha256: String,
    pub profile_sha256: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_sha256: Option<String>,
    pub result_chars: u64,
    pub ms: u64,
    pub utc_ms: u64,
}

pub trait Receipts: Send + Sync {
    fn record(&self, r: &Receipt) -> std::io::Result<()>;
}

#[derive(Default)]
pub struct Memory(pub std::sync::Mutex<Vec<Receipt>>);

impl Receipts for Memory {
    fn record(&self, r: &Receipt) -> std::io::Result<()> {
        self.0.lock().unwrap().push(r.clone());
        Ok(())
    }
}

impl Memory {
    pub fn all(&self) -> Vec<Receipt> {
        self.0.lock().unwrap().clone()
    }
}
