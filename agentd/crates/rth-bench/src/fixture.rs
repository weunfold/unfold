

use std::collections::BTreeMap;

use rth_protocol::Role;
use serde::{Deserialize, Serialize};

pub const FIXTURE_FORMAT: &str = "rth-bench.fixture/v1";
pub const TASK_FORMAT: &str = "rth-bench.task/v1";
pub const SUITE_FORMAT: &str = "rth-bench.suite/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Dev,
    Promotion,
    Probe,
}

impl Split {
    pub fn as_str(self) -> &'static str {
        match self {
            Split::Dev => "dev",
            Split::Promotion => "promotion",
            Split::Probe => "probe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaskRef {
    pub id: String,
    pub version: String,
}

impl std::fmt::Display for TaskRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.id, self.version)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub fixture_format: String,
    pub scenario_id: String,

    pub group_id: String,
    pub task: TaskRef,
    pub split: Split,

    pub feed_mode: String,

    pub reactive: bool,
    pub description: String,

    pub deadline_ms: u64,

    pub settle_ms: u64,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub expect: Expect,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    #[serde(default)]
    pub at_ms: Option<u64>,
    #[serde(default)]
    pub after_ms: Option<u64>,
    #[serde(default)]
    pub on: Option<Trigger>,
    #[serde(default)]
    pub delay_ms: u64,
    pub conn: String,
    #[serde(flatten)]
    pub op: Op,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {

    Attach { role: Role },

    ConnectNoAttach {},
    UserText { label: String, text: String },
    Cancel {
        #[serde(default)]
        label: Option<String>,
        target: Target,
    },

    Raw { label: String, line: String },

    Oversize { bytes: usize },
    Disconnect {},
    Wait { until: Until, deadline_ms: u64 },

    UserAudio {
        label: String,
        tone_hz: f64,
        ms: u64,
        #[serde(default = "default_chunk_ms")]
        chunk_ms: u64,
        #[serde(default)]
        skip_chunks: Vec<u64>,
    },

    Speaker {},

    BargeIn {
        #[serde(default)]
        label: Option<String>,
    },
}

fn default_chunk_ms() -> u64 {
    40
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::Attach { .. } => "attach",
            Op::ConnectNoAttach {} => "connect_no_attach",
            Op::UserText { .. } => "user_text",
            Op::Cancel { .. } => "cancel",
            Op::Raw { .. } => "raw",
            Op::Oversize { .. } => "oversize",
            Op::Disconnect {} => "disconnect",
            Op::Wait { .. } => "wait",
            Op::UserAudio { .. } => "user_audio",
            Op::Speaker {} => "speaker",
            Op::BargeIn { .. } => "barge_in",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {

    Request(String),

    Nth(usize),

    UnknownId(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Trigger {
    ResponseStarted {
        request: String,
        #[serde(default)]
        conn: Option<String>,
    },
    Delta {
        request: String,
        count: usize,
        #[serde(default)]
        conn: Option<String>,
    },
    Terminal {
        request: String,
        #[serde(default)]
        conn: Option<String>,
    },

    Audio {
        request: String,
        count: usize,
        #[serde(default)]
        conn: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cond", rename_all = "snake_case")]
pub enum Until {

    Terminal { requests: Vec<String> },

    Closed {},

    Rejected { count: usize },

    ForMs { ms: u64 },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Expect {

    #[serde(default)]
    pub roles: BTreeMap<String, Role>,

    #[serde(default)]
    pub completed: Vec<String>,

    #[serde(default)]
    pub cancelled: Vec<String>,

    #[serde(default)]
    pub contract: BTreeMap<String, String>,

    #[serde(default)]
    pub overlap: Vec<(String, String)>,
    #[serde(default)]
    pub rejections: Vec<RejectionExpect>,

    #[serde(default)]
    pub rejected_total: BTreeMap<String, usize>,
    #[serde(default)]
    pub closed_by_agent: Vec<String>,

    #[serde(default)]
    pub no_response: Vec<String>,

    #[serde(default)]
    pub unobserved_ok: Vec<String>,

    #[serde(default)]
    pub audio: Vec<String>,

    #[serde(default)]
    pub ingress_gaps: BTreeMap<String, u64>,

    #[serde(default)]
    pub flush: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionExpect {
    pub conn: String,

    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default)]
    pub reason_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDef {
    pub task_format: String,
    pub task_id: String,
    pub version: String,
    pub purpose: String,
    pub family: String,

    pub capabilities: Vec<String>,
    pub input_protocol: InputProtocol,
    pub provenance: String,
    pub license: String,

    pub fixture_manifest_sha256: String,
    pub fixtures: Vec<FixtureEntry>,
    pub scorer: ScorerRef,
    pub output_schema: String,
    pub metrics: Vec<MetricDef>,
    pub normalization: String,
    pub missing_timeout_error_policy: String,
    pub critical_checks: Vec<String>,
    pub mandatory_checks: Vec<String>,
    pub variant_labels: Vec<String>,

    #[serde(default)]
    pub response_contract: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputProtocol {
    pub name: String,
    pub version: u16,
    pub transport: String,
    pub feed_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureEntry {
    pub scenario_id: String,
    pub split: Split,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScorerRef {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDef {
    pub name: String,
    pub direction: Direction,
    pub unit: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    HigherIsBetter,
    LowerIsBetter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suite {
    pub suite_format: String,
    pub suite_id: String,
    pub epoch: u32,
    pub description: String,

    pub policy_approval_id: String,
    pub tasks: Vec<SuiteTask>,
    pub probes: Vec<SuiteProbe>,
    pub split_manifest: SplitManifest,
    pub envelopes: Vec<Envelope>,
    pub aggregate: Aggregate,
    pub statistics: Statistics,
    pub guest: GuestEnvelope,
    pub max_submissions_per_epoch: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteTask {
    pub task: TaskRef,
    pub weight: f64,

    pub floor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteProbe {
    pub task: TaskRef,
    pub scenario_id: String,
    pub repeats: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitManifest {
    pub dev: Vec<String>,
    pub promotion: Vec<String>,

    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub metric: String,
    pub stat: String,
    pub max: f64,
    pub unit: String,

    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Aggregate {
    pub method: String,
    pub selection_split: Split,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statistics {
    pub percentile: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestEnvelope {
    pub cpus: u8,
    pub memory_mib: u32,
}
