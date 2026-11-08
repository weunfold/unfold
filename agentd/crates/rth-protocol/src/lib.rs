

use serde::{Deserialize, Serialize};

pub mod deploy;
pub mod observe;
#[cfg(feature = "io")]
pub mod io;

pub const PROTOCOL_VERSION: u16 = 2;
pub const AGENT_PORT: u16 = 7700;

pub const MAX_FRAME: usize = 64 * 1024;

pub const MAX_AUDIO_CHUNK_BYTES: usize = 16 * 1024;

pub const MIC_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Host,
    Guest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {

    Controller,

    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u16,
    pub id: String,
    pub producer: String,
    pub seq: u64,
    pub t_ns: u64,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<Received>,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Received {
    pub by: String,
    pub t_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {

    Hello {
        protocol: u16,
        agent: String,
        version: String,
        boot_id: String,
        utc_ms_at_origin: u64,
        journal: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backend: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deployment: Option<deploy::DeploymentIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instance: Option<deploy::InstanceIdentity>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_generation: Option<u64>,
    },

    Ready { attachment_id: String, role: Role },
    ObserveReady { schema: String, session: String },
    ObserveReply { reply: observe::Reply },
    ObserveStored { observation: observe::Observation },
    Heartbeat { uptime_ms: u64, attachments: u64 },

    ResponseStarted {
        response_id: String,
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utterance_id: Option<String>,
    },

    AgentOpener { response_id: String, text: String },
    AgentDelta { response_id: String, text: String },

    AgentReplace { response_id: String, text: String },
    AgentDone { response_id: String, status: ResponseStatus },

    ReplySpoken { response_id: String },

    AgentAudio { response_id: String, chunk: u64, sample_offset: u64, sample_rate: u32, format: String, data: String },

    Decision {
        decision_id: String,
        episode_id: String,
        trigger_event: String,
        context_rev: u64,
        candidate_schema: String,
        candidates: Vec<String>,
        selected: String,
        policy: String,
        latency_ns: u64,
        training_eligibility: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        utterance_id: Option<String>,
    },

    JournalFault { detail: String },
    Rejected { reason: String },
    Log { level: String, msg: String },

    AudioIngress { stream_id: String, chunks: u64, samples: u64, bytes: u64, gaps: u64, sha256: String },

    InputTranscript {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utterance_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utterance_seq: Option<u64>,
        #[serde(default, rename = "final", skip_serializing_if = "Option::is_none")]
        is_final: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        association: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        boundary: Option<String>,
    },

    ProviderSignal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        signal: String,
    },

    CommandAdmitted {
        command_id: String,
        generation: u64,
        status: CommandStatus,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        duplicate: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },

    MediaStored {
        ref_format: String,
        media: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
        sha256: String,
        bytes: u64,
        encoding: String,
        sample_rate: u32,
        channels: u16,
        chunk_first: u64,
        chunk_last: u64,
        sample_start: u64,
        sample_end: u64,
        first_event: String,
        last_event: String,
        clock: String,
        t_ns_first: u64,
        t_ns_last: u64,
        path: String,
    },

    ContextRestore {
        status: String,
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        watermark: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lineage_since: Option<u64>,
        turns: u64,
        tail_records: u64,
        truncated_turns: u64,
        detail: String,
    },

    ResponseDiagnostic {
        response_id: String,
        session_generation: u64,
        terminal_event_id: String,
        #[serde(default)]
        provider_status: Option<String>,
        #[serde(default)]
        provider_reason: Option<String>,
        terminal_status_mapped: ResponseStatus,
        outcome_source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        truncated: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sentence_case_applied: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history_guard_applied: Option<bool>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw_text_clipped: Option<bool>,
    },

    TasksReady { schema: String },

    TaskState { task_id: String, record: serde_json::Value },

    MemoryAct { act_id: String, state: String, act: serde_json::Value },

    MemoryRecall { call_id: String, count: u64, total: u64, recall: serde_json::Value },

    ResponseProvenance {
        response_id: String,
        backend: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_sha256: Option<String>,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_sha256: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_kind: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_token_ms: Option<u64>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        spec_version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template_version: Option<String>,
    },

    MemoryDecided { response_id: String, decision: String, receipt: serde_json::Value },

    EffectHeld { effect_id: String, effect_kind: String, rule: String, at: Option<serde_json::Value>, args_sha256: String, token_sha256: String, expires_utc_ms: u64 },

    EffectCard { effect_id: String, effect_kind: String, token: String, summary: String, why: String, expires_utc_ms: u64 },

    EffectReleased { effect_id: String, effect_kind: String, args_sha256: String, result: serde_json::Value },

    EffectRefused { effect_id: String, effect_kind: String, reason: String, at: Option<serde_json::Value>, why: String },

    SpendStatus {
        month: String,
        spent_pico: u64,
        cap_pico: u64,
        capped: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by_purpose: Option<std::collections::BTreeMap<String, u64>>,
    },

    TurnSummary {
        response_id: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        opener_ms: Option<u64>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_delta_ms: Option<u64>,

        #[serde(default)]
        lang: Option<String>,
        #[serde(default)]
        memory: TurnMemory,
        #[serde(default)]
        shown_text: ShownText,

        #[serde(default)]
        effects: Vec<TurnEffect>,

        #[serde(default)]
        fixed_reply: Option<String>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pt_gender: Option<serde_json::Value>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        pt_opener: Option<serde_json::Value>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        spoken: Option<String>,
    },

    TurnMemoryText { response_id: String, items: Vec<MemoryText> },

    MemoryTextRevoked { memory_id: String },

    TaskToolCall { task_id: String, receipt: serde_json::Value },

    FolderGrant { event: String, record: serde_json::Value },

    FolderGrants { grants: serde_json::Value },

    TaskReport { task_id: String, text: String, result_sha256: String },

    PartState { parts: serde_json::Value },

    SelfState { request_id: String, state: serde_json::Value },

    SelfResult { request_id: String, result: serde_json::Value },

    ExpressionSent {
        expression_id: String,
        reach_kind: String,
        destination: String,
        subject_id: String,
        reason_kind: String,
        text_sha256: String,
        #[serde(default)]
        memory_refs: Vec<MemoryRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spend: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<serde_json::Value>,
    },

    ExpressionDeliver {
        expression_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_id: Option<String>,
        reach_kind: String,
        destination: String,
        message: ExpressionMessage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        held_since_utc_ms: Option<u64>,
    },

    DshNotice {
        event_kind: String,
        agent_name: String,
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change_id: Option<String>,
    },

    DshWatchStatus { on: bool, connected: bool, subscriptions: u32, drafts: u32 },

    PeopleAck { frame_id: String, person_id: String, record_rev: u64 },

    PeopleRefused { frame_id: String, reason: String },

    PeopleList {
        people: Vec<PersonRow>,
        #[serde(default)]
        forgotten: Vec<ForgottenRow>,
        prompts: Vec<String>,
    },

    PeopleProposal {
        proposal_kind: ProposalKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        turn_id: String,
    },

    Attach { protocol: u16, attachment_id: String, role: Role },

    UserText {
        request_id: String,
        text: String,
        #[serde(default)]
        speaker: TurnSpeaker,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_voice: Option<ReplyVoice>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<Effort>,

        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "reply_max_tokens")]
        reply_max_tokens: Option<u32>,
    },

    CommandSubmitted {
        command_id: String,
        session_id: String,
        expected_generation: u64,
        text: String,
        #[serde(default)]
        speaker: TurnSpeaker,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_voice: Option<ReplyVoice>,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<Effort>,

        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "reply_max_tokens")]
        reply_max_tokens: Option<u32>,
    },
    ObserveRequest { request: observe::Request },
    Cancel { response_id: String },

    SelfGet { request_id: String },

    SelfSet { request_id: String, base_revision: String, spec: serde_json::Value },

    ProposalDecide { request_id: String, proposal_id: String, approve: bool },

    HorizonDecide { request_id: String, horizon_id: String, approve: bool },

    ConsentSet {
        request_id: String,
        channel: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recipient: Option<String>,
        grant: bool,
        copy_version: u32,
    },

    Presence {
        locked: bool,
        display_asleep: bool,
        idle_s: u64,
        output_busy: bool,
        last_origin: String,
        utc_offset_min: i32,
        frontmost: bool,
        notify_ok: bool,
    },

    ExpressionOutcome {
        expression_id: String,
        outcome: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },

    UserAudio {
        stream_id: String,
        chunk: u64,
        sample_offset: u64,
        sample_rate: u32,
        format: String,
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        digest: Option<AudioDigest>,
    },

    UserAudioEnd {
        stream_id: String,
        chunks: u64,
        samples: u64,
        dropped_chunks: u64,
        #[serde(default)]
        speaker: TurnSpeaker,
    },

    Playback {
        response_id: String,
        epoch: u64,
        phase: PlaybackPhase,
        played_samples: u64,
        dropped_samples: u64,
        sample_rate: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clear_ns: Option<u64>,
    },

    TaskUpdate { task_id: String, update: serde_json::Value },

    ExecutorStatus { status: serde_json::Value },

    PeopleEnrol {
        frame_id: String,
        person_id: String,
        name: String,
        relation: String,
        owner: bool,
        modality: PeopleModality,
        consent_copy_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<OwnerGrant>,
    },

    PeopleUpdate {
        frame_id: String,
        person_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recognize: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<OwnerGrant>,
    },

    PeopleForget {
        frame_id: String,
        person_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modality: Option<PeopleModality>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<OwnerGrant>,
    },

    PeopleUndo {
        frame_id: String,
        person_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth: Option<OwnerGrant>,
    },

    PeopleCorrect {
        frame_id: String,
        turn_id: String,
        person_id: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        owner_confirmed: bool,
    },

    PeopleListRequest { frame_id: String },

    PeoplePresence {
        present: Vec<String>,
        unknown_faces: u32,
        at_utc_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "who", rename_all = "snake_case")]
