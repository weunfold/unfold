

use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let need = |k: &str| {
        var(k).unwrap_or_else(|| {
            eprintln!("rth-memory-birth: {k} is required");
            std::process::exit(2)
        })
    };
    let instance = need("RTH_MEMORY_INSTANCE");
    let binding = rth_memory_brain::Binding {
        instance_id: instance.clone(),
        brain_id: need("RTH_MEMORY_BRAIN_ID"),
        binding_rev: var("RTH_MEMORY_BINDING_REV").and_then(|v| v.parse().ok()).unwrap_or(1),
        mcp_bin: PathBuf::from(var("RTH_MEMORY_BRAIN_MCP").unwrap_or_else(|| format!("{}/.brain/bin/brain-mcp", std::env::var("HOME").unwrap_or_default()))),
        principal: var("RTH_MEMORY_BRAIN_PRINCIPAL").unwrap_or_else(|| format!(r#"{{"subject":"rth-{instance}","installationId":"rth","entitlement":"brain","host":"rth-agentd"}}"#)),
        runtime_dir: var("RTH_MEMORY_BRAIN_RUNTIME"),
        index: PathBuf::from(need("RTH_MEMORY_BINDING_INDEX")),
        read_timeout: Duration::from_secs(10),
        write_timeout: Duration::from_secs(10),
        no_network: var("RTH_MEMORY_BRAIN_SANDBOX").as_deref() != Some("0"),
        record: PathBuf::from(need("RTH_MEMORY_BRAIN_RECORD")),
        home: Some(PathBuf::from(need("RTH_MEMORY_BRAIN_HOME"))),
        database: Some(need("RTH_MEMORY_BRAIN_DATABASE")),
        seam: None,
    };
    match rth_memory_brain::Brain::birth(&binding, &need("RTH_MEMORY_NONCE")) {
        Ok(r) => println!("{}", serde_json::to_string_pretty(&r).unwrap()),
        Err(f) => {
            eprintln!("rth-memory-birth: {f:?}");
            std::process::exit(1)
        }
    }
}
