

pub fn split_for_cap(text: &str, cap: usize) -> Vec<String> {
    let cap = cap.max(1);
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    level(text, cap, Level::Sentence)
}

pub fn split_half(text: &str) -> Option<(String, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let total = len(&words.join(" "));
    let mut left = 0;
    let mut best: Option<(bool, usize, usize)> = None;
    for (i, w) in words[..words.len() - 1].iter().enumerate() {
        left += len(w) + usize::from(i > 0);
        let candidate = (ends_clause(w), (2 * left).abs_diff(total), i + 1);
        best = match best {
            None => Some(candidate),
            Some(b) if (candidate.0 && !b.0) || (candidate.0 == b.0 && candidate.1 < b.1) => Some(candidate),
            keep => keep,
        };
    }
    let cut = best?.2;
    Some((words[..cut].join(" "), words[cut..].join(" ")))
}

#[derive(Clone, Copy)]
enum Level {
    Sentence,
    Clause,
    Word,
    Char,
}

fn level(text: &str, cap: usize, at: Level) -> Vec<String> {
    if len(text) <= cap {
        return vec![text.to_string()];
    }
    let (units, next): (Vec<String>, Level) = match at {
        Level::Sentence => (units_after(text, ends_sentence), Level::Clause),
        Level::Clause => (units_after(text, ends_clause), Level::Word),
        Level::Word => (text.split_whitespace().map(str::to_string).collect(), Level::Char),
        Level::Char => {
            let chars: Vec<char> = text.chars().collect();
            return chars.chunks(cap).map(|c| c.iter().collect()).collect();
        }
    };
    if units.len() < 2 {
        return level(text, cap, next);
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for u in units {
        if len(&u) > cap {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.extend(level(&u, cap, next));
        } else if cur.is_empty() {
            cur = u;
        } else if len(&cur) + 1 + len(&u) <= cap {
            cur.push(' ');
            cur.push_str(&u);
        } else {
            out.push(std::mem::replace(&mut cur, u));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn units_after(text: &str, ends: fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
        if ends(w) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn last_mark(word: &str) -> Option<char> {
    word.trim_end_matches(['"', '\'', ')', '»', '”', '’']).chars().last()
}

fn ends_sentence(word: &str) -> bool {
    matches!(last_mark(word), Some('.' | '!' | '?' | '…'))
}

fn ends_clause(word: &str) -> bool {
    ends_sentence(word) || matches!(last_mark(word), Some(',' | ';' | ':' | '—' | '–')) || word == "-"
}

fn len(s: &str) -> usize {
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_prefers_the_clause_mark_nearest_the_middle() {
        assert_eq!(
            split_half("Sim, essa frase é longa demais; então vai ser dividida."),
            Some(("Sim, essa frase é longa demais;".into(), "então vai ser dividida.".into()))
        );
        assert_eq!(
            split_half("Ontem à noite — já tarde — choveu forte"),
            Some(("Ontem à noite —".into(), "já tarde — choveu forte".into()))
        );
    }

    #[test]
    fn half_falls_back_to_the_word_nearest_the_middle() {
        assert_eq!(split_half("um dois três quatro"), Some(("um dois".into(), "três quatro".into())));
        assert_eq!(split_half("palavra"), None);
        assert_eq!(split_half("  "), None);
    }

    #[test]
    fn cap_keeps_short_text_whole() {
        assert_eq!(split_for_cap("Oi. Tudo bem?", 1000), vec!["Oi. Tudo bem?"]);
    }

    #[test]
    fn cap_packs_sentences_then_clauses_then_words() {
        assert_eq!(split_for_cap("Um dois. Três quatro! Cinco?", 18), vec!["Um dois.", "Três quatro!", "Cinco?"]);
        assert_eq!(split_for_cap("um dois, três quatro, cinco seis", 22), vec!["um dois, três quatro,", "cinco seis"]);
        let words = split_for_cap("aaa bbb ccc ddd eee", 8);
        assert_eq!(words, vec!["aaa bbb", "ccc ddd", "eee"]);

        assert!(split_for_cap(&"é ".repeat(600), 1000).iter().all(|p| p.chars().count() <= 1000));
    }

    #[test]
    fn cap_cuts_one_huge_word_by_characters() {
        let parts = split_for_cap(&"x".repeat(25), 10);
        assert_eq!(parts, vec!["x".repeat(10), "x".repeat(10), "x".repeat(5)]);
    }
}