pub enum TurnSpeaker {
    Person { person_id: String },
    Unknown,
    Uncertain,
    #[default]
    OwnerKeyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyVoice {
    pub dictated: bool,
    pub spoken: SpokenReply,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Auto,
    #[default]
    Off,
    Low,
    High,
    Max,
}

pub const REPLY_MAX_TOKENS_LIMIT: u32 = 1_048_576;

fn reply_max_tokens<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let n = u32::deserialize(d)?;
    if !(1..=REPLY_MAX_TOKENS_LIMIT).contains(&n) {
        return Err(serde::de::Error::custom(format!("reply_max_tokens must be 1-{REPLY_MAX_TOKENS_LIMIT}, not {n}")));
    }
    Ok(Some(n))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpokenReply {
    FirstSentence,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerGrantMethod {
    TouchIdOrPassword,
    FaceLiveness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerGrant {
    pub grant_id: String,
    pub method: OwnerGrantMethod,
    pub at_utc_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeopleModality {
    Face,
    ReadAlong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonRow {
    pub person_id: String,
    pub name: String,
    pub relation: String,
    pub owner: bool,

    pub recognize: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgottenRow {
    pub person_id: String,
    pub name: String,
    pub forgotten_at_utc_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Introduce,
    Correct,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnMemory {

    #[serde(default)]
    pub decision: Option<String>,

    #[serde(default)]
    pub used: Vec<MemoryRef>,

    #[serde(default)]
    pub sent_to_cloud: bool,

    #[serde(default)]
    pub picked: Vec<MemoryRef>,

    #[serde(default)]
    pub changed: Option<Vec<MemoryChange>>,

    #[serde(default)]
    pub blocked: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryText {
    pub memory_id: String,
    pub text: String,
}

impl std::fmt::Debug for MemoryText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryText({}, {} bytes)", self.memory_id, self.text.len())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressionMessage {

    pub title: String,
    pub text: String,

    pub reason: String,
}

impl std::fmt::Debug for ExpressionMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExpressionMessage({} + {} + {} bytes)", self.title.len(), self.text.len(), self.reason.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRef {
    pub memory_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryChange {
    pub memory_id: String,
    #[serde(default)]
    pub revision: Option<u64>,

    pub op: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShownText {

    #[serde(default)]
    pub history_guard: bool,

    #[serde(default)]
    pub markdown_stripped: bool,

    #[serde(default)]
    pub sentence_cased: bool,

    #[serde(default)]
    pub truncated: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnEffect {
    pub effect_id: String,
    pub effect_kind: String,

    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDigest {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackPhase {
    Started,
    Finished,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {

    Admitted,

    Dispatched,
    Completed,

    Cancelled,
    Failed,

    Interrupted,
}

impl CommandStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, CommandStatus::Admitted | CommandStatus::Dispatched)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Completed,
    Cancelled,
    Failed,
}

impl Event {

    pub fn sender(&self) -> Source {
        match self {
            Event::Attach { .. }
            | Event::UserText { .. }
            | Event::CommandSubmitted { .. }
            | Event::ObserveRequest { .. }
            | Event::Cancel { .. }
            | Event::SelfGet { .. }
            | Event::SelfSet { .. }
            | Event::ProposalDecide { .. }
            | Event::HorizonDecide { .. }
            | Event::ConsentSet { .. }
            | Event::Presence { .. }
            | Event::ExpressionOutcome { .. }
            | Event::UserAudio { .. }
            | Event::UserAudioEnd { .. }
            | Event::Playback { .. }
            | Event::TaskUpdate { .. }
            | Event::ExecutorStatus { .. }
            | Event::PeopleEnrol { .. }
            | Event::PeopleUpdate { .. }
            | Event::PeopleForget { .. }
            | Event::PeopleUndo { .. }
            | Event::PeopleCorrect { .. }
            | Event::PeopleListRequest { .. }
            | Event::PeoplePresence { .. } => Source::Host,
            _ => Source::Guest,
        }
    }
}

pub const MEDIA_REF_FORMAT: &str = "rth.media-ref/v1";

pub const KINDS: &[&str] = &[
    "hello",
    "ready",
    "observe_ready",
    "observe_reply",
    "observe_stored",
    "heartbeat",
    "response_started",
    "agent_opener",
    "agent_delta",
    "agent_replace",
    "agent_done",
    "reply_spoken",
    "agent_audio",
    "decision",
    "journal_fault",
    "rejected",
    "log",
    "audio_ingress",
    "input_transcript",
    "provider_signal",
    "command_admitted",
    "media_stored",
    "context_restore",
    "response_diagnostic",
    "tasks_ready",
    "task_state",
    "memory_act",
    "memory_recall",
    "response_provenance",
    "memory_decided",
    "effect_held",
    "effect_card",
    "effect_released",
    "effect_refused",
    "spend_status",
    "turn_summary",
    "turn_memory_text",
    "memory_text_revoked",
    "task_tool_call",
    "folder_grant",
    "folder_grants",
    "task_report",
    "part_state",
    "self_state",
    "self_result",
    "expression_sent",
    "expression_deliver",
    "dsh_notice",
    "dsh_watch_status",
    "people_ack",
    "people_refused",
    "people_list",
    "people_proposal",
    "attach",
    "user_text",
    "command_submitted",
    "observe_request",
    "cancel",
    "self_get",
    "self_set",
    "proposal_decide",
    "horizon_decide",
    "consent_set",
    "presence",
    "expression_outcome",
    "user_audio",
    "user_audio_end",
    "playback",
    "task_update",
    "executor_status",
    "people_enrol",
    "people_update",
    "people_forget",
    "people_undo",
    "people_correct",
    "people_list_request",
    "people_presence",
];

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Malformed(String),
    Version(u16),
    WrongSender,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Malformed(e) => write!(f, "malformed frame: {e}"),
            FrameError::Version(v) => write!(f, "protocol version {v}, expected {PROTOCOL_VERSION}"),
            FrameError::WrongSender => write!(f, "event not allowed from this sender"),
        }
    }
}

impl Envelope {
    pub fn to_line(&self) -> String {
        let mut line = serde_json::to_string(self).expect("envelope serializes");
        line.push('\n');
        line
    }

    pub fn parse_known(line: &str, expected: Source) -> Result<Option<Self>, FrameError> {
        match Self::parse_from(line, expected) {
            Ok(env) => Ok(Some(env)),
            Err(FrameError::Malformed(e)) => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return Err(FrameError::Malformed(e)) };
                let kind = v.get("kind").and_then(|k| k.as_str());
                let ver = v.get("v").and_then(|k| k.as_u64());
                let source = v.get("source").and_then(|s| serde_json::from_value::<Source>(s.clone()).ok());
                let header_ok = ["id", "producer"].iter().all(|k| v.get(*k).is_some_and(|x| x.is_string()))
                    && ["seq", "t_ns"].iter().all(|k| v.get(*k).is_some_and(|x| x.is_u64()));
                match (kind, ver, source) {
                    (Some(k), Some(ver), Some(src)) if header_ok && !KINDS.contains(&k) => {
                        if ver != PROTOCOL_VERSION as u64 {
                            Err(FrameError::Version(ver.min(u16::MAX as u64) as u16))
                        } else if src != expected {
                            Err(FrameError::WrongSender)
                        } else {
                            Ok(None)
                        }
                    }
                    _ => Err(FrameError::Malformed(e)),
                }
            }
            Err(other) => Err(other),
        }
    }

    pub fn parse_from(line: &str, expected: Source) -> Result<Self, FrameError> {
        let env: Envelope = serde_json::from_str(line).map_err(|e| FrameError::Malformed(e.to_string()))?;
        if env.v != PROTOCOL_VERSION {
            return Err(FrameError::Version(env.v));
        }
        if env.source != expected || env.event.sender() != expected {
            return Err(FrameError::WrongSender);
        }
        Ok(env)
    }
}

pub trait Clock: Send {
    fn now_ns(&self) -> u64;
}

#[derive(Clone, Copy)]
pub struct MonotonicClock(std::time::Instant);

impl MonotonicClock {
    pub fn start() -> Self {
        Self(std::time::Instant::now())
    }
}

impl Clock for MonotonicClock {
    fn now_ns(&self) -> u64 {
        self.0.elapsed().as_nanos() as u64
    }
}

pub struct Stamper {
    source: Source,
    producer: String,
    seq: u64,
    clock: Box<dyn Clock>,
}

impl Stamper {
    pub fn new(source: Source, producer: impl Into<String>, clock: Box<dyn Clock>) -> Self {
        Self { source, producer: producer.into(), seq: 0, clock }
    }

    pub fn producer(&self) -> &str {
        &self.producer
    }

    pub fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    pub fn stamp(&mut self, event: Event, cause: Option<String>) -> Envelope {
        self.seq += 1;
        self.envelope(self.seq, event, cause)
    }

    pub fn ephemeral(&self, event: Event) -> Envelope {
        self.envelope(0, event, None)
    }

    fn envelope(&self, seq: u64, event: Event, cause: Option<String>) -> Envelope {
        let id = if seq == 0 { format!("{}:~", self.producer) } else { format!("{}:{seq}", self.producer) };
        Envelope {
            v: PROTOCOL_VERSION,
            id,
            producer: self.producer.clone(),
            seq,
            t_ns: self.clock.now_ns(),
            source: self.source,
            cause,
            received: None,
            event,
        }
    }
}

pub fn producer_name(role: &str) -> String {
    format!("{role}-{:x}-{}", utc_ms(), std::process::id())
}

pub fn utc_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
#[path = "../testdata/memory-r8/lib.rs"]
#[allow(dead_code, unused_imports, clippy::all)]
mod r8;

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock(u64);
    impl Clock for FixedClock {
        fn now_ns(&self) -> u64 {
            self.0
        }
    }

    fn host() -> Stamper {
        Stamper::new(Source::Host, "host-a", Box::new(FixedClock(7)))
    }

    #[test]
    fn turn_summary_round_trips_and_a_bare_row_reads_as_nothing_known() {
        let full: Event = serde_json::from_str(r#"{"kind":"turn_summary","response_id":"r1","lang":"pt","memory":{"decision":"recall","used":[{"memory_id":"m-1","revision":2}],"sent_to_cloud":true,"changed":[],"blocked":null},"shown_text":{"history_guard":false,"markdown_stripped":true,"sentence_cased":false,"truncated":null},"effects":[{"effect_id":"e-1","effect_kind":"forget","state":"card"}]}"#).unwrap();
        let v = serde_json::to_value(&full).unwrap();
        assert_eq!(serde_json::to_value(serde_json::from_value::<Event>(v.clone()).unwrap()).unwrap(), v);
        assert_eq!((v["memory"]["used"][0]["memory_id"].as_str(), v["memory"]["sent_to_cloud"].as_bool(), v["effects"][0]["state"].as_str()), (Some("m-1"), Some(true), Some("card")));

        let bare: Event = serde_json::from_str(r#"{"kind":"turn_summary","response_id":"r1"}"#).unwrap();
        let Event::TurnSummary { lang, memory, shown_text, effects, .. } = bare else { panic!() };
        assert_eq!((lang, memory, shown_text, effects), (None, TurnMemory::default(), ShownText::default(), vec![]));
        assert!(!TurnMemory::default().sent_to_cloud && TurnMemory::default().changed.is_none());
    }

    #[test]
    fn blocked_reason_is_additive() {
        let old: TurnMemory = serde_json::from_str(r#"{"decision":"remember","blocked":"secret.password"}"#).unwrap();
        assert_eq!((old.blocked.as_deref(), old.blocked_reason), (Some("secret.password"), None));
        let none = serde_json::to_value(TurnMemory::default()).unwrap();
        assert!(none.get("blocked_reason").is_none(), "absent when nothing was blocked: {none}");
        let new = serde_json::to_value(TurnMemory { blocked: Some("secret.password".into()), blocked_reason: Some("secret".into()), ..TurnMemory::default() }).unwrap();
        assert_eq!(new["blocked_reason"], "secret");
    }

    #[test]
    fn route_nod_provenance_carries_the_device_and_an_old_row_still_reads() {

        let old: Event = serde_json::from_str(r#"{"kind":"response_provenance","response_id":"r1","backend":"local@granite","model":"granite","prompt":"unfold-local-text-v4.1"}"#).unwrap();
        assert!(matches!(&old, Event::ResponseProvenance { device_kind: None, config: None, fallback_reason: None, .. }), "{old:?}");

        let mid: Event = serde_json::from_str(r#"{"kind":"response_provenance","response_id":"r1","backend":"flash+local@granite","model":"deepseek","prompt":"p","config":{"path":"chat","temperature":1.0,"top_p":1.0,"thinking":false}}"#).unwrap();
        assert!(matches!(&mid, Event::ResponseProvenance { device_kind: None, config: Some(c), .. } if c.get("effort").is_none()), "{mid:?}");
        let new = Event::ResponseProvenance { response_id: "r1".into(), backend: "flash+local@granite".into(), model: "deepseek".into(), model_sha256: None, prompt: "p".into(), prompt_sha256: None, fallback_reason: None, provider_model: None, config: Some(serde_json::json!({"effort": "off", "thread": "main"})), device_kind: Some("Mac".into()), first_token_ms: Some(1336), spec_version: None, template_version: None };
        let line = serde_json::to_value(&new).unwrap();
        assert_eq!((line["device_kind"].as_str(), line["config"]["effort"].as_str(), line["config"]["thread"].as_str()), (Some("Mac"), Some("off"), Some("main")));
        assert_eq!(serde_json::to_value(serde_json::from_value::<Event>(line.clone()).unwrap()).unwrap(), line);
    }

    #[test]
    fn a7_the_self_frames_round_trip_through_the_envelope_with_numbers_intact() {
        let frames = [
            r#"{"kind":"self_get","request_id":"g1"}"#,
            r#"{"kind":"self_set","request_id":"s1","base_revision":"ab12","spec":{"schema_version":1,"name":{"current":"Lua"}}}"#,
            r#"{"kind":"proposal_decide","request_id":"p1","proposal_id":"prop-1","approve":true}"#,
            r#"{"kind":"consent_set","request_id":"c1","channel":"omni","recipient":"5511999999999@s.whatsapp.net","grant":true,"copy_version":1}"#,
            r#"{"kind":"consent_set","request_id":"c2","channel":"notification","grant":false,"copy_version":1}"#,
            r#"{"kind":"self_result","request_id":"s1","result":{"refused":{"field":"spec","code":"budget","reason":"r","need":2594,"max":2506}}}"#,
            r#"{"kind":"self_state","request_id":"g1","state":{"revision":"ab12","born_utc_ms":1793000000123,"prompt":{"local":{"est_tokens":2206,"bound":2506}}}}"#,
        ];
        for f in frames {
            let event: Event = serde_json::from_str(f).unwrap_or_else(|e| panic!("{f}: {e}"));

            let from = event.sender();
            assert_eq!(from == Source::Guest, f.contains(r#""kind":"self_"#) && (f.contains("self_result") || f.contains("self_state")), "{f}");
            let line = Stamper::new(from, "a", Box::new(FixedClock(7))).stamp(event, None).to_line();
            let back = Envelope::parse_from(line.trim_end(), from).unwrap();
            let other = if from == Source::Host { Source::Guest } else { Source::Host };
            assert_eq!(Envelope::parse_from(line.trim_end(), other).unwrap_err(), FrameError::WrongSender, "{f}");
            let want: serde_json::Value = serde_json::from_str(f).unwrap();
            let mut got = serde_json::to_value(&back.event).unwrap();
            assert_eq!(got, want, "{f}");
            got = serde_json::from_str(&line).unwrap();
            for (path, n) in [("/result/refused/need", 2594u64), ("/state/born_utc_ms", 1_793_000_000_123), ("/copy_version", 1)] {
                if let Some(v) = got.pointer(path) {
                    assert_eq!(v.as_u64(), Some(n), "{path} in {line}");
                }
            }
        }

        assert!(serde_json::from_str::<Event>(r#"{"kind":"consent_set","request_id":"c3","channel":"omni","grant":true}"#).is_err());
    }

    #[test]
    fn a8_horizon_decide_round_trips_as_a_host_command() {
        let f = r#"{"kind":"horizon_decide","request_id":"h1","horizon_id":"h-1793000000123-0","approve":false}"#;
        let event: Event = serde_json::from_str(f).unwrap();
        assert_eq!(event.sender(), Source::Host);
        let line = Stamper::new(Source::Host, "a", Box::new(FixedClock(7))).stamp(event, None).to_line();
        let back = Envelope::parse_from(line.trim_end(), Source::Host).unwrap();
        assert_eq!(serde_json::to_value(&back.event).unwrap(), serde_json::from_str::<serde_json::Value>(f).unwrap());
        assert_eq!(Envelope::parse_from(line.trim_end(), Source::Guest).unwrap_err(), FrameError::WrongSender);
        assert!(KINDS.contains(&"horizon_decide"));
        assert!(serde_json::from_str::<Event>(r#"{"kind":"horizon_decide","request_id":"h1","approve":true}"#).is_err(), "the horizon is named");
    }
    #[test]
    fn envelope_round_trips_with_flattened_kind() {
        let line = host().stamp(Event::UserText { request_id: "q1".into(), text: "hi".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None).to_line();
        assert!(line.contains(r#""kind":"user_text""#));
        let back = Envelope::parse_from(line.trim_end(), Source::Host).unwrap();
        assert_eq!((back.id.as_str(), back.t_ns), ("host-a:1", 7));
    }

    #[test]
    fn ids_differ_across_producers_even_with_equal_seq() {
        let a = host().stamp(Event::Cancel { response_id: "r".into() }, None);
        let b = Stamper::new(Source::Host, "host-b", Box::new(FixedClock(7))).stamp(Event::Cancel { response_id: "r".into() }, None);
        assert_eq!(a.seq, b.seq);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn rejects_wrong_version_and_spoofed_sender() {
        let mut env = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None);
        env.v = 65535;
        assert_eq!(Envelope::parse_from(env.to_line().trim_end(), Source::Host).unwrap_err(), FrameError::Version(65535));

        let mut spoof = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None);
        spoof.source = Source::Guest;
        assert_eq!(Envelope::parse_from(spoof.to_line().trim_end(), Source::Guest).unwrap_err(), FrameError::WrongSender);
    }

    #[test]
    fn hello_deployment_fields_are_additive() {

        let old = r#"{"v":2,"id":"guest-x:0","producer":"guest-x","seq":0,"t_ns":1,"source":"guest","kind":"hello","protocol":2,"agent":"rth-agentd","version":"0.1.0","boot_id":"x","utc_ms_at_origin":5,"journal":"/rth/journal.jsonl"}"#;
        let env = Envelope::parse_from(old, Source::Guest).unwrap();
        let Event::Hello { backend: None, deployment: None, instance: None, .. } = env.event else { panic!("{:?}", env.event) };

        let stamper = Stamper::new(Source::Guest, "guest-x", Box::new(FixedClock(1)));
        let hello = |deployment, instance| Event::Hello {
            protocol: PROTOCOL_VERSION,
            agent: "rth-agentd".into(),
            version: "0.1.0".into(),
            boot_id: "x".into(),
            utc_ms_at_origin: 5,
            journal: "j".into(),
            backend: Some("fake@v1".into()),
            deployment,
            instance,
            command_generation: None,
        };
        let bare = stamper.ephemeral(hello(None, None)).to_line();
        assert!(!bare.contains("deployment") && !bare.contains("instance"));
        let dep = deploy::DeploymentIdentity { deployment_id: "d1".into(), artifact_id: "a".repeat(64), manifest_sha256: "m".repeat(64) };
        let inst = deploy::InstanceIdentity {
            instance_id: "i".into(),
            session_id: "s".into(),
            state_schema: 1,
            journal_bytes_at_boot: 9,
            context: deploy::CONTEXT_NOT_RESTORED.into(),
            context_restore: None,
        };
        let line = stamper.ephemeral(hello(Some(dep.clone()), Some(inst.clone()))).to_line();
        let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap();
        let Event::Hello { deployment: Some(d), instance: Some(i), .. } = back.event else { panic!() };
        assert_eq!((d, i), (dep, inst));
    }

    #[test]
    fn command_kinds_have_wire_names_senders_and_additive_fields() {
        let sub = host().stamp(
            Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "hi".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
            None,
        );
        let line = sub.to_line();
        assert!(line.contains(r#""kind":"command_submitted""#), "{line}");
        assert!(Envelope::parse_from(line.trim_end(), Source::Host).is_ok());
        assert_eq!(Envelope::parse_from(line.trim_end(), Source::Guest).unwrap_err(), FrameError::WrongSender);

        let guest = Stamper::new(Source::Guest, "guest-x", Box::new(FixedClock(1)));
        let mut guest = guest;
        let first = guest.stamp(
            Event::CommandAdmitted {
                command_id: "c1".into(),
                generation: 1,
                status: CommandStatus::Admitted,
                duplicate: false,
                decision_id: None,
                response_id: None,
                result_text: None,
                detail: None,
            },
            Some(sub.id.clone()),
        );
        let line = first.to_line();
        assert!(line.contains(r#""kind":"command_admitted""#) && line.contains(r#""status":"admitted""#), "{line}");

        for absent in ["duplicate", "decision_id", "response_id", "result_text", "detail"] {
            assert!(!line.contains(absent), "{absent} in {line}");
        }
        let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap();
        assert!(matches!(back.event, Event::CommandAdmitted { duplicate: false, status: CommandStatus::Admitted, .. }));
        let interrupted = r#"{"v":2,"id":"guest-x:9","producer":"guest-x","seq":9,"t_ns":1,"source":"guest","kind":"command_admitted","command_id":"c1","generation":1,"status":"interrupted","duplicate":true,"response_id":"r1","detail":"x"}"#;
        let Event::CommandAdmitted { status, duplicate: true, response_id: Some(r), .. } = Envelope::parse_from(interrupted, Source::Guest).unwrap().event else { panic!() };
        assert_eq!((status, r.as_str()), (CommandStatus::Interrupted, "r1"));
        assert!(status.is_terminal() && !CommandStatus::Dispatched.is_terminal());

        let old = r#"{"v":2,"id":"guest-x:0","producer":"guest-x","seq":0,"t_ns":1,"source":"guest","kind":"hello","protocol":2,"agent":"rth-agentd","version":"0.1.0","boot_id":"x","utc_ms_at_origin":5,"journal":"j"}"#;
        assert!(matches!(Envelope::parse_from(old, Source::Guest).unwrap().event, Event::Hello { command_generation: None, .. }));
    }

    #[test]
    fn received_stamp_is_optional_on_the_wire() {
        let env = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None);
        let line = env.to_line();
        assert!(!line.contains("received"), "a sender never writes the admission stamp");
        let mut admitted = Envelope::parse_from(line.trim_end(), Source::Host).unwrap();
        admitted.received = Some(Received { by: "guest-a".into(), t_ns: 42 });
        let back: Envelope = serde_json::from_str(admitted.to_line().trim_end()).unwrap();
        assert_eq!(back.received, Some(Received { by: "guest-a".into(), t_ns: 42 }));
    }

    #[test]
    fn audio_chunk_fits_a_frame_at_the_cap() {
        let data = "A".repeat(MAX_AUDIO_CHUNK_BYTES.div_ceil(3) * 4);
        let event = Event::AgentAudio {
            response_id: "boot-r123456".into(),
            chunk: u64::MAX,
            sample_offset: u64::MAX,
            sample_rate: 24_000,
            format: "s16le".into(),
            data,
        };
        let mut s = Stamper::new(Source::Guest, "guest-0123456789abcdef-99999", Box::new(FixedClock(u64::MAX)));
        let line = s.stamp(event, Some("guest-0123456789abcdef-99999:18446744073709551615".into())).to_line();
        assert!(line.len() < MAX_FRAME, "{} bytes", line.len());
        assert_eq!(Envelope::parse_from(line.trim_end(), Source::Guest).unwrap().event.sender(), Source::Guest);
    }

    fn user_audio(data: String) -> Event {
        Event::UserAudio {
            stream_id: "mic-1".into(),
            chunk: 3,
            sample_offset: 1920,
            sample_rate: MIC_SAMPLE_RATE,
            format: "s16le".into(),
            data,
            digest: None,
        }
    }

    #[test]
    fn voice_kinds_have_wire_names_and_the_right_sender() {
        let mut h = host();
        let host_kinds = [
            (user_audio("AAA=".into()), "user_audio"),
            (Event::UserAudioEnd { stream_id: "mic-1".into(), chunks: 4, samples: 2560, dropped_chunks: 0, speaker: Default::default() }, "user_audio_end"),
            (
                Event::Playback {
                    response_id: "r1".into(),
                    epoch: 2,
                    phase: PlaybackPhase::Cleared,
                    played_samples: 4800,
                    dropped_samples: 96_000,
                    sample_rate: 48_000,
                    reason: Some("local_vad".into()),
                    clear_ns: Some(12_000),
                },
                "playback",
            ),
        ];
        for (event, kind) in host_kinds {
            let line = h.stamp(event, None).to_line();
            assert!(line.contains(&format!(r#""kind":"{kind}""#)), "{line}");
            assert!(Envelope::parse_from(line.trim_end(), Source::Host).is_ok(), "{kind} from the host");
            let mut spoof: Envelope = serde_json::from_str(line.trim_end()).unwrap();
            spoof.source = Source::Guest;
            assert_eq!(Envelope::parse_from(&spoof.to_line(), Source::Guest).unwrap_err(), FrameError::WrongSender, "{kind}");
        }
        let mut g = Stamper::new(Source::Guest, "guest-a", Box::new(FixedClock(1)));
        let guest_kinds = [
            (Event::AudioIngress { stream_id: "mic-1".into(), chunks: 4, samples: 2560, bytes: 5120, gaps: 0, sha256: "ab".into() }, "audio_ingress"),
            (Event::InputTranscript { text: "hello".into(), utterance_id: None, utterance_seq: None, is_final: None, response_id: None, association: None, boundary: None }, "input_transcript"),
            (Event::ProviderSignal { response_id: Some("r1".into()), signal: "interrupted".into() }, "provider_signal"),
        ];
        for (event, kind) in guest_kinds {
            let line = g.stamp(event, None).to_line();
            assert!(line.contains(&format!(r#""kind":"{kind}""#)), "{line}");
            assert!(Envelope::parse_from(line.trim_end(), Source::Guest).is_ok(), "{kind} from the guest");
            assert_eq!(Envelope::parse_from(line.trim_end(), Source::Host).unwrap_err(), FrameError::WrongSender, "{kind}");
        }
    }

    #[test]
    fn user_audio_digest_is_absent_on_the_wire_and_survives_the_journal() {
        let env = host().stamp(user_audio("AQACAA==".into()), None);
        let line = env.to_line();
        assert!(!line.contains("digest"), "a sender never writes the digest");
        let mut journaled = Envelope::parse_from(line.trim_end(), Source::Host).unwrap();
        if let Event::UserAudio { data, digest, .. } = &mut journaled.event {
            data.clear();
            *digest = Some(AudioDigest { bytes: 4, sha256: "00".into() });
        }
        let back = Envelope::parse_from(journaled.to_line().trim_end(), Source::Host).unwrap();
        let Event::UserAudio { data, digest, .. } = back.event else { panic!() };
        assert_eq!((data.as_str(), digest), ("", Some(AudioDigest { bytes: 4, sha256: "00".into() })));
    }

    #[test]
    fn mic_chunk_fits_a_frame_at_the_cap() {
        let mut e = user_audio("A".repeat(MAX_AUDIO_CHUNK_BYTES.div_ceil(3) * 4));
        if let Event::UserAudio { chunk, sample_offset, stream_id, .. } = &mut e {
            *chunk = u64::MAX;
            *sample_offset = u64::MAX;
            *stream_id = "x".repeat(128);
        }
        let line = Stamper::new(Source::Host, "host-0123456789abcdef-99999-a99", Box::new(FixedClock(u64::MAX))).stamp(e, None).to_line();
        assert!(line.len() < MAX_FRAME, "{} bytes", line.len());
    }

    #[test]
    fn heartbeats_do_not_consume_sequence() {
        let mut s = host();
        let _ = s.ephemeral(Event::Heartbeat { uptime_ms: 1, attachments: 0 });
        assert_eq!(s.stamp(Event::Cancel { response_id: "r".into() }, None).seq, 1);
    }

    #[test]
    fn task_kinds_round_trip_with_their_senders() {
        let mut g = Stamper::new(Source::Guest, "guest-x", Box::new(FixedClock(5)));
        let ready = g.stamp(Event::TasksReady { schema: "rth-tasks/v1".into() }, None);
        let rec = serde_json::json!({"task_id": "b-t1", "rev": 3, "execution": "running"});
        let st = g.stamp(Event::TaskState { task_id: "b-t1".into(), record: rec.clone() }, Some("host-x:9".into()));
        for e in [&ready, &st] {
            let back = Envelope::parse_from(e.to_line().trim_end(), Source::Guest).unwrap();
            assert_eq!(back.to_line(), e.to_line());
        }
        assert!(st.to_line().contains("\"kind\":\"task_state\""));
        let mut h = host();
        let up = h.stamp(Event::TaskUpdate { task_id: "b-t1".into(), update: serde_json::json!({"kind": {"kind": "claim"}}) }, None);
        let ex = h.stamp(Event::ExecutorStatus { status: serde_json::json!({"available": true}) }, None);
        for e in [&up, &ex] {
            assert_eq!(Envelope::parse_from(e.to_line().trim_end(), Source::Host).unwrap().to_line(), e.to_line());
            assert!(Envelope::parse_from(e.to_line().trim_end(), Source::Guest).is_err(), "a guest may not send it");
        }
        assert!(Envelope::parse_from(st.to_line().trim_end(), Source::Host).is_err(), "a host may not send task_state");
    }

    #[test]
    fn s2_kinds_round_trip_and_are_guest_only() {
        let mut g = Stamper::new(Source::Guest, "guest-b1", Box::new(FixedClock(1)));
        for e in [
            Event::TaskToolCall { task_id: "b-t1".into(), receipt: serde_json::json!({"call_seq": 1, "outcome": "ok"}) },
            Event::FolderGrant { event: "granted".into(), record: serde_json::json!({"event": "granted"}) },
            Event::FolderGrants { grants: serde_json::json!([]) },
            Event::TaskReport { task_id: "b-t1".into(), text: "ANSWER: 7421".into(), result_sha256: "ab".into() },
            Event::PartState { parts: serde_json::json!({"servers": []}) },
        ] {
            let env = g.stamp(e, None);
            let line = env.to_line();
            assert_eq!(Envelope::parse_from(line.trim_end(), Source::Guest).unwrap().to_line(), line);
            assert!(Envelope::parse_from(line.trim_end(), Source::Host).is_err(), "a host may not send {line}");
        }
    }

    #[test]
    fn kinds_list_matches_the_enum() {
        let src = include_str!("lib.rs");
        let start = src.find("pub enum Event {").unwrap();
        let body = &src[start..start + src[start..].find("\n}\n").unwrap()];
        let mut kinds = Vec::new();
        for line in body.lines() {
            let Some(rest) = line.strip_prefix("    ") else { continue };
            if rest.starts_with(' ') {
                continue;
            }
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if name.is_empty() || !name.chars().next().unwrap().is_ascii_uppercase() || !rest[name.len()..].trim_start().starts_with('{') {
                continue;
            }
            let mut snake = String::new();
            for (i, c) in name.chars().enumerate() {
                if c.is_ascii_uppercase() && i > 0 {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            kinds.push(snake);
        }
        assert_eq!(kinds, KINDS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn response_diagnostic_round_trips_and_an_old_reader_skips_it() {
        let mut g = Stamper::new(Source::Guest, "guest-x", Box::new(FixedClock(3)));
        let done = g.stamp(Event::AgentDone { response_id: "v1".into(), status: ResponseStatus::Failed }, Some("guest-x:1".into()));
        let diag = Event::ResponseDiagnostic {
            response_id: "v1".into(),
            session_generation: 2,
            terminal_event_id: done.id.clone(),
            provider_status: Some("incomplete".into()),
            provider_reason: None,
            terminal_status_mapped: ResponseStatus::Failed,
            outcome_source: "provider".into(),
            local_reason: None,
            truncated: None,
            sentence_case_applied: None,
            raw_text: None,
            history_guard_applied: None,
            raw_text_clipped: None,
        };
        let line = g.stamp(diag, Some("guest-x:1".into())).to_line();
        assert!(line.contains(r#""kind":"response_diagnostic""#) && line.contains(r#""provider_reason":null"#), "{line}");
        let back = Envelope::parse_known(&line, Source::Guest).unwrap().unwrap();
        let Event::ResponseDiagnostic { terminal_event_id, provider_status, terminal_status_mapped, .. } = back.event else { panic!() };
        assert_eq!((terminal_event_id, provider_status.as_deref(), terminal_status_mapped), (done.id.clone(), Some("incomplete"), ResponseStatus::Failed));
        assert_eq!(back.cause.as_deref(), Some("guest-x:1"));
        let old_view = line.replace(r#""kind":"response_diagnostic""#, r#""kind":"kind_this_reader_predates""#);
        let heartbeat = g.ephemeral(Event::Heartbeat { uptime_ms: 1, attachments: 1 }).to_line();
        let mut terminals = 0;
        let mut beats = 0;
        for l in [done.to_line(), old_view, heartbeat] {
            match Envelope::parse_known(&l, Source::Guest).unwrap() {
                None => {}
                Some(Envelope { event: Event::AgentDone { status: ResponseStatus::Failed, .. }, .. }) => terminals += 1,
                Some(Envelope { event: Event::Heartbeat { .. }, .. }) => beats += 1,
                Some(other) => panic!("{other:?}"),
            }
        }
        assert_eq!((terminals, beats), (1, 1));
    }

    use super::r8 as memory_effect;

    #[test]
    fn the_previous_protocol_reader_accepts_current_frames() {
        let src = include_str!("lib.rs");
        assert!(!src.contains(concat!("#[path = \"", "/")), "no absolute module path");
        assert_eq!(memory_effect::KINDS.len(), memory_effect::KINDS.iter().collect::<std::collections::BTreeSet<_>>().len());
    }

    #[test]
    fn memory_text_frames_round_trip_never_debug_the_text_and_an_reader_skips_them() {
        let guest = Stamper::new(Source::Guest, "agentd-a", Box::new(FixedClock(7)));
        let text = Event::TurnMemoryText { response_id: "r1".into(), items: vec![MemoryText { memory_id: "m-1".into(), text: "my dog is called Sol".into() }] };
        let revoked = Event::MemoryTextRevoked { memory_id: "m-1".into() };
        assert!(!format!("{text:?}").contains("Sol"), "{text:?}");
        for e in [text, revoked] {
            let line = guest.ephemeral(e.clone()).to_line();
            let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap().event;
            assert_eq!(serde_json::to_value(&back).unwrap(), serde_json::to_value(&e).unwrap());
            assert!(memory_effect::Envelope::parse_known(line.trim_end(), memory_effect::Source::Guest).unwrap().is_none(), "a memory reader skips it: {line}");
        }
    }

    #[test]
    fn unknown_kinds_are_skippable_but_everything_else_stays_fail_closed() {
        let newer = r#"{"v":2,"id":"guest-x:3","producer":"guest-x","seq":3,"t_ns":1,"source":"guest","kind":"something_new","x":1}"#;
        assert!(Envelope::parse_known(newer, Source::Guest).unwrap().is_none());
        assert!(matches!(Envelope::parse_from(newer, Source::Guest), Err(FrameError::Malformed(_))));

        let v3 = newer.replace(r#""v":2"#, r#""v":3"#);
        assert_eq!(Envelope::parse_known(&v3, Source::Guest).unwrap_err(), FrameError::Version(3));
        assert_eq!(Envelope::parse_known(newer, Source::Host).unwrap_err(), FrameError::WrongSender);

        let broken = r#"{"v":2,"id":"guest-x:3","producer":"guest-x","seq":3,"t_ns":1,"source":"guest","kind":"agent_delta"}"#;
        assert!(matches!(Envelope::parse_known(broken, Source::Guest), Err(FrameError::Malformed(_))));
        let headless = r#"{"v":2,"source":"guest","kind":"something_new"}"#;
        assert!(matches!(Envelope::parse_known(headless, Source::Guest), Err(FrameError::Malformed(_))));
        assert!(matches!(Envelope::parse_known("nope", Source::Guest), Err(FrameError::Malformed(_))));

        let ok = host().stamp(Event::Cancel { response_id: "r".into() }, None).to_line();
        assert!(Envelope::parse_known(ok.trim_end(), Source::Host).unwrap().is_some());
    }

    #[test]
    fn utterance_and_media_fields_are_additive() {

        let old = r#"{"v":2,"id":"guest-x:4","producer":"guest-x","seq":4,"t_ns":1,"source":"guest","kind":"input_transcript","text":"hi"}"#;
        let Event::InputTranscript { utterance_id: None, utterance_seq: None, is_final: None, response_id: None, association: None, .. } =
            Envelope::parse_from(old, Source::Guest).unwrap().event
        else {
            panic!()
        };
        let old = r#"{"v":2,"id":"guest-x:5","producer":"guest-x","seq":5,"t_ns":1,"source":"guest","kind":"response_started","response_id":"r","request_id":"q"}"#;
        assert!(matches!(Envelope::parse_from(old, Source::Guest).unwrap().event, Event::ResponseStarted { utterance_id: None, .. }));

        let mut g = Stamper::new(Source::Guest, "guest-x", Box::new(FixedClock(1)));
        let t = g.stamp(
            Event::InputTranscript {
                text: "hel".into(),
                utterance_id: Some("b-u1".into()),
                utterance_seq: Some(0),
                is_final: Some(true),
                response_id: None,
                association: Some("unknown".into()),
                boundary: None,
            },
            None,
        );
        let line = t.to_line();
        assert!(line.contains(r#""final":true"#) && line.contains(r#""utterance_seq":0"#) && !line.contains("response_id"), "{line}");
        assert!(line.contains(r#""seq":1"#), "the envelope keeps its own seq: {line}");
        let bare = g.stamp(Event::ResponseStarted { response_id: "r".into(), request_id: "q".into(), utterance_id: None }, None).to_line();
        assert!(!bare.contains("utterance_id"));
        let m = g.stamp(
            Event::MediaStored {
                ref_format: MEDIA_REF_FORMAT.into(),
                media: "agent_audio".into(),
                response_id: Some("r".into()),
                stream_id: None,
                sha256: "0".repeat(64),
                bytes: 4,
                encoding: "s16le".into(),
                sample_rate: 24_000,
                channels: 1,
                chunk_first: 0,
                chunk_last: 0,
                sample_start: 0,
                sample_end: 2,
                first_event: "guest-x:2".into(),
                last_event: "guest-x:2".into(),
                clock: "guest-x".into(),
                t_ns_first: 1,
                t_ns_last: 1,
                path: format!("media/{}", "0".repeat(64)),
            },
            None,
        );
        let line = m.to_line();
        assert!(line.contains(r#""kind":"media_stored""#) && !line.contains("stream_id"), "{line}");
        assert!(Envelope::parse_from(line.trim_end(), Source::Guest).is_ok());
        assert_eq!(Envelope::parse_from(line.trim_end(), Source::Host).unwrap_err(), FrameError::WrongSender);
    }

    const PRESENCE: &str = r#"{"kind":"presence","locked":false,"display_asleep":false,"idle_s":12,"output_busy":false,"last_origin":"mac","utc_offset_min":-180,"frontmost":true,"notify_ok":true}"#;
    const SENT: &str = r#"{"kind":"expression_sent","expression_id":"x-1","reach_kind":"open_threads","destination":"chat","subject_id":"h-1","reason_kind":"open_threads","text_sha256":"ab","memory_refs":[{"memory_id":"m-1","revision":2}],"spend":{"n":4,"cost_pico":1200000000},"provenance":{"thread":"background","effort":"off"}}"#;
    const DELIVER: &str = r#"{"kind":"expression_deliver","expression_id":"x-1","response_id":"b-r9","reach_kind":"open_threads","destination":"notification","message":{"title":"Lua","text":"my dog Sol","reason":"Why now: this thread has been open for 3 days."},"held_since_utc_ms":1793000000123}"#;
    const OUTCOME: &str = r#"{"kind":"expression_outcome","expression_id":"x-1","outcome":"notified","detail":null}"#;

    #[test]
    fn b2_spend_status_by_purpose_is_optional_and_round_trips() {
        let old = r#"{"kind":"spend_status","month":"2026-10","spent_pico":5,"cap_pico":50000000000000,"capped":false}"#;
        let Event::SpendStatus { by_purpose, .. } = serde_json::from_str::<Event>(old).unwrap() else { panic!() };
        assert_eq!(by_purpose, None);
        let new = r#"{"kind":"spend_status","month":"2026-10","spent_pico":5,"cap_pico":50000000000000,"capped":false,"by_purpose":{"chat":3,"reachout":2}}"#;
        let line = Stamper::new(Source::Guest, "a", Box::new(FixedClock(7))).stamp(serde_json::from_str(new).unwrap(), None).to_line();
        let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap();
        assert_eq!(serde_json::to_value(&back.event).unwrap(), serde_json::from_str::<serde_json::Value>(new).unwrap());
        assert!(!serde_json::to_string(&serde_json::from_str::<Event>(old).unwrap()).unwrap().contains("by_purpose"), "absent stays absent");
    }

    #[test]
    fn b1_expression_frames_round_trip_and_each_comes_from_one_side_only() {
        for (f, from) in [(PRESENCE, Source::Host), (OUTCOME, Source::Host), (SENT, Source::Guest), (DELIVER, Source::Guest)] {
            let event: Event = serde_json::from_str(f).unwrap_or_else(|e| panic!("{f}: {e}"));
            assert_eq!(event.sender(), from, "{f}");
            let line = Stamper::new(from, "a", Box::new(FixedClock(7))).stamp(event, None).to_line();
            let back = Envelope::parse_from(line.trim_end(), from).unwrap();
            let other = if from == Source::Host { Source::Guest } else { Source::Host };
            assert_eq!(Envelope::parse_from(line.trim_end(), other).unwrap_err(), FrameError::WrongSender, "{f}");
            let mut want: serde_json::Value = serde_json::from_str(f).unwrap();
            want.as_object_mut().unwrap().retain(|_, v| !v.is_null());
            assert_eq!(serde_json::to_value(&back.event).unwrap(), want, "{f}");
            assert!(line.contains(r#""held_since_utc_ms":1793000000123"#) || !f.contains("held_since"), "numbers stay numbers: {line}");
        }
    }

    #[test]
    fn b1_a_reader_without_wish_b_skips_the_expression_kinds() {
        for (f, from, old) in [(PRESENCE, Source::Host, memory_effect::Source::Host), (OUTCOME, Source::Host, memory_effect::Source::Host), (SENT, Source::Guest, memory_effect::Source::Guest), (DELIVER, Source::Guest, memory_effect::Source::Guest)] {
            let line = Stamper::new(from, "a", Box::new(FixedClock(7))).stamp(serde_json::from_str(f).unwrap(), None).to_line();
            assert!(memory_effect::Envelope::parse_known(line.trim_end(), old).unwrap().is_none(), "the memory freeze skips it: {line}");
        }
    }

    #[test]
    fn b1_presence_needs_its_utc_offset_and_tolerates_a_newer_field() {
        let mut v: serde_json::Value = serde_json::from_str(PRESENCE).unwrap();
        v["focus"] = serde_json::json!("work");
        assert!(serde_json::from_value::<Event>(v.clone()).is_ok(), "an additive field");
        v.as_object_mut().unwrap().remove("utc_offset_min");
        assert!(serde_json::from_value::<Event>(v).is_err(), "no offset, no presence");
    }

    #[test]
    fn b1_expression_sent_has_no_text_and_deliver_never_debugs_it() {
        let sent: Event = serde_json::from_str(SENT).unwrap();
        let keys: Vec<String> = serde_json::to_value(&sent).unwrap().as_object().unwrap().keys().cloned().collect();
        assert!(!keys.iter().any(|k| k.contains("text") && k != "text_sha256"), "{keys:?}");
        let deliver: Event = serde_json::from_str(DELIVER).unwrap();
        let shown = format!("{deliver:?}");
        assert!(!shown.contains("Sol") && !shown.contains("Why now") && !shown.contains("Lua"), "{shown}");
    }

    mod pv05_g1 {
        use super::*;

        const GRANT: &str = r#"{"grant_id":"g-1","method":"touch_id_or_password","at_utc_ms":1793000000000}"#;

        fn frames() -> Vec<(String, Source)> {
            let host = [
                format!(r#"{{"kind":"people_enrol","frame_id":"f1","person_id":"p-1","name":"Ana","relation":"partner","owner":true,"modality":"read_along","consent_copy_version":2,"auth":{GRANT}}}"#),
                format!(r#"{{"kind":"people_update","frame_id":"f2","person_id":"p-2","name":"Bia","relation":null,"owner":null,"recognize":false,"auth":{GRANT}}}"#),
                format!(r#"{{"kind":"people_forget","frame_id":"f3","person_id":"p-2","modality":"face","auth":{GRANT}}}"#),
                format!(r#"{{"kind":"people_undo","frame_id":"f4","person_id":"p-2","auth":{GRANT}}}"#),
                r#"{"kind":"people_correct","frame_id":"f5","turn_id":"r-9","person_id":"p-2"}"#.to_string(),
                r#"{"kind":"people_list_request","frame_id":"f6"}"#.to_string(),
                r#"{"kind":"people_presence","present":["p-1","p-2"],"unknown_faces":1,"at_utc_ms":1793000000000}"#.to_string(),
            ];
            let guest = [
                r#"{"kind":"people_ack","frame_id":"f1","person_id":"p-1","record_rev":3}"#.to_string(),
                r#"{"kind":"people_refused","frame_id":"f2","reason":"needs_owner_grant"}"#.to_string(),
                r#"{"kind":"people_list","people":[{"person_id":"p-1","name":"Ana","relation":"partner","owner":true,"recognize":true}],"forgotten":[{"person_id":"p-3","name":"Caio","forgotten_at_utc_ms":1793000000000}],"prompts":["Who is this?"]}"#.to_string(),
                r#"{"kind":"people_proposal","proposal_kind":"introduce","turn_id":"r-8"}"#.to_string(),
                r#"{"kind":"people_proposal","proposal_kind":"correct","name":"Bia","turn_id":"r-9"}"#.to_string(),
            ];
            host.into_iter().map(|f| (f, Source::Host)).chain(guest.into_iter().map(|f| (f, Source::Guest))).collect()
        }

        #[test]
        fn every_people_frame_parses_comes_from_one_side_only_and_round_trips() {
            for (f, from) in frames() {
                let event: Event = serde_json::from_str(&f).unwrap_or_else(|e| panic!("{f}: {e}"));
                assert_eq!(event.sender(), from, "{f}");
                let line = Stamper::new(from, "a", Box::new(FixedClock(7))).stamp(event, None).to_line();
                let back = Envelope::parse_from(line.trim_end(), from).unwrap_or_else(|e| panic!("{f}: {e}"));
                let other = if from == Source::Host { Source::Guest } else { Source::Host };
                assert_eq!(Envelope::parse_from(line.trim_end(), other).unwrap_err(), FrameError::WrongSender, "{f}");
                let mut want: serde_json::Value = serde_json::from_str(&f).unwrap();
                want.as_object_mut().unwrap().retain(|_, v| !v.is_null());
                assert_eq!(serde_json::to_value(&back.event).unwrap(), want, "{f}");
            }
        }

        #[test]
        fn a_people_list_without_forgotten_has_none_forgotten() {
            let e: Event = serde_json::from_str(r#"{"kind":"people_list","people":[],"prompts":[]}"#).unwrap();
            assert!(matches!(e, Event::PeopleList { forgotten, .. } if forgotten.is_empty()));
        }

        #[test]
        fn a_turn_frame_carries_its_speaker_and_one_without_is_the_owner_at_the_keyboard() {
            for f in [
                r#"{"kind":"user_text","request_id":"q1","text":"hi","speaker":{"who":"person","person_id":"p-2"}}"#,
                r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi","speaker":{"who":"uncertain"}}"#,
            ] {
                let back = serde_json::to_value(serde_json::from_str::<Event>(f).unwrap()).unwrap();
                assert_eq!(back["speaker"], serde_json::from_str::<serde_json::Value>(f).unwrap()["speaker"], "{f}");
            }
            for f in [r#"{"kind":"user_text","request_id":"q1","text":"hi"}"#, r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi"}"#] {
                let back = serde_json::to_value(serde_json::from_str::<Event>(f).unwrap()).unwrap();
                assert_eq!(back["speaker"], serde_json::json!({"who": "owner_keyboard"}), "an older client: {f}");
            }
        }

        #[test]
        fn a_realtime_voice_turn_carries_its_speaker_on_user_audio_end() {
            let f = r#"{"kind":"user_audio_end","stream_id":"mic-1","chunks":4,"samples":2560,"dropped_chunks":0,"speaker":{"who":"person","person_id":"p-1"}}"#;
            let back = serde_json::to_value(serde_json::from_str::<Event>(f).unwrap()).unwrap();
            assert_eq!(back["speaker"], serde_json::json!({"who": "person", "person_id": "p-1"}));
            let old = r#"{"kind":"user_audio_end","stream_id":"mic-1","chunks":4,"samples":2560,"dropped_chunks":0}"#;
            assert_eq!(serde_json::to_value(serde_json::from_str::<Event>(old).unwrap()).unwrap()["speaker"], serde_json::json!({"who": "owner_keyboard"}));
        }

        #[test]
        fn an_identical_retry_reads_the_same_and_an_absent_speaker_is_the_keyboard_owner() {
            let f = r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi","speaker":{"who":"person","person_id":"p-2"}}"#;
            let (a, b): (Event, Event) = (serde_json::from_str(f).unwrap(), serde_json::from_str(f).unwrap());
            assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::to_value(&b).unwrap());
            let old = r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi"}"#;
            let new = r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi","speaker":{"who":"owner_keyboard"}}"#;
            assert_eq!(serde_json::to_value(serde_json::from_str::<Event>(old).unwrap()).unwrap(), serde_json::to_value(serde_json::from_str::<Event>(new).unwrap()).unwrap());
            let Event::CommandSubmitted { speaker, .. } = serde_json::from_str::<Event>(old).unwrap() else { unreachable!() };
            assert_eq!(speaker, TurnSpeaker::OwnerKeyboard);
        }

        #[test]
        fn a_turn_frame_with_reply_voice_reads_as_one_without_on_any_agentd() {
            let frames = [
                Event::UserText { request_id: "q".into(), text: "can you speak?".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
                Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "can you speak?".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
            ];
            for event in frames {
                let line = host().stamp(event, None).to_line();
                let mut v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
                v["reply_voice"] = serde_json::json!({"dictated": true, "spoken": "first_sentence"});
                let with = Envelope::parse_from(&v.to_string(), Source::Host).unwrap();
                let mut read = serde_json::to_value(&with).unwrap();
                read.as_object_mut().unwrap().remove("reply_voice");
                assert_eq!(read, serde_json::from_str::<serde_json::Value>(line.trim_end()).unwrap(), "{line}");
            }
        }

        #[test]
        fn a_turn_frames_reply_voice_reads_back_and_an_absent_one_stays_absent() {
            let f = r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi","reply_voice":{"dictated":true,"spoken":"full"}}"#;
            let Event::CommandSubmitted { reply_voice, .. } = serde_json::from_str::<Event>(f).unwrap() else { unreachable!() };
            assert_eq!(reply_voice, Some(ReplyVoice { dictated: true, spoken: SpokenReply::Full }));
            let u: Event = serde_json::from_str(r#"{"kind":"user_text","request_id":"q","text":"hi","reply_voice":{"dictated":false,"spoken":"first_sentence"}}"#).unwrap();
            assert_eq!(serde_json::to_value(&u).unwrap()["reply_voice"], serde_json::json!({"dictated": false, "spoken": "first_sentence"}));
            for old in [r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi"}"#, r#"{"kind":"user_text","request_id":"q","text":"hi"}"#] {
                let back = serde_json::to_string(&serde_json::from_str::<Event>(old).unwrap()).unwrap();
                assert!(!back.contains("reply_voice"), "{back}");
            }
            assert!(serde_json::from_str::<Event>(r#"{"kind":"user_text","request_id":"q","text":"hi","reply_voice":{"dictated":true,"spoken":"loud"}}"#).is_err());
        }

        #[test]
        fn a_turn_frames_effort_reads_back_and_an_absent_one_is_off() {
            for (lvl, e) in [("auto", Effort::Auto), ("off", Effort::Off), ("low", Effort::Low), ("high", Effort::High), ("max", Effort::Max)] {
                let f = format!(r#"{{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi","effort":"{lvl}"}}"#);
                let Event::CommandSubmitted { effort, .. } = serde_json::from_str::<Event>(&f).unwrap() else { unreachable!() };
                assert_eq!(effort, Some(e));
                let u: Event = serde_json::from_str(&format!(r#"{{"kind":"user_text","request_id":"q","text":"hi","effort":"{lvl}"}}"#)).unwrap();
                assert_eq!(serde_json::to_value(&u).unwrap()["effort"], serde_json::json!(lvl));
            }
            for old in [r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi"}"#, r#"{"kind":"user_text","request_id":"q","text":"hi"}"#] {
                let e = serde_json::from_str::<Event>(old).unwrap();
                let back = serde_json::to_string(&e).unwrap();
                assert!(!back.contains("effort"), "{back}");
                let (Event::CommandSubmitted { effort, .. } | Event::UserText { effort, .. }) = e else { unreachable!() };
                assert_eq!(effort.unwrap_or_default(), Effort::Off, "absent is off");
            }
            for bad in ["medium", "xhigh", "none", "High"] {
                assert!(serde_json::from_str::<Event>(&format!(r#"{{"kind":"user_text","request_id":"q","text":"hi","effort":"{bad}"}}"#)).is_err(), "{bad}");
            }
        }

        #[test]
        fn a_turn_frame_with_effort_reads_as_one_without_on_an_older_agentd() {
            let frames = [
                Event::UserText { request_id: "q".into(), text: "prove it".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
                Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "prove it".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
            ];
            for event in frames {
                let line = host().stamp(event, None).to_line();
                let mut v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
                v["effort"] = serde_json::json!("high");
                assert!(crate::r8::Envelope::parse_known(&v.to_string(), crate::r8::Source::Host).unwrap().is_some(), "the memory freeze reads it");
                let with = Envelope::parse_from(&v.to_string(), Source::Host).unwrap();
                let mut read = serde_json::to_value(&with).unwrap();
                assert_eq!(read["effort"], serde_json::json!("high"));
                read.as_object_mut().unwrap().remove("effort");
                assert_eq!(read, serde_json::from_str::<serde_json::Value>(line.trim_end()).unwrap(), "{line}");
            }
        }

        #[test]
        fn a_turn_frames_reply_max_tokens_reads_back_and_a_bad_one_refuses_the_frame() {
            for frame in [r#"{"kind":"command_submitted","command_id":"c1","session_id":"s","expected_generation":0,"text":"hi""#, r#"{"kind":"user_text","request_id":"q","text":"hi""#] {
                for n in [1, 2000, REPLY_MAX_TOKENS_LIMIT] {
                    let e: Event = serde_json::from_str(&format!(r#"{frame},"reply_max_tokens":{n}}}"#)).unwrap();
                    let (Event::CommandSubmitted { reply_max_tokens, .. } | Event::UserText { reply_max_tokens, .. }) = &e else { unreachable!() };
                    assert_eq!(*reply_max_tokens, Some(n));
                    assert_eq!(serde_json::to_value(&e).unwrap()["reply_max_tokens"], serde_json::json!(n));
                }
                let old = serde_json::to_string(&serde_json::from_str::<Event>(&format!("{frame}}}")).unwrap()).unwrap();
                assert!(!old.contains("reply_max_tokens"), "{old}");
                for bad in ["0", "-1", "1.5", r#""2000""#, "1048577", "4294967296", "null", "true", "{}"] {
                    assert!(serde_json::from_str::<Event>(&format!(r#"{frame},"reply_max_tokens":{bad}}}"#)).is_err(), "{bad}");
                }
            }
        }

        #[test]
        fn a_turn_frame_with_reply_max_tokens_reads_as_one_without_on_an_older_agentd() {
            let frames = [
                Event::UserText { request_id: "q".into(), text: "write it all".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
                Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "write it all".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None },
            ];
            for event in frames {
                let line = host().stamp(event, None).to_line();
                let mut v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
                v["reply_max_tokens"] = serde_json::json!(4000);
                assert!(crate::r8::Envelope::parse_known(&v.to_string(), crate::r8::Source::Host).unwrap().is_some(), "the memory freeze reads it");
                let with = Envelope::parse_from(&v.to_string(), Source::Host).unwrap();
                let mut read = serde_json::to_value(&with).unwrap();
                assert_eq!(read["reply_max_tokens"], serde_json::json!(4000));
                read.as_object_mut().unwrap().remove("reply_max_tokens");
                assert_eq!(read, serde_json::from_str::<serde_json::Value>(line.trim_end()).unwrap(), "{line}");
            }
        }

        #[test]
        fn a_reply_spoken_frame_reads_back_and_an_older_reader_skips_it() {
            let line = Stamper::new(Source::Guest, "agent", Box::new(FixedClock(7))).stamp(Event::ReplySpoken { response_id: "r1".into() }, None).to_line();
            assert!(line.contains(r#""kind":"reply_spoken","response_id":"r1""#), "{line}");
            assert!(Envelope::parse_from(line.trim_end(), Source::Guest).is_ok());
            assert!(crate::r8::Envelope::parse_known(line.trim_end(), crate::r8::Source::Guest).unwrap().is_none(), "{line}");
        }

        #[test]
        fn opener_round_trips_skips_on_old_readers_and_rejects_spoofing() {
            let event = Event::AgentOpener { response_id: "r1".into(), text: "Let me think.".into() };
            let line = Stamper::new(Source::Guest, "agent", Box::new(FixedClock(7))).stamp(event, None).to_line();
            assert!(KINDS.contains(&"agent_opener"));
            let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap();
            assert!(matches!(back.event, Event::AgentOpener { response_id, text } if response_id == "r1" && text == "Let me think."));
            assert!(crate::r8::Envelope::parse_known(line.trim_end(), crate::r8::Source::Guest).unwrap().is_none());
            assert!(Envelope::parse_from(line.trim_end(), Source::Host).is_err());
            let mut broken: serde_json::Value = serde_json::from_str(&line).unwrap();
            broken.as_object_mut().unwrap().remove("text");
            assert!(Envelope::parse_known(&broken.to_string(), Source::Guest).is_err());
        }

        #[test]
        fn an_agent_replace_frame_round_trips_and_an_older_reader_skips_it() {
            let event = Event::AgentReplace { response_id: "r1".into(), text: "4 — \"quatro\".\n".into() };
            let line = Stamper::new(Source::Guest, "agent", Box::new(FixedClock(7))).stamp(event, None).to_line();
            assert!(line.contains(r#""kind":"agent_replace","response_id":"r1""#), "{line}");
            let back = Envelope::parse_from(line.trim_end(), Source::Guest).unwrap();
            match back.event {
                Event::AgentReplace { response_id, text } => assert_eq!((response_id.as_str(), text.as_str()), ("r1", "4 — \"quatro\".\n"), "{line}"),
                other => panic!("{other:?} from {line}"),
            }
            assert!(crate::r8::Envelope::parse_known(line.trim_end(), crate::r8::Source::Guest).unwrap().is_none(), "{line}");
        }

        #[test]
        fn no_people_frame_carries_samples_embeddings_or_scores() {
            fn keys(v: &serde_json::Value, out: &mut Vec<String>) {
                match v {
                    serde_json::Value::Object(m) => m.iter().for_each(|(k, v)| {
                        out.push(k.clone());
                        keys(v, out)
                    }),
                    serde_json::Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
                    _ => {}
                }
            }
            let mut all = Vec::new();
            for (f, _) in frames() {
                keys(&serde_json::to_value(serde_json::from_str::<Event>(&f).unwrap()).unwrap(), &mut all);
            }
            all.sort();
            all.dedup();
            for bad in ["audio", "pcm", "wav", "clip", "embedding", "vector", "template", "crop", "image", "jpeg", "png", "pixels", "score", "distance", "similarity", "confidence"] {
                assert!(!all.iter().any(|k| k.contains(bad)), "{bad} in {all:?}");
            }
        }

        #[test]
        fn a_reader_without_people_skips_every_people_kind() {
            for (f, from) in frames() {
                let line = Stamper::new(from, "a", Box::new(FixedClock(7))).stamp(serde_json::from_str(&f).unwrap(), None).to_line();
                let old = if from == Source::Host { memory_effect::Source::Host } else { memory_effect::Source::Guest };
                assert!(memory_effect::Envelope::parse_known(line.trim_end(), old).unwrap().is_none(), "the memory freeze skips it: {line}");
            }
        }

        #[test]
        fn each_people_variant_has_its_sender() {
            let grant = || Some(OwnerGrant { grant_id: "g".into(), method: OwnerGrantMethod::FaceLiveness, at_utc_ms: 1 });
            let host = [
                Event::PeopleEnrol { frame_id: "f".into(), person_id: "p".into(), name: "Ana".into(), relation: "self".into(), owner: true, modality: PeopleModality::Face, consent_copy_version: 2, auth: grant() },
                Event::PeopleUpdate { frame_id: "f".into(), person_id: "p".into(), name: None, relation: None, owner: None, recognize: Some(false), auth: grant() },
                Event::PeopleForget { frame_id: "f".into(), person_id: "p".into(), modality: None, auth: grant() },
                Event::PeopleUndo { frame_id: "f".into(), person_id: "p".into(), auth: None },
                Event::PeopleCorrect { frame_id: "f".into(), turn_id: "r".into(), person_id: "p".into(), owner_confirmed: true },
                Event::PeopleListRequest { frame_id: "f".into() },
                Event::PeoplePresence { present: vec![], unknown_faces: 0, at_utc_ms: 1, source: Some("camera".into()) },
            ];
            let guest = [
                Event::PeopleAck { frame_id: "f".into(), person_id: "p".into(), record_rev: 1 },
                Event::PeopleRefused { frame_id: "f".into(), reason: "needs_owner_grant".into() },
                Event::PeopleList { people: vec![PersonRow { person_id: "p".into(), name: "Ana".into(), relation: "self".into(), owner: true, recognize: true }], forgotten: vec![], prompts: vec![] },
                Event::PeopleProposal { proposal_kind: ProposalKind::Introduce, name: None, turn_id: "r".into() },
            ];
            assert!(host.iter().all(|e| e.sender() == Source::Host) && guest.iter().all(|e| e.sender() == Source::Guest));
        }
    }
}
