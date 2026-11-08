

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub const SCHEMA: &str = "rth-a2-spend/v1";
pub const TOTAL_CAP_PUSD: u64 = 5_000_000_000_000;
const LAYER: &str = "realtime";

#[derive(Debug, Clone)]
pub struct Expect {
    pub model: String,
    pub text_only: bool,
    pub vad: String,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prices {
    pub text_in: u64,
    pub text_cached: u64,
    pub text_out: u64,
    pub audio_in: u64,
    pub audio_cached: u64,
    pub audio_out: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub n: u64,
    pub attempt_id: String,
    pub bound_pusd: u64,
}

#[derive(Debug)]
pub struct Spend {

    ident: Value,
    dir: PathBuf,
    stop: PathBuf,
    cap: u64,
    max_attempts: u64,
    bound: u64,
    prices: Prices,

    failed_here: Option<String>,

    #[cfg(test)]
    pub stop_after_claim: bool,
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

pub fn write_once(path: &Path, v: &Value) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(format!("{v}\n").as_bytes())?;
        f.sync_all()?;
    }
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    linked?;
    fsync_dir(path.parent().unwrap_or(Path::new(".")))
}

fn num(v: &Value, p: &str) -> Option<u64> {
    v.pointer(p).and_then(Value::as_u64)
}

pub fn realtime_cost(u: &Value, p: &Prices) -> Result<u64, String> {
    if !crate::openai_realtime::usage_complete(u) {
        return Err("incomplete usage".into());
    }
    let input = num(u, "/input_tokens").unwrap();
    let text_in = num(u, "/input_token_details/text_tokens").unwrap();
    let audio_in = num(u, "/input_token_details/audio_tokens").unwrap();
    let text_out = num(u, "/output_token_details/text_tokens").unwrap();
    let audio_out = num(u, "/output_token_details/audio_tokens").unwrap();
    if audio_in > 0 || audio_out > 0 {
        return Err(format!("audio tokens in a text-only acceptance (in {audio_in}, out {audio_out})"));
    }
    if input > text_in + audio_in {
        return Err(format!("unpriced input tokens ({input} > text {text_in} + audio {audio_in})"));
    }
    let output = num(u, "/output_tokens").unwrap();
    if output > text_out + audio_out {
        return Err(format!("unpriced output tokens ({output} > text {text_out} + audio {audio_out})"));
    }
    let cached_text = num(u, "/input_token_details/cached_tokens_details/text_tokens").unwrap_or(0).min(text_in);
    let cost = (text_in - cached_text).checked_mul(p.text_in).and_then(|a| a.checked_add(cached_text.checked_mul(p.text_cached)?)).and_then(|a| a.checked_add(text_out.checked_mul(p.text_out)?));
    cost.ok_or_else(|| "cost overflows".into())
}

struct Read {
    attempts: Vec<Value>,
    settles: BTreeMap<u64, Value>,
    committed: u64,
}

impl Spend {

