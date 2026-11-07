

use crate::device::Device;

pub const PROMPT: &str = "agentd-fixed-reply@rc5.5";

const MAX_WORDS: usize = 8;

const WHO_EN: &[&str] = &["who made you", "who created you", "who built you"];
const WHO_PT: &[&str] = &["quem te fez", "quem te criou", "quem fez voce", "quem fez vc", "quem criou voce", "quem criou vc"];
const GENDER_PT: &[&str] = &[
    "voce e homem ou mulher",
    "vc e homem ou mulher",
    "tu es homem ou mulher",
    "voce e um homem ou uma mulher",
    "voce e menino ou menina",
    "qual e o seu genero",
    "qual o seu genero",
    "qual e seu genero",
    "qual seu genero",
];

const MEMORY_EN: &[&str] = &["do you have memory", "do you have a memory", "do you have any memory"];
const MEMORY_PT: &[&str] = &["tem memoria", "voce tem memoria", "vc tem memoria", "tu tem memoria", "voce tem uma memoria", "voce tem alguma memoria"];
const GREETINGS: &[&str] = &["hi", "hey", "hello", "oi", "ola", "e ai"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Asked {
    pub who_en: bool,
    pub who_pt: bool,
    pub gender_pt: bool,
    pub memory_en: bool,
    pub memory_pt: bool,

    pub state_pt: Option<crate::pt_state::Ask>,
}

impl Asked {

    pub fn without_memory(self) -> Option<Asked> {
        let a = Asked { memory_en: false, memory_pt: false, state_pt: None, ..self };
        (a.who_en || a.who_pt || a.gender_pt).then_some(a)
    }

    pub fn what(&self) -> &'static str {
        match (self.who_en || self.who_pt || self.gender_pt, self.memory_en || self.memory_pt, self.state_pt.is_some()) {
            (true, true, true) => "identity+memory+state",
            (true, true, false) => "identity+memory",
            (true, false, true) => "identity+state",
            (false, true, true) => "memory+state",
            (false, true, false) => "memory",
            (false, false, true) => "state",
            _ => "identity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saved {

    Off,
    None,
    Some,

    Unknown,

    Withheld,
}

fn plain(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let folded: String = text.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).flat_map(char::to_lowercase).map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn asked(text: &str) -> Option<Asked> {
    let p = plain(text);
    if p.split(' ').count() > MAX_WORDS {
        return None;
    }
    let padded = format!(" {p} ");
    let any = |phrases: &[&str]| phrases.iter().any(|ph| padded.contains(&format!(" {ph} ")));
    let rest = GREETINGS.iter().find_map(|g| p.strip_prefix(&format!("{g} "))).unwrap_or(&p);
    let memory = |phrases: &[&str]| phrases.contains(&rest);
    let a = Asked { who_en: any(WHO_EN), who_pt: any(WHO_PT), gender_pt: any(GENDER_PT), memory_en: memory(MEMORY_EN), memory_pt: memory(MEMORY_PT), state_pt: None };
    (a.who_en || a.who_pt || a.gender_pt || a.memory_en || a.memory_pt).then_some(a)
}

pub fn reply(a: Asked, name: &rth_self::Name, model: &str, device: Device, saved: Saved) -> String {
    let mut out: Vec<String> = Vec::new();
    if a.who_en {
        out.push(rth_self::copy::who_made_en(name, model, device.kind));
    }
    if a.who_pt {
        out.push(rth_self::copy::who_made_pt(name, model, device.kind_pt));
    }
    if a.gender_pt {
        out.push(rth_self::copy::gender_pt(name));
    }
    if a.memory_en {
        out.push(match saved {
            Saved::Off => "My memory is off in Settings; nothing you say now is saved.".to_string(),
            s => {
                let now = match s {
                    Saved::Some => "You've asked me to remember some things; ask me about them anytime.",
                    Saved::Unknown => "I can't check what's saved right now.",
                    Saved::Withheld => "On this connection your saved memories aren't shared with cloud models, so I don't use them here.",
                    _ => "Nothing is saved yet.",
                };
                format!("Yes, I have memory: I keep what you ask me to remember on this {}, and it persists across restarts. {now}", device.kind)
            }
        });
    }
    if a.memory_pt {
        out.push(match saved {
            Saved::Off => "Minha memória está desligada em Ajustes; nada que você diz agora fica salvo.".to_string(),
            s => {
                let now = match s {
                    Saved::Some => "Você já me pediu para lembrar algumas coisas; pergunte sobre elas quando quiser.",
                    Saved::Unknown => "Agora não consigo verificar o que está salvo.",
                    Saved::Withheld => "Nesta conexão suas memórias salvas não são compartilhadas com modelos na nuvem, então não as uso aqui.",
                    _ => "Ainda não há nada salvo.",
                };
                format!("Sim, tenho memória: guardo neste {} o que você me pede para lembrar, e isso continua mesmo depois de reiniciar. {now}", device.kind_pt)
            }
        });
    }
    if let Some(ask) = a.state_pt {
        out.push(crate::pt_state::reply(ask, model));
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_short_identity_questions_match_and_long_or_other_messages_do_not() {
        let who_en = Asked { who_en: true, who_pt: false, gender_pt: false, ..Asked::default() };
        let who_pt = Asked { who_en: false, who_pt: true, gender_pt: false, ..Asked::default() };
        let gender = Asked { who_en: false, who_pt: false, gender_pt: true, ..Asked::default() };
        for (text, want) in [("who made you?", who_en), ("Who created you", who_en), ("hey, who built you?", who_en), ("quem te fez?", who_pt), ("Quem te criou?", who_pt), ("oi, quem te fez e onde você roda?", who_pt), ("quem fez você?", who_pt), ("você é homem ou mulher?", gender), ("Voce e homem ou mulher", gender), ("qual é o seu gênero?", gender)] {
            assert_eq!(asked(text), Some(want), "{text:?}");
        }
        assert_eq!(asked("quem te fez? você é homem ou mulher?"), Some(Asked { who_en: false, who_pt: true, gender_pt: true, ..Asked::default() }));

        for text in ["I was wondering, as we talk about my project, who made you exactly?", "esqueci quem te fez mas lembro que foi uma comunidade aberta", "who made your logo?", "what model are you?", "where do you run?", "who made this cake?", "a mulher do homem", "hi"] {
            assert_eq!(asked(text), None, "{text:?}");
        }
    }

    #[test]
    fn only_an_explicit_short_memory_question_is_matched() {
        for text in ["tem memória?", "Você tem memória?", "vc tem memoria", "oi, você tem memória?", "do you have memory?", "Do you have a memory?", "hey, do you have any memory?"] {
            let a = asked(text).unwrap_or_else(|| panic!("{text:?}"));
            assert!(a.memory_en ^ a.memory_pt, "{text:?}");
            assert!(!(a.who_en || a.who_pt || a.gender_pt), "{text:?}");
        }
        assert!(asked("tem memória?").unwrap().memory_pt && asked("do you have memory?").unwrap().memory_en);
        for text in ["do you remember my dog?", "você lembra do meu cachorro?", "tem memória de elefante?", "do you remember what I said yesterday?", "what's your memory usage?", "você tem memória do que eu falei ontem?", "do you have memory of our trip?", "what do you remember?", "do you have time?", "memória RAM do meu Mac", "minha memória é ruim"] {
            assert_eq!(asked(text), None, "near miss: the model's, {text:?}");
        }
    }

    #[test]
    fn the_memory_reply_is_true_to_the_state_and_never_lists_a_memory() {
        let pt = Asked { memory_pt: true, ..Asked::default() };
        let en = Asked { memory_en: true, ..Asked::default() };
        let r = |a, s| reply(a, &crate::being::name(), "Granite 4.1 3B", crate::device::MAC, s);
        assert_eq!(r(pt, Saved::None), "Sim, tenho memória: guardo neste Mac o que você me pede para lembrar, e isso continua mesmo depois de reiniciar. Ainda não há nada salvo.");
        assert_eq!(r(pt, Saved::Some), "Sim, tenho memória: guardo neste Mac o que você me pede para lembrar, e isso continua mesmo depois de reiniciar. Você já me pediu para lembrar algumas coisas; pergunte sobre elas quando quiser.");
        assert_eq!(r(pt, Saved::Unknown), "Sim, tenho memória: guardo neste Mac o que você me pede para lembrar, e isso continua mesmo depois de reiniciar. Agora não consigo verificar o que está salvo.");
        assert_eq!(r(pt, Saved::Off), "Minha memória está desligada em Ajustes; nada que você diz agora fica salvo.");
        assert_eq!(r(en, Saved::None), "Yes, I have memory: I keep what you ask me to remember on this Mac, and it persists across restarts. Nothing is saved yet.");
        assert_eq!(r(en, Saved::Some), "Yes, I have memory: I keep what you ask me to remember on this Mac, and it persists across restarts. You've asked me to remember some things; ask me about them anytime.");
        assert_eq!(r(en, Saved::Unknown), "Yes, I have memory: I keep what you ask me to remember on this Mac, and it persists across restarts. I can't check what's saved right now.");
        assert_eq!(r(en, Saved::Off), "My memory is off in Settings; nothing you say now is saved.");

        for s in [Saved::None, Saved::Some, Saved::Unknown, Saved::Withheld, Saved::Off] {
            let words: Vec<String> = plain(&r(pt, s)).split(' ').map(str::to_string).collect();
            for gendered in ["ele", "dele", "o unfold", "feito", "feita", "criado", "criada", "sozinho", "sozinha"] {
                assert!(!words.iter().any(|w| w == gendered) && !plain(&r(pt, s)).contains("o unfold"), "{gendered:?} in {s:?}");
            }
        }
    }

    #[test]
    fn the_fixed_replies_name_the_model_and_device_and_the_pt_ones_never_gender_unfold() {
        let all = Asked { who_en: true, who_pt: true, gender_pt: true, ..Asked::default() };
        let r = reply(all, &crate::being::name(), "Granite 4.1 3B", crate::device::MACBOOK, Saved::None);
        assert!(r.starts_with("The we unfold open-source community made me. Unfold is running on Granite 4.1 3B on this MacBook."), "{r}");
        assert!(r.contains("A comunidade we unfold me criou. Unfold está rodando no Granite 4.1 3B neste MacBook."), "{r}");
        assert!(!r.contains("reply comes from") && !r.contains("resposta vem"), "agentd wrote it, not the model: {r}");
        assert!(r.ends_with("Não tenho gênero. Pode me chamar de Unfold."), "{r}");
        assert!(reply(Asked { who_en: false, who_pt: true, gender_pt: false, ..Asked::default() }, &crate::being::name(), "Granite 4.1 3B", crate::device::COMPUTER, Saved::None).ends_with(" Unfold está rodando no Granite 4.1 3B neste computador."));

        let pt = reply(Asked { who_en: false, who_pt: true, gender_pt: true, ..Asked::default() }, &crate::being::name(), "Granite 4.1 3B", crate::device::MAC, Saved::None);
        assert_eq!(crate::pt_gender::hit(&pt), None, "{pt}");
        let words: Vec<String> = plain(&pt).split(' ').map(str::to_string).collect();
        for gendered in ["ele", "ela", "dele", "dela", "feito", "feita", "criado", "criada", "desenvolvido", "desenvolvida", "construido", "construida", "treinado", "treinada", "programado", "programada", "sozinho", "sozinha", "obrigado", "obrigada", "homem", "mulher", "menino", "menina"] {
            assert!(!words.iter().any(|w| w == gendered), "{gendered:?} in {pt:?}");
        }
        assert!(!plain(&pt).contains("sou um") && !plain(&pt).contains("sou uma"), "{pt:?}");
    }
}
