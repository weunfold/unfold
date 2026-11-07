
use crate::{
    bc_detector::{Action, Stream, CHUNK},
    bc_playout::Snapshot,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Read, path::Path};
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn integer(v: &Value, k: &str) -> usize {
    v[k].as_u64().unwrap() as usize
}
fn waveform(root: &Path, asset: &Value) -> (usize, Vec<f32>) {
    let text = fs::read_to_string(root.join(asset["file"].as_str().unwrap())).unwrap();
    let hex: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let zipped: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut raw = vec![];
    flate2::read::GzDecoder::new(&zipped[..])
        .take(8 * 1024 * 1024)
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(sha(&raw), asset["sha256"].as_str().unwrap());
    assert_eq!(&raw[..4], b"RIFF");
    assert_eq!(&raw[8..12], b"WAVE");
    let word = |at| u16::from_le_bytes(raw[at..at + 2].try_into().unwrap()) as usize;
    let long = |at| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap()) as usize;
    let (mut channels, mut rate, mut data) = (0, 0, &[][..]);
    let mut at = 12;
    while at + 8 <= raw.len() {
        let n = long(at + 4);
        let begin = at + 8;
        assert!(begin + n <= raw.len());
        match &raw[at..at + 4] {
            b"fmt " => {
                assert_eq!(word(begin), 1);
                channels = word(begin + 2);
                rate = long(begin + 4);
                assert_eq!(word(begin + 14), 16);
            }
            b"data" => data = &raw[begin..begin + n],
            _ => {}
        }
        at = begin + n + (n % 2);
    }
    assert!(channels > 0 && rate > 0);
    let samples = data
        .chunks_exact(channels * 2)
        .map(|frame| {
            frame
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes(b.try_into().unwrap()) as f32 / 32768.)
                .sum::<f32>()
                / channels as f32
        })
        .collect();
    (rate, samples)
}
fn pcm(root: &Path, m: &Value, case: &Value) -> Vec<f32> {
    let mut out = vec![0.; integer(case, "duration_ms") * 16];
    for segment in case["segments"].as_array().unwrap() {
        let (start, end, gain) = (
            integer(segment, "start_ms") * 16,
            integer(segment, "end_ms") * 16,
            segment["gain"].as_f64().unwrap() as f32,
        );
        let id = segment["asset"].as_str().unwrap();
        if id == "signal" {
            for (i, s) in out.iter_mut().enumerate().take(end).skip(start) {
                *s += if i % 2 == 0 { 0.1 * gain } else { -0.1 * gain };
            }
            continue;
        }
        let asset = m["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap();
        let (rate, source) = waveform(root, asset);
        for (i, s) in out.iter_mut().enumerate().take(end).skip(start) {
            if let Some(sample) =
                source.get((i - start) * rate / 16000 + integer(segment, "offset_ms") * rate / 1000)
            {
                *s += sample * gain;
            }
        }
    }
    out
}
fn offline(record: &Value) -> Option<(&'static str, f64)> {
    let pause = record["pause_ms"].as_f64()?;
    let (mut streak, mut peak) = (0, 0_f64);
    for row in record["rows"].as_array().unwrap() {
        if row["at_ms"].as_f64().unwrap() < pause {
            continue;
        }
        let floor = row["floor"].as_f64().unwrap();
        peak = peak.max(row["backchannel"].as_f64().unwrap());
        streak = if floor >= 0.7 && row["vad"].as_f64().unwrap() >= 0.5 {
            streak + 1
        } else {
            0
        };
        if streak >= 2 {
            return Some(("yield", row["at_ms"].as_f64().unwrap()));
        }
        if row["quiet_ms"].as_f64().unwrap() >= 200. && peak >= 0.7 {
            return Some(("resume", row["at_ms"].as_f64().unwrap()));
        }
    }
    None
}
#[test]
#[ignore = "external licensed conversation corpus; run explicitly with fixture, split, offline and output paths"]
fn integrated_conversation_overlap_card() {
    let root =
        std::env::var("RTH_OVERLAP_FIXTURES").expect("external conversation fixture directory");
    let root = Path::new(&root);
    let manifest = fs::read(root.join("manifest.json")).unwrap();
    let m: Value = serde_json::from_slice(&manifest).unwrap();
    let split = fs::read(std::env::var("RTH_OVERLAP_SPLIT").unwrap()).unwrap();
    assert_eq!(
        sha(&split),
        "1c4ff6c69f0677ce951ca9d1760a996e90e6939be6551d6df63a250714f1bf14"
    );
    let split: Value = serde_json::from_slice(&split).unwrap();
    assert_eq!(sha(&manifest), split["manifest_sha256"].as_str().unwrap());
    let reference: Value =
        serde_json::from_slice(&fs::read(std::env::var("RTH_OVERLAP_OFFLINE").unwrap()).unwrap())
            .unwrap();
    let mut rows = vec![];
    let mut counts: BTreeMap<String, [u64; 5]> = BTreeMap::new();
    let mut differences = vec![];
    for item in split["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["partition"] != "dev")
    {
        let case = m["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == item["id"])
            .unwrap();
        let audio = pcm(root, &m, case);
        let mut stream = Stream::load().unwrap();
        stream.configure(1, 1);
        let mut render = Snapshot::default();
        let (mut held, mut pause, mut final_action, mut final_ms) = (false, None, None, None);
        for chunk in audio.chunks(CHUNK) {
            let mut padded = [0.; CHUNK];
            padded[..chunk.len()].copy_from_slice(chunk);
            render.advance(CHUNK * 3, if held { 0 } else { CHUNK * 3 }, true);
            for d in stream.push(&padded, render, held).unwrap() {
                match d.action {
                    Action::Speech => {}
                    Action::Hold => {
                        held = true;
                        pause = Some(d.end_sample as f64 / 16.);
                    }
                    Action::Yield => {
                        final_action = Some("yield");
                        final_ms = Some(d.end_sample as f64 / 16.);
                    }
                    Action::Resume | Action::Noise => {
                        held = false;
                        final_action = Some("resume");
                        final_ms = Some(d.end_sample as f64 / 16.);
                    }
                }
            }
            if final_action.is_some() {
                break;
            }
        }
        let onset = case["gold"]["overlap"]["start_ms"].as_f64().unwrap();
        let end = case["gold"]["overlap"]["end_ms"].as_f64().unwrap();
        let valid = final_ms.is_some_and(|at| {
            at >= onset && at <= (end + 1500.).min(case["duration_ms"].as_f64().unwrap())
        });
        let correct = valid
            && final_action
                == Some(if item["kind"] == "content" {
                    "yield"
                } else {
                    "resume"
                });
        let key = format!(
            "{}:{}:{}",
            item["partition"].as_str().unwrap(),
            item["language"].as_str().unwrap(),
            item["kind"].as_str().unwrap()
        );
        let count = counts.entry(key).or_default();
        count[0] += 1;
        count[1] += u64::from(correct);
        count[2] += u64::from(pause.is_some_and(|at| at >= onset && at <= onset + 300.));
        count[3] += u64::from(valid);
        count[4] += u64::from(final_action == Some("resume"));
        if item["partition"] == "heldout" {
            let r = reference["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == item["id"])
                .unwrap();
            let expected = offline(r);
            if expected.map(|x| x.0) != final_action
                || expected
                    .zip(final_ms)
                    .is_some_and(|(e, at)| (e.1 - at).abs() > 40.)
            {
                differences.push(item["id"].clone());
            }
        }
        rows.push(json!({"id":item["id"],"partition":item["partition"],"language":item["language"],"kind":item["kind"],"pause_ms":pause,"final_action":final_action,"final_ms":final_ms,"correct":correct,"resume_from_offset_ms":if final_action==Some("resume") {final_ms.map(|t|t-end)}else{None}}));
    }
    fs::write(std::env::var("RTH_OVERLAP_OUTPUT").unwrap(),serde_json::to_vec_pretty(&json!({"schema":1,"contract":"card-contract, not reference-matched","count_columns":["n","correct","pause300","valid_terminal","resume"],"counts":counts,"offline_differences":differences,"cases":rows})).unwrap()).unwrap();
    if std::env::var_os("RTH_OVERLAP_UPDATED_POLICY").is_none() {
        assert!(
            differences.is_empty(),
            "integrated/offline differences: {differences:?}"
        );
        assert_eq!(counts["heldout:en:content"][1], 15);
        assert_eq!(counts["heldout:en:backchannel"][1], 8);
    }
}
