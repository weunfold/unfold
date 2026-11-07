

#[path = "../src/onnx_cpu.rs"]
mod onnx_cpu;
#[path = "../src/turn_detect.rs"]
mod turn_detect;
#[path = "../src/turn_shadow.rs"]
mod turn_shadow;
#[path = "../src/turn_features.rs"]
mod turn_features;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[allow(dead_code)]
#[path = "../src/conversation_input.rs"]
mod conversation_input;
#[allow(dead_code)]
#[path = "../src/conversation_overlap.rs"]
mod conversation_overlap;
#[allow(dead_code)]
#[path = "../src/conversation_state.rs"]
mod conversation_state;

const RATE: u64 = 16_000;
const FRAME: u64 = 800;
#[derive(Clone, Deserialize)]
struct Asset {
    id: String,
    file: String,
    sha256: String,
    licence: String,
}
#[derive(Clone, Deserialize)]
struct Segment {
    asset: String,
    start_ms: u64,
    end_ms: u64,
    offset_ms: u64,
    gain: f32,
}
#[derive(Clone, Deserialize)]
struct Asr {
    at_ms: u64,
    text: String,
    final_words: bool,
}
#[derive(Clone, Deserialize)]
struct OverlapGold {
    start_ms: u64,
    end_ms: u64,
    kind: String,
}
#[derive(Clone, Deserialize)]
struct Gold {
    provenance_id: String,
    endpoint_ms: Option<u64>,
    hold: Vec<[u64; 2]>,
    overlap: Option<OverlapGold>,
}
#[derive(Clone, Deserialize)]
struct Case {
    id: String,
    language: String,
    split: String,
    duration_ms: u64,
    segments: Vec<Segment>,
    asr: Vec<Asr>,
    gold: Gold,
    seed_reply: bool,
    reply_text: String,
    reply_delay_ms: u64,
    reply_duration_ms: u64,
    #[serde(default)]
    asr_provenance: String,
}
#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    assets: Vec<Asset>,
    cases: Vec<Case>,
    splits: Value,
}
struct Audio {
    rate: u64,
    samples: Vec<f32>,
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/conversation")
}
fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn unpack(path: &Path) -> Result<Vec<u8>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let hex: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return Err("invalid hex fixture".into());
    }
    let compressed: Result<Vec<_>, _> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect();
    let mut out = Vec::new();
    GzDecoder::new(compressed.map_err(|e| e.to_string())?.as_slice())
        .take(8 * 1024 * 1024)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}
