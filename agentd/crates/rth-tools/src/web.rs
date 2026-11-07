

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

pub trait Resolve: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, std::io::Result<Vec<SocketAddr>>>;
}

pub trait Connect: Send + Sync {
    fn connect<'a>(&'a self, addr: SocketAddr, host: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn Stream>>>;
}

pub struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, std::io::Result<Vec<SocketAddr>>> {
        Box::pin(async move { Ok(tokio::net::lookup_host((host, port)).await?.collect()) })
    }
}

pub struct TlsConnect {
    tls: tokio_rustls::TlsConnector,
}

impl TlsConnect {

    pub fn system() -> std::io::Result<TlsConnect> {
        let mut roots = rustls::RootCertStore::empty();
        let (added, _) = roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
        if added == 0 {
            return Err(std::io::Error::other("no system trust anchors found"));
        }
        Ok(TlsConnect::from_config(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth()))
    }

    pub fn from_config(config: rustls::ClientConfig) -> TlsConnect {
        let _ = rustls::crypto::ring::default_provider().install_default();
        TlsConnect { tls: tokio_rustls::TlsConnector::from(Arc::new(config)) }
    }
}

impl Connect for TlsConnect {
    fn connect<'a>(&'a self, addr: SocketAddr, host: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn Stream>>> {
        Box::pin(async move {
            let name = rustls_pki_types::ServerName::try_from(host.to_string()).map_err(|e| std::io::Error::other(e.to_string()))?;
            let tcp = tokio::net::TcpStream::connect(addr).await?;
            let tls = self.tls.connect(name, tcp).await?;
            Ok(Box::new(tls) as Box<dyn Stream>)
        })
    }
}

pub type Vet = Arc<dyn Fn(&str, IpAddr) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct FetchConfig {
    pub max_bytes: usize,
    pub timeout: Duration,
    pub max_redirects: usize,
    pub vet: Vet,
}

impl Default for FetchConfig {
    fn default() -> Self {
        FetchConfig { max_bytes: 2 * 1024 * 1024, timeout: Duration::from_secs(15), max_redirects: 5, vet: Arc::new(|_, ip| is_public(ip)) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fetched {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: String,

    pub text: String,

    pub bytes: usize,

    pub complete: bool,
    pub redirects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebError {
    Invalid(String),
    NotPublic(String),
    Dns(String),
    Connect(String),
    Timeout,
    TooManyRedirects,
    UnsupportedType(String),
    Protocol(String),
}

impl WebError {
    pub fn reason(&self) -> &'static str {
        match self {
            WebError::Invalid(_) => "invalid_url",
            WebError::NotPublic(_) => "not_public",
            WebError::Dns(_) => "dns_failed",
            WebError::Connect(_) => "connect_failed",
            WebError::Timeout => "timeout",
            WebError::TooManyRedirects => "too_many_redirects",
            WebError::UnsupportedType(_) => "unsupported_type",
            WebError::Protocol(_) => "protocol_error",
        }
    }
}

pub fn vet_url(raw: &str) -> Result<url::Url, WebError> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 4096 {
        return Err(WebError::Invalid("an address is 1-4096 characters".into()));
    }
    let mut u = url::Url::parse(raw).map_err(|e| WebError::Invalid(e.to_string()))?;
    if u.scheme() != "https" {
        return Err(WebError::Invalid(format!("only https is fetched, not {}", u.scheme())));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(WebError::Invalid("an address with a user or password is not fetched".into()));
    }
    match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            let local = d == "localhost" || [".localhost", ".local", ".internal", ".lan", ".home.arpa", ".onion"].iter().any(|s| d.ends_with(s)) || !d.contains('.');
            if local {
                return Err(WebError::Invalid(format!("{d} is a local name")));
            }
        }
        Some(_) => return Err(WebError::Invalid("an IP address is not fetched; use a name".into())),
        None => return Err(WebError::Invalid("no host".into())),
    }
    u.set_fragment(None);
    Ok(u)
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();

