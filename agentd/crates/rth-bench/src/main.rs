

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rth_bench::fixture::Split;
use rth_bench::run::{RunOptions, rescore, run};
use rth_bench::{default_agent_dir, default_bench_dir, registry, vm};

struct Args {
    positional: Vec<String>,
    bench_dir: PathBuf,
    agent_dir: PathBuf,
    results_dir: Option<PathBuf>,
    splits: Vec<Split>,
    keep_vm: bool,
    agent: Option<std::net::SocketAddr>,
    journal: Option<PathBuf>,
    state_dir: Option<PathBuf>,
}

fn parse() -> Result<Args> {
    let mut a = Args { positional: vec![], bench_dir: default_bench_dir(), agent_dir: default_agent_dir(), results_dir: None, splits: vec![], keep_vm: false, agent: None, journal: None, state_dir: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--bench-dir" => a.bench_dir = PathBuf::from(value()?),
            "--agent-dir" => a.agent_dir = PathBuf::from(value()?),
            "--results-dir" => a.results_dir = Some(PathBuf::from(value()?)),
            "--split" => a.splits.push(match value()?.as_str() {
                "dev" => Split::Dev,
                "promotion" => Split::Promotion,
                "probe" => Split::Probe,
                other => bail!("unknown split {other}"),
            }),
            "--keep-vm" => a.keep_vm = true,
            "--agent" => {
                let addr: std::net::SocketAddr = value()?.parse()?;
                if !addr.ip().is_loopback() || addr.port() == rth_protocol::AGENT_PORT {
                    bail!("--agent must be a loopback address and never 127.0.0.1:7700 (rth-core)");
                }
                a.agent = Some(addr);
            }
            "--journal" => a.journal = Some(PathBuf::from(value()?)),
            "--state-dir" => a.state_dir = Some(PathBuf::from(value()?)),
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            _ if arg.starts_with("--") => bail!("unknown flag {arg}"),
            _ => a.positional.push(arg),
        }
    }
    if a.splits.is_empty() {
        a.splits = vec![Split::Dev, Split::Promotion, Split::Probe];
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> Result<()> {
    let a = parse()?;
    let arg = |i: usize, what: &str| a.positional.get(i).cloned().with_context(|| format!("missing {what}"));
    match a.positional.first().map(String::as_str) {
        Some("run") => {
            let opts = RunOptions {
                results_dir: a.results_dir.clone().unwrap_or_else(|| a.bench_dir.join("results")),
                bench_dir: a.bench_dir.clone(),
                agent_dir: if a.agent.is_some() { a.agent_dir.clone() } else { a.agent_dir.canonicalize().with_context(|| format!("agent dir {}", a.agent_dir.display()))? },
                suite_id: arg(1, "suite id")?,
                splits: a.splits.clone(),
                keep_vm: a.keep_vm,
                agent: a.agent,
                journal: a.journal.clone().or_else(|| a.state_dir.as_ref().map(|d| d.join("journal.jsonl"))),
                state_dir: a.state_dir.clone(),
            };
            let out = run(&opts).await?;
            print_summary(&out.scorecard);
            println!("results: {}", out.run_dir.display());
        }
        Some("verify") => {
            let reg = registry::load(&a.bench_dir, &arg(1, "suite id")?)?;
            println!(
                "ok: suite {} epoch {} ({} tasks, {} dev, {} promotion, {} probe fixtures, split manifest {})",
                reg.suite.suite_id,
                reg.suite.epoch,
                reg.tasks.len(),
                reg.suite.split_manifest.dev.len(),
                reg.suite.split_manifest.promotion.len(),
                reg.suite.probes.len(),
                &reg.suite.split_manifest.sha256[..12]
            );
        }
        Some("seal") => {
            for f in registry::seal(&a.bench_dir, &arg(1, "suite id")?)? {
                println!("sealed {f}");
            }
        }
        Some("score") => println!("{}", serde_json::to_string_pretty(&rescore(&PathBuf::from(arg(1, "run dir")?), &a.bench_dir)?)?),
        Some("stop") => vm::stop()?,
        _ => bail!("usage: rth-bench run|verify|seal|score|stop … (see --help)"),
    }
    Ok(())
}

fn print_summary(card: &serde_json::Value) {
    println!("suite {} · eligible: {} · {}", card["suite_id"], card["eligible"], card["activation"]);
    println!("aggregate dev {:.3} · promotion {:.3}", card["aggregate"]["dev"].as_f64().unwrap_or(0.0), card["aggregate"]["promotion"].as_f64().unwrap_or(0.0));
    for t in card["tasks"].as_array().into_iter().flatten() {
        let s = |split: &str| format!("{}/{} {}", t["splits"][split]["pass"], t["splits"][split]["cases"], t["splits"][split]["status"].as_str().unwrap_or("?"));
        println!("  {:<38} dev {:<16} promotion {}", t["task"].as_str().unwrap_or("?"), s("dev"), s("promotion"));
    }
    for p in card["probes"].as_array().into_iter().flatten() {
        println!("  probe {:<32} {}/{} {}", p["task"].as_str().unwrap_or("?"), p["pass"], p["cases"], p["status"].as_str().unwrap_or("?"));
    }
    let l = &card["latency"]["probe_cancel_to_terminal_ms"];
    if l["n"].as_u64().unwrap_or(0) > 0 {
        println!("cancel→terminal (probes): n={} p50={} p95={} max={} ms", l["n"], l["p50"], l["p95"], l["max"]);
    }
    for (name, key) in [("first audio", "first_audio_ms"), ("barge-in flush", "flush_us"), ("barge-in→terminal", "barge_in_to_terminal_ms")] {
        let l = &card["latency"][key];
        if l["n"].as_u64().unwrap_or(0) > 0 {
            println!("{name} (scenarios): n={} p50={} p95={} max={} {}", l["n"], l["p50"], l["p95"], l["max"], l["unit"].as_str().unwrap_or(""));
        }
    }
    for v in card["vetoes"].as_array().into_iter().flatten() {
        println!("  VETO {} {} #{}: {} {}", v["split"].as_str().unwrap_or(""), v["scenario_id"].as_str().unwrap_or(""), v["rep"], v["check"].as_str().unwrap_or(""), v["detail"]);
    }
    for r in card["eligibility"].as_array().into_iter().flatten().filter(|r| r["pass"] == false) {
        println!("  ineligible: {} ({})", r["rule"].as_str().unwrap_or(""), r["detail"].as_str().unwrap_or(""));
    }
}
