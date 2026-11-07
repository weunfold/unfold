mod common;

use common::*;
use serde_json::json;
use speech_markup::{parse, plan, Event, Mood, Step};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use unfold_speech::{normalize_pt, ApiKey, AudioChunk, Client, Language, Playback, Sink, StepError, Warning};

fn client(fake: &Fake) -> Client {
    Client::new(&fake.url, Some(ApiKey::new(KEY))).unwrap()
}

fn hold(fake: &Fake) -> Client {
    let mut c = client(fake);
    c.playback = Playback::HoldUntilDone;
    c
}

fn with(fake: &Fake, playback: Playback) -> Client {
    let mut c = client(fake);
    c.playback = playback;
    c
}

fn speak(text: &str, mood: Mood) -> Step {
    Step::Speak { text: text.into(), mood, requested: mood }
}

#[test]
fn normal_stream_plays_every_chunk_and_done_matches_frames() {
    let fake = serve(|_| ok(vec![(7680, 1), (7680, 2), (7680, 3), (7680, 4)]));
    let (_c, voice) = voice("normal", &[Mood::Joy, Mood::Calm]);
    let (steps, _) = plan(&parse("⟨say:speech mood=joy⟩\nBom dia!"), &voice.to_markup_voice());
    assert_eq!(steps, vec![speak("Bom dia!", Mood::Joy)]);
    let mut sink = Recorder::default();
    let report = client(&fake).speak_plan(&steps, &voice, &mut sink);

    assert_eq!(client(&fake).playback, Playback::Live, "Live is the default");
    let spoken = report.steps[0].result.clone().unwrap();
    assert_eq!((spoken.requests, spoken.frames, spoken.audio_seconds), (1, 16, 1.28));
    assert!(spoken.first_audio.is_some());
    assert_eq!(spoken.frames as f64 / 12.5, spoken.audio_seconds);
    assert!(report.steps[0].warnings.is_empty());
    assert_eq!(sink.samples(), 4 * 7680);

    let req = &fake.requests.lock().unwrap()[0];
    assert!(req.head.starts_with("POST /v1/audio/speech HTTP/1.1\r\n"));
    assert!(req.head.lines().any(|l| l == format!("Authorization: Bearer {KEY}")));
    assert_eq!(req.body["stream"], json!(true));
    assert_eq!(req.body["stream_format"], json!("events"));
    assert_eq!(req.body["model"], json!("qwen3-tts-1.7b-base"));
    assert_eq!(req.body["references"][0]["text"], json!(transcript(Mood::Joy)));

    let allowed = ["model", "input", "voice", "mood", "language", "response_format", "references", "seed",
        "temperature", "top_p", "max_new_tokens", "stream", "stream_format"];
    assert!(req.body.as_object().unwrap().keys().all(|k| allowed.contains(&k.as_str())), "{}", req.body);

    for absent in ["voice", "mood", "language"] {
        assert!(req.body.get(absent).is_none(), "{absent} sent: {}", req.body);
    }
}

#[test]
fn truncated_reply_is_resplit_and_only_the_halves_play() {
    let fake = serve(|r| {
        if r.input().chars().count() > 30 {
            Reply::Events { chunks: vec![(3840, 999)], done: Some(done(3840, true, None)), cut: false }
        } else {
            ok(vec![(1920, 7)])
        }
    });
    let (_c, voice) = voice("trunc", &[Mood::Joy]);
    let mut sink = Recorder::default();
    let text = "Essa frase é longa demais, então vai ser dividida.";
    let report = hold(&fake).speak_plan(&[speak(text, Mood::Joy)], &voice, &mut sink);

    assert_eq!(fake.inputs(), vec![text, "Essa frase é longa demais,", "então vai ser dividida."]);
    assert!(!sink.has_value(999), "the cut audio must not reach the sink");
    assert_eq!(sink.samples(), 2 * 1920);
    let o = &report.steps[0];
    let spoken = o.result.clone().unwrap();
    assert_eq!((spoken.requests, spoken.frames, spoken.audio_seconds), (3, 2, 0.16));
    assert!(spoken.first_audio.is_some());
    assert!(o.warnings.is_empty());
}

