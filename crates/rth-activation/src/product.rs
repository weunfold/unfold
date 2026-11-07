

use crate::activation::{Cmd, Effects, Reconciled};
use crate::agentd_env::{agentd_env, AgentLaunch, MemoryEnv};
use serde_json::{json, Value};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

pub const MEMORY_EGRESS: &str = "allow:openai-realtime";

#[derive(Clone, Debug)]
pub struct Pinned {
    pub path: PathBuf,
    pub sha256: String,
}

impl Pinned {
    pub fn check(&self) -> io::Result<&Path> {

        let b = crate::record::read_bytes_strict(&self.path)?.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{}: missing", self.path.display())))?;
        let got = crate::boot::sha256_hex(&b);
        if got != self.sha256 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not the pinned build (sha256 {got})", self.path.display())));
        }
        Ok(&self.path)
    }
}

#[derive(Clone, Debug)]
pub struct BrainBundle {
    pub brain_mcp: PathBuf,
    pub brain_src: PathBuf,
    pub seam: PathBuf,
    pub bun: PathBuf,
}

pub trait BrainRegistry {
    fn create(&self, tuple: &Value) -> Cmd;
    fn lookup(&self, tuple: &Value) -> Reconciled;
}

pub struct Product<'a> {

    pub exe: PathBuf,
    pub agentd: Pinned,
    pub birth: Pinned,
    pub brain: BrainBundle,
    pub registry: &'a dyn BrainRegistry,

    pub service: Value,
    pub path: String,
    pub home: PathBuf,
}

fn free_port() -> io::Result<SocketAddr> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    l.local_addr()
}

fn failed(step: &str, e: impl std::fmt::Display) -> Cmd {

    Cmd { argv: vec!["/bin/sh".into(), "-c".into(), "printf '%s\\n' \"$1\" >&2; exit 70".into(), "rth-activate".into(), format!("{step}: {e}")], env: vec![("PATH".into(), "/usr/bin:/bin".into())], cwd: "/".into() }
}

impl Product<'_> {
    fn launch(&self, root: &Path, memory: Option<MemoryEnv>, instance_id: Option<String>) -> io::Result<AgentLaunch> {
        Ok(AgentLaunch { instance_root: root.to_path_buf(), bind: free_port()?, backend: "fake".into(), delegate: false, memory, instance_id, path: self.path.clone(), home: self.home.clone() })
    }

    fn memory(&self, brain: &Value, egress: Option<String>) -> io::Result<MemoryEnv> {
        let t = &brain["tuple"];
        let s = |v: &Value, k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| io::Error::other(format!("the brain record has no {k}")));
        Ok(MemoryEnv {
            brain_id: s(brain, "brain_id")?,
            brain_home: PathBuf::from(s(t, "home")?),
            control_db: s(t, "control_db")?,
            brain_mcp: self.brain.brain_mcp.clone(),
            brain_src: self.brain.brain_src.clone(),
            seam: self.brain.seam.clone(),
            bun: self.brain.bun.clone(),
            egress,
        })
    }

    fn mint_cmd(&self, root: &Path) -> io::Result<Cmd> {
        let agentd = self.agentd.check()?;
        let env = agentd_env(&self.launch(root, None, None)?, None)?;
        Ok(Cmd { argv: vec![s(&self.exe), "--boot-once".into(), s(agentd)], env, cwd: "/".into() })
    }

    fn bind_cmd(&self, root: &Path, brain: &Value, nonce: &Path) -> io::Result<Cmd> {
        let birth = self.birth.check()?;
        let m = self.memory(brain, None)?;
        let state = crate::agentd_env::state_dir(root);
        let instance = crate::agentd_env::minted_instance(root)?.ok_or_else(|| io::Error::other("no minted instance to bind"))?;
        let runtime = crate::agentd_env::accepted_runtime(root)?;

        let env = vec![
            ("PATH".into(), self.path.clone()),
            ("HOME".into(), s(&self.home)),
            ("RTH_MEMORY_INSTANCE".into(), instance),
            ("RTH_MEMORY_BRAIN_ID".into(), m.brain_id),
            ("RTH_MEMORY_BRAIN_MCP".into(), s(&m.brain_mcp)),
            ("RTH_MEMORY_BRAIN_RUNTIME".into(), s(&runtime)),
            ("RTH_MEMORY_BINDING_INDEX".into(), s(&state.join("memory-binding.jsonl"))),
            ("RTH_MEMORY_BRAIN_RECORD".into(), s(&state.join("memory-binding-record.json"))),
            ("RTH_MEMORY_BRAIN_HOME".into(), s(&m.brain_home)),
            ("RTH_MEMORY_BRAIN_DATABASE".into(), m.control_db),
        ];
        Ok(Cmd { argv: vec![s(&self.exe), "--birth".into(), s(birth), s(nonce)], env, cwd: "/".into() })
    }

    fn verify_cmd(&self, root: &Path, brain: &Value, bound: &Value) -> io::Result<Cmd> {
        let agentd = self.agentd.check()?;
        let instance = bound["instance_id"].as_str().map(str::to_string);

        let mut l = self.launch(root, Some(self.memory(brain, None)?), instance)?;
        l.backend = "openai-realtime".into();
        let mut env = agentd_env(&l, None)?;
        env.push(("RTH_VERIFY_BOUND".into(), bound.to_string()));
        Ok(Cmd { argv: vec![s(&self.exe), "--verify-boot".into(), s(agentd), s(&root.join("verify.json"))], env, cwd: "/".into() })
    }
}