    pub fn open(control: &Path, sha256: &str, expect: &Expect) -> Result<Spend, String> {
        let bytes = fs::read(control).map_err(|e| format!("spend: control record unreadable ({e})"))?;
        if sha256_hex(&bytes) != sha256 {
            return Err("spend: control record is not the pinned one".into());
        }
        let rec: Value = serde_json::from_slice(&bytes).map_err(|_| "spend: control record does not parse".to_string())?;
        if rec["schema"] != SCHEMA {
            return Err("spend: unknown control schema".into());
        }
        let l = rec.pointer(&format!("/layers/{LAYER}")).ok_or("spend: no realtime layer in the control record")?;
        let mut caps: u64 = 0;
        for x in rec["layers"].as_object().ok_or("spend: no layers")?.values() {
            caps = caps.checked_add(x["cap_pusd"].as_u64().ok_or("spend: a cap is not an integer")?).ok_or("spend: caps overflow")?;
        }
        if caps > TOTAL_CAP_PUSD {
            return Err(format!("spend: caps {caps} exceed {TOTAL_CAP_PUSD}"));
        }
        if l["model"] != expect.model.as_str() {
            return Err(format!("spend: model {} is not the record's {}", expect.model, l["model"]));
        }
        let o = &l["options"];
        let mine = json!({ "text_only": expect.text_only, "vad": expect.vad, "max_output_tokens": expect.max_output_tokens });
        for k in ["text_only", "vad", "max_output_tokens"] {
            if o[k] != mine[k] {
                return Err(format!("spend: option {k}={} is not the record's {}", mine[k], o[k]));
            }
        }
        let pr = &l["prices"];
        let price = |k: &str| pr[k].as_u64().ok_or(format!("spend: price {k} missing"));
        let prices = Prices { text_in: price("text_in")?, text_cached: price("text_cached")?, text_out: price("text_out")?, audio_in: price("audio_in")?, audio_cached: price("audio_cached")?, audio_out: price("audio_out")? };
        let bound = l["attempt_bound_pusd"].as_u64().ok_or("spend: no attempt bound")?;
        let window = l["context_window"].as_u64().ok_or("spend: no context window")?;
        let mine_bound = window.saturating_mul(prices.text_in).saturating_add(u64::from(expect.max_output_tokens.unwrap_or(u32::MAX)).saturating_mul(prices.text_out));
        if mine_bound > bound {
            return Err(format!("spend: the bound {mine_bound} exceeds the record's {bound}"));
        }
        let root = control.parent().unwrap_or(Path::new("."));
        let dir = root.join(l["dir"].as_str().ok_or("spend: no layer dir")?);
        let open: Value = fs::read(dir.join("open.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or("spend: the realtime ledger is missing or torn: never recreated")?;
        if open["accept_id"] != rec["accept_id"] || open["layer"] != LAYER {
            return Err("spend: the ledger belongs to another acceptance".into());
        }
        Ok(Spend {
            ident: json!({ "accept_id": rec["accept_id"], "layer": LAYER, "control_sha256": sha256 }),
            dir,
            stop: root.join(rec["stop"].as_str().ok_or("spend: no stop marker name")?),
            cap: l["cap_pusd"].as_u64().unwrap(),
            max_attempts: l["max_attempts"].as_u64().ok_or("spend: no allowance")?,
            bound,
            prices,
            failed_here: None,
            #[cfg(test)]
            stop_after_claim: false,
        })
    }

    #[cfg(test)]
    pub fn set_stop_path(&mut self, p: PathBuf) {
        self.stop = p;
    }

    pub fn stopped(&self) -> Option<String> {
        if let Some(f) = &self.failed_here {
            return Some(f.clone());
        }
        match fs::metadata(&self.stop) {
            Ok(_) => Some("spend: stopped".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(format!("spend: stop marker unreadable ({e})")),
        }
    }

    pub fn fail(&mut self, reason: &str) -> String {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        if let Err(e) = write_once(&self.stop, &json!({ "layer": LAYER, "reason": reason, "at_ms": at }))
            && e.kind() != std::io::ErrorKind::AlreadyExists
        {
            self.failed_here = Some(format!("spend: stop marker not written ({e}): {reason}"));
        }
        reason.to_string()
    }

    fn mine(&self, v: &Value) -> bool {
        ["accept_id", "layer", "control_sha256"].iter().all(|k| v.get(*k).is_some() && v[*k] == self.ident[*k])
    }

    fn read(&self) -> Result<Read, String> {
        let mut attempts: BTreeMap<u64, Value> = BTreeMap::new();
        let mut settles = BTreeMap::new();
        for e in fs::read_dir(&self.dir).map_err(|e| format!("spend: ledger unreadable ({e})"))? {
            let name = e.map_err(|e| format!("spend: ledger unreadable ({e})"))?.file_name().to_string_lossy().to_string();
            if name == "open.json" || name.contains(".tmp-") {
                continue;
            }
            let v: Value = fs::read(self.dir.join(&name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).ok_or(format!("spend: {name} does not parse"))?;
            let number = |s: &str| -> Option<u64> {
                let d = s.strip_prefix("attempt-")?;
                if d.len() == 3 && d.bytes().all(|b| b.is_ascii_digit()) { d.parse().ok() } else { None }
            };
            if let Some(n) = name.strip_suffix(".settle.json").and_then(number) {
                settles.insert(n, v);
            } else if let Some(n) = name.strip_suffix(".json").and_then(number) {
                attempts.insert(n, v);
            } else if name.starts_with("anomaly-") && name.ends_with(".json") {
                return Err(format!("spend: a fatal record is present ({name})"));
            } else {
                return Err(format!("spend: foreign file {name}"));
            }
        }
        let attempts: Vec<Value> = attempts.into_iter().enumerate().map(|(i, (n, v))| if i as u64 == n && v["n"] == n { Ok(v) } else { Err(format!("spend: attempt numbering broken at {i}")) }).collect::<Result<_, _>>()?;
        let mut ids = std::collections::BTreeSet::new();
        for a in &attempts {
            let n = a["n"].as_u64().unwrap_or(u64::MAX);
            if !self.mine(a) {
                return Err(format!("spend: attempt {n} belongs to another acceptance or layer"));
            }
            match a["attempt_id"].as_str() {
                Some(id) if !id.is_empty() && ids.insert(id.to_string()) => {}
                _ => return Err(format!("spend: attempt {n} has no unique identity")),
            }
            if a["bound_pusd"].as_u64() != Some(self.bound) {
                return Err(format!("spend: attempt {n} does not carry the record's bound"));
            }
        }
        for (k, st) in &settles {
            let Some(a) = attempts.get(*k as usize) else { return Err(format!("spend: settlement without attempt {k}")) };
            if !self.mine(st) || st["n"] != *k || st["attempt_id"] != a["attempt_id"] || !st["provider_response_id"].is_string() {
                return Err(format!("spend: settlement {k} does not match its reservation"));
            }
            match realtime_cost(&st["usage"], &self.prices) {
                Ok(c) if Some(c) == st["cost_pusd"].as_u64() && c <= self.bound => {}
                _ => return Err(format!("spend: settlement {k} is not its usage at the pinned prices")),
            }
        }
        let mut committed: u64 = 0;
        for a in &attempts {
            let n = a["n"].as_u64().unwrap();
            let c = match settles.get(&n) {
                Some(s) => s["cost_pusd"].as_u64().ok_or(format!("spend: settlement {n} has no cost"))?,
                None => a["bound_pusd"].as_u64().ok_or(format!("spend: attempt {n} has no bound"))?,
            };
            committed = committed.saturating_add(c);
        }
        Ok(Read { attempts, settles, committed })
    }

    pub fn admit(&mut self, attempt_id: &str, fields: Value) -> Result<Attempt, String> {
        for _ in 0..64 {
            if let Some(why) = self.stopped() {
                return Err(why);
            }
            let r = match self.read() {
                Ok(r) => r,
                Err(torn) => return Err(self.fail(&torn)),
            };
            let n = r.attempts.len() as u64;
            if n >= self.max_attempts {
                return Err(format!("spend: the realtime allowance of {} is used", self.max_attempts));
            }
            if r.committed.saturating_add(self.bound) > self.cap {
                return Err("spend: the realtime cap would be exceeded".into());
            }

            let mut v = fields.as_object().cloned().map(Value::Object).unwrap_or_else(|| json!({}));
            for (k, x) in [("n", json!(n)), ("attempt_id", json!(attempt_id)), ("bound_pusd", json!(self.bound))].into_iter().chain(self.ident.as_object().unwrap().iter().map(|(k, x)| (k.as_str(), x.clone()))) {
                v[k] = x;
            }
            match write_once(&self.dir.join(format!("attempt-{n:03}.json")), &v) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(self.fail(&format!("spend: attempt {n} not claimed ({e})"))),
            }
            #[cfg(test)]
            if self.stop_after_claim {
                let _ = fs::write(&self.stop, "{\"layer\":\"worker\"}");
            }

            if let Some(why) = self.stopped() {
                return Err(format!("{why} (attempt {n} stays reserved, not dispatched)"));
            }
            return Ok(Attempt { n, attempt_id: attempt_id.into(), bound_pusd: self.bound });
        }
        Err(self.fail("spend: no attempt could be claimed"))
    }

    pub fn settle(&mut self, a: &Attempt, provider_response_id: &str, usage: Option<&Value>) -> Result<u64, String> {

        let persisted: Option<Value> = fs::read(self.dir.join(format!("attempt-{:03}.json", a.n))).ok().and_then(|b| serde_json::from_slice(&b).ok());
        if !persisted.as_ref().is_some_and(|p| self.mine(p) && p["n"] == a.n && p["attempt_id"] == a.attempt_id.as_str() && p["bound_pusd"].as_u64() == Some(a.bound_pusd)) {
            return Err(self.fail(&format!("spend: attempt {} is not the persisted reservation", a.n)));
        }
        let cost = match usage.map(|u| realtime_cost(u, &self.prices)) {
            None => return Err(self.fail(&format!("spend: no usage for attempt {}", a.n))),
            Some(Err(why)) => return Err(self.fail(&format!("spend: attempt {}: {why}", a.n))),
            Some(Ok(c)) => c,
        };
        if cost > a.bound_pusd {
            return Err(self.fail(&format!("spend: attempt {} cost {cost} above its bound", a.n)));
        }
        let mut s = json!({ "n": a.n, "attempt_id": a.attempt_id, "provider_response_id": provider_response_id, "usage": usage, "cost_pusd": cost });
        for (k, x) in self.ident.as_object().unwrap() {
            s[k] = x.clone();
        }
        let path = self.dir.join(format!("attempt-{:03}.settle.json", a.n));
        match write_once(&path, &s) {
            Ok(()) => Ok(cost),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let prior: Option<Value> = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
                if prior.as_ref() == Some(&s) { Ok(cost) } else { Err(self.fail(&format!("spend: conflicting settlement for attempt {}", a.n))) }
            }
            Err(e) => Err(self.fail(&format!("spend: settlement {} not written ({e})", a.n))),
        }
    }

    pub fn anomaly(&mut self, provider_response_id: &str, usage: Option<&Value>) -> String {
        let safe: String = provider_response_id.chars().filter(|c| c.is_ascii_alphanumeric() || "_.:-".contains(*c)).take(200).collect();

        let why = self.fail(&format!("spend: unbound provider response {provider_response_id}"));
        let mut v = json!({ "provider_response_id": provider_response_id, "usage": usage, "bound_pusd": self.bound });
        for (k, x) in self.ident.as_object().unwrap() {
            v[k] = x.clone();
        }
        let _ = write_once(&self.dir.join(format!("anomaly-{safe}.json")), &v);
        why
    }

    pub fn reread(&self) -> Result<(usize, usize, u64), String> {
        self.read().map(|r| (r.attempts.len(), r.settles.len(), r.committed))
    }
}

#[cfg(test)]
pub(crate) mod tests;