fn wav(bytes: &[u8]) -> Result<Audio, String> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("expected RIFF WAVE".into());
    }
    let u16at = |n: usize| u16::from_le_bytes([bytes[n], bytes[n + 1]]);
    let u32at = |n: usize| u32::from_le_bytes([bytes[n], bytes[n + 1], bytes[n + 2], bytes[n + 3]]);
    let mut fmt = None;
    let mut data = None;
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let size = u32at(i + 4) as usize;
        if size > bytes.len() - i - 8 {
            return Err("truncated WAV chunk".into());
        }
        if &bytes[i..i + 4] == b"fmt " && size >= 16 {
            fmt = Some((u16at(i + 8), u16at(i + 10), u32at(i + 12), u16at(i + 22)));
        }
        if &bytes[i..i + 4] == b"data" {
            data = Some(&bytes[i + 8..i + 8 + size]);
        }
        i += 8 + size + size % 2;
    }
    let (format, channels, rate, bits) = fmt.ok_or("missing WAV fmt")?;
    if format != 1 || bits != 16 || channels == 0 || rate == 0 {
        return Err("expected PCM16 WAV".into());
    }
    let data = data.ok_or("missing WAV data")?;
    let stride = usize::from(channels) * 2;
    if data.len() % stride != 0 {
        return Err("unaligned WAV data".into());
    }
    let samples = data
        .chunks_exact(stride)
        .map(|f| {
            f.chunks_exact(2)
                .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
                .sum::<f32>()
                / f32::from(channels)
        })
        .collect();
    Ok(Audio {
        rate: u64::from(rate),
        samples,
    })
}
fn validate(m: &Manifest) -> Result<(), String> {
    if m.schema != 1 || m.cases.is_empty() {
        return Err("empty/unsupported fixture set".into());
    }
    let assets: BTreeSet<_> = m.assets.iter().map(|a| a.id.as_str()).collect();
    let mut ids = BTreeSet::new();
    for a in &m.assets {
        if !["CC-BY-4.0", "Apache-2.0", "MIT"].contains(&a.licence.as_str())
            || Path::new(&a.file).components().count() != 1
        {
            return Err("unapproved asset licence/path".into());
        }
    }
    for c in &m.cases {
        if !ids.insert(&c.id)
            || c.gold.provenance_id.is_empty()
            || c.duration_ms == 0
            || c.duration_ms > 120_000
        {
            return Err("duplicate/unlabelled/invalid case".into());
        }
        if c.asr.windows(2).any(|p| p[0].at_ms > p[1].at_ms)
            || c.asr
                .iter()
                .any(|e| e.at_ms > c.duration_ms || e.at_ms % 50 != 0)
        {
            return Err("ASR must be ordered on audio-frame boundaries".into());
        }
        for s in &c.segments {
            if s.start_ms >= s.end_ms
                || s.end_ms > c.duration_ms
                || !s.gain.is_finite()
                || (s.asset != "signal" && !assets.contains(s.asset.as_str()))
            {
                return Err("invalid audio span".into());
            }
        }
        if c.gold
            .hold
            .iter()
            .any(|p| p[0] >= p[1] || p[1] > c.duration_ms)
            || c.gold.endpoint_ms.is_some_and(|end| end > c.duration_ms)
        {
            return Err("invalid endpoint/hold gold".into());
        }
        if let Some(g) = &c.gold.overlap {
            if g.start_ms >= g.end_ms
                || g.end_ms > c.duration_ms
                || !["content", "backchannel", "echo", "noise", "side_talk"]
                    .contains(&g.kind.as_str())
                || !c.seed_reply
            {
                return Err("invalid overlap gold".into());
            }
        }
    }
    Ok(())
}
fn load() -> (Manifest, BTreeMap<String, Audio>) {
    let m: Manifest =
        serde_json::from_str(&fs::read_to_string(fixtures().join("manifest.json")).unwrap())
            .unwrap();
    validate(&m).unwrap();
    let audio = m
        .assets
        .iter()
        .map(|a| {
            let b = unpack(&fixtures().join(&a.file)).unwrap();
            assert_eq!(digest(&b), a.sha256, "fixture hash {}", a.id);
            (a.id.clone(), wav(&b).unwrap())
        })
        .collect();
    (m, audio)
}
fn user_sample(c: &Case, audio: &BTreeMap<String, Audio>, sample: u64) -> f32 {
    c.segments
        .iter()
        .map(|s| {
            if sample < s.start_ms * RATE / 1000 || sample >= s.end_ms * RATE / 1000 {
                return 0.0;
            }
            if s.asset == "signal" {
                return if sample.is_multiple_of(2) {
                    0.1 * s.gain
                } else {
                    -0.1 * s.gain
                };
            }
            let a = &audio[&s.asset];
            let n =
                (sample - s.start_ms * RATE / 1000) * a.rate / RATE + s.offset_ms * a.rate / 1000;
            a.samples.get(n as usize).copied().unwrap_or(0.0) * s.gain
        })
        .sum()
}

