

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, Result};

use crate::local;

pub const PROVIDER: &str = "dsh";

pub const FIRST_TOKEN: Duration = Duration::from_millis(8000);
pub const STALL: Duration = Duration::from_millis(8000);
pub const N_CTX: u32 = crate::flash::N_CTX;

fn models() -> &'static Mutex<HashMap<String, &'static str>> {
    static M: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

pub fn display_name(model: &str) -> Option<&'static str> {
    models().lock().unwrap().get(model).copied()
}

pub fn is_model(model: &str) -> bool {
    display_name(model).is_some()
}

pub fn endpoint(url: &str) -> Result<String> {
    let shape = || anyhow::anyhow!("RTH_DSH_URL must be http://127.0.0.1:<port>/v1, not {url:?}");
    let (authority, path) = local::parse_loopback_url(url).map_err(|_| shape())?;
    if !authority.starts_with("127.0.0.1:") || path != "/v1" {
        return Err(shape());
    }
    Ok(authority)
}

pub fn config_with_model(key: &str, url: &str, model: &str, name: Option<&str>) -> Result<local::Config> {
    let authority = endpoint(url)?;
    if !crate::flash::key_ok(key) {
        bail!("RTH_DSH_KEY must be 8-512 printable ASCII characters without spaces");
    }
    if model.is_empty() || model.len() > 200 || !model.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("RTH_DSH_MODEL must be 1-200 printable ASCII characters without spaces");
    }
    let name = name.map(str::trim).filter(|n| !n.is_empty() && n.len() <= 200 && !n.chars().any(char::is_control)).unwrap_or(model);
    local::register_keyed(local::Keyed { authority: authority.clone(), bearer: key.to_string(), stall: STALL });
    models().lock().unwrap().insert(model.to_string(), Box::leak(name.to_string().into_boxed_str()));
    Ok(local::Config {
        authority,
        base_path: "/v1".into(),
        model: model.to_string(),
        model_sha256: None,
        slot: None,
        decide_slot: None, opener_slot: None,

        max_tokens: None,
        n_ctx: N_CTX,
        first_token: FIRST_TOKEN,
        system_instruction: None,
        health_every: Duration::from_millis(500),
    })
}

pub fn config_from_env() -> Result<local::Config> {
    config_from(&|k| std::env::var(k).ok())
}

