

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::{Log, Shared};

pub const ID: &str = "unfold";
const OPEN: &str = "tray-open";
const BROWSER: &str = "tray-browser";
#[cfg(feature = "voice_pane")]
const CONVERSATION: &str = "tray-conversation";
const QUIT: &str = "tray-quit";

const ITEMS: [(&str, &str, &str); 3] = [(OPEN, "Open Unfold", "Abrir Unfold"), (BROWSER, "Open in browser", "Abrir no navegador"), (QUIT, "Quit Unfold", "Encerrar Unfold")];

const GRACE: Duration = Duration::from_secs(5);

fn items(pt: bool) -> Vec<(&'static str, &'static str)> {
    ITEMS.iter().map(|&(id, en, p)| (id, if pt { p } else { en })).collect()
}

fn mark(size: u32) -> Vec<u8> {

    let c = size as f32 / 2.0;
    let outer = c - 2.0;
    let (ring, half, dot) = (outer * 210.0 / 242.0, outer * 32.0 / 242.0, outer * 60.0 / 242.0);
    let mut px = vec![0u8; (size * size * 4) as usize];
    const S: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let mut hit = 0;
            for sy in 0..S {
                for sx in 0..S {
                    let dx = x as f32 + (sx as f32 + 0.5) / S as f32 - c;
                    let dy = y as f32 + (sy as f32 + 0.5) / S as f32 - c;
                    let r = (dx * dx + dy * dy).sqrt();
                    if r <= dot || (r - ring).abs() <= half {
                        hit += 1;
                    }
                }
            }
            px[((y * size + x) * 4 + 3) as usize] = (hit * 255 / (S * S)) as u8;
        }
    }
    px
}

static ADDRESS: Mutex<Option<String>> = Mutex::new(None);

static BROWSER_ITEM: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>> = Mutex::new(None);

#[cfg(feature = "voice_pane")]
static CONVERSATION_ITEM: Mutex<Option<tauri::menu::MenuItem<tauri::Wry>>> = Mutex::new(None);

#[cfg(feature = "voice_pane")]
fn conversation_words(phase: &str, pt: bool) -> (&'static str, &'static str) {
    let action = match (phase == "off", pt) {
        (true, false) => "Start conversation", (false, false) => "End conversation",
        (true, true) => "Iniciar conversa", (false, true) => "Encerrar conversa",
    };
    let state = match (phase, pt) {
        ("listening", false) => "Listening", ("listening", true) => "Ouvindo",
        ("thinking", false) => "Thinking", ("thinking", true) => "Pensando",
        ("interrupted", false) => "Interrupted", ("interrupted", true) => "Interrompido",
        ("speaking", false) => "Speaking", ("speaking", true) => "Falando",
        _ => "",
    };
    (action, state)
}

#[cfg(feature = "voice_pane")]
pub fn conversation_state(app: &AppHandle, phase: &str) {
    let (action, state) = conversation_words(phase, crate::settings::pt());
    if let Some(item) = CONVERSATION_ITEM.lock().unwrap().as_ref() {
        let _ = item.set_text(format!("{action} · {}", crate::conversation_hotkey::CONVERSATION_HOTKEY));
    }
    if let Some(icon) = app.tray_by_id(ID) {
        let _ = icon.set_title(Some(state));
        let _ = icon.set_tooltip(Some(if state.is_empty() { "Unfold".into() } else { format!("Unfold · {state}") }));
    }
}
#[cfg(feature = "voice_pane")]
pub fn conversation_error(app: &AppHandle, reason: &str) {
    if let Some(icon) = app.tray_by_id(ID) { let _ = icon.set_tooltip(Some(format!("Unfold · {reason}"))); }
}

pub fn set_address(address: Option<String>) {
    let on = address.is_some();
    *ADDRESS.lock().unwrap() = address;
    if let Some(item) = BROWSER_ITEM.lock().unwrap().as_ref() {
        let _ = item.set_enabled(on);
    }
}

