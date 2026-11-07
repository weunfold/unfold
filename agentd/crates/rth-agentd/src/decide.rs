

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::local;

pub const INSTRUCTIONS: &str = include_str!("decide/instructions-mini-v4.3.txt");
pub const INSTRUCTIONS_SHA256: &str = "be6fccfb5a28243f7034d732da58143fa8034f1ca2f5dd5cbb6456a3eb8808f3";

pub const PART: &str = "memory.decide@1/mini-v4.3";

pub const INSTRUCTIONS_ASKED: &str = include_str!("decide/instructions-mini-v4.1.txt");
pub const INSTRUCTIONS_ASKED_SHA256: &str = "e9e32f3ca1575d603c9a927fce5bacea70b468879797f262f5d6d364c703182c";
pub const PART_ASKED: &str = "memory.decide@1/mini-v4.1";

pub const DEADLINE: Duration = Duration::from_millis(600);
pub const MAX_ITEMS: usize = 3;
pub const MAX_PROPOSAL_CHARS: usize = 280;
const MAX_TOKENS: u32 = 160;

pub const REMEMBER_CUE: &str = r"(?i)^(?:(?:please|por favor)[,]?\s+)?(?:remember|note|save this|(?:don['’]t|do\s+not)\s+forget|lembra(?:-te)?|lembre(?:-se)?|anota|anote|guarda|guarde|n[aã]o\s+esque[cç]a|n[aã]o\s+esquece)(?:\s+(?:that|que|disso|isso|de\s+que))?\s*[,:]?\s+";

pub const CORRECT_CUE: &str = r"(?i)^(?:(?:actually|correction|wait|na\s+verdade|corre[cç][aã]o|espera)\s*[,:]?|(?:no|n[aã]o)\s*[,:])\s+";

pub const FORGET_CUE: &str = r"(?i)^(?:(?:please|por\s+favor)[,]?\s+)?(?:forget|esque[cç]a|esquece)(?:\s+(?:about|that|que|disso|isso|de\s+que|sobre))?\s*[,:]?\s+";

pub const FORGET_CUE_RC5: &str = r"(?i)^(?:(?:please|por\s+favor)[,]?\s+)?(?:(?:(?:don['’]t|do\s+not)\s+remember)(?:\s+(?:that|about))?\s+(?P<anymore>.+?)\s+any\s*more\s*[.!]*\s*$|(?:delete|erase)\s+what\s+i\s+(?:said|told\s+you)\s+about\s+|n[aã]o\s+lembr[ae]\s+mais\s+(?:de\s+|do\s+|da\s+|dos\s+|das\s+|que\s+)?|apag(?:a|ue)\s+o\s+que\s+eu\s+(?:disse|falei)\s+sobre\s+)";

pub const FORGET_DISMISS: &str = r"(?i)^\s*(?:(?:oh|ok|okay|ah|bom|t[aá])[,]?\s+)?(?:forget\s+(?:it|that|about\s+it)|never\s*mind|esque[cç]a(?:\s+isso)?|esquece(?:\s+isso)?|deixa\s+(?:pra|para)\s+l[aá]|deixa\s+quieto)\s*[.!?]*\s*$";

pub const REMEMBER_QUESTION: &str = r"(?i)^(?:(?:please|por\s+favor)[,]?\s+)?(?:remember|lembra(?:-te)?|lembre(?:-se)?|lembras|voc[eê]\s+lembra|remind\s+me|me\s+lembr[ae]|lembra-me|lembre-me)\s+(?:of|about|what|when|where|who|whose|which|how|whether|if|qual|quais|quando|onde|quem|como|o\s+que|se|do|da|de|to)\b";
const UPDATED: &str = "; updated ";

fn cue(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).expect("cue pattern")
}

fn strip_cue(pattern: &str, text: &str) -> String {
    let t = text.trim();
    cue(pattern).find(t).map_or_else(|| t.to_string(), |m| t[m.end()..].trim().to_string())
}

fn bare_cue(pattern: &str, text: &str) -> bool {
    let probe = format!("{} ", text.trim());
    cue(pattern).find(&probe).is_some_and(|m| m.end() == probe.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgetCue {
    Forget(String),
    Dismiss,

    None,
}

pub fn forget_cue(utterance: &str) -> ForgetCue {
    let t = utterance.trim();
    if cue(REMEMBER_QUESTION).is_match(t) || cue(REMEMBER_CUE).is_match(t) {
        return ForgetCue::None;
    }
    if cue(FORGET_DISMISS).is_match(t) {
        return ForgetCue::Dismiss;
    }
    if cue(CORRECT_CUE).is_match(t) {
        return ForgetCue::None;
    }
    let tidy = |rest: &str| {
        let rest = rest.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?')).trim();
        if rest.is_empty() { ForgetCue::Dismiss } else { ForgetCue::Forget(rest.to_string()) }
    };
    if let Some(m) = cue(FORGET_CUE).find(t) {
        return tidy(&t[m.end()..]);
    }
    match cue(FORGET_CUE_RC5).captures(t) {

        Some(c) if c.name("anymore").is_some() => tidy(c.name("anymore").unwrap().as_str()),
        Some(c) => tidy(&t[c.get(0).unwrap().end()..]),
        None => ForgetCue::None,
    }
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "my", "mine", "me", "i", "is", "are", "was", "of", "to", "in", "on", "at", "for", "and", "or", "that", "this", "it", "its", "it's", "about", "old", "new", "o", "os", "as",
    "um", "uma", "de", "do", "da", "dos", "das", "e", "é", "que", "meu", "minha", "meus", "minhas", "no", "na", "em", "para", "pra", "sobre", "isso", "esse", "essa",
];

fn match_words(text: &str) -> Vec<String> {
    let fold = |c: char| match c {
        'á' | 'à' | 'â' | 'ã' => 'a',
        'é' | 'ê' => 'e',
        'í' => 'i',
        'ó' | 'ô' | 'õ' => 'o',
        'ú' | 'ü' => 'u',
        'ç' => 'c',
        c => c,
    };
    let lower = text.to_lowercase();
    lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.trim_matches('\'').trim_end_matches("'s"))
        .filter(|w| w.chars().count() > 1 && !STOP_WORDS.contains(w))
        .map(|w| w.chars().map(fold).collect())
        .collect()
}

pub fn forget_candidates<'a>(what: &str, saved: &'a [Candidate]) -> Vec<&'a Candidate> {
    let want = match_words(what);
    if want.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, usize, &Candidate)> = saved
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let have = match_words(&c.text);
            let shared = want.iter().filter(|w| have.contains(w)).count();
            let score = shared as f64 / want.len() as f64;
            (score >= 0.5).then_some((score, i, c))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
    scored.into_iter().take(3).map(|(_, _, c)| c).collect()
}

pub fn has_correct_cue(text: &str) -> bool {
    cue(CORRECT_CUE).is_match(text.trim())
}

pub fn has_remember_cue(text: &str) -> bool {
    cue(REMEMBER_CUE).is_match(text.trim())
}

pub fn is_remember_question(text: &str) -> bool {
    cue(REMEMBER_QUESTION).is_match(text.trim())
}

pub fn remember_content(utterance: &str) -> String {
    let rest = strip_cue(REMEMBER_CUE, utterance);
    if bare_cue(REMEMBER_CUE, utterance) || bare_cue(REMEMBER_CUE, &rest) {
        return String::new();
    }
    cut(&rest)
}

pub const SELF_FACT: &str = r"(?i)^(?:(?:good\s+morning|bom\s+dia|boa\s+tarde|boa\s+noite|hi|hey|hello|ok|okay|so|well|oi|ol[aá]|bom|ent[aã]o)\s*[,!.]?\s+)?(?:(?P<name>my\s+(?:full\s+)?name(?:\s+is|'s|’s)?|i'?m\s+called|(?:you\s+can\s+)?call\s+me|meu\s+nome\s+(?:completo\s+)?(?:[eé]|eh)|(?:eu\s+)?me\s+chamo|(?:voc[eê]\s+)?pode\s+me\s+chamar\s+de|me\s+chama\s+de|(?P<kin>(?:my|meu|minha)\s+(?:wife|husband|partner|sister|brother|mother|mom|mum|father|dad|son|daughter|girlfriend|boyfriend|dog|cat|irm[ãa]|irm[ãa]o|m[ãa]e|pai|filho|filha|esposa|marido|namorada|namorado|cachorro|cachorra|gato|gata)(?:'s\s+name\s+is|’s\s+name\s+is|\s+is\s+called|\s+is|\s+(?:se\s+)?chama|\s+[eé](?:\s+[ao])?)))|i\s+live\s+(?:in|at|on)|i\s+work\s+(?:at|for|in)|(?:eu\s+)?moro\s+(?:em|no|na|nos|nas)|(?:eu\s+)?trabalho\s+(?:em|no|na|nos|nas|para|pra)|(?P<prefer>i\s+prefer|(?:eu\s+)?prefiro))\s+(?P<value>\S.*)$";

const ASKS_MY_NAME: &str = r"(?i)\b(?:what'?s|what\s+is|whats|do\s+you\s+know|you\s+know|remember)\s+my\s+(?:full\s+)?name\b|\bwhat\s+my\s+(?:full\s+)?name\s+is\b|\bqual\s+(?:[eé]\s+)?(?:o\s+)?meu\s+nome\b|\bsabe\s+(?:o\s+)?meu\s+nome\b|\bcomo\s+(?:[eé]\s+que\s+)?eu\s+me\s+chamo\b";

const NAME_OPENER: &str = r"(?i)^(?:i'?m|i’m|i\s+am|it'?s|it’s|(?:(?:eu\s+)?sou|é)(?:\s+[oa])?)\s+";

const NOT_NAMED_AFTER_OPENER: &[&str] = &[
    "american", "brazilian", "british", "english", "portuguese", "spanish", "french", "italian", "canadian", "mexican", "argentinian", "argentine", "chilean", "colombian", "peruvian",
    "venezuelan", "uruguayan", "indian", "chinese", "japanese", "korean", "australian", "irish", "scottish", "welsh", "dutch", "swedish", "norwegian", "finnish", "polish", "russian",
    "ukrainian", "turkish", "greek", "israeli", "african", "european", "asian", "latino", "latina", "catholic", "protestant", "evangelical", "jewish", "muslim", "buddhist", "hindu",
    "atheist", "agnostic", "mormon",
];

const ASKS_YOUR_NAME: &str = r"(?i)\bwhat'?s\s+your\s+(?:full\s+)?name\b|\bwhat\s+is\s+your\s+(?:full\s+)?name\b|\bwhat\s+should\s+i\s+call\s+you\b|\bqual\s+(?:[eé]\s+)?(?:o\s+)?seu\s+nome\b|\bcomo\s+(?:voc[eê]\s+se\s+chama|posso\s+te\s+chamar|devo\s+te\s+chamar|te\s+chamo)\b";

