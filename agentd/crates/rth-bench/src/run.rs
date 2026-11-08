

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use rth_protocol::{PROTOCOL_VERSION, utc_ms};
use serde_json::{Value, json};

use crate::fixture::Split;
use crate::registry::{self, LoadedFixture, Registry};
use crate::results::{self, Caps, MANIFEST_FORMAT};
use crate::runner::{self, HelloInfo, RunClock};
use crate::scorer::{self, CaseResult, CaseStatus};
use crate::timeline::Rec;
use crate::util::{sha256_file, sha256_hex, utc_compact, utc_iso, write_new};
use crate::vm;

pub struct RunOptions {
    pub bench_dir: PathBuf,
    pub agent_dir: PathBuf,
    pub results_dir: PathBuf,
    pub suite_id: String,
    pub splits: Vec<Split>,
    pub keep_vm: bool,

    pub agent: Option<SocketAddr>,

    pub journal: Option<PathBuf>,

    pub state_dir: Option<PathBuf>,
}

fn vm_state_dir() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("HOME")?).join(".microsandbox/volumes").join(vm::STATE_VOLUME);
    p.is_dir().then_some(p)
}

pub struct RunOutcome {
    pub run_dir: PathBuf,
    pub scorecard: Value,
}

fn cmd(args: &[&str]) -> String {
    match std::process::Command::new(args[0]).args(&args[1..]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Ok(o) => format!("unavailable (exit {})", o.status),
        Err(e) => format!("unavailable: {e}"),
    }
}

fn host_info(workspace: &Path) -> Value {
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .current_dir(workspace)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|e| format!("unavailable: {e}"));
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "uname": cmd(&["uname", "-srm"]),
        "cpu": cmd(&["sysctl", "-n", "machdep.cpu.brand_string"]),
        "ncpu": cmd(&["sysctl", "-n", "hw.ncpu"]),
        "memsize_bytes": cmd(&["sysctl", "-n", "hw.memsize"]),
        "msb": cmd(&["msb", "--version"]),
        "rustc": rustc,
    })
}

fn timeline_path(run_dir: &Path, f: &LoadedFixture, rep: u32, repeats: u32) -> PathBuf {
    let id = &f.fixture.scenario_id;
    match f.fixture.split {
        Split::Dev => run_dir.join("cases/dev").join(format!("{id}.jsonl")),
        Split::Promotion => run_dir.join("private/cases/promotion").join(format!("{id}.jsonl")),
        Split::Probe if repeats > 1 => run_dir.join("cases/probe").join(format!("{id}.r{rep:02}.jsonl")),
        Split::Probe => run_dir.join("cases/probe").join(format!("{id}.jsonl")),
    }
}

fn plan<'a>(reg: &'a Registry, splits: &[Split]) -> Vec<(&'a LoadedFixture, u32, u32)> {
    let mut out = Vec::new();
    for split in [Split::Dev, Split::Promotion] {
        if splits.contains(&split) {
            out.extend(reg.scenarios(split).into_iter().map(|f| (f, 1, 1)));
        }
    }
    if splits.contains(&Split::Probe) {
        for p in &reg.suite.probes {
            let f = &reg.fixtures[&p.scenario_id];
            out.extend((1..=p.repeats).map(|rep| (f, rep, p.repeats)));
        }
    }
    out
}

