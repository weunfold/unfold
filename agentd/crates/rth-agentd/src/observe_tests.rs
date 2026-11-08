use super::*;
use crate::{context, instance, journal::Journal, memory::Memory, observe_store::Receiver};
use rth_protocol::observe::{Observation, Payload, Span, Terminal};
use std::path::PathBuf;
fn dir() -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("observe-integration-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn user() -> Observation {
    Observation {
        session: "session".into(),
        event_id: "event-one".into(),
        turn_id: "turn-one".into(),
        response_id: "reply-one".into(),
        version: 1,
        parent: String::new(),
        payload: Payload::User {
            input: "Please send a message, but this is observation only.".into(),
        },
    }
}
#[tokio::test]
async fn controller_observe_and_replay_never_send_generation_or_effect_commands() {
    let dir = dir();
    let path = dir.join("journal");
    let (faults, _) = tokio::sync::mpsc::unbounded_channel();
    let (journal, _) = Journal::open(&path, faults, None).unwrap();
    let (backend, mut calls) = tokio::sync::mpsc::channel(8);
    let mut agent = Agent::new(
        journal,
        path.to_string_lossy().into(),
        "guest-test".into(),
        backend,
        instance::Boot::default(),
        "fake".into(),
        "test",
    );
    agent.observe = Some(Arc::new(std::sync::Mutex::new(
        Receiver::open(&dir.join("observe"), "session").unwrap(),
    )));
    let agent = Arc::new(agent);
    *agent.controller.lock().await = Some((7, "controller".into()));
    let item = user();
    handle(
        &agent,
        8,
        Request::Observe {
            observation: item.clone(),
        },
        "observer".into(),
    )
    .await;
    assert!(
        agent
            .observe
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .history()
            .is_empty()
    );
    for _ in 0..2 {
        handle(
            &agent,
            7,
            Request::Observe {
                observation: item.clone(),
            },
            "request".into(),
        )
        .await;
    }
    assert_eq!(agent.journal.turns().len(), 1);
    assert_eq!(agent.journal.turns()[0].text, item.payload.text());
    assert!(
        calls.try_recv().is_err(),
        "Observe must not reach any backend/effect command"
    );
    let mut wrong = item.clone();
    wrong.session = "another".into();
    handle(
        &agent,
        7,
        Request::Observe { observation: wrong },
        "foreign".into(),
    )
    .await;
    assert_eq!(
        agent
            .observe
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .history()
            .len(),
        1
    );
}
#[test]
fn receiver_window_replay_and_context_snapshot_deduplicate_rendered_history() {
    let item = user();
    let mut fold = context::Fold::default();
    fold.observe(&item);
    let bytes = serde_json::to_vec(&fold).unwrap();
    let mut fold: context::Fold = serde_json::from_slice(&bytes).unwrap();
    fold.observe(&item);
    assert_eq!(fold.turns.len(), 1);
    let mut assistant = item.clone();
    assistant.event_id = "rendered-one".into();
    assistant.version = 2;
    assistant.parent = item.receipt().digest;
    assistant.payload = Payload::Assistant {
        spans: vec![Span {
            first: 0,
            end: 100,
            text: "Actual prefix.".into(),
        }],
        terminal: Terminal::Interrupted,
    };
    fold.observe(&assistant);
    fold.observe(&assistant);
    assert_eq!(fold.turns.len(), 2);
    assert_eq!(fold.turns[1].text, "Actual prefix.");
    assert_eq!(fold.turns[1].status.as_deref(), Some("cancelled"));
}
#[test]
fn real_brain_adapter_uses_stable_keys_and_transcript_provenance_after_ack_loss() {
    use rth_memory_brain::{Binding, Brain};
    use std::os::unix::fs::PermissionsExt;
    let dir = dir();
    let script = dir.join("fake-mcp.py");
    std::fs::write(
        &script,
        include_str!("../../rth-memory-brain/tests/fake_brain_mcp.py"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let binding = Binding {
        instance_id: "instance".into(),
        brain_id: "brn-fake".into(),
        binding_rev: 1,
        mcp_bin: script,
        principal: "{}".into(),
        runtime_dir: None,
        index: dir.join("binding"),
        read_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(5),
        no_network: false,
        record: dir.join("record"),
        home: Some(dir.join("fake-home")),
        database: Some("fake-only".into()),
        seam: None,
    };
    Brain::birth(&binding, "fictional-test").unwrap();
    let boot = || {
        Memory::open(
            Box::new(Brain::open(binding.clone()).unwrap()),
            &dir.join("acts"),
            "instance",
            "boot",
            "fake",
        )
        .unwrap()
        .0
    };
    let memory = boot();
    let item = user();
    let fail = dir.join("fake-home/fake-brain-fail");
    std::fs::write(&fail, "brain_remember:after\n").unwrap();
    assert!(
        memory.observe_conversation(&item).is_err(),
        "Brain committed but its answer was lost"
    );
    memory.close();
    drop(memory);
    std::fs::write(&fail, "").unwrap();
    let memory = boot();
    let first = memory.observe_conversation(&item).unwrap();
    memory.close();
    drop(memory);
    let memory = boot();
    let second = memory.observe_conversation(&item).unwrap();
    assert_eq!(first, second);
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("fake-home/fake-brain.json")).unwrap())
            .unwrap();
    let transcripts = state["mems"]
        .as_object()
        .unwrap()
        .values()
        .filter(|m| {
            let rows=m["rows"].as_array().unwrap();
            if rows.last().unwrap()["metadata"]["rth_kind"]=="conversation" { assert_eq!(rows.len(),1); true } else {false}
        })
        .count();
    assert_eq!(
        transcripts,
        first.items.len(),
        "lost write answer never creates a second Brain item"
    );
    let calls = std::fs::read_to_string(dir.join("fake-home/fake-brain-calls.jsonl")).unwrap();
    let written: Vec<serde_json::Value> = calls
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| {
            v["tool"] == "brain_remember"
                && v["arguments"]["metadata"]["rth_kind"] == "conversation"
        })
        .collect();
    assert!(!written.is_empty());
    for call in written {
        assert_eq!(call["arguments"]["extract"], false);
        assert!(
            call["arguments"]["source"]
                .as_str()
                .unwrap()
                .contains("quoted conversation")
        );
    }
    memory.close();
}

