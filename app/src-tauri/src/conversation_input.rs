
pub const LEVEL_MS: u32 = 50;
pub const VOICE_LEVEL: f32 = 0.44;
pub const BARGE_MS: u32 = 300;
pub const GAP_MS: u32 = 100;
pub const END_MS: u32 = 800;
pub const SHORT_END_MS: u32 = 900;
pub const TRAILING_END_MS: u32 = 1200;
pub fn has_words(text: &str) -> bool {
    let mut words = text.to_lowercase();
    for noise in [
        "[noise]",
        "[silence]",
        "[music]",
        "[inaudible]",
        "(noise)",
        "(silence)",
    ] {
        words = words.replace(noise, "");
    }
    words.chars().any(char::is_alphanumeric)
}

pub fn incomplete(text: &str) -> bool {
    if text.trim_end().ends_with(['.', '?', '!']) {
        return false;
    }
    let last = text
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    trailing_connector(&last)
        || [
            "is", "are", "was", "my", "your", "meu", "minha", "um", "uma", "é", "está",
        ]
        .contains(&last.as_str())
}
fn trailing_connector(last: &str) -> bool {
    [
        "and", "or", "but", "about", "to", "the", "a", "of", "with", "because", "e", "ou", "mas",
        "sobre", "para", "de", "com", "porque",
    ]
    .contains(&last)
}
#[derive(Default)]
pub struct Input {
    semantic: bool,
    model_settled: usize,
    turn_ms: u32,
    voice_ms: u32,
    quiet_ms: u32,
    speech_seen: bool,
    evidence_words: bool,
    partial: String,
    final_text: String,
}
impl Input {
    pub fn set_semantic(&mut self, on: bool) {
        self.semantic = on;
    }
    pub fn semantic() -> Self {
        Self {
            semantic: true,
            ..Self::default()
        }
    }
    pub fn settled_words(&self) -> usize {
        self.model_settled
    }
    pub fn settle_partial(&mut self, n: usize) {
        self.model_settled =
            (self.final_words_count() + n).min(self.text().split_whitespace().count());
    }
    pub fn model_words(&mut self, text: &str, final_words: bool) {
        self.semantic = true;
        if final_words {
            self.partial.clear();
        }
        if !has_words(text) {
            return;
        }
        if final_words {
            if !self.final_text.is_empty() {
                self.final_text.push(' ');
            }
            self.final_text.push_str(text.trim());
        } else {
            self.partial = text.trim().into();
        }
        self.model_settled = self.final_words_count();
    }

    pub fn model_ready(&mut self, take: bool) -> Option<String> {
        let text = self.text();
        if !has_words(&text) || !take {
            return None;
        }
        *self = Self::semantic();
        Some(text)
    }

    pub fn rules_ready(&mut self) -> Option<String> {
        self.semantic = false;
        let out = self.ready(false);
        self.semantic = true;
        out
    }
    pub fn final_words_count(&self) -> usize {
        self.final_text.split_whitespace().count()
    }

    pub fn quiet_ms(&self) -> u32 {
        self.quiet_ms
    }
    pub fn text(&self) -> String {
        format!("{} {}", self.final_text, self.partial)
            .trim()
            .into()
    }

