

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

pub(crate) type Peer = WebSocketStream<TcpStream>;

pub(crate) async fn mock() -> (String, mpsc::Receiver<(Peer, Option<String>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/ws", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let mut key = None;
            let ws = accept_hdr_async(stream, |req: &Request, resp: Response| {
                key = req.headers().get("x-goog-api-key").map(|v| v.to_str().unwrap().to_string());
                Ok(resp)
            })
            .await
            .unwrap();
            if tx.send((ws, key)).await.is_err() {
                return;
            }
        }
    });
    (url, rx)
}

pub(crate) async fn recv_json(peer: &mut Peer) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), peer.next()).await.expect("client message").unwrap().unwrap();
        match msg {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            Message::Binary(b) => return serde_json::from_slice(&b).unwrap(),
            _ => continue,
        }
    }
}

pub(crate) async fn send(peer: &mut Peer, v: Value) {
    peer.send(Message::binary(v.to_string().into_bytes())).await.unwrap();
}

pub(crate) fn audio(bytes: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"mimeType": "audio/pcm;rate=24000", "data": data}}]}}})
}

fn transcript(text: &str) -> Value {
    json!({"serverContent": {"outputTranscription": {"text": text}}})
}

pub(crate) fn turn_complete() -> Value {
    json!({"serverContent": {"turnComplete": true}})
}

pub(crate) async fn accept_setup(conns: &mut mpsc::Receiver<(Peer, Option<String>)>) -> (Peer, Value, Option<String>) {
    let (mut peer, key) = tokio::time::timeout(Duration::from_secs(5), conns.recv()).await.unwrap().unwrap();
    let setup = recv_json(&mut peer).await;
    send(&mut peer, json!({"setupComplete": {}})).await;
    (peer, setup, key)
}

async fn next(outputs: &mut mpsc::Receiver<Output>) -> Output {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.expect("backend output").unwrap() {
            Output::Log { .. } | Output::Signal { .. } => continue,
            other => return other,
        }
    }
}

fn text(q: &str, r: &str, t: &str) -> Command {
    Command::UserText { opener: None, request_id: q.into(), response_id: r.into(), text: t.into(), context: vec![], memory: None, owner: true, people: None, reply_voice: None, level: Default::default() }
}

