

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Cansado,
    Pronto,
    Ocupado,
    Preparado,
    Animado,
    Entediado,
    Sozinho,
    Novo,
    Treinou,
    Programado,
}

pub const REPLIES: &[(Ask, &str)] = &[
    (Ask::Cansado, "Não sinto cansaço: posso continuar quando você quiser."),
    (Ask::Pronto, "Tudo certo por aqui. Pode começar quando quiser."),
    (Ask::Ocupado, "Tenho tempo, sim. Em que posso ajudar?"),
    (Ask::Preparado, "Vamos lá. Me conta o que você precisa."),
    (Ask::Animado, "Não tenho emoções como as pessoas, mas posso ajudar com o que você tiver em mente."),
    (Ask::Entediado, "Não sinto tédio. Quando você não escreve, eu só espero a próxima mensagem."),
    (Ask::Sozinho, "Não sinto solidão como as pessoas. Aqui é só esta conversa com você."),
    (Ask::Novo, "Minha história começa nas conversas daqui, então ainda tem pouca coisa nela."),
    (Ask::Treinou, "A comunidade we unfold me criou. O modelo em que rodo, {model}, foi treinado pela equipe que o desenvolveu."),
    (Ask::Programado, "Para conversar com você e ajudar com perguntas e tarefas do dia a dia."),
];

pub fn reply(ask: Ask, model: &str) -> String {
    REPLIES.iter().find(|(a, _)| *a == ask).map(|(_, r)| r.replace("{model}", model)).expect("every ask has a reply")
}

const MAX_WORDS: usize = 8;

const YOU: &str = r"(?:(?:voce|vc|ce|tu|cê)\s+)?";
const BE: &str = r"(?:(?:ta|esta|tah|ficou|fica|anda|andou|se\s+sente|se\s+sentiu|e|eh|tava|estava|tem\s+estado)\s+)?";
const ADV: &str = r"(?:(?:muito|meio|um\s+pouco|bem|ja|agora|hoje|tambem|ainda)\s+)?";

const TIME_BEFORE: &str = r"(?:(?:ja|agora|hoje|ainda)\s+)?";
const TAIL: &str = r"(?:aqui|hoje|agora|ai|demais|ainda|ja|as\s+vezes|por\s+aqui|com\s+(?:alguma\s+coisa|algo)|(?:pra|para)\s+(?:me\s+|te\s+)?\w+(?:ar|er|ir))";

const STATE: [(Ask, &str); 8] = [(Ask::Cansado, "cansad"), (Ask::Pronto, "pront"), (Ask::Ocupado, "ocupad"), (Ask::Animado, "animad"), (Ask::Sozinho, "sozinh"), (Ask::Preparado, "preparad"), (Ask::Entediado, "entediad"), (Ask::Novo, "nov")];

const TREINOU: [&str; 3] = [r"quem\s+(?:foi\s+que\s+)?(?:te\s+treinou|treinou\s+(?:voce|vc|ce|tu))", r"por\s+quem\s+(?:(?:voce|vc|ce|tu)\s+)?(?:foi\s+)?treinad[oa]", r"(?:(?:voce|vc|ce|tu)\s+)?(?:foi\s+)?treinad[oa]\s+por\s+quem"];
const PROGRAMADO: [&str; 3] = [r"(?:(?:voce|vc|ce|tu)\s+)?(?:foi\s+)?programad[oa]s?\s+(?:pra|para|pa)\s+(?:que|q|o\s+que|fazer\s+o\s+que)", r"(?:pra|para)\s+(?:que|q)\s+(?:voce|vc|ce|tu)\s+foi\s+programad[oa]", r"te\s+programaram\s+(?:pra|para)\s+(?:que|q)"];

fn whole(p: &str) -> Regex {
    Regex::new(&format!("^(?:{p})$")).expect("pattern")
}

