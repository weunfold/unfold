

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rth_expression::draft::{self, DRAFTS_PER_DAY, Draft};
use rth_expression::{Candidate, Lang};
use rth_self::{Destinations, Name};
use serde_json::Value;

use crate::flash_side::{FlashSide, SideCall, SideError, SideReply};
use crate::flash_spend::{Purpose, Status};

pub const DEADLINE: Duration = Duration::from_secs(15);

pub type SideFuture<'a> = Pin<Box<dyn Future<Output = Result<SideReply, SideError>> + Send + 'a>>;

pub trait Drafter: Send + Sync {
    fn call(&self, c: SideCall) -> SideFuture<'_>;
}

impl Drafter for FlashSide {
    fn call(&self, c: SideCall) -> SideFuture<'_> {
        Box::pin(FlashSide::call(self, c))
    }
}

#[derive(Debug)]
pub struct Drafted {
    pub attempt_id: String,

    pub text: Option<String>,
    pub spend: Status,
    pub provenance: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Miss {
    Skip(&'static str),
    Failed(&'static str),
}

pub struct Reachout {
    drafter: Option<Arc<dyn Drafter>>,

    day: std::sync::Mutex<(i64, usize)>,
    busy: AtomicBool,
}

struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn skip_of(e: &SideError) -> Miss {
    match e {
        SideError::FlashOff => Miss::Skip("flash_off"),
        SideError::BreakerOpen => Miss::Skip("breaker_open"),
        SideError::NoEgressGrant => Miss::Skip("no_egress_grant"),
        SideError::Refused(_) => Miss::Skip("refused"),
        SideError::Timeout => Miss::Failed("timeout"),
        SideError::Unavailable(_) => Miss::Failed("unavailable"),
        SideError::Invalid(_) => Miss::Failed("invalid"),
    }
}

fn fault_name(f: draft::Fault) -> &'static str {
    match f {
        draft::Fault::NotJson => "not_json",
        draft::Fault::Empty => "empty",
        draft::Fault::TooLong => "too_long",
        draft::Fault::Paragraphs => "paragraphs",
        draft::Fault::List => "list",
        draft::Fault::Questions => "questions",
        draft::Fault::Article => "article",
        draft::Fault::Gendered => "gendered",
    }
}

impl Reachout {

    pub fn new(drafter: Option<Arc<dyn Drafter>>) -> Reachout {
        Reachout { drafter, day: std::sync::Mutex::new((i64::MIN, 0)), busy: AtomicBool::new(false) }
    }

    pub fn drafts_on(&self, local_day: i64) -> usize {
        let d = self.day.lock().unwrap();
        if d.0 == local_day { d.1 } else { 0 }
    }

    fn count(&self, local_day: i64) {
        let mut d = self.day.lock().unwrap();
        *d = if d.0 == local_day { (local_day, d.1 + 1) } else { (local_day, 1) };
    }

    pub async fn draft(&self, c: &Candidate, name: &Name, lang: Lang, destinations: &Destinations, local_day: i64) -> Result<Drafted, Miss> {
        let Some(drafter) = self.drafter.as_ref() else { return Err(Miss::Skip("flash_off")) };
        if self.drafts_on(local_day) >= DRAFTS_PER_DAY {
            return Err(Miss::Skip("draft_limit"));
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(Miss::Skip("in_flight"));
        }
        let _busy = Busy(&self.busy);
        let p = draft::prompt(c, name, lang, destinations);
        let call = SideCall { purpose: Purpose::Reachout, system: p.system, user: p.user, schema: draft::schema(), max_tokens: draft::MAX_TOKENS, carries_memory: p.carries_memory, deadline: DEADLINE };
        let reply = match drafter.call(call).await {
            Ok(r) => r,
            Err(e) => {
                let miss = skip_of(&e);
                if matches!(miss, Miss::Failed(_)) {
                    self.count(local_day);
                }
                return Err(miss);
            }
        };
        self.count(local_day);
        let text = match draft::check(&reply.text, lang, name) {
            Ok(Draft::Declined) => None,
            Ok(Draft::Text(t)) if crate::guard::secret_in(&t).is_some() => return Err(Miss::Failed("secret")),
            Ok(Draft::Text(t)) => Some(t),
            Err(f) => return Err(Miss::Failed(fault_name(f))),
        };
        Ok(Drafted { attempt_id: reply.attempt_id, text, spend: reply.spend, provenance: reply.provenance })
    }
}

#[cfg(test)]
#[path = "drafter_tests.rs"]
mod tests;
