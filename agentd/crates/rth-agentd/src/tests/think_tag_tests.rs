

use super::*;
use serde_json::json;

fn shown(recs: &[Envelope]) -> String {
    let mut s = String::new();
    for e in recs {
        match &e.event {
            Event::AgentDelta { text, .. } => s.push_str(text),
            Event::AgentReplace { text, .. } => s = text.clone(),
            _ => {}
        }
    }
    s
}

fn replaces(recs: &[Envelope]) -> usize {
    recs.iter().filter(|e| matches!(e.event, Event::AgentReplace { .. })).count()
}

fn dropped_log(recs: &[Envelope]) -> bool {
    recs.iter().any(|e| matches!(&e.event, Event::Log { msg, .. } if msg.contains("visible reasoning")))
}

async fn leak_rig(name: &str, pieces: &[&str]) -> (Rig, local::fake_server::Fake) {
    use crate::local::fake_server::{start as fake_llama, Mode};
    let f = fake_llama(Mode::Stream { pieces: pieces.iter().map(|p| p.to_string()).collect(), gap: Duration::ZERO }, Duration::ZERO).await;
    let rig = state_rig(&command_dir(name), "t1", local::connect(local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) }), ctx_opts()).await;
    (rig, f)
}

#[tokio::test]
async fn a_flash_reply_that_mentions_the_think_tag_streams_unchanged() {
    use crate::flash::test_server::{start, Answer};
    let parts = ["Remove it with re.sub on the pattern <think>.*?</think> and an empty string. ", "Then print what is left."];
    let events = parts.iter().map(|t| json!({"model": "deepseek-flash", "choices": [{"delta": {"content": t}}]}).to_string()).collect();
    let s = start(Answer::Stream { events, gap: Duration::ZERO, hang: false }).await;
    let dir = command_dir("think-tag-flash-tag");
    let (rig, _f) = reasoning_limits_rig(&dir, "b1", &s.url, false).await;
    let recs = reasoning_limits_one_turn(&rig, "how do I strip the think block from a reply in python?", None).await;
    assert_eq!(replaces(&recs), 0, "no agent_replace on a Flash turn");
    assert!(!dropped_log(&recs), "nothing logged as dropped reasoning");

    assert_eq!(shown(&recs), parts.concat());
}

#[tokio::test]
async fn the_replacement_starts_at_its_first_visible_char() {
    for (name, pieces) in [("think-tag-lead-1", vec!["Three packs cost 6, so 20 - 16 = 4.\n</think>\n4"]), ("think-tag-lead-2", vec!["Three packs cost 6, so 20 - 16 = 4.\n</think>", "\n", " 4"])] {
        let (rig, _f) = leak_rig(name, &pieces).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        assert_eq!(ctx_turn(&rig, &mut c, "c0", "how much change does Ana get?", 0).await, "Completed");
        let recs = journal_records(&rig.journal, |_| true).await;
        assert_eq!(shown(&recs), "4", "{name}: exactly the answer");
        assert_eq!(replaces(&recs), 1, "{name}");
    }
}

#[tokio::test]
async fn a_held_turn_cuts_inside_its_held_reply() {
    let (rig, f) = leak_rig("think-tag-held", &["Let me look through our chats for that. ", "</think>\n", "I don't have that earlier conversation, so I can't recall what was said."]).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    assert_eq!(ctx_turn(&rig, &mut c, "c0", "what did we talk about last time we chatted?", 0).await, "Completed");
    let recs = journal_records(&rig.journal, |_| true).await;
    assert_eq!(replaces(&recs), 0, "a held reply is swapped before anything is shown");
    assert_eq!(shown(&recs), "I don't have that earlier conversation, so I can't recall what was said.");
    assert!(dropped_log(&recs));
    assert_eq!(ctx_turn(&rig, &mut c, "c1", "ok", 1).await, "Completed");
    let next = f.requests.lock().unwrap().iter().filter(|r| r["stream"] == true).last().unwrap()["messages"].to_string();
    assert!(!next.contains("look through our chats") && !next.contains("</think>"), "no reasoning in history: {next}");
}

#[tokio::test]
async fn a_voiced_turn_is_cut_too() {
    let (rig, _f) = leak_rig("think-tag-voiced", &["Ana buys 7 pens. So 4.\n</th", "ink>\n4"]).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    let generation = Client::generation(&rig).await.unwrap_or(0);
    let voice = rth_protocol::ReplyVoice { dictated: true, spoken: rth_protocol::SpokenReply::Full };
    c.send(Event::CommandSubmitted { command_id: "c0".into(), session_id: SESSION.into(), expected_generation: generation, text: "how much change does Ana get?".into(), speaker: Default::default(), reply_voice: Some(voice), effort: None, reply_max_tokens: None }).await;
    let recs = journal_records(&rig.journal, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await;
    assert_eq!(shown(&recs), "4");
    assert_eq!(replaces(&recs), 1);
}