#[tokio::test]
async fn blocked_brain_provider_does_not_hold_observe_persistence_or_receipts() {
    use rth_memory::contract::{Failure, Found, Provenance, Provider, Recalled, Saved};
    struct Held {
        inner: rth_memory::ledger::Reference,
        entered: std::sync::mpsc::Sender<()>,
        release: Option<std::sync::mpsc::Receiver<()>>,
    }
    impl Provider for Held {
        fn name(&self) -> &'static str {
            "reference"
        }
        fn remember(&mut self, key: &str, text: &str, from: &Provenance) -> Result<Saved, Failure> {
            if let Some(wait) = self.release.take() {
                self.entered.send(()).unwrap();
                wait.recv().unwrap();
            }
            self.inner.remember(key, text, from)
        }
        fn correct(
            &mut self,
            k: &str,
            id: &str,
            v: u64,
            t: &str,
            p: &Provenance,
        ) -> Result<Saved, Failure> {
            self.inner.correct(k, id, v, t, p)
        }
        fn recall(&mut self, q: &str, n: usize) -> Result<Recalled, Failure> {
            self.inner.recall(q, n)
        }
        fn get(&mut self, id: &str) -> Result<Found, Failure> {
            self.inner.get(id)
        }
        fn history(&mut self, id: &str) -> Result<Vec<Found>, Failure> {
            self.inner.history(id)
        }
    }
    let dir = dir();
    let path = dir.join("journal");
    let (faults, _) = tokio::sync::mpsc::unbounded_channel();
    let (journal, _) = Journal::open(&path, faults, None).unwrap();
    let (backend, mut calls) = tokio::sync::mpsc::channel(8);
    let mut agent = Agent::new(
        journal,
        path.to_string_lossy().into(),
        "guest-test".into(),
        backend,
        instance::Boot::default(),
        "fake".into(),
        "test",
    );
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, hold) = std::sync::mpsc::channel();
    let (ledger, _) =
        rth_memory::ledger::Ledger::open(&dir.join("reference"), "instance", "local", "boot")
            .unwrap();
    let provider = Held {
        inner: rth_memory::ledger::Reference { ledger },
        entered,
        release: Some(hold),
    };
    agent.memory = Some(Arc::new(
        Memory::open(
            Box::new(provider),
            &dir.join("acts"),
            "instance",
            "boot",
            "fake",
        )
        .unwrap()
        .0,
    ));
    agent.observe = Some(Arc::new(std::sync::Mutex::new(
        Receiver::open(&dir.join("observe"), "session").unwrap(),
    )));
    let agent = Arc::new(agent);
    *agent.controller.lock().await = Some((7, "controller".into()));
    attach(&agent);
    let item = user();
    handle(
        &agent,
        7,
        Request::Observe {
            observation: item.clone(),
        },
        "user".into(),
    )
    .await;
    tokio::task::spawn_blocking(move || waiting.recv().unwrap())
        .await
        .unwrap();

    let mut assistant = item.clone();
    assistant.event_id = "assistant".into();
    assistant.version = 2;
    assistant.parent = item.receipt().digest;
    assistant.payload = Payload::Assistant {
        spans: vec![Span {
            first: 0,
            end: 1600,
            text: "Rendered answer.".into(),
        }],
        terminal: Terminal::Completed,
    };
    handle(
        &agent,
        7,
        Request::Observe {
            observation: assistant,
        },
        "assistant".into(),
    )
    .await;
    assert_eq!(agent.journal.turns().len(), 2);
    assert!(calls.try_recv().is_err());
    assert_eq!(
        agent
            .observe
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .pending_memory()
            .len(),
        2
    );
    let mut replies = agent.bus.subscribe();
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let reply = replies.recv().await.unwrap();
            if matches!(
                reply.env.event,
                Event::ObserveReply {
                    reply: Reply::Memory { .. }
                }
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    agent.memory.as_ref().unwrap().close();
}