#[test]
fn still_truncated_after_two_levels_plays_what_it_has_and_warns() {
    let fake = serve(|_| Reply::Events { chunks: vec![(1920, 5)], done: Some(done(1920, true, None)), cut: false });
    let (_c, voice) = voice("trunc2", &[Mood::Joy]);
    let mut sink = Recorder::default();
    let report = hold(&fake).speak_plan(&[speak("um dois três quatro cinco seis sete oito", Mood::Joy)], &voice, &mut sink);

    assert_eq!(fake.count(), 1 + 2 + 4);
    assert_eq!(sink.samples(), 4 * 1920);
    let o = &report.steps[0];
    assert_eq!(o.result.as_ref().unwrap().requests, 7);
    assert_eq!(o.warnings.iter().filter(|w| matches!(w, Warning::StillTruncated { depth: 2, .. })).count(), 4);
}

#[test]
fn error_mid_stream_is_typed_and_the_next_step_runs() {
    for playback in [Playback::Live, Playback::HoldUntilDone] {
        let fake = serve(|r| {
            if r.input() == "Falha aqui." {
                Reply::Events { chunks: vec![(1920, 999)], done: Some(done(1920, false, Some(("inference_failed", "RuntimeError")))), cut: false }
            } else {
                ok(vec![(1920, 7)])
            }
        });
        let (_c, voice) = voice("err", &[Mood::Calm]);
        let mut sink = Recorder::default();
        let steps = [speak("Falha aqui.", Mood::Calm), Step::Silence { ms: 300 }, speak("Depois funciona.", Mood::Calm)];
        let report = with(&fake, playback).speak_plan(&steps, &voice, &mut sink);

        match &report.steps[0].result {
            Err(StepError::Server { code, message, audio_seconds }) => {
                assert_eq!((code.as_str(), message.as_str(), *audio_seconds), ("inference_failed", "RuntimeError", 0.08));
            }
            other => panic!("expected a Server error, got {other:?}"),
        }
        assert!(report.steps[1].result.is_ok() && report.steps[2].result.is_ok());

        let live = playback == Playback::Live;
        assert_eq!(sink.order, if live { vec!["a0", "s1", "a2"] } else { vec!["s1", "a2"] });
        assert_eq!(sink.has_value(999), live);
        assert_eq!(report.errors().count(), 1);
    }
}

#[test]
fn stream_dropped_without_done_is_typed_and_the_next_step_runs() {
    for playback in [Playback::Live, Playback::HoldUntilDone] {
        let fake = serve(|r| {
            if r.input() == "Cai." {
                Reply::Events { chunks: vec![(1920, 999)], done: None, cut: true }
            } else {
                ok(vec![(1920, 7)])
            }
        });
        let (_c, voice) = voice("drop", &[Mood::Neutral]);
        let mut sink = Recorder::default();
        let report = with(&fake, playback).speak_plan(&[speak("Cai.", Mood::Neutral), speak("Volta.", Mood::Neutral)], &voice, &mut sink);

        assert!(matches!(report.steps[0].result, Err(StepError::NoDone { audio_events: 1, .. })), "{:?}", report.steps[0]);
        assert!(report.steps[1].result.is_ok());
        let live = playback == Playback::Live;
        assert_eq!(sink.has_value(999), live);
        assert_eq!(sink.order, if live { vec!["a0", "a1"] } else { vec!["a1"] });
    }
}

#[test]
fn stream_closed_cleanly_without_done_is_no_done() {
    let fake = serve(|_| Reply::Events { chunks: vec![(1920, 1), (1920, 2)], done: None, cut: false });
    let (_c, voice) = voice("nodone", &[Mood::Neutral]);
    let report = client(&fake).speak_plan(&[speak("Oi.", Mood::Neutral)], &voice, &mut Recorder::default());
    assert!(matches!(report.steps[0].result, Err(StepError::NoDone { audio_events: 2, cause: None })));
}

#[test]
fn input_over_the_cap_is_split_at_sentence_ends() {
    let fake = serve(|_| ok(vec![(1920, 7)]));
    let (_c, voice) = voice("cap", &[Mood::Neutral]);
    let text: String = (0..60).map(|i| format!("Frase número {i:02} tem algumas palavras.")).collect::<Vec<_>>().join(" ");
    assert!(text.chars().count() > 2000);
    let report = client(&fake).speak_plan(&[speak(&text, Mood::Neutral)], &voice, &mut Recorder::default());

    let inputs = fake.inputs();
    assert!(inputs.len() >= 3);
    assert!(inputs.iter().all(|i| i.chars().count() <= 1000 && i.ends_with('.')), "{inputs:?}");
    assert_eq!(inputs.join(" "), normalize_pt(&text));
    assert_eq!(report.steps[0].warnings, vec![Warning::SplitForCap { pieces: inputs.len() }]);
    assert_eq!(report.steps[0].result.as_ref().unwrap().requests as usize, inputs.len());
}

