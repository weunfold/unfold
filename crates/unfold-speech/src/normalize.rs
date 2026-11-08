

pub fn normalize_pt(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        if let Some(end) = skipped_span(&c, i) {
            out.extend(&c[i..end]);
            i = end;
            continue;
        }
        if matches!(c[i], 'R' | 'r') && c.get(i + 1) == Some(&'$') && (i == 0 || free_before_currency(c[i - 1])) {
            let mut j = i + 2;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if let Some((words, end)) = currency(&c, j) {
                out.push_str(&words);
                i = end;
                continue;
            }

            out.extend(&c[i..j]);
            i = copy_run(&c, j, &mut out);
            continue;
        }
        if c[i].is_ascii_digit() {
            if let Some((words, end)) = plain_number(&c, i) {
                out.push_str(&words);
                i = end;
                continue;
            }
            i = copy_run(&c, i, &mut out);
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn skipped_span(c: &[char], i: usize) -> Option<usize> {
    let at = |s: &str| s.chars().enumerate().all(|(k, ch)| c.get(i + k) == Some(&ch));
    let find = |from: usize, close: &str| -> Option<usize> {
        let close: Vec<char> = close.chars().collect();
        (from..c.len()).find(|&k| c[k..].starts_with(&close)).map(|k| k + close.len())
    };
    if at("```") {
        return Some(find(i + 3, "```").unwrap_or(c.len()));
    }
    if c[i] == '`' {

        return (i + 1..c.len().min(i + 500)).take_while(|&k| c[k] != '\n').find(|&k| c[k] == '`').map(|k| k + 1);
    }
    if c[i] == '⟨' {
        return (i + 1..c.len().min(i + 64)).take_while(|&k| c[k] != '\n').find(|&k| c[k] == '⟩').map(|k| k + 1);
    }
    if c[i] == '<' && c.get(i + 1).is_some_and(|n| n.is_alphabetic() || *n == '/') {
        return (i + 1..c.len().min(i + 64)).take_while(|&k| c[k] != '\n').find(|&k| c[k] == '>').map(|k| k + 1);
    }
    let word_start = i == 0 || c[i - 1].is_whitespace() || "(\"'".contains(c[i - 1]);
    if word_start && (at("http://") || at("https://") || at("www.")) {
        return Some((i..c.len()).find(|&k| c[k].is_whitespace()).unwrap_or(c.len()));
    }
    None
}

fn copy_run(c: &[char], mut i: usize, out: &mut String) -> usize {
    while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == ',') {
        out.push(c[i]);
        i += 1;
    }
    i
}

fn free_before_currency(prev: char) -> bool {
    !(prev.is_alphanumeric() || "-−–—$_@#".contains(prev))
}

fn free_before(c: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    if c[i - 1].is_alphanumeric() || "-+−–—/:.,_@#$=[*%€£¥".contains(c[i - 1]) {
        return false;
    }
    let mut k = i;
    while k > 0 && c[k - 1].is_whitespace() {
        k -= 1;
    }
    !(k > 0 && k < i && "$€£¥".contains(c[k - 1]))
}

fn number_run(c: &[char], start: usize) -> Option<(String, usize)> {
    let mut end = start;
    while end < c.len() && (c[end].is_ascii_digit() || c[end] == '.' || c[end] == ',') {
        end += 1;
    }
    while end > start && (c[end - 1] == '.' || c[end - 1] == ',') {
        end -= 1;
    }
    if end == start || !c[start].is_ascii_digit() {
        return None;
    }
    if let Some(&next) = c.get(end) {
        let glued_mark = (next == '.' || next == ',') && c.get(end + 1).is_some_and(|d| d.is_alphanumeric());
        if next.is_alphanumeric() || "/:_-–—−@°+*=]".contains(next) || glued_mark {
            return None;
        }
    }
    Some((c[start..end].iter().collect(), end))
}

fn word_after(c: &[char], end: usize) -> Option<(String, usize)> {
    let mut i = end;
    while i < c.len() && c[i].is_whitespace() {
        i += 1;
    }
    let start = i;
    while i < c.len() && c[i].is_alphabetic() {
        i += 1;
    }
    (i > start && start > end).then(|| (c[start..i].iter().flat_map(|ch| ch.to_lowercase()).collect(), i))
}

fn word_before(c: &[char], start: usize) -> Option<String> {
    let mut i = start;
    while i > 0 && c[i - 1].is_whitespace() {
        i -= 1;
    }
    let end = i;
    while i > 0 && c[i - 1].is_alphabetic() {
        i -= 1;
    }
    (end > i && end < start).then(|| c[i..end].iter().flat_map(|ch| ch.to_lowercase()).collect())
}

const SCALES: [&str; 7] = ["mil", "milhão", "milhões", "bilhão", "bilhões", "trilhão", "trilhões"];

const SHORT_SCALES: [&str; 4] = ["mi", "bi", "tri", "milhares"];

fn currency(c: &[char], j: usize) -> Option<(String, usize)> {
    let (body, end) = number_run(c, j)?;
    if c.get(end) == Some(&' ') && c.get(end + 1).is_some_and(|d| d.is_ascii_digit()) {
        return None;
    }
    if word_after(c, end).is_some_and(|(w, _)| SHORT_SCALES.contains(&w.as_str())) {
        return None;
    }
    if let Some((scale, after)) = word_after(c, end).filter(|(w, _)| SCALES.contains(&w.as_str())) {
        if body.contains(',') {
            return None;
        }
        let n = integer(&body, 12)?;
        let unit = if scale == "mil" { "reais" } else { "de reais" };
        let mut end = after;
        for tail in [" de reais", " reais"] {
            if c[end..].iter().take(tail.chars().count()).copied().eq(tail.chars()) {
                end += tail.chars().count();
                break;
            }
        }
        return Some((format!("{} {} {}", cardinal(n, false), &c[after - scale.chars().count()..after].iter().collect::<String>(), unit), end));
    }
    money(&body).map(|w| (w, end))
}

fn plain_number(c: &[char], i: usize) -> Option<(String, usize)> {
    if !free_before(c, i) {
        return None;
    }

    if i >= 2 && c[i - 1] == ' ' && (c[i - 2].is_ascii_digit() || c[i - 2] == ')') {
        return None;
    }
    if word_before(c, i).is_some_and(|w| ["versão", "versao", "version", "ver", "v"].contains(&w.as_str())) {
        return None;
    }
    let (body, end) = number_run(c, i)?;
    let space_digit = |k: usize| c.get(k) == Some(&' ') && c.get(k + 1).is_some_and(|d| d.is_ascii_digit());
    if space_digit(end) || (i > 0 && c[i - 1] == '(' && c.get(end) == Some(&')') && space_digit(end + 1)) {
        return None;
    }
    let percent_at = match (c.get(end), c.get(end + 1)) {
        (Some('%'), _) => Some(end),
        (Some(' '), Some('%')) => Some(end + 1),
        _ => None,
    };
    if let Some(p) = percent_at {
        if c.get(p + 1).is_some_and(|n| n.is_alphanumeric()) {
            return None;
        }
        return Some((format!("{} por cento", number(&body, false)?), p + 1));
    }
    if body == "1" {
        if let Some((de, after)) = word_after(c, end) {
            if de == "de" && word_after(c, after).is_some_and(|(m, _)| MONTHS.contains(&m.as_str())) {
                return Some(("primeiro".to_string(), end));
            }
        }
    }
    let mut words = number(&body, feminine_after(c, i, end))?;
    if let Some(n) = (!body.contains(',')).then(|| integer(&body, 4)).flatten() {
        let next = word_after(c, end).map(|(w, _)| w);
        if n >= 1_000_000 && n % 1_000_000 == 0 && next.is_some_and(|w| listed_noun(&w)) {
            words.push_str(" de");
        }
    }
    Some((words, end))
}

fn integer(s: &str, plain_max: usize) -> Option<u64> {
    if !s.chars().all(|d| d.is_ascii_digit() || d == '.') {
        return None;
    }
    let ok = if s.contains('.') {
        let groups: Vec<&str> = s.split('.').collect();
        let first = groups[0];
        let grouped = (1..=3).contains(&first.len()) && !first.starts_with('0') && groups[1..].iter().all(|g| g.len() == 3);
        let ip_like =
            groups.len() == 4 && first.len() == 3 && groups.iter().all(|g| g.parse::<u32>().is_ok_and(|v| v <= 255));
        grouped && !ip_like
    } else {
        s.len() <= plain_max && (s == "0" || !s.starts_with('0'))
    };
    let digits: String = s.chars().filter(|d| *d != '.').collect();
    (ok && digits.len() <= 12).then(|| digits.parse().ok())?
}

fn money(body: &str) -> Option<String> {
    let (int, cents) = match body.split_once(',') {
        Some((i, c)) if c.len() == 2 && c.chars().all(|d| d.is_ascii_digit()) => (i, c.parse::<u64>().ok()?),
        Some(_) => return None,
        None => (body, 0),
    };
    let reais = integer(int, 12)?;
    let reais_words = match reais {
        0 if cents > 0 => None,
        0 => Some("zero reais".to_string()),
        1 => Some("um real".to_string()),
        n if n % 1_000_000 == 0 => Some(format!("{} de reais", cardinal(n, false))),
        n => Some(format!("{} reais", cardinal(n, false))),
    };
    let cents_words = match cents {
        0 => None,
        1 => Some("um centavo".to_string()),
        n => Some(format!("{} centavos", cardinal(n, false))),
    };
    Some(match (reais_words, cents_words) {
        (Some(r), Some(c)) => format!("{r} e {c}"),
        (Some(r), None) => r,
        (None, Some(c)) => c,
        (None, None) => return None,
    })
}

fn number(body: &str, fem: bool) -> Option<String> {
    match body.split_once(',') {
        None => Some(cardinal(integer(body, 4)?, fem)),
        Some((int, frac)) => {
            if frac.is_empty() || frac.len() > 6 || !frac.chars().all(|d| d.is_ascii_digit()) {
                return None;
            }
            let mut words = format!("{} vírgula", cardinal(integer(int, 4)?, false));
            let rest = frac.trim_start_matches('0');
            for _ in 0..frac.len() - rest.len() {
                words.push_str(" zero");
            }
            if !rest.is_empty() {
                words.push(' ');
                words.push_str(&cardinal(rest.parse().ok()?, false));
            }
            Some(words)
        }
    }
}

fn cardinal(n: u64, fem: bool) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    let mut parts: Vec<(String, u64)> = Vec::new();
    for (scale, one, many) in [(1_000_000_000, "bilhão", "bilhões"), (1_000_000, "milhão", "milhões")] {
        let g = n / scale % 1000;
        if g > 0 {
            parts.push((format!("{} {}", below_thousand(g, false), if g == 1 { one } else { many }), g));
        }
    }
    let thousands = n / 1000 % 1000;
    if thousands > 0 {
        let w = if thousands == 1 { "mil".to_string() } else { format!("{} mil", below_thousand(thousands, fem)) };
        parts.push((w, thousands));
    }
    let units = n % 1000;
    if units > 0 {
        parts.push((below_thousand(units, fem), units));
    }
    let last = parts.len() - 1;
    let mut out = String::new();
    for (k, (w, g)) in parts.iter().enumerate() {
        if k > 0 {
            out.push_str(if k == last && (*g < 100 || g % 100 == 0) { " e " } else { " " });
        }
        out.push_str(w);
    }
    out
}

