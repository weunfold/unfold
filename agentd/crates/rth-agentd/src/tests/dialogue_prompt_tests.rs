
use super::*;
use serde_json::json;

#[tokio::test]
async fn local_prompt_bytes_and_hash_ignore_reply_voice() {
    use crate::local::fake_server::{start, Mode};
    let f = start(Mode::Stream { pieces: vec!["Hello there.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let cfg = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) };
    let rig = state_rig(&command_dir("dialogue-one-local"), "t1", local::connect(cfg), ctx_opts()).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    for (i, voice) in [None, Some(json!({"dictated":true,"spoken":"first_sentence"})), Some(json!({"dictated":true,"spoken":"full"})), Some(json!({"dictated":false,"spoken":"full"}))].into_iter().enumerate() {
        let e = submit(&format!("c{i}"), i as u64, "hello there");
        if let Some(v) = voice { rv_send(&mut c, e, v).await; } else { c.send(e).await; }
        rv_done(&rig, i + 1).await;
    }
    let systems = a7b_systems(&f);
    assert_eq!(systems.len(), 4);
    assert!(systems.iter().all(|s| s == &systems[0]), "delivery must not rewrite the system prompt");
    assert!(!systems[0].contains("not speech") && !systems[0].contains("No voice"));
    assert!(systems[0].contains("Replies may be said aloud"));
    let recs = journal_records(&rig.journal, |_| true).await;
    let prov = routing_provenance(&recs);
    assert_eq!(prov.len(), 4);
    for p in &prov {
        assert_eq!(p["prompt"], prov[0]["prompt"]);
        assert_eq!(p["prompt_sha256"], crate::backend::text_sha256(&systems[0]));
        assert!(!p["prompt"].as_str().unwrap().contains("+voice") && !p["prompt"].as_str().unwrap().contains("+dictated"));
    }
    let voices: Vec<_> = recs.iter().filter_map(|e| match &e.event { Event::CommandSubmitted { reply_voice, .. } => Some(*reply_voice), _ => None }).collect();
    assert_eq!(voices.len(), 4);
    assert!(voices[0].is_none() && voices[1].unwrap().dictated && voices[2].unwrap().dictated && !voices[3].unwrap().dictated);
}

#[tokio::test]
async fn flash_prompt_bytes_ignore_reply_voice() {
    use crate::flash::test_server::{start, Answer};
    let s = start(Answer::Stream { events: vec![routing_delta("Hello there.")], gap: Duration::ZERO, hang: false }).await;
    let (rig, _) = router_rig(&command_dir("dialogue-one-flash"), "b1", &s.url, crate::flash::FIRST_TOKEN).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c0", 0, "hello")).await;
    rv_done(&rig, 1).await;
    rv_send(&mut c, submit("c1", 1, "hello again"), json!({"dictated":true,"spoken":"full"})).await;
    rv_done(&rig, 2).await;
    let sys = rv_flash_systems(&s);
    assert_eq!(sys.len(), 2);
    assert_eq!(sys[0], sys[1]);
    assert!(!sys[0].contains("not speech") && !sys[0].contains("No voice"));
}

#[tokio::test]
async fn local_labels_never_reach_deltas_replacements_or_history() {
    use crate::local::fake_server::{start, Mode};
    for (i, raw) in ["The system says MEMORY_NEEDED_BUT_UNAVAILABLE.", "Veja as SAVED_MEMORIES_UPDATE e MEMORY_NONE.", "MEMORY_NONE", "Useful text. </think>\nThe label is MEMORY_NOT_SAVED."] .into_iter().enumerate() {

        let f = start(Mode::Stream { pieces: raw.chars().map(|c| c.to_string()).collect(), gap: Duration::ZERO }, Duration::ZERO).await;
        let cfg = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) };
        let rig = state_rig(&command_dir(&format!("dialogue-label-{i}")), "t1", local::connect(cfg), ctx_opts()).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        assert_eq!(ctx_turn(&rig, &mut c, "c0", "hello there", 0).await, "Completed");
        let recs = journal_records(&rig.journal, |_| true).await;
        let mut text = String::new();
        for e in &recs {
            match &e.event { Event::AgentDelta { text: t, .. } => text.push_str(t), Event::AgentReplace { text: t, .. } => text = t.clone(), _ => {} }
            assert!(!text.contains("MEMORY_") && !text.contains("SAVED_MEMORIES"), "visible label at {i}: {text}");
        }
        assert!(!text.trim().is_empty(), "a label-only answer gets readable output");
        assert_eq!(ctx_turn(&rig, &mut c, "c1", "thanks", 1).await, "Completed");
        let reqs = f.requests.lock().unwrap();
        let next = reqs.iter().rev().find(|r| r["stream"] == true).unwrap();
        for m in next["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "assistant") {
            let t = m["content"].as_str().unwrap();
            assert!(!t.contains("MEMORY_") && !t.contains("SAVED_MEMORIES"), "history: {t}");
        }
    }
}

#[tokio::test]
async fn hosted_literal_labels_are_unchanged() {
    use crate::flash::test_server::{start, Answer};
    let text = "The identifier MEMORY_NONE is internal; SAVED_MEMORIES is another identifier.";
    let s = start(Answer::Stream { events: vec![routing_delta(text)], gap: Duration::ZERO, hang: false }).await;
    let (rig, _) = reasoning_limits_rig(&command_dir("dialogue-hosted-label"), "b1", &s.url, false).await;
    let recs = reasoning_limits_one_turn(&rig, "describe these identifiers", None).await;
    let got: String = recs.iter().filter_map(|e| match &e.event { Event::AgentDelta { text, .. } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(got, text);
}

#[test]
fn saved_self_prompts_fit_dialogue_without_default_sentence_caps() {
    let spec = rth_self::SelfSpec::release_default("dialog-test");
    for memory in [true, false] {
        for shared in [true, false] {
            let (local, flash) = crate::being::compiled(&spec, "Granite 4.2 3B", Default::default(), memory, shared);
            for p in [local.unwrap(), flash.unwrap()] {
                assert!(p.text.contains("Replies may be said aloud"), "{}", p.text);
                assert!(!p.text.contains("not speech") && !p.text.contains("No voice"));
                assert!(!p.text.contains("one to three short sentences"));
            }
        }
    }
}

#[tokio::test]
#[ignore = "writes files: set RTH_PROMPT_DUMP_DIR"]
async fn dump_granite42_dialog_prompt() {
    use crate::local::fake_server::{start, Mode};
    let f = start(Mode::Stream { pieces: vec!["Hi.".into()], gap: Duration::ZERO }, Duration::ZERO).await;
    let cfg = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) };
    let (name, text) = cfg.prompt_for(true);
    let dir = std::path::PathBuf::from(std::env::var("RTH_PROMPT_DUMP_DIR").expect("RTH_PROMPT_DUMP_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("granite42.txt"), &text).unwrap();
    std::fs::write(dir.join("granite42.json"), serde_json::to_string_pretty(&json!({"prompt":name,"sha256":crate::backend::text_sha256(&text),"device":crate::device::this().kind,"watch":crate::dsh_watch::watching()})).unwrap()).unwrap();
}

#[test]
fn dialogue_compilation_preserves_quoted_self_data() {
    let mut spec = rth_self::SelfSpec::release_default("dialog-data");
    let words = "You talk by text, in this chat window.";
    spec.drives.motivations.push(rth_self::Drive { text: rth_self::BoundedText::new(words).unwrap(), source: rth_self::DriveSource::Person });
    let (local, flash) = crate::being::compiled(&spec, "Granite 4.2 3B", Default::default(), true, true);
    for p in [local.unwrap(), flash.unwrap()] {
        assert!(p.text.contains(&format!("\"{words}\" (the person set it)")));
        assert_eq!(p.text.matches("Replies may be said aloud").count(), 1);
    }
}

async fn missed_recall(text: &str, name: &str) -> (String, serde_json::Value) {
    let dir = command_dir(name);
    std::fs::create_dir_all(&dir).unwrap();
    let gate = memory::test_hold::install(&dir);
    use crate::local::fake_server::{start, Mode};
    let f = start(Mode::Mixed { pieces: vec!["Hello.".into()], answer: r#"{"d":"no_op"}"#.into(), warm_delay: Duration::ZERO }, Duration::ZERO).await;
    let cfg = local::Config { model: "granite42-3b-q4km".into(), ..ctx_cfg(&f) };
    let rig = state_rig(&dir, "recall-miss", local::connect_with(cfg.clone(), true), turn_lifecycle_opts(&cfg)).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(submit("c1", 0, text)).await;
    let recs = turn_lifecycle_until(&rig, |r| r.iter().any(|e| matches!(e.event, Event::AgentDone { .. }))).await;
    gate.send(()).unwrap();
    let decided = turn_lifecycle_decided(&recs);
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].0, "none");
    assert_eq!(decided[0].1["fallback"], "memory_unavailable");

    assert!(turn_lifecycle_acts(&recs).is_empty());
    assert!(!request_kinds(&f).contains(&"decide"));
    let recall = recs.iter().find_map(|e| match &e.event {
        Event::MemoryRecall { call_id, recall, .. } if call_id == "command:c1" && recall["deadline_ms"] == 800 => Some(recall),
        _ => None,
    }).expect("the real recall exceeded its unchanged 800 ms budget");
    assert!(recall["items"].as_array().unwrap().is_empty());
    let user = chat_user(&f);
    assert_eq!(user.len(), 1);
    let request = f.requests.lock().unwrap().iter().find(|r| r["stream"] == true && r["max_tokens"] != 1).cloned().unwrap();
    (user[0].clone(), request)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn greeting_with_deadline_miss_has_no_unavailable_block() {
    for (i, text) in ["Hello.", "What can I cook tonight?", "What did I just say?", "Can you help with my homework?"].into_iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("missed-recall-greeting-{i}")).await;
        assert_eq!(user, text, "an ordinary or in-chat turn reaches the model without a memory status block");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn memory_question_with_deadline_miss_keeps_unavailable_block() {
    let text = "What do you remember about me?";
    let (user, _) = missed_recall(text, "missed-recall-question").await;
    assert!(user.starts_with(&blocks::unavailable()), "{user}");
    assert!(user.ends_with(text));
    assert!(user.contains("I couldn't check your saved memories right now."));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_write_with_deadline_miss_does_not_claim_saved() {
    for (i, text) in ["Remember that my favourite colour is blue.", "Actually, my favourite colour is green."].into_iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("missed-recall-write-{i}")).await;
        assert!(user.contains("NOT saved"), "an unperformed write must be disclosed: {user}");
        assert!(user.ends_with(text));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes files: set RTH_RECALL_MISS_DUMP_DIR"]
async fn dump_missed_recall_requests() {
    let cases = [
        ("ordinary", "Hello."), ("ordinary", "Hi there!"),
        ("ordinary", "Good morning."), ("ordinary", "How are you?"),
        ("ordinary", "Thanks!"), ("ordinary", "What can I cook tonight?"),
        ("ordinary", "Olá."), ("ordinary", "Bom dia."),
        ("ordinary", "Tudo bem?"), ("ordinary", "Obrigado!"),
        ("memory", "What do you remember about me?"),
        ("memory", "What do you know about me?"),
        ("memory", "O que você lembra de mim?"),
        ("memory", "Qual é meu café favorito?"),
        ("ordinary", "How do I fix my bike?"),
        ("ordinary", "No, thanks."), ("ordinary", "Não, obrigado."),
        ("ordinary", "How much memory does a MacBook Air have?"),
        ("memory", "Do you know my name?"), ("memory", "Quem sou eu?"),
    ];
    let mut rows = Vec::new();
    for (i, (kind, text)) in cases.into_iter().enumerate() {
        let id = format!("missed-recall-eval-{i:02}");
        let (user, request) = missed_recall(text, &id).await;
        if kind == "ordinary" { assert_eq!(user, text); }
        else { assert!(user.starts_with(&blocks::unavailable())); }
        rows.push(json!({"id":id,"kind":kind,"user":text,"messages":request["messages"]}));
    }
    let dir = std::path::PathBuf::from(std::env::var("RTH_RECALL_MISS_DUMP_DIR").expect("RTH_RECALL_MISS_DUMP_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cases.json"), serde_json::to_string_pretty(&rows).unwrap()).unwrap();
}

const RC5141_R2_CORRECTION_CHAT: &[&str] = &[
    "No, thanks.", "Não, obrigado.", "No, I'm fine.", "Não, estou bem.",
    "Actually, let's talk about something else.", "Na verdade, vamos falar de outra coisa.",
];
const RC5141_R2_GENERAL: &[&str] = &[
    "How do I fix my bike?", "Como conserto minha bicicleta?",
    "Where can I buy my train ticket?", "Onde posso comprar minha passagem de trem?",
    "How much memory does a MacBook Air have?", "Quanta memória tem um MacBook Air?",
    "Do you remember the movie Inception?", "Você lembra do filme Inception?",
    "I remember my grandmother's house.", "Eu lembro da casa da minha avó.",
];
const RC5141_R2_PERSONAL: &[&str] = &[
    "Do you know my name?", "Você sabe meu nome?", "Who am I?", "Quem sou eu?",
    "Where do I live?", "Onde eu moro?", "Tell me what you know about me.",
    "Diga o que você sabe sobre mim.", "Can you check my saved memories?",
    "Você pode consultar minhas memórias salvas?",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn correction_cues_in_ordinary_chat_do_not_disclose_a_write() {
    for (i, text) in RC5141_R2_CORRECTION_CHAT.iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-intent-correction-chat-{i}")).await;
        assert_eq!(user, *text);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn general_memory_words_and_personal_possessives_are_plain_chat() {
    for (i, text) in RC5141_R2_GENERAL.iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-intent-general-{i}")).await;
        assert_eq!(user, *text);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn personal_fact_questions_and_retrieval_requests_disclose_the_miss() {
    for (i, text) in RC5141_R2_PERSONAL.iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-intent-personal-{i}")).await;
        assert!(user.starts_with(&blocks::unavailable()), "{text}: {user}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn personal_corrections_keep_the_unsaved_disclosure_in_both_languages() {
    for (i, text) in ["Actually, my favourite colour is green.", "Na verdade, minha cor favorita é verde."].into_iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-intent-personal-correction-{i}")).await;
        assert!(user.contains("NOT saved"), "{text}: {user}");
    }
}

#[tokio::test]
async fn decide_error_with_previous_offer_uses_the_same_fallback() {
    let (rig, f) = routing_rig(&command_dir("recall-intent-decide-error"), "decide-error", &["Okay."], &["invalid"]).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    assert_eq!(ctx_turn(&rig, &mut c, "c0", "/remember my favourite colour is blue", 0).await, "Completed");

    assert_eq!(ctx_turn(&rig, &mut c, "c1", "What is my favourite colour?", 1).await, "Completed");
    for (i, (text, memory_question)) in [
        ("Hello.", false), ("Olá.", false), ("No, thanks.", false), ("Não, obrigado.", false),
        ("Do you know my name?", true), ("Quem sou eu?", true),
    ].into_iter().enumerate() {
        let id = format!("c{}", i + 2);
        assert_eq!(ctx_turn(&rig, &mut c, &id, text, i + 2).await, "Completed");
        let recs = turn_lifecycle_until(&rig, |r| turn_lifecycle_decided(r).iter().any(|(_, receipt)| receipt["command_id"] == id)).await;
        let receipt = turn_lifecycle_decided(&recs).into_iter().find(|(_, receipt)| receipt["command_id"] == id).unwrap().1;
        assert!(!receipt["offered"].as_array().unwrap().is_empty(), "the previous offer exercises the decide-error arm");
        assert_eq!(receipt["fallback"], "decide_invalid");
        let user = chat_user(&f).last().unwrap().clone();
        if memory_question { assert!(user.starts_with(&blocks::unavailable()), "{text}: {user}"); }
        else { assert_eq!(user, text); }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn personal_fact_words_inside_statements_stay_plain() {
    let mut changed = Vec::new();
    for (i, text) in [
        "Hoje está muito quente onde eu moro.",
        "Na cidade onde moro tem muito trânsito.",
        "Perto de onde trabalho abriu um café.",
        "Minha mãe sabe tudo sobre mim.",
        "Ela lembra de mim desde criança.",
        "My friends know a lot about me.",
    ].into_iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-boundary-statement-{i}")).await;
        if user != text { changed.push((text, user)); }
    }
    assert!(changed.is_empty(), "statements must reach the model unchanged: {changed:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn explicit_retrieval_imperatives_keep_the_unavailable_block() {
    for (i, text) in ["Tell me what you know about me.", "Diga o que você sabe sobre mim."].into_iter().enumerate() {
        let (user, _) = missed_recall(text, &format!("recall-boundary-imperative-{i}")).await;
        assert!(user.starts_with(&blocks::unavailable()), "{text}: {user}");
    }
}
