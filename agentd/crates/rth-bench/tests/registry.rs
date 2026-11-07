

use rth_bench::fixture::Split;
use rth_bench::{default_bench_dir, registry};

#[test]
fn text_contract_v0_registry_verifies() {
    let reg = registry::load(&default_bench_dir(), "text-contract-v0").expect("registry verifies");
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
fn a_tampered_fixture_is_refused() {
    let src = default_bench_dir();
    let dir = std::env::temp_dir().join(format!("rth-bench-tamper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&src, &dir, &["results"]);
    let victim = dir.join("fixtures/dev/tc-dev-01-greeting.json");
    let text = std::fs::read_to_string(&victim).unwrap().replace("\"hi\"", "\"hi!\"");
    std::fs::write(&victim, text).unwrap();
    let err = registry::load(&dir, "text-contract-v0").err().expect("tampered fixture must fail").to_string();
    assert!(err.contains("content hash"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path, skip: &[&str]) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        if src.is_dir() {
            copy_dir(&src, &dst, &[]);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}