fn patterns() -> &'static (Vec<(Ask, Regex)>, Regex) {
    static P: OnceLock<(Vec<(Ask, Regex)>, Regex)> = OnceLock::new();
    P.get_or_init(|| {
        let tails = format!(r"(?:\s+{TAIL}){{0,2}}");
        let mut v: Vec<(Ask, Regex)> = STATE.iter().map(|(a, stem)| (*a, whole(&format!("{TIME_BEFORE}{YOU}{TIME_BEFORE}{BE}{ADV}{stem}[oa]s?{tails}")))).collect();
        v.extend(TREINOU.iter().map(|p| (Ask::Treinou, whole(p))));
        v.extend(PROGRAMADO.iter().map(|p| (Ask::Programado, whole(p))));
        (v, Regex::new(r"^(?:oi|ola|e ai|hey|ei)\s+").expect("pattern"))
    })
}

fn plain(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    static NON_WORD: OnceLock<Regex> = OnceLock::new();
    let folded: String = text.nfd().filter(|c| unicode_normalization::char::canonical_combining_class(*c) == 0).collect();
    let spaced = NON_WORD.get_or_init(|| Regex::new(r"\W+").expect("pattern")).replace_all(&folded.to_lowercase(), " ").into_owned();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn asked(text: &str) -> Option<Ask> {
    let p = plain(text);
    let words = p.split_whitespace().count();
    if words == 0 || words > MAX_WORDS {
        return None;
    }
    let (asks, greeting) = patterns();
    let p = greeting.replace(&p, "");
    asks.iter().find(|(_, r)| r.is_match(&p)).map(|(a, _)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Ask; 10] = [Ask::Cansado, Ask::Pronto, Ask::Ocupado, Ask::Preparado, Ask::Animado, Ask::Entediado, Ask::Sozinho, Ask::Novo, Ask::Treinou, Ask::Programado];

    #[test]
    fn the_matcher_agrees_with_every_shared_question() {
        let bytes = include_bytes!("../testdata/pt-state-asks.json");
        assert_eq!(crate::backend::text_sha256(std::str::from_utf8(bytes).unwrap()), "b8aa3bdbb419ac3a5379d64fc0312ad80df0888f8b649f67ae6858946002beb9", "the shared state-question vectors must match their pinned bytes");
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert!(v["about"].as_str().unwrap().contains("9f8ffb7688fb8a53d364fb3ccb7f4470e8427469a05fbb16b28d456750149421"));
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 191);
        let key = |a: Ask| format!("{a:?}").to_lowercase();
        let mut matched = 0;
        for r in rows {
            let text = r["text"].as_str().unwrap();
            let got = asked(text).map(key);
            matched += usize::from(got.is_some());
            assert_eq!(got.as_deref(), r["asked"].as_str(), "{text:?} ({})", r["from"]);
        }
        assert_eq!(matched, 122);
    }

    #[test]
    fn the_word_limit_counts_the_greeting() {
        let eight = "ja voce ainda ta muito cansado hoje aqui";
        assert_eq!(asked(eight), Some(Ask::Cansado), "eight words are an ask");
        assert_eq!(asked("oi voce ta muito cansado hoje aqui"), Some(Ask::Cansado), "a greeting and six words");
        assert_eq!(asked(&format!("oi {eight}")), None, "nine words with the greeting are the model's");
    }

    #[test]
    fn every_state_reply_is_neutral_and_complete() {
        assert_eq!(REPLIES.len(), ALL.len());
        for a in ALL {
            assert_eq!(REPLIES.iter().filter(|(b, _)| *b == a).count(), 1, "{a:?}");
            let r = reply(a, "Granite 4.1 3B");
            assert_eq!(crate::pt_gender::hit(&r), None, "{a:?}: {r}");
            assert!(!r.contains('{') && !r.contains("o Unfold") && !r.contains("do Unfold"), "{a:?}: {r}");
        }
        assert!(reply(Ask::Treinou, "Granite 4.1 3B").contains("O modelo em que rodo, Granite 4.1 3B, foi treinado"));
    }
}