const NOT_A_NAME: &[&str] = &[
    "hard", "weird", "long", "short", "difficult", "easy", "common", "rare", "unusual", "spelled", "pronounced", "written", "the", "a", "an", "so", "too", "very", "kind", "pretty", "funny", "actually", "really",
    "also", "still", "just", "complicated", "secret", "private", "is", "later", "back", "now", "when", "if", "at", "on", "in", "after", "before", "tomorrow", "tonight", "today", "soon", "asap",
    "again", "anytime", "whenever", "please", "maybe", "depois", "amanhã", "amanha", "hoje", "quando", "agora", "mais", "would", "could", "might", "will", "should", "was",
    "used", "seria", "era", "difícil", "dificil", "estranho", "comprido", "grande", "curto", "complicado", "muito", "bem", "meio", "bastante", "um", "uma", "igual", "parecido", "secreto",
];

const CHOICE_WORDS: &[&str] = &[
    "first", "second", "third", "last", "option", "one", "this", "that", "these", "those", "other", "primeira", "primeiro", "segunda", "segundo", "terceira", "terceiro", "última", "último", "opção",
    "opcao", "esse", "essa", "este", "esta", "isso", "isto", "aquele", "aquela", "outra", "outro",
];

const PREFER_REFS: &[&str] = &["it", "them", "ones", "way", "former", "latter", "assim", "jeito", "forma"];

const NOT_LASTING: &[&str] = &[
    "if", "unless", "would", "could", "might", "maybe", "perhaps", "someday", "se", "caso", "talvez", "seria", "today", "tonight", "tomorrow", "yesterday", "hoje", "amanhã", "amanha", "ontem", "said",
    "says", "disse", "diz", "falou", "which", "qual", "quais",
];

const NOT_LASTING_PHRASES: &[&str] = &[
    "this week", "this weekend", "this month", "this year", "for now", "right now", "for a while", "esta semana", "essa semana", "este mês", "esse mês", "este ano", "por enquanto", "por agora",
    "fim de semana",
];

const NOT_IN_NAME: &[&str] = &[
    "by", "for", "to", "from", "with", "of", "and", "or", "but", "as", "in", "the", "at", "on", "an", "por", "para", "pra", "com", "sem", "no", "na", "nos", "nas", "ou", "mas", "mesmo", "here", "there",
    "aqui", "morning", "afternoon", "evening", "night", "manhã", "noite",
];

const LAUGHTER: &str = r"(?i)^(?:k{2,}|(?:rs)+|(?:h[aeiou]+){2,}h?|(?:[aeiou]h){2,}|l+o+l+|lmao|rofl|xd+|h+u*m+|u+[hm]+|w+o+w+|o+h+|a+h+|af+s?|vish|oxe(?:nte)?)$";

const DONT_KNOW: &str = r"(?i)\bdon'?t\s+know\b|\bhaven'?t\s+told\b|\bnot\s+sure\b|\bn[ãa]o\s+sei\b|\bn[ãa]o\s+(?:me\s+)?(?:contou|disse|falou)\b|\bainda\s+n[ãa]o\b";

const NOT_NAME_WORDS: &[&str] = &[
    "yes", "yeah", "yep", "no", "nope", "ok", "okay", "idk", "dunno", "why", "what", "who", "how", "lol", "haha", "hi", "hello", "hey", "thanks", "sure", "maybe", "nothing", "me", "you", "i", "my", "it", "that",
    "this", "tell", "know", "please", "guess", "sim", "não", "nao", "oi", "olá", "ola", "obrigado", "obrigada", "talvez", "nada", "eu", "você", "voce", "isso", "quem", "qual", "por", "que", "sei", "sabe", "kkk", "rs",
    "adivinha", "cool", "whatever", "beleza", "correct", "not", "nah", "right", "wrong", "nice", "great", "nobody", "ninguém", "ninguem", "hmm", "hm", "ah", "oh", "eita", "uai", "ué", "ue",
    "correto", "certo", "errado", "legal", "show", "blz", "vlw", "tchau", "bye", "later", "now", "depois", "agora", "sério", "serio", "tanto", "faz",

    "sorry", "wait", "awesome", "secret", "skip", "pass", "anonymous", "exactly", "yup", "yea", "ya", "aha", "good", "fine", "nevermind", "nvm", "none", "oops", "huh", "uh", "thx", "test", "testing",
    "brb", "whoops", "go", "nossa", "caramba", "chuta", "fala", "vai",
    "morning", "evening", "night", "entendi", "desculpa", "desculpe", "espera", "pera", "peraí", "perai", "segredo", "anônimo", "anonimo", "anônima", "anonima", "pular", "passo", "exato", "exatamente",
    "claro", "valeu", "opa", "ops", "pois", "tá", "ta", "massa", "top", "demais", "ótimo", "otimo", "perfeito", "obg", "teste", "bom", "boa", "tudo", "pode", "ser", "dia", "tarde", "noite",
];

fn is_name(value: &str, strict: bool) -> bool {
    let words: Vec<&str> = value.trim().trim_end_matches(['.', '!']).split_whitespace().collect();
    let letters = |w: &str| w.chars().next().is_some_and(char::is_alphabetic) && w.chars().all(|c| c.is_alphabetic() || matches!(c, '-' | '\'' | '’'));
    let linking = |w: &str| {
        let l = w.to_lowercase();
        NOT_IN_NAME.contains(&l.as_str()) || cue(LAUGHTER).is_match(&l)
    };
    let Some(first) = words.first() else { return false };
    let lower = first.to_lowercase();
    let common = NOT_NAME_WORDS.contains(&lower.as_str()) || (NOT_A_NAME.contains(&lower.as_str()) && (strict || !first.starts_with(char::is_uppercase)));
    words.len() <= 6 && !common && words.iter().all(|w| letters(w) && !linking(w))
}

fn name_end(value: &str) -> usize {
    let lower = value.to_ascii_lowercase();
    let mut end = value.find(['.', '!', '?', ',', ';', ':']).unwrap_or(value.len());
    for joined in [" and ", " e ", " but ", " mas "] {
        if let Some(i) = lower.find(joined) {
            end = end.min(i);
        }
    }
    value[..end].trim_end_matches(|c: char| !c.is_alphabetic()).len()
}

fn first_sentence(value: &str) -> &str {
    value.char_indices().find(|&(i, c)| matches!(c, '.' | '!' | '?') && value[i + c.len_utf8()..].starts_with(char::is_whitespace)).map_or(value, |(i, c)| &value[..i + c.len_utf8()])
}

fn value_words(value: &str) -> Vec<String> {
    value.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’')).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

const POLITE_REMEMBER: &str = r"(?i)^(?:(?:can|could|will|would)\s+you\s+(?:please\s+)?remember|(?:voc[eê]\s+)?(?:pode|poderia|consegue)\s+(?:se\s+)?lembrar)\s+(?:that|que|de\s+que)\s+(?P<rest>\S.*?)\s*\?$";

fn statement(s: &str) -> Option<&str> {
    if let Some(rest) = cue(POLITE_REMEMBER).captures(s).and_then(|c| c.name("rest")) {
        return Some(rest.as_str());
    }
    if s.ends_with('?') { s.rfind(',').map(|i| s[..i].trim()).filter(|p| !crate::blocks::is_question(p)) } else { Some(s) }
}

fn first_person(s: &str) -> bool {
    s.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’')).any(|w| FIRST_PERSON.contains(&w.replace('’', "'").as_str()))
}

const PASSING: &[&str] = &[
    "tired", "exhausted", "hungry", "sleepy", "bored", "busy", "sick", "stressed", "cansado", "cansada", "exausto", "exausta", "fome", "sono", "entediado", "entediada", "ocupado", "ocupada", "doente",
    "estressado", "estressada",
];

fn negated(text: &str) -> bool {
    text.to_lowercase().split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’')).any(|w| ["not", "never", "nao", "não", "nunca", "nem"].contains(&w) || w.ends_with("n't") || w.ends_with("n’t"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Saves {
    Asked,
    Realtime,
}

impl Saves {

    pub fn parse(v: Option<&str>) -> Result<Saves, String> {
        match v.filter(|v| !v.is_empty()) {
            None | Some("realtime") => Ok(Saves::Realtime),
            Some("asked") => Ok(Saves::Asked),
            Some(other) => Err(format!("unknown RTH_MEMORY_DECIDE {other:?} (expected realtime or asked)")),
        }
    }

    pub fn instructions(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Saves::Realtime => (INSTRUCTIONS, INSTRUCTIONS_SHA256, PART),
            Saves::Asked => (INSTRUCTIONS_ASKED, INSTRUCTIONS_ASKED_SHA256, PART_ASKED),
        }
    }
}

const FIRST_PERSON: &[&str] = &["my", "mine", "i", "i'm", "i've", "im", "meu", "minha", "meus", "minhas", "eu", "moro", "trabalho", "prefiro", "chamo", "sou"];

pub fn fact_text(words: &str, explicit: bool) -> Option<String> {
    let t = words.trim();
    if explicit {
        return Some(cut(&frame_name(t, true).unwrap_or_else(|| t.to_string()))).filter(|s| !s.is_empty());
    }
    let mut sentences = Vec::new();
    let mut start = 0;
    for (i, c) in t.char_indices() {
        if matches!(c, '.' | '!' | '?') && t[i + c.len_utf8()..].starts_with(char::is_whitespace) {
            sentences.push(t[start..i + c.len_utf8()].trim());
            start = i + c.len_utf8();
        }
    }
    sentences.push(t[start..].trim());
    let statements: Vec<&str> = sentences.iter().copied().filter_map(statement).filter(|s| !s.is_empty() && !crate::blocks::is_question(s) && !small_talk(s)).collect();
    let passing = |s: &str| value_words(s).iter().any(|w| PASSING.contains(&w.as_str()));
    let facts: Vec<&str> = statements.iter().copied().filter(|s| self_fact(s).is_some()).collect();
    let mine: Vec<&str> = statements.iter().copied().filter(|s| first_person(s) && !passing(s)).collect();
    let others: Vec<&str> = statements.iter().copied().filter(|s| !first_person(s) && !passing(s)).collect();

    let kept = if !facts.is_empty() { facts } else if !mine.is_empty() { mine } else if !others.is_empty() { others } else { statements };
    let joined = kept.join(" ");
    let text = cue(GREETING).replace(&joined, "").trim().to_string();

    let greeted = text.len() < joined.trim().len();
    if text.is_empty() || small_talk(&text) || (greeted && text.split_whitespace().count() < 2) {
        return None;
    }
    Some(cut(&frame_name(&text, false).unwrap_or(text)))
}

