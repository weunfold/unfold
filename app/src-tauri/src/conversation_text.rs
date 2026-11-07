
pub fn clean(reply: &str) -> String {
    let mut out = String::new();
    let mut fence = false;
    for line in reply.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") { fence = !fence; continue; }
        if fence { continue; }
        let mut t = line.trim_start_matches(['#', '>', '-', '*', '+', '•', ' ']);
        if let Some((lead, rest)) = t.split_once(['.', ')']) {
            if !lead.is_empty() && lead.chars().all(|c| c.is_ascii_digit()) && rest.starts_with(' ') { t = rest.trim_start(); }
        }
        let mut chars = t.chars().peekable();
        let mut inline = false;
        while let Some(c) = chars.next() {
            if c == '⟨' { for x in chars.by_ref() { if x == '⟩' { break; } } continue; }
            if c == '`' { inline = !inline; continue; }
            if inline { continue; }
            if c == ']' && chars.peek() == Some(&'(') {
                chars.next(); let mut depth = 1;
                for x in chars.by_ref() { if x == '(' { depth += 1; } else if x == ')' { depth -= 1; if depth == 0 { break; } } }
                continue;
            }

            if (c == 'h' || c == 'w') && (out.ends_with(char::is_whitespace) || out.is_empty()) {
                let tail = chars.clone().collect::<String>();
                if (c == 'h' && (tail.starts_with("ttps://") || tail.starts_with("ttp://"))) || (c == 'w' && tail.starts_with("ww.")) {
                    while chars.peek().is_some_and(|c| !c.is_whitespace()) { chars.next(); }
                    out.push(' '); continue;
                }
            }
            if matches!(c, '*' | '_' | '[' | ']' | '~') { continue; }
            if c.is_alphabetic() || c.is_ascii_digit() || c.is_whitespace() || ".,!?;:'’\"()-–—/:%".contains(c) { out.push(c); }
            else { out.push(' '); }
        }
        out.push(' ');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn parts(reply: &str) -> Vec<String> {
    let text = clean(reply);
    let mut parts = Vec::new(); let mut start = 0;
    for (i, c) in text.char_indices() {
        let end = i + c.len_utf8();
        if matches!(c, '.' | '!' | '?') && text[end..].chars().next().is_none_or(char::is_whitespace) {
            let part = text[start..end].trim();
            if part.chars().any(char::is_alphanumeric) { parts.push(part.to_string()); }
            start = end;
        }
    }
    let tail = text[start..].trim();
    if tail.chars().any(char::is_alphanumeric) { parts.push(tail.to_string()); }
    parts
}
pub fn complete(reply: &str) -> Vec<String> {
    let mut ready = parts(reply);

    ready.pop();
    ready.into_iter().take_while(|s| s.ends_with(['.', '?', '!'])).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_markdown_bullets_emoji_urls_and_code_in_both_languages() {
        let text = "⟨say:speech mood=joy⟩\n# **Hello!** 😊\n- Read [the guide](https://example.test/a_(b)).\n2. Olá, você pode seguir. https://example.test/?q=1&x=2\n```rust\nsecret();\n```\nUse `cargo test` on screen.";
        assert_eq!(clean(text), "Hello! Read the guide. Olá, você pode seguir. Use on screen.");
        assert_eq!(clean("Go www.example.test now. 🎉"), "Go now.");
    }
    #[test]
    fn streams_first_and_later_finished_sentences_with_an_unfinished_tail() {
        assert!(complete("Hello").is_empty());
        assert!(complete("Hello.").is_empty());
        assert_eq!(complete("Hello. **How are you?** Still writing"), ["Hello.", "How are you?"]);
        assert_eq!(parts("Hello. Final tail"), ["Hello.", "Final tail"]);
    }
}
