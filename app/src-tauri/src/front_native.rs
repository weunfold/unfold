
use crate::voice_front::{
    capture::{Capture, Reader},
    policy::Selection,
    runtime::*,
    worker::{self, Input, Notice, Update, Worker},
    Error, Result,
};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static CURRENT: Mutex<Option<Weak<Route>>> = Mutex::new(None);

pub enum PageEvent { Status(String), Reply(Value) }
pub type Page = Arc<dyn Fn(PageEvent) + Send + Sync>;
pub fn page(win: &tauri::WebviewWindow) -> Page {
    let win = win.clone();
    Arc::new(move |event| {
        let (hook, value) = match event {
            PageEvent::Status(text) => ("__rthStatus", json!(text)),
            PageEvent::Reply(value) => ("__rthReply", value),
        };
        let _ = win.eval(format!("window.{hook} && window.{hook}({value})"));
    })
}
pub(crate) struct Route {
    capture: Arc<Capture>,
    pub delivery: Arc<Mutex<()>>,
    order: Arc<Mutex<worker::BackendOrder>>,
    activated: AtomicBool,
    pub(crate) owner: Arc<Mutex<Option<Epoch>>>,
    delivery_unknown: std::sync::atomic::AtomicU64,
    order_refusals: std::sync::atomic::AtomicU64,
    worker: Mutex<Option<Weak<Worker>>>,
    closed: AtomicBool,
    presentation: Mutex<(Option<Page>, Option<String>)>,
}
impl Default for Route {
    fn default() -> Self {
        Self {
            capture: Arc::new(Capture::new()),
            delivery: Default::default(),
            order: Default::default(),
            activated: AtomicBool::new(false),
            owner: Arc::new(Mutex::new(None)),
            delivery_unknown: std::sync::atomic::AtomicU64::new(0),
            order_refusals: std::sync::atomic::AtomicU64::new(0),
            worker: Mutex::new(None),
            closed: AtomicBool::new(false),
            presentation: Mutex::new((None, None)),
        }
    }
}
impl Route {
    pub(crate) fn order_refused(&self, log: &crate::Log) {
        let n = self.order_refusals.fetch_add(1, Ordering::SeqCst) + 1;
        log.line(&format!("voice_front: order_refusals={n}"));
        crate::floor_decide::journal("voice_front_counts", json!({"order_refusals":n}));
    }
    fn terminal(&self, awaiting: &AtomicBool, epoch: Epoch) {
        let mut owner = self.owner.lock().unwrap_or_else(|e|e.into_inner());
        if *owner == Some(epoch) {
            *owner = None;
            awaiting.store(false, Ordering::SeqCst);
        }
        drop(owner);
        crate::settings::front_terminal(epoch);
        crate::conversation::front_progress(epoch, true);
    }
    fn unknown(&self, awaiting: &AtomicBool, epoch: Epoch) -> u64 {
        let n = self.delivery_unknown.fetch_add(1, Ordering::SeqCst) + 1;
        self.close();
        awaiting.store(false, Ordering::SeqCst);
        self.terminal(awaiting, epoch);
        crate::floor_decide::journal(
            "voice_front_counts",
            json!({"mode":"LegacyOnly","delivery_unknown":n,"capture_arena":self.capture.metrics()}),
        );
        n
    }
    pub(crate) fn emit(&self, update: Update, awaiting: &AtomicBool, page: &Page, log: &crate::Log) {
                if self.closed.load(Ordering::SeqCst) {
                    return;
                }
                match update {
                    Update::Status(mode) => {
                        log.line(&format!("voice_front: mode={mode}"));
                        let line = if mode == "Ready" {
                            crate::settings::t("Conversation voice front ready.", "Voz da conversa disponível.")
                        } else {
                            crate::settings::t("Conversation sync unavailable; using the standard voice.", "Sincronização da conversa indisponível; usando a voz padrão.")
                        };
                        page(PageEvent::Status(line.into()));
                    }
                    Update::Rendered(epoch) => crate::conversation::front_progress(epoch, false),
                    Update::Terminal(epoch) => {
                        self.terminal(awaiting, epoch);
                    }
                    Update::Outcome(epoch, nonce, outcome) => {
                        crate::floor_decide::journal(
                            "voice_front",
                            json!({"capture":epoch.capture,"turn":epoch.turn,"nonce":nonce,"outcome":format!("{outcome:?}")}),
                        );
                        if matches!(outcome, Outcome::DeliveryUnknown) {
                            let n = self.unknown(awaiting, epoch);
                            let status = if crate::settings::pt() {
                                format!("Sincronização da conversa incerta; usando a voz padrão (turnos sem confirmação: {n}).")
                            } else {
                                format!("Conversation sync uncertain; using the standard voice (uncertain turns: {n}).")
                            };
                            page(PageEvent::Status(status));
                            page(PageEvent::Reply(json!({"error":crate::settings::t("The voice turn could not be confirmed, so it was not sent again. If no answer comes, say it again.","Não foi possível confirmar o turno de voz, então não houve reenvio. Se a resposta não vier, repita.")})));
                        }
                    }
                    Update::Counts(counts) => {
                        crate::floor_decide::journal("voice_front_counts", counts);
                    }
                    Update::Mark(mark) => {
                        if let Ok(row) = serde_json::to_value(mark) {
                            crate::floor_decide::journal("voice_front_stage", row);
                        }
                    }
                }
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.capture.close();
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(Weak::upgrade)
        {
            worker.close();
        }
    }
}
fn current() -> Option<Arc<Route>> {
    CURRENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(Weak::upgrade)
}

