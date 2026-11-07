
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
#[allow(dead_code)]
#[path = "h5c/support.rs"]
mod support;
#[allow(dead_code)]
#[path = "../src/voice_front.rs"]
mod voice_front;
use serde_json::{json, Value};
use support::*;
use voice_front::{context::*, Error};
struct Count;
impl Counter for Count {
    fn text(&mut self, text: &str) -> Result<usize, Error> {
        Ok(text.chars().count())
    }
    fn prompt(&mut self, messages: &[Value]) -> Result<usize, Error> {
        Ok(messages.iter().map(|m| m["content"].as_str().unwrap().chars().count() + 8).sum())
    }
}
fn turn(n: usize) -> conversation_log::Turn {
    use conversation_log::{
        observe::{Payload, Span, Terminal},
        Owner,
    };
    conversation_log::Turn {
        id: format!("turn-{n}"),
        response_id: format!("reply-{n}"),
        observation_id: format!("observe-{n}"),
        nonce: format!("nonce-{n}"),
        input: "u".repeat(80),
        owner: Some(Owner::Front),
        rendered: true,
        presentation_uncertain: false,
        legacy_attempted: false,
        backend_response: None,
        acknowledged: false,
        presentation: Some(Payload::Assistant {
            spans: vec![Span { first: 0, end: 100, text: "a".repeat(80) }],
            terminal: Terminal::Interrupted,
        }),
        silent_replay: false,
    }
}
fn tail() -> Tail {
    Tail { locale: "pt-BR".into(), status: "idle".into(), evidence: String::new() }
}
#[test]
fn context_is_append_only_keeps_exact_input_and_only_terminal_rendered_words() {
    let mut ctx = Context::new("stable instructions").unwrap();
    let t = turn(1);
    ctx.append_rendered(&t).unwrap();
    let p = ctx.prepare(&mut Count, &tail(), "current exact input").unwrap();
    let body = p.payload();
    assert_eq!(body["messages"][0]["content"], "stable instructions");
    assert_eq!(body["messages"][1]["content"], t.input);
    assert_eq!(body["messages"][2]["content"], "a".repeat(80));
    assert_eq!(body["messages"].as_array().unwrap().last().unwrap()["content"], "current exact input");
    assert_eq!(body["id_slot"], 0);
    assert_eq!(body["max_tokens"], 96);
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
    assert!(ctx.trim(&mut Count, true).is_err());
    drop(p);
    let before = ctx.prefix();
    ctx.append_rendered(&turn(2)).unwrap();
    assert_eq!(&ctx.prefix()[..before.len()], before.as_slice());
    assert!(ctx.prepare(&mut Count, &tail(), &"x".repeat(481)).is_err());
    let mut unknown = turn(3);
    unknown.presentation = None;
    assert!(ctx.append_rendered(&unknown).is_err());
}
#[test]
fn trim_is_large_and_quiet_and_blocks_generation_until_matching_prefill() {
    let mut ctx = Context::new("stable").unwrap();
    for n in 0..9 {
        ctx.append_rendered(&turn(n)).unwrap();
    }
    assert!(ctx.trim(&mut Count, false).is_err());
    let change = ctx.trim(&mut Count, true).unwrap();
    assert!(change.removed_tokens >= 512);
    assert_eq!(ctx.prefix().len(), 11);
    assert!(ctx.prepare(&mut Count, &tail(), "new").is_err());
    let prefix = ctx.prefill_plan(true).unwrap();
    assert!(ctx.prefill_plan(true).is_err());
    drop(prefix);
    assert!(ctx.prefill_plan(false).is_err());
}
#[test]
fn server_template_and_tokenizer_counts_are_used_without_a_character_estimate() {
    let (mut owner, listener) = bound();
    let server = json_server(
        listener,
        vec![json!({"tokens":[1,2,3]}), json!({"prompt":"EXACT APPLIED TEMPLATE"}), json!({"tokens":[1,2]})],
    );
    let lease = owner.request().unwrap();
    let mut counter = RemoteCounter(&lease);
    assert_eq!(counter.text(&"x".repeat(1000)).unwrap(), 3);
    assert_eq!(counter.prompt(&[json!({"role":"user","content":"input"})]).unwrap(), 2);
    let sent = server.join().unwrap();
    assert_eq!(sent[2]["content"], "EXACT APPLIED TEMPLATE");
    owner.close().unwrap();
}
#[test]
fn prefill_is_zero_generation_and_stale_correction_and_child_receipts_are_fenced() {
    let mut ctx = Context::new("stable").unwrap();
    for n in 0..9 {
        ctx.append_rendered(&turn(n)).unwrap();
    }
    ctx.trim(&mut Count, true).unwrap();
    let (mut owner, listener) = bound();
    let server = json_server(
        listener,
        vec![
            json!({"prompt":"prefix one"}),
            json!({"tokens_predicted":0}),
            json!({"prompt":"prefix two"}),
            json!({"tokens_predicted":0}),
        ],
    );
    let lease = owner.request().unwrap();
    let prefix = ctx.prefill_plan(true).unwrap();
    let old = prefill(&lease, &prefix).unwrap();
    drop(prefix);
    let mut corrected = turn(8);
    if let Some(conversation_log::observe::Payload::Assistant { spans, .. }) = &mut corrected.presentation {
        spans[0].text = "Corrected rendered prefix.".into();
    }
    let change = ctx.correct_rendered(&corrected, true).unwrap();
    assert_eq!(change.kind, "rendered_correction");
    assert!(ctx.mark_prefilled(old).is_err());
    let prefix = ctx.prefill_plan(true).unwrap();
    let receipt = prefill(&lease, &prefix).unwrap();
    drop(prefix);
    ctx.mark_prefilled(receipt).unwrap();
    let p = ctx.prepare(&mut Count, &tail(), "new").unwrap();
    let sent = server.join().unwrap();
    assert_eq!(sent[1]["n_predict"], 0);
    assert_eq!(sent[3]["n_predict"], 0);
    assert_eq!(sent[1]["id_slot"], 0);
    assert_eq!(
        p.payload()["messages"][p.payload()["messages"].as_array().unwrap().len() - 3]["content"],
        "Corrected rendered prefix."
    );
    owner.close().unwrap();
    drop(lease);
    let (second, listener) = bound();
    let lease = second.request().unwrap();
    assert!(matches!(generate(&lease, &p), Err(Error::Prefill)));
    drop(p);
    let server = json_server(listener.try_clone().unwrap(), vec![json!({"prompt":"new child prefix"}),json!({"tokens_predicted":0})]);
    let prefix = ctx.prefill_plan(true).expect("a new child must be able to prefill again");
    let receipt = prefill(&lease, &prefix).unwrap(); drop(prefix);
    ctx.mark_prefilled(receipt).unwrap(); server.join().unwrap();
    let p = ctx.prepare(&mut Count, &tail(), "new").unwrap();
    let server = serve(listener, response(&stream(&["New child answer."], "stop"), false));
    assert_eq!(generate(&lease, &p).unwrap().text, "New child answer."); server.join().unwrap();
}
#[test]
fn complete_template_overflow_refuses_without_truncating_input() {
    struct Overflow;
    impl Counter for Overflow {
        fn text(&mut self, _: &str) -> Result<usize, Error> {
            Ok(1)
        }
        fn prompt(&mut self, _: &[Value]) -> Result<usize, Error> {
            Ok(4001)
        }
    }
    let ctx = Context::new("stable").unwrap();
    assert!(matches!(ctx.prepare(&mut Overflow, &tail(), "exact untruncated input"), Err(Error::Context)));
}

