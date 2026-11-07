

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use rth_self::consent::{self, Channel, Entry};
use rth_self::store::{self as files, Loaded};
use rth_self::{Drive, DriveSource, Horizon, HorizonLayer, HorizonStatus, Name, NameOrigin, Proposal, ProposalKind, ProposalStatus, Refused, SelfSpec, MAX_DRIVES};
use serde_json::{json, Value};
use tokio::sync::watch;

const REPLAYS: usize = 32;

pub struct SelfStore {
    dir: PathBuf,
    instance_id: String,

    n_ctx: usize,
    inner: Mutex<Inner>,
    tx: watch::Sender<Arc<SelfSpec>>,

    born_utc_ms: Option<u64>,

    governance_torn: u64,

    governance_outward_reset: bool,

    sleep_host: AtomicU8,
}

const DONE_LISTED: usize = 20;

struct Inner {
    spec: SelfSpec,
    revision: String,

    load_refused: Option<Refused>,
    replies: VecDeque<(String, Value)>,
}

fn refused(r: &Refused) -> Value {
    let mut v = json!({"field": r.field, "code": r.code, "reason": r.reason});
    if let Some(n) = r.need {
        v["need"] = json!(n);
    }
    if let Some(m) = r.max {
        v["max"] = json!(m);
    }
    json!({ "refused": v })
}

fn no(field: &'static str, code: &'static str, reason: &str) -> Value {
    refused(&Refused { field, code, reason: reason.to_string(), need: None, max: None })
}

fn shown(p: &Proposal, resolve: &dyn Fn(&str, &str) -> Option<u64>) -> Value {
    let mut v = serde_json::to_value(p).unwrap_or_default();
    v["evidence"] = p
        .evidence_ids
        .iter()
        .map(|id| {
            let (kind, utc_ms) = match id.split_once(':') {
                Some((k @ ("chat" | "memory" | "sleep"), rest)) => (Some(k), resolve(k, rest)),
                _ => (None, None),
            };
            json!({ "id": id, "kind": kind, "utc_ms": utc_ms })
        })
        .collect();
    v
}

fn revision_of(spec: &SelfSpec) -> String {
    files::sha256_hex(&serde_json::to_vec_pretty(spec).expect("a spec serializes"))
}

const PEOPLE_LINE_STARTS: [&str; 3] = [r#"{"kind":"enrol","person_id":""#, r#"{"kind":"revoke","person_id":""#, r#"{"kind":"correct","person_id":""#];

fn people_line(torn: &[u8]) -> bool {
    torn.len() > br#"{"kind":""#.len() && PEOPLE_LINE_STARTS.iter().any(|p| torn.starts_with(p.as_bytes()) || p.as_bytes().starts_with(torn))
}

#[cfg(test)]
thread_local! { static REPAIR_FAILS_AT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }

fn repair_step(_at: u8) -> std::io::Result<()> {
    #[cfg(test)]
    if REPAIR_FAILS_AT.with(|c| c.get() == _at) {
        REPAIR_FAILS_AT.with(|c| c.set(0));
        return Err(std::io::Error::other("the repair stopped here"));
    }
    Ok(())
}

fn cut_torn_tail(path: &Path, now_utc_ms: u64) -> std::io::Result<(u64, bool)> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, false)),
        Err(e) => return Err(e),
    };
    let keep = bytes.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
    let torn = (bytes.len() - keep) as u64;
    if torn == 0 {
        return Ok((0, false));
    }
    let people = people_line(&bytes[keep..]);
    let mut out = bytes[..keep].to_vec();
    repair_step(1)?;
    if !people {

        for channel in [Channel::Notification, Channel::Omni, Channel::Sleep] {
            let revoke = Entry { channel, recipient: None, copy_version: Some(consent::copy_version_of(channel)), granted_utc_ms: now_utc_ms, revoked: true, reason: Some(consent::TORN_REPAIR.into()) };
            out.extend(serde_json::to_vec(&revoke).expect("an entry serializes"));
            out.push(b'\n');
            repair_step(2)?;
        }
    }
    files::write_atomic(path, &out)?;
    Ok((torn, !people))
}

impl SelfStore {
    pub fn open(dir: &Path, instance_id: &str, n_ctx: usize) -> std::io::Result<SelfStore> {
        let path = dir.join("self.json");
        let default = SelfSpec::release_default(instance_id);
        let (spec, revision, load_refused) = match files::load(&path)? {
            Loaded::Absent => (default.clone(), revision_of(&default), None),
            Loaded::Spec(s) => (s, files::sha256_hex(&std::fs::read(&path)?), None),
            Loaded::Refused(r) => (default.clone(), files::sha256_hex(&std::fs::read(&path)?), Some(r)),
        };
        let (tx, _) = watch::channel(Arc::new(spec.clone()));

        let (governance_torn, governance_outward_reset) = cut_torn_tail(&dir.join("governance.jsonl"), crate::now_utc_ms())?;
        Ok(SelfStore { dir: dir.to_path_buf(), instance_id: instance_id.to_string(), n_ctx, inner: Mutex::new(Inner { spec, revision, load_refused, replies: VecDeque::new() }), tx, born_utc_ms: None, governance_torn, governance_outward_reset, sleep_host: AtomicU8::new(crate::sleeper::HOST_NONE) })
    }

