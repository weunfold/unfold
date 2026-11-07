

#![cfg_attr(not(feature = "menu_bar"), allow(dead_code))]

use serde_json::{json, Value};
use std::sync::Arc;

pub const LEVELS: [&str; 5] = crate::settings::EFFORTS;

pub struct Sources {

    pub local_name: String,

    pub deepseek: Option<(String, bool)>,

    pub bridge: Vec<crate::dsh::Model>,
}

pub fn options(src: &Sources) -> Vec<Value> {
    let mut o = vec![json!({"id": crate::settings::ANSWER_GRANITE, "name": src.local_name, "where": "mac", "provider": null, "memories": true, "confirm": false, "efforts": false})];
    if let Some((name, confirm)) = &src.deepseek {
        o.push(json!({"id": crate::settings::ANSWER_DEEPSEEK, "name": name, "where": "direct", "provider": "DeepSeek", "memories": true, "confirm": confirm, "efforts": true}));
    }
    for m in crate::dsh::offered(&src.bridge) {
        let provider = m.provider.clone().unwrap_or_else(|| m.id.split_once('/').map_or(m.id.clone(), |(p, _)| p.to_string()));
        o.push(json!({"id": m.id, "name": m.name, "where": "automagik", "provider": provider, "memories": false, "confirm": false, "efforts": false}));
    }
    o
}

pub fn facts(rev: u64, chosen: &str, answering: Option<&str>, why: Option<&str>, effort: &str, options: Vec<Value>) -> Value {
    json!({"rev": rev, "chosen": chosen, "answering": answering, "why": why, "effort": {"chosen": effort, "levels": LEVELS}, "options": options})
}

pub fn check(facts: &Value, choice: Option<&str>, effort: Option<&str>, ack: bool) -> Result<(), (u16, &'static str)> {
    let option = match choice {
        Some(c) => Some(facts["options"].as_array().into_iter().flatten().find(|o| o["id"] == c).ok_or((409, "not_offered"))?),
        None => None,
    };
    if let Some(e) = effort {
        if !facts["effort"]["levels"].as_array().is_some_and(|l| l.iter().any(|x| x == e)) {
            return Err((409, "not_offered"));
        }
    }
    if option.is_some_and(|o| o["confirm"] == true) && !ack {
        return Err((409, "needs_ack"));
    }
    Ok(())
}

struct App;

impl crate::dsh_control::AnswerWith for App {
    fn facts(&self) -> Value {
        app_facts()
    }

    fn choose(&self, choice: Option<&str>, effort: Option<&str>, ack: bool) -> Result<Value, (u16, &'static str)> {
        check(&app_facts(), choice, effort, ack)?;
        let (rev, after_reply) = crate::settings::choose(choice, effort, ack, "Unfold Automagik's menu").map_err(|e| {
            if let Some(c) = crate::settings::CTX.get() {
                c.log.line(&format!("answer-with: the pick was not saved: {e}"));
            }
            (503, "unavailable")
        })?;
        Ok(json!({"rev": rev, "after_reply": after_reply}))
    }
}

pub fn app() -> Arc<dyn crate::dsh_control::AnswerWith> {
    Arc::new(App)
}

fn app_facts() -> Value {
    let s = crate::settings::current();
    let src = crate::dsh::source();
    let Some(c) = crate::settings::CTX.get() else {
        return facts(crate::settings::rev(), &s.answer_with, None, None, &s.deepseek_effort, Vec::new());
    };
    let bridge = if s.automagik_on() && crate::dsh::bridge_live(&src) {
        crate::dsh::list_now(&src, &c.log);
        crate::dsh::listed()
    } else {
        Vec::new()
    };
    let deepseek = crate::flash::provider(&c.data, None).filter(|p| crate::flash::key_present(&p.keychain_service) == Some(true)).map(|p| (p.label, crate::flash::GRANT_NEEDS_ACK && !s.cloud_ack));
    let talk = crate::models::load_choice(&c.data).and_then(|ch| ch["talk"].as_str().map(crate::settings::model_name_of));
    let local_name = talk.or_else(|| Some(crate::flash::local_name()).filter(|n| !n.is_empty())).unwrap_or_else(|| "Granite 4.2 3B".into());
    let answering = answering();
    let why = answering.as_deref().filter(|a| *a != s.answer_with).and_then(|_| crate::flash::local_reason().or_else(crate::dsh::local_reason).map(|(w, _)| w));
    facts(crate::settings::rev(), &s.answer_with, answering.as_deref(), why, &s.deepseek_effort, options(&Sources { local_name, deepseek, bridge }))
}

