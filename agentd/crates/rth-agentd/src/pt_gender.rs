

use std::sync::OnceLock;

use regex::Regex;

pub const HOLD_MAX_CHARS: usize = 160;

const CORRECTIVE: &str = "Your last reply used a gendered word for yourself (\"{hit}\"). Reply to the same message again, in Portuguese, with the same full answer: change only that self-reference, using no gendered word for yourself: say \"agradeço\" or \"valeu\" instead of obrigado or obrigada, \"a comunidade we unfold me criou\" instead of fui criado or desenvolvido, and a noun or a verb for your state (\"estou com sono\", \"estou bem\") instead of an adjective or participle; never put an article before your name; keep everything else you said.";

const OPENER_CORRECTIVE: &str = "Your last reply opened with \"{opener}\". Reply to the same message again, in Portuguese, with the same full answer, starting with the answer itself: leave out only where you run, and keep everything else you said.";

fn longer_name() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^[\s-]+(?:mini|pro|air|studio)\b").expect("pattern"))
}

fn names_another(kind_pt: &str, after: &str) -> bool {
    let words: Vec<&str> = kind_pt.split_whitespace().collect();
    longer_name().is_match(after)
        || crate::device::ALL.iter().any(|d| {
            let more: Vec<&str> = d.kind_pt.split_whitespace().collect();
            more.len() > words.len()
                && more.iter().zip(&words).all(|(a, b)| a.eq_ignore_ascii_case(b))
                && Regex::new(&format!(r"(?i)^[\s-]+{}\b", more[words.len()..].iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join(r"\s+"))).is_ok_and(|r| r.is_match(after))
        })
}

const PAIRS: [(&str, &str); 11] = [("**", "**"), ("*", "*"), ("_", "_"), ("`", "`"), ("\"", "\""), ("'", "'"), ("\u{201c}", "\u{201d}"), ("\u{2018}", "\u{2019}"), ("\u{ab}", "\u{bb}"), ("(", ")"), ("[", "]")];

fn list_mark() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*[-*+>\u{2022}]\s+").expect("pattern"))
}

fn opens_after() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(concat!(
            r"(?i)^[*_\x22'`\u{201d}\u{2019}\u{bb})\]]*[ \t\u{a0}]*(?:$|[\r\n,:;.!?(\u{2013}\u{2014}]|--?[ \t]",
            r"|(?:o|a|os|as|um|uma|uns|umas|você|vocês|eu|isso|isto|esse|essa|este|esta|tudo|nada|é|está|estão|são|tem|têm|há|pode|podem|dá|sim|não|basta|existe|existem)\b)",
        ))
        .expect("pattern")
    })
}

fn paired(s: &str) -> bool {
    PAIRS.iter().all(|(open, close)| if open == close { s.matches(open).count() % 2 == 0 } else { s.matches(open).count() == s.matches(close).count() })
}

fn opener_at(text: &str, kind_pt: &str) -> Option<(usize, usize)> {
    let words: Vec<String> = kind_pt.split_whitespace().map(regex::escape).collect();
    let c = Regex::new(&format!(r"(?i)^(\W*)neste\s+{}\b", words.join(r"\s+"))).ok()?.captures(text)?;
    let end = c.get(0)?.end();
    let after = &text[end..];
    (!names_another(kind_pt, after) && opens_after().is_match(after)).then_some((c.get(1)?.end(), end))
}

pub fn regenerates(text: &str, kind_pt: &str) -> Option<String> {
    let words: Vec<String> = kind_pt.split_whitespace().map(regex::escape).collect();
    let c = Regex::new(&format!(r"(?i)^(\W*)neste\s+{}\b", words.join(r"\s+"))).ok()?.captures(text)?;
    let (lead, end) = (c.get(1)?.end(), c.get(0)?.end());
    (!names_another(kind_pt, &text[end..])).then(|| text[lead..end].to_string())
}

#[cfg(test)]
pub fn opener_for(text: &str, kind_pt: &str) -> Option<String> {
    opener_at(text, kind_pt).map(|(lead, end)| text[lead..end].to_string())
}