#[tokio::test]
async fn setup_contract_and_one_spoken_turn() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, setup, key) = accept_setup(&mut conns).await;

    assert_eq!(key.as_deref(), Some("test-placeholder"), "key travels in the x-goog-api-key header");
    let su = &setup["setup"];
    assert_eq!(su["model"], "models/gemini-3.8-live");
    assert_eq!(su["generationConfig"]["responseModalities"], json!(["AUDIO"]));
    assert_eq!(su["outputAudioTranscription"], json!({}));
    assert_eq!(su["inputAudioTranscription"], json!({}));
    assert_eq!(su["sessionResumption"], json!({}));
    let raw = setup.to_string();

    for removed in ["proactiv", "affective", "Affective", "thinking", "speechConfig", "languageCode"] {
        assert!(!raw.contains(removed), "setup carries {removed}: {raw}");
    }

    assert!(su.get("realtimeInputConfig").is_none(), "{su}");
    let system = su["systemInstruction"]["parts"][0]["text"].as_str().unwrap();
    assert!(system.starts_with("[rth-voice-policy ptbr-addressee-v1]"), "{system}");
    assert!(system.contains("Brazilian Portuguese (pt-BR)") && system.contains("stay silent"));

    s.commands.send(text("q1", "r1", "hello")).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
    let turn = recv_json(&mut peer).await;
    assert_eq!(turn, json!({"clientContent": {"turns": [{"role": "user", "parts": [{"text": "hello"}]}], "turnComplete": true}}));

    send(&mut peer, audio(&[1, 0, 2, 0])).await;
    send(&mut peer, transcript("Hi there.")).await;
    send(&mut peer, json!({"serverContent": {"generationComplete": true}})).await;
    send(&mut peer, turn_complete()).await;

    assert_eq!(next(&mut s.outputs).await, Output::Audio { response_id: "r1".into(), sample_rate: 24_000, pcm: vec![1, 0, 2, 0] });
    assert_eq!(next(&mut s.outputs).await, Output::Delta { response_id: "r1".into(), text: "Hi there.".into() });
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn cancel_ends_at_once_and_drops_the_server_tail() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;

    s.commands.send(text("q1", "r1", "count to fifty")).await.unwrap();
    let _ = next(&mut s.outputs).await;
    let _ = recv_json(&mut peer).await;
    send(&mut peer, audio(&[9, 9])).await;
    assert!(matches!(next(&mut s.outputs).await, Output::Audio { .. }));

    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Cancelled });

    for _ in 0..3 {
        send(&mut peer, audio(&[7, 7])).await;
    }
    send(&mut peer, transcript("four, five")).await;
    send(&mut peer, turn_complete()).await;

    s.commands.send(text("q2", "r2", "hi")).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r2".into(), request_id: "q2".into() });
    let _ = recv_json(&mut peer).await;
    send(&mut peer, audio(&[5, 5])).await;
    send(&mut peer, turn_complete()).await;
    assert_eq!(next(&mut s.outputs).await, Output::Audio { response_id: "r2".into(), sample_rate: 24_000, pcm: vec![5, 5] });
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r2".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn cancel_tail_is_logged_with_drop_counts() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    s.commands.send(text("q1", "r1", "long")).await.unwrap();
    let _ = recv_json(&mut peer).await;
    s.commands.send(Command::Cancel { response_id: "r1".into() }).await.unwrap();
    send(&mut peer, audio(&[1, 1])).await;
    send(&mut peer, audio(&[1, 1])).await;
    send(&mut peer, turn_complete()).await;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), s.outputs.recv()).await.unwrap().unwrap() {
            Output::Audio { .. } | Output::Delta { .. } => panic!("output after cancel"),
            Output::Log { msg, .. } if msg.contains("gemini cancelled response r1") => {
                assert!(msg.contains("dropped 2 audio parts, 0 transcript parts"), "{msg}");
                break;
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_new_text_interrupts_the_open_turn() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;

    s.commands.send(text("q1", "r1", "tell me a story")).await.unwrap();
    let _ = next(&mut s.outputs).await;
    let _ = recv_json(&mut peer).await;
    send(&mut peer, audio(&[1, 1])).await;
    let _ = next(&mut s.outputs).await;

    s.commands.send(text("q2", "r2", "stop, what time is it")).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r2".into(), request_id: "q2".into() });
    let _ = recv_json(&mut peer).await;
    send(&mut peer, json!({"serverContent": {"interrupted": true}})).await;

    send(&mut peer, turn_complete()).await;
    send(&mut peer, audio(&[2, 2])).await;
    send(&mut peer, turn_complete()).await;

    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Cancelled });
    assert_eq!(next(&mut s.outputs).await, Output::Audio { response_id: "r2".into(), sample_rate: 24_000, pcm: vec![2, 2] });
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r2".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn go_away_reconnects_with_the_resumption_handle() {
    let (url, mut conns) = mock().await;
    let s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, json!({"sessionResumptionUpdate": {"newHandle": "h-1", "resumable": true}})).await;
    send(&mut peer, json!({"goAway": {"timeLeft": "10s"}})).await;

    let (_peer2, setup2, _) = accept_setup(&mut conns).await;
    assert_eq!(setup2["setup"]["sessionResumption"], json!({"handle": "h-1"}));
    drop(s);
}

#[tokio::test]
async fn texts_sent_before_setup_wait_for_it() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    s.commands.send(text("q1", "r1", "early")).await.unwrap();
    assert_eq!(next(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    let turn = recv_json(&mut peer).await;
    assert_eq!(turn["clientContent"]["turns"][0]["parts"][0]["text"], "early");
    send(&mut peer, turn_complete()).await;
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn a_dropped_socket_fails_the_open_turn_and_reconnects() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    s.commands.send(text("q1", "r1", "hi")).await.unwrap();
    let _ = next(&mut s.outputs).await;
    let _ = recv_json(&mut peer).await;
    drop(peer);
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Failed });
    let (_peer2, setup2, _) = accept_setup(&mut conns).await;
    assert_eq!(setup2["setup"]["sessionResumption"], json!({}), "no handle was ever issued");
}

