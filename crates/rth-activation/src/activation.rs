

use crate::capture::{self, Root};
use crate::effect::{self, Classification, Identity, Quiescence};
use crate::lease::{Lease, LeaseError, Mode};
use crate::record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
}

impl Paths {
    pub fn op_lock(&self) -> PathBuf {
        self.data.join("rth.op.lock")
    }
    pub fn fence(&self) -> PathBuf {
        self.data.join("rth.effect.lock")
    }
    pub fn op_record(&self) -> PathBuf {
        self.data.join("rth.op.json")
    }
    pub fn activation(&self) -> PathBuf {
        self.data.join("activation.json")
    }
    pub fn backup(&self, op: &str) -> PathBuf {
        self.data.join("backups").join(op).join("test")
    }
    pub fn instance(&self, dir: &str) -> PathBuf {
        self.data.join("instances").join(dir)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub op_id: String,

    pub test_set: Vec<Root>,

    pub instance_dir: String,

    #[serde(default)]
    pub legacy: Option<Legacy>,

    #[serde(default)]
    pub live: crate::live::LiveChecks,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Legacy {
    pub launcher: PathBuf,
    pub prior_sha256: String,

    pub fence: PathBuf,
    pub fence_sha256: String,
}

pub fn load_plan(bytes: &[u8], expected_sha256: &str) -> Result<Plan, ActivationError> {
    load_plan_in(bytes, expected_sha256, None)
}

pub fn load_plan_in(bytes: &[u8], expected_sha256: &str, contents: Option<&Path>) -> Result<Plan, ActivationError> {
    let got = crate::boot::sha256_hex(bytes);
    if got != expected_sha256 {
        return Err(ActivationError::Refused(format!("the plan is not the authenticated one (sha256 {got})")));
    }
    let mut v: Value = serde_json::from_slice(bytes).map_err(|e| ActivationError::Refused(format!("the plan does not parse: {e}")))?;
    if let Some(c) = contents {
        resolve_bundle(&mut v, c);
    }
    let plan: Plan = serde_json::from_value(v).map_err(|e| ActivationError::Refused(format!("the plan does not parse: {e}")))?;

    let plain = |n: &str| !n.is_empty() && n.len() <= 64 && !n.starts_with('.') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    for n in [plan.op_id.as_str(), plan.instance_dir.as_str()].into_iter().chain(plan.test_set.iter().map(|r| r.name.as_str())) {
        if !plain(n) {
            return Err(ActivationError::Refused(format!("the plan names {n:?}: use 1-64 letters, digits, '.', '_' or '-'")));
        }
    }
    Ok(plan)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cmd {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reconciled {

    Done(Value),

    NotDone,

    Unknown(String),
}

pub trait Effects {
    fn mint(&self, instance_root: &Path) -> Cmd;
    fn brain_create(&self, tuple: &Value) -> Cmd;

    fn reconcile_brain(&self, tuple: &Value) -> Reconciled;
    fn bind(&self, instance_root: &Path, brain: &Value, nonce_file: &Path) -> Cmd;

    fn verify(&self, instance_root: &Path, brain: &Value, bound: &Value) -> Cmd;

    fn agentd_sha256(&self) -> String;

    fn service_record(&self) -> Option<Value> {
        None
    }
}

#[derive(Debug)]
pub enum ActivationError {
    Busy,

    Refused(String),

    Unknown { step: String, why: String },
    Failed { step: String, why: String },
    Io(String),
}

impl std::fmt::Display for ActivationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivationError::Busy => f.write_str("another RTH operation or a running RTH holds the operation lock"),
            ActivationError::Refused(why) => write!(f, "refused: {why}"),
            ActivationError::Unknown { step, why } => write!(f, "{step}: outcome unknown ({why}); stopped without repeating it"),
            ActivationError::Failed { step, why } => write!(f, "{step} failed: {why}"),
            ActivationError::Io(e) => f.write_str(e),
        }
    }
}

fn io_e(e: impl std::fmt::Display) -> ActivationError {
    ActivationError::Io(e.to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectRec {
    pub intent: Option<Value>,
    pub helper: Option<Identity>,
    pub release_intent: bool,
    pub released: bool,
    pub outcome: Option<Value>,
}

pub struct Operation<'a> {
    paths: &'a Paths,
    exe: PathBuf,
    rec: Value,
    _lease: Lease,

    pub fault: Option<String>,

    pub pause: Option<String>,

    pub quiesce_within: std::time::Duration,
}

fn activation_state(paths: &Paths) -> Result<Option<Value>, ActivationError> {
    record::read(&paths.activation()).map_err(io_e)
}

fn set_activation(paths: &Paths, state: &str, extra: Value) -> Result<(), ActivationError> {
    let cur = activation_state(paths)?;
    let mut next = json!({"activation_state": state});
    if let Value::Object(m) = extra {
        for (k, v) in m {
            next[k] = v;
        }
    }
    match cur {
        None => {
            next["seq"] = json!(0);
            record::write(&paths.activation(), &next, 0o600).map_err(io_e)
        }
        Some(c) => record::advance(&paths.activation(), &c, next, 0o600).map(|_| ()).map_err(io_e),
    }
}

impl<'a> Operation<'a> {

    pub fn begin(paths: &'a Paths, exe: &Path, plan: &Plan) -> Result<Operation<'a>, ActivationError> {
        std::fs::create_dir_all(&paths.data).map_err(io_e)?;
        let lease = match Lease::acquire(&paths.op_lock(), Mode::Exclusive) {
            Ok(l) => l,
            Err(LeaseError::Busy) => return Err(ActivationError::Busy),
            Err(e) => return Err(io_e(e)),
        };
        if let Some(a) = activation_state(paths)?
            && a["activation_state"] == "interim_active"
            && a["op_id"] != json!(plan.op_id)
        {
            return Err(ActivationError::Refused(format!("an interim instance is already active from operation {}", a["op_id"])));
        }
        let rec = match record::read(&paths.op_record()).map_err(io_e)? {
            Some(r) if r["op_id"] == json!(plan.op_id) && r["plan"] == serde_json::to_value(plan).unwrap() => r,

            Some(r) if r["effects"].as_object().is_some_and(|m| m.is_empty()) && r["rollback"].is_null() => {
                let kept = (1..).map(|n| paths.data.join(format!("rth.op.superseded-{n}.json"))).find(|p| std::fs::symlink_metadata(p).is_err()).unwrap();
                std::fs::rename(paths.op_record(), &kept).map_err(io_e)?;
                let r = json!({"seq": 0, "op_id": plan.op_id, "plan": plan, "effects": {}, "supersedes": {"op_id": r["op_id"], "record": kept}});
                record::write(&paths.op_record(), &r, 0o600).map_err(io_e)?;
                r
            }
            Some(r) if r["op_id"] == json!(plan.op_id) => return Err(ActivationError::Refused("the operation record's plan differs from this plan".into())),
            Some(r) => return Err(ActivationError::Refused(format!("another operation {} is recorded and not finished", r["op_id"]))),
            None => {
                let r = json!({"seq": 0, "op_id": plan.op_id, "plan": plan, "effects": {}});
                record::write(&paths.op_record(), &r, 0o600).map_err(io_e)?;
                r
            }
        };
        Ok(Operation { paths, exe: exe.to_path_buf(), rec, _lease: lease, fault: None, pause: None, quiesce_within: effect::QUIESCE_WITHIN })
    }

    pub fn plan(&self) -> Plan {
        serde_json::from_value(self.rec["plan"].clone()).unwrap()
    }

    pub fn roots_committed(&self) -> bool {
        self.effect("roots").outcome.is_some()
    }

    fn effect(&self, step: &str) -> EffectRec {
        serde_json::from_value(self.rec["effects"][step].clone()).unwrap_or_default()
    }

    fn save_effect(&mut self, step: &str, e: &EffectRec) -> Result<(), ActivationError> {
        let mut next = self.rec.clone();
        next["effects"][step] = serde_json::to_value(e).unwrap();
        self.rec = record::advance(&self.paths.op_record(), &self.rec, next, 0o600).map_err(io_e)?;
        self.check_fault(&format!("{step}:{}", phase(e)))
    }

    fn check_fault(&self, point: &str) -> Result<(), ActivationError> {
        if cfg!(debug_assertions) && self.fault.as_deref() == Some(point) {

            std::process::exit(99);
        }
        Ok(())
    }

    fn check_pause(&self, point: &str) {
        if !cfg!(debug_assertions) || self.pause.as_deref() != Some(point) {
            return;
        }
        let _ = std::fs::write(self.paths.data.join(format!("pause-{point}.reached")), b"");
        let go = self.paths.data.join(format!("pause-{point}.go"));
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !go.exists() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn instance_root(&self) -> PathBuf {
        self.paths.instance(&self.plan().instance_dir)
    }

    fn external(&mut self, step: &str, intent: Value, cmd: impl FnOnce(&Self) -> Cmd, settle: impl Fn(&Self, Option<i32>) -> Reconciled) -> Result<Value, ActivationError> {
        let mut e = self.effect(step);
        if let Some(o) = &e.outcome {
            return Ok(o.clone());
        }
        match effect::classify(e.intent.is_some(), e.release_intent) {
            Classification::MayHaveRun => {

                let _fence = match effect::quiesce(&self.paths.fence(), e.helper.map(|h| h.pgid)).map_err(io_e)? {
                    Quiescence::Quiet(g) => g,
                    Quiescence::FenceBusy => return Err(ActivationError::Unknown { step: step.into(), why: "the effect fence is held: a fenced writer may still be running".into() }),
                    Quiescence::GroupAlive => return Err(ActivationError::Unknown { step: step.into(), why: "the effect's process group is still alive".into() }),
                };
                return match settle(self, None) {
                    Reconciled::Done(o) => {
                        self.check_pause(&format!("recover:{step}:settled"));
                        e.outcome = Some(o.clone());
                        self.save_effect(step, &e)?;
                        self.check_pause(&format!("recover:{step}:saved"));
                        Ok(o)
                    }
                    Reconciled::NotDone => Err(ActivationError::Unknown { step: step.into(), why: "released but no evidence it ran; not re-run automatically".into() }),
                    Reconciled::Unknown(why) => Err(ActivationError::Unknown { step: step.into(), why }),
                };
            }
            Classification::NotRun | Classification::NotAuthorized => {}
        }

        self.check_pause(&format!("before:{step}"));
        let c = cmd(self);
        e = EffectRec { intent: Some(intent), ..Default::default() };
        self.save_effect(step, &e)?;
        let pending = effect::start(&self.exe, &self.paths.fence(), &c.argv, &c.env, &c.cwd, Stdio::null()).map_err(io_e)?;
        e.helper = Some(pending.identity);
        self.save_effect(step, &e)?;
        e.release_intent = true;
        self.save_effect(step, &e)?;
        let mut child = pending.go().map_err(io_e)?;
        let status = child.wait().map_err(io_e)?;

        self.check_fault(&format!("{step}:ran"))?;
        e.released = true;

        let _quiet = match effect::quiesce_within(&self.paths.fence(), e.helper.map(|h| h.pgid), self.quiesce_within).map_err(io_e)? {
            Quiescence::Quiet(g) => g,
            q => {
                self.save_effect(step, &e)?;
                let why = if matches!(q, Quiescence::FenceBusy) { "the fence is still held" } else { "its process group is still alive" };
                return Err(ActivationError::Unknown { step: step.into(), why: format!("it exited but is not quiescent: {why}") });
            }
        };
        match settle(self, status.code()) {
            Reconciled::Done(o) => {
                e.outcome = Some(o.clone());
                self.save_effect(step, &e)?;
                Ok(o)
            }
            Reconciled::NotDone => Err(ActivationError::Failed { step: step.into(), why: format!("it exited {:?} without its result", status.code()) }),
            Reconciled::Unknown(why) => Err(ActivationError::Unknown { step: step.into(), why }),
        }
    }

    fn fence_legacy(&mut self, l: &Legacy) -> Result<(), ActivationError> {
        let fence_bytes = crate::record::read_bytes_strict(&l.fence).map_err(io_e)?.ok_or_else(|| ActivationError::Refused(format!("{}: no fence source", l.fence.display())))?;
        if crate::boot::sha256_hex(&fence_bytes) != l.fence_sha256 {
            return Err(ActivationError::Refused("the fence source is not the pinned one".into()));
        }
        let absent = matches!(std::fs::symlink_metadata(&l.launcher), Err(e) if e.kind() == io::ErrorKind::NotFound);
        let current = read_nofollow(&l.launcher).map(|b| crate::boot::sha256_hex(&b));
        let e = self.effect("fence");
        if let Some(o) = &e.outcome {
            return match current {
                Ok(c) if c == l.fence_sha256 => Ok(()),
                _ if absent && o["absent"] == true => Ok(()),
                _ if o["absent"] == true => Err(ActivationError::Unknown { step: "fence".into(), why: "a legacy launcher appeared after the fence step found none".into() }),
                _ => Err(ActivationError::Unknown { step: "fence".into(), why: "the legacy launcher is no longer the fence (foreign)".into() }),
            };
        }
        if absent {
            self.save_effect("fence", &EffectRec { intent: Some(json!({"launcher": l.launcher, "absent": true})), release_intent: true, released: true, outcome: Some(json!({"absent": true})), ..Default::default() })?;
            return self.check_fault("fence:done");
        }
        if e.intent.is_none() && matches!(&current, Ok(c) if *c == l.fence_sha256) {
            self.save_effect("fence", &EffectRec { intent: Some(json!({"launcher": l.launcher, "adopted": true})), release_intent: true, released: true, outcome: Some(json!({"installed": false, "adopted": true})), ..Default::default() })?;
            return self.check_fault("fence:done");
        }
        let backup = self.paths.data.join("backups").join(&self.plan().op_id).join("legacy");
        match current {
            Ok(c) if c == l.prior_sha256 => {
                let mode = std::fs::symlink_metadata(&l.launcher).map_err(io_e)?.mode() & 0o7777;
                if e.intent.is_none() {
                    self.save_effect("fence", &EffectRec { intent: Some(json!({"launcher": l.launcher, "prior_sha256": l.prior_sha256, "mode": mode, "backup": backup})), release_intent: true, ..Default::default() })?;
                }
                std::fs::create_dir_all(&backup).map_err(io_e)?;
                std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o700)).map_err(io_e)?;
                let saved = backup.join("launcher");
                match read_nofollow(&saved) {
                    Ok(b) if crate::boot::sha256_hex(&b) == l.prior_sha256 => {}
                    Ok(_) => return Err(ActivationError::Unknown { step: "fence".into(), why: "a different launcher backup exists".into() }),
                    Err(_) => write_private(&saved, &read_nofollow(&l.launcher).map_err(io_e)?).map_err(io_e)?,
                }
                self.check_fault("fence:backup")?;
                replace_file(&l.launcher, &fence_bytes, mode).map_err(io_e)?;
            }
            Ok(c) if c == l.fence_sha256 && e.intent.is_some() => {}
            Ok(_) => return Err(ActivationError::Refused("the legacy launcher is neither the expected prior nor the fence: foreign, nothing changed".into())),
            Err(e) => return Err(ActivationError::Refused(format!("the legacy launcher cannot be read without following links: {e}"))),
        }
        match read_nofollow(&l.launcher) {
            Ok(b) if crate::boot::sha256_hex(&b) == l.fence_sha256 => {}
            _ => return Err(ActivationError::Unknown { step: "fence".into(), why: "the fence did not verify after installation".into() }),
        }
        let mut e = self.effect("fence");
        e.released = true;
        e.outcome = Some(json!({"installed": true, "backup": backup}));
        self.save_effect("fence", &e)?;
        self.check_fault("fence:done")
    }

    fn restore_launcher(&mut self) -> Result<bool, ActivationError> {
        let (Some(l), Some(intent)) = (self.plan().legacy, self.effect("fence").intent) else {
            return Ok(false);
        };

        if intent["absent"] == true || intent["adopted"] == true {
            return Ok(false);
        }
        let current = read_nofollow(&l.launcher).map(|b| crate::boot::sha256_hex(&b)).map_err(io_e)?;
        if current == l.prior_sha256 {
            return Ok(true);
        }
        if current != l.fence_sha256 {
            return Err(ActivationError::Refused("the legacy launcher is neither the fence nor its prior: never overwritten".into()));
        }
        let saved = read_nofollow(&PathBuf::from(intent["backup"].as_str().unwrap_or_default()).join("launcher")).map_err(io_e)?;
        if crate::boot::sha256_hex(&saved) != l.prior_sha256 {
            return Err(ActivationError::Refused("the launcher backup no longer matches its prior".into()));
        }
        replace_file(&l.launcher, &saved, intent["mode"].as_u64().unwrap_or(0o644) as u32).map_err(io_e)?;
        Ok(true)
    }

    pub fn run(&mut self, fx: &dyn Effects) -> Result<Value, ActivationError> {
        let plan = self.plan();

        match &plan.legacy {
            Some(l) => self.fence_legacy(l)?,
            None if self.effect("capture").intent.is_none() && plan.test_set.iter().any(|r| std::fs::symlink_metadata(&r.path).is_ok()) => {
                return Err(ActivationError::Refused("a test set exists but the plan fences no legacy launcher".into()));
            }
            None => {}
        }
        let found = crate::live::live(&plan.live).map_err(io_e)?;
        if !found.is_empty() {

            if self.effect("capture").intent.is_none() && self.restore_launcher()? {
                self.save_effect("fence", &EffectRec::default())?;
            }
            return Err(ActivationError::Refused(format!("Quit the current RTH to continue ({})", found.join("; "))));
        }

        let cap = self.effect("capture");
        if cap.outcome.is_none() {
            let dest = self.paths.backup(&plan.op_id);
            let mut e = cap;
            if e.intent.is_none() {
                for r in &plan.test_set {
                    classify_root(&r.path)?;
                }
                let mut roots = Vec::new();
                for r in &plan.test_set {
                    match std::fs::symlink_metadata(&r.path) {
                        Ok(m) if m.file_type().is_symlink() => return Err(ActivationError::Refused(format!("{} is a symlink", r.path.display()))),
                        Ok(_) => roots.push(json!({"name": r.name, "path": r.path, "digest": capture::sums(&r.path).map_err(io_e)?.digest})),
                        Err(err) if err.kind() == io::ErrorKind::NotFound => roots.push(json!({"name": r.name, "path": r.path, "absent": true})),
                        Err(err) => return Err(io_e(err)),
                    }
                }
                set_activation(self.paths, "capturing", json!({"op_id": plan.op_id}))?;
                e = EffectRec { intent: Some(json!({"dest": dest, "roots": roots})), release_intent: true, ..Default::default() };
                self.save_effect("capture", &e)?;
                capture::make_backup_dir(&dest).map_err(io_e)?;
            }
            if !dest.is_dir() {

                let untouched = e.intent.as_ref().unwrap()["roots"].as_array().map(|rs| rs.iter().all(|r| r["absent"] == true || std::path::Path::new(r["path"].as_str().unwrap_or_default()).exists())).unwrap_or(false);
                if !untouched || std::fs::symlink_metadata(&dest).is_ok() {
                    return Err(ActivationError::Unknown { step: "capture".into(), why: format!("the recorded backup {} is missing and roots have moved", dest.display()) });
                }
                capture::make_backup_dir(&dest).map_err(io_e)?;
            }
            let dest_dev = real_dir(&dest).map_err(io_e)?.dev();
            let roots = e.intent.as_ref().unwrap()["roots"].as_array().cloned().unwrap_or_default();
            for r in &roots {
                let name = r["name"].as_str().unwrap_or_default();
                let origin = PathBuf::from(r["path"].as_str().unwrap_or_default());
                let target = dest.join(name);
                let done: Vec<String> = e.outcome.as_ref().map(|_| vec![]).unwrap_or_else(|| self.rec["capture_moves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default());
                let recorded = done.iter().any(|d| d == name);
                let (o, t) = (std::fs::symlink_metadata(&origin).is_ok(), std::fs::symlink_metadata(&target).is_ok());
                if r["absent"] == true {
                    if o || t {
                        return Err(ActivationError::Unknown { step: "capture".into(), why: format!("{name} was absent at intent but now exists") });
                    }
                    continue;
                }
                let want = r["digest"].as_str().unwrap_or_default();
                match (o, t) {
                    (false, true) => {
                        let got = capture::sums(&target).map_err(io_e)?.digest;
                        if got != want {
                            return Err(ActivationError::Unknown { step: "capture".into(), why: format!("{name} in the backup does not match its recorded digest") });
                        }
                    }
                    (true, false) if !recorded => {
                        if std::fs::symlink_metadata(&origin).map_err(io_e)?.dev() != dest_dev {
                            return Err(ActivationError::Refused(format!("{} is on another filesystem: no copy fallback", origin.display())));
                        }
                        if capture::sums(&origin).map_err(io_e)?.digest != want {
                            return Err(ActivationError::Refused(format!("{name} changed since its digest was recorded; nothing more is moved")));
                        }
                        std::fs::rename(&origin, &target).map_err(|err| if err.raw_os_error() == Some(libc::EXDEV) { ActivationError::Refused(format!("{}: another filesystem", origin.display())) } else { io_e(err) })?;
                    }
                    _ => return Err(ActivationError::Unknown { step: "capture".into(), why: format!("{name}: origin {o}, backup {t}, recorded {recorded}") }),
                }
                if !recorded {
                    let mut next = self.rec.clone();
                    let mut moves = next["capture_moves"].as_array().cloned().unwrap_or_default();
                    moves.push(json!(name));
                    next["capture_moves"] = json!(moves);
                    self.rec = record::advance(&self.paths.op_record(), &self.rec, next, 0o600).map_err(io_e)?;
                    self.check_fault(&format!("capture:moved:{name}"))?;
                }
            }
            e.released = true;
            e.outcome = Some(json!({"dest": dest, "roots": roots}));
            self.save_effect("capture", &e)?;
            set_activation(self.paths, "test_captured", json!({"op_id": plan.op_id, "backup": dest}))?;
        }

        let root = self.instance_root();
        let roots = self.effect("roots");
        if roots.outcome.is_none() {
            match op_marker(&root)? {
                Some(t) if t == plan.op_id => {}
                Some(_) => return Err(ActivationError::Refused(format!("{} belongs to another operation", root.display()))),
                None if std::fs::symlink_metadata(&root).is_ok() => return Err(ActivationError::Refused(format!("{} exists and is not this operation's", root.display()))),
                None => {

                    let staging = root.with_file_name(format!("{}.op-{}", plan.instance_dir, plan.op_id));
                    let e = EffectRec { intent: Some(json!({"root": root, "staging": staging})), release_intent: true, ..Default::default() };
                    self.save_effect("roots", &e)?;
                    std::fs::create_dir_all(root.parent().unwrap()).map_err(io_e)?;
                    let staged = op_marker(&staging)?.is_some_and(|t| t == plan.op_id);
                    if !staged {
                        if std::fs::symlink_metadata(&staging).is_ok() {
                            return Err(ActivationError::Unknown { step: "roots".into(), why: format!("{} exists without this operation's marker", staging.display()) });
                        }
                        std::fs::create_dir(&staging).map_err(io_e)?;
                        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).map_err(io_e)?;
                        write_private(&staging.join(".rth-op"), format!("{}\n", plan.op_id).as_bytes()).map_err(io_e)?;
                    }
                    if std::fs::symlink_metadata(staging.join("agentd")).is_err() {
                        std::fs::create_dir(staging.join("agentd")).map_err(io_e)?;
                        std::fs::set_permissions(staging.join("agentd"), std::fs::Permissions::from_mode(0o700)).map_err(io_e)?;
                    }
                    if let Some(rec) = fx.service_record() {

                        let f = staging.join("memory-service.json");
                        match crate::record::read_json_strict(&f).map_err(|e| ActivationError::Unknown { step: "roots".into(), why: e.to_string() })? {
                            None => write_private(&f, format!("{rec:#}\n").as_bytes()).map_err(io_e)?,
                            Some(was) if crate::service::same_service(&was, &rec) => {}
                            Some(_) => return Err(ActivationError::Unknown { step: "roots".into(), why: "the staged memory-service record is another service".into() }),
                        }
                    }
                    self.check_fault("roots:staged")?;
                    std::fs::rename(&staging, &root).map_err(io_e)?;
                }
            }
            set_activation(self.paths, "interim_initializing", json!({"op_id": plan.op_id, "instance_root": root}))?;
            let mut e = self.effect("roots");
            e.intent.get_or_insert(json!({"root": root}));
            e.release_intent = true;
            e.released = true;
            e.outcome = Some(json!({"root": root}));
            self.save_effect("roots", &e)?;
        }

        let state = root.join("agentd");
        let attempt = match self.effect("mint").intent {
            Some(i) => i["attempt"].as_str().unwrap_or_default().to_string(),
            None => random_hex(16),
        };
        let marker_file = state.join("bootstrap-attempt.json");
        if self.effect("mint").outcome.is_none() && !marker_file.exists() {
            std::fs::write(&marker_file, json!({"attempt": attempt, "op_id": plan.op_id}).to_string()).map_err(io_e)?;
        }
        let mint = self.external("mint", json!({"attempt": attempt}), |op| fx.mint(&op.instance_root()), |op, _code| reconcile_mint(&op.instance_root(), &attempt))?;
        let instance_id = mint["instance_id"].as_str().unwrap_or_default().to_string();

        let tuple = json!({
            "name": format!("rth-interim-{instance_id}"),
            "owner": format!("agent:rth-interim-{instance_id}"),
            "lifecycle": "permanent",
            "control_db": format!("brain_rth_interim_{}_ctl", db_ident(&instance_id)),

            "memory_db": crate::registry::database_name_for(&format!("rth-interim-{instance_id}")),
            "home": root.join("brain-home"),
        });

        let made = self.external("brain", json!({"tuple": tuple}), |_| fx.brain_create(&tuple), |_, _code| fx.reconcile_brain(&tuple))?;
        let brain = json!({"brain_id": made["brain_id"], "tuple": tuple});

        let nonce = root.join("birth.nonce");
        let bind_rec = self.effect("bind");
        if bind_rec.outcome.is_none() {
            match (bind_rec.release_intent, nonce.exists()) {
                (true, false) => return Err(ActivationError::Unknown { step: "bind".into(), why: "the nonce file is missing after a possible sentinel write; a new nonce is never made".into() }),
                (false, false) => {
                    write_private(&nonce, random_hex(32).as_bytes()).map_err(io_e)?;
                }
                _ => {}
            }
        }
        let nonce_sha = sha256_file(&nonce).map_err(io_e)?;
        let binding = self.external("bind", json!({"brain": brain, "nonce_sha256": nonce_sha}), |op| fx.bind(&op.instance_root(), &brain, &nonce), |op, _code| reconcile_bind(&op.instance_root(), &brain, &instance_id))?;

        let bound = match self.effect("verify").intent {
            Some(i) => i["bound"].clone(),
            None => {
                let sentinel = binding["record"]["sentinel_ref"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| ActivationError::Failed { step: "verify".into(), why: "the binding record names no sentinel".into() })?;
                json!({"op_id": plan.op_id, "attempt": random_hex(16), "instance_id": instance_id, "brain_id": brain["brain_id"], "sentinel_ref": sentinel, "agentd_sha256": fx.agentd_sha256()})
            }
        };
        let verified = self.external("verify", json!({"bound": bound}), |op| fx.verify(&op.instance_root(), &brain, &bound), |op, code| reconcile_verify(&op.instance_root(), &bound, code))?;

        let fin = json!({"op_id": plan.op_id, "instance_root": root, "instance_id": instance_id, "brain": brain, "binding": binding, "verified": verified, "backup": self.paths.backup(&plan.op_id)});
        if activation_state(self.paths)?.map(|a| a["activation_state"] == "interim_active").unwrap_or(false) {
            return activation_state(self.paths).map(|a| a.unwrap());
        }
        set_activation(self.paths, "interim_active", fin)?;
        self.check_fault("commit:done")?;
        activation_state(self.paths).map(|a| a.unwrap())
    }
}

fn phase(e: &EffectRec) -> &'static str {
    match (e.intent.is_some(), e.helper.is_some(), e.release_intent, e.outcome.is_some()) {
        (_, _, _, true) => "done",
        (_, _, true, false) => "released",
        (_, true, false, false) => "helper",
        (true, false, false, false) => "intent",
        _ => "none",
    }
}

fn reconcile_mint(root: &Path, attempt: &str) -> Reconciled {
    let state = root.join("agentd");
    let (marker, st) = match (crate::record::read_json_strict(&state.join("bootstrap-attempt.json")), crate::record::read_json_strict(&state.join("state.json"))) {
        (Ok(m), Ok(s)) => (m, s),
        (Err(e), _) | (_, Err(e)) => return Reconciled::Unknown(e.to_string()),
    };
    match (marker, st) {
        (Some(m), Some(s)) if m["attempt"] == json!(attempt) => match s["instance_id"].as_str() {
            Some(id) if id.starts_with("inst-") => Reconciled::Done(json!({"instance_id": id})),
            _ => Reconciled::Unknown("state.json names no instance".into()),
        },
        (Some(_), Some(_)) => Reconciled::Unknown("another attempt's marker beside state.json: foreign state".into()),
        (Some(m), None) if m["attempt"] != json!(attempt) => Reconciled::Unknown("another attempt's marker: foreign".into()),
        (Some(_), None) => Reconciled::Unknown("the mint was released but wrote no state.json".into()),
        (None, Some(_)) => Reconciled::Unknown("state.json without this attempt's marker: foreign".into()),
        (None, None) => Reconciled::Unknown("neither marker nor state".into()),
    }
}

fn reconcile_bind(root: &Path, brain: &Value, instance_id: &str) -> Reconciled {
    let p = root.join("agentd/memory-binding-record.json");
    let found = match crate::record::read_json_strict(&p) {
        Ok(f) => f,
        Err(e) => return Reconciled::Unknown(e.to_string()),
    };
    match found {
        Some(r) if r["instance_id"] == json!(instance_id) && r["brain_id"] == brain["brain_id"] => Reconciled::Done(json!({"record": r})),
        Some(_) => Reconciled::Unknown("a binding record for another instance or brain".into()),
        None => Reconciled::Unknown("no binding record".into()),
    }
}

fn reconcile_verify(root: &Path, bound: &Value, code: Option<i32>) -> Reconciled {
    let found = match crate::record::read_json_strict(&root.join("verify.json")) {
        Ok(f) => f,
        Err(e) => return Reconciled::Unknown(e.to_string()),
    };
    match found {
        Some(v) if &v["bound"] != bound => Reconciled::Unknown("a verification receipt from another attempt".into()),
        Some(v) if !crate::boot::receipt_agrees(&v) => Reconciled::Unknown(format!("the verification boot did not observe its bound identity: {v}")),
        Some(v) if code.is_none_or(|c| c == 0) && v["readiness"].as_str().is_some_and(|r| r.starts_with("attached")) => Reconciled::Done(v),
        Some(v) => Reconciled::Unknown(format!("verify did not pass: {v}")),
        None => Reconciled::NotDone,
    }
}

pub fn resolve_bundle(v: &mut Value, contents: &Path) {
    match v {
        Value::String(s) => {
            if let Some(rel) = s.strip_prefix("@bundle/")
                && !rel.split('/').any(|c| c.is_empty() || c == "." || c == "..")
            {
                *s = contents.join(rel).display().to_string();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| resolve_bundle(x, contents)),
        Value::Object(o) => o.values_mut().for_each(|x| resolve_bundle(x, contents)),
        _ => {}
    }
}

fn classify_root(path: &Path) -> Result<(), ActivationError> {
    let m = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ActivationError::Refused(format!("{} cannot be inspected: {e}", path.display()))),
    };
    let foreign = |why: &str| Err(ActivationError::Refused(format!("{} is foreign ({why}): nothing moved", path.display())));
    if !m.file_type().is_dir() {
        return foreign("not a real directory");
    }
    if m.uid() != unsafe { libc::getuid() } {
        return foreign("another owner");
    }
    if std::fs::canonicalize(path).map_err(io_e)? != path {
        return foreign("reached through a symlink");
    }
    Ok(())
}

fn op_marker(dir: &Path) -> Result<Option<String>, ActivationError> {
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ActivationError::Refused(format!("{} cannot be inspected: {e}", dir.display()))),
        Ok(m) if !m.file_type().is_dir() => return Err(ActivationError::Refused(format!("{} is not a real directory", dir.display()))),
        Ok(_) => {}
    }
    let b = crate::record::read_bytes_strict(&dir.join(".rth-op")).map_err(|e| ActivationError::Refused(e.to_string()))?;
    Ok(b.map(|b| String::from_utf8_lossy(&b).trim().to_string()))
}