pub async fn run(opts: &RunOptions) -> Result<RunOutcome> {
    let reg = registry::load(&opts.bench_dir, &opts.suite_id)?;
    let agent_bin = opts.agent_dir.join("rth-agentd");
    let agent_sha = if opts.agent.is_some() {
        String::new()
    } else {
        ensure!(
            agent_bin.exists(),
            "no rth-agentd in {}. Build it: cargo build -p rth-agentd --release --target aarch64-unknown-linux-musl",
            opts.agent_dir.display()
        );
        sha256_file(&agent_bin)?
    };
    let exe = std::env::current_exe()?;
    let exe_sha = sha256_file(&exe)?;

    let started_ms = utc_ms();
    let tag = sha256_hex(format!("{started_ms}-{}-{:?}", std::process::id(), std::time::Instant::now()).as_bytes())[..8].to_string();
    let run_id = format!("{}-{tag}", utc_compact(started_ms));
    std::fs::create_dir_all(&opts.results_dir)?;
    let run_dir = opts.results_dir.join(&run_id);
    std::fs::create_dir(&run_dir).with_context(|| format!("{} exists; a run is never overwritten", run_dir.display()))?;
    eprintln!("rth-bench: run {run_id} → {}", run_dir.display());

    let clock = RunClock::start();
    let clock_origin_utc_ms = utc_ms();
    let mut ctx = Ctx { reg: &reg, opts, run_dir: &run_dir, run_tag: format!("b{tag}"), clock, cases: vec![], harness: vec![], facts: json!({}) };
    let outcome = ctx.execute(&agent_sha).await;
    if !opts.keep_vm && opts.agent.is_none() {
        if let Err(e) = vm::stop() {
            eprintln!("rth-bench: could not stop {}: {e:#}", vm::VM_NAME);
        }
    }
    let ended_ms = utc_ms();

    let caps = Caps::parse(ctx.facts["backend_boot_line"].as_str().unwrap_or(""));
    let guest_sha = ctx.facts["guest_agent_sha256"].as_str().unwrap_or("").to_string();
    let identity_rule = if opts.agent.is_some() {

        ("candidate_identified_by_hello_deployment".to_string(), guest_sha.len() == 64, format!("hello deployment artifact {guest_sha}"))
    } else {
        ("guest_runs_hashed_agentd".to_string(), guest_sha == agent_sha, format!("host {agent_sha} guest {guest_sha}"))
    };
    let integrity = vec![
        ("run_completed".to_string(), outcome.is_ok(), outcome.as_ref().err().map(|e| format!("{e:#}")).unwrap_or_default()),
        identity_rule,
        ("agent_protocol_matches".to_string(), ctx.facts["hello"]["protocol"].as_u64() == Some(PROTOCOL_VERSION as u64), format!("hello {:?}", ctx.facts["hello"]["protocol"])),
    ];
    let mut card = results::scorecard(&reg, &ctx.cases, &caps, &integrity, &opts.splits);

    results::write_jsonl(&run_dir.join("harness.jsonl"), &ctx.harness)?;
    let (private, public): (Vec<&CaseResult>, Vec<&CaseResult>) = ctx.cases.iter().partition(|c| c.split == Split::Promotion);
    results::write_json(&run_dir.join("case-results.json"), &public)?;
    write_new(
        &run_dir.join("private/EVALUATOR-ONLY.txt"),
        b"Promotion-split timelines and verdicts. Evaluator-only: never expose to a candidate or optimizer.\n",
    )?;
    results::write_json(&run_dir.join("private/promotion-case-results.json"), &private)?;

    let fixtures = |split: Split| -> Vec<Value> {
        reg.fixtures.values().filter(|f| f.fixture.split == split).map(|f| json!({"scenario_id": f.fixture.scenario_id, "sha256": f.sha256})).collect()
    };
    let manifest = json!({
        "manifest_format": MANIFEST_FORMAT,
        "run_id": run_id,
        "status": if outcome.is_ok() { "complete" } else { "aborted" },
        "error": outcome.as_ref().err().map(|e| format!("{e:#}")),
        "started_utc": utc_iso(started_ms),
        "ended_utc": utc_iso(ended_ms),
        "suite": {"id": reg.suite.suite_id, "epoch": reg.suite.epoch, "sha256": reg.suite_sha256, "policy_approval_id": reg.suite.policy_approval_id},
        "tasks": reg.tasks.iter().map(|(r, t)| json!({"task": r.to_string(), "sha256": t.sha256})).collect::<Vec<_>>(),
        "fixtures": {"dev": fixtures(Split::Dev), "promotion": fixtures(Split::Promotion), "probe": fixtures(Split::Probe), "split_manifest_sha256": reg.suite.split_manifest.sha256},
        "splits_run": opts.splits.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        "case_count": ctx.cases.len(),
        "candidate": {
            "agentd_path": if opts.agent.is_some() { Value::Null } else { json!(agent_bin.display().to_string()) },
            "agentd_sha256": if opts.agent.is_some() { json!(guest_sha) } else { json!(agent_sha) },
            "agentd_sha256_in_guest": guest_sha,
            "hello": ctx.facts["hello"],
            "backend_boot_line": ctx.facts["backend_boot_line"],
            "backend": caps.backend,
        },
        "protocol": {"name": "rth-protocol", "version": PROTOCOL_VERSION, "transport": "tcp json-lines", "max_frame": rth_protocol::MAX_FRAME},
        "scorer": {"id": reg.scorer().id, "version": reg.scorer().version, "exe": exe.display().to_string(), "exe_sha256": exe_sha},
        "platform": if let Some(addr) = opts.agent { json!({
            "vm": "external (attached, not restarted)", "agent_addr": addr.to_string(),
            "deployment": ctx.facts["hello"]["deployment"], "agent_restarted_for_run": false,
            "journal": opts.journal.as_ref().map(|p| p.display().to_string()),
            "state_dir": opts.state_dir.as_ref().map(|p| p.display().to_string()),
        }) } else { json!({
            "vm": vm::VM_NAME, "image": "alpine", "cpus": reg.suite.guest.cpus, "memory_mib": reg.suite.guest.memory_mib,
            "state_volume": vm::STATE_VOLUME, "port_bind": format!("127.0.0.1:{} -> guest {}", vm::HOST_PORT, rth_protocol::AGENT_PORT),
            "agent_mount": "/opt/rth (read-only bind of the agent dir)", "microsandbox_crate": "0.7.8",
            "boot_ms": ctx.facts["boot_ms"], "guest_uname": ctx.facts["guest_uname"], "agent_restarted_for_run": true,
        }) },
        "host": host_info(&opts.bench_dir.join("..")),
        "clocks": {
            "runner": "one std::time::Instant for the whole run; every timeline t_ns is ns since it",
            "runner_origin_utc_ms": clock_origin_utc_ms,
            "guest_utc_ms_at_origin": ctx.facts["hello"]["utc_ms_at_origin"],
            "note": "guest envelope t_ns is the guest's own monotonic clock; it is kept as a diagnostic and never subtracted from runner times",
        },
        "concurrency": "cases run one at a time against one agent process; an observer isolation check precedes each case",
        "seeds": null,
        "cache": "none",
        "usage": if caps.backend.as_deref().is_some_and(|b| b.starts_with("fake")) {
            json!({"external_spend_usd": 0.0, "note": format!("{} backend: no provider calls", caps.backend.as_deref().unwrap_or("fake"))})
        } else {
            json!({"external_spend_usd": null, "note": format!("{:?} backend: provider usage is in the agent journal (gemini usage lines)", caps.backend)})
        },
    });
    results::write_json(&run_dir.join("manifest.json"), &manifest)?;

    let mut artifacts = serde_json::Map::new();
    collect_hashes(&run_dir, &run_dir, &mut artifacts)?;
    card["run_id"] = json!(run_id);
    card["started_utc"] = json!(utc_iso(started_ms));
    card["ended_utc"] = json!(utc_iso(ended_ms));
    card["artifacts_sha256"] = Value::Object(artifacts);
    results::write_json(&run_dir.join("scorecard.json"), &card)?;
    results::freeze(&run_dir)?;
    notify_landed(&run_id).await;
    outcome?;
    Ok(RunOutcome { run_dir, scorecard: card })
}

