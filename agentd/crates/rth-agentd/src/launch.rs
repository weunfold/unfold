

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};

use rth_protocol::AGENT_PORT;

pub const ENV_MODE: &str = "RTH_AGENT_MODE";
pub const ENV_ROOT: &str = "RTH_INSTANCE_ROOT";
pub const ENV_STATE: &str = rth_protocol::deploy::ENV_STATE;
pub const ENV_JOURNAL: &str = "RTH_JOURNAL";
pub const ENV_BIND: &str = "RTH_AGENT_BIND";

pub const BOX_JOURNAL: &str = "/rth/journal.jsonl";
pub const BOX_STATE: &str = crate::instance::DEFAULT_STATE;

pub const USAGE: &str = "rth-agentd: the RTH agent (launch contract: desktop|box)

Configured by the environment only; arguments never configure.
  RTH_AGENT_MODE     desktop (default) | box
  desktop: every one of these is required, explicit, and checked before anything starts:
    RTH_INSTANCE_ROOT  absolute, normalized, existing directory (the admitted instance root)
    RTH_STATE          absolute instance state file inside RTH_INSTANCE_ROOT
    RTH_JOURNAL        absolute journal file inside RTH_INSTANCE_ROOT
    RTH_AGENT_BIND     literal loopback address with a port (127.0.0.1:<port> or [::1]:<port>)
  box: the rth-core guest deployment; defaults /rth/instance.json, /rth/journal.jsonl, 0.0.0.0:7700
Arguments: --help, -h | --version, -V. Anything else is refused.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Desktop,
    Box,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Help,
    Version,

    Start(Contract),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub mode: Mode,
    pub state: PathBuf,
    pub journal: PathBuf,
    pub bind: SocketAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn normalized_absolute(name: &str, v: &str) -> Result<PathBuf, Refusal> {
    let p = Path::new(v);
    if !p.is_absolute() {
        return Err(Refusal(format!("{name} must be an absolute path (got {v:?})")));
    }

    if v.contains("//") || v.split('/').any(|seg| seg == "." || seg == "..") || p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(Refusal(format!("{name} must be a normalized path, without . or .. (got {v:?})")));
    }
    Ok(p.to_path_buf())
}

const DERIVED_FILES: &[&str] = &["tasks.jsonl", "memory-acts.jsonl", "memory-binding.jsonl", "memory-reference.jsonl", "memory-binding-record.json", "memory-forgotten.jsonl", "people-answers.jsonl"];

pub const SELF_FILES: &[&str] = &["self.json", "proposals.jsonl", "horizons.jsonl", "governance.jsonl", "expression.jsonl", "expression-unsent.jsonl", "omni-cursor.json", "sleep.jsonl"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    File,
    Dir,
}

fn destination(root: &Path, real_root: &Path, role: &str, path: &Path, leaf: Leaf) -> Result<(PathBuf, Option<(u64, u64)>), Refusal> {
    use std::os::unix::fs::MetadataExt;
    let rel = path.strip_prefix(root).map_err(|_| Refusal(format!("{role} {} is not inside {ENV_ROOT} {}", path.display(), root.display())))?;
    let mut cur = real_root.to_path_buf();
    let parts: Vec<_> = rel.components().collect();
    for (i, c) in parts.iter().enumerate() {
        cur.push(c);
        let last = i + 1 == parts.len();
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => return Err(Refusal(format!("{role} {}: {} is a symlink; nothing below {ENV_ROOT} may be one", path.display(), cur.display()))),
            Ok(m) if !last && !m.is_dir() => return Err(Refusal(format!("{role} {}: {} is not a directory", path.display(), cur.display()))),
            Ok(m) if last && leaf == Leaf::File && !m.is_file() => return Err(Refusal(format!("{role} {} is not a regular file", path.display()))),
            Ok(m) if last && leaf == Leaf::Dir && !m.is_dir() => return Err(Refusal(format!("{role} {} is not a directory", path.display()))),
            Ok(m) if last => return Ok((cur, Some((m.dev(), m.ino())))),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && last => return Ok((cur, None)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Refusal(format!("{role} {}: the directory {} does not exist (create the instance's directories first)", path.display(), cur.display()))),
            Err(e) => return Err(Refusal(format!("{role} {}: cannot inspect {}: {e}", path.display(), cur.display()))),
        }
    }
    Err(Refusal(format!("{role} {} names no file", path.display())))
}

