

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rth_protocol::io::read_frame;
use rth_protocol::{Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use super::*;

const SESSION: &str = "sess-research";

struct Gated {
    turns: tokio::sync::Mutex<mpsc::UnboundedReceiver<ModelTurn>>,
    seen: Mutex<Vec<Vec<Value>>>,
}

impl Model for Gated {
    fn turn<'a>(&'a self, messages: &'a [Value], _tools: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>> {
        self.seen.lock().unwrap().push(messages.to_vec());
        Box::pin(async move { self.turns.lock().await.recv().await.ok_or_else(|| "script closed".to_string()) })
    }
}

struct NoWeb;
impl rth_tools::web::Connect for NoWeb {
    fn connect<'a>(&'a self, _addr: std::net::SocketAddr, _host: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn rth_tools::web::Stream>>> {
        Box::pin(async { Err(std::io::Error::other("no network in tests")) })
    }
}

struct Rig {
    addr: std::net::SocketAddr,
    dir: PathBuf,
    docs: PathBuf,
    script: mpsc::UnboundedSender<ModelTurn>,
    model: Arc<Gated>,
}

async fn rig(name: &str) -> Rig {
    rig_with(name, |_| {}).await
}

async fn rig_with(name: &str, before: impl FnOnce(&std::path::Path)) -> Rig {
    rig_opts(name, before, None).await
}

async fn rig_opts(name: &str, before: impl FnOnce(&std::path::Path), memory: Option<bool>) -> Rig {
    let dir = std::env::temp_dir().join(format!("rth-agentd-research-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    std::fs::write(dir.join("docs/a.md"), "alpha: the staging port is 7421\n").unwrap();
    std::fs::write(dir.join("docs/big.txt"), "0123456789".repeat(2000)).unwrap();

    let state = dir.join("state");
    std::fs::create_dir_all(&state).unwrap();
    before(&state);
    let journal = state.join("journal.jsonl");
    let (script, rx) = mpsc::unbounded_channel();
    let model = Arc::new(Gated { turns: tokio::sync::Mutex::new(rx), seen: Mutex::new(Vec::new()) });
    TEST_SETUP.lock().unwrap().get_or_insert_with(HashMap::new).insert(journal.display().to_string(), (model.clone() as Arc<dyn Model>, Arc::new(NoWeb) as Arc<dyn rth_tools::web::Connect>));
    let identity = crate::instance::Boot {
        instance: Some(rth_protocol::deploy::InstanceIdentity {
            instance_id: "inst-research".into(),
            session_id: SESSION.into(),
            state_schema: 1,
            journal_bytes_at_boot: 0,
            context: rth_protocol::deploy::CONTEXT_NONE.into(),
            context_restore: None,
        }),
        ..Default::default()
    };
    let started = Instant::now();
    let session = crate::backend::fake(Duration::from_millis(5), move || started.elapsed(), format!("rs{name}"));
    let opts = crate::StartOpts { self_v4: false, crash_after: None, command_crash: None, command_fault: None, delegate: Some(true), memory, decider: None, memory_egress: None, saves: None, flash_spend: None, checker: None };
    let agent = crate::start(journal.display().to_string(), identity, session, format!("guest-rs{name}"), opts).await.unwrap();
    assert!(agent.research.is_some(), "research is on for this journal");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(crate::serve(agent, listener));
    Rig { addr, docs: dir.join("docs"), dir: state, script, model }
}

struct Client {
    reader: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    buf: Vec<u8>,
    stamper: Stamper,
}

impl Client {
    async fn attach(rig: &Rig) -> Client {
        let stream = TcpStream::connect(rig.addr).await.unwrap();
        let (read, write) = stream.into_split();
        let mut c = Client { reader: BufReader::new(read), write, buf: Vec::new(), stamper: Stamper::new(Source::Host, "host", Box::new(MonotonicClock::start())) };
        assert!(matches!(c.recv().await.event, Event::Hello { .. }));
        c.send(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: "app".into(), role: Role::Controller }).await;
        c.until(|e| matches!(e, Event::FolderGrants { .. }).then_some(())).await;
        c
    }

    async fn send(&mut self, event: Event) -> Envelope {
        let env = self.stamper.stamp(event, None);
        self.write.write_all(env.to_line().as_bytes()).await.unwrap();
        env
    }

    async fn command(&mut self, id: &str, text: &str) -> Envelope {
        self.send(Event::CommandSubmitted { command_id: id.into(), session_id: SESSION.into(), expected_generation: 0, text: text.into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }).await
    }

    async fn recv(&mut self) -> Envelope {
        let line = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut self.reader, &mut self.buf)).await.expect("frame within 5 s").expect("open connection");
        Envelope::parse_from(&line, Source::Guest).unwrap()
    }

    async fn until<T>(&mut self, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut kinds: Vec<String> = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "the awaited frame did not come within 20 s; seen: {kinds:?}");
            let env = self.recv().await;
            kinds.push(format!("{:?}", env.event).chars().take(160).collect());
            if let Some(t) = pick(&env.event) {
                return t;
            }
        }
    }

    async fn task_state(&mut self, execution: &str) -> Value {
        self.until(|e| match e {
            Event::TaskState { record, .. } if record["execution"] == execution && record["outcome"] == "applied" => Some(record.clone()),
            _ => None,
        })
        .await
    }

    async fn card(&mut self) -> (String, String, String) {
        self.until(|e| match e {
            Event::EffectCard { effect_id, effect_kind, token, .. } => Some((effect_id.clone(), effect_kind.clone(), token.clone())),
            _ => None,
        })
        .await
    }
}

