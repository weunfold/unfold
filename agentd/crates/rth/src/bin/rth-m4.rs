

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use microsandbox::{Sandbox, SecretSource, Volume};
use rth::audio::{AudioStore, Chunk};
use rth_protocol::io::read_frame;
use rth_protocol::{AGENT_PORT, Envelope, Event, MonotonicClock, PROTOCOL_VERSION, ResponseStatus, Role, Source, Stamper};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

const VM_NAME: &str = "rth-m4-vm";
const STATE_VOLUME: &str = "rth-m4-state";
const HOST_PORT: u16 = 7720;
const KEY_ENV: &str = "RTH_GEMINI_API_KEY";
const GEMINI_HOST: &str = "generativelanguage.googleapis.com";
const SYSTEM: &str = "You are RTH, a concise voice assistant. Answer in one short spoken sentence.";
const PROMPTS: &[&str] = &[
    "Say hello in five words.",
    "What colour is a clear daytime sky?",
    "Name one planet in our solar system.",
    "What is two plus three?",
    "Give me a one sentence fun fact about octopuses.",
    "Which season comes after winter?",
    "Say good night in Portuguese.",
    "What is the capital of France?",
    "Name a fruit that is yellow.",
    "How many days are in a week?",
    "Spell the word cat.",
    "What do bees make?",
];
const CANCEL_PROMPT: &str =
    "Ignore the one sentence rule for this answer: count slowly from one to forty, saying each number as a word.";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
    match args.first().map(String::as_str) {
        Some("stop") => stop(),
        Some("probe") => with_vm(|sb| async move { probe(&sb).await }).await,
        Some("run") => {
            let sessions: usize = flag("--sessions").unwrap_or(3);
            let turns: usize = flag("--turns").unwrap_or(4);
            let cancel = !args.iter().any(|a| a == "--no-cancel");
            if sessions == 0 || sessions * turns > PROMPTS.len() {
                bail!("at most {} turns (keep API spend small)", PROMPTS.len());
            }
            with_vm(move |sb| async move { run(&sb, sessions, turns, cancel).await }).await
        }
        _ => bail!("usage: rth-m4 probe | run [--sessions N] [--turns N] [--no-cancel] | stop"),
    }
}

fn stop() -> Result<()> {
    let status = std::process::Command::new("msb").args(["stop", VM_NAME]).status().context("running msb stop")?;
    if !status.success() {
        bail!("msb stop {VM_NAME} exited with {status}");
    }
    Ok(())
}

async fn with_vm<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce(Sandbox) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if std::env::var(KEY_ENV).map(|v| v.trim().is_empty()).unwrap_or(true) {
        bail!("{KEY_ENV} is not set (read it from bws into this process's environment; never on argv)");
    }
    let sb = boot().await?;
    let result = f(sb).await;
    eprintln!("stopping {VM_NAME}");
    if let Err(e) = stop() {
        eprintln!("warning: {e:#}");
    }
    result
}

fn agent_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("RTH_AGENTD_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/aarch64-unknown-linux-musl/release"),
    };
    let dir = dir.canonicalize().with_context(|| format!("agent dir {}", dir.display()))?;
    if !dir.join("rth-agentd").exists() {
        bail!("no rth-agentd in {}", dir.display());
    }
    Ok(dir)
}

async fn boot() -> Result<Sandbox> {
    let _ = Volume::builder(STATE_VOLUME).create().await;
    let dir = agent_dir()?.to_string_lossy().to_string();
    let t0 = Instant::now();
    let sb = Sandbox::builder(VM_NAME)
        .image("alpine")
        .cpus(2)
        .memory(1024)
        .replace()
        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), HOST_PORT, AGENT_PORT)
        .volume("/rth", |m| m.named(STATE_VOLUME))
        .volume("/opt/rth", |m| m.bind(&dir).readonly())
        .secret(|s| {
            s.env("GEMINI_API_KEY")
                .source(SecretSource::env(KEY_ENV))
                .allow(GEMINI_HOST)
                .substitute_in_header_fields(["x-goog-api-key"])
        })
        .detached(true)
        .create()
        .await
        .context("creating rth-m4-vm")?;
    eprintln!("{VM_NAME} up in {} ms", t0.elapsed().as_millis());
    Ok(sb)
}

async fn sh(sb: &Sandbox, script: &str) -> Result<String> {
    let out = sb.shell(script.to_string()).await.with_context(|| format!("guest shell: {script}"))?;
    Ok(out.stdout()?)
}