pub fn cut_opener_for(text: &str, kind_pt: &str) -> String {
    let Some((lead, end)) = opener_at(text, kind_pt) else { return text.to_string() };
    let marks = &text[..lead];
    let mut rest = &text[end..];
    for (open, close) in PAIRS {
        if marks.contains(open) && rest.starts_with(close) {
            rest = &rest[close.len()..];
            break;
        }
    }
    let mut rest = rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | ';' | '\u{2013}' | '\u{2014}' | '-'));
    let mut cut = &text[..text.len() - rest.len()];
    let kept = list_mark().find(marks).map_or("", |m| &text[..m.end()]);
    let mut next = "";

    if let Some(line) = cut.rfind('\n').map(|nl| nl + 1)
        && let Some(m) = list_mark().find(&text[line..]).filter(|m| line + m.end() >= cut.len())
    {
        (cut, rest) = (&text[..line], &text[line + m.end()..]);
        next = &text[line..line + m.end()];
    }
    if !paired(&cut[kept.len()..]) {
        return text.to_string();
    }
    let kept = if next.is_empty() { kept } else { next };
    let Some(first) = rest.chars().next().filter(|c| c.is_alphanumeric()) else { return text.to_string() };
    let word = rest.split(|c: char| !c.is_alphanumeric()).next().unwrap_or_default();
    if !word.chars().all(char::is_lowercase) {
        return format!("{kept}{rest}");
    }
    format!("{kept}{}{}", first.to_uppercase(), &rest[first.len_utf8()..])
}

pub fn opener_corrective(opener: &str) -> String {
    OPENER_CORRECTIVE.replace("{opener}", opener)
}

fn self_form() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(concat!(
            r"(?im)\b(?:estou|tô|to|sou|fui|fiquei|fico|ficar|ficaria|ficando|ando|me sinto|me senti|me sentindo|tava|estava)\s+(?:muito\s+|bem\s+|um pouco\s+|meio\s+|super\s+)?",
            r"(?:cansad|pront|confus|criad|feit|animad|preparad|curios|sozinh|ocupad|programad|treinad|desenvolvid|projetad|contente?|entediad|grat|novo|nova|obrigad)(?:íssim)?[oa]?s?\b",
            r"|\b(?:o|a|do|da|ao|à|no|na|pelo|pela)\s+unfold\b",
        ))
        .expect("pattern")
    })
}

fn thanks() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bobrigad(?:íssim)?[oa]s?\b").expect("pattern"))
}

fn obliged_to() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)^\s+a\s+\w+r\b").expect("pattern"))
}

pub fn hit(text: &str) -> Option<String> {
    let a = self_form().find(text);
    let b = thanks().find_iter(text).find(|m| !obliged_to().is_match(&text[m.end()..]));
    match (a, b) {
        (Some(a), Some(b)) if b.start() < a.start() => Some(b.as_str().to_string()),
        (Some(a), _) => Some(a.as_str().to_string()),
        (None, b) => b.map(|m| m.as_str().to_string()),
    }
}

pub fn first_sentence_end(text: &str) -> Option<usize> {
    for (n, (i, c)) in text.char_indices().enumerate() {
        if matches!(c, '.' | '!' | '?' | '\n') {
            return Some(i + c.len_utf8());
        }
        if n + 1 >= HOLD_MAX_CHARS {
            return Some(i + c.len_utf8());
        }
    }
    None
}