async fn notify_landed(run_id: &str) {
    let Ok(url) = std::env::var("RTH_BENCH_NOTIFY") else { return };
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), post_empty(&url)).await;
    match result {
        Ok(Ok(status)) => eprintln!("rth-bench: notified {url} of {run_id}: {status}"),
        Ok(Err(e)) => eprintln!("rth-bench: notify {url} failed: {e:#}"),
        Err(_) => eprintln!("rth-bench: notify {url} timed out"),
    }
}

async fn post_empty(url: &str) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").context("RTH_BENCH_NOTIFY must be an http:// URL")?;
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let mut stream = tokio::net::TcpStream::connect(authority).await?;
    let request = format!("POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut head = vec![0u8; 256];
    let n = stream.read(&mut head).await?;
    let status = String::from_utf8_lossy(&head[..n]).lines().next().unwrap_or("").to_string();
    ensure!(status.split_whitespace().nth(1) == Some("200"), "unexpected response {status:?}");
    Ok(status)
}

fn collect_hashes(root: &Path, dir: &Path, out: &mut serde_json::Map<String, Value>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.map(|e| e.map(|e| e.path())).collect::<std::io::Result<_>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_hashes(root, &p, out)?;
        } else {
            let rel = p.strip_prefix(root)?.display().to_string();
            out.insert(rel, json!(sha256_file(&p)?));
        }
    }
    Ok(())
}

