

use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::{WebSocketStream, accept_hdr_async};

use super::*;
use crate::backend::{Command, Heard, Output};

pub(crate) const TEST_KEY: &str = "sk-test-placeholder-not-a-key";

pub(crate) type Peer = WebSocketStream<TcpStream>;

pub(crate) async fn mock() -> (String, mpsc::Receiver<(Peer, Option<String>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/v1/realtime?model=gpt-realtime-2.1", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut auth = None;
            let ws = accept_hdr_async(stream, |req: &Request, resp: Response| {
                auth = req.headers().get("authorization").map(|v| v.to_str().unwrap().to_string());
                Ok(resp)
            })
            .await
            .unwrap();
            if tx.send((ws, auth)).await.is_err() {
                return;
            }
        }
    });
    (url, rx)
}

pub(crate) async fn recv_json(peer: &mut Peer) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), peer.next()).await.expect("client event").unwrap().unwrap();
        match msg {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            Message::Binary(b) => return serde_json::from_slice(&b).unwrap(),
            _ => continue,
        }
    }
}

async fn silent(peer: &mut Peer, ms: u64) {
    if let Ok(Some(Ok(m))) = tokio::time::timeout(Duration::from_millis(ms), peer.next()).await {
        panic!("unexpected client event {m:?}");
    }
}

pub(crate) async fn send(peer: &mut Peer, v: Value) {
    peer.send(Message::text(v.to_string())).await.unwrap();
}

pub(crate) async fn accept_setup(conns: &mut mpsc::Receiver<(Peer, Option<String>)>) -> (Peer, Value, Option<String>) {
    let (mut peer, auth) = tokio::time::timeout(Duration::from_secs(5), conns.recv()).await.unwrap().unwrap();
    send(&mut peer, json!({"type": "session.created", "session": {"id": "sess_1"}})).await;
    let setup = recv_json(&mut peer).await;
    send(&mut peer, json!({"type": "session.updated", "session": setup["session"].clone()})).await;
    (peer, setup, auth)
}

pub(crate) fn created(id: &str, metadata: Option<Value>) -> Value {
    let mut r = json!({"id": id, "object": "realtime.response", "status": "in_progress"});
    if let Some(m) = metadata {
        r["metadata"] = m;
    }
    json!({"type": "response.created", "response": r})
}

pub(crate) async fn reply(peer: &mut Peer, pid: &str, after: Option<&str>) {
    let item = format!("item_{pid}");
    send(peer, created(pid, None)).await;
    send(peer, json!({"type": "response.output_item.added", "response_id": pid, "output_index": 0, "item": {"id": item, "type": "message", "role": "assistant"}})).await;
    send(peer, json!({"type": "conversation.item.added", "previous_item_id": after.unwrap_or("item_other"), "item": {"id": item, "type": "message", "role": "assistant"}})).await;
}

pub(crate) fn audio_delta(rid: &str, pcm: &[u8]) -> Value {
    let delta = base64::engine::general_purpose::STANDARD.encode(pcm);
    json!({"type": "response.output_audio.delta", "response_id": rid, "item_id": "item_a", "output_index": 0, "content_index": 0, "delta": delta})
}

fn text_delta(rid: &str, t: &str) -> Value {
    json!({"type": "response.output_audio_transcript.delta", "response_id": rid, "item_id": "item_a", "delta": t})
}

pub(crate) fn done(rid: &str, status: &str, reason: Option<&str>) -> Value {
    let mut r = json!({"id": rid, "status": status});
    if let Some(reason) = reason {
        r["status_details"] = json!({"type": status, "reason": reason});
    }
    json!({"type": "response.done", "response": r})
}

async fn next(outputs: &mut mpsc::Receiver<Output>) -> Output {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.expect("output").unwrap() {
            Output::Log { .. } => continue,
            o => return o,
        }
    }
}

async fn next_any(outputs: &mut mpsc::Receiver<Output>) -> Output {
    tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.expect("output").unwrap()
}

async fn drain(outputs: &mut mpsc::Receiver<Output>, ms: u64) -> Vec<Output> {
    let mut v = Vec::new();
    while let Ok(Some(o)) = tokio::time::timeout(Duration::from_millis(ms), outputs.recv()).await {
        v.push(o);
    }
    v
}

fn non_log(v: &[Output]) -> Vec<&Output> {
    v.iter().filter(|o| !matches!(o, Output::Log { .. })).collect()
}

async fn ready(outputs: &mut mpsc::Receiver<Output>) {
    loop {
        if let Output::Log { msg, .. } = tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.unwrap().unwrap()
            && msg.starts_with("openai-realtime ready")
        {
            return;
        }
    }
}

async fn rig(tweak: impl FnOnce(&mut Config)) -> (Session, Peer, mpsc::Receiver<(Peer, Option<String>)>) {
    let (url, conns) = mock().await;
    let mut cfg = Config::for_test(url);
    tweak(&mut cfg);
    let mut s = connect(cfg);
    let mut conns = conns;
    let (peer, _, _) = accept_setup(&mut conns).await;
    ready(&mut s.outputs).await;
    (s, peer, conns)
}

fn text(q: &str, r: &str, t: &str) -> Command {
    Command::UserText { opener: None, request_id: q.into(), response_id: r.into(), text: t.into(), context: vec![], memory: None, owner: true, people: None, reply_voice: None, level: Default::default() }
}

fn meta(q: &str) -> Option<Value> {
    Some(json!({"rth_request": q, "rth_gen": "1"}))
}

trait WithGen {
    fn with_gen(self, generation: u64) -> Self;
}

impl WithGen for Output {
    fn with_gen(mut self, generation: u64) -> Self {
        if let Output::Diagnostic { session_generation, .. } = &mut self {
            *session_generation = generation;
        }
        self
    }
}

fn diag(rid: &str, status: Option<&str>, reason: Option<&str>, source: &'static str, local: Option<&str>) -> Output {
    Output::Diagnostic {
        response_id: rid.into(),
        session_generation: 1,
        provider_status: status.map(str::to_string),
        provider_reason: reason.map(str::to_string),
        outcome_source: source,
        local_reason: local.map(str::to_string),
        truncated: None,
        raw_text: None,
        history_guard_applied: false,
    }
}

async fn expect_request(peer: &mut Peer, t: &str) -> Value {
    let item = recv_json(peer).await;
    assert_eq!(item["type"], "conversation.item.create");
    assert_eq!(item["item"], json!({"type": "message", "role": "user", "content": [{"type": "input_text", "text": t}]}));
    let create = recv_json(peer).await;
    assert_eq!(create["type"], "response.create");
    create
}

