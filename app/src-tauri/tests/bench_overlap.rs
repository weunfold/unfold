
#[path = "../src/bc_detector.rs"]
#[allow(dead_code)]
mod bc_detector;
#[path = "../src/bc_playout.rs"]
#[allow(dead_code)]
mod bc_playout;
#[path = "../src/conversation_input.rs"]
#[allow(dead_code)]
mod conversation_input;
#[allow(dead_code)]
#[path = "../src/conversation_overlap.rs"]
mod conversation_overlap;
#[path = "../src/onnx_cpu.rs"]
mod onnx_cpu;

#[test]
fn python_rust_ort_parity_includes_every_cache_and_reset() {
    use bc_detector::Model;
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/bc-detector/parity.json")).unwrap();
    let mut model = Model::load().unwrap();
    for step in fixture["steps"].as_array().unwrap() {
        let pcm: Vec<f32> = serde_json::from_value(step["chunk"].clone()).unwrap();
        let bot: Vec<f32> = serde_json::from_value(step["bot_state"].clone()).unwrap();
        let prediction = model
            .infer(
                &pcm,
                [bot[..8].try_into().unwrap(), bot[8..].try_into().unwrap()],
            )
            .unwrap();
        let mut actual = std::collections::BTreeMap::new();
        actual.insert(
            "intent".to_string(),
            prediction.intent.into_iter().flatten().collect::<Vec<_>>(),
        );
        actual.insert("vad".to_string(), prediction.vad.to_vec());
        actual.insert(
            "vap".to_string(),
            prediction.vap.into_iter().flatten().collect::<Vec<_>>(),
        );
        for cache in model.caches() {
            actual.insert(format!("{}_out", cache.name), cache.values.clone());
        }
        for (name, values) in actual {
            let expected: Vec<f32> =
                serde_json::from_value(step["outputs"][&name]["values"].clone()).unwrap();
            assert_eq!(expected.len(), values.len());
            let error = values
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0_f32, f32::max);
            assert!(error < 0.0002, "{name} parity max_abs_error={error}");
        }
    }
    model.reset();
    assert!(model
        .caches()
        .iter()
        .all(|c| c.values.iter().all(|v| *v == 0.)));
    assert!(model.infer(&[f32::NAN; 640], [[0.; 8]; 2]).is_err());
    assert!(Model::from_bytes(b"wrong model").is_err());
}

#[path = "support/overlap_card.rs"]
mod overlap_card;

#[test]
#[ignore = "explicit CPU inference timing receipt"]
fn streaming_cpu_cost_and_load_time() {
    use std::time::Instant;
    let started = Instant::now();
    let mut model = bc_detector::Model::load().unwrap();
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    let pcm: Vec<f32> = (0..640).map(|i| (i as f32 * 0.04).sin() * 0.15).collect();
    for _ in 0..50 {
        model.infer(&pcm, [[0.; 8]; 2]).unwrap();
    }
    let mut ms = Vec::with_capacity(250);
    for _ in 0..250 {
        let at = Instant::now();
        model.infer(&pcm, [[0.; 8]; 2]).unwrap();
        ms.push(at.elapsed().as_secs_f64() * 1000.);
    }
    ms.sort_by(f64::total_cmp);
    let receipt = serde_json::json!({"provider":"CPUExecutionProvider","threads":{"intra":1,"inter":1},"model_sha256":bc_detector::MODEL_SHA256,"chunk_ms":40,"warmup_calls":50,"n":250,"load_ms":load_ms,"p50_ms":ms[124],"p99_ms":ms[247],"max_ms":ms[249],"limits":"synthetic sine PCM; wall time under nice19, not a simultaneous ASR or physical audio measurement"});
    std::fs::write(
        std::env::var("RTH_OVERLAP_CPU_OUTPUT").expect("receipt path required"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
