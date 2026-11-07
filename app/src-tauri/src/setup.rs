

use crate::{cancelled, owned_open, port_open, spawn_owned, status, until, Config, InstanceHomes, Log, Open, Shared, Slot, CLOSING};
use rth_activation::{keychain::KeyStore, lease::Lease, product, seam, service};
use serde_json::{json, Value};
use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use tauri::WebviewWindow;

#[derive(Debug, PartialEq)]
pub enum Action {

    Setup,

    RemoveKey,

    Refresh,

    Models(Value),

    Say(String, bool),

    Release(String),
    CancelEffect(String),

    DshUndo(String),

    SessionSay(String, SessionOrigin, Option<Answer>),
    #[cfg(feature = "voice_pane")]
    ConversationCancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionOrigin {
    Typed,
    #[cfg(feature = "voice_pane")]
    Dictated(u64),
    #[cfg(feature = "voice_pane")]
    Composer(u64),
}
fn enqueue_session(gate: &mpsc::Sender<Action>, text: String, origin: SessionOrigin, answer: Option<Answer>) -> Result<(), String> {
    gate.send(Action::SessionSay(text, origin, answer)).map_err(|_| "the chat is closed".into())
}

static SETUP: Mutex<Option<mpsc::Sender<Action>>> = Mutex::new(None);

#[cfg(feature = "voice_pane")]
pub fn conversation_say(text: String, epoch: u64, composer: bool) -> Result<(), String> {
    let gate = SETUP.lock().unwrap();
    let origin = if composer { SessionOrigin::Composer(epoch) } else { SessionOrigin::Dictated(epoch) };
    enqueue_session(gate.as_ref().ok_or("chat is not ready")?, text, origin, None)
}

#[cfg(feature = "voice_pane")]
pub fn conversation_cancel() -> Result<(), String> {
    let gate = SETUP.lock().unwrap();
    gate.as_ref().ok_or("chat is not ready")?.send(Action::ConversationCancel).map_err(|_| "chat is closed".into())
}

pub fn poke() {
    if let Some(tx) = SETUP.lock().unwrap().as_ref() {
        let _ = tx.send(Action::Refresh);
    }
}

pub fn choose_models(choice: Value) -> Result<(), String> {
    match SETUP.lock().unwrap().as_ref() {
        Some(tx) => tx.send(Action::Models(choice)).map_err(|_| "setup is closed".to_string()),
        None => Err("Models can be changed once Unfold is on its home screen.".into()),
    }
}

pub fn restart_chat() {
    let Some(c) = crate::settings::CTX.get() else { return };
    let Some(choice) = crate::models::load_choice(&c.data) else { return };
    if let Some(tx) = SETUP.lock().unwrap().as_ref() {
        let _ = tx.send(Action::Models(choice));
    }
}

#[derive(Debug)]
pub struct Answer(mpsc::Sender<Result<String, String>>);
impl PartialEq for Answer {
    fn eq(&self, _: &Answer) -> bool {
        false
    }
}

const SESSION_SAY_WITHIN: Duration = Duration::from_secs(10);

#[cfg_attr(not(feature = "menu_bar"), allow(dead_code))]
pub fn session_say(text: &str) -> Result<String, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > SAY_MAX {
        return Err("not a turn".into());
    }
    if t.starts_with('/') {
        return Err("a command, not a turn".into());
    }
    let (tx, rx) = mpsc::channel();
    match SETUP.lock().unwrap().as_ref() {
        Some(gate) => enqueue_session(gate, t.to_string(), SessionOrigin::Typed, Some(Answer(tx)))?,
        None => return Err("the chat is not ready".into()),
    }
    rx.recv_timeout(SESSION_SAY_WITHIN).unwrap_or_else(|_| Err("the chat did not send it".into()))
}

pub fn session_said_js(text: &str) -> String {
    format!("window.__rthSessionSaid && window.__rthSessionSaid({})", json!({"text": text}))
}