#[test]
fn vox_decode_fixtures_map_to_server_events() {
    let p = |s: &str| parse_server(s.as_bytes()).unwrap();
    assert_eq!(
        p(r#"{"type":"response.output_audio.delta","response_id":"r1","item_id":"i1","delta":"QUJD"}"#),
        ServerEvent::AudioDelta { response_id: Some("r1".into()), item_id: Some("i1".into()), content_index: 0, pcm: b"ABC".to_vec() }
    );

    assert!(matches!(p(r#"{"type":"response.audio.delta","item_id":"i1","delta":"QUJD"}"#), ServerEvent::AudioDelta { pcm, .. } if pcm == b"ABC"));
    assert_eq!(p(r#"{"type":"response.output_audio_transcript.delta","delta":"He"}"#), ServerEvent::TextDelta { response_id: None, item_id: None, delta: "He".into() });
    assert_eq!(p(r#"{"type":"response.audio_transcript.delta","response_id":"r1","delta":"He"}"#), ServerEvent::TextDelta { response_id: Some("r1".into()), item_id: None, delta: "He".into() });
    assert_eq!(p(r#"{"type":"response.output_audio_transcript.done","transcript":"Hello."}"#), ServerEvent::Ignored("response.output_audio_transcript.done".into()));
    assert_eq!(p(r#"{"type":"response.audio_transcript.done","transcript":"Hello."}"#), ServerEvent::Ignored("response.audio_transcript.done".into()));
    assert_eq!(
        p(r#"{"event_id":"e2","type":"conversation.item.input_audio_transcription.completed","item_id":"u1","transcript":"open chrome"}"#),
        ServerEvent::TranscriptCompleted { item_id: Some("u1".into()), transcript: "open chrome".into() }
    );
    assert_eq!(
        p(r#"{"type":"conversation.item.input_audio_transcription.completed","transcript":"yes"}"#),
        ServerEvent::TranscriptCompleted { item_id: None, transcript: "yes".into() }
    );
    assert_eq!(p(r#"{"type":"input_audio_buffer.speech_started"}"#), ServerEvent::SpeechStarted { item_id: None });
    assert_eq!(p(r#"{"type":"input_audio_buffer.speech_stopped","item_id":"u1"}"#), ServerEvent::SpeechStopped { item_id: Some("u1".into()) });
    assert_eq!(p(r#"{"event_id":"e3","type":"input_audio_buffer.committed","previous_item_id":"i0","item_id":"u1"}"#), ServerEvent::Committed { item_id: Some("u1".into()) });

    assert_eq!(p(r#"{"type":"input_audio_buffer.committed","item_id":""}"#), ServerEvent::Committed { item_id: None });
    assert_eq!(
        p(r#"{"type":"response.created","response":{"id":"resp_1","metadata":{"rth_request":"q1","rth_gen":"3"}}}"#),
        ServerEvent::ResponseCreated { response_id: "resp_1".into(), rth_request: Some("q1".into()), rth_gen: Some("3".into()) }
    );
    assert_eq!(
        p(r#"{"type":"response.done","response":{"id":"resp_1","status":"incomplete","status_details":{"type":"incomplete","reason":"max_output_tokens"}}}"#),
        ServerEvent::ResponseDone { response_id: "resp_1".into(), status: "incomplete".into(), reason: Some("max_output_tokens".into()), usage: None }
    );

    assert_eq!(p(r#"{"type":"conversation.item.truncated","item_id":"i1","content_index":0,"audio_end_ms":1200}"#), ServerEvent::Ignored("conversation.item.truncated".into()));
    assert_eq!(p(r#"{"type":"session.updated"}"#), ServerEvent::SessionUpdated);
    assert_eq!(
        p(r#"{"type":"conversation.item.added","previous_item_id":"item_u1","item":{"id":"item_a","type":"message","role":"assistant"}}"#),
        ServerEvent::ItemAdded { item_id: "item_a".into(), previous_item_id: Some("item_u1".into()), role: Some("assistant".into()) }
    );

    assert_eq!(
        p(r#"{"type":"conversation.item.created","previous_item_id":null,"item":{"id":"item_u1","role":"user"}}"#),
        ServerEvent::ItemAdded { item_id: "item_u1".into(), previous_item_id: None, role: Some("user".into()) }
    );
    assert_eq!(
        p(r#"{"type":"response.output_item.added","response_id":"resp_1","output_index":0,"item":{"id":"item_a"}}"#),
        ServerEvent::OutputItemAdded { response_id: "resp_1".into(), item_id: "item_a".into() }
    );
    assert_eq!(p(r#"{"type":"response.function_call_arguments.done","call_id":"c1"}"#), ServerEvent::Ignored("response.function_call_arguments.done".into()));
    assert_eq!(map_status("completed"), ResponseStatus::Completed);
    assert_eq!(map_status("cancelled"), ResponseStatus::Cancelled);
    assert_eq!((map_status("failed"), map_status("incomplete"), map_status("new")), (ResponseStatus::Failed, ResponseStatus::Failed, ResponseStatus::Failed));
}

#[test]
fn failure_fixtures_error_unknown_and_malformed() {
    let p = |s: &str| parse_server(s.as_bytes());
    assert_eq!(
        p(r#"{"type":"error","event_id":"evt_9","error":{"type":"invalid_request_error","code":"invalid_value","message":"bad voice","event_id":"rth-1-create3"}}"#).unwrap(),
        ServerEvent::Error { client_event: Some("rth-1-create3".into()), error_type: Some("invalid_request_error".into()), code: "invalid_value".into(), message: "bad voice".into() }
    );
    assert_eq!(p(r#"{"type":"error","error":{"message":"boom"}}"#).unwrap(), ServerEvent::Error { client_event: None, error_type: None, code: "unknown".into(), message: "boom".into() });
    assert_eq!(p(r#"{"type":"response.mcp_call.in_progress"}"#).unwrap(), ServerEvent::Unknown("response.mcp_call.in_progress".into()));
    assert!(p("not json").is_err());
    assert!(p(r#"{"no":"type"}"#).is_err());
    assert!(p(r#"{"type":"response.created","response":{}}"#).is_err(), "a created without an id is malformed");
    assert!(p(r#"{"type":"response.output_audio.delta","delta":"%%%"}"#).is_err());
}

#[tokio::test]
async fn a_refused_setup_is_logged_and_retried() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _) = conns.recv().await.unwrap();
    let setup = recv_json(&mut peer).await;
    send(&mut peer, json!({"type": "error", "error": {"type": "invalid_request_error", "code": "unknown_parameter", "message": "Unknown parameter: 'session.audio.input.foo'.", "event_id": "rth-setup"}})).await;
    loop {
        if let Output::Log { level: "error", msg } = s.outputs.recv().await.unwrap() {
            assert!(msg.contains("setup refused: unknown_parameter"), "{msg}");
            break;
        }
    }
    let (_peer2, setup2, _) = accept_setup(&mut conns).await;
    assert_eq!(setup, setup2);
    ready(&mut s.outputs).await;
}

#[tokio::test]
async fn unknown_and_malformed_events_do_not_end_the_session() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, json!({"type": "response.mcp_call.in_progress"})).await;
    send(&mut peer, json!({"type": "response.mcp_call.in_progress"})).await;
    peer.send(Message::text("{oops")).await.unwrap();
    send(&mut peer, json!({"type": "input_audio_buffer.speech_started"})).await;
    let out = drain(&mut s.outputs, 300).await;
    let logs: Vec<&String> = out.iter().filter_map(|o| if let Output::Log { msg, .. } = o { Some(msg) } else { None }).collect();
    assert_eq!(logs.iter().filter(|m| m.contains("unknown event type response.mcp_call.in_progress")).count(), 1, "{logs:?}");
    assert!(logs.iter().any(|m| m.contains("unreadable server event")), "{logs:?}");
    assert!(out.contains(&Output::Signal { response_id: None, signal: "voice_activity_start" }));
}

#[test]
fn the_default_setup_is_vox_equivalent_and_pinned() {

    let mut cfg = Config::for_test("ws://x".into());
    cfg.persona = crate::persona::PERSONA_V2_NAME;
    let v = setup_message(&cfg);
    assert_eq!(
        v,
        json!({
            "type": "session.update",
            "event_id": "rth-setup",
            "session": {
                "type": "realtime",
                "instructions": crate::persona::PERSONA_V2,
                "output_modalities": ["audio"],
                "audio": {
                    "input": {
                        "format": {"type": "audio/pcm", "rate": 24000},
                        "transcription": {"model": "gpt-transcribe"},
                        "turn_detection": {"type": "semantic_vad", "create_response": true, "interrupt_response": true},
                    },
                    "output": {"format": {"type": "audio/pcm", "rate": 24000}, "voice": "marin"},
                },
            },
        })
    );
    assert!(cfg.knobs().starts_with("vad=semantic_vad eagerness=auto create_response=true interrupt_response=true transcription=gpt-transcribe transcription_lang=- voice=marin"));
    assert!(cfg.knobs().ends_with("persona=rth-persona-v2"));
    assert_eq!(setup_identity(&cfg), "b469c6d0317cc3c8");

    let mut v1 = Config::for_test("ws://x".into());
    v1.persona = "rth-persona-v1";
    assert_eq!(setup_identity(&v1), "1290e95ea7864296");

    v1.persona = "none";
    assert!(setup_message(&v1)["session"].get("instructions").is_none());
}

#[test]
fn the_knobs_toggle_separately() {
    let base = setup_identity(&Config::for_test("ws://x".into()));
    let with = |f: &dyn Fn(&mut Config)| {
        let mut c = Config::for_test("ws://x".into());
        f(&mut c);
        (setup_message(&c), setup_identity(&c))
    };
    let (v, id) = with(&|c| c.eagerness = Some("low".into()));
    assert_eq!(v["session"]["audio"]["input"]["turn_detection"]["eagerness"], "low");
    assert_ne!(id, base);
    let (v, _) = with(&|c| {
        c.vad = "server_vad".into();
        c.eagerness = Some("low".into());
    });
    assert_eq!(v["session"]["audio"]["input"]["turn_detection"], json!({"type": "server_vad", "create_response": true, "interrupt_response": true}), "eagerness is semantic_vad only");
    let (v, _) = with(&|c| c.vad = "none".into());
    assert_eq!(v["session"]["audio"]["input"]["turn_detection"], Value::Null);
    let (v, _) = with(&|c| {
        c.create_response = false;
        c.interrupt_response = false;
    });
    assert_eq!(v["session"]["audio"]["input"]["turn_detection"], json!({"type": "semantic_vad", "create_response": false, "interrupt_response": false}));
    let (v, _) = with(&|c| c.transcription_lang = Some("pt".into()));
    assert_eq!(v["session"]["audio"]["input"]["transcription"], json!({"model": "gpt-transcribe", "language": "pt"}));
    let (v, _) = with(&|c| c.noise_reduction = Some("far_field".into()));
    assert_eq!(v["session"]["audio"]["input"]["noise_reduction"], json!({"type": "far_field"}));
    let (v, _) = with(&|c| c.reasoning_effort = Some("low".into()));
    assert_eq!(v["session"]["reasoning"], json!({"effort": "low"}));
    let (v, id) = with(&|c| c.system_instruction = Some("Your name is RTH.".into()));
    assert_eq!(v["session"]["instructions"], "Your name is RTH.");
    assert_ne!(id, base);
    let (v, _) = with(&|c| c.voice = "cedar".into());
    assert_eq!(v["session"]["audio"]["output"]["voice"], "cedar");
}

#[test]
fn command_encodings_are_exact() {
    assert_eq!(
        user_text("oi", "e1"),
        json!({"type": "conversation.item.create", "event_id": "e1", "item": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "oi"}]}})
    );
    assert_eq!(response_create("e2", "q1", 4), json!({"type": "response.create", "event_id": "e2", "response": {"metadata": {"rth_request": "q1", "rth_gen": "4"}}}));
    assert_eq!(response_cancel("e3", "resp_1"), json!({"type": "response.cancel", "event_id": "e3", "response_id": "resp_1"}));
    assert_eq!(audio_append(b"ABC"), json!({"type": "input_audio_buffer.append", "audio": "QUJD"}));
    assert_eq!(input_commit("e4"), json!({"type": "input_audio_buffer.commit", "event_id": "e4"}));
}

#[test]
fn the_grok_dialect_is_a_refused_stub() {
    assert!(Config::from_env(Dialect::Grok).is_err());
    assert_eq!((Dialect::Grok.name(), Dialect::Grok.input_rate()), ("grok-voice@v1", 16_000));
    assert_eq!(Dialect::Grok.flags().truncate, Cap::Yes(Evidence::Client));
    assert_eq!(Dialect::Grok.flags().deferred_replies, Cap::Unknown, "undocumented is unknown, not false");
    assert_eq!(Dialect::OpenAI.flags().truncate_returns_text, Cap::No(Evidence::Documented));
}

#[tokio::test]
async fn one_setup_per_connection_and_no_live_session_update() {
    let (s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let _ = expect_request(&mut peer, "oi").await;
    s.commands.send(Command::Audio { sample_rate: 16_000, pcm: vec![0; 640] }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["type"], "input_audio_buffer.append");
    s.commands.send(Command::AudioEnd { stream_id: "m1".into() }).await.unwrap();
    silent(&mut peer, 200).await;
}

#[tokio::test]
async fn mode_a_barge_in_cancels_then_the_provider_opens_the_next_reply() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    reply(&mut peer, "resp_1", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    send(&mut peer, audio_delta("resp_1", &[1, 0, 2, 0])).await;
    assert_eq!(next(&mut s.outputs).await, Output::Audio { response_id: "v1".into(), sample_rate: 24_000, pcm: vec![1, 0, 2, 0] });
    send(&mut peer, json!({"type": "input_audio_buffer.speech_started", "item_id": "item_u2"})).await;
    assert_eq!(next(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "voice_activity_start" });
    send(&mut peer, done("resp_1", "cancelled", Some("turn_detected"))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "interrupted" });
    assert_eq!(next(&mut s.outputs).await, diag("v1", Some("cancelled"), Some("turn_detected"), "provider", None));
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Cancelled });
    send(&mut peer, json!({"type": "input_audio_buffer.speech_stopped", "item_id": "item_u2"})).await;
    assert_eq!(next(&mut s.outputs).await, Output::Signal { response_id: None, signal: "voice_activity_end" });
    reply(&mut peer, "resp_2", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v2".into(), utterance_id: None });
}

#[tokio::test]
async fn create_true_interrupt_false_speech_is_a_signal_only() {
    let (mut s, mut peer, _c) = rig(|c| c.interrupt_response = false).await;
    reply(&mut peer, "resp_1", None).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Opened { .. }));
    send(&mut peer, json!({"type": "input_audio_buffer.speech_started"})).await;
    assert_eq!(next(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "voice_activity_start" });
    send(&mut peer, audio_delta("resp_1", &[3, 0])).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Audio { .. }), "the reply continues");
    send(&mut peer, done("resp_1", "completed", None)).await;
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Completed });
    reply(&mut peer, "resp_2", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v2".into(), utterance_id: None });
}

#[tokio::test]
async fn create_false_interrupt_true_cancels_and_waits_for_respond() {
    let (mut s, mut peer, _c) = rig(|c| c.create_response = false).await;
    s.commands.send(Command::Respond { request_id: "q1".into(), response_id: "r1".into(), decision_id: "d1".into(), utterance_id: None }).await.unwrap();
    let create = recv_json(&mut peer).await;
    assert_eq!(create["type"], "response.create", "VAD on: no commit");
    assert_eq!(create["response"]["metadata"], json!({"rth_request": "q1", "rth_gen": "1"}));
    send(&mut peer, created("resp_1", meta("q1"))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
    send(&mut peer, json!({"type": "input_audio_buffer.speech_started"})).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Signal { signal: "voice_activity_start", .. }));
    send(&mut peer, done("resp_1", "cancelled", Some("turn_detected"))).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Signal { signal: "interrupted", .. }));
    assert!(matches!(next(&mut s.outputs).await, Output::Diagnostic { .. }));
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Cancelled });
    send(&mut peer, json!({"type": "input_audio_buffer.speech_stopped"})).await;
    let out = drain(&mut s.outputs, 200).await;
    assert!(!out.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Started { .. })), "{out:?}");
}

#[tokio::test]
async fn mode_b_speech_is_a_signal_only_and_nothing_is_terminal() {
    let (mut s, mut peer, _c) = rig(|c| {
        c.create_response = false;
        c.interrupt_response = false;
    })
    .await;
    s.commands.send(text("q1", "r1", "conte")).await.unwrap();
    let _ = expect_request(&mut peer, "conte").await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Started { .. }));
    send(&mut peer, json!({"type": "input_audio_buffer.speech_started"})).await;
    send(&mut peer, json!({"type": "input_audio_buffer.speech_stopped"})).await;
    send(&mut peer, json!({"type": "input_audio_buffer.committed", "item_id": "item_u1"})).await;
    let out = non_log(&drain(&mut s.outputs, 300).await).into_iter().map(|o| format!("{o:?}")).collect::<Vec<_>>();
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out.iter().all(|o| o.starts_with("Signal")), "{out:?}");
}

#[tokio::test]
async fn done_only_follows_response_done_with_each_status_mapped() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    for (n, (status, reason, mapped)) in [
        ("completed", None, ResponseStatus::Completed),
        ("cancelled", Some("client_cancelled"), ResponseStatus::Cancelled),
        ("failed", None, ResponseStatus::Failed),
        ("incomplete", Some("content_filter"), ResponseStatus::Failed),
    ]
    .into_iter()
    .enumerate()
    {
        let pid = format!("resp_{n}");
        let rid = format!("v{}", n + 1);
        reply(&mut peer, &pid, None).await;
        assert!(matches!(next(&mut s.outputs).await, Output::Opened { .. }));
        send(&mut peer, json!({"type": "input_audio_buffer.speech_started"})).await;
        assert!(matches!(next(&mut s.outputs).await, Output::Signal { signal: "voice_activity_start", .. }));
        assert!(drain(&mut s.outputs, 100).await.is_empty(), "speech_started alone is not terminal");
        send(&mut peer, done(&pid, status, reason)).await;
        if status != "completed" {
            assert_eq!(next(&mut s.outputs).await, diag(&rid, Some(status), reason, "provider", None));
        }
        assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: rid, status: mapped }, "{status}");
    }
}

#[tokio::test]
async fn a_stop_fences_at_once_and_its_terminal_waits_for_the_provider() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let _ = expect_request(&mut peer, "oi").await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Started { .. }));
    send(&mut peer, text_delta("resp_1", "Olá")).await;
    assert_eq!(next(&mut s.outputs).await, Output::Delta { response_id: "r1".into(), text: "Olá".into() });
    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    let cancel = recv_json(&mut peer).await;
    assert_eq!((cancel["type"].as_str(), cancel["response_id"].as_str()), (Some("response.cancel"), Some("resp_1")));
    send(&mut peer, audio_delta("resp_1", &[9, 9])).await;
    send(&mut peer, text_delta("resp_1", " tudo")).await;
    assert!(non_log(&drain(&mut s.outputs, 150).await).is_empty(), "fenced, and no terminal yet");
    send(&mut peer, done("resp_1", "cancelled", Some("client_cancelled"))).await;
    let out = drain(&mut s.outputs, 200).await;
    assert!(out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("dropped 1 audio, 1 text"))), "{out:?}");
    assert_eq!(non_log(&out), vec![&diag("r1", Some("cancelled"), Some("client_cancelled"), "provider", None), &Output::Done { response_id: "r1".into(), status: ResponseStatus::Cancelled }]);
}

#[tokio::test]
async fn an_unacknowledged_stop_fails_locally_and_a_late_outcome_is_metadata() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    reply(&mut peer, "resp_1", None).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Opened { .. }));
    s.commands.send(Command::Cancel { response_id: "v1".into() }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["type"], "response.cancel");
    assert_eq!(next(&mut s.outputs).await, diag("v1", None, None, "local", Some("cancel_unacknowledged")));
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Failed });
    send(&mut peer, audio_delta("resp_1", &[1, 1])).await;
    send(&mut peer, done("resp_1", "cancelled", Some("client_cancelled"))).await;
    let out = non_log(&drain(&mut s.outputs, 200).await).into_iter().cloned_debug();
    assert_eq!(out, vec![format!("{:?}", diag("v1", Some("cancelled"), Some("client_cancelled"), "provider", Some("after_local_terminal")))]);
}

#[tokio::test]
async fn a_stop_after_the_terminal_keeps_it_and_sends_nothing() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    reply(&mut peer, "resp_1", None).await;
    let _ = next(&mut s.outputs).await;
    send(&mut peer, done("resp_1", "completed", None)).await;
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Completed });
    s.commands.send(Command::Cancel { response_id: "v1".into() }).await.unwrap();
    silent(&mut peer, 200).await;
    assert!(non_log(&drain(&mut s.outputs, 100).await).is_empty());
}

#[tokio::test]
async fn a_completed_vs_cancel_race_keeps_completed() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    reply(&mut peer, "resp_1", None).await;
    let _ = next(&mut s.outputs).await;
    s.commands.send(Command::Cancel { response_id: "v1".into() }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["type"], "response.cancel");
    send(&mut peer, done("resp_1", "completed", None)).await;
    send(&mut peer, json!({"type": "error", "error": {"type": "invalid_request_error", "code": "response_cancel_not_active", "message": "no active response", "event_id": "rth-1-cancel1"}})).await;
    let out = drain(&mut s.outputs, 250).await;
    assert!(out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("stop of v1: provider outcome completed"))), "{out:?}");
    assert_eq!(non_log(&out).iter().filter(|o| matches!(o, Output::Done { .. })).collect::<Vec<_>>(), vec![&&Output::Done { response_id: "v1".into(), status: ResponseStatus::Completed }]);
    assert!(!out.iter().any(|o| matches!(o, Output::Error { .. })), "the late cancel error names an ended response: {out:?}");
}

#[tokio::test]
async fn incomplete_fails_with_a_linked_diagnostic_once_and_keeps_partial_output() {
    let (mut s, mut peer, mut conns) = rig(|_| {}).await;
    for (n, reason) in [(1, Some("max_output_tokens")), (2, None)] {
        let pid = format!("resp_{n}");
        let rid = format!("v{n}");
        reply(&mut peer, &pid, None).await;
        let _ = next(&mut s.outputs).await;
        send(&mut peer, text_delta(&pid, "parte")).await;
        assert_eq!(next(&mut s.outputs).await, Output::Delta { response_id: rid.clone(), text: "parte".into() });
        send(&mut peer, done(&pid, "incomplete", reason)).await;
        assert_eq!(next(&mut s.outputs).await, diag(&rid, Some("incomplete"), reason, "provider", None), "reason absent stays null");
        assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: rid, status: ResponseStatus::Failed });

        send(&mut peer, done(&pid, "incomplete", reason)).await;
        assert!(non_log(&drain(&mut s.outputs, 150).await).is_empty());
    }

    assert!(tokio::time::timeout(Duration::from_millis(200), conns.recv()).await.is_err());
}