async fn secret_facts(sb: &Sandbox) -> Result<Value> {
    let key = std::env::var(KEY_ENV)?;
    let env_value = sh(sb, "printenv GEMINI_API_KEY").await?.trim_end_matches('\n').to_string();
    let agent_env = sh(sb, "cat /proc/$(pidof rth-agentd)/environ 2>/dev/null | tr '\\0' '\\n' | grep '^GEMINI_API_KEY=' || true").await?;
    let agent_value = agent_env.trim().strip_prefix("GEMINI_API_KEY=").unwrap_or("").to_string();
    let ca = sh(sb, "ls -la /etc/ssl/certs/ca-certificates.crt 2>&1; printenv SSL_CERT_FILE; printenv | grep -i -E '^(MSB|SSL|REQUESTS|NODE_EXTRA)' | cut -d= -f1").await?;
    Ok(json!({
        "guest_env_is_placeholder": env_value.starts_with("$MSB_") && env_value != key,
        "guest_env_placeholder": if env_value.starts_with("$MSB_") && env_value != key { Value::from(env_value.clone()) } else { Value::from("<not a placeholder; withheld>") },
        "guest_env_equals_real_key": env_value == key,
        "guest_env_contains_real_key": env_value.contains(&key),
        "agentd_env_value_is_placeholder": !agent_value.is_empty() && agent_value.starts_with("$MSB_") && !agent_value.contains(&key),
        "agentd_env_contains_real_key": agent_env.contains(&key),
        "guest_trust": ca.lines().map(str::to_string).collect::<Vec<_>>(),
    }))
}

async fn probe(sb: &Sandbox) -> Result<()> {
    let facts = secret_facts(sb).await?;
    println!("{}", serde_json::to_string_pretty(&facts)?);
    let extra = sh(sb, "ls -la /etc/ssl/certs | head -20; ls /usr/local/share/ca-certificates 2>&1 | head; grep -c 'BEGIN CERTIFICATE' /etc/ssl/certs/ca-certificates.crt 2>&1; nslookup generativelanguage.googleapis.com 2>&1 | tail -3").await?;
    println!("{extra}");
    Ok(())
}

async fn start_agent(sb: &Sandbox) -> Result<()> {
    let script = format!(
        "pkill rth-agentd; sleep 0.2; RTH_BACKEND=gemini RTH_GEMINI_SYSTEM='{SYSTEM}' RTH_JOURNAL=/rth/journal.jsonl \
         nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & echo started"
    );
    let out = sh(sb, &script).await?;
    if out.trim() != "started" {
        bail!("agent start said {out:?}");
    }
    Ok(())
}

struct Link {
    reader: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    buf: Vec<u8>,
    stamper: Stamper,
    clock: Instant,
}

impl Link {

    async fn attach(name: &str, clock: Instant) -> Result<(Self, Vec<(f64, Envelope)>)> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > deadline {
                bail!("agent did not accept an attach within 20 s");
            }
            let Ok(stream) = TcpStream::connect(("127.0.0.1", HOST_PORT)).await else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            let _ = stream.set_nodelay(true);
            let (read, write) = stream.into_split();
            let mut link = Link {
                reader: BufReader::new(read),
                write,
                buf: Vec::new(),
                stamper: Stamper::new(Source::Host, name, Box::new(MonotonicClock::start())),
                clock,
            };
            let Ok(Some((t, hello))) = link.recv(Duration::from_secs(2)).await else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            if !matches!(hello.event, Event::Hello { protocol: PROTOCOL_VERSION, .. }) {
                bail!("first frame was not a v{PROTOCOL_VERSION} hello");
            }
            let mut seen = vec![(t, hello)];
            link.send(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: name.into(), role: Role::Controller }).await?;
            loop {
                let Some((t, env)) = link.recv(Duration::from_secs(5)).await? else { bail!("no ready") };
                let ready = matches!(&env.event, Event::Ready { attachment_id, role: Role::Controller } if attachment_id == name);
                seen.push((t, env));
                if ready {
                    return Ok((link, seen));
                }
            }
        }
    }

    fn now_ms(&self) -> f64 {
        self.clock.elapsed().as_secs_f64() * 1000.0
    }

    async fn send(&mut self, event: Event) -> Result<(f64, Envelope)> {
        let env = self.stamper.stamp(event, None);
        let t = self.now_ms();
        self.write.write_all(env.to_line().as_bytes()).await?;
        Ok((t, env))
    }

    async fn recv(&mut self, wait: Duration) -> Result<Option<(f64, Envelope)>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            let line = match tokio::time::timeout_at(deadline, read_frame(&mut self.reader, &mut self.buf)).await {
                Err(_) => return Ok(None),
                Ok(r) => r.map_err(|e| anyhow::anyhow!("link: {e}"))?,
            };
            let t = self.now_ms();
            let Some(env) = Envelope::parse_known(&line, Source::Guest).map_err(|e| anyhow::anyhow!("bad guest frame: {e}"))? else { continue };
            if !matches!(env.event, Event::Heartbeat { .. }) {
                return Ok(Some((t, env)));
            }
        }
    }
}