#[test]
fn the_key_never_appears_in_debug_or_errors() {

    let fake = serve(|r| {
        let auth = r.head.lines().find(|l| l.starts_with("Authorization")).unwrap_or("").to_string();
        Reply::Json { status: 401, body: json!({"error": {"type": "granite_error", "code": "unauthorized", "message": auth}}), headers: vec![] }
    });
    let (_c, voice) = voice("key", &[Mood::Neutral]);
    let client = client(&fake);
    let report = client.speak_plan(&[speak("Oi.", Mood::Neutral)], &voice, &mut Recorder::default());
    let err = report.steps[0].result.clone().unwrap_err();
    assert!(matches!(&err, StepError::Http { status: 401, code: Some(c), .. } if c == "unauthorized"), "{err:?}");
    for text in [format!("{client:?}"), format!("{report:?}"), format!("{err}"), format!("{err:?}"), format!("{:?}", ApiKey::new(KEY))] {
        assert!(!text.contains(KEY), "key leaked: {text}");
    }

    let req = voice.request(Mood::Neutral, "Oi.").unwrap();
    assert!(!format!("{req:?}").contains(&req.references[0].audio_base64[..40]));
}

#[test]
fn raw_wav_stream_is_rejected_clearly() {
    let fake = serve(|_| Reply::Wav);
    let (_c, voice) = voice("wav", &[Mood::Neutral]);
    let mut sink = Recorder::default();
    let report = client(&fake).speak_plan(&[speak("Oi.", Mood::Neutral)], &voice, &mut sink);
    let err = report.steps[0].result.clone().unwrap_err();
    assert_eq!(err, StepError::NotEventStream { status: 200, content_type: "audio/wav".into() });
    assert!(err.to_string().contains("stream_format \"events\""));
    assert_eq!(sink.samples(), 0);
}

#[test]
fn refusals_before_synthesis_are_typed() {
    let fake = serve(|r| match r.input().as_str() {
        "busy" => Reply::Json {
            status: 429,
            body: json!({"error": {"type": "granite_error", "code": "admission_busy", "message": "one synthesis at a time"}}),
            headers: vec![("Retry-After", "2")],
        },
        _ => Reply::Json {
            status: 400,
            body: json!({"error": {"type": "granite_error", "code": "invalid_schema", "message": "input must be 1 to 1000 characters"}}),
            headers: vec![],
        },
    });
    let (_c, voice) = voice("refuse", &[Mood::Neutral]);
    let report = client(&fake).speak_plan(&[speak("busy", Mood::Neutral), speak("bad", Mood::Neutral)], &voice, &mut Recorder::default());
    assert_eq!(report.steps[0].result, Err(StepError::Busy { retry_after_s: Some(2) }));
    assert_eq!(
        report.steps[1].result,
        Err(StepError::Http {
            status: 400,
            code: Some("invalid_schema".into()),
            message: Some("input must be 1 to 1000 characters".into())
        })
    );
}

#[test]
fn unreachable_server_is_an_io_error() {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let client = Client::new(&format!("http://127.0.0.1:{port}"), None).unwrap();
    let (_c, voice) = voice("down", &[Mood::Neutral]);
    let report = client.speak_plan(&[speak("Oi.", Mood::Neutral)], &voice, &mut Recorder::default());
    assert!(matches!(report.steps[0].result, Err(StepError::Io { .. })), "{:?}", report.steps[0]);
}

#[test]
fn clips_and_silence_go_to_the_sink_as_markers() {
    let fake = serve(|_| ok(vec![(1920, 7)]));
    let (_c, voice) = voice("markers", &[Mood::Joy]);
    let (steps, _) = plan(&parse("⟨say:speech mood=joy⟩\n<laugh> Essa foi boa! <pause> Né."), &voice.to_markup_voice());
    assert_eq!(steps[0], Step::Clip(Event::Laugh));
    let mut sink = Recorder::default();
    let report = client(&fake).speak_plan(&steps, &voice, &mut sink);
    assert_eq!(report.errors().count(), 0);
    assert_eq!(sink.order, vec!["c0", "a1", "s2", "a3"]);
    let file = sink.clips[0].2.clone().unwrap();
    assert!(file.is_file());
    assert_eq!(fake.count(), 2);
}

#[test]
fn base_urls_are_checked() {
    assert!(Client::new("http://127.0.0.1:8310", None).is_ok());
    assert!(Client::new("http://localhost:8310/", None).is_ok());
    assert!(Client::new("http://[::1]:8310", None).is_ok());
    for bad in ["https://x:1", "http://user:pw@x:1", "http://x:1/v1", "x:1", "http://x:port"] {
        assert!(Client::new(bad, None).is_err(), "{bad}");
    }
}