const SMALL_TALK: &str = r"oi|ol[aá]|al[oô]|e\s+a[ií]|bom\s+dia|boa\s+tarde|boa\s+noite|tudo\s+(?:bem|bom|certo)|beleza|como\s+vai|(?:voc[eê]\s+)?(?:t[aá]|est[aá])\s+me\s+(?:ouvindo|escutando)|(?:voc[eê]\s+)?consegue\s+me\s+(?:ouvir|escutar)|me\s+(?:ouve|escuta)|t[aá]\s+funcionando|testando|obrigad[oa]|valeu|hi|hello|hey|good\s+(?:morning|afternoon|evening)|how\s+are\s+you|how'?s\s+it\s+going|what'?s\s+up|(?:can|do)\s+you\s+hear\s+me|(?:are\s+)?you\s+there|is\s+this\s+working|testing|thanks|thank\s+you";

pub fn small_talk(text: &str) -> bool {
    let words: String = text.chars().map(|c| if matches!(c, ',' | '.' | '!' | '?' | '…' | ';' | ':') { ' ' } else { c }).collect();
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    !words.is_empty() && cue(&format!(r"(?i)^(?:{SMALL_TALK})(?:\s+(?:e\s+|and\s+)?(?:{SMALL_TALK}))*$")).is_match(&words)
}

const GREETING: &str = r"(?i)^(?:good\s+morning|bom\s+dia|boa\s+tarde|boa\s+noite|hi|hey|hello|ok|okay|so|well|oi|ol[aá]|bom|ent[aã]o)(?:\s*[,!.]+\s*|\s+)";

fn frame_name(text: &str, whole: bool) -> Option<String> {
    let c = cue(r"(?i)^(?:my\s+(?P<full>full\s+)?name(?:\s+is|'s|’s)?|i'?m\s+called|(?:eu\s+)?me\s+chamo)\s+(?P<value>\S.*)$").captures(text)?;
    let value = c.name("value").map_or("", |v| v.as_str());
    let (name, after) = value.split_at(name_end(value));
    if !is_name(name, false) {
        return None;
    }
    let full = if c.name("full").is_some() { "full " } else { "" };
    let lower = text.to_lowercase();
    let pt = lower.starts_with("me chamo") || lower.starts_with("eu me chamo");
    let said = after.trim_start_matches(|c: char| !c.is_alphanumeric());
    let after = if whole || said.is_empty() || first_person(said) { after } else { "" };
    Some(if pt { format!("meu nome é {name}{after}") } else { format!("my {full}name is {name}{after}") })
}

pub fn self_fact(utterance: &str) -> Option<String> {
    let rest = strip_cue(CORRECT_CUE, &strip_cue(REMEMBER_CUE, utterance));

    if rest.contains('?') || negated(&rest) || rest.contains(['"', '“', '”', '«', '»']) {
        return None;
    }
    let c = cue(SELF_FACT).captures(&rest)?;
    let value = c.name("value").map_or("", |v| v.as_str());
    let words = value_words(first_sentence(value));

    let spaced = format!(" {} ", words.join(" "));
    let marker = |(i, w): (usize, &String)| NOT_LASTING.contains(&w.as_str()) && !(w == "se" && words.get(i + 1).is_some_and(|n| n.starts_with("cham")));
    if words.iter().enumerate().any(marker) || NOT_LASTING_PHRASES.iter().any(|p| spaced.contains(&format!(" {p} "))) {
        return None;
    }
    let raw_first = value.split_whitespace().next().unwrap_or("").trim_matches(|ch: char| !ch.is_alphanumeric());
    let first = raw_first.to_lowercase();
    if c.name("kin").is_some() {

        if NOT_A_NAME.contains(&first.as_str()) || !raw_first.starts_with(char::is_uppercase) {
            return None;
        }
    } else if c.name("name").is_some() {
        let prefix = rest[..c.name("value").map_or(0, |v| v.start())].trim_end().to_lowercase();
        let copula = [" is", "'s", "’s", " é", " e", " eh", "chamo"].iter().any(|k| prefix.ends_with(k));

        if !is_name(&value[..name_end(value)], !copula) || (!copula && !raw_first.starts_with(char::is_uppercase)) {
            return None;
        }
    } else if raw_first.contains(|ch: char| ch.is_ascii_digit()) || words.iter().take(2).any(|w| CHOICE_WORDS.contains(&w.as_str())) {

        return None;
    } else if c.name("prefer").is_some() && words.iter().any(|w| CHOICE_WORDS.contains(&w.as_str()) || PREFER_REFS.contains(&w.as_str())) {

        return None;
    }
    Some(cut(&rest))
}

pub fn name_answer(utterance: &str, asked_by_user: Option<&str>, last_reply: Option<&str>) -> Option<String> {

    let reply_admits = last_reply.is_some_and(|r| cue(DONT_KNOW).is_match(r));
    let question = last_reply.filter(|r| ends_asking_your_name(r)).or(asked_by_user.filter(|q| reply_admits && cue(ASKS_MY_NAME).is_match(q)))?;
    if negated(utterance) {
        return None;
    }
    let name = utterance.trim().trim_end_matches(['.', '!']).trim();

    let stripped = cue(NAME_OPENER).replace(name, "");
    let opened = stripped.len() != name.len();
    let name = stripped.trim();
    let first = name.split_whitespace().next().unwrap_or("").to_lowercase();
    if opened && (!name.starts_with(char::is_uppercase) || NOT_NAMED_AFTER_OPENER.contains(&first.as_str()) || first.ends_with("-based")) {
        return None;
    }
    let words: Vec<&str> = name.split_whitespace().collect();

    let common_later = words.iter().skip(1).any(|w| w.starts_with(char::is_lowercase) && [NOT_NAME_WORDS, NOT_A_NAME].iter().any(|l| l.contains(&w.to_lowercase().as_str())));
    if words.len() > 3 || name.chars().count() > 40 || !is_name(name, true) || common_later {
        return None;
    }
    let subject = if local::detect_language(local::genuine_utterance(question)) == Some(local::Lang::Pt) { "meu nome é" } else { "my name is" };
    Some(cut(&format!("{subject} {}", words.join(" "))))
}

fn ends_asking_your_name(reply: &str) -> bool {
    let Some(q) = reply.rfind('?') else { return false };
    if reply[q + 1..].chars().any(char::is_alphanumeric) {
        return false;
    }
    let start = reply[..q].rfind(['.', '!', '?', '\n']).map_or(0, |i| i + 1);
    cue(ASKS_YOUR_NAME).is_match(&reply[start..=q])
}

pub fn correction(text: &str) -> Option<(&str, &str, &str)> {
    let i = text.rfind(UPDATED)?;
    let (date, current) = text[i + UPDATED.len()..].split_once(": ")?;
    Some((text[..i].trim(), date, current.trim()))
}