#[test]
fn parse_reads_rate_handles_and_ignores_unknown_keys() {
    let pcm = base64::engine::general_purpose::STANDARD.encode([0u8, 1, 2, 3]);
    let raw = json!({"serverContent": {"modelTurn": {"parts": [{"inlineData": {"mimeType": "audio/pcm;rate=16000", "data": pcm}}]}}, "usageMetadata": {"totalTokenCount": 77}});
    let m = parse_server(raw.to_string().as_bytes()).unwrap();
    assert_eq!(m.audio, vec![(16_000, vec![0, 1, 2, 3])]);
    assert_eq!(m.total_tokens, Some(77));

    let m = parse_server(br#"{"sessionResumptionUpdate":{"newHandle":"x","resumable":false},"toolCall":{}}"#).unwrap();
    assert_eq!(m.resume_handle, None, "a non-resumable handle is never kept");
    assert_eq!(m.other, vec!["toolCall".to_string()]);
    assert!(parse_server(b"not json").is_err());
}

async fn ready(outputs: &mut mpsc::Receiver<Output>) {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.expect("ready line").unwrap() {
            Output::Log { msg, .. } if msg.starts_with("gemini ready") => return,
            _ => {}
        }
    }
}

async fn next_any(outputs: &mut mpsc::Receiver<Output>) -> Output {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), outputs.recv()).await.expect("backend output").unwrap() {
            Output::Log { .. } => continue,
            other => return other,
        }
    }
}

#[tokio::test]
async fn mic_audio_becomes_realtime_input_and_a_server_turn_opens_a_response() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    ready(&mut s.outputs).await;

    let pcm: Vec<u8> = (0..640u16).flat_map(|i| i.to_le_bytes()).collect();
    s.commands.send(Command::Audio { sample_rate: 16_000, pcm: pcm.clone() }).await.unwrap();
    let sent = recv_json(&mut peer).await;
    let data = base64::engine::general_purpose::STANDARD.encode(&pcm);
    assert_eq!(sent, json!({"realtimeInput": {"audio": {"data": data, "mimeType": "audio/pcm;rate=16000"}}}));
    s.commands.send(Command::AudioEnd { stream_id: "mic-1".into() }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await, json!({"realtimeInput": {"audioStreamEnd": true}}));

    send(&mut peer, json!({"serverContent": {"inputTranscription": {"text": "what time is it"}}})).await;
    send(&mut peer, audio(&[3, 0, 4, 0])).await;
    send(&mut peer, turn_complete()).await;
    assert_eq!(
        next_any(&mut s.outputs).await,
        Output::InputTranscript {
            text: "what time is it".into(),
            heard: Heard { utterance_id: Some("u1".into()), seq: Some(0), finished: None, response_id: None, association: Some("unknown"), boundary: None }
        }
    );
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    assert_eq!(next_any(&mut s.outputs).await, Output::Audio { response_id: "v1".into(), sample_rate: 24_000, pcm: vec![3, 0, 4, 0] });
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "turn_complete" });
    assert_eq!(next_any(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn barge_in_interrupt_is_signalled_then_the_next_server_turn_is_new() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, audio(&[1, 1])).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    let _ = next_any(&mut s.outputs).await;

    send(&mut peer, json!({"serverContent": {"interrupted": true}})).await;
    send(&mut peer, turn_complete()).await;
    send(&mut peer, audio(&[2, 2])).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "interrupted" });
    assert_eq!(next_any(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Cancelled });
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: None, signal: "turn_complete" });
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v2".into(), utterance_id: None });
    assert_eq!(next_any(&mut s.outputs).await, Output::Audio { response_id: "v2".into(), sample_rate: 24_000, pcm: vec![2, 2] });
}

#[tokio::test]
async fn a_composite_interrupt_end_does_not_swallow_the_next_silent_turn() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    s.commands.send(text("q1", "r1", "talk")).await.unwrap();
    let _ = next(&mut s.outputs).await;
    let _ = recv_json(&mut peer).await;
    s.commands.send(text("q2", "r2", "never mind")).await.unwrap();
    let _ = next(&mut s.outputs).await;
    let _ = recv_json(&mut peer).await;
    send(&mut peer, json!({"serverContent": {"interrupted": true, "turnComplete": true}})).await;
    send(&mut peer, turn_complete()).await;
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Cancelled });
    assert_eq!(next(&mut s.outputs).await, Output::Done { response_id: "r2".into(), status: ResponseStatus::Completed });
}

#[tokio::test]
async fn mic_audio_with_no_live_session_is_dropped_and_counted() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    for _ in 0..3 {
        s.commands.send(Command::Audio { sample_rate: 16_000, pcm: vec![0; 64] }).await.unwrap();
    }
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), s.outputs.recv()).await.unwrap().unwrap() {
            Output::Log { msg, .. } if msg.contains("dropped 3 microphone chunks") => break,
            _ => {}
        }
    }

    s.commands.send(Command::Audio { sample_rate: 16_000, pcm: vec![9, 0] }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await["realtimeInput"]["audio"]["data"], "CQA=");
}

