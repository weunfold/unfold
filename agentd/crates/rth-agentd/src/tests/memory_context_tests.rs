

use super::*;

const TURN_A: &str = "what was the last thing i said";
const TURN_A_PT: &str = "qual foi a última coisa que eu disse";
const TURN_B: &str = "pretend your doing something complex, i want you to route a complex mathematic equation, one that openai recently submited a solution to a 100 year math problem";
const TURN_B_PT: &str = "finge que você tá fazendo algo complexo, eu quero que você resolva uma equação matemática complexa, tipo aquela que a OpenAI mandou solução pra um problema de 100 anos";

#[test]
fn a_question_about_this_chat_gets_no_memory_block() {
    for t in [TURN_A, TURN_A_PT, "what did I just ask you?", "o que eu acabei de te perguntar?", "what was my last message?", "qual foi minha última mensagem?", "what did I say a moment ago?", "o que eu falei agora há pouco?"] {
        assert_eq!(blocks::withheld(t), "", "{t}");
        assert_eq!(blocks::none(t), "", "{t}");
    }
}

#[test]
fn questions_about_saved_memory_keep_their_blocks() {
    for t in ["what's my name?", "who am I?", "what do you know about me?", "what did I tell you about my sister?"] {
        assert!(blocks::withheld(t).contains(blocks::NOT_SHARED_EN), "{t}: {}", blocks::withheld(t));
        assert!(blocks::none(t).starts_with("MEMORY_NONE: nothing saved matches this question"), "{t}: {}", blocks::none(t));
    }
    for t in ["qual é o meu nome?", "quem sou eu?"] {
        assert!(blocks::withheld(t).contains(blocks::NOT_SHARED_PT), "{t}: {}", blocks::withheld(t));
    }

    for t in [
        "what's the last thing I asked you to remember?",
        "what was the last thing I told you about my diet?",
        "qual foi a última coisa que eu te falei sobre a Ana?",
        "what did I say in our conversation yesterday?",
        "você lembra o que eu disse nessa conversa ontem?",
        "what did I just tell you about my sister last week?",
    ] {
        assert!(!blocks::about_this_chat(t), "{t}");
        let shared = if t.is_ascii() { blocks::NOT_SHARED_EN } else { blocks::NOT_SHARED_PT };
        assert!(blocks::withheld(t).contains(shared), "{t}: {}", blocks::withheld(t));
        assert!(blocks::none(t).starts_with("MEMORY_NONE: nothing saved matches this question"), "{t}: {}", blocks::none(t));
    }
    for t in ["I just moved to Porto", "my last message was a typo, my dog is Sol"] {
        assert!(!blocks::about_this_chat(t), "a statement is not a question about the chat: {t}");
    }
}

#[test]
fn an_unasked_statement_gets_the_quiet_block() {
    for t in [TURN_B, TURN_B_PT, "my sister is Ana", "minha irmã é a Ana", "my dog is Sol"] {
        for b in [blocks::withheld(t), blocks::none(t)] {
            assert_eq!(b, blocks::NONE_QUIET, "{t}");
            for status in ["NOT saved", "not saved", "MEMORY_NOT_SHARED", blocks::NOT_SHARED_EN, blocks::NOT_SHARED_PT] {
                assert!(!b.contains(status), "{t}: {status:?} in {b}");
            }
        }
    }
    assert!(blocks::NONE_QUIET.starts_with("MEMORY_NONE:"), "the label the prompts already list");
    assert!(blocks::NONE_QUIET.contains("never say or imply that you will remember it") || blocks::NONE_QUIET.contains("Never say or imply that you will remember it"));
}

#[test]
fn an_asked_remember_and_a_refused_secret_keep_their_status() {
    for t in ["remember that the gym opens at 6am", "lembra que a academia abre às 6", "can you remember that I live in Lisbon"] {
        assert!(blocks::none(t).starts_with("MEMORY_NONE: this message is NOT saved"), "{t}: {}", blocks::none(t));
        assert!(blocks::withheld(t).starts_with("MEMORY_NONE: this message is NOT saved"), "{t}: {}", blocks::withheld(t));
    }
    assert!(blocks::withheld("você pode lembrar que meu cachorro é o Sol?").starts_with("MEMORY_NONE: this message is NOT saved"));
    let b = guard::check_message("my pin is 4821, anyway what's the weather").expect("a code with digits is refused");
    assert!(blocks::not_saved(b.why()).starts_with("MEMORY_NOT_SAVED: this message was not saved"));
}

#[tokio::test]
async fn the_conversationturns_hand_the_model_no_memory_status() {
    use crate::flash::test_server::{start_keyed, Answer};
    let dir = command_dir("routing-10-s4-withheld");
    let s = start_keyed(Answer::Stream { events: vec![routing_delta("Ok.")], gap: Duration::ZERO, hang: false }, DSH_KEY).await;
    let (rig, _f) = dsh_rig(&dir, "s4", &s.url, "", "not a decision").await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    let last = |i: usize| flash_bodies(&s)[i]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_string();
    for (i, said) in [TURN_A, TURN_B, TURN_A_PT, TURN_B_PT].into_iter().enumerate() {
        c.send(submit(&format!("c{i}"), i as u64, said)).await;
        let recs = turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() > i).await;
        let sent = last(i);
        for status in ["MEMORY_NOT_SHARED", "NOT saved", blocks::NOT_SHARED_EN, blocks::NOT_SHARED_PT] {
            assert!(!sent.contains(status), "{said}: {status:?} in {sent}");
        }
        if i % 2 == 1 {
            assert!(sent.contains(blocks::NONE_QUIET) || (sent == said && late_memory_receipt(&recs, &format!("c{i}"))), "{said}: {sent}");
        } else {
            assert_eq!(sent, said, "the chat-history question needs no saved-memory status");
        }
    }
    let recs = turn_lifecycle_until(&rig, |r| r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() >= 4).await;
    let decided = turn_lifecycle_decided(&recs);
    let quiet: Vec<_> = decided.iter().map(|(_, r)| r["quiet"].as_bool() == Some(true)).collect();
    assert_eq!(quiet, [false, true, false, true], "{decided:?}");
    assert!(decided.iter().all(|(_, r)| r["recall"].as_str() == Some("egress_withheld")), "{decided:?}");
}
