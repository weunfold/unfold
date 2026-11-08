
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Answer(String),
    Delegate,
    Fallback,
}
const MARK: &str = "[DELEGATE]";
pub fn route(text: &str, literal_name: &str) -> Route {
    if text.trim().is_empty()
        || text.len() > 8192
        || literal_name.is_empty()
        || literal_name.len() > 128
        || literal_name.contains('[')
    {
        return Route::Fallback;
    }
    let mut at = 0;
    let mut literal: Option<(String, bool)> = None;
    let mut escaped = false;
    let mut delegate = false;
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.chars().next().unwrap();
        if let Some((end, code)) = &literal {
            if escaped {
                escaped = false;
                at += c.len_utf8();
                continue;
            }
            if !code && c == '\\' {
                escaped = true;
                at += 1;
                continue;
            }
            if *code && c == '`' {
                let n = rest.bytes().take_while(|b| *b == b'`').count();
                let closes = n == end.len();
                at += n;
                if closes {
                    literal = None;
                }
            } else if rest.starts_with(end.as_str()) {
                let internal = (end == "'" || end == "’")
                    && text[..at].chars().next_back().is_some_and(char::is_alphanumeric)
                    && rest[c.len_utf8()..].chars().next().is_some_and(char::is_alphanumeric);
                if internal {
                    at += c.len_utf8();
                } else {
                    at += end.len();
                    literal = None;
                }
            } else {
                at += c.len_utf8();
            }
            continue;
        }
        if rest.starts_with(MARK) {
            delegate = true;
            at += MARK.len();
            continue;
        }
        if c == '`' {
            let n = rest.bytes().take_while(|b| *b == b'`').count();
            literal = Some(("`".repeat(n), true));
            at += n;
            continue;
        }
        let internal_apostrophe = c == '\'' && text[..at].chars().next_back().is_some_and(char::is_alphanumeric);
        let end = match c {
            '"' => Some("\""),
            '\'' if !internal_apostrophe => Some("'"),
            '“' => Some("”"),
            '‘' => Some("’"),
            '«' => Some("»"),
            _ => None,
        };
        if let Some(end) = end {
            literal = Some((end.into(), false));
        } else if matches!(c, '”' | '»') || (c == '[' && rest.len() < MARK.len() && MARK.starts_with(rest)) {
            return Route::Fallback;
        }
        at += c.len_utf8();
    }
    if literal.is_some() {
        Route::Fallback
    } else if delegate {
        Route::Delegate
    } else {
        Route::Answer(text.replace(MARK, literal_name))
    }
}
