

use rth_mcp::{connect, parse};
use rth_tools::parts::Part;
use serde_json::json;

const PYTHON: &str = "/usr/bin/python3";

fn fixture() -> String {
    format!("{}/tests/fixtures/echo_mcp.py", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn a_stdio_server_starts_with_a_clean_environment_and_answers() {
    let s = json!({"id": "echo", "transport": {"stdio": {"command": PYTHON, "args": [fixture()], "env": {"ECHO_MODE": "test"}}}, "tools": {"echo": {"effect": "none"}}});
    let cfg = parse(&json!({"schema": rth_mcp::SCHEMA, "servers": [s]}).to_string()).unwrap().servers.remove(0);
    let env = |k: &str| match k {
        "PATH" => Some("/usr/bin:/bin".to_string()),
        "SECRET_API_KEY" => Some("must-not-leak".to_string()),
        _ => None,
    };
    let part = connect(&cfg, &env).await.unwrap();
    assert_eq!(part.manifest().id, "mcp.echo");
    assert_eq!(part.manifest().tools.len(), 1);
    let out = part.call("echo", json!({"text": "hi"}).as_object().unwrap().clone()).await.unwrap();
    let (said, vars) = out.text.split_once('|').unwrap();
    assert_eq!(said, "hi");
    let vars: Vec<&str> = vars.split(',').collect();
    assert!(vars.contains(&"ECHO_MODE") && vars.contains(&"PATH"), "{vars:?}");

    assert!(std::env::vars().any(|(k, _)| k.starts_with("CARGO")));
    assert!(!vars.iter().any(|v| v.starts_with("CARGO") || *v == "SECRET_API_KEY" || *v == "HOME"), "{vars:?}");
    part.close().await;
}

fn alive(pid: i32) -> bool {

    let out = std::process::Command::new("/bin/ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    !stat.trim().is_empty() && !stat.trim().starts_with('Z')
}

async fn gone_within(pids: &[i32], limit: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline {
        if pids.iter().all(|p| !alive(*p)) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    false
}

async fn started(mark: &std::path::Path) -> Vec<i32> {
    for _ in 0..50 {
        if let Ok(t) = std::fs::read_to_string(mark)
            && let Some(l) = t.lines().next()
        {
            return l.split_whitespace().skip(1).map(|p| p.parse().unwrap()).collect();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the fixture never started");
}

async fn with_helper(tag: &str) -> (rth_mcp::McpPart, Vec<i32>) {
    let mark = std::env::temp_dir().join(format!("rth-mcp-{tag}-{}-{}", std::process::id(), rth_tools::utc_ms()));
    let s = json!({"id": "echo", "transport": {"stdio": {"command": PYTHON, "args": [fixture()], "env": {"ECHO_MARK": mark.display().to_string(), "ECHO_GRANDCHILD": "1"}}}});
    let cfg = parse(&json!({"schema": rth_mcp::SCHEMA, "servers": [s]}).to_string()).unwrap().servers.remove(0);
    let part = connect(&cfg, &|k| (k == "PATH").then(|| "/usr/bin:/bin".to_string())).await.unwrap();
    let pids = started(&mark).await;
    let _ = std::fs::remove_file(&mark);
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|p| alive(*p)), "both run while the part is on");
    (part, pids)
}

#[tokio::test]
async fn closing_a_part_ends_its_server_and_the_helpers_the_server_started() {
    let (part, pids) = with_helper("close").await;
    part.close().await;
    assert!(gone_within(&pids, std::time::Duration::from_secs(5)).await, "left running: {:?}", pids.iter().filter(|p| alive(**p)).collect::<Vec<_>>());
}

#[tokio::test]
async fn dropping_a_part_ends_its_server_and_the_helpers_the_server_started() {
    let (part, pids) = with_helper("drop").await;
    drop(part);
    assert!(gone_within(&pids, std::time::Duration::from_secs(5)).await, "left running: {:?}", pids.iter().filter(|p| alive(**p)).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_command_that_is_not_an_mcp_server_fails_within_its_startup_timeout() {
    let s = json!({"id": "cat", "startup_timeout_sec": 2, "transport": {"stdio": {"command": "/bin/sleep", "args": ["30"]}}});
    let cfg = parse(&json!({"schema": rth_mcp::SCHEMA, "servers": [s]}).to_string()).unwrap().servers.remove(0);
    let t0 = std::time::Instant::now();
    let e = connect(&cfg, &|_| None).await.err().unwrap();
    assert!(e.contains("within 2 s") && t0.elapsed() < std::time::Duration::from_secs(4), "{e}");
}
