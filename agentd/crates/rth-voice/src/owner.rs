

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use rth_protocol::{Envelope, Event, PlaybackPhase, ResponseStatus};

use crate::queue::{Flushed, PlayQueue, Push};
use crate::resample::Resampler;
use crate::samples_from_le;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearReason {

    LocalVad,

    ProviderInterrupted,

    UserCancel,

    ResponseCancelled,

    ProviderTail,

    Superseded,

    PushToTalk,

    LocalGate,

    DeviceUnavailable,
}

impl ClearReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ClearReason::LocalVad => "local_vad",
            ClearReason::ProviderInterrupted => "provider_interrupted",
            ClearReason::UserCancel => "user_cancel",
            ClearReason::ResponseCancelled => "response_cancelled",
            ClearReason::ProviderTail => "provider_tail",
            ClearReason::Superseded => "superseded",
            ClearReason::PushToTalk => "push_to_talk",
            ClearReason::LocalGate => "local_gate",
            ClearReason::DeviceUnavailable => "device_unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Cleared {
    pub reason: ClearReason,

    pub responses: Vec<(String, u64, bool)>,
    pub flushed: Flushed,
    pub dropped_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Queued { epoch: u64 },

    Fenced,
    Overflow { epoch: u64, dropped: usize },
    Refused(String),
}

struct Track {
    epoch: u64,
    resampler: Resampler,
    in_rate: u32,
    ended: Option<ResponseStatus>,
    started_sent: bool,
    next_chunk: u64,
    next_sample: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OwnerStats {
    pub chunks_queued: u64,
    pub late_chunks_dropped: u64,
    pub overflow_samples: u64,
    pub refused: u64,
    pub gaps: u64,
    pub clears: u64,
}

pub struct SpeechOwner {
    queue: Arc<PlayQueue>,
    tracks: HashMap<String, Track>,

    closed: VecDeque<String>,
    closed_set: HashSet<String>,
    next_epoch: u64,
    outbox: Vec<Event>,
    pub stats: OwnerStats,
}

impl SpeechOwner {
    const CLOSED_CAP: usize = 1024;

    pub fn new(queue: Arc<PlayQueue>) -> Self {
        Self {
            queue,
            tracks: HashMap::new(),
            closed: VecDeque::new(),
            closed_set: HashSet::new(),
            next_epoch: 1,
            outbox: Vec::new(),
            stats: OwnerStats::default(),
        }
    }

    pub fn queue(&self) -> &Arc<PlayQueue> {
        &self.queue
    }

    pub fn epoch_of(&self, response_id: &str) -> Option<u64> {
        self.tracks.get(response_id).map(|t| t.epoch)
    }

    pub fn live(&self) -> Vec<String> {
        self.tracks.keys().cloned().collect()
    }

    pub fn audible(&self) -> bool {
        !self.tracks.is_empty()
    }

    pub fn tails(&self) -> Vec<String> {
        self.tracks.iter().filter(|(_, t)| t.ended.is_some()).map(|(r, _)| r.clone()).collect()
    }

    pub fn on_envelope(&mut self, env: &Envelope) -> Option<Cleared> {
        match &env.event {
            Event::AgentAudio { response_id, chunk, sample_offset, sample_rate, format, data } => {
                self.audio(response_id, *chunk, *sample_offset, *sample_rate, format, data);
                None
            }
            Event::AgentDone { response_id, status } => self.done(response_id, *status),
            Event::ProviderSignal { response_id, signal } if signal == "interrupted" => {
                let targets: Vec<String> = match response_id {
                    Some(r) => vec![r.clone()],
                    None => self.live(),
                };
                self.clear(ClearReason::ProviderInterrupted, &targets)
            }
            _ => None,
        }
    }

