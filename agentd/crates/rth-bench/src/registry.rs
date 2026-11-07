

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

use crate::fixture::*;
use crate::scorer::{CRITICAL, MANDATORY, SCORER_ID, SCORER_VERSION};
use crate::util::{sha256_file, sha256_hex, strict_parse};

pub struct LoadedFixture {
    pub fixture: Fixture,
    pub sha256: String,
    pub path: PathBuf,
}

pub struct LoadedTask {
    pub def: TaskDef,
    pub sha256: String,
    pub path: PathBuf,
}

pub struct Registry {
    pub root: PathBuf,
    pub suite: Suite,
    pub suite_sha256: String,
    pub suite_path: PathBuf,
    pub tasks: BTreeMap<TaskRef, LoadedTask>,
    pub fixtures: BTreeMap<String, LoadedFixture>,
}

impl Registry {
    pub fn task(&self, r: &TaskRef) -> &TaskDef {
        &self.tasks[r].def
    }

    pub fn scorer(&self) -> &ScorerRef {
        &self.tasks.values().next().expect("a suite has tasks").def.scorer
    }

    pub fn scenarios(&self, split: Split) -> Vec<&LoadedFixture> {
        let ids = match split {
            Split::Dev => &self.suite.split_manifest.dev,
            Split::Promotion => &self.suite.split_manifest.promotion,
            Split::Probe => return self.suite.probes.iter().map(|p| &self.fixtures[&p.scenario_id]).collect(),
        };
        ids.iter().map(|id| &self.fixtures[id]).collect()
    }
}

pub fn suite_path(root: &Path, suite_id: &str) -> PathBuf {
    root.join("suites").join(format!("{suite_id}.json"))
}

pub fn task_path(root: &Path, r: &TaskRef) -> PathBuf {
    root.join("tasks").join(&r.id).join(&r.version).join("task.json")
}

pub fn fixture_path(root: &Path, split: Split, scenario_id: &str) -> PathBuf {
    let dir = match split {
        Split::Dev => root.join("fixtures").join("dev"),
        Split::Probe => root.join("fixtures").join("probe"),
        Split::Promotion => root.join("private").join("promotion"),
    };
    dir.join(format!("{scenario_id}.json"))
}

pub fn manifest_hash<'a>(entries: impl IntoIterator<Item = (Split, &'a str, &'a str)>) -> String {
    let mut lines: Vec<String> = entries.into_iter().map(|(s, id, h)| format!("{} {id} {h}\n", s.as_str())).collect();
    lines.sort();
    sha256_hex(lines.concat().as_bytes())
}

