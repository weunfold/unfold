

use speech_markup::Mood;
use unfold_speech::catalog::mood_name;

const LINES: &str = include_str!("../data/clone_lines_v1.json");
const MOODS: [Mood; 8] = [Mood::Neutral, Mood::Calm, Mood::Joy, Mood::Serious, Mood::Question, Mood::Surprised, Mood::Tired, Mood::Tender];

#[test]
fn every_mood_has_a_line_in_each_language_of_a_take_s_length() {
    let v: serde_json::Value = serde_json::from_str(LINES).unwrap();
    assert_eq!(v["version"], 1);
    let lines = v["lines"].as_object().unwrap();
    assert_eq!(lines.len(), MOODS.len(), "exactly the crate's moods");
    for m in MOODS {
        let by_lang = lines[mood_name(m)].as_object().unwrap_or_else(|| panic!("{}", mood_name(m)));
        assert_eq!(by_lang.len(), 2, "{}: en and pt-BR only", mood_name(m));
        for lang in ["en", "pt-BR"] {
            let text = by_lang[lang].as_str().unwrap();
            let words = text.split_whitespace().count();
            assert!((12..=20).contains(&words), "{} {lang}: {words} words", mood_name(m));
            assert!(!text.chars().any(|c| c.is_ascii_digit()), "{} {lang}: numbers are spelled out", mood_name(m));
            assert_eq!(text.trim(), text);
        }
    }
}