fn session_turn(chat: Option<&mut crate::chat::Chat>, text: &str, origin: SessionOrigin, answer: Option<Answer>, win: &WebviewWindow, log: &Log) {
    let mut reported = false;
    let r = (|| {
        let c = chat.ok_or("the chat is not ready")?;
        c.session_available()?;
        #[cfg(feature = "voice_pane")]
        if crate::conversation::active() && c.waiting() { return Err("wait for this reply to finish".into()); }
        #[cfg(feature = "voice_pane")]
        let conversation = match origin {
            SessionOrigin::Dictated(epoch) | SessionOrigin::Composer(epoch) => {
                if !crate::conversation::take_pending(epoch) { return Err("conversation turn expired".into()); }
                true
            }
            SessionOrigin::Typed => crate::conversation::typed(text)?,
        };
        #[cfg(feature = "voice_pane")]
        let reply_voice = crate::settings::reply_turn(matches!(origin, SessionOrigin::Dictated(_) | SessionOrigin::Composer(_)));
        #[cfg(not(feature = "voice_pane"))]
        let reply_voice: Option<Value> = { let _ = origin; None };
        #[cfg(feature = "voice_pane")]
        if matches!(origin, SessionOrigin::Dictated(_)) {
            let _ = win.eval(format!("window.__rthConversationSaid && window.__rthConversationSaid({})", json!({"text": text})));
        } else if origin == SessionOrigin::Typed { let _ = win.eval(session_said_js(text)); }
        #[cfg(not(feature = "voice_pane"))]
        let _ = win.eval(session_said_js(text));
        #[cfg(feature = "voice_pane")]
        let (wire, interruption) = if conversation { crate::conversation::interruption_context(text) } else { (text.to_string(), None) };
        #[cfg(not(feature = "voice_pane"))]
        let wire = text.to_string();
        #[cfg(feature = "voice_pane")]
        if conversation && answer.is_none() {
            if let Some(epoch)=crate::conversation::front_epoch() {
                if let Some(nonce)=c.front_enqueue(crate::front_native::Submission{text,wire:&wire,interruption:interruption.clone(),reply_voice:reply_voice.clone(),epoch},&crate::front_native::page(win),log) {return Ok(nonce);}
            }
        }
        let sent = c.say_session(&wire, reply_voice.as_ref());
        #[cfg(feature = "voice_pane")]
        if sent.is_ok() { crate::conversation::interruption_sent(interruption); }
        #[cfg(feature = "voice_pane")]
        if sent.is_err() {
            crate::settings::reply_unsent();
            if conversation { crate::conversation::unsent(); }
        }
        session_send_result(sent, |line, frame| {
            reported = true;
            log.line(&line);
            let _ = win.eval(format!("window.__rthReply && window.__rthReply({frame})"));
        })
    })();
    if let Err(e) = &r {
        log.line("chat: a session turn was not sent");

        session_preflight_error(origin, reported, e, |frame| { let _ = win.eval(format!("window.__rthReply && window.__rthReply({frame})")); });
        #[cfg(feature = "voice_pane")]
        if let SessionOrigin::Dictated(epoch) | SessionOrigin::Composer(epoch) = origin { crate::conversation::unsent_epoch(epoch); }
    }
    if let Some(answer) = answer { let _ = answer.0.send(r); }
}

fn session_preflight_error(origin: SessionOrigin, reported: bool, error: &str, report: impl FnOnce(Value)) {
    if !reported && origin != SessionOrigin::Typed { report(json!({"error": error})); }
}

fn session_send_result(sent: Result<String, String>, report: impl FnOnce(String, Value)) -> Result<String, String> {
    sent.map_err(|e| {
        report(format!("chat: a session turn was not sent: {e}"), json!({"error": e}));
        "the chat did not send it".into()
    })
}

static LEASE: Mutex<Option<Lease>> = Mutex::new(None);

pub fn plan_path(contents: &Path) -> PathBuf {
    contents.join("Resources/activation/plan.json")
}
pub fn product_path(contents: &Path) -> PathBuf {
    contents.join("Resources/activation/product.json")
}

pub fn plan_instance_dir(contents: &Path) -> Result<String, String> {
    let p = plan_path(contents);
    let v = rth_activation::record::read_json_strict(&p).map_err(|e| format!("{}: {e}", p.display()))?.ok_or(format!("{} is missing", p.display()))?;
    v["instance_dir"].as_str().map(str::to_string).ok_or(format!("{}: no instance_dir", p.display()))
}

pub fn contents(cfg: &Config) -> Result<PathBuf, String> {
    cfg.contents.clone().ok_or_else(|| "no bundle Contents".into())
}

#[tauri::command]
pub fn rth_setup(webview: tauri::Webview, action: String, key: Option<String>, models: Option<Value>, text: Option<String>) -> Result<(), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if !local_page(&url) {
        return Err("setup is only available from RTH's own welcome page".into());
    }
    let a = parse_action(&action, key, models, text)?;
    match SETUP.lock().unwrap().as_ref() {
        Some(tx) => tx.send(a).map_err(|_| "setup is closed".to_string()),
        None => Err("setup is not open".into()),
    }
}

fn local_page(url: &tauri::Url) -> bool {
    url.scheme() == "tauri" && url.host_str() == Some("localhost")
}

pub const SAY_MAX: usize = 8000;

fn parse_action(action: &str, key: Option<String>, models: Option<Value>, text: Option<String>) -> Result<Action, String> {
    let plain = models.is_none() && text.is_none();
    match action {
        "setup" if plain && key.as_deref().is_none_or(|k| k.trim().is_empty()) => Ok(Action::Setup),
        "setup" if plain => Err("setup: this version takes no key (voice is off)".into()),
        "remove_key" if plain && key.is_none() => Ok(Action::RemoveKey),
        "refresh" if plain && key.is_none() => Ok(Action::Refresh),
        "models" if key.is_none() && text.is_none() => match models {
            Some(m) if m.is_object() => Ok(Action::Models(m)),
            _ => Err("models: a {part: id} choice is required".into()),
        },
        "say" | "say_spoken" if key.is_none() && models.is_none() => match text.map(|t| t.trim().to_string()) {
            Some(t) if !t.is_empty() && t.chars().count() <= SAY_MAX => Ok(Action::Say(t, action == "say_spoken")),
            Some(t) if !t.is_empty() => Err(format!("say: at most {SAY_MAX} characters")),
            _ => Err("say: empty".into()),
        },
        "release_effect" | "cancel_effect" if key.is_none() && models.is_none() => match text {
            Some(id) if crate::effects::valid_id(&id) => Ok(if action == "release_effect" { Action::Release(id) } else { Action::CancelEffect(id) }),
            _ => Err(format!("{action}: one effect id")),
        },
        "dsh_undo" if key.is_none() && models.is_none() => match text {
            Some(id) if crate::dsh::change_id_ok(&id) => Ok(Action::DshUndo(id)),
            _ => Err("dsh_undo: one change id".into()),
        },
        _ => Err("unknown setup action".into()),
    }
}