#[tokio::test]
async fn respond_binds_on_the_metadata_echo() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(Command::Respond { request_id: "q7".into(), response_id: "r7".into(), decision_id: "d7".into(), utterance_id: Some("u1".into()) }).await.unwrap();
    let create = recv_json(&mut peer).await;
    assert_eq!(create["response"]["metadata"]["rth_request"], "q7");
    send(&mut peer, created("resp_x", meta("q7"))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r7".into(), request_id: "q7".into() });
    send(&mut peer, audio_delta("resp_x", &[5, 0])).await;
    assert_eq!(next(&mut s.outputs).await, Output::Audio { response_id: "r7".into(), sample_rate: 24_000, pcm: vec![5, 0] });
}

#[tokio::test]
async fn overlapping_requests_without_an_echo_both_fail_closed() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "um")).await.unwrap();
    s.commands.send(text("q2", "r2", "dois")).await.unwrap();
    let _ = expect_request(&mut peer, "um").await;
    let _ = expect_request(&mut peer, "dois").await;
    send(&mut peer, created("resp_1", None)).await;
    send(&mut peer, audio_delta("resp_1", &[1, 0])).await;
    send(&mut peer, created("resp_2", None)).await;
    send(&mut peer, audio_delta("resp_2", &[2, 0])).await;
    let out = non_log(&drain(&mut s.outputs, 600).await).into_iter().cloned_debug();
    assert_eq!(
        out,
        vec![
            format!("{:?}", diag("r1", None, None, "local", Some("unassociated"))),
            format!("{:?}", Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed }),
            format!("{:?}", diag("r2", None, None, "local", Some("unassociated"))),
            format!("{:?}", Output::Done { response_id: "r2".into(), status: ResponseStatus::Failed }),
        ]
    );
}

#[tokio::test]
async fn a_spontaneous_response_first_is_not_bound_to_the_request() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let _ = expect_request(&mut peer, "oi").await;
    send(&mut peer, created("resp_spont", None)).await;
    send(&mut peer, audio_delta("resp_spont", &[7, 0])).await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });

    reply(&mut peer, "resp_spont2", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    send(&mut peer, done("resp_spont", "completed", None)).await;
    let out = drain(&mut s.outputs, 150).await;
    assert!(out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("quarantined response resp_spont (ambiguous) ended completed; dropped 1 audio"))), "{out:?}");
    assert!(non_log(&out).is_empty());
}

#[tokio::test]
async fn any_create_error_fails_once_resets_and_the_next_request_binds() {
    for (n, (error, reason)) in [
        (json!({"type": "invalid_request_error", "code": "conversation_already_has_active_response", "message": "busy"}), "create_rejected: type=invalid_request_error code=conversation_already_has_active_response"),
        (json!({"type": "server_error", "message": "internal processing error"}), "create_rejected: type=server_error code=server_error"),
        (json!({"type": "invalid_request_error", "code": "something_new", "message": "?"}), "create_rejected: type=invalid_request_error code=something_new"),
    ]
    .into_iter()
    .enumerate()
    {
        let (mut s, mut peer, mut conns) = rig(|c| c.associate_wait = Duration::from_secs(5)).await;
        s.commands.send(text("q1", "r1", "um")).await.unwrap();
        let create = expect_request(&mut peer, "um").await;
        let mut error = error;
        error["event_id"] = create["event_id"].clone();
        send(&mut peer, json!({"type": "error", "error": error})).await;
        assert_eq!(next(&mut s.outputs).await, diag("r1", None, None, "local", Some(reason)), "case {n}");
        assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed });
        let _ = peer.send(Message::text(created("maybe_ran", None).to_string())).await;
        let _ = peer.send(Message::text(audio_delta("maybe_ran", &[1, 0]).to_string())).await;
        let (mut peer2, _, _) = accept_setup(&mut conns).await;
        let mut before_ready = Vec::new();
        loop {
            match next_any(&mut s.outputs).await {
                Output::Log { msg, .. } if msg.starts_with("openai-realtime ready gen=2") => break,
                o => before_ready.push(o),
            }
        }
        assert!(before_ready.contains(&Output::ContextLost { why: "provider session gen=2 started (no native resumption)".into() }), "case {n}");
        assert!(!before_ready.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. } | Output::Done { .. })), "case {n}: {before_ready:?}");
        s.commands.send(text("q2", "r2", "dois")).await.unwrap();
        let create2 = expect_request(&mut peer2, "dois").await;
        assert_eq!(create2["response"]["metadata"]["rth_gen"], "2");
        send(&mut peer2, created("resp_2", Some(json!({"rth_request": "q2", "rth_gen": "2"})))).await;
        assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r2".into(), request_id: "q2".into() });
    }
}

