
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Off,
    Listening,
    Thinking,
    Speaking,
    Paused,
    Interrupted,
}
impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Listening => "listening",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
            Self::Paused => "paused",
            Self::Interrupted => "interrupted",
        }
    }
}
#[derive(Default)]
pub struct Conversation {
    pub phase: Phase,

    pub epoch: u64,
    pub turn: u64,
    pub busy: bool,
    pub submitted: bool,
    pub(crate) model_done: bool,
    pub(crate) speech_pending: bool,
    pub(crate) barged: bool,
    pub(crate) pending: Option<String>,
}
impl Conversation {
    pub fn start(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.barged = false;
        self.phase = if self.busy {
            Phase::Thinking
        } else {
            Phase::Listening
        };
    }
    pub fn stop(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.speech_pending = false;
        if !self.submitted || self.model_done {
            self.busy = false;
        }
        self.pending = None;
        self.barged = true;
        self.phase = Phase::Off;
    }
    pub fn endpoint(&mut self, text: &str) -> Option<String> {
        let text = text.trim();
        if self.phase == Phase::Off || !super::conversation_input::has_words(text) {
            return None;
        }
        if self.busy {

            if self.barged {
                self.pending = Some(match self.pending.take() {
                    Some(p) => format!("{p} {text}"),
                    None => text.into(),
                });
            }
            return None;
        }
        if !matches!(self.phase, Phase::Listening | Phase::Interrupted) {
            return None;
        }
        self.turn = self.turn.wrapping_add(1);
        self.busy = true;
        self.submitted = false;
        self.model_done = false;
        self.speech_pending = false;
        self.barged = false;
        self.phase = Phase::Thinking;
        Some(text.to_string())
    }
    pub fn take_ready(&mut self) -> Option<String> {
        if self.busy || self.phase == Phase::Off {
            return None;
        }
        let text = self.pending.take()?;
        self.endpoint(&text)
    }
    pub fn typed(&mut self, text: &str) -> bool {

        if self.busy {
            return false;
        }
        self.endpoint(text).is_some()
    }
    pub fn manual(&mut self, text: &str) -> bool {
        if !self.typed(text) {
            return false;
        }

        self.epoch = self.epoch.wrapping_add(1);
        true
    }

    pub fn yield_floor(&mut self) {
        if matches!(self.phase, Phase::Speaking | Phase::Thinking) {
            self.phase = Phase::Paused;
        }
    }
    pub fn continue_floor(&mut self) {
        if self.phase == Phase::Paused {
            self.phase = if self.speech_pending {
                Phase::Speaking
            } else {
                Phase::Thinking
            };
        }
    }

    pub fn answer_floor(&mut self) {
        self.barged = true;
        self.speech_pending = false;
        if !self.submitted || self.model_done {
            self.busy = false;
        }
        self.phase = Phase::Interrupted;
    }
    pub fn speech_started(&mut self) {
        if self.busy && self.phase != Phase::Off && !self.barged {
            self.speech_pending = true;
            if self.phase != Phase::Paused {
                self.phase = Phase::Speaking;
            }
        }
    }
    pub fn speech_ended_for(&mut self, turn: u64) {
        if turn == self.turn {
            self.speech_ended();
        }
    }
    pub fn speech_ended(&mut self) {
        self.speech_pending = false;
        self.resume();
    }
    pub fn reply_done(&mut self) {
        self.model_done = true;
        self.resume();
    }
    fn resume(&mut self) {
        if self.model_done && !self.speech_pending {
            self.busy = false;
            if self.phase != Phase::Off {
                self.phase = if self.barged {
                    Phase::Interrupted
                } else {
                    Phase::Listening
                };
            }
        } else if !self.speech_pending
            && !matches!(self.phase, Phase::Off | Phase::Paused)
            && !self.barged
        {
            self.phase = Phase::Thinking;
        }
    }