    pub fn audio(&mut self, response_id: &str, chunk: u64, sample_offset: u64, sample_rate: u32, format: &str, data: &str) -> Heard {
        if self.closed_set.contains(response_id) {
            self.stats.late_chunks_dropped += 1;
            return Heard::Fenced;
        }
        if format != "s16le" {
            self.stats.refused += 1;
            return Heard::Refused(format!("unsupported format {format}"));
        }
        let pcm = match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(p) if p.len() % 2 == 0 => p,
            _ => {
                self.stats.refused += 1;
                return Heard::Refused("bad s16le payload".into());
            }
        };
        let out_rate = self.queue.rate;
        let epoch = self.next_epoch;
        let track = self.tracks.entry(response_id.to_string()).or_insert_with(|| Track {
            epoch,
            resampler: Resampler::new(sample_rate, out_rate),
            in_rate: sample_rate,
            ended: None,
            started_sent: false,
            next_chunk: 0,
            next_sample: 0,
        });
        if track.epoch == epoch {
            self.next_epoch += 1;
        }
        if chunk != track.next_chunk || sample_offset != track.next_sample || sample_rate != track.in_rate {
            self.stats.gaps += 1;
        }
        let samples = samples_from_le(&pcm);
        track.next_chunk = chunk + 1;
        track.next_sample = sample_offset + samples.len() as u64;
        let mut out = Vec::with_capacity(samples.len() * out_rate as usize / sample_rate.max(1) as usize + 2);
        track.resampler.process_i16(&samples, &mut out);
        match self.queue.push(track.epoch, out) {
            Push::Queued => {
                self.stats.chunks_queued += 1;
                Heard::Queued { epoch: track.epoch }
            }
            Push::Fenced => {
                self.stats.late_chunks_dropped += 1;
                Heard::Fenced
            }
            Push::Overflow { dropped, .. } => {
                self.stats.overflow_samples += dropped as u64;
                Heard::Overflow { epoch: track.epoch, dropped }
            }
        }
    }

    pub fn refuse(&mut self, response_id: &str, sample_rate: u32, data: &str) -> Option<Cleared> {
        self.stats.refused += 1;
        if self.closed_set.contains(response_id) {
            self.stats.late_chunks_dropped += 1;
            return None;
        }
        let samples = base64::engine::general_purpose::STANDARD.decode(data).map(|b| b.len() as u64 / 2).unwrap_or(0);
        let dropped = samples * self.queue.rate as u64 / sample_rate.max(1) as u64;
        let epoch = self.next_epoch;
        self.next_epoch += 1;
        self.outbox.push(Event::Playback {
            response_id: response_id.into(),
            epoch,
            phase: PlaybackPhase::Cleared,
            played_samples: 0,
            dropped_samples: dropped,
            sample_rate: self.queue.rate,
            reason: Some(ClearReason::DeviceUnavailable.as_str().into()),
            clear_ns: Some(0),
        });
        self.close(response_id.to_string());
        self.stats.clears += 1;
        let flushed = Flushed { dropped: vec![(epoch, dropped)], took_ns: 0, at: Instant::now() };
        Some(Cleared { reason: ClearReason::DeviceUnavailable, responses: vec![(response_id.into(), epoch, true)], flushed, dropped_samples: dropped })
    }

    pub fn done(&mut self, response_id: &str, status: ResponseStatus) -> Option<Cleared> {
        if status == ResponseStatus::Cancelled {
            return self.clear(ClearReason::ResponseCancelled, &[response_id.to_string()]);
        }
        if let Some(t) = self.tracks.get_mut(response_id) {
            t.ended = Some(status);
        }
        None
    }

    pub fn barge_in(&mut self) -> Option<Cleared> {
        let live = self.live();
        self.clear(ClearReason::LocalVad, &live)
    }

    pub fn cancel(&mut self, response_id: &str) -> Option<Cleared> {
        self.clear(ClearReason::UserCancel, &[response_id.to_string()])
    }

    pub fn clear(&mut self, reason: ClearReason, targets: &[String]) -> Option<Cleared> {
        let mut through = 0u64;
        for r in targets {
            match self.tracks.get(r) {
                Some(t) => through = through.max(t.epoch),
                None => self.close(r.clone()),
            }
        }
        if through == 0 {
            return None;
        }
        let flushed = self.queue.flush_through(through);
        let mut stopped: Vec<(String, u64, bool)> =
            self.tracks.iter().filter(|(_, t)| t.epoch <= through).map(|(r, t)| (r.clone(), t.epoch, t.ended.is_none())).collect();
        stopped.sort_by_key(|(_, e, _)| *e);
        let mut dropped_total = 0;
        for (rid, epoch, _) in &stopped {
            let rendered = self.queue.rendered(*epoch);
            let dropped = flushed.dropped_for(*epoch);
            dropped_total += dropped;
            self.outbox.push(Event::Playback {
                response_id: rid.clone(),
                epoch: *epoch,
                phase: PlaybackPhase::Cleared,
                played_samples: rendered.samples,
                dropped_samples: dropped,
                sample_rate: self.queue.rate,
                reason: Some(reason.as_str().into()),
                clear_ns: Some(flushed.took_ns),
            });
            self.tracks.remove(rid);
            self.close(rid.clone());
        }
        self.stats.clears += 1;
        Some(Cleared { reason, responses: stopped, flushed, dropped_samples: dropped_total })
    }

