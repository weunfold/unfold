

use crate::Log;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const KEY_SERVICE: &str = "ai.weunfold.unfold.cloud";

pub const KEY_ACCOUNT: &str = "unfold-app";

pub const ENV_KEY: &str = "RTH_FLASH_KEY";
const ENV_URL: &str = "RTH_FLASH_URL";
const ENV_MODEL: &str = "RTH_FLASH_MODEL";
const BACKEND_FLASH: &str = "flash";
pub const EGRESS: &str = "allow:flash";
const HELLO_FLASH: &str = "flash+";

pub const GRANT_NEEDS_ACK: bool = false;

pub const MODEL: &str = "DeepSeek V4.1 Flash";

const DEEPSEEK_HOST: &str = "deepseek.com";

const REASONS: [&str; 5] = ["offline", "cap", "error", "timeout", "stall"];

#[derive(Clone, Debug, PartialEq)]
pub struct Provider {
    pub url: String,
    pub model: String,

    pub label: String,
    pub keychain_service: String,
}

pub fn provider_path(data: &Path) -> PathBuf {
    data.join("app").join("cloud.json")
}

pub fn provider_from(text: Option<&str>) -> Result<Option<Provider>, String> {
    let Some(text) = text else { return Ok(None) };
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let field = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let url = field("url").ok_or("no url")?;
    if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
        return Err("the url is not https".into());
    }
    let model = field("model").ok_or("no model")?;
    let label = field("label").unwrap_or_else(|| MODEL.to_string());
    let keychain_service = field("keychain_service").unwrap_or_else(|| KEY_SERVICE.to_string());
    Ok(Some(Provider { url, model, label, keychain_service }))
}

pub fn provider(data: &Path, log: Option<&Log>) -> Option<Provider> {
    let p = provider_path(data);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            if let Some(l) = log {
                l.line(&format!("flash: {} cannot be read ({}): no cloud", p.display(), e.kind()));
            }
            return None;
        }
    };
    match provider_from(text.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            if let Some(l) = log {
                l.line(&format!("flash: {} is not a provider ({e}): no cloud", p.display()));
            }
            None
        }
    }
}

pub fn page_state(data: &Path, on: bool) -> Value {
    let p = provider(data, None);
    let key = p.as_ref().and_then(|p| key_present(&p.keychain_service));
    json!({"on": on, "provider": p.is_some(), "key": key, "model": p.as_ref().map(|p| p.label.clone())})
}

pub fn is_deepseek(p: &Provider) -> bool {
    let rest = p.url.strip_prefix("https://").unwrap_or_default();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or_default().trim_end_matches('.').to_ascii_lowercase();
    host == DEEPSEEK_HOST || host.ends_with(&format!(".{DEEPSEEK_HOST}"))
}

#[cfg_attr(not(feature = "menu_bar"), allow(dead_code))]
pub fn deepseek_key_with(provider: Option<&Provider>, read: impl FnOnce(&str) -> Result<Option<String>, String>) -> Result<Option<String>, String> {
    let Some(p) = provider.filter(|p| is_deepseek(p)) else { return Ok(None) };
    Ok(read(&p.keychain_service)?.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()))
}