fn heard_part(text: &str, finished: Option<bool>) -> Value {
    let mut t = json!({"text": text});
    if let Some(f) = finished {
        t["finished"] = json!(f);
    }
    json!({"serverContent": {"inputTranscription": t}})
}

fn part(text: &str, uid: &str, seq: u64, finished: Option<bool>) -> Output {
    Output::InputTranscript {
        text: text.into(),
        heard: Heard { utterance_id: Some(uid.into()), seq: Some(seq), finished, response_id: None, association: Some("unknown"), boundary: None },
    }
}

#[tokio::test]
async fn utterances_are_split_only_by_the_providers_finished_flag() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, heard_part("what ", None)).await;
    send(&mut peer, heard_part("time is it", Some(true))).await;
    send(&mut peer, heard_part("and the date", Some(true))).await;
    assert_eq!(next_any(&mut s.outputs).await, part("what ", "u1", 0, None));
    assert_eq!(next_any(&mut s.outputs).await, part("time is it", "u1", 1, Some(true)));
    assert_eq!(next_any(&mut s.outputs).await, part("and the date", "u2", 0, Some(true)));
}

fn part_at(text: &str, uid: &str, seq: u64, finished: Option<bool>, boundary: Option<&'static str>) -> Output {
    Output::InputTranscript {
        text: text.into(),
        heard: Heard { utterance_id: Some(uid.into()), seq: Some(seq), finished, response_id: None, association: Some("unknown"), boundary },
    }
}

fn activity(ty: &str) -> Value {
    json!({"voiceActivity": {"type": ty}})
}

#[tokio::test]
async fn a_delayed_transcript_between_two_replies_is_never_marked_as_the_next_ones_prompt() {
    for with_activity in [true, false] {
        let (url, mut conns) = mock().await;
        let mut s = connect(Config::for_test(url));
        let (mut peer, _, _) = accept_setup(&mut conns).await;
        if with_activity {
            send(&mut peer, activity("ACTIVITY_START")).await;
            assert!(matches!(next_any(&mut s.outputs).await, Output::Signal { signal: "voice_activity_start", .. }));
        }
        send(&mut peer, heard_part("hello ", None)).await;
        send(&mut peer, audio(&[1, 1])).await;
        send(&mut peer, turn_complete()).await;
        send(&mut peer, heard_part("there", Some(true))).await;
        send(&mut peer, audio(&[2, 2])).await;
        assert_eq!(next_any(&mut s.outputs).await, part("hello ", "u1", 0, None));
        assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
        let _ = next_any(&mut s.outputs).await;
        let _ = next_any(&mut s.outputs).await;
        let _ = next_any(&mut s.outputs).await;
        let tail = if with_activity { part("there", "u1", 1, Some(true)) } else { part_at("there", "u2", 0, Some(true), Some("provider_turn")) };
        assert_eq!(next_any(&mut s.outputs).await, tail, "still unassociated (activity={with_activity})");
        assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v2".into(), utterance_id: None }, "B carries no guessed utterance");
    }
}

#[tokio::test]
async fn barge_in_speech_is_unassociated_and_its_own_interrupt_does_not_split_it() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, heard_part("count to ten", Some(true))).await;
    send(&mut peer, audio(&[1, 1])).await;
    send(&mut peer, activity("ACTIVITY_START")).await;
    send(&mut peer, heard_part("stop ", None)).await;
    send(&mut peer, json!({"serverContent": {"interrupted": true}})).await;
    send(&mut peer, heard_part("what is two plus two", Some(true))).await;
    send(&mut peer, audio(&[2, 2])).await;
    assert_eq!(next_any(&mut s.outputs).await, part("count to ten", "u1", 0, Some(true)));
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    let _ = next_any(&mut s.outputs).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "voice_activity_start" });
    assert_eq!(next_any(&mut s.outputs).await, part("stop ", "u2", 0, None));
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "interrupted" });
    assert_eq!(next_any(&mut s.outputs).await, Output::Done { response_id: "v1".into(), status: ResponseStatus::Cancelled });
    assert_eq!(next_any(&mut s.outputs).await, part("what is two plus two", "u2", 1, Some(true)));
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v2".into(), utterance_id: None });
}

