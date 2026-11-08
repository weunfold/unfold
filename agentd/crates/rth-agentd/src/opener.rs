

use crate::local::{self, Config};
use rth_protocol::{ReplyVoice, SpokenReply};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_millis(450);
const PROMPT: &str = "Choose a neutral spoken acknowledgment for this message, in its language. Output exactly one of: NONE | Hmm. | Let me think. | Hmm, let me think. | Deixa eu pensar. | Hmm, deixa eu pensar. Never answer the question, state a fact, claim a result or promise any action. Greetings and thanks: NONE. No reasoning, tags or other words.";

#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub text: String,
    pub ms: u64,
}

pub struct Pending(pub tokio::task::JoinHandle<Option<Ready>>);
impl Pending {
    #[cfg(test)]
    pub fn start(
        cfg: Config,
        text: &str,
        voice: Option<ReplyVoice>,
        admitted: Instant,
    ) -> Option<Self> {
        Self::start_with_context(cfg, text, &[], voice, admitted)
    }
    pub fn start_with_context(
        cfg: Config,
        text: &str,
        context: &[crate::context::Turn],
        voice: Option<ReplyVoice>,
        admitted: Instant,
    ) -> Option<Self> {
        let language = local::reply_language(context, text);
        let text = local::genuine_utterance(text);
        if !voice.is_some_and(|v| v.spoken == SpokenReply::Full || v.dictated)
            || social_reply(text).is_some()
            || cfg.opener_slot.is_none()
            || cfg.slot.is_none()
            || cfg.opener_slot == cfg.slot
            || cfg.opener_slot == cfg.decide_slot
            || !matches!(local::dial(&cfg.authority), local::Dial::Loopback)
        {
            return None;
        }
        let words = text.to_string();
        Some(Self(tokio::spawn(async move {

            let front = Config {
                slot: cfg.opener_slot,
                max_tokens: Some(16),
                first_token: DEADLINE,
                ..cfg
            };
            let (tx, mut rx) = tokio::sync::mpsc::channel(64);
            let work = local::stream_chat(
                &front,
                vec![
                    serde_json::json!({"role":"system","content":match language {
                        Some(lang) => format!("{PROMPT} {}", local::language_hint(lang)),
                        None => PROMPT.to_string(),
                    }}),
                    serde_json::json!({"role":"user","content":words}),
                ],
                &tx,
                language,
            );
            let result = tokio::time::timeout(DEADLINE, work).await;
            drop(tx);
            if !matches!(result, Ok(Ok(_))) {
                return None;
            }
            let mut text = String::new();
            while let Ok(piece) = rx.try_recv() {
                text.push_str(&piece);
            }
            let text = validated(&text, language)?;
            Some(Ready {
                text,
                ms: admitted.elapsed().as_millis() as u64,
            })
        })))
    }
    pub async fn finish(mut self) -> Option<Ready> {
        (&mut self.0).await.ok().flatten()
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn validated(raw: &str, lang: Option<local::Lang>) -> Option<String> {

    let raw = raw
        .trim()
        .strip_prefix("<think></think>")
        .unwrap_or(raw.trim())
        .trim();
    let allowed: &[&str] = match lang {
        Some(local::Lang::Pt) => &["Hmm.", "Deixa eu pensar.", "Hmm, deixa eu pensar."],
        _ => &["Hmm.", "Let me think.", "Hmm, let me think."],
    };
    allowed.contains(&raw).then(|| raw.to_string())
}

pub fn social_reply(text: &str) -> Option<&'static str> {
    match local::genuine_utterance(text)
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_lowercase()
        .as_str()
    {
        "hello" | "hi" | "hey" => Some("Hello!"),
        "oi" | "olá" | "ola" => Some("Oi!"),
        "no, thanks" | "no thanks" => Some("No problem."),
        "thanks" | "thank you" => Some("You're welcome!"),
        "obrigado" | "obrigada" | "valeu" => Some("De nada!"),
        _ => None,
    }
}

pub fn memory_detour(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "memor", "memór", "saved", "salv", "recall", "remember", "lembr", "guardad",
    ]
    .iter()
    .any(|s| lower.contains(s))
}