struct GateSink {
    gate: Arc<Gate>,
    audio_before_done: Option<bool>,
    samples: usize,
}

impl Sink for GateSink {
    fn audio(&mut self, _step: usize, chunk: &AudioChunk) {
        if self.audio_before_done.is_none() {
            self.audio_before_done = Some(!self.gate.done_sent.load(Ordering::SeqCst));
            self.gate.open();
        }
        self.samples += chunk.samples.len();
    }
    fn clip(&mut self, _: usize, _: Event, _: Option<&std::path::Path>) {}
    fn silence(&mut self, _: usize, _: u32) {}
}

#[test]
fn live_hands_chunks_to_the_sink_before_done_and_times_first_audio() {
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let fake = serve(move |_| Reply::Gated {
        chunks: vec![(1920, 1), (1920, 2)],
        done: done(3840, false, None),
        delay: Duration::from_millis(60),
        gate: g.clone(),
    });
    let (_c, voice) = voice("live", &[Mood::Joy]);
    let mut sink = GateSink { gate: gate.clone(), audio_before_done: None, samples: 0 };
    let report = client(&fake).speak_plan(&[speak("Oi!", Mood::Joy)], &voice, &mut sink);

    assert_eq!(sink.audio_before_done, Some(true), "the sink got audio while the request was still open");
    assert_eq!(sink.samples, 3840);
    let spoken = report.steps[0].result.clone().unwrap();
    let first = spoken.first_audio.expect("first_audio is measured");
    assert!(first >= Duration::from_millis(60) && first < Duration::from_secs(2), "{first:?}");
}

#[test]
fn hold_until_done_hands_nothing_over_before_done() {
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let fake = serve(move |_| Reply::Gated { chunks: vec![(1920, 1)], done: done(1920, false, None), delay: Duration::ZERO, gate: g.clone() });
    let (_c, voice) = voice("holdgate", &[Mood::Joy]);
    let mut sink = GateSink { gate: gate.clone(), audio_before_done: None, samples: 0 };
    let report = hold(&fake).speak_plan(&[speak("Oi!", Mood::Joy)], &voice, &mut sink);
    assert_eq!(sink.audio_before_done, Some(false));
    assert!(report.steps[0].result.as_ref().unwrap().first_audio.is_some());
}

#[test]
fn live_truncated_reply_is_played_reported_and_not_retried() {
    let fake = serve(|_| Reply::Events { chunks: vec![(3840, 5)], done: Some(done(3840, true, None)), cut: false });
    let (_c, voice) = voice("livetrunc", &[Mood::Joy]);
    let mut sink = Recorder::default();
    let steps = [Step::Silence { ms: 100 }, speak("Essa frase é longa demais, então vai ser dividida.", Mood::Joy)];
    let report = client(&fake).speak_plan(&steps, &voice, &mut sink);

    assert_eq!(fake.count(), 1, "no re-request in Live");
    assert_eq!(sink.samples(), 3840);
    let o = &report.steps[1];
    assert_eq!(o.result.as_ref().unwrap().requests, 1);
    assert_eq!(o.warnings, vec![Warning::TruncatedPlayed { step: 1, audio_seconds: 0.16 }]);
}

#[test]
fn language_is_sent_lowercase_and_only_when_set() {
    let fake = serve(|_| ok(vec![(1920, 7)]));
    let (_c, voice) = voice("lang", &[Mood::Neutral]);
    let mut pinned = client(&fake);
    pinned.language = Some("Portuguese".into());
    pinned.speak_plan(&[speak("Oi, tudo bem? Let's go.", Mood::Neutral)], &voice, &mut Recorder::default());
    client(&fake).speak_plan(&[speak("Oi.", Mood::Neutral)], &voice, &mut Recorder::default());
    let reqs = fake.requests.lock().unwrap();
    assert_eq!(reqs[0].body["language"], json!("portuguese"));
    assert!(reqs[1].body.get("language").is_none());
    assert_eq!(voice.request(Mood::Neutral, "Oi.").unwrap().language, None);
}

