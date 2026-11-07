

use crate::dictation::{Recognizer, RATE};

pub const SETTLE_EVERY: usize = RATE;

pub const LAG: f32 = 1.5;

pub const LONGEST: usize = RATE * 30;

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

fn key(word: &str) -> String {
    word.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[derive(Default, Debug)]
pub struct Agreement {
    settled: Vec<String>,

    settled_end: f32,
    tail: Vec<String>,
    last: Vec<Word>,
    passes: u32,
}

const SAME_WORD: f32 = 0.25;

fn after(words: &[Word], end: f32, last: Option<&String>) -> usize {
    let anchor = last.map(|l| key(l)).and_then(|l| {
        words.iter().enumerate().filter(|(_, w)| key(&w.text) == l && (w.end - end).abs() <= SAME_WORD).min_by(|a, b| (a.1.end - end).abs().total_cmp(&(b.1.end - end).abs())).map(|(i, _)| i + 1)
    });
    anchor.unwrap_or_else(|| words.iter().position(|w| (w.start + w.end) / 2.0 >= end).unwrap_or(words.len()))
}

impl Agreement {

    pub fn pass(&mut self, words: &[Word], heard: f32) {
        let (from, was) = (after(words, self.settled_end, self.settled.last()), after(&self.last, self.settled_end, self.settled.last()));
        let mut k = 0;
        while from + k < words.len()
            && was + k < self.last.len()
            && key(&words[from + k].text) == key(&self.last[was + k].text)
            && words[from + k].end <= heard - LAG
        {
            k += 1;
        }
        if k > 0 {
            self.settled.extend(words[from..from + k].iter().map(|w| w.text.clone()));
            self.settled_end = words[from + k - 1].end;
        }
        self.tail = words[from + k..].iter().map(|w| w.text.clone()).collect();
        self.last = words.to_vec();
        self.passes += 1;
    }

    pub fn settle_all(&mut self, words: &[Word]) {
        let from = after(words, self.settled_end, self.settled.last());
        self.settled.extend(words[from..].iter().map(|w| w.text.clone()));
        if let Some(w) = words.last() {
            self.settled_end = self.settled_end.max(w.end);
        }
        self.tail.clear();
        self.last = words.to_vec();
        self.passes += 1;
    }

    pub fn settled(&self) -> &[String] {
        &self.settled
    }

    pub fn tail(&self) -> &[String] {
        &self.tail
    }

    pub fn has_pass(&self) -> bool {
        self.passes > 0
    }

    fn pass_len(&self) -> usize {
        self.last.len()
    }
}

pub struct Settled<R: Recognizer, D: FnMut(&[f32], bool) -> Result<Vec<Word>, String>> {
    live: R,
    decode: D,
    audio: Vec<f32>,
    since: usize,

    passed: f32,
    agreement: Agreement,

    off: bool,

    slowest: Option<std::time::Duration>,

    off_at: Option<std::time::Instant>,

    long: bool,

    ended: Option<String>,
}

impl<R: Recognizer, D: FnMut(&[f32], bool) -> Result<Vec<Word>, String>> Settled<R, D> {
    pub fn new(live: R, decode: D) -> Self {
        Settled { live, decode, audio: vec![], since: 0, passed: 0.0, agreement: Agreement::default(), off: false, slowest: None, off_at: None, long: false, ended: None }
    }

    fn run(&mut self, last: bool) -> Option<Vec<Word>> {
        if (self.off && !last) || self.audio.is_empty() {
            return None;
        }
        let t = std::time::Instant::now();
        let r = (self.decode)(&self.audio, last);
        self.slowest = self.slowest.max(Some(t.elapsed()));
        match r {
            Ok(words) => Some(words),
            Err(_) => {
                self.off = true;
                self.off_at = Some(std::time::Instant::now());
                None
            }
        }
    }

    fn shown(&mut self, live: &str) -> String {
        if self.off || !self.agreement.has_pass() {
            return live.to_string();
        }
        let mut words: Vec<String> = self.agreement.settled().iter().chain(self.agreement.tail()).cloned().collect();
        let beyond: Vec<String> = match self.live.heard_after(self.passed) {
            Some(after) => {
                let after: Vec<String> = after.split_whitespace().map(str::to_string).collect();
                let same = (0..=after.len().min(words.len())).rev().find(|&k| (0..k).all(|i| key(&words[words.len() - k + i]) == key(&after[i]))).unwrap_or(0);
                after[same..].to_vec()
            }
            None => live.split_whitespace().skip(self.agreement.pass_len()).map(str::to_string).collect(),
        };
        words.extend(beyond);
        Self::joined(&words)
    }

    fn joined(words: &[String]) -> String {
        words.join(" ")
    }

    fn keep(&mut self, samples: &[f32]) {
        if self.audio.len() + samples.len() > LONGEST {
            self.long = true;
        }
        if !self.long {
            self.audio.extend_from_slice(samples);
        }
    }

    fn last_pass(&mut self, live: String) -> String {
        if self.long {
            return live;
        }
        match self.run(true) {
            Some(words) if !words.is_empty() => {
                self.agreement = Agreement::default();
                self.agreement.settle_all(&words);
                Self::joined(self.agreement.settled())
            }
            _ => live,
        }
    }
}

impl<R: Recognizer, D: FnMut(&[f32], bool) -> Result<Vec<Word>, String>> Recognizer for Settled<R, D> {
    fn accept(&mut self, samples: &[f32]) {
        self.live.accept(samples);
        if self.ended.is_some() {
            return;
        }
        self.keep(samples);
        self.since += samples.len();
        if self.since >= SETTLE_EVERY && !self.long {
            self.since = 0;
            let heard = self.audio.len() as f32 / RATE as f32;
            self.passed = heard;
            if let Some(words) = self.run(false) {
                self.agreement.pass(&words, heard);
            }
        }
    }

    fn text(&mut self) -> String {
        if let Some(t) = &self.ended {
            return t.clone();
        }
        let live = self.live.text();
        self.shown(&live)
    }

    fn settled_words(&mut self) -> Option<usize> {
        if self.off {
            return None;
        }
        Some(match &self.ended {
            Some(t) => t.split_whitespace().count(),

            None => self.agreement.settled().iter().map(|w| w.split_whitespace().count()).sum(),
        })
    }

    fn endpoint(&mut self) -> bool {
        if self.ended.is_some() {
            return true;
        }
        if !self.live.endpoint() {
            return false;
        }

        let live = self.live.text();
        self.ended = Some(self.last_pass(live));
        true
    }

    fn reset(&mut self) {
        self.live.reset();
        self.audio.clear();
        self.since = 0;
        self.passed = 0.0;
        self.long = false;
        self.agreement = Agreement::default();
        self.ended = None;
    }

    fn finish(&mut self) -> String {
        if let Some(t) = self.ended.take() {
            return t;
        }
        let live = self.live.finish();
        self.last_pass(live)
    }

    fn accept_tail(&mut self, samples: &[f32]) {

        self.live.accept(samples);
        if self.ended.is_none() && !self.long {
            self.audio.extend_from_slice(samples);
        }
    }

    fn slowest_pass(&mut self) -> Option<std::time::Duration> {
        self.slowest
    }

    fn settling_off_at(&mut self) -> Option<std::time::Instant> {
        self.off_at
    }

    fn failed(&mut self) -> Option<String> {
        self.live.failed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn w(text: &str, start: f32, end: f32) -> Word {
        Word { text: text.into(), start, end }
    }

    fn pass(text: &str) -> Vec<Word> {
        text.split_whitespace().enumerate().map(|(i, t)| w(t, i as f32 * 0.5, i as f32 * 0.5 + 0.4)).collect()
    }

    #[test]
    fn a_word_settles_only_after_two_passes_agree_and_it_is_far_enough_behind() {
        let mut a = Agreement::default();
        a.pass(&pass("o gabinete do governo"), 4.0);
        assert!(a.settled().is_empty(), "one pass settles nothing");
        assert_eq!(a.tail(), ["o", "gabinete", "do", "governo"]);
        a.pass(&pass("o gabinete do governador afirmou"), 5.0);

        assert_eq!(a.settled(), ["o", "gabinete", "do"]);
        assert_eq!(a.tail(), ["governador", "afirmou"]);
    }

    #[test]
    fn a_pass_that_spells_the_settled_stretch_in_other_words_loses_nothing_after_it() {
        let mut a = Agreement::default();
        let first = [w("eram", 0.0, 0.3), w("vinte", 0.4, 0.7), w("e", 0.75, 0.8), w("três", 0.85, 1.2)];
        a.pass(&first, 3.0);
        a.pass(&first, 3.5);
        assert_eq!(a.settled(), ["eram", "vinte", "e", "três"]);

        let next = [w("eram", 0.0, 0.3), w("23", 0.4, 1.2), w("pessoas", 1.3, 1.8), w("feridas", 1.9, 2.4), w("hoje", 2.5, 3.1)];
        a.pass(&next, 4.0);
        assert_eq!(a.settled(), ["eram", "vinte", "e", "três"], "settled words never change");
        assert_eq!(a.tail(), ["pessoas", "feridas", "hoje"], "lined up after the settled stretch by time");
        a.pass(&next, 4.5);
        assert_eq!(a.settled(), ["eram", "vinte", "e", "três", "pessoas", "feridas"], "pessoas and feridas end by 3.0, hoje at 3.1");
        let mut b = Agreement::default();
        b.pass(&first, 3.0);
        b.pass(&first, 3.5);
        b.settle_all(&next);
        assert_eq!(b.settled(), ["eram", "vinte", "e", "três", "pessoas", "feridas", "hoje"], "and so does the last pass");
    }

    #[test]
    fn a_short_word_moving_a_frame_between_passes_is_neither_repeated_nor_lost() {
        let base = [w("o", 0.0, 0.2), w("gabinete", 0.3, 0.88), w("e", 0.88, 0.96)];
        let mut a = Agreement::default();
        a.pass(&base, 3.0);
        a.pass(&base, 3.5);
        assert_eq!(a.settled(), ["o", "gabinete", "e"]);

        let later = [w("o", 0.0, 0.2), w("gabinete", 0.3, 0.96), w("e", 0.96, 1.04), w("a", 1.1, 1.2), w("casa", 1.3, 1.7)];
        a.pass(&later, 4.0);
        assert_eq!(a.tail(), ["a", "casa"]);

        let mut b = Agreement::default();
        b.pass(&base, 3.0);
        b.pass(&base, 3.5);
        let earlier = [w("o", 0.0, 0.2), w("gabinete", 0.3, 0.8), w("e", 0.8, 0.88), w("a", 0.88, 0.96), w("casa", 1.0, 1.5)];
        b.pass(&earlier, 4.0);
        assert_eq!(b.tail(), ["a", "casa"]);
        b.settle_all(&earlier);
        assert_eq!(b.settled(), ["o", "gabinete", "e", "a", "casa"]);
    }

    #[test]
    fn a_recent_word_waits_for_the_lag_even_when_passes_agree() {
        let mut a = Agreement::default();
        a.pass(&pass("olá mundo"), 1.0);
        a.pass(&pass("olá mundo"), 1.2);

        assert!(a.settled().is_empty());
        a.pass(&pass("olá mundo"), 2.0);
        assert_eq!(a.settled(), ["olá"], "0.4 <= 0.5, 0.9 > 0.5");
    }

    #[test]
    fn punctuation_and_case_do_not_break_agreement() {
        let mut a = Agreement::default();
        a.pass(&pass("Olá, mundo"), 5.0);
        a.pass(&pass("olá mundo."), 5.0);
        assert_eq!(a.settled(), ["olá", "mundo."], "the latest pass's spelling is the one settled");
    }

    #[test]
    fn settled_words_never_change_or_shrink_whatever_later_passes_say() {
        let mut a = Agreement::default();
        let passes = ["a b c d", "a b c d e", "x y c d e f", "a", "", "a b q d e f g h"];
        let mut before: Vec<String> = vec![];
        for (i, p) in passes.iter().enumerate() {
            a.pass(&pass(p), 10.0 + i as f32);
            assert!(a.settled().starts_with(&before), "pass {i}: {:?} keeps {before:?}", a.settled());
            before = a.settled().to_vec();
        }
        assert_eq!(before, ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn the_tail_is_the_latest_pass_after_the_settled_words_replaced_whole() {
        let mut a = Agreement::default();
        a.pass(&pass("um dois três"), 1.0);
        a.pass(&pass("um dois tres quatro"), 1.0);
        assert!(a.settled().is_empty());
        assert_eq!(a.tail(), ["um", "dois", "tres", "quatro"]);
        a.pass(&pass("hum dois"), 1.0);
        assert_eq!(a.tail(), ["hum", "dois"], "a shorter pass shortens the tail");
    }

    #[test]
    fn settle_all_settles_the_final_pass_after_what_was_settled() {
        let mut a = Agreement::default();
        a.pass(&pass("o gabinete do"), 9.0);
        a.pass(&pass("o gabinete do governo"), 9.0);
        assert_eq!(a.settled(), ["o", "gabinete", "do"]);
        a.settle_all(&pass("O gabinete do governador afirmou"));
        assert_eq!(a.settled(), ["o", "gabinete", "do", "governador", "afirmou"]);
        assert!(a.tail().is_empty());
    }

    #[derive(Default, Clone)]
    struct Live {
        text: Rc<RefCell<String>>,
        end: Rc<RefCell<bool>>,
        finished: Rc<RefCell<u32>>,
    }
    impl Recognizer for Live {
        fn accept(&mut self, _: &[f32]) {}
        fn text(&mut self) -> String {
            self.text.borrow().clone()
        }
        fn endpoint(&mut self) -> bool {
            *self.end.borrow()
        }
        fn reset(&mut self) {
            *self.end.borrow_mut() = false;
            self.text.borrow_mut().clear();
        }
        fn finish(&mut self) -> String {
            *self.finished.borrow_mut() += 1;
            self.text.borrow().clone()
        }
    }

    struct Timed(Rc<RefCell<crate::nemo::Heard>>);
    impl Recognizer for Timed {
        fn accept(&mut self, _: &[f32]) {}
        fn text(&mut self) -> String {
            self.0.borrow().text.trim().to_string()
        }
        fn endpoint(&mut self) -> bool {
            false
        }
        fn reset(&mut self) {}
        fn finish(&mut self) -> String {
            self.text()
        }
        fn heard_after(&mut self, t: f32) -> Option<String> {
            Some(self.0.borrow().after(t))
        }
    }

    const DELTAS: [(&str, f32); 29] = [
        ("Un", 1.29), ("for", 1.45), ("tunate", 1.61), ("ly", 1.77), (",", 1.93), (" stud", 2.09), ("y", 2.25), ("ing", 2.41), (" traffic", 2.57), (" flo", 2.89),
        ("w", 3.05), (" is di", 3.21), ("fficult", 3.37), (" because", 3.53), (" dri", 3.85), ("ver", 4.01), (" beha", 4.33), ("vior", 4.49), (" can", 4.65), ("not be", 4.81),
        (" predic", 5.13), ("ted", 5.29), (" with", 5.45), (" one", 5.77), (" hundred", 5.93), (" perc", 6.09), ("ent", 6.25), (" cer", 6.57), ("tainty", 6.73),
    ];
    const PASSED: &str = "Unfortunately, studying traffic flow is difficult because driver behavior cannot be predicted with 100 % certainty.";

    #[test]
    fn the_words_heard_after_a_point_start_at_a_whole_word() {
        let mut h = crate::nemo::Heard::default();
        for (d, at) in DELTAS {
            h.push(d, Some(at));
        }
        assert_eq!(h.text, "Unfortunately, studying traffic flow is difficult because driver behavior cannot be predicted with one hundred percent certainty");
        assert_eq!(h.after(6.0), "percent certainty");
        assert_eq!(h.after(6.6), "certainty", "'tainty' is part of certainty");
        assert_eq!(h.after(7.0), "");
        h.next_segment(Some(8.0));
        h.push(" Then", Some(8.5));
        assert_eq!(h.after(0.4), "Then", "times from the segment's start");
        assert!(h.timed());
        h.push(" no stamp", None);
        assert!(!h.timed(), "a server without stamps: the settle step counts words instead");
    }

    #[test]
    fn live_words_past_a_normalized_pass_are_taken_by_time_not_by_count() {
        let heard = Rc::new(RefCell::new(crate::nemo::Heard::default()));
        for (d, at) in DELTAS {
            heard.borrow_mut().push(d, Some(at));
        }
        let mut s = Settled::new(Timed(Rc::clone(&heard)), |_: &[f32], _: bool| Ok(pass(PASSED)));
        for _ in 0..7 {
            s.accept(&[0.0; RATE]);
        }
        assert_eq!(s.text(), PASSED, "no live 'certainty' after the pass's own");
        heard.borrow_mut().push(" today", Some(7.4));
        assert_eq!(s.text(), format!("{PASSED} today"), "a word heard after the pass still shows");
    }

    fn scripted(passes: Vec<Result<&'static str, &'static str>>, lens: Rc<RefCell<Vec<usize>>>) -> impl FnMut(&[f32], bool) -> Result<Vec<Word>, String> {
        let mut i = 0;
        move |audio: &[f32], _last: bool| {
            lens.borrow_mut().push(audio.len());
            let r = passes[i.min(passes.len() - 1)];
            i += 1;
            r.map(pass).map_err(str::to_string)
        }
    }

    #[test]
    fn a_settle_pass_runs_every_second_over_the_whole_segment_and_the_live_words_fill_in_after_it() {
        let live = Live::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("o gabinete"), Ok("o gabinete do"), Ok("o gabinete do governo"), Ok("o gabinete do governo afirmou")], Rc::clone(&lens)));
        *live.text.borrow_mut() = "o gabi".into();
        s.accept(&[0.0; RATE / 2]);
        assert_eq!(s.text(), "o gabi", "before a pass the live words are the text");
        assert!(lens.borrow().is_empty());
        s.accept(&[0.0; RATE / 2]);
        assert_eq!(*lens.borrow(), [RATE], "one pass at 1 s of audio, over all of it");
        *live.text.borrow_mut() = "o gabinete do gover".into();
        assert_eq!(s.text(), "o gabinete do gover", "the pass's 2 words, then the live words past the pass's 2");
        *live.text.borrow_mut() = "o gabinet do gover".into();
        assert_eq!(s.text(), "o gabinete do gover", "the pass's spelling wins where it reaches");
        for _ in 0..3 {
            s.accept(&[0.0; RATE]);
        }
        assert_eq!(*lens.borrow(), [RATE, 2 * RATE, 3 * RATE, 4 * RATE]);
        assert_eq!(s.settled_words(), Some(4), "o gabinete do governo: each in two passes in a row and ending by 2.5 s");
        assert_eq!(s.text(), "o gabinete do governo afirmou");
    }

    #[test]
    fn the_endpoint_runs_a_final_pass_that_settles_the_segment_then_reset_starts_over() {
        let live = Live::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("o gabinete"), Ok("O gabinete do governador.")], Rc::clone(&lens)));
        s.accept(&[0.0; RATE]);
        *live.text.borrow_mut() = "o gabinete do governo".into();
        *live.end.borrow_mut() = true;
        s.accept(&[0.0; RATE / 4]);
        assert!(s.endpoint());
        assert_eq!(s.text(), "O gabinete do governador.");
        assert_eq!(s.settled_words(), Some(4));
        assert_eq!(lens.borrow().last(), Some(&(RATE + RATE / 4)));
        s.reset();
        assert!(!s.endpoint());
        assert_eq!(s.settled_words(), Some(0));
        assert_eq!(s.text(), "");
    }

    #[test]
    fn a_failing_pass_turns_settling_off_and_the_live_words_carry_on() {
        let live = Live::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Err("sidecar gone")], Rc::clone(&lens)));
        *live.text.borrow_mut() = "olá mundo".into();
        s.accept(&[0.0; RATE]);
        s.accept(&[0.0; RATE]);
        assert_eq!(lens.borrow().len(), 1, "no pass after a failure");
        assert_eq!(s.text(), "olá mundo");
        assert_eq!(s.failed(), None, "a failed pass is not the engine failing");
        assert_eq!(s.settled_words(), None, "the page goes back to its plain wake");
        assert_eq!(s.finish(), "olá mundo");
    }

    #[derive(Clone, Default)]
    struct Segmented {
        heard: Rc<RefCell<crate::nemo::Heard>>,
        end: Rc<RefCell<bool>>,
        completed: Rc<RefCell<Option<String>>>,
    }
    impl Recognizer for Segmented {
        fn accept(&mut self, _: &[f32]) {}
        fn text(&mut self) -> String {
            self.completed.borrow().clone().unwrap_or_else(|| self.heard.borrow().text.trim().to_string())
        }
        fn endpoint(&mut self) -> bool {
            if *self.end.borrow() && self.completed.borrow().is_none() {
                let said = self.heard.borrow().text.trim().to_string();
                *self.completed.borrow_mut() = Some(said);
                self.heard.borrow_mut().next_segment(None);
            }
            self.completed.borrow().is_some()
        }
        fn reset(&mut self) {
            *self.completed.borrow_mut() = None;
            *self.end.borrow_mut() = false;
        }
        fn finish(&mut self) -> String {
            let said = self.heard.borrow().text.trim().to_string();
            self.heard.borrow_mut().next_segment(None);
            said
        }
        fn heard_after(&mut self, t: f32) -> Option<String> {
            let h = self.heard.borrow();
            h.timed().then(|| h.after(t))
        }
    }

    fn hear(heard: &Rc<RefCell<crate::nemo::Heard>>, from: f32, to: f32) {
        for (d, at) in DELTAS.iter().filter(|(_, at)| *at > from && *at <= to) {
            heard.borrow_mut().push(d, Some(*at));
        }
    }
    const SAID: &str = "Unfortunately, studying traffic flow is difficult because driver behavior cannot be predicted with one hundred percent certainty";

    #[test]
    fn a_later_pass_that_fails_loses_no_live_word_in_the_segment_or_at_its_end() {
        let live = Segmented::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("Unfortunately,"), Ok("Unfortunately, studying"), Err("too slow")], Rc::clone(&lens)));
        for sec in 1..=7 {
            hear(&live.heard, sec as f32 - 1.0, sec as f32);
            s.accept(&[0.0; RATE]);
        }
        assert_eq!(lens.borrow().len(), 3, "two good passes, then the failing one; none after it");
        assert_eq!(s.text(), SAID, "settling is off: the live words, all of them");
        assert_eq!(s.settled_words(), None);
        *live.end.borrow_mut() = true;
        assert!(s.endpoint());
        assert_eq!(s.text(), SAID, "the segment closes on the server's own final");
    }

    #[test]
    fn a_failing_final_pass_falls_back_to_the_servers_own_final_at_the_endpoint_and_at_stop() {
        for at_stop in [false, true] {
            let live = Segmented::default();
            let mut s = Settled::new(live.clone(), |_: &[f32], last: bool| if last { Err("too slow".to_string()) } else { Ok(pass(PASSED)) });
            for sec in 1..=7 {
                hear(&live.heard, sec as f32 - 1.0, sec as f32);
                s.accept(&[0.0; RATE]);
            }
            assert_eq!(s.text(), PASSED);
            live.heard.borrow_mut().push(" today", Some(7.4));
            let said = format!("{SAID} today");
            if at_stop {
                assert_eq!(s.finish(), said, "Stop: every word heard");
            } else {
                *live.end.borrow_mut() = true;
                assert!(s.endpoint());
                assert_eq!(s.text(), said, "the endpoint: every word heard");
            }
        }
    }

    #[test]
    fn stop_runs_the_final_pass_only_not_one_in_between_for_the_tail() {
        let flags = Rc::new(RefCell::new(vec![]));
        let f = Rc::clone(&flags);
        let live = Live::default();
        *live.text.borrow_mut() = "o gabinete".into();
        let mut s = crate::dictation::Session::new(Settled::new(live.clone(), move |_: &[f32], last: bool| {
            f.borrow_mut().push(last);
            Ok(pass("o gabinete"))
        }));
        s.feed(&[0.0; RATE / 2]);
        s.stop();
        assert_eq!(*flags.borrow(), [true], "the stop's silence tail crosses a second of audio, but only the final pass runs");
    }

    #[test]
    fn the_slowest_settle_pass_is_kept_for_the_log() {
        let live = Live::default();
        let mut s = Settled::new(live.clone(), |_: &[f32], _: bool| Ok(pass("a")));
        assert_eq!(s.slowest_pass(), None, "no pass yet");
        s.accept(&[0.0; RATE]);
        assert!(s.slowest_pass().is_some());
        let mut session = crate::dictation::Session::new(s);
        session.feed(&[0.0; RATE]);
        assert!(session.timing().contains(", slowest settle pass "), "{}", session.timing());
    }

    #[test]
    fn a_final_pass_that_fails_falls_back_to_the_live_final_and_asks_with_the_last_flag() {
        let live = Live::default();
        let flags = Rc::new(RefCell::new(vec![]));
        let f = Rc::clone(&flags);
        let mut s = Settled::new(live.clone(), move |_: &[f32], last: bool| {
            f.borrow_mut().push(last);
            if last { Err("too slow".to_string()) } else { Ok(pass("o gabinete do governador")) }
        });
        *live.text.borrow_mut() = "o gabinete do governo".into();
        s.accept(&[0.0; RATE]);
        assert_eq!(s.text(), "o gabinete do governador");
        *live.end.borrow_mut() = true;
        s.accept(&[0.0; RATE / 4]);
        assert!(s.endpoint());
        assert_eq!(*flags.borrow(), [false, true], "the in-between pass, then the final one");
        assert_eq!(s.text(), "o gabinete do governo", "the server's own final: nothing after the last good pass is lost");
    }

    #[test]
    fn finish_settles_with_one_last_pass_over_the_whole_segment() {
        let live = Live::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("o gabi"), Ok("o gabinete afirmou")], Rc::clone(&lens)));
        s.accept(&[0.0; RATE]);
        s.accept(&[0.0; RATE / 2]);
        assert_eq!(s.finish(), "o gabinete afirmou");
        assert_eq!(*live.finished.borrow(), 1, "the live stream is finished too");
        assert_eq!(lens.borrow().last(), Some(&(RATE + RATE / 2)));
    }

    #[test]
    fn a_segment_past_the_longest_keeps_no_more_audio_and_ends_on_the_live_final() {

        let live = Live::default();
        let lens = Rc::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("a")], Rc::clone(&lens)));
        for _ in 0..(LONGEST / RATE + 3) {
            s.accept(&[0.0; RATE]);
        }
        assert_eq!(lens.borrow().len(), LONGEST / RATE);
        assert!(s.audio.len() <= LONGEST, "{} samples kept", s.audio.len());
        *live.text.borrow_mut() = "a b c".into();
        *live.end.borrow_mut() = true;
        assert!(s.endpoint());
        assert_eq!(lens.borrow().len(), LONGEST / RATE, "no final pass over a segment past LONGEST");
        assert_eq!(s.text(), "a b c");
        s.reset();
        s.accept(&[0.0; RATE]);
        assert_eq!(lens.borrow().len(), LONGEST / RATE + 1, "the next segment settles again");
    }

    #[test]
    fn stop_just_short_of_the_longest_still_runs_the_final_pass() {

        let flags = Rc::new(RefCell::new(vec![]));
        let f = Rc::clone(&flags);
        let live = Live::default();
        let mut s = crate::dictation::Session::new(Settled::new(live.clone(), move |_: &[f32], last: bool| {
            f.borrow_mut().push(last);
            Ok(pass("a b"))
        }));
        for _ in 0..59 {
            s.feed(&[0.0; RATE / 2]);
        }
        s.stop();
        assert_eq!(flags.borrow().last(), Some(&true), "the final pass ran: {:?}", flags.borrow());
    }

    #[test]
    fn the_settled_count_is_in_the_pages_words_when_a_pass_word_has_a_space() {

        let words = vec![w("cerca", 0.0, 0.4), w("de", 0.4, 0.6), w("100 %", 0.6, 1.2), w("das", 1.2, 1.5), w("pessoas", 1.5, 2.0)];
        let live = Live::default();
        let mut s = Settled::new(live.clone(), move |_: &[f32], _: bool| Ok(words.clone()));
        for _ in 0..4 {
            s.accept(&[0.0; RATE]);
        }
        assert_eq!(s.text(), "cerca de 100 % das pessoas");
        assert_eq!(s.settled_words(), Some(6), "every word settled, counted as the page counts them");
    }

    #[test]
    fn when_a_failed_pass_turned_settling_off_is_kept_for_the_log() {
        let live = Live::default();
        let mut s = Settled::new(live.clone(), scripted(vec![Ok("a"), Err("too slow")], Rc::default()));
        s.accept(&[0.0; RATE]);
        assert_eq!(s.settling_off_at(), None);
        s.accept(&[0.0; RATE]);
        assert!(s.settling_off_at().is_some());
        let mut session = crate::dictation::Session::new(s);
        session.feed(&[0.0; 160]);
        assert!(session.timing().contains(", settling off after a failed pass at "), "{}", session.timing());
    }
    #[test]
    fn endpoint_final_redecodes_early_words_instead_of_locking_partial_mistakes() {
        for stopped in [false, true] {
            let live = Live::default();
            let mut s = Settled::new(live.clone(), |_: &[f32], last: bool| {
                Ok(pass(if last { "Hello Unfold how is it going" } else { "Qual on Fold" }))
            });
            for _ in 0..5 { s.accept(&[0.01; RATE]); }
            assert!(s.text().starts_with("Qual"), "partial present before final");
            assert!(s.settled_words().unwrap() > 0, "the early mistake was committed");
            let final_text = if stopped { s.finish() } else {
                *live.end.borrow_mut() = true;
                assert!(s.endpoint());
                s.text()
            };
            assert_eq!(final_text, "Hello Unfold how is it going");
        }
    }

    #[test]
    fn endpoint_final_can_remove_a_hallucinated_partial_prefix() {
        let live = Live::default();
        let mut s = Settled::new(live, |_: &[f32], last: bool| {
            Ok(pass(if last { "thank you" } else { "we are really nice thank you" }))
        });
        for _ in 0..6 { s.accept(&[0.01; RATE]); }
        assert!(s.settled_words().unwrap() > 2);
        assert_eq!(s.finish(), "thank you");
    }

    #[test]
    fn endpoint_final_still_runs_after_a_partial_pass_timed_out() {
        for stopped in [false, true] {
            let live = Live::default();
            *live.text.borrow_mut() = "Hello on Fold it going".into();
            let calls = Rc::new(RefCell::new(Vec::new()));
            let c2 = calls.clone();
            let mut s = Settled::new(live.clone(), move |_: &[f32], last: bool| {
                c2.borrow_mut().push(last);
                if last { Ok(pass("Hello Unfold how is it going")) } else { Err("partial deadline".into()) }
            });
            s.accept(&[0.01; RATE]);
            assert_eq!(s.settled_words(), None, "a timed-out partial disables live passes");
            let final_text = if stopped { s.finish() } else {
                *live.end.borrow_mut() = true;
                assert!(s.endpoint()); s.text()
            };
            assert_eq!(final_text, "Hello Unfold how is it going");
            assert_eq!(*calls.borrow(), [false, true], "the endpoint has its own final-pass budget");
        }
    }

}
