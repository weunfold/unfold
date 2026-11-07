

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

#[derive(Default)]
pub struct Cards {
    boot: Option<String>,
    tokens: BTreeMap<String, String>,

    cancelled: BTreeSet<String>,
}

pub static CARDS: Mutex<Cards> = Mutex::new(Cards { boot: None, tokens: BTreeMap::new(), cancelled: BTreeSet::new() });

impl Cards {

    pub fn on_frame(&mut self, f: &Value) -> Option<Value> {
        let id = f["effect_id"].as_str()?;
        match f["kind"].as_str()? {
            "effect_card" => {
                let token = f["token"].as_str().filter(|t| !t.is_empty())?;
                if self.cancelled.contains(id) {
                    return None;
                }
                self.tokens.insert(id.to_string(), token.to_string());
                let mut w = json!({"wire": "effect_card", "effect_id": id});
                for k in ["effect_kind", "summary", "expires_utc_ms"] {
                    if !f[k].is_null() {
                        w[k] = f[k].clone();
                    }
                }
                Some(w)
            }
            "effect_released" => {
                let mut w = json!({"wire": "effect_update", "effect_id": id, "state": "released"});
                for (k, v) in [("effect_kind", &f["effect_kind"]), ("memory_id", &f["result"]["memory_id"]), ("undo_effect_id", &f["result"]["undo_effect_id"])] {
                    if v.is_string() {
                        w[k] = v.clone();
                    }
                }
                Some(w)
            }
            "effect_refused" => {
                let mut w = json!({"wire": "effect_update", "effect_id": id, "state": "refused"});
                if f["effect_kind"].is_string() {
                    w["effect_kind"] = f["effect_kind"].clone();
                }
                if let Some(r) = f["reason"].as_str() {
                    w["reason"] = json!(r);
                }
                Some(w)
            }
            _ => None,
        }
    }

    pub fn take(&mut self, id: &str) -> Option<String> {
        self.tokens.remove(id)
    }

    pub fn cancel(&mut self, id: &str) -> bool {
        self.cancelled.insert(id.to_string());
        self.tokens.remove(id).is_some()
    }

    pub fn on_boot(&mut self, boot: &str) -> bool {
        let lost = self.boot.as_deref().is_some_and(|b| b != boot);
        if lost {
            self.tokens.clear();
            self.cancelled.clear();
        }
        self.boot = Some(boot.to_string());
        lost
    }
}

pub fn release_text(id: &str, token: &str) -> String {
    format!("/effect release {id} {token}")
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(c: &mut Cards, id: &str, token: &str) -> Option<Value> {
        c.on_frame(&json!({"kind": "effect_card", "effect_id": id, "effect_kind": "forget", "token": token, "summary": "My dog is called Sol", "why": "x", "expires_utc_ms": 5}))
    }

    #[test]
    fn a_card_reaches_the_page_without_its_token() {
        let mut c = Cards::default();
        let w = card(&mut c, "e-7", "tok-secret-1").unwrap();
        assert_eq!(w, json!({"wire": "effect_card", "effect_id": "e-7", "effect_kind": "forget", "summary": "My dog is called Sol", "expires_utc_ms": 5}));
        assert!(!w.to_string().contains("tok-secret-1"));
        assert_eq!(c.on_frame(&json!({"kind": "effect_card", "effect_id": "e-8", "effect_kind": "forget"})), None, "no token: not a card anyone can tap");
    }

    #[test]
    fn a_tap_takes_the_token_once() {
        let mut c = Cards::default();
        card(&mut c, "e-7", "tok-1");
        assert_eq!(c.take("e-7").as_deref(), Some("tok-1"));
        assert_eq!(c.take("e-7"), None, "a double tap sends nothing");
        assert_eq!(c.take("e-404"), None);
        assert_eq!(release_text("e-7", "tok-1"), "/effect release e-7 tok-1");
    }

    #[test]
    fn a_cancel_means_no_release_ever() {
        let mut c = Cards::default();
        card(&mut c, "e-7", "tok-1");
        assert!(c.cancel("e-7"));
        assert_eq!(c.take("e-7"), None);
        assert!(!c.cancel("e-7"));
    }

    #[test]
    fn a_cancelled_card_re_sent_on_re_attach_stays_cancelled() {

        let mut c = Cards::default();
        card(&mut c, "e-7", "tok-1");
        assert!(c.cancel("e-7"));
        assert_eq!(card(&mut c, "e-7", "tok-1"), None, "not shown again");
        assert_eq!(c.take("e-7"), None, "and never releasable");
        assert!(card(&mut c, "e-8", "tok-2").is_some(), "another card still is");
    }

    #[test]
    fn a_new_agentd_boot_loses_the_cards_and_the_same_boot_keeps_them() {
        let mut c = Cards::default();
        assert!(!c.on_boot("b1"), "the first boot loses nothing");
        card(&mut c, "e-7", "tok-1");
        assert!(!c.on_boot("b1"), "a reconnect to the same agentd keeps its cards");
        assert_eq!(c.take("e-7").as_deref(), Some("tok-1"));
        card(&mut c, "e-8", "tok-2");
        assert!(c.on_boot("b2"), "agentd restarted: the page is told");
        assert_eq!(c.take("e-8"), None, "an old boot's token is never sent");
    }

    #[test]
    fn outcomes_reach_the_page_by_effect_id() {
        let mut c = Cards::default();
        assert_eq!(c.on_frame(&json!({"kind": "effect_released", "effect_id": "e-7", "effect_kind": "forget", "args_sha256": "a", "result": {"memory_id": "m-1", "undo_effect_id": "e-9", "brain": "ok"}})),
            Some(json!({"wire": "effect_update", "effect_id": "e-7", "effect_kind": "forget", "state": "released", "memory_id": "m-1", "undo_effect_id": "e-9"})));
        assert_eq!(c.on_frame(&json!({"kind": "effect_released", "effect_id": "e-9", "effect_kind": "restore", "args_sha256": "a", "result": {"memory_id": "m-1", "eligibility_rev": 3, "brain": "restored"}})),
            Some(json!({"wire": "effect_update", "effect_id": "e-9", "effect_kind": "restore", "state": "released", "memory_id": "m-1"})), "an Undo names the memory it brought back");
        assert_eq!(c.on_frame(&json!({"kind": "effect_refused", "effect_id": "e-7", "effect_kind": "forget", "reason": "stale", "why": "Nothing changed"})),
            Some(json!({"wire": "effect_update", "effect_id": "e-7", "effect_kind": "forget", "state": "refused", "reason": "stale"})));
        assert_eq!(c.on_frame(&json!({"kind": "effect_held", "effect_id": "e-7"})), None, "the card carries what the page needs");
        assert_eq!(c.on_frame(&json!({"kind": "agent_delta"})), None);
    }

    #[test]
    fn the_page_action_names_one_effect_id() {
        assert!(valid_id("b1-e7") && valid_id("e_9"));
        assert!(!valid_id("") && !valid_id("e 7") && !valid_id("e-7 tok") && !valid_id(&"e".repeat(65)) && !valid_id("e;rm"));
    }
}