#[tokio::test]
async fn item_memory_fails_closed_past_both_bounds() {

    let (out, mut outputs) = mpsc::channel(64);
    let mut st = State::new(Config::for_test("ws://127.0.0.1:1".into()), out);
    st.on_transcript(Some("rejected".into()), "delete all".into(), false).await;
    st.on_transcript(Some("rejected".into()), "do not delete".into(), true).await;
    for n in 0..MAX_TRACKED {
        st.utterance_of(&format!("a_{n}"));
    }
    assert!(!st.saturated && st.retired.contains(&"rejected".to_string()), "past the first bound, the rejection is still known");
    for n in 0..MAX_RETIRED {
        st.utterance_of(&format!("b_{n}"));
    }
    assert!(st.saturated, "one past the retired bound, the session is fenced");
    assert!(st.items.len() <= MAX_TRACKED && st.retired.len() <= MAX_RETIRED && st.ended_items.len() <= MAX_TRACKED);
    drain(&mut outputs, 5).await;
    st.on_transcript(Some("rejected".into()), "delete all".into(), false).await;
    st.on_transcript(Some("rejected".into()), "delete all".into(), true).await;
    st.on_transcript(Some("brand_new".into()), "oi".into(), true).await;
    let out = drain(&mut outputs, 5).await;
    assert!(!out.iter().any(|o| matches!(o, Output::InputTranscript { .. })), "{out:?}");
    assert_eq!(out.iter().filter(|o| matches!(o, Output::Log { msg, .. } if msg.contains("item memory is saturated"))).count(), 3);

    let (mut s, mut peer, mut conns) = rig(|_| {}).await;
    let delta = |item: &str, d: &str| json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": item, "delta": d});
    let completed = |item: &str, t: &str| json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": item, "transcript": t});

    let (late_delta, late_completed) = (delta("rejected", " now"), completed("rejected", "delete all"));
    send(&mut peer, delta("rejected", "delete all")).await;
    send(&mut peer, completed("rejected", "do not delete")).await;
    let provider = tokio::spawn(async move {
        for n in 0..(MAX_TRACKED + MAX_RETIRED) {
            let _ = peer.send(Message::text(json!({"type": "input_audio_buffer.committed", "item_id": format!("c_{n}")}).to_string())).await;
        }
        let _ = peer.send(Message::text(late_delta.to_string())).await;
        let _ = peer.send(Message::text(late_completed.to_string())).await;
    });
    let next_session = tokio::spawn(async move { accept_setup(&mut conns).await.0 });
    let mut before_ready = Vec::new();
    loop {
        match next_any(&mut s.outputs).await {
            Output::Log { msg, .. } if msg.starts_with("openai-realtime ready gen=2") => break,
            o => before_ready.push(o),
        }
    }
    let parts: Vec<&Output> = before_ready.iter().filter(|o| matches!(o, Output::InputTranscript { .. })).collect();
    assert_eq!(parts.len(), 1, "only the rejected item's first, unfinished part ever appeared: {parts:?}");
    assert!(matches!(parts[0], Output::InputTranscript { heard, .. } if heard.finished.is_none()));
    assert!(before_ready.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("item memory saturated"))));
    assert!(before_ready.contains(&Output::ContextLost { why: "provider session gen=2 started (no native resumption)".into() }));
    let _ = provider.await;
    let mut peer2 = next_session.await.unwrap();
    send(&mut peer2, json!({"type": "input_audio_buffer.committed", "item_id": "fresh"})).await;
    send(&mut peer2, completed("fresh", "oi de novo")).await;
    let out = drain(&mut s.outputs, 200).await;
    assert_eq!(settled_after(&out).into_iter().map(|t| t.text).collect::<Vec<_>>(), vec!["oi de novo".to_string()]);
}

#[tokio::test]
async fn a_delayed_response_from_an_old_session_is_never_bound() {
    let (mut s, peer, mut conns) = rig(|_| {}).await;
    drop(peer);
    let (mut peer2, _, _) = accept_setup(&mut conns).await;

    loop {
        match next_any(&mut s.outputs).await {
            Output::ContextLost { why } => assert_eq!(why, "provider session gen=2 started (no native resumption)"),
            Output::Log { msg, .. } if msg.starts_with("openai-realtime ready gen=2") => break,
            _ => {}
        }
    }
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let create = expect_request(&mut peer2, "oi").await;
    assert_eq!(create["response"]["metadata"]["rth_gen"], "2");
    send(&mut peer2, created("resp_old", Some(json!({"rth_request": "q1", "rth_gen": "1"})))).await;
    assert_eq!(recv_json(&mut peer2).await["type"], "response.cancel", "quarantined and cancelled");
    send(&mut peer2, created("resp_new", Some(json!({"rth_request": "q1", "rth_gen": "2"})))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
}

#[tokio::test]
async fn an_unsolicited_response_under_create_false_is_quarantined_and_cancelled() {
    let (mut s, mut peer, _c) = rig(|c| c.create_response = false).await;
    send(&mut peer, created("resp_u", None)).await;
    let cancel = recv_json(&mut peer).await;
    assert_eq!((cancel["type"].as_str(), cancel["response_id"].as_str()), (Some("response.cancel"), Some("resp_u")));
    send(&mut peer, audio_delta("resp_u", &[4, 4])).await;
    send(&mut peer, text_delta("resp_u", "x")).await;
    send(&mut peer, done("resp_u", "cancelled", Some("client_cancelled"))).await;
    let out = drain(&mut s.outputs, 250).await;
    assert!(non_log(&out).is_empty(), "{out:?}");
    assert!(out.iter().any(|o| matches!(o, Output::Log { level: "warn", msg } if msg.contains("anomaly: unsolicited response resp_u"))));
}

#[tokio::test]
async fn mode_c_commits_only_inside_respond() {
    let (s, mut peer, _c) = rig(|c| c.vad = "none".into()).await;
    s.commands.send(Command::Audio { sample_rate: 24_000, pcm: vec![1, 0, 2, 0] }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await, json!({"type": "input_audio_buffer.append", "audio": "AQACAA=="}));
    s.commands.send(Command::AudioEnd { stream_id: "m1".into() }).await.unwrap();
    silent(&mut peer, 150).await;
    s.commands.send(Command::Respond { request_id: "q1".into(), response_id: "r1".into(), decision_id: "d1".into(), utterance_id: None }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["type"], "input_audio_buffer.commit");
    assert_eq!(recv_json(&mut peer).await["type"], "response.create");

    s.commands.send(Command::Audio { sample_rate: 24_000, pcm: vec![3, 0] }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["type"], "input_audio_buffer.append");
    silent(&mut peer, 150).await;
}

fn turn(role: &str, text: &str, kind: &str, status: Option<&str>, finished: Option<bool>) -> crate::context::Turn {
    crate::context::Turn {
        role: role.into(),
        text: text.into(),
        kind: kind.into(),
        event: "e".into(),
        response_id: None,
        status: status.map(str::to_string),
        finished,
        boundary: None,
    }
}

#[tokio::test]
async fn restored_turns_go_out_first_as_items_only() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let turns = vec![
        turn("user", "lembra: azul", "typed", None, None),
        turn("model", "Ok.", "reply", Some("completed"), None),
        turn("model", "cortad", "reply", Some("cancelled"), None),
        turn("user", "meia fr", "speech", None, Some(false)),
    ];
    s.commands.send(Command::Restore { turns }).await.unwrap();
    s.commands.send(text("q1", "r1", "qual cor?")).await.unwrap();
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    assert_eq!(
        recv_json(&mut peer).await,
        json!({"type": "conversation.item.create", "item": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "lembra: azul"}]}})
    );
    assert_eq!(
        recv_json(&mut peer).await,
        json!({"type": "conversation.item.create", "item": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Ok."}]}})
    );
    let _ = expect_request(&mut peer, "qual cor?").await;
    let out = drain(&mut s.outputs, 200).await;
    assert!(out.contains(&Output::ContextInjected { turns: 2, bytes: 15, omitted: 2, truncated: 0 }), "{out:?}");
}

fn pcm(samples: impl Iterator<Item = i16>) -> Vec<u8> {
    samples.flat_map(i16::to_le_bytes).collect()
}

#[test]
fn resample_counts_are_exact_across_odd_chunks() {
    let n = 16_000usize + 7;
    let input = pcm((0..n).map(|i| ((i as f64 * 0.05).sin() * 8000.0) as i16));
    let whole = Resampler::new(16_000, 24_000).push(&input);
    let expect = ((n as u64 - 1) * 24_000).div_ceil(16_000);
    assert_eq!(whole.len() as u64 / 2, expect);
    let mut r = Resampler::new(16_000, 24_000);
    let mut chunked = Vec::new();
    let mut at = 0;
    for size in [1usize, 2, 3, 319, 320, 641, 7, 13].iter().cycle() {
        if at >= input.len() {
            break;
        }
        let end = (at + size * 2).min(input.len());
        chunked.extend(r.push(&input[at..end]));
        at = end;
    }
    assert_eq!(chunked, whole, "same samples, whatever the chunking");

    let mut r = Resampler::new(16_000, 24_000);
    let first = r.push(&vec![0u8; 640]).len() / 2;
    let second = r.push(&vec![0u8; 640]).len() / 2;
    assert_eq!((first, second), (479, 480));

    assert_eq!(Resampler::new(16_000, 16_000).push(&input), input);

    assert_eq!(Resampler::new(16_000, 24_000).push(&pcm([0i16, 300, 600].into_iter())), pcm([0i16, 200, 400].into_iter()));
}

#[tokio::test]
async fn mic_audio_is_appended_at_24_khz() {
    let (s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(Command::Audio { sample_rate: 16_000, pcm: vec![0; 640] }).await.unwrap();
    s.commands.send(Command::Audio { sample_rate: 16_000, pcm: vec![0; 640] }).await.unwrap();
    let mut got = 0;
    for _ in 0..2 {
        let m = recv_json(&mut peer).await;
        assert_eq!(m["type"], "input_audio_buffer.append");
        got += base64::engine::general_purpose::STANDARD.decode(m["audio"].as_str().unwrap()).unwrap().len() / 2;
    }
    assert_eq!(got, 479 + 480);
}

#[tokio::test]
async fn transcripts_are_per_item_local_utterances() {
    let (mut s, mut peer, _c) = rig(|c| c.response_prefix = "b1-v".into()).await;
    let delta = |item: &str, d: &str| json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": item, "content_index": 0, "delta": d});
    let completed = |item: &str, t: &str| json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": item, "content_index": 0, "transcript": t});
    send(&mut peer, delta("item_a", "olá ")).await;
    send(&mut peer, completed("item_b", "tchau")).await;
    send(&mut peer, delta("item_a", "mundo")).await;
    send(&mut peer, completed("item_a", "olá mundo!")).await;
    let part = |text: &str, uid: &str, seq: u64, finished: Option<bool>| Output::InputTranscript {
        text: text.into(),
        heard: Heard { utterance_id: Some(uid.into()), seq: Some(seq), finished, response_id: None, association: Some("unknown"), boundary: None },
    };
    assert_eq!(next(&mut s.outputs).await, part("olá ", "b1-u1", 0, None));
    assert_eq!(next(&mut s.outputs).await, part("tchau", "b1-u2", 0, Some(true)));
    assert_eq!(next(&mut s.outputs).await, part("mundo", "b1-u1", 1, None));
    assert_eq!(next(&mut s.outputs).await, part("!", "b1-u1", 2, Some(true)));
}

fn settled_after(outputs: &[Output]) -> Vec<crate::context::Turn> {
    use rth_protocol::{Event, MonotonicClock, Source, Stamper};
    let mut fold = crate::context::Fold::default();
    let mut stamp = Stamper::new(Source::Guest, "test", Box::new(MonotonicClock::start()));
    for o in outputs {
        if let Output::InputTranscript { text, heard } = o {
            let ev = Event::InputTranscript {
                text: text.clone(),
                utterance_id: heard.utterance_id.clone(),
                utterance_seq: heard.seq,
                is_final: heard.finished,
                response_id: heard.response_id.clone(),
                association: heard.association.map(str::to_owned),
                boundary: heard.boundary.map(str::to_owned),
            };
            fold.apply(&stamp.stamp(ev, None));
        }
    }
    let turns: Vec<_> = fold.turns.into_iter().collect();
    crate::gemini::settled_history(&turns).0.into_iter().cloned().collect()
}

#[tokio::test]
async fn a_completion_that_does_not_extend_the_parts_is_never_final() {
    let delta = |item: &str, d: &str| json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": item, "delta": d});
    let completed = |item: &str, t: &str| json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": item, "transcript": t});
    for (deltas, done_text) in [("delete all", "do not delete"), ("delete all", "delete"), ("abc", "xyz")] {
        let (mut s, mut peer, _c) = rig(|_| {}).await;
        send(&mut peer, delta("item_c", deltas)).await;
        send(&mut peer, completed("item_c", done_text)).await;

        send(&mut peer, completed("item_d", "ok")).await;
        let out = drain(&mut s.outputs, 250).await;
        let parts: Vec<&Output> = out.iter().filter(|o| matches!(o, Output::InputTranscript { .. })).collect();
        assert_eq!(parts.len(), 2, "{out:?}");
        let Output::InputTranscript { heard: first, .. } = parts[0] else { unreachable!() };
        assert_eq!(first.finished, None, "the provisional part is never final ({deltas:?} → {done_text:?})");
        let Output::InputTranscript { text, heard: second } = parts[1] else { unreachable!() };
        assert_eq!((text.as_str(), second.finished, second.boundary), ("ok", Some(true), Some("local")));
        assert!(
            out.iter().any(|o| matches!(o, Output::Log { level: "warn", msg } if msg.contains(&format!("completed as {done_text:?}")) && msg.contains(&format!("{deltas:?} sent as parts")))),
            "{out:?}"
        );
        let kept: Vec<String> = settled_after(&out).into_iter().map(|t| t.text).collect();
        assert_eq!(kept, vec!["ok".to_string()], "only the next, whole utterance is settled ({deltas:?} → {done_text:?})");
    }

    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, delta("item_e", "do not ")).await;
    send(&mut peer, completed("item_e", "do not delete")).await;
    let out = drain(&mut s.outputs, 200).await;
    assert_eq!(settled_after(&out).into_iter().map(|t| t.text).collect::<Vec<_>>(), vec!["do not delete".to_string()]);
}

#[tokio::test]
async fn a_replys_position_is_logged_and_never_becomes_an_identity() {
    let (mut s, mut peer, _c) = rig(|c| c.response_prefix = "b1-v".into()).await;
    send(&mut peer, json!({"type": "input_audio_buffer.committed", "item_id": "item_u1"})).await;
    reply(&mut peer, "resp_1", Some("item_u1")).await;
    send(&mut peer, json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_u1", "transcript": "oi"})).await;
    let out = drain(&mut s.outputs, 250).await;
    assert_eq!(
        non_log(&out),
        vec![
            &Output::Opened { response_id: "b1-v1".into(), utterance_id: None },
            &Output::InputTranscript {
                text: "oi".into(),
                heard: Heard { utterance_id: Some("b1-u1".into()), seq: Some(0), finished: Some(true), response_id: None, association: Some("unknown"), boundary: None },
            },
        ]
    );
    assert!(
        out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("adjacency only, not an answers-to link") && msg.contains("follows item item_u1 (utterance b1-u1)"))),
        "{out:?}"
    );
}