pub fn load(root: &Path, suite_id: &str) -> Result<Registry> {
    let root = root.canonicalize().with_context(|| format!("bench dir {}", root.display()))?;
    let suite_path = suite_path(&root, suite_id);
    let bytes = std::fs::read(&suite_path).with_context(|| format!("reading {}", suite_path.display()))?;
    let suite: Suite = strict_parse(&bytes, &suite_path.display().to_string())?;
    let suite_sha256 = sha256_hex(&bytes);
    ensure!(suite.suite_format == SUITE_FORMAT, "suite format {}", suite.suite_format);
    ensure!(suite.suite_id == suite_id, "suite file names {}", suite.suite_id);

    let mut task_refs: Vec<TaskRef> = suite.tasks.iter().map(|t| t.task.clone()).collect();
    task_refs.extend(suite.probes.iter().map(|p| p.task.clone()));
    let mut tasks = BTreeMap::new();
    let mut fixtures = BTreeMap::new();
    for r in task_refs {
        if tasks.contains_key(&r) {
            continue;
        }
        let path = task_path(&root, &r);
        let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let def: TaskDef = strict_parse(&bytes, &path.display().to_string())?;
        verify_task(&def, &r)?;
        for entry in &def.fixtures {
            let path = fixture_path(&root, entry.split, &entry.scenario_id);
            let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let sha256 = sha256_hex(&bytes);
            ensure!(sha256 == entry.sha256, "{}: content hash {sha256} != pinned {} (a changed fixture needs a new task version)", path.display(), entry.sha256);
            let fixture: Fixture = strict_parse(&bytes, &path.display().to_string())?;
            ensure!(fixture.scenario_id == entry.scenario_id && fixture.split == entry.split && fixture.task == r, "{}: id/split/task disagree with {r}", path.display());
            validate_fixture(&fixture, &def).with_context(|| format!("{}", path.display()))?;
            ensure!(fixtures.insert(entry.scenario_id.clone(), LoadedFixture { fixture, sha256, path }).is_none(), "duplicate scenario {}", entry.scenario_id);
        }
        let got = manifest_hash(def.fixtures.iter().map(|e| (e.split, e.scenario_id.as_str(), e.sha256.as_str())));
        ensure!(got == def.fixture_manifest_sha256, "{r}: fixture manifest hash {got} != pinned {}", def.fixture_manifest_sha256);
        tasks.insert(r, LoadedTask { def, sha256: sha256_hex(&bytes), path });
    }

    let suite_tasks: BTreeSet<&TaskRef> = suite.tasks.iter().map(|t| &t.task).collect();
    for (split, ids) in [(Split::Dev, &suite.split_manifest.dev), (Split::Promotion, &suite.split_manifest.promotion)] {
        let listed: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        ensure!(listed.len() == ids.len(), "suite {split:?} split lists a scenario twice");
        let owned: BTreeSet<&str> = fixtures
            .values()
            .filter(|f| f.fixture.split == split && suite_tasks.contains(&f.fixture.task))
            .map(|f| f.fixture.scenario_id.as_str())
            .collect();
        ensure!(listed == owned, "suite {split:?} split {listed:?} != task fixtures {owned:?}");
    }
    let split_entries = suite.split_manifest.dev.iter().map(|id| (Split::Dev, id)).chain(suite.split_manifest.promotion.iter().map(|id| (Split::Promotion, id)));
    let got = manifest_hash(split_entries.map(|(s, id)| (s, id.as_str(), fixtures[id].sha256.as_str())));
    ensure!(got == suite.split_manifest.sha256, "split manifest hash {got} != pinned {}", suite.split_manifest.sha256);

    let mut group_split: BTreeMap<&str, Split> = BTreeMap::new();
    for f in fixtures.values() {
        let prev = group_split.entry(&f.fixture.group_id).or_insert(f.fixture.split);
        ensure!(*prev == f.fixture.split, "group {} straddles {prev:?} and {:?}", f.fixture.group_id, f.fixture.split);
    }
    for p in &suite.probes {
        let f = fixtures.get(&p.scenario_id).with_context(|| format!("probe {} not pinned by {}", p.scenario_id, p.task))?;
        ensure!(f.fixture.split == Split::Probe && p.repeats > 0, "probe {} must be a probe fixture", p.scenario_id);
    }
    let scorers: BTreeSet<(&str, &str)> = tasks.values().map(|t| (t.def.scorer.id.as_str(), t.def.scorer.version.as_str())).collect();
    ensure!(scorers.len() == 1, "a suite's tasks must share one scorer revision, found {scorers:?}");
    let weights: f64 = suite.tasks.iter().map(|t| t.weight).sum();
    ensure!((weights - 1.0).abs() < 1e-9, "suite weights sum to {weights}, not 1");

    Ok(Registry { root, suite, suite_sha256, suite_path, tasks, fixtures })
}

