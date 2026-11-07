

#![cfg_attr(not(test), allow(dead_code))]

use crate::dsh_pack::Lock;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

pub const BAND: std::ops::RangeInclusive<u16> = 41700..=41799;
const FILE: &str = "dsh-ports.json";
const NAMES: [&str; 5] = ["web", "bridge", "omni_api", "omni_db", "omni_nats"];
const WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports { pub web: u16, pub bridge: u16, pub omni_api: u16, pub omni_db: u16, pub omni_nats: u16 }

pub fn ports_for(data: &Path, instance: &str) -> Result<Ports, String> {
    with_registry(data, |reg| assign(reg, instance))
}

pub fn next_web_port(data: &Path, instance: &str) -> Result<u16, String> {
    with_registry(data, |reg| {
        let old = assign(reg, instance)?.web;
        let entry = &mut reg["instances"][instance];
        if !entry["retired"].is_array() {
            entry["retired"] = json!([]);
        }
        entry["retired"].as_array_mut().unwrap().push(json!(old));
        let web = next_free(reg)?;
        reg["instances"][instance]["web"] = json!(web);
        Ok(web)
    })
}

pub fn release_instance(data: &Path, instance: &str) -> Result<(), String> {
    with_registry(data, |reg| {
        reg["instances"].as_object_mut().unwrap().remove(instance);
        Ok(())
    })
}