pub fn correct_content(previous: &str, utterance: &str, date: &str) -> String {
    let base = previous.rfind(UPDATED).map_or(previous, |i| &previous[..i]).trim();
    let tail = format!("{UPDATED}{date}: {}", strip_cue(CORRECT_CUE, utterance));
    let room = MAX_PROPOSAL_CHARS.saturating_sub(tail.chars().count());
    cut(&format!("{}{tail}", base.chars().take(room).collect::<String>()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub memory_id: String,
    pub revision: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    NoOp,

    Recall(Vec<(String, u64)>),
    Remember { text: String },
    Correct { text: String, target: (String, u64) },
}

impl Decision {
    pub fn name(&self) -> &'static str {
        match self {
            Decision::NoOp => "no_op",
            Decision::Recall(_) => "recall",
            Decision::Remember { .. } => "remember",
            Decision::Correct { .. } => "correct",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fallback {
    Timeout,
    Invalid(String),
    Unavailable(String),
}

impl Fallback {
    pub fn reason(&self) -> &'static str {
        match self {
            Fallback::Timeout => "decide_timeout",
            Fallback::Invalid(_) => "decide_invalid",
            Fallback::Unavailable(_) => "decide_unavailable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Run {
    pub outcome: Result<Decision, Fallback>,
    pub ms: u64,
    pub model_text: Option<String>,
}

fn cut(s: &str) -> String {
    s.chars().take(MAX_PROPOSAL_CHARS).collect()
}

pub fn offer(recalled: &[Candidate]) -> Vec<Candidate> {
    recalled.iter().take(MAX_ITEMS).map(|c| Candidate { text: cut(&c.text), ..c.clone() }).collect()
}

pub fn render_user(utterance: &str, offered: &[Candidate]) -> String {
    let candidates: Vec<Value> = offered.iter().map(|c| json!({"memory_id": c.memory_id, "revision": c.revision, "text": c.text})).collect();

    format!("{{\"candidates\":{},\"utterance\":{}}}", Value::Array(candidates), Value::String(utterance.to_string()))
}

fn lit(x: &str) -> String {
    format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))
}

fn subsets(ids: &[String]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for n in 1..=ids.len().min(MAX_ITEMS) {
        let mut idx: Vec<usize> = (0..n).collect();
        loop {
            out.push(idx.iter().map(|&i| ids[i].clone()).collect());
            let mut i = n;
            while i > 0 && idx[i - 1] == ids.len() - n + i - 1 {
                i -= 1;
            }
            if i == 0 {
                break;
            }
            idx[i - 1] += 1;
            for j in i..n {
                idx[j] = idx[j - 1] + 1;
            }
        }
    }
    out
}

pub fn grammar(offered: &[Candidate]) -> String {
    let ids: Vec<String> = offered.iter().map(|c| c.memory_id.clone()).collect();
    let mut br = vec![lit(r#"{"d":"no_op"}"#), lit(r#"{"d":"remember"}"#)];
    if !ids.is_empty() {
        let sel: Vec<String> = subsets(&ids).iter().map(|s| lit(&serde_json::to_string(s).unwrap())).collect();
        br.insert(0, format!("\"{{\\\"d\\\":\\\"recall\\\",\\\"s\\\":\" ({}) \"}}\"", sel.join(" | ")));
        let tg: Vec<String> = ids.iter().map(|i| lit(&serde_json::to_string(i).unwrap())).collect();
        br.push(format!("\"{{\\\"d\\\":\\\"correct\\\",\\\"t\\\":\" ({}) \"}}\"", tg.join(" | ")));
    }
    format!("root ::= {}\n", br.join(" | "))
}

pub fn choices(offered: &[Candidate]) -> Vec<Value> {
    let ids: Vec<String> = offered.iter().map(|c| c.memory_id.clone()).collect();
    let mut out: Vec<Value> = Vec::new();
    if !ids.is_empty() {
        out.extend(subsets(&ids).into_iter().map(|s| json!({"d": "recall", "s": s})));
    }
    out.push(json!({"d": "no_op"}));
    out.push(json!({"d": "remember"}));
    out.extend(ids.iter().map(|i| json!({"d": "correct", "t": i})));
    out
}

pub fn schema(offered: &[Candidate]) -> Value {
    json!({"type": "json_schema", "json_schema": {"name": "out", "schema": {"enum": choices(offered)}, "strict": true}})
}

pub fn decide_from(answer: &str, utterance: &str, offered: &[Candidate]) -> Result<Decision, String> {
    let v: Value = serde_json::from_str(answer).map_err(|_| "not json".to_string())?;
    let o = v.as_object().ok_or("not an object")?;
    let rev = |id: &str| offered.iter().find(|c| c.memory_id == id).map(|c| c.revision);
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    let words = cut(utterance.trim());
    let proposal = || if words.is_empty() { Err("empty utterance".to_string()) } else { Ok(words.clone()) };

    let question = cue(REMEMBER_QUESTION).is_match(utterance.trim());
    let lone_correction = offered.is_empty() && cue(CORRECT_CUE).is_match(utterance.trim());
    match o.get("d").and_then(Value::as_str) {
        Some("no_op") if keys == ["d"] => Ok(Decision::NoOp),
        Some("remember") if keys == ["d"] && (question || lone_correction || (!has_remember_cue(utterance) && small_talk(utterance))) => Ok(Decision::NoOp),
        Some("correct") if keys.len() == 2 && o.contains_key("t") && question => {
            o["t"].as_str().and_then(rev).ok_or("correct target not offered")?;
            Ok(Decision::NoOp)
        }
        Some("remember") if keys == ["d"] => Ok(Decision::Remember { text: proposal().map(|w| remember_content(&w)).and_then(|w| if w.is_empty() { Err("nothing left after the cue".to_string()) } else { Ok(w) })? }),
        Some("recall") if keys.len() == 2 && o.contains_key("s") => {
            let s = o["s"].as_array().ok_or("s not a list")?;
            let mut pairs = Vec::new();
            for id in s {
                let id = id.as_str().ok_or("id not a string")?;
                let r = rev(id).ok_or("recall id not offered")?;
                if pairs.iter().any(|(p, _): &(String, u64)| p == id) {
                    return Err("duplicate recall id".into());
                }
                pairs.push((id.to_string(), r));
            }
            if pairs.is_empty() || pairs.len() > MAX_ITEMS {
                return Err("recall ids".into());
            }
            Ok(Decision::Recall(pairs))
        }
        Some("correct") if keys.len() == 2 && o.contains_key("t") => {
            let t = o["t"].as_str().ok_or("t not a string")?;
            let r = rev(t).ok_or("correct target not offered")?;
            Ok(Decision::Correct { text: proposal()?, target: (t.to_string(), r) })
        }
        _ => Err("unknown d or extra keys".into()),
    }
}

const INTRO_START: &str = r"(?i:(?:(?:hi|hey|hello|oi|ol[aá])[,!]?\s+)?(?:this\s+is|meet|est[ae]\s+[ée]|ess[ae]\s+[ée]|apresento)\s+(?:(?:a|o)\s+)?)";
const RELATIONS: &str = r"(?i:sister|brother|mother|mom|mum|father|dad|wife|husband|son|daughter|friend|partner|girlfriend|boyfriend|aunt|uncle|cousin|grandma|grandmother|grandpa|grandfather|niece|nephew|colleague|roommate|neighbou?r|boss|irm[ãa]o?|m[ãa]e|pai|esposa|esposo|marido|mulher|filh[ao]|amig[ao]|namorad[ao]|ti[ao]|prim[ao]|av[óô]|sobrinh[ao]|colega|vizinh[ao]|chefe|sogr[ao]|cunhad[ao])";

const TITLES: &str = r"(?:Dr|Dra|Mr|Mrs|Ms|Sr|Sra|Srta|Prof|Profa)";
const TITLE_WORDS: &[&str] = &["dr", "dra", "mr", "mrs", "ms", "sr", "sra", "srta", "prof", "profa"];

const TITLE_DOT: char = '\u{2024}';

const NOT_NAMES: &[&str] = &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december", "english", "portuguese", "spanish", "french", "german", "italian", "segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo", "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro", "inglês", "português", "espanhol", "francês", "alemão", "italiano"];

pub fn introduction(utterance: &str, not: &[String]) -> Option<String> {
    let name = format!(r"(?P<name>(?:{TITLES}{TITLE_DOT}?\s+)?\p{{Lu}}\p{{Ll}}+)");
    let tail = r"(?:\s*,?\s*(?i:say\s+(?:hi|hello)|diga\s+oi|diz\s+oi))?\s*[.!]*$";
    let forms = [format!(r"^{INTRO_START}(?i:my|minha|meu)\s+{RELATIONS}\s+{name}{tail}"), format!(r"^{INTRO_START}{name}{tail}")];
    let held = cue(&format!(r"\b({TITLES})\.")).replace_all(utterance, format!("${{1}}{TITLE_DOT}").as_str()).into_owned();
    cue(r"[^.!?]+[.!?]*").find_iter(&held).map(|s| s.as_str().trim()).filter(|s| !s.ends_with('?')).find_map(|sentence| {
        let found = forms.iter().find_map(|f| cue(f).captures(sentence).and_then(|c| c.name("name")).map(|n| n.as_str().replace(TITLE_DOT, ".")))?;

        let lower = found.rsplit(char::is_whitespace).next().unwrap_or(&found).to_lowercase();
        let own = not.iter().any(|n| n.split_whitespace().next().is_some_and(|f| f.to_lowercase() == lower));
        (!own && !NOT_NAMES.contains(&lower.as_str()) && !TITLE_WORDS.contains(&lower.as_str())).then_some(found)
    })
}

#[cfg(test)]
pub fn body(model: &str, utterance: &str, offered: &[Candidate], slot: Option<u32>) -> Value {
    body_with(INSTRUCTIONS, model, utterance, offered, slot)
}

pub fn body_with(instructions: &str, model: &str, utterance: &str, offered: &[Candidate], slot: Option<u32>) -> Value {
    let mut b = json!({
        "model": model,
        "messages": [{"role": "system", "content": instructions}, {"role": "user", "content": render_user(utterance, offered)}],
        "temperature": 0,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "chat_template_kwargs": {"enable_thinking": false},
        "grammar": grammar(offered),
        "cache_prompt": true,
    });
    if let Some(s) = slot {
        b["id_slot"] = json!(s);
    }
    b
}

#[cfg(test)]
pub fn body_flash(model: &str, utterance: &str, offered: &[Candidate]) -> Value {
    body_flash_with(INSTRUCTIONS, model, utterance, offered)
}

pub fn body_flash_with(instructions: &str, model: &str, utterance: &str, offered: &[Candidate]) -> Value {
    let mut b = json!({
        "model": model,
        "messages": [{"role": "system", "content": instructions}, {"role": "user", "content": render_user(utterance, offered)}],
        "temperature": 0,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "response_format": schema(offered),
    });
    crate::route::Level::Off.deepseek_fields(&mut b);
    b
}

#[cfg(test)]
pub async fn run(cfg: &local::Config, slot: Option<u32>, utterance: &str, recalled: &[Candidate], deadline: Duration) -> Run {
    run_with(Saves::Realtime, cfg, slot, utterance, recalled, deadline).await
}

pub async fn run_with(saves: Saves, cfg: &local::Config, slot: Option<u32>, utterance: &str, recalled: &[Candidate], deadline: Duration) -> Run {
    let (instructions, sha, _) = saves.instructions();
    let t = Instant::now();
    let ms = |t: Instant| t.elapsed().as_millis() as u64;
    if crate::backend::text_sha256(instructions) != sha {
        return Run { outcome: Err(Fallback::Invalid("instructions sha256 differs from the frozen file".into())), ms: 0, model_text: None };
    }
    let offered = offer(recalled);
    let path = format!("{}/chat/completions", cfg.base_path);
    let request = if local::dial(&cfg.authority).is_hosted() { body_flash_with(instructions, &cfg.model, utterance, &offered) } else { body_with(instructions, &cfg.model, utterance, &offered, slot) };
    let (status, raw) = match local::post_json(&cfg.authority, &path, &request, deadline).await {
        Ok(r) => r,
        Err(f) if f.reason == local::FIRST_TOKEN_TIMEOUT => return Run { outcome: Err(Fallback::Timeout), ms: ms(t), model_text: None },
        Err(f) => return Run { outcome: Err(Fallback::Unavailable(format!("{}: {}", f.reason, f.detail))), ms: ms(t), model_text: None },
    };
    if status != 200 {
        return Run { outcome: Err(Fallback::Unavailable(format!("HTTP {status}"))), ms: ms(t), model_text: None };
    }
    let v: Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Run { outcome: Err(Fallback::Invalid(format!("response not json: {e}"))), ms: ms(t), model_text: None },
    };
    let choice = &v["choices"][0];
    let text = choice["message"]["content"].as_str().map(str::to_string);
    let outcome = match (&text, choice["finish_reason"].as_str()) {
        (Some(a), Some("stop")) => decide_from(a.trim(), utterance, &offered).map_err(Fallback::Invalid),
        (_, reason) => Err(Fallback::Invalid(format!("not completed: {}", reason.unwrap_or("missing")))),
    };
    Run { outcome, ms: ms(t), model_text: text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, rev: u64, text: &str) -> Candidate {
        Candidate { memory_id: id.into(), revision: rev, text: text.into() }
    }

    #[test]
    fn the_bundled_instructions_are_the_frozen_file() {
        assert_eq!(crate::backend::text_sha256(INSTRUCTIONS), INSTRUCTIONS_SHA256);
    }

    fn language(g: &str) -> Vec<String> {
        let rule = g.strip_prefix("root ::= ").unwrap().trim_end();
        let mut toks: Vec<String> = Vec::new();
        let mut it = rule.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '"' => {
                    let mut lit = String::new();
                    while let Some(c) = it.next() {
                        match c {
                            '\\' => lit.push(it.next().unwrap()),
                            '"' => break,
                            c => lit.push(c),
                        }
                    }
                    toks.push(format!("L{lit}"));
                }
                '(' | ')' | '|' => toks.push(c.to_string()),
                _ => {}
            }
        }
        let (mut out, mut alt, mut group, mut depth): (Vec<String>, Vec<Vec<String>>, Vec<String>, u32) = (vec![], vec![vec![String::new()]], vec![], 0);
        for t in toks {
            match t.as_str() {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    let cur = alt.pop().unwrap();
                    alt.push(cur.iter().flat_map(|p| group.iter().map(move |g| format!("{p}{g}"))).collect());
                    group.clear();
                }
                "|" if depth == 0 => {
                    out.extend(alt.pop().unwrap());
                    alt.push(vec![String::new()]);
                }
                "|" => {}
                l if depth > 0 => group.push(l[1..].to_string()),
                l => {
                    let cur = alt.pop().unwrap();
                    alt.push(cur.into_iter().map(|p| format!("{p}{}", &l[1..])).collect());
                }
            }
        }
        out.extend(alt.pop().unwrap());
        out
    }

    #[test]
    fn the_flash_schema_is_the_grammar_s_language_exactly() {
        for n in 0..=4 {
            let offered: Vec<Candidate> = (1..=n).map(|i| c(&format!("rm-{i}"), 1, "x")).collect();
            let from_choices: Vec<String> = choices(&offered).iter().map(|v| v.to_string()).collect();
            assert_eq!(from_choices, language(&grammar(&offered)), "{n} offered");
            assert_eq!(schema(&offered)["json_schema"]["schema"]["enum"].as_array().unwrap().len(), from_choices.len());

            for v in &from_choices {
                assert!(decide_from(v, "u", &offered).is_ok(), "{v}");
            }
        }
        let b = body_flash(crate::flash::MODEL, "oi", &[]);
        assert_eq!(b["messages"][0]["content"].as_str(), Some(INSTRUCTIONS));
        assert!(b.get("grammar").is_none() && b.get("cache_prompt").is_none() && b.get("id_slot").is_none());
    }

    #[test]
    fn the_grammar_matches_the_benched_part_byte_for_byte() {
        let g = grammar(&[c("rm-1", 1, "x"), c("rm-2", 1, "y")]);
        assert_eq!(g, "root ::= \"{\\\"d\\\":\\\"recall\\\",\\\"s\\\":\" (\"[\\\"rm-1\\\"]\" | \"[\\\"rm-2\\\"]\" | \"[\\\"rm-1\\\",\\\"rm-2\\\"]\") \"}\" | \"{\\\"d\\\":\\\"no_op\\\"}\" | \"{\\\"d\\\":\\\"remember\\\"}\" | \"{\\\"d\\\":\\\"correct\\\",\\\"t\\\":\" (\"\\\"rm-1\\\"\" | \"\\\"rm-2\\\"\") \"}\"\n");
        assert_eq!(grammar(&[]), "root ::= \"{\\\"d\\\":\\\"no_op\\\"}\" | \"{\\\"d\\\":\\\"remember\\\"}\"\n");
        assert_eq!(subsets(&["a".into(), "b".into(), "c".into(), "d".into()]).len(), 4 + 6 + 4);
    }

    #[test]
    fn the_user_message_is_the_frozen_compact_shape() {
        let u = render_user("what's my favourite café?", &[c("m-1", 1, "remember that my favourite café is Lua")]);
        assert_eq!(u, r#"{"candidates":[{"memory_id":"m-1","revision":1,"text":"remember that my favourite café is Lua"}],"utterance":"what's my favourite café?"}"#);
        assert_eq!(render_user("oi", &[]), r#"{"candidates":[],"utterance":"oi"}"#);
    }

    #[test]
    fn a_remember_on_small_talk_alone_is_no_op() {
        let remember = |u: &str| decide_from(r#"{"d":"remember"}"#, u, &[]);
        for u in ["Oi tudo bem tá me ouvindo", "hi, can you hear me?", "oi, tudo bem?", "hello, are you there", "testando, testando"] {
            assert_eq!(remember(u), Ok(Decision::NoOp), "{u}");
        }
        for (u, kept) in [("remember that Ana prefers tea", "Ana prefers tea"), ("lembra que tudo bem é o nome da padaria", "tudo bem é o nome da padaria"), ("Ana prefere chá", "Ana prefere chá"), ("oi, eu moro em Lisboa", "oi, eu moro em Lisboa")] {
            assert_eq!(remember(u), Ok(Decision::Remember { text: kept.into() }), "{u}");
        }
    }

    #[test]
    fn a_message_that_states_nothing_has_no_fact_text() {
        for u in ["Oi tudo bem tá me ouvindo", "hi, can you hear me?", "Which one", "oi!", "obrigado, valeu", "can you remember what my name is?"] {
            assert_eq!(fact_text(u, false), None, "{u}");
        }
        assert_eq!(fact_text("I'm tired.", false).as_deref(), Some("I'm tired."), "a passing state alone is still a statement");
        assert_eq!(fact_text("can you remember that my cat is Lua?", false).as_deref(), Some("my cat is Lua"));
        assert_eq!(fact_text("Ana prefere chá", false).as_deref(), Some("Ana prefere chá"));
        assert_eq!(fact_text("hi, can you hear me? My sister is Ana.", false).as_deref(), Some("My sister is Ana."));
        assert_eq!(fact_text("remember", true), Some("remember".into()), "an explicit ask is the user's choice");
    }

    #[test]
    fn a_polite_ask_without_that_and_a_greeting_before_one_word_state_nothing() {
        let saved: Vec<(&str, Option<String>)> = ["can you remember my name?", "você pode lembrar meu nome?", "could you remember to call mom tomorrow?", "você consegue lembrar meu aniversário?", "hello there", "Boa noite, Ana"]
            .into_iter()
            .map(|u| (u, fact_text(u, false)))
            .filter(|(_, f)| f.is_some())
            .collect();
        assert!(saved.is_empty(), "{saved:?}");
        assert_eq!(fact_text("can you remember that my cat is Lua?", false).as_deref(), Some("my cat is Lua"));
        assert_eq!(fact_text("você pode lembrar que meu gato é o Lua?", false).as_deref(), Some("meu gato é o Lua"));
        assert_eq!(fact_text("Boa noite, eu moro em Lisboa", false).as_deref(), Some("eu moro em Lisboa"));
    }

    #[test]
    fn small_talk_is_only_greetings_check_ins_and_thanks() {
        for u in ["Oi tudo bem tá me ouvindo", "oi, tudo bem?", "Olá! Você consegue me ouvir?", "e aí, beleza", "hi, can you hear me?", "hello, are you there", "testing, testing", "thanks", "bom dia e obrigado"] {
            assert!(small_talk(u), "{u}");
        }
        for u in ["oi, eu moro em Lisboa", "tudo bem é o nome da padaria", "hello kitty is my cat", "thanks to Ana I moved", "hi, my name is Ana", "", "testing the new bike"] {
            assert!(!small_talk(u), "{u}");
        }
    }

    #[test]
    fn answers_map_to_decisions_with_offered_revisions_and_the_users_words() {
        let o = [c("m-1", 2, "x"), c("m-2", 1, "y")];
        assert_eq!(decide_from(r#"{"d":"no_op"}"#, "u", &o), Ok(Decision::NoOp));
        assert_eq!(decide_from(r#"{"d":"remember"}"#, " remember that Ana prefers tea ", &o), Ok(Decision::Remember { text: "Ana prefers tea".into() }));
        assert!(decide_from(r#"{"d":"remember"}"#, "remember that ", &o).is_err(), "a bare cue saves nothing");
        assert_eq!(decide_from(r#"{"d":"recall","s":["m-2","m-1"]}"#, "u", &o), Ok(Decision::Recall(vec![("m-2".into(), 1), ("m-1".into(), 2)])));
        assert_eq!(decide_from(r#"{"d":"correct","t":"m-1"}"#, "actually it's Sol now", &o), Ok(Decision::Correct { text: "actually it's Sol now".into(), target: ("m-1".into(), 2) }));
        let long = "x".repeat(400);
        assert_eq!(decide_from(r#"{"d":"remember"}"#, &long, &o), Ok(Decision::Remember { text: "x".repeat(MAX_PROPOSAL_CHARS) }));
    }

    #[test]
    fn content_rules_match_pa_fixtures() {
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/turn-remember-fixtures.json")).unwrap();
        let rules = &f["content_rules"];

        let memory_effect: Value = serde_json::from_str(include_str!("../tests/fixtures/memory-forget-cues.json")).unwrap();
        assert_eq!(rules["remember_cue"].as_str(), memory_effect["previous_remember_cue"].as_str(), "turn's remember cue, as reference recorded it");
        assert_eq!(memory_effect["remember_cue"].as_str(), Some(REMEMBER_CUE), "the memory remember cue is the fixture");
        assert_eq!(rules["correct_cue"].as_str(), Some(CORRECT_CUE), "the correct cue is the fixture");
        let cases = rules["strip_cases"].as_array().unwrap();
        assert!(cases.len() >= 21, "the fixture strip cases");
        for c in cases {
            let (u, want) = (c["in"].as_str().unwrap(), c["out"].as_str().unwrap());
            match c["action"].as_str() {
                Some("remember") => assert_eq!(remember_content(u), want, "{u}"),
                Some("correct") => assert_eq!(correct_content("P", u, "2026-10-24"), format!("P; updated 2026-10-24: {want}"), "{u}"),
                other => panic!("unknown action {other:?}"),
            }
        }
        let steps = f["acceptance_script"].as_array().unwrap();
        let step = |n: u64| steps.iter().find(|s| s["step"] == n).unwrap();

        assert_eq!(remember_content(step(2)["utterance"].as_str().unwrap()), step(4)["candidates"][0]["text"].as_str().unwrap());
        let prev = step(5)["candidates"][0]["text"].as_str().unwrap();
        assert_eq!(correct_content(prev, step(5)["utterance"].as_str().unwrap(), "2026-10-24"), step(6)["candidates"][0]["text"].as_str().unwrap());
        for (u, want) in [("please remember Ana prefers tea over coffee", "Ana prefers tea over coffee"), ("Note: the gym opens at 6am", "the gym opens at 6am"), ("save this: my dentist is Dr. Okafor", "my dentist is Dr. Okafor"), ("Don't forget that Ana lands Friday", "Ana lands Friday"), ("Remember remember the fifth", "remember the fifth")] {
            assert_eq!(remember_content(u), want, "{u}");
        }
        let once = correct_content("my favourite café is Lua", "actually it's Sol now", "2026-10-24");
        assert_eq!(correct_content(&once, "No, it's Bica", "2026-10-25"), "my favourite café is Lua; updated 2026-10-25: it's Bica", "a second correction replaces only the last update");
        let long = correct_content(&"x".repeat(300), "wait, it's new", "2026-10-24");
        assert!(long.chars().count() <= MAX_PROPOSAL_CHARS && long.ends_with("; updated 2026-10-24: it's new"));
    }

    #[test]
    fn lang_pt_acceptance_stores_what_pa_lists() {
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/turn-remember-fixtures.json")).unwrap();
        let steps = f["pt_suite"]["acceptance_script"].as_array().unwrap();
        let step = |n: u64| steps.iter().find(|s| s["step"] == n).unwrap();
        assert_eq!(remember_content(step(2)["utterance"].as_str().unwrap()), step(2)["stored"].as_str().unwrap());
        let prev = step(5)["candidates"][0]["text"].as_str().unwrap();
        assert_eq!(correct_content(prev, step(5)["utterance"].as_str().unwrap(), "2026-10-24"), step(5)["stored"].as_str().unwrap());
        assert_eq!(remember_content("lembra que "), "", "a bare cue saves nothing");
    }

    #[test]
    fn anything_off_contract_is_invalid() {
        let o = [c("m-1", 1, "x"), c("m-2", 1, "y")];
        for bad in [r#"{"d":"garbage"}"#, r#"{"d":"no_op","x":1}"#, r#"{"d":"recall","s":["m-9"]}"#, r#"{"d":"recall","s":["m-1","m-1"]}"#, r#"{"d":"recall","s":[]}"#, r#"{"d":"correct","t":"m-9"}"#, "no json", "[]"] {
            assert!(decide_from(bad, "u", &o).is_err(), "{bad}");
        }
        assert!(decide_from(r#"{"d":"remember"}"#, "   ", &o).is_err(), "no words, no proposal");
    }

    #[test]
    fn the_offer_is_at_most_three_and_cut_to_280_chars() {
        let many: Vec<Candidate> = (1..=5).map(|i| c(&format!("m-{i}"), 1, &"y".repeat(300))).collect();
        let o = offer(&many);
        assert_eq!(o.len(), 3);
        assert!(o.iter().all(|c| c.text.chars().count() == MAX_PROPOSAL_CHARS));
    }

    #[test]
    fn the_request_carries_the_frozen_settings_and_the_slot() {
        let b = body("granite41-3b-q4km", "oi", &[], Some(1));
        assert_eq!((b["temperature"].clone(), b["max_tokens"].clone(), b["stream"].clone(), b["id_slot"].clone()), (json!(0), json!(160), json!(false), json!(1)));
        assert_eq!(b["messages"][0]["content"].as_str(), Some(INSTRUCTIONS));
        assert_eq!(b["chat_template_kwargs"], json!({"enable_thinking": false}));
        assert!(body("m", "oi", &[], None).get("id_slot").is_none());
    }

    #[tokio::test]
    async fn a_served_answer_becomes_the_decision_and_the_request_is_the_frozen_one() {
        use crate::local::fake_server::{start as fake_llama, Mode};
        let f = fake_llama(Mode::Mixed { pieces: vec![], answer: r#"{"d":"correct","t":"m-1"}"#.into(), warm_delay: Duration::ZERO }, Duration::ZERO).await;
        let (authority, base_path) = local::parse_loopback_url(&f.url).unwrap();
        let cfg = local::Config { authority, base_path, model: "granite41-3b-q4km".into(), model_sha256: None, slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(64), n_ctx: 4096, first_token: Duration::from_secs(5), system_instruction: None, health_every: Duration::from_millis(20) };
        let offered = [c("m-1", 1, "remember that my favourite café is Lua")];
        let r = run(&cfg, Some(1), "actually it's Sol now", &offered, DEADLINE).await;
        assert_eq!(r.outcome, Ok(Decision::Correct { text: "actually it's Sol now".into(), target: ("m-1".into(), 1) }));
        let req = f.requests.lock().unwrap()[0].clone();
        assert_eq!(req, body("granite41-3b-q4km", "actually it's Sol now", &offered, Some(1)));
    }

    #[tokio::test]
    async fn a_slow_server_times_out_and_garbage_is_invalid_never_a_decision() {
        use crate::local::fake_server::{start as fake_llama, Mode};
        let f = fake_llama(Mode::Silent(Duration::from_secs(5)), Duration::ZERO).await;
        let (authority, base_path) = local::parse_loopback_url(&f.url).unwrap();
        let cfg = local::Config { authority, base_path, model: "m".into(), model_sha256: None, slot: None, decide_slot: None, opener_slot: None, max_tokens: Some(64), n_ctx: 4096, first_token: Duration::from_secs(5), system_instruction: None, health_every: Duration::from_millis(20) };
        let r = run(&cfg, None, "remember that Ana prefers tea", &[], Duration::from_millis(150)).await;
        assert_eq!(r.outcome, Err(Fallback::Timeout));
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let down = local::Config { authority: format!("127.0.0.1:{port}"), ..cfg };
        let r = run(&down, None, "remember that Ana prefers tea", &[], Duration::from_millis(300)).await;
        assert!(matches!(r.outcome, Err(Fallback::Unavailable(_))), "{:?}", r.outcome);
    }

    #[test]
    fn forget_cues_match_pa_fixtures_in_pas_order() {
        let f: Value = serde_json::from_str(include_str!("../tests/fixtures/memory-forget-cues.json")).unwrap();
        assert_eq!((f["forget_cue"].as_str(), f["forget_dismiss"].as_str(), f["correct_cue"].as_str()), (Some(FORGET_CUE), Some(FORGET_DISMISS), Some(CORRECT_CUE)));
        assert_eq!(f["remember_question"].as_str(), Some(REMEMBER_QUESTION), "the fixture remember_question");
        assert_eq!(f["order"], serde_json::json!(["remember_question", "remember_cue", "forget_dismiss", "correct_cue", "forget_cue"]));
        let cases = f["strip_cases"].as_array().unwrap();
        assert_eq!(cases.len(), 45, "the set reference checks");
        for c in cases {
            let (u, want) = (c["in"].as_str().unwrap(), c["out"].as_str().unwrap());
            match c["action"].as_str().unwrap() {
                "forget" => assert_eq!(forget_cue(u), ForgetCue::Forget(want.into()), "{u}"),
                "no_op" => assert_eq!(forget_cue(u), ForgetCue::Dismiss, "{u}"),
                "remember" => {
                    assert_eq!(forget_cue(u), ForgetCue::None, "{u}");
                    assert_eq!(remember_content(u), want, "{u}");
                }
                "decide" => {
                    assert_eq!(forget_cue(u), ForgetCue::None, "{u}");

                    if !cue(REMEMBER_QUESTION).is_match(u) {
                        continue;
                    }
                    let offer = [Candidate { memory_id: "m1".into(), revision: 2, text: "x".into() }];
                    let d = decide_from(r#"{"d":"remember"}"#, u, &offer);
                    assert!(!matches!(d, Ok(Decision::Remember { .. })), "{u}: {d:?}");
                }
                other => panic!("unknown action {other}"),
            }
        }
    }

    #[test]
    fn pa_s_two_missed_forget_phrasings_are_forgets() {

        let f = |u: &str| forget_cue(u);
        assert_eq!(f("don't remember Leo's old address anymore"), ForgetCue::Forget("Leo's old address".into()));
        assert_eq!(f("Please do not remember that Leo's old address any more."), ForgetCue::Forget("Leo's old address".into()));
        assert_eq!(f("delete what I said about Tobin's tea"), ForgetCue::Forget("Tobin's tea".into()));
        assert_eq!(f("Erase what I told you about Tobin's tea."), ForgetCue::Forget("Tobin's tea".into()));
        assert_eq!(f("não lembra mais do endereço antigo do Leo"), ForgetCue::Forget("endereço antigo do Leo".into()));
        assert_eq!(f("apaga o que eu disse sobre o chá do Tobin"), ForgetCue::Forget("o chá do Tobin".into()));

        for u in ["don't remember this, it's private", "I don't remember Leo's old address anymore", "I can't remember what I said about Tobin's tea", "don't forget Leo's address", "remind me what I said about Tobin's tea"] {
            assert_eq!(f(u), ForgetCue::None, "{u}");
        }

        assert!(!cue(FORGET_CUE).is_match("delete what I said about Tobin's tea"));
    }

    #[test]
    fn r2_remind_me_of_reads_and_never_saves() {
        let offer = [Candidate { memory_id: "m1".into(), revision: 2, text: "my favourite café is Lua".into() }];

        assert_eq!(decide_from(r#"{"d":"remember"}"#, "remind me of my favourite café", &offer), Ok(Decision::NoOp));
        assert_eq!(decide_from(r#"{"d":"remember"}"#, "remind me of my favourite café", &[]), Ok(Decision::NoOp));
        assert_eq!(decide_from(r#"{"d":"remember"}"#, "remember that my café is Lua", &offer), Ok(Decision::Remember { text: "my café is Lua".into() }));
    }

    #[test]
    fn r4_questions_and_reminders_never_write() {
        let offer = [Candidate { memory_id: "m1".into(), revision: 2, text: "my cafe is Lua".into() }];
        let d = |answer: &str, u: &str| decide_from(answer, u, &offer);

        for u in ["remind me what my cafe is", "remind me to call Ana tomorrow", "remember when we met?", "me lembra qual é o meu café", "lembra-me de ligar para a Ana"] {
            assert_eq!(d(r#"{"d":"remember"}"#, u), Ok(Decision::NoOp), "{u}");
            assert_eq!(d(r#"{"d":"correct","t":"m1"}"#, u), Ok(Decision::NoOp), "{u}");
        }

        assert!(d(r#"{"d":"correct","t":"m9"}"#, "remind me what my cafe is").is_err());

        assert_eq!(d(r#"{"d":"recall","s":["m1"]}"#, "remind me what my cafe is"), Ok(Decision::Recall(vec![("m1".into(), 2)])));
        assert!(d(r#"{"d":"recall","s":["m9"]}"#, "remind me what my cafe is").is_err());

        assert_eq!(d(r#"{"d":"remember"}"#, "remember that my gym is Iron Box"), Ok(Decision::Remember { text: "my gym is Iron Box".into() }));
        assert!(matches!(d(r#"{"d":"correct","t":"m1"}"#, "actually it's Sol now"), Ok(Decision::Correct { target, .. }) if target == ("m1".to_string(), 2)));
    }

    #[test]
    fn r4_a_correction_with_nothing_offered_stores_nothing() {

        for u in ["actually it's Sol now", "correction: it's Sol", "no, it's Sol now", "na verdade é o Sol agora"] {
            assert_eq!(decide_from(r#"{"d":"remember"}"#, u, &[]), Ok(Decision::NoOp), "{u}");
        }

        let offer = [Candidate { memory_id: "m1".into(), revision: 1, text: "my cafe is Lua".into() }];
        assert!(matches!(decide_from(r#"{"d":"remember"}"#, "actually it's Sol now", &offer), Ok(Decision::Remember { .. })));
        assert_eq!(decide_from(r#"{"d":"remember"}"#, "remember that my cafe is Sol", &[]), Ok(Decision::Remember { text: "my cafe is Sol".into() }));
    }

    #[test]
    fn a_correction_is_never_a_forget_whatever_the_order_of_checks_would_otherwise_give() {

        assert!(cue(FORGET_CUE).is_match("forget the old one, it's Sol now"));
        assert_eq!(forget_cue("no, forget the old one, it's Sol now"), ForgetCue::None);
        assert_eq!(forget_cue("don’t forget the bins"), ForgetCue::None, "a curly don't-forget remembers");
    }

    #[test]
    fn a_forget_matches_saved_memories_by_shared_words_best_first() {
        let c = |id: &str, text: &str| Candidate { memory_id: id.into(), revision: 1, text: text.into() };
        let saved = vec![c("m-1", "my gym is Iron Box"), c("m-2", "Rui's old number is 555-0101"), c("m-3", "my favourite café is Lua; updated 2026-10-25: it's Sol now"), c("m-4", "Rui's new number is 555-0199")];
        let ids = |what: &str| forget_candidates(what, &saved).iter().map(|c| c.memory_id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("my gym is Iron Box"), ["m-1"]);
        assert_eq!(ids("Rui's old number"), ["m-2", "m-4"], "both share Rui and number; the card lists them");
        assert_eq!(ids("meu café favorito é o Lua"), ["m-3"], "accents fold for matching only");
        assert!(ids("the dentist on Friday").is_empty(), "no match: the fixed reply, nothing changes");
        assert!(ids("the").is_empty(), "no content words, no candidates");
    }
}

#[cfg(test)] mod regression_question {
 use super::*;
 #[test] fn regression_reminder_does_not_select_arbitrary_memory() {
  let c=vec![Candidate{memory_id:"m1".into(),revision:1,text:"my cafe is Lua".into()}];
  let got=decide_from(r#"{"d":"remember"}"#,"remind me to call Ana tomorrow",&c).unwrap();
  assert!(matches!(got,Decision::NoOp),"{got:?}");
 }
 #[test] fn regression_memory_question_never_becomes_correction() {
  let c=vec![Candidate{memory_id:"m1".into(),revision:1,text:"my cafe is Lua".into()}];
  let got=decide_from(r#"{"d":"correct","t":"m1"}"#,"remind me what my cafe is",&c).unwrap();
  assert!(!matches!(got,Decision::Correct{..}|Decision::Remember{..}),"{got:?}");
 }
}

#[cfg(test)]
mod natural_saves {
    use super::*;

    #[test]
    fn a_self_statement_of_a_lasting_fact_is_saved_in_the_users_own_words() {
        for (u, want) in [

            ("my full name Ana Souza", "my full name Ana Souza"),
            ("Remember that my full name is Ana Souza.", "my full name is Ana Souza."),

            ("my name is Ana", "my name is Ana"),
            ("My full name is Ana Souza.", "My full name is Ana Souza."),
            ("meu nome é Ana Souza", "meu nome é Ana Souza"),
            ("Meu nome completo é Ana Souza.", "Meu nome completo é Ana Souza."),
            ("me chamo Ana", "me chamo Ana"),
            ("pode me chamar de Bia", "pode me chamar de Bia"),
            ("call me Teo", "call me Teo"),
            ("eu moro em Belo Horizonte", "eu moro em Belo Horizonte"),
            ("moro no Rio", "moro no Rio"),
            ("I live in Lisbon", "I live in Lisbon"),
            ("I work at Namastex", "I work at Namastex"),
            ("eu trabalho na Namastex", "eu trabalho na Namastex"),
            ("prefiro café sem açúcar", "prefiro café sem açúcar"),
            ("I prefer tea", "I prefer tea"),
            ("oi, meu nome é Ana", "oi, meu nome é Ana"),
            ("actually my name is Ana Souza", "my name is Ana Souza"),
        ] {
            assert_eq!(self_fact(u).as_deref(), Some(want), "{u}");
        }
    }

    #[test]
    fn near_misses_are_not_self_statements() {
        for u in [
            "what's my name?",
            "whats my name",
            "meu nome é?",
            "my name is",
            "meu nome é",
            "my name is hard to spell",
            "my name isn't Ana",
            "my name is not Ana",
            "meu nome não é Ana",
            "if my name is Ana, what's yours?",
            "his name is Rui",
            "a Ana mora em Lisboa",
            "I worked at Google",
            "I prefer not to say",
            "prefiro não dizer",
            "where do I work?",
            "can you call me Teo?",
            "don't call me Teo",
            "remind me to call Ana",
            "I'm tired",
            "estou com fome",
            "tell me a joke",
            "I think my name is weird",
            "call me later",
            "call me back at 5",
            "call me when you land",
            "me chama depois",
        ] {
            assert_eq!(self_fact(u), None, "{u}");
        }
    }

    #[test]
    fn a_bare_name_answering_a_name_question_is_saved_with_the_questions_subject() {
        const NONE_EN: &str = "I don't know; you haven't told me that.";
        const NONE_PT: &str = "Não sei; você ainda não me contou isso.";

        assert_eq!(name_answer("Ana", Some("whats my name?"), Some(NONE_EN)).as_deref(), Some("my name is Ana"));
        assert_eq!(name_answer("Ana Souza", Some("qual é o meu nome?"), Some(NONE_PT)).as_deref(), Some("meu nome é Ana Souza"));
        assert_eq!(name_answer("Ana.", Some("do you know my name?"), Some(NONE_EN)).as_deref(), Some("my name is Ana"));
        assert_eq!(name_answer("Ana", None, Some("Hi! What's your name?")).as_deref(), Some("my name is Ana"));
        assert_eq!(name_answer("ana souza", Some("oi"), Some("Oi! Como você se chama?")).as_deref(), Some("meu nome é ana souza"));

        for (u, asked, reply) in [
            ("no", Some("whats my name?"), Some(NONE_EN)),
            ("idk", Some("whats my name?"), Some(NONE_EN)),
            ("why?", Some("whats my name?"), Some(NONE_EN)),
            ("lol", Some("whats my name?"), Some(NONE_EN)),
            ("sim", Some("qual é o meu nome?"), Some(NONE_PT)),
            ("eu não sei", Some("qual é o meu nome?"), Some(NONE_PT)),
            ("you tell me", Some("whats my name?"), Some(NONE_EN)),
            ("tell me a joke please", Some("whats my name?"), Some(NONE_EN)),
            ("Ana?", Some("whats my name?"), Some(NONE_EN)),
            ("Ana", Some("what's the capital of France?"), Some("Paris.")),
            ("Ana", None, Some("I'm Unfold.")),
            ("Ana", None, None),
            ("Paris", Some("what's my favourite city?"), Some(NONE_EN)),

            ("adivinha", Some("qual é o meu nome?"), Some(NONE_PT)),
            ("Not now", Some("whats my name?"), Some(NONE_EN)),
            ("whatever", Some("whats my name?"), Some(NONE_EN)),
            ("beleza", Some("qual é o meu nome?"), Some(NONE_PT)),
            ("Ana", Some("whats my name?"), Some("Your name is Rui; you told me on 2026-10-24.")),
        ] {
            assert_eq!(name_answer(u, asked, reply), None, "{u} after {asked:?} / {reply:?}");
        }
    }

    #[test]
    fn real_time_instructions_and_the_ask_only_switch() {
        assert!(INSTRUCTIONS.contains("even in passing") && INSTRUCTIONS.contains("family"), "{INSTRUCTIONS}");
        for near in ["hypothetical", "someone else thinks", "passing state", "quotes"] {
            assert!(INSTRUCTIONS.contains(near), "{near}: {INSTRUCTIONS}");
        }
        assert_eq!(PART, "memory.decide@1/mini-v4.3");
        assert_eq!(Saves::parse(None), Ok(Saves::Realtime));
        assert_eq!(Saves::parse(Some("realtime")), Ok(Saves::Realtime));
        assert_eq!(Saves::parse(Some("asked")), Ok(Saves::Asked));
        assert!(Saves::parse(Some("sometimes")).is_err());
        let (text, sha, part) = Saves::Asked.instructions();
        assert_eq!((sha, part), ("e9e32f3ca1575d603c9a927fce5bacea70b468879797f262f5d6d364c703182c", "memory.decide@1/mini-v4.1"));
        assert_eq!(crate::backend::text_sha256(text), sha);
        assert_eq!(Saves::Realtime.instructions(), (INSTRUCTIONS, INSTRUCTIONS_SHA256, PART));
    }

    #[test]
    fn a_family_member_named_in_passing_is_a_floor_fact() {
        for u in ["my sister is Ana", "My wife's name is Lia", "minha irmã é a Ana", "meu filho se chama Rui", "my dog is called Pipoca"] {
            assert_eq!(self_fact(u).as_deref(), Some(u), "{u}");
        }
    }

    #[test]
    fn near_miss_self_statements_are_never_floor_facts() {
        for u in [
            "if I lived in Lisbon I'd surf every day",
            "se eu morasse em Lisboa eu surfava",
            "my name would be Ana if I were a girl",
            "my boss thinks I should move to Porto",
            "minha mãe acha que eu devia morar em Lisboa",
            "my sister says I prefer tea",
            "estou cansado",
            "I'm hungry right now",
            "he said \"my name is Bond\"",
            "\"I live in Paris\" is a great song",
            "minha irmã é chata",
            "my sister is tall",

            "I'm called to jury duty tomorrow",
            "my name means light in Greek",
            "I work at 9 tomorrow",
            "call me around 6",
            "call me crazy, but I like rain",
            "me chama de manhã",
            "I prefer the second option",
            "prefiro a primeira",
        ] {
            assert_eq!(self_fact(u), None, "{u}");
        }
    }

    #[test]
    fn the_stored_text_is_subject_plus_fact() {
        for (u, want) in [
            ("my full name Ana Souza", "my full name is Ana Souza"),
            ("my name Ana", "my name is Ana"),
            ("my name's Ana", "my name is Ana"),
            ("I'm called Teo", "my name is Teo"),
            ("me chamo Ana", "meu nome é Ana"),
            ("eu me chamo Ana Souza.", "meu nome é Ana Souza."),
            ("oi, meu nome é Ana", "meu nome é Ana"),
            ("hi! I live in Lisbon", "I live in Lisbon"),
            ("my full name is Ana Souza.", "my full name is Ana Souza."),
            ("pode me chamar de Bia", "pode me chamar de Bia"),
            ("my sister is Ana", "my sister is Ana"),
            ("Long day today. My sister is Ana, by the way. What should I cook?", "My sister is Ana, by the way."),
            ("Que dia. Eu moro em Recife! Alguma dica de praia?", "Eu moro em Recife!"),

            ("ok, my name is Ana", "my name is Ana"),
            ("então, me chamo Rui", "meu nome é Rui"),
            ("I'm tired. My sister is Ana.", "My sister is Ana."),
        ] {
            assert_eq!(fact_text(u, false).as_deref(), Some(want), "{u}");
        }
    }

    #[test]
    fn round2_markers_after_the_shape_are_no_floor_facts() {
        for u in [
            "I live in Paris if I get the job",
            "I work at Google if they hire me",
            "eu moro em Lisboa se eu passar",
            "I work at the office today",
            "trabalho em casa hoje",
            "I live in a hotel this week",
            "moro no Rio por enquanto",
            "my name is Bob in the game",
            "my name is Bob, he said",
            "call me Monday morning",
            "I prefer it shorter",
            "I prefer the blue one",
            "prefiro assim",
            "my name is what",
            "meu nome é qual mesmo",
            "I live in which city",
        ] {
            assert_eq!(self_fact(u), None, "{u}");
        }

        assert_eq!(self_fact("minha irmã se chama Ana").as_deref(), Some("minha irmã se chama Ana"));
        assert_eq!(self_fact("my name is Ana, nice to meet you").as_deref(), Some("my name is Ana, nice to meet you"));
    }

    #[test]
    fn round2_reactions_to_a_name_question_are_no_names() {
        const NONE_EN: &str = "I don't know; you haven't told me that.";
        for u in ["Sorry", "Wait", "Awesome", "Secret", "Skip", "Pass", "Anonymous", "Exactly", "Yup", "Hahaha", "kkkk", "rsrs", "hehe", "Bom dia", "Good morning", "Tudo bem", "Entendi", "Pode ser"] {
            assert_eq!(name_answer(u, None, Some("Hi! What's your name?")), None, "{u}");
            assert_eq!(name_answer(u, Some("whats my name?"), Some(NONE_EN)), None, "{u}");
        }
        assert_eq!(name_answer("Ana", None, Some("What's your name? Never mind, tell me about your day.")), None);
        assert_eq!(name_answer("Ana", None, Some("Prazer! Como você se chama? 🙂")).as_deref(), Some("meu nome é Ana"));
        assert_eq!(name_answer("Marta", Some("do you know what my name is?"), Some(NONE_EN)).as_deref(), Some("my name is Marta"));
    }

    #[test]
    fn round2_an_explicit_remember_is_stored_whole_and_a_frame_needs_a_name() {
        for (u, want) in [
            ("the dentist is on Friday. I need to bring my card.", "the dentist is on Friday. I need to bring my card."),
            ("a reunião é sexta. Eu levo o projetor.", "a reunião é sexta. Eu levo o projetor."),
            ("Ana's birthday is May 3. I want to buy her a gift.", "Ana's birthday is May 3. I want to buy her a gift."),
            ("my full name Ana Souza", "my full name is Ana Souza"),
        ] {
            assert_eq!(fact_text(u, true).as_deref(), Some(want), "{u}");
        }
        for (u, want) in [
            ("my name means light in Greek", "my name means light in Greek"),
            ("I'm called Ana by my friends", "I'm called Ana by my friends"),
            ("my name is Ana, nice to meet you", "my name is Ana"),
            ("my name is Ana, and I live in Lisbon", "my name is Ana, and I live in Lisbon"),
            ("My name is Ana. I'm a nurse.", "my name is Ana."),
            ("long week. my brother Rui lives in Natal now, any trip ideas?", "my brother Rui lives in Natal now"),
        ] {
            assert_eq!(fact_text(u, false).as_deref(), Some(want), "{u}");
        }
    }

    #[test]
    fn round3_the_common_ways_to_state_a_fact_still_save() {
        for u in [
            "My name is Ana. Nice to meet you.",
            "me chamo Ana. Prazer!",
            "my name is Ana and I live in Lisbon",
            "my name is Ana :)",
            "meu nome completo é João Pedro da Silva Santos",
            "Moro em Recife. Hoje está muito calor.",
            "I work at Nubank. If you have tips, tell me.",
            "I work in IT",
            "I work at a startup that builds robots",
            "trabalho em uma empresa que se chama Acme",
            "my name is Will",
            "my name is Uma",
            "call me Em",
            "my name is Ana Grande",
        ] {
            assert_eq!(self_fact(u).as_deref(), Some(u), "{u}");
        }
        for (u, want) in [("Kika", "my name is Kika"), ("Bruno Lima", "my name is Bruno Lima")] {
            assert_eq!(name_answer(u, None, Some("Hi! What's your name?")).as_deref(), Some(want), "{u}");
        }
    }

    #[test]
    fn round3_interjections_are_no_names() {
        for u in ["Hum", "Hmmm", "Umm", "Uhh", "Wow", "Nossa", "Caramba", "Vish", "Aff", "Oxe", "brb", "whoops", "go ahead", "fala aí", "vai saber", "chuta aí", "Actually", "Soon"] {
            assert_eq!(name_answer(u, None, Some("Hi! What's your name?")), None, "{u}");
        }
    }

    #[test]
    fn round3_what_a_longer_message_stores() {
        for (u, want) in [
            ("I had a rough day. My sister is Ana.", "My sister is Ana."),
            ("long week. my brother Rui lives in Natal now. What should I get him, a book or a hat?", "my brother Rui lives in Natal now."),
            ("I'm so tired. my brother Rui lives in Natal now.", "my brother Rui lives in Natal now."),
        ] {
            assert_eq!(fact_text(u, false).as_deref(), Some(want), "{u}");
        }
        assert_eq!(fact_text("my name is Ana, the wifi code is X", true).as_deref(), Some("my name is Ana, the wifi code is X"));
    }

    #[test]
    fn round4_a_command_after_a_name_question_is_no_name() {
        for u in ["Help me", "Fix my code", "Set a timer", "Traduz isso", "Check my email", "call me later"] {
            assert_eq!(name_answer(u, None, Some("Hi! What's your name?")), None, "{u}");
        }
        for (u, want) in [("Bruno Lima", "my name is Bruno Lima"), ("Ana Grande", "my name is Ana Grande"), ("Ana Souza", "my name is Ana Souza")] {
            assert_eq!(name_answer(u, None, Some("Hi! What's your name?")).as_deref(), Some(want), "{u}");
        }
        for u in ["My name is Ana. Nice to meet you.", "Meu nome é Ana, prazer.", "Remember that my full name is Ana Souza."] {
            assert!(self_fact(u).is_some(), "{u}");
        }
    }

    #[test]
    fn a_name_after_an_opener_is_that_name() {
        const ASK: Option<&str> = Some("Hi! What's your name?");
        const PERGUNTA: Option<&str> = Some("Olá! Qual é o seu nome?");
        for (u, reply, want) in [("I'm Ana", ASK, "my name is Ana"), ("It’s Teo", ASK, "my name is Teo"), ("Sou a Ana", PERGUNTA, "meu nome é Ana"), ("eu sou o Rui", PERGUNTA, "meu nome é Rui"), ("Joana", ASK, "my name is Joana"),

            ("I'm Christian", ASK, "my name is Christian"), ("I'm Christian Silva", ASK, "my name is Christian Silva"), ("Sou o German", PERGUNTA, "meu nome é German"),
            ("I'm Danish Khan", ASK, "my name is Danish Khan")] {
            assert_eq!(name_answer(u, None, reply).as_deref(), Some(want), "{u}");
        }
        for (u, reply) in [
            ("I'm a student", ASK),
            ("I'm tired", ASK),
            ("It's a girl", ASK),
            ("sou a mãe", PERGUNTA),
            ("sou brasileiro", PERGUNTA),
            ("é a mesma", PERGUNTA),

            ("I'm a Student", ASK),
            ("It's a Girl", ASK),
            ("I'm Brazilian", ASK),
            ("I'm American", ASK),
            ("I'm Catholic", ASK),
            ("I'm Lisbon-based", ASK),
        ] {
            assert_eq!(name_answer(u, None, reply), None, "{u}");
        }
    }

}

#[cfg(test)]
mod pv05_g6_intro {
    use super::introduction;

    #[test]
    fn an_introduction_names_a_person_by_a_relation_or_alone_and_nothing_else_is_one() {
        let owner = vec!["Teo Lima".to_string(), "Unfold".to_string()];
        for (said, name) in [("This is Ana", "Ana"), ("this is Ana.", "Ana"), ("Esta é a Ana", "Ana"), ("Meet my sister Bia!", "Bia"), ("Oi, este é o Caio", "Caio"), ("esta é a minha irmã Lia", "Lia"), ("Hi! This is Ana. She likes tea.", "Ana"), ("This is Ana, say hi", "Ana"), ("Oi! Esta é a minha irmã Lia, diga oi", "Lia"), ("Meet Dr. Smith", "Dr. Smith"), ("This is Mr. Lee.", "Mr. Lee"), ("Esta é a Dra. Ana", "Dra. Ana"), ("Este é o Prof. Caio, diga oi", "Prof. Caio")] {
            assert_eq!(introduction(said, &owner).as_deref(), Some(name), "{said}");
        }
        for said in [
            "this is great", "This is Ana?", "Is this Ana?", "my name is Ana", "essa é a minha opinião", "meet me at five",
            "This is Netflix, why is it so slow", "This is Google Drive", "Este é o Brasil que eu quero", "this is my Mac", "Meet Tuesday", "Apresento Python para iniciantes", "this is English", "Esta é a minha Casa", "This is Ana's book",
            "Hi, this is Teo", "Aqui é o Teo falando", "This is Unfold",

            "this is my friend Rui and he likes chess", "This is Ana, my sister", "essa é a Ana, minha irmã", "This is Ana? No, Bia",

            "Meet Dr.", "This is Mr. Teo",
        ] {
            assert_eq!(introduction(said, &owner), None, "{said}");
        }
    }
}

#[cfg(test)]
mod name_reply_language_tests {
    use super::*;
    fn wire(quoted:&str,text:&str)->String {
        format!("[Unfold was interrupted. Complete speech segments rendered before interruption: {}. Any partial final segment is omitted; the rest of the generated reply was not rendered. This is a playout observation, not a correction of saved history.]\n\n{text}",serde_json::json!(quoted))
    }
    #[test]
    fn name_answer_subject_follows_genuine_question_language() {
        let pt=wire("","Qual é o meu nome?");
        assert_eq!(name_answer("Ana",Some(&pt),Some("Não sei; você ainda não me contou isso.")).as_deref(),Some("meu nome é Ana"));
        let english=wire(&"você não eu meu minha como quem é pode para agora".repeat(4),"What is my name?");
        assert_eq!(name_answer("Ana",Some(&english),Some("I don't know; you haven't told me that.")).as_deref(),Some("my name is Ana"));
        assert_eq!(name_answer("Ana",None,Some("No pending question.")).as_deref(),None,"name admission policy unchanged");
    }
}