            if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
                let v4 = std::net::Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
                return is_public(IpAddr::V4(v4));
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)

                || (s[0] & 0xffc0) == 0xfec0
                || s[0] == 0x2002
                || (s[0] == 0x2001 && s[1] == 0)
                || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1)
                || s[0..6].iter().all(|x| *x == 0))
        }
    }
}

pub async fn fetch(raw: &str, resolver: &dyn Resolve, connector: &dyn Connect, cfg: &FetchConfig) -> Result<Fetched, WebError> {
    match tokio::time::timeout(cfg.timeout, fetch_inner(raw, resolver, connector, cfg)).await {
        Ok(r) => r,
        Err(_) => Err(WebError::Timeout),
    }
}

async fn fetch_inner(raw: &str, resolver: &dyn Resolve, connector: &dyn Connect, cfg: &FetchConfig) -> Result<Fetched, WebError> {
    let mut url = vet_url(raw)?;
    let requested = url.to_string();
    let mut redirects = Vec::new();
    loop {
        let host = url.host_str().unwrap_or_default().to_string();
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs = resolver.resolve(&host, port).await.map_err(|e| WebError::Dns(e.to_string()))?;
        if addrs.is_empty() {
            return Err(WebError::Dns(format!("{host} has no address")));
        }
        if addrs.iter().any(|a| !(cfg.vet)(&host, a.ip())) {
            return Err(WebError::NotPublic(host));
        }
        let stream = connector.connect(addrs[0], &host).await.map_err(|e| WebError::Connect(e.to_string()))?;
        let mut target = url.path().to_string();
        if let Some(q) = url.query() {
            target.push('?');
            target.push_str(q);
        }
        let mut r = tokio::io::BufReader::new(stream);
        let req = format!(
            "GET {target} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Unfold-research/1\r\nAccept: text/html, text/plain, application/json;q=0.9, */*;q=0.1\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            host_header(&url)
        );
        r.get_mut().write_all(req.as_bytes()).await.map_err(|e| WebError::Connect(e.to_string()))?;
        let (status, headers) = read_head(&mut r).await?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let Some(loc) = headers.get("location") else { return Err(WebError::Protocol(format!("{status} without a Location"))) };
            if redirects.len() >= cfg.max_redirects {
                return Err(WebError::TooManyRedirects);
            }
            let next = url.join(loc).map_err(|e| WebError::Invalid(e.to_string()))?;
            url = vet_url(next.as_str())?;
            redirects.push(url.to_string());
            continue;
        }
        if let Some(enc) = headers.get("content-encoding").filter(|e| !e.eq_ignore_ascii_case("identity")) {
            return Err(WebError::Protocol(format!("content encoding {enc} was not asked for")));
        }
        let content_type = headers.get("content-type").cloned().unwrap_or_else(|| "text/plain".into());
        let ct = content_type.to_ascii_lowercase();
        let html = ct.contains("html");
        if !(ct.starts_with("text/") || ct.contains("json") || ct.contains("xml")) {
            return Err(WebError::UnsupportedType(content_type));
        }
        let (body, complete) = read_body(&mut r, &headers, cfg.max_bytes).await?;
        let text = String::from_utf8_lossy(&body).into_owned();
        let text = if html { html_to_text(&text) } else { text };
        return Ok(Fetched { url: requested, final_url: url.to_string(), status, content_type, text, bytes: body.len(), complete, redirects });
    }
}

fn host_header(u: &url::Url) -> String {
    let host = u.host_str().unwrap_or_default();
    match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    }
}

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

const MAX_HEAD: usize = 64 * 1024;

async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, budget: &mut usize) -> Result<String, WebError> {
    let mut line = Vec::new();
    let n = r.take(*budget as u64 + 1).read_until(b'\n', &mut line).await.map_err(|e| WebError::Protocol(e.to_string()))?;
    if n > *budget {
        return Err(WebError::Protocol("response head too large".into()));
    }
    *budget -= n;
    if n == 0 {
        return Err(WebError::Protocol("connection closed before the response".into()));
    }
    Ok(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string())
}

