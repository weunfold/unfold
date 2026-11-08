

use super::{context::{self, Context}, child::Lease, http, policy::*, Error, Result};
use crate::conversation_log::{observe::{opaque, Payload, Span, Terminal}, Owner, Store, Turn};
use serde::Serialize;
use std::{collections::VecDeque, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Instant};
const STOP: u64 = 1 << 63;
const CANCEL: u64 = 1 << 62;
const FRAMES: u64 = CANCEL - 1;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Epoch { pub capture: u64, pub turn: u64 }
struct Signal { epoch: Epoch, response: String, state: AtomicU64 }

#[derive(Clone)]
pub struct Ticket(Arc<Signal>);
impl Ticket {
    pub(crate) fn new(epoch: Epoch, response: String) -> Self {
        Self(Arc::new(Signal { epoch, response, state: AtomicU64::new(0) }))
    }
    pub fn response(&self) -> &str { &self.0.response }
    pub(crate) fn epoch(&self) -> Epoch { self.0.epoch }
    pub(crate) fn closed(&self) -> bool { self.0.state.load(Ordering::SeqCst) & STOP != 0 }
    pub fn frames(&self) -> u64 { self.0.state.load(Ordering::SeqCst) & FRAMES }

    pub fn render(&self, epoch: Epoch, response: &str, frames: u32) -> bool {
        if epoch != self.0.epoch || response != self.response() || frames == 0 { return false; }
        self.0.state.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
            if state & STOP != 0 { return None; }
            state.checked_add(frames.into()).filter(|n| n & (STOP | CANCEL) == 0)
        }).is_ok()
    }

    pub fn fence(&self) -> bool { self.0.state.fetch_or(STOP | CANCEL, Ordering::SeqCst) & FRAMES != 0 }
    pub(crate) fn close(&self) -> u64 { self.0.state.fetch_or(STOP, Ordering::SeqCst) }
    pub(crate) fn cancelled(&self) -> bool { self.0.state.load(Ordering::SeqCst) & CANCEL != 0 }
    pub(crate) fn same(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}
pub struct SpeechRequest { pub ticket: Ticket, pub text: String, pub deadline:Instant }

pub trait Speech {
    fn start(&mut self, request: SpeechRequest) -> Result<()>;

    fn stop(&mut self, ticket: &Ticket);
}
pub trait Legacy {

    fn send(&mut self, turn: &Turn) -> Result<String>;

