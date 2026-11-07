

use crate::activation::{Cmd, Operation, Paths, Reconciled};
use crate::product::{BrainBundle, BrainRegistry, Pinned, Product};
use crate::registry::ListRegistry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn out(v: Value, code: i32) -> ! {
    println!("{v}");
    std::process::exit(code)
}

fn refuse(e: impl std::fmt::Display) -> ! {
    out(json!({"error": e.to_string()}), 1)
}

fn read_json(p: &Path) -> Value {
    match crate::record::read_json_strict(p) {
        Ok(Some(v)) => v,
        Ok(None) => refuse(format!("{} is missing", p.display())),
        Err(e) => refuse(e),
    }
}

fn contents(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    (macos.file_name()? == "MacOS" && macos.parent()?.file_name()? == "Contents").then(|| macos.parent().unwrap().to_path_buf())
}

fn user_home() -> Option<PathBuf> {
    let pw = unsafe { libc::getpwuid(libc::getuid()) };
    if pw.is_null() || unsafe { (*pw).pw_dir }.is_null() {
        return None;
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*pw).pw_dir) };
    Some(PathBuf::from(dir.to_string_lossy().into_owned()))
}

fn read_plan(p: &Path) -> Vec<u8> {
    match crate::record::read_bytes_strict(p) {
        Ok(Some(b)) => b,
        Ok(None) => refuse(format!("{} is missing", p.display())),
        Err(e) => refuse(e),
    }
}

fn pin(compiled: Option<&'static str>, test: &Value, name: &str) -> String {
    match compiled {
        Some(s) => s.to_string(),
        None if cfg!(debug_assertions) => test[name].as_str().map(str::to_string).unwrap_or_else(|| refuse(format!("no {name} pin (debug build without test_pins)"))),
        None => refuse(format!("this build carries no {name} pin")),
    }
}

fn plan_sha() -> String {
    match option_env!("RTH_ACTIVATION_PLAN_SHA256") {
        Some(s) => s.to_string(),
        None if cfg!(debug_assertions) => std::env::var("RTH_ACTIVATION_TEST_PLAN_SHA256").unwrap_or_else(|_| refuse("no authenticated plan sha (debug build without RTH_ACTIVATION_TEST_PLAN_SHA256)")),
        None => refuse("this build carries no authenticated plan"),
    }
}

struct FileRegistry(PathBuf);
impl BrainRegistry for FileRegistry {
    fn create(&self, tuple: &Value) -> Cmd {
        let v = json!({"brain_id": "brn_testregistry", "tuple": tuple});
        Cmd { argv: vec!["/bin/sh".into(), "-c".into(), "printf '%s' \"$1\" > \"$2\"".into(), "sh".into(), v.to_string(), self.0.display().to_string()], env: vec![("PATH".into(), "/usr/bin:/bin".into())], cwd: "/".into() }
    }
    fn lookup(&self, tuple: &Value) -> Reconciled {
        match std::fs::read(&self.0).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            Some(r) if &r["tuple"] == tuple => Reconciled::Done(json!({"brain_id": r["brain_id"]})),
            Some(_) => Reconciled::Unknown("a registration with another tuple".into()),
            None => Reconciled::NotDone,
        }
    }
}

pub fn status(data: &Path) -> ! {
    out(crate::seam::records_status(data), 0)
}