    pub fn admit_overlap(
        &mut self,
        text: &str,
        final_words: bool,
        voice_ms: u32,
        quiet_ms: u32,
    ) -> Option<String> {
        *self = Self {
            semantic: self.semantic,
            voice_ms,
            quiet_ms,
            speech_seen: voice_ms >= BARGE_MS,
            ..Self::default()
        };
        if self.semantic {
            self.model_words(text, final_words);
            None
        } else {
            self.words(text, final_words, false)
        }
    }
    pub fn turn_ms(&self) -> u32 {
        self.turn_ms
    }
    pub fn level(&mut self, level: f32) {
        self.turn_ms = self.turn_ms.saturating_add(LEVEL_MS);
        if level.is_finite() && level >= VOICE_LEVEL {
            self.voice_ms = self.voice_ms.saturating_add(LEVEL_MS);
            self.quiet_ms = 0;
            self.speech_seen |= self.voice_ms >= BARGE_MS;
        } else {
            self.quiet_ms = self.quiet_ms.saturating_add(LEVEL_MS);
            if self.quiet_ms >= GAP_MS {
                self.voice_ms = 0;
                self.evidence_words = false;
            }
        }
    }
    pub fn words(&mut self, text: &str, final_words: bool, playing: bool) -> Option<String> {

        if final_words {
            self.partial.clear();
        }
        if !has_words(text) {
            return self.ready(playing);
        }
        self.evidence_words = true;
        if final_words {
            if !self.final_text.is_empty() {
                self.final_text.push(' ');
            }
            self.final_text.push_str(text.trim());
        } else {
            self.partial = text.trim().into();
        }
        self.ready(playing)
    }
    pub fn ready(&mut self, playing: bool) -> Option<String> {
        if self.semantic {
            return self.model_ready(false);
        }
        if (playing && !self.speech_seen) || self.final_text.is_empty() || !self.partial.is_empty()
        {

            if playing && self.quiet_ms >= TRAILING_END_MS {
                *self = Self::default();
            }
            return None;
        }
        let last = self
            .final_text
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        let trailing = trailing_connector(&last);
        let wait = if trailing {
            TRAILING_END_MS
        } else if self.final_text.split_whitespace().count() < 3 {
            SHORT_END_MS
        } else {
            END_MS
        };
        if self.quiet_ms < wait {
            return None;
        }
        let text = std::mem::take(&mut self.final_text);
        *self = Self::default();
        Some(text)
    }
    #[cfg(test)]
    pub fn should_barge(&self) -> bool {
        self.voice_ms >= BARGE_MS && self.quiet_ms < GAP_MS && self.evidence_words
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn levels(i: &mut Input, level: f32, ms: u32) {
        for _ in 0..ms / LEVEL_MS {
            i.level(level);
        }
    }
    #[test]
    fn floor_mode_change_preserves_words_and_pause_clock() {
        let mut i = Input::semantic();
        i.model_words("a complete request", true);
        levels(&mut i, 0.0, 800);
        assert_eq!(i.ready(false), None);
        i.set_semantic(false);
        assert_eq!(i.quiet_ms(), 800);
        assert_eq!(i.ready(false), Some("a complete request".into()));
        i.model_words("one two three", false);
        i.settle_partial(1);
        levels(&mut i, 0.0, 50);
        assert_eq!(
            i.settled_words(),
            1,
            "a level tick cannot settle an ASR partial"
        );
    }
    #[test]
    fn own_voice_energy_alone_never_interrupts() {
        let mut i = Input::default();
        levels(&mut i, 0.9, 5000);
        assert!(!i.should_barge());
        i.words("[noise] [music]", false, true);
        assert!(!i.should_barge());
    }
    #[test]
    fn words_and_sustained_post_aec_voice_interrupt_at_300ms() {
        let mut i = Input::default();
        i.words("wait a moment", false, true);
        levels(&mut i, 0.7, 250);
        assert!(!i.should_barge());
        levels(&mut i, 0.7, 50);
        assert!(i.should_barge());
    }
    #[test]
    fn one_loud_click_and_old_words_cannot_accumulate_into_a_barge() {
        let mut i = Input::default();
        i.words("wait", false, true);
        for _ in 0..10 {
            levels(&mut i, 0.9, 50);
            levels(&mut i, 0.0, 200);
        }
        assert!(!i.should_barge());
    }
    #[test]
    fn short_mid_sentence_pause_waits_then_joins_the_continuation() {
        let mut i = Input::default();
        levels(&mut i, 0.7, 500);
        levels(&mut i, 0.0, 300);
        assert_eq!(i.words("tell me about", true, true), None);
        levels(&mut i, 0.7, 500);
        assert_eq!(i.words("the project", false, true), None);
        levels(&mut i, 0.0, 800);
        assert_eq!(
            i.words("the project", true, true),
            Some("tell me about the project".into())
        );
        assert_eq!(i.ready(true), None, "a turn is released only once");
    }
    #[test]
    fn short_complete_turn_and_trailing_connector_have_bounded_grace() {
        let mut i = Input::default();
        levels(&mut i, 0.7, 300);
        levels(&mut i, 0.0, 800);
        assert_eq!(i.words("yes", true, true), None);
        levels(&mut i, 0.0, 100);
        assert_eq!(i.ready(true), Some("yes".into()));
        levels(&mut i, 0.7, 300);
        levels(&mut i, 0.0, 900);
        assert_eq!(i.words("I want to", true, true), None);
        levels(&mut i, 0.0, 300);
        assert_eq!(i.ready(true), Some("I want to".into()));
    }
}

#[cfg(test)]
mod review_r2 {
    use super::*;
    fn ticks(i: &mut Input, level: f32, ms: u32, playing: bool) -> Vec<String> {
        let mut released = Vec::new();
        for _ in 0..ms / LEVEL_MS {
            i.level(level);
            if let Some(text) = i.ready(playing) {
                released.push(text);
            }
        }
        released
    }
    #[test]
    fn review_r2_soft_final_listening_releases_without_barge_energy() {
        let mut i = Input::default();
        assert!(ticks(&mut i, 0.3, 300, false).is_empty());
        assert_eq!(i.words("yes that works", true, false), None);
        assert_eq!(
            ticks(&mut i, 0.3, END_MS - 300, false),
            vec!["yes that works"]
        );

        assert_eq!(i.words("yes", true, false), None);
        assert_eq!(ticks(&mut i, 0.3, SHORT_END_MS, false), vec!["yes"]);
    }
    #[test]
    fn review_r2_echo_final_is_discarded_before_the_next_user_sentence() {
        let mut i = Input::default();
        assert_eq!(i.words("okay", true, true), None);
        assert!(ticks(&mut i, 0.0, TRAILING_END_MS, true).is_empty());
        assert!(ticks(&mut i, 0.7, 500, true).is_empty());
        assert_eq!(i.words("what time is it", true, true), None);
        assert_eq!(ticks(&mut i, 0.0, END_MS, true), vec!["what time is it"]);
    }
    #[test]
    fn review_r2_noise_only_final_clears_a_stale_partial_and_releases() {
        let mut i = Input::default();
        assert!(ticks(&mut i, 0.7, BARGE_MS, false).is_empty());
        assert_eq!(i.words("the actual question", true, false), None);
        assert_eq!(i.words("phantom partial", false, false), None);
        assert!(ticks(&mut i, 0.0, END_MS, false).is_empty());
        assert_eq!(
            i.words("[noise] [silence]", true, false),
            Some("the actual question".into())
        );
        assert_eq!(i.ready(false), None);
    }
    #[test]
    fn review_r2_wordless_burst_cannot_back_a_later_echo_barge_or_submit() {
        let mut i = Input::default();
        assert!(ticks(&mut i, 0.7, BARGE_MS, true).is_empty());
        assert!(!i.should_barge());
        assert!(ticks(&mut i, 0.0, 1300, true).is_empty());
        assert_eq!(i.words("sure", true, true), None);
        assert!(!i.should_barge());
        assert!(ticks(&mut i, 0.0, TRAILING_END_MS, true).is_empty());
    }
    #[test]
    fn review_r2_playback_stale_partial_expires_without_an_asr_final() {
        let mut i = Input::default();
        assert!(ticks(&mut i, 0.7, BARGE_MS, true).is_empty());
        assert_eq!(i.words("residual echo", false, true), None);
        assert!(ticks(&mut i, 0.0, TRAILING_END_MS, true).is_empty());
        assert!(i.partial.is_empty());
        assert!(i.final_text.is_empty());
        assert!(!i.speech_seen);
    }
    #[test]
    fn review_r2_asr_lag_keeps_voice_evidence_past_the_short_gap() {
        let mut i = Input::default();
        assert!(ticks(&mut i, 0.7, BARGE_MS, true).is_empty());
        assert!(ticks(&mut i, 0.0, 400, true).is_empty());
        assert_eq!(i.words("wait", true, true), None);
        assert_eq!(ticks(&mut i, 0.0, SHORT_END_MS - 400, true), vec!["wait"]);
    }
}
