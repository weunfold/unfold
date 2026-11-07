

use super::*;
use serde_json::json;

#[tokio::test]
async fn a_local_chat_turn_asks_for_no_thinking_at_temperature_0() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let f = fake_llama(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let cfg = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) };
    let msgs = vec![json!({"role": "system", "content": "You are Unfold."}), json!({"role": "user", "content": "hi"})];
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let r = local::stream_chat_metered(&cfg, msgs, &tx, None).await;
    drop(tx);
    while rx.recv().await.is_some() {}
    assert!(r.is_ok(), "{r:?}");
    let reqs = f.requests.lock().unwrap();
    let chat = reqs.iter().find(|b| b["stream"] == true).expect("one chat request");
    assert_eq!(chat["chat_template_kwargs"], json!({"enable_thinking": false}), "{chat}");
    assert_eq!(chat["temperature"].as_f64(), Some(0.0), "{chat}");
}

#[tokio::test]
async fn an_oversized_message_fails_as_context_exceeded() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let dir = command_dir("local-reply-too-big");
    let f = fake_llama(Mode::CtxLimit { n_ctx: 4096, chars_per_token: 2.0, pieces: vec!["Sure.".into()] }, Duration::ZERO).await;
    let rig = state_rig(&dir, "t1", local::connect(ctx_cfg(&f)), ctx_opts()).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    assert_eq!(ctx_turn(&rig, &mut c, "c0", "hi", 0).await, "Completed");
    assert_eq!(ctx_turn(&rig, &mut c, "c1", &"a very long paste ".repeat(440), 1).await, "Failed");
    assert_eq!(ctx_requests(&f, 2.0).len(), 1, "no request for the paste");
    let recs = journal_records(&rig.journal, |_| true).await;
    let reasons: Vec<_> = recs.iter().filter_map(|e| if let Event::ResponseDiagnostic { local_reason: Some(r), .. } = &e.event { Some(r.clone()) } else { None }).collect();
    assert_eq!(reasons, [local::CONTEXT_EXCEEDED], "the paste's reason");
    assert_eq!(ctx_turn(&rig, &mut c, "c2", "sorry, what was that?", 2).await, "Completed", "the next turn still fits");
}

#[test]
fn granite_42_has_its_name() {
    assert_eq!(local::display_name("granite42-3b-q4km"), "Granite 4.2 3B");
    assert!(local::is_granite("granite42-3b-q4km"), "the PT hold covers it");
}

#[test]
fn the_check_asks_the_retuned_question() {
    assert_eq!(crate::backend::text_sha256(crate::route::CHECK_PROMPT), "ac592e743f5b01b0acb74aedb2a111faf5927d03705aec6759f5e5917c06a181");
    let b = crate::route::body("granite42-3b-q4km", "hi", 1);
    assert_eq!(b["messages"][0]["content"], crate::route::CHECK_PROMPT);
    assert_eq!(b["chat_template_kwargs"], json!({"enable_thinking": false}));
}

#[tokio::test]
async fn the_granite_42_prompt_asks_for_short_plain_replies() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let f = fake_llama(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let (name, text) = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) }.prompt_for(true);
    assert!(name.ends_with("+plain"), "{name}");
    let lines: Vec<&str> = text.lines().collect();
    let typed = lines.iter().position(|l| l.starts_with(rth_self::copy::DIALOGUE_LINE)).expect("the dialogue line");
    assert!(lines[typed + 1].starts_with("Never use markdown: no bullet or numbered lists"), "{}", lines[typed + 1]);
    assert!(text.contains("Ask at most one question in a reply.") && text.contains("If the person mentions an earlier conversation"));
    assert!(text.contains("you run on Granite 4.2 3B"), "the model's own name");
    let (old_name, old) = ctx_cfg(&f).prompt_for(true);
    assert!(!old.contains("Never use markdown") && !old_name.ends_with("+plain"), "4.1 keeps its prompt");
}

#[tokio::test]
async fn a_leaked_think_tag_replaces_the_reply_and_leaves_history() {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let dir = command_dir("local-reply-think");
    let pieces = vec!["Ana buys 7 pens and 2 notebooks. ".into(), "Three packs cost 6 and the notebooks 10, so 20 - 16 = 4.\n</thi".into(), "nk>\n4".into()];
    let f = fake_llama(Mode::Stream { pieces, gap: Duration::ZERO }, Duration::ZERO).await;
    let rig = state_rig(&dir, "t1", local::connect(local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) }), ctx_opts()).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    assert_eq!(ctx_turn(&rig, &mut c, "c0", "how much change does Ana get?", 0).await, "Completed");
    let recs = journal_records(&rig.journal, |_| true).await;
    let mut shown = String::new();
    for e in &recs {
        let v = serde_json::to_value(e).unwrap();
        match v["kind"].as_str() {
            Some("agent_delta") => shown.push_str(v["text"].as_str().unwrap()),
            Some("agent_replace") => shown = v["text"].as_str().unwrap().to_string(),
            _ => {}
        }
    }
    assert_eq!(shown.trim(), "4", "what the app shows");
    assert!(recs.iter().any(|e| matches!(&e.event, Event::Log { msg, .. } if msg.contains("dropped") && msg.contains("</think>"))), "the dropped length is journaled");
    assert_eq!(ctx_turn(&rig, &mut c, "c1", "thanks", 1).await, "Completed");
    let reqs = f.requests.lock().unwrap();
    let next = reqs.iter().filter(|r| r["stream"] == true).last().unwrap()["messages"].to_string();
    assert!(!next.contains("Ana buys") && !next.contains("</think>"), "no reasoning in history: {next}");
}
