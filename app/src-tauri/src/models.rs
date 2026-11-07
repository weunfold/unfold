

use crate::{owned_dir, owned_open, Open};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn catalog_path(contents: &Path) -> PathBuf {
    contents.join("Resources/models/catalog.json")
}

pub fn load_catalog(contents: &Path) -> Result<Value, String> {
    let p = catalog_path(contents);
    let v: Value = serde_json::from_slice(&std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
    if !v["models"].is_array() || !v["parts"].is_array() {
        return Err(format!("{}: not a model catalog", p.display()));
    }
    Ok(v)
}

fn model<'a>(cat: &'a Value, id: &str) -> Option<&'a Value> {
    cat["models"].as_array()?.iter().find(|m| m["id"] == id)
}

pub fn dest(data: &Path, id: &str) -> PathBuf {
    data.join("models").join(format!("{id}.gguf"))
}

fn marker(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".sha256");
    PathBuf::from(s)
}

#[derive(Debug, PartialEq)]
pub enum Where {
    Ready(PathBuf),
    Local(PathBuf),
    Download,
}

fn ready(m: &Value, data: &Path) -> Option<PathBuf> {
    let (size, sha) = (m["size"].as_u64().unwrap_or(0), m["sha256"].as_str().unwrap_or(""));
    let d = dest(data, m["id"].as_str().unwrap_or(""));
    (std::fs::metadata(&d).is_ok_and(|x| x.is_file() && x.len() == size) && std::fs::read_to_string(marker(&d)).is_ok_and(|s| s.trim() == sha)).then_some(d)
}