#[test]
fn frame_budget_header_is_read_and_checked_against_done() {
    let fake = serve(|r| {
        let budget = if r.input() == "Dentro." { 778 } else { 2 };
        Reply::Budget { chunks: vec![(4 * 1920, 7)], done: done(4 * 1920, false, None), budget }
    });
    let (_c, voice) = voice("budget", &[Mood::Neutral]);
    for playback in [Playback::Live, Playback::HoldUntilDone] {
        let steps = [speak("Dentro.", Mood::Neutral), speak("Fora.", Mood::Neutral)];
        let report = with(&fake, playback).speak_plan(&steps, &voice, &mut Recorder::default());
        let (inside, over) = (&report.steps[0], &report.steps[1]);
        assert_eq!(inside.result.as_ref().unwrap().frame_budget, Some(778));
        assert!(inside.warnings.is_empty(), "{:?}", inside.warnings);
        assert_eq!(over.result.as_ref().unwrap().frame_budget, Some(2));
        assert_eq!(
            over.warnings,
            vec![Warning::DoneMismatch { samples: 4 * 1920, frames: 4, audio_seconds: 0.32, frame_budget: Some(2) }]
        );
    }
}

#[test]
fn gateway_done_after_a_backend_break_is_a_server_error_and_the_next_step_runs() {
    let fake = serve(|r| {
        if r.input() == "Quebra." {
            Reply::GatewayBreak {
                chunks: vec![(1920, 999)],

                cut: "event: audio\ndata: {\"pcm_base64\": \"AAAA",
                done: serde_json::json!({"truncated": false, "frames": 1, "audio_seconds": 0.08,
                    "error": {"code": "backend_error", "message": "the backend stream broke before it finished"}}),
            }
        } else {
            ok(vec![(1920, 7)])
        }
    });
    let (_c, voice) = voice("gwdone", &[Mood::Neutral]);
    for playback in [Playback::Live, Playback::HoldUntilDone] {
        let mut sink = Recorder::default();
        let steps = [speak("Quebra.", Mood::Neutral), speak("Segue.", Mood::Neutral)];
        let report = with(&fake, playback).speak_plan(&steps, &voice, &mut sink);
        match &report.steps[0].result {
            Err(StepError::Server { code, audio_seconds, .. }) => assert_eq!((code.as_str(), *audio_seconds), ("backend_error", 0.08)),
            other => panic!("{playback:?}: expected Server backend_error, got {other:?}"),
        }
        assert!(report.steps[1].result.is_ok());
        assert_eq!(sink.has_value(999), playback == Playback::Live);
    }
}

#[test]
fn refusals_before_admission_map_to_http_with_the_gateways_codes() {

    let fake = serve(|r| {
        let message = if r.input().chars().count() > 1000 {
            "qwen3-tts-1.7b-base takes at most 1000 input characters"
        } else {
            "qwen3-tts-1.7b-base has no default voice: send references, or a stored voice id as voice"
        };
        Reply::Json { status: 400, body: json!({"error": {"type": "granite_error", "code": "invalid_schema", "message": message}}), headers: vec![] }
    });
    let (_c, voice) = voice("admission", &[Mood::Neutral]);
    let mut c = client(&fake);
    c.max_input_chars = 4000;
    let long = "palavra ".repeat(150).trim().to_string();
    let report = c.speak_plan(&[speak(&long, Mood::Neutral), speak("Oi.", Mood::Neutral)], &voice, &mut Recorder::default());
    let http = |m: &str| Err(StepError::Http { status: 400, code: Some("invalid_schema".into()), message: Some(m.into()) });
    assert_eq!(report.steps[0].result, http("qwen3-tts-1.7b-base takes at most 1000 input characters"));
    assert_eq!(
        report.steps[1].result,
        http("qwen3-tts-1.7b-base has no default voice: send references, or a stored voice id as voice")
    );
}

#[test]
fn a_pt_voice_speaks_normalized_text_and_an_en_voice_speaks_it_as_written() {
    let text = "Dá R$ 42,50 para vocês dois.";
    let fake = serve(|_| ok(vec![(1920, 7)]));
    let (_c, pt_voice) = voice("pt-normalized", &[Mood::Neutral]);
    client(&fake).speak_plan(&[speak(text, Mood::Neutral)], &pt_voice, &mut Recorder::default());
    assert_eq!(fake.inputs(), vec!["Dá quarenta e dois reais e cinquenta centavos para vocês dois.".to_string()]);

    let fake = serve(|_| ok(vec![(1920, 7)]));
    let (_c, mut en_voice) = voice("en-as-written", &[Mood::Neutral]);
    en_voice.language = Language::En;
    client(&fake).speak_plan(&[speak(text, Mood::Neutral)], &en_voice, &mut Recorder::default());
    assert_eq!(fake.inputs(), vec![text.to_string()]);
}