#[tokio::test]
async fn an_earlier_echo_does_not_open_a_later_unlabelled_response() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "um")).await.unwrap();
    let _ = expect_request(&mut peer, "um").await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
    s.commands.send(text("q2", "r2", "dois")).await.unwrap();
    let _ = expect_request(&mut peer, "dois").await;
    send(&mut peer, created("resp_unlabelled", None)).await;
    send(&mut peer, audio_delta("resp_unlabelled", &[7, 0])).await;
    let out = drain(&mut s.outputs, 150).await;
    assert!(!out.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. })), "{out:?}");
    assert!(out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("resp_unlabelled quarantined: no metadata while 1 request(s) are pending"))), "{out:?}");
}

#[tokio::test]
async fn an_unassociated_request_fails_once_and_resets_the_session() {
    let (mut s, mut peer, mut conns) = rig(|_| {}).await;
    s.commands.send(text("q1", "r1", "uma resposta longa")).await.unwrap();
    let _ = expect_request(&mut peer, "uma resposta longa").await;
    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, diag("r1", None, None, "local", Some("unassociated")));
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed });

    let _ = peer.send(Message::text(created("late", None).to_string())).await;
    let _ = peer.send(Message::text(audio_delta("late", &[1, 0]).to_string())).await;
    let (mut peer2, _, _) = accept_setup(&mut conns).await;
    let mut before_ready = Vec::new();
    loop {
        match next_any(&mut s.outputs).await {
            Output::Log { msg, .. } if msg.starts_with("openai-realtime ready gen=2") => break,
            o => before_ready.push(o),
        }
    }
    assert!(before_ready.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("reset after unassociated request q1"))), "{before_ready:?}");
    assert!(before_ready.contains(&Output::ContextLost { why: "provider session gen=2 started (no native resumption)".into() }));
    assert!(!before_ready.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. } | Output::Done { .. })), "exactly one terminal: {before_ready:?}");
    reply(&mut peer2, "resp_new", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
}

#[tokio::test]
async fn zero_output_responses_keep_their_lifecycle() {
    let (mut s, mut peer, mut conns) = rig(|_| {}).await;
    for (n, (status, reason, mapped)) in [
        ("failed", Some("server_error"), ResponseStatus::Failed),
        ("incomplete", Some("max_output_tokens"), ResponseStatus::Failed),
        ("cancelled", Some("turn_detected"), ResponseStatus::Cancelled),
        ("completed", None, ResponseStatus::Completed),
    ]
    .into_iter()
    .enumerate()
    {
        let (pid, rid) = (format!("silent_{n}"), format!("v{}", n + 1));
        send(&mut peer, created(&pid, None)).await;
        send(&mut peer, done(&pid, status, reason)).await;
        let mut want = vec![format!("{:?}", Output::Opened { response_id: rid.clone(), utterance_id: None })];
        if status == "cancelled" {
            want.push(format!("{:?}", Output::Signal { response_id: Some(rid.clone()), signal: "interrupted" }));
        }
        if status != "completed" {
            want.push(format!("{:?}", diag(&rid, Some(status), reason, "provider", None)));
        }
        want.push(format!("{:?}", Output::Done { response_id: rid, status: mapped }));
        assert_eq!(non_log(&drain(&mut s.outputs, 150).await).into_iter().cloned_debug(), want, "{status}");
    }
    send(&mut peer, created("cut_off", None)).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v5".into(), utterance_id: None });
    drop(peer);
    let (_peer2, _, _) = accept_setup(&mut conns).await;
    let out = non_log(&drain(&mut s.outputs, 250).await).into_iter().cloned_debug();
    assert_eq!(out.iter().filter(|o| o.starts_with("Done")).count(), 1, "{out:?}");
    assert!(out.contains(&format!("{:?}", diag("v5", None, None, "local", Some("session_ended")))), "{out:?}");
}

#[tokio::test]
async fn a_write_error_settles_each_kind_of_request_once() {
    for (kind, manual) in [("text", false), ("respond-commit", true), ("respond-create", false)] {
        let (url, mut conns) = mock().await;
        let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str()).await.unwrap();
        let (_peer, _) = conns.recv().await.unwrap();
        let mut cfg = Config::for_test(url);
        if manual {
            cfg.vad = "none".into();
        }
        let (out, mut outputs) = mpsc::channel(32);
        let mut st = State::new(cfg, out);
        ws.close(None).await.unwrap();
        let text = (kind == "text").then(|| "olá".to_string());
        assert!(st.request(&mut ws, "q1".into(), "r1".into(), text, manual).await.is_err(), "{kind}");
        st.fail_all("write failure").await;
        let got = non_log(&drain(&mut outputs, 50).await).into_iter().cloned_debug();
        assert_eq!(
            got,
            vec![format!("{:?}", diag("r1", None, None, "local", Some("send_failed")).with_gen(0)), format!("{:?}", Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed })],
            "{kind}"
        );
    }
}

#[tokio::test]
async fn a_write_error_while_draining_the_waiting_queue_settles_once() {
    let (url, mut conns) = mock().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str()).await.unwrap();
    let (_peer, _) = conns.recv().await.unwrap();
    let (out, mut outputs) = mpsc::channel(32);
    let mut st = State::new(Config::for_test(url), out);
    st.offline(text("q1", "r1", "esperou")).await;
    ws.close(None).await.unwrap();
    let (_tx, mut rx) = mpsc::channel(1);
    assert!(matches!(session(&mut st, ws, &mut rx).await, End::Reconnect(_)));
    st.fail_all("write failure").await;
    let got = non_log(&drain(&mut outputs, 50).await).into_iter().cloned_debug();
    assert_eq!(got.iter().filter(|o| o.starts_with("Done")).count(), 1, "{got:?}");
    assert!(got.iter().any(|o| o.contains("send_failed")), "{got:?}");
}

#[tokio::test]
async fn the_key_is_sent_as_a_bearer_header_and_never_logged() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, setup, auth) = accept_setup(&mut conns).await;
    assert_eq!(auth.as_deref(), Some(format!("Bearer {TEST_KEY}").as_str()));
    assert!(!setup.to_string().contains(TEST_KEY));
    send(&mut peer, json!({"type": "error", "error": {"message": "boom"}})).await;
    drop(peer);
    let (_peer2, _, _) = accept_setup(&mut conns).await;
    let out = drain(&mut s.outputs, 300).await;
    assert!(!out.is_empty());
    for o in &out {
        assert!(!format!("{o:?}").contains(TEST_KEY), "{o:?}");
    }
}

#[tokio::test]
async fn regression_cancelled_unechoed_request_does_not_reappear_as_audio() {
    let (mut s, mut peer, _c) = rig(|c| c.associate_wait = Duration::from_millis(40)).await;
    s.commands.send(text("q1", "r1", "a long answer")).await.unwrap();
    let _ = expect_request(&mut peer, "a long answer").await;
    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    let expired = drain(&mut s.outputs, 150).await;
    assert!(expired.iter().any(|o| matches!(o, Output::Done { response_id, .. } if response_id == "r1")), "{expired:?}");

    let _ = peer.send(Message::text(created("late_requested_reply_without_echo", None).to_string())).await;
    let _ = peer.send(Message::text(audio_delta("late_requested_reply_without_echo", &[1,0,2,0]).to_string())).await;
    let out = drain(&mut s.outputs, 100).await;
    assert!(!out.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. })), "cancelled request escaped as {out:?}");
}

#[tokio::test]
async fn regression_failed_zero_output_response_has_a_terminal() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, created("silent_failure", None)).await;
    send(&mut peer, done("silent_failure", "failed", Some("server_error"))).await;
    let out = drain(&mut s.outputs, 100).await;
    assert!(out.iter().any(|o| matches!(o, Output::Done { status: ResponseStatus::Failed, .. })), "provider failure invisible: {out:?}");
}

#[tokio::test]
async fn regression_completed_correction_is_not_a_false_final() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"u1","delta":"delete all"})).await;
    send(&mut peer, json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"u1","transcript":"do not delete"})).await;
    let out = drain(&mut s.outputs, 100).await;
    let joined: String = out.iter().filter_map(|o| match o { Output::InputTranscript { text, .. } => Some(text.as_str()), _ => None }).collect();
    let final_seen = out.iter().any(|o| matches!(o, Output::InputTranscript { heard, .. } if heard.finished == Some(true)));
    assert!(!final_seen || joined == "do not delete", "false final: {joined:?}; {out:?}");
}

#[tokio::test]
async fn regression_conversation_adjacency_is_not_an_answer_identity() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"u1","transcript":"an unrelated remark"})).await;
    let _ = next(&mut s.outputs).await;

    reply(&mut peer, "out_of_band_reply", Some("u1")).await;
    let out = next(&mut s.outputs).await;
    assert!(matches!(out, Output::Opened { utterance_id: None, .. }), "adjacency promoted into canonical identity: {out:?}");
}

#[tokio::test]
async fn regression_send_failure_settles_the_admitted_request() {
    let (url, mut conns) = mock().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str()).await.unwrap();
    let (_peer, _) = conns.recv().await.unwrap();
    let cfg = Config::for_test(url);
    let (out, mut outputs) = mpsc::channel(32);

    let mut st = State::new(cfg, out);

    ws.close(None).await.unwrap();
    assert!(st.request(&mut ws, "q1".into(), "r1".into(), Some("hello".into()), false).await.is_err());
    st.fail_all("write failure").await;
    let observed = drain(&mut outputs, 30).await;
    assert!(observed.iter().any(|o| matches!(o, Output::Done { response_id, status: ResponseStatus::Failed } if response_id == "r1")), "admitted request has no terminal: {observed:?}");
}

