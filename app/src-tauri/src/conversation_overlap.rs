
use crate::conversation_input::{
    has_words, incomplete, GAP_MS, LEVEL_MS, TRAILING_END_MS, VOICE_LEVEL,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Noise,

    #[allow(dead_code)]
    Backchannel,
    Pending,
    Barge,
}
impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Self::Noise => "noise",
            Self::Backchannel => "backchannel",
            Self::Pending => "pending",
            Self::Barge => "barge",
        }
    }
    pub fn decision(self) -> &'static str {
        match self {
            Self::Barge => "yield",
            Self::Backchannel => "keep",
            _ => "ignore",
        }
    }
}
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}
fn echo_token(a: &str, b: &str) -> bool {
    if a == b
        || a.chars()
            .filter(|c| *c != '-')
            .eq(b.chars().filter(|c| *c != '-'))
    {
        return true;
    }
    let a: Vec<_> = a.chars().filter(|c| *c != '-').collect();
    let b: Vec<_> = b.chars().filter(|c| *c != '-').collect();
    if a.len().min(b.len()) < 3 || a.len().max(b.len()) < 4 {
        return false;
    }
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

pub fn filter_echo(text: &str, sentence: &str, played: &str) -> Option<String> {
    let raw: Vec<_> = text.split_whitespace().collect();
    let normalized: Vec<_> = raw.iter().map(|s| words(s).join(" ")).collect();
    let sentence = words(sentence);
    let played = words(played);
    let sources = [
        sentence.as_slice(),
        &played[played.len().saturating_sub(16)..],
    ];
    let mut keep = vec![true; raw.len()];
    for source in sources {
        for i in 0..raw.len() {
            for j in 0..source.len() {
                let n = normalized[i..]
                    .iter()
                    .zip(&source[j..])
                    .take_while(|(a, b)| {
                        if ["wait", "stop", "espera", "peraí", "para", "hold", "on"]
                            .contains(&a.as_str())
                        {
                            a == b
                        } else {
                            echo_token(a, b)
                        }
                    })
                    .count();
                if n >= 2 || (n == 1 && raw.len() == 1) {
                    keep[i..i + n].fill(false);
                }
            }
        }
    }
    keep.iter().any(|keep| !*keep).then(|| {
        raw.iter()
            .zip(keep)
            .filter_map(|(s, keep)| keep.then_some(*s))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

pub fn content_words(text: &str) -> bool {
    has_words(text) && words(text).len() >= 2
}
pub fn classify(text: &str, _voice_ms: u32) -> Class {
    if !has_words(text) {
        Class::Noise
    } else if content_words(text) {
        Class::Barge
    } else {
        Class::Pending
    }
}
#[derive(Debug)]
pub struct Decision {
    pub class: Class,
    pub text: String,
    pub voice_ms: u32,
    pub quiet_ms: u32,
    pub final_words: bool,
    pub began: std::time::Instant,
}
#[derive(Default)]
pub struct Overlap {
    began: Option<std::time::Instant>,
    voice_ms: u32,
    run_ms: u32,
    quiet_ms: u32,
    text: String,
    final_words: bool,
    reported: bool,
}
impl Overlap {

    #[allow(dead_code)]
    pub fn stop_yield(&self) -> bool {
        false
    }
    pub fn hold_due(&self) -> bool {
        self.run_ms >= 300 && self.quiet_ms < GAP_MS
    }
    pub fn snapshot(&self) -> Option<Decision> {
        Some(Decision {
            class: self.class(),
            text: self.text.clone(),
            voice_ms: self.voice_ms,
            quiet_ms: self.quiet_ms,
            final_words: self.final_words,
            began: self.began?,
        })
    }
    pub fn level(&mut self, level: f32) -> Option<Decision> {
        if level.is_finite() && level >= VOICE_LEVEL {
            if self.quiet_ms >= GAP_MS && (self.reported || self.quiet_ms >= TRAILING_END_MS) {
                *self = Self::default();
            }
            self.began.get_or_insert_with(std::time::Instant::now);
            self.run_ms = self.run_ms.saturating_add(LEVEL_MS);
            self.voice_ms = self.voice_ms.max(self.run_ms);
            self.quiet_ms = 0;
        } else {
            self.quiet_ms = self.quiet_ms.saturating_add(LEVEL_MS);
            if self.quiet_ms >= GAP_MS {
                self.run_ms = 0;
            }
        }
        self.decide()
    }
    pub fn words(&mut self, text: &str, final_words: bool) -> Option<Decision> {

        if has_words(text) {
            self.began.get_or_insert_with(std::time::Instant::now);
        }
        let was = self.class();
        self.text = text.into();
        self.final_words = final_words;

        if self.reported && was != Class::Barge && self.class() == Class::Barge {
            self.reported = false;
        }
        self.decide()
    }
    fn class(&self) -> Class {
        classify(&self.text, self.voice_ms)
    }
    fn decide(&mut self) -> Option<Decision> {
        let class = self.class();
        if self.reported
            || (class != Class::Barge
                && self.quiet_ms < TRAILING_END_MS
                && !(self.quiet_ms >= GAP_MS && self.final_words))
        {
            return None;
        }
        if self.voice_ms == 0 && !has_words(&self.text) {
            return None;
        }
        self.report(class)
    }
    fn report(&mut self, class: Class) -> Option<Decision> {
        if self.reported || self.began.is_none() {
            return None;
        }
        self.reported = true;
        Some(Decision {
            class,
            text: self.text.clone(),
            voice_ms: self.voice_ms,
            quiet_ms: self.quiet_ms,
            final_words: self.final_words,
            began: self.began.unwrap_or_else(std::time::Instant::now),
        })
    }
    pub fn finish(&mut self) -> Option<Decision> {
        self.report(self.class())
    }

    pub fn idle(&mut self, level: f32) -> Option<Decision> {
        let decision = self.finish();
        if self.text.is_empty() || classify(&self.text, self.voice_ms) != Class::Pending {
            *self = Self::default();
            return decision;
        }
        if level.is_finite() && level >= VOICE_LEVEL {
            self.voice_ms = self.voice_ms.saturating_add(LEVEL_MS);
            self.quiet_ms = 0;
        } else {
            self.quiet_ms = self.quiet_ms.saturating_add(LEVEL_MS);
        }
        if self.quiet_ms >= TRAILING_END_MS {
            *self = Self::default();
        }
        decision
    }

    pub fn discard_tail(&mut self, text: &str, final_words: bool) -> bool {
        let discard = !self.final_words
            && !self.text.is_empty()
            && self.quiet_ms < TRAILING_END_MS
            && classify(&self.text, self.voice_ms) == Class::Pending
            && words(text) == words(&self.text);
        if discard {
            self.final_words = final_words;
        } else {
            *self = Self::default();
        }
        discard
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Off,
    Low,
    Moderate,
}
impl Level {
    pub fn from_pref(value: Option<&str>, pt: bool) -> Self {
        match value {
            Some("off") => Self::Off,
            Some("moderate") => Self::Moderate,
            Some("low") => Self::Low,
            _ if pt => Self::Off,
            _ => Self::Low,
        }
    }
}
pub const EN: [&str; 4] = ["mm-hm", "yeah", "right", "I see"];
pub const PT: [&str; 4] = ["uhum", "sei", "certo", "entendi"];
#[derive(Default)]
pub struct Listener {
    clock_ms: u64,
    turn_ms: u32,
    voice_ms: u32,
    quiet_ms: u32,
    last: Option<u64>,
    next: usize,
    offered: bool,
    echo: Option<ClipEcho>,
}
struct ClipEcho {
    form: &'static str,

    until: Option<u64>,
    hard_until: u64,
}
impl Listener {
    pub fn clip_started(&mut self, form: &'static str) {
        self.echo = Some(ClipEcho {
            form,
            until: None,
            hard_until: self.clock_ms.saturating_add(5000),
        });
    }
    pub fn clip_finished(&mut self) {
        if let Some(echo) = self.echo.as_mut() {
            echo.until = Some(self.clock_ms.saturating_add(300).min(echo.hard_until));
        }
    }
    pub fn echo_active(&self) -> bool {
        self.echo.as_ref().is_some_and(|e| {
            self.clock_ms < e.hard_until && e.until.is_none_or(|until| self.clock_ms < until)
        })
    }

    pub fn filter_words(&self, text: &str) -> Option<String> {
        if !self.echo_active() {
            return None;
        }
        let form = words(self.echo.as_ref()?.form);
        let tokens: Vec<_> = text.split_whitespace().collect();
        let normalized: Vec<_> = tokens.iter().map(|w| words(w).join(" ")).collect();
        let mut kept = Vec::new();
        let mut n = 0;
        let mut changed = false;
        while n < tokens.len() {
            if normalized.len() - n >= form.len()
                && normalized[n..]
                    .iter()
                    .zip(&form)
                    .all(|(a, b)| echo_token(a, b))
            {
                n += form.len();
                changed = true;
            } else {
                kept.push(tokens[n]);
                n += 1;
            }
        }
        changed.then(|| kept.join(" "))
    }

    pub fn reset_turn(&mut self) {
        self.turn_ms = 0;
        self.voice_ms = 0;
        self.quiet_ms = 0;
        self.offered = false;
    }

    pub fn level(&mut self, level: f32, listening: bool) -> bool {
        let guarded = self.echo_active();
        self.clock_ms = self.clock_ms.saturating_add(u64::from(LEVEL_MS));
        if !listening {
            self.reset_turn();
            return !guarded;
        }
        if guarded {
            return false;
        }
        self.turn_ms = self.turn_ms.saturating_add(LEVEL_MS);
        if level.is_finite() && level >= VOICE_LEVEL {
            self.voice_ms = self.voice_ms.saturating_add(LEVEL_MS);
            self.quiet_ms = 0;
            self.offered = false;
        } else {
            self.quiet_ms = self.quiet_ms.saturating_add(LEVEL_MS);
        }
        true
    }
    pub fn due(&self, text: &str, level: Level) -> Option<usize> {
        self.opportunity(text, level).ok()
    }
    pub fn opportunity(&self, text: &str, level: Level) -> Result<usize, &'static str> {
        let interval = match level {
            Level::Off => return Err("off_or_cache_unready"),
            Level::Low => 24_000,
            Level::Moderate => 20_000,
        };
        if self.offered {
            return Err("already_offered");
        }
        if self.turn_ms < 2000 {
            return Err("first_two_seconds");
        }
        if self.voice_ms < 4000 && (self.turn_ms < 4000 || words(text).len() < 8) {
            return Err("short_turn");
        }
        if !(300..=700).contains(&self.quiet_ms) {
            return Err("outside_pause");
        }
        if !incomplete(text) && (text.trim_end().ends_with(['.', '?', '!']) || !has_words(text)) {
            return Err("completed_sentence");
        }
        if self
            .last
            .is_some_and(|last| self.clock_ms - last < interval)
        {
            return Err("cooldown");
        }
        Ok(self.next % 4)
    }
    pub fn played(&mut self) {
        self.last = Some(self.clock_ms);
        self.voice_ms = 0;
        self.next += 1;
        self.offered = true;
    }
    pub fn pause_ms(&self) -> u32 {
        self.quiet_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_delayed_words_and_noise_do_not_leak_or_repeat() {
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        for _ in 0..8 {
            gate.level(0.0);
        }
        assert_eq!(gate.words("new topic", true).unwrap().class, Class::Barge);
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        for _ in 0..23 {
            assert!(gate.level(0.0).is_none());
        }
        assert_eq!(gate.level(0.0).unwrap().class, Class::Noise);
        assert!(gate.words("[noise]", true).is_none());
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        gate.words("uhum", false);
        assert!(gate.discard_tail("uhum", true));
        assert!(!gate.discard_tail("new question", true));
    }
    #[test]
    fn overlap_delayed_barge_keeps_the_original_audio_endpoint_clock() {
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        for _ in 0..18 {
            gate.level(0.0);
        }
        let d = gate.words("new topic", true).unwrap();
        assert_eq!(d.class, Class::Barge);
        let mut input = crate::conversation_input::Input::default();
        assert_eq!(
            input.admit_overlap(&d.text, d.final_words, d.voice_ms, d.quiet_ms),
            Some("new topic".into()),
            "late ASR does not restart the silence timer"
        );
    }
    #[test]
    fn overlap_tail_gets_one_log_when_playback_ends_before_asr_final() {
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        gate.words("uhum", false);
        assert_eq!(gate.idle(0.0).unwrap().class, Class::Pending);
        assert!(gate.idle(0.0).is_none());
        assert!(gate.discard_tail("uhum", true));
        assert!(gate.finish().is_none());
        let mut gate = Overlap::default();
        for _ in 0..6 {
            gate.level(0.8);
        }
        assert_eq!(gate.idle(0.0).unwrap().class, Class::Noise);
        assert!(gate.idle(0.0).is_none());
    }
    #[test]
    fn listening_two_minute_monologue_has_four_forms_and_no_real_end_sound() {
        for level in [Level::Low, Level::Moderate] {
            let mut p = Listener::default();
            let mut played = Vec::new();
            for ms in (0..120_000).step_by(50) {
                let pause = ms % 5000 >= 4500;
                let real_end = ms >= 115_000;
                p.level(if pause { 0.0 } else { 0.8 }, true);
                if let Some(clip) = p.due(
                    if real_end {
                        "That is everything."
                    } else {
                        "and then I wanted to"
                    },
                    level,
                ) {
                    assert!(ms >= 2000);
                    assert!((300..=700).contains(&p.pause_ms()));
                    assert!(!real_end);
                    if let Some((last, _)) = played.last() {
                        assert!(ms - last >= 8000);
                    }
                    played.push((ms, clip));
                    p.played();
                }
            }
            assert!((2..=6).contains(&played.len()), "{played:?}");
            assert_eq!(
                played
                    .iter()
                    .map(|(_, c)| *c)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                4
            );
        }
    }
    #[test]
    fn listening_defaults_off_boundaries_and_cooldown_across_turns() {
        assert_eq!(Level::from_pref(None, false), Level::Low);
        assert_eq!(Level::from_pref(None, true), Level::Off);
        let mut p = Listener::default();
        for _ in 0..80 {
            p.level(0.8, true);
        }
        for _ in 0..5 {
            p.level(0.0, true);
        }
        assert_eq!(p.due("and", Level::Low), None);
        p.level(0.0, true);
        assert_eq!(p.due("and", Level::Off), None);
        assert_eq!(p.due("Done.", Level::Low), None);
        assert_eq!(p.due("and", Level::Low), Some(0));
        p.played();
        p.reset_turn();
        for _ in 0..80 {
            p.level(0.8, true);
        }
        for _ in 0..6 {
            p.level(0.0, true);
        }
        assert_eq!(p.due("and", Level::Moderate), None);
        p.reset_turn();
        for _ in 0..30 {
            p.level(0.8, true);
        }
        for _ in 0..6 {
            p.level(0.0, true);
        }
        assert_eq!(p.due("and", Level::Low), None);
        p.level(0.0, false);
        assert_eq!(p.due("and", Level::Low), None);
    }
}

#[cfg(test)]
mod review_fix {
    use super::*;
    #[test]
    fn listening_echo_window_preserves_user_words_and_expires_on_audio_time() {
        for form in EN.into_iter().chain(PT) {
            let mut listener = Listener::default();
            let mut input = crate::conversation_input::Input::default();
            input.words("and then", false, false);
            listener.clip_started(form);
            let revision = format!("and then {form}");
            let filtered = listener.filter_words(&revision).unwrap();
            assert_eq!(filtered, "and then");
            input.words(&filtered, true, false);
            for _ in 0..4 {
                if listener.level(0.9, true) {
                    input.level(0.9);
                }
            }
            assert_eq!(input.quiet_ms(), 0, "echo frames freeze endpoint silence");
            assert_eq!(input.text(), "and then");
            assert_eq!(listener.voice_ms, 0, "echo cannot earn another sound");

            listener.clip_finished();
            for _ in 0..5 {
                listener.level(0.0, true);
            }
            assert_eq!(listener.filter_words(form), Some(String::new()));
            listener.level(0.0, true);
            assert_eq!(
                listener.filter_words(form),
                None,
                "genuine later form is preserved"
            );
            assert!(
                listener.level(0.9, true),
                "ordinary accounting resumes after the tail"
            );
        }
        let mut listener = Listener::default();
        listener.clip_started("I see");
        assert_eq!(
            listener.filter_words("Well, I see, then carry on"),
            Some("Well, then carry on".into())
        );
        assert_eq!(listener.filter_words("I need to see the result"), None);
        listener.clip_started("right");
        assert_eq!(
            listener.filter_words("rightly so"),
            None,
            "whole words only"
        );
    }
    #[test]
    fn listening_irregular_pauses_never_play_at_real_endpoints() {
        let mut listener = Listener::default();
        let mut elapsed = 0;
        let mut played = Vec::new();
        for n in 0..24 {
            for _ in 0..[80, 90, 100, 85][n % 4] {
                listener.level(0.8, true);
                elapsed += 50;
            }
            let pause = [200, 500, 850, 650, 100][n % 5];
            let text = if n % 6 == 5 {
                "That is everything."
            } else {
                "and then I wanted to"
            };
            for _ in 0..pause / 50 {
                listener.level(0.0, true);
                elapsed += 50;
                if let Some(form) = listener.due(text, Level::Moderate) {
                    assert!(elapsed >= 2000);
                    assert!((300..=700).contains(&listener.pause_ms()));
                    assert!(!text.ends_with('.'));
                    if let Some((last, _)) = played.last() {
                        assert!(elapsed - last >= 8000);
                    }
                    played.push((elapsed, form));
                    listener.played();
                }
            }
        }
        assert!((2..=6).contains(&played.len()), "{played:?}");
    }
}

#[cfg(test)]
mod floor_echo_cap {
    use super::*;
    #[test]
    fn missing_terminal_cannot_hold_the_floor_forever() {
        let mut listener = Listener::default();
        listener.clip_started("yeah");
        for _ in 0..100 {
            listener.level(0.9, true);
        }
        assert!(listener.level(0.9, true));
        assert_eq!(listener.filter_words("yeah"), None);
        listener.clip_finished();
        assert!(listener.level(0.9, true));
    }
}

#[cfg(test)]
mod doubletalk_regressions {
    use super::*;
    #[test]
    fn doubletalk_listening_partial_clause_with_sparse_voice_can_acknowledge() {
        let mut listener = Listener::default();

        for n in 0..100 {
            listener.level(if n % 5 == 0 { 0.5 } else { 0.2 }, true);
        }
        for _ in 0..6 {
            listener.level(0.0, true);
        }
        assert_eq!(
            listener.due("and I was thinking about how this would work", Level::Low),
            Some(0)
        );
        assert_eq!(listener.due("That is everything.", Level::Low), None);
    }
}

#[cfg(test)]
mod doubletalk_echo_controls {
    use super::*;
    #[test]
    fn doubletalk_delayed_words_can_upgrade_a_reported_noise_or_backchannel() {
        let mut gate = Overlap::default();
        gate.level(0.5);
        for _ in 0..24 {
            gate.level(0.0);
        }
        assert_eq!(
            gate.words("new subject now", false).unwrap().class,
            Class::Barge
        );
        let mut gate = Overlap::default();
        assert!(gate.words("yeah", false).is_none());
        assert_eq!(gate.finish().unwrap().class, Class::Pending);
        assert_eq!(
            gate.words("yeah but what changed", false).unwrap().class,
            Class::Barge
        );
    }
}

#[cfg(test)]
mod doubletalk_clip_echo {
    use super::*;
    #[test]
    fn doubletalk_fuzzy_listening_clip_echo_preserves_user_words() {
        let mut listener = Listener::default();
        listener.clip_started("mm-hm");
        assert_eq!(
            listener.filter_words("and then mhm"),
            Some("and then".into())
        );
        listener.clip_started("right");
        assert_eq!(
            listener.filter_words("and then righ"),
            Some("and then".into())
        );
        assert_eq!(listener.filter_words("and then a new question"), None);
    }
}

#[cfg(test)]
mod review04 {
    use super::*;
    #[test]
    fn h1_two_words_yield_without_word_list_semantics() {
        for text in ["yeah yeah", "makes sense", "pois é", "what changed"] {
            assert_eq!(classify(text, 100), Class::Barge, "{text}");
        }
        for text in ["stop", "espera", "yeah", "banana"] {
            assert_eq!(classify(text, 200), Class::Pending, "{text}");
        }
    }
}
