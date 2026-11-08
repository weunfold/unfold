

use serde::{Deserialize, Serialize};

pub mod deploy;
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
    Heartbeat { uptime_ms: u64, attachments: u64 },

    ResponseStarted {
        response_id: String,
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        utterance_id: Option<String>,
    },
    AgentDelta { response_id: String, text: String },
    AgentDone { response_id: String, status: ResponseStatus },

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
    },

    MemoryDecided { response_id: String, decision: String, receipt: serde_json::Value },

    EffectHeld { effect_id: String, effect_kind: String, rule: String, at: Option<serde_json::Value>, args_sha256: String, token_sha256: String, expires_utc_ms: u64 },

    EffectCard { effect_id: String, effect_kind: String, token: String, summary: String, why: String, expires_utc_ms: u64 },

    EffectReleased { effect_id: String, effect_kind: String, args_sha256: String, result: serde_json::Value },

    EffectRefused { effect_id: String, effect_kind: String, reason: String, at: Option<serde_json::Value>, why: String },

    Attach { protocol: u16, attachment_id: String, role: Role },
    UserText { request_id: String, text: String },

    CommandSubmitted { command_id: String, session_id: String, expected_generation: u64, text: String },
    Cancel { response_id: String },

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

    UserAudioEnd { stream_id: String, chunks: u64, samples: u64, dropped_chunks: u64 },

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
            | Event::Cancel { .. }
            | Event::UserAudio { .. }
            | Event::UserAudioEnd { .. }
            | Event::Playback { .. }
            | Event::TaskUpdate { .. }
            | Event::ExecutorStatus { .. } => Source::Host,
            _ => Source::Guest,
        }
    }
}

pub const MEDIA_REF_FORMAT: &str = "rth.media-ref/v1";

pub const KINDS: &[&str] = &[
    "hello",
    "ready",
    "heartbeat",
    "response_started",
    "agent_delta",
    "agent_done",
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
    "attach",
    "user_text",
    "command_submitted",
    "cancel",
    "user_audio",
    "user_audio_end",
    "playback",
    "task_update",
    "executor_status",
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
    fn envelope_round_trips_with_flattened_kind() {
        let line = host().stamp(Event::UserText { request_id: "q1".into(), text: "hi".into() }, None).to_line();
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
        let mut env = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into() }, None);
        env.v = 65535;
        assert_eq!(Envelope::parse_from(env.to_line().trim_end(), Source::Host).unwrap_err(), FrameError::Version(65535));

        let mut spoof = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into() }, None);
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
            Event::CommandSubmitted { command_id: "c1".into(), session_id: "s".into(), expected_generation: 0, text: "hi".into() },
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
        let env = host().stamp(Event::UserText { request_id: "q".into(), text: "x".into() }, None);
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
            (Event::UserAudioEnd { stream_id: "mic-1".into(), chunks: 4, samples: 2560, dropped_chunks: 0 }, "user_audio_end"),
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
}