pub fn activate(exe: &Path, data: &Path, plan: &Path, product: &Path) -> ! {
    let plan = crate::activation::load_plan_in(&read_plan(plan), &plan_sha(), contents(exe).as_deref()).unwrap_or_else(|e| refuse(e));
    let mut p = read_json(product);
    if let Some(c) = contents(exe) {
        crate::activation::resolve_bundle(&mut p, &c);
    }

    if p["home"].is_null() {
        p["home"] = json!(user_home().unwrap_or_else(|| refuse("this user has no home directory")));
    }
    let test = if cfg!(debug_assertions) { p["test_pins"].clone() } else { Value::Null };
    let path_of = |k: &str| PathBuf::from(p[k].as_str().unwrap_or_else(|| refuse(format!("product.json has no {k}"))));

    let test_registry = p["test_registry"].as_str().filter(|_| cfg!(debug_assertions)).map(PathBuf::from);
    let reg = &p["registry"];
    if test_registry.is_none() {
        if !reg.is_object() {
            refuse("no Brain registry configured; nothing was changed");
        }
        if reg["create"].as_str().is_none() {
            refuse("no pinned interim-brain creation command configured; nothing was changed");
        }
    }
    let paths = Paths { data: data.into() };
    let mut op = Operation::begin(&paths, exe, &plan).unwrap_or_else(|e| refuse(e));

    let service = match p["test_service_record"].as_str() {
        Some(f) if cfg!(debug_assertions) => read_json(Path::new(f)),
        _ => match crate::service::discover_autopg(&path_of("home")) {
            Ok(b) => crate::service::binding_json(&b),
            Err(e) => refuse(format!("{}\n{e}", crate::service::MEMORY_UNAVAILABLE)),
        },
    };

    let root = paths.instance(&plan.instance_dir);
    let accepted = root.join("memory-service.json");
    match crate::record::read_json_strict(&accepted) {
        Ok(Some(was)) if crate::service::same_service(&was, &service) => {}
        Ok(Some(_)) => refuse(format!("the memory service changed since this instance accepted it ({}); its new record needs review", accepted.display())),
        Ok(None) if !op.roots_committed() && std::fs::symlink_metadata(&root).is_err() => {}
        Ok(None) => refuse(format!("recovery needed: {} is missing after this instance was created", accepted.display())),
        Err(e) => refuse(format!("recovery needed: {e}")),
    }
    let file_registry;
    let list_registry;
    let registry: &dyn BrainRegistry = match test_registry {
        Some(f) => {
            file_registry = FileRegistry(f);
            &file_registry
        }
        None => {
            let rp = |k: &str| PathBuf::from(reg[k].as_str().unwrap_or_else(|| refuse(format!("product.json registry has no {k}"))));
            let socket = PathBuf::from(service["socket_dir"].as_str().unwrap_or_else(|| refuse("the validated memory service names no socket_dir")));
            let port = service["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or_else(|| refuse("the validated memory service names no port"));
            list_registry = ListRegistry {
                bun: Pinned { path: rp("bun"), sha256: pin(option_env!("RTH_PIN_BUN_SHA256"), &test, "bun") },
                list: Pinned { path: rp("list"), sha256: option_env!("RTH_PIN_REGISTRY_LIST_SHA256").unwrap_or(crate::registry::REGISTRY_LIST_SHA256).into() },
                brain_root: rp("brain_root"),
                create: Some(Pinned { path: rp("create"), sha256: option_env!("RTH_PIN_BRAIN_CREATE_SHA256").unwrap_or(crate::registry::CREATE_INTERIM_SHA256).into() }),
                interim_db_sha256: option_env!("RTH_PIN_INTERIM_DB_SHA256").unwrap_or(crate::registry::INTERIM_DB_SHA256).into(),
                socket,
                port,
                home: path_of("home"),
            };
            &list_registry
        }
    };
    let fx = Product {
        exe: exe.into(),
        agentd: Pinned { path: path_of("agentd"), sha256: pin(option_env!("RTH_PIN_AGENTD_SHA256"), &test, "agentd") },
        birth: Pinned { path: path_of("birth"), sha256: pin(option_env!("RTH_PIN_BIRTH_SHA256"), &test, "birth") },
        brain: BrainBundle { brain_mcp: path_of("brain_mcp"), brain_src: path_of("brain_src"), seam: path_of("seam"), bun: path_of("bun") },
        registry,
        service,
        path: p["path"].as_str().unwrap_or("/usr/bin:/bin").into(),
        home: path_of("home"),
    };

    let app_json = paths.data.join("app.json");
    let mut app_cfg = match crate::record::read_json_strict(&app_json) {
        Ok(None) => json!({}),
        Ok(Some(v)) if v.is_object() && (v["instance"].is_null() || v["instance"] == json!(plan.instance_dir)) => v,
        Ok(Some(v)) if v.is_object() => refuse(format!("{} names another instance ({}); nothing was changed", app_json.display(), v["instance"])),
        Ok(Some(_)) => refuse(format!("{} is not a JSON object; nothing was changed", app_json.display())),
        Err(e) => refuse(format!("recovery needed: {e}")),
    };
    match op.run(&fx) {
        Ok(a) => {
            if app_cfg["instance"] != json!(plan.instance_dir) {
                app_cfg["instance"] = json!(plan.instance_dir);
                if let Err(e) = crate::activation::replace_file(&app_json, format!("{app_cfg:#}\n").as_bytes(), 0o600) {
                    refuse(format!("activated, but {} could not be written: {e} (rerun activate)", app_json.display()));
                }
            }
            out(json!({"ok": a}), 0)
        }
        Err(e) => refuse(e),
    }
}

pub fn rollback(exe: &Path, data: &Path, plan: &Path) -> ! {
    let plan = crate::activation::load_plan_in(&read_plan(plan), &plan_sha(), contents(exe).as_deref()).unwrap_or_else(|e| refuse(e));
    let paths = Paths { data: data.into() };
    let mut op = Operation::begin(&paths, exe, &plan).unwrap_or_else(|e| refuse(e));
    match op.rollback() {
        Ok(a) => out(json!({"ok": a}), 0),
        Err(e) => refuse(e),
    }
}