    fn send_admitted(&mut self, _turn:&Turn, fence:&mut dyn FnMut()->Result<Turn>) -> Result<String> {
        self.send(&fence()?)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome { NotSent, Started, LegacyAccepted, DeliveryUnknown, Existing, Cancelled, PresentationUnknown }
pub struct Submission<'a> {
    pub epoch: Epoch, pub nonce: &'a str, pub input: &'a str, pub selection: Selection,
    pub inventory: &'a Inventory, pub evidence: Evidence<'a>, pub history: &'a [RenderedReply], pub intent: Option<CapabilityIntent<'a>>,
    pub candidate: Candidate, pub control: Control<'a>,
}
#[derive(Default, Serialize)]
pub struct LanguageCounts {
    pub front: u64, pub legacy: u64, pub unknown: u64, pub cancelled: u64,
    pub marker: u64, pub guard: u64, pub deadline: u64, pub front_failure: u64,
}
#[derive(Default, Serialize)]
pub struct Counts { pub en: LanguageCounts, pub pt_br: LanguageCounts }
impl Counts {
    fn language(&mut self, locale: Locale) -> &mut LanguageCounts {
        match locale { Locale::En => &mut self.en, Locale::PtBr => &mut self.pt_br }
    }
}
#[derive(Serialize)]
pub struct Mark {
    pub stage: &'static str, pub at_ns: u64, pub clock: String, pub epoch: Epoch,
    pub response: String, pub locale: &'static str,
}
struct Active {
    turn: Turn, ticket: Ticket, text: String, selection: Selection, first_recorded: bool, cancelled: bool,
}

pub struct Controller {
    epoch: Option<Epoch>, active: Option<Active>, clock: String, started: Instant,
    marks: VecDeque<Mark>, dropped_marks: u64, counts: Counts,
}
impl Controller {
    pub fn new(clock: &str) -> Result<Self> {
        if !opaque(clock) { return Err(Error::Context); }
        Ok(Self { epoch: None, active: None, clock: clock.into(), started: Instant::now(),
            marks: VecDeque::new(), dropped_marks: 0, counts: Counts::default() })
    }
    pub fn activate(&mut self, epoch: Epoch) -> Result<()> {
        if self.active.is_some() { return Err(Error::Busy); }
        self.epoch = Some(epoch); Ok(())
    }
    pub fn counts(&self) -> &Counts { &self.counts }
    pub fn dropped_marks(&self) -> u64 { self.dropped_marks }
    pub fn take_marks(&mut self) -> Vec<Mark> { self.marks.drain(..).collect() }
    fn mark(&mut self, stage: &'static str, response: &str, selection: Selection) {
        if self.marks.len() == 64 { self.marks.pop_front(); self.dropped_marks = self.dropped_marks.saturating_add(1); }
        self.marks.push_back(Mark { stage, at_ns: self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            clock: self.clock.clone(), epoch: self.epoch.expect("admitted epoch"), response: response.into(), locale: selection.locale.tag() });
    }
    fn matched(&self, ticket: &Ticket) -> Result<&Active> {
        self.active.as_ref().filter(|a| a.ticket.same(ticket) && Some(ticket.0.epoch) == self.epoch).ok_or(Error::Cancelled)
    }
    fn legacy(&mut self, store:&mut Store, turn:&Turn, selection:Selection, backend:&mut impl Legacy, cancelled:impl Fn()->bool) -> Result<Outcome> {
        let (mut attempted,mut existing,mut stopped)=(false,false,false);
        let result=backend.send_admitted(turn,&mut|| {
            if cancelled() {
                store.cancel_before_render(&turn.id).map_err(|_|Error::Persistence)?;
                stopped=true;return Err(Error::Cancelled);
            }
            if !store.begin_legacy_send(&turn.id).map_err(|_|Error::Persistence)? {existing=true;return Err(Error::Closed);}
            attempted=true;
            self.mark("legacy_send",&turn.response_id,selection);
            let mut sent=turn.clone();sent.owner=Some(Owner::Legacy);sent.legacy_attempted=true;Ok(sent)
        });
        match result {
            Ok(response)=>{
                if !attempted {return Err(Error::Context);}
                store.accept_legacy(&turn.id,&response).map_err(|_|Error::Persistence)?;
                Ok(Outcome::LegacyAccepted)
            }
            Err(_) if stopped=>{self.counts.language(selection.locale).cancelled+=1;Ok(Outcome::Cancelled)}
            Err(_) if existing=>Ok(Outcome::Existing),
            Err(Error::Persistence)=>Err(Error::Persistence),
            Err(_) if !attempted=>Ok(Outcome::NotSent),
            Err(_)=>Ok(Outcome::DeliveryUnknown),
        }
    }
    pub fn submit(&mut self, store: &mut Store, sub: Submission<'_>, speech: &mut impl Speech, backend: &mut impl Legacy) -> Result<Outcome> {
        if self.epoch != Some(sub.epoch) || sub.control.cancel.load(Ordering::SeqCst) { return Ok(Outcome::Cancelled); }
        if self.active.as_ref().is_some_and(|a| a.turn.nonce != sub.nonce) { return Err(Error::Busy); }
        let turn = store.commit(sub.nonce, sub.input).map_err(|_| Error::Persistence)?;
        if turn.owner==Some(Owner::Legacy) && !turn.legacy_attempted && turn.presentation.is_none() {
            return self.legacy(store,&turn,sub.selection,backend,||sub.control.cancel.load(Ordering::SeqCst));
        }
        if turn.owner.is_some() || turn.presentation.is_some() { return Ok(Outcome::Existing); }
        self.mark("local_durable_commit", &turn.response_id, sub.selection);
        let cancel=sub.control.cancel;
        let req = TurnRequest { id: &turn.id, selection: sub.selection, inventory: sub.inventory, evidence: sub.evidence,
            history: sub.history, intent: sub.intent, repeat: None, control: sub.control };

        let decision = match route_committed(store, &req, sub.candidate, &mut Unqualified, |_| Err(Error::Admission)) {
            Err(Error::Cancelled) => {
                store.cancel_before_render(&turn.id).map_err(|_| Error::Persistence)?;
                return Ok(Outcome::Cancelled);
            },
            other => other?,
        };
        #[cfg(test)]
        after_select_for_test();
        self.mark("owner_selected", &turn.response_id, sub.selection);
        let counts = self.counts.language(sub.selection.locale);
        if decision.owner == Owner::Legacy {
            if req.control.cancel.load(Ordering::SeqCst) {
                store.cancel_before_render(&turn.id).map_err(|_| Error::Persistence)?;
                counts.cancelled = counts.cancelled.saturating_add(1);
                return Ok(Outcome::Cancelled);
            }
            counts.legacy = counts.legacy.saturating_add(1);
            let reason = match decision.reason {
                Reason::Marker => &mut counts.marker, Reason::Deadline => &mut counts.deadline,
                Reason::FrontFailure => &mut counts.front_failure, _ => &mut counts.guard,
            };
            *reason = reason.saturating_add(1);
            return self.legacy(store, &turn, sub.selection, backend,||cancel.load(Ordering::SeqCst));
        }
        counts.front = counts.front.saturating_add(1);
        let ticket = Ticket::new(sub.epoch, turn.response_id.clone());
        let text = decision.speech.ok_or(Error::Context)?;
        if req.control.cancel.load(Ordering::SeqCst) {
            ticket.fence();
            store.cancel_before_render(&turn.id).map_err(|_| Error::Persistence)?;
            self.counts.language(sub.selection.locale).cancelled += 1;
            return Ok(Outcome::Cancelled);
        }
        let deadline=req.control.started+std::time::Duration::from_secs(5);
        self.active = Some(Active { turn, ticket: ticket.clone(), text: text.clone(), selection: sub.selection, first_recorded: false, cancelled: false });
        self.mark("tts_request", ticket.response(), sub.selection);
        if speech.start(SpeechRequest { ticket: ticket.clone(), text, deadline }).is_err() {
            return self.failed(store, &ticket, speech, backend);
        }
        Ok(Outcome::Started)
    }