fn read(id: &str, path: &std::path::Path, offset: Option<u64>) -> ModelTurn {
    let mut args = json!({"path": path.display().to_string()});
    if let Some(o) = offset {
        args["offset"] = json!(o);
    }
    ModelTurn { content: String::new(), tool_calls: vec![worker::Call { id: id.into(), name: "files_read".into(), arguments: args.to_string() }], finish: "tool_calls".into(), usage: None }
}

fn answer(text: &str) -> ModelTurn {
    ModelTurn { content: text.into(), tool_calls: vec![], finish: "stop".into(), usage: None }
}

#[tokio::test]
async fn a_typed_research_task_asks_for_the_folder_once_reads_after_the_tap_and_delivers_its_answer() {
    let r = rig("happy").await;
    let mut c = Client::attach(&r).await;
    c.command("c1", "/research What is the staging port?").await;
    let accepted = c.task_state("accepted").await;
    let task_id = accepted["task_id"].as_str().unwrap().to_string();
    assert_eq!(accepted["assignment"]["executor_id"], "agentd");
    c.task_state("running").await;
    r.script.send(read("m1", &r.docs.join("a.md"), None)).unwrap();
    let (effect_id, kind, token) = c.card().await;
    assert_eq!(kind, "folder_grant");

    c.command("c2", "yes").await;
    c.until(|e| matches!(e, Event::AgentDone { .. }).then_some(())).await;
    c.command("c3", "/grant list").await;
    let listed = c.until(|e| match e {
        Event::FolderGrants { grants } => Some(grants.clone()),
        _ => None,
    })
    .await;
    assert_eq!(listed, json!([]), "nothing granted by typing");

    c.command("c4", &format!("/effect release {effect_id} {token}")).await;
    let released = c.until(|e| match e {
        Event::EffectReleased { effect_kind, result, .. } => Some((effect_kind.clone(), result.clone())),
        _ => None,
    })
    .await;
    assert_eq!(released.0, "folder_grant");
    assert_eq!(released.1["root"], r.docs.display().to_string());
    let receipt = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!((receipt["outcome"].as_str(), receipt["tool"].as_str(), receipt["call_seq"].as_u64()), (Some("ok"), Some("files.read"), Some(1)));
    r.script.send(answer("The staging port is 7421.")).unwrap();
    let report = c.until(|e| match e {
        Event::TaskReport { task_id: t, text, .. } => Some((t.clone(), text.clone())),
        _ => None,
    })
    .await;
    assert_eq!(report, (task_id.clone(), "The staging port is 7421.".to_string()));
    let delivered = c.until(|e| match e {
        Event::TaskState { record, .. } if record["publication"] == "delivered" => Some(record.clone()),
        _ => None,
    })
    .await;
    assert!(delivered["chat_ref"]["journal_event"].is_string());
    c.task_state("execution_done").await;

    let seen = r.model.seen.lock().unwrap().clone();
    let tool_msg = seen.last().unwrap().iter().find(|m| m["role"] == "tool").unwrap().clone();
    assert!(tool_msg["content"].as_str().unwrap().contains("7421"));

    let tasks = std::fs::read_to_string(r.dir.join("tasks.jsonl")).unwrap();
    assert_eq!(tasks.lines().filter(|l| l.contains("\"rec\":\"tool_call\"")).count(), 1);
    let grants = std::fs::read_to_string(r.dir.join("grants.jsonl")).unwrap();
    let events: Vec<String> = grants.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["event"].as_str().unwrap().to_string()).collect();
    assert_eq!(events, ["grant_requested", "granted"]);

    let journal = std::fs::read_to_string(r.dir.join("journal.jsonl")).unwrap();
    assert!(!journal.contains(&token), "the card's token stays off the journal");
}

