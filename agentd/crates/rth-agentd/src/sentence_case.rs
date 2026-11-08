

use crate::local::Lang;

const ENDS: &[char] = &['.', '!', '?', '…'];

const MAX_HOLD: usize = 256;

#[cfg_attr(not(test), allow(dead_code))]
pub fn apply(text: &str, lang: Option<Lang>) -> String {
    if text.chars().any(char::is_uppercase) {
        return text.to_string();
    }
    let mut s = SentenceCase::new(lang);
    let mut out = s.push(text);
    out.push_str(&s.finish());
    out
}

pub struct SentenceCase {
    english: bool,

    off: bool,

    start: bool,

    in_word: bool,

    holding: bool,
    held: String,

    last: Option<char>,

    ticks: usize,

    fence: Option<usize>,
    raw: String,
    changed: bool,
}

impl SentenceCase {
    pub fn new(lang: Option<Lang>) -> Self {
        SentenceCase { english: lang == Some(Lang::En), off: false, start: true, in_word: false, holding: false, held: String::new(), last: None, ticks: 0, fence: None, raw: String::new(), changed: false }
    }

    pub fn push(&mut self, piece: &str) -> String {
        self.raw.push_str(piece);
        let mut out = String::new();
        for c in piece.chars() {
            if !self.off && c.is_uppercase() {
                self.off = true;
                out.push_str(&std::mem::take(&mut self.held));
            }
            if self.off {
                out.push(c);
            } else {
                self.step(c, &mut out);
            }
        }
        out
    }

    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        if !self.off {
            self.end_word(&mut out, false);
        }
        out
    }

    pub fn original(self) -> Option<String> {
        self.changed.then_some(self.raw)
    }

    fn step(&mut self, c: char, out: &mut String) {
        if c == '`' {

            self.end_word(out, false);
            self.start = false;
            self.ticks += 1;
            out.push(c);
            return;
        }
        if self.ticks > 0 {
            self.fence = match self.fence {
                None => Some(self.ticks),
                Some(n) if n == self.ticks => None,
                open => open,
            };
            self.ticks = 0;
        }
        if self.fence.is_some() {
            out.push(c);
            return;
        }
        if c.is_whitespace() {
            self.end_word(out, true);
            out.push(c);
            return;
        }
        if !self.in_word {
            self.in_word = true;
            self.holding = self.start || self.english;
        }
        self.last = Some(c);
        if !self.holding {
            out.push(c);
            return;
        }
        self.held.push(c);
        if self.held.len() > MAX_HOLD {
            out.push_str(&std::mem::take(&mut self.held));
            (self.holding, self.start) = (false, false);
        } else if !self.start && !maybe_i(&self.held) {
            out.push_str(&std::mem::take(&mut self.held));
            self.holding = false;
        }
    }

    fn end_word(&mut self, out: &mut String, at_space: bool) {
        if !self.in_word {
            return;
        }
        let word = std::mem::take(&mut self.held);
        if !word.is_empty() {
            let cased = self.case_word(&word);
            self.changed |= cased != word;
            out.push_str(&cased);
        }
        if at_space && self.last.is_some_and(|c| ENDS.contains(&c)) {
            self.start = true;
        }
        (self.in_word, self.holding, self.last) = (false, false, None);
    }

    fn case_word(&mut self, word: &str) -> String {
        if is_span(word) {
            self.start = false;
            return word.to_string();
        }
        let Some(at) = word.find(char::is_alphanumeric) else { return word.to_string() };
        let cap = self.start || (self.english && is_i(word));
        self.start = false;
        let first = word[at..].chars().next().expect("an alphanumeric char at `at`");
        if !cap {
            return word.to_string();
        }
        format!("{}{}{}", &word[..at], first.to_uppercase(), &word[at + first.len_utf8()..])
    }
}

fn is_span(word: &str) -> bool {
    word.contains(['/', '\\', '@']) || word.trim_start_matches(|c: char| !c.is_alphanumeric()).starts_with("www.")
}