pub fn install(app: &AppHandle, pt: bool, quit: impl Fn(&AppHandle) + Send + Sync + 'static) -> tauri::Result<()> {
    let mut menu = MenuBuilder::new(app);
    for (id, label) in items(pt) {
        if id == QUIT {
            #[cfg(feature = "voice_pane")]
            {
                let label = if pt { "Iniciar conversa" } else { "Start conversation" };
                let item = MenuItemBuilder::with_id(CONVERSATION, format!("{label} · {}", crate::conversation_hotkey::CONVERSATION_HOTKEY)).build(app)?;
                *CONVERSATION_ITEM.lock().unwrap() = Some(item.clone());
                menu = menu.item(&item);
            }
            menu = menu.separator();
        }
        let item = MenuItemBuilder::with_id(id, label).enabled(id != BROWSER || ADDRESS.lock().unwrap().is_some()).build(app)?;
        if id == BROWSER {
            *BROWSER_ITEM.lock().unwrap() = Some(item.clone());
        }
        menu = menu.item(&item);
    }
    TrayIconBuilder::with_id(ID)
        .icon(Image::new_owned(mark(36), 36, 36))
        .icon_as_template(true)
        .tooltip("Unfold")
        .menu(&menu.build()?)

        .show_menu_on_left_click(false)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            OPEN => show_main(app),
            BROWSER => {
                if let Some(a) = ADDRESS.lock().unwrap().clone() {
                    let _ = Command::new("/usr/bin/open").arg(a).spawn().and_then(|mut c| c.wait());
                }
            }
            #[cfg(feature = "voice_pane")]
            CONVERSATION => crate::conversation::control(app),
            QUIT => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|icon, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                show_main(icon.app_handle());
            }
        })
        .build(app)?;
    #[cfg(feature = "voice_pane")]
    if let Err(e) = crate::conversation::install_global(app) { conversation_error(app, &e); }
    Ok(())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn main_closed(log: &Log, hide: impl FnOnce() -> tauri::Result<()>) {
    match hide() {
        Ok(()) => log.line("window closed: hidden; the app keeps running in the menu bar"),
        Err(e) => log.line(&format!("window closed: not hidden ({e})")),
    }
}

pub struct Recorded {
    name: &'static str,
    child: Child,
    pgid: libc::pid_t,
    start_us: u64,
}

#[derive(Debug, PartialEq)]
enum Held {
    Running,

    Exited,
}

fn exited_unreaped(pid: u32) -> bool {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) };
    r == 0 && info.si_pid == pid as libc::pid_t
}

fn still_recorded(i: &crate::nemo::ProcInfo, pgid: libc::pid_t, start_us: u64, me: u32) -> Result<(), String> {
    if i.ppid != me {
        return Err(format!("its parent is now pid {}", i.ppid));
    }
    if i.pgid != pgid {
        return Err("another process group".into());
    }
    if i.start_us != start_us {
        return Err("another start time".into());
    }
    Ok(())
}

fn recordable(i: &crate::nemo::ProcInfo, pid: u32, me: u32) -> bool {
    i.ppid == me && i.pgid == pid as libc::pid_t
}