struct Ctx<'a> {
    reg: &'a Registry,
    opts: &'a RunOptions,
    run_dir: &'a Path,
    run_tag: String,
    clock: RunClock,
    cases: Vec<CaseResult>,
    harness: Vec<Rec>,
    facts: Value,
}

impl Ctx<'_> {
    async fn execute(&mut self, agent_sha: &str) -> Result<()> {
        if let Some(addr) = self.opts.agent {
            return self.execute_attached(addr).await;
        }
        let g = &self.reg.suite.guest;
        eprintln!("rth-bench: converging on {} and restarting rth-agentd …", vm::VM_NAME);
        let booted = vm::ensure(&self.opts.agent_dir, g.cpus, g.memory_mib).await?;
        self.facts["boot_ms"] = json!(booted.boot_ms as u64);
        self.facts["guest_agent_sha256"] = json!(booted.guest_agent_sha256);
        self.facts["guest_uname"] = json!(vm::guest_uname(&booted.sandbox).await);
        if booted.guest_agent_sha256 != agent_sha {
            bail!("guest sees rth-agentd {} but the host hashed {agent_sha}", booted.guest_agent_sha256);
        }
        let addr: SocketAddr = ([127, 0, 0, 1], vm::HOST_PORT).into();

        let plan = plan(self.reg, &self.opts.splits);
        let total = plan.len();
        for (n, (f, rep, repeats)) in plan.into_iter().enumerate() {
            let (iso, verdict) = runner::isolation_check(addr, self.clock, &self.run_tag, n + 1).await;
            self.harness.extend(iso);
            let isolation = match verdict {
                Ok(hello) => {
                    if self.facts.get("hello").is_none() {
                        self.record_agent(&hello, &booted.sandbox).await;
                    }
                    None
                }
                Err(why) => Some(why),
            };
            let storage = vm_state_dir();
            let mut recs = runner::run_case(addr, self.clock, &self.run_tag, &f.fixture, &f.sha256, rep, storage.as_deref()).await;
            if let Some(why) = isolation {
                let t_ns = recs[0].t_ns();
                recs.insert(1, Rec::Note { t_ns, conn: None, code: "isolation_failed".into(), detail: why });
            }
            results::write_jsonl(&timeline_path(self.run_dir, f, rep, repeats), &recs)?;
            let result = scorer::score_any(&f.fixture, self.reg.task(&f.fixture.task), &recs);
            let vetoes: Vec<&str> = result.vetoes().map(|c| c.id.as_str()).collect();
            let label = if f.fixture.split == Split::Promotion { "promotion".to_string() } else { f.fixture.split.as_str().to_string() };
            eprintln!(
                "  [{:>2}/{total}] {label:<9} {:<28} r{rep:<2} {:<13}{}",
                n + 1,
                f.fixture.scenario_id,
                format!("{:?}", result.status).to_lowercase(),
                if vetoes.is_empty() { String::new() } else { format!(" VETO {}", vetoes.join(",")) }
            );
            self.cases.push(result);
        }
        Ok(())
    }

    async fn execute_attached(&mut self, addr: SocketAddr) -> Result<()> {
        let plan = plan(self.reg, &self.opts.splits);
        let total = plan.len();
        for (n, (f, rep, repeats)) in plan.into_iter().enumerate() {
            let (iso, verdict) = runner::isolation_check(addr, self.clock, &self.run_tag, n + 1).await;
            self.harness.extend(iso);
            let isolation = match verdict {
                Ok(hello) => {
                    if self.facts.get("hello").is_none() {
                        self.facts["hello"] = json!(hello);
                        self.facts["guest_agent_sha256"] = json!(hello.artifact_id);
                        let line = self.opts.journal.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|j| {
                            j.lines().rev().find(|l| l.contains(&format!("agentd boot {}", hello.boot_id))).map(str::to_string)
                        });
                        self.facts["backend_boot_line"] = line.map(Value::from).unwrap_or(Value::Null);
                    }
                    None
                }
                Err(why) => Some(why),
            };
            let mut recs = runner::run_case(addr, self.clock, &self.run_tag, &f.fixture, &f.sha256, rep, self.opts.state_dir.as_deref()).await;
            if let Some(why) = isolation {
                let t_ns = recs[0].t_ns();
                recs.insert(1, Rec::Note { t_ns, conn: None, code: "isolation_failed".into(), detail: why });
            }
            results::write_jsonl(&timeline_path(self.run_dir, f, rep, repeats), &recs)?;
            let result = scorer::score_any(&f.fixture, self.reg.task(&f.fixture.task), &recs);
            let vetoes: Vec<&str> = result.vetoes().map(|c| c.id.as_str()).collect();
            eprintln!(
                "  [{:>2}/{total}] {:<9} {:<34} r{rep:<2} {:<13}{}",
                n + 1,
                f.fixture.split.as_str(),
                f.fixture.scenario_id,
                format!("{:?}", result.status).to_lowercase(),
                if vetoes.is_empty() { String::new() } else { format!(" VETO {}", vetoes.join(",")) }
            );
            self.cases.push(result);
        }
        Ok(())
    }

    async fn record_agent(&mut self, hello: &HelloInfo, sandbox: &microsandbox::Sandbox) {
        self.facts["hello"] = json!(hello);
        self.facts["backend_boot_line"] = match vm::backend_line(sandbox).await {
            Ok(line) if !line.is_empty() => json!(line),
            _ => Value::Null,
        };
    }
}