async fn read_head<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R) -> Result<(u16, std::collections::HashMap<String, String>), WebError> {
    let mut budget = MAX_HEAD;
    let status_line = read_line(r, &mut budget).await?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or_default();
    let status: u16 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| WebError::Protocol(format!("bad status line {status_line:?}")))?;
    if !version.starts_with("HTTP/1.") {
        return Err(WebError::Protocol(format!("bad status line {status_line:?}")));
    }
    let mut headers = std::collections::HashMap::new();
    loop {
        let line = read_line(r, &mut budget).await?;
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    Ok((status, headers))
}

async fn read_body<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, headers: &std::collections::HashMap<String, String>, cap: usize) -> Result<(Vec<u8>, bool), WebError> {
    let io = |e: std::io::Error| WebError::Protocol(e.to_string());
    let mut body = Vec::new();
    if headers.get("transfer-encoding").is_some_and(|t| t.to_ascii_lowercase().contains("chunked")) {
        loop {
            let mut budget = 1024;
            let size_line = read_line(r, &mut budget).await?;
            let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| WebError::Protocol("bad chunk size".into()))?;
            if size == 0 {
                return Ok((body, true));
            }
            let room = cap - body.len();
            let take = size.min(room);
            let mut chunk = vec![0u8; take];
            r.read_exact(&mut chunk).await.map_err(io)?;
            body.extend_from_slice(&chunk);
            if take < size {
                return Ok((body, false));
            }
            let mut crlf = [0u8; 2];
            r.read_exact(&mut crlf).await.map_err(io)?;
        }
    }
    let limit = match headers.get("content-length").and_then(|v| v.parse::<usize>().ok()) {
        Some(n) => n,
        None => usize::MAX,
    };
    let want = limit.min(cap);
    let mut taken = r.take(want as u64);
    taken.read_to_end(&mut body).await.map_err(io)?;
    let complete = if limit == usize::MAX {

        if body.len() < cap {
            true
        } else {
            let mut probe = [0u8; 1];
            matches!(r.read(&mut probe).await, Ok(0))
        }
    } else {
        body.len() == limit
    };
    Ok((body, complete))
}

pub fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {

            let mut skipped = false;
            for tag in ["script", "style", "noscript", "template", "svg"] {
                if lower[i + 1..].starts_with(tag) {
                    let close = format!("</{tag}");
                    i = match lower[i..].find(&close) {
                        Some(j) => i + j + close.len(),
                        None => bytes.len(),
                    };
                    i = lower[i..].find('>').map(|j| i + j + 1).unwrap_or(bytes.len());
                    skipped = true;
                    break;
                }
            }
            if skipped {
                out.push(' ');
                continue;
            }
            let end = lower[i..].find('>').map(|j| i + j + 1).unwrap_or(bytes.len());
            let tag = &lower[i..end];
            let block = ["<p", "</p", "<br", "<div", "</div", "<li", "<h1", "<h2", "<h3", "<h4", "<tr", "</tr", "<section", "<article", "</h"].iter().any(|b| tag.starts_with(b));
            out.push(if block { '\n' } else { ' ' });
            i = end;
            continue;
        }
        let next = lower[i..].find('<').map(|j| i + j).unwrap_or(bytes.len());
        out.push_str(&decode_entities(&html[i..next]));
        i = next;
    }

    let mut lines: Vec<String> = Vec::new();
    for line in out.lines() {
        let l = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            if lines.last().is_some_and(|x| !x.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(l);
        }
    }
    let mut text = lines.join("\n").trim().to_string();

    for p in [" .", " ,", " ;", " :", " !", " ?"] {
        text = text.replace(p, &p[1..]);
    }
    text
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&amp;", "&")
}

