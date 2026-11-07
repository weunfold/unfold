

use super::{runtime::Ticket, Error, Result};
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    OnceLock,
};

pub struct Capture<const N: usize = 2048> {
    revision: AtomicU64,
    current: AtomicUsize,
    next: AtomicUsize,
    slots: [OnceLock<Ticket>; N],
    readers: AtomicUsize,
    legacy_capture_fallbacks: AtomicU64,
}
impl<const N: usize> Default for Capture<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> Capture<N> {
    pub const fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
            current: AtomicUsize::new(usize::MAX),
            next: AtomicUsize::new(0),
            slots: [const { OnceLock::new() }; N],
            readers: AtomicUsize::new(0),
            legacy_capture_fallbacks: AtomicU64::new(0),
        }
    }

    pub fn reader(self: &std::sync::Arc<Self>) -> Reader<N> {
        Reader(self.clone(), std::sync::atomic::AtomicBool::new(false))
    }
    pub fn readers(&self) -> usize { self.readers.load(Ordering::SeqCst) }
    pub fn record_capture_fallback(&self) -> u64 { self.legacy_capture_fallbacks.fetch_add(1,Ordering::SeqCst)+1 }
    pub fn metrics(&self) -> serde_json::Value { serde_json::json!({
        "retained":self.retained(),"capacity":self.capacity(),"readers":self.readers(),
        "legacy_capture_fallbacks":self.legacy_capture_fallbacks.load(Ordering::SeqCst)
    }) }
    pub fn matches(&self,epoch:super::runtime::Epoch,response:&str)->bool {
        self.slots.get(self.current.load(Ordering::SeqCst)).and_then(OnceLock::get)
            .is_some_and(|ticket|ticket.epoch()==epoch && ticket.response()==response)
    }
    pub fn retained(&self) -> usize { self.next.load(Ordering::SeqCst) }
    pub const fn capacity(&self) -> usize { N }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }
    fn fence_current(&self) {
        if let Some(ticket) = self
            .slots
            .get(self.current.load(Ordering::SeqCst))
            .and_then(OnceLock::get)
        {
            ticket.fence();
        }
    }

    pub fn close_matching(&self,epoch:super::runtime::Epoch,response:&str)->bool {
        if let Some(t)=self.slots.get(self.current.load(Ordering::SeqCst)).and_then(OnceLock::get) {
            if t.epoch()==epoch && t.response()==response {t.close();return true}
        }
        false
    }

    pub fn close(&self) {
        if let Some(t)=self.slots.get(self.current.load(Ordering::SeqCst)).and_then(OnceLock::get) { t.close(); }
    }

    pub fn stop_user(&self,stop:impl FnOnce(&Ticket)) {
        let old=self.slots.get(self.current.load(Ordering::SeqCst)).and_then(OnceLock::get);
        self.fence();
        if let Some(ticket)=old {stop(ticket);}
    }

    pub fn fence(&self) -> u64 {
        let revision = self.revision.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        self.fence_current();
        revision
    }

    pub fn publish(&self, ticket: &Ticket, expected: u64) -> Result<()> {
        if self.revision() != expected {
            ticket.fence();
            return Err(Error::Cancelled);
        }
        let index = self
            .next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < N).then(|| n + 1)
            })
            .map_err(|_| Error::Busy)?;
        let slot = self.slots.get(index).ok_or(Error::Busy)?;
        slot.set(ticket.clone()).map_err(|_| Error::Context)?;
        if let Some(old) = self
            .slots
            .get(self.current.load(Ordering::SeqCst))
            .and_then(OnceLock::get)
        {
            old.close();
        }
        self.current.store(index, Ordering::SeqCst);
        if self.revision() != expected {
            ticket.fence();
            return Err(Error::Cancelled);
        }
        Ok(())
    }
}

pub struct Reader<const N: usize = 2048>(std::sync::Arc<Capture<N>>, std::sync::atomic::AtomicBool);
impl<const N: usize> Reader<N> {