#[test]
fn trim_cannot_reach_512_by_eating_into_the_newest_four_exchanges() {
    let mut ctx = Context::new("stable").unwrap();
    for n in 0..6 {ctx.append_rendered(&turn(n)).unwrap();}
    let before = ctx.prefix(); assert!(matches!(ctx.trim(&mut Count,true),Err(Error::Context)));
    assert_eq!(ctx.prefix(),before);
}
#[test]
fn history_cap_is_checked_even_when_the_complete_template_fits() {
    let mut ctx = Context::new("stable").unwrap();
    for n in 0..13 {ctx.append_rendered(&turn(n)).unwrap();}
    assert!(matches!(ctx.prepare(&mut Count,&tail(),"new"),Err(Error::Context)));
}
#[test]
fn prefill_refuses_a_server_that_actually_generated_tokens() {
    let mut ctx = Context::new("stable").unwrap();
    for n in 0..9 {ctx.append_rendered(&turn(n)).unwrap();} ctx.trim(&mut Count,true).unwrap();
    let (owner,listener)=bound();
    let server=json_server(listener,vec![json!({"prompt":"prefix"}),json!({"tokens_predicted":3})]);
    let lease=owner.request().unwrap();let prefix=ctx.prefill_plan(true).unwrap();
    assert!(matches!(prefill(&lease,&prefix),Err(Error::Prefill)));server.join().unwrap();
}