pub fn capture_reader() -> Option<Reader> {
    if !crate::settings::current().voice_front_templates {
        return None;
    }
    let route = current()?;
    (!route.closed.load(Ordering::SeqCst)).then(|| route.capture.reader())
}

pub fn fence() {
    if let Some(route) = current() {
        route.capture.fence();
    }
}

pub fn new_turn<T>(start: impl FnOnce() -> T) -> T {
    let Some(route) = current() else {
        return start();
    };
    let _delivery = route.delivery.lock().unwrap_or_else(|e| e.into_inner());
    route.capture.fence();
    route.activated.store(crate::settings::current().voice_front_templates, Ordering::SeqCst);
    if route.activated.load(Ordering::SeqCst) {
        route
            .order
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .begin();
    } else {
        route.order.lock().unwrap_or_else(|e|e.into_inner()).disable();
    }
    start()
}

pub fn flag_changed() {
    let Some(route) = current() else { return };
    let _delivery = route.delivery.lock().unwrap_or_else(|e|e.into_inner());
    route.capture.fence();
    let enabled = crate::settings::current().voice_front_templates;
    route.activated.store(enabled, Ordering::SeqCst);
    if !enabled { route.order.lock().unwrap_or_else(|e|e.into_inner()).disable(); }
}

pub fn before_typed() {
    let Some(route) = current() else { return };
    let epoch = {
        let mut owner = route.owner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(epoch) = (*owner).filter(|e| crate::conversation::front_epoch() == Some(*e))
        else {
            return;
        };
        route.capture.stop_user(crate::settings::front_stop);
        *owner = None;
        epoch
    };
    crate::settings::front_terminal(epoch);
    crate::conversation::front_progress(epoch, true);
}
pub fn close() {
    if let Some(route) = current() {
        route.close();
    }
}

pub struct Submission<'a> {
    pub text: &'a str,
    pub wire: &'a str,
    pub interruption: Option<String>,
    pub reply_voice: Option<Value>,
    pub epoch: Epoch,
}

