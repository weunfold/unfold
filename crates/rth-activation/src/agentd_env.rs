

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct MemoryEnv {
    pub brain_id: String,
    pub brain_home: PathBuf,

    pub control_db: String,
    pub brain_mcp: PathBuf,
    pub brain_src: PathBuf,
    pub seam: PathBuf,
    pub bun: PathBuf,

    pub egress: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentLaunch {
    pub instance_root: PathBuf,
    pub bind: SocketAddr,

    pub backend: String,
    pub delegate: bool,
    pub memory: Option<MemoryEnv>,

    pub instance_id: Option<String>,

    pub path: String,
    pub home: PathBuf,
}

pub fn accepted_runtime(instance_root: &Path) -> io::Result<PathBuf> {
    let p = instance_root.join("memory-service.json");
    let v = crate::record::read_json_strict(&p)?.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{}: no accepted memory service", p.display())))?;
    v.get("runtime_dir").and_then(serde_json::Value::as_str).filter(|r| r.starts_with('/')).map(PathBuf::from).ok_or_else(|| io::Error::other(format!("{}: no absolute runtime_dir", p.display())))
}

pub fn state_dir(instance_root: &Path) -> PathBuf {
    instance_root.join("agentd")
}

pub fn minted_instance(instance_root: &Path) -> io::Result<Option<String>> {
    let p = state_dir(instance_root).join("state.json");
    let Some(v) = crate::record::read_json_strict(&p)? else { return Ok(None) };
    v.get("instance_id").and_then(serde_json::Value::as_str).map(|s| Some(s.to_string())).ok_or_else(|| io::Error::other(format!("{}: no instance_id", p.display())))
}

pub fn agentd_env(l: &AgentLaunch, key: Option<&str>) -> io::Result<Vec<(String, String)>> {
    if !l.bind.ip().is_loopback() || l.bind.port() == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("agentd must listen on loopback with a port, not {}", l.bind)));
    }
    if !std::fs::symlink_metadata(&l.instance_root)?.file_type().is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} is not a directory", l.instance_root.display())));
    }
    let state = state_dir(&l.instance_root);
    match std::fs::symlink_metadata(&state) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not a directory", state.display()))),
        Err(e) => return Err(io::Error::new(e.kind(), format!("{}: {e} (recovery, never recreated here)", state.display()))),
    }
    match (minted_instance(&l.instance_root)?, &l.instance_id) {
        (None, None) => {}
        (Some(found), Some(want)) if &found == want => {}
        (Some(found), None) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} already holds {found}: not a fresh mint", state.display()))),
        (found, Some(want)) => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} holds {found:?}, not the recorded {want} (recovery)", state.display()))),
    }
    let s = |p: &Path| p.display().to_string();
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), l.path.clone()),
        ("HOME".into(), s(&l.home)),
        ("RTH_AGENT_MODE".into(), "desktop".into()),
        ("RTH_INSTANCE_ROOT".into(), s(&l.instance_root)),
        ("RTH_STATE".into(), s(&state.join("state.json"))),
        ("RTH_JOURNAL".into(), s(&state.join("journal.jsonl"))),
        ("RTH_AGENT_BIND".into(), l.bind.to_string()),
        ("RTH_BACKEND".into(), l.backend.clone()),
    ];
    let mut tools = Vec::new();
    if l.delegate {
        env.push(("RTH_DELEGATE".into(), "1".into()));
        tools.push("delegate");
    }
    if let Some(m) = &l.memory {
        tools.push("memory");
        let runtime = accepted_runtime(&l.instance_root)?;
        env.extend([
            ("RTH_MEMORY".into(), "1".into()),
            ("RTH_MEMORY_PROVIDER".into(), "brain".into()),
            ("RTH_MEMORY_BRAIN_ID".into(), m.brain_id.clone()),
            ("RTH_MEMORY_BRAIN_HOME".into(), s(&m.brain_home)),
            ("RTH_MEMORY_BRAIN_DATABASE".into(), m.control_db.clone()),
            ("RTH_MEMORY_BRAIN_MCP".into(), s(&m.brain_mcp)),
            ("RTH_MEMORY_BRAIN_SRC".into(), s(&m.brain_src)),
            ("RTH_MEMORY_BRAIN_SEAM".into(), s(&m.seam)),
            ("RTH_MEMORY_BUN".into(), s(&m.bun)),
            ("RTH_MEMORY_BRAIN_RUNTIME".into(), s(&runtime)),
        ]);
        if let Some(e) = &m.egress {
            env.push(("RTH_MEMORY_EGRESS".into(), e.clone()));
        }
    }
    if !tools.is_empty() && l.backend == "openai-realtime" {
        env.push(("RTH_OPENAI_TOOLS".into(), tools.join(",")));
    }
    if let Some(k) = key {
        crate::keychain::check_format(k).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        env.push(("OPENAI_API_KEY".into(), k.to_string()));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(root: &Path, memory: bool) -> AgentLaunch {
        AgentLaunch {
            instance_root: root.to_path_buf(),
            bind: "127.0.0.1:7801".parse().unwrap(),
            backend: "openai-realtime".into(),
            delegate: true,
            memory: memory.then(|| MemoryEnv {
                brain_id: "brn_fixture".into(),
                brain_home: root.join("brain-home"),
                control_db: "brain_rth_interim_fixture_ctl".into(),
                brain_mcp: "/Applications/RTH.app/Contents/MacOS/brain-mcp".into(),
                brain_src: "/Applications/RTH.app/Contents/Resources/brain/src".into(),
                seam: "/Applications/RTH.app/Contents/Resources/brain/memory-search.ts".into(),
                bun: "/Applications/RTH.app/Contents/MacOS/bun".into(),
                egress: None,
            }),
            instance_id: None,
            path: "/usr/bin:/bin".into(),
            home: "/Users/fixture".into(),
        }
    }
    fn root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rth-agentd-env-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("agentd")).unwrap();
        d
    }
    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn the_desktop_contract_is_complete_and_nothing_is_created() {
        let r = root("contract");
        let env = agentd_env(&launch(&r, false), None).unwrap();
        assert_eq!(get(&env, "RTH_AGENT_MODE"), Some("desktop"));
        assert_eq!(get(&env, "RTH_INSTANCE_ROOT"), Some(r.to_str().unwrap()));
        assert_eq!(get(&env, "RTH_STATE"), Some(r.join("agentd/state.json").to_str().unwrap()));
        assert_eq!(get(&env, "RTH_JOURNAL"), Some(r.join("agentd/journal.jsonl").to_str().unwrap()));
        assert_eq!(get(&env, "RTH_AGENT_BIND"), Some("127.0.0.1:7801"));
        assert_eq!(std::fs::read_dir(r.join("agentd")).unwrap().count(), 0, "nothing written");
        assert_eq!(get(&env, "OPENAI_API_KEY"), None, "no key unless given");
        assert_eq!(get(&env, "RTH_OPENAI_TOOLS"), Some("delegate"));
    }

    #[test]
    fn the_key_is_the_one_credential_and_memory_comes_from_the_accepted_service() {
        let r = root("memory");
        assert!(agentd_env(&launch(&r, true), None).is_err(), "memory without an accepted memory-service.json refuses");
        std::fs::write(r.join("memory-service.json"), br#"{"runtime_dir":"/private/tmp/fixture-runtime","port":5432}"#).unwrap();
        let env = agentd_env(&launch(&r, true), Some("sk-test-fictional-0000000000000003")).unwrap();
        assert_eq!(get(&env, "OPENAI_API_KEY"), Some("sk-test-fictional-0000000000000003"));
        assert_eq!(get(&env, "RTH_MEMORY_BRAIN_RUNTIME"), Some("/private/tmp/fixture-runtime"));
        assert_eq!(get(&env, "RTH_MEMORY_BRAIN_ID"), Some("brn_fixture"));
        assert_eq!(get(&env, "RTH_OPENAI_TOOLS"), Some("delegate,memory"));
        assert_eq!(get(&env, "RTH_MEMORY_EGRESS"), None, "egress stays agentd's default (deny) unless the decision is recorded");
        let creds: Vec<_> = env.iter().filter(|(k, _)| k.contains("KEY") || k.contains("SECRET") || k.contains("TOKEN")).map(|(k, _)| k.as_str()).collect();
        assert_eq!(creds, vec!["OPENAI_API_KEY"], "exactly one credential");
        assert!(agentd_env(&launch(&r, true), Some("not a key")).is_err());
    }

    #[test]
    fn a_non_loopback_bind_or_missing_root_refuses() {
        let r = root("refuse");
        let mut l = launch(&r, false);
        l.bind = "0.0.0.0:7801".parse().unwrap();
        assert!(agentd_env(&l, None).is_err());
        let mut l = launch(&r.join("absent"), false);
        l.bind = "127.0.0.1:7801".parse().unwrap();
        assert!(agentd_env(&l, None).is_err());
        assert!(!r.join("absent").exists(), "the instance root is never created here");
    }

    #[test]
    fn a_missing_state_dir_or_a_wrong_identity_refuses_and_creates_nothing() {
        let r = root("identity");
        std::fs::remove_dir(r.join("agentd")).unwrap();
        assert!(agentd_env(&launch(&r, false), None).is_err());
        assert!(!r.join("agentd").exists(), "never recreated");
        std::fs::create_dir(r.join("agentd")).unwrap();
        let mut l = launch(&r, false);
        l.instance_id = Some("inst-recorded".into());
        assert!(agentd_env(&l, None).is_err(), "an admitted launch without identity refuses (no bootstrap)");
        std::fs::write(r.join("agentd/state.json"), br#"{"instance_id":"inst-other"}"#).unwrap();
        assert!(agentd_env(&l, None).is_err(), "a foreign identity refuses");
        assert!(agentd_env(&launch(&r, false), None).is_err(), "a mint over existing identity refuses");
        std::fs::write(r.join("agentd/state.json"), br#"{"instance_id":"inst-recorded"}"#).unwrap();
        assert!(agentd_env(&l, None).is_ok());
    }
}
