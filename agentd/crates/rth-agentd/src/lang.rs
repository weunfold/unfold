

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Pt,
    En,
}

const PT_WORDS: &[&str] = &[
    "você", "voce", "vc", "não", "nao", "eu", "que", "é", "oi", "olá", "ola", "quem", "como", "está", "esta", "estou", "obrigado", "obrigada", "para", "pra", "com", "uma", "um", "meu",
    "minha", "isso", "isto", "sim", "tudo", "bem", "qual", "quais", "onde", "quando", "porque", "por", "também", "tambem", "muito", "seu", "sua", "fala", "faz", "pode", "são", "sao", "ser", "tem",
    "aqui", "agora", "então", "entao", "bom", "dia", "noite", "tarde", "repita", "diga", "primeira", "palavra",

    "sou", "fui", "pela", "pelo", "te", "criou", "treinou", "comunidade",
];

const EN_WORDS: &[&str] = &[
    "the", "you", "who", "what", "is", "are", "i", "i'm", "hi", "hello", "hey", "how", "can", "do", "does", "your", "my", "and", "to", "of", "it", "this", "that", "please", "thanks", "yes",
    "where", "when", "why", "which", "repeat", "first", "word", "said", "answer", "sentence", "tell", "about", "here", "there", "with", "would", "could", "should", "have", "has", "am",
    "what's", "it's", "that's", "i'll", "don't", "now", "actually", "remember", "favourite", "favorite", "number", "code",
];

pub fn detect_language(text: &str) -> Option<Lang> {
    let lower = text.to_lowercase();
    let accents = lower.chars().filter(|c| "ãõçáéíóúâêôà".contains(*c)).count();
    let words: Vec<&str> = lower.split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()).collect();

    let pt = words.iter().filter(|w| PT_WORDS.contains(w)).count() + usize::from(accents > 0);
    let en = words.iter().filter(|w| EN_WORDS.contains(w)).count();
    match pt.cmp(&en) {
        std::cmp::Ordering::Greater => Some(Lang::Pt),
        std::cmp::Ordering::Less => Some(Lang::En),
        std::cmp::Ordering::Equal => None,
    }
}