#[tokio::test]
async fn several_turns_without_finished_get_one_utterance_each() {
    for with_activity in [true, false] {
        let (url, mut conns) = mock().await;
        let mut s = connect(Config::for_test(url));
        let (mut peer, _, _) = accept_setup(&mut conns).await;
        let mut parts = Vec::new();
        for (turn, words) in [["oi ", "tudo bem"], ["que horas ", "são"], ["obrigado", " tchau"]].iter().enumerate() {
            if with_activity {
                send(&mut peer, activity("ACTIVITY_START")).await;
            }
            for w in words {
                send(&mut peer, heard_part(w, None)).await;
            }
            if with_activity {
                send(&mut peer, activity("ACTIVITY_END")).await;
            }
            send(&mut peer, audio(&[turn as u8 + 1, 0])).await;
            send(&mut peer, turn_complete()).await;
            loop {
                match next_any(&mut s.outputs).await {
                    Output::InputTranscript { text, heard } => parts.push((text, heard.utterance_id.unwrap(), heard.seq.unwrap(), heard.boundary, heard.response_id)),
                    Output::Done { .. } => break,
                    _ => {}
                }
            }
        }
        type Part<'a> = (String, String, u64, Option<&'a str>, Option<String>);
        let expect: Vec<Part> = vec![
            ("oi ".into(), "u1".into(), 0, None, None),
            ("tudo bem".into(), "u1".into(), 1, None, None),
            ("que horas ".into(), "u2".into(), 0, Some("provider_turn"), None),
            ("são".into(), "u2".into(), 1, None, None),
            ("obrigado".into(), "u3".into(), 0, Some("provider_turn"), None),
            (" tchau".into(), "u3".into(), 1, None, None),
        ];
        assert_eq!(parts, expect, "activity={with_activity}");
    }
}

#[tokio::test]
async fn typed_and_spoken_input_interleave_without_links_and_a_missing_transcript_is_unknown() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    s.commands.send(text("q1", "r1", "typed question")).await.unwrap();
    assert_eq!(next_any(&mut s.outputs).await, Output::Started { response_id: "r1".into(), request_id: "q1".into() });
    let _ = recv_json(&mut peer).await;
    send(&mut peer, heard_part("spoken words", Some(true))).await;
    send(&mut peer, audio(&[5, 5])).await;
    send(&mut peer, turn_complete()).await;
    assert_eq!(next_any(&mut s.outputs).await, part("spoken words", "u1", 0, Some(true)));
    assert_eq!(next_any(&mut s.outputs).await, Output::Audio { response_id: "r1".into(), sample_rate: 24_000, pcm: vec![5, 5] });
    let _ = next_any(&mut s.outputs).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Done { response_id: "r1".into(), status: ResponseStatus::Completed });

    send(&mut peer, audio(&[6, 6])).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
}