fn is_i(word: &str) -> bool {
    let core = word.trim_matches(|c: char| !c.is_alphanumeric()).replace('’', "'");
    matches!(core.as_str(), "i" | "i'm" | "i've" | "i'll" | "i'd")
}

fn maybe_i(prefix: &str) -> bool {
    let core = prefix.trim_start_matches(|c: char| !c.is_alphanumeric());
    core.is_empty() || (core.starts_with('i') && core.len() <= 12 && !core.contains(['/', '\\', '@']) && core.chars().all(|c| !c.is_alphanumeric() || "imveld".contains(c)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(pieces: &[&str], lang: Option<Lang>) -> (String, Option<String>) {
        let mut s = SentenceCase::new(lang);
        let mut out: String = pieces.iter().map(|p| s.push(p)).collect();
        out.push_str(&s.finish());
        (out, s.original())
    }

    const EN: Option<Lang> = Some(Lang::En);
    const PT: Option<Lang> = Some(Lang::Pt);

    #[test]
    fn the_granite_pattern_gets_sentence_starts_and_i() {
        let raw = "hi! i'm unfold, a local assistant. i can help with notes, reminders and questions.\n\nwhat would you like to do? i'll wait… or not.";
        assert_eq!(apply(raw, EN), "Hi! I'm unfold, a local assistant. I can help with notes, reminders and questions.\n\nWhat would you like to do? I'll wait… Or not.");
    }

    #[test]
    fn a_normal_or_mixed_reply_is_untouched_whole() {
        let normal = "Hi! I'm Unfold. How can I help?";
        assert_eq!(apply(normal, EN), normal);
        let mixed = "sure. here is the plan for Friday. i think it works.";
        assert_eq!(apply(mixed, EN), mixed, "one uppercase letter anywhere: nothing changes");
        assert_eq!(stream(&["Hi!", " i", " am", " here."], EN), ("Hi! i am here.".to_string(), None));
    }

    #[test]
    fn code_urls_emails_and_paths_are_skipped() {
        assert_eq!(apply("run `ls -la. then` again. ok.", EN), "Run `ls -la. then` again. Ok.");
        assert_eq!(apply("`npm` is a tool. try it.", EN), "`npm` is a tool. Try it.", "a start inside code is used up");
        assert_eq!(apply("like this:\n```\nfn main() { println!(\"hi. there\"); }\n```\nthat's it. bye.", EN), "Like this:\n```\nfn main() { println!(\"hi. there\"); }\n```\nthat's it. Bye.");
        assert_eq!(apply("https://example.com/a. is the page. www.example.com works too.", EN), "https://example.com/a. Is the page. www.example.com works too.", "the start the URL uses up is its own; the sentence end after it counts");
        assert_eq!(apply("see http://x.org/i/2. ok", EN), "See http://x.org/i/2. Ok");
        assert_eq!(apply("mail me@example.com. thanks.", EN), "Mail me@example.com. Thanks.");
        assert_eq!(apply("bob.smith@example.com is mine. ok.", EN), "bob.smith@example.com is mine. Ok.");
        assert_eq!(apply("open ~/notes/todo.md. then src/main.rs. c:\\users\\me is windows. i/o too.", EN), "Open ~/notes/todo.md. Then src/main.rs. c:\\users\\me is windows. i/o too.");
    }

    #[test]
    fn the_i_rule_is_english_only() {
        let raw = "yes, i think i'd go. i've said i'll try, and i did. in time it is.";
        assert_eq!(apply(raw, EN), "Yes, I think I'd go. I've said I'll try, and I did. In time it is.");
        assert_eq!(apply(raw, None), "Yes, i think i'd go. I've said i'll try, and i did. In time it is.", "unknown language: starts only");
        assert_eq!(apply("so (i.e., that) (i) and \"i\" and i.", EN), "So (i.e., that) (I) and \"I\" and I.");
        assert_eq!(apply("i.e. this", EN), "I.e. This", "the plain rule: an abbreviation's stop before a space ends a sentence");
        assert_eq!(apply("so in it id ill idle.", EN), "So in it id ill idle.");
    }

    #[test]
    fn portuguese_gets_sentence_starts_and_no_i_rule() {
        assert_eq!(apply("olá! é bom te ver. o item i está aqui? sim.", PT), "Olá! É bom te ver. O item i está aqui? Sim.");
    }

    #[test]
    fn state_carries_across_pieces_split_mid_word_and_mid_span() {
        let pieces = ["he", "llo. i", "'", "m he", "re. see `co", "de. x", "` and htt", "ps://a.b/c. ", "ok", " i", "n time."];
        let (out, original) = stream(&pieces, EN);
        assert_eq!(out, "Hello. I'm here. See `code. x` and https://a.b/c. Ok in time.");
        assert_eq!(original.as_deref(), Some(pieces.concat().as_str()), "the original is the raw text");
        let mut s = SentenceCase::new(EN);
        assert_eq!(s.push("ok. h"), "Ok. ", "a sentence-start word is held until it ends");
        assert_eq!(s.push("ttp://x.y"), "");
        assert_eq!(s.push(" z"), "http://x.y z");
        assert_eq!(s.push(" i"), " ", "an English `i` is held until the next character");
        assert_eq!(s.push("n"), "in");
        assert_eq!(s.finish(), "");
    }

    #[test]
    fn streaming_equals_the_whole_text_for_any_split() {
        let raw = "hi! i'm here. try `cargo test`. see https://x.dev/i. mail a@b.co. i’ll go… “quoted.” 3 cats. 👋 bye.";
        let whole = apply(raw, EN);
        let chars: Vec<char> = raw.chars().collect();
        for size in 1..=9 {
            let pieces: Vec<String> = chars.chunks(size).map(|c| c.iter().collect()).collect();
            let refs: Vec<&str> = pieces.iter().map(String::as_str).collect();
            let (out, original) = stream(&refs, EN);
            assert_eq!(out, whole, "pieces of {size} chars");
            assert_eq!(original.as_deref(), Some(raw));
        }
    }

    #[test]
    fn curly_apostrophes_count() {
        assert_eq!(apply("ok, i’m in. i’ve got it, i’ll go, i’d say.", EN), "Ok, I’m in. I’ve got it, I’ll go, I’d say.");
    }

    #[test]
    fn emoji_quotes_and_digits_at_a_start() {
        assert_eq!(apply("👋 hi there. 🎉 nice!", EN), "👋 Hi there. 🎉 Nice!");
        assert_eq!(apply("\"hello,\" she said. 'yes.' (maybe) ok.", EN), "\"Hello,\" she said. 'Yes.' (maybe) ok.", "a quote after the stop: not a sentence end");
        assert_eq!(apply("“hello.” ‘yes’ ok.", EN), "“Hello.” ‘yes’ ok.");
        assert_eq!(apply("3 apples. 2nd try. 1. buy milk", EN), "3 apples. 2nd try. 1. Buy milk");
        assert_eq!(apply("- item one. - item two", EN), "- Item one. - Item two");
    }

    #[test]
    fn the_stream_stops_at_the_first_uppercase_letter() {

        let (out, original) = stream(&["hi. ok", " then. Here", " it is. bye."], EN);
        assert_eq!(out, "Hi. Ok then. Here it is. bye.");
        assert_eq!(original.as_deref(), Some("hi. ok then. Here it is. bye."));
        assert_eq!(stream(&["i", "Phone. ok."], EN).0, "iPhone. ok.", "a held word with a capital is shown as typed");
    }

    #[test]
    fn nothing_to_change_is_not_flagged() {
        assert_eq!(stream(&["123 ", "`x`", " 👍"], EN), ("123 `x` 👍".to_string(), None));
        assert_eq!(stream(&[], EN), (String::new(), None));
        assert_eq!(stream(&[" ", "\n"], EN), (" \n".to_string(), None));
    }
}