fn verify_task(def: &TaskDef, r: &TaskRef) -> Result<()> {
    ensure!(def.task_format == TASK_FORMAT, "{r}: task format {}", def.task_format);
    ensure!(def.task_id == r.id && def.version == r.version, "{r}: task.json names {}@{}", def.task_id, def.version);

    let (critical, mandatory): (&[&str], &[&str]) = if def.scorer == (ScorerRef { id: SCORER_ID.into(), version: SCORER_VERSION.into() }) {
        (CRITICAL, MANDATORY)
    } else if def.scorer == (ScorerRef { id: crate::audio::SCORER_ID.into(), version: crate::audio::SCORER_VERSION.into() }) {
        (crate::audio::CRITICAL, crate::audio::MANDATORY)
    } else {
        bail!(
            "{r}: wants scorer {}@{}, this binary is {SCORER_ID}@{SCORER_VERSION} or {}@{}",
            def.scorer.id,
            def.scorer.version,
            crate::audio::SCORER_ID,
            crate::audio::SCORER_VERSION
        );
    };
    let declared: BTreeSet<&str> = def.critical_checks.iter().map(String::as_str).collect();
    let ours: BTreeSet<&str> = critical.iter().copied().collect();
    ensure!(declared == ours, "{r}: critical checks {declared:?} != scorer vetoes {ours:?}");
    for m in &def.mandatory_checks {
        ensure!(mandatory.contains(&m.as_str()), "{r}: unknown mandatory check {m}");
    }
    Ok(())
}

pub fn validate_fixture(fx: &Fixture, task: &TaskDef) -> Result<()> {
    ensure!(fx.fixture_format == FIXTURE_FORMAT, "fixture format {}", fx.fixture_format);
    ensure!(fx.feed_mode == "paced_causal", "feed mode {} not supported by this runner", fx.feed_mode);
    let mut labels: HashSet<&str> = HashSet::new();
    let mut requests: HashSet<&str> = HashSet::new();
    let mut conns: HashSet<&str> = HashSet::new();
    let mut reactive = false;
    for (i, s) in fx.steps.iter().enumerate() {
        let timing = [s.at_ms.is_some(), s.after_ms.is_some(), s.on.is_some()].iter().filter(|b| **b).count();
        ensure!(timing <= 1, "step {i}: at most one of at_ms, after_ms, on");
        ensure!(s.delay_ms == 0 || s.on.is_some(), "step {i}: delay_ms needs an `on` trigger");
        if let Some(t) = &s.on {
            reactive = true;
            let (Trigger::ResponseStarted { request, .. } | Trigger::Delta { request, .. } | Trigger::Terminal { request, .. } | Trigger::Audio { request, .. }) = t;
            ensure!(requests.contains(request.as_str()), "step {i}: trigger names unknown request {request}");
        }
        match &s.op {
            Op::Attach { .. } | Op::ConnectNoAttach {} => {
                ensure!(conns.insert(&s.conn), "step {i}: connection {} opened twice (use a new name to reconnect)", s.conn);
            }
            other => ensure!(conns.contains(s.conn.as_str()), "step {i}: {} on unopened connection {}", other.name(), s.conn),
        }
        match &s.op {
            Op::UserText { label, .. } | Op::Raw { label, .. } | Op::UserAudio { label, .. } => {
                ensure!(labels.insert(label), "step {i}: duplicate label {label}");
                if matches!(s.op, Op::UserText { .. } | Op::UserAudio { .. }) {
                    requests.insert(label);
                }
                if let Op::UserAudio { ms, chunk_ms, tone_hz, .. } = &s.op {
                    ensure!(*chunk_ms > 0 && *ms >= *chunk_ms && *ms <= 10_000, "step {i}: user_audio needs 0 < chunk_ms <= ms <= 10000");
                    ensure!(tone_hz.is_finite() && *tone_hz >= 0.0 && *tone_hz < 8_000.0, "step {i}: tone_hz out of range");
                }
            }
            Op::BargeIn { label: Some(l) } => ensure!(labels.insert(l), "step {i}: duplicate label {l}"),
            Op::Cancel { label, target } => {
                if let Some(l) = label {
                    ensure!(labels.insert(l), "step {i}: duplicate label {l}");
                }
                if let Target::Request(r) = target {
                    ensure!(requests.contains(r.as_str()), "step {i}: cancel names unknown request {r}");
                }
            }
            Op::Wait { until: Until::Terminal { requests: rs }, .. } => {
                for r in rs {
                    ensure!(requests.contains(r.as_str()), "step {i}: wait names unknown request {r}");
                }
            }
            _ => {}
        }
    }
    ensure!(reactive == fx.reactive, "reactive flag is {} but triggers say {reactive}", fx.reactive);
    let audio_scored = task.scorer.id == crate::audio::SCORER_ID;
    let audio_ops = fx.steps.iter().any(|s| matches!(s.op, Op::UserAudio { .. } | Op::Speaker {} | Op::BargeIn { .. }));
    ensure!(!audio_ops || audio_scored, "audio ops need the audio scorer");
    ensure!(!fx.expect.flush || fx.steps.iter().any(|s| matches!(s.op, Op::BargeIn { .. })), "expect.flush needs a barge_in step");
    let e = &fx.expect;
    for l in e.completed.iter().chain(&e.cancelled).chain(&e.no_response).chain(&e.unobserved_ok).chain(e.contract.keys()).chain(&e.audio).chain(e.ingress_gaps.keys()) {
        ensure!(requests.contains(l.as_str()), "expect names unknown request {l}");
    }
    for (a, b) in &e.overlap {
        ensure!(requests.contains(a.as_str()) && requests.contains(b.as_str()), "overlap names unknown request");
    }
    for name in e.contract.values() {
        ensure!(task.response_contract.contains_key(name), "task declares no response contract {name}");
    }
    for c in e.roles.keys().chain(e.rejected_total.keys()).chain(&e.closed_by_agent).chain(e.rejections.iter().map(|r| &r.conn)) {
        ensure!(conns.contains(c.as_str()), "expect names unknown connection {c}");
    }
    for r in &e.rejections {
        if let Some(l) = &r.cause {
            ensure!(labels.contains(l.as_str()), "rejection cause names unknown label {l}");
        }
    }
    Ok(())
}

