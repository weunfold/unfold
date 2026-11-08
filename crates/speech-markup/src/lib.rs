

pub mod say_gate;

use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mood {
    Neutral,
    Calm,
    Joy,
    Serious,
    Question,
    Surprised,
    Tired,
    Tender,
}

impl Mood {
    pub fn parse(s: &str) -> Option<Mood> {
        Some(match s {
            "neutral" => Mood::Neutral,
            "calm" => Mood::Calm,
            "joy" => Mood::Joy,
            "serious" => Mood::Serious,
            "question" => Mood::Question,
            "surprised" => Mood::Surprised,
            "tired" => Mood::Tired,
            "tender" => Mood::Tender,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Event {
    Laugh,
    Giggle,
    Sigh,
    Gasp,
    Breath,
    Um,
    Pause,
}

impl Event {
    pub fn parse(s: &str) -> Option<Event> {
        Some(match s {
            "laugh" => Event::Laugh,
            "giggle" => Event::Giggle,
            "sigh" => Event::Sigh,
            "gasp" => Event::Gasp,
            "breath" => Event::Breath,
            "um" => Event::Um,
            "pause" => Event::Pause,
            _ => return None,
        })
    }

    pub fn text_form(self, lang: Lang) -> Option<&'static str> {
        match (self, lang) {
            (Event::Laugh, _) => Some("Haha,"),
            (Event::Giggle, Lang::Pt) => Some("Hihi,"),
            (Event::Giggle, Lang::En) => Some("Hehe,"),
            (Event::Sigh, Lang::Pt) => Some("Ufa."),
            (Event::Sigh, Lang::En) => Some("Phew."),
            (Event::Gasp, Lang::Pt) => Some("Nossa!"),
            (Event::Gasp, Lang::En) => Some("Oh!"),
            (Event::Um, _) => Some("Hm,"),
            (Event::Breath, _) | (Event::Pause, _) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Pt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    UnknownMood(String),
    UnknownEvent(String),
    MalformedMarker(String),
    MarkerNotOnFirstLine,
    MoodUnavailable { requested: Mood, used: Mood },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Event(Event),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sentence {
    pub mood: Mood,
    pub pieces: Vec<Piece>,
}

impl Sentence {
    pub fn text(&self) -> String {
        join(self.pieces.iter().filter_map(|p| match p {
            Piece::Text(t) => Some(t.as_str()),
            Piece::Event(_) => None,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {

    pub speak: bool,

    pub reply_mood: Mood,

    pub display: String,
    pub sentences: Vec<Sentence>,
    pub warnings: Vec<Warning>,
}

const OPEN: char = '⟨';
const CLOSE: char = '⟩';

pub fn parse(reply: &str) -> Parsed {
    let mut warnings = Vec::new();
    let (first, rest) = match reply.split_once('\n') {
        Some((a, b)) => (a, b),
        None => (reply, ""),
    };
    let mut speak = false;
    let mut reply_mood = Mood::Neutral;
    let body = match first.trim().strip_prefix(OPEN).and_then(|s| s.strip_suffix(CLOSE)) {
        Some(inner) if inner.starts_with("say:") => {
            match parse_say(inner, &mut warnings) {
                Some(m) => {
                    speak = true;
                    reply_mood = m;
                }
                None => warnings.push(Warning::MalformedMarker(first.trim().to_string())),
            }
            rest.to_string()
        }
        _ => reply.to_string(),
    };

    let mut mood = reply_mood;
    let mut sentences: Vec<Sentence> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut words = String::new();
    let mut chars = body.chars().peekable();
    let flush_words = |words: &mut String, pieces: &mut Vec<Piece>| {
        let t = words.trim();
        if !t.is_empty() {
            pieces.push(Piece::Text(t.to_string()));
        }
        words.clear();
    };
    let close_sentence = |pieces: &mut Vec<Piece>, mood: Mood, sentences: &mut Vec<Sentence>| {
        if !pieces.is_empty() {
            sentences.push(Sentence { mood, pieces: std::mem::take(pieces) });
        }
    };

    while let Some(c) = chars.next() {
        match c {
            OPEN => {
                let tag: String = chars.by_ref().take_while(|&x| x != CLOSE).collect();
                if tag.starts_with("say:") {
                    warnings.push(Warning::MarkerNotOnFirstLine);
                } else if let Some(m) = tag.strip_prefix("mood=") {
                    match Mood::parse(m.trim()) {
                        Some(m) => {
                            flush_words(&mut words, &mut pieces);

                            if pieces.is_empty() {
                                mood = m;
                            } else {
                                close_sentence(&mut pieces, mood, &mut sentences);
                                mood = m;
                            }
                        }
                        None => warnings.push(Warning::UnknownMood(m.trim().to_string())),
                    }
                } else {
                    warnings.push(Warning::MalformedMarker(format!("{OPEN}{tag}{CLOSE}")));
                }
            }
            '<' => {
                let mut tag = String::new();
                let mut closed = false;
                while let Some(&x) = chars.peek() {
                    if x == '>' {
                        chars.next();
                        closed = true;
                        break;
                    }
                    if !(x.is_ascii_lowercase() || x == '-') || tag.len() > 12 {
                        break;
                    }
                    tag.push(x);
                    chars.next();
                }
                if closed {
                    match Event::parse(&tag) {
                        Some(e) => {
                            flush_words(&mut words, &mut pieces);
                            pieces.push(Piece::Event(e));
                        }
                        None => warnings.push(Warning::UnknownEvent(tag)),
                    }
                } else {

                    words.push('<');
                    words.push_str(&tag);
                }
            }
            '(' if body_has_emphasis(&mut chars) => {

                let mut inner = String::new();
                while let Some(x) = chars.next() {
                    if x == ')' && chars.peek() == Some(&')') {
                        chars.next();
                        if chars.peek() == Some(&')') {
                            chars.next();
                        }
                        break;
                    }
                    inner.push(x);
                }
                words.push_str(&inner);
            }
            '.' | '!' | '?' | '…' => {
                words.push(c);
                while let Some(&x) = chars.peek() {
                    if matches!(x, '.' | '!' | '?' | '…' | '"' | '\'' | ')') {
                        words.push(x);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let next_is_space = chars.peek().is_none_or(|x| x.is_whitespace());
                if next_is_space && !ends_with_abbreviation(&words) {
                    flush_words(&mut words, &mut pieces);
                    let question = pieces.iter().rev().find_map(|p| match p {
                        Piece::Text(t) => Some(t.ends_with('?')),
                        Piece::Event(_) => None,
                    });
                    let m = if question == Some(true) && mood != Mood::Question { Mood::Question } else { mood };
                    close_sentence(&mut pieces, m, &mut sentences);
                }
            }
            '\n' => {
                flush_words(&mut words, &mut pieces);
                close_sentence(&mut pieces, mood, &mut sentences);
            }
            _ => words.push(c),
        }
    }
    flush_words(&mut words, &mut pieces);
    close_sentence(&mut pieces, mood, &mut sentences);
    sentences.retain(|s| !s.pieces.is_empty());

    let display = sentences.iter().map(Sentence::text).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
    Parsed { speak, reply_mood, display, sentences, warnings }
}

fn parse_say(inner: &str, warnings: &mut Vec<Warning>) -> Option<Mood> {
    let mut parts = inner.split_whitespace();
    if parts.next()? != "say:speech" {
        return None;
    }
    let mut mood = Mood::Neutral;
    for p in parts {
        match p.strip_prefix("mood=") {
            Some(m) => match Mood::parse(m) {
                Some(m) => mood = m,
                None => warnings.push(Warning::UnknownMood(m.to_string())),
            },
            None => return None,
        }
    }
    Some(mood)
}

fn body_has_emphasis(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let mut look = chars.clone();
    if look.next() == Some('(') && look.next() == Some('(') {
        chars.next();
        chars.next();
        true
    } else {
        false
    }
}

fn ends_with_abbreviation(words: &str) -> bool {
    const ABBR: [&str; 10] = ["Sr.", "Sra.", "Dr.", "Dra.", "Mr.", "Mrs.", "Ms.", "etc.", "p.ex.", "e.g."];
    let last = words.split_whitespace().last().unwrap_or("");
    ABBR.contains(&last) || (last.len() == 2 && last.ends_with('.') && last.chars().next().is_some_and(char::is_uppercase))
}

fn join<'a>(it: impl Iterator<Item = &'a str>) -> String {
    it.collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
pub struct Voice {
    pub lang: Lang,
    pub moods: BTreeSet<Mood>,
    pub clips: BTreeSet<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {

    Speak { text: String, mood: Mood, requested: Mood },

    Clip(Event),
    Silence { ms: u32 },
}

pub fn resolve_mood(requested: Mood, voice: &Voice) -> Mood {
    if voice.moods.contains(&requested) {
        return requested;
    }
    for m in [Mood::Neutral, Mood::Calm] {
        if voice.moods.contains(&m) {
            return m;
        }
    }
    voice.moods.iter().next().copied().unwrap_or(Mood::Neutral)
}

pub fn plan(parsed: &Parsed, voice: &Voice) -> (Vec<Step>, Vec<Warning>) {
    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    if !parsed.speak {
        return (steps, warnings);
    }
    for s in &parsed.sentences {
        let mood = resolve_mood(s.mood, voice);
        if mood != s.mood && !warnings.contains(&Warning::MoodUnavailable { requested: s.mood, used: mood }) {
            warnings.push(Warning::MoodUnavailable { requested: s.mood, used: mood });
        }
        let mut buf = String::new();
        let flush = |buf: &mut String, steps: &mut Vec<Step>| {
            if !buf.trim().is_empty() {
                steps.push(Step::Speak { text: buf.trim().to_string(), mood, requested: s.mood });
            }
            buf.clear();
        };
        for p in &s.pieces {
            match p {
                Piece::Text(t) => {
                    if !buf.is_empty() {
                        buf.push(' ');
                    }
                    buf.push_str(t);
                }
                Piece::Event(e) if voice.clips.contains(e) => {
                    flush(&mut buf, &mut steps);
                    steps.push(Step::Clip(*e));
                }
                Piece::Event(Event::Pause) => {
                    flush(&mut buf, &mut steps);
                    steps.push(Step::Silence { ms: 300 });
                }
                Piece::Event(e) => {
                    if let Some(t) = e.text_form(voice.lang) {
                        if !buf.is_empty() {
                            buf.push(' ');
                        }
                        buf.push_str(t);
                    }
                }
            }
        }
        flush(&mut buf, &mut steps);
    }
    (steps, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(moods: &[Mood], clips: &[Event]) -> Voice {
        Voice { lang: Lang::Pt, moods: moods.iter().copied().collect(), clips: clips.iter().copied().collect() }
    }
    fn speak_texts(steps: &[Step]) -> Vec<(String, Mood)> {
        steps.iter().filter_map(|s| match s { Step::Speak { text, mood, .. } => Some((text.clone(), *mood)), _ => None }).collect()
    }

    #[test]
    fn no_marker_is_text_only() {
        let p = parse("Bom dia! Tudo bem?");
        assert!(!p.speak);
        assert_eq!(p.display, "Bom dia! Tudo bem?");
        assert!(plan(&p, &voice(&[Mood::Neutral], &[])).0.is_empty());
    }

    #[test]
    fn bare_marker_is_neutral_and_stripped() {
        let p = parse("⟨say:speech⟩\nOi! Eu sou o Unfold.");
        assert!(p.speak);
        assert_eq!(p.reply_mood, Mood::Neutral);
        assert_eq!(p.display, "Oi! Eu sou o Unfold.");
        assert_eq!(p.sentences.len(), 2);
    }

    #[test]
    fn marker_mood_and_inline_switch() {
        let p = parse("⟨say:speech mood=joy⟩\nBom dia! ⟨mood=calm⟩Hoje é dia de descanso.");
        assert_eq!(p.sentences[0].mood, Mood::Joy);
        assert_eq!(p.sentences[1].mood, Mood::Calm);
        assert_eq!(p.display, "Bom dia! Hoje é dia de descanso.");
    }

    #[test]
    fn question_sentence_uses_question_mood() {
        let p = parse("⟨say:speech mood=calm⟩\nEntendi. Você quer a versão curta?");
        assert_eq!(p.sentences[0].mood, Mood::Calm);
        assert_eq!(p.sentences[1].mood, Mood::Question);
    }

    #[test]
    fn events_are_stripped_from_display() {
        let p = parse("⟨say:speech mood=joy⟩\n<laugh> Essa foi boa! <sigh> Que dia.");
        assert_eq!(p.display, "Essa foi boa! Que dia.");
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn event_uses_clip_when_the_voice_has_one_else_text_form() {
        let p = parse("⟨say:speech mood=joy⟩\n<laugh> Essa foi boa!");
        let with_clip = plan(&p, &voice(&[Mood::Joy], &[Event::Laugh])).0;
        assert_eq!(with_clip[0], Step::Clip(Event::Laugh));
        assert_eq!(speak_texts(&with_clip), vec![("Essa foi boa!".to_string(), Mood::Joy)]);
        let without = plan(&p, &voice(&[Mood::Joy], &[])).0;
        assert_eq!(speak_texts(&without), vec![("Haha, Essa foi boa!".to_string(), Mood::Joy)]);
    }

    #[test]
    fn um_is_always_text_and_pause_is_silence() {
        let p = parse("⟨say:speech mood=calm⟩\n<um> Deixa eu pensar. <pause> Acho que dá.");
        let steps = plan(&p, &voice(&[Mood::Calm], &[Event::Um])).0;
        assert_eq!(steps[0], Step::Clip(Event::Um));
        assert!(steps.contains(&Step::Silence { ms: 300 }));
        let steps = plan(&p, &voice(&[Mood::Calm], &[])).0;
        assert_eq!(speak_texts(&steps)[0].0, "Hm, Deixa eu pensar.");
    }

    #[test]
    fn unavailable_mood_falls_back_and_warns() {
        let p = parse("⟨say:speech mood=serious⟩\nNão dá para fazer isso.");
        let (steps, w) = plan(&p, &voice(&[Mood::Calm, Mood::Joy], &[]));
        assert_eq!(speak_texts(&steps)[0].1, Mood::Calm);
        assert_eq!(w, vec![Warning::MoodUnavailable { requested: Mood::Serious, used: Mood::Calm }]);
    }

    #[test]
    fn unknown_markup_never_fails_and_never_leaks() {
        let p = parse("⟨say:speech mood=ecstatic⟩\n<dance> Oi! ⟨volume=loud⟩Tudo bem?");
        assert!(p.speak);
        assert_eq!(p.reply_mood, Mood::Neutral);
        assert_eq!(p.display, "Oi! Tudo bem?");
        assert!(p.warnings.contains(&Warning::UnknownMood("ecstatic".into())));
        assert!(p.warnings.contains(&Warning::UnknownEvent("dance".into())));
        assert!(p.warnings.iter().any(|w| matches!(w, Warning::MalformedMarker(_))));
    }

    #[test]
    fn marker_not_on_first_line_is_ignored() {
        let p = parse("Oi!\n⟨say:speech⟩ Tudo bem?");
        assert!(!p.speak);
        assert_eq!(p.display, "Oi! Tudo bem?");
        assert!(p.warnings.contains(&Warning::MarkerNotOnFirstLine));
    }

    #[test]
    fn less_than_in_text_is_kept() {
        let p = parse("⟨say:speech⟩\nSe x < 5, para.");
        assert_eq!(p.display, "Se x < 5, para.");
    }

    #[test]
    fn emphasis_keeps_the_word() {
        let p = parse("⟨say:speech⟩\nVocê dormiu (((melhor))) que ontem.");
        assert_eq!(p.display, "Você dormiu melhor que ontem.");
    }

    #[test]
    fn abbreviations_do_not_split_sentences() {
        let p = parse("⟨say:speech⟩\nO Dr. Silva chega às 15h. Tudo certo.");
        assert_eq!(p.sentences.len(), 2);
        assert_eq!(p.sentences[0].text(), "O Dr. Silva chega às 15h.");
    }

    #[test]
    fn decimals_do_not_split_sentences() {
        let p = parse("⟨say:speech⟩\nA versão 0.4 saiu. Deu R$ 42,50.");
        assert_eq!(p.sentences.len(), 2);
    }
}