pub fn page(text: &str, offset: usize, page_chars: usize) -> (usize, String, Option<usize>) {
    let mut start = offset.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let rest = &text[start..];
    let end = rest.char_indices().nth(page_chars).map(|(i, _)| i).unwrap_or(rest.len());
    let next = start + end;
    (start, rest[..end].to_string(), (next < text.len()).then_some(next))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Table(HashMap<String, Vec<IpAddr>>);

    impl Resolve for Table {
        fn resolve<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, std::io::Result<Vec<SocketAddr>>> {
            let r = self.0.get(host).map(|ips| ips.iter().map(|ip| SocketAddr::new(*ip, port)).collect()).ok_or_else(|| std::io::Error::other("no such host"));
            Box::pin(async move { r })
        }
    }

    struct Plain(u16);

    impl Connect for Plain {
        fn connect<'a>(&'a self, _addr: SocketAddr, _host: &'a str) -> BoxFuture<'a, std::io::Result<Box<dyn Stream>>> {
            Box::pin(async move { Ok(Box::new(tokio::net::TcpStream::connect(("127.0.0.1", self.0)).await?) as Box<dyn Stream>) })
        }
    }

    async fn server(responses: Vec<Vec<u8>>) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        tokio::spawn(async move {
            for r in responses {
                let (mut c, _) = l.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = c.read(&mut buf).await.unwrap_or(0);
                s2.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).into_owned());
                if r.is_empty() {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                let _ = c.write_all(&r).await;
                let _ = c.shutdown().await;
            }
        });
        (port, seen)
    }

    fn lo() -> IpAddr {
        "127.0.0.1".parse().unwrap()
    }

    fn table(hosts: &[(&str, &str)]) -> Table {
        let mut m: HashMap<String, Vec<IpAddr>> = HashMap::new();
        for (h, ip) in hosts {
            m.entry(h.to_string()).or_default().push(ip.parse().unwrap());
        }
        Table(m)
    }

    fn test_cfg() -> FetchConfig {
        FetchConfig { vet: Arc::new(|host, ip| host == "ok.test" || is_public(ip)), timeout: Duration::from_millis(1500), ..FetchConfig::default() }
    }

    #[test]
    fn only_https_dns_names_without_credentials_are_fetchable() {
        assert_eq!(vet_url("https://example.com/a?b=1#frag").unwrap().as_str(), "https://example.com/a?b=1");
        for bad in [
            "http://example.com/",
            "ftp://example.com/",
            "https://user:pw@example.com/",
            "https://user@example.com/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://10.0.0.8/",
            "https://localhost/",
            "https://printer.local/",
            "https://db.internal/",
            "https://app.localhost/",
            "file:///etc/passwd",
            "not a url",
            "",
        ] {
            assert!(matches!(vet_url(bad), Err(WebError::Invalid(_))), "{bad}");
        }
    }

    #[test]
    fn only_public_addresses_are_public() {
        for ok in ["93.184.215.14", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ok.parse().unwrap()), "{ok}");
        }
        for bad in [
            "127.0.0.1", "10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "255.255.255.255",
            "224.0.0.1", "192.0.2.1", "198.51.100.7", "203.0.113.9", "198.18.0.1", "240.0.0.1", "::1", "::", "fe80::1", "fc00::1", "fd12::1",
            "ff02::1", "2001:db8::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "64:ff9b::7f00:1",
            "fec0::1", "2002:7f00:1::1", "2001:0:4136:e378:8000:63bf:3fff:fdd2", "64:ff9b:1::a00:1",
        ] {
            assert!(!is_public(bad.parse().unwrap()), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_name_that_resolves_to_any_private_address_is_refused_before_connecting() {
        let cfg = FetchConfig::default();
        let never = Plain(1);
        for (host, ips) in [("evil.test", vec!["10.0.0.5"]), ("mixed.test", vec!["93.184.215.14", "192.168.0.10"]), ("meta.test", vec!["169.254.169.254"])] {
            let t = table(&ips.iter().map(|ip| (host, *ip)).collect::<Vec<_>>());
            let e = fetch(&format!("https://{host}/"), &t, &never, &cfg).await.unwrap_err();
            assert_eq!(e.reason(), "not_public", "{host}");
        }
        assert_eq!(fetch("https://nowhere.test/", &table(&[]), &never, &cfg).await.unwrap_err().reason(), "dns_failed");
    }

    #[tokio::test]
    async fn a_page_is_fetched_with_get_and_html_becomes_text() {
        let body = "<html><head><style>p{}</style><script>alert(1)</script></head><body><h1>Port</h1><p>The port is <b>7421</b> &amp; up.</p></body></html>";
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let (port, seen) = server(vec![resp.into_bytes()]).await;
        let f = fetch("https://ok.test/page?q=1", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &test_cfg()).await.unwrap();
        assert_eq!((f.status, f.complete, f.final_url.as_str()), (200, true, "https://ok.test/page?q=1"));
        assert!(f.text.contains("The port is 7421 & up.") && !f.text.contains("alert") && !f.text.contains("<"), "{}", f.text);
        let head = seen.lock().unwrap()[0].clone();
        assert!(head.starts_with("GET /page?q=1 HTTP/1.1\r\n") && head.contains("\r\nHost: ok.test\r\n"), "{head}");
        let _ = lo();
    }

    #[tokio::test]
    async fn chunked_bodies_and_relative_redirects_work_and_redirects_are_vetted() {
        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n".to_vec();
        let (port, seen) = server(vec![b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n".to_vec(), chunked]).await;
        let f = fetch("https://ok.test/start", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &test_cfg()).await.unwrap();
        assert_eq!((f.text.as_str(), f.final_url.as_str(), f.redirects.len()), ("hello world", "https://ok.test/next", 1));
        assert!(seen.lock().unwrap()[1].starts_with("GET /next "));

        let to_http = b"HTTP/1.1 301 Moved\r\nLocation: http://ok.test/plain\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (port, _) = server(vec![to_http]).await;
        assert_eq!(fetch("https://ok.test/", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &test_cfg()).await.unwrap_err().reason(), "invalid_url");

        let to_private = b"HTTP/1.1 307 Temporary\r\nLocation: https://evil.test/admin\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (port, seen) = server(vec![to_private, b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nno".to_vec()]).await;
        let t = table(&[("ok.test", "127.0.0.1"), ("evil.test", "127.0.0.1")]);
        assert_eq!(fetch("https://ok.test/", &t, &Plain(port), &test_cfg()).await.unwrap_err().reason(), "not_public");
        assert_eq!(seen.lock().unwrap().len(), 1, "the private hop was never dialed");

        let loop_resp = b"HTTP/1.1 302 Found\r\nLocation: /again\r\nContent-Length: 0\r\n\r\n".to_vec();
        let (port, _) = server(vec![loop_resp; 7]).await;
        assert_eq!(fetch("https://ok.test/", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &test_cfg()).await.unwrap_err().reason(), "too_many_redirects");
    }

    #[tokio::test]
    async fn bodies_are_capped_types_are_text_only_and_a_silent_server_times_out() {
        let big = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n{}", "x".repeat(5000));
        let (port, _) = server(vec![big.into_bytes()]).await;
        let cfg = FetchConfig { max_bytes: 1000, ..test_cfg() };
        let f = fetch("https://ok.test/", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &cfg).await.unwrap();
        assert_eq!((f.bytes, f.complete), (1000, false), "a capped body says it is capped");

        let png = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\n\x89PNG".to_vec();
        let (port, _) = server(vec![png]).await;
        assert_eq!(fetch("https://ok.test/", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &test_cfg()).await.unwrap_err().reason(), "unsupported_type");

        let (port, _) = server(vec![Vec::new()]).await;
        let cfg = FetchConfig { timeout: Duration::from_millis(300), ..test_cfg() };
        assert_eq!(fetch("https://ok.test/", &table(&[("ok.test", "127.0.0.1")]), &Plain(port), &cfg).await.unwrap_err(), WebError::Timeout);
    }

    #[test]
    fn text_pages_are_utf8_safe() {
        let t = "aé東🙂z";
        assert_eq!(page(t, 0, 2), (0, "aé".to_string(), Some(3)));
        assert_eq!(page(t, 2, 10), (3, "東🙂z".to_string(), None));
        assert_eq!(page(t, 99, 10), (t.len(), String::new(), None));
    }
}
