
use super::{
    capture::Capture, context::Context, http::Burst, policy::*, runtime::*, Error, Result,
};
use crate::conversation_log::{
    mirror::{Client, Transport},
    observe::{Span, Terminal},
    FrontStore, SilentWindow,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
const OPENING: u8 = 0;
const READY: u8 = 1;
const LEGACY_ONLY: u8 = 2;
const CLOSED: u8 = 3;
pub struct Input {
    pub epoch: Epoch,
    pub revision: u64,
    pub nonce: String,
    pub original: String,
    pub started: Instant,
    pub selection: Selection,
    pub backend: Box<dyn Legacy + Send>,
}
pub enum Update {
    Status(&'static str),
    Outcome(Epoch, String, Outcome),
    Rendered(Epoch),
    Terminal(Epoch),
    Mark(Mark),
    Counts(serde_json::Value),
}
pub struct Notice {
    pub epoch: Epoch,
    pub response: String,
    pub spans: Vec<Span>,
    pub terminal: Terminal,
}
struct Shared {
    state: AtomicU8,
    closed: AtomicBool,
    ticket: Mutex<Option<Ticket>>,
    admission: Mutex<Option<Epoch>>,
}
#[cfg(test)]
type SubmitProbe = Box<dyn Fn(Epoch) + Send>;
pub struct Worker {
    session: String,
    tx: SyncSender<Input>,
    notices: SyncSender<Notice>,
    shared: Arc<Shared>,
    capture: Arc<Capture>,
    #[cfg(test)]
    before_submit: Mutex<Option<SubmitProbe>>,
}
impl Worker {
    pub fn start(
        session: String,
        capture: Arc<Capture>,
        open: impl FnOnce() -> FrontStore + Send + 'static,
        speech: impl Speech + Send + 'static,
        transport: impl Transport,
        emit: impl Fn(Update) + Send + 'static,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Input>(1);
        let (notices, events) = mpsc::sync_channel::<Notice>(8);
        let shared = Arc::new(Shared {
            state: AtomicU8::new(OPENING),
            closed: AtomicBool::new(false),
            ticket: Mutex::new(None),
            admission: Mutex::new(None),
        });
        let (state, id, owned_capture) = (shared.clone(), session.clone(), capture.clone());
        std::thread::Builder::new()
            .name("voice-front-controller".into())
            .spawn(move || {
                let capture = owned_capture;
                let FrontStore::Ready(mut store) = open() else {
                    state.state.store(LEGACY_ONLY, Ordering::SeqCst);
                    emit(Update::Status("LegacyOnly"));
                    return;
                };
                let run = (|| -> Result<()> {
                    if state.closed.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    if store.session() != id {
                        return Err(Error::Context);
                    }
                    let mut mirror = Client::start(transport).map_err(|_| Error::Closed)?;
                    mirror.resume(&id).map_err(|_| Error::Closed)?;
                    let clock_id = format!(
                        "native-{}-{}",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos()
                    );
                    let mut controller = Controller::new(&clock_id)?;
                    let mut speech = BoundSpeech {
                        inner: speech,
                        capture: capture.clone(),
                        current: None,
                        epoch: Epoch {
                            capture: 0,
                            turn: 0,
                        },
                        revision: 0,
                        shared: state.clone(),
                    };
                    let mut context = Context::new(STABLE_PROMPT)?;
                    let mut window = SilentWindow::default();
                    let mut history_cursor = 0;
                    let mut silent = std::collections::HashSet::new();
                    let mut rendered_history = Vec::new();
                    let clock = Instant::now();
                    let mut active: Option<Input> = None;
                    let mut retry: Option<Input> = None;
                    let mut rendered = false;
                    while !state.closed.load(Ordering::SeqCst) {
                        mirror
                            .pump(
                                &mut store,
                                clock.elapsed().as_millis().min(u64::MAX as u128) as u64,
                            )
                            .map_err(|_| Error::Persistence)?;
                        let legacy_ready = !mirror.busy()
                            && store
                                .prepare_observation()
                                .map_err(|_| Error::Persistence)?
                                .is_none();
                        let batch = store.ready_silent_cancels(&mut window);
                        silent.extend(batch.turns.into_iter().map(|t| t.id));
                        append_history(&store,&mut context,&mut history_cursor,&mut silent,&mut rendered_history)?;
                        if state.state.load(Ordering::SeqCst) == OPENING && mirror.reconciled() {
                            let ready = store.front_admission_allowed();
                            state
                                .state
                                .store(if ready { READY } else { LEGACY_ONLY }, Ordering::SeqCst);
                            emit(Update::Status(if ready { "Ready" } else { "LegacyOnly" }));
                        }
                        if let Some(ticket) = speech.current.clone() {

                            if ticket.frames() > 0 && !rendered {
                                controller.render_notice(&mut store, &ticket)?;
                                emit(Update::Rendered(speech.epoch));
                                rendered = true;
                            }
                            let mut terminal = None;
                            for _ in 0..8 {
                                let Ok(event) = events.try_recv() else { break };
                                if event.response == ticket.response()
                                    && event.epoch == speech.epoch
                                {
                                    terminal = Some(event);
                                    break;
                                }
                            }
                            let cancelled = capture.revision() != speech.revision;
                            if cancelled {
                                controller.cancel(&ticket, &mut speech)?;
                            }
                            let mut settled = None;
                            if let Some(event) = terminal {
                                if ticket.frames() > 0
                                    && controller
                                        .finish(&mut store, &ticket, event.spans, event.terminal)
                                        .is_ok()
                                {
                                    settled = Some(Outcome::Existing);
                                } else {
                                    settled = Some(controller.failed(
                                        &mut store,
                                        &ticket,
                                        &mut speech,
                                        &mut Backend(
                                            active.as_mut().ok_or(Error::Context)?.backend.as_mut(),
                                            legacy_ready,
                                        ),
                                    )?);
                                }
                            } else if cancelled || ticket.closed() {
                                settled = Some(controller.failed(
                                    &mut store,
                                    &ticket,
                                    &mut speech,
                                    &mut Backend(
                                        active.as_mut().ok_or(Error::Context)?.backend.as_mut(),
                                        legacy_ready,
                                    ),
                                )?);
                            }
                            if let Some(outcome) = settled {
                                let input = active.take().ok_or(Error::Context)?;
                                if outcome != Outcome::Existing {
                                    emit(Update::Outcome(
                                        input.epoch,
                                        input.nonce.clone(),
                                        outcome,
                                    ));
                                }
                                counts(&controller, &capture, &emit);
                                speech.current = None;
                                if outcome == Outcome::LegacyAccepted { release_admission(&state, input.epoch); }
                                if outcome == Outcome::NotSent {
                                    retry = Some(input);
                                } else if outcome != Outcome::LegacyAccepted {
                                    release_admission(&state, input.epoch);
                                    emit(Update::Terminal(input.epoch));
                                }
                            }
                        }
                        for mark in controller.take_marks() {
                            emit(Update::Mark(mark));
                        }
                        let input = match retry.take() {
                            Some(i) => Some(i),
                            None => match rx.recv_timeout(Duration::from_millis(20)) {
                                Ok(i) => Some(i),
                                Err(mpsc::RecvTimeoutError::Timeout) => None,
                                Err(_) => break,
                            },
                        };
                        let Some(mut input) = input else { continue };
                        if capture.revision() != input.revision {
                            if let Some(t) = store
                                .turns()
                                .iter()
                                .find(|t| t.nonce == input.nonce)
                                .cloned()
                            {
                                if !t.legacy_attempted && !t.rendered && t.presentation.is_none() {
                                    store
                                        .cancel_before_render(&t.id)
                                        .map_err(|_| Error::Persistence)?;
                                }
                            }
                            emit(Update::Outcome(
                                input.epoch,
                                input.nonce,
                                Outcome::Cancelled,
                            ));
                            release_admission(&state, input.epoch);
                                    emit(Update::Terminal(input.epoch));
                            continue;
                        }
                        if speech.current.is_some() {
                            retry = Some(input);
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        controller.activate(input.epoch)?;
                        speech.epoch = input.epoch;
                        speech.revision = input.revision;
                        rendered = false;
                        let cancel = AtomicBool::new(false);
                        let evidence = Evidence {
                            revision: 1,
                            text: "Only controller-owned configuration status is available.",
                        };
                        let inventory = Inventory::default();
                        let capability = intent(&input.original, input.selection.locale);
                        let candidate = if capability.is_some() {
                            Candidate::Controller(ControllerLine::Clarify)
                        } else {
                            Candidate::Model(Burst::default())
                        };
                        let outcome = controller.submit(
                            &mut store,
                            Submission {
                                epoch: input.epoch,
                                nonce: &input.nonce,
                                input: &input.original,
                                selection: input.selection,
                                inventory: &inventory,
                                evidence,
                                history: &rendered_history,
                                intent: capability.map(|capability| CapabilityIntent {
                                    input: &input.original,
                                    capability,
                                }),
                                candidate,
                                control: Control {
                                    started: input.started,
                                    cancel: &cancel,
                                },
                            },
                            &mut speech,
                            &mut CheckedBackend {
                                inner: input.backend.as_mut(),
                                capture: &capture,
                                revision: input.revision,
                                cancel: &cancel,
                                ready: legacy_ready,
                            },
                        )?;
                        if matches!(outcome, Outcome::LegacyAccepted | Outcome::Existing) { release_admission(&state, input.epoch); }
                        emit(Update::Outcome(input.epoch, input.nonce.clone(), outcome));
                        counts(&controller, &capture, &emit);
                        match outcome {
                            Outcome::Started => active = Some(input),
                            Outcome::NotSent => {
                                speech.current = None;
                                retry = Some(input);
                                std::thread::sleep(Duration::from_millis(20));
                            }
                            Outcome::DeliveryUnknown => return Err(Error::Closed),
                            Outcome::LegacyAccepted | Outcome::Existing => {
                                speech.current = None;
                            }
                            _ => {
                                speech.current = None;
                                release_admission(&state, input.epoch);
                                    emit(Update::Terminal(input.epoch));
                            }
                        }
                    }
                    if let Some(ticket) = speech.current.clone() {
                        ticket.close();
                        controller.failed(
                            &mut store,
                            &ticket,
                            &mut speech,
                            &mut Backend(
                                active.as_mut().ok_or(Error::Context)?.backend.as_mut(),
                                false,
                            ),
                        )?;
                    }
                    mirror.close().map_err(|_| Error::Closed)?;
                    Ok(())
                })();
                let unfinished = {
                    let mut admission = state.admission.lock().unwrap_or_else(|e|e.into_inner());
                    state.state.store(if run.is_ok() { CLOSED } else { LEGACY_ONLY }, Ordering::SeqCst);
                    admission.take()
                };
                if run.is_err() {
                    if let Some(t) = state
                        .ticket
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_ref()
                    {
                        t.close();
                    }
                    if let Some(epoch) = unfinished { emit(Update::Terminal(epoch)); }
                    emit(Update::Status("LegacyOnly"));
                } else {
                    emit(Update::Status("Closed"));
                }
            })?;
        Ok(Self {
            session,
            tx,
            notices,
            shared,
            capture,
            #[cfg(test)]
            before_submit: Mutex::new(None),
        })
    }

    pub fn submit(
        &self,
        enabled: bool,
        session: &str,
        input: Input,
    ) -> std::result::Result<String, Input> {
        let mut admission = self.shared.admission.lock().unwrap_or_else(|e|e.into_inner());
        if !enabled
            || self.session != session
            || self.shared.state.load(Ordering::SeqCst) != READY
            || self.shared.closed.load(Ordering::SeqCst)
            || input.original.trim().is_empty()
            || input.original.len() > 8192
            || !crate::conversation_log::observe::opaque(&input.nonce)
        {
            return Err(input);
        }
        #[cfg(test)]
        if let Some(probe) = self.before_submit.lock().unwrap().as_ref() { probe(input.epoch); }
        let nonce = input.nonce.clone();
        let epoch = input.epoch;
        self.tx
            .try_send(input)
            .map(|()| { *admission = Some(epoch); nonce })
            .map_err(|e| match e {
                TrySendError::Full(i) | TrySendError::Disconnected(i) => i,
            })
    }
    #[cfg(test)]
    pub(crate) fn review_before_submit(&self, probe: impl Fn(Epoch) + Send + 'static) {
        *self.before_submit.lock().unwrap() = Some(Box::new(probe));
    }
    pub fn notice(&self, event: Notice) {
        let epoch = event.epoch;
        let response = event.response.clone();
        if self.notices.try_send(event).is_err() {
            self.capture.close_matching(epoch, &response);
        }
    }
    pub fn close(&self) {
        self.capture.close();
        self.shared.closed.store(true, Ordering::SeqCst);
    }
}
fn release_admission(state: &Shared, epoch: Epoch) {
    let mut admission = state.admission.lock().unwrap_or_else(|e|e.into_inner());
    if *admission == Some(epoch) { *admission = None; }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
    }
}
fn append_history(store:&crate::conversation_log::Store,context:&mut Context,history_cursor:&mut usize,
    silent:&mut std::collections::HashSet<String>,rendered_history:&mut Vec<RenderedReply>)->Result<()> {
                        for turn in store.turns().iter().skip(*history_cursor).take(16) {
                            if turn.silent_replay {
                                if !silent.remove(&turn.id) {
                                    break;
                                }
                                context.append_silent_cancel(turn)?;
                            } else if turn.rendered
                                && turn.presentation.is_some()
                                && !turn.presentation_uncertain
                            {
                                if turn
                                    .presentation
                                    .as_ref()
                                    .is_some_and(|p| !p.text().is_empty())
                                {
                                    context.append_rendered(turn)?;
                                }
                                if let Some(
                                    crate::conversation_log::observe::Payload::Assistant {
                                        spans,
                                        terminal,
                                    },
                                ) = &turn.presentation
                                {
                                    if !spans.is_empty() {
                                        rendered_history.push(RenderedReply::from_renderer(
                                            &turn.response_id,
                                            &turn.input,
                                            spans.clone(),
                                            *terminal,
                                        )?);
                                    }
                                }
                            } else if turn.presentation.is_none() && !turn.acknowledged {
                                break;
                            }
                            *history_cursor += 1;
                        }
    Ok(())
}
fn counts(controller: &Controller, capture: &Capture, emit: &impl Fn(Update)) {
    if let Ok(serde_json::Value::Object(mut row)) = serde_json::to_value(controller.counts()) {
        row.insert(
            "capture_arena".into(),
            capture.metrics(),
        );
        emit(Update::Counts(serde_json::Value::Object(row)));
    }
}
struct BoundSpeech<S> {
    inner: S,
    capture: Arc<Capture>,
    current: Option<Ticket>,
    epoch: Epoch,
    revision: u64,
    shared: Arc<Shared>,
}
impl<S: Speech> Speech for BoundSpeech<S> {
    fn start(&mut self, r: SpeechRequest) -> Result<()> {
        self.current = Some(r.ticket.clone());
        *self.shared.ticket.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.ticket.clone());
        self.capture.publish(&r.ticket, self.revision)?;
        self.inner.start(r)
    }
    fn stop(&mut self, t: &Ticket) {
        self.inner.stop(t)
    }
}
struct Backend<'a>(&'a mut (dyn Legacy + Send), bool);
impl Legacy for Backend<'_> {
    fn send(&mut self, t: &crate::conversation_log::Turn) -> Result<String> {
        self.0.send(t)
    }
    fn send_admitted(
        &mut self,
        t: &crate::conversation_log::Turn,
        f: &mut dyn FnMut() -> Result<crate::conversation_log::Turn>,
    ) -> Result<String> {
        if !self.1 {
            return Err(Error::Busy);
        }
        self.0.send_admitted(t, f)
    }
}
struct CheckedBackend<'a> {
    inner: &'a mut (dyn Legacy + Send),
    capture: &'a Capture,
    revision: u64,
    cancel: &'a AtomicBool,
    ready: bool,
}
impl Legacy for CheckedBackend<'_> {
    fn send(&mut self, t: &crate::conversation_log::Turn) -> Result<String> {
        self.inner.send(t)
    }
    fn send_admitted(
        &mut self,
        t: &crate::conversation_log::Turn,
        f: &mut dyn FnMut() -> Result<crate::conversation_log::Turn>,
    ) -> Result<String> {
        if !self.ready {
            return Err(Error::Busy);
        }
        self.inner.send_admitted(t, &mut || {
            if self.capture.revision() != self.revision {
                self.cancel.store(true, Ordering::SeqCst);
            }
            f()
        })
    }
}

