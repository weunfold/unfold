

use rth_voice::echo::EchoConfig;
use rth_voice::sim::matrix::{self, pct};
use rth_voice::sim::Policy;
use rth_voice::vad::VadConfig;

fn main() {
    let dir = std::env::var_os("UNFOLD_VOICE_FIXTURES_DIR")
        .map(std::path::PathBuf::from)
        .expect("set UNFOLD_VOICE_FIXTURES_DIR; generate fixtures with sh crates/rth/testdata/voice-fixtures/make.sh");
    let f = matrix::load(&dir).expect("voice fixtures from UNFOLD_VOICE_FIXTURES_DIR");
    let levels = [-25.0, -35.0];
    let (e, g) = (Policy::LocalEnergy(VadConfig::default()), Policy::Gated(EchoConfig::default()));
    let cases: Vec<(Policy, f32, bool)> = vec![
        (e, 0.0, false),
        (g, 0.0, false),
        (g, 0.0, true),
        (e, 25.0, false),
        (e, 15.0, false),
        (g, 25.0, true),
        (Policy::ProviderOnly(VadConfig::default()), 25.0, false),
        (Policy::ProviderOnly(VadConfig::default()), 0.0, false),
    ];
    let mut rows = Vec::new();
    for (p, erle, oracle) in cases {
        let r = matrix::row(&f, p, erle, &levels, oracle);
        rows.push(serde_like(&r));
    }
    let near: Vec<String> = levels.iter().map(|&l| { let (s, n) = matrix::near_only(&f, l); format!("{{\"level_dbfs\": {l}, \"local_vad_saw\": {s}, \"of\": {n}}}") }).collect();
    println!(
        "{{\n  \"fixtures\": \"UNFOLD_VOICE_FIXTURES_DIR (synthetic voice fixtures)\",\n  \"echo_grid\": {{\"delay_ms\": {:?}, \"gain_db\": {:?}, \"tail_ms\": {:?}}},\n  \"far_end_rms_dbfs\": -20, \"interruption_at_ms_into_reply\": 6000,\n  \"gate\": \"{}\",\n  \"rows\": [\n{}\n  ],\n  \"near_end_only\": [{}]\n}}",
        matrix::DELAYS, matrix::GAINS, matrix::TAILS, EchoConfig::default().describe(), rows.join(",\n"), near.join(", ")
    );
}

fn serde_like(r: &matrix::Row) -> String {
    let opt = |v: Option<u32>| v.map_or("null".to_string(), |x| x.to_string());
    let lv: Vec<String> = r.by_level.iter().map(|(l, d, t)| format!("{{\"level_dbfs\": {l}, \"detected\": {d}, \"of\": {t}}}")).collect();
    format!(
        "    {{\"policy\": \"{}\", \"mic\": \"{}\", \"echo_only_runs\": {}, \"echo_only_false_cuts\": {}, \"noise_false_cuts\": {}, \"false_cuts_per_playback_min\": {:.2}, \"playback_min\": {:.2}, \"first_false_cut_ms_p50\": {}, \"interruptions_detected\": {}, \"interruptions_missed\": {}, \"by_level\": [{}], \"onset_to_decision_ms_p50\": {}, \"onset_to_decision_ms_p95\": {}, \"onset_to_last_device_sample_ms_p50\": {}, \"onset_to_last_device_sample_ms_max\": {}, \"local_onsets_unexplained_echo_only\": {}}}",
        r.policy, r.mic, r.echo_runs, r.false_cuts, r.noise_false_cuts, r.false_cuts_per_min(), r.playback_min + r.noise_playback_min,
        opt(pct(&r.first_false_cut_ms, 0.5)), r.detected, r.missed, lv.join(", "),
        opt(pct(&r.decision_ms, 0.5)), opt(pct(&r.decision_ms, 0.95)), opt(pct(&r.to_last_sample_ms, 0.5)), opt(pct(&r.to_last_sample_ms, 1.0)), r.local_onsets_unexplained
    )
}