fn level(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    let db = 20.0 * (sum / samples.len() as f64).sqrt().max(1e-9).log10();
    ((db + 60.0) / 50.0).clamp(0.0, 1.0) as f32
}
#[derive(Clone, Default, Debug)]
struct Trace {
    starts: Vec<u64>,
    stops: Vec<u64>,
    cuts: Vec<u64>,
    submits: Vec<u64>,
    rendered: Vec<bool>,
    user: Vec<bool>,
    rows: Vec<Value>,
}
fn row(t: &mut Trace, c: &Case, ms: u64, kind: &str, sentence: &str, extra: Value) {
    let id = format!("{}:{}", c.id, t.rows.len());
    let mut reference = json!({"host_life":format!("h0:{}",c.id),"fixture_id":c.id,"provenance_id":c.gold.provenance_id});
    if kind == "decision_made" {
        reference["event_type"] = json!("decision_made");
    }
    if let Some(object) = extra.as_object() {
        for (k, v) in object {
            reference[k] = v.clone();
        }
    }
    if let Some(id) = reference.get("playback_id").cloned() {
        reference["response_id"] = id;
    }
    t.rows.push(json!({"id":id,"lane":"host","kind":if kind=="decision_made" {"decision"} else {kind},"sentence":sentence,"clock":{"t_ms":ms},"ref":reference,"episode":c.id,"observed_at":0}));
}
fn quantile(values: &[f64], p: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);

    if p == 0.5 && v.len().is_multiple_of(2) {
        Some((v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0)
    } else {
        Some(v[((v.len() as f64 * p).ceil() as usize).max(1) - 1])
    }
}
fn wilson(n: usize, d: usize) -> Option<[f64; 2]> {
    if d == 0 {
        return None;
    }
    let p = n as f64 / d as f64;
    let z2 = 1.959963984540054_f64.powi(2);
    let den = 1.0 + z2 / d as f64;
    let center = (p + z2 / (2.0 * d as f64)) / den;
    let half = (z2 * (p * (1.0 - p) / d as f64 + z2 / (4.0 * (d as f64).powi(2)))).sqrt() / den;
    Some([(center - half).max(0.0), (center + half).min(1.0)])
}
fn proportion(n: usize, d: usize, limit: f64, lower_bound: bool) -> Value {
    let mut m = metric(rate(n, d), limit, lower_bound, d);
    m["count"] = json!(n);
    m["denominator"] = json!(d);
    m["ci95_wilson"] = json!(wilson(n, d));
    m
}
fn rate(n: usize, d: usize) -> Option<f64> {
    (d > 0).then(|| n as f64 / d as f64)
}
fn dead_silences(t: &Trace) -> usize {
    let mut run = 0;
    let mut count = 0;
    for (&user, &agent) in t.user.iter().zip(&t.rendered) {
        if user || agent {
            run = 0;
        } else {
            run += 1;
            if run == 40 {
                count += 1;
            }
        }
    }
    count
}
fn metric(value: Option<f64>, limit: f64, lower_bound: bool, n: usize) -> Value {
    let passed = value.map(|v| if lower_bound { v >= limit } else { v <= limit });
    json!({"value":value,"limit":limit,"comparison":if lower_bound {">="}else{"<="},"n":n,"status":match passed {Some(true)=>"PASS",Some(false)=>"FAIL",None=>"UNMEASURED"}})
}
fn score(runs: &[(&Case, Trace)]) -> Value {
    let mut gaps = Vec::new();
    let mut gap_missing = 0;
    let mut content = 0;
    let mut yields = 0;
    let mut delays = Vec::new();
    let mut bc = 0;
    let mut keeps = 0;
    let mut false_cuts = 0;
    let mut endpoints = 0;
    let mut floor_exchanges = 0;
    let mut premature = 0;
    let mut dead = 0;
    let mut playback_ms = 0;
    let mut duration_ms = 0;
    for (c, t) in runs {
        duration_ms += c.duration_ms;
        playback_ms += t.rendered.iter().filter(|x| **x).count() as u64 * 50;
        dead += dead_silences(t);
        if let Some(g) = &c.gold.overlap {
            let cut = t
                .cuts
                .iter()
                .copied()
                .find(|&at| at >= g.start_ms && at <= g.end_ms + 1500);
            if g.kind == "content" {
                content += 1;
                if let Some(at) = cut {
                    let delay = (at - g.start_ms) as f64;
                    delays.push(delay);
                    if delay <= 300.0 {
                        yields += 1;
                    }
                }
            } else {
                if cut.is_some() {
                    false_cuts += 1;
                }
                if g.kind == "backchannel" {
                    bc += 1;
                    let start = (g.start_ms / 50) as usize;
                    let end = g.end_ms.div_ceil(50) as usize;
                    if cut.is_none()
                        && t.rendered
                            .get(start..end)
                            .is_some_and(|frames| frames.iter().all(|x| *x))
                    {
                        keeps += 1;
                    }
                }
            }
        }
        if let Some(end) = c.gold.endpoint_ms {
            endpoints += 1;
            if c.gold.overlap.is_none() {
                floor_exchanges += 1;
            }
            let starts: Vec<_> = t
                .starts
                .iter()
                .copied()
                .filter(|&at| !(c.seed_reply && at == 0))
                .collect();
            if starts
                .iter()
                .any(|&at| at < end || c.gold.hold.iter().any(|p| at >= p[0] && at < p[1]))
            {
                premature += 1;
            }

            if let Some(start) = starts.iter().copied().find(|&at| at >= end) {
                gaps.push((start - end) as f64);
            } else {
                gap_missing += 1;
            }
        }
    }
    let gap50 = if gap_missing == 0 {
        quantile(&gaps, 0.5)
    } else {
        None
    };
    let gap90 = if gap_missing == 0 {
        quantile(&gaps, 0.9)
    } else {
        None
    };
    let metrics = json!({
        "yield_content_within_300ms":proportion(yields,content,0.9,true),
        "rendered_yield_ms_p50":metric(quantile(&delays,0.5),300.0,false,delays.len()),
        "rendered_yield_ms_p90":metric(quantile(&delays,0.9),600.0,false,delays.len()),
        "backchannel_keep_rate":proportion(keeps,bc,0.9,true),
        "response_gap_ms_median":metric(gap50,250.0,false,endpoints),
        "response_gap_ms_p90":metric(gap90,1000.0,false,endpoints),
        "premature_take_rate":proportion(premature,endpoints,0.05,false),
        "false_cut_per_playback_minute":{"value":if playback_ms>0 {Some(false_cuts as f64*60000.0/playback_ms as f64)} else {None},"n":false_cuts,"denominator_ms":playback_ms,"status":"OBSERVED_NO_ACCEPTED_THRESHOLD"},
        "dead_silence_per_minute":metric((duration_ms>0).then(||dead as f64*60000.0/duration_ms as f64),0.3,false,runs.len()),
        "repeated_reply_rate":{"value":null,"n":0,"status":"UNMEASURED_STUB_BRAIN"}
    });
    let fail = metrics
        .as_object()
        .unwrap()
        .values()
        .any(|m| m["status"] == "FAIL");
    let insufficient_corpus = floor_exchanges < 30 || content < 20 || bc < 20;
    let missing = insufficient_corpus
        || metrics
            .as_object()
            .unwrap()
            .values()
            .any(|m| m["status"] == "UNMEASURED")
        || gap_missing > 0;
    json!({"cases":runs.len(),"metrics":metrics,"coverage":{"floor_exchanges":floor_exchanges,"response_endpoints":endpoints,"content_cut_ins":content,"backchannels":bc,"minimum_floor_exchanges":30,"minimum_content_cut_ins":20,"minimum_backchannels":20,"sufficient":floor_exchanges>=30 && content>=20 && bc>=20},"missing_responses":gap_missing,"response_gaps_ms":gaps,"yield_delays_ms":delays,"yield_latency_missing":content-delays.len(),"verdict":if fail {"RED"}else if missing {"UNMEASURED"}else{"GREEN"},"exit_code":if fail {1}else if missing {2}else{0}})
}
fn report(m: &Manifest, runs: &[(&Case, Trace)]) -> Value {
    let mut splits = serde_json::Map::new();
    for language in ["en", "pt-BR"] {
        for split in ["full_duplex_bench", "public_audio_probe", "signal_control"] {
            let subset: Vec<_> = runs
                .iter()
                .filter(|(c, _)| c.language == language && c.split == split)
                .map(|(c, t)| (*c, t.clone()))
                .collect();
            splits.insert(format!("{language}/{split}"), score(&subset));
        }
    }
    let en: Vec<_> = runs
        .iter()
        .filter(|(c, _)| c.language == "en" && c.split != "signal_control")
        .map(|(c, t)| (*c, t.clone()))
        .collect();
    let gate = score(&en);
    let cases: Vec<_> = runs.iter().map(|(c,t)|json!({"fixture_id":c.id,"provenance_id":c.gold.provenance_id,"split":c.split,"language":c.language,"asr_provenance":if c.asr_provenance.is_empty(){"scripted test transcript, not measured ASR"}else{c.asr_provenance.as_str()},"render_starts_ms":t.starts,"render_stops_ms":t.stops,"cuts_ms":t.cuts,"submissions_ms":t.submits,"user_audio_frames":t.user.len(),"render_reference_frames":t.rendered.len()})).collect();
    json!({"schema":1,"candidate":std::env::var("UNFOLD_BENCH_CANDIDATE").unwrap_or_else(|_|"source-replay".into()),"verdict":gate["verdict"],"exit_code":gate["exit_code"],"qualification":if gate["coverage"]["sufficient"]==true {"REPLAY_ONLY"}else{"BLOCKED_INSUFFICIENT_CORPUS"},"english_gate":gate,"splits":splits,"fixture_splits":m.splits,"cases":cases,
        "models":{"stt":"upstream word annotations where available; externally supplied, approved-licence finals at declared audio times; per-case provenance; no ASR inference","turn":"imported product Input and Overlap rules, experimental floor model Off","tts":"stub PCM render, fixed 200 ms after submission, no TTS model","reply_llm":"stub recorded public reply text"},
        "limits":["Replay measures imported device-free seams, not the installed app.","Render samples are stub reference PCM; latencies are causal replay proxies, not device/acoustic measurements.","No acoustic AEC, VPIO 600 ms capture hold, real playout queue, network, live floor model or TTS jitter is exercised.","FLEURS ASR text and availability are scripted; full-file endpoint includes trailing silence.","Signal controls are not speech qualification and are excluded from the English gate.","Synthetic local-TTS and owner recording splits are empty; no semantic/acoustic model qualification is claimed.","A future adapter must explicitly call the new product seam; this baseline adapter cannot qualify H2/H3 wiring."],
        "fixture_case_count":m.cases.len(),"manifest_sha256":digest(&fs::read(fixtures().join("manifest.json")).unwrap()),
        "source_sha256":{"conversation_input.rs":digest(include_bytes!("../src/conversation_input.rs")),"conversation_state.rs":digest(include_bytes!("../src/conversation_state.rs")),"conversation_overlap.rs":digest(include_bytes!("../src/conversation_overlap.rs"))}})
}
fn replay_h2(c: &Case, audio: &BTreeMap<String, Audio>, knobs: crate::turn_detect::Knobs) -> Trace {
    use conversation_overlap::{Class, Overlap};
    use conversation_state::Conversation;
    let mut state = Conversation::default();
    state.start();
    let mut input = conversation_input::Input::semantic();
    static DETECTOR: std::sync::OnceLock<std::sync::Mutex<Option<crate::turn_detect::Detector>>> =
        std::sync::OnceLock::new();
    let mut shared = DETECTOR
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap();
    if shared.is_none() {
        *shared = Some(
            match std::env::var("UNFOLD_TURN_MODEL_DIR") {
                Ok(dir) => crate::turn_detect::Detector::from_dir(Path::new(&dir), knobs),
                Err(_) => crate::turn_detect::Detector::load(knobs),
            }
            .expect("CPU model load"),
        );
    }
    let detector = shared.as_mut().unwrap();
    detector.configure(knobs).unwrap();
    let mut overlap = Overlap::default();
    let mut trace = Trace::default();
    let mut scheduled: Option<u64> = None;
    let mut render_end: Option<u64> = None;
    let mut playback_id = String::from("seed");
    let mut asr = 0;
    let mut voiced = false;
    if c.seed_reply {
        state.endpoint("A seeded public test request.");
        state.submitted = true;
        state.speech_started();
        state.reply_done();
        render_end = Some(c.duration_ms);
        trace.starts.push(0);
        row(
            &mut trace,
            c,
            0,
            "playback",
            "stub render started",
            json!({"phase":"started","playback_id":"seed"}),
        );
    }
    row(
        &mut trace,
        c,
        0,
        "started",
        "device-free replay started",
        json!({}),
    );
    for sample in (0..c.duration_ms * RATE / 1000).step_by(FRAME as usize) {
        let now = sample * 1000 / RATE;
        if render_end.is_some_and(|end| now >= end) {
            render_end = None;
            state.speech_ended();
            trace.stops.push(now);
            row(
                &mut trace,
                c,
                now,
                "playback",
                "stub render finished",
                json!({"phase":"finished","playback_id":playback_id}),
            );
        }
        if scheduled.is_some_and(|at| now >= at) && !state.barged {
            scheduled = None;
            render_end = Some(now + c.reply_duration_ms);
            state.speech_started();
            state.reply_done();
            playback_id = format!("turn-{}", state.turn);
            trace.starts.push(now);
            row(
                &mut trace,
                c,
                now,
                "playback",
                "stub render started",
                json!({"phase":"started","playback_id":format!("turn-{}",state.turn)}),
            );
        }
        let block: Vec<_> = (sample..sample + FRAME)
            .map(|n| user_sample(c, audio, n))
            .collect();
        let playing = render_end.is_some();

        let reference: Vec<f32> = (sample..sample + FRAME)
            .map(|n| {
                if playing {
                    if n.is_multiple_of(2) {
                        0.1
                    } else {
                        -0.1
                    }
                } else {
                    0.0
                }
            })
            .collect();
        trace.rendered.push(reference.iter().any(|s| *s != 0.0));
        trace.user.push(block.iter().any(|s| s.abs() > 0.001));
        let meter = level(&block);
        let at = now + 50;
        let active_voice = meter >= conversation_input::VOICE_LEVEL;
        if active_voice != voiced {
            row(
                &mut trace,
                c,
                at,
                if active_voice {
                    "local_vad_onset"
                } else {
                    "local_vad_end"
                },
                "replay level threshold crossing",
                json!({"level":meter}),
            );
            voiced = active_voice;
        }
        let decisions = detector
            .push_with(&block, cached_probability)
            .expect("CPU model inference");
        let mut events = vec![(None, Some(meter))];
        while asr < c.asr.len() && c.asr[asr].at_ms <= at {
            events.push((Some(&c.asr[asr]), None));
            asr += 1;
        }
        let event_count = events.len();
        for (event_index, (words, meter)) in events.into_iter().enumerate() {
            let playing = render_end.is_some();

            let text = words.map(|w| {
                if playing {
                    conversation_overlap::filter_echo(&w.text, &c.reply_text, &c.reply_text)
                        .unwrap_or_else(|| w.text.clone())
                } else {
                    w.text.clone()
                }
            });
            let mut ready = None;
            if playing {
                let decision = if let Some(w) = words {
                    overlap.words(text.as_deref().unwrap(), w.final_words)
                } else {
                    overlap.level(meter.unwrap())
                };
                if let Some(d) = decision {
                    row(
                        &mut trace,
                        c,
                        at,
                        "decision_made",
                        &format!("decided to {}", d.class.decision()),
                        json!({"decision":d.class.decision(),"part":"overlap.rules","mode":"product-rules-replay"}),
                    );
                    if d.class == Class::Barge {
                        state.barge();
                        scheduled = None;
                        render_end = None;
                        trace.cuts.push(at);
                        trace.stops.push(at);
                        row(
                            &mut trace,
                            c,
                            at,
                            "playback",
                            "stub render cleared",
                            json!({"phase":"cleared","playback_id":playback_id}),
                        );
                        row(
                            &mut trace,
                            c,
                            at,
                            "speaker_cleared",
                            "stub render cleared",
                            json!({"phase":"cleared","playback_id":playback_id}),
                        );
                        ready = input.admit_overlap(&d.text, d.final_words, d.voice_ms, d.quiet_ms);
                    }
                }
            } else if let Some(w) = words {
                let _ = overlap.finish();
                if !overlap.discard_tail(text.as_deref().unwrap(), w.final_words) {
                    ready = input.words(text.as_deref().unwrap(), w.final_words, false);
                }
            } else {
                overlap.idle(meter.unwrap());
                input.level(meter.unwrap());
                ready = input.ready(false);
            }
            if event_index + 1 == event_count {
                for d in &decisions {
                    if matches!(
                        state.phase,
                        conversation_state::Phase::Listening
                            | conversation_state::Phase::Interrupted
                    ) && d.commit
                    {
                        ready = input.model_ready(true);
                    }
                    row(
                        &mut trace,
                        c,
                        at,
                        "decision_made",
                        "Smart Turn endpoint decision",
                        json!({
                        "part":"turn.detect","source":"smart-turn","probability":d.probability,
                        "S_ms":d.knobs.silence_ms,"T":d.knobs.threshold,"max_hold_ms":d.knobs.max_hold_ms,
                        "audio_start_ms":d.audio_start_ms,"audio_end_ms":d.audio_end_ms,
                        "quiet_ms":d.quiet_ms,"max_hold":d.max_hold,"latency_ns":d.latency_ns,"effective_threshold":d.effective_threshold,
                        "selected":if ready.is_some(){"TAKE_TURN"}else{"WAIT"}}),
                    );
                    if ready.is_some() {
                        detector.reset();
                    }
                }
            }
            if let Some(text) = ready {
                if state.endpoint(&text).is_some() {
                    state.submitted = true;
                    trace.submits.push(at);
                    scheduled = Some(at + c.reply_delay_ms);
                    row(
                        &mut trace,
                        c,
                        at,
                        "decision_made",
                        "decided to take_turn",
                        json!({"decision":"take_turn","part":"turn.detect","mode":"product-rules-replay"}),
                    );
                    row(
                        &mut trace,
                        c,
                        at,
                        "command_written",
                        "stub reply requested",
                        json!({"command_id":format!("turn-{}",state.turn)}),
                    );
                }
            }
        }
    }
    if render_end.is_some() {
        trace.stops.push(c.duration_ms);
        row(
            &mut trace,
            c,
            c.duration_ms,
            "playback",
            "stub render finished at fixture boundary",
            json!({"phase":"finished","playback_id":playback_id}),
        );
    }
    trace
}