pub fn intent(text: &str, locale: Locale) -> Option<Capability> {
    match (locale, text.trim()) {
        (Locale::En, "Can you access files?") | (Locale::PtBr, "Você pode acessar arquivos?") => {
            Some(Capability::Files)
        }
        (Locale::En, "Can you access the network?")
        | (Locale::PtBr, "Você pode acessar a rede?") => Some(Capability::Network),
        (Locale::En, "Do you have memory?") | (Locale::PtBr, "Você tem memória?") => {
            Some(Capability::Memory)
        }
        _ => None,
    }
}

#[derive(Default)]
pub struct BackendOrder {
    revision: u64,
    strict: bool,
    command: Option<String>,
    response: Option<String>,
    ended: Option<String>,
}
impl BackendOrder {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn begin(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.strict = true;
        self.command = None;
        self.response = None;
        self.ended = None;
    }
    pub fn disable(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.strict = false;
        self.command = None;
        self.response = None;
        self.ended = None;
    }
    pub fn expect(&mut self, command: &str) {
        if self.strict && self.command.as_deref() != Some(command) {
            self.revision = self.revision.wrapping_add(1);
            self.command = Some(command.into());
            self.response = None;
            self.ended = None;
        }
    }
    pub fn allows(&mut self, f: &serde_json::Value) -> bool {
        if !self.strict {
            return true;
        }
        let kind = f["kind"].as_str().unwrap_or("");
        if kind == "response_diagnostic" {
            return self.ended.is_some() && self.ended.as_deref() == f["response_id"].as_str();
        }
        if kind == "response_started" {
            let Some(command) = self.command.as_deref() else {
                return false;
            };
            if f["request_id"].as_str() != Some(command) {
                return false;
            }
            let Some(id) = f["response_id"]
                .as_str()
                .filter(|s| crate::conversation_log::observe::opaque(s))
            else {
                return false;
            };
            if self.response.as_deref().is_some_and(|old| old != id) {
                return false;
            }
            self.response = Some(id.into());
            return true;
        }
        if matches!(
            kind,
            "agent_opener" | "agent_delta" | "agent_replace" | "reply_spoken" | "agent_done"
        ) {
            if self.response.is_none() || self.response.as_deref() != f["response_id"].as_str() {
                return false;
            }
            if kind == "agent_done" {
                self.ended = self.response.take();
                self.command = None;
            }
        }
        true
    }
}
#[cfg(test)]
#[path = "front_worker_tests.rs"]
mod tests;