fn with_registry<T>(data: &Path, f: impl FnOnce(&mut Value) -> Result<T, String>) -> Result<T, String> {
    let _lock = Lock::take(&data.join("dsh-ports.lock"), WAIT)?;
    let path = data.join(FILE);
    let mut reg = match fs::read(&path) {
        Ok(b) => serde_json::from_slice::<Value>(&b).ok().filter(|v| v["instances"].is_object()).ok_or_else(|| format!("{}: not a port record; left as is", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({"v": 1, "instances": {}}),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let before = reg.clone();
    let out = f(&mut reg)?;
    if reg != before {
        let tmp = data.join(format!("{FILE}.tmp"));
        let write = || -> std::io::Result<()> {
            let mut o = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
            o.write_all(&serde_json::to_vec_pretty(&reg)?)?;
            o.sync_all()?;
            fs::rename(&tmp, &path)
        };
        write().map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(out)
}

fn assign(reg: &mut Value, instance: &str) -> Result<Ports, String> {
    if !reg["instances"][instance].is_object() {
        reg["instances"][instance] = Value::Object(Map::new());
    }
    let mut got = [0u16; 5];
    for (i, name) in NAMES.iter().enumerate() {
        got[i] = match reg["instances"][instance][*name].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| BAND.contains(p)) {
            Some(p) => p,
            None => {
                let p = next_free(reg)?;
                reg["instances"][instance][*name] = json!(p);
                p
            }
        };
    }
    Ok(Ports { web: got[0], bridge: got[1], omni_api: got[2], omni_db: got[3], omni_nats: got[4] })
}

fn next_free(reg: &Value) -> Result<u16, String> {
    let mut taken = BTreeSet::new();
    for ports in reg["instances"].as_object().into_iter().flat_map(|m| m.values()) {
        taken.extend(NAMES.iter().filter_map(|n| ports[*n].as_u64()));
        taken.extend(ports["retired"].as_array().into_iter().flatten().filter_map(Value::as_u64));
    }
    BAND.clone().find(|p| !taken.contains(&u64::from(*p))).ok_or_else(|| format!("no unregistered port left in {}-{}", BAND.start(), BAND.end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    static N: AtomicUsize = AtomicUsize::new(0);

    struct Tmp(PathBuf);
    impl std::ops::Deref for Tmp {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("unfold-ports-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn all(p: &Ports) -> [u16; 5] {
        [p.web, p.bridge, p.omni_api, p.omni_db, p.omni_nats]
    }

    #[test]
    fn an_instance_gets_five_ports_from_the_band_start_and_keeps_them() {
        let data = tmp("first");
        let p = ports_for(&data, "a").unwrap();
        assert_eq!(p, Ports { web: 41700, bridge: 41701, omni_api: 41702, omni_db: 41703, omni_nats: 41704 });
        assert_eq!(ports_for(&data, "a").unwrap(), p);
        let v: serde_json::Value = serde_json::from_slice(&fs::read(data.join("dsh-ports.json")).unwrap()).unwrap();
        assert_eq!(v["instances"]["a"]["omni_nats"], 41704);
        assert_eq!(ports_for(&data, "b").unwrap().web, 41705);
    }

    #[test]
    fn two_concurrent_instances_get_ten_distinct_ports_in_the_band() {
        let data = tmp("race");
        let go = Barrier::new(2);
        let got: Vec<Ports> = std::thread::scope(|s| {
            let hs: Vec<_> = ["one", "two"].map(|id| s.spawn(|| { go.wait(); ports_for(&data, id).unwrap() })).into_iter().collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let mut ports: Vec<u16> = got.iter().flat_map(all).collect();
        assert!(ports.iter().all(|p| BAND.contains(p)), "{ports:?}");
        ports.sort();
        ports.dedup();
        assert_eq!(ports.len(), 10, "{got:?}");
        assert_eq!(ports_for(&data, "one").unwrap(), got[0]);
        assert_eq!(ports_for(&data, "two").unwrap(), got[1]);
    }

    #[test]
    fn many_concurrent_instances_never_share_a_port() {
        let data = tmp("many");
        let ids: Vec<String> = (0..12).map(|i| format!("inst-{i}")).collect();
        let go = Barrier::new(ids.len());
        let mut ports: Vec<u16> = std::thread::scope(|s| {
            let hs: Vec<_> = ids.iter().map(|id| s.spawn(|| { go.wait(); ports_for(&data, id).unwrap() })).collect();
            hs.into_iter().flat_map(|h| all(&h.join().unwrap())).collect()
        });
        ports.sort();
        ports.dedup();
        assert_eq!(ports.len(), 60);
    }

    #[test]
    fn a_full_band_is_refused() {
        let data = tmp("full");
        for i in 0..20 {
            ports_for(&data, &format!("i{i}")).unwrap();
        }
        let e = ports_for(&data, "one-more").unwrap_err();
        assert!(e.contains("41700-41799"), "{e}");
        assert_eq!(ports_for(&data, "i19").unwrap().omni_nats, 41799);
    }

    #[test]
    fn use_another_port_moves_only_web_and_never_hands_the_old_one_out() {
        let data = tmp("another");
        let a = ports_for(&data, "a").unwrap();
        let web = next_web_port(&data, "a").unwrap();
        assert!(BAND.contains(&web) && !all(&a).contains(&web), "{web}");
        let a2 = ports_for(&data, "a").unwrap();
        assert_eq!(a2, Ports { web, ..a });
        let b = ports_for(&data, "b").unwrap();
        assert!(!all(&b).contains(&a.web) && !all(&b).contains(&web), "{b:?}");

        let c_web = next_web_port(&data, "c").unwrap();
        assert_eq!(ports_for(&data, "c").unwrap().web, c_web);
    }

    #[test]
    fn releasing_an_instance_frees_its_ports_and_its_retired_ones() {
        let data = tmp("release");
        let a = ports_for(&data, "a").unwrap();
        assert_eq!(next_web_port(&data, "a").unwrap(), 41705);
        let b = ports_for(&data, "b").unwrap();
        assert_eq!(b.web, 41706);
        release_instance(&data, "a").unwrap();
        release_instance(&data, "never-registered").unwrap();

        assert_eq!(ports_for(&data, "c").unwrap(), a);
        assert_eq!(ports_for(&data, "b").unwrap(), b);
        assert_eq!(ports_for(&data, "d").unwrap().web, 41705);
    }

    #[test]
    fn a_registered_port_is_kept_even_while_something_listens_on_it() {

        let data = tmp("noprobe");
        let held = std::net::TcpListener::bind("127.0.0.1:41700");
        assert_eq!(ports_for(&data, "a").unwrap().web, 41700, "held: {}", held.is_ok());
    }

    #[test]
    fn a_partial_entry_keeps_its_ports_and_gets_the_missing_ones() {
        let data = tmp("partial");
        fs::write(data.join("dsh-ports.json"), r#"{"v": 1, "instances": {"a": {"web": 41710, "bridge": 41711}}}"#).unwrap();
        let p = ports_for(&data, "a").unwrap();
        assert_eq!((p.web, p.bridge), (41710, 41711));
        assert_eq!([p.omni_api, p.omni_db, p.omni_nats], [41700, 41701, 41702]);
    }

    #[test]
    fn an_unreadable_registry_is_refused_and_left_as_is() {
        let data = tmp("corrupt");
        fs::write(data.join("dsh-ports.json"), "{not json").unwrap();
        assert!(ports_for(&data, "a").unwrap_err().contains("dsh-ports.json"));
        assert_eq!(fs::read_to_string(data.join("dsh-ports.json")).unwrap(), "{not json");
    }
}
