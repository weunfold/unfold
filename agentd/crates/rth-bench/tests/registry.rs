

use rth_bench::fixture::Split;
use rth_bench::registry;

#[test]
#[ignore = "needs the text-contract bench tree: set UNFOLD_BENCH_DIR"]
fn text_contract_v0_registry_verifies() {
    let reg = registry::load(&bench_dir(), "text-contract-v0").expect("registry verifies");
    assert_eq!(reg.suite.split_manifest.dev.len(), 12);
    assert_eq!(reg.suite.split_manifest.promotion.len(), 12);
    assert_eq!(reg.suite.probes[0].repeats, 30);
    for f in reg.scenarios(Split::Promotion) {
        assert!(f.path.starts_with(reg.root.join("private")), "{} must live under bench/private", f.path.display());
    }
    for f in reg.scenarios(Split::Dev) {
        assert!(!f.path.starts_with(reg.root.join("private")));
    }

    for t in &reg.suite.tasks {
        for split in [Split::Dev, Split::Promotion] {
            assert!(reg.scenarios(split).iter().any(|f| f.fixture.task == t.task), "{} has no {split:?} case", t.task);
        }
    }
}

#[test]
#[ignore = "needs the text-contract bench tree: set UNFOLD_BENCH_DIR"]
fn a_tampered_fixture_is_refused() {
    let src = bench_dir();
    let dir = std::env::temp_dir().join(format!("rth-bench-tamper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_registry_inputs(&src, &dir);
    let victim = dir.join("fixtures/dev/tc-dev-01-greeting.json");
    let text = std::fs::read_to_string(&victim).unwrap().replace("\"hi\"", "\"hi!\"");
    std::fs::write(&victim, text).unwrap();
    let err = registry::load(&dir, "text-contract-v0").err().expect("tampered fixture must fail").to_string();
    assert!(err.contains("content hash"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn bench_dir() -> std::path::PathBuf {
    std::env::var_os("UNFOLD_BENCH_DIR")
        .map(std::path::PathBuf::from)
        .expect("set UNFOLD_BENCH_DIR to the text-contract benchmark tree")
}

fn copy_registry_inputs(from: &std::path::Path, to: &std::path::Path) {
    let reg = registry::load(from, "text-contract-v0").expect("source registry verifies before tampering");
    // Copy the verified dependency closure, excluding unrelated benchmark data and outputs.
    for source in std::iter::once(&reg.suite_path)
        .chain(reg.tasks.values().map(|task| &task.path))
        .chain(reg.fixtures.values().map(|fixture| &fixture.path))
    {
        let relative = source.strip_prefix(&reg.root).expect("registry input is under its root");
        let relative = relative.components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
            .then_some(relative)
            .expect("registry input must be a plain path under its root");
        let target = to.join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(source, target).unwrap();
    }
}
