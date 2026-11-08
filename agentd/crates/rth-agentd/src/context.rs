

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use rth_protocol::deploy::{CONTEXT_NONE, CONTEXT_PARTIAL, CONTEXT_RESTORED, ContextReport, InstanceIdentity};
use rth_protocol::{Envelope, Event, ResponseStatus, utc_ms};
use serde::{Deserialize, Serialize};

pub const SNAPSHOT_FORMAT: &str = "rth.context-snapshot/v1";

pub const MAX_TURNS: usize = 128;

pub const MAX_CONTEXT_BYTES: usize = 32 * 1024;

pub const MAX_TURN_BYTES: usize = 4 * 1024;

pub const REPLY_HEAD_BYTES: usize = 1024;

pub const ELIDED: &str = "\n[…]\n";

const MAX_PENDING: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {

    pub role: String,
    pub text: String,

    pub kind: String,

    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Fold {
    pub turns: VecDeque<Turn>,
    pub truncated_turns: u64,
    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    observed: VecDeque<String>,

    pub lineage_since: u64,

    pending: VecDeque<(String, String, String)>,

    replies: VecDeque<(String, String)>,
    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    openers: VecDeque<(String, String)>,

    speech: VecDeque<(String, String, String)>,

    #[serde(default, skip_serializing_if = "VecDeque::is_empty")]
    uncertain: VecDeque<String>,
}

fn clip(mut s: String) -> String {
    if s.len() > MAX_TURN_BYTES {
        let mut cut = MAX_TURN_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

fn bounded<T>(q: &mut VecDeque<T>, item: T) {
    q.push_back(item);
    while q.len() > MAX_PENDING {
        q.pop_front();
    }
}

impl Fold {
    pub fn starting_at(offset: u64) -> Self {
        Fold { lineage_since: offset, ..Default::default() }
    }

    fn push(&mut self, turn: Turn) {
        self.turns.push_back(Turn { text: clip(turn.text.clone()), ..turn });
        let mut bytes: usize = self.turns.iter().map(|t| t.text.len()).sum();
        while self.turns.len() > MAX_TURNS || bytes > MAX_CONTEXT_BYTES {
            let Some(old) = self.turns.pop_front() else { break };
            bytes -= old.text.len();
            self.truncated_turns += 1;
        }
    }

    pub fn close_open_speech(&mut self, except: Option<&str>) {
        let keep: VecDeque<_> = self.speech.iter().filter(|(k, _, _)| Some(k.as_str()) == except).cloned().collect();
        let closing: Vec<_> = self.speech.drain(..).filter(|(k, _, _)| Some(k.as_str()) != except).collect();
        self.speech = keep;
        for (key, text, event) in closing {
            let boundary = self.take_uncertain(&key);
            if !text.trim().is_empty() {
                self.push(Turn { role: "user".into(), text, kind: "speech".into(), event, response_id: None, status: None, finished: Some(false), boundary });
            }
        }
    }

    fn take_uncertain(&mut self, key: &str) -> Option<String> {
        let i = self.uncertain.iter().position(|k| k == key)?;
        self.uncertain.remove(i);
        Some("provider_turn".into())
    }

    pub fn open_speech(&self) -> usize {
        self.speech.len()
    }

    pub fn observe(&mut self, item: &rth_protocol::observe::Observation) {
        use rth_protocol::observe::{Payload, Terminal};
        let key = format!("{}/{}", item.session, item.event_id);
        if item.validate().is_err() || self.observed.contains(&key) { return; }
        if self.observed.len() >= rth_protocol::observe::MAX_EVENTS { return; }
        self.observed.push_back(key);
        let (role, kind, response_id, status) = match &item.payload {
            Payload::User { .. } => ("user", "observed-user", None, None),
            Payload::Assistant { terminal, .. } => ("model", "observed-rendered", Some(item.response_id.clone()),
                Some(match terminal { Terminal::Completed => "completed", Terminal::Interrupted => "cancelled", Terminal::Failed => "failed", Terminal::PresentationUnknown => "presentation_unknown" }.into())),
        };
        let text = item.payload.text();
        if !text.is_empty() {
            self.push(Turn { role: role.into(), text, kind: kind.into(), event: item.event_id.clone(), response_id,
                status, finished: Some(true), boundary: None });
        }
    }

    pub fn apply(&mut self, env: &Envelope) {
        match &env.event {
            Event::ObserveStored { observation } => self.observe(observation),
            Event::UserText { text, .. } if env.received.is_some() && !crate::effects::is_reserved(text) => bounded(&mut self.pending, (env.id.clone(), text.clone(), "typed".into())),

            Event::CommandSubmitted { text, .. } if env.received.is_some() && !crate::effects::is_reserved(text) => {
                bounded(&mut self.pending, (env.id.clone(), text.clone(), "command".into()))
            }
            Event::Decision { trigger_event, .. } => {
                if let Some(i) = self.pending.iter().position(|(id, _, _)| id == trigger_event) {
                    let (id, text, kind) = self.pending.remove(i).unwrap();
                    self.push(Turn { role: "user".into(), text, kind, event: id, response_id: None, status: None, finished: None, boundary: None });
                }
            }
            Event::InputTranscript { text, utterance_id, is_final, boundary, .. } => {

                let key = utterance_id.clone().unwrap_or_default();
                if matches!(boundary.as_deref(), Some("local" | "provider_turn")) {
                    self.close_open_speech(Some(&key));
                }
                if boundary.as_deref() == Some("provider_turn") && !self.uncertain.contains(&key) {
                    bounded(&mut self.uncertain, key.clone());
                }
                match self.speech.iter_mut().find(|(k, _, _)| *k == key) {
                    Some((_, acc, _)) => {
                        if acc.len() < MAX_TURN_BYTES {
                            acc.push_str(text);
                        }
                    }
                    None => bounded(&mut self.speech, (key.clone(), text.clone(), env.id.clone())),
                }
                if *is_final == Some(true)
                    && let Some(i) = self.speech.iter().position(|(k, _, _)| *k == key)
                {
                    let (_, text, event) = self.speech.remove(i).unwrap();
                    let boundary = self.take_uncertain(&key);
                    if !text.trim().is_empty() {
                        self.push(Turn { role: "user".into(), text, kind: "speech".into(), event, response_id: None, status: None, finished: Some(true), boundary });
                    }
                }
            }
            Event::AgentOpener { response_id, text } => {
                if !self.openers.iter().any(|(r, _)| r == response_id) {
                    bounded(&mut self.openers, (response_id.clone(), text.clone()));
                }
            }
            Event::AgentDelta { response_id, text } => match self.replies.iter_mut().find(|(r, _)| r == response_id) {
                Some((_, acc)) => fold_reply(acc, text),
                None => {
                    let mut acc = String::new();
                    fold_reply(&mut acc, text);
                    bounded(&mut self.replies, (response_id.clone(), acc));
                }
            },

            Event::AgentReplace { response_id, text } => {
                let mut acc = String::new();
                fold_reply(&mut acc, text);
                match self.replies.iter_mut().find(|(r, _)| r == response_id) {
                    Some((_, a)) => *a = acc,
                    None => bounded(&mut self.replies, (response_id.clone(), acc)),
                }
            }
            Event::AgentDone { response_id, status } => {
                if let Some(i) = self.openers.iter().position(|(r, _)| r == response_id) {
                    let (_, prefix) = self.openers.remove(i).unwrap();
                    match self.replies.iter_mut().find(|(r, _)| r == response_id) {
                        Some((_, acc)) => *acc = clip(format!("{prefix} {acc}")),
                        None => bounded(&mut self.replies, (response_id.clone(), prefix)),
                    }
                }
                if let Some(i) = self.replies.iter().position(|(r, _)| r == response_id) {
                    let (rid, text) = self.replies.remove(i).unwrap();
                    if !text.is_empty() {
                        let status = match status {
                            ResponseStatus::Completed => "completed",
                            ResponseStatus::Cancelled => "cancelled",
                            ResponseStatus::Failed => "failed",
                        };
                        self.push(Turn {
                            role: "model".into(),
                            text,
                            kind: "reply".into(),
                            event: env.id.clone(),
                            response_id: Some(rid),
                            status: Some(status.into()),
                            finished: None,
                            boundary: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

fn fold_reply(acc: &mut String, text: &str) {
    if acc.len() + text.len() <= MAX_TURN_BYTES {
        acc.push_str(text);
        return;
    }

    let from = floor_boundary(acc, REPLY_HEAD_BYTES.saturating_sub(3).min(acc.len()));
    let to = floor_boundary(acc, (REPLY_HEAD_BYTES + ELIDED.len()).min(acc.len()));
    let mark = acc[from..to].find(ELIDED).map(|i| from + i);
    let (head, rest) = if let Some(at) = mark.filter(|_| acc.len() > REPLY_HEAD_BYTES) {
        (acc[..at].to_string(), format!("{}{text}", &acc[at + ELIDED.len()..]))
    } else {
        let whole = format!("{acc}{text}");
        let h = floor_boundary(&whole, REPLY_HEAD_BYTES);
        (whole[..h].to_string(), whole[h..].to_string())
    };
    let tail_room = MAX_TURN_BYTES - head.len() - ELIDED.len();
    let cut = ceil_boundary(&rest, rest.len().saturating_sub(tail_room));
    *acc = format!("{head}{ELIDED}{}", &rest[cut..]);
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    pub journal_bytes: u64,
    pub prefix_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub snapshot_format: String,
    pub instance_id: String,
    pub session_id: String,
    pub watermark: Watermark,
    pub fold: Fold,
    pub written_utc_ms: u64,

    pub written_by: String,
}

pub fn default_path(journal: &Path) -> PathBuf {
    match std::env::var_os("RTH_CONTEXT") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => journal.parent().unwrap_or(Path::new(".")).join("context.json"),
    }
}

pub enum Loaded {
    Candidate(Box<Snapshot>),
    Missing,
    Unusable(String),
}

pub fn load(path: &Path, instance: Option<&InstanceIdentity>) -> Loaded {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(e) => return Loaded::Unusable(format!("snapshot unreadable: {e}")),
    };
    let v: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return Loaded::Unusable(format!("snapshot is corrupt: {e}")),
    };
    if v.get("snapshot_format").and_then(|f| f.as_str()) != Some(SNAPSHOT_FORMAT) {
        return Loaded::Unusable(format!("snapshot format {} is not {SNAPSHOT_FORMAT} (incompatible)", v.get("snapshot_format").unwrap_or(&serde_json::Value::Null)));
    }
    let snap: Snapshot = match serde_json::from_value(v) {
        Ok(s) => s,
        Err(e) => return Loaded::Unusable(format!("snapshot is corrupt: {e}")),
    };
    let Some(inst) = instance else { return Loaded::Unusable("no instance session to check the snapshot against".into()) };
    if snap.instance_id != inst.instance_id || snap.session_id != inst.session_id {
        return Loaded::Unusable(format!("snapshot belongs to {} / {}, not this instance session (incompatible)", snap.instance_id, snap.session_id));
    }
    Loaded::Candidate(Box::new(snap))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {

    Fake,

    Gemini,

    OpenAIRealtime,

    Local,
}

impl BackendKind {

    pub fn history_wire(self) -> &'static str {
        match self {
            BackendKind::Fake => "backend state",
            BackendKind::Gemini => "clientContent text history",
            BackendKind::OpenAIRealtime => "conversation.item.create text items",
            BackendKind::Local => "the transcript sent with each request",
        }
    }
}

pub struct Decided {
    pub fold: Fold,
    pub report: ContextReport,
}

fn none(detail: String, journal_len: u64) -> Decided {
    let report = ContextReport {
        status: CONTEXT_NONE.into(),
        method: "none".into(),
        snapshot: None,
        watermark: None,
        lineage_since: Some(journal_len),
        turns: 0,
        tail_records: 0,
        truncated_turns: 0,
        detail: format!("{detail}; context starts empty, history is retained"),
    };
    Decided { fold: Fold::starting_at(journal_len), report }
}

pub fn decide(loaded: Loaded, rec: &crate::journal::Recovery, backend: BackendKind) -> Decided {
    let snap = match loaded {
        Loaded::Missing => return none("no snapshot".into(), rec.len),
        Loaded::Unusable(why) => return none(why, rec.len),
        Loaded::Candidate(s) => s,
    };
    let w = &snap.watermark;
    if w.journal_bytes > rec.len {
        return none(format!("stale snapshot: it covers {} journal bytes, the journal has {}", w.journal_bytes, rec.len), rec.len);
    }
    if rec.prefix_sha256.as_deref() != Some(w.prefix_sha256.as_str()) {
        return none(format!("the journal's first {} bytes no longer hash to the snapshot's prefix", w.journal_bytes), rec.len);
    }
    let Some(tail) = &rec.tail else {
        return none(format!("journal tail after the watermark is over {} bytes; not folded", crate::journal::MAX_TAIL_BYTES), rec.len);
    };
    let mut fold = snap.fold.clone();
    let (mut folded, mut unreadable) = (0u64, 0u64);
    for line in tail.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        match std::str::from_utf8(line).ok().and_then(|l| serde_json::from_str::<Envelope>(l).ok()) {
            Some(env) => {
                fold.apply(&env);
                folded += 1;
            }
            None => unreadable += 1,
        }
    }
    let mut detail = format!(
        "snapshot by {} at {} journal bytes; {folded} tail records folded; lineage since journal byte {} (earlier history was never in this context)",
        snap.written_by, w.journal_bytes, fold.lineage_since
    );
    if unreadable > 0 {
        detail += &format!("; {unreadable} tail records did not parse and were skipped");
    }

    let open = fold.open_speech();
    if open > 0 {
        fold.close_open_speech(None);
        detail += &format!("; {open} unfinished utterances closed at boot (finished=false)");
    }
    let turns = fold.turns.len() as u64;
    let exact = if unreadable > 0 { CONTEXT_PARTIAL } else { CONTEXT_RESTORED };
    let (status, method) = match backend {
        BackendKind::Fake | BackendKind::Local => (exact, "backend_state"),
        BackendKind::Gemini | BackendKind::OpenAIRealtime if turns == 0 => {
            detail += "; the lineage has no turns, so there is nothing to give back";
            (exact, "none")
        }
        BackendKind::Gemini | BackendKind::OpenAIRealtime => {
            detail += &format!("; pending: the turns are sent as {} when the provider session is ready", backend.history_wire());
            (CONTEXT_NONE, "text_history")
        }
    };
    let report = ContextReport {
        status: status.into(),
        method: method.into(),
        snapshot: Some(SNAPSHOT_FORMAT.into()),
        watermark: Some(w.journal_bytes),
        lineage_since: Some(fold.lineage_since),
        turns,
        tail_records: folded,
        truncated_turns: fold.truncated_turns,
        detail,
    };
    Decided { fold, report }
}

pub fn set_aside(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    let aside = path.with_extension(format!("rejected-{}.json", utc_ms()));
    std::fs::rename(path, &aside).ok().map(|_| aside)
}

pub fn write(path: &Path, snap: &Snapshot) -> std::io::Result<()> {
    crate::instance::write_atomic(path, &serde_json::to_vec(snap).expect("snapshot serializes"))
}

pub fn remembered(turns: &[Turn]) -> Option<String> {
    turns.iter().rev().filter(|t| t.role == "user").find_map(|t| {
        let lower = t.text.to_lowercase();
        let at = lower.find("remember")?;
        if lower.contains("what") && lower.contains('?') {
            return None;
        }
        let rest = t.text[at + "remember".len()..].trim_start_matches([':', ',', ' ']);
        let rest = rest.strip_prefix("that ").unwrap_or(rest).trim().trim_end_matches(['.', '!']);
        (!rest.is_empty()).then(|| rest.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::{MonotonicClock, Received, Source, Stamper};

    #[test]
    fn a_long_reply_folds_to_its_head_and_its_end() {
        let mut short = String::new();
        fold_reply(&mut short, "Hi there.");
        assert_eq!(short, "Hi there.");
        let text: String = (0..2000).map(|i| format!("Frase número {i} está aqui. ")).collect();
        for piece in [7usize, 300, 9000] {
            let mut acc = String::new();
            let mut rest = text.as_str();
            while !rest.is_empty() {
                let mut n = piece.min(rest.len());
                while !rest.is_char_boundary(n) {
                    n += 1;
                }
                fold_reply(&mut acc, &rest[..n]);
                rest = &rest[n..];
            }
            assert!(acc.len() <= MAX_TURN_BYTES, "{piece}: {}", acc.len());
            assert!(acc.starts_with("Frase número 0 está aqui.") && acc.ends_with("Frase número 1999 está aqui. "), "{piece}");
            assert_eq!(acc.matches(ELIDED).count(), 1, "{piece}");
            assert!(text.starts_with(&acc[..acc.find(ELIDED).unwrap()]) && text.ends_with(&acc[acc.find(ELIDED).unwrap() + ELIDED.len()..]), "{piece}");
        }
    }

    fn host_text(h: &mut Stamper, text: &str) -> Envelope {
        let mut e = h.stamp(Event::UserText { request_id: "q".into(), text: text.into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None);
        e.received = Some(Received { by: "guest".into(), t_ns: 1 });
        e
    }

    fn decision(g: &mut Stamper, trigger: &str) -> Envelope {
        g.stamp(
            Event::Decision {
                decision_id: "d".into(),
                episode_id: "e".into(),
                trigger_event: trigger.into(),
                context_rev: 1,
                candidate_schema: "c".into(),
                candidates: vec![],
                selected: "respond".into(),
                policy: "p".into(),
                latency_ns: 0,
                training_eligibility: "unknown".into(),
                utterance_id: None,
            },
            None,
        )
    }

    #[test]
    fn a_replace_drops_the_reply_so_far_from_history() {
        let mut g = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
        let mut f = Fold::default();
        f.apply(&g.stamp(Event::AgentDelta { response_id: "r1".into(), text: "Ana buys 7 pens. ".repeat(40) }, None));
        f.apply(&g.stamp(Event::AgentReplace { response_id: "r1".into(), text: "4".into() }, None));
        f.apply(&g.stamp(Event::AgentDelta { response_id: "r1".into(), text: " euros.".into() }, None));
        f.apply(&g.stamp(Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Completed }, None));
        assert_eq!(f.turns.iter().map(|t| (t.role.as_str(), t.text.as_str())).collect::<Vec<_>>(), vec![("model", "4 euros.")]);
    }

    #[test]
    fn the_mark_after_a_straddling_char_is_found_again() {

        for (n, c) in [(REPLY_HEAD_BYTES - 1, "é"), (REPLY_HEAD_BYTES - 2, "…"), (REPLY_HEAD_BYTES - 3, "😀")] {
            let mut acc = String::new();
            fold_reply(&mut acc, &format!("{}{c}", "a".repeat(n)));
            for i in 0..300 {
                fold_reply(&mut acc, &format!("piece {i} of the reply. "));
            }
            let at = acc.find(ELIDED).expect("one mark");
            assert_eq!(acc[..at], "a".repeat(n), "{c}: the head is the bytes before the char");
            assert_eq!(acc.matches(ELIDED).count(), 1, "{c}");
            assert!(!acc.contains("[…]\n[…]") && acc.ends_with("piece 299 of the reply. ") && acc.len() <= MAX_TURN_BYTES, "{c}");
        }
    }

    #[test]
    fn the_fold_keeps_decided_inputs_and_finished_replies_only() {
        let mut h = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut g = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
        let mut f = Fold::default();
        let q1 = host_text(&mut h, "remember that the code word is heron");
        f.apply(&q1);
        assert!(f.turns.is_empty(), "no turn before the decision");
        f.apply(&decision(&mut g, &q1.id));
        f.apply(&g.stamp(Event::AgentDelta { response_id: "r1".into(), text: "OK, ".into() }, None));
        f.apply(&g.stamp(Event::AgentDelta { response_id: "r1".into(), text: "noted.".into() }, None));
        assert_eq!(f.turns.len(), 1, "an unfinished reply is not a turn yet");
        f.apply(&g.stamp(Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Completed }, None));

        f.apply(&host_text(&mut h, "remember that it is a trap"));
        assert_eq!(f.turns.iter().map(|t| (t.role.as_str(), t.text.as_str())).collect::<Vec<_>>(), vec![("user", "remember that the code word is heron"), ("model", "OK, noted.")]);
        assert_eq!(remembered(&f.turns.iter().cloned().collect::<Vec<_>>()).as_deref(), Some("the code word is heron"));
    }

    #[test]
    fn the_context_is_bounded_and_counts_what_it_dropped() {
        let mut h = Stamper::new(Source::Host, "host-t", Box::new(MonotonicClock::start()));
        let mut g = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
        let mut f = Fold::default();
        for i in 0..(MAX_TURNS + 10) {
            let q = host_text(&mut h, &format!("message {i}"));
            f.apply(&q);
            f.apply(&decision(&mut g, &q.id));
        }
        assert_eq!(f.turns.len(), MAX_TURNS);
        assert_eq!(f.truncated_turns, 10);
        assert_eq!(f.turns.front().unwrap().text, "message 10");
    }

    fn speech(g: &mut Stamper, uid: &str, seq: u64, text: &str, fin: bool) -> Envelope {
        g.stamp(
            Event::InputTranscript {
                text: text.into(),
                utterance_id: Some(uid.into()),
                utterance_seq: Some(seq),
                is_final: fin.then_some(true),
                response_id: None,
                association: Some("unknown".into()),
                boundary: None,
            },
            None,
        )
    }

    #[test]
    fn an_utterance_split_by_an_unrelated_reply_stays_one_turn() {
        let mut g = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
        let envs = vec![
            speech(&mut g, "b-u1", 0, "remember the code ", false),
            g.stamp(Event::AgentDelta { response_id: "rA".into(), text: "unrelated".into() }, None),
            g.stamp(Event::AgentDone { response_id: "rA".into(), status: ResponseStatus::Completed }, None),
            speech(&mut g, "b-u1", 1, "word is violet", true),
        ];
        let mut straight = Fold::default();
        envs.iter().for_each(|e| straight.apply(e));
        let got: Vec<_> = straight.turns.iter().map(|t| (t.role.as_str(), t.text.as_str(), t.finished)).collect();
        assert_eq!(got, vec![("model", "unrelated", None), ("user", "remember the code word is violet", Some(true))]);

        for cut in 0..=envs.len() {
            let mut snap = Fold::default();
            envs[..cut].iter().for_each(|e| snap.apply(e));
            let mut resumed: Fold = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
            envs[cut..].iter().for_each(|e| resumed.apply(e));
            assert_eq!(resumed, straight, "cut at {cut}");
        }

        for label in ["local", "provider_turn"] {
            let mut f = Fold::default();
            f.apply(&speech(&mut g, "b-u2", 0, "half a thought", false));
            let mut next = speech(&mut g, "b-u3", 0, "new", false);
            if let Event::InputTranscript { boundary, .. } = &mut next.event {
                *boundary = Some(label.into());
            }
            f.apply(&next);
            assert_eq!(f.turns.back().map(|t| (t.text.as_str(), t.finished)), Some(("half a thought", Some(false))), "{label}");
            assert_eq!(f.open_speech(), 1);
        }
    }

    #[test]
    fn an_uncertain_beginning_survives_snapshots_and_old_snapshots_still_load() {
        let mut g = Stamper::new(Source::Guest, "guest-t", Box::new(MonotonicClock::start()));
        let mut first = speech(&mut g, "b-u2", 0, "delete ", false);
        if let Event::InputTranscript { boundary, .. } = &mut first.event {
            *boundary = Some("provider_turn".into());
        }
        let envs = [speech(&mut g, "b-u1", 0, "do not ", false), first, speech(&mut g, "b-u2", 1, "everything", true)];
        let mut straight = Fold::default();
        envs.iter().for_each(|e| straight.apply(e));
        let got: Vec<_> = straight.turns.iter().map(|t| (t.text.as_str(), t.finished, t.boundary.as_deref())).collect();
        assert_eq!(got, vec![("do not ", Some(false), None), ("delete everything", Some(true), Some("provider_turn"))]);
        let turns: Vec<_> = straight.turns.iter().cloned().collect();
        assert!(crate::gemini::settled_history(&turns).0.is_empty(), "neither the prefix nor the uncertain tail is restored");
        for cut in 0..=envs.len() {
            let mut snap = Fold::default();
            envs[..cut].iter().for_each(|e| snap.apply(e));
            let mut resumed: Fold = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
            envs[cut..].iter().for_each(|e| resumed.apply(e));
            assert_eq!(resumed, straight, "cut at {cut}");
        }

        let empty = serde_json::to_string(&Fold::default()).unwrap();
        assert!(!empty.contains("uncertain"), "{empty}");
        let old_turn = r#"{"role":"user","text":"hi","kind":"speech","event":"e","finished":true}"#;
        let t: Turn = serde_json::from_str(old_turn).unwrap();
        assert_eq!(t.boundary, None);
        assert_eq!(serde_json::to_string(&t).unwrap(), old_turn, "an unmarked turn keeps its old bytes");
    }
}