pub fn seal(root: &Path, suite_id: &str) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    let suite_path = suite_path(root, suite_id);
    let mut suite: Suite = strict_parse(&std::fs::read(&suite_path)?, "suite")?;
    let mut shas: BTreeMap<String, String> = BTreeMap::new();
    let mut refs: Vec<TaskRef> = suite.tasks.iter().map(|t| t.task.clone()).collect();
    refs.extend(suite.probes.iter().map(|p| p.task.clone()));
    refs.dedup();
    for r in refs {
        let path = task_path(root, &r);
        let mut def: TaskDef = strict_parse(&std::fs::read(&path)?, "task")?;
        let mut dirty = false;
        for e in &mut def.fixtures {
            let sha = sha256_file(&fixture_path(root, e.split, &e.scenario_id))?;
            if e.sha256.is_empty() {
                e.sha256 = sha.clone();
                dirty = true;
            } else if e.sha256 != sha {
                bail!("{}: fixture {} changed; bump the task version instead of resealing", r, e.scenario_id);
            }
            shas.insert(e.scenario_id.clone(), sha);
        }
        let manifest = manifest_hash(def.fixtures.iter().map(|e| (e.split, e.scenario_id.as_str(), e.sha256.as_str())));
        if def.fixture_manifest_sha256.is_empty() {
            def.fixture_manifest_sha256 = manifest;
            dirty = true;
        } else if def.fixture_manifest_sha256 != manifest {
            bail!("{r}: fixture manifest changed; bump the task version");
        }
        if dirty {
            std::fs::write(&path, serde_json::to_string_pretty(&def)? + "\n")?;
            changed.push(path.display().to_string());
        }
    }
    let entries = suite.split_manifest.dev.iter().map(|id| (Split::Dev, id)).chain(suite.split_manifest.promotion.iter().map(|id| (Split::Promotion, id)));
    let entries: Vec<(Split, String, String)> = entries
        .map(|(s, id)| Ok((s, id.clone(), shas.get(id).with_context(|| format!("{id} not owned by any suite task"))?.clone())))
        .collect::<Result<_>>()?;
    let manifest = manifest_hash(entries.iter().map(|(s, id, h)| (*s, id.as_str(), h.as_str())));
    if suite.split_manifest.sha256.is_empty() {
        suite.split_manifest.sha256 = manifest;
        std::fs::write(&suite_path, serde_json::to_string_pretty(&suite)? + "\n")?;
        changed.push(suite_path.display().to_string());
    } else if suite.split_manifest.sha256 != manifest {
        bail!("split manifest changed; start a new suite epoch");
    }
    Ok(changed)
}