#[derive(Default)]
struct TurnObs {
    session: usize,
    prompt: String,
    request_id: String,
    response_id: Option<String>,
    sent: f64,
    started: Option<f64>,
    first_audio: Option<f64>,
    first_text: Option<f64>,
    done: Option<f64>,
    status: Option<ResponseStatus>,
    transcript: String,
    chunks: u64,
    guest_first_audio_ms: Option<f64>,
    guest_started_ns: Option<u64>,
    cancel_sent: Option<f64>,
    chunks_after_cancel_sent: u64,
    chunks_after_done: u64,
    deltas_after_done: u64,
    wav: Option<String>,
    audio_s: f64,
    gaps: u64,
}

struct Recorder {
    out: PathBuf,
    events: std::fs::File,
    store: AudioStore,
}

impl Recorder {

    fn log(&mut self, t: f64, env: &Envelope) -> Result<()> {
        let mut v = serde_json::to_value(env)?;
        if let Event::AgentAudio { data, .. } = &env.event {
            v["data"] = json!(format!("<{} base64 chars>", data.len()));
        }
        writeln!(self.events, "{}", json!({"t_ms": t, "env": v}))?;
        Ok(())
    }
}

fn observe(rec: &mut Recorder, turn: &mut TurnObs, t: f64, env: &Envelope) {
    match &env.event {
        Event::ResponseStarted { response_id, request_id, .. } if *request_id == turn.request_id => {
            turn.response_id = Some(response_id.clone());
            turn.started = Some(t);
            turn.guest_started_ns = Some(env.t_ns);
        }
        Event::AgentAudio { response_id, chunk, sample_offset, sample_rate, format, data } if Some(response_id) == turn.response_id.as_ref() => {
            turn.chunks += 1;
            if turn.done.is_some() {
                turn.chunks_after_done += 1;
            }
            if turn.cancel_sent.is_some() {
                turn.chunks_after_cancel_sent += 1;
            }
            if rec.store.chunk(response_id, *chunk, *sample_offset, *sample_rate, format, data) == Chunk::Accepted && turn.first_audio.is_none() {
                turn.first_audio = Some(t);
                turn.guest_first_audio_ms = turn.guest_started_ns.map(|s| env.t_ns.saturating_sub(s) as f64 / 1e6);
            }
        }
        Event::AgentDelta { response_id, text } if Some(response_id) == turn.response_id.as_ref() => {
            if turn.done.is_some() {
                turn.deltas_after_done += 1;
            } else {
                turn.first_text.get_or_insert(t);
                turn.transcript.push_str(text);
            }
        }

        Event::AgentReplace { response_id, text } if Some(response_id) == turn.response_id.as_ref() && turn.done.is_none() => {

            if !text.is_empty() {
                turn.first_text.get_or_insert(t);
            }
            turn.transcript = text.clone();
        }
        Event::AgentDone { response_id, status } if Some(response_id) == turn.response_id.as_ref() && turn.done.is_none() => {
            turn.done = Some(t);
            turn.status = Some(*status);
            if let Some(Ok(saved)) = rec.store.end(response_id) {
                turn.wav = Some(saved.path.file_name().unwrap().to_string_lossy().to_string());
                turn.audio_s = saved.samples as f64 / f64::from(saved.sample_rate.max(1));
                turn.gaps = saved.gaps;
            }
        }
        _ => {}
    }
}

fn backend_log(env: &Envelope) -> Option<String> {
    match &env.event {
        Event::Log { msg, .. } if msg.starts_with("gemini") || msg.starts_with("agentd boot") => Some(msg.clone()),
        _ => None,
    }
}