fn show(win: &WebviewWindow, v: &Value) {
    let _ = win.eval(format!("window.__rthSetup && window.__rthSetup({v})"));
}

fn fresh_service() -> Result<Value, String> {
    service::discover_autopg(&crate::home()).map(|b| service::binding_json(&b)).map_err(|e| format!("{}\n{e}", service::MEMORY_UNAVAILABLE))
}

#[derive(Debug, PartialEq)]
pub enum Next {

    Admit,

    Ask,
}

pub fn next(st: &Value) -> Next {

    if st["admitted"] == true {
        Next::Admit
    } else {
        Next::Ask
    }
}

fn with_progress<T>(data: &Path, win: &WebviewWindow, f: impl FnOnce() -> T) -> T {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (s, d, w) = (stop.clone(), data.to_path_buf(), win.clone());
    let poll = std::thread::spawn(move || {
        while !s.load(std::sync::atomic::Ordering::SeqCst) {
            show(&w, &progress_state(&seam::records_status(&d)));
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let r = f();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = poll.join();
    r
}

pub fn progress_state(records: &Value) -> Value {
    json!({"screen": "activating", "steps": records["operation"]["steps"]})
}

pub(crate) trait OpenAiKey {

    fn key_status(&self, data: &Path) -> Value;

    fn remove_key(&self) -> Result<bool, String>;
}

impl OpenAiKey for KeyStore {
    fn key_status(&self, data: &Path) -> Value {
        match crate::keychain_call() {
            Ok(()) => seam::setup_status(data, self),
            Err(e) => {
                let mut v = seam::records_status(data);
                v["key_present"] = json!({"error": e});
                v
            }
        }
    }
    fn remove_key(&self) -> Result<bool, String> {
        crate::keychain_call()?;
        seam::remove_key(self)
    }
}

#[cfg(feature = "rehearsal_pack")]
impl OpenAiKey for crate::scratch_keys::Item<'static> {
    fn key_status(&self, data: &Path) -> Value {
        let mut v = seam::records_status(data);
        v["key_present"] = match self.present() {
            Ok(p) => json!(p),
            Err(e) => json!({"error": e}),
        };
        v
    }
    fn remove_key(&self) -> Result<bool, String> {
        self.clear()
    }
}

#[cfg(not(feature = "rehearsal_pack"))]
pub(crate) fn openai_key() -> KeyStore {
    KeyStore::user_default()
}

#[cfg(feature = "rehearsal_pack")]
pub(crate) fn openai_key() -> crate::scratch_keys::Item<'static> {
    crate::scratch_keys::Item { service: rth_activation::keychain::SERVICE, account: rth_activation::keychain::ACCOUNT }
}

pub fn gate(cfg: &Config, h: &InstanceHomes, win: &WebviewWindow, owned: &Shared, log: &Log) -> Result<(), String> {
    let c = contents(cfg)?;
    let keys = openai_key();
    let (tx, rx) = mpsc::channel();
    *SETUP.lock().unwrap() = Some(tx);
    let close = || *SETUP.lock().unwrap() = None;
    let mut note: Option<String> = None;
    let mut home;

    let catalog = crate::models::load_catalog(&c).map_err(|e| log.line(&format!("models: {e}"))).ok();
    let mut chat: Option<crate::chat::Chat> = None;
    let mut admitted_record: Option<Value> = None;

    let mut resume_tried = false;
    let r = loop {
        if cancelled(owned) {
            break Err(CLOSING.to_string());
        }
        home = false;
        let mut st = keys.key_status(&cfg.data);
        log.line(&format!("setup: state {} admitted={} key_present={}", st["activation_state"], st["admitted"], st["key_present"].is_boolean().then(|| st["key_present"].to_string()).unwrap_or_else(|| "error".into())));
        if next(&st) == Next::Admit {
            match fresh_service().and_then(|s| seam::admit(&cfg.data, &s)) {
                Ok(a) => {
                    let root = a.record["instance_root"].as_str().map(PathBuf::from);
                    if root.as_deref() != Some(h.instance.as_path()) {
                        break Err(format!("the admitted instance {:?} is not this app's instance {} (recovery)", root, h.instance.display()));
                    }
                    *LEASE.lock().unwrap() = Some(a.lease);
                    log.line(&format!("admitted: instance {} (activation {})", a.record["instance_id"], a.record["op_id"]));

                    log.line("setup: admitted: home");
                    home = true;
                    admitted_record = Some(a.record.clone());

                    let first = !std::mem::replace(&mut resume_tried, true);
                    if let (true, None, Some(cat), Some(saved)) = (first, &chat, &catalog, crate::models::load_choice(&cfg.data)) {

                        let (ch, moved) = crate::models::migrate_choice(cat, &saved);
                        match crate::models::validate_choice(cat, &ch).and_then(|ch| prepare_chat(cfg, &a.record, cat, &ch, win, owned, log).map(|c| (c, ch))) {
                            Ok((c, ch)) => {
                                if moved {
                                    match crate::models::save_choice(&cfg.data, &ch) {
                                        Ok(()) => {
                                            log.line(&format!("setup: saved model choice moved to {ch}"));
                                            for f in crate::models::retire_replaced(cat, &saved, &ch, &cfg.data) {
                                                log.line(&format!("models: removed the replaced {}", f.display()));
                                            }
                                        }
                                        Err(e) => log.line(&format!("setup: saved model choice moved, not saved: {e}")),
                                    }
                                }
                                chat = Some(c);
                            }
                            Err(e) if moved => {
                                log.line(&format!("chat: {ch} not woken ({e}): the saved choice serves until the next launch"));
                                match prepare_chat(cfg, &a.record, cat, &saved, win, owned, log) {
                                    Ok(c) => chat = Some(c),
                                    Err(e) => {
                                        log.line(&format!("chat: not resumed: {e}"));
                                        note = Some(e);
                                    }
                                }
                            }
                            Err(e) => {
                                log.line(&format!("chat: not resumed: {e}"));
                                note = Some(e);
                            }
                        }
                    }
                }
                Err(e) => {
                    log.line(&format!("not admitted: {e}"));
                    note = Some(e);
                }
            }
        }

        if chat.as_ref().is_some_and(|c| !c.alive()) {
            #[cfg(feature = "menu_bar")]
            crate::dsh_child::lease_gone();
            chat = None;
            log.line("chat: the connection ended: stopping the local chat for a retry");
            crate::stop_local(owned, log);
            note.get_or_insert_with(|| "Its model stopped answering. Try again, or choose another model.".into());
        }
        st["screen"] = json!(if home { "home" } else { "setup" });
        if home {
            if let Some(cat) = &catalog {
                st["models"] = crate::models::offer(cat, &cfg.data, &crate::home());
                if let Some(ch) = crate::models::load_choice(&cfg.data) {
                    st["chosen"] = ch;
                }

                st["cloud"] = crate::flash::page_state(&cfg.data, crate::settings::current().answer_with != crate::settings::ANSWER_GRANITE);
            }
            if chat.is_some() {
                st["chat"] = json!("ready");

                if let Some((why, local)) = crate::flash::local_reason().or_else(crate::dsh::local_reason) {
                    st["cloud"]["local_reason"] = json!(why);
                    st["cloud"]["local_model"] = json!(local);
                }
            }
        }
        if let Some(n) = note.take() {
            st["note"] = json!(n);
        }
        show(win, &st);
        log.line("setup: waiting for the welcome page");

        let a = loop {
            match rx.recv_timeout(Duration::from_millis(500)) {

                Ok(Action::Say(text, spoken)) => {
                    #[cfg(feature = "voice_pane")]
                    if crate::conversation::active() {
                        if let Err(e) = crate::conversation::send_page(&text) {
                            log.line("conversation: manual turn was not sent");
                            let _ = win.eval(format!("window.__rthReply && window.__rthReply({})", json!({"error": e})));
                        }
                        continue;
                    }

                    #[cfg(feature = "voice_pane")]
                    let reply_voice = crate::settings::reply_turn(spoken);
                    #[cfg(not(feature = "voice_pane"))]
                    let reply_voice: Option<Value> = {
                        let _ = spoken;
                        None
                    };
                    let r = match chat.as_mut() {
                        Some(c) => c.say(&text, reply_voice.as_ref()),
                        None => Err("It is still waking: choose its models first.".into()),
                    };
                    if let Err(e) = r {
                        log.line(&format!("chat: {e}"));
                        #[cfg(feature = "voice_pane")]
                        crate::settings::reply_unsent();
                        let _ = win.eval(format!("window.__rthReply && window.__rthReply({})", json!({"error": e})));
                    }
                }
                #[cfg(feature = "voice_pane")]
                Ok(Action::ConversationCancel) => {
                    if let Some(c) = chat.as_mut() {
                        match c.interrupt() {
                            Ok(sent) => log.line(if sent { "conversation: cancel requested" } else { "conversation: model already finished" }),
                            Err(e) => {
                                log.line(&format!("conversation: cancel failed: {e}"));
                                let _ = win.eval(format!("window.__rthReply && window.__rthReply({})", json!({"error":"Could not interrupt this reply."})));
                            }
                        }
                    }
                }
                Ok(Action::SessionSay(text, origin, answer)) => session_turn(chat.as_mut(), &text, origin, answer, win, log),

                Ok(Action::Release(id)) => {
                    let r = match chat.as_mut() {
                        Some(c) => c.release(&id),
                        None => Err("the chat is not connected".into()),
                    };
                    match r {
                        Ok(true) => log.line(&format!("effect: release sent for {id}")),
                        Ok(false) | Err(_) => {
                            log.line(&format!("effect: release for {id} not sent ({})", r.err().unwrap_or_else(|| "no live card".into())));
                            let _ = win.eval(format!("window.__rthMemory && window.__rthMemory({})", json!({"wire": "effect_update", "effect_id": id, "state": "refused", "reason": "not_sent"})));
                        }
                    }
                }
                Ok(Action::CancelEffect(id)) => {
                    if crate::effects::CARDS.lock().unwrap().cancel(&id) {
                        log.line(&format!("effect: {id} cancelled here (no release sent)"));
                    }
                }

                Ok(Action::DshUndo(id)) => {
                    let r = match chat.as_mut() {
                        Some(c) => c.dsh_undo(&id),
                        None => Err("the chat is not connected".into()),
                    };
                    match r {
                        Ok(()) => log.line(&format!("dsh watch: undo sent for {id}")),
                        Err(e) => {
                            log.line(&format!("dsh watch: undo for {id} not sent ({e})"));
                            let _ = win.eval(format!("window.__rthMemory && window.__rthMemory({})", json!({"wire": "dsh_undo", "change_id": id, "state": "refused"})));
                        }
                    }
                }
                Ok(a) => break Some(a),
                Err(mpsc::RecvTimeoutError::Timeout) if !cancelled(owned) => continue,
                _ => break None,
            }
        };
        let Some(a) = a else { continue };
        match a {
            Action::Refresh => {}
            Action::Models(choice) => {

                if let Some(on) = choice["use_cloud"].as_bool() {
                    if let Err(e) = crate::settings::set_answer_from_welcome(on) {
                        log.line(&format!("settings: answer_with not saved: {e}"));
                    }
                }
                let Some(cat) = &catalog else {
                    note = Some("This build has no model catalog.".into());
                    continue;
                };
                match crate::models::validate_choice(cat, &choice).and_then(|ch| crate::models::save_choice(&cfg.data, &ch).map(|()| ch)) {
                    Err(e) => note = Some(e),
                    Ok(ch) => {
                        log.line(&format!("models: chosen {ch}"));
                        if let Some(rec) = &admitted_record {

                            #[cfg(feature = "menu_bar")]
                            crate::dsh_child::lease_gone();
                            if chat.take().is_some() {
                                log.line("chat: restarting on the chosen model");
                                crate::stop_local(owned, log);
                            }
                            match prepare_chat(cfg, rec, cat, &ch, win, owned, log) {
                                Ok(c) => chat = Some(c),
                                Err(e) => {
                                    log.line(&format!("chat: not started: {e}"));
                                    note = Some(e);
                                }
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "voice_pane")]
            Action::ConversationCancel => unreachable!("handled while waiting"),
            Action::Say(..) | Action::SessionSay(..) | Action::Release(_) | Action::CancelEffect(_) | Action::DshUndo(_) => unreachable!("handled while waiting"),
            Action::RemoveKey => match keys.remove_key() {
                Ok(had) => note = Some(if had { "The key was removed from your Keychain.".into() } else { "There was no key to remove.".into() }),
                Err(e) => note = Some(e),
            },
            Action::Setup => {
                let admitted = keys.key_status(&cfg.data)["admitted"] == true;
                log.line(&format!("setup: {}", if admitted { "already admitted" } else { "activate" }));
                if !admitted {
                    show(win, &json!({"screen": "activating"}));
                    status(win, log, "setting up this Mac (activation)…");
                    let v = with_progress(&cfg.data, win, || seam::activate(&c.join("MacOS/rth-activate"), &cfg.data, &plan_path(&c), &product_path(&c)));
                    match v.get("error") {
                        None => log.line("setup: the activation finished"),
                        Some(e) => {
                            let e = e.as_str().unwrap_or("the activation failed").to_string();
                            log.line(&format!("setup: the activation refused: {e}"));
                            note = Some(e);
                        }
                    }
                }
            }
        }
    };
    close();
    r
}

fn prepare_chat(cfg: &Config, record: &Value, cat: &Value, choice: &Value, win: &WebviewWindow, owned: &Shared, log: &Log) -> Result<crate::chat::Chat, String> {
    let talk = choice["talk"].as_str().ok_or("no talk model chosen")?;
    let last = std::sync::Mutex::new(std::time::Instant::now() - Duration::from_secs(1));
    let progress = |what: &str, done: u64, total: u64| {
        let mut l = last.lock().unwrap();
        if l.elapsed() < Duration::from_millis(400) && done != 0 {
            return;
        }
        *l = std::time::Instant::now();
        let pct = if total > 0 { done * 100 / total } else { 0 };
        show(win, &json!({"screen": "waiting", "note": format!("{what}… {pct}%")}));
    };
    let r = (|| {
        let model = crate::models::ensure(cat, talk, &cfg.data, &crate::home(), &progress)?;
        log.line(&format!("models: {talk} ready at {}", model.display()));
        show(win, &json!({"screen": "waiting", "note": "Waking it…"}));
        crate::chat::start(cfg, record, cat, talk, &model, win, owned, log)
    })();
    if r.is_err() {

        crate::stop_local(owned, log);
    } else {

        crate::updater::agent_ready();
    }

    #[cfg(feature = "menu_bar")]
    if let Ok(c) = &r {
        let (bind, session) = c.lease();
        crate::dsh_child::lease_held(bind, session);
    }
    r.map_err(|e| prep_note(talk, &e))
}

pub fn prep_note(talk: &str, e: &str) -> String {
    format!("Couldn't wake it with {talk}: {e}")
}

pub fn start_local_agent(cfg: &Config, record: &Value, llm_url: &str, model_id: &str, model_sha256: Option<&str>, ctx: Option<u32>, slots: Vec<(String, String)>, owned: &Shared, log: &Log, app_dir: &Path) -> Result<(SocketAddr, u32), String> {
    let c = contents(cfg)?;
    let bind = crate::free_loopback()?;
    let bundle = product::BrainBundle { brain_mcp: c.join("MacOS/brain-mcp"), brain_src: c.join("Resources/brain-seam/src"), seam: c.join("Resources/brain-seam/seam/memory-search.ts"), bun: c.join("MacOS/bun") };
    let mut env = with_app_version(with_local_ctx(with_model_sha256(local_launch_env(record, &bundle, bind, &crate::home(), llm_url, model_id)?, model_sha256)?, ctx));

    env.extend(slots);
    let name = crate::settings::model_name_of(model_id);
    let env = with_providers(env, &crate::settings::current(), &crate::dsh::source(), |env| crate::flash::prepare(env, &cfg.data, &name, log), &name, log);

    #[cfg(feature = "menu_bar")]
    crate::dsh_child::key_check();
    let root = PathBuf::from(record["instance_root"].as_str().ok_or("no instance_root")?);
    let out = owned_open(&app_dir.join("agentd.log"), Open::Truncate).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(c.join("MacOS/rth-agentd"));
    cmd.env_clear().envs(env).current_dir(root.join("agentd")).stdin(Stdio::null()).stdout(out).stderr(err);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    match LEASE.lock().unwrap().as_ref() {
        Some(l) => l.inherit_into(&mut cmd),
        None => return Err("not admitted: no lease is held".into()),
    }
    let (pid, _) = spawn_owned(owned, Slot::Agent, &mut cmd)?;
    log.line(&format!("started rth-agentd (pid {pid}) on {bind}, backend {}, admitted", crate::flash::hello_prefix(model_id)));
    let prefix = crate::flash::hello_prefix(model_id);
    let hello = until(owned, Duration::from_secs(30), || {
        if port_open(bind) {
            return Some(verify_hello_for(bind, pid, Duration::from_secs(2), &prefix));
        }
        let mut o = owned.lock().unwrap();
        match o.agent.as_mut().map(|c| c.try_wait()) {
            Some(Ok(Some(s))) => Some(Err(format!("rth-agentd exited before listening ({s}); see {}", app_dir.join("agentd.log").display()))),
            _ => None,
        }
    })?;
    match hello {
        Some(r) => r.map(|_| (bind, pid)),
        None => Err(format!("rth-agentd did not listen on {bind} within 30 s")),
    }
}

pub fn with_providers(env: Vec<(String, String)>, s: &crate::settings::Settings, endpoint: &crate::dsh::Source, flash: impl FnOnce(Vec<(String, String)>) -> Vec<(String, String)>, local_name: &str, log: &Log) -> Vec<(String, String)> {
    let ask = crate::dsh::Ask { automagik: s.automagik_on(), answer: &s.answer_with, watch: s.use_dsh_watch };
    let p = crate::dsh::prepare(&env, endpoint, ask, log);
    let env = match p.dsh {
        Some(env) => {
            crate::flash::paused_for_dsh(local_name, log);
            env
        }
        None => flash(env),
    };
    crate::dsh::with_watch(env, p.watch.as_ref())
}

fn local_launch_env(record: &Value, bundle: &product::BrainBundle, bind: SocketAddr, home: &Path, llm_url: &str, model_id: &str) -> Result<Vec<(String, String)>, String> {
    product::admitted_launch_local(record, bundle, bind, "/usr/bin:/bin", home, llm_url, model_id).map_err(|e| format!("not admitted: {e}"))
}

pub fn with_model_sha256(mut env: Vec<(String, String)>, sha: Option<&str>) -> Result<Vec<(String, String)>, String> {
    if let Some(s) = sha {
        if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(format!("the catalog sha256 {s:?} is not 64 lowercase hex"));
        }
        env.push(("RTH_LOCAL_MODEL_SHA256".into(), s.into()));
    }
    Ok(env)
}

#[used]
static APP_VERSION_MARK: &str = concat!("unfold-app-version=", env!("UNFOLD_APP_VERSION"), ";");

pub fn with_app_version(mut env: Vec<(String, String)>) -> Vec<(String, String)> {
    env.push(("RTH_APP_VERSION".into(), env!("UNFOLD_APP_VERSION").into()));
    env
}

pub fn with_local_ctx(mut env: Vec<(String, String)>, ctx: Option<u32>) -> Vec<(String, String)> {
    if let Some(n) = ctx {
        env.push(("RTH_LOCAL_CTX".into(), n.to_string()));
    }
    env
}

pub fn verify_hello_for(bind: SocketAddr, pid: u32, within: Duration, backend_prefix: &str) -> Result<String, String> {
    let mut s = TcpStream::connect_timeout(&bind, within).map_err(|e| format!("agent Hello: connect: {e}"))?;
    s.set_read_timeout(Some(within)).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buf.contains(&b'\n') {
        if buf.len() > 64 * 1024 {
            return Err("agent Hello: first frame larger than 64 KiB".into());
        }
        let n = s.read(&mut chunk).map_err(|e| format!("agent Hello: {e}"))?;
        if n == 0 {
            return Err("agent Hello: closed before a Hello".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let line = &buf[..buf.iter().position(|&b| b == b'\n').unwrap()];
    hello_boot_for(line, pid, backend_prefix)
}

pub fn hello_boot_for(line: &[u8], pid: u32, backend_prefix: &str) -> Result<String, String> {
    let h: Value = serde_json::from_slice(line).map_err(|_| "agent Hello: the first frame is not JSON")?;
    if h["v"] != 2 || h["kind"] != "hello" || h["source"] != "guest" {
        return Err("agent Hello: the first frame is not a v2 guest Hello".into());
    }
    if !h["backend"].as_str().is_some_and(|b| crate::flash::accept(b, backend_prefix)) {
        return Err(format!("agent Hello: backend {} is not {}", h["backend"], backend_prefix.trim_end_matches('@')));
    }
    match h["boot_id"].as_str() {
        Some(b) if b.ends_with(&format!("-{pid}")) => Ok(b.to_string()),
        b => Err(format!("agent Hello: boot {b:?} is not the launched agent (pid {pid})")),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_turn_typed_in_unfold_automagik_reaches_the_page_as_one_json_string() {
        let text = "oi \"Lua\" </script> \u{2028} 'x'); alert(1); ('";
        let js = super::session_said_js(text);
        let arg = js.strip_prefix("window.__rthSessionSaid && window.__rthSessionSaid(").and_then(|r| r.strip_suffix(')')).expect("one call");
        let v: serde_json::Value = serde_json::from_str(arg).expect("its argument is one JSON value");
        assert_eq!(v, serde_json::json!({"text": text}), "the text whole, as data");
    }

    #[test]
    fn a_session_turn_is_a_typed_turn_only_and_its_error_never_holds_the_text() {
        use super::{session_say, SAY_MAX};

        assert_eq!(session_say("/effect release x"), Err("a command, not a turn".into()));
        assert_eq!(session_say("  /remember this  "), Err("a command, not a turn".into()));
        assert_eq!(session_say("   "), Err("not a turn".into()));
        assert_eq!(session_say(&"x".repeat(SAY_MAX + 1)), Err("not a turn".into()));
        let e = session_say("my words here").unwrap_err();
        assert!(!e.contains("my words"), "{e}");
    }

    #[cfg(feature = "voice_pane")]
    #[test]
    fn a_conversation_endpoint_and_dsh_typed_turn_use_the_same_session_sender() {
        let (tx, rx) = mpsc::channel();
        enqueue_session(&tx, "spoken words".into(), SessionOrigin::Dictated(7), None).unwrap();
        let (answer, _) = mpsc::channel();
        enqueue_session(&tx, "typed words".into(), SessionOrigin::Typed, Some(Answer(answer))).unwrap();
        enqueue_session(&tx, "composer words".into(), SessionOrigin::Composer(8), None).unwrap();
        assert!(matches!(rx.recv().unwrap(), Action::SessionSay(t, SessionOrigin::Dictated(7), None) if t == "spoken words"));
        assert!(matches!(rx.recv().unwrap(), Action::SessionSay(t, SessionOrigin::Typed, Some(_)) if t == "typed words"));
        assert!(matches!(rx.recv().unwrap(), Action::SessionSay(t, SessionOrigin::Composer(8), None) if t == "composer words"));
    }

    #[test]
    fn a_refused_dsh_preflight_never_evaluates_a_page_reply() {
        let mut frames = Vec::new();
        for reported in [false, true] {
            session_preflight_error(SessionOrigin::Typed, reported, "wait for this reply to finish", |frame| frames.push(frame));
        }
        assert!(frames.is_empty(), "a refused DSH turn must leave the live reply untouched");
        #[cfg(feature = "voice_pane")]
        for origin in [SessionOrigin::Dictated(7), SessionOrigin::Composer(8)] {
            session_preflight_error(origin, false, "the chat is not ready", |frame| frames.push(frame));
            assert_eq!(frames.pop(), Some(json!({"error": "the chat is not ready"})), "native preflight failures remain visible");
            session_preflight_error(origin, true, "already reported", |frame| frames.push(frame));
            assert!(frames.is_empty(), "a failure is reported only once");
        }
    }

    #[test]
    fn review_r3_a_failed_session_send_reports_the_reason_to_the_page_and_log() {
        let notices = std::cell::RefCell::new(Vec::new());
        let error = "could not send to it: connection closed \"quoted\"";
        let result = session_send_result(Err(error.into()), |line, page| notices.borrow_mut().push((line, page)));
        assert_eq!(result, Err("the chat did not send it".into()));
        let notices = notices.into_inner();
        assert_eq!(notices.len(), 1, "the page reply must terminate with an error");
        assert_eq!(notices[0].0, format!("chat: a session turn was not sent: {error}"));
        assert_eq!(notices[0].1, json!({"error":error}));
        assert_eq!(session_send_result(Ok("command-id".into()), |_, _| panic!("a sent turn has no error")), Ok("command-id".into()));
    }

    #[test]
    fn a_failed_preparation_names_the_model_and_the_reason() {
        assert_eq!(super::prep_note("granite41-3b-q4km", "its model did not load within 180 s"), "Couldn't wake it with granite41-3b-q4km: its model did not load within 180 s");
    }

    #[test]
    fn agentd_is_told_the_apps_version() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let v = conf["version"].as_str().unwrap();
        assert_eq!(super::with_app_version(vec![("A".into(), "1".into())]), [("A".to_string(), "1".to_string()), ("RTH_APP_VERSION".to_string(), v.to_string())]);
    }

    #[test]
    fn local_ctx_env() {
        assert_eq!(super::with_local_ctx(vec![("A".into(), "1".into())], Some(8192)).last().unwrap(), &("RTH_LOCAL_CTX".to_string(), "8192".to_string()));
        assert_eq!(super::with_local_ctx(vec![], None), vec![]);
    }

    #[test]
    fn model_sha256_env() {
        let sha = "662b0626cd58f443baea23559b469df6576a81d349649c59413b36a9fb32eb29";
        let env = super::with_model_sha256(vec![("A".into(), "1".into())], Some(sha)).unwrap();
        assert_eq!(env.last().unwrap(), &("RTH_LOCAL_MODEL_SHA256".to_string(), sha.to_string()));
        assert_eq!(super::with_model_sha256(vec![], None).unwrap(), vec![]);
        assert!(super::with_model_sha256(vec![], Some(&sha.to_uppercase())).is_err());
        assert!(super::with_model_sha256(vec![], Some(&sha[1..])).is_err());
    }

    use super::*;

    #[test]
    fn setup_takes_no_key_and_a_stored_one_never_selects_a_voice() {

        assert_eq!(parse_action("setup", None, None, None), Ok(Action::Setup));
        assert_eq!(parse_action("setup", Some(" \t\n".into()), None, None), Ok(Action::Setup));
        assert!(parse_action("setup", Some("sk-abc".into()), None, None).unwrap_err().contains("no key"));
        assert_eq!(next(&json!({"admitted": true, "key_present": true})), Next::Admit);
    }

    #[test]
    fn hello_must_be_this_agent_on_its_backend() {
        let ok = br#"{"v":2,"kind":"hello","source":"guest","backend":"local@granite41-3b-q4km","boot_id":"b-123-4242"}"#;
        assert_eq!(hello_boot_for(ok, 4242, "local@"), Ok("b-123-4242".into()));
        assert!(hello_boot_for(ok, 4243, "local@").unwrap_err().contains("not the launched agent"));
        assert!(hello_boot_for(br#"{"v":2,"kind":"hello","source":"guest","backend":"fake@1","boot_id":"b-4242"}"#, 4242, "local@").unwrap_err().contains("not local"));
        assert!(hello_boot_for(br#"{"v":1,"kind":"hello","source":"guest"}"#, 1, "local@").unwrap_err().contains("v2 guest"));
        assert!(hello_boot_for(b"not json", 1, "local@").unwrap_err().contains("not JSON"));
    }

    #[test]
    fn progress_passes_the_operation_steps() {
        let r = json!({"operation": {"op_id": "op", "steps": {"roots": "done", "fence": "recorded"}, "rollback": false}});
        assert_eq!(progress_state(&r), json!({"screen": "activating", "steps": {"roots": "done", "fence": "recorded"}}));
        assert_eq!(progress_state(&json!({"operation": null})), json!({"screen": "activating", "steps": null}));
    }

    #[test]
    fn the_gate_admits_on_the_records_alone() {
        assert_eq!(next(&json!({"admitted": true, "key_present": true})), Next::Admit);
        assert_eq!(next(&json!({"admitted": true, "key_present": false})), Next::Admit, "keyless admission");
        assert_eq!(next(&json!({"admitted": false, "key_present": true})), Next::Ask);
        assert_eq!(next(&json!({"admitted": true, "key_present": {"error": "keychain locked"}})), Next::Admit);
        assert_eq!(next(&json!({"activation_state": "unreadable", "admitted": false})), Next::Ask);
    }

    #[test]
    fn only_the_bundled_page_and_known_actions_reach_setup() {
        let u = |s: &str| tauri::Url::parse(s).unwrap();
        assert!(local_page(&u("tauri://localhost/index.html")));
        for bad in ["http://127.0.0.1:3790/", "http://localhost/", "https://tauri.localhost/", "file:///index.html", "tauri://evil/index.html"] {
            assert!(!local_page(&u(bad)), "{bad}");
        }
        assert!(parse_action("setup", Some("sk-x".into()), None, None).is_err());
        assert_eq!(parse_action("setup", Some("  ".into()), None, None), Ok(Action::Setup));
        assert_eq!(parse_action("remove_key", None, None, None), Ok(Action::RemoveKey));
        assert!(parse_action("remove_key", Some("k".into()), None, None).is_err());
        assert!(parse_action("read_key", None, None, None).is_err());

        assert_eq!(parse_action("models", None, Some(json!({"talk": "a"})), None), Ok(Action::Models(json!({"talk": "a"}))));
        assert!(parse_action("models", None, Some(json!("a")), None).is_err());
        assert!(parse_action("models", Some("sk-x".into()), Some(json!({})), None).is_err());
        assert_eq!(parse_action("say", None, None, Some("  oi  ".into())), Ok(Action::Say("oi".into(), false)));
        assert_eq!(parse_action("say_spoken", None, None, Some(" oi ".into())), Ok(Action::Say("oi".into(), true)));
        assert!(parse_action("say_spoken", None, None, Some("  ".into())).is_err());
        assert!(parse_action("say", None, None, Some("   ".into())).is_err());
        assert!(parse_action("say", None, None, Some("x".repeat(SAY_MAX + 1))).is_err());
        assert_eq!(parse_action("release_effect", None, None, Some("b1-e7".into())), Ok(Action::Release("b1-e7".into())));
        assert_eq!(parse_action("cancel_effect", None, None, Some("b1-e7".into())), Ok(Action::CancelEffect("b1-e7".into())));
        assert!(parse_action("release_effect", None, None, Some("b1-e7 tok".into())).is_err(), "an id only: a token never comes from the page");
        assert!(parse_action("release_effect", Some("k".into()), None, Some("b1-e7".into())).is_err());

        assert_eq!(parse_action("dsh_undo", None, None, Some("c-1777:2".into())), Ok(Action::DshUndo("c-1777:2".into())));
        for bad in ["", "c-1 /effect", "c-1\n", &"c".repeat(129)] {
            assert!(parse_action("dsh_undo", None, None, Some(bad.to_string())).is_err(), "{bad:?}");
        }
        assert!(parse_action("dsh_undo", Some("k".into()), None, Some("c-1".into())).is_err());
        assert!(parse_action("setup", None, Some(json!({})), None).is_err(), "setup never carries a model choice");
        assert!(rth_setup_closed());
    }

    fn rth_setup_closed() -> bool {
        SETUP.lock().unwrap().is_none()
    }

}
