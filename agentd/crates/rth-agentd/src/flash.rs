

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::local;

#[cfg(test)]
pub const MODEL: &str = "flash-test-model";
pub const DISPLAY_NAME: &str = "DeepSeek V4.1 Flash";

pub const NO_ENDPOINT: &str = "no_endpoint";

pub const NO_MODEL: &str = "no_model";

pub const FIRST_TOKEN: Duration = Duration::from_millis(4120);
pub const STALL: Duration = Duration::from_millis(4000);

pub const REPLY_LIMIT: u32 = 256_000;

pub const SIDE_MAX_TOKENS: u32 = 400;

pub const N_CTX: u32 = 65_536;

pub const DECIDE_DEADLINE: Duration = Duration::from_millis(2000);

fn models() -> &'static Mutex<HashSet<String>> {
    static M: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

pub fn is_model(model: &str) -> bool {
    models().lock().unwrap().contains(model)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub authority: String,
    pub host: String,
    pub base_path: String,
}

fn dns_name(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && host.len() <= 253
        && labels.iter().all(|l| (1..=63).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !l.starts_with('-') && !l.ends_with('-'))
        && !labels.last().unwrap().bytes().all(|b| b.is_ascii_digit())
}

pub fn endpoint(url: &str, test_ca: bool) -> Result<Endpoint> {
    let rest = url.strip_prefix("https://").ok_or_else(|| anyhow::anyhow!("RTH_FLASH_URL must be https, not {url:?}"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let path = path.trim_end_matches('/');
    if !(path.is_empty() || path == "/v1") {
        bail!("RTH_FLASH_URL path must be /v1, not {path:?}");
    }
    if authority.contains('@') {
        bail!("RTH_FLASH_URL must not carry credentials");
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, Some(p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| anyhow::anyhow!("RTH_FLASH_URL port {p:?} is not a port"))?)),
        None => (authority, None),
    };
    match (host, port) {
        ("127.0.0.1", Some(p)) if test_ca => Ok(Endpoint { authority: format!("127.0.0.1:{p}"), host: "127.0.0.1".into(), base_path: "/v1".into() }),
        ("127.0.0.1", _) => bail!("RTH_FLASH_URL may name loopback only with RTH_CA_FILE and an explicit port (a test server)"),
        (h, p) if dns_name(h) => Ok(Endpoint { authority: format!("{}:{}", h.to_ascii_lowercase(), p.unwrap_or(443)), host: h.to_ascii_lowercase(), base_path: "/v1".into() }),
        _ => bail!("RTH_FLASH_URL must name a host by its DNS name, not {url:?}"),
    }
}

pub(crate) fn key_ok(key: &str) -> bool {
    (8..=512).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic())
}