impl Recorded {
    fn held(&self) -> Result<Held, String> {
        let pid = self.child.id();
        if exited_unreaped(pid) {
            return Ok(Held::Exited);
        }
        let info = crate::nemo::proc_info(pid).ok_or("no longer listed")?;
        still_recorded(&info, self.pgid, self.start_us, std::process::id()).map(|()| Held::Running)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub pid: u32,
    pub pgid: libc::pid_t,
    pub start_us: u64,
    pub command: std::path::PathBuf,
}

fn take(owned: &Shared, pid: u32) -> Option<Recorded> {
    let mut o = owned.lock().unwrap();
    let i = o.recorded.iter().position(|r| r.child.id() == pid)?;
    Some(o.recorded.remove(i))
}

pub fn take_stdout(owned: &Shared, pid: u32) -> Option<std::process::ChildStdout> {
    owned.lock().unwrap().recorded.iter_mut().find(|r| r.child.id() == pid)?.child.stdout.take()
}

pub fn identity(owned: &Shared, pid: u32) -> Option<Identity> {
    let o = owned.lock().unwrap();
    let r = o.recorded.iter().find(|r| r.child.id() == pid)?;
    if r.held() != Ok(Held::Running) {
        return None;
    }
    Some(Identity { pid, pgid: r.pgid, start_us: r.start_us, command: crate::pid_path(pid as i32)? })
}

pub fn stop_one(owned: &Shared, pid: u32, log: &Log) {
    if let Some(r) = take(owned, pid) {
        stop_recorded(vec![r], log);
    }
}

pub fn stop_matching(owned: &Shared, pid: u32, start_us: u64, log: &Log) {
    let r = {
        let mut o = owned.lock().unwrap();
        let Some(i) = o.recorded.iter().position(|r| r.child.id() == pid && r.start_us == start_us) else { return };
        o.recorded.remove(i)
    };
    stop_recorded(vec![r], log);
}

pub fn exited(owned: &Shared, pid: u32, log: &Log) -> Option<bool> {
    let r = {
        let mut o = owned.lock().unwrap();
        let i = o.recorded.iter().position(|r| r.child.id() == pid)?;
        if !exited_unreaped(pid) {
            return Some(false);
        }
        o.recorded.remove(i)
    };
    stop_recorded(vec![r], log);
    Some(true)
}

fn all_pids() -> Vec<u32> {
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return Vec::new();
    }
    let mut buf = vec![0 as libc::pid_t; n as usize + 64];
    let got = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast(), (buf.len() * std::mem::size_of::<libc::pid_t>()) as i32) };
    buf.truncate(got.max(0) as usize);
    buf.into_iter().filter(|&p| p > 0).map(|p| p as u32).collect()
}

fn outside_descendants(pgid: libc::pid_t) -> Vec<(u32, u64)> {
    let procs: Vec<(u32, crate::nemo::ProcInfo)> = all_pids().into_iter().filter_map(|p| crate::nemo::proc_info(p).map(|i| (p, i))).collect();
    let mut found: Vec<u32> = live_members(pgid).into_iter().map(|p| p as u32).collect();
    let mut next = 0;
    while next < found.len() {
        let parent = found[next];
        next += 1;
        for (p, i) in &procs {
            if i.ppid == parent && *p != parent && !found.contains(p) {
                found.push(*p);
            }
        }
    }
    procs.iter().filter(|(p, i)| found.contains(p) && i.pgid != pgid).map(|(p, i)| (*p, i.start_us)).collect()
}

fn same_process(pid: u32, start_us: u64) -> bool {
    crate::nemo::proc_info(pid).is_some_and(|i| i.start_us == start_us)
}

fn live_members(pgid: libc::pid_t) -> Vec<i32> {
    crate::nemo::group_members(pgid).into_iter().filter(|&p| crate::nemo::proc_info(p as u32).is_some()).collect()
}

pub fn spawn_recorded(owned: &Shared, name: &'static str, cmd: &mut Command) -> Result<u32, String> {
    crate::unblock_quit_signals(cmd);
    cmd.process_group(0);
    let mut o = owned.lock().unwrap();
    if o.stopped {
        return Err(crate::CLOSING.into());
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn {:?}: {e}", cmd.get_program()))?;
    let pid = child.id();
    match crate::nemo::proc_info(pid) {
        Some(i) if recordable(&i, pid, std::process::id()) => {
            o.recorded.push(Recorded { name, child, pgid: i.pgid, start_us: i.start_us });
            Ok(pid)
        }
        _ => {

            let _ = child.kill();
            let _ = child.wait();
            Err(format!("{name} (pid {pid}) could not be recorded; stopped"))
        }
    }
}

pub fn stop_recorded(recorded: Vec<Recorded>, log: &Log) {
    let mut held = Vec::new();

    let mut outside: Vec<(&'static str, u32, u64)> = Vec::new();
    for r in recorded {
        match r.held() {
            Ok(h) => {
                outside.extend(outside_descendants(r.pgid).into_iter().map(|(p, start)| (r.name, p, start)));
                unsafe { libc::killpg(r.pgid, libc::SIGTERM) };
                let leader = if h == Held::Exited { " (its leader had exited)" } else { "" };
                log.line(&format!("close: SIGTERM -> recorded {} group {}{leader}", r.name, r.pgid));
                held.push(r);
            }
            Err(why) => log.line(&format!("{} (pid {}) {why}; not signalled", r.name, r.child.id())),
        }
    }
    for &(name, p, start) in &outside {
        if same_process(p, start) {
            unsafe { libc::kill(p as i32, libc::SIGTERM) };
            log.line(&format!("close: SIGTERM -> pid {p}, started by recorded {name} outside its group"));
        }
    }
    let end = Instant::now() + GRACE;
    while (held.iter().any(|r| !live_members(r.pgid).is_empty()) || outside.iter().any(|&(_, p, start)| same_process(p, start))) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(25));
    }
    for r in &held {
        if !live_members(r.pgid).is_empty() {
            unsafe { libc::killpg(r.pgid, libc::SIGKILL) };
            log.line(&format!("close: SIGKILL -> recorded {} group {} after {} s", r.name, r.pgid, GRACE.as_secs()));
        }
    }
    for &(name, p, start) in &outside {
        if same_process(p, start) {
            unsafe { libc::kill(p as i32, libc::SIGKILL) };
            log.line(&format!("close: SIGKILL -> pid {p}, started by recorded {name} outside its group, after {} s", GRACE.as_secs()));
        }
    }
    for mut r in held {
        match r.child.wait() {
            Ok(s) => log.line(&format!("{} (pid {}) exited: {s}", r.name, r.child.id())),
            Err(e) => log.line(&format!("{} (pid {}): not reaped ({e})", r.name, r.child.id())),
        }
    }
}

