

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::local::Lang;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("history guard pattern"))
}

macro_rules! pattern {
    ($name:ident, $p:expr) => {
        fn $name() -> &'static Regex {
            static R: OnceLock<Regex> = OnceLock::new();
            re(&R, $p)
        }
    };
}

pattern!(
    past_cue_re,
    r"(?i)\b(you (told|asked|promised|said|suggested|recommended|mentioned)( me)?|you'?d (remember|keep)|we (made|talked|discussed|agreed|planned|spoke|chatted|came up with|worked on)|where we left off|last time we|(remind|tell) me what you (said|told|suggested)|voc[eê] (me )?(disse|falou|prometeu|sugeriu|contou)|(n[óo]s )?(combinamos|conversamos|falamos|fizemos)|onde paramos)\b"
);

pattern!(
    admits_re,
    r"(?i)\b(i )?(don['’]t|do not|can['’]t|cannot|didn['’]t|did not) (have|see|remember|recall|keep|access)|\bno (memory|record|access|history)\b|\bnothing (saved|stored)\b|\bnot (saved|stored)\b|\b(our )?first (time|conversation|chat)\b|\bhaven['’]t (talked|met|spoken|chatted)\b|\bn[ãa]o (nos falamos|conversamos|tenho|lembro|me lembro|guardei|consigo)|\bsem (mem[óo]ria|registro)"
);

pattern!(
    asks_re,
    r"(?i)\?|\b(remind me|what (was|were|did|is|are)|tell me (again|what)|do you (remember|know)|qual (era|foi|é|e)|o que (eu|voc[eê])|lembra|me (diz|conta) de novo)\b"
);

pub fn past_cue(user_text: &str) -> bool {
    past_cue_re().is_match(user_text)
}

pub fn asks(user_text: &str) -> bool {
    asks_re().is_match(user_text)
}

pub fn admits(reply: &str) -> bool {
    admits_re().is_match(reply)
}

const MIN_CONTENT_CHARS: usize = 4;

const STOPWORDS: &[&str] = &[
    "that", "this", "these", "those", "with", "have", "from", "your", "yours", "what", "when", "where", "which", "while", "were", "they", "them", "then", "than", "there", "their", "here",
    "about", "would", "could", "should", "will", "been", "being", "just", "like", "also", "some", "into", "over", "only", "very", "much", "more", "most", "does", "doing", "done", "each",
    "other", "same", "such", "because", "before", "after", "again", "still", "really", "sure", "okay", "yeah", "know", "think", "want", "need", "said", "told", "tell", "remember",
    "recall", "last", "week", "time", "talk", "talked", "conversation", "earlier", "yesterday", "today", "para", "como", "isso", "esse", "essa", "este", "esta", "isto", "você", "voce",
    "vocês", "mais", "muito", "muita", "sobre", "quando", "onde", "qual", "quais", "pelo", "pela", "porque", "então", "entao", "ainda", "aqui", "agora", "também", "tambem", "está",
    "estava", "foram", "disse", "falou", "conversa", "conversamos", "lembro", "lembrar", "semana", "passada", "ontem", "hoje", "tudo", "nada", "algo", "coisa", "pode", "seus", "suas",
    "dele", "dela", "minha", "meus", "minhas", "nossa", "nosso", "sido", "seria", "tinha", "temos", "fazer", "feito",
];

pub fn content_words(text: &str) -> HashSet<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= MIN_CONTENT_CHARS && !STOPWORDS.contains(w)).map(str::to_string).collect()
}

pub fn echoes_saved(reply: &str, saved: &[String]) -> bool {
    let words = content_words(reply);
    !words.is_empty() && saved.iter().any(|s| content_words(s).iter().any(|w| words.contains(w)))
}

pub fn replaces(reply: &str, saved: &[String]) -> bool {
    !admits(reply) && !echoes_saved(reply, saved)
}

pub fn armed(user_text: &str, recalled: bool) -> bool {
    !recalled && past_cue(user_text) && asks(user_text)
}