fn dev(c: &Case) -> bool {

    let mut assets = c
        .segments
        .iter()
        .map(|s| s.asset.as_str())
        .collect::<Vec<_>>();
    assets.sort();
    assets.dedup();
    Sha256::digest(assets.join("|").as_bytes())[0] % 2 == 0
}
#[test]
fn turn_detect_scorecard() {
    let (m, audio) = load();
    let mut knobs = crate::turn_detect::Knobs::default();
    if let Ok(t) = std::env::var("UNFOLD_TURN_THRESHOLD") {
        knobs.threshold = t.parse().unwrap();
    }
    if let Ok(s) = std::env::var("UNFOLD_TURN_SILENCE_MS") {
        knobs.silence_ms = s.parse().unwrap();
    }
    if let Ok(s) = std::env::var("UNFOLD_TURN_MAX_HOLD_MS") {
        knobs.max_hold_ms = s.parse().unwrap();
    }
    if let Ok(r) = std::env::var("UNFOLD_TURN_RETRY_MS") {
        knobs.retry_ms = r.parse().unwrap();
    }
    if let Ok(f) = std::env::var("UNFOLD_TURN_FLOOR") {
        knobs.threshold_floor = Some(f.parse().unwrap());
    }
    let mode = std::env::var("UNFOLD_TURN_SPLIT").unwrap_or_else(|_| "dev".into());
    assert!(mode == "dev" || mode == "test" || mode == "all");
    let runs: Vec<_> = m
        .cases
        .iter()
        .filter(|c| mode == "all" || (dev(c) == (mode == "dev")))
        .map(|c| (c, replay_h2(c, &audio, knobs)))
        .collect();
    let mut result = report(&m, &runs);
    result["part"] = json!("turn.detect");
    result["evaluation_split"] = json!(mode);
    result["knobs"] = json!({"S_ms":knobs.silence_ms,"T":knobs.threshold,"max_hold_ms":knobs.max_hold_ms,"retry_ms":knobs.retry_ms,"threshold_floor":knobs.threshold_floor});
    result["models"]["turn"]=json!("real CPU Silero VAD + pinned Smart Turn v3.2 fp32, imported product Detector and Input::semantic");
    result["limits"]=json!(["Device-free replay: scripted/annotated ASR, stub render, no AEC/device/network/TTS qualification.","Overlap remains interim; this detector does not qualify full conversation behavior.","Split grouped by source asset IDs with SHA256 parity, independent of labels/scores. PT is sparse."]);
    let mut endpoint = serde_json::Map::new();
    for lang in ["en", "pt-BR"] {
        let subset: Vec<_> = runs
            .iter()
            .filter(|(c, _)| c.language == lang && c.split != "signal_control")
            .map(|(c, t)| (*c, t.clone()))
            .collect();
        let metrics = score(&subset);
        let gap = &metrics["metrics"]["response_gap_ms_p90"];
        let premature = &metrics["metrics"]["premature_take_rate"];
        let exit = if gap["status"] == "FAIL" || premature["status"] == "FAIL" {
            1
        } else if gap["status"] == "UNMEASURED" || premature["status"] == "UNMEASURED" {
            2
        } else {
            0
        };
        endpoint.insert(lang.into(),json!({"exit_code":exit,"cases":subset.len(),"gap_p90":gap,"gap_median":metrics["metrics"]["response_gap_ms_median"],"premature":premature,"missing_responses":metrics["missing_responses"]}));
    }
    result["endpoint_gate"] = json!(endpoint);
    result["exit_code"] = result["endpoint_gate"]["en"]["exit_code"].clone();
    if let Ok(path) = std::env::var("UNFOLD_BENCH_RESULT") {
        fs::write(path, serde_json::to_string_pretty(&result).unwrap() + "\n").unwrap();
    }
    if let Ok(path) = std::env::var("UNFOLD_BENCH_TRAJECTORY") {
        let rows = runs
            .iter()
            .flat_map(|(_, t)| t.rows.iter())
            .map(|r| serde_json::to_string(r).unwrap() + "\n")
            .collect::<String>();
        fs::write(path, rows).unwrap();
    }
    println!("turn.detect {}", result["endpoint_gate"]);
    assert_eq!(
        result["exit_code"], 0,
        "turn.detect measured RED=1, unavailable=2; see receipt"
    );
}

