

use crate::Name;

pub fn forget_why(name: &Name) -> String {
    format!("{} won't bring this up from your saved memories again. It stays in your Brain until real deletion ships, and this chat can still see it.", name.as_str())
}

pub fn forget_reply(name: &Name, pt: bool) -> String {
    let n = name.as_str();
    if pt {
        format!("Toque em Esquecer no cartão para confirmar. Depois disso, {n} não vai trazer isso das suas memórias salvas; esta conversa ainda pode ver, e continua no seu Brain até a exclusão de verdade chegar.")
    } else {
        format!("Tap Forget on the card to confirm. After that, {n} won't bring it up from your saved memories; this chat can still see it, and it stays in your Brain until real deletion ships.")
    }
}

pub fn forget_unknown(name: &Name) -> String {
    format!("It may not have been recorded. {} won't use saved memories until it restarts and checks.", name.as_str())
}

pub fn brain_pending(name: &Name) -> String {
    format!("Your Brain copy is not marked yet; {} will retry. Saved memories already leave it out.", name.as_str())
}

pub fn who_made_en(name: &Name, model: &str, device: &str) -> String {
    format!("The we unfold open-source community made me. {} is running on {model} on this {device}.", name.as_str())
}

pub fn who_made_pt(name: &Name, model: &str, device_pt: &str) -> String {
    format!("A comunidade we unfold me criou. {} está rodando no {model} neste {device_pt}.", name.as_str())
}

pub fn gender_pt(name: &Name) -> String {
    format!("Não tenho gênero. Pode me chamar de {}.", name.as_str())
}

pub const DIALOGUE_LINE: &str = "Replies may be said aloud in the person's chosen voice. Write natural conversational replies in plain sentences and paragraphs, with no markdown, lists, tables, emoji or symbols that cannot be read aloud. Let the person's request determine the detail; give a complete answer without cutting thoughts short.";

pub const SPEECH_REQUEST_LINE: &str = "When the person types and wants to hear your reply, make ⟨say:speech⟩ the first line of your reply, alone.";

pub fn dialogue_template(text: &str) -> String {
    let mut out = text.replace(
        "This conversation is typed text in a chat window, not speech. Reply in plain text, in one to three short sentences unless the user asks for more, without markdown headings",
        DIALOGUE_LINE.trim_end_matches('.'),
    )
    .replace(" No voice, and no image, file or screen input.", " No image, file or screen input.")
    .replace(voice_line(false), " No image, file or screen input.")
    .replace("You talk by text, in this chat window.", "You talk in this chat window.")
    .replace("Text conversation in this chat window only.", "Conversation in this chat window.");
    if let Some(start) = out.find(DIALOGUE_LINE.trim_end_matches('.'))
        && !out.contains(SPEECH_REQUEST_LINE)
    {

        let end = out[start..].find("\n\n").map_or(out.len(), |i| start + i);
        out.insert_str(end, &format!("\n{SPEECH_REQUEST_LINE}"));
    }
    out
}

pub const TYPED_LINE: &str = "This conversation is typed text in a chat window, not speech.";
pub const VOICED_LINE: &str = "This conversation is in a chat window.";
pub const NO_VOICE: &str = " No voice, and no image, file or screen input.";

pub const SAY_SPEECH: &str = "⟨say:speech⟩";

pub fn dictated_line(full: bool) -> &'static str {
    if full { "This message was dictated, and your reply is said aloud in the person's chosen voice." } else { "This message was dictated, and the first sentence of your reply is said aloud in the person's chosen voice." }
}

pub fn voice_line(full: bool) -> &'static str {
    if full {
        " You have a voice: when the person dictates a message, your reply is said aloud in their chosen voice. When they type and want to hear you, make ⟨say:speech⟩ the first line of your reply, alone, and it is said aloud too; otherwise a typed message gets a text reply. No image, file or screen input."
    } else {
        " You have a voice: when the person dictates a message, the first sentence of your reply is said aloud in their chosen voice. When they type and want to hear you, make ⟨say:speech⟩ the first line of your reply, alone, and it is said aloud too; otherwise a typed message gets a text reply. No image, file or screen input."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(name: &Name) -> Vec<String> {
        vec![
            forget_why(name),
            forget_reply(name, true),
            forget_reply(name, false),
            forget_unknown(name),
            brain_pending(name),
            who_made_en(name, "Granite 4.1 3B", "Mac"),
            who_made_pt(name, "Granite 4.1 3B", "Mac"),
            gender_pt(name),
        ]
    }

    #[test]
    fn a4_the_being_is_called_by_its_name_and_the_community_keeps_its_own() {
        let lua = Name::new("Lua").unwrap();
        for s in all(&lua) {
            assert!(s.contains("Lua"), "{s}");
            assert!(!s.contains("Unfold"), "the being's name replaces Unfold: {s}");
        }
        assert!(who_made_en(&lua, "m", "Mac").starts_with("The we unfold open-source community made me."));
    }

    #[test]
    fn a4_portuguese_uses_no_article_and_no_gendered_word_for_any_name() {
        for n in ["Unfold", "Lua", "Ana Clara", "Jean-Luc"] {
            let name = Name::new(n).unwrap();
            for s in [forget_reply(&name, true), who_made_pt(&name, "Granite 4.1 3B", "Mac"), gender_pt(&name)] {
                let lower = s.to_lowercase();
                let n = n.to_lowercase();
                for article in ["o", "a", "do", "da", "no", "na", "ao", "à", "pelo", "pela"] {
                    assert!(!lower.contains(&format!(" {article} {n}")), "{article} {n}: {s}");
                }
                for gendered in ["criado", "criada", "feito", "feita", " ele ", " ela ", "dele", "dela"] {
                    assert!(!lower.contains(gendered), "{gendered}: {s}");
                }
            }
        }
        assert_eq!(who_made_pt(&Name::new("Unfold").unwrap(), "Granite 4.1 3B", "Mac"), "A comunidade we unfold me criou. Unfold está rodando no Granite 4.1 3B neste Mac.");
    }
}