pub fn resolve(args: &[String], var: &dyn Fn(&str) -> Option<String>) -> Result<Action, Refusal> {
    match args.get(1..).unwrap_or(&[]) {
        [] => {}
        [a] if a == "--help" || a == "-h" => return Ok(Action::Help),
        [a] if a == "--version" || a == "-V" => return Ok(Action::Version),
        rest => return Err(Refusal(format!("unknown arguments {rest:?}: rth-agentd is configured by its environment only (see --help)"))),
    }
    let get = |k: &str| var(k).filter(|v| !v.is_empty());
    let mode = match get(ENV_MODE).as_deref() {
        None | Some("desktop") => Mode::Desktop,
        Some("box") => Mode::Box,
        Some(other) => return Err(Refusal(format!("{ENV_MODE} must be desktop or box (got {other:?})"))),
    };

    if let Some(other) = get("RTH_MEMORY_DECIDE").filter(|v| v != "realtime" && v != "asked") {
        return Err(Refusal(format!("RTH_MEMORY_DECIDE must be realtime or asked (got {other:?})")));
    }
    if mode == Mode::Box {
        let bind = get(ENV_BIND).unwrap_or_else(|| format!("0.0.0.0:{AGENT_PORT}"));
        let bind = bind.parse().map_err(|e| Refusal(format!("{ENV_BIND} {bind:?}: {e}")))?;
        return Ok(Action::Start(Contract {
            mode,
            state: PathBuf::from(get(ENV_STATE).unwrap_or_else(|| BOX_STATE.into())),
            journal: PathBuf::from(get(ENV_JOURNAL).unwrap_or_else(|| BOX_JOURNAL.into())),
            bind,
        }));
    }
    let need = |k: &str| get(k).ok_or_else(|| Refusal(format!("desktop launch: {k} is required (see --help); nothing was started")));
    let root = normalized_absolute(ENV_ROOT, &need(ENV_ROOT)?)?;
    match std::fs::metadata(&root) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(Refusal(format!("{ENV_ROOT} {} is not a directory", root.display()))),
        Err(e) => return Err(Refusal(format!("{ENV_ROOT} {}: {e}", root.display()))),
    }

    let real_root = std::fs::canonicalize(&root).map_err(|e| Refusal(format!("{ENV_ROOT} {}: {e}", root.display())))?;
    let lexical = |k: &str, v: &str| -> Result<PathBuf, Refusal> {
        let p = normalized_absolute(k, v)?;
        if p == root || !p.starts_with(&root) {
            return Err(Refusal(format!("{k} {} is not inside {ENV_ROOT} {}", p.display(), root.display())));
        }
        Ok(p)
    };
    let state = lexical(ENV_STATE, &need(ENV_STATE)?)?;
    let journal = lexical(ENV_JOURNAL, &need(ENV_JOURNAL)?)?;

    let raw = need(ENV_BIND)?;
    let bind: SocketAddr = raw.parse().map_err(|_| Refusal(format!("{ENV_BIND} must be a literal address with a port, e.g. 127.0.0.1:7801 (got {raw:?})")))?;
    if !bind.ip().is_loopback() {
        return Err(Refusal(format!("{ENV_BIND} {bind} is not a loopback address: a desktop agent listens on 127.0.0.1 or ::1 only")));
    }
    if bind.port() == 0 {
        return Err(Refusal(format!("{ENV_BIND} {bind} needs an explicit port")));
    }

    let jdir = journal.parent().unwrap_or(&root).to_path_buf();
    let mut roles: Vec<(String, PathBuf, Leaf)> = vec![(ENV_STATE.into(), state.clone(), Leaf::File), (ENV_JOURNAL.into(), journal.clone(), Leaf::File)];
    for (var, name, leaf) in [("RTH_COMMANDS", "commands.jsonl", Leaf::File), ("RTH_CONTEXT", "context.json", Leaf::File), ("RTH_MEDIA", "media", Leaf::Dir)] {
        let p = match get(var) {
            Some(v) => lexical(var, &v)?,
            None => jdir.join(name),
        };
        roles.push((name.into(), p, leaf));
    }
    for name in DERIVED_FILES.iter().chain(SELF_FILES) {
        roles.push(((*name).into(), jdir.join(name), Leaf::File));
    }
    let mut seen: Vec<(String, PathBuf, Option<(u64, u64)>)> = Vec::new();
    for (role, path, leaf) in roles {
        let (resolved, id) = destination(&root, &real_root, &role, &path, leaf)?;
        for (other, p, oid) in &seen {
            if *p == resolved || (id.is_some() && id == *oid) {
                return Err(Refusal(format!("{role} {} and {other} {} are the same file: each role needs its own file", path.display(), p.display())));
            }
        }
        seen.push((role, resolved, id));
    }
    Ok(Action::Start(Contract { mode, state, journal, bind }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn root() -> PathBuf {
        let d = std::env::temp_dir().join(format!("agentd-launch-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(d.join("agentd-state")).unwrap();
        d
    }
    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }
    fn run(args: &[&str], env: &[(&str, String)]) -> Result<Action, Refusal> {
        let m: HashMap<String, String> = env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let args: Vec<String> = std::iter::once("rth-agentd").chain(args.iter().copied()).map(String::from).collect();
        resolve(&args, &|k| m.get(k).cloned())
    }
    fn desktop(r: &Path) -> Vec<(&'static str, String)> {
        vec![
            (ENV_ROOT, r.display().to_string()),
            (ENV_STATE, r.join("agentd-state/state.json").display().to_string()),
            (ENV_JOURNAL, r.join("agentd-state/journal.jsonl").display().to_string()),
            (ENV_BIND, "127.0.0.1:7801".into()),
        ]
    }
    fn refused(r: Result<Action, Refusal>, needle: &str) {
        match r {
            Err(Refusal(why)) => assert!(why.contains(needle), "refusal {why:?} should mention {needle:?}"),
            Ok(a) => panic!("expected a refusal mentioning {needle:?}, got {a:?}"),
        }
    }

    #[test]
    fn help_and_version_return_without_a_start() {
        assert_eq!(run(&["--help"], &[]), Ok(Action::Help));
        assert_eq!(run(&["-h"], &[]), Ok(Action::Help));
        assert_eq!(run(&["--version"], &[]), Ok(Action::Version));
        assert_eq!(run(&["-V"], &[]), Ok(Action::Version));
    }

    #[test]
    fn unknown_or_extra_arguments_are_refused_even_with_a_valid_environment() {
        let r = root();
        for args in [&["--bind", "0.0.0.0:7700"][..], &["start"], &["--help", "--version"], &["--instance-root", "/tmp"]] {
            refused(run(args, &desktop(&r)), "unknown arguments");
        }
    }

    #[test]
    fn an_empty_environment_is_refused_in_desktop_mode_never_defaulting_to_rth_or_7700() {
        refused(run(&[], &[]), "RTH_INSTANCE_ROOT is required");
        refused(run(&[], &[(ENV_MODE, "desktop".into())]), "RTH_INSTANCE_ROOT is required");

        let r = root();
        let mut env = desktop(&r);
        env.retain(|(k, _)| *k != ENV_BIND);
        env.push((ENV_BIND, String::new()));
        refused(run(&[], &env), "RTH_AGENT_BIND is required");
        for missing in [ENV_STATE, ENV_JOURNAL, ENV_BIND] {
            let env: Vec<_> = desktop(&r).into_iter().filter(|(k, _)| *k != missing).collect();
            refused(run(&[], &env), &format!("{missing} is required"));
        }
    }

    #[test]
    fn malformed_paths_are_refused_relative_traversal_outside_or_a_missing_root() {
        let r = root();
        let set = |k: &'static str, v: String| {
            let mut e: Vec<_> = desktop(&r).into_iter().filter(|(x, _)| *x != k).collect();
            e.push((k, v));
            e
        };
        refused(run(&[], &set(ENV_STATE, "state.json".into())), "absolute");
        refused(run(&[], &set(ENV_JOURNAL, format!("{}/../journal.jsonl", r.display()))), "normalized");
        refused(run(&[], &set(ENV_JOURNAL, format!("{}/./journal.jsonl", r.display()))), "normalized");
        refused(run(&[], &set(ENV_STATE, "/rth/instance.json".into())), "not inside");
        refused(run(&[], &set(ENV_JOURNAL, r.display().to_string())), "not inside");
        refused(run(&[], &set(ENV_ROOT, format!("{}/absent", r.display()))), "RTH_INSTANCE_ROOT");
        let file = r.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        refused(run(&[], &set(ENV_ROOT, file.display().to_string())), "not a directory");
        refused(run(&[], &[(ENV_MODE, "server".into())]), "desktop or box");
    }

    #[test]
    fn binds_must_be_literal_loopback_with_a_port() {
        let r = root();
        let bind = |v: &str| {
            let mut e: Vec<_> = desktop(&r).into_iter().filter(|(x, _)| *x != ENV_BIND).collect();
            e.push((ENV_BIND, v.into()));
            run(&[], &e)
        };
        for wildcard in ["0.0.0.0:7801", "[::]:7801", "0.0.0.0:7700"] {
            refused(bind(wildcard), "not a loopback");
        }
        refused(bind("192.168.1.5:7801"), "not a loopback");
        refused(bind("localhost:7801"), "literal address");
        refused(bind("127.0.0.1"), "literal address");
        refused(bind("127.0.0.1:0"), "explicit port");
        assert!(matches!(bind("[::1]:7801"), Ok(Action::Start(_))));
    }

    #[test]
    fn the_admitted_desktop_launch_yields_exactly_its_explicit_values() {
        let r = root();
        let Ok(Action::Start(c)) = run(&[], &desktop(&r)) else { panic!("the admitted launch must start") };
        assert_eq!(c.mode, Mode::Desktop);
        assert_eq!(c.state, r.join("agentd-state/state.json"));
        assert_eq!(c.journal, r.join("agentd-state/journal.jsonl"));
        assert_eq!(c.bind, "127.0.0.1:7801".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn box_mode_keeps_the_reviewed_guest_defaults_and_their_overrides() {
        let Ok(Action::Start(c)) = run(&[], &[(ENV_MODE, "box".into())]) else { panic!("box mode starts") };
        assert_eq!((c.state.to_str().unwrap(), c.journal.to_str().unwrap(), c.bind.to_string().as_str()), ("/rth/instance.json", "/rth/journal.jsonl", "0.0.0.0:7700"));
        let Ok(Action::Start(c)) = run(&[], &[(ENV_MODE, "box".into()), (ENV_BIND, "0.0.0.0:7801".into()), (ENV_JOURNAL, "/rth/j.jsonl".into())]) else { panic!() };
        assert_eq!((c.journal.to_str().unwrap(), c.bind.port()), ("/rth/j.jsonl", 7801));
        refused(run(&["--x"], &[(ENV_MODE, "box".into())]), "unknown arguments");
    }

    fn with(r: &Path, k: &'static str, v: PathBuf) -> Vec<(&'static str, String)> {
        let mut e: Vec<_> = desktop(r).into_iter().filter(|(x, _)| *x != k).collect();
        e.push((k, v.display().to_string()));
        e
    }
    fn outside() -> PathBuf {
        let d = std::env::temp_dir().join(format!("agentd-launch-outside-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ad_r1_a_parent_symlink_out_of_the_root_is_refused() {
        let r = root();
        let out = outside();
        std::os::unix::fs::symlink(&out, r.join("alias")).unwrap();
        let mut env = with(&r, ENV_STATE, r.join("alias/state.json"));
        env.retain(|(k, _)| *k != ENV_JOURNAL);
        env.push((ENV_JOURNAL, r.join("alias/journal.jsonl").display().to_string()));
        refused(run(&[], &env), "symlink");
        assert!(std::fs::read_dir(&out).unwrap().next().is_none(), "nothing written outside");
    }

    #[test]
    fn ad_r1_a_symlinked_journal_or_derived_file_is_refused() {
        let r = root();
        let out = outside();
        let foreign = out.join("canary.txt");
        std::fs::write(&foreign, b"foreign").unwrap();
        std::os::unix::fs::symlink(&foreign, r.join("agentd-state/journal.jsonl")).unwrap();
        refused(run(&[], &desktop(&r)), "symlink");
        std::fs::remove_file(r.join("agentd-state/journal.jsonl")).unwrap();
        for derived in ["commands.jsonl", "context.json", "tasks.jsonl", "memory-acts.jsonl", "memory-binding.jsonl", "memory-binding-record.json", "memory-reference.jsonl", "memory-forgotten.jsonl", "people-answers.jsonl", "media"] {
            let p = r.join("agentd-state").join(derived);
            std::os::unix::fs::symlink(&foreign, &p).unwrap();
            refused(run(&[], &desktop(&r)), derived);
            std::fs::remove_file(&p).unwrap();
        }
        assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign");
    }

    #[test]
    fn a2_the_self_files_are_admitted_beside_the_journal() {
        let r = root();
        let selfs = ["self.json", "proposals.jsonl", "horizons.jsonl", "governance.jsonl", "expression.jsonl", "expression-unsent.jsonl", "omni-cursor.json", "sleep.jsonl"];
        assert!(matches!(run(&[], &desktop(&r)), Ok(Action::Start(_))), "absent Self files start");
        let out = outside();
        let foreign = out.join("canary.txt");
        std::fs::write(&foreign, b"foreign").unwrap();
        for name in selfs {
            let p = r.join("agentd-state").join(name);
            std::os::unix::fs::symlink(&foreign, &p).unwrap();
            refused(run(&[], &desktop(&r)), name);
            std::fs::remove_file(&p).unwrap();
            std::fs::write(r.join("agentd-state/journal.jsonl"), b"").unwrap();
            std::fs::hard_link(r.join("agentd-state/journal.jsonl"), &p).unwrap();
            refused(run(&[], &desktop(&r)), "same file");
            std::fs::remove_file(&p).unwrap();
            std::fs::remove_file(r.join("agentd-state/journal.jsonl")).unwrap();
        }
        assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign");
    }

    #[test]
    fn ad_r1_missing_parents_wrong_leaf_types_and_outside_overrides_are_refused() {
        let r = root();
        refused(run(&[], &with(&r, ENV_STATE, r.join("absent-dir/state.json"))), "absent-dir");
        std::fs::create_dir_all(r.join("agentd-state/journal.jsonl")).unwrap();
        refused(run(&[], &desktop(&r)), "not a regular file");
        std::fs::remove_dir(r.join("agentd-state/journal.jsonl")).unwrap();
        std::fs::write(r.join("agentd-state/media"), b"x").unwrap();
        refused(run(&[], &desktop(&r)), "media");
        std::fs::remove_file(r.join("agentd-state/media")).unwrap();
        let out = outside();
        for k in ["RTH_COMMANDS", "RTH_CONTEXT", "RTH_MEDIA"] {
            let mut env = desktop(&r);
            env.push((k, out.join("x").display().to_string()));
            refused(run(&[], &env), "not inside");
        }
    }

    #[test]
    fn ad_r1_a_root_given_through_a_symlink_is_admitted_by_its_resolved_directory() {
        let real = root();
        let link = std::env::temp_dir().join(format!("agentd-launch-rootlink-{}-{}", std::process::id(), rand_suffix()));
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let Ok(Action::Start(c)) = run(&[], &desktop(&link)) else { panic!("a symlinked root resolves to its directory") };
        assert_eq!(c.journal, link.join("agentd-state/journal.jsonl"));
    }

    #[test]
    fn ad_r2_state_and_journal_may_not_be_the_same_file_or_a_derived_file() {
        let r = root();
        let same = r.join("agentd-state/same.json");
        let mut env = with(&r, ENV_STATE, same.clone());
        env.retain(|(k, _)| *k != ENV_JOURNAL);
        env.push((ENV_JOURNAL, same.display().to_string()));
        refused(run(&[], &env), "same file");

        std::fs::write(r.join("agentd-state/state.json"), b"{}").unwrap();
        std::fs::hard_link(r.join("agentd-state/state.json"), r.join("agentd-state/journal.jsonl")).unwrap();
        refused(run(&[], &desktop(&r)), "same file");
        assert_eq!(std::fs::read(r.join("agentd-state/state.json")).unwrap(), b"{}");
        std::fs::remove_file(r.join("agentd-state/journal.jsonl")).unwrap();
        for derived in ["commands.jsonl", "context.json", "tasks.jsonl", "memory-acts.jsonl", "memory-forgotten.jsonl"] {
            refused(run(&[], &with(&r, ENV_STATE, r.join("agentd-state").join(derived))), derived);
        }
    }
}