    pub fn barge(&mut self) {
        if matches!(
            self.phase,
            Phase::Thinking | Phase::Speaking | Phase::Paused
        ) {
            self.barged = true;
            self.speech_pending = false;
            if !self.submitted || self.model_done {
                self.busy = false;
            }
            self.phase = Phase::Interrupted;
        }
    }
    pub fn accepts(&self, epoch: u64) -> bool {
        self.phase != Phase::Off && self.epoch == epoch
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_r4_manual_turn_invalidates_captured_partial_and_refuses_busy_without_reset() {
        let mut c = Conversation::default();
        c.start();
        let old = c.epoch;
        assert!(c.manual("current words"));
        assert!(
            !c.accepts(old),
            "queued partial audio must not repeat the manual turn"
        );
        assert!(
            c.accepts(c.epoch),
            "microphone remains admitted for the new generation"
        );
        let epoch = c.epoch;
        let turn = c.turn;
        assert!(!c.manual("second press"));
        assert_eq!((c.epoch, c.turn), (epoch, turn));
        assert_eq!(c.phase, Phase::Thinking);
    }
    #[test]
    fn endpoint_submits_once_and_drops_noise_and_busy_endpoints() {
        let mut c = Conversation::default();
        c.start();
        assert_eq!(c.endpoint(" ... ♪ [noise] "), None);
        assert_eq!(c.endpoint(" Hello there! "), Some("Hello there!".into()));
        assert_eq!(c.phase, Phase::Thinking);
        assert_eq!(c.endpoint("a second turn"), None);
    }
    #[test]
    fn aec_mic_stays_live_and_state_resumes_after_audio_and_model_end() {
        let mut c = Conversation::default();
        c.start();
        let before = c.epoch;
        assert!(c.accepts(before));
        c.endpoint("Hello");
        c.speech_started();
        assert_eq!(c.phase, Phase::Speaking);
        assert!(c.accepts(before));
        c.reply_done();
        assert_eq!(c.phase, Phase::Speaking);
        c.speech_ended();
        assert_eq!(c.phase, Phase::Listening);
        assert!(c.accepts(before));
        assert!(c.accepts(c.epoch));
    }
    #[test]
    fn barge_listens_but_never_submits_over_an_inflight_model() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("Hi");
        c.submitted = true;
        c.speech_started();
        c.barge();
        assert_eq!(c.phase, Phase::Interrupted);
        assert!(c.accepts(c.epoch));
        assert_eq!(c.endpoint("too soon"), None);
        c.speech_started();
        assert_eq!(
            c.phase,
            Phase::Interrupted,
            "late speech cannot re-arm after barge"
        );
        c.reply_done();
        assert_eq!(c.take_ready(), Some("too soon".into()));
        c.reply_done();
        assert_eq!(c.endpoint("next"), Some("next".into()));
    }
    #[test]
    fn escape_stops_and_late_events_cannot_restart() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("Hi");
        c.speech_started();
        c.stop();
        c.speech_ended();
        c.reply_done();
        assert_eq!(c.phase, Phase::Off);
        assert_eq!(c.endpoint("late"), None);
        assert!(!c.accepts(c.epoch));
    }
    #[test]
    fn dsh_typed_turn_is_spoken_and_uses_the_same_busy_and_audio_fence() {
        let mut c = Conversation::default();
        c.start();
        let audio = c.epoch;
        assert!(c.typed("A question from the DSH page"));
        assert_eq!(c.phase, Phase::Thinking);
        assert!(c.accepts(audio));
        assert!(!c.typed("second"));
        assert_eq!(c.endpoint("mic overlap"), None);
        c.speech_started();
        c.reply_done();
        assert_eq!(c.phase, Phase::Speaking);
        c.speech_ended();
        assert!(c.typed("next typed question"));
        c.stop();
        assert!(!c.typed("off"));
    }
    #[test]
    fn a_previous_speech_callback_cannot_unmute_a_new_turn() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("first");
        let old = c.turn;
        c.submitted = true;
        c.speech_started();
        c.reply_done();
        c.stop();
        c.start();
        c.endpoint("second");
        c.speech_started();
        c.reply_done();
        c.speech_ended_for(old);
        assert_eq!(c.phase, Phase::Speaking);
        c.speech_ended_for(c.turn);
        assert_eq!(c.phase, Phase::Listening);
    }
    #[test]
    fn restart_waits_for_an_uncancelled_model_and_stale_audio_is_rejected() {
        let mut c = Conversation::default();
        c.start();
        let audio = c.epoch;
        c.endpoint("first");
        c.submitted = true;
        c.stop();
        c.start();
        assert_eq!(c.phase, Phase::Thinking);
        assert!(!c.accepts(audio));
        assert_eq!(c.endpoint("overlap"), None);
        c.reply_done();
        assert_eq!(c.phase, Phase::Listening);
        assert_eq!(c.endpoint("next"), Some("next".into()));
    }
    #[test]
    fn failed_or_silent_reply_resumes_and_audio_ending_early_waits_for_model() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("Hi");
        c.reply_done();
        assert_eq!(c.phase, Phase::Listening);
        c.endpoint("Hi again");
        c.speech_started();
        c.speech_ended();
        assert_eq!(c.phase, Phase::Thinking);
        c.reply_done();
        assert_eq!(c.phase, Phase::Listening);
    }
}