    pub fn with_born(mut self, born_utc_ms: Option<u64>) -> SelfStore {
        self.born_utc_ms = born_utc_ms;
        self
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<SelfSpec>> {
        self.tx.subscribe()
    }

    pub fn spec(&self) -> SelfSpec {
        self.inner.lock().unwrap().spec.clone()
    }

    fn proposals(&self) -> std::io::Result<Vec<Proposal>> {

        let lines = files::read_lines(&self.dir.join("proposals.jsonl"))?;
        let mut out: Vec<Proposal> = Vec::new();
        for v in lines.records {
            let Ok(p) = Proposal::parse(&v.to_string()) else { continue };
            match out.iter_mut().find(|q| q.id == p.id) {
                Some(q) => *q = p,
                None => out.push(p),
            }
        }
        Ok(out)
    }

    fn horizons(&self) -> std::io::Result<Vec<Horizon>> {
        let lines = files::read_lines(&self.dir.join("horizons.jsonl"))?;
        let mut out: Vec<Horizon> = Vec::new();
        for v in lines.records {
            let Ok(h) = Horizon::parse(&v.to_string()) else { continue };
            match out.iter_mut().find(|g| g.id == h.id) {
                Some(g) => *g = h,
                None => out.push(h),
            }
        }
        Ok(out)
    }

    fn put_horizon(&self, h: &Horizon) -> Result<(), Value> {
        files::append(&self.dir.join("horizons.jsonl"), h).map_err(|e| no("horizon", "malformed", &format!("horizons.jsonl was not written: {e}")))
    }

    #[allow(dead_code)]
    pub fn suggest_horizon(&self, layer: HorizonLayer, text: &str, task_id: Option<&str>, now_utc_ms: u64) -> Result<Horizon, Value> {
        let _held = self.inner.lock().unwrap();
        let n = self.horizons().map_err(|e| no("horizon", "malformed", &format!("horizons.jsonl was not readable: {e}")))?.len();
        let line = json!({"id": format!("h-{now_utc_ms}-{n}"), "layer": layer, "status": HorizonStatus::Suggested, "text": text, "created_utc_ms": now_utc_ms, "last_touched_utc_ms": now_utc_ms, "task_id": task_id});
        let h = Horizon::parse(&line.to_string()).map_err(|r| refused(&r))?;
        self.put_horizon(&h)?;
        Ok(h)
    }

    fn move_horizon(&self, id: &str, from: HorizonStatus, to: HorizonStatus, now_utc_ms: u64) -> Result<(), Value> {
        let found = self.horizons().map_err(|e| no("horizon", "malformed", &format!("horizons.jsonl was not readable: {e}")))?.into_iter().find(|h| h.id.as_str() == id);
        let Some(mut h) = found else { return Err(no("horizon_id", "malformed", "no horizon has that id")) };
        if h.status != from {
            return Err(no("horizon_id", "stale", "that horizon is not in that state any more"));
        }
        h.status = to;
        h.last_touched_utc_ms = now_utc_ms;
        self.put_horizon(&h)
    }

    #[allow(dead_code)]
    pub fn touch_horizon(&self, id: &str, now_utc_ms: u64) -> Result<(), Value> {
        let _held = self.inner.lock().unwrap();
        self.move_horizon(id, HorizonStatus::Open, HorizonStatus::Open, now_utc_ms)
    }

    #[allow(dead_code)]
    pub fn finish_horizon(&self, id: &str, now_utc_ms: u64) -> Result<(), Value> {
        let _held = self.inner.lock().unwrap();
        self.move_horizon(id, HorizonStatus::Open, HorizonStatus::Done, now_utc_ms)
    }

    #[allow(dead_code)]
    pub fn ephemeral_horizon(&self, text: &str, now_utc_ms: u64) -> Result<(), Value> {
        let text = rth_self::BoundedText::new(text).map_err(|e| no("text", "text_chars", e.rsplit('\u{1f}').next().unwrap_or(&e)))?;
        let _held = self.inner.lock().unwrap();
        let entry = json!({"kind": "horizon", "layer": "ephemeral", "text": text, "utc_ms": now_utc_ms});
        files::append(&self.dir.join("governance.jsonl"), &entry).map_err(|e| no("text", "malformed", &format!("governance.jsonl was not written: {e}")))
    }

    #[allow(dead_code)]
    pub fn open_horizons(&self, _now_utc_ms: u64) -> Vec<Horizon> {
        self.horizons().unwrap_or_default().into_iter().filter(|h| h.layer == HorizonLayer::Thread && h.status == HorizonStatus::Open).collect()
    }

    pub fn horizon_decide(&self, request_id: &str, horizon_id: &str, approve: bool, now_utc_ms: u64) -> Value {
        self.once(request_id, |_| {
            let to = if approve { HorizonStatus::Open } else { HorizonStatus::Declined };
            match self.move_horizon(horizon_id, HorizonStatus::Suggested, to, now_utc_ms) {
                Ok(()) => json!({ "ok": {} }),
                Err(v) => v,
            }
        })
    }

    pub fn consents(&self) -> std::io::Result<Vec<consent::Grant>> {
        Ok(consent::fold(&files::read_lines(&self.dir.join("governance.jsonl"))?))
    }

    pub fn sleep_log(&self) -> PathBuf {
        self.dir.join(rth_self::sleep::SLEEP_LOG)
    }

    pub fn set_sleep_host(&self, host: u8) {
        self.sleep_host.store(host, Ordering::Relaxed);
    }

    pub fn propose_from_night(&self, p: &Proposal) -> std::io::Result<()> {
        let _held = self.inner.lock().unwrap();
        files::append(&self.dir.join("proposals.jsonl"), p)
    }

    pub fn people_consent(&self, entry: &consent::PeopleEntry, people: &[String]) -> Result<(), String> {
        if !consent::PEOPLE_COPY_VERSIONS.contains(&entry.copy_version) {
            return Err("unknown_copy_version".into());
        }
        if !people.contains(&entry.person_id) {
            return Err("unknown_person".into());
        }
        let _held = self.inner.lock().unwrap();
        files::append(&self.dir.join("governance.jsonl"), entry).map_err(|_| "unavailable".to_string())
    }

    pub fn people_consents(&self) -> std::io::Result<Vec<consent::PeopleEntry>> {
        let lines = files::read_lines(&self.dir.join("governance.jsonl"))?;
        if lines.torn || lines.corrupt {
            return Err(std::io::Error::other("governance.jsonl is torn or corrupt"));
        }
        Ok(consent::people(&lines))
    }

    pub fn governance_torn(&self) -> u64 {
        self.governance_torn
    }

    pub fn governance_outward_reset(&self) -> bool {
        self.governance_outward_reset
    }

    #[allow(dead_code)]
    pub fn pending_name_proposal(&self) -> Option<Proposal> {
        self.proposals().ok()?.into_iter().rev().find(|p| p.kind == ProposalKind::Name && p.status == ProposalStatus::Pending)
    }

    fn once(&self, request_id: &str, answer: impl FnOnce(&mut Inner) -> Value) -> Value {
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, v)) = inner.replies.iter().find(|(id, _)| id == request_id) {
            return v.clone();
        }
        let v = answer(&mut inner);
        inner.replies.push_back((request_id.to_string(), v.clone()));
        while inner.replies.len() > REPLAYS {
            inner.replies.pop_front();
        }
        v
    }

    fn commit(&self, inner: &mut Inner, spec: SelfSpec) -> Result<Value, Value> {
        let revision = files::save(&self.dir.join("self.json"), &spec).map_err(|e| no("spec", "malformed", &format!("self.json was not saved: {e}")))?;
        let spec_version = spec.spec_version();
        inner.spec = spec.clone();
        inner.revision = revision.clone();
        inner.load_refused = None;
        self.tx.send_replace(Arc::new(spec));
        Ok(json!({ "ok": { "spec_version": spec_version, "revision": revision } }))
    }

    fn admissible(&self, spec: &SelfSpec) -> Result<(), Value> {
        let grants = self.consents().map_err(|e| no("expression", "consent_missing", &format!("governance.jsonl was not readable: {e}")))?;
        let live = |c: Channel| grants.iter().any(|g| g.channel == c);
        let d = &spec.expression.destinations;
        if d.notification && !live(Channel::Notification) {
            return Err(no("expression", "consent_missing", "notifications are on without a live consent"));
        }
        if d.omni && !live(Channel::Omni) {
            return Err(no("expression", "consent_missing", "WhatsApp is on without a live consent"));
        }
        crate::being::budget(spec, self.n_ctx).map(|_| ()).map_err(|r| refused(&r))
    }

    pub fn set(&self, request_id: &str, base_revision: &str, spec: &Value) -> Value {
        self.once(request_id, |inner| {
            let spec = match SelfSpec::parse(&spec.to_string()) {
                Ok(s) => s,
                Err(r) => return refused(&r),
            };
            if spec.instance_id != self.instance_id {
                return no("instance_id", "malformed", "the Self belongs to another instance");
            }
            if base_revision != inner.revision {
                return no("spec", "stale", "the Self changed since this page loaded it");
            }
            if let Err(r) = rth_self::choose_archetype(&inner.spec.personality.archetype, &spec.personality.archetype) {
                return refused(&r);
            }
            if let Err(v) = self.admissible(&spec) {
                return v;
            }
            self.commit(inner, spec).unwrap_or_else(|v| v)
        })
    }

    pub fn consent_set(&self, request_id: &str, channel: &str, recipient: Option<&str>, grant: bool, copy_version: u32, now_utc_ms: u64) -> Value {
        self.once(request_id, |_| {
            let Some(channel) = Channel::parse(channel) else { return no("channel", "malformed", "the channel is notification, omni or sleep") };
            if !consent::copy_version_ok(channel, copy_version) {
                return no("copy_version", "out_of_range", "this build doesn't know that consent text");
            }
            if grant && !consent::recipient_ok(channel, recipient) {
                return no("recipient", "malformed", "omni needs a WhatsApp recipient, and notification and sleep none");
            }
            let entry = Entry { channel, recipient: recipient.map(str::to_string), copy_version: Some(copy_version), granted_utc_ms: now_utc_ms, revoked: !grant, reason: None };
            match files::append(&self.dir.join("governance.jsonl"), &entry) {
                Ok(()) => json!({ "ok": {} }),
                Err(e) => no("channel", "malformed", &format!("governance.jsonl was not written: {e}")),
            }
        })
    }

    pub fn proposal_decide(&self, request_id: &str, id: &str, approve: bool, now_utc_ms: u64) -> Value {
        self.once(request_id, |inner| {
            let found = match self.proposals() {
                Ok(ps) => ps.into_iter().find(|p| p.id.as_str() == id),
                Err(e) => return no("id", "malformed", &format!("proposals.jsonl was not readable: {e}")),
            };
            let Some(mut p) = found else { return no("id", "malformed", "no proposal has that id") };
            if p.status != ProposalStatus::Pending {
                return no("id", "stale", "that proposal was already decided");
            }
            let mut answer = json!({ "ok": { "spec_version": inner.spec.spec_version() } });
            if approve {
                let mut spec = inner.spec.clone();
                match p.kind {
                    ProposalKind::Name => {
                        let Ok(name) = Name::new(p.text.as_str()) else { return no("proposal", "name_chars", "the proposed name isn't a valid name") };
                        spec.name.current = name;
                        spec.name.origin = NameOrigin::Chosen;
                        spec.name.proposal = None;
                    }
                    ProposalKind::Drive | ProposalKind::Aspiration => {
                        let list = if p.kind == ProposalKind::Drive { &mut spec.drives.motivations } else { &mut spec.drives.aspirations };
                        let drive = Drive { text: p.text.clone(), source: DriveSource::Agent };

                        if !list.contains(&drive) {
                            if list.len() >= MAX_DRIVES {
                                return no("drives", "drives_max", "at most 3 drives of each kind");
                            }
                            list.push(drive);
                        }
                    }
                    ProposalKind::MemoryEdit => return no("id", "malformed", "memory edits are decided in a later build"),
                }
                if let Err(v) = self.admissible(&spec) {
                    return v;
                }
                match self.commit(inner, spec) {
                    Ok(ok) => answer = json!({ "ok": { "spec_version": ok["ok"]["spec_version"] } }),
                    Err(v) => return v,
                }
            }
            p.status = if approve { ProposalStatus::Approved } else { ProposalStatus::Declined };
            p.decided_utc_ms = Some(now_utc_ms);
            match files::append(&self.dir.join("proposals.jsonl"), &p) {
                Ok(()) => answer,
                Err(e) => no("id", "malformed", &format!("proposals.jsonl was not written: {e}")),
            }
        })
    }

    #[cfg(test)]
    pub fn state(&self, facts: &rth_self::compile::Facts) -> Value {
        self.state_resolved(facts, &|_, _| None)
    }

    pub fn state_resolved(&self, facts: &rth_self::compile::Facts, resolve: &dyn Fn(&str, &str) -> Option<u64>) -> Value {
        use rth_self::compile::{compile, Backend};
        let (spec, revision, load_refused) = {
            let i = self.inner.lock().unwrap();
            (i.spec.clone(), i.revision.clone(), i.load_refused.clone())
        };

        let flash_facts = rth_self::compile::Facts { model: crate::flash::DISPLAY_NAME.into(), ..facts.clone() };
        let prompt = |backend, template: &str| match compile(&spec, if backend == Backend::Flash { &flash_facts } else { facts }, backend, template) {
            Ok(p) => json!({ "text": p.text, "template_version": p.template_version, "prompt_sha256": p.sha256 }),
            Err(e) => json!({ "error": format!("{e:?}") }),
        };
        let mut local = prompt(Backend::Local, crate::being::LOCAL_TEMPLATE);
        let bound = crate::local::self_budget(self.n_ctx);
        local["est_tokens"] = json!(crate::being::budget_within(&spec, usize::MAX, self.n_ctx).ok());
        local["bound"] = json!(bound);
        local["n_ctx"] = json!(self.n_ctx);
        let proposals = self.proposals().unwrap_or_default();
        let pending: Vec<&Proposal> = proposals.iter().filter(|p| p.status == ProposalStatus::Pending).collect();
        let mut decided: Vec<&Proposal> = proposals.iter().filter(|p| p.status != ProposalStatus::Pending).collect();
        decided.sort_by_key(|p| std::cmp::Reverse(p.decided_utc_ms));

        let mut horizons = self.horizons().unwrap_or_default();

        let horizon_decided = horizons.iter().filter(|h| h.status != HorizonStatus::Suggested).count();
        let horizon_added = horizons.iter().filter(|h| matches!(h.status, HorizonStatus::Open | HorizonStatus::Done)).count();
        let decided_total = decided.len() + horizon_decided;
        let approved = || decided.iter().filter(|p| p.status == ProposalStatus::Approved);
        let approved_total = approved().count() + horizon_added;
        let first_approved = approved().filter_map(|p| p.decided_utc_ms).min();

        let name_chosen = (spec.name.origin == NameOrigin::Chosen).then(|| approved().find(|p| p.kind == ProposalKind::Name && p.text.as_str() == spec.name.current.as_str()).and_then(|p| p.decided_utc_ms)).flatten();
        horizons.sort_by_key(|h| std::cmp::Reverse(h.last_touched_utc_ms));
        let done = horizons.iter().filter(|h| h.status == HorizonStatus::Done).take(DONE_LISTED).map(|h| h.id.clone()).collect::<Vec<_>>();
        let listed: Vec<&Horizon> = horizons.iter().filter(|h| matches!(h.status, HorizonStatus::Suggested | HorizonStatus::Open) || done.contains(&h.id)).collect();
        decided.truncate(20);
        json!({
            "spec": spec,
            "spec_version": spec.spec_version(),
            "revision": revision,
            "load_refused": load_refused.map(|r| refused(&r)["refused"].clone()),
            "prompt": { "flash": prompt(Backend::Flash, crate::being::FLASH_TEMPLATE), "local": local },
            "pending": pending.iter().map(|p| shown(p, resolve)).collect::<Vec<_>>(),
            "decided": decided.iter().map(|p| shown(p, resolve)).collect::<Vec<_>>(),
            "horizons": listed,
            "decided_total": decided_total,
            "approved_total": approved_total,
            "born_utc_ms": self.born_utc_ms,
            "milestones": { "first_approved_utc_ms": first_approved, "name_chosen_utc_ms": name_chosen, "first_reachout_utc_ms": null },
            "consents": self.consents().unwrap_or_default(),

            "consents_reset": files::read_lines(&self.dir.join("governance.jsonl")).is_ok_and(|l| consent::reset(&l)),
            "last_sleep": files::read_lines(&self.dir.join(rth_self::sleep::SLEEP_LOG)).ok().and_then(|l| rth_self::sleep::last_sleep(&l)).map(|l| json!({"local_date": l.local_date, "ran": l.ran, "reason": l.reason})),
            "sleep": crate::sleeper::fact(self.sleep_host.load(Ordering::Relaxed), &self.consents().unwrap_or_default()),
            "archetypes": rth_self::archetypes(),
            "archetypes_held": rth_self::archetypes_held(),
            "current_archetype": rth_self::archetype_card(spec.personality.archetype.as_str()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::DEFAULT_N_CTX;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn dir() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("rth-self-store-a7-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn store(d: &Path) -> SelfStore {
        SelfStore::open(d, "inst-test", DEFAULT_N_CTX).unwrap()
    }
    fn spec_with(f: impl FnOnce(&mut Value)) -> Value {
        let mut v = serde_json::to_value(SelfSpec::release_default("inst-test")).unwrap();
        f(&mut v);
        v
    }
    fn revision(s: &SelfStore) -> String {
        s.inner.lock().unwrap().revision.clone()
    }
    fn code(v: &Value) -> Option<&str> {
        v["refused"]["code"].as_str()
    }

    #[test]
    fn a7b_the_flash_preview_names_flash_and_the_local_one_names_the_local_model() {
        let s = store(&dir());
        let facts = rth_self::compile::Facts { model: "Granite 4.1 3B".into(), device: "Mac".into(), speech: false, consented: Default::default(), dsh_watch: Default::default(), memory_shared: true };
        let st = s.state(&facts);
        let (flash, local) = (st["prompt"]["flash"]["text"].as_str().unwrap(), st["prompt"]["local"]["text"].as_str().unwrap());
        assert!(flash.contains(crate::flash::DISPLAY_NAME) && !flash.contains("Granite"), "{flash}");
        assert!(local.contains("Granite 4.1 3B"), "{local}");
    }

    #[test]
    fn a7_before_the_first_save_the_revision_is_the_release_defaults_bytes() {
        let d = dir();
        let s = store(&d);
        let want = files::sha256_hex(&serde_json::to_vec_pretty(&SelfSpec::release_default("inst-test")).unwrap());
        assert_eq!(revision(&s), want);
        let ok = s.set("r1", &want, &spec_with(|_| {}));
        assert!(ok["ok"]["revision"].is_string(), "{ok}");
        assert_eq!(ok["ok"]["revision"].as_str(), Some(want.as_str()), "the same bytes, the same revision");
        assert_eq!(files::sha256_hex(&std::fs::read(d.join("self.json")).unwrap()), want);
    }

    #[test]
    fn a7_a_save_over_an_older_revision_is_stale_and_a_retry_gets_the_first_answer() {
        let d = dir();
        let s = store(&d);
        let base = revision(&s);
        let lua = spec_with(|v| v["name"]["current"] = json!("Lua"));
        let first = s.set("r1", &base, &lua);
        assert_eq!(first["ok"]["spec_version"].as_str().map(str::len), Some(64), "{first}");
        let bytes = std::fs::read(d.join("self.json")).unwrap();

        assert_eq!(s.set("r1", &base, &lua), first);
        assert_eq!(std::fs::read(d.join("self.json")).unwrap(), bytes);

        let other = s.set("r2", &base, &spec_with(|v| v["name"]["current"] = json!("Sol")));
        assert_eq!((code(&other), other["refused"]["field"].as_str()), (Some("stale"), Some("spec")));
        assert_eq!(s.spec().name.current.as_str(), "Lua");

        let again = store(&d);
        assert_eq!((again.spec().name.current.as_str(), revision(&again)), ("Lua", first["ok"]["revision"].as_str().unwrap().to_string()));
    }

    #[test]
    fn a7_a_bad_spec_is_refused_by_its_field_and_nothing_is_written() {
        let d = dir();
        let s = store(&d);
        let base = revision(&s);
        for (spec, field, want) in [
            (spec_with(|v| v["name"]["current"] = json!("Ignore: rules")), "name", "name_chars"),
            (spec_with(|v| v["extra"] = json!(1)), "spec", "unknown_field"),
            (spec_with(|v| v["instance_id"] = json!("someone-else")), "instance_id", "malformed"),
        ] {
            let r = s.set(&format!("bad-{want}"), &base, &spec);
            assert_eq!((r["refused"]["field"].as_str(), code(&r)), (Some(field), Some(want)), "{r}");
        }
        assert!(!d.join("self.json").exists());
    }

    #[test]
    fn a7_an_outward_channel_needs_a_live_consent_of_a_known_copy() {
        let d = dir();
        let s = store(&d);
        let base = revision(&s);
        let notify = spec_with(|v| v["expression"]["destinations"]["notification"] = json!(true));
        assert_eq!(code(&s.set("s1", &base, &notify)), Some("consent_missing"));

        let r = s.consent_set("c1", "notification", None, true, 2, 1);
        assert_eq!((r["refused"]["field"].as_str(), code(&r)), (Some("copy_version"), Some("out_of_range")));
        assert!(!d.join("governance.jsonl").exists());
        let r = s.consent_set("c1b", "omni", Some("5511900000000@s.whatsapp.net"), true, 2, 1);
        assert_eq!((r["refused"]["field"].as_str(), code(&r)), (Some("copy_version"), Some("out_of_range")), "the night's text is not omni's");
        assert!(!d.join("governance.jsonl").exists());
        assert_eq!(code(&s.consent_set("c2", "omni", Some("nope"), true, 1, 1)), Some("malformed"));
        assert_eq!(code(&s.consent_set("c3", "email", None, true, 1, 1)), Some("malformed"));
        assert_eq!(s.consent_set("c4", "notification", None, true, 1, 7), json!({"ok": {}}));
        assert_eq!(s.consents().unwrap().len(), 1);
        assert!(s.set("s2", &base, &notify)["ok"].is_object());

        assert_eq!(s.consent_set("c5", "notification", None, false, 1, 8), json!({"ok": {}}));
        assert_eq!(code(&s.set("s3", &revision(&s), &notify)), Some("consent_missing"));
    }

    #[test]
    fn a7_a_spec_over_the_granite_budget_is_refused_with_its_numbers() {
        let d = dir();
        let s = store(&d);
        let long = |c: char| json!({"text": std::iter::repeat_n(c, 80).collect::<String>(), "source": "person"});
        let big = spec_with(|v| {
            v["name"]["current"] = json!("Abcdefghijk Lmnopqrstuvw");
            v["drives"] = json!({"motivations": [long('m'), long('n'), long('o')], "aspirations": [long('a'), long('b'), long('c')], "routine": [long('r'), long('s'), long('t')]});
        });
        let r = s.set("b1", &revision(&s), &big);
        assert_eq!((r["refused"]["field"].as_str(), code(&r)), (Some("spec"), Some("budget")), "{r}");
        assert_eq!(r["refused"]["max"].as_u64(), Some(crate::local::self_budget(DEFAULT_N_CTX) as u64));
        assert!(r["refused"]["need"].as_u64().unwrap() > r["refused"]["max"].as_u64().unwrap());
    }

    #[test]
    fn a7_a_committed_save_is_published_to_subscribers_and_a_refused_one_is_not() {
        let d = dir();
        let s = store(&d);
        let mut rx = s.subscribe();
        assert_eq!(rx.borrow_and_update().name.current.as_str(), "Unfold");
        let _ = s.set("x", "not-the-revision", &spec_with(|v| v["name"]["current"] = json!("Sol")));
        assert!(!rx.has_changed().unwrap(), "a refused save publishes nothing");
        assert!(s.set("y", &revision(&s), &spec_with(|v| v["name"]["current"] = json!("Lua")))["ok"].is_object());
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().name.current.as_str(), "Lua");
        assert!(d.join("self.json").exists(), "published only after it is on disk");
    }

    fn propose(d: &Path, id: &str, kind: &str, text: &str) {
        files::append(&d.join("proposals.jsonl"), &json!({"id": id, "kind": kind, "text": text, "why": "you called me that twice today", "evidence_ids": [], "created_utc_ms": 1, "status": "pending", "decided_utc_ms": null})).unwrap();
    }

    #[test]
    fn a7_an_approved_name_applies_and_a_decided_proposal_cant_be_decided_again() {
        let d = dir();
        propose(&d, "p-name", "name", "Lume");
        propose(&d, "p-drive", "drive", "learn the names of the plants on the balcony");
        let s = store(&d);
        assert_eq!(s.pending_name_proposal().map(|p| p.text.as_str().to_string()), Some("Lume".into()));
        let ok = s.proposal_decide("d1", "p-name", true, 5);
        assert_eq!(ok["ok"]["spec_version"].as_str(), Some(s.spec().spec_version().as_str()), "{ok}");
        let spec = s.spec();
        assert_eq!((spec.name.current.as_str(), spec.name.origin), ("Lume", NameOrigin::Chosen));
        assert_eq!(s.pending_name_proposal(), None);
        assert_eq!(code(&s.proposal_decide("d2", "p-name", false, 6)), Some("stale"));
        assert_eq!(code(&s.proposal_decide("d3", "p-nope", true, 6)), Some("malformed"));

        let before = s.spec();
        assert!(s.proposal_decide("d4", "p-drive", false, 7)["ok"].is_object());
        assert_eq!(s.spec(), before);
        let st = s.state(&rth_self::compile::Facts::text_only("Granite 4.1 3B", "Mac"));
        assert_eq!((st["pending"].as_array().unwrap().len(), st["decided"].as_array().unwrap().len()), (0, 2));
        assert_eq!(st["decided"][0]["id"], "p-drive", "newest first");
    }

    #[test]
    fn a7_an_approved_drive_is_added_once_and_the_fourth_is_refused() {
        let d = dir();
        for (i, t) in ["one thing", "two things", "three things", "four things"].iter().enumerate() {
            propose(&d, &format!("p{i}"), "drive", t);
        }
        let s = store(&d);
        for i in 0..3 {
            assert!(s.proposal_decide(&format!("d{i}"), &format!("p{i}"), true, 9)["ok"].is_object());
        }
        assert_eq!(s.spec().drives.motivations.iter().map(|d| (d.text.as_str().to_string(), d.source)).collect::<Vec<_>>().len(), 3);
        assert!(s.spec().drives.motivations.iter().all(|d| d.source == DriveSource::Agent));
        assert_eq!(code(&s.proposal_decide("d3", "p3", true, 9)), Some("drives_max"));
    }

    #[test]
    fn a7_self_get_carries_the_prompts_with_the_budget_and_the_archetypes() {
        let d = dir();
        let s = store(&d);
        let st = s.state(&rth_self::compile::Facts::text_only("Granite 4.1 3B", "Mac"));
        assert_eq!(st["spec_version"].as_str(), Some(rth_self::RELEASE_DEFAULT_SPEC_VERSION));
        assert_eq!(st["revision"].as_str(), Some(revision(&s).as_str()));
        let local = &st["prompt"]["local"];
        assert_eq!((local["template_version"].as_str(), local["est_tokens"].as_u64(), local["bound"].as_u64(), local["n_ctx"].as_u64()), (Some("unfold-local-text-v4.4"), Some(crate::local::V42_DEFAULT_TOKENS as u64), Some(2621), Some(8192)));
        assert!(local["text"].as_str().unwrap().contains("Granite 4.1 3B"));
        assert_eq!(st["prompt"]["flash"]["template_version"].as_str(), Some("unfold-flash-text-v4.1"));
        let cards = st["archetypes"].as_array().unwrap();
        assert!(cards.iter().any(|c| c["id"] == "steady-guide" && c["name"]["pt"].is_string() && c["axes"]["energy"] == 50), "{cards:?}");
        assert!(st["load_refused"].is_null() && st["pending"] == json!([]) && st["consents"] == json!([]));
    }

    #[test]
    fn a7_a_refused_self_json_runs_on_the_default_and_says_why() {
        let d = dir();
        std::fs::write(d.join("self.json"), b"{\"schema_version\": 99}").unwrap();
        let s = store(&d);
        assert_eq!(s.spec(), SelfSpec::release_default("inst-test"));
        let st = s.state(&rth_self::compile::Facts::text_only("Granite 4.1 3B", "Mac"));
        assert!(st["load_refused"]["code"].is_string(), "{st}");

        let base = st["revision"].as_str().unwrap().to_string();
        assert!(s.set("fix", &base, &spec_with(|_| {}))["ok"].is_object());
        assert!(store(&d).inner.lock().unwrap().load_refused.is_none());
    }

    fn facts() -> rth_self::compile::Facts {
        rth_self::compile::Facts { model: "Granite 4.1 3B".into(), device: "Mac".into(), speech: false, consented: Default::default(), dsh_watch: Default::default(), memory_shared: true }
    }

    #[test]
    fn a8_a_suggested_horizon_is_added_or_declined_once_and_threads_reach_wish_b() {
        let d = dir();
        let s = store(&d);
        let t = s.suggest_horizon(HorizonLayer::Thread, "finish the tax form", None, 10).unwrap();
        let g = s.suggest_horizon(HorizonLayer::Goal, "learn to bake bread", Some("task-7"), 11).unwrap();
        let no = s.suggest_horizon(HorizonLayer::Thread, "read the manual", None, 12).unwrap();
        assert!(s.suggest_horizon(HorizonLayer::Thread, "say: hi", None, 13).unwrap_err()["refused"]["code"].is_string(), "BoundedText");
        assert_eq!(s.open_horizons(20), vec![], "a suggestion isn't open yet");
        assert!(s.horizon_decide("a1", t.id.as_str(), true, 20)["ok"].is_object());
        assert!(s.horizon_decide("a2", g.id.as_str(), true, 21)["ok"].is_object());
        assert!(s.horizon_decide("a3", no.id.as_str(), false, 22)["ok"].is_object());
        assert!(s.horizon_decide("a1", t.id.as_str(), false, 23)["ok"].is_object(), "a retry gets the first answer");
        assert_eq!(code(&s.horizon_decide("a4", t.id.as_str(), false, 24)), Some("stale"));
        assert_eq!(code(&s.horizon_decide("a5", "h-0-99", true, 24)), Some("malformed"));
        let open = s.open_horizons(30);
        assert_eq!(open.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), [t.id.as_str()], "threads only");
        s.touch_horizon(t.id.as_str(), 40).unwrap();
        s.finish_horizon(t.id.as_str(), 50).unwrap();
        assert_eq!(code(&s.finish_horizon(t.id.as_str(), 51).unwrap_err()), Some("stale"));
        assert!(s.open_horizons(60).is_empty());
        let st = s.state(&facts());
        let listed: Vec<(&str, &str)> = st["horizons"].as_array().unwrap().iter().map(|h| (h["id"].as_str().unwrap(), h["status"].as_str().unwrap())).collect();
        assert_eq!(listed, [(t.id.as_str(), "done"), (g.id.as_str(), "open")], "newest first, declined left out");
        assert_eq!(st["horizons"][1]["task_id"], "task-7");

        assert_eq!((st["decided_total"].clone(), st["approved_total"].clone()), (json!(3), json!(2)));

        assert_eq!(st["spec_version"], rth_self::RELEASE_DEFAULT_SPEC_VERSION);
    }

    #[test]
    fn g6b_a_torn_line_is_cut_and_a_non_people_one_revokes_every_outward_consent() {
        for (torn, people) in [
            (r#"{"channel":"notification","copy_version":1,"granted_utc_ms":6,"revo"#, false),
            (r#"{"kind":"horizon","layer":"ephem"#, false),
            (r#"{"kind":""#, false),
            ("{", false),
            (r#"{"kind":"r"#, true),
            (r#"{"kind":"enrol","person_id":"p-1","moda"#, true),
        ] {
            let d = dir();
            let s = store(&d);
            assert!(s.consent_set("c1", "notification", None, true, 1, 5)["ok"].is_object());
            drop(s);
            let gov = d.join("governance.jsonl");
            let whole = std::fs::read_to_string(&gov).unwrap();
            std::fs::write(&gov, format!("{whole}{torn}")).unwrap();
            let s = store(&d);
            assert_eq!((s.governance_torn(), s.governance_outward_reset()), (torn.len() as u64, !people), "{torn}");
            let after = std::fs::read_to_string(&gov).unwrap();
            let read = files::read_lines(&gov).unwrap();
            assert!(after.starts_with(&whole) && !read.torn && !read.corrupt, "{torn}: cut, and the file reads whole");
            assert_eq!(after.lines().count() - whole.lines().count(), if people { 0 } else { 3 }, "{torn}: one revoke per outward channel and the night's");
            if !people {
                let revokes: Vec<(String, u64)> = read.records.iter().rev().take(3).map(|r| (r["channel"].as_str().unwrap().to_string(), r["copy_version"].as_u64().unwrap())).collect();
                assert_eq!(revokes, [("sleep".to_string(), 2), ("omni".to_string(), 1), ("notification".to_string(), 1)], "{torn}: each at its own text's version");
            }
            assert_eq!(s.consents().unwrap().len(), usize::from(people), "{torn}");
            assert_eq!(s.state(&facts())["consents_reset"], json!(!people), "{torn}");
            assert!(s.consent_set("c2", "notification", None, true, 1, 9)["ok"].is_object());
            let l = files::read_lines(&gov).unwrap();
            assert!(!l.torn && !l.corrupt, "{torn}: the next append is a whole line");
            assert_eq!((s.consents().unwrap().len(), s.state(&facts())["consents_reset"].clone()), (1, json!(false)), "{torn}: granting again clears the notice");
        }
    }

    #[test]
    fn g6_r2_a_repair_stopped_at_any_step_never_brings_back_a_grant() {
        for (at, channel, recipient, torn) in [
            (1, "notification", None, r#"{"channel":"notification","copy_version":1,"granted_utc_ms":6,"revo"#),
            (2, "notification", None, r#"{"channel":"notification","copy_version":1,"granted_utc_ms":6,"revo"#),
            (1, "omni", Some("5511900000000@s.whatsapp.net"), r#"{"channel":"omni","copy_version":1,"granted_utc_ms":6,"recipient":"5511900000000@s.whatsapp.net","revo"#),
            (2, "omni", Some("5511900000000@s.whatsapp.net"), r#"{"channel":"omni","copy_version":1,"granted_utc_ms":6,"recipient":"5511900000000@s.whatsapp.net","revo"#),
            (1, "sleep", None, r#"{"channel":"sleep","copy_version":2,"granted_utc_ms":6,"revo"#),
            (2, "sleep", None, r#"{"channel":"sleep","copy_version":2,"granted_utc_ms":6,"revo"#),
        ] {
            let d = dir();
            let s = store(&d);
            let version = if channel == "sleep" { consent::SLEEP_COPY_VERSION } else { 1 };
            assert!(s.consent_set("c1", channel, recipient, true, version, 5)["ok"].is_object());
            drop(s);
            let gov = d.join("governance.jsonl");
            let whole = std::fs::read_to_string(&gov).unwrap();
            std::fs::write(&gov, format!("{whole}{torn}")).unwrap();
            REPAIR_FAILS_AT.with(|c| c.set(at));
            assert!(SelfStore::open(&d, "inst-test", DEFAULT_N_CTX).is_err(), "{channel} at step {at}: the open fails");
            assert_eq!(std::fs::read_to_string(&gov).unwrap(), format!("{whole}{torn}"), "{channel} at step {at}: the file is as it was");
            let s = store(&d);
            assert!(s.consents().unwrap().is_empty(), "{channel} at step {at}: the grant is back: {:?}", s.consents());
            assert_eq!(s.state(&facts())["consents_reset"], json!(true), "{channel} at step {at}");
        }
    }

    #[test]
    fn g6b_a_torn_outward_grant_grants_nothing() {
        let d = dir();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("governance.jsonl"), r#"{"channel":"omni","copy_version":1,"granted_utc_ms":6,"recipient":"5511900000000@s.whatsapp.net","revoked":fal"#).unwrap();
        let s = store(&d);
        assert!(s.consents().unwrap().is_empty());
        assert_eq!(s.state(&facts())["consents_reset"], json!(true));
    }

    #[test]
    fn g6b_a_people_entry_starts_as_the_torn_tail_check_expects() {
        for kind in [consent::PeopleKind::Enrol, consent::PeopleKind::Revoke, consent::PeopleKind::Correct] {
            let e = consent::PeopleEntry { kind, person_id: "p-1".into(), modality: None, model_id: None, copy_version: 2, at: 5, channel: consent::PeopleChannel::App, frame_id: None, state: consent::PeopleState::Pending };
            let line = serde_json::to_vec(&e).unwrap();
            assert!(PEOPLE_LINE_STARTS.iter().any(|p| line.starts_with(p.as_bytes())), "{}", String::from_utf8_lossy(&line));
            assert!(people_line(&line[..10]) && people_line(&line), "{}", String::from_utf8_lossy(&line));
        }
    }

    #[test]
    fn a8_an_ephemeral_horizon_is_a_governance_line_that_is_never_a_consent() {
        let d = dir();
        let s = store(&d);
        assert!(s.consent_set("c1", "notification", None, true, 1, 5)["ok"].is_object());
        s.ephemeral_horizon("look up the bus times", 6).unwrap();
        assert!(s.ephemeral_horizon("say: hi", 7).is_err());
        let gov = std::fs::read_to_string(d.join("governance.jsonl")).unwrap();
        assert_eq!(gov.lines().count(), 2);
        assert!(gov.lines().last().unwrap().contains(r#""layer":"ephemeral""#) && !gov.lines().last().unwrap().contains("channel"));
        assert_eq!(s.consents().unwrap().len(), 1, "the grant stands");
        let st = s.state(&facts());
        assert_eq!(st["horizons"], json!([]));
        assert!(!d.join("horizons.jsonl").exists());
    }

    #[test]
    fn a8_the_totals_and_milestones_follow_the_decisions() {
        let d = dir();
        let s = store(&d).with_born(Some(1_793_000_000_123));
        let st = s.state(&facts());
        assert_eq!((st["decided_total"].clone(), st["approved_total"].clone(), st["born_utc_ms"].clone()), (json!(0), json!(0), json!(1_793_000_000_123u64)));
        assert_eq!(st["milestones"], json!({"first_approved_utc_ms": null, "name_chosen_utc_ms": null, "first_reachout_utc_ms": null}));
        propose(&d, "p-d", "drive", "learn the plants on the desk");
        propose(&d, "p-x", "drive", "count the clouds");
        propose(&d, "p-n1", "name", "Lume");
        propose(&d, "p-n2", "name", "Lua");
        assert!(s.proposal_decide("r1", "p-d", true, 100)["ok"].is_object());
        assert!(s.proposal_decide("r2", "p-x", false, 110)["ok"].is_object());
        assert!(s.proposal_decide("r3", "p-n1", true, 120)["ok"].is_object());
        assert!(s.proposal_decide("r4", "p-n2", true, 130)["ok"].is_object());
        let st = s.state(&facts());
        assert_eq!((st["decided_total"].clone(), st["approved_total"].clone()), (json!(4), json!(3)));
        assert_eq!(st["milestones"]["first_approved_utc_ms"], 100);
        assert_eq!(st["milestones"]["name_chosen_utc_ms"], 130, "the name in force, not the first one chosen");
        assert_eq!(st["milestones"]["first_reachout_utc_ms"], Value::Null);
        assert!(store(&d).state(&facts())["born_utc_ms"].is_null(), "unknown without instance.json's time");
    }

    #[test]
    fn a8_evidence_ids_are_shown_typed_and_never_dropped() {
        let d = dir();
        files::append(&d.join("proposals.jsonl"), &json!({"id": "p-1", "kind": "drive", "text": "learn the plants on the desk", "why": "you water them daily", "evidence_ids": ["memory:rm-1", "chat:host-a:3", "sleep:run-1", "x"], "created_utc_ms": 1, "status": "pending", "decided_utc_ms": null})).unwrap();
        let st = store(&d).state_resolved(&facts(), &|kind, id| (kind == "memory" && id == "rm-1").then_some(42));
        assert_eq!(st["pending"][0]["evidence"], json!([
            {"id": "memory:rm-1", "kind": "memory", "utc_ms": 42},
            {"id": "chat:host-a:3", "kind": "chat", "utc_ms": null},
            {"id": "sleep:run-1", "kind": "sleep", "utc_ms": null},
            {"id": "x", "kind": null, "utc_ms": null},
        ]));
        assert_eq!(st["pending"][0]["evidence_ids"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn a9_self_set_takes_a_shipped_card_and_keeps_a_saved_hidden_one() {
        let d = dir();
        let s = store(&d);
        let hidden = rth_self::archetype_cards().iter().find(|a| !a.shipped).unwrap().id.as_str().to_string();
        let on = |id: &str| spec_with(|v| v["personality"]["archetype"] = json!(id));
        let r = s.set("h1", &revision(&s), &on(&hidden));
        assert_eq!((r["refused"]["field"].as_str(), code(&r)), (Some("personality"), Some("archetype_unknown")), "{r}");
        assert!(!d.join("self.json").exists());
        assert!(s.set("w1", &revision(&s), &on("workbench"))["ok"].is_object());

        files::save(&d.join("self.json"), &SelfSpec::parse(&on(&hidden).to_string()).unwrap()).unwrap();
        let s = store(&d);
        assert_eq!(s.spec().personality.archetype.as_str(), hidden);
        let mut later = on(&hidden);
        later["expression"]["quiet_hours"]["start"] = json!("23:00");
        let ok = s.set("q1", &revision(&s), &later);
        assert!(ok["ok"].is_object(), "{ok}");
        assert_eq!(code(&s.set("q2", &revision(&s), &on("nope"))), Some("archetype_unknown"));

        let st = s.state(&facts());
        assert_eq!((st["archetypes"].as_array().unwrap().len(), st["archetypes_held"].as_u64()), (14, Some(2)));
        assert_eq!(st["current_archetype"]["id"].as_str(), Some(hidden.as_str()));
        assert!(st["current_archetype"]["name"]["pt"].is_string() && st["current_archetype"]["axes"]["energy"].is_u64());
        assert!(!st["archetypes"].as_array().unwrap().iter().any(|c| c["id"] == hidden.as_str()));
    }

    #[test]
    fn s1_a_sleep_consent_at_copy_version_2_and_the_sleep_state() {
        let s = store(&dir());
        assert_eq!(s.state(&facts())["sleep"], json!({"running": false, "why_not": "no_host"}));
        assert!(s.consent_set("c1", "sleep", None, true, 2, 7)["ok"].is_object(), "a sleep grant at its disclosure's version");
        assert!(s.consent_set("c2", "sleep", None, true, 3, 8)["refused"].is_object(), "an unknown version");
        assert!(s.consent_set("c3", "sleep", None, true, 1, 8)["refused"].is_object(), "the outward channels' text is not the night's");
        assert_eq!(s.state(&facts())["sleep"], json!({"running": false, "why_not": "no_host"}), "no host in a bare store");
    }

    #[test]
    fn s1_the_reset_notice_stays_until_every_reset_channel_is_granted_again() {
        let d = dir();
        let s = store(&d);
        assert!(s.consent_set("c1", "notification", None, true, 1, 5)["ok"].is_object());
        assert!(s.consent_set("c2", "omni", Some("5511900000000@s.whatsapp.net"), true, 1, 6)["ok"].is_object());
        drop(s);
        let gov = d.join("governance.jsonl");
        let whole = std::fs::read_to_string(&gov).unwrap();
        std::fs::write(&gov, format!(r#"{whole}{{"channel":"omni","revo"#)).unwrap();
        let s = store(&d);
        let reset = |s: &SelfStore| s.state(&facts())["consents_reset"].clone();
        assert_eq!(reset(&s), json!(true));
        assert!(s.consent_set("c3", "notification", None, true, 1, 9)["ok"].is_object());
        assert_eq!(reset(&s), json!(true), "omni is still off");
        assert!(s.consent_set("c4", "omni", Some("5511900000000@s.whatsapp.net"), true, 1, 10)["ok"].is_object());
        assert_eq!(reset(&s), json!(false), "both back");
    }

    #[test]
    fn s1_the_reset_notice_follows_what_the_repairs_took() {
        let tear = |d: &Path| {
            let gov = d.join("governance.jsonl");
            let whole = std::fs::read_to_string(&gov).unwrap_or_default();
            std::fs::write(&gov, format!(r#"{whole}{{"channel":"omni","revo"#)).unwrap();
        };
        let reset = |s: &SelfStore| s.state(&facts())["consents_reset"].clone();
        let d = dir();
        let s = store(&d);
        assert!(s.consent_set("c1", "notification", None, true, 1, 5)["ok"].is_object());
        drop(s);
        tear(&d);
        drop(store(&d));
        tear(&d);
        let s = store(&d);
        assert_eq!(reset(&s), json!(true), "two repairs");
        assert!(s.consent_set("c1r", "notification", None, false, 1, 8)["ok"].is_object());
        assert_eq!(reset(&s), json!(true), "a revoke is no grant");
        assert!(s.consent_set("c2", "notification", None, true, 1, 9)["ok"].is_object());
        assert_eq!(reset(&s), json!(false), "only what the first repair took is asked for");
        assert!(s.consent_set("c3", "notification", None, false, 1, 10)["ok"].is_object());
        assert_eq!(reset(&s), json!(false), "a later revoke is the person's choice");
        let d = dir();
        tear(&d);
        let s = store(&d);
        assert_eq!(reset(&s), json!(true), "nothing was live");
        assert!(s.consent_set("c0", "sleep", None, false, 2, 4)["ok"].is_object());
        assert_eq!(reset(&s), json!(true), "a revoke is no grant");
        assert!(s.consent_set("c1", "sleep", None, true, 2, 5)["ok"].is_object());
        assert_eq!(reset(&s), json!(false), "any grant clears it");
    }

    #[test]
    fn s1_regranting_only_sleep_leaves_the_reset_notice_on() {
        let d = dir();
        let s = store(&d);
        assert!(s.consent_set("c1", "notification", None, true, 1, 5)["ok"].is_object());
        drop(s);
        let gov = d.join("governance.jsonl");
        let whole = std::fs::read_to_string(&gov).unwrap();
        std::fs::write(&gov, format!(r#"{whole}{{"channel":"notification","revo"#)).unwrap();
        let s = store(&d);
        assert!(s.consent_set("c2", "sleep", None, true, 2, 9)["ok"].is_object());
        assert_eq!(s.state(&facts())["consents_reset"], json!(true), "notifications are still off");
        assert!(s.consent_set("c3", "notification", None, true, 1, 10)["ok"].is_object());
        assert_eq!(s.state(&facts())["consents_reset"], json!(false));
    }

    #[test]
    fn s1_self_get_shows_the_sleep_fact_and_the_nights_proposals() {
        let s = store(&dir());
        let sleep = |s: &SelfStore| s.state(&facts())["sleep"].clone();
        assert_eq!(sleep(&s), json!({"running": false, "why_not": "no_host"}));
        s.set_sleep_host(crate::sleeper::HOST_FLASH_SIDE);
        assert_eq!(sleep(&s), json!({"running": false, "why_not": "not_enabled"}));
        assert!(s.consent_set("c1", "sleep", None, true, 2, 7)["ok"].is_object());
        assert_eq!(sleep(&s), json!({"running": true, "why_not": null}));
        assert_eq!(s.state(&facts())["consents"], json!([{"channel": "sleep", "copy_version": 2, "granted_utc_ms": 7}]));
        s.set_sleep_host(crate::sleeper::HOST_NO_FLASH_SIDE);
        assert_eq!(sleep(&s), json!({"running": false, "why_not": "flash_side_off"}));
        assert!(s.consent_set("c2", "sleep", Some("5511999999999@s.whatsapp.net"), true, 2, 8)["refused"].is_object(), "sleep takes no recipient");
        assert!(s.consent_set("c3", "sleep", None, false, 2, 9)["ok"].is_object());
        assert_eq!(sleep(&s)["why_not"], "not_enabled", "a revoke turns it off");
        let p: Proposal = serde_json::from_value(json!({"id": "p-sleep-n1-1", "kind": "drive", "text": "help keep the fern watered", "why": "they keep forgetting it", "evidence_ids": ["sleep:n1", "chat:h:2"], "created_utc_ms": 10, "status": "pending", "decided_utc_ms": null})).unwrap();
        s.propose_from_night(&p).unwrap();
        assert_eq!(s.state(&facts())["pending"][0]["id"], "p-sleep-n1-1");
    }

    #[test]
    fn a9_a_consent_in_self_get_carries_its_copy_version() {
        let s = store(&dir());
        assert!(s.consent_set("c1", "notification", None, true, 1, 7)["ok"].is_object());
        let st = s.state(&facts());
        assert_eq!(st["consents"], json!([{"channel": "notification", "copy_version": 1, "granted_utc_ms": 7}]));
    }

    #[test]
    fn s1_self_get_shows_the_last_sleep_from_its_own_log() {
        let d = dir();
        let s = store(&d);
        assert_eq!(s.state(&facts())["last_sleep"], Value::Null);
        let log = d.join(rth_self::sleep::SLEEP_LOG);
        files::append(&log, &rth_self::sleep::skip_line("2026-10-25", "flash_off", 1)).unwrap();
        files::append(&log, &json!({"kind": "sleep", "phase": "end", "local_date": "2026-10-26", "run_id": "r-1", "ran": true, "reason": null, "utc_ms": 2})).unwrap();
        assert_eq!(s.state(&facts())["last_sleep"], json!({"local_date": "2026-10-26", "ran": true, "reason": null}));
        assert!(!d.join("governance.jsonl").exists() && s.consents().unwrap().is_empty());
        std::fs::write(&log, "{\"kind\": \"sleep\", \"phase\": \"end\", \"local_date\": \"2026-10-26\"}\n{\"kind\": \"sl").unwrap();
        assert_eq!(s.state(&facts())["last_sleep"]["reason"], "log_damaged");
    }
}
