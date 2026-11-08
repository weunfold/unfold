

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use rth_deploy::controller::{BoxSpec, Controller, Layout};
use rth_deploy::record::Kind;
use rth_deploy::store::{ArtifactStore, PublishOptions};
use rth_deploy::{migrate, selfrepo, source};
use serde_json::json;

const RTH_CORE_LEGACY_SHA: &str = "6de6c27ebda65d41d9efa8b53d0c78d4084318f09780892dfed4912dd343ee10";

struct Args(Vec<String>);

impl Args {
    fn value(&self, name: &str) -> Option<String> {
        self.0.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
    }
    fn need(&self, name: &str) -> Result<String> {
        self.value(name).ok_or_else(|| anyhow!("missing {name}"))
    }
    fn flag(&self, name: &str) -> bool {
        self.0.iter().any(|a| a == name)
    }
}

fn print(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).expect("json"));
}

fn repo_snapshot(c: &Controller) -> Result<Option<rth_deploy::record::RepoSnapshot>> {
    match selfrepo::load_pin(c.box_dir())? {
        Some(pin) => Ok(Some(selfrepo::snapshot(&pin)?)),
        None => Ok(None),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = Args(raw);
    let data = args.value("--data").map(PathBuf::from).unwrap_or_else(rth_deploy::data_dir);
    let layout = Layout::new(&data);

    let mut words = Vec::new();
    let mut skip = false;
    for a in &args.0 {
        if skip {
            skip = false;
            continue;
        }
        if a.starts_with("--") || a == "-m" {
            skip = !matches!(a.as_str(), "--execute" | "--restore-volume" | "--legacy" | "--gemini-secret" | "--no-gemini-secret");
            continue;
        }
        words.push(a.clone());
    }
    let w = |i: usize| words.get(i).map(String::as_str);
    match (w(0), w(1)) {
        (Some("publish"), _) => {
            let store = ArtifactStore::open(&layout.artifacts())?;
            let source = args.value("--source-dir").map(|d| source::workspace_identity(Path::new(&d))).transpose()?;
            let p = store.publish(Path::new(&args.need("--binary")?), PublishOptions { label: args.value("--label"), source, ..Default::default() }, None)?;
            print(&json!({"artifact_id": p.verified.manifest.artifact_id, "manifest_sha256": p.verified.manifest_sha256, "created": p.created, "dir": p.verified.dir, "manifest": p.verified.manifest}));
        }
        (Some("list"), _) => {
            let store = ArtifactStore::open(&layout.artifacts())?;
            for a in store.list()? {
                match a {
                    Ok(v) => println!("{} {} schema {} {:?} {}", v.manifest.artifact_id, v.manifest_sha256, v.manifest.state_schema, v.manifest.attestation, v.manifest.build.label.unwrap_or_default()),
                    Err(e) => println!("INVALID {e}"),
                }
            }
        }
        (Some("box"), Some("create")) => {
            let vm = args.need("--vm")?;
            let mut spec = BoxSpec::disposable(&vm, args.need("--port")?.parse()?);
            spec.gemini_secret = args.flag("--gemini-secret");
            if let Some(b) = args.value("--backend") {
                spec.backend = b;
            }
            Controller::register(&layout, &spec)?;
            let mut c = Controller::open(&layout, &vm)?;
            c.create_box().await?;
            let r = c.activate(&args.need("--artifact")?, Kind::Activate, None, None).await?;
            print(&serde_json::to_value(r)?);
        }
        (Some("box"), Some("recreate")) => {
            let vm = args.need("--vm")?;
            let execute = args.flag("--execute");
            if execute && args.value("--confirm").as_deref() != Some(vm.as_str()) {
                bail!("--execute needs --confirm {vm}");
            }
            let gemini = if args.flag("--gemini-secret") { Some(true) } else if args.flag("--no-gemini-secret") { Some(false) } else { None };
            let web = if args.flag("--no-web") {
                Some(None)
            } else if let Some(p) = args.value("--web-port") {
                Some(Some(rth_deploy::controller::WebPlacement {
                    port: p.parse()?,
                    ui_volume: args.need("--ui-volume")?,
                    web_store: args.need("--web-store")?,
                    owner_dir: args.need("--owner-dir")?,
                    owner_port: args.need("--owner-port")?.parse()?,
                }))
            } else {
                None
            };
            let o = rth_deploy::recreate::RecreateOpts {
                vm,
                artifact: args.need("--artifact")?,
                gemini_secret: gemini,
                backend: args.value("--backend"),
                image: args.value("--image"),
                web,
                candidate: args.value("--candidate"),
                backup_root: args.value("--backup-root").map(PathBuf::from).unwrap_or_else(|| rth_deploy::recreate::default_backup_root(&data)),
                execute,
            };
            print(&rth_deploy::recreate::recreate(&layout, &o, &repo_snapshot).await?);
        }
        (Some("box"), Some("recreate-recover")) => {
            let vm = args.need("--vm")?;
            let execute = args.flag("--execute");
            if execute && args.value("--confirm").as_deref() != Some(vm.as_str()) {
                bail!("--execute needs --confirm {vm}");
            }
            print(&rth_deploy::recreate::recover(&layout, &vm, execute, &repo_snapshot).await?);
        }
        (Some("box"), Some("stop")) => Controller::open(&layout, &args.need("--vm")?)?.stop_box().await?,
        (Some("box"), Some("remove")) => Controller::open(&layout, &args.need("--vm")?)?.remove_box().await?,
        (Some("activate"), _) => {
            let mut c = Controller::open(&layout, &args.need("--vm")?)?;
            let src = repo_snapshot(&c)?;
            let r = c.activate(&args.need("--artifact")?, Kind::Activate, args.value("--candidate"), src).await;
            print(&json!({"result": r.as_ref().map_err(|e| format!("{e:#}")).map(|r| serde_json::to_value(r).unwrap()), "record": c.rec}));
            r?;
        }
        (Some("rollback"), _) => {
            let mut c = Controller::open(&layout, &args.need("--vm")?)?;
            let src = repo_snapshot(&c)?;
            let r = c.rollback(src).await;
            print(&json!({"result": r.as_ref().map_err(|e| format!("{e:#}")).map(|r| serde_json::to_value(r).unwrap()), "record": c.rec}));
            r?;
        }
        (Some("restart"), _) => {
            let mut c = Controller::open(&layout, &args.need("--vm")?)?;
            let r = c.restart().await?;
            print(&serde_json::to_value(r)?);
        }
        (Some("reconcile"), _) => {
            let mut c = Controller::open(&layout, &args.need("--vm")?)?;
            let outcome = c.reconcile().await?;
            print(&json!({"outcome": outcome, "record": c.rec}));
        }
        (Some("status"), _) => {
            let c = Controller::open(&layout, &args.need("--vm")?)?;
            print(&json!({"box": c.spec, "record": c.rec, "self_repo": selfrepo::load_pin(c.box_dir())?, "recreate_pending": c.pending_recreate()}));
        }
        (Some("repo"), Some("init")) => {
            let c = Controller::open(&layout, &args.need("--vm")?)?;
            let work_tree = c.volume_path().await?.join("self-repo");
            let pin = selfrepo::init(c.box_dir(), &work_tree, Path::new(&args.need("--source")?))?;
            print(&serde_json::to_value(pin)?);
        }
        (Some("repo"), Some("status")) => {
            let c = Controller::open(&layout, &args.need("--vm")?)?;
            let pin = selfrepo::load_pin(c.box_dir())?.context("box has no self-repo")?;
            print(&json!({"snapshot": selfrepo::snapshot(&pin)?, "log": selfrepo::log(&pin)?}));
        }
        (Some("repo"), Some("commit")) => {
            let c = Controller::open(&layout, &args.need("--vm")?)?;
            let pin = selfrepo::load_pin(c.box_dir())?.context("box has no self-repo")?;
            let paths: Vec<String> = words[2..].to_vec();
            let head = selfrepo::commit(&pin, &paths, &args.need("-m")?)?;
            print(&json!({"head": head}));
        }
        (Some("repo"), Some("import")) => {
            let c = Controller::open(&layout, &args.need("--vm")?)?;
            let pin = selfrepo::load_pin(c.box_dir())?.context("box has no self-repo")?;
            let actor = selfrepo::Actor::parse(&args.need("--actor")?, args.value("--model"))?;
            let report = selfrepo::import(&pin, Path::new(&args.need("--source")?), &actor, &args.need("-m")?)?;
            let changed = report.added.len() + report.changed.len() + report.removed.len();
            print(&json!({"head": report.head, "changed": changed, "report": report}));
        }
        (Some("migrate-core"), _) => {
            let vm = args.value("--vm").unwrap_or_else(|| "rth-core".into());
            let execute = args.flag("--execute");
            if execute && args.value("--confirm").as_deref() != Some(vm.as_str()) {
                bail!("--execute needs --confirm {vm}");
            }
            if let Some(backup) = args.value("--rollback") {
                migrate::rollback(Path::new(&backup), args.flag("--restore-volume"), execute).await?;
            } else {
                let o = migrate::MigrateOpts {
                    vm,
                    expect_sha: args.value("--expect-sha").unwrap_or_else(|| RTH_CORE_LEGACY_SHA.into()),
                    backend: args.value("--backend").unwrap_or_else(|| "fake".into()),
                    backup_root: args.value("--backup-root").map(PathBuf::from).unwrap_or_else(|| data.join("backups")),
                    data: data.clone(),
                    execute,
                };
                let plan = migrate::migrate(&o).await?;
                if execute {
                    print(&plan);
                }
            }
        }
        _ => {
            eprintln!("{}", include_str!("rth-deploy.rs").lines().take_while(|l| l.starts_with("//!")).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
            std::process::exit(2);
        }
    }
    Ok(())
}
