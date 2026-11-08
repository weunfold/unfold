use std::process::{Command, Stdio};
use std::sync::LazyLock;

pub struct Material {
    pub cert: Vec<u8>,
    pub key: Vec<u8>,
    ca: String,
}

static MATERIAL: LazyLock<Material> = LazyLock::new(|| {
    let dir = std::env::temp_dir().join(format!("unfold-test-tls-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&dir).expect("create an exclusive test TLS directory");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = |name: &str| dir.join(name);
    let config = path("openssl.cnf");
    std::fs::write(&config, "[req]\ndistinguished_name=dn\n[dn]\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\n[server]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n").unwrap();
    let run = |args: &[&str]| {
        let status = Command::new("openssl")
            .args(args).current_dir(&dir).env("RANDFILE", path("random-seed"))
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .status().expect("OpenSSL is required for local TLS regression tests");
        assert!(status.success(), "OpenSSL failed to generate local TLS test material");
    };
    run(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "1", "-subj", "/CN=Unfold test CA", "-config", "openssl.cnf", "-extensions", "ca"]);
    run(&["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=localhost", "-config", "openssl.cnf"]);
    run(&["x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.pem", "-days", "1", "-extfile", "openssl.cnf", "-extensions", "server"]);
    Material {
        cert: std::fs::read(path("server.pem")).unwrap(),
        key: std::fs::read(path("server.key")).unwrap(),
        ca: path("ca.pem").to_string_lossy().into_owned(),
    }
});

pub fn material() -> &'static Material { &MATERIAL }
pub fn ca() -> &'static str { &MATERIAL.ca }
