use super::*;

#[tokio::test]
async fn reply_does_not_wait_for_memory_decide() {
    let dir = command_dir("opening-parallel");
    let (rig, _) = routing_rig(
        &dir,
        "parallel",
        &["A heat pump moves heat."],
        &[local::fake_server::SILENT_ANSWER],
    )
    .await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, "Explain a heat pump.")).await;
    let recs = turn_lifecycle_until(&rig, |r| {
        r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))
    })
    .await;
    let delta = recs
        .iter()
        .position(|e| matches!(e.event, Event::AgentDelta { .. }))
        .unwrap();
    let decide = recs
        .iter()
        .position(|e| matches!(e.event, Event::MemoryDecided { .. }))
        .unwrap();
    assert!(
        delta < decide,
        "first reply delta must precede the stalled memory decision"
    );
}

#[test]
fn agent_opener_is_an_additive_wire_event() {
    let value =
        serde_json::json!({"kind":"agent_opener", "response_id":"r1", "text":"Let me think."});
    let event: Event = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(event).unwrap(), value);
}

#[tokio::test]
async fn cold_hello_cannot_relay_a_models_memory_detour_ten_times() {
    let dir = command_dir("opening-hello");
    let (rig, _) = routing_rig(
        &dir,
        "hello",
        &["I don't have any saved memories to check for this message."],
        &[local::fake_server::SILENT_ANSWER],
    )
    .await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    for n in 0..10 {
        c.send(submit(&format!("c{n}"), n, "Hello.")).await;
        let response = loop {
            if let Event::ResponseStarted { response_id, .. } = c.recv().await.event {
                break response_id;
            }
        };
        let frames = c.until_done(&response).await;
        let reply = frames
            .iter()
            .filter_map(|e| match &e.event {
                Event::AgentDelta { text, .. } | Event::AgentReplace { text, .. } => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(
            !reply.to_lowercase().contains("memor"),
            "greeting {n}: {reply}"
        );
        assert!(!reply.is_empty());
    }
}

async fn front_rig(
    name: &str,
    answer: &str,
    delay: Duration,
    main: &[&str],
) -> (Rig, local::fake_server::Fake) {
    let f = local::fake_server::start(
        local::fake_server::Mode::Front {
            opener: answer.into(),
            delay,
            decide_delay: Duration::from_millis(150),
            pieces: main.iter().map(|t| t.to_string()).collect(),
        },
        Duration::ZERO,
    )
    .await;
    let (authority, base_path) = local::parse_loopback_url(&f.url).unwrap();
    let cfg = local::Config {
        authority,
        base_path,
        model: "granite41-3b-q4km".into(),
        model_sha256: None,
        slot: Some(0),
        decide_slot: Some(1),
        opener_slot: Some(2),
        max_tokens: Some(64),
        n_ctx: 4096,
        first_token: Duration::from_secs(5),
        system_instruction: None,
        health_every: Duration::from_secs(60),
    };
    let rig = state_rig(
        &command_dir(name),
        name,
        local::connect_with(cfg.clone(), true),
        turn_lifecycle_opts(&cfg),
    )
    .await;
    (rig, f)
}

async fn front_turn(
    rig: &Rig,
    voice: Option<rth_protocol::ReplyVoice>,
    text: &str,
) -> Vec<Envelope> {
    let (mut c, _) = Client::attach(rig, "host", Role::Controller).await;
    let mut event = submit("c1", 0, text);
    if let Event::CommandSubmitted { reply_voice, .. } = &mut event {
        *reply_voice = voice;
    }
    c.send(event).await;
    let mut frames = Vec::new();
    loop {
        let e = c.recv().await;
        let done = matches!(e.event, Event::AgentDone { .. });
        frames.push(e);
        if done {
            return frames;
        }
    }
}
fn voice() -> Option<rth_protocol::ReplyVoice> {
    Some(rth_protocol::ReplyVoice {
        dictated: false,
        spoken: rth_protocol::SpokenReply::Full,
    })
}

#[tokio::test]
async fn voice_front_slot_order_context_timing_and_echo() {
    let (rig, f) = front_rig(
        "opening-front",
        "<think></think>Let me think.",
        Duration::from_millis(10),
        &["Let me ", "think.", " Heat pumps move heat."],
    )
    .await;
    let frames = front_turn(&rig, voice(), "Explain a heat pump.").await;
    let at = |kind: &str| {
        frames
            .iter()
            .position(|e| serde_json::to_value(&e.event).unwrap()["kind"] == kind)
            .unwrap()
    };
    assert!(at("reply_spoken") < at("agent_opener"));
    assert!(at("agent_opener") < at("agent_delta"));
    assert_eq!(
        frames
            .iter()
            .filter(|e| matches!(e.event, Event::AgentOpener { .. }))
            .count(),
        1
    );
    let text = frames
        .iter()
        .filter_map(|e| match &e.event {
            Event::AgentDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text.trim(), "Heat pumps move heat.");
    let summary = frames
        .iter()
        .find_map(|e| match &e.event {
            Event::TurnSummary {
                opener_ms,
                first_delta_ms,
                ..
            } => Some((*opener_ms, *first_delta_ms)),
            _ => None,
        })
        .unwrap();
    assert!(summary.0.is_some() && summary.1 >= summary.0, "{summary:?}");
    let reqs = f.requests.lock().unwrap();
    let front = reqs.iter().find(|r| r["id_slot"] == 2).unwrap();
    assert_eq!(front["max_tokens"], 16);
    let main = reqs
        .iter()
        .find(|r| r["id_slot"] == 0 && r["max_tokens"] != 1)
        .unwrap();
    assert!(
        main["messages"]
            .to_string()
            .contains("Already said aloud: Let me think.")
    );
}

#[tokio::test]
async fn typed_turn_has_no_opener_or_extra_slot_request() {
    let (rig, f) = front_rig(
        "opening-typed",
        "Let me think.",
        Duration::ZERO,
        &["A heat pump moves heat."],
    )
    .await;
    let frames = front_turn(&rig, None, "Explain a heat pump.").await;
    assert!(
        !frames
            .iter()
            .any(|e| matches!(e.event, Event::AgentOpener { .. }))
    );
    assert!(!f.requests.lock().unwrap().iter().any(|r| r["id_slot"] == 2));
}

#[tokio::test]
async fn invalid_none_and_slow_openers_do_not_block_the_reply() {
    for (n, raw, delay) in [
        (0, "NONE", Duration::ZERO),
        (1, "Canberra is the capital.", Duration::ZERO),
        (2, "Let me think.", Duration::from_secs(2)),
    ] {
        let (rig, f) = front_rig(
            &format!("opening-no-front-{n}"),
            raw,
            delay,
            &["A heat pump moves heat."],
        )
        .await;
        let frames = front_turn(&rig, voice(), "Explain a heat pump.").await;
        assert!(
            !frames
                .iter()
                .any(|e| matches!(e.event, Event::AgentOpener { .. }))
        );
        assert!(
            frames
                .iter()
                .any(|e| matches!(e.event, Event::AgentDelta { .. }))
        );
        assert!(
            f.requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r["id_slot"] == 0 && r["max_tokens"] != 1)
        );
    }
}

#[test]
fn memory_questions_and_actions_keep_the_honesty_fence() {
    for words in [
        "what do you know about me",
        "você sabe meu nome",
        "você sabe meu nome?",
        "remember that my dog is Mia",
        "forget my dog",
        "qual é meu nome",
        "actually my dog is Bo now",
    ] {
        assert!(memory_required(words), "{words}");
    }
    for words in [
        "Hello.",
        "No, thanks.",
        "How do I fix my bike?",
        "Explain a heat pump.",
        "me conta uma curiosidade sobre o espaço",
    ] {
        assert!(!memory_required(words));
    }
}

#[tokio::test]
async fn passing_personal_facts_still_save_after_first_text_and_before_summary() {
    for (n, text) in ["my name is Sam", "I live in Lisbon"]
        .into_iter()
        .enumerate()
    {
        let (rig, _) = routing_rig(
            &command_dir(&format!("opening-fact-{n}")),
            "fact",
            &["Okay."],
            &[local::fake_server::SILENT_ANSWER],
        )
        .await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        c.send(submit("c1", 0, text)).await;
        let recs = turn_lifecycle_until(&rig, |r| {
            r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))
        })
        .await;
        let delta = recs
            .iter()
            .position(|e| matches!(e.event, Event::AgentDelta { .. }))
            .unwrap();
        let committed = recs
            .iter()
            .position(
                |e| matches!(&e.event, Event::MemoryAct { state, .. } if state == "committed"),
            )
            .unwrap();
        let summary = recs
            .iter()
            .position(|e| matches!(e.event, Event::TurnSummary { .. }))
            .unwrap();
        assert!(delta < committed && committed < summary, "{text}");
        assert!(
            !routing_summaries(&recs)[0]["memory"]["changed"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn decide_miss_keeps_ordinary_text_clean_and_memory_questions_honest() {
    for (n, text, needs_memory) in [
        (0, "No, thanks.", false),
        (1, "How do I fix my bike?", false),
        (2, "Do you know my name?", true),
        (3, "Você sabe meu nome?", true),
        (4, "Você sabe meu nome", true),
        (5, "Hoje está quente onde eu moro.", false),
        (6, "Do you know my name", true),
    ] {
        let (rig, f) = routing_rig(
            &command_dir(&format!("opening-miss-{n}")),
            "miss",
            &["Okay."],
            &[local::fake_server::SILENT_ANSWER],
        )
        .await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        c.send(submit("c1", 0, text)).await;
        let recs = turn_lifecycle_until(&rig, |r| {
            r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))
        })
        .await;
        let inputs = chat_user(&f);
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0].contains("MEMORY_NEEDED_BUT_UNAVAILABLE"),
            needs_memory,
            "{text}: {}",
            inputs[0]
        );
        assert!(
            !inputs[0].contains("MEMORY_NONE"),
            "failed decision must not claim empty memory: {text}"
        );
        let decisions = turn_lifecycle_decided(&recs);
        assert_eq!(decisions[0].1["fallback"], "decide_timeout");
    }
}

