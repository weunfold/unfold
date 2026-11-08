

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

pub fn utc_iso(ms: u64) -> String {
    let (secs, millis) = (ms / 1000, ms % 1000);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn utc_compact(ms: u64) -> String {
    let iso = utc_iso(ms);
    format!("{}{}{}T{}{}{}Z", &iso[0..4], &iso[5..7], &iso[8..10], &iso[11..13], &iso[14..16], &iso[17..19])
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {

    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn strict_parse<T: DeserializeOwned + serde::Serialize>(bytes: &[u8], what: &str) -> Result<T> {
    let raw: serde_json::Value = serde_json::from_slice(bytes).with_context(|| format!("{what}: not JSON"))?;
    let typed: T = serde_json::from_value(raw.clone()).with_context(|| format!("{what}: schema"))?;
    let back = serde_json::to_value(&typed)?;
    let mut unknown = Vec::new();
    unknown_keys(&raw, &back, "$", &mut unknown);
    if !unknown.is_empty() {
        bail!("{what}: unknown fields {}", unknown.join(", "));
    }
    Ok(typed)
}

fn unknown_keys(raw: &serde_json::Value, back: &serde_json::Value, at: &str, out: &mut Vec<String>) {
    use serde_json::Value;
    match (raw, back) {
        (Value::Object(r), Value::Object(b)) => {
            for (k, v) in r {
                match b.get(k) {
                    Some(bv) => unknown_keys(v, bv, &format!("{at}.{k}"), out),
                    None => out.push(format!("{at}.{k}")),
                }
            }
        }
        (Value::Array(r), Value::Array(b)) => {
            for (i, (rv, bv)) in r.iter().zip(b).enumerate() {
                unknown_keys(rv, bv, &format!("{at}[{i}]"), out);
            }
        }
        _ => {}
    }
}

pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("refusing to overwrite {}", path.display()))?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formats_known_instants() {
        assert_eq!(utc_iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(utc_iso(1_792_531_580_123), "2026-10-20T21:26:20.123Z");
        assert_eq!(utc_compact(1_792_531_580_123), "20261020T212620Z");
    }

    #[test]
    fn strict_parse_rejects_typos() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct S {
            a: u32,
            #[serde(default)]
            b: Option<u32>,
        }
        assert!(strict_parse::<S>(br#"{"a":1}"#, "s").is_ok());
        let err = strict_parse::<S>(br#"{"a":1,"bb":2}"#, "s").err().unwrap().to_string();
        assert!(err.contains("$.bb"), "{err}");
    }
}