pub(crate) fn real_dir(p: &Path) -> io::Result<std::fs::Metadata> {
    let m = std::fs::symlink_metadata(p)?;
    if !m.file_type().is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{} is not a real directory", p.display())));
    }
    Ok(m)
}

fn read_nofollow(p: &Path) -> io::Result<Vec<u8>> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(p)?;
    if !f.metadata()?.is_file() {
        return Err(io::Error::other(format!("{} is not a regular file", p.display())));
    }
    let mut b = Vec::new();
    io::Read::read_to_end(&mut f, &mut b)?;
    Ok(b)
}

pub(crate) fn replace_file(target: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = target.parent().ok_or_else(|| io::Error::other("no parent"))?;
    let tmp = dir.join(format!(".{}.rth-{}", target.file_name().and_then(|n| n.to_str()).unwrap_or("launcher"), random_hex(6)));
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
    f.write_all(bytes)?;
    f.set_permissions(std::fs::Permissions::from_mode(mode))?;
    f.sync_all()?;
    drop(f);
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::File::open(dir)?.sync_all()
}

fn db_ident(instance_id: &str) -> String {
    instance_id.trim_start_matches("inst-").chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let ok = std::fs::File::open("/dev/urandom").and_then(|mut f| io::Read::read_exact(&mut f, &mut buf)).is_ok();
    assert!(ok, "no randomness");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn sha256_file(p: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let b = crate::record::read_bytes_strict(p)?.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{}: missing", p.display())))?;
    Ok(Sha256::digest(b).iter().map(|b| format!("{b:02x}")).collect())
}

pub fn admitted(paths: &Paths) -> Result<Option<Value>, ActivationError> {
    Ok(activation_state(paths)?.filter(|a| a["activation_state"] == "interim_active"))
}

impl<'a> Operation<'a> {

    pub fn rollback(&mut self) -> Result<Value, ActivationError> {
        let plan = self.plan();
        let Some(a) = activation_state(self.paths)? else {

            if self.effect("capture").intent.is_none() && self.restore_launcher()? {
                return Ok(json!({"activation_state": "test_active", "op_id": plan.op_id, "launcher_restored": true}));
            }
            return Err(ActivationError::Refused("nothing was activated".into()));
        };
        if a["op_id"] != json!(plan.op_id) {
            return Err(ActivationError::Refused(format!("the activation belongs to operation {}", a["op_id"])));
        }
        let state = a["activation_state"].as_str().unwrap_or_default().to_string();
        if state == "test_active" {

            self.restore_launcher()?;
            return Ok(a);
        }
        if !["interim_active", "test_captured", "interim_initializing", "interim_preserving", "interim_preserved", "test_restoring"].contains(&state.as_str()) {
            return Err(ActivationError::Refused(format!("rollback is not defined from {state}")));
        }
        let cap = self.effect("capture");
        let Some(captured) = cap.outcome.clone() else {
            return Err(ActivationError::Refused("the capture is not complete: recover it first".into()));
        };

        let fence = match effect::quiesce(&self.paths.fence(), None).map_err(io_e)? {
            Quiescence::Quiet(g) => g,
            _ => return Err(ActivationError::Refused("an effect may still be running (the fence is held)".into())),
        };
        for step in ["mint", "brain", "bind", "verify"] {
            if let Some(h) = self.effect(step).helper
                && effect::group_alive(h.pgid).map_err(io_e)?
            {
                return Err(ActivationError::Refused(format!("{step}'s process group is still alive")));
            }
        }
        let root = self.instance_root();
        let preserved = self.paths.data.join("backups").join(&plan.op_id).join(format!("interim-{}", a["seq"]));
        let roots = captured["roots"].as_array().cloned().unwrap_or_default();
        let rb = self.rec["rollback"].clone();
        let preserved = rb["preserved"].as_str().map(PathBuf::from).unwrap_or(preserved);

        let backup = self.paths.backup(&plan.op_id);
        let restored: Vec<String> = rb["restored"].as_array().map(|x| x.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        for r in &roots {
            let name = r["name"].as_str().unwrap_or_default();
            if r["absent"] == true || restored.iter().any(|x| x == name) {
                continue;
            }
            let origin = Path::new(r["path"].as_str().unwrap_or_default());
            if std::fs::symlink_metadata(origin).is_ok() {
                return Err(ActivationError::Refused(format!("{} is occupied: never overwritten", origin.display())));
            }
            if capture::sums(&backup.join(name)).map_err(io_e)?.digest != r["digest"].as_str().unwrap_or_default() {
                return Err(ActivationError::Refused(format!("the captured {name} no longer matches its digest")));
            }
        }

        if rb["preserved_done"] != true {
            set_activation(self.paths, "interim_preserving", json!({"op_id": plan.op_id}))?;
            self.save_rollback(json!({"preserved": preserved}))?;
            if root.exists() {
                std::fs::create_dir_all(&preserved).map_err(io_e)?;
                std::fs::set_permissions(&preserved, std::fs::Permissions::from_mode(0o700)).map_err(io_e)?;
                let target = preserved.join("instance");
                if std::fs::symlink_metadata(&target).is_ok() {
                    return Err(ActivationError::Unknown { step: "rollback".into(), why: format!("{} already exists", target.display()) });
                }
                std::fs::rename(&root, &target).map_err(io_e)?;
            } else if !preserved.join("instance").exists() {
                return Err(ActivationError::Unknown { step: "rollback".into(), why: "the interim root is neither live nor preserved".into() });
            }
            self.save_rollback(json!({"preserved": preserved, "preserved_done": true}))?;
            set_activation(self.paths, "interim_preserved", json!({"op_id": plan.op_id, "preserved": preserved}))?;
            self.check_fault("rollback:preserved")?;
        }

        set_activation(self.paths, "test_restoring", json!({"op_id": plan.op_id, "preserved": preserved}))?;
        let mut done = restored;
        for r in &roots {
            let name = r["name"].as_str().unwrap_or_default().to_string();
            if r["absent"] == true || done.contains(&name) {
                continue;
            }
            let origin = PathBuf::from(r["path"].as_str().unwrap_or_default());
            if let Some(parent) = origin.parent() {
                std::fs::create_dir_all(parent).map_err(io_e)?;
            }
            std::fs::rename(backup.join(&name), &origin).map_err(io_e)?;
            done.push(name);
            self.save_rollback(json!({"preserved": preserved, "preserved_done": true, "restored": done}))?;
        }
        drop(fence);

        self.check_fault("rollback:restored")?;
        let launcher = self.restore_launcher()?;
        set_activation(self.paths, "test_active", json!({"op_id": plan.op_id, "preserved": preserved, "rolled_back": true, "launcher_restored": launcher}))?;
        activation_state(self.paths).map(|a| a.unwrap())
    }

    fn save_rollback(&mut self, v: Value) -> Result<(), ActivationError> {
        let mut next = self.rec.clone();
        next["rollback"] = v;
        self.rec = record::advance(&self.paths.op_record(), &self.rec, next, 0o600).map_err(io_e)?;
        Ok(())
    }
}
