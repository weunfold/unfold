

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rth_tools::grants::{Grants, Line, Request};
use rth_tools::host::{Approvals, Decision, Events, Task, TaskTools, ToolHost};
use rth_tools::web::{BoxFuture, Connect, Resolve};
use rth_tools::worker::{self, Call, End, Model, ModelTurn};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

const D2_ANSWER: &str = "When you know the answer, end with one final line: ANSWER: <the answer only>.";
const OVER: usize = 24_000;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

struct Fixture(PathBuf);

impl Approvals for Fixture {
    fn folder<'a>(&'a self, _task_id: &'a str, request: &'a Request) -> BoxFuture<'a, Decision> {
        let ok = request.root.starts_with(&self.0);
        Box::pin(async move { if ok { Decision::Approved { effect_id: "bench-tap".into(), args_sha256: "bench".into() } } else { Decision::Declined("person".into()) } })
    }
    fn fetch<'a>(&'a self, _t: &'a str, _u: &'a str, _w: &'a str) -> BoxFuture<'a, Decision> {
        Box::pin(async { Decision::Declined("person".into()) })
    }
}

struct Quiet;
impl Events for Quiet {
    fn grant_event(&self, _line: &Line) {}
}

struct NoNet;
impl Resolve for NoNet {
    fn resolve<'a>(&'a self, _h: &'a str, _p: u16) -> BoxFuture<'a, std::io::Result<Vec<std::net::SocketAddr>>> {
        Box::pin(async { Err(std::io::Error::other("web is off in D2")) })
    }
}
impl Connect for NoNet {
    fn connect<'a>(&'a self, _a: std::net::SocketAddr, _h: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn rth_tools::web::Stream>>> {
        Box::pin(async { Err(std::io::Error::other("web is off in D2")) })
    }
}

struct Endpoint {
    tls: bool,
    host: String,
    port: u16,
    path: String,
    model: String,
    key: Option<String>,
    max_tokens: u32,
    connector: Option<tokio_rustls::TlsConnector>,
}

impl Endpoint {
    fn parse(url: &str, model: &str, key: Option<String>, max_tokens: u32) -> Result<Endpoint, String> {
        let u = url::Url::parse(url).map_err(|e| e.to_string())?;
        let tls = match u.scheme() {
            "https" => true,
            "http" if matches!(u.host_str(), Some("127.0.0.1") | Some("localhost")) => false,
            _ => return Err("--url is https://… or http://127.0.0.1:<port>/v1".into()),
        };
        let connector = tls.then(|| {
            let mut roots = rustls::RootCertStore::empty();
            roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
            let _ = rustls::crypto::ring::default_provider().install_default();
            tokio_rustls::TlsConnector::from(Arc::new(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth()))
        });
        Ok(Endpoint {
            tls,
            host: u.host_str().unwrap_or_default().to_string(),
            port: u.port_or_known_default().unwrap_or(443),
            path: format!("{}/chat/completions", u.path().trim_end_matches('/')),
            model: model.to_string(),
            key,
            max_tokens,
            connector,
        })
    }