pub fn config_from(var: &dyn Fn(&str) -> Option<String>) -> Result<local::Config> {
    let var = |k: &str| var(k).filter(|v| !v.is_empty());
    let key = var("RTH_DSH_KEY").ok_or_else(|| anyhow::anyhow!("RTH_BACKEND=dsh needs RTH_DSH_KEY (the app reads it from the bridge's endpoint file)"))?;
    let url = var("RTH_DSH_URL").ok_or_else(|| anyhow::anyhow!("RTH_BACKEND=dsh needs RTH_DSH_URL"))?;
    let model = var("RTH_DSH_MODEL").ok_or_else(|| anyhow::anyhow!("RTH_BACKEND=dsh needs RTH_DSH_MODEL"))?;
    config_with_model(&key, &url, &model, var("RTH_DSH_MODEL_NAME").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flash::test_server::{start_keyed, Answer};
    use serde_json::{json, Value};

    const KEY: &str = "dsh-bridge-key-not-a-secret-0123456789";

    fn delta(text: &str) -> String {
        json!({"model": "deepseek-official/deepseek-v4-flash", "choices": [{"delta": {"content": text}}]}).to_string()
    }

    fn body_of(raw: &str) -> Value {
        serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    }

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        let pairs = pairs.to_vec();
        move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn the_bridge_url_is_loopback_http_on_127_0_0_1_with_a_port_on_v1() {
        for ok in ["http://127.0.0.1:8123/v1", "http://127.0.0.1:8123/v1/", "http://127.0.0.1:8123"] {
            assert_eq!(endpoint(ok).unwrap(), "127.0.0.1:8123", "{ok}");
        }
        for bad in [
            "http://localhost:8123/v1",
            "http://[::1]:8123/v1",
            "http://10.0.0.2:8123/v1",
            "http://127.0.0.2:8123/v1",
            "https://127.0.0.1:8123/v1",
            "http://u:p@127.0.0.1:8123/v1",
            "http://127.0.0.1:8123/v1?x=1",
            "http://127.0.0.1/v1",
            "http://127.0.0.1:0/v1",
            "http://127.0.0.1:99999/v1",
            "http://127.0.0.1:8123/v2",
            "http://127.0.0.1:8123/v1/../x",
            "http://127.0.0.1.evil.com:1/v1",
            "http://api.dsh.example/v1",
            "",
        ] {
            assert!(endpoint(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dsh_needs_its_key_url_and_model_and_names_the_model_as_the_bridge_lists_it() {
        let url = ("RTH_DSH_URL", "http://127.0.0.1:8123/v1");
        let model = ("RTH_DSH_MODEL", "unit-provider/unit-model");
        assert!(config_from(&env(&[url, model])).is_err(), "the key is required");
        assert!(config_from(&env(&[("RTH_DSH_KEY", KEY), model])).is_err(), "the URL is required");
        assert!(config_from(&env(&[("RTH_DSH_KEY", KEY), url])).is_err(), "the model is required");
        assert!(config_from(&env(&[("RTH_DSH_KEY", KEY), url, ("RTH_DSH_MODEL", "has space")])).is_err());
        assert!(config_from(&env(&[("RTH_DSH_KEY", "short"), url, model])).is_err(), "a key is 8-512 printable characters");
        assert!(!is_model("unit-provider/unit-model"), "no id is DSH's until configured");
        let cfg = config_from(&env(&[("RTH_DSH_KEY", KEY), url, model, ("RTH_DSH_MODEL_NAME", "Unit Model (via DSH)")])).unwrap();
        assert_eq!((cfg.authority.as_str(), cfg.base_path.as_str(), cfg.model.as_str()), ("127.0.0.1:8123", "/v1", "unit-provider/unit-model"));
        assert_eq!((cfg.max_tokens, cfg.first_token, cfg.slot), (None, FIRST_TOKEN, None));
        assert!(is_model("unit-provider/unit-model") && !crate::flash::is_model("unit-provider/unit-model"), "a DSH model is not Flash");
        assert_eq!(local::display_name("unit-provider/unit-model"), "Unit Model (via DSH)", "the prompt names it as the bridge lists it");
        let cfg = config_with_model(KEY, "http://127.0.0.1:8124/v1", "kimi/kimi-k2", None).unwrap();
        assert_eq!(local::display_name(&cfg.model), "kimi/kimi-k2", "without a listed name, the id");
    }

    #[test]
    fn the_key_is_never_printed() {
        let cfg = config_with_model(KEY, "http://127.0.0.1:8125/v1", "p/m", None).unwrap();
        let shown = format!("{:?} {cfg:?}", local::dial(&cfg.authority));
        assert!(!shown.contains(KEY) && shown.contains("Keyed(127.0.0.1:8125, key redacted)"), "{shown}");
        assert!(local::dial(&cfg.authority).is_hosted(), "a DSH reply is written off this Mac");
    }

    #[tokio::test]
    async fn a_reply_streams_over_plain_loopback_http_with_the_bearer_and_the_hosted_request_shape() {
        let events = vec![delta("Hi"), delta(" there."), json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 3}}).to_string()];
        let s = start_keyed(Answer::Stream { events, gap: Duration::from_millis(5), hang: false }, KEY).await;
        let cfg = config_with_model(KEY, &s.url, "deepseek-official/deepseek-v4-flash", None).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let done = local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap();
        drop(tx);
        let mut got = String::new();
        while let Some(t) = rx.recv().await {
            got += &t;
        }
        assert_eq!(got, "Hi there.");
        assert_eq!(done.provider_model.as_deref(), Some("deepseek-official/deepseek-v4-flash"));
        let raw = s.requests.lock().unwrap()[0].clone();
        assert!(raw.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"), "{raw}");
        assert!(raw.contains(&format!("\r\nAuthorization: Bearer {KEY}\r\n")) && raw.contains(&format!("\r\nHost: {}\r\n", cfg.authority)), "{raw}");
        let b = body_of(&raw);
        assert_eq!((b["model"].as_str(), b["stream_options"]["include_usage"].as_bool()), (Some("deepseek-official/deepseek-v4-flash"), Some(true)));
        assert!(b.get("cache_prompt").is_none() && b.get("id_slot").is_none() && b.get("tools").is_none(), "no llama-only fields, no tools: {b}");
    }

    #[tokio::test]
    async fn a_wrong_key_is_refused_with_401_and_never_echoed() {
        use base64::Engine;
        let s = start_keyed(Answer::Stream { events: vec![delta("never")], gap: Duration::ZERO, hang: false }, "the-bridge-s-new-key-0000000000").await;
        let cfg = config_with_model(KEY, &s.url, "p/m", None).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!((e.reason, e.detail.as_str()), (local::SERVER_ERROR, "HTTP 401: invalid_api_key"));
        assert_eq!(local::fallback_reason(&e), "error");
        drop(tx);
        assert!(rx.recv().await.is_none(), "no words");
        let b64 = base64::engine::general_purpose::STANDARD.encode(KEY);
        let redacted = local::dial(&cfg.authority).redact(&format!("x {KEY} y {b64} z"));
        assert!(!redacted.contains(KEY) && !redacted.contains(&b64), "{redacted}");
    }

    #[tokio::test]
    async fn memory_decide_on_dsh_sends_the_schema() {
        let answer = json!({"choices": [{"message": {"content": "{\"d\":\"no_op\"}"}, "finish_reason": "stop"}]}).to_string();
        let s = start_keyed(Answer::Json(answer), KEY).await;
        let cfg = config_with_model(KEY, &s.url, "p/m", None).unwrap();
        let offered = [crate::decide::Candidate { memory_id: "rm-1".into(), revision: 1, text: "my cafe is Lua".into() }];
        let run = crate::decide::run(&cfg, None, "hi", &offered, Duration::from_secs(2)).await;
        assert_eq!(run.outcome, Ok(crate::decide::Decision::NoOp));
        let b = body_of(&s.requests.lock().unwrap()[0]);
        assert_eq!(b["response_format"], crate::decide::schema(&crate::decide::offer(&offered)));
        assert!(b.get("grammar").is_none(), "{b}");
    }
}
