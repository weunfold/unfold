

use rth_bench::fixture::Split;
use rth_bench::run::{RunOptions, rescore, run};
use rth_bench::{default_agent_dir, default_bench_dir};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "boots rth-bench-vm; run explicitly"]
async fn dev_split_runs_end_to_end_and_rescoring_agrees() {
    let results = std::env::temp_dir().join(format!("rth-bench-e2e-{}", std::process::id()));
    let opts = RunOptions {
        bench_dir: default_bench_dir(),
        agent_dir: default_agent_dir().canonicalize().expect("build rth-agentd for aarch64-unknown-linux-musl first"),
        results_dir: results.clone(),
        suite_id: "text-contract-v0".into(),
        splits: vec![Split::Dev],
        keep_vm: false,
        agent: None,
        journal: None,
        state_dir: None,
    };
    let out = run(&opts).await.expect("run completes");
    let card = &out.scorecard;
    for f in ["manifest.json", "scorecard.json", "case-results.json", "harness.jsonl"] {
        assert!(out.run_dir.join(f).exists(), "{f} missing");
    }
    assert_eq!(std::fs::read_dir(out.run_dir.join("cases/dev")).unwrap().count(), 12);

    assert!(card["statuses"]["invalid"].as_array().unwrap().is_empty(), "{:#}", card["statuses"]);
    let eligibility = card["eligibility"].as_array().unwrap();
    let rule = |name: &str| eligibility.iter().find(|r| r["rule"] == name).unwrap()["pass"].as_bool().unwrap();
    assert!(rule("guest_runs_hashed_agentd") && rule("agent_protocol_matches") && rule("run_completed"));
    let again = rescore(&out.run_dir, &opts.bench_dir).unwrap();
    assert_eq!(again["matches_stored_verdicts"], true, "{again:#}");
}