fn answering() -> Option<String> {
    if crate::settings::SWITCHING.load(std::sync::atomic::Ordering::SeqCst) || !crate::chat::attached() {
        return None;
    }
    Some(crate::dsh::answering_model().unwrap_or_else(|| if crate::flash::direct() { crate::settings::ANSWER_DEEPSEEK } else { crate::settings::ANSWER_GRANITE }.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsh::Model;

    fn src(deepseek: bool, bridge: Vec<Model>) -> Sources {
        Sources { local_name: "Granite 4.1 3B".into(), deepseek: deepseek.then(|| ("DeepSeek V4.1 Flash".into(), false)), bridge }
    }

    #[test]
    fn granite_always_deepseek_once_with_its_key_and_the_bridge_s_ready_models() {
        let codex = Model { provider: Some("ChatGPT".into()), ready: Some(true), ..Model::new("openai-codex/gpt-5.5", "GPT-5.5") };
        let kimi_out = Model { ready: Some(false), ..Model::new("kimi/kimi-k2", "Kimi K2") };
        let ds = Model { ready: Some(true), ..Model::new("deepseek-official/deepseek-flash", "DeepSeek Flash") };
        let o = options(&src(true, vec![codex, kimi_out, ds]));
        assert_eq!(
            o,
            vec![
                json!({"id": "granite", "name": "Granite 4.1 3B", "where": "mac", "provider": null, "memories": true, "confirm": false, "efforts": false}),
                json!({"id": "deepseek", "name": "DeepSeek V4.1 Flash", "where": "direct", "provider": "DeepSeek", "memories": true, "confirm": false, "efforts": true}),
                json!({"id": "openai-codex/gpt-5.5", "name": "GPT-5.5", "where": "automagik", "provider": "ChatGPT", "memories": false, "confirm": false, "efforts": false}),
            ],
            "DeepSeek once (direct, with memories); a bridge model not ready and the bridge's DeepSeek left out"
        );
        let ids: Vec<Value> = options(&src(false, vec![])).iter().map(|o| o["id"].clone()).collect();
        assert_eq!(ids, [json!("granite")], "no provider or no key: no DeepSeek; Unfold Automagik off: no bridge models");
        let glm = options(&src(false, vec![Model::new("glm/glm-5", "GLM 5")]));
        assert_eq!(glm[1]["provider"], "glm", "no provider name: the route's id");
    }

    #[test]
    fn the_facts_carry_exactly_the_contract_s_fields() {
        let f = facts(7, "deepseek", None, None, "auto", options(&src(true, vec![])));
        let mut keys: Vec<&String> = f.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["answering", "chosen", "effort", "options", "rev", "why"]);
        assert_eq!(f["effort"], json!({"chosen": "auto", "levels": ["auto", "off", "low", "high", "max"]}), "no Medium");
        assert_eq!(f["answering"], Value::Null, "null while agentd restarts");
        for o in f["options"].as_array().unwrap() {
            let mut k: Vec<&String> = o.as_object().unwrap().keys().collect();
            k.sort();
            assert_eq!(k, ["confirm", "efforts", "id", "memories", "name", "provider", "where"]);
        }
    }

    #[test]
    fn a_pick_must_be_offered_and_a_confirm_needs_its_ack() {
        let mut f = facts(1, "deepseek", Some("deepseek"), None, "auto", options(&src(true, vec![Model { ready: Some(true), ..Model::new("kimi/kimi-k2", "Kimi K2") }])));
        assert_eq!(check(&f, Some("granite"), None, false), Ok(()));
        assert_eq!(check(&f, Some("kimi/kimi-k2"), Some("high"), false), Ok(()));
        assert_eq!(check(&f, None, Some("max"), false), Ok(()));
        for (choice, effort) in [(Some("deepseek-official/deepseek-flash"), None), (Some("glm/glm-5"), None), (Some(""), None), (None, Some("medium")), (None, Some("xhigh")), (Some("granite"), Some("none"))] {
            assert_eq!(check(&f, choice, effort, false), Err((409, "not_offered")), "{choice:?} {effort:?}");
        }
        f["options"][1]["confirm"] = json!(true);
        assert_eq!(check(&f, Some("deepseek"), None, false), Err((409, "needs_ack")));
        assert_eq!(check(&f, Some("deepseek"), None, true), Ok(()));
        assert_eq!(check(&f, None, Some("high"), false), Ok(()), "a level alone needs no confirm");
    }
}
