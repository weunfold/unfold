
mod common;

use common::*;
use speech_markup::{Event, Mood, Step};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use unfold_speech::{AudioChunk, Cancel, Client, Sink, StepError};

fn speak(text: &str) -> Step {
    Step::Speak { text: text.into(), mood: Mood::Joy, requested: Mood::Joy }
}

struct Stopper {
    rec: Recorder,
    cancel: Cancel,
    on_audio: bool,
    on_silence: bool,
}

impl Stopper {
    fn new(on_audio: bool, on_silence: bool) -> Self {
        Stopper { rec: Recorder::default(), cancel: Cancel::new(), on_audio, on_silence }
    }
}

impl Sink for Stopper {
    fn audio(&mut self, step: usize, chunk: &AudioChunk) {
        self.rec.audio(step, chunk);
        if self.on_audio {
            self.cancel.cancel();
        }
    }
    fn clip(&mut self, step: usize, event: Event, file: Option<&Path>) {
        self.rec.clip(step, event, file);
    }
    fn silence(&mut self, step: usize, ms: u32) {
        self.rec.silence(step, ms);
        if self.on_silence {
            self.cancel.cancel();
        }
    }
    fn cancel(&self) -> Option<Cancel> {
        Some(self.cancel.clone())
    }
}

#[test]
fn a_cancel_between_steps_skips_the_rest_of_the_plan() {
    let fake = serve(|_| ok(vec![(1920, 1)]));
    let (_c, voice) = voice("cancel-between", &[Mood::Joy]);
    let mut sink = Stopper::new(false, true);
    let steps = [Step::Silence { ms: 100 }, speak("Oi."), Step::Silence { ms: 200 }];
    let report = Client::new(&fake.url, None).unwrap().speak_plan(&steps, &voice, &mut sink);

    assert_eq!(fake.count(), 0, "no request went out after the cancel");
    assert_eq!(sink.rec.silences, vec![(0, 100)]);
    assert_eq!(report.steps.len(), 2, "{report:?}");
    assert!(report.steps[0].result.is_ok());
    assert_eq!(report.steps[1].result, Err(StepError::Cancelled));
}

#[test]
fn a_cancel_mid_stream_stops_after_the_current_chunk() {
    let fake = serve(|_| ok(vec![(1920, 1), (1920, 2), (1920, 3)]));
    let (_c, voice) = voice("cancel-mid", &[Mood::Joy]);
    let mut sink = Stopper::new(true, false);
    let steps = [speak("Oi."), speak("Tchau.")];
    let report = Client::new(&fake.url, None).unwrap().speak_plan(&steps, &voice, &mut sink);

    assert!(sink.rec.has_value(1));
    assert!(!sink.rec.has_value(2), "no chunk reached the sink after the cancel");
    assert_eq!(fake.count(), 1, "the second step never sent its request");
    assert_eq!(report.steps.len(), 1, "{report:?}");
    assert_eq!(report.steps[0].result, Err(StepError::Cancelled));
}

#[test]
fn a_cancel_drops_a_request_the_server_is_still_silent_on() {
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let fake = serve(move |_| Reply::Gated {
        chunks: vec![(1920, 1)],
        done: done(1920, false, None),
        delay: Duration::from_millis(1500),
        gate: g.clone(),
    });
    let (_c, voice) = voice("cancel-silent", &[Mood::Joy]);
    let mut sink = Stopper::new(false, false);
    let cancel = sink.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        cancel.cancel();
    });
    let started = Instant::now();
    let report = Client::new(&fake.url, None).unwrap().speak_plan(&[speak("Oi.")], &voice, &mut sink);
    gate.open();

    let took = started.elapsed();
    assert!(took < Duration::from_millis(800), "the cancel was seen within a tick or two, not after the audio: {took:?}");
    assert_eq!(sink.rec.samples(), 0);
    assert_eq!(report.steps[0].result, Err(StepError::Cancelled));
}

#[test]
fn a_silent_server_still_times_out_when_a_cancel_is_held() {
    let gate = Arc::new(Gate::default());
    let g = gate.clone();
    let fake = serve(move |_| Reply::Gated {
        chunks: vec![(1920, 1)],
        done: done(1920, false, None),
        delay: Duration::from_millis(1500),
        gate: g.clone(),
    });
    let (_c, voice) = voice("cancel-timeout", &[Mood::Joy]);
    let mut client = Client::new(&fake.url, None).unwrap();
    client.timeout = Duration::from_millis(400);
    let mut sink = Stopper::new(false, false);
    let started = Instant::now();
    let report = client.speak_plan(&[speak("Oi.")], &voice, &mut sink);
    gate.open();

    let took = started.elapsed();
    assert!(took >= Duration::from_millis(350) && took < Duration::from_millis(1200), "{took:?}");
    assert!(matches!(report.steps[0].result, Err(StepError::NoDone { audio_events: 0, cause: Some(_) })), "{report:?}");
}

fn serve_split(pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let Ok((mut conn, _)) = listener.accept() else { return };
        let mut reader = BufReader::new(conn.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; len];
        reader.read_exact(&mut body).unwrap();
        let body = format!("event: audio\ndata: {}\n\nevent: done\ndata: {}\n\n", audio_data(1920, 4), done(1920, false, None));
        let (a, b) = body.split_at(40);
        let _ = conn.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
        let _ = conn.write_all(a.as_bytes());
        let _ = conn.flush();
        std::thread::sleep(pause);
        let _ = conn.write_all(b.as_bytes());
        let _ = conn.flush();
    });
    url
}

fn audio_data(n: usize, v: i16) -> String {
    use base64::Engine;
    let pcm: Vec<u8> = std::iter::repeat_n(v.to_le_bytes(), n).flatten().collect();
    serde_json::json!({"pcm_base64": base64::engine::general_purpose::STANDARD.encode(pcm), "sample_rate": 24000, "format": "s16le"})
        .to_string()
}

#[test]
fn waiting_in_ticks_loses_no_bytes_of_a_slow_reply() {
    let url = serve_split(Cancel::TICK * 3);
    let (_c, voice) = voice("cancel-split", &[Mood::Joy]);
    let mut sink = Stopper::new(false, false);
    let report = Client::new(&url, None).unwrap().speak_plan(&[speak("Oi.")], &voice, &mut sink);

    assert!(report.steps[0].result.is_ok(), "{report:?}");
    assert_eq!(sink.rec.samples(), 1920);
    assert!(sink.rec.has_value(4));
}