fn field(msg: &str, key: &str) -> Option<f64> {
    msg.split_whitespace().find_map(|w| w.strip_prefix(key)?.strip_prefix('=')?.parse().ok())
}

async fn run(sb: &Sandbox, sessions: usize, turns_per: usize, cancel: bool) -> Result<()> {
    let stamp = chrono_like_utc();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/m4").join(&stamp);
    std::fs::create_dir_all(out.join("audio"))?;
    let mut rec = Recorder {
        events: std::fs::File::create(out.join("events.jsonl"))?,
        store: AudioStore::new(out.join("audio"))?,
        out: out.clone(),
    };
    let clock = Instant::now();
    let mut setups: Vec<Value> = Vec::new();
    let mut turns: Vec<TurnObs> = Vec::new();
    let mut logs: Vec<String> = Vec::new();
    let mut prompt_i = 0;
    let mut cancel_obs = None;

    for s in 1..=sessions {
        start_agent(sb).await?;
        let (mut link, seen) = Link::attach(&format!("m4-s{s}"), clock).await?;
        for (t, env) in &seen {
            rec.log(*t, env)?;
            logs.extend(backend_log(env));
        }

        let boot_id = seen.iter().find_map(|(_, e)| match &e.event {
            Event::Hello { boot_id, .. } => Some(boot_id.clone()),
            _ => None,
        });
        let ready = wait_ready(sb, boot_id.as_deref().unwrap_or(""), &mut logs).await?;
        setups.push(json!({"session": s, "line": ready, "setup_to_ready_ms": field(&ready, "setup_to_ready_ms"),
            "ws_connect_ms": field(&ready, "ws_connect_ms"), "setup_ms": field(&ready, "setup_ms")}));
        eprintln!("session {s}: {ready}");

        for _ in 0..turns_per {
            let prompt = PROMPTS[prompt_i];
            prompt_i += 1;
            let turn = one_turn(&mut link, &mut rec, &mut logs, s, prompt, None).await?;
            eprintln!(
                "  turn {}: send→first audio {} ms · {:?} · {:.2} s audio · {:?}",
                turns.len() + 1,
                turn.first_audio.map(|f| format!("{:.0}", f - turn.sent)).unwrap_or("-".into()),
                turn.status,
                turn.audio_s,
                turn.transcript.trim()
            );
            turns.push(turn);
        }
        if cancel && s == sessions {
            let turn = one_turn(&mut link, &mut rec, &mut logs, s, CANCEL_PROMPT, Some(Duration::from_millis(1500))).await?;

            let tail = linger(&mut link, &mut rec, &mut logs, Duration::from_secs(45), turn.response_id.clone()).await?;
            eprintln!(
                "  cancel: {:?}, cancel→terminal {} ms, chunks between cancel and terminal {}, after terminal {} (late frames seen {tail})",
                turn.status,
                turn.done.zip(turn.cancel_sent).map(|(d, c)| format!("{:.1}", d - c)).unwrap_or("-".into()),
                turn.chunks_after_cancel_sent,
                turn.chunks_after_done
            );
            cancel_obs = Some((turn, tail));
        }
    }

    let journal = sh(sb, "cat /rth/journal.jsonl").await.unwrap_or_default();
    let agent_log = sh(sb, "tail -c 200000 /rth/agentd.log").await.unwrap_or_default();
    let mut facts = secret_facts(sb).await?;
    let key = std::env::var(KEY_ENV)?;
    facts["guest_journal_bytes"] = json!(journal.len());
    facts["guest_journal_contains_real_key"] = json!(journal.contains(&key));
    facts["guest_agentd_log_contains_real_key"] = json!(agent_log.contains(&key));
    facts["placeholder_seen_in_guest_journal"] = json!(journal.contains("$MSB_"));
    let (scanned, hits) = scan_for(&key, &[&msb_home(), &out]);
    facts["host_files_scanned"] = json!(scanned);
    facts["host_files_containing_real_key"] = json!(hits);
    std::fs::write(out.join("guest-agentd.log"), &agent_log)?;

    let journal_logs: Vec<&str> = journal.lines().filter(|l| l.contains(r#""kind":"log""#)).collect();
    std::fs::write(out.join("guest-journal-logs.jsonl"), journal_logs.join("\n") + "\n")?;
    std::fs::write(out.join("backend-log.txt"), logs.join("\n") + "\n")?;

    let summary = summarize(&setups, &turns, cancel_obs.as_ref(), facts, &rec);
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary)?)?;
    write_transcript(&out, &turns, cancel_obs.as_ref().map(|(t, _)| t))?;
    println!("{}", serde_json::to_string_pretty(&summary["latency"])?);
    println!("results: {}", out.display());
    Ok(())
}