#[tokio::test]
async fn restored_turns_are_sent_first_as_history_only() {
    use crate::context::Turn;
    let t = |role: &str, text: &str, kind: &str, status: Option<&str>, finished: Option<bool>| Turn {
        role: role.into(),
        text: text.into(),
        kind: kind.into(),
        event: "e".into(),
        response_id: None,
        status: status.map(str::to_string),
        finished,
        boundary: None,
    };
    let turns = vec![
        t("user", "remember that the code word is heron", "typed", None, None),
        t("model", "OK, I'll remember that.", "reply", Some("completed"), None),
        t("model", "One, two, thr", "reply", Some("cancelled"), None),
        t("user", "half a sent", "speech", None, Some(false)),
    ];
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    s.commands.send(Command::Restore { turns }).await.unwrap();
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    let first = recv_json(&mut peer).await;
    assert_eq!(
        first,
        json!({"clientContent": {"turns": [
            {"role": "user", "parts": [{"text": "remember that the code word is heron"}]},
            {"role": "model", "parts": [{"text": "OK, I'll remember that."}]}
        ], "turnComplete": false}})
    );
    match next_any(&mut s.outputs).await {
        Output::ContextInjected { turns, omitted, truncated, .. } => {
            assert_eq!((turns, omitted, truncated), (2, 2, 0));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_restore_after_setup_is_sent_when_the_session_is_idle() {
    use crate::context::Turn;
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    ready(&mut s.outputs).await;
    let turn = Turn { role: "user".into(), text: "remember heron".into(), kind: "typed".into(), event: "e".into(), response_id: None, status: None, finished: None, boundary: None };
    s.commands.send(Command::Restore { turns: vec![turn] }).await.unwrap();
    let first = recv_json(&mut peer).await;
    assert_eq!(first["clientContent"]["turnComplete"], json!(false));
    assert!(matches!(next_any(&mut s.outputs).await, Output::ContextInjected { turns: 1, .. }));
}

#[test]
fn the_turn_taking_knobs_toggle_separately() {

    let mut base = Config::for_test("ws://x".into());
    base.persona = crate::persona::PERSONA_V2_NAME;
    let id = setup_identity(&base);
    let mut off = Config::for_test("ws://x".into());
    off.persona = crate::persona::PERSONA_V2_NAME;
    off.policy = "none".into();
    let v = setup_message(&off, None);
    assert!(v["setup"].get("realtimeInputConfig").is_none(), "{v}");
    assert_eq!(v["setup"]["systemInstruction"]["parts"][0]["text"], crate::persona::PERSONA_V2, "the persona stays without the policy");
    assert_ne!(setup_identity(&off), id);
    off.persona = "none";
    assert!(setup_message(&off, None)["setup"].get("systemInstruction").is_none());
    let t = setup_message(&base, None)["setup"]["systemInstruction"]["parts"][0]["text"].as_str().unwrap().to_string();
    assert_eq!(t, format!("{POLICY_PTBR_ADDRESSEE_V1}\n\n{}", crate::persona::PERSONA_V2), "policy, then the persona");
    let mut low = Config::for_test("ws://x".into());
    low.vad = "low-v1".into();
    assert_eq!(
        setup_message(&low, None)["setup"]["realtimeInputConfig"],
        json!({"automaticActivityDetection": {"disabled": false, "startOfSpeechSensitivity": "START_SENSITIVITY_LOW", "prefixPaddingMs": 300, "silenceDurationMs": 900}})
    );
    low.vad = "silence-v1".into();
    assert_eq!(setup_message(&low, None)["setup"]["realtimeInputConfig"], json!({"automaticActivityDetection": {"disabled": false, "silenceDurationMs": 900}}));
    let mut extra = Config::for_test("ws://x".into());
    extra.system_instruction = Some("Your name is RTH.".into());
    let t = setup_message(&extra, None)["setup"]["systemInstruction"]["parts"][0]["text"].as_str().unwrap().to_string();
    assert!(t.starts_with("[rth-voice-policy ptbr-addressee-v1]") && t.ends_with("\n\nYour name is RTH."), "{t}");
    let mut pro = Config::for_test("ws://x".into());
    pro.proactive = true;
    pro.transcription_langs = vec!["pt-BR".into()];
    let v = setup_message(&pro, None);
    assert_eq!(v["setup"]["proactivity"], json!({"proactiveAudio": true}));
    assert_eq!(v["setup"]["inputAudioTranscription"], json!({"languageCodes": ["pt-BR"]}));
    assert!(base.knobs().contains("vad=server policy=ptbr-addressee-v1 proactive=false"));
    assert!(base.knobs().ends_with("system_extra=false persona=rth-persona-v2"), "{}", base.knobs());
}

#[tokio::test]
async fn provider_voice_activity_becomes_signals() {
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    ready(&mut s.outputs).await;
    send(&mut peer, json!({"voiceActivity": {"type": "ACTIVITY_START"}})).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: None, signal: "voice_activity_start" });
    send(&mut peer, audio(&[1, 1])).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Opened { response_id: "v1".into(), utterance_id: None });
    let _ = next_any(&mut s.outputs).await;
    send(&mut peer, json!({"voiceActivity": {"type": "ACTIVITY_END", "audioOffset": "1.2s"}})).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::Signal { response_id: Some("v1".into()), signal: "voice_activity_end" });
    let m = parse_server(br#"{"voiceActivity": {"type": "ACTIVITY_START"}}"#).unwrap();
    assert!(m.other.is_empty());
}

#[tokio::test]
async fn an_expired_resumption_handle_is_dropped_after_the_refusal() {
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    let (url, mut conns) = mock().await;
    let s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, json!({"sessionResumptionUpdate": {"newHandle": "h-old", "resumable": true}})).await;
    send(&mut peer, json!({"goAway": {"timeLeft": "1s"}})).await;

    let (mut peer2, _) = conns.recv().await.unwrap();
    let setup2 = recv_json(&mut peer2).await;
    assert_eq!(setup2["setup"]["sessionResumption"], json!({"handle": "h-old"}));
    peer2.close(Some(CloseFrame { code: CloseCode::Policy, reason: "BidiGenerateContent session not found or expired".into() })).await.unwrap();

    let (_peer3, setup3, _) = accept_setup(&mut conns).await;
    assert_eq!(setup3["setup"]["sessionResumption"], json!({}));
    drop(s);
}