    pub fn supersede(&mut self, store: &mut Store, id: &str, speech: &mut impl Speech, backend: &mut impl Legacy) -> Result<Outcome> {
        if let Some(a) = &self.active {
            if a.turn.id != id { return Err(Error::Busy); }
            let ticket = a.ticket.clone(); self.cancel(&ticket, speech)?;
            return self.failed(store, &ticket, speech, backend);
        }
        store.cancel_before_render(id).map_err(|_| Error::Persistence)?;
        Ok(Outcome::Cancelled)
    }

    pub fn render_notice(&mut self, store: &mut Store, ticket: &Ticket) -> Result<()> {
        let a = self.matched(ticket)?;
        if ticket.frames() == 0 { return Err(Error::Context); }
        if !a.first_recorded {
            let id = a.turn.id.clone(); let selection = a.selection;
            store.first_rendered(&id).map_err(|_| Error::Persistence)?;
            self.active.as_mut().expect("matched").first_recorded = true;

            self.mark("first_render_notice", ticket.response(), selection);
        }
        Ok(())
    }
    pub fn cancel(&mut self, ticket: &Ticket, speech: &mut impl Speech) -> Result<()> {
        self.matched(ticket)?; ticket.fence();
        let a = self.active.as_mut().expect("matched"); a.cancelled = true; let selection = a.selection;
        speech.stop(ticket); self.mark("cancel", ticket.response(), selection); Ok(())
    }
    pub fn failed(&mut self, store: &mut Store, ticket: &Ticket, speech: &mut impl Speech, backend: &mut impl Legacy) -> Result<Outcome> {
        if self.active.is_none() { return Ok(Outcome::Existing); }
        let a = self.matched(ticket)?;
        let turn = a.turn.clone(); let selection = a.selection; let cancelled = a.cancelled;
        let state = ticket.close(); let rendered = state & FRAMES != 0;
        let cancelled = cancelled || state & CANCEL != 0; speech.stop(ticket);
        if rendered {
            self.render_notice(store, ticket)?;
            if cancelled {
                store.finish_rendered(&turn.id, vec![], Terminal::Interrupted).map_err(|_| Error::Persistence)?;
                self.counts.language(selection.locale).cancelled += 1;
                self.active = None; return Ok(Outcome::Cancelled);
            }
            store.finish_rendered(&turn.id, vec![], Terminal::PresentationUnknown).map_err(|_| Error::Persistence)?;
            store.mark_mirror_conflict().map_err(|_| Error::Persistence)?;
            self.counts.language(selection.locale).unknown += 1;
            self.mark("presentation_unknown", ticket.response(), selection); self.active = None;
            return Ok(Outcome::PresentationUnknown);
        }
        if cancelled {
            store.cancel_before_render(&turn.id).map_err(|_| Error::Persistence)?;
            self.counts.language(selection.locale).cancelled += 1;
            self.active = None; return Ok(Outcome::Cancelled);
        }
        store.select_owner(&turn.id, Owner::Legacy).map_err(|_| Error::Persistence)?;
        let counts = self.counts.language(selection.locale); counts.front = counts.front.saturating_sub(1);
        counts.legacy = counts.legacy.saturating_add(1); counts.front_failure = counts.front_failure.saturating_add(1);
        self.active = None;
        self.legacy(store, &turn, selection, backend,||ticket.cancelled())
    }

