use super::*;
use crate::front_native::{Page, PageEvent, Submission};
use crate::voice_front::{
    runtime::{Epoch, Speech, SpeechRequest, Ticket},
    worker::{Update, Worker},
};
use std::sync::{atomic::Ordering, mpsc, Arc, Mutex};

fn isolated(name: &str) -> bool {
    if std::env::var_os("RTH_F2D_CHILD").is_some() {
        return false;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("chat::pre_enable_tests::{name}"),
            "--test-threads",
            "4",
            "--nocapture",
        ])
        .env("RTH_F2D_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}
fn environment(enabled: bool) -> (std::path::PathBuf, Log) {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_TARGET_DIR")
            .unwrap_or_else(|| std::env::temp_dir().into_os_string()),
    )
    .join(format!(
        "front-pre-enable-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let log = Log::open_in(None, &root);
    assert!(crate::settings::CTX
        .set(crate::settings::Ctx {
            data: root.clone(),
            contents: None,
            instance: None,
            version: "test".into(),
            log: log.clone()
        })
        .is_ok());
    set_flag(enabled);
    (root, log)
}
fn set_flag(enabled: bool) {
    let c = crate::settings::CTX.get().unwrap();
    let mut settings = crate::settings::current();
    settings.voice_front_templates = enabled;
    settings.language = "en".into();
    crate::settings::save(&c.data, &settings).unwrap();
}
fn chat() -> (Chat, TcpStream) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let observe = Arc::new(crate::voice_front::link::Link::new(
        "session",
        front_observe_frame,
    ));
    observe.on_frame(&json!({"kind":"observe_ready","schema":crate::conversation_log::observe::SCHEMA,"session":"session"}));
    let mut chat = Chat {
        front: Default::default(),
        observe,
        conn: Arc::new(Mutex::new(socket)),
        alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        session: Some(Arc::new(Mutex::new(Session {
            id: "session".into(),
            generation: 1,
            boot: "test".into(),
            n: 0,
            last: None,
            undos: vec![],
        }))),
        #[cfg(feature = "menu_bar")]
        bind: listener.local_addr().unwrap(),
        awaiting: Default::default(),
        fence: Default::default(),
    };
    chat.front.bind("session");
    (chat, peer)
}
fn submission(epoch: Epoch) -> Submission<'static> {
    Submission {
        text: "Can you access files?",
        wire: "Can you access files?",
        interruption: None,
        reply_voice: Some(json!({"dictated":true,"spoken":"full"})),
        epoch,
    }
}
struct Mirror;
impl crate::conversation_log::mirror::Transport for Mirror {
    fn resume(&mut self, s: &str) -> std::io::Result<crate::conversation_log::observe::Snapshot> {
        Ok(crate::conversation_log::observe::Snapshot {
            session: s.into(),
            version: 0,
            tip: String::new(),
        })
    }
    fn observe(
        &mut self,
        o: &crate::conversation_log::observe::Observation,
    ) -> std::io::Result<crate::conversation_log::observe::Receipt> {
        Ok(o.receipt())
    }
}
struct SpeechProbe {
    owner: Arc<Mutex<Option<Epoch>>>,
    tx: mpsc::SyncSender<(bool, Ticket)>,
}
impl Speech for SpeechProbe {
    fn start(&mut self, r: SpeechRequest) -> crate::voice_front::Result<()> {
        self.tx
            .send((
                *self.owner.lock().unwrap() == Some(r.ticket.epoch()),
                r.ticket,
            ))
            .unwrap();
        Ok(())
    }
    fn stop(&mut self, _: &Ticket) {}
}
fn event(rx: &mpsc::Receiver<Update>, wanted: impl Fn(&Update) -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let e = rx
            .recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
            .expect("bounded pre-enable event");
        if wanted(&e) {
            return;
        }
    }
}
#[test]
fn enqueue_publishes_owner_and_following_typed_turn_fences_front() {
    if isolated("enqueue_publishes_owner_and_following_typed_turn_fences_front") {
        return;
    }
    let (root, log) = environment(true);
    let (mut chat, _peer) = chat();
    let reader = chat.front.capture().reader();
    reader.activate();
    let (tx, rx) = mpsc::channel();
    let (played, tickets) = mpsc::sync_channel(1);
    let path = root.join("composition");
    std::fs::create_dir(&path).unwrap();
    let worker = Arc::new(
        Worker::start(
            "session".into(),
            chat.front.capture(),
            move || crate::conversation_log::Store::for_front(&path, "session"),
            SpeechProbe {
                owner: chat.front.route().owner.clone(),
                tx: played,
            },
            Mirror,
            move |u| {
                let _ = tx.send(u);
            },
        )
        .unwrap(),
    );
    event(&rx, |e| matches!(e, Update::Status("Ready")));
    let before_submit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let checked = before_submit.clone();
    let route = chat.front.route();
    worker.review_before_submit(move |epoch| {
        checked.store(
            *route.owner.lock().unwrap() == Some(epoch),
            Ordering::SeqCst,
        );
    });
    chat.front.worker = Some(worker.clone());
    let page: Page = Arc::new(|_| {});
    crate::conversation::review_busy_front(|epoch| {
        assert!(
            chat.front_enqueue(submission(epoch), &page, &log).is_some(),
            "real front_enqueue must admit this ready fixed-line turn"
        );
        assert!(
            before_submit.load(Ordering::SeqCst),
            "native owner must be published at the actual Worker::submit boundary"
        );
        let (owned, ticket) = tickets.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            owned,
            "front_enqueue must publish its native owner before worker admission"
        );
        assert!(ticket.render(epoch, ticket.response(), 4));
        assert_eq!(crate::conversation::typed("Newer typed input."), Ok(true));
        assert!(
            ticket.closed(),
            "the following typed turn must stop the actual enqueued front ticket"
        );
        assert!(chat.front.route().owner.lock().unwrap().is_none());
    });
    worker.close();
    event(&rx, |e| matches!(e, Update::Status("Closed")));
}
#[test]
fn flag_off_enqueue_and_capture_have_no_native_side_effects() {
    if isolated("flag_off_enqueue_and_capture_have_no_native_side_effects") {
        return;
    }
    let (root, log) = environment(false);
    let (mut chat, _peer) = chat();
    let (tx, rx) = mpsc::channel();
    let page: Page = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    for live in [false, true] {
        let reader = live.then(|| chat.front.capture().reader());
        if let Some(r) = &reader {
            r.activate();
        }
        assert!(chat
            .front_enqueue(
                submission(Epoch {
                    capture: 1,
                    turn: 1
                }),
                &page,
                &log
            )
            .is_none());
        assert!(
            chat.front.worker.is_none(),
            "flag off must not install a worker"
        );
        assert!(
            chat.front.route().owner.lock().unwrap().is_none(),
            "flag off must not publish an owner"
        );
        assert!(
            !root.join("conversation-front").exists(),
            "flag off must not create a Store directory"
        );
        assert!(
            crate::front_native::capture_reader().is_none(),
            "flag off must not attach a microphone reader"
        );
        assert!(
            rx.try_recv().is_err(),
            "flag off enqueue must not display front status"
        );
        assert_eq!(
            chat.front.capture().metrics()["legacy_capture_fallbacks"],
            0,
            "flag off must not count front capture fallback"
        );
    }
}
#[test]
fn sessionless_typed_reply_survives_flag_on_and_order_refusals_are_visible() {
    if isolated("sessionless_typed_reply_survives_flag_on_and_order_refusals_are_visible") {
        return;
    }
    sessionless_reader_case(false);
}
#[test]
fn reader_counts_and_logs_only_refused_response_starts() {
    if isolated("reader_counts_and_logs_only_refused_response_starts") {
        return;
    }
    sessionless_reader_case(true);
}
fn sessionless_reader_case(register_expected: bool) {
    let (root, log) = environment(true);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (done, closed) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        writeln!(peer,"{}",json!({"v":2,"kind":"hello","source":"guest","backend":"local@test","boot_id":"test-4242"})).unwrap();
        let mut r = BufReader::new(peer.try_clone().unwrap());
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap()["kind"],
            "attach"
        );
        writeln!(peer, "{}", json!({"kind":"ready","role":"controller"})).unwrap();
        line.clear();
        r.read_line(&mut line).unwrap();
        let input: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(input["kind"], "user_text");
        for f in [
            json!({"kind":"response_started","request_id":"stale","response_id":"stale-reply"}),
            json!({"kind":"agent_delta","response_id":"stale-reply","text":"stale"}),
            json!({"kind":"response_started","request_id":input["request_id"],"response_id":"typed-reply"}),
            json!({"kind":"agent_delta","response_id":"typed-reply","text":"Public typed reply."}),
            json!({"kind":"agent_done","response_id":"typed-reply"}),
        ] {
            writeln!(peer, "{f}").unwrap();
        }

        line.clear();
        r.read_line(&mut line).unwrap();
        let cancel: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(cancel["kind"], "cancel");
        assert_eq!(cancel["response_id"], "stale-reply");
        done.send(()).unwrap();
    });
    let (page, events) = mpsc::channel();
    let sink: Sink = Arc::new(move |v| {
        let _ = page.send(v);
    });
    let (close_tx, close_rx) = mpsc::sync_channel(1);
    let mut chat = Chat::connect(
        addr,
        4242,
        "test",
        sink,
        &log,
        Some(Box::new(move || {
            let _ = close_tx.send(());
        })),
    )
    .unwrap();
    assert!(chat.session.is_none());
    crate::front_native::new_turn(|| {
        if register_expected {
            chat.front
                .order
                .lock()
                .unwrap()
                .expect(&format!("q{}", SEQ.load(Ordering::SeqCst) + 1));
        }
        chat.say("Public typed input.", None)
    })
    .unwrap();
    closed.recv_timeout(Duration::from_secs(4)).unwrap();
    server.join().unwrap();
    close_rx.recv_timeout(Duration::from_secs(4)).unwrap();
    let rows: Vec<_> = events.try_iter().collect();
    assert!(
        rows.iter().any(|v| v["delta"] == "Public typed reply."),
        "flag-on sessionless typed reply must be delivered, not silently cancelled: {rows:?}"
    );
    assert!(
        !chat.waiting(),
        "the matching typed reply must clear awaiting"
    );

    let log_lines = std::fs::read_to_string(root.join("app.log")).unwrap();
    if register_expected {
        assert_eq!(
            log_lines
                .lines()
                .filter(|line| line.contains("voice_front: order_refusals=1"))
                .count(),
            1,
            "refused response_started must be counted/logged exactly once"
        );
        assert!(
            !log_lines.contains("voice_front: order_refusals=2"),
            "stale chunks must not inflate the refused-start counter"
        );
    }
}
#[test]
fn before_typed_does_not_supersede_another_conversation_owner() {
    if isolated("before_typed_does_not_supersede_another_conversation_owner") {
        return;
    }
    let (_root, _log) = environment(true);
    let (chat, _peer) = chat();
    crate::conversation::review_busy_front(|epoch| {
        let foreign = Epoch {
            capture: epoch.capture,
            turn: epoch.turn + 1,
        };
        chat.front.accepted(foreign);
        let ticket = Ticket::new(foreign, "unrelated".into());
        let capture = chat.front.capture();
        capture.publish(&ticket, 0).unwrap();
        crate::front_native::before_typed();
        assert!(
            !ticket.closed(),
            "typed admission must not stop an owner from a different Conversation epoch"
        );
        assert_eq!(
            capture.revision(),
            0,
            "unrelated owner must keep its capture revision"
        );
        assert_eq!(*chat.front.route().owner.lock().unwrap(), Some(foreign));
        assert_eq!(crate::conversation::front_epoch(), Some(epoch));
    });
}
#[test]
fn newer_turn_fences_already_read_old_audio_and_flag_off_restores_legacy_order() {
    if isolated("newer_turn_fences_already_read_old_audio_and_flag_off_restores_legacy_order") {
        return;
    }
    let (_root, _log) = environment(true);
    let (chat, _peer) = chat();
    let epoch = Epoch {
        capture: 1,
        turn: 1,
    };
    let old = Ticket::new(epoch, "already-read".into());
    let capture = chat.front.capture();
    capture.publish(&old, 0).unwrap();
    crate::front_native::new_turn(|| {
        assert!(
            !old.render(epoch, old.response(), 4),
            "already-read old frame cannot cross a newer turn's capture fence"
        );
    });
    assert!(!chat.front.order.lock().unwrap().allows(
        &json!({"kind":"response_started","request_id":"unsolicited","response_id":"unsolicited"})
    ));
    set_flag(false);
    crate::front_native::new_turn(|| {});
    assert!(
        chat.front.order.lock().unwrap().allows(
            &json!({"kind":"response_started","request_id":"ordinary","response_id":"ordinary"})
        ),
        "flag off must reset strict ordering in the same connection"
    );
    assert!(chat
        .front
        .order
        .lock()
        .unwrap()
        .allows(&json!({"kind":"agent_delta","response_id":"ordinary","text":"Legacy reply."})));
}
struct FencedFault {
    first: bool,
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
impl crate::voice_front::runtime::Legacy for FencedFault {
    fn send(&mut self, _: &crate::conversation_log::Turn) -> crate::voice_front::Result<String> {
        panic!("durable admission path required")
    }
    fn send_admitted(
        &mut self,
        _: &crate::conversation_log::Turn,
        f: &mut dyn FnMut() -> crate::voice_front::Result<crate::conversation_log::Turn>,
    ) -> crate::voice_front::Result<String> {
        if self.first {
            self.first = false;
            return Err(crate::voice_front::Error::Busy);
        }
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(3)).unwrap();
        let _ = f()?;
        Ok("accepted".into())
    }
}
#[test]
fn store_failure_while_active_or_retrying_releases_real_conversation_busy() {
    if isolated("store_failure_while_active_or_retrying_releases_real_conversation_busy") {
        return;
    }
    let (root, log) = environment(true);
    for retrying in [false, true] {
        let (chat, _peer) = chat();
        let capture = chat.front.capture();
        let (tx, rx) = mpsc::channel();
        let (speech, tickets) = mpsc::sync_channel(1);
        let path = root.join(if retrying {
            "retry-fault"
        } else {
            "active-fault"
        });
        std::fs::create_dir(&path).unwrap();
        let opener = path.clone();
        let route = chat.front.route();
        let page: Page = Arc::new(|_| {});
        let log = log.clone();
        let awaiting = chat.awaiting.clone();
        let worker = Worker::start(
            "session".into(),
            capture.clone(),
            move || crate::conversation_log::Store::for_front(&opener, "session"),
            SpeechProbe {
                owner: route.owner.clone(),
                tx: speech,
            },
            Mirror,
            move |u| {
                let failed = matches!(&u, Update::Status("LegacyOnly"));
                route.emit(u, &awaiting, &page, &log);
                let _ = tx.send(failed);
            },
        )
        .unwrap();

        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (entered, at) = mpsc::sync_channel(1);
        let (release, wait) = mpsc::sync_channel(1);
        crate::conversation::review_busy_front(|epoch| {
            chat.front.accepted(epoch);
            let original = if retrying {
                "A freeform input."
            } else {
                "Can you access files?"
            };
            let input = crate::front_native::input(
                epoch,
                0,
                "fault-nonce".into(),
                original,
                crate::voice_front::policy::Selection {
                    locale: crate::voice_front::policy::Locale::En,
                    source: crate::voice_front::policy::LocaleSource::Setting,
                    confidence: None,
                },
                Box::new(FencedFault {
                    first: true,
                    entered,
                    release: wait,
                }),
            );
            assert!(worker.submit(true, "session", input).is_ok());
            let ticket = if retrying {
                at.recv_timeout(Duration::from_secs(3)).unwrap();
                None
            } else {
                Some(tickets.recv_timeout(Duration::from_secs(3)).unwrap().1)
            };

            std::fs::OpenOptions::new()
                .write(true)
                .open(path.join("conversation-v2.jsonl"))
                .unwrap()
                .set_len(64 << 20)
                .unwrap();
            if let Some(t) = &ticket {
                assert!(t.render(epoch, t.response(), 4));
            } else {
                release.send(()).unwrap();
            }
            let end = std::time::Instant::now() + Duration::from_secs(3);
            while !rx
                .recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
                .expect("worker must report its Store failure")
            {}
            assert!(
                crate::conversation::front_epoch().is_none(),
                "worker Store failure must release active/retry Conversation ownership and busy"
            );
            if let Some(t) = ticket {
                assert!(t.closed());
            }
            assert_eq!(crate::conversation::typed("Next ordinary input."), Ok(true));
        });
        worker.close();
    }
}
type NativeCopyRow = (&'static str, &'static str, &'static str, &'static str, &'static str);
const NATIVE_COPY: &[NativeCopyRow] = &[
        ("en","Conversation voice front ready.","Conversation sync unavailable; using the standard voice.","Conversation sync uncertain; using the standard voice (uncertain turns: 1).","The voice turn could not be confirmed, so it was not sent again. If no answer comes, say it again."),
        ("pt-BR","Voz da conversa disponível.","Sincronização da conversa indisponível; usando a voz padrão.","Sincronização da conversa incerta; usando a voz padrão (turnos sem confirmação: 1).","Não foi possível confirmar o turno de voz, então não houve reenvio. Se a resposta não vier, repita.")
    ];

#[test]
fn native_status_and_uncertainty_copy_follow_en_and_pt_br_settings() {
    if isolated("native_status_and_uncertainty_copy_follow_en_and_pt_br_settings") {
        return;
    }
    let (_root, log) = environment(true);
    for &(locale,ready,unavailable,uncertain,error) in NATIVE_COPY {
        let c=crate::settings::CTX.get().unwrap();let mut settings=crate::settings::current();settings.language=locale.into();crate::settings::save(&c.data,&settings).unwrap();
        let (chat,_peer)=chat();let (tx,rx)=mpsc::channel();let page:Page=Arc::new(move|e|{let _=tx.send(e);});let route=chat.front.route();
        route.emit(Update::Status("Ready"),&chat.awaiting,&page,&log);
        route.emit(Update::Status("LegacyOnly"),&chat.awaiting,&page,&log);
        crate::conversation::review_busy_front(|epoch| {
            chat.front.accepted(epoch);chat.awaiting.store(true,Ordering::SeqCst);
            route.emit(Update::Outcome(epoch,"nonce".into(),crate::voice_front::runtime::Outcome::DeliveryUnknown),&chat.awaiting,&page,&log);
        });
        let rows:Vec<_>=rx.try_iter().map(|e|match e {PageEvent::Status(s)=>s,PageEvent::Reply(v)=>v["error"].as_str().unwrap().to_string()}).collect();
        assert_eq!(rows,vec![ready,unavailable,uncertain,error],"front status/error wording must follow the selected EN/pt-BR locale");
    }
}

#[test]
fn toggle_off_immediately_allows_legacy_before_any_new_turn() {
    if isolated("toggle_off_immediately_allows_legacy_before_any_new_turn") {
        return;
    }
    let (_root, _log) = environment(true);
    let (chat, _peer) = chat();
    crate::front_native::new_turn(|| {});
    assert!(!chat.front.order.lock().unwrap().allows(
        &json!({"kind":"response_started","request_id":"foreign","response_id":"foreign"})
    ));
    let capture = chat.front.capture();
    let epoch = Epoch { capture: 1, turn: 1 };
    let ticket = Ticket::new(epoch, "toggle-off".into());
    capture.publish(&ticket, capture.revision()).unwrap();
    set_flag(false);
    crate::front_native::flag_changed();
    assert!(!ticket.render(epoch, ticket.response(), 4),
        "toggle off must fence an already-read front frame immediately");

    assert!(chat.front.order.lock().unwrap().allows(
        &json!({"kind":"response_started","request_id":"after-toggle","response_id":"after-toggle"})
    ), "toggle off must immediately admit a foreign legacy response before any new turn");
    assert!(chat.front.order.lock().unwrap().allows(
        &json!({"kind":"agent_delta","response_id":"after-toggle","text":"Public legacy reply."})
    ));
}

#[test]
fn h5_pt_copy_table_has_no_gendered_pronouns() {
    for &(locale, ready, unavailable, uncertain, error) in NATIVE_COPY {
        if locale != "pt-BR" { continue; }
        for text in [ready, unavailable, uncertain, error] {
            let bad: Vec<_> = text.split(|c: char| !c.is_alphanumeric())
                .filter(|word| ["ele", "ela"].contains(&word.to_lowercase().as_str())).collect();
            assert!(bad.is_empty(), "gendered pronouns in H5 PT copy table: {bad:?}");
        }
    }
}