fn below_thousand(n: u64, fem: bool) -> String {
    const TEENS: [&str; 10] =
        ["dez", "onze", "doze", "treze", "quatorze", "quinze", "dezesseis", "dezessete", "dezoito", "dezenove"];
    const TENS: [&str; 10] = ["", "", "vinte", "trinta", "quarenta", "cinquenta", "sessenta", "setenta", "oitenta", "noventa"];
    const HUNDREDS: [&str; 10] =
        ["", "cento", "duzent", "trezent", "quatrocent", "quinhent", "seiscent", "setecent", "oitocent", "novecent"];
    if n == 100 {
        return "cem".to_string();
    }
    let (h, r) = (n / 100, n % 100);
    let mut words: Vec<String> = Vec::new();
    if h == 1 {
        words.push("cento".to_string());
    } else if h > 1 {
        words.push(format!("{}{}", HUNDREDS[h as usize], if fem { "as" } else { "os" }));
    }
    if r > 0 {
        words.push(match r {
            1..=9 => unit(r, fem).to_string(),
            10..=19 => TEENS[r as usize - 10].to_string(),
            _ if r % 10 == 0 => TENS[r as usize / 10].to_string(),
            _ => format!("{} e {}", TENS[r as usize / 10], unit(r % 10, fem)),
        });
    }
    words.join(" e ")
}

