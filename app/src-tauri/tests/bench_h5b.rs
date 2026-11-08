
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
use conversation_log::mirror::{Client, Transport};
use conversation_log::observe::{Observation, Payload, Receipt, Snapshot, Span, Terminal};
use conversation_log::{Owner, Store};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};

fn path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
        .join(format!("h5b-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("log")
}
fn rendered(store: &mut Store, nonce: &str) -> String {
    let turn = store.commit(nonce, "A public input.").unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    store.first_rendered(&turn.id).unwrap();
    turn.id
}
fn finish(store: &mut Store, id: &str) {
    store
        .finish_rendered(
            id,
            vec![Span {
                first: 0,
                end: 1600,
                text: "Heard prefix.".into(),
            }],
            Terminal::Interrupted,
        )
        .unwrap();
}
#[derive(Clone, Default)]
struct Receiver(Arc<Mutex<Vec<Observation>>>);
impl Transport for Receiver {
    fn observe(&mut self, item: &Observation) -> io::Result<Receipt> {
        let mut rows = self.0.lock().unwrap();
        if let Some(prior) = rows.iter().find(|r| r.event_id == item.event_id) {
            if prior != item {
                return Err(io::Error::other("conflict"));
            }
            return Ok(prior.receipt());
        }
        assert_eq!(item.version, rows.len() as u64 + 1);
        rows.push(item.clone());
        Ok(item.receipt())
    }
}
#[test]
fn lost_ack_replays_one_observation_after_restart() {
    let path = path();
    let mut store = Store::open(&path, "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let first = store.prepare_observation().unwrap().unwrap();
    let mut receiver = Receiver::default();
    receiver.observe(&first).unwrap();
    drop(store);
    let mut store = Store::open(&path, "session").unwrap();
    assert_eq!(store.prepare_observation().unwrap(), Some(first.clone()));
    store
        .accept_observation(receiver.observe(&first).unwrap())
        .unwrap();
    let assistant = store.prepare_observation().unwrap().unwrap();
    assert!(
        matches!(&assistant.payload, Payload::Assistant { spans, .. } if spans[0].text == "Heard prefix.")
    );
    store
        .accept_observation(receiver.observe(&assistant).unwrap())
        .unwrap();
    assert!(store.prepare_observation().unwrap().is_none());
    assert_eq!(receiver.0.lock().unwrap().len(), 2);
}
#[test]
fn post_playback_conflict_preserves_words_and_blocks_front_until_known_prefix_recovery() {
    let path = path();
    let mut store = Store::open(&path, "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let first = store.prepare_observation().unwrap().unwrap();
    let mut wrong = first.receipt();
    wrong.digest = "foreign".into();
    assert!(store.accept_observation(wrong).is_err());
    assert!(!store.front_admission_allowed());
    assert_eq!(store.turns()[0].input, "A public input.");
    drop(store);
    let mut store = Store::open(&path, "session").unwrap();
    assert!(!store.front_admission_allowed());
    assert!(
        store
            .reconcile_mirror(Snapshot {
                session: "other".into(),
                version: 0,
                tip: String::new()
            })
            .is_err()
    );
    store
        .reconcile_mirror(Snapshot {
            session: "session".into(),
            version: 0,
            tip: String::new(),
        })
        .unwrap();
    assert!(store.front_admission_allowed());
    assert_eq!(store.prepare_observation().unwrap(), Some(first));
}
#[test]
fn assistant_observation_has_only_renderer_spans_and_terminal_is_immutable() {
    let mut store = Store::open(&path(), "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let user = store.prepare_observation().unwrap().unwrap();
    store.accept_observation(user.receipt()).unwrap();
    let assistant = store.prepare_observation().unwrap().unwrap();
    let wire = serde_json::to_string(&assistant).unwrap();
    assert!(wire.contains("Heard prefix."));
    assert!(!wire.contains("generated suffix"));
    assert!(
        store
            .finish_rendered(&id, vec![], Terminal::Completed)
            .is_err()
    );
    assert!(
        store
            .finish_rendered(
                &id,
                vec![Span {
                    first: 0,
                    end: 0,
                    text: "Invented".into()
                }],
                Terminal::Completed
            )
            .is_err()
    );
}
#[test]
fn delayed_receiver_does_not_hold_controller_or_local_render() {
    struct Held {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }
    impl Transport for Held {
        fn observe(&mut self, row: &Observation) -> io::Result<Receipt> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            Ok(row.receipt())
        }
    }
    let (entered, at_receiver) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut client = Client::start(Held {
        entered,
        release: wait,
    })
    .unwrap();
    let mut store = Store::open(&path(), "session").unwrap();
    let id = rendered(&mut store, "one");
    assert!(client.pump(&mut store, 0).unwrap());
    at_receiver.recv().unwrap();
    finish(&mut store, &id);
    store
        .commit("two", "Committed while receiver is held.")
        .unwrap();
    assert_eq!(store.turns().len(), 2);
    release.send(()).unwrap();
    client.close().unwrap();
}
#[test]
fn remote_prefix_loss_replays_without_regressing_local_receipts() {
    let mut store = Store::open(&path(), "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let first = store.prepare_observation().unwrap().unwrap();
    store.accept_observation(first.receipt()).unwrap();
    let second = store.prepare_observation().unwrap().unwrap();
    store.accept_observation(second.receipt()).unwrap();
    store
        .reconcile_mirror(Snapshot {
            session: "session".into(),
            version: 0,
            tip: String::new(),
        })
        .unwrap();
    assert_eq!(store.prepare_observation().unwrap(), Some(first.clone()));
    store.accept_observation(first.receipt()).unwrap();
    assert_eq!(store.prepare_observation().unwrap(), Some(second.clone()));
    store.accept_observation(second.receipt()).unwrap();
    assert!(store.prepare_observation().unwrap().is_none());
}

#[test]
fn stale_never_sent_legacy_is_mirrored_silently_after_restart() {
    let path = path();
    let mut store = Store::open(&path, "session").unwrap();
    let turn = store.commit("old", "A stale committed request.").unwrap();
    store.select_owner(&turn.id, Owner::Legacy).unwrap();
    drop(store);
    let mut store = Store::open(&path, "session").unwrap();
    assert!(
        store.begin_legacy_send(&turn.id).is_err(),
        "restart must never speak old work"
    );
    store.reconcile_without_speech(&turn.id).unwrap();
    let user = store.prepare_observation().unwrap().unwrap();
    store.accept_observation(user.receipt()).unwrap();
    let terminal = store.prepare_observation().unwrap().unwrap();
    assert!(
        matches!(terminal.payload,Payload::Assistant{ref spans,terminal:Terminal::PresentationUnknown} if spans.is_empty())
    );
    assert!(!store.turns()[0].rendered, "no invented playback evidence");
}

#[test]
fn uncertain_front_can_reconcile_without_inventing_audio_or_rerouting() {
    let path = path();
    let mut store = Store::open(&path, "session").unwrap();
    let turn = store
        .commit("uncertain", "Words survive an uncertain presentation.")
        .unwrap();
    store.select_owner(&turn.id, Owner::Front).unwrap();
    drop(store);
    let mut store = Store::open(&path, "session").unwrap();
    store.reconcile_without_speech(&turn.id).unwrap();
    assert!(store.select_owner(&turn.id, Owner::Legacy).is_err());
    for _ in 0..2 {
        let row = store.prepare_observation().unwrap().unwrap();
        store.accept_observation(row.receipt()).unwrap();
    }
    assert!(store.prepare_observation().unwrap().is_none());
    assert!(!store.turns()[0].rendered);
}

#[test]
fn compaction_preserves_pending_receipts_session_and_exclusive_ownership() {
    let path = path();
    let dir = path.parent().unwrap();
    let mut store = Store::open_persisted(dir, "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let user = store.prepare_observation().unwrap().unwrap();
    store.accept_observation(user.receipt()).unwrap();
    let pending = store.prepare_observation().unwrap().unwrap();
    store.compact().unwrap();
    assert!(
        Store::open_persisted(dir, "session").is_err(),
        "rename must not lose the writer fence"
    );
    drop(store);
    let mut store = Store::open_persisted(dir, "session").unwrap();
    assert_eq!(store.session(), "session");
    assert_eq!(store.prepare_observation().unwrap(), Some(pending));
    drop(store);
    assert!(
        Store::open_persisted(dir, "new-session").is_err(),
        "foreign session never overwrites pending history"
    );
}

#[test]
fn observation_ack_is_not_a_brain_ingestion_receipt() {
    use conversation_log::observe::{MemoryOutcome, MemoryReceipt};
    let mut store = Store::open(&path(), "session").unwrap();
    let id = rendered(&mut store, "one");
    let row = store.prepare_observation().unwrap().unwrap();
    let receipt = row.receipt();
    assert!(
        store
            .accept_memory_receipt(MemoryReceipt {
                receipt: receipt.clone(),
                items: vec![],
                outcome: MemoryOutcome::Stored
            })
            .is_err()
    );
    store.accept_observation(receipt.clone()).unwrap();
    assert!(!store.memory_synced(&row.event_id));
    let memory = MemoryReceipt {
        receipt,
        items: vec![("memory-one".into(), 1)],
        outcome: MemoryOutcome::Stored,
    };
    store.accept_memory_receipt(memory.clone()).unwrap();
    store.accept_memory_receipt(memory).unwrap();
    assert!(store.memory_synced(&row.event_id));
    assert_eq!(store.turns()[0].id, id);
}

#[test]
fn reconciliation_can_resolve_the_exact_prepared_receipt_lost_in_transit() {
    let mut store = Store::open(&path(), "session").unwrap();
    let id = rendered(&mut store, "one");
    finish(&mut store, &id);
    let row = store.prepare_observation().unwrap().unwrap();
    store.mark_mirror_conflict().unwrap();
    store
        .reconcile_mirror(Snapshot {
            session: "session".into(),
            version: row.version,
            tip: row.receipt().digest,
        })
        .unwrap();
    assert!(store.front_admission_allowed());
    assert!(matches!(
        store.prepare_observation().unwrap().unwrap().payload,
        Payload::Assistant { .. }
    ));
}

#[test]
fn corrupt_optional_store_has_explicit_legacy_only_status_and_preserves_bytes() {
    let path = path();
    let dir = path.parent().unwrap();
    let log = dir.join("conversation-v2.jsonl");
    std::fs::write(&log, b"corrupt\n").unwrap();
    assert!(matches!(
        Store::for_front(dir, "session"),
        conversation_log::FrontStore::LegacyOnly { .. }
    ));
    assert_eq!(std::fs::read(&log).unwrap(), b"corrupt\n");
}

#[test]
fn wire_negotiation_requires_the_observe_capability_and_session() {
    use conversation_log::mirror::RpcTransport;
    use conversation_log::observe::{Reply, Request, SCHEMA};
    assert!(RpcTransport::new("old", "session", |_: Request| Ok(Reply::Pending)).is_err());
    let mut rpc = RpcTransport::new(SCHEMA, "session", |request| match request {
        Request::Lookup { .. } => Ok(Reply::Pending),
        Request::Observe { observation } => Ok(Reply::Accepted {
            receipt: observation.receipt(),
        }),
        _ => Err(io::Error::other("unexpected request")),
    })
    .unwrap();
    let mut store = Store::open(&path(), "session").unwrap();
    rendered(&mut store, "one");
    let row = store.prepare_observation().unwrap().unwrap();
    assert_eq!(rpc.observe(&row).unwrap(), row.receipt());
    let mut foreign = row;
    foreign.session = "another".into();
    assert!(rpc.observe(&foreign).is_err());
}

#[test]
fn transient_resume_failure_retries_before_front_readiness_can_stay_stuck() {
    struct RetryResume { attempts: mpsc::SyncSender<u8>, n: u8 }
    impl Transport for RetryResume {
        fn observe(&mut self, o: &Observation) -> io::Result<Receipt> { Ok(o.receipt()) }
        fn resume(&mut self, session: &str) -> io::Result<Snapshot> {
            self.n += 1;
            self.attempts.send(self.n).unwrap();
            if self.n == 1 { return Err(io::Error::new(io::ErrorKind::ConnectionReset, "fixture disconnect")); }
            Ok(Snapshot { session:session.into(), version:0, tip:String::new() })
        }
    }
    let mut store=Store::open(&path(),"session").unwrap();
    let (tx,rx)=mpsc::sync_channel(4);
    let mut client=Client::start(RetryResume{attempts:tx,n:0}).unwrap();
    assert!(client.resume("session").unwrap());
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap(),1);
    let at=std::time::Instant::now();let mut retried=false;
    while at.elapsed()<std::time::Duration::from_secs(2) {
        client.pump(&mut store,at.elapsed().as_millis() as u64).unwrap();
        if rx.try_recv()==Ok(2){retried=true;break;}
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    client.close().unwrap();
    assert!(retried,"a failed Resume must retry after backoff instead of pinning Opening forever");
}
