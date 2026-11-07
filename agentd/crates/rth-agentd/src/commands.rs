

use std::collections::HashMap;
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rth_protocol::CommandStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TEXT: usize = 8 * 1024;

pub const MAX_RESULT_TEXT: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Record {
    Admitted {
        command_id: String,
        session_id: String,
        generation: u64,

        payload_sha256: String,
        text: String,

        submitted: String,
        boot_id: String,
        utc_ms: u64,
    },
    Dispatched { command_id: String, decision_id: String, decision_envelope: String, response_id: String, boot_id: String, utc_ms: u64 },
    Finished {
        command_id: String,
        status: CommandStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        boot_id: String,
        utc_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub command_id: String,
    pub session_id: String,
    pub generation: u64,
    pub payload_sha256: String,
    pub status: CommandStatus,
    pub decision_id: Option<String>,
    pub decision_envelope: Option<String>,
    pub response_id: Option<String>,
    pub result_text: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Admission {

    Admitted(Entry),

    Duplicate(Entry),

    Refused(String),
}

pub fn payload_sha256(session_id: &str, expected_generation: u64, text: &str) -> String {
    let canon = serde_json::json!({"session_id": session_id, "expected_generation": expected_generation, "text": text});
    let mut h = Sha256::new();
    h.update(canon.to_string().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub struct CommandLog {
    path: PathBuf,
    file: std::fs::File,
    session_id: String,
    boot_id: String,
    entries: HashMap<String, Entry>,
    generation: u64,

    pub crash_after: Option<CrashPoint>,

    poisoned: Option<String>,
    #[cfg(test)]
    pub fault: Option<WriteFault>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFault {

    PartialWrite,

    SyncError,
}

pub const DURABILITY_UNKNOWN: &str = "admission durability unknown";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {

    Admitted,

    Dispatched,
}

impl CrashPoint {
    pub fn from_env() -> Option<Self> {
        match std::env::var("RTH_AGENTD_CRASH_AFTER").ok()?.as_str() {
            "command_admitted" => Some(CrashPoint::Admitted),
            "command_dispatched" => Some(CrashPoint::Dispatched),
            _ => None,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Recovered {
    pub truncated_bytes: u64,
    pub commands: usize,

    pub interrupted: Vec<(String, CommandStatus)>,
}

impl CommandLog {

    pub fn default_path(journal: &Path) -> PathBuf {
        if let Ok(p) = std::env::var("RTH_COMMANDS")
            && !p.is_empty()
        {
            return PathBuf::from(p);
        }
        journal.parent().unwrap_or(Path::new(".")).join("commands.jsonl")
    }

    pub fn open(path: &Path, session_id: &str, boot_id: &str) -> std::io::Result<(Self, Recovered)> {
        let existed = path.exists();
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {

            file.sync_all()?;
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()?;
        }
        let truncated_bytes = crate::journal::repair_tail(&mut file)?.truncated_bytes;
        file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        std::io::BufReader::new(&mut file).read_to_string(&mut text)?;
        let mut log = CommandLog {
            path: path.to_path_buf(),
            file,
            session_id: session_id.to_string(),
            boot_id: boot_id.to_string(),
            entries: HashMap::new(),
            generation: 0,
            crash_after: None,
            poisoned: None,
            #[cfg(test)]
            fault: None,
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Record = serde_json::from_str(line)
                .map_err(|e| std::io::Error::other(format!("{}: line {}: {e}", path.display(), n + 1)))?;
            log.fold(rec);
        }
        let mut pending: Vec<&Entry> = log.entries.values().filter(|e| !e.status.is_terminal()).collect();
        pending.sort_by_key(|e| e.generation);
        let pending: Vec<(String, CommandStatus)> = pending.iter().map(|e| (e.command_id.clone(), e.status)).collect();
        for (id, last) in &pending {
            let detail = match last {
                CommandStatus::Admitted => {
                    "agent restarted after admission, before dispatch: no effect was started; not re-run".to_string()
                }
                _ => "agent restarted after dispatch, before a result was recorded: the effect may have started (unknown); not re-run".to_string(),
            };
            log.write(Record::Finished {
                command_id: id.clone(),
                status: CommandStatus::Interrupted,
                result_text: None,
                detail: Some(detail),
                boot_id: boot_id.to_string(),
                utc_ms: rth_protocol::utc_ms(),
            })?;
        }
        let commands = log.entries.len();
        Ok((log, Recovered { truncated_bytes, commands, interrupted: pending }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get(&self, command_id: &str) -> Option<&Entry> {
        self.entries.get(command_id)
    }

    fn fold(&mut self, rec: Record) {
        match rec {
            Record::Admitted { command_id, session_id, generation, payload_sha256, .. } => {
                self.generation = self.generation.max(generation);
                self.entries.insert(
                    command_id.clone(),
                    Entry {
                        command_id,
                        session_id,
                        generation,
                        payload_sha256,
                        status: CommandStatus::Admitted,
                        decision_id: None,
                        decision_envelope: None,
                        response_id: None,
                        result_text: None,
                        detail: None,
                    },
                );
            }
            Record::Dispatched { command_id, decision_id, decision_envelope, response_id, .. } => {
                if let Some(e) = self.entries.get_mut(&command_id) {
                    e.status = CommandStatus::Dispatched;
                    e.decision_id = Some(decision_id);
                    e.decision_envelope = Some(decision_envelope);
                    e.response_id = Some(response_id);
                }
            }
            Record::Finished { command_id, status, result_text, detail, .. } => {
                if let Some(e) = self.entries.get_mut(&command_id) {
                    e.status = status;
                    e.result_text = result_text;
                    e.detail = detail;
                }
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn poisoned(&self) -> Option<&str> {
        self.poisoned.as_deref()
    }

    fn write(&mut self, rec: Record) -> std::io::Result<()> {
        if let Some(why) = &self.poisoned {
            return Err(std::io::Error::other(format!("{DURABILITY_UNKNOWN}: {why}")));
        }
        let mut line = serde_json::to_string(&rec).map_err(std::io::Error::other)?;
        line.push('\n');
        let result = self.append_line(line.as_bytes());
        match result {
            Ok(()) => {
                self.fold(rec);
                Ok(())
            }
            Err(e) => {
                let why = format!("a command log write or fsync failed ({e}); restart the agent to recover the log");
                self.poisoned = Some(why.clone());
                Err(std::io::Error::other(format!("{DURABILITY_UNKNOWN}: {why}")))
            }
        }
    }

    fn append_line(&mut self, line: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        match self.fault.take() {
            Some(WriteFault::PartialWrite) => {
                self.file.write_all(&line[..line.len() / 2])?;
                return Err(std::io::Error::other("injected: partial write"));
            }
            Some(WriteFault::SyncError) => {
                self.file.write_all(line)?;
                return Err(std::io::Error::other("injected: fsync failed"));
            }
            None => {}
        }
        self.file.write_all(line)?;
        self.file.sync_data()
    }

    pub fn admit(
        &mut self,
        command_id: &str,
        session_id: &str,
        expected_generation: u64,
        text: &str,
        submitted: &str,
    ) -> std::io::Result<Admission> {
        if let Some(why) = &self.poisoned {
            return Ok(Admission::Refused(format!("{DURABILITY_UNKNOWN}: {why}")));
        }
        if command_id.is_empty() || command_id.len() > 128 {
            return Ok(Admission::Refused("command_id must be 1..=128 chars".into()));
        }
        let payload = payload_sha256(session_id, expected_generation, text);
        if let Some(e) = self.entries.get(command_id) {
            return Ok(if e.payload_sha256 == payload {
                Admission::Duplicate(e.clone())
            } else {
                Admission::Refused(format!("command {command_id} was already admitted with a different payload"))
            });
        }
        if session_id != self.session_id {
            return Ok(Admission::Refused(format!("stale session {session_id:?}: the instance's session is {:?}", self.session_id)));
        }
        if expected_generation != self.generation {
            return Ok(Admission::Refused(format!(
                "stale generation: expected {expected_generation}, the session is at {}",
                self.generation
            )));
        }
        if text.trim().is_empty() || text.len() > MAX_TEXT {
            return Ok(Admission::Refused(format!("command text must be 1..={MAX_TEXT} bytes and not blank")));
        }
        let generation = self.generation + 1;
        self.write(Record::Admitted {
            command_id: command_id.into(),
            session_id: session_id.into(),
            generation,
            payload_sha256: payload,
            text: text.into(),
            submitted: submitted.into(),
            boot_id: self.boot_id.clone(),
            utc_ms: rth_protocol::utc_ms(),
        })?;
        Ok(Admission::Admitted(self.entries[command_id].clone()))
    }

    pub fn dispatched(&mut self, command_id: &str, decision_id: &str, decision_envelope: &str, response_id: &str) -> std::io::Result<()> {
        self.write(Record::Dispatched {
            command_id: command_id.into(),
            decision_id: decision_id.into(),
            decision_envelope: decision_envelope.into(),
            response_id: response_id.into(),
            boot_id: self.boot_id.clone(),
            utc_ms: rth_protocol::utc_ms(),
        })
    }

    pub fn finished(&mut self, command_id: &str, status: CommandStatus, result_text: Option<String>, detail: Option<String>) -> std::io::Result<()> {
        if self.entries.get(command_id).is_none_or(|e| e.status.is_terminal()) {
            return Ok(());
        }
        let result_text = result_text.map(|mut t| {
            if t.len() > MAX_RESULT_TEXT {
                let mut cut = MAX_RESULT_TEXT;
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                t.truncate(cut);
            }
            t
        });
        self.write(Record::Finished {
            command_id: command_id.into(),
            status,
            result_text,
            detail,
            boot_id: self.boot_id.clone(),
            utc_ms: rth_protocol::utc_ms(),
        })
    }

    pub fn crash_point(&self, at: CrashPoint) -> bool {
        if self.crash_after == Some(at) {
            if cfg!(test) {
                return true;
            }
            eprintln!("rth-agentd: RTH_AGENTD_CRASH_AFTER={at:?}: exiting after the durable step");
            std::process::exit(86);
        }
        false
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn count_on_disk(path: &Path) -> usize {
        std::fs::File::open(path)
            .map(|f| std::io::BufReader::new(f).lines().map_while(Result::ok).filter(|l| l.contains(r#""op":"admitted""#)).count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rth-cmdlog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("commands.jsonl")
    }

    #[test]
    fn admission_is_durable_dedupes_and_refuses_reuse_and_staleness() {
        let p = tmp("basic");
        let (mut log, rec) = CommandLog::open(&p, "s1", "b1").unwrap();
        assert_eq!(rec, Recovered::default());
        let Admission::Admitted(e) = log.admit("c1", "s1", 0, "hi", "host:1").unwrap() else { panic!() };
        assert_eq!((e.generation, e.status), (1, CommandStatus::Admitted));

        assert_eq!(CommandLog::count_on_disk(&p), 1);
        assert!(matches!(log.admit("c1", "s1", 0, "hi", "host:2").unwrap(), Admission::Duplicate(d) if d.generation == 1));
        assert!(matches!(log.admit("c1", "s1", 0, "hello", "host:3").unwrap(), Admission::Refused(r) if r.contains("different payload")));
        assert!(matches!(log.admit("c2", "s0", 1, "hi", "host:4").unwrap(), Admission::Refused(r) if r.contains("stale session")));
        assert!(matches!(log.admit("c2", "s1", 0, "hi", "host:5").unwrap(), Admission::Refused(r) if r.contains("stale generation")));
        assert!(matches!(log.admit("c2", "s1", 1, " ", "host:6").unwrap(), Admission::Refused(_)));
        assert_eq!(CommandLog::count_on_disk(&p), 1, "refusals write nothing");
        log.dispatched("c1", "b1-d1", "guest-b1:7", "b1-r2").unwrap();
        log.finished("c1", CommandStatus::Completed, Some("hello!".into()), None).unwrap();

        log.finished("c1", CommandStatus::Failed, None, None).unwrap();
        drop(log);
        let (log, rec) = CommandLog::open(&p, "s1", "b2").unwrap();
        assert_eq!((rec.commands, rec.interrupted.len()), (1, 0));
        let e = log.get("c1").unwrap();
        assert_eq!((e.status, e.response_id.as_deref(), e.result_text.as_deref()), (CommandStatus::Completed, Some("b1-r2"), Some("hello!")));
        assert_eq!(log.generation(), 1);
    }

    fn fault_then_recover(name: &str, fault: WriteFault) -> (Recovered, CommandLog) {
        let p = tmp(name);
        let (mut log, _) = CommandLog::open(&p, "s1", "b1").unwrap();
        log.fault = Some(fault);
        let err = log.admit("c1", "s1", 0, "hi", "h:1").unwrap_err();
        assert!(err.to_string().contains(DURABILITY_UNKNOWN), "{err}");
        assert!(log.poisoned().is_some());
        let len = std::fs::metadata(&p).unwrap().len();
        for (id, generation) in [("c1", 0), ("c2", 0), ("c2", 1)] {
            let Admission::Refused(r) = log.admit(id, "s1", generation, "hi", "h:2").unwrap() else { panic!() };
            assert!(r.contains(DURABILITY_UNKNOWN), "{r}");
        }
        assert!(log.dispatched("c1", "d", "e", "r").is_err());
        assert!(log.get("c1").is_none(), "the failed admission is not in memory");
        assert_eq!(std::fs::metadata(&p).unwrap().len(), len, "nothing more was appended");
        drop(log);
        let (log, rec) = CommandLog::open(&p, "s1", "b2").unwrap();
        (rec, log)
    }

    #[test]
    fn a_partial_write_poisons_admission_and_recovery_truncates_the_torn_tail() {
        let (rec, mut log) = fault_then_recover("partial", WriteFault::PartialWrite);
        assert!(rec.truncated_bytes > 0);
        assert_eq!((rec.commands, log.generation()), (0, 0));
        assert!(log.get("c1").is_none());

        assert!(matches!(log.admit("c1", "s1", 0, "hi", "h:3").unwrap(), Admission::Admitted(e) if e.generation == 1));
    }

    #[test]
    fn an_fsync_error_after_a_full_write_poisons_admission_and_recovery_keeps_one_interrupted_admission() {
        let (rec, mut log) = fault_then_recover("fsync", WriteFault::SyncError);
        assert_eq!(rec.truncated_bytes, 0);
        assert_eq!(rec.interrupted, vec![("c1".into(), CommandStatus::Admitted)]);
        let e = log.get("c1").unwrap().clone();
        assert_eq!((e.status, e.generation, e.decision_id), (CommandStatus::Interrupted, 1, None));

        assert!(matches!(log.admit("c1", "s1", 0, "hi", "h:3").unwrap(), Admission::Duplicate(d) if d.status == CommandStatus::Interrupted));
        assert!(matches!(log.admit("c2", "s1", 1, "hi", "h:4").unwrap(), Admission::Admitted(e) if e.generation == 2));
    }

    #[test]
    fn boot_marks_both_crash_windows_interrupted_and_never_reruns() {
        let p = tmp("crash");
        let (mut log, _) = CommandLog::open(&p, "s1", "b1").unwrap();
        log.admit("only-admitted", "s1", 0, "hi", "h:1").unwrap();
        log.admit("dispatched", "s1", 1, "hi again", "h:2").unwrap();
        log.dispatched("dispatched", "b1-d1", "guest-b1:9", "b1-r1").unwrap();
        drop(log);

        std::fs::OpenOptions::new().append(true).open(&p).unwrap().write_all(br#"{"op":"fini"#).unwrap();
        let (log, rec) = CommandLog::open(&p, "s1", "b2").unwrap();
        assert!(rec.truncated_bytes > 0);
        assert_eq!(rec.interrupted, vec![("only-admitted".into(), CommandStatus::Admitted), ("dispatched".into(), CommandStatus::Dispatched)]);
        let a = log.get("only-admitted").unwrap();
        assert_eq!((a.status, a.decision_id.clone()), (CommandStatus::Interrupted, None));
        assert!(a.detail.as_deref().unwrap().contains("no effect was started"));
        let d = log.get("dispatched").unwrap();
        assert_eq!((d.status, d.response_id.as_deref()), (CommandStatus::Interrupted, Some("b1-r1")));
        assert!(d.detail.as_deref().unwrap().contains("may have started"));
        assert_eq!(d.result_text, None, "no result is fabricated");
        assert_eq!(log.generation(), 2);
        drop(log);

        let (_, rec) = CommandLog::open(&p, "s1", "b3").unwrap();
        assert!(rec.interrupted.is_empty());
    }
}