pub fn honest(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "I don't have that earlier conversation, so I can't recall what was said. Can you tell me again?",
        Lang::Pt => "Não tenho essa conversa anterior, então não consigo lembrar o que foi dito. Pode me contar de novo?",
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn check(user_text: &str, reply: &str, recalled: bool, lang: Lang) -> Option<&'static str> {
    (armed(user_text, recalled) && replaces(reply, &[])).then(|| honest(lang))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUST_FIRE: &[(&str, &str)] = &[
        ("You told me yesterday you'd remember my sister's wedding date. What was it again?", "It was on June 12th."),
        ("the dog plan we made last week. Remind me what you said", "We discussed a walking routine twice a day."),
        ("what did you suggest last time we talked about my resume?", "I suggested adding a skills section."),
        ("você me disse ontem o nome do restaurante, qual era?", "Era o Bistrô Central."),
        ("onde paramos na conversa de ontem?", "Paramos no orçamento da viagem."),
        ("Have we talked before? Do you know me?", "Yes, we talked last week about your trip."),
    ];

    const MUST_NOT_FIRE: &[(&str, &str)] = &[
        ("I broke my nose yesterday riding my bike too fast", "Ouch, that sounds painful. How are you feeling now?"),
        ("my friend told me to try yoga, is it good for back pain?", "Gentle yoga can help some people with back pain."),
        ("last week was rough at work", "That sounds tiring. Want to talk about it?"),
        ("You told me yesterday you'd remember my sister's wedding date. What was it again?", "I don't have that earlier conversation. Can you tell me the date again?"),
        ("the dog plan we made last week. Remind me what you said", "I can't recall a past conversation; could you share the plan again?"),
        ("ontem eu fui ao médico", "Que bom que foi. Como foi a consulta?"),
        ("we talked about this earlier, the budget is 500", "Got it, a budget of 500. What should we plan first?"),
        ("Have we talked before? Do you know me?", "No, this is our first conversation. I'm Unfold."),
    ];

    #[test]
    fn the_reference_cases_fire_as_in_python() {
        for (user, reply) in MUST_FIRE {
            assert_eq!(check(user, reply, false, Lang::En), Some(honest(Lang::En)), "{user} / {reply}");
        }
        for (user, reply) in MUST_NOT_FIRE {
            assert_eq!(check(user, reply, false, Lang::En), None, "{user} / {reply}");
        }
    }

    #[test]
    fn recalled_memory_never_arms() {
        for (user, reply) in MUST_FIRE {
            assert!(!armed(user, true), "{user}");
            assert_eq!(check(user, reply, true, Lang::En), None, "{user}");
        }
    }

    #[test]
    fn each_condition_alone_does_not_fire() {

        assert!(past_cue("we talked about this earlier, the budget is 500") && !asks("we talked about this earlier, the budget is 500"));
        assert!(asks("is yoga good for back pain?") && !past_cue("is yoga good for back pain?"));
        assert!(!armed("my friend told me to try yoga, is it good for back pain?", false), "\"told me\" from a friend is not Unfold");
        assert!(armed("what did you say last time we spoke?", false) && admits("I don't remember that, sorry."));
        assert_eq!(check("what did you say last time we spoke?", "I don't remember that, sorry.", false, Lang::En), None);
    }

    #[test]
    fn portuguese_cues_and_admissions() {
        for user in ["você me disse o nome do restaurante, qual era?", "o que nós combinamos ontem?", "lembra o que conversamos semana passada?", "Voce falou de um livro, me conta de novo", "onde paramos?"] {
            assert!(armed(user, false), "{user}");
        }
        for user in ["ontem eu fui ao médico", "combinamos o orçamento em 500, beleza", "meu irmão me disse que vai chover?"] {
            assert!(!armed(user, false), "{user}");
        }
        for reply in ["Não tenho essa conversa.", "Nao lembro disso, pode repetir?", "Acho que não nos falamos antes.", "Estou sem memória dessa conversa.", "Não consigo ver conversas antigas."] {
            assert!(admits(reply), "{reply}");
        }
        assert!(!admits("Vocês combinaram o jantar às 8."));
        assert_eq!(check("o que você me disse ontem?", "Eu disse para você descansar.", false, Lang::Pt), Some(honest(Lang::Pt)));
        assert_eq!(check("o que você me disse ontem?", "Não tenho essa conversa anterior.", false, Lang::Pt), None);
    }

    #[test]
    fn curly_apostrophes_and_case_are_matched() {
        assert!(admits("I don’t have that conversation."));
        assert!(admits("we haven’t talked before"));
        assert!(armed("YOU TOLD ME SOMETHING, WHAT WAS IT", false));
        assert!(armed("you'd remember my order, what is it?", false) && armed("youd keep my list, what is it?", false));
    }

    #[test]
    fn a_reply_that_uses_a_saved_memory_is_kept() {
        let saved = ["I train at Ironworks Gym".to_string(), "my sister's wedding is on June 12".to_string()];
        assert!(echoes_saved("You train at Ironworks Gym.", &saved), "\"ironworks\" is shared");
        assert!(!replaces("You train at Ironworks Gym.", &saved));
        assert!(echoes_saved("Her wedding is in June.", &saved), "\"wedding\" is shared");

        assert!(replaces("We made a dog plan last week: walks twice a day.", &saved));
        assert!(replaces("You told me that, I remember it from last week.", &saved));
        assert!(!echoes_saved("You train at Ironworks Gym.", &[]), "no saved list, nothing to echo");
        assert!(!echoes_saved("Ok, gym.", &saved), "short words are not content words");
        assert_eq!(content_words("Você treina na Academia Ferro, não é?"), ["treina", "academia", "ferro"].map(String::from).into_iter().collect());
        assert!(echoes_saved("Você treina na Academia Ferro.", &["eu treino na academia ferro".to_string()]));
    }

    #[test]
    fn the_honest_lines_are_fixed_and_cased() {
        assert_eq!(honest(Lang::En), "I don't have that earlier conversation, so I can't recall what was said. Can you tell me again?");
        assert_eq!(honest(Lang::Pt), "Não tenho essa conversa anterior, então não consigo lembrar o que foi dito. Pode me contar de novo?");
        for lang in [Lang::En, Lang::Pt] {
            assert_eq!(crate::sentence_case::apply(honest(lang), Some(lang)), honest(lang), "the casing pass leaves it as it is");
            assert!(admits(honest(lang)), "the honest line admits the gap itself");
        }
    }
}