    pub fn finish(&mut self, store: &mut Store, ticket: &Ticket, spans: Vec<Span>, terminal: Terminal) -> Result<()> {
        let a = self.matched(ticket)?;
        let terminal = if a.cancelled || ticket.cancelled() { Terminal::Interrupted } else { terminal };
        let payload = Payload::Assistant { spans: spans.clone(), terminal };
        payload.validate().map_err(|_| Error::Context)?;
        let heard = payload.text();
        if ticket.frames() == 0 || spans.iter().any(|s| s.end > ticket.frames())
            || (!heard.is_empty() && !a.text.starts_with(&heard))
            || (terminal == Terminal::Completed && heard != a.text) { return Err(Error::Context); }
        let id = a.turn.id.clone(); let selection = a.selection;
        let state = ticket.close();
        let terminal = if state & CANCEL != 0 { Terminal::Interrupted } else { terminal };
        self.render_notice(store, ticket)?;
        store.finish_rendered(&id, spans, terminal).map_err(|_| Error::Persistence)?;
        if terminal == Terminal::PresentationUnknown { store.mark_mirror_conflict().map_err(|_| Error::Persistence)?; }
        self.mark("presentation_terminal", ticket.response(), selection); self.active = None; Ok(())
    }
}

pub fn ready_prefill(lease: &Lease, ctx: &mut Context, quiet: bool) -> Result<()> {
    if !quiet { return Err(Error::Prefill); }
    if !http::ready(lease)? { return Err(Error::Child); }
    ctx.child_started(lease.generation);
    if ctx.needs_prefill() {
        let prefix = ctx.prefill_plan(true)?;
        let receipt = context::prefill(lease, &prefix)?; drop(prefix);
        ctx.mark_prefilled(receipt)?;
    }
    Ok(())
}