#[tokio::test]
async fn a_rejected_item_or_commit_fails_once_and_resets_the_session() {
    for (kind, manual) in [("item", false), ("commit", true)] {
        let (mut s, mut peer, mut conns) = rig(|c| {
            c.associate_wait = Duration::from_secs(5);
            if manual {
                c.vad = "none".into();
            }
        })
        .await;
        let cmd = if manual {
            Command::Respond { request_id: "q1".into(), response_id: "r1".into(), decision_id: "d1".into(), utterance_id: None }
        } else {
            text("q1", "r1", "uma resposta")
        };
        s.commands.send(cmd).await.unwrap();
        let first = recv_json(&mut peer).await;
        assert_eq!(first["type"], if manual { "input_audio_buffer.commit" } else { "conversation.item.create" });
        assert_eq!(recv_json(&mut peer).await["type"], "response.create");
        s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
        send(&mut peer, json!({"type": "error", "error": {"type": "invalid_request_error", "code": "invalid_value", "message": "refused", "event_id": first["event_id"]}})).await;
        assert_eq!(next(&mut s.outputs).await, diag("r1", None, None, "local", Some(&format!("{kind}_rejected: type=invalid_request_error code=invalid_value"))), "{kind}");
        assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed });
        let _ = peer.send(Message::text(created("inflight", None).to_string())).await;
        let _ = peer.send(Message::text(audio_delta("inflight", &[1, 0]).to_string())).await;
        let (_peer2, _, _) = accept_setup(&mut conns).await;
        let mut before_ready = Vec::new();
        loop {
            match next_any(&mut s.outputs).await {
                Output::Log { msg, .. } if msg.starts_with("openai-realtime ready gen=2") => break,
                o => before_ready.push(o),
            }
        }
        assert!(before_ready.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains(&format!("reset after an error on the {kind} of request q1")))), "{before_ready:?}");
        assert!(before_ready.contains(&Output::ContextLost { why: "provider session gen=2 started (no native resumption)".into() }));
        assert!(!before_ready.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. } | Output::Done { .. })), "{kind}: exactly one terminal, nothing uncertain plays: {before_ready:?}");
    }
}

#[tokio::test]
async fn an_ended_item_never_reopens() {
    let delta = |item: &str, d: &str| json!({"type": "conversation.item.input_audio_transcription.delta", "item_id": item, "delta": d});
    let completed = |item: &str, t: &str| json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": item, "transcript": t});
    let (mut s, mut peer, _c) = rig(|_| {}).await;

    send(&mut peer, delta("u1", "delete all")).await;
    send(&mut peer, completed("u1", "do not delete")).await;
    send(&mut peer, completed("u1", "do not delete")).await;
    send(&mut peer, delta("u1", " now")).await;
    send(&mut peer, completed("u1", "delete all now")).await;

    send(&mut peer, completed("u2", "bom dia")).await;
    send(&mut peer, completed("u2", "bom dia")).await;
    send(&mut peer, completed("u2", "bom dia, RTH")).await;

    send(&mut peer, completed("u3", "que horas são")).await;
    let out = drain(&mut s.outputs, 300).await;
    type Part = (String, Option<String>, Option<u64>, Option<bool>);
    let parts: Vec<Part> = out
        .iter()
        .filter_map(|o| match o {
            Output::InputTranscript { text, heard } => Some((text.clone(), heard.utterance_id.clone(), heard.seq, heard.finished)),
            _ => None,
        })
        .collect();
    assert_eq!(
        parts,
        vec![
            ("delete all".into(), Some("u1".into()), Some(0), None),
            ("bom dia".into(), Some("u2".into()), Some(0), Some(true)),
            ("que horas são".into(), Some("u3".into()), Some(0), Some(true)),
        ],
        "{out:?}"
    );
    let logs = |needle: &str| out.iter().filter(|o| matches!(o, Output::Log { msg, .. } if msg.contains(needle))).count();
    assert_eq!(logs("for item u1 after its completion was rejected; refused"), 3, "{out:?}");
    assert_eq!(logs("for item u2 a duplicate of its completion; ignored"), 1);
    assert_eq!(logs("for item u2 after its completion; refused"), 1);
    assert_eq!(settled_after(&out).into_iter().map(|t| t.text).collect::<Vec<_>>(), vec!["bom dia".to_string(), "que horas são".to_string()]);
}

#[tokio::test]
async fn an_evicted_item_is_refused_if_it_recurs() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    for n in 0..=MAX_TRACKED {
        send(&mut peer, json!({"type": "input_audio_buffer.committed", "item_id": format!("item_{n}")})).await;
    }
    send(&mut peer, json!({"type": "conversation.item.input_audio_transcription.completed", "item_id": "item_0", "transcript": "tarde demais"})).await;
    let out = drain(&mut s.outputs, 400).await;
    assert!(!out.iter().any(|o| matches!(o, Output::InputTranscript { .. })), "{out:?}");
    assert!(out.iter().any(|o| matches!(o, Output::Log { msg, .. } if msg.contains("for item item_0 for an item no longer tracked; refused"))));
}

#[tokio::test]
async fn regression_rev2_item_error_does_not_release_an_inflight_create() {
    let (mut s, mut peer, _c) = rig(|c| c.associate_wait = Duration::from_secs(2)).await;
    s.commands.send(text("q1", "r1", "a long answer")).await.unwrap();
    let item = recv_json(&mut peer).await;
    let create = recv_json(&mut peer).await;
    assert_eq!(create["type"], "response.create");
    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    send(&mut peer, json!({"type":"error","error":{"type":"invalid_request_error","code":"invalid_value","message":"item refused", "event_id":item["event_id"]}})).await;
    let failed = drain(&mut s.outputs, 70).await;
    assert!(failed.iter().any(|o| matches!(o, Output::Done { response_id, .. } if response_id == "r1")), "{failed:?}");

    let _ = peer.send(Message::text(created("already_inflight_without_echo", None).to_string())).await;
    let _ = peer.send(Message::text(audio_delta("already_inflight_without_echo", &[1,0,2,0]).to_string())).await;
    let out = drain(&mut s.outputs, 70).await;
    assert!(!out.iter().any(|o| matches!(o, Output::Opened { .. } | Output::Audio { .. })), "cancelled create escaped after item rejection: {out:?}");
}

#[tokio::test]
async fn regression_rev2_repeated_correction_cannot_restore_corrupt_words() {
    use rth_protocol::{Event, MonotonicClock, Source, Stamper};
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    send(&mut peer, json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"u1","delta":"delete all"})).await;
    let completed = json!({"type":"conversation.item.input_audio_transcription.completed","event_id":"finished-u1","item_id":"u1","transcript":"do not delete"});
    send(&mut peer, completed.clone()).await;
    send(&mut peer, completed).await;
    let out = drain(&mut s.outputs, 70).await;
    let mut fold = crate::context::Fold::default();
    let mut stamp = Stamper::new(Source::Guest, "test", Box::new(MonotonicClock::start()));
    for o in &out {
        if let Output::InputTranscript { text, heard } = o {
            fold.apply(&stamp.stamp(Event::InputTranscript {
                text:text.clone(), utterance_id:heard.utterance_id.clone(), utterance_seq:heard.seq,
                is_final:heard.finished,response_id:heard.response_id.clone(),association:heard.association.map(str::to_owned),boundary:heard.boundary.map(str::to_owned)
            },None));
        }
    }
    let turns: Vec<_> = fold.turns.into_iter().collect();
    let restored: Vec<_> = crate::gemini::settled_history(&turns).0.into_iter().map(|t| t.text.clone()).collect();
    assert!(restored.is_empty() || restored == ["do not delete"], "false settled words: {restored:?}; {out:?}");
}

#[tokio::test]
async fn regression_rev3_unknown_create_error_does_not_release_ambiguity() {
    let (mut s, mut peer, _c) = rig(|c| c.associate_wait = Duration::from_secs(2)).await;
    s.commands.send(text("q1", "r1", "a long answer")).await.unwrap();
    let create = expect_request(&mut peer, "a long answer").await;
    s.commands.send(Command::Cancel { response_id:"r1".into() }).await.unwrap();
    send(&mut peer, json!({"type":"error","error":{"type":"server_error","message":"internal processing error","event_id":create["event_id"]}})).await;
    let first = drain(&mut s.outputs, 70).await;
    assert!(first.iter().any(|o| matches!(o, Output::Done { response_id,.. } if response_id=="r1")), "{first:?}");

    let _ = peer.send(Message::Text(created("uncertain_create_without_echo",None).to_string().into())).await;
    let _ = peer.send(Message::Text(audio_delta("uncertain_create_without_echo",&[1,0,2,0]).to_string().into())).await;
    let out = drain(&mut s.outputs,70).await;
    assert!(!out.iter().any(|o| matches!(o, Output::Opened{..}|Output::Audio{..})), "unknown create error was treated as definitive rejection: {out:?}");
}

#[tokio::test]
async fn regression_rev3_retirement_capacity_cannot_resurrect_rejected_item() {
    let cfg = Config::for_test("ws://127.0.0.1:1".into());
    let (out, mut outputs) = mpsc::channel(32);
    let mut st = State::new(cfg,out);
    st.on_transcript(Some("rejected".into()),"delete all".into(),false).await;
    st.on_transcript(Some("rejected".into()),"do not delete".into(),true).await;
    let first = drain(&mut outputs,5).await;
    assert!(first.iter().any(|o| matches!(o, Output::Log{msg,..} if msg.contains("does not extend"))));

    for n in 0..(MAX_TRACKED+MAX_RETIRED) { st.utterance_of(&format!("later_{n}")); }
    st.on_transcript(Some("rejected".into()),"delete all".into(),true).await;
    let out = drain(&mut outputs,5).await;
    assert!(!out.iter().any(|o| matches!(o, Output::InputTranscript{..})), "retired rejection forgotten: {out:?}");
}

trait ClonedDebug {
    fn cloned_debug(self) -> Vec<String>;
}

impl<'a, I: Iterator<Item = &'a Output>> ClonedDebug for I {
    fn cloned_debug(self) -> Vec<String> {
        self.map(|o| format!("{o:?}")).collect()
    }
}