#[derive(Default)]
pub struct Attachment {
    pub order: Arc<Mutex<worker::BackendOrder>>,
    pub worker: Option<Arc<Worker>>,
    route: Arc<Route>,
    session: String,
    boot: String,
}
impl Attachment {
    pub fn bind(&mut self, session: &str) {
        self.route.close();
        self.worker = None;
        self.order = Default::default();
        self.route = Arc::new(Route {
            order: self.order.clone(),
            ..Route::default()
        });
        self.session = session.into();
        let old = CURRENT
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Arc::downgrade(&self.route));
        if let Some(old) = old.and_then(|old| old.upgrade()) {
            old.close();
        }
    }
    pub(crate) fn route(&self) -> Arc<Route> {
        self.route.clone()
    }
    pub fn accepted(&self, epoch: Epoch) {
        *self.route.owner.lock().unwrap_or_else(|e| e.into_inner()) = Some(epoch);
    }
    pub fn refused(&self, epoch: Epoch) {
        let mut owner = self.route.owner.lock().unwrap_or_else(|e| e.into_inner());
        if *owner == Some(epoch) {
            *owner = None;
        }
    }
    pub fn capture(&self) -> Arc<Capture> {
        self.route.capture.clone()
    }

    pub fn admit_capture(&self, session: &str, selection: Selection) -> bool {
        if self.live(session) {
            return true;
        }
        if self.session == session
            && !self.route.closed.load(Ordering::SeqCst)
            && self.route.capture.readers() == 0
        {
            self.route.capture.record_capture_fallback();
            crate::floor_decide::journal(
                "voice_front_counts",
                json!({"session":session,"capture_arena":self.route.capture.metrics(),"fallback_locale":selection.locale.tag()}),
            );
        }
        false
    }
    pub fn show_capture_fallback(&self, selection: Selection, page: &Page) {
        if self.route.closed.load(Ordering::SeqCst) || self.route.capture.readers() > 0 {
            return;
        }
        let n = self.route.capture.metrics()["legacy_capture_fallbacks"]
            .as_u64()
            .unwrap_or(0);
        let text = if selection.locale == crate::voice_front::policy::Locale::PtBr {
            format!("Usando a voz padrão (turnos desde a reconexão: {n}).")
        } else {
            format!("Using the standard voice (turns since reconnect: {n}).")
        };
        page(PageEvent::Status(text));
    }
    pub fn live(&self, session: &str) -> bool {
        self.session == session
            && !self.route.closed.load(Ordering::SeqCst)
            && self.route.capture.readers() > 0
    }
    pub fn boot(&mut self) -> &str {
        if self.boot.is_empty() {
            self.boot = format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
        }
        &self.boot
    }

    pub fn install(
        &mut self,
        session: &str,
        transport: impl crate::conversation_log::mirror::Transport,
        page: Page,
        log: crate::Log,
        awaiting: Arc<AtomicBool>,
    ) -> std::io::Result<()> {
        if !self.live(session) {
            return Err(std::io::Error::other("front capture unavailable"));
        }
        if self.worker.is_some() {
            return Ok(());
        }
        let data = crate::settings::CTX
            .get()
            .ok_or_else(|| std::io::Error::other("front settings unavailable"))?
            .data
            .clone();
        let id = session.to_string();
        let dir = data.join("conversation-front").join(&id);
        let route = self.route.clone();
        route
            .presentation
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0 = Some(page.clone());
        let handle = Arc::new(Worker::start(
            session.into(),
            self.capture(),
            move || {
                if crate::owned_dir(&data, &dir).is_err() {
                    return crate::conversation_log::FrontStore::LegacyOnly {
                        message: "Conversation sync unavailable; local front is disabled.",
                    };
                }
                crate::conversation_log::Store::for_front(&dir, &id)
            },
            SelectedSpeech,
            transport,
            move |update| route.emit(update, &awaiting, &page, &log),
        )?);
        *self.route.worker.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Arc::downgrade(&handle));
        self.worker = Some(handle);
        Ok(())
    }
}
impl Drop for Attachment {
    fn drop(&mut self) {
        self.route.close();
    }
}
struct SelectedSpeech;
impl Speech for SelectedSpeech {
    fn start(&mut self, r: SpeechRequest) -> Result<()> {
        crate::settings::front_speak(r)
    }
    fn stop(&mut self, t: &Ticket) {
        crate::settings::front_stop(t);
    }
}

