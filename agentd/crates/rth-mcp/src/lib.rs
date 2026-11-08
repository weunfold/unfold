

use std::collections::BTreeMap;
use std::time::Duration;

use rmcp::model::{CallToolRequestParams, CallToolResult, ProtocolVersion, Tool};
use rmcp::service::{ClientInitializeError, ClientLifecycleMode, ClientServiceExt, RoleClient, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use rth_tools::authority::{PartManifest, PartTool};
use rth_tools::parts::{Part, PartOutput};
use rth_tools::profile::Effect;
use rth_tools::web::BoxFuture;

pub const SCHEMA: &str = "rth-mcp/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub schema: String,
    pub servers: Vec<ServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default = "on")]
    pub enabled: bool,
    pub transport: Transport,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    #[serde(default = "startup_default")]
    pub startup_timeout_sec: u64,
    #[serde(default = "tool_default")]
    pub tool_timeout_sec: u64,
    #[serde(default)]
    pub default_approval: Approval,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, ToolOverride>,
}

fn on() -> bool {
    true
}

fn startup_default() -> u64 {
    10
}

fn tool_default() -> u64 {
    60
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    StreamableHttp {
        url: String,

        #[serde(default, skip_serializing_if = "Option::is_none")]
        bearer_token_env_var: Option<String>,
    },
    Stdio {

        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,

        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    #[default]
    Ask,

    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<Approval>,
}

fn word(s: &str, max: usize, ok: impl Fn(u8) -> bool) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(ok)
}

fn is_env_name(s: &str) -> bool {
    word(s, 64, |b| b.is_ascii_alphanumeric() || b == b'_') && !s.as_bytes()[0].is_ascii_digit()
}

fn loopback(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(d) => *d == "localhost",
        url::Host::Ipv4(a) => a.is_loopback(),
        url::Host::Ipv6(a) => a.is_loopback(),
    }
}