fn unit(n: u64, fem: bool) -> &'static str {
    const UNITS: [&str; 10] = ["zero", "um", "dois", "três", "quatro", "cinco", "seis", "sete", "oito", "nove"];
    match (n, fem) {
        (1, true) => "uma",
        (2, true) => "duas",
        _ => UNITS[n as usize],
    }
}

const MONTHS: [&str; 12] = [
    "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro",
    "dezembro",
];

const LABELS: &[&str] = &[
    "plano", "página", "fase", "capítulo", "item", "passo", "etapa", "versão", "número", "sala", "andar", "linha",
    "questão", "exercício", "parte", "seção", "temporada", "episódio", "lote", "modelo", "opção", "nível", "rodada",
    "grupo", "time", "aula", "artigo", "figura", "tabela", "slide",
];

const STOP: &[&str] = &[
    "a", "à", "às", "ao", "aos", "na", "nas", "no", "nos", "para", "pra", "contra", "cada", "agora", "ainda", "fora",
    "nunca", "sobre", "entre", "e", "ou", "de", "do", "dos", "em", "por", "com", "sem", "até", "que",
];

const FEMININE: &[&str] = &[
    "pessoa", "hora", "semana", "coisa", "vez", "casa", "mulher", "criança", "filha", "mãe", "noite", "tarde", "manhã",
    "madrugada", "página", "palavra", "linha", "letra", "tarefa", "pergunta", "resposta", "ideia", "foto", "moto",
    "lei", "voz", "luz", "paz", "classe", "frase", "fase", "base", "arte", "árvore", "colher", "parede", "nuvem",
    "morte", "pele", "mão", "razão", "cor", "flor", "parte", "ordem", "chave", "fonte", "ponte", "rede", "gente",
    "xícara", "fatia", "garrafa", "caixa", "conta", "nota", "aula", "música", "vaga", "rua", "porta", "janela", "mesa",
    "cadeira", "cama", "forma", "turma", "peça", "cerveja", "bola", "carta", "chamada", "tentativa", "etapa", "rodada",
    "partida", "volta", "vida", "irmã", "amiga", "lista", "regra", "tela", "pasta", "imagem", "viagem", "origem", "garagem", "mensagem",
    "contagem", "margem", "vantagem", "passagem", "paisagem", "postagem", "reportagem", "linguagem", "embalagem",
    "bagagem", "homenagem", "questão", "sugestão",
];