#[tokio::test]
async fn delegation_tools_are_declared_and_a_visible_responses_call_is_answered() {
    use rth_tasks::schema::Tool;
    let off = setup_message(&Config::for_test("ws://x".into()));
    assert!(off["session"].get("tools").is_none(), "off: the pinned default setup is unchanged");

    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    cfg.tools = vec![crate::tools::Tool::Task(Tool::Delegate), crate::tools::Tool::Task(Tool::CancelTask)];
    let mut s = connect(cfg);
    let (mut peer, setup, _) = accept_setup(&mut conns).await;
    assert_eq!(setup["session"]["tools"][0]["name"], "delegate");
    assert_eq!(setup["session"]["tool_choice"], "auto");
    assert!(!setup["session"]["instructions"].as_str().unwrap().contains("You have no tools"));
    assert!(setup["session"]["instructions"].as_str().unwrap().contains("- delegation: not observed\n"), "v2.1: declared, readiness not observed (P21-R1)");
    ready(&mut s.outputs).await;

    reply(&mut peer, "resp_1", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    let call = json!({"type": "response.output_item.done", "response_id": "resp_1", "item": {"type": "function_call", "id": "fi_1", "call_id": "call_1", "name": "delegate", "arguments": "{\"purpose\":\"report\",\"task\":\"relatório\"}"}});
    send(&mut peer, call).await;
    assert_eq!(
        next(&mut s.outputs).await,
        Output::ToolCall { response_id: Some("v1".into()), generation: 1, call_id: "call_1".into(), name: "delegate".into(), args: json!({"purpose": "report", "task": "relatório"}) }
    );
    let output = json!({"status": "accepted", "task_id": "b-t1"});
    s.commands.send(Command::ToolOutput { call_id: "call_1".into(), name: "delegate".into(), output: output.clone() }).await.unwrap();
    let item = recv_json(&mut peer).await;
    assert_eq!(item["type"], "conversation.item.create");
    assert_eq!(item["item"], json!({"type": "function_call_output", "call_id": "call_1", "output": output.to_string()}));
    assert_eq!(next(&mut s.outputs).await, Output::ToolOutputSent { call_id: "call_1".into() });

    send(&mut peer, json!({"type": "response.output_item.done", "response_id": "resp_unknown", "item": {"type": "function_call", "call_id": "call_9", "name": "delegate", "arguments": "{}"}})).await;
    assert!(non_log(&drain(&mut s.outputs, 200).await).iter().all(|o| !matches!(o, Output::ToolCall { .. })));

    assert!(matches!(parse_server(br#"{"type":"response.output_item.done","item":{"type":"message"}}"#).unwrap(), ServerEvent::Ignored(_)));
}

#[tokio::test]
async fn delegation_a_cancel_fenced_responses_call_has_no_tool_effect() {
    use rth_tasks::schema::Tool;
    let (mut s, mut peer, _c) = rig(|c| c.tools = vec![crate::tools::Tool::Task(Tool::Delegate)]).await;
    reply(&mut peer, "resp_1", None).await;
    assert_eq!(next(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    s.commands.send(Command::Cancel { response_id: "v1".into() }).await.unwrap();
    let cancel = recv_json(&mut peer).await;
    assert_eq!(cancel["type"], "response.cancel");
    send(&mut peer, json!({"type": "response.output_item.done", "response_id": "resp_1", "item": {"type": "function_call", "call_id": "call_x", "name": "delegate", "arguments": "{}"}})).await;
    assert!(non_log(&drain(&mut s.outputs, 200).await).iter().all(|o| !matches!(o, Output::ToolCall { .. })), "a committed stop fences the call too");
}

#[tokio::test]
async fn s5r5_the_ready_identity_is_the_identity_of_the_setup_actually_sent() {
    use sha2::Digest;
    let _readiness = crate::persona::READINESS_TEST_LOCK.lock().await;
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    cfg.persona = crate::persona::ORIGIN_V0_1_NAME;
    cfg.tools = vec![crate::tools::Tool::Memory(rth_memory::schema::Tool::Recall)];
    let model = cfg.model.clone();
    let dialect = cfg.dialect.name();
    let mut s = connect(cfg);
    let (mut peer, _) = tokio::time::timeout(Duration::from_secs(5), conns.recv()).await.unwrap().unwrap();
    send(&mut peer, json!({"type": "session.created", "session": {"id": "mock"}})).await;
    let sent = recv_json(&mut peer).await;
    let v = json!({"model": model, "dialect": dialect, "setup": sent});
    let sent_id: String = sha2::Sha256::digest(v.to_string().as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    crate::persona::set_readiness(crate::persona::Readiness { memory: Some(format!("changed after setup {}", std::process::id())), delegation: None });
    send(&mut peer, json!({"type": "session.updated", "session": sent["session"]})).await;
    loop {
        if let Output::Log { msg, .. } = next_any(&mut s.outputs).await
            && msg.starts_with("openai-realtime ready")
        {
            assert!(msg.contains(&format!("setup_id={sent_id}")), "ready must name the sent setup: {msg}");
            break;
        }
    }
}

async fn gated_first_setup(tools: Vec<crate::tools::Tool>, readiness: Option<crate::persona::Readiness>) -> (bool, Value) {
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    cfg.persona = crate::persona::DEFAULT_PERSONA;
    cfg.tools = tools;
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let _s = connect(cfg);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let early = conns.try_recv().ok();
    if let Some(r) = readiness {
        crate::persona::set_readiness(r);
    }
    let _ = open.send(true);
    let was_early = early.is_some();
    let (mut peer, _) = match early {
        Some(p) => p,
        None => tokio::time::timeout(Duration::from_secs(5), conns.recv()).await.expect("the backend dials once the gate opens").unwrap(),
    };
    send(&mut peer, json!({"type": "session.created", "session": {"id": "mock"}})).await;
    (was_early, recv_json(&mut peer).await)
}

fn instructions(setup: &Value) -> String {
    setup["session"]["instructions"].as_str().unwrap_or_default().to_string()
}

const MEMORY_TOOL: crate::tools::Tool = crate::tools::Tool::Memory(rth_memory::schema::Tool::Recall);

#[tokio::test]
async fn dmg_r0_the_first_setup_carries_the_memory_readiness_observed_at_boot() {
    let _readiness = crate::persona::READINESS_TEST_LOCK.lock().await;
    crate::persona::reset_readiness_for_test();
    let attached = format!("attached: brain (binding rev 1, dmg-r0 {})", std::process::id());
    let (early, setup) = gated_first_setup(vec![MEMORY_TOOL], Some(crate::persona::Readiness { memory: Some(attached.clone()), delegation: None })).await;
    let ins = instructions(&setup);
    assert!(!ins.contains("- memory: not observed"), "attached memory is never reported as not observed: {ins}");
    assert!(ins.contains(&format!("- memory: {attached}\n")), "the first setup names the observed memory readiness: {ins}");
    assert!(!early, "no setup may go out before the boot has decided readiness");

    assert!(ins.contains("- delegation: not declared\n"), "{ins}");
}

#[tokio::test]
async fn dmg_r0_controls_unavailable_stays_unavailable_undeclared_stays_undeclared_absent_stays_not_observed() {
    let _readiness = crate::persona::READINESS_TEST_LOCK.lock().await;

    crate::persona::reset_readiness_for_test();
    let why = "unavailable: brain is not attached (attached_unattested: no binding record)".to_string();
    let (_, setup) = gated_first_setup(vec![MEMORY_TOOL], Some(crate::persona::Readiness { memory: Some(why.clone()), delegation: None })).await;
    assert!(instructions(&setup).contains(&format!("- memory: {why}\n")), "{}", instructions(&setup));

    crate::persona::reset_readiness_for_test();
    let (_, setup) = gated_first_setup(Vec::new(), Some(crate::persona::Readiness { memory: Some("attached: brain".into()), delegation: None })).await;
    assert!(instructions(&setup).contains("- memory: not declared\n"), "{}", instructions(&setup));

    crate::persona::reset_readiness_for_test();
    let (_, setup) = gated_first_setup(vec![MEMORY_TOOL, crate::tools::Tool::Task(rth_tasks::schema::Tool::Delegate)], None).await;
    let ins = instructions(&setup);
    assert!(ins.contains("- memory: not observed\n") && ins.contains("- delegation: not observed\n"), "{ins}");
}

#[tokio::test]
async fn dmg_r2_a_cancelled_gate_ends_the_backend_without_setup_or_dial() {
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let mut s = connect(cfg);
    drop(open);
    assert!(tokio::time::timeout(Duration::from_millis(800), conns.recv()).await.is_err(), "a cancelled gate must not dial");
    let ended = tokio::time::timeout(Duration::from_secs(2), async { while s.outputs.recv().await.is_some() {} }).await;
    assert!(ended.is_ok(), "the backend task ends (its output channel closes)");
}

#[tokio::test]
async fn dmg_r2_an_opened_then_dropped_gate_still_dials() {
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let _s = connect(cfg);
    open.send(true).unwrap();
    drop(open);
    let (mut peer, _) = tokio::time::timeout(Duration::from_secs(5), conns.recv()).await.expect("an opened gate dials").unwrap();
    send(&mut peer, json!({"type": "session.created", "session": {"id": "mock"}})).await;
    assert_eq!(recv_json(&mut peer).await["type"], "session.update");
}

fn a2(c: &mut Config) {
    c.vad = "none".into();
    c.text_only = true;
    c.max_output_tokens = Some(1024);
}

#[test]
fn a2r2_text_only_setup_asks_for_text_with_a_cap_and_no_turn_detection() {
    let base = setup_identity(&Config::for_test("ws://x".into()));
    let mut c = Config::for_test("ws://x".into());
    a2(&mut c);
    let v = setup_message(&c);
    assert_eq!(v["session"]["output_modalities"], json!(["text"]));
    assert_eq!(v["session"]["max_output_tokens"], 1024);
    assert_eq!(v["session"]["audio"]["input"]["turn_detection"], Value::Null);
    assert_ne!(setup_identity(&c), base, "the limits are part of the setup identity");
    assert!(c.knobs().ends_with(" text_only=1 max_output_tokens=1024"), "{}", c.knobs());

    let d = Config::for_test("ws://x".into());
    assert!(setup_message(&d)["session"].get("max_output_tokens").is_none());
    assert!(!d.knobs().contains("text_only") && !d.knobs().contains("max_output_tokens"));
    assert_eq!(response_create_for(&d, "e", "q", 1), response_create("e", "q", 1));
}

#[test]
fn a2r2_the_knobs_parse_strictly() {
    let with = |pairs: &[(&str, &str)]| {
        let m: std::collections::HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(Dialect::OpenAI, &move |k: &str| m.get(k).cloned())
    };
    let url = ("RTH_OPENAI_URL", "ws://127.0.0.1:1/v1/realtime");
    let ok = with(&[url, ("RTH_OPENAI_TEXT_ONLY", "1"), ("RTH_OPENAI_VAD", "none"), ("RTH_OPENAI_MAX_OUTPUT_TOKENS", "1024"), ("RTH_OPENAI_USAGE_LOG", "/x/usage.jsonl")]).unwrap();
    assert!(ok.text_only && ok.max_output_tokens == Some(1024) && ok.usage_log.as_deref() == Some(std::path::Path::new("/x/usage.jsonl")));
    assert!(with(&[url, ("RTH_OPENAI_TEXT_ONLY", "1")]).err().expect("refused").to_string().contains("RTH_OPENAI_VAD=none"), "text-only needs manual turns");
    for bad in ["0", "4097", "-1", "lots"] {
        assert!(with(&[url, ("RTH_OPENAI_MAX_OUTPUT_TOKENS", bad)]).is_err(), "{bad}");
    }
    assert!(with(&[url, ("RTH_OPENAI_TEXT_ONLY", "yes")]).is_err());
    let none = with(&[url]).unwrap();
    assert!(!none.text_only && none.max_output_tokens.is_none() && none.usage_log.is_none());
}

#[tokio::test]
async fn a2r2_every_response_create_carries_the_limits_and_respond_commits_no_audio() {
    let (s, mut peer, _c) = rig(a2).await;
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let create = expect_request(&mut peer, "oi").await;
    assert_eq!(create["response"]["output_modalities"], json!(["text"]));
    assert_eq!(create["response"]["max_output_tokens"], 1024);
    send(&mut peer, created("resp_1", meta("q1"))).await;
    send(&mut peer, done("resp_1", "completed", None)).await;

    s.commands.send(Command::Respond { request_id: "q2".into(), response_id: "r2".into(), decision_id: "d2".into(), utterance_id: None }).await.unwrap();
    let ack = recv_json(&mut peer).await;
    assert_eq!(ack["type"], "response.create", "text-only never commits audio");
    assert_eq!(ack["response"]["max_output_tokens"], 1024);
    assert_eq!(ack["response"]["output_modalities"], json!(["text"]));
}

#[tokio::test]
async fn a2r2_text_only_refuses_input_audio() {
    let (s, mut peer, _c) = rig(a2).await;
    s.commands.send(Command::Audio { sample_rate: 24_000, pcm: vec![1, 0, 2, 0] }).await.unwrap();
    silent(&mut peer, 300).await;
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    expect_request(&mut peer, "oi").await;
}

#[tokio::test]
async fn a2r2_each_response_done_writes_its_complete_usage_bound_to_our_ids() {
    let dir = std::env::temp_dir().join(format!("rth-a2r2-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("usage.jsonl");
    let p2 = path.clone();
    let (mut s, mut peer, _c) = rig(move |c| {
        a2(c);
        c.usage_log = Some(p2);
    })
    .await;

    let usage = json!({"total_tokens": 1534, "input_tokens": 1500, "output_tokens": 34,
        "input_token_details": {"text_tokens": 1500, "audio_tokens": 0, "image_tokens": 0, "cached_tokens": 1024, "cached_tokens_details": {"text_tokens": 1024, "audio_tokens": 0, "image_tokens": 0}},
        "output_token_details": {"text_tokens": 34, "audio_tokens": 0}, "note": "x".repeat(400)});
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    expect_request(&mut peer, "oi").await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    let mut d = done("resp_1", "completed", None);
    d["response"]["usage"] = usage.clone();
    send(&mut peer, d).await;

    s.commands.send(text("q2", "r2", "de novo")).await.unwrap();
    expect_request(&mut peer, "de novo").await;
    send(&mut peer, created("resp_2", meta("q2"))).await;
    send(&mut peer, done("resp_2", "completed", None)).await;

    send(&mut peer, done("resp_ghost", "completed", None)).await;
    drain(&mut s.outputs, 300).await;
    let rows: Vec<Value> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(rows[0]["usage"], usage, "the complete usage, not a clipped line");
    assert_eq!((rows[0]["complete"].clone(), rows[0]["rth_response_id"].clone(), rows[0]["provider_response_id"].clone(), rows[0]["binding"].clone()), (json!(true), json!("r1"), json!("resp_1"), json!("live")));
    assert_eq!(rows[0]["generation"], 1);
    assert_eq!((rows[1]["complete"].clone(), rows[1]["usage"].clone(), rows[1]["rth_response_id"].clone()), (json!(false), Value::Null, json!("r2")));
    assert_eq!((rows[2]["binding"].clone(), rows[2]["rth_response_id"].clone()), (json!("unbound"), Value::Null));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a2r2_usage_is_complete_only_with_every_priced_field() {
    let full = json!({"input_tokens": 1, "output_tokens": 1, "input_token_details": {"text_tokens": 1, "audio_tokens": 0, "cached_tokens": 0}, "output_token_details": {"text_tokens": 1, "audio_tokens": 0}});
    assert!(usage_complete(&full));
    for p in ["input_tokens", "output_tokens"] {
        let mut u = full.clone();
        u.as_object_mut().unwrap().remove(p);
        assert!(!usage_complete(&u), "{p}");
    }
    let mut u = full.clone();
    u["output_token_details"].as_object_mut().unwrap().remove("audio_tokens");
    assert!(!usage_complete(&u));
    let mut u = full;
    u["input_tokens"] = json!("1");
    assert!(!usage_complete(&u), "a string is not a count");
}

fn item_text_delta(rid: &str, item: &str, t: &str) -> Value {
    json!({"type": "response.output_audio_transcript.delta", "response_id": rid, "item_id": item, "content_index": 0, "delta": t})
}

#[tokio::test]
async fn conversation_a_second_output_item_is_separated_from_the_first() {
    let (mut s, mut peer, _c) = rig(|_| {}).await;
    reply(&mut peer, "resp_1", None).await;
    let _ = next(&mut s.outputs).await;
    for (item, t) in [("item_a", "Beleza, "), ("item_a", "já explico."), ("item_b", "Não lembro"), ("item_b", " disso."), ("item_c", " Certo.")] {
        send(&mut peer, item_text_delta("resp_1", item, t)).await;
    }
    send(&mut peer, done("resp_1", "completed", None)).await;
    let mut text = String::new();
    for o in drain(&mut s.outputs, 300).await {
        if let Output::Delta { response_id, text: t } = o {
            assert_eq!(response_id, "v1");
            text.push_str(&t);
        }
    }
    assert_eq!(text, "Beleza, já explico. Não lembro disso. Certo.");
}

#[test]
fn conversation_the_default_setup_is_persona_v2_1_and_pinned() {
    let cfg = Config::for_test("ws://x".into());
    let v = setup_message(&cfg);
    let instr = v["session"]["instructions"].as_str().unwrap();
    assert!(instr.starts_with("You are RTH, the agent of this RTH instance.") && instr.contains("Declared operations: none\n") && instr.contains("- memory: not declared\n- delegation: not declared"), "{instr}");
    assert!(cfg.knobs().ends_with("persona=rth-persona-v2.1"), "{}", cfg.knobs());
    assert_eq!(setup_identity(&cfg), "ec8ced40d7986add");
}

fn a2_spend(c: &mut Config) -> PathBuf {
    a2(c);
    let mut l = serde_json::json!({ "dir": "realtime", "cap_pusd": 4_300_000_000_000u64, "max_attempts": 12, "attempt_bound_pusd": 536_600_000_000u64, "model": "gpt-realtime-2.1", "context_window": 128_000,
        "options": { "text_only": true, "vad": "none", "max_output_tokens": 1024 },
        "prices": { "text_in": 4_000_000, "text_cached": 400_000, "text_out": 24_000_000, "audio_in": 32_000_000, "audio_cached": 400_000, "audio_out": 64_000_000, "source": "test fixture", "read": "2026-10-23" } });
    l["model"] = serde_json::json!(c.model.clone());
    let (control, sha) = crate::spend::tests::init(l);
    let e = crate::spend::Expect { model: c.model.clone(), text_only: c.text_only, vad: c.vad.clone(), max_output_tokens: c.max_output_tokens };
    c.spend = Some(crate::spend::Spend::open(&control, &sha, &e).unwrap());
    control.parent().unwrap().to_path_buf()
}

fn ledger(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir.join("realtime")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).filter(|n| n != "open.json").collect();
    v.sort();
    v
}

#[tokio::test]
async fn a2_spend_every_response_create_follows_its_claimed_attempt_and_settles_from_its_usage() {
    let dir = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let d2 = dir.clone();
    let (mut s, mut peer, _c) = rig(move |c| *d2.lock().unwrap() = a2_spend(c)).await;
    let dir = dir.lock().unwrap().clone();
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    let create = expect_request(&mut peer, "oi").await;
    assert_eq!(create["response"]["metadata"]["rth_attempt"], "v:1:q1");
    assert_eq!(ledger(&dir), vec!["attempt-000.json"], "claimed before the create was sent");
    send(&mut peer, created("resp_1", meta("q1"))).await;
    let mut d = done("resp_1", "completed", None);
    d["response"]["usage"] = crate::spend::tests::usage(1500, 34);
    send(&mut peer, d).await;
    while !matches!(next(&mut s.outputs).await, Output::Done { .. }) {}

    s.commands.send(Command::Respond { request_id: "q2".into(), response_id: "r2".into(), decision_id: "d2".into(), utterance_id: None }).await.unwrap();
    let ack = recv_json(&mut peer).await;
    assert_eq!(ack["type"], "response.create");
    assert_eq!(ack["response"]["metadata"]["rth_attempt"], "v:1:q2");
    assert_eq!(ledger(&dir), vec!["attempt-000.json", "attempt-000.settle.json", "attempt-001.json"]);
    let settle: Value = serde_json::from_slice(&std::fs::read(dir.join("realtime/attempt-000.settle.json")).unwrap()).unwrap();
    assert_eq!(settle["cost_pusd"], 1500 * 4_000_000u64 + 34 * 24_000_000u64);
    assert_eq!(settle["provider_response_id"], "resp_1");
}

#[tokio::test]
async fn a2_spend_a_stopped_acceptance_sends_nothing_and_fails_the_request_locally() {
    let dir = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let d2 = dir.clone();
    let (mut s, mut peer, _c) = rig(move |c| *d2.lock().unwrap() = a2_spend(c)).await;
    let dir = dir.lock().unwrap().clone();
    std::fs::write(dir.join("stop.json"), "{\"layer\":\"worker\"}").unwrap();
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    silent(&mut peer, 300).await;
    let outs = drain(&mut s.outputs, 200).await;
    assert!(outs.iter().any(|o| matches!(o, Output::Done { response_id, status: ResponseStatus::Failed } if response_id == "r1")), "{outs:?}");
    assert!(ledger(&dir).is_empty(), "no attempt was claimed");
}

#[tokio::test]
async fn a2_spend_missing_usage_and_an_unbound_response_stop_the_acceptance() {
    let dir = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let d2 = dir.clone();
    let (s, mut peer, _c) = rig(move |c| *d2.lock().unwrap() = a2_spend(c)).await;
    let dir = dir.lock().unwrap().clone();
    s.commands.send(text("q1", "r1", "oi")).await.unwrap();
    expect_request(&mut peer, "oi").await;
    send(&mut peer, created("resp_1", meta("q1"))).await;
    send(&mut peer, done("resp_1", "completed", None)).await;
    send(&mut peer, done("resp_ghost", "completed", None)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ledger(&dir), vec!["anomaly-resp_ghost.json", "attempt-000.json"], "the reservation is kept; the anomaly carries the full bound");
    assert!(dir.join("stop.json").exists());
    s.commands.send(text("q2", "r2", "de novo")).await.unwrap();
    silent(&mut peer, 300).await;
}

#[test]
fn a2_spend_the_knobs_come_together_and_need_text_only() {
    let with = |pairs: &[(&str, &str)]| {
        let m: std::collections::HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Config::from_lookup(Dialect::OpenAI, &move |k: &str| m.get(k).cloned())
    };
    let url = ("RTH_OPENAI_URL", "ws://127.0.0.1:1/v1/realtime");
    assert!(with(&[url, ("RTH_SPEND_CONTROL", "/x/control.json")]).err().unwrap().to_string().contains("together"));
    assert!(with(&[url, ("RTH_SPEND_CONTROL", "/x/control.json"), ("RTH_SPEND_CONTROL_SHA256", "ab")]).err().unwrap().to_string().contains("TEXT_ONLY"));
    let e = with(&[url, ("RTH_SPEND_CONTROL", "/no/such/control.json"), ("RTH_SPEND_CONTROL_SHA256", "ab"), ("RTH_OPENAI_TEXT_ONLY", "1"), ("RTH_OPENAI_VAD", "none"), ("RTH_OPENAI_MAX_OUTPUT_TOKENS", "1024")]).err().unwrap().to_string();
    assert!(e.contains("control record unreadable"), "{e}");
}

#[tokio::test]
async fn a2_spend_an_identical_completion_replay_is_one_charge_and_a_changed_one_stops() {
    for conflicting in [false, true] {
        let dir = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
        let d2 = dir.clone();
        let (mut s, mut peer, _c) = rig(move |c| *d2.lock().unwrap() = a2_spend(c)).await;
        let dir = dir.lock().unwrap().clone();
        s.commands.send(text("q1", "r1", "oi")).await.unwrap();
        expect_request(&mut peer, "oi").await;
        send(&mut peer, created("resp_1", meta("q1"))).await;
        let mut d = done("resp_1", "completed", None);
        d["response"]["usage"] = crate::spend::tests::usage(1500, 34);
        send(&mut peer, d.clone()).await;
        while !matches!(next(&mut s.outputs).await, Output::Done { .. }) {}
        if conflicting {
            d["response"]["usage"] = crate::spend::tests::usage(1500, 35);
        }
        send(&mut peer, d).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(ledger(&dir), vec!["attempt-000.json", "attempt-000.settle.json"], "conflicting={conflicting}: one charge, no anomaly");
        assert_eq!(dir.join("stop.json").exists(), conflicting);
        s.commands.send(text("q2", "r2", "de novo")).await.unwrap();
        if conflicting {
            silent(&mut peer, 300).await;
        } else {
            expect_request(&mut peer, "de novo").await;
            assert_eq!(ledger(&dir), vec!["attempt-000.json", "attempt-000.settle.json", "attempt-001.json"]);
        }
    }
}
