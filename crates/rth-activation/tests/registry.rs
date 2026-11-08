use rth_activation::activation::Reconciled;
use rth_activation::product::{BrainRegistry, Pinned};
use rth_activation::registry::{database_name_for, ListRegistry};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FAKE_BUN: &str = "#!/bin/sh\nd=$(dirname \"$0\")\nprintf '%s\\n' \"BRAIN_ROOT=$BRAIN_ROOT $*\" >> \"$d/argv.log\"\ncat \"$d/out.jsonl\"\nexit $(cat \"$d/exit\")\n";

fn setup(name: &str) -> (PathBuf, ListRegistry, Value) {
    use std::os::unix::fs::PermissionsExt;
    let d = std::env::temp_dir().join(format!("rth-registry-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("bun"), FAKE_BUN).unwrap();
    std::fs::set_permissions(d.join("bun"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(d.join("registry-list.ts"), b"// synthetic registry reader\n").unwrap();
    let sha = |p: &Path| rth_activation::boot::sha256_hex(&std::fs::read(p).unwrap());
    let reg = ListRegistry {
        bun: Pinned { path: d.join("bun"), sha256: sha(&d.join("bun")) },
        list: Pinned { path: d.join("registry-list.ts"), sha256: sha(&d.join("registry-list.ts")) },
        brain_root: "/Applications/RTH.app/Contents/Resources/brain-seam/src".into(),
        create: None,
        interim_db_sha256: String::new(),
        socket: "/fixture/rt/pgserve".into(),
        port: 5999,
        home: "/Users/fixture".into(),
    };
    let home = d.join("brain-home");
    std::fs::create_dir_all(home.join(".brain")).unwrap();
    let id = "inst-cc1d81f5a91ee1d96328cdae5c4eb8ab";
    let tuple = json!({
        "name": format!("rth-interim-{id}"), "owner": format!("agent:rth-interim-{id}"), "lifecycle": "permanent",
        "control_db": "brain_rth_interim_cc1d81f5a91ee1d96328cdae5c4eb8ab_ctl", "memory_db": database_name_for(&format!("rth-interim-{id}")),
        "home": home
    });
    std::fs::write(home.join(".brain/config.json"), json!({"database": tuple["control_db"], "brainSlug": tuple["name"]}).to_string()).unwrap();
    (d, reg, tuple)
}

fn row(t: &Value) -> Value {
    json!({"control_db": t["control_db"], "id": "brn_abc123", "short_name": t["name"], "name": t["name"], "owner": t["owner"], "lifecycle": "permanent", "archive_ttl": null, "archived_at": null, "created_at": "2026-10-23", "home_path": "", "memory_db": t["memory_db"], "memory_db_checked": true, "memory_db_exists": true, "memory_db_holds_id": true})
}

fn replay(d: &Path, rows: &[Value], code: i32) {
    let mut out: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
    out.push(json!({"summary": true, "rows": rows.len()}).to_string());
    std::fs::write(d.join("out.jsonl"), out.join("\n") + "\n").unwrap();
    std::fs::write(d.join("exit"), code.to_string()).unwrap();
}

#[test]
fn the_exact_tuple_is_queried_and_only_one_fully_matching_row_is_adopted() {
    let (d, reg, t) = setup("adopt");
    replay(&d, &[row(&t)], 0);
    assert_eq!(reg.lookup(&t), Reconciled::Done(json!({"brain_id": "brn_abc123"})));
    let argv = std::fs::read_to_string(d.join("argv.log")).unwrap();
    assert!(argv.contains(&format!("{} --name {} --owner {} --socket /fixture/rt/pgserve --port 5999", t["control_db"].as_str().unwrap(), t["name"].as_str().unwrap(), t["owner"].as_str().unwrap())), "the validated service's socket: {argv}");
    assert!(argv.starts_with("BRAIN_ROOT=/Applications/RTH.app/Contents/Resources/brain-seam/src "), "the bundled Brain source: {argv}");
    replay(&d, &[], 0);
    assert_eq!(reg.lookup(&t), Reconciled::NotDone, "zero rows: not registered");
}

#[test]
fn uncertain_refused_multiple_or_foreign_results_are_unknown() {
    let (d, reg, t) = setup("unknown");
    let unknown = |r: Reconciled| matches!(r, Reconciled::Unknown(_));
    replay(&d, &[row(&t)], 3);
    assert!(unknown(reg.lookup(&t)), "exit 3: uncertain, never 'absent'");
    replay(&d, &[], 3);
    assert!(unknown(reg.lookup(&t)), "exit 3 with no rows is still uncertain");
    replay(&d, &[], 2);
    assert!(unknown(reg.lookup(&t)), "exit 2");
    replay(&d, &[row(&t), row(&t)], 0);
    assert!(unknown(reg.lookup(&t)), "several rows");
    for (k, v) in [("owner", json!("agent:someone-else")), ("lifecycle", json!("ephemeral")), ("home_path", json!("/elsewhere")), ("short_name", json!("rth-interim-inst-other")), ("memory_db", json!("brain_rth")), ("memory_db_holds_id", json!(false)), ("archived_at", json!("2026-10-01"))] {
        let mut r = row(&t);
        r[k] = v;
        replay(&d, &[r], 0);
        assert!(unknown(reg.lookup(&t)), "a foreign {k} is never adopted");
    }
}

#[test]
fn a_wrong_pin_or_a_tuple_outside_the_rth_namespace_never_runs_the_reader() {
    let (d, reg, t) = setup("never");
    replay(&d, &[row(&t)], 0);
    let bad = ListRegistry { list: Pinned { path: reg.list.path.clone(), sha256: "0".repeat(64) }, ..reg };
    assert!(matches!(bad.lookup(&t), Reconciled::Unknown(_)));
    let reg = ListRegistry { list: Pinned { path: bad.list.path.clone(), sha256: rth_activation::boot::sha256_hex(&std::fs::read(&bad.list.path).unwrap()) }, ..bad };
    let mut foreign_tuple = t.clone();
    foreign_tuple["memory_db"] = json!("brain_rth");
    assert!(matches!(reg.lookup(&foreign_tuple), Reconciled::Unknown(_)));
    assert!(!d.join("argv.log").exists(), "the reader never ran");
    assert_eq!(reg.create(&t).argv[0], "/bin/sh", "no pinned creation command: creation fails without effect");
}

#[test]
fn the_home_is_bound_by_its_config_and_creation_runs_only_with_every_pin_and_the_validated_socket() {
    let (d, reg, t) = setup("create");
    replay(&d, &[row(&t)], 0);
    let cfg = PathBuf::from(t["home"].as_str().unwrap()).join(".brain/config.json");
    std::fs::write(&cfg, json!({"database": "brain_rth_someone_else_ctl"}).to_string()).unwrap();
    assert!(matches!(reg.lookup(&t), Reconciled::Unknown(_)), "a home naming another control DB");
    std::fs::remove_file(&cfg).unwrap();
    std::os::unix::fs::symlink(d.join("elsewhere.json"), &cfg).unwrap();
    assert!(matches!(reg.lookup(&t), Reconciled::Unknown(_)), "a linked config is never followed");

    let tools = d.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for f in ["create-interim.sh", "interim-db.ts", "registry-list.ts"] {
        std::fs::write(tools.join(f), format!("// stand-in {f}\n")).unwrap();
    }
    let sha = |p: &Path| rth_activation::boot::sha256_hex(&std::fs::read(p).unwrap());
    let mk = |db_sha: String| ListRegistry {
        bun: Pinned { path: reg.bun.path.clone(), sha256: reg.bun.sha256.clone() },
        list: Pinned { path: tools.join("registry-list.ts"), sha256: sha(&tools.join("registry-list.ts")) },
        brain_root: reg.brain_root.clone(),
        create: Some(Pinned { path: tools.join("create-interim.sh"), sha256: sha(&tools.join("create-interim.sh")) }),
        interim_db_sha256: db_sha,
        socket: reg.socket.clone(),
        port: reg.port,
        home: reg.home.clone(),
    };
    let good = mk(sha(&tools.join("interim-db.ts")));
    let c = good.create(&t);
    assert_eq!(c.argv[0], tools.join("create-interim.sh").display().to_string());
    assert_eq!(&c.argv[1..], &[t["control_db"].as_str().unwrap(), t["name"].as_str().unwrap(), t["owner"].as_str().unwrap(), t["home"].as_str().unwrap(), "--socket", "/fixture/rt/pgserve", "--port", "5999"]);
    assert!(c.env.iter().any(|(k, v)| k == "HOME" && v == "/Users/fixture") && c.env.iter().any(|(k, _)| k == "BRAIN_ROOT"));
    assert_eq!(mk("0".repeat(64)).create(&t).argv[0], "/bin/sh", "a wrong interim-db.ts pin never runs the script");
    assert_eq!(rth_activation::registry::REGISTRY_LIST_SHA256, "0192c3d169ac0fa4d150bd979a3dfab138b2c4b03e7d9e655761c9ff54921fc1");
    assert_eq!(rth_activation::registry::CREATE_INTERIM_SHA256, "b0f7966aaec959582300f70e5490090e33544e607350b57a27d6640c42473c89");
    assert_eq!(rth_activation::registry::INTERIM_DB_SHA256, "7b1e16fc45363445c87aadbc383ab29fc43526b04e9873a2baca2a5cd127b030");
}

#[test]
fn bundle_relative_paths_resolve_only_as_plain_relative_paths() {
    let mut v = json!({"a": "@bundle/MacOS/rth-agentd", "b": ["@bundle/../etc/passwd", "@bundle//x", "@bundle/./y", "/abs", "plain"], "c": {"d": "@bundle/Resources/activation/fence.mjs"}});
    rth_activation::activation::resolve_bundle(&mut v, Path::new("/Applications/RTH.app/Contents"));
    assert_eq!(v["a"], "/Applications/RTH.app/Contents/MacOS/rth-agentd");
    assert_eq!(v["b"], json!(["@bundle/../etc/passwd", "@bundle//x", "@bundle/./y", "/abs", "plain"]), "never resolved: they then fail as relative paths");
    assert_eq!(v["c"]["d"], "/Applications/RTH.app/Contents/Resources/activation/fence.mjs");
}