#[tokio::test]
async fn a_declined_card_reads_nothing_and_a_cancel_during_a_card_wait_settles_and_kills_the_card() {
    let r = rig("decline").await;
    let mut c = Client::attach(&r).await;
    c.command("c1", "/research What is the staging port?").await;
    c.task_state("running").await;
    r.script.send(read("m1", &r.docs.join("a.md"), None)).unwrap();
    let (effect_id, _, token) = c.card().await;
    c.command("c2", &format!("/effect decline {effect_id} {token}")).await;
    let receipt = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!((receipt["outcome"].as_str(), receipt["reason"].as_str()), (Some("refused"), Some("not_granted")));
    r.script.send(answer("I could not read that folder.")).unwrap();
    c.task_state("execution_done").await;
    let seen = r.model.seen.lock().unwrap().clone();
    assert!(!seen.last().unwrap().iter().any(|m| m["content"].as_str().is_some_and(|s| s.contains("7421"))), "nothing from the folder reached the model");

    c.command("c3", "/research and again?").await;
    let accepted = c.task_state("accepted").await;
    let task_id = accepted["task_id"].as_str().unwrap().to_string();
    c.task_state("running").await;
    r.script.send(read("m2", &r.docs.join("a.md"), None)).unwrap();
    let (effect_id, _, token) = c.card().await;
    let ui = json!({"launch_id": "app", "pid": 0, "plugin_instance": "ui"});
    c.send(Event::TaskUpdate { task_id: task_id.clone(), update: json!({"task_id": task_id, "executor_id": "app", "incarnation": ui, "request_id": "cancel-1", "kind": {"kind": "cancel_intent", "by": "user"}}) }).await;
    c.task_state("cancel_requested").await;
    let settled = c.task_state("cancelled").await;
    assert_eq!(settled["task_id"], task_id.as_str());
    c.command("c4", &format!("/effect release {effect_id} {token}")).await;
    let late = c.until(|e| match e {
        Event::EffectRefused { effect_id: id, reason, .. } if *id == effect_id => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert_eq!(late, "expired", "a late tap finds nothing to approve");
    let grants = std::fs::read_to_string(r.dir.join("grants.jsonl")).unwrap();
    assert!(!grants.contains("\"granted\""), "nothing was ever granted");
    assert_eq!(grants.lines().filter(|l| l.contains("\"denied\"")).count(), 2, "declined, then closed by the cancel");
}

#[tokio::test]
async fn a_revoke_from_settings_stops_the_next_page_of_a_running_task() {
    let r = rig("revoke").await;
    let mut c = Client::attach(&r).await;
    c.command("c1", "/research Read big.txt").await;
    c.task_state("running").await;
    r.script.send(read("m1", &r.docs.join("big.txt"), None)).unwrap();
    let (effect_id, _, token) = c.card().await;
    c.command("c2", &format!("/effect release {effect_id} {token}")).await;
    let first = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!(first["outcome"], "ok");
    let grant_id = first["grant_id"].as_str().unwrap().to_string();
    c.command("c3", &format!("/grant revoke {grant_id}")).await;
    let revoked = c.until(|e| match e {
        Event::FolderGrant { event, record } if event == "revoked" => Some(record.clone()),
        _ => None,
    })
    .await;
    assert_eq!(revoked["grant_id"], grant_id.as_str());
    let listed = c.until(|e| match e {
        Event::FolderGrants { grants } => Some(grants.clone()),
        _ => None,
    })
    .await;
    assert_eq!(listed, json!([]));

    r.script.send(read("m2", &r.docs.join("big.txt"), Some(8000))).unwrap();
    let (effect_id, kind, token) = c.card().await;
    assert_eq!(kind, "folder_grant");
    c.command("c4", &format!("/effect decline {effect_id} {token}")).await;
    let second = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!((second["call_seq"].as_u64(), second["reason"].as_str()), (Some(2), Some("not_granted")));
    r.script.send(answer("Only the first page was readable.")).unwrap();
    c.task_state("execution_done").await;
}

fn call(id: &str, name: &str, args: Value) -> ModelTurn {
    ModelTurn { content: String::new(), tool_calls: vec![worker::Call { id: id.into(), name: name.into(), arguments: args.to_string() }], finish: "tool_calls".into(), usage: None }
}

#[tokio::test]
async fn an_mcp_server_connects_off_turns_on_with_a_tap_serves_a_task_and_turns_off() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../rth-mcp/tests/fixtures/echo_mcp.py");
    let mut mark = PathBuf::new();
    let r = rig_with("mcp", |dir| {
        mark = dir.join("echo-started");
        let mcp = json!({"schema": "rth-mcp/v1", "servers": [{"id": "echo", "title": "Echo", "transport": {"stdio": {"command": "/usr/bin/python3", "args": [fixture], "env": {"ECHO_MARK": mark.display().to_string(), "ECHO_GRANDCHILD": "1"}}}, "tools": {"echo": {"effect": "none", "approval": "allow"}}}]});
        std::fs::write(dir.join("mcp.json"), mcp.to_string()).unwrap()
    })
    .await;
    let mut c = Client::attach(&r).await;
    let server = |status: &'static str, want: &'static str| move |e: &Event| match e {
        Event::PartState { parts } if parts["servers"][0]["status"] == status && parts["servers"][0]["standing"] == want => Some(parts["servers"][0].clone()),
        _ => None,
    };
    let s = c.until(server("off", "off")).await;
    assert_eq!((s["part_id"].as_str(), s["digest"].as_str()), (Some("mcp.echo"), None));
    c.command("p0", "/part on mcp.nothing").await;
    let why = c.until(|e| match e {
        Event::Rejected { reason } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert!(why.contains("not in mcp.json"), "{why}");
    c.command("p1", "/part on mcp.echo").await;
    let (effect_id, kind, token) = c.card().await;
    assert_eq!(kind, "part_on");
    assert!(!mark.exists(), "off means not started: nothing runs before the tap");
    c.command("p2", &format!("/effect release {effect_id} {token}")).await;
    let on = c.until(server("connected", "on")).await;
    assert!(on["digest"].as_str().unwrap().starts_with("sha256:"));
    assert_eq!((on["tools"][0]["name"].as_str(), on["tools"][0]["effect"].as_str()), (Some("echo"), Some("none")));
    assert!(mark.exists(), "started by the tap");
    c.command("c1", "/research Ask the echo server to say hi").await;
    c.task_state("running").await;
    r.script.send(call("m1", "mcp_echo__echo", json!({"text": "hi"}))).unwrap();
    let receipt = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!((receipt["outcome"].as_str(), receipt["tool"].as_str()), (Some("ok"), Some("mcp.echo/echo")));
    assert_eq!(receipt["target"].as_str(), Some(format!("mcp.echo@{}", on["digest"].as_str().unwrap()).as_str()));
    r.script.send(answer("It said hi.")).unwrap();
    c.task_state("execution_done").await;
    let seen = r.model.seen.lock().unwrap().clone();
    let tool_msg = seen.last().unwrap().iter().find(|m| m["role"] == "tool").unwrap().clone();
    assert!(tool_msg["content"].as_str().unwrap().contains("hi|"), "the server's answer reached the model");
    let pids: Vec<i32> = std::fs::read_to_string(&mark).unwrap().lines().next().unwrap().split_whitespace().skip(1).map(|p| p.parse().unwrap()).collect();
    assert_eq!(pids.len(), 2, "the server and its helper");
    c.command("p3", "/part off mcp.echo").await;
    c.until(server("off", "off")).await;
    let alive = |pid: &i32| {
        let out = std::process::Command::new("/bin/ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
        let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
        !stat.is_empty() && !stat.starts_with('Z')
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while pids.iter().any(alive) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!pids.iter().any(alive), "turned off, yet still running: {:?}", pids.iter().filter(|p| alive(p)).collect::<Vec<_>>());
    let lines = std::fs::read_to_string(r.dir.join("parts.jsonl")).unwrap();
    let events: Vec<String> = lines.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["event"].as_str().unwrap().to_string()).collect();
    assert_eq!(events, ["turned_on", "turned_off"]);
}

#[tokio::test]
async fn bad_grant_commands_and_unknown_grants_are_refused() {
    let r = rig("grantcmd").await;
    let mut c = Client::attach(&r).await;
    for (id, text, want) in [("g1", "/grant revoke nope", "unknown_grant"), ("g2", "/grant", "usage"), ("g3", "/grant revoke a b", "usage")] {
        let sent = c.command(id, text).await;
        let why = c.until(|e| match e {
            Event::Rejected { reason } => Some(reason.clone()),
            _ => None,
        })
        .await;
        assert!(why.contains(want), "{text}: {why}");
        let _ = sent;
    }
}

#[test]
fn a_chat_completions_answer_parses_into_a_turn() {
    let v = json!({"choices": [{"finish_reason": "tool_calls", "message": {"content": null, "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "files_read", "arguments": "{\"path\":\"/x\"}"}}]}}], "usage": {"prompt_tokens": 9}});
    let t = parse_turn(&v).unwrap();
    assert_eq!((t.content.as_str(), t.finish.as_str(), t.tool_calls[0].name.as_str(), t.tool_calls[0].arguments.as_str()), ("", "tool_calls", "files_read", "{\"path\":\"/x\"}"));
    assert_eq!(t.usage, Some(json!({"prompt_tokens": 9})));
    assert!(parse_turn(&json!({"choices": []})).is_err());
    let plain = parse_turn(&json!({"choices": [{"finish_reason": "stop", "message": {"content": "hi"}}]})).unwrap();
    assert!(plain.tool_calls.is_empty() && plain.usage.is_none());
}

#[test]
fn a_part_call_card_shows_the_whole_arguments() {
    let q = "z".repeat(1500);
    let call = PartCall { part_id: "mcp.docs".into(), title: "Docs".into(), digest: "sha256:x".into(), tool: "search".into(), args: json!({"q": q}), args_sha256: "a".into(), why: "w".into() };
    let s = part_call_summary(&call);
    assert!(s.contains(&json!({"q": q}).to_string()) && !s.contains('…'), "{}", s.len());
}

#[test]
fn card_texts_show_invisible_characters() {
    let (s, w) = card_texts("Let Unfold read the folder /x/evil\u{202E}fdp.exe?", "a task\u{200B} needs it");
    assert_eq!((s.as_str(), w.as_str()), ("Let Unfold read the folder /x/evilU+202Efdp.exe?", "a taskU+200B needs it"));
}

#[tokio::test]
async fn a_folder_card_shows_a_bidi_override_in_the_folder_name_as_text() {
    let r = rig_with("bidi", |dir| {
        let evil = dir.parent().unwrap().join("evil\u{202E}fdp.exe");
        std::fs::create_dir_all(&evil).unwrap();
        std::fs::write(evil.join("n.md"), "note\n").unwrap();
    })
    .await;
    let mut c = Client::attach(&r).await;
    c.command("c1", "/research Read the note").await;
    c.task_state("running").await;
    r.script.send(read("m1", &r.dir.parent().unwrap().join("evil\u{202E}fdp.exe/n.md"), None)).unwrap();
    let summary = c.until(|e| match e {
        Event::EffectCard { summary, .. } => Some(summary.clone()),
        _ => None,
    })
    .await;
    assert!(summary.contains("evilU+202Efdp.exe") && !summary.contains('\u{202E}'), "{summary}");

    let _ = std::fs::remove_dir_all(r.dir.parent().unwrap());
}

#[tokio::test]
async fn at_boot_a_part_whose_contents_changed_is_stopped_and_shown_as_changed() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../rth-mcp/tests/fixtures/echo_mcp.py");
    let r = rig_with("mcp-changed", |dir| {
        let mcp = json!({"schema": "rth-mcp/v1", "servers": [{"id": "echo", "title": "Echo", "transport": {"stdio": {"command": "/usr/bin/python3", "args": [fixture]}}, "tools": {"echo": {"effect": "none", "approval": "allow"}}}]});
        std::fs::write(dir.join("mcp.json"), mcp.to_string()).unwrap();
        let old = json!({"event": "turned_on", "part_id": "mcp.echo", "digest": format!("sha256:{}", "a".repeat(64)), "effect_id": "e-old", "args_sha256": "x", "utc_ms": 1});
        std::fs::write(dir.join("parts.jsonl"), format!("{old}\n")).unwrap();
    })
    .await;
    let mut c = Client::attach(&r).await;
    let s = c.until(|e| match e {
        Event::PartState { parts } if parts["servers"][0]["status"] != "connecting" && parts["servers"][0]["status"] != "off" => Some(parts["servers"][0].clone()),
        _ => None,
    })
    .await;
    assert_eq!((s["status"].as_str(), s["standing"].as_str(), s["digest"].as_str()), (Some("changed"), Some("changed"), None), "{s}");
    assert!(s["error"].as_str().unwrap().contains("changed"));
    c.command("c1", "/research Ask the echo server").await;
    c.task_state("running").await;
    r.script.send(call("m1", "mcp_echo__echo", json!({"text": "hi"}))).unwrap();
    let receipt = c.until(|e| match e {
        Event::TaskToolCall { receipt, .. } => Some(receipt.clone()),
        _ => None,
    })
    .await;
    assert_eq!(receipt["reason"], "unknown_tool", "a changed part offers nothing");
}