#[tokio::test]
async fn memory_question_gets_its_opener_before_recall_and_decide_finish() {
    let (rig, _) = front_rig(
        "opening-front-memory",
        "Let me think.",
        Duration::from_millis(5),
        &["I don't know."],
    )
    .await;
    let frames = front_turn(&rig, voice(), "Do you know my name?").await;
    let opener = frames
        .iter()
        .position(|e| matches!(e.event, Event::AgentOpener { .. }))
        .unwrap();
    let decided = frames
        .iter()
        .position(|e| matches!(e.event, Event::MemoryDecided { .. }))
        .unwrap();
    assert!(
        opener < decided,
        "only the main answer waits for memory, never the voice front"
    );
}

#[test]
fn opener_prefix_survives_a_main_reply_replacement_in_history() {
    let mut fold = context::Fold::default();
    let mut stamp = Stamper::new(
        Source::Guest,
        "front-test",
        Box::new(MonotonicClock::start()),
    );
    for event in [
        Event::AgentOpener {
            response_id: "r1".into(),
            text: "Let me think.".into(),
        },
        Event::AgentDelta {
            response_id: "r1".into(),
            text: "unshown reasoning".into(),
        },
        Event::AgentReplace {
            response_id: "r1".into(),
            text: "Heat pumps move heat.".into(),
        },
        Event::AgentDone {
            response_id: "r1".into(),
            status: ResponseStatus::Completed,
        },
    ] {
        fold.apply(&stamp.stamp(event, None));
    }
    assert_eq!(
        fold.turns.back().unwrap().text,
        "Let me think. Heat pumps move heat."
    );
}