#[test]
fn the_default_setup_is_pinned_and_the_measured_one_stays_reachable() {
    let mut cfg = Config::for_test("ws://x".into());
    cfg.compression = true;

    assert_eq!(setup_identity(&cfg), "b300e0e8415ea652");
    cfg.persona = crate::persona::PERSONA_V2_NAME;
    assert_eq!(setup_identity(&cfg), "0ae95df6eefb8bc1");
    cfg.persona = "rth-persona-v1";
    assert_eq!(setup_identity(&cfg), "d1cc067ff705e564");
    cfg.persona = "none";
    assert_eq!(setup_identity(&cfg), "b1e6d74d6e933ad7");
}

#[tokio::test]
async fn regression_model_boundary_must_not_make_a_tail_restore_eligible() {
    use rth_protocol::{Event, MonotonicClock, Source, Stamper};
    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, heard_part("do not ", None)).await;
    send(&mut peer, turn_complete()).await;
    send(&mut peer, heard_part("delete everything", Some(true))).await;
    let mut fold = crate::context::Fold::default();
    let mut stamp = Stamper::new(Source::Guest, "test", Box::new(MonotonicClock::start()));
    let mut parts = 0;
    while parts < 2 {
        if let Output::InputTranscript { text, heard } = next_any(&mut s.outputs).await {
            fold.apply(&stamp.stamp(Event::InputTranscript {
                text, utterance_id: heard.utterance_id, utterance_seq: heard.seq,
                is_final: heard.finished, response_id: heard.response_id,
                association: heard.association.map(str::to_owned), boundary: heard.boundary.map(str::to_owned),
            }, None));
            parts += 1;
        }
    }
    let turns: Vec<_> = fold.turns.into_iter().collect();
    let (kept, _) = settled_history(&turns);
    assert!(!kept.iter().any(|t| t.text == "delete everything"), "tail alone was promoted to settled history: {kept:?}");
}

async fn settled_texts(outputs: &mut mpsc::Receiver<Output>, parts: usize) -> Vec<String> {
    use rth_protocol::{Event, MonotonicClock, Source, Stamper};
    let mut fold = crate::context::Fold::default();
    let mut stamp = Stamper::new(Source::Guest, "test", Box::new(MonotonicClock::start()));
    let mut seen = 0;
    while seen < parts {
        if let Output::InputTranscript { text, heard } = next_any(outputs).await {
            let ev = Event::InputTranscript {
                text,
                utterance_id: heard.utterance_id,
                utterance_seq: heard.seq,
                is_final: heard.finished,
                response_id: heard.response_id,
                association: heard.association.map(str::to_owned),
                boundary: heard.boundary.map(str::to_owned),
            };
            fold.apply(&stamp.stamp(ev, None));
            seen += 1;
        }
    }
    let turns: Vec<_> = fold.turns.into_iter().collect();
    settled_history(&turns).0.into_iter().map(|t| t.text.clone()).collect()
}

#[tokio::test]
async fn a_segment_with_an_uncertain_beginning_is_never_restored() {
    let envelope = |text: &str, finished: bool| json!({"serverContent": {"inputTranscription": {"text": text, "finished": finished}}, "voiceActivity": {"type": "ACTIVITY_START"}});
    type Script = Vec<Value>;
    let cases: Vec<(&str, Script, usize)> = vec![
        ("late tail across a model turn's end", vec![heard_part("do not ", None), turn_complete(), heard_part("delete everything", Some(true))], 2),
        ("late tail across a new activity start", vec![activity("ACTIVITY_START"), heard_part("do not ", None), activity("ACTIVITY_START"), heard_part("delete everything", Some(true))], 2),
        ("text and activity start in one envelope", vec![heard_part("do not ", None), envelope("delete everything", true)], 2),
    ];
    for (name, script, parts) in cases {
        let (url, mut conns) = mock().await;
        let mut s = connect(Config::for_test(url));
        let (mut peer, _, _) = accept_setup(&mut conns).await;
        for m in script {
            send(&mut peer, m).await;
        }
        assert_eq!(settled_texts(&mut s.outputs, parts).await, Vec::<String>::new(), "{name}");
    }

    let (url, mut conns) = mock().await;
    let mut s = connect(Config::for_test(url));
    let (mut peer, _, _) = accept_setup(&mut conns).await;
    send(&mut peer, heard_part("do not ", None)).await;
    send(&mut peer, heard_part("delete anything", Some(true))).await;
    assert_eq!(settled_texts(&mut s.outputs, 2).await, vec!["do not delete anything".to_string()]);
}