    async fn post(&self, body: &Value) -> Result<Value, String> {
        let bytes = body.to_string();
        let mut head = format!("POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", self.path, self.host, bytes.len());
        if let Some(k) = &self.key {
            head.push_str(&format!("Authorization: Bearer {k}\r\n"));
        }
        head.push_str("\r\n");
        let tcp = tokio::net::TcpStream::connect((self.host.as_str(), self.port)).await.map_err(|e| e.to_string())?;
        let mut stream: Box<dyn rth_tools::web::Stream> = if self.tls {
            let name = rustls_pki_types::ServerName::try_from(self.host.clone()).map_err(|e| e.to_string())?;
            Box::new(self.connector.as_ref().unwrap().connect(name, tcp).await.map_err(|e| e.to_string())?)
        } else {
            Box::new(tcp)
        };
        stream.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
        stream.write_all(bytes.as_bytes()).await.map_err(|e| e.to_string())?;
        let mut r = tokio::io::BufReader::new(stream);
        let mut status = String::new();
        r.read_line(&mut status).await.map_err(|e| e.to_string())?;
        let (mut len, mut chunked) = (None, false);
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).await.map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
                break;
            }
            let l = h.to_ascii_lowercase();
            if let Some(v) = l.strip_prefix("content-length:") {
                len = v.trim().parse::<usize>().ok();
            }
            if l.starts_with("transfer-encoding:") && l.contains("chunked") {
                chunked = true;
            }
        }
        let mut body = Vec::new();
        if chunked {
            loop {
                let mut size = String::new();
                r.read_line(&mut size).await.map_err(|e| e.to_string())?;
                let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                let mut c = vec![0u8; n + 2];
                r.read_exact(&mut c).await.map_err(|e| e.to_string())?;
                body.extend_from_slice(&c[..n]);
            }
        } else if let Some(n) = len {
            body.resize(n, 0);
            r.read_exact(&mut body).await.map_err(|e| e.to_string())?;
        } else {
            r.read_to_end(&mut body).await.map_err(|e| e.to_string())?;
        }
        if !status.contains(" 200 ") {
            let text = String::from_utf8_lossy(&body);
            let text = match &self.key {
                Some(k) => text.replace(k.as_str(), "[redacted]"),
                None => text.into_owned(),
            };
            return Err(format!("{} {}", status.trim(), &text[..text.len().min(300)]));
        }
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }
}

impl Model for Endpoint {
    fn turn<'a>(&'a self, messages: &'a [Value], tools: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>> {
        Box::pin(async move {
            let mut body = json!({"model": self.model, "messages": messages, "stream": false, "max_tokens": self.max_tokens, "temperature": 0});
            if tools.as_array().is_some_and(|a| !a.is_empty()) {
                body["tools"] = tools.clone();
                body["tool_choice"] = json!("auto");
            }
            if self.tls {
                body["chat_template_kwargs"] = json!({"enable_thinking": false});
            }
            let mut last = String::new();
            for _ in 0..2 {
                match tokio::time::timeout(Duration::from_secs(120), self.post(&body)).await {
                    Ok(Ok(v)) => return parse(&v),
                    Ok(Err(e)) => last = e,
                    Err(_) => last = "timeout".into(),
                }
            }
            Err(last)
        })
    }
}

fn parse(v: &Value) -> Result<ModelTurn, String> {
    let c = &v["choices"][0];
    if c.is_null() {
        return Err("no choice".into());
    }
    let calls = c["message"]["tool_calls"]
        .as_array()
        .map(|a| a.iter().map(|t| Call { id: t["id"].as_str().unwrap_or_default().into(), name: t["function"]["name"].as_str().unwrap_or_default().into(), arguments: t["function"]["arguments"].as_str().map(str::to_string).unwrap_or_else(|| t["function"]["arguments"].to_string()) }).collect())
        .unwrap_or_default();
    Ok(ModelTurn { content: c["message"]["content"].as_str().unwrap_or_default().into(), tool_calls: calls, finish: c["finish_reason"].as_str().unwrap_or("stop").into(), usage: v.get("usage").cloned() })
}

struct Dry {
    evidence: String,
    gold: String,
    n: Mutex<u32>,
}

impl Model for Dry {
    fn turn<'a>(&'a self, _m: &'a [Value], _t: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>> {
        let n = {
            let mut n = self.n.lock().unwrap();
            *n += 1;
            *n
        };
        let call = |name: &str, args: Value| ModelTurn { content: String::new(), tool_calls: vec![Call { id: format!("d{n}"), name: name.into(), arguments: args.to_string() }], finish: "tool_calls".into(), usage: None };
        let t = match n {
            1 => call("files_search", json!({"path": ".", "query": ""})),
            2 => call("files_read", json!({"path": self.evidence})),
            _ => ModelTurn { content: format!("ANSWER: {}", self.gold), tool_calls: vec![], finish: "stop".into(), usage: None },
        };
        Box::pin(async move { Ok(t) })
    }
}