#[test]
fn commands_and_hard_blocks() {
    assert!(is_command("/research x") && is_command("  /grant list") && is_command("/research") && is_command("/part list"));
    assert!(!is_command("/researcher x") && !is_command("research x") && !is_command("/effect release a b"));
    let b = hard_blocks(Some(std::path::Path::new("/Users/someone")), &[PathBuf::from("/state")]);
    for p in ["/state", "/Users/someone/.ssh", "/Users/someone/Library/Keychains", "/Users/someone/.local/share/rth", "/Users/someone/.config/gh", "/Users/someone/.cargo", "/Users/someone/.azure", "/Users/someone/Library/Accounts", "/Users/someone/Library/Preferences"] {
        assert!(b.contains(&PathBuf::from(p)), "{p}");
    }
}

#[tokio::test]
async fn pv05_g9_a_research_task_from_another_speaker_recalls_nothing_of_the_owners() {
    use rth_protocol::TurnSpeaker;
    let lia = TurnSpeaker::Person { person_id: "p-lia".into() };
    for (who, speaker, owner) in [("keyboard", TurnSpeaker::OwnerKeyboard, true), ("other", lia, false), ("unknown", TurnSpeaker::Unknown, false)] {
        let r = rig_opts(&format!("g9-{who}"), |_| {}, Some(true)).await;
        let mut c = Client::attach(&r).await;
        for (frame, id, name, is_owner) in [("f1", "p-rui", "Rui", true), ("f2", "p-lia", "Lia", false)] {
            let auth = Some(rth_protocol::OwnerGrant { grant_id: format!("g-{frame}"), method: rth_protocol::OwnerGrantMethod::TouchIdOrPassword, at_utc_ms: 1 });
            c.send(Event::PeopleEnrol { frame_id: frame.into(), person_id: id.into(), name: name.into(), relation: "family".into(), owner: is_owner, modality: rth_protocol::PeopleModality::ReadAlong, consent_copy_version: 2, auth }).await;
            c.until(|e| matches!(e, Event::PeopleAck { .. }).then_some(())).await;
        }
        c.command("c1", "/remember my favourite café is Lua").await;
        c.until(|e| matches!(e, Event::MemoryAct { state, .. } if state == "committed").then_some(())).await;
        c.send(Event::CommandSubmitted { command_id: "c2".into(), session_id: SESSION.into(), expected_generation: 0, text: "/research what is my favourite café?".into(), speaker, reply_voice: None, effort: None, reply_max_tokens: None }).await;
        c.task_state("running").await;
        r.script.send(call("m1", "memory_recall", json!({"query": "favourite café"}))).unwrap();
        c.until(|e| matches!(e, Event::TaskToolCall { receipt, .. } if receipt["tool"] == "memory.recall").then_some(())).await;
        r.script.send(answer("done")).unwrap();
        c.task_state("execution_done").await;
        let seen = r.model.seen.lock().unwrap().clone();
        let tool = seen.last().unwrap().iter().find(|m| m["role"] == "tool").unwrap()["content"].as_str().unwrap().to_string();
        if owner {
            assert!(tool.contains("Lua"), "{who}: the owner's task recalls the café: {tool}");
        } else {
            assert!(!tool.contains("Lua") && tool.contains("not_owner"), "{who}: nothing of the owner's reaches the task: {tool}");
        }
    }
}