pub fn speech_event(e: &Value) {
    crate::conversation::rendered_interrupt(e);
    if !matches!(e["state"].as_str(), Some("done" | "failed")) {
        return;
    }
    let Some(event) = notice(e) else {
        return;
    };
    let Some(route) = current() else {
        return;
    };
    if route.closed.load(Ordering::SeqCst) || !route.capture.matches(event.epoch, &event.response) {
        return;
    }

    if e["stopped"] == true {
        let rendered = event
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        crate::conversation::front_interrupted(event.epoch, &rendered);
    }
    let page = {
        let mut presentation = route.presentation.lock().unwrap_or_else(|e| e.into_inner());
        if presentation.1.as_deref() == Some(event.response.as_str()) {
            None
        } else {
            presentation.1 = Some(event.response.clone());
            presentation.0.clone()
        }
    };
    if let Some(page) = page.filter(|_| !event.spans.is_empty()) {
        let text = event
            .spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        page(PageEvent::Reply(json!({"response_id":event.response,"replace":text,"done":true})));
    }
    let worker = route
        .worker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(Weak::upgrade);
    if let Some(worker) = worker {
        worker.notice(event);
    }
}
fn notice(e: &Value) -> Option<Notice> {
    let front = &e["front"];
    let epoch = Epoch {
        capture: front["epoch"]["capture"].as_u64()?,
        turn: front["epoch"]["turn"].as_u64()?,
    };
    let response = front["response"].as_str()?.to_string();
    if !crate::conversation_log::observe::opaque(&response) {
        return None;
    }
    let frames = front["frames"].as_u64()?;
    let spans = front["spans"].as_array()?;
    if spans.len() > 128 {
        return None;
    }
    let spans = spans
        .iter()
        .map(|s| {
            Some(crate::conversation_log::observe::Span {
                first: s["start"].as_u64()?,
                end: s["end"].as_u64()?,
                text: s["text"].as_str()?.to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let terminal = if e["stopped"] == true {
        crate::conversation_log::observe::Terminal::Interrupted
    } else if e["state"] == "failed" || e["truncated"] == true {
        crate::conversation_log::observe::Terminal::Failed
    } else {
        crate::conversation_log::observe::Terminal::Completed
    };
    if spans.iter().any(|s| s.end > frames) {
        return None;
    }
    crate::conversation_log::observe::Payload::Assistant {
        spans: spans.clone(),
        terminal,
    }
    .validate()
    .ok()?;
    Some(Notice {
        epoch,
        response,
        spans,
        terminal,
    })
}
pub struct Backend {
    pub inner: Box<dyn Legacy + Send>,
    pub interruption: Option<String>,
    pub alive: Arc<AtomicBool>,
    pub capture: Arc<Capture>,
    pub owner: Arc<Mutex<Option<Epoch>>>,
    pub revision: u64,
}
impl Legacy for Backend {
    fn send(&mut self, t: &crate::conversation_log::Turn) -> Result<String> {
        self.inner.send(t)
    }
    fn send_admitted(
        &mut self,
        t: &crate::conversation_log::Turn,
        fence: &mut dyn FnMut() -> Result<crate::conversation_log::Turn>,
    ) -> Result<String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        let capture = self.capture.clone();
        let revision = self.revision;
        let owner = self.owner.clone();
        let result = self.inner.send_admitted(t, &mut || {
            let mut owner = owner.lock().unwrap_or_else(|e| e.into_inner());
            if capture.revision() != revision {
                return Err(Error::Cancelled);
            }
            let committed = fence();
            if committed.is_ok() {
                *owner = None;
            }
            committed
        });
        if result.is_ok() {
            crate::conversation::interruption_sent(self.interruption.take());
        }
        result
    }
}
pub fn input(
    epoch: Epoch,
    revision: u64,
    nonce: String,
    text: &str,
    selection: Selection,
    backend: Box<dyn Legacy + Send>,
) -> Input {
    Input {
        epoch,
        revision,
        nonce,
        original: text.into(),
        started: Instant::now(),
        selection,
        backend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outer_uncertain_delivery_releases_waiting_and_keeps_the_native_session_closed() {
        let mut attached = Attachment::default();
        attached.bind("unknown-session");
        let reader = attached.capture().reader();
        reader.activate();
        assert!(attached.live("unknown-session"));
        crate::conversation::review_busy_front(|epoch| {
            attached.accepted(epoch);
            let awaiting = AtomicBool::new(true);
            assert_eq!(attached.route.unknown(&awaiting, epoch), 1);
            assert!(
                !awaiting.load(Ordering::SeqCst),
                "uncertain delivery must release the outer Chat waiting gate"
            );
            assert!(
                crate::conversation::front_epoch().is_none(),
                "uncertain native turn must release Conversation ownership"
            );
        });
        assert!(
            !attached.live("unknown-session"),
            "uncertain sends must leave the session LegacyOnly"
        );
        assert!(attached.route.closed.load(Ordering::SeqCst));
        assert_eq!(attached.route.delivery_unknown.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn outer_typed_admission_supersedes_unplayed_native_but_keeps_legacy_busy() {
        const TEST:&str="front_native::tests::outer_typed_admission_supersedes_unplayed_native_but_keeps_legacy_busy";
        if std::env::var_os("RTH_FRONT_PENDING_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--test-threads", "4"])
                .env("RTH_FRONT_PENDING_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                child.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&child.stdout),
                String::from_utf8_lossy(&child.stderr)
            );
            return;
        }
        crate::conversation::review_busy_front(|epoch| {
            let mut attached = Attachment::default();
            attached.bind("pending-session");
            attached.accepted(epoch);
            let capture = attached.capture();
            assert_eq!(
                crate::conversation::typed("Newer typed input."),
                Ok(true),
                "typed admission must supersede the owned NotSent voice turn"
            );
            assert_eq!(
                capture.revision(),
                1,
                "a pending voice retry must lose even before it published speech"
            );
            assert!(
                crate::conversation::typed("Later busy legacy input.").is_err(),
                "an unrelated busy legacy turn keeps its existing refusal"
            );
            assert_eq!(
                capture.revision(),
                1,
                "stale front ownership cannot stop a newer ordinary turn"
            );

            struct Admitted;
            impl Legacy for Admitted {
                fn send(&mut self, _: &crate::conversation_log::Turn) -> Result<String> {
                    Ok("legacy-response".into())
                }
                fn send_admitted(
                    &mut self,
                    _: &crate::conversation_log::Turn,
                    f: &mut dyn FnMut() -> Result<crate::conversation_log::Turn>,
                ) -> Result<String> {
                    let _ = f()?;
                    Ok("legacy-response".into())
                }
            }
            let newer = crate::conversation::front_epoch().unwrap();
            attached.accepted(newer);
            let root = std::path::PathBuf::from(
                std::env::var_os("CARGO_TARGET_DIR")
                    .unwrap_or_else(|| std::env::temp_dir().into_os_string()),
            )
            .join(format!("native-transfer-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
            std::fs::create_dir_all(&root).unwrap();
            let mut store =
                crate::conversation_log::Store::open_persisted(&root, "transfer").unwrap();
            let turn = store
                .commit("transfer-nonce", "Public legacy input.")
                .unwrap();
            let mut backend = Backend {
                inner: Box::new(Admitted),
                interruption: None,
                alive: Arc::new(AtomicBool::new(true)),
                capture: capture.clone(),
                owner: attached.route.owner.clone(),
                revision: capture.revision(),
            };
            backend
                .send_admitted(&turn, &mut || Ok(turn.clone()))
                .unwrap();
            assert!(
                crate::conversation::typed("Must wait for the admitted legacy turn.").is_err(),
                "durable legacy transfer must remove native supersession authority"
            );
            assert_eq!(capture.revision(), 1);
        });
    }
    #[test]
    fn native_notice_uses_only_complete_spans_within_actual_rendered_frames() {
        let mut e = json!({"state":"done","front":{"epoch":{"capture":1,"turn":2},"response":"response","frames":8,
            "spans":[{"start":0,"end":8,"text":"Complete rendered text."}]}});
        let notice = super::notice(&e).unwrap();
        assert_eq!(
            notice.terminal,
            crate::conversation_log::observe::Terminal::Completed
        );
        assert_eq!(notice.spans.len(), 1);
        e["front"]["frames"] = json!(7);
        assert!(
            super::notice(&e).is_none(),
            "queued/unheard text is not presentation evidence"
        );
        e["front"]["frames"] = json!(8);
        e["stopped"] = json!(true);
        assert_eq!(
            super::notice(&e).unwrap().terminal,
            crate::conversation_log::observe::Terminal::Interrupted
        );
        e["front"]["spans"] = json!([]);
        e["front"]["frames"] = json!(0);
        assert!(super::notice(&e).unwrap().spans.is_empty());
        e["front"]["response"] = json!("");
        assert!(super::notice(&e).is_none());
    }
}