const MASCULINE_NOUNS: &[&str] = &[
    "real", "dólar", "euro", "habitante", "ano", "dia", "mês", "usuário", "voto", "brasileiro", "cliente", "litro",
    "quilômetro", "metro", "caso", "emprego", "eleitor", "aluno", "acesso", "seguidor", "download", "inscrito", "quilo",
    "barril", "turista", "visitante", "assinante", "leitor", "trabalhador", "reais", "meses", "países", "barris",
];

const ARTICLE_M: &[&str] = &["o", "os", "um", "uns", "do", "dos", "no", "nos", "ao", "aos"];
const ARTICLE_F: &[&str] = &["as", "uma", "umas", "da", "das", "na", "nas"];

const ARTICLE_OR_PREPOSITION: &[&str] = &["a", "à", "às"];

const MASCULINE: &[&str] = &["coração", "personagem", "são"];

fn feminine_after(c: &[char], start: usize, end: usize) -> bool {
    let before = word_before(c, start);

    let time = before.as_deref().is_some_and(|w| w == "às" || (w == "à" && c[start..end] == ['1']));
    let Some((mut word, mut after)) = word_after(c, end) else {
        return time || before.is_some_and(|w| ARTICLE_F.contains(&w.as_str()));
    };
    if time && (word == "e" || word == "h") {
        return true;
    }
    if SCALES[1..].contains(&word.as_str()) {
        return false;
    }
    match before {
        Some(w) if ARTICLE_M.contains(&w.as_str()) || LABELS.contains(&w.as_str()) => return false,
        Some(w) if ARTICLE_F.contains(&w.as_str()) => return true,

        Some(w) if ARTICLE_OR_PREPOSITION.contains(&w.as_str()) => {}
        _ => {}
    }
    if word == "mil" {
        let Some(next) = word_after(c, after) else { return false };
        (word, after) = next;
    }
    if word == "da" || word == "das" {
        return word_after(c, after).is_some_and(|(w, _)| feminine_noun(&w));
    }
    !STOP.contains(&word.as_str()) && feminine_noun(&word)
}

fn feminine_noun(word: &str) -> bool {
    let mut forms = vec![word.to_string()];
    for (plural, singular) in [("ões", "ão"), ("ns", "m"), ("zes", "z"), ("es", ""), ("s", "")] {
        if let Some(stem) = word.strip_suffix(plural) {
            forms.push(format!("{stem}{singular}"));
        }
    }
    if forms.iter().any(|f| MASCULINE.contains(&f.as_str())) {
        return false;
    }
    forms.iter().any(|f| {
        FEMININE.contains(&f.as_str())
            || ["ção", "são", "gião", "nião", "dade"].iter().any(|s| f.ends_with(s) && f.len() > s.len())
    })
}

fn listed_noun(word: &str) -> bool {
    if STOP.contains(&word) {
        return false;
    }
    let singular = [("ões", "ão"), ("es", ""), ("s", "")]
        .iter()
        .find_map(|(p, r)| word.strip_suffix(p).map(|stem| format!("{stem}{r}")))
        .unwrap_or_else(|| word.to_string());
    feminine_noun(word) || MASCULINE_NOUNS.contains(&word) || MASCULINE_NOUNS.contains(&singular.as_str())
        || MASCULINE_NOUNS.iter().any(|m| word.strip_suffix('s') == Some(m))
}