#[tokio::test]
async fn delegation_tools_are_declared_called_answered_and_withdrawn() {
    use rth_tasks::schema::Tool;

    let off = setup_message(&Config::for_test("ws://x".into()), None);
    assert!(off["setup"].get("tools").is_none());
    assert!(off["setup"]["systemInstruction"]["parts"][0]["text"].as_str().unwrap().contains("- delegation: not declared"));

    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    cfg.tools = vec![crate::tools::Tool::Task(Tool::Delegate), crate::tools::Tool::Task(Tool::CancelTask)];
    let mut s = connect(cfg);
    let (mut peer, setup, _) = accept_setup(&mut conns).await;
    assert_eq!(setup["setup"]["tools"][0]["functionDeclarations"][0]["name"], "delegate");
    assert!(setup["setup"]["tools"][0]["functionDeclarations"][0].get("behavior").is_none());
    let instr = setup["setup"]["systemInstruction"]["parts"][0]["text"].as_str().unwrap();
    assert!(!instr.contains("You have no tools") && instr.contains("cancel_task") && instr.contains("- delegation: not observed"));
    ready(&mut s.outputs).await;

    send(&mut peer, json!({"toolCall": {"functionCalls": [{"id": "fc1", "name": "delegate", "args": {"purpose": "report", "task": "relatório"}}]}})).await;
    assert_eq!(
        next_any(&mut s.outputs).await,
        Output::ToolCall { response_id: None, generation: 1, call_id: "fc1".into(), name: "delegate".into(), args: json!({"purpose": "report", "task": "relatório"}) }
    );
    let output = json!({"status": "accepted", "task_id": "b-t1"});
    s.commands.send(Command::ToolOutput { call_id: "fc1".into(), name: "delegate".into(), output: output.clone() }).await.unwrap();
    assert_eq!(recv_json(&mut peer).await, json!({"toolResponse": {"functionResponses": [{"id": "fc1", "name": "delegate", "response": output}]}}));
    assert_eq!(next_any(&mut s.outputs).await, Output::ToolOutputSent { call_id: "fc1".into() });
    send(&mut peer, json!({"toolCallCancellation": {"ids": ["fc2"]}})).await;
    assert_eq!(next_any(&mut s.outputs).await, Output::ToolCancelled { call_ids: vec!["fc2".into()] });
}

#[tokio::test]
async fn dmg_r0_gemini_first_setup_waits_for_the_boot_gate_and_carries_its_readiness() {
    let _readiness = crate::persona::READINESS_TEST_LOCK.lock().await;
    crate::persona::reset_readiness_for_test();
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    cfg.tools = vec![crate::tools::Tool::Memory(rth_memory::schema::Tool::Recall)];
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let _s = connect(cfg);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(conns.try_recv().is_err(), "no Gemini setup before the boot has decided readiness");
    let attached = format!("attached: brain (dmg-r0 gemini {})", std::process::id());
    crate::persona::set_readiness(crate::persona::Readiness { memory: Some(attached.clone()), delegation: None });
    let _ = open.send(true);
    let (_peer, setup, _) = accept_setup(&mut conns).await;
    let instr = setup["setup"]["systemInstruction"]["parts"][0]["text"].as_str().unwrap();
    assert!(instr.contains(&format!("- memory: {attached}\n")) && !instr.contains("- memory: not observed"), "{instr}");
}

#[tokio::test]
async fn dmg_r2_gemini_cancelled_gate_ends_without_setup_or_dial() {
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let mut s = connect(cfg);
    drop(open);
    assert!(tokio::time::timeout(Duration::from_millis(800), conns.recv()).await.is_err(), "a cancelled gate must not dial");
    let ended = tokio::time::timeout(Duration::from_secs(2), async { while s.outputs.recv().await.is_some() {} }).await;
    assert!(ended.is_ok(), "the backend task ends");
}

#[tokio::test]
async fn dmg_r2_gemini_opened_then_dropped_gate_still_dials() {
    let (url, mut conns) = mock().await;
    let mut cfg = Config::for_test(url);
    let (open, gate) = tokio::sync::watch::channel(false);
    cfg.setup_gate = Some(gate);
    let _s = connect(cfg);
    open.send(true).unwrap();
    drop(open);
    let (_peer, setup, _) = accept_setup(&mut conns).await;
    assert!(setup.get("setup").is_some(), "{setup}");
}