fn s(p: &Path) -> String {
    p.display().to_string()
}

impl Effects for Product<'_> {
    fn mint(&self, root: &Path) -> Cmd {
        self.mint_cmd(root).unwrap_or_else(|e| failed("mint", e))
    }
    fn brain_create(&self, tuple: &Value) -> Cmd {
        self.registry.create(tuple)
    }
    fn reconcile_brain(&self, tuple: &Value) -> Reconciled {
        self.registry.lookup(tuple)
    }
    fn bind(&self, root: &Path, brain: &Value, nonce: &Path) -> Cmd {
        self.bind_cmd(root, brain, nonce).unwrap_or_else(|e| failed("bind", e))
    }
    fn verify(&self, root: &Path, brain: &Value, bound: &Value) -> Cmd {
        self.verify_cmd(root, brain, bound).unwrap_or_else(|e| failed("verify", e))
    }
    fn agentd_sha256(&self) -> String {
        self.agentd.sha256.clone()
    }
    fn service_record(&self) -> Option<Value> {
        Some(self.service.clone())
    }
}

pub fn admitted_launch(admitted: &Value, bundle: &BrainBundle, bind: SocketAddr, path: &str, home: &Path, key: Option<&str>) -> io::Result<Vec<(String, String)>> {
    let Some(key) = key.filter(|k| !k.trim().is_empty()) else {
        return Err(io::Error::new(io::ErrorKind::NotFound, NO_KEY));
    };
    let l = admitted_agent(admitted, bundle, bind, path, home, "openai-realtime", true, Some(MEMORY_EGRESS))?;
    agentd_env(&l, Some(key))
}

#[allow(clippy::too_many_arguments)]
fn admitted_agent(admitted: &Value, bundle: &BrainBundle, bind: SocketAddr, path: &str, home: &Path, backend: &str, delegate: bool, egress: Option<&str>) -> io::Result<AgentLaunch> {
    if admitted["activation_state"] != "interim_active" {
        return Err(io::Error::other("not admitted: the activation is not interim_active"));
    }
    let root = PathBuf::from(admitted["instance_root"].as_str().ok_or_else(|| io::Error::other("no instance_root"))?);
    let id = admitted["instance_id"].as_str().ok_or_else(|| io::Error::other("no instance_id"))?;
    let brain = &admitted["brain"];
    let rec = crate::record::read_json_strict(&crate::agentd_env::state_dir(&root).join("memory-binding-record.json"))?.ok_or_else(|| io::Error::other("no binding record (recovery)"))?;
    let want = &admitted["binding"]["record"];
    if rec["instance_id"] != json!(id) || rec["brain_id"] != brain["brain_id"] || rec["sentinel_ref"].is_null() || rec["sentinel_ref"] != want["sentinel_ref"] {
        return Err(io::Error::other("the binding record is not the one this activation recorded (recovery)"));
    }
    let p = Product { exe: PathBuf::new(), agentd: Pinned { path: PathBuf::new(), sha256: String::new() }, birth: Pinned { path: PathBuf::new(), sha256: String::new() }, brain: bundle.clone(), registry: &NoRegistry, service: Value::Null, path: path.into(), home: home.into() };
    Ok(AgentLaunch { instance_root: root, bind, backend: backend.into(), delegate, memory: Some(p.memory(brain, egress.map(str::to_string))?), instance_id: Some(id.into()), path: path.into(), home: home.into() })
}

pub fn admitted_launch_local(admitted: &Value, bundle: &BrainBundle, bind: SocketAddr, path: &str, home: &Path, llm_url: &str, model: &str) -> io::Result<Vec<(String, String)>> {
    let loopback = ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"].iter().any(|p| llm_url.starts_with(p));
    if !loopback {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("the local model must be on loopback http, not {llm_url:?}")));
    }
    if model.is_empty() || model.len() > 128 || !model.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the local model id must be 1-128 printable characters without spaces"));
    }
    let l = admitted_agent(admitted, bundle, bind, path, home, "local", false, None)?;
    let mut env = agentd_env(&l, None)?;
    env.retain(|(k, _)| k != "RTH_OPENAI_TOOLS");
    env.push(("RTH_LOCAL_LLM_URL".into(), llm_url.into()));
    env.push(("RTH_LOCAL_MODEL".into(), model.into()));
    Ok(env)
}

pub const NO_KEY: &str = "no key stored: ask for the OpenAI key before starting the voice agent";

struct NoRegistry;
impl BrainRegistry for NoRegistry {
    fn create(&self, _: &Value) -> Cmd {
        failed("brain", "no registry")
    }
    fn lookup(&self, _: &Value) -> Reconciled {
        Reconciled::Unknown("no registry".into())
    }
}

pub fn birth_main(tool: &Path, nonce_file: &Path) -> io::Error {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::process::CommandExt;
    let read = || -> io::Result<String> {
        let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(nonce_file)?;
        let m = f.metadata()?;
        if !m.is_file() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
            return Err(io::Error::other(format!("{} is not a private regular file", nonce_file.display())));
        }
        let mut t = String::new();
        io::Read::read_to_string(&mut f, &mut t)?;
        let t = t.trim().to_string();
        if t.len() < 32 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::other("the nonce file does not hold a nonce"));
        }
        Ok(t)
    };
    match read() {
        Ok(nonce) => std::process::Command::new(tool).env("RTH_MEMORY_NONCE", nonce).exec(),
        Err(e) => e,
    }
}
