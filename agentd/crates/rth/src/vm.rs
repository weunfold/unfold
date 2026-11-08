

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use microsandbox::{Sandbox, Volume};
use rth_protocol::AGENT_PORT;

pub const VM_NAME: &str = "rth-core";
pub const STATE_VOLUME: &str = "rth-state";

pub struct Booted {
    pub boot_ms: u128,
    pub agent_started: bool,
}

pub async fn ensure(agent_dir: &Path, restart_agent: bool) -> Result<Booted> {
    if managed() {
        return ensure_managed(restart_agent).await;
    }
    let started = Instant::now();

    let _ = Volume::builder(STATE_VOLUME).create().await;

    let dir = agent_dir.to_string_lossy().to_string();
    let sandbox = Sandbox::builder(VM_NAME)
        .image("alpine")
        .cpus(2)
        .memory(1024)

        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), AGENT_PORT, AGENT_PORT)
        .volume("/rth", |m| m.named(STATE_VOLUME))
        .volume("/opt/rth", |m| m.bind(&dir).readonly())
        .detached(true)
        .connect_or_create()
        .await
        .context("converging on the rth-core microVM")?;
    let boot_ms = started.elapsed().as_millis();

    let script = if restart_agent {
        "pkill rth-agentd; sleep 0.2; START=1"
    } else {
        "pidof rth-agentd >/dev/null && START=0 || START=1"
    };
    let out = sandbox
        .shell(format!(
            "{script}; if [ $START = 1 ]; then RTH_JOURNAL=/rth/journal.jsonl \
             nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & echo started; else echo running; fi"
        ))
        .await
        .context("starting rth-agentd inside the VM")?;
    let agent_started = out.stdout()?.trim() == "started";

    Ok(Booted { boot_ms, agent_started })
}

fn layout() -> rth_deploy::controller::Layout {
    rth_deploy::controller::Layout::new(&rth_deploy::data_dir())
}

pub fn managed() -> bool {
    layout().box_dir(VM_NAME).join(rth_deploy::record::RECORD_FILE).exists()
}

async fn ensure_managed(restart_agent: bool) -> Result<Booted> {
    let started = Instant::now();
    let mut c = rth_deploy::controller::Controller::open(&layout(), VM_NAME).context("opening the rth-core controller record")?;
    let agent_started = if restart_agent {
        c.restart().await.context("controller restart of the active deployment")?;
        true
    } else {
        let outcome = c.reconcile().await.context("controller reconcile")?;
        !outcome.starts_with("verified")
    };
    Ok(Booted { boot_ms: started.elapsed().as_millis(), agent_started })
}