pub fn corrective(hit: &str) -> String {
    CORRECTIVE.replace("{hit}", hit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opener_is_the_devices_own_words() {
        assert_eq!(opener_for("Neste Mac mini, o Wi-Fi cai.", "Mac mini").as_deref(), Some("Neste Mac mini"));
        assert_eq!(cut_opener_for("Neste Mac mini, o Wi-Fi cai.", "Mac mini"), "O Wi-Fi cai.");
        assert_eq!(cut_opener_for("neste Mac Pro: a rede cai.", "Mac Pro"), "A rede cai.");
        assert_eq!(cut_opener_for("Neste MacBook, a rede cai.", "MacBook"), "A rede cai.");
        assert_eq!(cut_opener_for("Neste iMac, a rede cai.", "iMac"), "A rede cai.");
        for (reply, kind) in [("Neste Mac mini, o Wi-Fi cai.", "Mac"), ("Neste Mac Pro, sim.", "Mac"), ("Neste Mac, sim.", "Mac mini"), ("neste MacBook, sim.", "Mac")] {
            assert_eq!(opener_for(reply, kind), None, "{reply} on a {kind}");
            assert_eq!(cut_opener_for(reply, kind), reply, "{reply} on a {kind}");
        }
    }

    #[test]
    fn the_opener_matches_the_kind_exactly_on_every_device() {
        for here in crate::device::ALL {
            for other in crate::device::ALL {
                let mine = here == other;
                for reply in [format!("Neste {}, a rede cai.", other.kind_pt), format!("neste {}, a rede cai.", other.kind_pt.to_lowercase())] {
                    let want = mine.then(|| reply[..6 + here.kind_pt.len()].to_string());
                    assert_eq!(opener_for(&reply, here.kind_pt), want, "{reply} on a {}", here.kind);
                    assert_eq!(cut_opener_for(&reply, here.kind_pt), if mine { "A rede cai." } else { reply.as_str() }, "{reply} on a {}", here.kind);
                }
            }
        }
    }

    #[test]
    fn the_cut_never_leaves_an_orphan_quote() {
        for reply in ["\"Neste Mac, a rede cai.\"", "\u{201c}Neste Mac, a rede cai.\u{201d}", "**Neste Mac, sim.**", "*Neste Mac, sim.*", "\u{ab}Neste Mac, sim.\u{bb}", "(Neste Mac, a rede cai.)", "[Neste Mac, sim.]", "'Neste Mac, sim.'"] {
            assert_eq!(cut_opener_for(reply, "Mac"), reply, "left as written: {reply}");
        }
        for (reply, want) in [("**Neste Mac**, o Wi-Fi cai.", "O Wi-Fi cai."), ("*Neste Mac*, sim.", "Sim."), ("(Neste Mac), sim.", "Sim."), ("- \"Neste Mac\": sim.", "- Sim."), ("- Neste Mac, sim.", "- Sim."), ("* Neste Mac, sim.", "* Sim.")] {
            assert_eq!(cut_opener_for(reply, "Mac"), want, "{reply}");
        }
    }

    #[test]
    fn the_device_words_open_the_reply_only_before_a_separator_or_a_clause() {
        for (reply, kind) in [
            ("Neste Mac-mini, sim.", "Mac"), ("Neste Mac M5 Pro, a rede cai.", "Mac"), ("Neste Mac minis, sim.", "Mac"), ("Neste Mac Pros, sim.", "Mac"),
            ("Neste Mac Linux, sim.", "Mac"), ("Neste Mac Windows, sim.", "Mac"), ("Neste Mac que você usa, a rede cai.", "Mac"), ("Neste Mac's app, sim.", "Mac"),
            ("Neste computador portátil, sim.", "computador"), ("Neste computador Windows, a rede cai.", "computador"),
            ("Neste Mac de 2020, sim.", "Mac"), ("Neste Mac com 16 GB, sim.", "Mac"), ("Neste Mac e no iPhone, sim.", "Mac"),
        ] {
            assert_eq!(opener_for(reply, kind), None, "{reply} on a {kind}");
            assert_eq!(cut_opener_for(reply, kind), reply, "{reply} on a {kind}");
        }
        for (reply, want) in [("Neste Mac - a rede cai.", "A rede cai."), ("Neste Mac:\n\n1. Abra o app.", "1. Abra o app."), ("neste Mac é possível, sim.", "É possível, sim."), ("Neste Mac você pode abrir o app.", "Você pode abrir o app."), ("Neste Mac.", "Neste Mac.")] {
            assert!(opener_for(reply, "Mac").is_some(), "{reply}");
            assert_eq!(cut_opener_for(reply, "Mac"), want, "{reply}");
        }
    }

    #[test]
    fn any_opening_with_the_device_words_regenerates_and_only_the_strict_one_is_cut() {
        for (reply, want) in [("Neste Mac aqui, a rede cai.", "Neste Mac"), ("Neste Mac M5, a rede cai.", "Neste Mac"), ("Neste Mac de 2020, sim.", "Neste Mac"), ("**Neste Mac** local", "Neste Mac"), ("Neste Mac, sim.", "Neste Mac")] {
            assert_eq!(regenerates(reply, "Mac").as_deref(), Some(want), "{reply}");
        }
        for reply in ["No seu Mac, sim.", "O Wi-Fi neste Mac cai.", "Neste MacBook, sim.", "Nestes Macs, sim.", "Olá! Neste Mac, sim.", "Nesse Mac, sim.", "Neste próprio Mac, sim.", "neste Mac-mini, sim.",
                      "Nesse Mac de 2015, o app roda, mas devagar.", "Nesse Mac mini, verifique o cabo de energia.", "Neste Mac mini, a porta HDMI fica atrás.", "Neste Mac Studio, a porta fica atrás.", "Nesse Mac Pro, a tela é boa."] {
            assert_eq!(regenerates(reply, "Mac"), None, "{reply}");
        }
        for (reply, kind) in [("Nesse computador Linux, use o apt.", "computador"), ("Neste computador Linux, use o apt.", "computador"), ("Nesse MacBook Air, a tela é menor.", "MacBook"), ("Neste MacBook Air, a tela é menor.", "MacBook")] {
            assert_eq!(regenerates(reply, kind), None, "{reply} on a {kind}");
        }
        for reply in ["Neste Mac aqui, a rede cai.", "Neste Mac M5, a rede cai.", "Neste Mac de 2020, sim."] {
            assert_eq!(cut_opener_for(reply, "Mac"), reply, "the cut stays strict: {reply}");
        }
    }

    #[test]
    fn the_cut_keeps_a_list_or_quote_mark() {
        for (reply, want) in [("- Neste Mac, sim.\n- No iPhone, não.", "- Sim.\n- No iPhone, não."), ("> Neste Mac, sim.", "> Sim."), ("  * Neste Mac: a rede cai.", "  * A rede cai."), ("- **Neste Mac**, sim.", "- Sim."), ("Neste Mac:\n- abra o app.", "- Abra o app."), ("Neste Mac:\n\n* Abra o app.", "* Abra o app."),

            ("- Neste Mac:\n- abra o app.", "- Abra o app."), ("- Neste Mac:\n\n- abra o app.", "- Abra o app."), ("- **Neste Mac**:\n- abra", "- Abra"), ("> Neste Mac:\n> abra o app.", "> Abra o app.")] {
            assert_eq!(cut_opener_for(reply, "Mac"), want, "{reply}");
        }
    }

    #[test]
    fn the_opener_vectors_are_this_modules_answers() {
        let mut rows = Vec::new();
        for d in crate::device::ALL {
            for reply in VECTOR_REPLIES.iter().map(|r| r.replace("{k}", d.kind_pt)) {
                rows.push(serde_json::json!({
                    "reply": reply, "kind": d.kind_pt, "regenerates": regenerates(&reply, d.kind_pt), "opener": opener_for(&reply, d.kind_pt), "cut": cut_opener_for(&reply, d.kind_pt),
                    "first_sentence": first_sentence_end(&reply).map(|e| &reply[..e]), "hit": hit(&reply),
                }));
            }
        }
        let about = "Device opener answers generated by the_opener_vectors_are_this_modules_answers.";
        let want = serde_json::to_string_pretty(&serde_json::json!({ "about": about, "vectors": rows })).unwrap() + "\n";
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/pt-opener-vectors.json");
        if std::env::var_os("RTH_WRITE_OPENER_VECTORS").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        assert!(std::fs::read_to_string(path).is_ok_and(|f| f == want), "the vector file is stale: rerun with RTH_WRITE_OPENER_VECTORS=1 and review its diff");
    }

    const VECTOR_REPLIES: &[&str] = &[
        "neste {k}, o Wi-Fi pode cair à noite. Tente reiniciar.", "neste {k} o roteador pode falhar. Reinicie.", "Neste {k}: é possível, sim.", "  Neste {k} \u{2014} a rede cai.",
        "\"Neste {k}\", 3 opções existem.", "Neste {k}.", "Neste {k}", "Neste {k}:\n\n1. Abra o app.", "Neste {k}:\n- Abra o app.", "Neste {k},\nabra o Monitor.", "Neste {k}! Abra o app.",
        "Neste {k}? Sim, aqui.", "Neste {k}...", "Neste {k} (o seu), a rede cai.", "Neste {k}, \u{201c}a rede\u{201d} cai.", "Neste {k}, `ls` lista.", "neste {k}, \u{301}a",
        "neste {k}, iPhone e iPad sincronizam.", "neste {k}, macOS 15 tem isso.", "**Neste {k}**, o Wi-Fi cai.", "\u{201c}Neste {k}\u{201d}, sim.", "\"Neste {k}, a rede cai.\"",
        "**Neste {k}, sim.**", "*Neste {k}*, sim.", "(Neste {k}), sim.", "(Neste {k}, a rede cai.)", "[Neste {k}, sim.]", "'Neste {k}, sim.'", "\u{ab}Neste {k}, sim.\u{bb}",
        "- \"Neste {k}\": sim.", "- Neste {k}, sim.\n- No iPhone, não.", "* Neste {k}, sim.", "> Neste {k}, sim.", "- **Neste {k}**, sim.", "# Neste {k}\nSim.",
        "Neste {k}-mini, sim.", "Neste {k} M5 Pro, a rede cai.", "Neste {k} mini, sim.", "Neste {k} Pro, sim.", "Neste {k} Linux, sim.", "Neste {k} que você usa, a rede cai.",
        "Neste {k} portátil, sim.", "Neste {k} de 2020, sim.", "Neste {k} com 16 GB, sim.", "Neste {k} e no iPhone, sim.", "Neste {k} - a rede cai.", "neste {k} é possível, sim.",
        "Neste {k} você pode abrir o app.", "No seu {k}, o Wi-Fi cai.", "O Wi-Fi neste {k} cai.", "Nestes {k}s, sim.", "Olá! Neste {k}, sim.", "A rede cai. Neste {k}, sim.",
        "\nNeste {k}, o Wi-Fi cai.", "NESTE {k}, sim.", "neste\u{a0}{k}, sim.", "Neste Mac, sim.", "Neste MacBook, sim.", "Neste computador Linux, sim.", "Neste computador, sim.",
        "- Neste {k}:\n- abra o app.", "> Neste {k}:\n> abra o app.", "Neste {k} aqui, a rede cai.", "Nesse {k}, sim.", "Neste próprio {k}, sim.",

        "Nesse {k} de 2015, o app roda, mas devagar.", "Nesse {k} mini, verifique o cabo de energia.", "Neste {k} Studio, a porta HDMI fica atrás.", "Nesse {k} Pro, a tela é boa.",
        "Nesse {k} Linux, use o apt.", "Nesse {k} Air, a tela é menor.", "Neste {k} Air, a tela é menor.",
        "Estou cansada, mas posso ajudar.", "Tudo certo. Estou pronta.", "Obrigado a todos por esperar.", "Estou contente.", "Sou a Unfold.", "",
    ];

    #[test]
    fn the_cut_never_breaks_brackets_quotes_code_or_names() {
        for reply in ["Neste Mac (o seu), a rede cai.", "Neste Mac, \u{201c}a rede\u{201d} cai.", "Neste Mac, `ls` lista.", "neste Mac, \u{301}a"] {
            assert_eq!(cut_opener_for(reply, "Mac"), reply, "left as written: {reply}");
        }
        for (reply, want) in [("neste Mac, iPhone e iPad sincronizam.", "iPhone e iPad sincronizam."), ("neste Mac, macOS 15 tem isso.", "macOS 15 tem isso."), ("**Neste Mac**, o Wi-Fi cai.", "O Wi-Fi cai."), ("\u{201c}Neste Mac\u{201d}, sim.", "Sim.")] {
            assert_eq!(cut_opener_for(reply, "Mac"), want, "{reply}");
        }
    }

    #[test]
    fn an_opener_is_found_and_cut_to_a_well_formed_sentence() {
        for (reply, want) in [
            ("neste Mac, o Wi\u{2011}Fi pode ficar caindo à noite.", "O Wi\u{2011}Fi pode ficar caindo à noite."),
            ("neste Mac o problema pode ser interferência.", "O problema pode ser interferência."),
            ("Neste Mac: é possível, sim.", "É possível, sim."),
            ("  Neste mac \u{2014} a rede cai.", "A rede cai."),
            ("\"Neste Mac\", 3 opções existem.", "3 opções existem."),
        ] {
            assert_eq!(opener_for(reply, "Mac").as_deref().map(str::to_lowercase).as_deref(), Some("neste mac"), "{reply}");
            let cut = cut_opener_for(reply, "Mac");
            assert_eq!(cut, want, "{reply}");
            let c = cut.chars().next().unwrap();
            assert!(!c.is_lowercase() && c.is_alphanumeric(), "starts with a capital or a digit: {cut}");
        }
        for reply in ["No seu Mac, o Wi-Fi cai.", "O Wi-Fi neste Mac cai.", "neste Macbook, a rede cai.", "Nestes Macs, sim.", "Olá! Neste Mac, sim."] {
            assert_eq!(opener_for(reply, "Mac"), None, "{reply}");
            assert_eq!(cut_opener_for(reply, "Mac"), reply);
        }
        assert_eq!(cut_opener_for("Neste Mac.", "Mac"), "Neste Mac.", "nothing but the opener stays as written");

        assert_eq!(opener_corrective("neste Mac"), "Your last reply opened with \"neste Mac\". Reply to the same message again, in Portuguese, with the same full answer, starting with the answer itself: leave out only where you run, and keep everything else you said.");
    }

    #[test]
    fn the_detector_agrees_with_every_shared_vector() {
        let bytes = include_bytes!("../testdata/pt-gender-vectors.json");
        assert_eq!(crate::backend::text_sha256(std::str::from_utf8(bytes).unwrap()), "5b50fc77b0b16cd6b6b5b9d091c53b702985e247a3b1acf3310eefe0998eec1d", "the pinned shared vector file");
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        let rows = v["vectors"].as_array().unwrap();
        assert_eq!(rows.len(), 43);
        for r in rows {
            let text = r["text"].as_str().unwrap();
            assert_eq!(hit(text).is_some(), r["hit"].as_bool().unwrap(), "{text:?}: {:?}", hit(text));
            if let Some(m) = r["gender_py_match"].as_str() {
                assert_eq!(hit(text).as_deref(), Some(m), "{text:?}: the same words as gender.py");
            }
        }
    }

    #[test]
    fn the_first_sentence_ends_at_its_mark_or_the_cap() {
        assert_eq!(first_sentence_end("Estou cansado"), None, "still arriving");
        assert_eq!(first_sentence_end("Estou cansado. E você?"), Some("Estou cansado.".len()));
        assert_eq!(first_sentence_end("Oi!\nTudo"), Some(3));
        let long = "á".repeat(200);
        assert_eq!(first_sentence_end(&long), Some("á".repeat(HOLD_MAX_CHARS).len()), "a cap in characters, on a char boundary");
    }

    #[test]
    fn the_corrective_turn_names_the_hit_and_stays_english() {
        let c = corrective("estou cansado");
        assert!(c.starts_with("Your last reply used a gendered word for yourself (\"estou cansado\")."), "{c}");
        assert!(!c.contains("{hit}"));
        assert_eq!(CORRECTIVE, include_str!("../testdata/pt-corrective.txt"), "the corrective text names the replacement and preserves the rest");
    }
}
