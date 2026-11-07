

use super::*;
use rth_protocol::TurnSpeaker;

const OWNER: &str = "p-rui";
const OTHER: &str = "p-lia";
const CAFE: &str = "Lua";

fn person(id: &str) -> TurnSpeaker {
    TurnSpeaker::Person { person_id: id.into() }
}

fn speakers() -> Vec<(&'static str, TurnSpeaker, bool)> {
    vec![("keyboard", TurnSpeaker::OwnerKeyboard, true), ("owner voice", person(OWNER), true), ("other", person(OTHER), false), ("unknown", TurnSpeaker::Unknown, false), ("uncertain", TurnSpeaker::Uncertain, false)]
}

fn said(id: &str, generation: u64, text: &str, speaker: TurnSpeaker) -> Event {
    Event::CommandSubmitted { command_id: id.into(), session_id: SESSION.into(), expected_generation: generation, text: text.into(), speaker, reply_voice: None, effort: None, reply_max_tokens: None }
}

async fn enrol(c: &mut Client, frame: &str, person_id: &str, name: &str, owner: bool) {
    let auth = Some(rth_protocol::OwnerGrant { grant_id: format!("g-{frame}"), method: rth_protocol::OwnerGrantMethod::TouchIdOrPassword, at_utc_ms: 1 });
    c.send(Event::PeopleEnrol { frame_id: frame.into(), person_id: person_id.into(), name: name.into(), relation: "family".into(), owner, modality: rth_protocol::PeopleModality::ReadAlong, consent_copy_version: 2, auth }).await;
    loop {
        match c.recv().await.event {
            Event::PeopleAck { .. } => return,
            Event::PeopleRefused { reason, .. } => panic!("enrol {person_id} refused: {reason}"),
            _ => {}
        }
    }
}

async fn seed(dir: &std::path::Path, people: bool) -> String {
    let (rig, _f) = turn_lifecycle_rig(dir, "s1", r#"{"d":"remember"}"#).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    if people {
        enrol(&mut c, "f1", OWNER, "Rui", true).await;
        enrol(&mut c, "f2", OTHER, "Lia", false).await;
    }
    c.send(submit("c1", 0, "remember that my favourite café is Lua")).await;
    let recs = turn_lifecycle_until(&rig, |r| turn_lifecycle_acts(r).iter().any(|(_, s, _)| s == "committed")).await;
    turn_lifecycle_acts(&recs).last().unwrap().2["outcome"]["memory_id"].as_str().unwrap().to_string()
}

fn recalls_of<'a>(recs: &'a [Envelope], command: &str) -> Vec<&'a serde_json::Value> {
    recs.iter().filter_map(|e| if let Event::MemoryRecall { recall, .. } = &e.event { (recall["for"] == format!("command:{command}")).then_some(recall) } else { None }).collect()
}

fn skipped_not_owner(recs: &[Envelope], command: &str) -> bool {
    recalls_of(recs, command).iter().any(|r| r["status"] == "skipped" && r["reason"] == "not_owner")
}

