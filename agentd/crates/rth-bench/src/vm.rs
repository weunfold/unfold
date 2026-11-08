

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use microsandbox::{Sandbox, Volume};
use rth_protocol::AGENT_PORT;

pub const VM_NAME: &str = "rth-bench-vm";
pub const STATE_VOLUME: &str = "rth-bench-state";

pub const HOST_PORT: u16 = 7710;

pub struct Booted {
    pub sandbox: Sandbox,
    pub boot_ms: u128,

    pub guest_agent_sha256: String,
}

pub async fn ensure(agent_dir: &Path, cpus: u8, memory_mib: u32) -> Result<Booted> {
    let started = Instant::now();
    let _ = Volume::builder(STATE_VOLUME).create().await;

    let dir = agent_dir.to_string_lossy().to_string();
    let sandbox = Sandbox::builder(VM_NAME)
        .image("alpine")
        .cpus(cpus)
        .memory(memory_mib)

        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), HOST_PORT, AGENT_PORT)
        .volume("/rth", |m| m.named(STATE_VOLUME))
        .volume("/opt/rth", |m| m.bind(&dir).readonly())
        .detached(true)
        .connect_or_create()
        .await
        .context("converging on the rth-bench-vm microVM")?;
    let boot_ms = started.elapsed().as_millis();

    let out = sandbox
        .shell(
            "pkill rth-agentd; sleep 0.2; RTH_JOURNAL=/rth/journal.jsonl \
             nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & echo started",
        )
        .await
        .context("starting rth-agentd inside rth-bench-vm")?;
    if out.stdout()?.trim() != "started" {
        bail!("agent start script said {:?}", out.stdout()?);
    }
    let hash = sandbox.shell("sha256sum /opt/rth/rth-agentd").await.context("hashing the guest binary")?;
    let guest_agent_sha256 = hash.stdout()?.split_whitespace().next().unwrap_or_default().to_string();
    Ok(Booted { sandbox, boot_ms, guest_agent_sha256 })
}

pub async fn backend_line(sandbox: &Sandbox) -> Result<String> {
    let out = sandbox
        .shell("tail -c 262144 /rth/journal.jsonl | grep '\"agentd boot ' | tail -n 1")
        .await
        .context("reading the agent journal")?;
    Ok(out.stdout()?.trim().to_string())
}

pub async fn guest_uname(sandbox: &Sandbox) -> String {
    match sandbox.shell("uname -a").await {
        Ok(out) => out.stdout().unwrap_or_default().trim().to_string(),
        Err(e) => format!("unavailable: {e}"),
    }
}

pub fn stop() -> Result<()> {
    let status = std::process::Command::new("msb").args(["stop", VM_NAME]).status().context("running msb stop")?;
    if !status.success() {
        bail!("msb stop {VM_NAME} exited with {status}");
    }
    Ok(())
}