    fn close(&mut self, rid: String) {
        if self.closed_set.insert(rid.clone()) {
            self.closed.push_back(rid);
            if self.closed.len() > Self::CLOSED_CAP
                && let Some(old) = self.closed.pop_front()
            {
                self.closed_set.remove(&old);
            }
        }
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = std::mem::take(&mut self.outbox);
        let mut finished = Vec::new();
        for (rid, t) in self.tracks.iter_mut() {
            let rendered = self.queue.rendered(t.epoch);
            if !t.started_sent && rendered.first_at.is_some() {
                t.started_sent = true;
                events.push(playback(rid, t.epoch, PlaybackPhase::Started, rendered.samples, self.queue.rate));
            }
            if t.ended.is_some() && self.queue.queued_for(t.epoch) == 0 {
                events.push(playback(rid, t.epoch, PlaybackPhase::Finished, rendered.samples, self.queue.rate));
                finished.push(rid.clone());
            }
        }
        for rid in finished {
            self.tracks.remove(&rid);
            self.close(rid);
        }
        events
    }

    pub fn first_rendered(&self, response_id: &str) -> Option<Instant> {
        self.tracks.get(response_id).and_then(|t| self.queue.rendered(t.epoch).first_at)
    }
}

fn playback(rid: &str, epoch: u64, phase: PlaybackPhase, played: u64, rate: u32) -> Event {
    Event::Playback {
        response_id: rid.into(),
        epoch,
        phase,
        played_samples: played,
        dropped_samples: 0,
        sample_rate: rate,
        reason: None,
        clear_ns: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::{Source, Stamper};

    struct Zero;
    impl rth_protocol::Clock for Zero {
        fn now_ns(&self) -> u64 {
            0
        }
    }

    fn b64(samples: &[i16]) -> String {
        base64::engine::general_purpose::STANDARD.encode(crate::le_from_samples(samples))
    }

    fn owner() -> SpeechOwner {
        SpeechOwner::new(Arc::new(PlayQueue::with_seconds(24_000, 30.0)))
    }

    fn feed(o: &mut SpeechOwner, rid: &str, chunk: u64, n: usize) -> Heard {
        o.audio(rid, chunk, chunk * n as u64, 24_000, "s16le", &b64(&vec![1000; n]))
    }

    #[test]
    fn each_response_gets_its_own_epoch_and_plays_in_order() {
        let mut o = owner();
        assert_eq!(feed(&mut o, "r1", 0, 2400), Heard::Queued { epoch: 1 });
        assert_eq!(feed(&mut o, "r1", 1, 2400), Heard::Queued { epoch: 1 });
        o.done("r1", ResponseStatus::Completed);
        assert_eq!(feed(&mut o, "r2", 0, 2400), Heard::Queued { epoch: 2 });
        let mut out = vec![0; 4800];
        o.queue().pull(&mut out);
        let events = o.poll();
        let phases: Vec<_> = events.iter().map(|e| match e { Event::Playback { response_id, phase, played_samples, .. } => (response_id.as_str(), *phase, *played_samples), _ => panic!() }).collect();
        assert_eq!(phases, vec![("r1", PlaybackPhase::Started, 4800), ("r1", PlaybackPhase::Finished, 4800)]);
        assert_eq!(feed(&mut o, "r1", 2, 2400), Heard::Fenced, "a finished response takes no more audio");
    }

    #[test]
    fn barge_in_flushes_at_once_and_late_chunks_of_the_old_epoch_are_dropped() {
        let mut o = owner();
        for c in 0..20 {
            feed(&mut o, "r1", c, 2400);
        }
        let mut out = vec![0; 2400];
        o.queue().pull(&mut out);
        let t0 = Instant::now();
        let cleared = o.barge_in().expect("audible, so a clear");
        let flush_us = t0.elapsed().as_secs_f64() * 1e6;
        assert_eq!(cleared.reason, ClearReason::LocalVad);
        assert_eq!(cleared.responses, vec![("r1".to_string(), 1, true)], "not ended: the caller cancels it");
        assert_eq!(cleared.dropped_samples, 19 * 2400 + 2400 - 2400);
        assert_eq!(o.queue().stats().queued, 0);

        for c in 20..25 {
            assert_eq!(feed(&mut o, "r1", c, 2400), Heard::Fenced);
        }
        assert_eq!(o.stats.late_chunks_dropped, 5);
        assert_eq!(o.queue().pull(&mut out), 0, "silence after the flush");

        let ev = o.poll();
        assert!(matches!(&ev[..], [Event::Playback { phase: PlaybackPhase::Cleared, played_samples: 2400, dropped_samples: 45_600, reason: Some(r), .. }] if r == "local_vad"), "{ev:?}");

        assert_eq!(feed(&mut o, "r2", 0, 2400), Heard::Queued { epoch: 2 });
        assert_eq!(o.queue().pull(&mut out), 2400);
        eprintln!("owner barge-in clear: {flush_us:.1} µs wall, queue held {:.1} µs", cleared.flushed.took_ns as f64 / 1e3);
    }

    #[test]
    fn provider_interrupt_and_cancelled_terminal_clear_and_nothing_audible_means_no_clear() {
        let mut o = owner();
        assert!(o.barge_in().is_none(), "nothing audible: the user just speaks");
        feed(&mut o, "r1", 0, 4800);
        let mut g = Stamper::new(Source::Guest, "g", Box::new(Zero));
        let sig = g.stamp(Event::ProviderSignal { response_id: Some("r1".into()), signal: "interrupted".into() }, None);
        let c = o.on_envelope(&sig).unwrap();
        assert_eq!((c.reason, c.dropped_samples), (ClearReason::ProviderInterrupted, 4800));
        let done = g.stamp(Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Cancelled }, None);
        assert!(o.on_envelope(&done).is_none(), "already cleared");

        assert!(o.cancel("r2").is_none());
        assert_eq!(feed(&mut o, "r2", 0, 100), Heard::Fenced);
        feed(&mut o, "r3", 0, 4800);
        let done = g.stamp(Event::AgentDone { response_id: "r3".into(), status: ResponseStatus::Cancelled }, None);
        assert_eq!(o.on_envelope(&done).unwrap().reason, ClearReason::ResponseCancelled);
    }

    #[test]
    fn output_is_resampled_to_the_device_rate() {
        let mut o = SpeechOwner::new(Arc::new(PlayQueue::with_seconds(48_000, 10.0)));
        feed(&mut o, "r1", 0, 2400);
        let q = o.queue().stats().queued;
        assert!((4797..=4800).contains(&q), "{q}");
    }

    #[test]
    fn with_no_device_a_response_is_refused_with_one_terminal_record_and_never_played() {
        let mut o = owner();
        let c = o.refuse("r1", 24_000, &b64(&vec![1000; 2400])).expect("first chunk: one record");
        assert_eq!((c.reason, c.dropped_samples), (ClearReason::DeviceUnavailable, 2400));
        for _ in 0..5 {
            assert!(o.refuse("r1", 24_000, &b64(&vec![1000; 2400])).is_none(), "later chunks: counted only");
        }
        assert!(o.done("r1", ResponseStatus::Completed).is_none());
        let ev = o.poll();
        assert!(matches!(&ev[..], [Event::Playback { phase: PlaybackPhase::Cleared, played_samples: 0, reason: Some(r), .. }] if r == "device_unavailable"), "{ev:?}");
        assert!(o.poll().is_empty(), "no started, no finished, ever");
        assert_eq!(o.queue().stats().queued, 0);
        assert_eq!(o.stats.late_chunks_dropped, 5);
        assert!(!o.audible());
    }
}

