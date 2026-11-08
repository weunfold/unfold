
use super::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
const GIB: u64 = 1 << 30;
pub const SERVER_SHA: &str = "aa1dbb424bef498c5f2cc6bc3584cd24cd0de2ed52b272233f4c443ff8c6c303";
pub const MODEL_SHA: &str = "00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4";
#[derive(Clone, Copy)]
pub struct Budget {
    pub pressure: u32,
    pub physical: u64,
    pub available: u64,
    pub other_reserved: Option<u64>,
}
impl Budget {
    fn admit(self) -> Result<()> {
        let others = self.other_reserved.ok_or(Error::Admission)?;
        if self.pressure != 1
            || self.available < 11 * GIB
            || others.checked_add(7 * GIB).is_none_or(|n| n > self.physical / 5 * 2)
        {
            return Err(Error::Admission);
        }
        Ok(())
    }
}
pub trait Launch {
    fn verify(&mut self) -> Result<()>;
    fn spawn(&mut self, port: u16) -> Result<Child>;
}
pub struct Serve {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub home: PathBuf,
}
pub fn args(port: u16) -> Vec<String> {
    [
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "--alias",
        "voice-front",
        "-c",
        "4096",
        "-np",
        "1",
        "-ngl",
        "99",
        "-t",
        "4",
        "-tb",
        "4",
        "--jinja",
        "--offline",
        "--no-webui",
        "--cache-ram",
        "0",
        "--chat-template-kwargs",
        "{\"enable_thinking\":false}",
    ]
    .map(String::from)
    .to_vec()
}
impl Launch for Serve {
    fn verify(&mut self) -> Result<()> {
        for (p, want) in [(&self.binary, SERVER_SHA), (&self.model, MODEL_SHA)] {
            let mut f = std::fs::File::open(p).map_err(|_| Error::Assets)?;
            let mut h = Sha256::new();
            let mut b = [0; 65536];
            loop {
                let n = f.read(&mut b).map_err(|_| Error::Assets)?;
                if n == 0 {
                    break;
                }
                h.update(&b[..n]);
            }
            if format!("{:x}", h.finalize()) != want {
                return Err(Error::Assets);
            }
        }
        Ok(())
    }
    fn spawn(&mut self, port: u16) -> Result<Child> {
        let mut c = Command::new(&self.binary);
        c.args(args(port)).arg("-m").arg(&self.model);
        use std::os::unix::process::CommandExt;
        let mut c = isolated(c, &self.home);
    unsafe {
        c.pre_exec(|| {
            let mut set = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            libc::pthread_sigmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
            Ok(())
        });
    }
        c.spawn().map_err(|_| Error::Child)
    }
}
fn isolated(mut c: Command, home: &std::path::Path) -> Command {
    use std::os::unix::process::CommandExt;
    c.env_clear().env("PATH", "/usr/bin:/bin").env("HOME", home).env("TMPDIR", home)
        .current_dir(home).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);

    c
}
struct Active {
    id: u64,
    cancel: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
}
struct State {
    child: Option<Child>,
    closed: bool,
    active: Option<Active>,
    next: u64,
}
pub struct Owner {
    state: Arc<Mutex<State>>,
    address: SocketAddr,
    pub generation: u64,
}
pub struct Lease {
    state: Arc<Mutex<State>>,
    id: u64,
    pub generation: u64,
    address: SocketAddr,
    pub(crate) cancel: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<TcpStream>>>,
    pub(crate) start: Instant,
}
impl Owner {