static PROBABILITY_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, f32>>,
> = std::sync::OnceLock::new();
static CACHE_HITS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CACHE_MISSES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn cached_probability(model: &mut ort::session::Session, audio: &[f32]) -> Result<f32, String> {
    if std::env::var_os("UNFOLD_TURN_CACHE").is_none()
        || std::env::var_os("UNFOLD_TURN_MODEL_DIR").is_some()
    {
        return crate::turn_detect::Detector::infer(model, audio);
    }
    let cache = PROBABILITY_CACHE.get_or_init(|| {
        let mut cache = std::collections::HashMap::new();
        if let Ok(path) = std::env::var("UNFOLD_TURN_CACHE") {
            if let Ok(text) = fs::read_to_string(path) {
                for line in text.lines() {
                    let row: Value = serde_json::from_str(line).expect("valid inference cache row");
                    let p = row["p"].as_f64().unwrap() as f32;
                    assert!(p.is_finite() && (0.0..=1.0).contains(&p));
                    cache.insert(row["key"].as_str().unwrap().to_string(), p);
                }
            }
        }
        std::sync::Mutex::new(cache)
    });
    let mut h = Sha256::new();
    h.update(include_bytes!("../turn-models/pins.json"));
    h.update(include_bytes!("../src/turn_features.rs"));
    h.update((audio.len() as u64).to_le_bytes());
    if cfg!(target_endian = "little") {

        let bytes = unsafe {
            std::slice::from_raw_parts(audio.as_ptr().cast::<u8>(), std::mem::size_of_val(audio))
        };
        h.update(bytes);
    } else {
        for x in audio {
            h.update(x.to_le_bytes());
        }
    }
    let key = format!("{:x}", h.finalize());
    let mut cache = cache.lock().unwrap();
    if let Some(p) = cache.get(&key) {
        CACHE_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Ok(*p);
    }
    let p = crate::turn_detect::Detector::infer(model, audio)?;
    if !p.is_finite() || !(0.0..=1.0).contains(&p) {
        return Err("Invalid inference cache probability".into());
    }
    cache.insert(key.clone(), p);
    CACHE_MISSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::var("UNFOLD_TURN_CACHE").unwrap())
        .map_err(|e| e.to_string())?;
    let line = format!("{}\n", json!({"key":key,"p":p}));
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(p)
}
#[test]
#[ignore = "explicit CPU frontier run on development audio only"]
fn frontier_dev_grid() {
    let (m, audio) = load();
    let selected: u64 = std::env::var("UNFOLD_FRONTIER_S")
        .expect("S shard")
        .parse()
        .unwrap();
    assert!([160, 240, 320, 512].contains(&selected));
    let path = std::env::var("UNFOLD_FRONTIER_OUT").expect("frontier output");
    let mut out = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .expect("new frontier result");
    use std::io::Write;
    for policy in ["fixed", "decay"] {
        for max_hold in [1200, 1600, 2400] {
            for threshold in [0.3_f32, 0.4, 0.5, 0.6, 0.7] {
                let knobs = crate::turn_detect::Knobs {
                    silence_ms: selected,
                    threshold,
                    max_hold_ms: max_hold,
                    retry_ms: if policy == "decay" { 100 } else { 256 },
                    threshold_floor: if policy == "decay" { Some(0.2) } else { None },
                };
                let runs: Vec<_> = m
                    .cases
                    .iter()
                    .filter(|c| dev(c) && c.language == "en" && c.split != "signal_control")
                    .map(|c| (c, replay_h2(c, &audio, knobs)))
                    .collect();
                let metrics = score(&runs);
                let v = &metrics["metrics"];
                let takes: Vec<_> = runs
                    .iter()
                    .flat_map(|(_, t)| t.rows.iter())
                    .filter(|r| {
                        r["ref"]["source"] == "smart-turn" && r["ref"]["selected"] == "TAKE_TURN"
                    })
                    .collect();
                let safety = takes
                    .iter()
                    .filter(|r| r["ref"]["max_hold"] == true)
                    .count();
                let row = json!({"policy":policy,"S_ms":selected,"T":threshold,"floor":knobs.threshold_floor,"max_hold_ms":max_hold,"retry_ms":knobs.retry_ms,"cases":runs.len(),"premature":v["premature_take_rate"],"gap_median_ms":v["response_gap_ms_median"]["value"],"gap_p90_ms":v["response_gap_ms_p90"]["value"],"response_gaps_ms":metrics["response_gaps_ms"],"missing_responses":metrics["missing_responses"],"safety_takes":safety,"takes":takes.len(),"cache_hits":CACHE_HITS.load(std::sync::atomic::Ordering::Relaxed),"cache_misses":CACHE_MISSES.load(std::sync::atomic::Ordering::Relaxed)});
                writeln!(out, "{row}").unwrap();
                out.flush().unwrap();
                println!("frontier {row}");
            }
        }
    }
}

#[test]
#[ignore = "explicit real CPU cache equivalence proof"]
fn probability_cache_matches_direct_inference() {
    assert!(std::env::var_os("UNFOLD_TURN_CACHE").is_some());
    let dir = crate::turn_detect::model_dir(&std::env::current_exe().unwrap()).unwrap();
    let mut model =
        crate::onnx_cpu::session(&fs::read(dir.join("smart-turn-v3.2-cpu.onnx")).unwrap()).unwrap();
    let mut audio: Vec<f32> = (0..8000).map(|i| (i as f32 * 0.081).sin() * 0.1).collect();
    let direct = crate::turn_detect::Detector::infer(&mut model, &audio).unwrap();
    assert!((cached_probability(&mut model, &audio).unwrap() - direct).abs() < 1e-6);
    assert!((cached_probability(&mut model, &audio).unwrap() - direct).abs() < 1e-6);
    audio[100] = 0.25;
    let changed = crate::turn_detect::Detector::infer(&mut model, &audio).unwrap();
    assert!((cached_probability(&mut model, &audio).unwrap() - changed).abs() < 1e-6);
    assert_eq!(CACHE_MISSES.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert_eq!(CACHE_HITS.load(std::sync::atomic::Ordering::Relaxed), 1);
}
