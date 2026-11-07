

use crate::Parsed;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    Dictated,
    Typed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Override {
    Speak,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presence {

    pub voice_mode: bool,

    pub muted: bool,
    pub override_: Option<Override>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    Code,
    UrlOrPath,
    Table,

    List,

    Numbers,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Say {

    Text(TextWhy),

    Full,

    FirstSentence(String),

    Summary(Vec<Trigger>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWhy {
    Muted,
    OverrideText,
    VoiceModeOff,
    Typed,
    Empty,
}

pub const LONG_WORDS: usize = 60;
const LIST_MAX: usize = 3;
const NUMBERS_MIN: usize = 6;

pub fn decide(parsed: &Parsed, modality: Modality, presence: Presence) -> Say {
    if presence.muted {
        return Say::Text(TextWhy::Muted);
    }
    match presence.override_ {
        Some(Override::Text) => return Say::Text(TextWhy::OverrideText),
        Some(Override::Speak) => {}
        None if !presence.voice_mode => return Say::Text(TextWhy::VoiceModeOff),
        None if modality == Modality::Typed => return Say::Text(TextWhy::Typed),
        None => {}
    }
    if parsed.display.trim().is_empty() {
        return Say::Text(TextWhy::Empty);
    }
    match content(&parsed.display) {
        Content::Visual(t) => Say::Summary(t),
        Content::Long => Say::FirstSentence(parsed.sentences.first().map(|s| s.text()).unwrap_or_default()),
        Content::Short => Say::Full,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Short,
    Long,
    Visual(Vec<Trigger>),
}

pub fn content(text: &str) -> Content {
    let mut t = Vec::new();
    if has_code(text) {
        t.push(Trigger::Code);
    }
    if has_url_or_path(text) {
        t.push(Trigger::UrlOrPath);
    }
    if text.lines().any(|l| l.matches('|').count() >= 2) {
        t.push(Trigger::Table);
    }
    if text.lines().filter(|l| is_list_item(l)).count() > LIST_MAX {
        t.push(Trigger::List);
    }
    if numeric_tokens(text) >= NUMBERS_MIN {
        t.push(Trigger::Numbers);
    }
    if !t.is_empty() {
        Content::Visual(t)
    } else if word_count(text) > LONG_WORDS {
        Content::Long
    } else {
        Content::Short
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn has_code(text: &str) -> bool {
    if text.contains("```") {
        return true;
    }

    if text.lines().any(|l| l.split('`').count() >= 3 && l.split('`').skip(1).step_by(2).any(|s| !s.is_empty()) && l.matches('`').count() >= 2) {
        return true;
    }
    text.lines().any(|l| {
        let rest = l.strip_prefix("    ").or_else(|| l.strip_prefix('\t'));
        rest.is_some_and(|r| r.chars().next().is_some_and(|c| !c.is_whitespace()))
    })
}

fn has_url_or_path(text: &str) -> bool {
    if ["http://", "https://", "www.", "~/"].iter().any(|p| text.contains(p)) {
        return true;
    }
    let c: Vec<char> = text.chars().collect();
    let seg = |c: char| is_word(c) || c == '.' || c == '-';
    for i in 0..c.len() {

        if c[i] == '/' && (i == 0 || !(is_word(c[i - 1]) || c[i - 1] == '.')) {
            let mut j = i + 1;
            let a = j;
            while j < c.len() && seg(c[j]) {
                j += 1;
            }
            if j > a && j < c.len() && c[j] == '/' && j + 1 < c.len() && seg(c[j + 1]) {
                return true;
            }
        }

        if c[i].is_ascii_uppercase() && i + 2 < c.len() && c[i + 1] == ':' && c[i + 2] == '\\' {
            return true;
        }
    }
    false
}

fn is_list_item(line: &str) -> bool {
    let l = line.trim_start();
    let rest = if let Some(r) = l.strip_prefix(['-', '*', '•']) {
        r
    } else {
        let d = l.chars().take_while(|c| c.is_ascii_digit()).count();
        match l[d..].chars().next() {
            Some('.' | ')') if d > 0 => &l[d + 1..],
            _ => return false,
        }
    };
    let ws = rest.chars().take_while(|c| c.is_whitespace()).count();
    ws > 0 && rest.chars().nth(ws).is_some()
}

fn numeric_tokens(text: &str) -> usize {
    let c: Vec<char> = text.chars().collect();
    let (mut n, mut i) = (0, 0);
    while i < c.len() {
        if c[i].is_ascii_digit() && (i == 0 || !is_word(c[i - 1])) {
            n += 1;
            while i < c.len() && (c[i].is_ascii_digit() || ".,:/%".contains(c[i])) {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    n
}

pub fn word_count(text: &str) -> usize {
    let w = |c: char| is_word(c) || c == '\'' || c == '’' || c == '-';
    text.split(|c: char| !w(c)).filter(|s| !s.is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    const ON: Presence = Presence { voice_mode: true, muted: false, override_: None };

    #[test]
    fn modality_decides_by_default() {
        let p = parse("Sure, happy to help.");
        assert_eq!(decide(&p, Modality::Dictated, ON), Say::Full);
        assert_eq!(decide(&p, Modality::Typed, ON), Say::Text(TextWhy::Typed));
    }

    #[test]
    fn mute_wins_over_override_and_override_wins_over_modality() {
        let p = parse("Sure.");
        let speak = Presence { override_: Some(Override::Speak), ..ON };
        assert_eq!(decide(&p, Modality::Typed, speak), Say::Full);
        assert_eq!(decide(&p, Modality::Typed, Presence { muted: true, ..speak }), Say::Text(TextWhy::Muted));
        let text = Presence { override_: Some(Override::Text), ..ON };
        assert_eq!(decide(&p, Modality::Dictated, text), Say::Text(TextWhy::OverrideText));
        assert_eq!(decide(&p, Modality::Dictated, Presence { voice_mode: false, ..ON }), Say::Text(TextWhy::VoiceModeOff));
    }

    #[test]
    fn the_marker_never_decides_whether() {
        let p = parse("⟨say:speech mood=joy⟩\nOi! Que bom.");
        assert_eq!(decide(&p, Modality::Typed, ON), Say::Text(TextWhy::Typed));
        let p = parse("Oi! Que bom.");
        assert_eq!(decide(&p, Modality::Dictated, ON), Say::Full);
    }

    #[test]
    fn triggers() {
        assert_eq!(content("Run `cargo test` first."), Content::Visual(vec![Trigger::Code]));
        assert_eq!(content("```\nls\n```"), Content::Visual(vec![Trigger::Code]));
        assert_eq!(content("See https://example.org now."), Content::Visual(vec![Trigger::UrlOrPath]));
        assert_eq!(content("It's in ~/notes."), Content::Visual(vec![Trigger::UrlOrPath]));
        assert_eq!(content("Open /etc/hosts."), Content::Visual(vec![Trigger::UrlOrPath]));
        assert_eq!(content("Open C:\\Users."), Content::Visual(vec![Trigger::UrlOrPath]));
        assert_eq!(content("| a | b |\n|---|---|"), Content::Visual(vec![Trigger::Table]));
        assert_eq!(content("- a\n- b\n- c\n- d"), Content::Visual(vec![Trigger::List]));
        assert_eq!(content("1. a\n2) b\n• c\n* d"), Content::Visual(vec![Trigger::List]));
        assert_eq!(content("Prices: 1, 2, 3.5, 40%, 10:30, 7/8."), Content::Visual(vec![Trigger::Numbers]));
    }

    #[test]
    fn near_triggers_stay_speakable() {
        assert_eq!(content("- a\n- b\n- c"), Content::Short);
        assert_eq!(content("and/or, either/or, 24/7 support"), Content::Short);
        assert_eq!(content("Room 12, floor 3, 5 people."), Content::Short);
        assert_eq!(content("A-1 B2 c3d4"), Content::Short);
        assert_eq!(content("It's a 'quote' and a pipe | alone."), Content::Short);
    }

    #[test]
    fn length_tier_speaks_the_first_sentence() {
        let long = format!("Primeira frase aqui. {}", "palavra ".repeat(LONG_WORDS));
        let p = parse(&long);
        match decide(&p, Modality::Dictated, ON) {
            Say::FirstSentence(s) => assert!(s.starts_with("Primeira frase aqui."), "{s}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(content(&"w ".repeat(LONG_WORDS)), Content::Short);
        assert_eq!(content(&"w ".repeat(LONG_WORDS + 1)), Content::Long);
    }

    #[test]
    fn empty_reply_is_text() {
        assert_eq!(decide(&parse("⟨say:speech⟩\n"), Modality::Dictated, ON), Say::Text(TextWhy::Empty));
    }
}