#[cfg_attr(not(feature = "menu_bar"), allow(dead_code))]
pub fn deepseek_key(data: &Path, log: &Log) -> Option<String> {
    deepseek_key_with(provider(data, Some(log)).as_ref(), read_key).unwrap_or_else(|e| {
        log.line(&format!("flash: the DeepSeek key could not be read from the Keychain: {e}"));
        None
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {

    Flash,

    Local(&'static str),
}

struct State {
    route: Option<Route>,

    local_name: String,
    spend: Option<Value>,
    last_fallback: Option<Value>,

    grant: bool,

    provider: Option<Provider>,
}
static STATE: Mutex<State> = Mutex::new(State { route: None, local_name: String::new(), spend: None, last_fallback: None, grant: false, provider: None });

pub fn grant_allowed(needs_ack: bool, acked: bool) -> bool {
    !needs_ack || acked
}

pub fn with_flash(mut env: Vec<(String, String)>, use_cloud: bool, grant: bool, provider: Option<&Provider>, key: Result<Option<String>, String>) -> (Vec<(String, String)>, Route) {
    if !use_cloud {
        return (env, Route::Local("off"));
    }
    let Some(p) = provider else { return (env, Route::Local("no_provider")) };
    match key {
        Ok(Some(k)) if !k.trim().is_empty() => {
            env.retain(|(n, _)| ![ENV_KEY, ENV_URL, ENV_MODEL, "RTH_MEMORY_EGRESS", "RTH_BACKEND"].contains(&n.as_str()));
            env.push(("RTH_BACKEND".into(), BACKEND_FLASH.into()));
            env.push((ENV_KEY.into(), k.trim().into()));
            env.push((ENV_URL.into(), p.url.clone()));
            env.push((ENV_MODEL.into(), p.model.clone()));
            if grant {
                env.push(("RTH_MEMORY_EGRESS".into(), EGRESS.into()));
            }
            (env, Route::Flash)
        }
        Ok(_) => (env, Route::Local("no_key")),
        Err(_) => (env, Route::Local("keychain")),
    }
}

pub fn prepare(env: Vec<(String, String)>, data: &Path, local_name: &str, log: &Log) -> Vec<(String, String)> {
    let s = crate::settings::current();
    let on = s.answers_direct();
    let p = if on { provider(data, Some(log)) } else { None };
    prepare_with(env, local_name, on, grant_allowed(GRANT_NEEDS_ACK, s.cloud_ack), p, read_key, log)
}

pub(crate) fn prepare_with(env: Vec<(String, String)>, local_name: &str, on: bool, grant: bool, provider: Option<Provider>, read: impl FnOnce(&str) -> Result<Option<String>, String>, log: &Log) -> Vec<(String, String)> {
    let key = match &provider {
        Some(p) if on => read(&p.keychain_service),
        _ => Ok(None),
    };
    if let Err(e) = &key {
        log.line(&format!("flash: the cloud key could not be read from the Keychain: {e}"));
    }
    let (env, route) = with_flash(env, on, grant, provider.as_ref(), key);
    let label = provider.as_ref().map_or(MODEL, |p| p.label.as_str());
    log.line(&format!("flash: {}", match &route {
        Route::Flash if grant => format!("on: {label} first, the local model as its fallback"),
        Route::Flash => format!("on, without the memory grant (not acknowledged yet): {label} first, no saved memories sent"),
        Route::Local(why) => format!("off ({why}): the local model only"),
    }));
    let mut s = STATE.lock().unwrap();

    s.last_fallback = None;
    s.grant = route == Route::Flash && grant;
    s.route = Some(route);
    s.local_name = local_name.to_string();
    s.provider = provider;
    env
}

pub fn paused_for_dsh(local_name: &str, log: &Log) {
    log.line("flash: not used: a model Unfold Automagik lists answers instead");
    let mut s = STATE.lock().unwrap();
    s.route = Some(Route::Local("dsh"));
    s.last_fallback = None;
    s.grant = false;
    s.local_name = local_name.to_string();
    s.provider = None;
}

pub fn local_reason() -> Option<(&'static str, String)> {
    let s = STATE.lock().unwrap();
    match s.route {
        Some(Route::Local(w)) if w != "off" && w != "dsh" => Some((w, s.local_name.clone())),
        _ => None,
    }
}

pub fn direct() -> bool {
    matches!(STATE.lock().unwrap().route, Some(Route::Flash))
}

fn label() -> String {
    STATE.lock().unwrap().provider.as_ref().map_or(MODEL.to_string(), |p| p.label.clone())
}

pub fn local_name() -> String {
    STATE.lock().unwrap().local_name.clone()
}

pub fn hello_prefix(model_id: &str) -> String {
    let flash = matches!(STATE.lock().unwrap().route, Some(Route::Flash));
    format!("{}local@{model_id}", if crate::dsh::active() { crate::dsh::HELLO } else if flash { HELLO_FLASH } else { "" })
}

pub fn accept(backend: &str, prefix: &str) -> bool {
    if backend.starts_with(prefix) {
        return true;
    }
    let head = [HELLO_FLASH, crate::dsh::HELLO].into_iter().find(|h| prefix.starts_with(h));
    let local = head.and_then(|h| prefix.strip_prefix(h)).filter(|l| backend.starts_with(l)).is_some();
    if local && head == Some(HELLO_FLASH) {
        let mut s = STATE.lock().unwrap();
        if s.route == Some(Route::Flash) {
            s.route = Some(Route::Local("unsupported"));
        }
    } else if local {
        crate::dsh::unsupported();
    }
    local
}

pub fn on_frame(f: &Value) -> Option<Option<Value>> {
    match f["kind"].as_str()? {
        "spend_status" => {
            let spend = spend_of(f)?;
            STATE.lock().unwrap().spend = Some(spend);
            crate::settings::refresh_open();
            Some(None)
        }
        "response_provenance" => {
            if let Some(reason) = fallback_note(f) {
                STATE.lock().unwrap().last_fallback = Some(json!({"reason": reason, "at_ms": now_ms()}));
                crate::settings::refresh_open();
            }
            let mut w = route_wire(f, &crate::settings::model_name_of);
            if let Some(w) = w.as_mut() {
                w["foot"] = foot(f, &crate::settings::model_name_of, &label());
                if w["foot"]["level"].is_string() && crate::settings::take_deepseek_note() {
                    w["note"] = json!("deepseek_direct");
                }
            }
            Some(w)
        }
        _ => None,
    }
}

fn fallback_note(f: &Value) -> Option<&'static str> {
    if f["backend"].as_str().is_some_and(|b| b.starts_with(crate::dsh::HELLO)) {
        return None;
    }
    let why = f["fallback_reason"].as_str()?;
    Some(REASONS.into_iter().find(|r| *r == why).unwrap_or("error"))
}

fn route_wire(f: &Value, name_of: &dyn Fn(&str) -> String) -> Option<Value> {
    let model = f["model"].as_str().filter(|m| !m.is_empty() && *m != "none")?;
    if f["backend"] == "agentd" {
        return None;
    }
    let c = &f["config"];
    let mut p = json!({"backend": f["backend"], "model": model});

    for k in ["response_id", "provider_model", "fallback_reason", "device_kind"] {
        if f[k].is_string() {
            p[k] = f[k].clone();
        }
    }
    if c.is_object() {
        p["config"] = json!({"effort": c["effort"], "thread": c["thread"], "thinking": c["thinking"]});

        if let Some(src) = c["effort_source"].as_str().filter(|s| ["person", "auto", "default"].contains(s)) {
            p["config"]["effort_source"] = json!(src);
        }
    }
    Some(json!({"wire": "route", "provenance": p, "names": {model: name_of(model)}}))
}

pub fn foot(f: &Value, name_of: &dyn Fn(&str) -> String, label: &str) -> Value {
    let model = f["model"].as_str().unwrap_or_default();
    let backend = f["backend"].as_str().unwrap_or_default();
    let local = backend.split_once('@').map(|(_, l)| l).unwrap_or_default();
    let direct = backend.starts_with(HELLO_FLASH) && !f["fallback_reason"].is_string() && model != local;
    let c = &f["config"];
    let level = c["effort"].as_str().filter(|e| direct && ["off", "low", "high", "max"].contains(e));
    json!({"model": if direct { label.to_string() } else { name_of(model) }, "level": level, "auto": level.is_some() && c["effort_source"] == "auto"})
}

pub fn violation(wire: &Value) -> bool {
    let c = &wire["provenance"]["config"];
    let (deep, source) = (c["effort"] == "high" || c["effort"] == "max", c["effort_source"].as_str().unwrap_or("default"));
    c["thread"] == "main" && deep && (source == "default" || (source == "auto" && c["effort"] == "max"))
}

fn spend_of(f: &Value) -> Option<Value> {
    let (spent, cap) = (f["spent_pico"].as_u64()?, f["cap_pico"].as_u64().filter(|c| *c > 0)?);
    let month = f["month"].as_str().filter(|m| m.len() == 7 && m.as_bytes()[4] == b'-' && m.bytes().enumerate().all(|(i, b)| i == 4 || b.is_ascii_digit()))?;
    Some(json!({"month": month, "spent_pico": spent, "cap_pico": cap, "capped": f["capped"] == true || spent >= cap}))
}

pub fn view(on: bool, data: &Path) -> Value {
    let p = provider(data, None);
    let service = p.as_ref().map_or(KEY_SERVICE, |p| p.keychain_service.as_str()).to_string();
    let present = key_present(&service);
    let s = STATE.lock().unwrap();
    let (route, why) = match &s.route {
        Some(Route::Flash) => ("flash", None),
        Some(Route::Local(w)) => ("local", Some(*w)),
        None => ("not_started", None),
    };
    json!({
        "on": on,
        "route": route,
        "why": why,
        "grant": s.grant,
        "needs_ack": GRANT_NEEDS_ACK,
        "provider": p.is_some(),
        "deepseek": p.as_ref().is_some_and(is_deepseek),
        "model": p.as_ref().map_or(MODEL, |p| p.label.as_str()),
        "local_model": if s.local_name.is_empty() { None } else { Some(s.local_name.clone()) },
        "key": {"service": service, "account": KEY_ACCOUNT, "present": present},
        "spend": s.spend,
        "last_fallback": s.last_fallback,
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg_attr(feature = "rehearsal_pack", allow(dead_code))]
const ERR_ITEM_NOT_FOUND: i32 = -25300;

#[cfg_attr(feature = "rehearsal_pack", allow(dead_code))]
fn services(service: &str) -> Vec<String> {
    let legacy = std::env::var("UNFOLD_LEGACY_DEEPSEEK_SERVICE").ok();
    services_with_legacy(service, legacy.as_deref())
}

#[cfg_attr(feature = "rehearsal_pack", allow(dead_code))]
fn services_with_legacy(service: &str, legacy: Option<&str>) -> Vec<String> {
    let mut services = vec![service.to_string()];
    if service == KEY_SERVICE {
        if let Some(old) = legacy.filter(|old| !old.is_empty() && *old != service) {
            services.push(old.to_string());
        }
    }
    services
}

#[cfg(all(target_os = "macos", not(feature = "rehearsal_pack")))]
fn read_key(service: &str) -> Result<Option<String>, String> {
    crate::keychain_call()?;
    for svc in services(service) {
        match security_framework::os::macos::passwords::find_generic_password(None, &svc, KEY_ACCOUNT) {
            Ok((pw, _)) => return String::from_utf8(pw.to_vec()).map(Some).map_err(|_| "the stored key is not UTF-8".to_string()),
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => continue,
            Err(e) => return Err(format!("Keychain error {}", e.code())),
        }
    }
    Ok(None)
}

#[cfg(all(target_os = "macos", not(feature = "rehearsal_pack")))]
pub fn key_present(service: &str) -> Option<bool> {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    crate::keychain_call().ok()?;
    for svc in services(service) {
        let mut q = ItemSearchOptions::new();
        q.class(ItemClass::generic_password()).service(&svc).account(KEY_ACCOUNT).load_attributes(true).limit(Limit::Max(1));
        match q.search() {
            Ok(found) if !found.is_empty() => return Some(true),
            Ok(_) => continue,
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => continue,
            Err(_) => return None,
        }
    }
    Some(false)
}

#[cfg(all(not(target_os = "macos"), not(feature = "rehearsal_pack")))]
fn read_key(_service: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(all(not(target_os = "macos"), not(feature = "rehearsal_pack")))]
pub fn key_present(_service: &str) -> Option<bool> {
    Some(false)
}

#[cfg(feature = "rehearsal_pack")]
fn read_key(service: &str) -> Result<Option<String>, String> {
    crate::scratch_keys::Item { service, account: KEY_ACCOUNT }.read()
}

#[cfg(feature = "rehearsal_pack")]
pub fn key_present(service: &str) -> Option<bool> {
    crate::scratch_keys::Item { service, account: KEY_ACCOUNT }.present().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Vec<(String, String)> {
        vec![("RTH_BACKEND".into(), "local".into()), ("RTH_LOCAL_MODEL".into(), "granite41-3b-q4km".into())]
    }
    fn has(env: &[(String, String)], k: &str) -> Option<String> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }
    fn fictional() -> Provider {
        Provider { url: "https://api.flash.example/v1".into(), model: "fictional-flash".into(), label: "DeepSeek V4.1 Flash".into(), keychain_service: KEY_SERVICE.into() }
    }
    fn at(url: &str) -> Provider {
        Provider { url: url.into(), ..fictional() }
    }

    #[test]
    fn only_deepseek_s_own_api_shares_its_key_with_unfold_automagik() {
        let fixture_secret_0 = format!("fixture-{}-{}-0", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        for url in ["https://api.deepseek.com/v1", "https://api.deepseek.com", "https://deepseek.com/", "https://API.DeepSeek.com:443/v1", "https://api.deepseek.com./v1"] {
            assert!(is_deepseek(&at(url)), "{url}");
        }
        for url in ["https://api.flash.example/v1", "https://notdeepseek.com/v1", "https://deepseek.com.example/v1", "https://deepseek.com@evil.example/v1", "https://evil.example/deepseek.com", "https://evil.example/?h=deepseek.com"] {
            assert!(!is_deepseek(&at(url)), "{url}");
        }
        let key = fixture_secret_0.as_str();
        let ds = at("https://api.deepseek.com/v1");
        assert_eq!(deepseek_key_with(Some(&ds), |svc| { assert_eq!(svc, KEY_SERVICE); Ok(Some(format!(" {key}\n"))) }), Ok(Some(key.to_string())), "the stored key, trimmed");
        assert_eq!(deepseek_key_with(Some(&ds), |_| Ok(None)), Ok(None), "no item: nothing to share");
        assert_eq!(deepseek_key_with(Some(&ds), |_| Ok(Some("  ".into()))), Ok(None), "an empty item is no key");
        assert_eq!(deepseek_key_with(Some(&ds), |_| Err("Keychain error -25293".into())), Err("Keychain error -25293".into()));
        assert_eq!(deepseek_key_with(Some(&fictional()), |_| panic!("another provider's key is never read")), Ok(None));
        assert_eq!(deepseek_key_with(None, |_| panic!("no cloud.json: nothing is read")), Ok(None));

        let d = std::env::temp_dir().join(format!("unfold-flash-dskey-{}-{}", std::process::id(), now_ms()));
        let log = crate::Log::open_in(None, &d);
        std::fs::create_dir_all(d.join("app")).unwrap();
        std::fs::write(provider_path(&d), r#"{"url": "https://api.flash.example/v1", "model": "m"}"#).unwrap();
        assert_eq!(deepseek_key(&d, &log), None, "not DeepSeek's API: no key, no Keychain read");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_key_and_the_grant_follow_the_switch() {
        let (env, route) = with_flash(base(), true, true, Some(&fictional()), Ok(Some("jk-fictional-0000".into())));
        assert_eq!(route, Route::Flash);
        assert_eq!((has(&env, ENV_KEY).as_deref(), has(&env, "RTH_MEMORY_EGRESS").as_deref()), (Some("jk-fictional-0000"), Some(EGRESS)));
        assert_eq!(has(&env, "RTH_BACKEND").as_deref(), Some("flash"), "the Router is selected by name");
        assert_eq!(has(&env, "RTH_LOCAL_MODEL").as_deref(), Some("granite41-3b-q4km"), "the local model stays: it is the fallback");
        assert_eq!(env.iter().filter(|(n, _)| n == "RTH_BACKEND").count(), 1);
        assert_eq!((has(&env, "RTH_FLASH_URL").as_deref(), has(&env, "RTH_FLASH_MODEL").as_deref()), (Some("https://api.flash.example/v1"), Some("fictional-flash")), "the endpoint and model come from cloud.json");
        let (env, route) = with_flash(base(), true, true, None, Ok(Some("jk-fictional-0000".into())));
        assert_eq!((route, has(&env, ENV_KEY), has(&env, "RTH_FLASH_URL"), has(&env, "RTH_BACKEND").as_deref()), (Route::Local("no_provider"), None, None, Some("local")), "no cloud.json: local only, even with a key");

        let (env, route) = with_flash(base(), false, true, Some(&fictional()), Ok(Some("jk-fictional-0000".into())));
        assert_eq!((route, has(&env, ENV_KEY), has(&env, "RTH_MEMORY_EGRESS")), (Route::Local("off"), None, None), "off: no key, no grant");
        assert_eq!(has(&env, "RTH_BACKEND").as_deref(), Some("local"));
        let (env, route) = with_flash(base(), true, true, Some(&fictional()), Ok(None));
        assert_eq!((route, has(&env, ENV_KEY), has(&env, "RTH_MEMORY_EGRESS")), (Route::Local("no_key"), None, None), "no item: local only");
        let (env, route) = with_flash(base(), true, true, Some(&fictional()), Err("denied".into()));
        assert_eq!((route, has(&env, ENV_KEY)), (Route::Local("keychain"), None));
        assert_eq!(with_flash(base(), true, true, Some(&fictional()), Ok(Some("  ".into()))).1, Route::Local("no_key"), "an empty item is no key");
    }

    #[test]
    fn the_public_build_holds_the_grant_until_acknowledged() {
        assert!(!GRANT_NEEDS_ACK, "this build: ON gives the grant from the first turn");
        assert!(grant_allowed(false, false), "this build: no acknowledgment needed");
        assert!(!grant_allowed(true, false) && grant_allowed(true, true), "the public build: only after it");
        let (env, route) = with_flash(base(), true, grant_allowed(true, false), Some(&fictional()), Ok(Some("jk-fictional-0000".into())));
        assert_eq!((route, has(&env, ENV_KEY).is_some(), has(&env, "RTH_MEMORY_EGRESS")), (Route::Flash, true, None), "Flash talks, no saved memories go");
    }

    #[test]
    fn spend_status_is_checked_before_settings_shows_it() {
        let ok = json!({"kind": "spend_status", "month": "2026-10", "spent_pico": 420_000_000_000u64, "cap_pico": 50_000_000_000_000u64, "capped": false});
        assert_eq!(spend_of(&ok).unwrap(), json!({"month": "2026-10", "spent_pico": 420_000_000_000u64, "cap_pico": 50_000_000_000_000u64, "capped": false}));
        let over = json!({"kind": "spend_status", "month": "2026-10", "spent_pico": 50_000_000_000_000u64, "cap_pico": 50_000_000_000_000u64});
        assert_eq!(spend_of(&over).unwrap()["capped"], true, "at the cap reads as capped");
        for bad in [json!({"month": "2026-10", "spent_pico": 1.5, "cap_pico": 5}), json!({"month": "2026-10", "spent_pico": 1, "cap_pico": 0}), json!({"month": "Oct", "spent_pico": 1, "cap_pico": 5})] {
            assert_eq!(spend_of(&bad), None, "{bad}");
        }
    }

    #[test]
    fn each_reply_s_provenance_becomes_its_route_nod() {
        let name = |id: &str| if id == "granite41-3b-q4km" { "Granite 4.1 3B".to_string() } else { id.to_string() };
        let w = route_wire(&json!({"kind": "response_provenance", "backend": "flash+local@granite41-3b-q4km", "model": "granite41-3b-q4km", "fallback_reason": "timeout", "prompt": "unfold-flash-text-v3", "prompt_sha256": "abc", "config": {"path": "chat", "temperature": 0.3, "top_p": null, "thinking": false, "effort": "off", "thread": "main"}, "device_kind": "Mac"}), &name).unwrap();
        assert_eq!(w, json!({"wire": "route", "provenance": {"backend": "flash+local@granite41-3b-q4km", "model": "granite41-3b-q4km", "fallback_reason": "timeout", "device_kind": "Mac", "config": {"effort": "off", "thread": "main", "thinking": false}}, "names": {"granite41-3b-q4km": "Granite 4.1 3B"}}), "the nod's fields only: no prompt");
        assert_eq!(route_wire(&json!({"kind": "response_provenance", "backend": "agentd", "model": "none", "prompt": "agentd-fixed-reply@rc4-forget"}), &name), None, "agentd's own fixed reply: no nod");
        assert_eq!(route_wire(&json!({"kind": "response_provenance", "backend": "local@granite41-3b-q4km"}), &name), None, "no model named: the time alone, never a guess");
        let old = route_wire(&json!({"kind": "response_provenance", "backend": "local@granite41-3b-q4km", "model": "granite41-3b-q4km", "prompt": "p"}), &name).unwrap();
        assert!(old["provenance"].get("config").is_none() && old["provenance"].get("device_kind").is_none(), "an old row: route only");
        assert!(violation(&json!({"provenance": {"config": {"effort": "high", "thread": "main"}}})));
        assert!(!violation(&json!({"provenance": {"config": {"effort": "high", "thread": "background"}}})));

        assert!(!violation(&json!({"provenance": {"config": {"effort": "high", "thread": "main", "effort_source": "person"}}})));
        assert!(!violation(&json!({"provenance": {"config": {"effort": "max", "thread": "main", "effort_source": "person"}}})));
        assert!(!violation(&json!({"provenance": {"config": {"effort": "high", "thread": "main", "effort_source": "auto"}}})));
        assert!(violation(&json!({"provenance": {"config": {"effort": "max", "thread": "main", "effort_source": "auto"}}})));
        assert!(violation(&json!({"provenance": {"config": {"effort": "high", "thread": "main", "effort_source": "default"}}})));
        assert!(on_frame(&json!({"kind": "response_provenance", "model": "granite41-3b-q4km", "fallback_reason": "gremlins"})).unwrap().is_some());
        assert_eq!(on_frame(&json!({"kind": "agent_delta", "text": "hi"})), None);

        let flash = json!({"kind": "response_provenance", "response_id": "r1", "backend": "flash+local@granite41-3b-q4km", "model": "deepseek-v4.1-flash-hosted", "provider_model": "deepseek-ai/DeepSeek-V4.1-Flash-1017", "config": {"path": "chat", "temperature": 0.7, "top_p": 0.95, "thinking": false}, "prompt": "p"});
        assert_eq!(on_frame(&flash).unwrap().unwrap()["provenance"]["provider_model"], "deepseek-ai/DeepSeek-V4.1-Flash-1017", "a Flash reply: its nod");
        assert_eq!(on_frame(&flash).unwrap().unwrap()["provenance"]["response_id"], "r1", "the nod names its reply, so the page keys its icon row to it");
        let local = json!({"kind": "response_provenance", "response_id": "r2", "backend": "flash+local@granite41-3b-q4km", "model": "granite41-3b-q4km", "fallback_reason": "cap", "config": {"path": "chat"}, "prompt": "p"});
        assert_eq!(on_frame(&local).unwrap().unwrap()["provenance"]["fallback_reason"], "cap");
        let spend = json!({"kind": "spend_status", "month": "2026-10", "spent_pico": 94_000_000u64, "cap_pico": 50_000_000_000_000u64, "capped": false});
        assert_eq!(on_frame(&spend), Some(None), "kept for Settings, nothing on the page");
    }

    #[test]
    fn a_dsh_fallback_is_never_noted_as_deepseek_s() {
        let frame = |backend: &str, why: &str| json!({"kind": "response_provenance", "backend": backend, "model": "granite41-3b-q4km", "fallback_reason": why});
        assert_eq!(fallback_note(&frame("dsh+local@granite41-3b-q4km", "offline")), None, "the person's DSH, not DeepSeek");
        assert_eq!(fallback_note(&frame("flash+local@granite41-3b-q4km", "cap")), Some("cap"), "DeepSeek's own fallback is kept");
        assert_eq!(fallback_note(&frame("flash+local@granite41-3b-q4km", "gremlins")), Some("error"));
        assert_eq!(fallback_note(&json!({"kind": "response_provenance", "model": "m", "fallback_reason": "timeout"})), Some("timeout"), "an older frame without a backend: as before");
        assert!(on_frame(&frame("dsh+local@granite41-3b-q4km", "offline")).unwrap().is_some(), "a DSH fallback's nod still renders");
    }

    #[test]
    fn a_flash_start_accepts_a_local_only_agentd_as_local() {
        assert!(accept("flash+local@granite41-3b-q4km", "flash+local@granite41-3b-q4km"));
        assert!(accept("local@granite41-3b-q4km", "local@granite41-3b-q4km"));
        assert!(accept("local@granite41-3b-q4km", "flash+local@granite41-3b-q4km"), "an agentd without Flash");
        assert!(!accept("flash+local@granite41-3b-q4km", "local@granite41-3b-q4km"), "Flash where none was set up: refused");
        assert!(!accept("openai-realtime", "flash+local@granite41-3b-q4km"));
        assert!(accept("dsh+local@granite41-3b-q4km", "dsh+local@granite41-3b-q4km"), "a DSH start");
        assert!(!accept("flash+local@granite41-3b-q4km", "dsh+local@granite41-3b-q4km"), "Flash where DSH was set up: refused");
        assert!(!accept("dsh+local@granite41-3b-q4km", "flash+local@granite41-3b-q4km"), "DSH where Flash was set up: refused");
    }

    #[test]
    fn the_key_reaches_only_the_childs_environment_never_the_log() {
        let fixture_secret_0 = format!("fixture-{}-{}-0", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        let dir = std::env::temp_dir().join(format!("unfold-flash-{}-{}", std::process::id(), now_ms()));
        let log = crate::Log::open_in(None, &dir);
        let key = fixture_secret_0.as_str();
        let env = prepare_with(base(), "Granite 4.1 3B", true, true, Some(fictional()), |svc| { assert_eq!(svc, KEY_SERVICE); Ok(Some(key.into())) }, &log);
        assert_eq!(has(&env, ENV_KEY).as_deref(), Some(key));
        let _ = prepare_with(base(), "Granite 4.1 3B", false, true, Some(fictional()), |_| panic!("off never reads the Keychain"), &log);
        let _ = prepare_with(base(), "Granite 4.1 3B", true, true, None, |_| panic!("no provider never reads the Keychain"), &log);
        assert_eq!(local_reason(), Some(("no_provider", "Granite 4.1 3B".to_string())));
        let text = std::fs::read_to_string(dir.join("app.log")).unwrap();
        assert!(text.contains("flash: on") && !text.contains(key), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_view_names_the_item_but_never_holds_a_key() {
        let dir = std::env::temp_dir().join(format!("unfold-flash-view-{}-{}", std::process::id(), now_ms()));
        let v = view(true, &dir);
        assert_eq!((v["key"]["service"].as_str(), v["key"]["account"].as_str()), (Some(KEY_SERVICE), Some(KEY_ACCOUNT)));
        assert_eq!(v["key"].as_object().unwrap().len(), 3, "names and presence only");
        assert_eq!((v["provider"].as_bool(), v["model"].as_str()), (Some(false), Some(MODEL)), "no cloud.json: no provider");
        assert!(v.get("via").is_none() && !v.to_string().contains("http"), "no host in the view: {v}");
        std::fs::create_dir_all(dir.join("app")).unwrap();
        std::fs::write(provider_path(&dir), r#"{"url": "https://api.flash.example/v1", "model": "fictional-flash", "label": "Fictional Flash", "keychain_service": "ai.example.fictional"}"#).unwrap();
        let v = view(true, &dir);
        assert_eq!((v["provider"].as_bool(), v["model"].as_str(), v["key"]["service"].as_str()), (Some(true), Some("Fictional Flash"), Some("ai.example.fictional")));
        assert!(!v.to_string().contains("flash.example"), "the url stays out of the view: {v}");
        let p = page_state(&dir, true);
        assert_eq!((p["on"].as_bool(), p["provider"].as_bool(), p["model"].as_str()), (Some(true), Some(true), Some("Fictional Flash")));
        assert!(!p.to_string().contains("flash.example"));
        assert_eq!(v["deepseek"].as_bool(), Some(false), "not DeepSeek's own API");
        std::fs::write(provider_path(&dir), r#"{"url": "https://api.deepseek.com/v1", "model": "deepseek-chat"}"#).unwrap();
        let v = view(true, &dir);
        assert_eq!((v["deepseek"].as_bool(), v["model"].as_str()), (Some(true), Some(MODEL)), "DeepSeek's own API: its key goes to Unfold Automagik too");
        assert!(!v.to_string().contains("deepseek.com"), "{v}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cloud_json_is_the_only_provider() {
        assert_eq!(provider_from(None), Ok(None), "no file: no cloud");
        let p = provider_from(Some(r#"{"url": " https://api.flash.example/v1 ", "model": "m1"}"#)).unwrap().unwrap();
        assert_eq!((p.url.as_str(), p.model.as_str(), p.label.as_str(), p.keychain_service.as_str()), ("https://api.flash.example/v1", "m1", MODEL, KEY_SERVICE), "defaults: the model's name and the neutral item");
        for bad in [r#"{"model": "m1"}"#, r#"{"url": "http://api.flash.example/v1", "model": "m1"}"#, r#"{"url": "https://api.flash.example/v1"}"#, "not json", r#"{"url": "https://a b/v1", "model": "m"}"#] {
            assert!(provider_from(Some(bad)).is_err(), "{bad}");
        }
        assert_eq!(services_with_legacy(KEY_SERVICE, None), vec![KEY_SERVICE]);
        assert_eq!(services_with_legacy(KEY_SERVICE, Some("ai.example.previous")), vec![KEY_SERVICE, "ai.example.previous"]);
        assert_eq!(services_with_legacy(KEY_SERVICE, Some("")), vec![KEY_SERVICE]);
        assert_eq!(services_with_legacy(KEY_SERVICE, Some(KEY_SERVICE)), vec![KEY_SERVICE]);
        assert_eq!(services_with_legacy("ai.example.fictional", Some("ai.example.previous")), vec!["ai.example.fictional"], "a named item only");
    }

    #[test]
    fn each_reply_s_footnote_names_its_model_and_deepseek_s_level_only() {
        let name = |id: &str| match id { "granite41-3b-q4km" => "Granite 4.1 3B".to_string(), "openai-codex/gpt-5.5" => "GPT-5.5".to_string(), o => o.to_string() };
        let direct = json!({"kind": "response_provenance", "backend": "flash+local@granite41-3b-q4km", "model": "deepseek-flash", "provider_model": "deepseek-flash", "config": {"effort": "high", "effort_source": "auto", "thread": "main", "think_budget": 4096, "max_tokens": 4496, "auto": {"class": "C", "confidence": 0.9, "ms": 53, "reason": "check"}}});
        assert_eq!(foot(&direct, &name, "DeepSeek V4.1 Flash"), json!({"model": "DeepSeek V4.1 Flash", "level": "high", "auto": true}), "DeepSeek V4.1 Flash · High (Auto)");
        let mut picked = direct.clone();
        picked["config"]["effort"] = json!("max");
        picked["config"]["effort_source"] = json!("person");
        assert_eq!(foot(&picked, &name, "DeepSeek V4.1 Flash"), json!({"model": "DeepSeek V4.1 Flash", "level": "max", "auto": false}));
        let mut off = direct.clone();
        off["config"] = json!({"effort": "off", "effort_source": "auto"});
        assert_eq!(foot(&off, &name, "DeepSeek V4.1 Flash")["level"], "off", "Off is a level too");
        let fallback = json!({"backend": "flash+local@granite41-3b-q4km", "model": "granite41-3b-q4km", "fallback_reason": "timeout", "config": {"effort": "high", "effort_source": "auto"}});
        assert_eq!(foot(&fallback, &name, "DeepSeek V4.1 Flash"), json!({"model": "Granite 4.1 3B", "level": null, "auto": false}), "Granite answered: no level");
        let granite = json!({"backend": "local@granite41-3b-q4km", "model": "granite41-3b-q4km", "config": {"effort": "off"}});
        assert_eq!(foot(&granite, &name, "DeepSeek V4.1 Flash"), json!({"model": "Granite 4.1 3B", "level": null, "auto": false}));
        let bridge = json!({"backend": "dsh+local@granite41-3b-q4km", "model": "openai-codex/gpt-5.5", "config": {"effort": "off"}});
        assert_eq!(foot(&bridge, &name, "DeepSeek V4.1 Flash"), json!({"model": "GPT-5.5", "level": null, "auto": false}), "a bridge model: its name, no level");
        let w = route_wire(&direct, &name).unwrap();
        assert_eq!(w["provenance"]["config"], json!({"effort": "high", "thread": "main", "thinking": null, "effort_source": "auto"}), "the source reaches the page; budgets and Auto's check stay out");
    }
}