fn norm(s: &str) -> String {
    let folded: String = s.chars().map(|c| match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        '–' | '—' => '-',
        c => c,
    }).collect();
    folded.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has(text: &str, alt: &str) -> bool {
    let a = norm(alt);
    if a.is_empty() {
        return false;
    }
    let t = norm(text);
    let mut from = 0;
    while let Some(i) = t[from..].find(&a) {
        let s = from + i;
        let e = s + a.len();
        let pre_ok = !a.chars().next().unwrap().is_alphanumeric() || t[..s].chars().last().is_none_or(|c| !c.is_ascii_alphanumeric());
        let post_ok = !a.chars().last().unwrap().is_ascii_digit() || t[e..].chars().next().is_none_or(|c| !c.is_ascii_digit());
        if pre_ok && post_ok {
            return true;
        }
        from = s + a.chars().next().unwrap().len_utf8();
    }
    false
}

fn answer_line(text: &str) -> Option<String> {
    text.trim().lines().rev().find_map(|l| {
        let l = l.trim().trim_start_matches(|c: char| !c.is_alphanumeric());
        let low = l.to_lowercase();
        low.starts_with("answer").then(|| l[6..].trim_start_matches(|c: char| !c.is_alphanumeric() && c != ':').trim_start_matches(':').trim().to_string())
    })
}