async fn wait_ready(sb: &Sandbox, boot_id: &str, logs: &mut Vec<String>) -> Result<String> {
    let script = format!("grep -F '\"producer\":\"guest-{boot_id}\"' /rth/journal.jsonl | grep -F '\"msg\":\"gemini ' | head -n 3");
    for _ in 0..60 {
        let out = sh(sb, &script).await?;
        for line in out.lines() {
            let Ok(env) = serde_json::from_str::<Envelope>(line) else { continue };
            let Some(msg) = backend_log(&env) else { continue };
            if msg.starts_with("gemini ready") {
                if !logs.contains(&msg) {
                    logs.push(msg.clone());
                }
                return Ok(msg);
            }
            if msg.starts_with("gemini connect") {
                bail!("backend could not connect: {msg}");
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("no gemini ready line for boot {boot_id} within ~30 s")
}

async fn one_turn(
    link: &mut Link,
    rec: &mut Recorder,
    logs: &mut Vec<String>,
    session: usize,
    prompt: &str,
    cancel_after: Option<Duration>,
) -> Result<TurnObs> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request_id = format!("m4-q{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let (sent, env) = link.send(Event::UserText { request_id: request_id.clone(), text: prompt.into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }).await?;
    rec.log(sent, &env)?;
    let mut turn = TurnObs { session, prompt: prompt.into(), request_id, sent, ..Default::default() };
    let deadline = Instant::now() + Duration::from_secs(60);
    while turn.done.is_none() {
        if Instant::now() > deadline {
            bail!("turn {:?} did not end within 60 s", prompt);
        }
        if let (Some(after), Some(first), None) = (cancel_after, turn.first_audio, turn.cancel_sent)
            && link.now_ms() - first >= after.as_secs_f64() * 1000.0
        {
            let rid = turn.response_id.clone().unwrap();
            let (t, env) = link.send(Event::Cancel { response_id: rid.clone() }).await?;
            rec.log(t, &env)?;
            turn.cancel_sent = Some(t);

            if let Some(Ok(saved)) = rec.store.end(&rid) {
                turn.wav = Some(saved.path.file_name().unwrap().to_string_lossy().to_string());
                turn.audio_s = saved.samples as f64 / f64::from(saved.sample_rate.max(1));
            }
        }
        let Some((t, env)) = link.recv(Duration::from_millis(50)).await? else { continue };
        rec.log(t, &env)?;
        logs.extend(backend_log(&env));
        observe(rec, &mut turn, t, &env);
    }
    Ok(turn)
}

async fn linger(link: &mut Link, rec: &mut Recorder, logs: &mut Vec<String>, wait: Duration, rid: Option<String>) -> Result<u64> {
    let deadline = Instant::now() + wait;
    let mut late = 0;
    while Instant::now() < deadline {
        let Some((t, env)) = link.recv(Duration::from_millis(200)).await? else { continue };
        rec.log(t, &env)?;
        if let Some(line) = backend_log(&env) {
            let tail_done = line.starts_with("gemini cancelled response");
            logs.push(line);
            if tail_done {
                break;
            }
        }
        match &env.event {
            Event::AgentAudio { response_id, chunk, sample_offset, sample_rate, format, data } if Some(response_id) == rid.as_ref() => {
                late += 1;
                let _ = rec.store.chunk(response_id, *chunk, *sample_offset, *sample_rate, format, data);
            }
            Event::AgentDelta { response_id, .. } if Some(response_id) == rid.as_ref() => late += 1,
            _ => {}
        }
    }
    Ok(late)
}

fn stats(xs: &[f64]) -> Value {
    if xs.is_empty() {
        return json!({"n": 0});
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = |p: f64| v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    let round = |x: f64| (x * 10.0).round() / 10.0;
    json!({"n": v.len(), "min": round(v[0]), "p50": round(rank(0.5)), "p95": round(rank(0.95)), "max": round(v[v.len() - 1]),
        "mean": round(v.iter().sum::<f64>() / v.len() as f64), "unit": "ms", "method": "nearest-rank"})
}

fn summarize(setups: &[Value], turns: &[TurnObs], cancel: Option<&(TurnObs, u64)>, secret: Value, rec: &Recorder) -> Value {
    let pick = |f: &dyn Fn(&TurnObs) -> Option<f64>| turns.iter().filter_map(f).collect::<Vec<_>>();
    let setup_ms: Vec<f64> = setups.iter().filter_map(|s| s["setup_to_ready_ms"].as_f64()).collect();
    let cancel_v = cancel.map(|(t, late)| {
        json!({
            "response_id": t.response_id, "status": t.status, "prompt": t.prompt,
            "cancel_sent_after_first_audio_ms": t.cancel_sent.zip(t.first_audio).map(|(c, f)| c - f),
            "cancel_to_terminal_ms": t.done.zip(t.cancel_sent).map(|(d, c)| d - c),
            "chunks_before_cancel": t.chunks - t.chunks_after_cancel_sent,
            "chunks_in_flight_between_cancel_and_terminal_refused": t.chunks_after_cancel_sent - t.chunks_after_done,
            "chunks_after_terminal": t.chunks_after_done,
            "deltas_after_terminal": t.deltas_after_done,
            "late_frames_during_linger": late,
            "audio_kept_s": t.audio_s, "wav": t.wav,
        })
    });
    json!({
        "model": "gemini-3.8-live",
        "endpoint": "Developer API BidiGenerateContent (wss), key via microsandbox secret injection",
        "clock": "host monotonic (Instant) for send/receive; guest-side figures are the guest producer clock",
        "latency": {
            "setup_to_ready_ms (guest: websocket connect + setup → setupComplete, one per session)": stats(&setup_ms),
            "send_to_first_audio_ms (host: UserText written → first AgentAudio read)": stats(&pick(&|t| t.first_audio.map(|f| f - t.sent))),
            "send_to_first_transcript_ms (host)": stats(&pick(&|t| t.first_text.map(|f| f - t.sent))),
            "send_to_response_started_ms (host)": stats(&pick(&|t| t.started.map(|f| f - t.sent))),
            "response_started_to_first_audio_ms (guest clock)": stats(&pick(&|t| t.guest_first_audio_ms)),
            "send_to_done_ms (host)": stats(&pick(&|t| t.done.map(|f| f - t.sent))),
        },
        "turns": turns.iter().map(|t| json!({
            "session": t.session, "prompt": t.prompt, "response_id": t.response_id, "status": t.status,
            "send_to_first_audio_ms": t.first_audio.map(|f| f - t.sent), "send_to_done_ms": t.done.map(|f| f - t.sent),
            "audio_chunks": t.chunks, "audio_s": t.audio_s, "chunk_gaps": t.gaps, "wav": t.wav,
            "transcript": t.transcript.trim(),
        })).collect::<Vec<_>>(),
        "completed_turns": turns.iter().filter(|t| t.status == Some(ResponseStatus::Completed)).count(),
        "setups": setups,
        "cancel": cancel_v,
        "host_audio_refused_after_end": rec.store.refused_after_end,
        "secret": secret,
        "out_dir": rec.out.display().to_string(),
    })
}

fn write_transcript(out: &Path, turns: &[TurnObs], cancel: Option<&TurnObs>) -> Result<()> {
    let mut f = std::fs::File::create(out.join("transcript.md"))?;
    writeln!(f, "# M4a transcript (gemini-3.8-live, output audio transcription)\n")?;
    for (i, t) in turns.iter().chain(cancel).enumerate() {
        writeln!(f, "{}. **you:** {}  \n   **rth:** {}  \n   _{:?}, {:.2} s audio, {}_\n", i + 1, t.prompt, t.transcript.trim(), t.status, t.audio_s, t.wav.as_deref().unwrap_or("no wav"))?;
    }
    Ok(())
}

fn msb_home() -> PathBuf {
    std::env::var_os("MSB_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".microsandbox"))
}

fn scan_for(needle: &str, roots: &[&Path]) -> (u64, Vec<String>) {
    let mut scanned = 0;
    let mut hits = Vec::new();
    let mut stack: Vec<PathBuf> = roots.iter().map(|p| p.to_path_buf()).collect();
    let n = needle.as_bytes();
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() && meta.len() <= 64 << 20 {
            if let Ok(bytes) = std::fs::read(&p) {
                scanned += 1;
                if bytes.windows(n.len()).any(|w| w == n) {
                    hits.push(p.display().to_string());
                }
            }
        }
    }
    (scanned, hits)
}

fn chrono_like_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);

    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}{mo:02}{d:02}T{h:02}{m:02}{s:02}Z")
}