impl ServerConfig {
    pub fn part_id(&self) -> String {
        format!("mcp.{}", self.id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !word(&self.id, 32, |b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(format!("server id {:?}: 1-32 of a-z, 0-9 and -", self.id));
        }
        if !(1..=600).contains(&self.startup_timeout_sec) || !(1..=3600).contains(&self.tool_timeout_sec) {
            return Err(format!("{}: startup_timeout_sec is 1-600 and tool_timeout_sec 1-3600", self.id));
        }
        match &self.transport {
            Transport::StreamableHttp { url, bearer_token_env_var } => {
                let u = url::Url::parse(url).map_err(|e| format!("{}: url: {e}", self.id))?;
                let host = u.host().ok_or_else(|| format!("{}: url has no host", self.id))?;
                if !(u.scheme() == "https" || (u.scheme() == "http" && loopback(&host))) {
                    return Err(format!("{}: url must be https (http only on this computer)", self.id));
                }
                if !u.username().is_empty() || u.password().is_some() {
                    return Err(format!("{}: no credentials in the url; use bearer_token_env_var", self.id));
                }
                if bearer_token_env_var.as_deref().is_some_and(|v| !is_env_name(v)) {
                    return Err(format!("{}: bearer_token_env_var is not a variable name", self.id));
                }
            }
            Transport::Stdio { command, env, cwd, .. } => {
                if !std::path::Path::new(command).is_absolute() {
                    return Err(format!("{}: command must be an absolute path", self.id));
                }
                if let Some(k) = env.keys().find(|k| !is_env_name(k)) {
                    return Err(format!("{}: env {k:?} is not a variable name", self.id));
                }
                if cwd.as_deref().is_some_and(|c| !std::path::Path::new(c).is_absolute()) {
                    return Err(format!("{}: cwd must be an absolute path", self.id));
                }
            }
        }
        for (name, o) in &self.tools {
            if o.approval == Some(Approval::Allow) && o.effect != Some(Effect::None) {
                return Err(format!("{}: tool {name} can skip its card only when marked effect none", self.id));
            }
        }
        Ok(())
    }

    fn exposed(&self, tool: &str) -> bool {
        self.enabled_tools.as_ref().is_none_or(|only| only.iter().any(|t| t == tool)) && !self.disabled_tools.iter().any(|t| t == tool)
    }

    pub fn effect(&self, tool: &str) -> Effect {
        self.tools.get(tool).and_then(|o| o.effect).unwrap_or(Effect::Irreversible)
    }

    pub fn asks(&self, tool: &str) -> bool {
        let approval = self.tools.get(tool).and_then(|o| o.approval).unwrap_or(self.default_approval);
        !(approval == Approval::Allow && self.effect(tool) == Effect::None)
    }
}

pub fn parse(text: &str) -> Result<File, String> {
    let f: File = serde_json::from_str(text).map_err(|e| format!("mcp.json: {e}"))?;
    if f.schema != SCHEMA {
        return Err(format!("mcp.json: schema must be {SCHEMA}"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for s in &f.servers {
        s.validate()?;
        if !ids.insert(&s.id) {
            return Err(format!("mcp.json: server id {} twice", s.id));
        }
    }
    Ok(f)
}

pub struct McpPart {
    config: ServerConfig,
    service: Option<RunningService<RoleClient, ()>>,

    _group: Option<Group>,
    manifest: PartManifest,
    digest: String,
    schemas: BTreeMap<String, (String, Value)>,
}

fn tool_identity(t: &Tool) -> Value {
    json!({"name": t.name, "title": t.title, "description": t.description, "input_schema": t.input_schema, "output_schema": t.output_schema, "annotations": t.annotations})
}

fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto { preferred_versions: vec![ProtocolVersion::V_2026_07_28], legacy_version: Some(ProtocolVersion::V_2025_11_25) }
}

struct Group(i32, Option<Started>);

impl Drop for Group {
    fn drop(&mut self) {
        if may_kill(self.0, self.1, holder(self.0)) {

            unsafe {
                libc::killpg(self.0, libc::SIGKILL);
            }
        }
    }
}

type Started = (u64, u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Holder {
    started: Started,
    pgid: i32,
}

fn started(pid: i32) -> Option<Started> {
    holder(pid).map(|h| h.started)
}

#[cfg(target_os = "macos")]
fn holder(pid: i32) -> Option<Holder> {

    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
    (n == size).then_some(Holder { started: (info.pbi_start_tvsec, info.pbi_start_tvusec), pgid: info.pbi_pgid as i32 })
}

#[cfg(target_os = "linux")]
fn holder(pid: i32) -> Option<Holder> {

    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let f: Vec<&str> = s.get(s.rfind(')')? + 2..)?.split_whitespace().collect();
    Some(Holder { started: (f.get(19)?.parse().ok()?, 0), pgid: f.get(2)?.parse().ok()? })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn holder(_pid: i32) -> Option<Holder> {
    None
}

fn may_kill(pgid: i32, recorded: Option<Started>, now: Option<Holder>) -> bool {
    match now {
        None => true,
        Some(h) => recorded == Some(h.started) && h.pgid == pgid,
    }
}

enum Failed {
    Start(Box<ClientInitializeError>),
    Other(String),
}

async fn serve(config: &ServerConfig, env: &(dyn Fn(&str) -> Option<String> + Sync), mode: ClientLifecycleMode) -> Result<(RunningService<RoleClient, ()>, Option<Group>), Failed> {
    let id = &config.id;
    match &config.transport {
        Transport::StreamableHttp { url, bearer_token_env_var } => {
            let mut c = StreamableHttpClientTransportConfig::with_uri(url.clone());
            if let Some(var) = bearer_token_env_var {
                c = c.auth_header(env(var).filter(|t| !t.is_empty()).ok_or_else(|| Failed::Other(format!("{id}: {var} is not set")))?);
            }
            let service = ().serve_with_lifecycle(StreamableHttpClientTransport::from_config(c), mode).await.map_err(|e| Failed::Start(Box::new(e)))?;
            Ok((service, None))
        }
        Transport::Stdio { command, args, env: vars, cwd } => {
            let mut cmd = tokio::process::Command::new(command);
            cmd.args(args).env_clear();
            for k in ["PATH", "HOME", "LANG"] {
                if let Some(v) = env(k) {
                    cmd.env(k, v);
                }
            }
            cmd.envs(vars);
            if let Some(d) = cwd {
                cmd.current_dir(d);
            }

            let mut wrapped = process_wrap::tokio::CommandWrap::from(cmd);
            wrapped.wrap(process_wrap::tokio::ProcessGroup::leader());
            let (child, _) = TokioChildProcess::builder(wrapped).stderr(std::process::Stdio::null()).spawn().map_err(|e| Failed::Other(format!("{id}: {e}")))?;

            let group = child.id().map(|pid| Group(pid as i32, started(pid as i32)));
            let service = ().serve_with_lifecycle(child, mode).await.map_err(|e| Failed::Start(Box::new(e)))?;
            Ok((service, group))
        }
    }
}

pub async fn connect(config: &ServerConfig, env: &(dyn Fn(&str) -> Option<String> + Sync)) -> Result<McpPart, String> {
    config.validate()?;
    let id = &config.id;
    let started = async {
        let service = match serve(config, env, lifecycle()).await {
            Err(Failed::Start(e)) if matches!(&*e, ClientInitializeError::NoCompatibleProtocolVersion { server_supported, .. } if server_supported.contains(&ProtocolVersion::V_2025_11_25)) => {
                serve(config, env, ClientLifecycleMode::Initialize).await
            }
            other => other,
        };
        let service = service.map_err(|e| match e {
            Failed::Start(e) => format!("{id}: {e}"),
            Failed::Other(e) => e,
        })?;
        let (service, group) = service;
        let tools = service.list_all_tools().await.map_err(|e| format!("{id}: tools/list: {e}"))?;
        Ok::<_, String>((service, group, tools))
    };
    let (service, group, mut listed) = tokio::time::timeout(Duration::from_secs(config.startup_timeout_sec), started).await.map_err(|_| format!("{id}: no answer within {} s", config.startup_timeout_sec))??;
    listed.retain(|t| config.exposed(&t.name));
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    listed.dedup_by(|a, b| a.name == b.name);
    let server = service.peer_info().and_then(|i| i.server_info.as_ref().map(|s| json!({"name": s.name, "version": s.version})));
    let digest = rth_tools::digest::json(&json!({"schema": SCHEMA, "config": config, "server": server, "tools": listed.iter().map(tool_identity).collect::<Vec<_>>()}));
    let manifest = PartManifest {
        schema: rth_tools::authority::SCHEMA.into(),
        id: config.part_id(),
        version: server.as_ref().and_then(|s| s["version"].as_str()).unwrap_or("unknown").to_string(),
        title: config.title.clone().unwrap_or_else(|| config.id.clone()),
        tools: listed.iter().map(|t| PartTool { name: t.name.to_string(), effect: config.effect(&t.name), description: t.description.as_ref().map(|d| d.to_string()) }).collect(),
    };
    let schemas = listed.iter().map(|t| (t.name.to_string(), (t.description.as_deref().unwrap_or_default().to_string(), Value::Object((*t.input_schema).clone())))).collect();
    Ok(McpPart { config: config.clone(), service: Some(service), _group: group, manifest, digest, schemas })
}

pub fn output(r: &CallToolResult) -> PartOutput {
    let mut parts = Vec::new();
    for c in &r.content {
        if let Some(t) = c.as_text() {
            parts.push(t.text.clone());
            continue;
        }
        let v = serde_json::to_value(c).unwrap_or(Value::Null);
        let kind = v["type"].as_str().unwrap_or("content");
        let what = v["uri"].as_str().or(v["resource"]["uri"].as_str()).or(v["mimeType"].as_str()).unwrap_or("");
        parts.push(if what.is_empty() { format!("[{kind}]") } else { format!("[{kind}: {what}]") });
    }
    if parts.is_empty()
        && let Some(s) = &r.structured_content
    {
        parts.push(s.to_string());
    }
    PartOutput { text: parts.join("\n"), is_error: r.is_error == Some(true) }
}

impl McpPart {
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub async fn close(mut self) {
        if let Some(s) = self.service.take() {
            let _ = s.cancel().await;
        }
    }
}

impl Part for McpPart {
    fn manifest(&self) -> &PartManifest {
        &self.manifest
    }

    fn digest(&self) -> &str {
        &self.digest
    }

    fn schema(&self, tool: &str) -> Option<(String, Value)> {
        self.schemas.get(tool).cloned()
    }

    fn asks(&self, tool: &str) -> bool {
        self.config.asks(tool)
    }

    fn call<'a>(&'a self, tool: &'a str, args: serde_json::Map<String, Value>) -> BoxFuture<'a, Result<PartOutput, String>> {
        Box::pin(async move {
            let p = CallToolRequestParams::new(tool.to_string()).with_arguments(args);
            let limit = self.config.tool_timeout_sec;
            match tokio::time::timeout(Duration::from_secs(limit), self.service.as_ref().expect("an open part has its service").call_tool(p)).await {
                Err(_) => Err(format!("no answer within {limit} s")),
                Ok(Err(e)) => Err(e.to_string()),
                Ok(Ok(r)) => Ok(output(&r)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(extra: Value) -> Value {
        let mut s = json!({"id": "docs", "transport": {"streamable_http": {"url": "https://mcp.example.com/mcp"}}});
        for (k, v) in extra.as_object().unwrap() {
            s[k] = v.clone();
        }
        s
    }

    fn one(s: Value) -> Result<File, String> {
        parse(&json!({"schema": SCHEMA, "servers": [s]}).to_string())
    }

    #[test]
    fn a_config_parses_with_defaults_and_bad_ones_are_refused() {
        let f = one(server(json!({}))).unwrap();
        let s = &f.servers[0];
        assert_eq!((s.enabled, s.startup_timeout_sec, s.tool_timeout_sec, s.default_approval), (true, 10, 60, Approval::Ask));
        assert_eq!(s.part_id(), "mcp.docs");
        assert_eq!(s.effect("anything"), Effect::Irreversible, "unmarked tools are irreversible");
        assert!(s.asks("anything"));
        let bad = [
            (server(json!({"id": "Docs"})), "server id"),
            (server(json!({"transport": {"streamable_http": {"url": "http://mcp.example.com/mcp"}}})), "https"),
            (server(json!({"transport": {"streamable_http": {"url": "https://u:p@mcp.example.com/mcp"}}})), "credentials"),
            (server(json!({"transport": {"streamable_http": {"url": "https://mcp.example.com", "bearer_token_env_var": "A-B"}}})), "variable name"),
            (server(json!({"transport": {"stdio": {"command": "npx"}}})), "absolute"),
            (server(json!({"transport": {"stdio": {"command": "/bin/x", "env": {"1A": "x"}}}})), "variable name"),
            (server(json!({"tool_timeout_sec": 0})), "timeout"),
            (server(json!({"tools": {"send": {"approval": "allow"}}})), "effect none"),
            (server(json!({"tools": {"send": {"approval": "allow", "effect": "reversible"}}})), "effect none"),
            (server(json!({"surprise": 1})), "unknown field"),
        ];
        for (s, want) in bad {
            let e = one(s.clone()).unwrap_err();
            assert!(e.contains(want), "{s}: {e}");
        }
        assert!(one(server(json!({"transport": {"streamable_http": {"url": "http://127.0.0.1:9/mcp"}}}))).is_ok(), "http on this computer");
        let twice = parse(&json!({"schema": SCHEMA, "servers": [server(json!({})), server(json!({}))]}).to_string()).unwrap_err();
        assert!(twice.contains("twice"));
        assert!(parse(&json!({"schema": "x", "servers": []}).to_string()).is_err());
    }

    #[test]
    fn only_a_tool_marked_effect_none_and_allow_skips_its_card() {
        let f = one(server(json!({"default_approval": "allow", "tools": {"search": {"effect": "none"}, "read": {"effect": "none", "approval": "ask"}, "edit": {"effect": "reversible"}}}))).unwrap();
        let s = &f.servers[0];
        assert!(!s.asks("search"), "marked none, default allow");
        assert!(s.asks("read"), "its own ask wins");
        assert!(s.asks("edit") && s.asks("unmarked"), "anything that changes something asks");
        let filtered = one(server(json!({"enabled_tools": ["a", "b"], "disabled_tools": ["b"]}))).unwrap();
        assert!(filtered.servers[0].exposed("a") && !filtered.servers[0].exposed("b") && !filtered.servers[0].exposed("c"));
    }

    #[test]
    fn a_group_is_killed_only_while_it_can_still_be_ours() {
        let (g, t) = (4242, (1_700_000_000, 5));
        let leader = Some(Holder { started: t, pgid: g });
        assert!(may_kill(g, Some(t), leader), "the leader we started is still there, in its group");
        assert!(may_kill(g, Some(t), None), "no process holds the pid: what is left of the group is ours");
        assert!(!may_kill(g, Some(t), Some(Holder { started: (1_700_000_900, 1), pgid: g })), "a later process reused the pid");
        assert!(!may_kill(g, Some(t), Some(Holder { started: t, pgid: 7 })), "a live process with that pid outside the group");
        assert!(!may_kill(g, None, Some(Holder { started: (1_700_000_900, 1), pgid: g })), "never recorded: nothing proves it is ours");
        let me = std::process::id() as i32;
        let h = holder(me).unwrap();
        assert_eq!((Some(h.started), h.pgid), (started(me), unsafe { libc::getpgrp() }), "this process reads back its own start time and group");
    }

    #[test]
    fn non_text_results_are_named_and_errors_flagged() {
        let r: CallToolResult = serde_json::from_value(json!({"content": [{"type": "text", "text": "hi"}, {"type": "image", "data": "AAAA", "mimeType": "image/png"}], "isError": true})).unwrap();
        let o = output(&r);
        assert_eq!(o.text, "hi\n[image: image/png]");
        assert!(o.is_error);
        let s: CallToolResult = serde_json::from_value(json!({"content": [], "structuredContent": {"n": 1}})).unwrap();
        assert_eq!(output(&s).text, "{\"n\":1}");
    }
}