pub fn locate(cat: &Value, m: &Value, data: &Path, home: &Path) -> Where {
    let size = m["size"].as_u64().unwrap_or(0);
    if let Some(d) = ready(m, data) {
        return Where::Ready(d);
    }
    for dir in cat["local_copies"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let dir = dir.strip_prefix("~/").map(|r| home.join(r)).unwrap_or_else(|| PathBuf::from(dir));
        let p = dir.join(m["file"].as_str().unwrap_or(""));
        if std::fs::symlink_metadata(&p).is_ok_and(|x| x.is_file() && x.len() == size) {
            return Where::Local(p);
        }
    }
    Where::Download
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

pub fn offer(cat: &Value, data: &Path, home: &Path) -> Value {
    let parts = cat["parts"].as_array().cloned().unwrap_or_default();
    Value::Array(
        parts
            .into_iter()
            .map(|mut p| {
                let opts: Vec<Value> = p["options"].as_array().cloned().unwrap_or_default().into_iter().map(|o| {
                    let Some(m) = model(cat, o["id"].as_str().unwrap_or("")) else { return o };
                    let size = m["size"].as_u64().unwrap_or(0);
                    let here = match locate(cat, m, data, home) {
                        Where::Ready(_) | Where::Local(_) => "on this Mac".to_string(),
                        Where::Download => format!("downloads {}", gb(size)),
                    };
                    let mut x = json!({"id": m["id"], "name": m["name"], "meta": format!("{} · {} · {here}", m["vendor"].as_str().unwrap_or(""), gb(size)), "stat": o["stat"]});
                    if o["best"] == true {
                        x["best"] = json!(true);
                    }
                    x
                }).collect();
                p["options"] = Value::Array(opts);
                p
            })
            .collect(),
    )
}

pub fn validate_choice(cat: &Value, choice: &Value) -> Result<Value, String> {
    let mut out = serde_json::Map::new();
    for p in cat["parts"].as_array().into_iter().flatten() {
        let part = p["part"].as_str().unwrap_or("");
        let id = choice[part].as_str().ok_or(format!("no choice for {part}"))?;
        if !p["options"].as_array().into_iter().flatten().any(|o| o["id"] == id) {
            return Err(format!("{id} is not offered for {part}"));
        }
        out.insert(part.into(), json!(id));
    }
    Ok(Value::Object(out))
}

pub fn migrate_choice(cat: &Value, choice: &Value) -> (Value, bool) {
    let mut out = choice.clone();
    let mut moved = false;
    for p in cat["parts"].as_array().into_iter().flatten() {
        let part = p["part"].as_str().unwrap_or("");
        let Some(id) = choice[part].as_str() else { continue };
        let offered = |id: &str| p["options"].as_array().into_iter().flatten().any(|o| o["id"] == id);
        if offered(id) {
            continue;
        }
        let to = cat["models"].as_array().into_iter().flatten().find(|m| m["replaces"].as_array().into_iter().flatten().any(|r| r == id) && m["id"].as_str().is_some_and(offered));
        if let Some(to) = to {
            out[part] = to["id"].clone();
            moved = true;
        }
    }
    (out, moved)
}

pub fn retire_replaced(cat: &Value, from: &Value, to: &Value, data: &Path) -> Vec<PathBuf> {
    let mut gone = Vec::new();
    for (part, old) in from.as_object().into_iter().flatten() {
        let (Some(old), Some(new)) = (old.as_str(), to[part].as_str()) else { continue };
        let Some(m) = model(cat, new).filter(|m| old != new && m["replaces"].as_array().into_iter().flatten().any(|r| r == old)) else { continue };
        if ready(m, data).is_none() {
            continue;
        }
        let d = dest(data, old);
        for f in [marker(&d), d] {
            if std::fs::remove_file(&f).is_ok() {
                gone.push(f);
            }
        }
    }
    gone
}

pub fn choice_path(data: &Path) -> PathBuf {
    data.join("app").join("models.json")
}

pub fn save_choice(data: &Path, choice: &Value) -> Result<(), String> {
    owned_dir(data, &data.join("app"))?;
    let p = choice_path(data);
    let mut f = owned_open(&p, Open::Truncate).map_err(|e| format!("{}: {e}", p.display()))?;
    f.write_all(format!("{:#}\n", json!({"models": choice})).as_bytes()).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn load_choice(data: &Path) -> Option<Value> {
    let v: Value = serde_json::from_slice(&std::fs::read(choice_path(data)).ok()?).ok()?;
    v["models"].is_object().then(|| v["models"].clone())
}

pub fn hash_file(p: &Path, progress: &dyn Fn(u64)) -> Result<String, String> {
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut next) = (0u64, 64u64 << 20);
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("{}: {e}", p.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        if done >= next {
            progress(done);
            next += 64 << 20;
        }
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn url(m: &Value) -> String {
    format!("https://huggingface.co/{}/resolve/{}/{}", m["repo"].as_str().unwrap_or(""), m["revision"].as_str().unwrap_or(""), m["file"].as_str().unwrap_or(""))
}

pub fn ensure(cat: &Value, id: &str, data: &Path, home: &Path, progress: &dyn Fn(&str, u64, u64)) -> Result<PathBuf, String> {
    let m = model(cat, id).ok_or(format!("{id} is not in the catalog"))?;
    let (size, sha) = (m["size"].as_u64().unwrap_or(0), m["sha256"].as_str().unwrap_or("").to_string());
    let name = m["name"].as_str().unwrap_or(id).to_string();
    let d = dest(data, id);
    owned_dir(data, &data.join("models"))?;
    let part = d.with_extension("gguf.part");
    let _ = std::fs::remove_file(&part);
    match locate(cat, m, data, home) {
        Where::Ready(p) => return Ok(p),
        Where::Local(src) => {
            progress(&format!("Checking {name} on this Mac"), 0, size);
            let got = hash_file(&src, &|n| progress(&format!("Checking {name} on this Mac"), n, size))?;
            if got != sha {
                return Err(format!("{} is not the pinned {name} (sha256 {got})", src.display()));
            }

            let ok = Command::new("/bin/cp").arg("-c").arg(&src).arg(&part).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
            if !ok {
                return Err(format!("could not copy {}", src.display()));
            }
        }
        Where::Download => {
            let mut child = Command::new("/usr/bin/curl")
                .args(["--fail", "--location", "--silent", "--show-error", "--retry", "3", "--proto", "=https", "--tlsv1.2", "-o"])
                .arg(&part)
                .arg(url(m))
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("curl: {e}"))?;
            loop {
                if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                    if !s.success() {
                        let mut e = String::new();
                        if let Some(mut er) = child.stderr.take() {
                            let _ = er.read_to_string(&mut e);
                        }
                        let _ = std::fs::remove_file(&part);
                        return Err(format!("the download of {name} failed: {}", e.trim()));
                    }
                    break;
                }
                progress(&format!("Getting {name}"), std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0), size);
                std::thread::sleep(Duration::from_millis(500));
            }
            progress(&format!("Checking {name}"), 0, size);
            let got = hash_file(&part, &|n| progress(&format!("Checking {name}"), n, size))?;
            if got != sha {
                let _ = std::fs::remove_file(&part);
                return Err(format!("the download of {name} is not the pinned file (sha256 {got})"));
            }
        }
    }
    std::fs::rename(&part, &d).map_err(|e| format!("{}: {e}", d.display()))?;
    std::fs::write(marker(&d), format!("{sha}\n")).map_err(|e| format!("{}: {e}", marker(&d).display()))?;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, Value) {
        let t = std::env::temp_dir().join(format!("unfold-models-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(t.join("home/models")).unwrap();
        std::fs::create_dir_all(t.join("data")).unwrap();
        let bytes = b"fictional gguf bytes\n";
        std::fs::write(t.join("home/models/a.gguf"), bytes).unwrap();
        let sha: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        let cat = json!({
            "models": [
                {"id": "a", "name": "A 1B", "vendor": "V", "repo": "o/r", "revision": "rev", "file": "a.gguf", "size": bytes.len(), "sha256": sha},
                {"id": "b", "name": "B 2B", "vendor": "W", "repo": "o/s", "revision": "rev2", "file": "b.gguf", "size": 2_000_000_000u64, "sha256": "00"}
            ],
            "parts": [
                {"part": "talk", "title": "Talk", "options": [{"id": "a", "stat": "s1"}, {"id": "b", "stat": "s2", "best": true}]},
                {"part": "voice", "title": "Voice", "later": true, "options": [{"id": "off", "name": "Later"}]}
            ],
            "local_copies": ["~/models"]
        });
        (t, cat)
    }

    fn rand_suffix() -> String {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        format!("{t}-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    #[test]
    fn the_offer_says_what_is_on_this_mac_and_what_downloads() {
        let (t, cat) = fixture();
        let o = offer(&cat, &t.join("data"), &t.join("home"));
        assert_eq!(o[0]["part"], "talk");
        assert!(o[0]["options"][0]["meta"].as_str().unwrap().ends_with("on this Mac"), "{o}");
        assert!(o[0]["options"][1]["meta"].as_str().unwrap().ends_with("downloads 2.0 GB"), "{o}");
        assert_eq!(o[0]["options"][1]["best"], true);
        assert_eq!(o[1]["options"][0]["name"], "Later", "non-catalog options pass through");
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_choice_must_be_offered_for_every_part() {
        let (t, cat) = fixture();
        assert_eq!(validate_choice(&cat, &json!({"talk": "a", "voice": "off"})).unwrap(), json!({"talk": "a", "voice": "off"}));
        assert!(validate_choice(&cat, &json!({"talk": "zzz", "voice": "off"})).unwrap_err().contains("not offered"));
        assert!(validate_choice(&cat, &json!({"voice": "off"})).unwrap_err().contains("no choice for talk"));

        let no_voice = json!({"parts": [cat["parts"][0].clone()]});
        assert_eq!(validate_choice(&no_voice, &json!({"talk": "a", "voice": "openai-realtime"})).unwrap(), json!({"talk": "a"}));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_saved_choice_of_a_replaced_model_moves_to_its_replacement() {
        let (t, mut cat) = fixture();
        cat["models"][0]["replaces"] = json!(["old"]);
        assert_eq!(migrate_choice(&cat, &json!({"talk": "old", "voice": "off"})), (json!({"talk": "a", "voice": "off"}), true));
        assert_eq!(migrate_choice(&cat, &json!({"talk": "b", "voice": "off"})), (json!({"talk": "b", "voice": "off"}), false), "an offered id stays");
        assert_eq!(migrate_choice(&cat, &json!({"talk": "zzz"})), (json!({"talk": "zzz"}), false), "an id nothing replaces stays, and validation refuses it");
        let (moved, _) = migrate_choice(&cat, &json!({"talk": "old", "voice": "off"}));
        assert!(validate_choice(&cat, &moved).is_ok());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_replaced_models_file_goes_only_once_its_replacement_is_ready() {
        let (t, mut cat) = fixture();
        let (data, home) = (t.join("data"), t.join("home"));
        cat["models"][0]["replaces"] = json!(["old"]);
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        std::fs::create_dir_all(data.join("models")).unwrap();
        let old = dest(&data, "old");
        std::fs::write(&old, b"fictional old gguf\n").unwrap();
        std::fs::write(marker(&old), "11\n").unwrap();
        let (from, to) = (json!({"talk": "old", "voice": "off"}), json!({"talk": "a", "voice": "off"}));

        assert!(retire_replaced(&cat, &from, &to, &data).is_empty());
        assert!(old.exists());

        ensure(&cat, "a", &data, &home, &|_, _, _| {}).unwrap();
        assert_eq!(retire_replaced(&cat, &from, &to, &data), vec![marker(&old), old.clone()]);
        assert!(!old.exists() && dest(&data, "a").exists());

        std::fs::write(dest(&data, "b"), b"x").unwrap();
        assert!(retire_replaced(&cat, &json!({"talk": "b"}), &json!({"talk": "a"}), &data).is_empty());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn a_local_copy_is_verified_then_cloned_and_marked_ready() {
        let (t, cat) = fixture();
        let (data, home) = (t.join("data"), t.join("home"));
        let _ = crate::WRITE_MODE.set(crate::WriteMode::Dev);
        let p = ensure(&cat, "a", &data, &home, &|_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"fictional gguf bytes\n");
        assert!(matches!(locate(&cat, &cat["models"][0], &data, &home), Where::Ready(_)));

        let _ = std::fs::remove_file(&p);
        std::fs::write(home.join("models/a.gguf"), b"fictional gguf BYTES\n").unwrap();
        assert!(ensure(&cat, "a", &data, &home, &|_, _, _| {}).unwrap_err().contains("is not the pinned"));
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn the_url_is_the_pinned_revision() {
        let (t, cat) = fixture();
        assert_eq!(url(&cat["models"][0]), "https://huggingface.co/o/r/resolve/rev/a.gguf");
        let _ = std::fs::remove_dir_all(&t);
    }
}