    pub fn activate(&self) {
        if !self.1.swap(true,Ordering::SeqCst) {self.0.readers.fetch_add(1,Ordering::SeqCst);}
    }
    pub fn fence(&self) { self.0.fence(); }
}
impl<const N: usize> Drop for Reader<N> {
    fn drop(&mut self) { if self.1.load(Ordering::SeqCst) {self.0.readers.fetch_sub(1, Ordering::SeqCst);} }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_front::{runtime::Epoch, Error};
    fn ticket(id: &str) -> Ticket {
        Ticket::new(
            Epoch {
                capture: 2,
                turn: 3,
            },
            id.into(),
        )
    }
    fn render(t: &Ticket) -> bool {
        t.render(
            Epoch {
                capture: 2,
                turn: 3,
            },
            t.response(),
            1,
        )
    }
    #[test]
    fn onset_before_publication_refuses_late_audio_and_marks_user_cancel() {
        let c = Capture::<2>::new();
        let before = c.revision();
        c.fence();
        let t = ticket("late");
        assert_eq!(c.publish(&t, before), Err(Error::Cancelled));
        assert!(!render(&t));
        assert!(t.cancelled());
    }
    #[test]
    fn published_ticket_and_superseded_ticket_are_fenced_without_lookup_locks() {
        let c = Capture::<2>::new();
        let first = ticket("first");
        let next = ticket("next");
        c.publish(&first, c.revision()).unwrap();
        assert!(render(&first));
        c.publish(&next, c.revision()).unwrap();
        assert!(!render(&first));
        assert!(
            !first.cancelled(),
            "publishing a newer response is not a user cancellation"
        );
        assert!(!next.cancelled());
        c.fence();
        assert!(!render(&next));
        assert!(next.cancelled());
        assert_eq!(first.frames(), 1);
    }
    #[test]
    fn bounded_retention_never_reuses_a_slot_or_turns_capacity_into_user_cancel() {
        let c = Capture::<1>::new();
        let first = ticket("first");
        c.publish(&first, 0).unwrap();
        let next = ticket("next");
        assert_eq!(c.publish(&next, 0), Err(Error::Busy));
        assert!(
            !next.cancelled(),
            "capacity failure must still permit zero-frame legacy fallback"
        );
        c.fence();
        assert!(!render(&first));
    }
    #[test]
    fn publication_racing_onset_always_fences_the_exact_ticket_before_late_render() {
        for _ in 0..128 {
            let c = std::sync::Arc::new(Capture::<2>::new());
            let t = ticket("race");
            let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
            let (other, g) = (c.clone(), gate.clone());
            let worker = std::thread::spawn(move || {
                g.wait();
                other.fence();
            });
            gate.wait();
            let result = c.publish(&t, 0);
            worker.join().unwrap();
            assert!(result.is_ok() || result == Err(Error::Cancelled));
            assert!(!render(&t));
        }
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn capture_readers_keep_only_their_own_rotation_alive() {
        let first = Arc::new(Capture::<2>::new());
        let weak = Arc::downgrade(&first);
        assert_eq!(first.readers(), 0);
        let reader = first.reader();
        assert_eq!(first.readers(), 0, "an engine still opening cannot admit front speech");
        reader.activate(); reader.activate();
        assert_eq!(first.readers(), 1);
        let next = Arc::new(Capture::<2>::new());
        let epoch = super::super::runtime::Epoch { capture: 2, turn: 3 };
        let old = Ticket::new(epoch, "old".into());
        let new = Ticket::new(epoch, "new".into());
        first.publish(&old, 0).unwrap();
        next.publish(&new, 0).unwrap();
        drop(first);
        reader.fence();
        assert!(old.cancelled());
        assert!(!new.cancelled(), "an old callback must never fence the new session");
        drop(reader);
        assert!(weak.upgrade().is_none(), "old arenas must retire after their last reader");
        assert_eq!(next.retained(), 1);
        assert_eq!(next.record_capture_fallback(), 1);
        assert_eq!(next.metrics()["legacy_capture_fallbacks"], 1);
        assert_eq!(next.readers(), 0, "new session cannot admit before capture attaches");
    }
}