    pub fn start(launch: &mut impl Launch, mut budget: impl FnMut() -> Budget) -> Result<Self> {
        budget().admit()?;
        launch.verify()?;
        budget().admit()?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| Error::Http)?;
        let address = listener.local_addr().map_err(|_| Error::Http)?;
        drop(listener);
        let child = launch.spawn(address.port())?;
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Ok(Self {
            state: Arc::new(Mutex::new(State { child: Some(child), closed: false, active: None, next: 0 })),
            address,
            generation: NEXT.fetch_add(1, Ordering::SeqCst),
        })
    }

    #[cfg(test)]
    pub fn for_test_listener(launch: &mut impl Launch, budget: impl FnMut() -> Budget, listener: &TcpListener) -> Result<Self> {
        let mut owner = Self::start(launch, budget)?;
        owner.address = listener.local_addr().map_err(|_| Error::Http)?;
        Ok(owner)
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn pid(&self) -> Option<u32> {
        self.state.lock().unwrap().child.as_ref().map(Child::id)
    }
    pub fn request(&self) -> Result<Lease> {
        let mut s = self.state.lock().unwrap();
        if s.closed {
            return Err(Error::Closed);
        }
        if !s.child.as_mut().is_some_and(|c| c.try_wait().is_ok_and(|r| r.is_none())) {
            return Err(Error::Child);
        }
        if s.active.is_some() {
            return Err(Error::Busy);
        }
        s.next += 1;
        let id = s.next;
        let cancel = Arc::new(AtomicBool::new(false));
        let socket = Arc::new(Mutex::new(None));
        s.active = Some(Active { id, cancel: cancel.clone(), socket: socket.clone() });
        Ok(Lease {
            state: self.state.clone(),
            id,
            generation: self.generation,
            address: self.address,
            cancel,
            socket,
            start: Instant::now(),
        })
    }
    pub fn cancel(&self) {
        let s = self.state.lock().unwrap();
        cancel(&s);
    }

    pub fn close(&mut self) -> Result<()> { self.close_with(stop_child) }
    fn close_with(&mut self, stop: impl FnOnce(&mut Child) -> Result<()>) -> Result<()> {
        let child = {
            let mut s = self.state.lock().unwrap();
            s.closed = true;
            cancel(&s);
            s.child.take()
        };
        if let Some(mut c) = child {
            let result = stop(&mut c);
            if result.is_err() {
                self.state.lock().unwrap().child = Some(c);
            }
            result
        } else {
            Ok(())
        }
    }
}
fn stop_child(c: &mut Child) -> Result<()> {
    if c.try_wait().map_err(|_| Error::Child)?.is_some() {
        return Ok(());
    }
    unsafe {
        libc::kill(c.id() as i32, libc::SIGTERM);
    }
    for (grace, kill) in [(100, false), (500, true)] {
        if kill {
            c.kill().map_err(|_| Error::Child)?;
        }
        let until = Instant::now() + Duration::from_millis(grace);
        while Instant::now() < until {
            if c.try_wait().map_err(|_| Error::Child)?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    Err(Error::Child)
}

fn cancel(s: &State) {
    if let Some(a) = &s.active {
        a.cancel.store(true, Ordering::SeqCst);
        if let Some(socket) = a.socket.lock().unwrap().as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl Lease {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        if self.start.elapsed() > Duration::from_secs(5) {
            return Err(Error::Deadline);
        }
        Ok(())
    }
    pub fn live(&self) -> Result<()> {
        self.check()?;
        let mut s = self.state.lock().unwrap();
        if s.closed || s.active.as_ref().is_none_or(|a| a.id != self.id) {
            return Err(Error::Cancelled);
        }
        if !s.child.as_mut().is_some_and(|c| c.try_wait().is_ok_and(|r| r.is_none())) {
            return Err(Error::Child);
        }
        Ok(())
    }
    pub fn valid(&self) -> bool {
        self.live().is_ok()
    }
    pub(crate) fn register(&self, socket: &TcpStream) -> Result<()> {
        let mut slot = self.socket.lock().unwrap();
        self.check()?;
        *slot = Some(socket.try_clone().map_err(|_| Error::Http)?);
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let mut s = self.state.lock().unwrap();
        if s.active.as_ref().is_some_and(|a| a.id == self.id) {
            s.active = None;
        }
    }
}

#[cfg(test)]
mod isolation_tests {
    use super::*;
    #[test]
    fn serve_builder_inherits_only_the_three_explicit_environment_names() {
        let home = std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()));
        let out = isolated(Command::new("/usr/bin/env"), &home).stdout(Stdio::piped()).output().unwrap();
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        let mut keys = text.lines().map(|l| l.split_once('=').unwrap().0).collect::<Vec<_>>(); keys.sort();
        assert_eq!(keys, ["HOME", "PATH", "TMPDIR"]);
    }
    #[test]
    fn failed_close_retains_the_child_but_request_stays_closed() {
        let child = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let mut owner = Owner { state: Arc::new(Mutex::new(State { child:Some(child),closed:false,active:None,next:0 })),
            address: "127.0.0.1:1".parse().unwrap(), generation:1 };
        assert_eq!(owner.close_with(|_| Err(Error::Child)), Err(Error::Child));
        assert!(owner.pid().is_some());
        assert!(matches!(owner.request(), Err(Error::Closed)));
        owner.close().unwrap();
    }
}