fn offered_of(recs: &[Envelope], command: &str) -> Vec<String> {
    let r = recs.iter().rev().find_map(|e| if let Event::MemoryDecided { receipt, .. } = &e.event { (receipt["command_id"] == command).then_some(receipt) } else { None });
    r.map(|r| r["offered"].as_array().unwrap().iter().map(|o| o["memory_id"].as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

fn reply_request(f: &local::fake_server::Fake, asked: &str) -> String {
    let reqs = f.requests.lock().unwrap();
    let r = reqs.iter().rev().find(|r| r["stream"] == true && r["max_tokens"] != 1 && r["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().ends_with(asked)).unwrap_or_else(|| panic!("no reply request for {asked:?}"));
    let msgs = r["messages"].as_array().unwrap();
    let mut out: Vec<String> = msgs.iter().filter(|m| m["role"] == "system").map(|m| m["content"].as_str().unwrap().to_string()).collect();
    out.push(msgs.last().unwrap()["content"].as_str().unwrap().to_string());
    out.join("\n")
}

fn turns_done(r: &[Envelope], n: usize) -> bool {
    r.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count() >= n
}

#[tokio::test]
async fn pv05_g9_owner_memory_reaches_only_the_owners_turns() {
    for (who, speaker, owner) in speakers() {
        let dir = command_dir(&format!("pv05-g9-recall-{}", who.replace(' ', "-")));
        let cafe = seed(&dir, true).await;
        let (rig, f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        let asked = "what's my favourite café?";
        c.send(said("c2", 1, asked, speaker)).await;
        let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
        let got = reply_request(&f, asked);
        if owner {
            assert!(offered_of(&recs, "c2").contains(&cafe), "{who}: the owner's turn recalls the café");
            assert!(!skipped_not_owner(&recs, "c2"), "{who}");
            assert!(got.contains(CAFE), "{who}: the owner's reply sees the café");
        } else {
            assert!(skipped_not_owner(&recs, "c2"), "{who}: the receipt says why recall was skipped: {:?}", recalls_of(&recs, "c2"));
            assert!(offered_of(&recs, "c2").is_empty(), "{who}: memory.decide is offered nothing");
            assert!(!got.contains(CAFE), "{who}: no owner memory reaches the reply model: {got}");
        }
    }
}

#[tokio::test]
async fn pv05_g9_with_no_people_every_turn_is_the_owners() {
    for (who, speaker, _) in speakers() {
        let dir = command_dir(&format!("pv05-g9-nobody-{}", who.replace(' ', "-")));
        let cafe = seed(&dir, false).await;
        let (rig, f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        let asked = "what's my favourite café?";
        c.send(said("c2", 1, asked, speaker)).await;
        let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
        assert!(offered_of(&recs, "c2").contains(&cafe), "{who}: with no one enrolled, 0.4 holds");
        assert!(reply_request(&f, asked).contains(CAFE), "{who}");
    }
}

#[tokio::test]
async fn pv05_g9_last_offered_never_crosses_to_another_speaker() {
    let dir = command_dir("pv05-g9-last-offered");
    let cafe = seed(&dir, true).await;
    let correct = format!(r#"{{"d":"correct","t":"{cafe}"}}"#);
    let (rig, f) = routing_rig(&dir, "s2", &["Okay.", "Okay.", "Okay."], &[r#"{"d":"no_op"}"#, &correct, r#"{"d":"no_op"}"#]).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(said("c2", 1, "what's my favourite café?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
    assert_eq!(offered_of(&recs, "c2"), [cafe.clone()]);
    let asked = "actually it's Sol now";
    c.send(said("c3", 2, asked, person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 2)).await;
    assert!(offered_of(&recs, "c3").is_empty(), "the owner's last offer is not offered to another speaker");
    assert!(!reply_request(&f, asked).contains(CAFE));
    assert!(!turn_lifecycle_acts(&recs).iter().any(|(_, _, a)| a["op"]["kind"] == "correct" || a["op"].get("memory_id").is_some()), "no correction of the owner's item: {:?}", turn_lifecycle_acts(&recs));
    c.send(said("c4", 3, "and which one was it?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 3)).await;
    assert!(offered_of(&recs, "c4").contains(&cafe), "the owner's follow-up still has the owner's offer");
}

fn acts_of(recs: &[Envelope], command: &str) -> Vec<serde_json::Value> {
    let acts: Vec<serde_json::Value> = turn_lifecycle_acts(recs).into_iter().map(|(_, _, a)| a).collect();
    let mine: Vec<&str> = acts.iter().filter(|a| a["origin"]["command_id"] == command || a["accepted_by_command"] == command).filter_map(|a| a["proposal_id"].as_str()).collect();
    acts.iter().filter(|a| a["proposal_id"].as_str().is_some_and(|p| mine.contains(&p))).cloned().collect()
}

fn refusal(acts: &[serde_json::Value]) -> Option<String> {
    acts.iter().find_map(|a| a["outcome"]["reason"].as_str().map(str::to_string))
}

#[tokio::test]
async fn pv05_g9_other_peoples_saves_are_theirs_and_never_the_owners() {
    let dir = command_dir("pv05-g9-theirs");
    let cafe = seed(&dir, true).await;
    let (rig, f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(said("c2", 1, "/remember Lia likes green tea", person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| turn_lifecycle_acts(r).iter().filter(|(_, s, _)| s == "committed").count() >= 1).await;
    let acts = acts_of(&recs, "c2");
    assert_eq!(acts[0]["origin"]["speaker"], format!("person:{OTHER}"), "the save keeps its speaker: {acts:?}");
    let tea = acts.iter().find_map(|a| a["outcome"]["memory_id"].as_str()).unwrap().to_string();

    let asked = "what does Lia like to drink?";
    c.send(said("c3", 2, asked, TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 2)).await;
    assert!(!offered_of(&recs, "c3").contains(&tea), "another person's save is never the owner's");
    assert!(offered_of(&recs, "c3").contains(&cafe) || !recalls_of(&recs, "c3").is_empty());
    assert!(!reply_request(&f, asked).contains("green tea"), "nor in the saved list");

    c.send(said("c4", 3, &format!("/memory correct {cafe} 1 my favourite café is Sol"), person(OTHER))).await;
    c.send(said("c5", 4, &format!("/memory correct {tea} 1 Lia likes black tea"), person(OTHER))).await;
    c.send(said("c6", 5, &format!("/memory correct {tea} 2 Lia likes no tea"), TurnSpeaker::Unknown)).await;
    c.send(said("c7", 6, "/memory confirm p-9 abcd", person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 6)).await;
    assert_eq!(refusal(&acts_of(&recs, "c4")).as_deref(), Some("not_owner"), "the owner's item: {:?}", acts_of(&recs, "c4"));
    assert!(acts_of(&recs, "c5").iter().any(|a| a["state"] == "committed"), "her own item: {:?}", acts_of(&recs, "c5"));
    assert_eq!(refusal(&acts_of(&recs, "c6")).as_deref(), Some("not_owner"), "an unknown voice owns nothing");
    assert_eq!(refusal(&acts_of(&recs, "c7")).as_deref(), Some("not_owner"), "no confirm of a proposal that is not hers");
}

#[tokio::test]
async fn pv05_g9_a_forget_on_another_speakers_turn_holds_no_card() {
    for (who, speaker, owner) in speakers() {
        let dir = command_dir(&format!("pv05-g9-forget-{}", who.replace(' ', "-")));
        seed(&dir, true).await;
        let (rig, _f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        c.send(said("c2", 1, "forget that my favourite café is Lua", speaker)).await;
        let recs = turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
        let held = recs.iter().any(|e| matches!(e.event, Event::EffectHeld { .. }));
        assert_eq!(held, owner, "{who}: a forget card only on the owner's turn");
    }
}

#[tokio::test]
async fn pv05_g9_the_memory_question_on_another_speakers_turn_says_nothing_is_saved() {
    let ask = |dir: PathBuf, speaker: TurnSpeaker, saved: bool| async move {
        if saved {
            seed(&dir, true).await;
        }
        let (rig, _f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
        let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
        c.send(said("c2", u64::from(saved), "você tem memória?", speaker)).await;
        reply_of(&turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await)
    };
    let nothing = ask(command_dir("pv05-g9-identity-none"), TurnSpeaker::OwnerKeyboard, false).await;
    let owner = ask(command_dir("pv05-g9-identity-owner"), TurnSpeaker::OwnerKeyboard, true).await;
    assert_ne!(owner, nothing, "the owner hears that something is saved");
    for (who, speaker, is_owner) in speakers() {
        if is_owner {
            continue;
        }
        let reply = ask(command_dir(&format!("pv05-g9-identity-{who}")), speaker, true).await;
        assert_eq!(reply, nothing, "{who}: the fixed reply says nothing is saved");
    }
}

async fn tool_rig(name: &str) -> (Rig, Client, String) {
    let dir = command_dir(name);
    let started = Instant::now();
    let rig = state_rig(&dir, "t1", backend::fake(Duration::from_millis(1), move || started.elapsed(), "t1".to_string()), memory_opts()).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    enrol(&mut c, "f1", OWNER, "Rui", true).await;
    enrol(&mut c, "f2", OTHER, "Lia", false).await;
    c.send(submit("c0", 0, "/remember my favourite café is Lua")).await;
    let recs = turn_lifecycle_until(&rig, |r| turn_lifecycle_acts(r).iter().any(|(_, s, _)| s == "committed")).await;
    let cafe = turn_lifecycle_acts(&recs).last().unwrap().2["outcome"]["memory_id"].as_str().unwrap().to_string();
    (rig, c, cafe)
}

fn tool_recalls(recs: &[Envelope]) -> Vec<serde_json::Value> {
    recs.iter().filter_map(|e| if let Event::MemoryRecall { recall, .. } = &e.event { recall["for"].as_str().is_some_and(|f| f.starts_with("call:")).then(|| recall.clone()) } else { None }).collect()
}

fn tool_proposals(recs: &[Envelope]) -> Vec<serde_json::Value> {
    turn_lifecycle_acts(recs).into_iter().filter(|(_, s, a)| s == "proposed" && a["origin"]["via"] == "tool").map(|(_, _, a)| a).collect()
}

fn spoken(id: &str, text: &str, speaker: TurnSpeaker) -> Event {
    Event::UserText { request_id: id.into(), text: text.into(), speaker, reply_voice: None, effort: None, reply_max_tokens: None }
}

fn dones(recs: &[Envelope]) -> usize {
    recs.iter().filter(|e| matches!(e.event, Event::AgentDone { .. })).count()
}

#[tokio::test]
async fn pv05_g9_the_recall_tool_fetches_nothing_on_another_speakers_turn() {
    let (rig, mut c, cafe) = tool_rig("pv05-g9-tool-recall").await;
    for (n, (who, speaker, owner)) in speakers().into_iter().enumerate() {
        c.send(spoken(&format!("u{n}"), "recall: favourite café", speaker)).await;
        let recs = turn_lifecycle_until(&rig, |r| tool_recalls(r).len() > n).await;
        let r = &tool_recalls(&recs)[n];
        let items: Vec<&str> = r["items"].as_array().unwrap().iter().map(|i| i["memory_id"].as_str().unwrap()).collect();
        if owner {
            assert!(items.contains(&cafe.as_str()), "{who}: the owner's tool recall finds the café: {r}");
        } else {
            assert_eq!((r["status"].as_str(), r["reason"].as_str()), (Some("skipped"), Some("not_owner")), "{who}: the receipt says why: {r}");
            assert!(items.is_empty(), "{who}: nothing of the owner's is fetched: {r}");
        }
    }
}

#[tokio::test]
async fn pv05_g9_a_tool_save_keeps_its_speaker_and_a_tool_correction_of_the_owners_item_is_refused() {
    let (rig, mut c, cafe) = tool_rig("pv05-g9-tool-acts").await;
    c.send(said("c1", 1, "remember: Lia likes green tea.", person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| !tool_proposals(r).is_empty()).await;
    assert_eq!(tool_proposals(&recs)[0]["origin"]["speaker"], format!("person:{OTHER}"), "a tool save is kept under its speaker");
    let done = dones(&recs);
    c.send(said("c2", 2, &format!("correct: {cafe} 1 my favourite café is Sol"), person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| dones(r) > done).await;
    assert_eq!(tool_proposals(&recs).len(), 1, "another speaker's tool correction of the owner's item proposes nothing");
    c.send(spoken("u3", &format!("correct: {cafe} 1 my favourite café is Sol"), person(OWNER))).await;
    let recs = turn_lifecycle_until(&rig, |r| tool_proposals(r).len() > 1).await;
    let p = &tool_proposals(&recs)[1];
    assert_eq!(p["origin"]["speaker"], format!("person:{OWNER}"));
    assert!(p["op"].to_string().contains(&cafe), "the owner's own correction is proposed: {p}");
}

#[tokio::test]
async fn pv05_g9_on_a_spoken_turn_the_recall_tool_fetches_only_for_the_owners_voice() {
    let (rig, mut c, cafe) = tool_rig("pv05-g9-voice-recall").await;
    let pcm: Vec<u8> = (0..12_800u32).map(|i| (i * 7) as u8).collect();
    for (n, (who, speaker, owner)) in speakers().into_iter().enumerate() {
        let stream = format!("recall-{n}");
        for (i, chunk) in pcm.chunks(1280).enumerate() {
            let data = base64::engine::general_purpose::STANDARD.encode(chunk);
            c.send(Event::UserAudio { stream_id: stream.clone(), chunk: i as u64, sample_offset: i as u64 * 640, sample_rate: 16_000, format: "s16le".into(), data, digest: None }).await;
        }
        c.send(Event::UserAudioEnd { stream_id: stream, chunks: 10, samples: 6400, dropped_chunks: 0, speaker }).await;
        let recs = turn_lifecycle_until(&rig, |r| tool_recalls(r).len() > n).await;
        let r = &tool_recalls(&recs)[n];
        let items: Vec<&str> = r["items"].as_array().unwrap().iter().map(|i| i["memory_id"].as_str().unwrap()).collect();
        if owner {
            assert!(items.contains(&cafe.as_str()), "{who}: the owner's voice recalls the café: {r}");
        } else {
            assert_eq!((r["status"].as_str(), r["reason"].as_str()), (Some("skipped"), Some("not_owner")), "{who}: {r}");
            assert!(items.is_empty(), "{who}: {r}");
        }
    }
}

fn response_of(recs: &[Envelope], command: &str) -> String {
    recs.iter().find_map(|e| if let Event::ResponseStarted { response_id, request_id, .. } = &e.event { (request_id == command).then(|| response_id.clone()) } else { None }).unwrap_or_else(|| panic!("no response for {command}"))
}

async fn correct(c: &mut Client, frame: &str, turn: &str, person_id: &str, owner_confirmed: bool) -> Event {
    c.send(Event::PeopleCorrect { frame_id: frame.into(), turn_id: turn.into(), person_id: person_id.into(), owner_confirmed }).await;
    loop {
        let e = c.recv().await.event;
        if matches!(e, Event::PeopleAck { .. } | Event::PeopleRefused { .. }) {
            return e;
        }
    }
}

fn left_out(recs: &[Envelope], command: &str, memory_id: &str) -> Option<String> {
    let done: Vec<&serde_json::Value> = recalls_of(recs, command).into_iter().filter(|r| r["excluded"].is_array()).collect();
    let r = done.last().unwrap_or_else(|| panic!("no recall receipt for {command}"));
    if r["items"].as_array().unwrap().iter().any(|i| i["memory_id"] == memory_id) {
        return None;
    }
    r["excluded"].as_array().unwrap().iter().find(|x| x["memory_id"] == memory_id).map(|x| x["reason"].as_str().unwrap().to_string()).or_else(|| Some("absent".into()))
}

#[tokio::test]
async fn pv05_g9_a_people_correction_moves_a_turns_saves_to_the_person_it_names() {
    let (rig, mut c, cafe) = tool_rig("pv05-g9-correct").await;
    c.send(said("c1", 1, "/remember Lia likes green tea", person(OTHER))).await;
    let recs = turn_lifecycle_until(&rig, |r| turn_lifecycle_acts(r).iter().filter(|(_, s, _)| s == "committed").count() >= 2).await;
    let tea = turn_lifecycle_acts(&recs).iter().rev().find(|(_, s, _)| s == "committed").unwrap().2["outcome"]["memory_id"].as_str().unwrap().to_string();
    c.send(said("c2", 2, "what does Lia like?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| recalls_of(r, "c2").iter().any(|x| x["excluded"].is_array())).await;
    assert_eq!(left_out(&recs, "c2", &tea).as_deref(), Some("other_speaker"), "before the correction the save is Lia's");

    let lia_turn = response_of(&recs, "c1");
    let refused = correct(&mut c, "fc0", &lia_turn, OWNER, false).await;
    assert!(matches!(&refused, Event::PeopleRefused { reason, .. } if reason == "needs_owner_grant"), "{refused:?}");
    c.send(said("c2b", 3, "what does Lia like?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| recalls_of(r, "c2b").iter().any(|x| x["excluded"].is_array())).await;
    assert_eq!(left_out(&recs, "c2b", &tea).as_deref(), Some("other_speaker"), "a refused card moves nothing");

    let ack = correct(&mut c, "fc1", &lia_turn, OWNER, true).await;
    assert!(matches!(&ack, Event::PeopleAck { person_id, .. } if person_id == OWNER), "{ack:?}");
    assert_eq!(format!("{:?}", correct(&mut c, "fc1", &lia_turn, OWNER, true).await), format!("{ack:?}"), "a resent card gets the same answer");
    c.send(said("c3", 4, "what does Lia like?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| recalls_of(r, "c3").iter().any(|x| x["excluded"].is_array())).await;
    assert_eq!(left_out(&recs, "c3", &tea), None, "after it the owner's recall offers the save");

    let cafe_turn = response_of(&recs, "c0");
    assert!(matches!(correct(&mut c, "fc2", &cafe_turn, OTHER, false).await, Event::PeopleAck { .. }));
    c.send(said("c4", 5, "what's my favourite café?", TurnSpeaker::OwnerKeyboard)).await;
    let recs = turn_lifecycle_until(&rig, |r| recalls_of(r, "c4").iter().any(|x| x["excluded"].is_array())).await;
    assert_eq!(left_out(&recs, "c4", &cafe).as_deref(), Some("other_speaker"));
    let refused = correct(&mut c, "fc3", &cafe_turn, "p-nobody", false).await;
    assert!(matches!(&refused, Event::PeopleRefused { reason, .. } if reason == "unknown_person"), "{refused:?}");
}

#[tokio::test]
async fn pv05_g9_the_reply_model_is_told_who_said_the_turn_and_who_is_here() {
    let dir = command_dir("pv05-g9-turn-block");
    seed(&dir, true).await;
    let (rig, f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(Event::PeoplePresence { present: vec![OWNER.into(), OTHER.into()], unknown_faces: 1, at_utc_ms: 1, source: None }).await;
    let asked = "hello there";
    c.send(said("c2", 1, asked, person(OTHER))).await;
    turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
    let got = reply_request(&f, asked);
    assert!(got.contains("Said by: Lia.") && got.contains("Here now: Rui (the owner), Lia; 1 person not recognised."), "the local model may see names: {got}");
    assert!(!got.contains("camera") && !got.contains("face"), "a presence frame that names no source claims no camera: {got}");

    let dir = command_dir("pv05-g9-turn-block-none");
    seed(&dir, false).await;
    let (rig, f) = turn_lifecycle_rig(&dir, "s2", r#"{"d":"no_op"}"#).await;
    let (mut c, _) = Client::attach(&rig, "host", Role::Controller).await;
    c.send(said("c2", 1, asked, TurnSpeaker::OwnerKeyboard)).await;
    turn_lifecycle_until(&rig, |r| turns_done(r, 1)).await;
    assert!(!reply_request(&f, asked).contains("Said by"), "no People block without People");
}
