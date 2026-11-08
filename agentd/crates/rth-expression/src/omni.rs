

use rth_self::consent::{self, Grant};

use crate::reason::Message;

pub const RECIPIENT_ENV: &str = "RTH_OMNI_RECIPIENT";

pub const BACKEND: &str = "omni";

pub fn recipient(configured: Option<&str>) -> Option<&str> {
    configured.map(str::trim).filter(|r| !r.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent,

    Failed(&'static str),
}

pub trait Channel {

    fn available(&self) -> bool;

    fn consented(&self, recipient: &str) -> bool;
    fn send(&mut self, msg: &Message) -> Outcome;
}

pub fn deliver(ch: &mut dyn Channel, configured: Option<&str>, msg: &Message) -> Outcome {
    let Some(r) = recipient(configured).filter(|_| ch.available()) else {
        return Outcome::Failed("no_recipient");
    };
    if !ch.consented(r) {
        return Outcome::Failed("no_consent");
    }
    ch.send(msg)
}

#[derive(Default)]
pub struct FakeOmni {
    pub recipient: Option<String>,
    pub grants: Vec<Grant>,
    pub sent: Vec<Message>,
}

impl Channel for FakeOmni {
    fn available(&self) -> bool {
        recipient(self.recipient.as_deref()).is_some()
    }

    fn consented(&self, r: &str) -> bool {
        self.grants.iter().any(|g| g.channel == consent::Channel::Omni && g.recipient.as_deref() == Some(r))
    }

    fn send(&mut self, msg: &Message) -> Outcome {
        self.sent.push(msg.clone());
        Outcome::Sent
    }
}

#[cfg(test)]
#[path = "omni_tests.rs"]
mod tests;