pub const STOP_AT_MOST: Duration = Duration::from_secs(6);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    fn gone_within(pid: u32, d: Duration) -> bool {
        let end = Instant::now() + d;
        while alive(pid) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        !alive(pid)
    }

    fn logged(tag: &str) -> (Log, impl Fn() -> String) {
        let dir = std::env::temp_dir().join(format!("tray-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log = Log::open_as(&crate::WriteMode::Dev, &dir);
        (log, move || std::fs::read_to_string(dir.join("app.log")).unwrap_or_default())
    }

    #[cfg(feature = "voice_pane")]
    #[test]
    fn conversation_is_visible_and_controllable_while_a_browser_is_in_front() {
        assert_eq!(conversation_words("off", false), ("Start conversation", ""));
        assert_eq!(conversation_words("listening", false), ("End conversation", "Listening"));
        assert_eq!(conversation_words("thinking", true), ("Encerrar conversa", "Pensando"));
        assert_eq!(conversation_words("speaking", true), ("Encerrar conversa", "Falando"));
        assert_eq!(conversation_words("interrupted", false), ("End conversation", "Interrupted"));
    }

    #[test]
    fn tray_menu_opens_unfold_opens_the_address_and_quits_in_both_languages() {
        assert_eq!(items(false), [(OPEN, "Open Unfold"), (BROWSER, "Open in browser"), (QUIT, "Quit Unfold")]);
        assert_eq!(items(true), [(OPEN, "Abrir Unfold"), (BROWSER, "Abrir no navegador"), (QUIT, "Encerrar Unfold")]);
        assert!(ADDRESS.lock().unwrap().is_none(), "no address until a child serves one");

        set_address(Some("http://127.0.0.1:1/".into()));
        assert_eq!(ADDRESS.lock().unwrap().as_deref(), Some("http://127.0.0.1:1/"));
        set_address(None);
        assert!(ADDRESS.lock().unwrap().is_none());
    }

    #[test]
    fn tray_icon_is_the_apps_mark_black_on_transparent() {
        let px = mark(36);
        let a = |x: u32, y: u32| px[((y * 36 + x) * 4 + 3) as usize];
        assert!(px.chunks(4).all(|p| p[..3] == [0, 0, 0]), "a template image: black, its shape in the alpha");
        assert_eq!(a(18, 18), 255, "the dot");
        assert_eq!(a(18, 18 - 10), 0, "between the dot and the ring");
        assert_eq!(a(18, 18 - 15), 255, "the ring");
        assert_eq!((a(0, 0), a(35, 35), a(1, 34)), (0, 0, 0), "the corners");
    }

    #[test]
    fn tray_a_record_holds_only_with_its_parent_group_and_start() {
        let me = 100;
        let i = |ppid, pgid, start_us| crate::nemo::ProcInfo { start_us, pgid, ppid };
        assert_eq!(still_recorded(&i(me, 7, 9), 7, 9, me), Ok(()));
        assert_eq!(still_recorded(&i(1, 7, 9), 7, 9, me), Err("its parent is now pid 1".into()));
        assert_eq!(still_recorded(&i(me, 8, 9), 7, 9, me), Err("another process group".into()));
        assert_eq!(still_recorded(&i(me, 7, 10), 7, 9, me), Err("another start time".into()));
        assert!(recordable(&i(me, 7, 9), 7, me));
        assert!(!recordable(&i(1, 7, 9), 7, me), "another parent");
        assert!(!recordable(&i(me, 8, 9), 7, me), "not its own group's leader");
    }

    #[test]
    fn tray_quit_stays_well_under_the_exit_cap() {

        assert!(STOP_AT_MOST + crate::OWNED_GRACE * 2 + Duration::from_secs(3) < crate::EXIT_CAP);
    }

    fn fake(owned: &Shared, name: &'static str) -> (u32, u32) {
        fake_with(owned, name, "/bin/sleep 30 & echo $!; wait")
    }

    struct Leftovers(Vec<(u32, u64)>);
    impl Leftovers {
        fn of(pids: &[u32]) -> Leftovers {
            Leftovers(pids.iter().filter_map(|&p| crate::nemo::proc_info(p).map(|i| (p, i.start_us))).collect())
        }
    }
    impl Drop for Leftovers {
        fn drop(&mut self) {
            for &(p, start) in &self.0 {
                if crate::nemo::proc_info(p).is_some_and(|i| i.start_us == start) {
                    unsafe { libc::kill(p as i32, libc::SIGKILL) };
                }
            }
        }
    }

    fn snapshot(pids: &[u32]) -> serde_json::Value {
        pids.iter()
            .filter_map(|&p| {
                let i = crate::nemo::proc_info(p)?;
                Some(serde_json::json!({"pid": p, "ppid": i.ppid, "start": i.start_us, "command": crate::pid_path(p as i32).map(|c| c.display().to_string())}))
            })
            .collect()
    }

    fn evidence(tag: &str, sets: [(&str, &[(&str, u32, u32)]); 2], decoys: &[u32], before: serde_json::Value, after: serde_json::Value, log: &str) {
        use sha2::Digest;
        let Some(out) = std::env::var_os("RTH_TRAY_CHECK_OUT").map(std::path::PathBuf::from) else { return };
        let exe = std::env::current_exe().unwrap();
        let exe_sha: String = sha2::Sha256::digest(std::fs::read(&exe).unwrap()).iter().map(|b| format!("{b:02x}")).collect();
        let mut v = serde_json::json!({"test_pid": std::process::id(), "decoys": decoys, "before": before, "after": after,
            "diff_sha": std::env::var("RTH_TRAY_DIFF_SHA").ok(), "test_binary": {"path": exe, "sha256": exe_sha}});
        for (key, kids) in sets {
            v[key] = kids.iter().map(|&(name, pid, member)| serde_json::json!({"name": name, "pid": pid, "members": [member]})).collect();
        }
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(format!("{tag}.json")), serde_json::to_vec_pretty(&v).unwrap()).unwrap();
        std::fs::write(out.join(format!("{tag}.log")), log).unwrap();
    }

    fn fake_with(owned: &Shared, name: &'static str, script: &str) -> (u32, u32) {
        let mut c = Command::new("/bin/bash");
        c.args(["-c", script]).stdout(std::process::Stdio::piped());
        let pid = spawn_recorded(owned, name, &mut c).unwrap();
        let mut o = owned.lock().unwrap();
        let out = o.recorded.iter_mut().find(|r| r.child.id() == pid).unwrap().child.stdout.take().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(out), &mut line).unwrap();
        (pid, line.trim().parse().unwrap())
    }

    fn within(d: Duration, ok: impl Fn() -> bool) -> bool {
        let end = Instant::now() + d;
        while !ok() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        ok()
    }

    #[test]
    fn tray_a_window_close_leaves_the_recorded_children_running_and_quit_stops_only_them() {
        let owned: Shared = Arc::default();
        let (log, lines) = logged("quit");
        let (dsh, dsh_sleep) = fake(&owned, "dsh");
        let (omni, omni_sleep) = fake(&owned, "omni");

        let (stubborn, left) = fake_with(&owned, "stubborn", "trap '' TERM; /bin/sleep 30 & echo $!; trap - TERM; wait");

        let (execd, execd_sleep) = fake_with(&owned, "execd", "/bin/sleep 30 & echo $!; exec /bin/sleep 31");
        assert!(within(Duration::from_secs(2), || crate::pid_path(execd as i32).as_deref() == Some(std::path::Path::new("/bin/sleep"))), "the exec");

        let (exited, exited_sleep) = fake_with(&owned, "exited", "/bin/sleep 30 & echo $!; exit 0");
        assert!(within(Duration::from_secs(2), || exited_unreaped(exited)), "the leader's exit");

        let mut other = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        let kids = [dsh, dsh_sleep, omni, omni_sleep, stubborn, left, execd, execd_sleep, exited, exited_sleep];
        let _left = Leftovers::of(&[&kids[..], &[other.id()]].concat());
        let hidden = std::cell::Cell::new(false);
        main_closed(&log, || {
            hidden.set(true);
            Ok(())
        });
        assert!(hidden.get());
        std::thread::sleep(Duration::from_millis(200));
        assert!(!owned.lock().unwrap().stopped, "a close begins no quit");
        assert!(kids.iter().filter(|&&p| p != exited).all(|&p| alive(p)), "all still running after the close");
        let all = [&kids[..], &[other.id()]].concat();
        let before = snapshot(&all);

        let taken = crate::take_owned(&owned).unwrap();
        let t0 = Instant::now();
        crate::stop_taken(taken, &log);
        let took = t0.elapsed();
        for p in kids {
            assert!(gone_within(p, Duration::from_secs(2)), "pid {p} still running after quit");
        }
        assert!(other.try_wait().unwrap().is_none(), "the unrecorded sleep is untouched");
        let recorded = [("dsh", dsh, dsh_sleep), ("omni", omni, omni_sleep), ("stubborn", stubborn, left), ("execd", execd, execd_sleep), ("exited", exited, exited_sleep)];
        evidence("quit", [("recorded", &recorded), ("unmatched", &[])], &[other.id()], before, snapshot(&all), &lines());

        let l = lines();
        assert_eq!(l.matches("SIGKILL").count(), 1, "{l}");
        assert!(l.contains(&format!("close: SIGKILL -> recorded stubborn group {stubborn} after 5 s")), "{l}");
        assert!(l.contains(&format!("close: SIGTERM -> recorded exited group {exited} (its leader had exited)")), "{l}");
        assert!(took < STOP_AT_MOST, "{took:?}");
        assert!(spawn_recorded(&owned, "late", &mut Command::new("/bin/sleep")).is_err(), "nothing starts once closing began");
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[test]
    fn tray_a_record_that_no_longer_matches_is_never_signalled() {
        let owned: Shared = Arc::default();
        let (log, lines) = logged("unmatched");
        let (moved, moved_sleep) = fake(&owned, "moved");
        let (regrouped, regrouped_sleep) = fake(&owned, "regrouped");
        let _left = Leftovers::of(&[moved, moved_sleep, regrouped, regrouped_sleep]);
        {
            let mut o = owned.lock().unwrap();
            for r in o.recorded.iter_mut() {
                match r.name {

                    "moved" => r.start_us += 1,

                    "regrouped" => r.pgid += 1,
                    _ => {}
                }
            }
        }
        let all = [moved, moved_sleep, regrouped, regrouped_sleep];
        let before = snapshot(&all);
        let taken = crate::take_owned(&owned).unwrap();
        crate::stop_taken(taken, &log);
        std::thread::sleep(Duration::from_millis(300));
        let unmatched = [("moved", moved, moved_sleep), ("regrouped", regrouped, regrouped_sleep)];
        evidence("unmatched", [("recorded", &[]), ("unmatched", &unmatched)], &[], before, snapshot(&all), &lines());
        assert!(all.iter().all(|&p| alive(p)), "a record that doesn't match is left alone");
        let l = lines();
        for (name, why) in [("moved", "another start time"), ("regrouped", "another process group")] {
            assert!(l.lines().any(|x| x.contains(&format!("{name} (pid ")) && x.contains(why) && x.ends_with("not signalled")), "{name}: {l}");
        }
        assert!(!l.contains("SIGTERM"), "{l}");
    }

    fn moved_out(parent_group: libc::pid_t, pid: u32) -> u32 {
        assert!(within(Duration::from_secs(3), || crate::nemo::proc_info(pid).is_some_and(|i| i.pgid == pid as libc::pid_t && i.pgid != parent_group)), "the setsid");
        pid
    }

    #[test]
    fn tray_quit_stops_what_a_recorded_child_started_outside_its_group() {
        let owned: Shared = Arc::default();
        let (log, lines) = logged("outside");

        let setsid = |ignore: bool| format!("/usr/bin/perl -MPOSIX -e '{}POSIX::setsid(); sleep 30' & echo $!; wait", if ignore { "$SIG{TERM}=\"IGNORE\"; " } else { "" });
        let (dsh, tool) = fake_with(&owned, "dsh", &setsid(false));
        let (omni, stubborn_tool) = fake_with(&owned, "omni", &setsid(true));
        let tool = moved_out(dsh as libc::pid_t, tool);
        let stubborn_tool = moved_out(omni as libc::pid_t, stubborn_tool);

        let mut decoy = Command::new("/usr/bin/perl").args(["-MPOSIX", "-e", "POSIX::setsid(); sleep 30"]).spawn().unwrap();
        let decoy_pid = moved_out(std::process::id() as libc::pid_t, decoy.id());
        let _left = Leftovers::of(&[dsh, tool, omni, stubborn_tool, decoy_pid]);
        let recorded = std::mem::take(&mut owned.lock().unwrap().recorded);
        let t = Instant::now();
        stop_recorded(recorded, &log);
        assert!(t.elapsed() < STOP_AT_MOST, "{:?}", t.elapsed());
        for (what, pid) in [("dsh", dsh), ("its tool", tool), ("omni", omni), ("its stubborn tool", stubborn_tool)] {
            assert!(gone_within(pid, Duration::from_secs(1)), "{what} (pid {pid}) still runs");
        }
        assert!(alive(decoy_pid), "the decoy runs");
        let l = lines();
        assert!(l.contains(&format!("close: SIGTERM -> pid {tool}, started by recorded dsh outside its group")), "{l}");
        assert!(l.contains(&format!("close: SIGKILL -> pid {stubborn_tool}, started by recorded omni outside its group, after 5 s")), "{l}");
        assert_eq!(l.matches("SIGKILL").count(), 1, "only the stubborn tool needed it: {l}");
        assert!(!l.contains(&format!("pid {decoy_pid}")), "{l}");
        decoy.kill().unwrap();
        decoy.wait().unwrap();
    }

    #[test]
    fn tray_a_recorded_leader_that_exits_has_its_group_stopped_then_is_reaped() {
        let owned: Shared = Arc::default();
        let (log, lines) = logged("exit");
        let (leader, left) = fake_with(&owned, "dsh", "/bin/sleep 30 & echo $!; exit 3");
        let _left = Leftovers::of(&[leader, left]);
        assert!(within(Duration::from_secs(2), || exited_unreaped(leader)), "the leader's exit");
        assert_eq!(exited(&owned, 1, &log), None, "not recorded");
        assert_eq!(exited(&owned, leader, &log), Some(true));
        assert!(gone_within(left, Duration::from_secs(1)), "what it left in its group is stopped");
        assert!(owned.lock().unwrap().recorded.is_empty(), "taken out of the list");
        let l = lines();
        assert!(l.contains("(its leader had exited)") && l.contains(&format!("dsh (pid {leader}) exited: exit status: 3")), "{l}");

        let (running, member) = fake(&owned, "omni");
        let _also = Leftovers::of(&[running, member]);
        assert_eq!(exited(&owned, running, &log), Some(false));
        stop_one(&owned, running, &log);
        assert!(gone_within(member, Duration::from_secs(1)) && owned.lock().unwrap().recorded.is_empty());
    }
}