#[cfg(test)]
mod duplex_red {
    use super::*;
    #[test]
    fn duplex_mic_keeps_accepting_during_thinking_and_speaking() {
        let mut c = Conversation::default();
        c.start();
        let epoch = c.epoch;
        c.endpoint("first question");
        assert!(
            c.accepts(epoch),
            "AEC input must reach ASR during inference"
        );
        c.speech_started();
        assert!(c.accepts(epoch), "AEC input must reach ASR during playback");
    }
    #[test]
    fn duplex_barge_fences_speech_while_thinking_too() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("first question");
        c.barge();
        c.speech_started();
        assert!(
            c.barged,
            "Thinking barge must cancel the old turn before its first audio"
        );
        assert!(!c.speech_pending, "late speech must stay fenced");
    }
    #[test]
    fn duplex_barge_without_inflight_model_admits_the_new_words() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("first question");
        c.barge();
        assert_eq!(
            c.endpoint("actually a different question"),
            Some("actually a different question".into()),
            "a queued but unsent old turn is invalidated, not awaited forever"
        );
    }
}

#[cfg(test)]
mod duplex_cancel_tests {
    use super::*;
    #[test]
    fn interrupting_words_wait_for_cancel_terminal_then_submit_once() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("first");
        c.submitted = true;
        c.speech_started();
        let old = c.turn;
        c.barge();
        assert_eq!(c.phase, Phase::Interrupted);
        assert!(!c.speech_pending);
        assert_eq!(c.endpoint("actually the other project"), None);
        assert!(c.busy);
        c.reply_done();
        assert_eq!(c.take_ready(), Some("actually the other project".into()));
        c.speech_started();
        c.speech_ended_for(old);
        assert_eq!(c.phase, Phase::Speaking);
        assert_eq!(c.take_ready(), None);
        c.stop();
        c.reply_done();
        assert_eq!(c.take_ready(), None);
    }
}

#[cfg(test)]
mod floor_state_tests {
    use super::*;
    #[test]
    fn yield_keeps_work_and_generation_then_keep_resumes_same_playout() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("explain the plan");
        c.submitted = true;
        c.speech_started();
        let before = (
            c.busy,
            c.submitted,
            c.model_done,
            c.speech_pending,
            c.barged,
            c.turn,
            c.epoch,
        );
        c.yield_floor();
        assert_eq!(c.phase, Phase::Paused);
        assert_eq!(
            (
                c.busy,
                c.submitted,
                c.model_done,
                c.speech_pending,
                c.barged,
                c.turn,
                c.epoch
            ),
            before
        );
        c.reply_done();
        assert_eq!(c.phase, Phase::Paused);
        assert!(c.busy);
        c.continue_floor();
        assert_eq!(c.phase, Phase::Speaking);
        c.speech_ended();
        assert_eq!(c.phase, Phase::Listening);
    }
    #[test]
    fn compatible_answer_waits_for_work_completion_and_submits_once() {
        let mut c = Conversation::default();
        c.start();
        c.endpoint("first request");
        c.submitted = true;
        c.speech_started();
        c.answer_floor();
        assert!(c.busy);
        assert_eq!(c.endpoint("also add the size"), None);
        c.reply_done();
        assert_eq!(c.take_ready(), Some("also add the size".into()));
        assert_eq!(c.take_ready(), None);
    }
}