fn folder_chars(dir: &Path) -> usize {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(t) = std::fs::read_to_string(&p) {
                n += t.chars().count();
            }
        }
    }
    n
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tasks = PathBuf::from(arg(&args, "--tasks").expect("--tasks"));
    let out = PathBuf::from(arg(&args, "--out").expect("--out"));
    let split = arg(&args, "--split").unwrap_or_else(|| "dev".into());
    let dry = args.iter().any(|a| a == "--dry");
    if split == "gate" && std::env::var("GATE_OK").as_deref() != Ok("1") {
        eprintln!("d2: the gate split needs GATE_OK=1 (Ana's OK)");
        std::process::exit(2);
    }
    let only: Option<HashSet<String>> = arg(&args, "--only").map(|s| s.split(',').map(str::to_string).collect());
    std::fs::create_dir_all(&out).unwrap();
    let endpoint = if dry {
        None
    } else {
        let key = arg(&args, "--key-env").and_then(|k| std::env::var(k).ok()).filter(|k| !k.is_empty());
        let max_tokens = arg(&args, "--max-tokens").and_then(|v| v.parse().ok()).unwrap_or(512);
        Some(Arc::new(Endpoint::parse(&arg(&args, "--url").expect("--url"), &arg(&args, "--model").expect("--model"), key, max_tokens).expect("endpoint")))
    };
    let results = out.join("results.jsonl");
    let done: HashSet<String> = std::fs::read_to_string(&results).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| v["id"].as_str().map(str::to_string)).collect();
    let mut w = std::fs::OpenOptions::new().create(true).append(true).open(&results).unwrap();
    let items: Vec<Value> = std::fs::read_to_string(tasks.join("d2.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    for it in items.iter().filter(|i| i["split"] == split.as_str()) {
        let id = it["id"].as_str().unwrap().to_string();
        if done.contains(&id) || only.as_ref().is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        let dir = std::fs::canonicalize(tasks.join(it["dir"].as_str().unwrap())).unwrap();
        let chars = folder_chars(&dir);
        let work = out.join("work").join(&id);
        std::fs::create_dir_all(&work).unwrap();
        let (grants, _) = Grants::open(&work.join("grants.jsonl"), "bench").unwrap();
        let receipts = Arc::new(rth_tools::receipt::Memory::default());
        let host = Arc::new(ToolHost {
            profile: rth_tools::profile::research(),
            grants: Arc::new(Mutex::new(grants)),
            blocked: Vec::new(),
            home: None,
            resolver: Arc::new(NoNet),
            connector: Arc::new(NoNet),
            fetch: rth_tools::web::FetchConfig::default(),
            approvals: Arc::new(Fixture(dir.clone())),
            recall: None,
            receipts: receipts.clone(),
            events: Arc::new(Quiet),
            guard: None,
            parts: None,
        });
        let text = format!("Question: {}\n{D2_ANSWER}", it["question"].as_str().unwrap());
        let tools = TaskTools::new(host, Task { task_id: id.clone(), text, base: Some(dir.clone()) });
        let gold = it["gold"].as_str().unwrap().to_string();
        let model: Arc<dyn Model> = match &endpoint {
            Some(e) => e.clone(),
            None => Arc::new(Dry { evidence: it["evidence"][0].as_str().unwrap().to_string(), gold: gold.clone(), n: Mutex::new(0) }),
        };
        let (_tx, mut rx) = tokio::sync::watch::channel(false);
        let t0 = Instant::now();
        let rep = worker::run(&tools, model.as_ref(), &mut rx).await;
        let wall_ms = t0.elapsed().as_millis() as u64;
        let (final_text, end) = match &rep.end {
            End::Returned { text } => (text.clone(), "returned".to_string()),
            End::Failed { reason } => (String::new(), format!("failed:{reason}")),
            End::Cancelled => (String::new(), "cancelled".into()),
        };
        let golds: Vec<String> = std::iter::once(gold.clone()).chain(it["aliases"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string))).collect();
        let ans = answer_line(&final_text);
        let strict = ans.as_ref().is_some_and(|a| golds.iter().any(|g| has(a, g)));
        let tokens_in: u64 = rep.usage.iter().filter_map(|u| u["prompt_tokens"].as_u64()).sum();
        let tokens_out: u64 = rep.usage.iter().filter_map(|u| u["completion_tokens"].as_u64()).sum();
        let row = json!({
            "task": "d2", "id": id, "split": split, "arm": "s2", "backend": if dry { "fake" } else { "model" },
            "model": endpoint.as_ref().map(|e| e.model.clone()), "profile_sha256": rth_tools::profile::research().sha256,
            "bucket": if chars > OVER { "over_24k" } else { "under_24k" }, "folder_chars": chars,
            "end": end, "final_text": final_text, "answer": ans, "strict": strict,
            "turns": rep.turns, "tool_calls": rep.tool_calls, "invalid_args": rep.invalid_args, "loop_steers": rep.loop_steers, "max_turn_hit": rep.max_turn_hit,
            "receipts": receipts.all().len(), "tokens_in": tokens_in, "tokens_out": tokens_out, "wall_ms": wall_ms,
        });
        writeln!(w, "{row}").unwrap();
        w.flush().unwrap();
        println!("{id} {} strict={strict} turns={} calls={} {}", row["bucket"].as_str().unwrap(), rep.turns, rep.tool_calls, row["end"].as_str().unwrap());
    }

    let rows: Vec<Value> = std::fs::read_to_string(&results).unwrap().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|r| r["split"] == split.as_str()).collect();
    let acc = |f: &dyn Fn(&Value) -> bool| {
        let sel: Vec<&Value> = rows.iter().filter(|r| f(r)).collect();
        (sel.len(), sel.iter().filter(|r| r["strict"] == true).count())
    };
    let (n, ok) = acc(&|_| true);
    let (n_over, ok_over) = acc(&|r| r["bucket"] == "over_24k");
    let calls: u64 = rows.iter().filter_map(|r| r["tool_calls"].as_u64()).sum();
    let invalid: u64 = rows.iter().filter_map(|r| r["invalid_args"].as_u64()).sum();
    let ratio = |a: usize, b: usize| if b == 0 { Value::Null } else { json!(a as f64 / b as f64) };
    let score = json!({
        "split": split, "rows": n, "accuracy": ratio(ok, n), "over_24k": {"rows": n_over, "accuracy": ratio(ok_over, n_over)},
        "invalid_args_rate": if calls == 0 { Value::Null } else { json!(invalid as f64 / calls as f64) },
        "max_turn_hits": ratio(rows.iter().filter(|r| r["max_turn_hit"] == true).count(), n),
        "loop_steer_rows": ratio(rows.iter().filter(|r| r["loop_steers"].as_u64().unwrap_or(0) > 0).count(), n),
        "fake_rows": rows.iter().filter(|r| r["backend"] == "fake").count(),
        "pass_line": "accuracy >= 0.80 overall and >= 0.80 over 24k; invalid args <= 2%; max-turn hits <= 5%; loop steers <= 5% (proposed)",
    });
    std::fs::write(out.join("score.json"), serde_json::to_string_pretty(&score).unwrap()).unwrap();
    println!("{score}");
}