pub fn hosted(ep: &Endpoint, ca_file: Option<&str>, key: &str, stall: Duration) -> Result<local::Hosted> {
    if !key_ok(key) {
        bail!("RTH_FLASH_KEY must be 8-512 printable ASCII characters without spaces");
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = tokio_rustls::TlsConnector::from(std::sync::Arc::new(crate::gemini::client_config(ca_file).context("Flash TLS roots")?));
    let server = rustls_pki_types::ServerName::try_from(ep.host.clone()).context("Flash host is not a TLS name")?;
    Ok(local::Hosted { host: ep.host.clone(), tls, server, bearer: key.to_string(), stall })
}

#[cfg(test)]
pub fn config(key: &str, url: &str, ca_file: Option<&str>) -> Result<local::Config> {
    config_with_model(key, url, ca_file, MODEL)
}

pub fn config_with_model(key: &str, url: &str, ca_file: Option<&str>, model: &str) -> Result<local::Config> {
    let ep = endpoint(url, ca_file.is_some())?;
    if model.is_empty() || model.len() > 200 || !model.bytes().all(|b| b.is_ascii_graphic()) {
        bail!("RTH_FLASH_MODEL must be 1-200 printable ASCII characters without spaces");
    }
    local::register_hosted(&ep.authority, hosted(&ep, ca_file, key, STALL)?);
    let model = model.to_string();
    models().lock().unwrap().insert(model.clone());
    Ok(local::Config {
        authority: ep.authority,
        base_path: ep.base_path,
        model,
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

pub fn config_from_env() -> Result<std::result::Result<local::Config, &'static str>> {
    config_from(&|k| std::env::var(k).ok())
}

pub fn config_from(var: &dyn Fn(&str) -> Option<String>) -> Result<std::result::Result<local::Config, &'static str>> {
    let var = |k: &str| var(k).filter(|v| !v.is_empty());
    let key = var("RTH_FLASH_KEY").ok_or_else(|| anyhow::anyhow!("RTH_BACKEND=flash needs RTH_FLASH_KEY (the app reads it from the Keychain)"))?;
    let Some(url) = var("RTH_FLASH_URL") else { return Ok(Err(NO_ENDPOINT)) };
    let Some(model) = var("RTH_FLASH_MODEL") else { return Ok(Err(NO_MODEL)) };
    config_with_model(&key, &url, var("RTH_CA_FILE").as_deref(), &model).map(Ok)
}

#[cfg(test)]
pub(crate) mod test_server {

    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    mod tls { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/tls.rs")); }
    pub use tls::ca;

    pub enum Answer {

        Stream { events: Vec<String>, gap: std::time::Duration, hang: bool },

        Json(String),

        Status(u16, String),

        ErrorDrip { gap: std::time::Duration, pieces: usize },

        LateErrorDrip { head: std::time::Duration, gap: std::time::Duration, pieces: usize },

        Delayed { head: std::time::Duration, events: Vec<String>, gap: std::time::Duration },
    }

    pub struct Server {
        pub url: String,
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    pub async fn start(answer: Answer) -> Server {
        use rustls_pki_types::pem::PemObject;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certs = rustls_pki_types::CertificateDer::pem_slice_iter(&tls::material().cert).collect::<Result<Vec<_>, _>>().unwrap();
        let key = rustls_pki_types::PrivateKeyDer::from_pem_slice(&tls::material().key).unwrap();
        let cfg = rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(certs, key).unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}/v1", l.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (seen, answer) = (requests.clone(), Arc::new(answer));
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                let (acceptor, seen, answer) = (acceptor.clone(), seen.clone(), answer.clone());
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else { return };
                    serve(tls, &seen, &answer, None).await;
                });
            }
        });
        Server { url, requests }
    }

    pub async fn start_keyed(answer: Answer, key: &str) -> Server {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", l.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (seen, answer, key) = (requests.clone(), Arc::new(answer), Arc::new(key.to_string()));
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                let (seen, answer, key) = (seen.clone(), answer.clone(), key.clone());
                tokio::spawn(async move { serve(tcp, &seen, &answer, Some(&key)).await });
            }
        });
        Server { url, requests }
    }

    async fn serve(mut io: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin, seen: &Mutex<Vec<String>>, answer: &Answer, key: Option<&str>) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];

        loop {
            let n = io.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(end) = text.find("\r\n\r\n") {
                let len = text[..end].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        let raw = String::from_utf8_lossy(&buf).to_string();
        seen.lock().unwrap().push(raw.clone());
        if let Some(key) = key {
            let sent = raw.lines().find_map(|l| l.strip_prefix("Authorization: Bearer ")).unwrap_or("");
            if sent != key {
                let body = serde_json::json!({"error": {"message": format!("bad key {sent}"), "type": "invalid_api_key"}}).to_string();
                let _ = io.write_all(format!("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                let _ = io.shutdown().await;
                return;
            }
        }
        let tls = &mut io;
        match answer {
            Answer::Json(body) => {
                let _ = tls.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            }
            Answer::ErrorDrip { gap, pieces } => {
                let _ = tls.write_all(b"HTTP/1.1 503 Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n").await;
                let _ = tls.flush().await;
                for _ in 0..*pieces {
                    let _ = tls.write_all(b".").await;
                    let _ = tls.flush().await;
                    tokio::time::sleep(*gap).await;
                }
            }
            Answer::Status(status, body) => {
                let _ = tls.write_all(format!("HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
            }
            Answer::LateErrorDrip { head, gap, pieces } => {
                tokio::time::sleep(*head).await;
                let _ = tls.write_all(b"HTTP/1.1 503 Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n").await;
                let _ = tls.flush().await;
                for _ in 0..*pieces {
                    let _ = tls.write_all(b".").await;
                    let _ = tls.flush().await;
                    tokio::time::sleep(*gap).await;
                }
            }
            Answer::Delayed { head, events, gap } => {
                tokio::time::sleep(*head).await;
                let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await;
                let _ = tls.flush().await;
                for e in events {
                    tokio::time::sleep(*gap).await;
                    let _ = tls.write_all(format!("data: {e}\n\n").as_bytes()).await;
                    let _ = tls.flush().await;
                }
                let _ = tls.write_all(b"data: [DONE]\n\n").await;
            }
            Answer::Stream { events, gap, hang } => {
                let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await;
                for e in events {
                    let _ = tls.write_all(format!("data: {e}\n\n").as_bytes()).await;
                    let _ = tls.flush().await;
                    tokio::time::sleep(*gap).await;
                }
                if *hang {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
                let _ = tls.write_all(b"data: [DONE]\n\n").await;
            }
        }
        let _ = tls.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{start, Answer, ca};
    use super::*;
    use crate::decide::Candidate;
    use serde_json::{json, Value};

    const KEY: &str = "test-key-not-a-secret-0123456789";

    fn delta(text: &str) -> String {
        json!({"model": "deepseek-ai/DeepSeek-V4.1-Flash-1017", "choices": [{"delta": {"content": text}}]}).to_string()
    }

    fn body_of(raw: &str) -> Value {
        serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    }

    #[test]
    fn any_https_dns_host_is_an_endpoint_and_loopback_only_with_a_test_ca() {
        let api = Endpoint { authority: "api.flash.example:443".into(), host: "api.flash.example".into(), base_path: "/v1".into() };
        for ok in ["https://api.flash.example/v1", "https://api.flash.example", "https://api.flash.example:443/v1/", "https://API.flash.example/v1"] {
            assert_eq!(endpoint(ok, false).unwrap(), api, "{ok}");
        }
        assert_eq!(endpoint("https://api.flash.example:8443/v1", false).unwrap().authority, "api.flash.example:8443");
        assert_eq!(endpoint("https://127.0.0.1:8443/v1", true).unwrap().authority, "127.0.0.1:8443");
        for bad in [
            "http://api.flash.example/v1",
            "https://u:p@api.flash.example/v1",
            "https://api.flash.example/v1/../x",
            "https://api.flash.example/v1?x=1",
            "https://api.flash.example/v2",
            "https://api.flash.example:0/v1",
            "https://api.flash.example:x/v1",
            "https://127.0.0.1:8443/v1",
            "https://10.0.0.2/v1",
            "https://localhost:8443/v1",
            "https://-bad.example/v1",
            "https://bad_host.example/v1",
            "https:///v1",
            "",
        ] {
            assert!(endpoint(bad, false).is_err(), "{bad}");
        }
        assert!(endpoint("https://127.0.0.1/v1", true).is_err(), "loopback needs an explicit port");
        assert!(endpoint("https://10.0.0.2:8443/v1", true).is_err(), "a CA file never admits another host");
    }

    #[test]
    fn flash_needs_its_url_and_takes_its_model_from_configuration() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let off = |pairs| config_from(&env(pairs)).unwrap().err();
        assert_eq!(off(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_MODEL", "provider-flash-id")]), Some(NO_ENDPOINT), "no URL: Flash off");
        assert_eq!(off(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", ""), ("RTH_FLASH_MODEL", "provider-flash-id")]), Some(NO_ENDPOINT), "an empty URL is unset");
        assert_eq!(off(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", "https://api.flash.example/v1")]), Some(NO_MODEL), "no model: Flash off");
        assert_eq!(off(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", "https://api.flash.example/v1"), ("RTH_FLASH_MODEL", "")]), Some(NO_MODEL));
        assert!(config_from(&env(&[("RTH_FLASH_URL", "https://api.flash.example/v1"), ("RTH_FLASH_MODEL", "provider-flash-id")])).is_err(), "the key is required");
        assert!(config_from(&env(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", "https://api.flash.example/v1"), ("RTH_FLASH_MODEL", "has space")])).is_err());
        assert!(!is_model("provider-flash-id"), "no id is Flash's until configured");
        let cfg = config_from(&env(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", "https://api.flash.example/v1"), ("RTH_FLASH_MODEL", "provider-flash-id")])).unwrap().unwrap();
        assert_eq!((cfg.authority.as_str(), cfg.model.as_str()), ("api.flash.example:443", "provider-flash-id"));
        assert!(is_model("provider-flash-id") && !is_model("granite41-3b-q4km"));
        assert_eq!(local::display_name("provider-flash-id"), DISPLAY_NAME, "a configured Flash id is named as Flash");
        assert!(config_from(&env(&[("RTH_FLASH_KEY", KEY), ("RTH_FLASH_URL", "https://evil@api.flash.example/v1"), ("RTH_FLASH_MODEL", "provider-flash-id")])).is_err());
    }

    #[test]
    fn the_key_is_validated_and_never_printed() {
        let ep = endpoint("https://api.flash.example/v1", false).unwrap();
        assert!(hosted(&ep, None, "short", STALL).is_err());
        assert!(hosted(&ep, None, "has a space in it", STALL).is_err());
        let j = hosted(&ep, None, KEY, STALL).unwrap();
        let shown = format!("{:?}", local::Dial::Hosted(std::sync::Arc::new(j)));
        assert!(!shown.contains(KEY) && shown.contains("api.flash.example"), "{shown}");
        let cfg = config(KEY, "https://api.flash.example/v1", None).unwrap();
        assert!(!format!("{cfg:?}").contains(KEY));
        assert_eq!((cfg.authority.as_str(), cfg.model.as_str(), cfg.max_tokens), ("api.flash.example:443", MODEL, None));
    }

    #[tokio::test]
    async fn a_reply_streams_over_tls_with_the_bearer_and_the_bench_request_shape() {
        let events = vec![delta("Hi"), delta(" there."), json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 3}}).to_string()];
        let s = start(Answer::Stream { events, gap: Duration::from_millis(5), hang: false }).await;
        let cfg = config(KEY, &s.url, Some(ca())).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let done = local::stream_chat_metered(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap();
        drop(tx);
        let mut got = String::new();
        while let Some(t) = rx.recv().await {
            got += &t;
        }
        assert_eq!((got.as_str(), done.capped), ("Hi there.", false));
        assert_eq!(done.usage, Some(json!({"prompt_tokens": 12, "completion_tokens": 3})), "the usage event is kept for the ledger");
        assert_eq!(done.provider_model.as_deref(), Some("deepseek-ai/DeepSeek-V4.1-Flash-1017"));
        let raw = s.requests.lock().unwrap()[0].clone();
        assert!(raw.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"), "{raw}");
        assert!(raw.contains(&format!("\r\nAuthorization: Bearer {KEY}\r\n")) && raw.contains("\r\nHost: 127.0.0.1\r\n"));
        let b = body_of(&raw);
        assert_eq!((b["model"].as_str(), b["max_tokens"].as_u64(), b["stream"].as_bool()), (Some(MODEL), Some(REPLY_LIMIT as u64), Some(true)));

        assert_eq!((b["thinking"]["type"].as_str(), b["reasoning_effort"].as_str(), b.get("chat_template_kwargs"), b["stream_options"]["include_usage"].as_bool()), (Some("disabled"), Some("none"), None, Some(true)));
        assert!(b.get("cache_prompt").is_none() && b.get("id_slot").is_none() && b.get("grammar").is_none(), "no llama-only fields: {b}");
    }

    #[tokio::test]
    async fn a_certificate_the_roots_do_not_trust_is_refused_before_any_byte_is_sent() {
        let s = start(Answer::Json("{}".into())).await;
        let ep = endpoint(&s.url, true).unwrap();

        local::register_hosted(&ep.authority, hosted(&ep, None, KEY, STALL).unwrap());
        let cfg = local::Config { authority: ep.authority.clone(), base_path: ep.base_path.clone(), model: MODEL.into(), model_sha256: None, slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(8), n_ctx: N_CTX, first_token: FIRST_TOKEN, system_instruction: None, health_every: Duration::from_millis(500) };
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::UNAVAILABLE, "{e:?}");
        assert!(s.requests.lock().unwrap().is_empty(), "no request reached the server");
    }

    #[tokio::test]
    async fn a_started_stream_that_goes_quiet_fails_as_stalled_and_keeps_its_words() {
        let s = start(Answer::Stream { events: vec![delta("Partial answer ")], gap: Duration::ZERO, hang: true }).await;
        let ep = endpoint(&s.url, true).unwrap();
        local::register_hosted(&ep.authority, hosted(&ep, Some(ca()), KEY, Duration::from_millis(200)).unwrap());
        let cfg = local::Config { authority: ep.authority.clone(), base_path: ep.base_path.clone(), model: MODEL.into(), model_sha256: None, slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(64), n_ctx: N_CTX, first_token: FIRST_TOKEN, system_instruction: None, health_every: Duration::from_millis(500) };
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let t = tokio::time::Instant::now();
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::STREAM_STALLED, "{e:?}");
        assert!(t.elapsed() < Duration::from_secs(5), "bounded by the stall, not the 30 s idle read");
        drop(tx);
        let mut got = String::new();
        while let Some(t) = rx.recv().await {
            got += &t;
        }
        assert_eq!(got, "Partial answer ", "every word that arrived is kept");
    }

    fn routing_cfg(s: &test_server::Server, stall: Duration, first_token: Duration) -> local::Config {
        let ep = endpoint(&s.url, true).unwrap();
        local::register_hosted(&ep.authority, hosted(&ep, Some(ca()), KEY, stall).unwrap());
        local::Config { authority: ep.authority.clone(), base_path: ep.base_path.clone(), model: MODEL.into(), model_sha256: None, slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(64), n_ctx: N_CTX, first_token, system_instruction: None, health_every: Duration::from_millis(500) }
    }

    fn event(delta: Value) -> String {
        json!({"model": "deepseek-ai/DeepSeek-V4.1-Flash-1017", "choices": [{"delta": delta}]}).to_string()
    }

    #[tokio::test]
    async fn late_headers_then_late_text_time_out_at_the_first_token_bound() {
        let s = start(Answer::Delayed { head: Duration::from_millis(250), events: vec![delta("Late.")], gap: Duration::from_millis(250) }).await;
        let cfg = routing_cfg(&s, STALL, Duration::from_millis(400));
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let t = tokio::time::Instant::now();
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::FIRST_TOKEN_TIMEOUT, "{e:?}");
        assert!(t.elapsed() < Duration::from_millis(480), "at the bound, not after the text: {:?}", t.elapsed());
    }

    #[tokio::test]
    async fn a_late_http_error_with_a_dripping_body_fails_at_the_deadline() {
        let s = start(Answer::LateErrorDrip { head: Duration::from_millis(300), gap: Duration::from_millis(100), pieces: 20 }).await;
        let cfg = routing_cfg(&s, STALL, Duration::from_millis(600));
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let t = tokio::time::Instant::now();
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::UNAVAILABLE, "{e:?}");
        assert!(t.elapsed() < Duration::from_millis(750), "at the deadline, not after the body: {:?}", t.elapsed());
    }

    #[tokio::test]
    async fn an_http_error_body_is_read_for_at_most_two_seconds() {
        let s = start(Answer::LateErrorDrip { head: Duration::ZERO, gap: Duration::from_millis(200), pieces: 20 }).await;
        let cfg = routing_cfg(&s, STALL, Duration::from_secs(10));
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let t = tokio::time::Instant::now();
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::UNAVAILABLE, "{e:?}");
        assert!(t.elapsed() < Duration::from_millis(2300), "the body wait is capped: {:?}", t.elapsed());
    }

    #[tokio::test]
    async fn role_only_deltas_do_not_extend_the_first_token_bound() {
        let mut events = vec![event(json!({"role": "assistant"})); 8];
        events.push(delta("Late."));
        let s = start(Answer::Stream { events, gap: Duration::from_millis(100), hang: false }).await;
        let cfg = routing_cfg(&s, STALL, Duration::from_millis(400));
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::FIRST_TOKEN_TIMEOUT, "{e:?}");
    }

    #[tokio::test]
    async fn reasoning_counts_as_progress_past_the_first_token_bound() {
        let mut events = vec![event(json!({"reasoning_content": "hmm "})); 6];
        events.push(delta("Answer."));
        let s = start(Answer::Stream { events, gap: Duration::from_millis(150), hang: false }).await;
        let cfg = routing_cfg(&s, Duration::from_millis(400), Duration::from_millis(400));
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap();
        drop(tx);
        assert_eq!(rx.recv().await.as_deref(), Some("Answer."));
    }

    #[tokio::test]
    async fn metadata_after_text_does_not_keep_a_stalled_stream_alive() {
        let mut events = vec![delta("Partial ")];
        events.extend(std::iter::repeat_n("{}".to_string(), 12));
        events.push(delta("never."));
        let s = start(Answer::Stream { events, gap: Duration::from_millis(100), hang: false }).await;
        let cfg = routing_cfg(&s, Duration::from_millis(400), FIRST_TOKEN);
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let t = tokio::time::Instant::now();
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::STREAM_STALLED, "{e:?}");
        assert!(t.elapsed() < Duration::from_millis(900), "{:?}", t.elapsed());
    }

    #[tokio::test]
    async fn a_provider_error_keeps_only_its_status_and_category() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(KEY);
        let body = json!({"error": {"message": format!("bad key {KEY} ({b64})"), "type": "invalid_api_key", "code": KEY}}).to_string();
        let s = start(Answer::Status(401, body)).await;
        let cfg = routing_cfg(&s, STALL, FIRST_TOKEN);
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!((e.reason, e.detail.as_str()), (local::SERVER_ERROR, "HTTP 401: invalid_api_key"));
        let sse = json!({"error": {"message": format!("upstream says {KEY}"), "code": "weird"}}).to_string();
        let s = start(Answer::Stream { events: vec![delta("Half "), sse], gap: Duration::ZERO, hang: false }).await;
        let cfg = routing_cfg(&s, STALL, FIRST_TOKEN);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!((e.reason, e.detail.as_str()), (local::SERVER_ERROR, "provider error"));
        let ep = endpoint(&s.url, true).unwrap();
        let echoed = format!("x {KEY} y {b64} z");
        assert!(!local::dial(&ep.authority).redact(&echoed).contains(KEY) && !local::dial(&ep.authority).redact(&echoed).contains(&b64));
    }

    #[tokio::test]
    async fn no_first_token_within_the_budget_is_a_timeout() {
        let s = start(Answer::Stream { events: vec![], gap: Duration::ZERO, hang: true }).await;
        let mut cfg = config(KEY, &s.url, Some(ca())).unwrap();
        cfg.first_token = Duration::from_millis(300);
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let e = local::stream_chat(&cfg, vec![json!({"role": "user", "content": "hello"})], &tx, None).await.unwrap_err();
        assert_eq!(e.reason, local::FIRST_TOKEN_TIMEOUT, "{e:?}");
    }

    #[tokio::test]
    async fn memory_decide_on_flash_sends_the_schema_and_reads_the_answer_strictly() {
        let answer = json!({"choices": [{"message": {"content": "{\"d\":\"recall\",\"s\":[\"rm-1\"]}"}, "finish_reason": "stop"}]}).to_string();
        let s = start(Answer::Json(answer)).await;
        let cfg = config(KEY, &s.url, Some(ca())).unwrap();
        let offered = [Candidate { memory_id: "rm-1".into(), revision: 1, text: "my cafe is Lua".into() }];
        let run = crate::decide::run(&cfg, None, "what's my cafe?", &offered, DECIDE_DEADLINE).await;
        assert_eq!(run.outcome, Ok(crate::decide::Decision::Recall(vec![("rm-1".into(), 1)])));
        let b = body_of(&s.requests.lock().unwrap()[0]);
        assert_eq!(b["response_format"], crate::decide::schema(&crate::decide::offer(&offered)));
        assert_eq!(b["messages"][0]["content"].as_str(), Some(crate::decide::INSTRUCTIONS), "the frozen instructions, byte for byte");
        assert!(b.get("grammar").is_none() && b.get("cache_prompt").is_none() && b.get("id_slot").is_none(), "{b}");
        assert_eq!((b["temperature"].as_u64(), b["thinking"]["type"].as_str(), b["reasoning_effort"].as_str(), b.get("chat_template_kwargs")), (Some(0), Some("disabled"), Some("none"), None));
    }
}

#[cfg(test)]
mod live {
    use super::*;
    use crate::decide::{self, Candidate};
    use serde_json::{json, Value};
    use std::io::Write;

    const USD_PER_M: (f64, f64, f64) = (0.60, 2.40, 0.012);
    const STOP_USD: f64 = 0.05;

    const FAILED_REPLY_USD: f64 = (4000.0 * USD_PER_M.0 + crate::route::REPLY_ESTIMATE as f64 * USD_PER_M.1) / 1e6;
    const FAILED_DECIDE_USD: f64 = (4000.0 * USD_PER_M.0 + 64.0 * USD_PER_M.1) / 1e6;

    fn cost(u: Option<&Value>) -> (f64, Value) {
        let Some(u) = u else { return (0.0, Value::Null) };
        let n = |p: &str| u.pointer(p).and_then(Value::as_u64).unwrap_or(0);
        let (pt, ct) = (n("/prompt_tokens"), n("/completion_tokens"));
        let cached = n("/prompt_tokens_details/cached_tokens").max(n("/prompt_cache_hit_tokens"));
        let usd = (pt.saturating_sub(cached) as f64 * USD_PER_M.0 + cached as f64 * USD_PER_M.2 + ct as f64 * USD_PER_M.1) / 1e6;
        (usd, json!({"prompt": pt, "completion": ct, "cached": cached}))
    }

    #[tokio::test]
    #[ignore]
    async fn ttft_probe() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k}"));
        let key = var("RTH_FLASH_KEY");
        let ep = endpoint(&var("RTH_FLASH_URL"), false).unwrap();
        let j = hosted(&ep, None, &key, STALL).unwrap();
        let model = var("RTH_FLASH_MODEL");
        drop(key);
        let prompt = std::fs::read_to_string(var("RTH_TTFT_PROMPT")).unwrap();
        let mut ledger = std::fs::OpenOptions::new().create(true).append(true).open(var("RTH_FLASH_SMOKE_LEDGER")).unwrap();
        let run_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let body = serde_json::to_vec(&json!({"model": model, "messages": [{"role": "system", "content": prompt}, {"role": "user", "content": "hi"}], "stream": true, "stream_options": {"include_usage": true}, "max_tokens": 1, "temperature": 1.0, "top_p": 1.0, "chat_template_kwargs": {"enable_thinking": false}})).unwrap();

        let plan: Vec<(&str, u64)> = std::iter::once(("cold", 0)).chain((0..9).map(|_| ("warm", 0))).chain([("idle60", 60)]).chain((0..8).map(|_| ("warm", 0))).chain([("idle60", 60)]).collect();
        let mut total = 0.0;
        let mut rows = Vec::new();

        let pessimistic = (4000.0 * USD_PER_M.0 + USD_PER_M.1) / 1e6;
        for (n, (kind, idle)) in plan.iter().enumerate() {

            if total + pessimistic > STOP_USD {
                println!("ttft probe stopped before call {n}: ${total:.5} spent");
                break;
            }
            tokio::time::sleep(Duration::from_secs(*idle)).await;
            let t0 = std::time::Instant::now();
            let ms = |t: std::time::Instant| (t.duration_since(t0).as_secs_f64() * 1000.0).round();
            let mut marks: Vec<(&str, f64)> = Vec::new();
            let (mut usage, mut provider_model, mut status) = (None, None, String::new());
            let run = async {
                let addr = tokio::net::lookup_host(&ep.authority).await.map_err(|e| format!("dns: {e}"))?.next().ok_or("dns: no address")?;
                marks.push(("dns_ms", ms(std::time::Instant::now())));
                let tcp = tokio::net::TcpStream::connect(addr).await.map_err(|e| format!("tcp: {e}"))?;
                let _ = tcp.set_nodelay(true);
                marks.push(("tcp_ms", ms(std::time::Instant::now())));
                let tls = j.tls.connect(j.server.clone(), tcp).await.map_err(|e| format!("tls: {e}"))?;
                marks.push(("tls_ms", ms(std::time::Instant::now())));
                let mut r = BufReader::new(tls);
                let head = format!("POST {}/chat/completions HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {}\r\n\r\n", ep.base_path, ep.host, j.bearer, body.len());
                r.get_mut().write_all(head.as_bytes()).await.map_err(|e| format!("send: {e}"))?;
                r.get_mut().write_all(&body).await.map_err(|e| format!("send: {e}"))?;
                r.get_mut().flush().await.map_err(|e| format!("send: {e}"))?;
                marks.push(("sent_ms", ms(std::time::Instant::now())));
                let mut line = String::new();
                r.read_line(&mut line).await.map_err(|e| format!("status: {e}"))?;
                marks.push(("first_byte_ms", ms(std::time::Instant::now())));
                status = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut token = false;
                loop {
                    line.clear();
                    match tokio::time::timeout(Duration::from_secs(30), r.read_line(&mut line)).await {
                        Err(_) => return Err("no event within 30 s".to_string()),
                        Ok(Err(e)) => return Err(format!("read: {e}")),
                        Ok(Ok(0)) => break,
                        Ok(Ok(_)) => {}
                    }
                    let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else { continue };
                    if data == "[DONE]" {
                        break;
                    }
                    let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
                    if !token && v.pointer("/choices/0/delta/content").and_then(Value::as_str).is_some_and(|c| !c.is_empty()) {
                        token = true;
                        marks.push(("first_token_ms", ms(std::time::Instant::now())));
                    }
                    if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                        usage = Some(u.clone());
                    }
                    if let Some(m) = v.get("model").and_then(Value::as_str) {
                        provider_model = Some(m.to_string());
                    }
                }
                Ok::<(), String>(())
            };
            let error = tokio::time::timeout(Duration::from_secs(60), run).await.map_err(|_| "the call took over 60 s".to_string()).and_then(|r| r).err();
            let (usd, tokens) = cost(usage.as_ref());
            let usd = if usage.is_some() { usd } else { pessimistic };
            total += usd;
            let mut row = json!({"n": n, "kind": kind, "status": status, "error": error, "tokens": tokens, "provider_model": provider_model});
            for (k, t) in &marks {
                row[*k] = json!(t);
            }
            println!("ttft {n} {kind}: {marks:?} error {error:?} | ${usd:.6}");
            writeln!(ledger, "{}", json!({"run_ts": run_ts, "host": ep.host, "model": model, "kind": "ttft_probe", "row": row, "cost_usd": usd, "run_total_usd": total})).unwrap();
            rows.push(row);
        }
        std::fs::write(var("RTH_TTFT_OUT"), serde_json::to_string_pretty(&rows).unwrap()).unwrap();
        println!("ttft probe total ${total:.6}");
    }

    #[tokio::test]
    #[ignore]
    async fn smoke() {
        let (Ok(key), Ok(ledger)) = (std::env::var("RTH_FLASH_KEY"), std::env::var("RTH_FLASH_SMOKE_LEDGER")) else { panic!("set RTH_FLASH_KEY and RTH_FLASH_SMOKE_LEDGER") };
        let cfg = config_from(&|k| std::env::var(k).ok()).unwrap().expect("set RTH_FLASH_URL and RTH_FLASH_MODEL");
        drop(key);
        let (host, model) = (cfg.authority.clone(), cfg.model.clone());
        let mut out = std::fs::OpenOptions::new().create(true).append(true).open(&ledger).unwrap();
        let run_ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let mut total = 0.0;
        let mut row = |r: Value, usd: f64| {
            total += usd;
            writeln!(out, "{}", json!({"run_ts": run_ts, "host": host, "model": model, "row": r, "cost_usd": usd, "run_total_usd": total})).unwrap();
            out.flush().unwrap();
            assert!(total <= STOP_USD, "smoke stopped at ${total:.5}");
            total
        };
        let system = local::system_prompt_route(DISPLAY_NAME, true, true, crate::device::this());
        let replies = ["hi", "who are you?", "what model do you run on?", "do you have memory?", "can you set a timer for 10 minutes?", "explain what a mortgage is", "give me three tips for sleeping better", "oi, tudo bem?", "quem te fez?", "me explica o que é inflação"];
        for (n, text) in replies.iter().enumerate() {
            let mut msgs = vec![json!({"role": "system", "content": system})];
            if let Some(lang) = local::detect_language(text) {
                msgs.push(json!({"role": "system", "content": local::language_hint(lang)}));
            }
            msgs.push(json!({"role": "user", "content": text}));
            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);
            let t = tokio::time::Instant::now();
            let reader = tokio::spawn(async move {
                let (mut first, mut got) = (None, String::new());
                while let Some(p) = rx.recv().await {
                    first.get_or_insert(t.elapsed().as_millis() as u64);
                    got += &p;
                }
                (first, got)
            });
            let r = local::stream_chat_metered(&cfg, msgs, &tx, None).await;
            drop(tx);
            let (ttft, got) = reader.await.unwrap();
            let total_ms = t.elapsed().as_millis() as u64;
            let (usd, rec) = match &r {
                Ok(s) => {
                    let (usd, usage) = cost(s.usage.as_ref());
                    let markdown = got.lines().any(|l| l.trim_start().starts_with('#') || l.trim_start().starts_with("- ") || l.trim_start().starts_with("* ")) || got.contains("**");
                    (usd, json!({"kind": "reply", "n": n, "input": text, "outcome": "ok", "capped": s.capped, "ttft_ms": ttft, "total_ms": total_ms, "words": got.split_whitespace().count(), "markdown": markdown, "provider_model": s.provider_model, "usage": usage, "text": got.chars().take(400).collect::<String>()}))
                }
                Err(f) => (FAILED_REPLY_USD, json!({"kind": "reply", "n": n, "input": text, "outcome": f.reason, "detail": f.detail, "ttft_ms": ttft, "total_ms": total_ms})),
            };
            println!("reply {n}: {} ttft={ttft:?} ms total={total_ms} ms usd={usd:.6}", rec["outcome"]);
            row(rec, usd);
        }
        let lua = Candidate { memory_id: "rm-1".into(), revision: 1, text: "my favourite café is Lua".into() };
        let gym = Candidate { memory_id: "rm-2".into(), revision: 1, text: "my gym is Iron Box".into() };
        let cases: Vec<(&str, Vec<Candidate>, &str)> = vec![
            ("remember that my favourite café is Lua", vec![], "remember"),
            ("what's my favourite café?", vec![lua.clone()], "recall"),
            ("actually it's Sol now", vec![lua.clone()], "correct"),
            ("where do I train?", vec![gym.clone(), lua.clone()], "recall"),
            ("hi there", vec![], "no_op"),
            ("remind me to call Ana tomorrow", vec![lua.clone()], "no_op"),
            ("what's my dentist's name?", vec![lua.clone(), gym.clone()], "no_op"),
            ("lembra que meu café favorito é o Lua", vec![], "remember"),
            ("qual é a minha academia?", vec![gym.clone()], "recall"),
            ("no, my gym is Steel Works now", vec![gym.clone()], "correct"),
        ];
        let path = format!("{}/chat/completions", cfg.base_path);
        for (n, (u, offered, want)) in cases.into_iter().enumerate() {
            let offered = decide::offer(&offered);
            let t = tokio::time::Instant::now();
            let r = local::post_json(&cfg.authority, &path, &decide::body_flash(&cfg.model, u, &offered), DECIDE_DEADLINE).await;
            let ms = t.elapsed().as_millis() as u64;
            let (usd, rec) = match r {
                Ok((status, raw)) => {
                    let v: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
                    let (usd, usage) = cost(v.get("usage"));
                    let answer = v.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or("").to_string();
                    let got = decide::decide_from(answer.trim(), u, &offered).map(|d| d.name().to_string());
                    let matched = got.as_deref() == Ok(want);
                    (usd, json!({"kind": "decide", "n": n, "input": u, "status": status, "ms": ms, "answer": answer, "decision": got.as_ref().ok(), "invalid": got.as_ref().err(), "want": want, "match": matched, "provider_model": v.get("model"), "usage": usage}))
                }
                Err(f) => (FAILED_DECIDE_USD, json!({"kind": "decide", "n": n, "input": u, "outcome": f.reason, "detail": f.detail, "ms": ms})),
            };
            println!("decide {n}: {} want={want} ms={ms} usd={usd:.6}", rec["decision"]);
            row(rec, usd);
        }
        println!("smoke total ${total:.6}");
    }
}

#[cfg(test)]
mod regression_neighbors {
    use super::*;
    use super::test_server::{start, Answer, ca};
    use serde_json::json;

    #[tokio::test]
    async fn regression_http_error_body_obeys_the_absolute_deadline() {
        let s = start(Answer::ErrorDrip { gap: Duration::from_millis(500), pieces: 12 }).await;
        let cfg = config("fixture-only-not-a-real-key",&s.url,Some(ca())).unwrap();
        let (tx,_rx)=tokio::sync::mpsc::channel(4);
        let t=std::time::Instant::now();
        let result=local::stream_chat_metered(&cfg,vec![json!({"role":"user","content":"hello"})],&tx,None).await;
        let elapsed=t.elapsed();
        println!("regression regression error-body: budget_ms={} elapsed_ms={} returned_error={}",cfg.first_token.as_millis(),elapsed.as_millis(),result.is_err());
        assert!(result.is_err(),"the 503 must fail");
        assert!(elapsed<cfg.first_token+Duration::from_millis(500),"a drip-fed HTTP error must not delay fallback beyond the absolute deadline");
    }

    #[tokio::test]
    async fn regression_http_structured_error_still_redacts_the_active_bearer() {

        let key=(12_000_000u64+std::process::id() as u64 % 1_000_000).to_string();
        assert!(key_ok(&key));
        let body=json!({"error":{"type":"exceed_context_size_error","n_prompt_tokens":key.parse::<u64>().unwrap(),"n_ctx":4096}}).to_string();
        let s=start(Answer::Status(400,body)).await;
        let cfg=config(&key,&s.url,Some(ca())).unwrap();
        let (tx,_rx)=tokio::sync::mpsc::channel(4);
        let error=local::stream_chat_metered(&cfg,vec![json!({"role":"user","content":"hello"})],&tx,None).await.unwrap_err();
        let retained=error.detail.contains(&key);
        println!("regression regression structured-error: active_bearer_retained={retained}");
        assert!(!retained,"HTTP early-return failures must pass through the same bearer redaction as SSE failures");
    }
}