pub fn rescore(run_dir: &Path, bench_dir: &Path) -> Result<Value> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(run_dir.join("manifest.json"))?)?;
    let suite_id = manifest["suite"]["id"].as_str().context("manifest has no suite id")?;
    let reg = registry::load(bench_dir, suite_id)?;
    ensure!(manifest["suite"]["sha256"].as_str() == Some(reg.suite_sha256.as_str()), "suite changed since this run; scores are not comparable across epochs");
    let mut stored: Vec<CaseResult> = serde_json::from_slice(&std::fs::read(run_dir.join("case-results.json"))?)?;
    let private = run_dir.join("private/promotion-case-results.json");
    if private.exists() {
        stored.extend(serde_json::from_slice::<Vec<CaseResult>>(&std::fs::read(&private)?)?);
    }
    let mut files = Vec::new();
    for sub in ["cases/dev", "cases/probe", "private/cases/promotion"] {
        let dir = run_dir.join(sub);
        if dir.exists() {
            for e in std::fs::read_dir(&dir)? {
                files.push(e?.path());
            }
        }
    }
    files.sort();
    let mut fresh = Vec::new();
    let mut mismatches = Vec::new();
    for path in &files {
        let recs = results::read_jsonl(path)?;
        let Some(Rec::Header { scenario_id, fixture_sha256, rep, .. }) = recs.first() else { bail!("{}: no header", path.display()) };
        let f = reg.fixtures.get(scenario_id).with_context(|| format!("{scenario_id} not in suite"))?;
        ensure!(&f.sha256 == fixture_sha256, "{scenario_id}: fixture changed since the run");
        let r = scorer::score_any(&f.fixture, reg.task(&f.fixture.task), &recs);
        let old = stored.iter().find(|c| &c.scenario_id == scenario_id && c.rep == *rep);
        if old.map(|o| (o.status, o.score)) != Some((r.status, r.score)) {
            mismatches.push(format!("{scenario_id}#{rep}: stored {:?} rescored {:?}", old.map(|o| o.status), r.status));
        }
        fresh.push(r);
    }
    let count = |s: CaseStatus| fresh.iter().filter(|c| c.status == s).count();
    Ok(json!({
        "run_dir": run_dir.display().to_string(),
        "cases": fresh.len(),
        "pass": count(CaseStatus::Pass), "fail": count(CaseStatus::Fail),
        "invalid": count(CaseStatus::Invalid), "indeterminate": count(CaseStatus::Indeterminate),
        "vetoes": fresh.iter().flat_map(|c| c.vetoes().map(move |k| format!("{}#{} {}", c.scenario_id, c.rep, k.id))).collect::<Vec<_>>(),
        "matches_stored_verdicts": mismatches.is_empty(),
        "mismatches": mismatches,
    }))
}

#[cfg(test)]
mod tests {
    use super::post_empty;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn notify_posts_to_the_refresh_path() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 512];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let status = post_empty(&format!("http://{addr}/api/rth-capabilities/refresh")).await.unwrap();
        assert_eq!(status, "HTTP/1.1 200 OK");
        assert!(server.await.unwrap().starts_with("POST /api/rth-capabilities/refresh HTTP/1.1\r\n"));
        assert!(post_empty("https://x/").await.is_err());
    }
}