#[derive(Default)]
pub struct Prefix {
    expected: Option<String>,
    pending: String,
}
impl Prefix {
    pub fn new(opener: Option<String>) -> Self {
        Self {
            expected: opener,
            pending: String::new(),
        }
    }
    pub fn push(&mut self, piece: &str) -> String {
        let Some(expected) = &self.expected else {
            return piece.into();
        };
        self.pending.push_str(piece);
        let candidate = self.pending.trim_start();
        if expected.starts_with(candidate) && candidate.len() < expected.len() {
            return String::new();
        }
        let result = candidate
            .strip_prefix(expected)
            .map(|s| s.trim_start().to_string())
            .unwrap_or_else(|| std::mem::take(&mut self.pending));
        self.pending.clear();
        self.expected = None;
        result
    }
    pub fn finish(&mut self) -> String {
        self.expected = None;
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opener_rejects_probe_facts_and_unpromised_actions() {
        for raw in [
            "A capital da Austrália é Canberra.",
            "I can't give real-time weather.",
            "Sure! A heat pump moves heat.",
            "Claro! Aqui vai uma curiosidade sobre o espaço:",
            "Hey there! I'm doing great!",
            "I will check your email.",
            "I saved that.",
            "Yes.",
            "No.",
            "Done.",
            "Your name is Sam.",
            "Remembered.",
            "<think>secret</think>Hmm.",
            "NONE",
            "Hmm. The answer is 4.",
        ] {
            assert_eq!(validated(raw, None), None, "{raw}");
            assert_eq!(validated(raw, Some(local::Lang::Pt)), None, "{raw}");
        }
        assert_eq!(
            validated("<think></think>Let me think.", None).as_deref(),
            Some("Let me think.")
        );
        assert_eq!(
            validated("Hmm, deixa eu pensar.", Some(local::Lang::Pt)).as_deref(),
            Some("Hmm, deixa eu pensar.")
        );
    }
    #[test]
    fn unset_opener_slot_is_a_noop_even_for_voice() {
        let cfg = Config {
            authority: "127.0.0.1:1".into(),
            base_path: "/v1".into(),
            model: "test-model".into(),
            model_sha256: None,
            slot: Some(0),
            decide_slot: Some(3),
            opener_slot: None,
            max_tokens: None,
            n_ctx: 8192,
            first_token: Duration::from_secs(5),
            system_instruction: None,
            health_every: Duration::from_secs(60),
        };
        let voice = Some(ReplyVoice {
            dictated: true,
            spoken: SpokenReply::Full,
        });
        assert!(Pending::start(cfg, "Explain a heat pump.", voice, Instant::now()).is_none());
    }

    #[test]
    fn echoed_opener_is_removed_across_every_split() {
        let text = "Let me think. A heat pump moves heat.";
        for split in 0..=text.len() {
            let mut prefix = Prefix::new(Some("Let me think.".into()));
            let out = prefix.push(&text[..split]) + &prefix.push(&text[split..]) + &prefix.finish();
            assert_eq!(out.trim_start(), "A heat pump moves heat.", "split {split}");
        }
        let mut prefix = Prefix::new(Some("Hmm.".into()));
        assert_eq!(
            prefix.push("Heat pumps move heat."),
            "Heat pumps move heat."
        );
    }
}

#[cfg(test)]
mod wrapped_social_utterance_tests {
    use super::*;
    #[test]
    fn wrapped_social_turn_is_still_a_genuine_social_turn() {
        let input = "[Unfold was interrupted. Complete speech segments rendered before interruption: \"The answer is here.\". Any partial final segment is omitted; the rest of the generated reply was not rendered. This is a playout observation, not a correction of saved history.]\n\nValeu!";
        assert_eq!(social_reply(input), Some("De nada!"));
    }
}
