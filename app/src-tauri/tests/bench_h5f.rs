
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"] mod conversation_log;
#[allow(dead_code)]
#[path = "../src/voice_front.rs"] mod voice_front;
#[allow(dead_code)]
#[path = "h5c/support.rs"] mod support;
use conversation_log::{Store, Turn, Owner, observe::{Span, Terminal, Payload}};
use voice_front::{runtime::*, policy::*, context::*, http::Burst, Error};
use std::{sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Instant};
static N: AtomicU64=AtomicU64::new(0);
fn path()->std::path::PathBuf {
    let p=std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
        .join(format!("h5f-{}-{}",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));
    std::fs::create_dir_all(&p).unwrap();p.join("log")
}
fn epoch()->Epoch {Epoch{capture:3,turn:7}}
fn choice(locale:Locale)->Selection {Selection{locale,source:LocaleSource::Setting,confidence:None}}
#[derive(Default)]
struct Audio { requests:Vec<SpeechRequest>, fail:bool, render_then_fail:bool, stops:usize }
impl Speech for Audio {
    fn start(&mut self,r:SpeechRequest)->Result<(),Error> {
        if self.render_then_fail {assert!(r.ticket.render(epoch(),r.ticket.response(),160));}
        self.requests.push(r);
        if self.fail||self.render_then_fail {Err(Error::Child)}else{Ok(())}
    }
    fn stop(&mut self,_:&Ticket){self.stops+=1;}
}
#[derive(Default)]
struct Backend {calls:Vec<Turn>, uncertain:bool}
impl Legacy for Backend {
    fn send(&mut self,t:&Turn)->Result<String,Error> {
        assert_eq!(t.owner,Some(Owner::Legacy));assert!(t.legacy_attempted);
        self.calls.push(t.clone());
        if self.uncertain {Err(Error::Http)} else {Ok("backend-response".into())}
    }
}
fn submit(c:&mut Controller,s:&mut Store,a:&mut Audio,b:&mut Backend,input:&str,locale:Locale,candidate:Candidate)->Outcome {
    let cancel=AtomicBool::new(false);let inventory=Inventory::default();
    c.submit(s,Submission{epoch:epoch(),nonce:"one",input,selection:choice(locale),inventory:&inventory,evidence:Evidence::default(),
        history:&[],intent:None,candidate,control:Control{started:Instant::now(),cancel:&cancel}},a,b).unwrap()
}
fn setup()->(Controller,Store,Audio,Backend) {
    let mut c=Controller::new("test-clock").unwrap();c.activate(epoch()).unwrap();
    (c,Store::open(&path(),"session").unwrap(),Audio::default(),Backend::default())
}
fn answer()->Candidate {Candidate::Controller(ControllerLine::Continue)}
#[test]
fn local_owner_precedes_speech_and_only_renderer_notice_enables_observe() {
    let (mut c,mut s,mut a,mut b)=setup();
    assert_eq!(submit(&mut c,&mut s,&mut a,&mut b,"public control",Locale::PtBr,answer()),Outcome::Started);
    assert_eq!(a.requests[0].text,"Entendi. Pode continuar.");assert!(b.calls.is_empty());
    assert_eq!(s.turns()[0].owner,Some(Owner::Front));assert!(s.prepare_observation().unwrap().is_none());
    let t=a.requests[0].ticket.clone();
    assert!(!t.render(Epoch{capture:4,..epoch()},t.response(),1));
    assert!(!t.render(epoch(),"foreign-response",1));
    assert!(!t.render(epoch(),t.response(),0));assert_eq!(t.frames(),0);
    assert!(t.render(epoch(),t.response(),160));
    assert!(!s.turns()[0].rendered,"callback must not fsync");
    c.render_notice(&mut s,&t).unwrap();
    assert!(matches!(s.prepare_observation().unwrap().unwrap().payload,Payload::User{..}));
    assert_eq!(c.counts().pt_br.front,1);assert_eq!(c.counts().pt_br.legacy,0);
}
#[test]
fn callback_render_before_delayed_durable_notice_prohibits_legacy_failover() {
    let (mut c,mut s,mut a,mut b)=setup();
    submit(&mut c,&mut s,&mut a,&mut b,"public control",Locale::En,answer());
    let t=a.requests[0].ticket.clone();assert!(t.render(epoch(),t.response(),160));
    assert!(!s.turns()[0].rendered);
    assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::PresentationUnknown);
    assert!(b.calls.is_empty());assert_eq!(s.turns()[0].owner,Some(Owner::Front));
    assert!(s.turns()[0].rendered);assert!(!s.front_admission_allowed());
    assert!(matches!(&s.turns()[0].presentation,Some(Payload::Assistant{spans,terminal:Terminal::PresentationUnknown}) if spans.is_empty()));
    assert!(!t.render(epoch(),t.response(),10));
}
#[test]
fn cancel_winning_before_first_frame_fences_all_late_pcm_and_sends_legacy_once() {
    let (mut c,mut s,mut a,mut b)=setup();
    submit(&mut c,&mut s,&mut a,&mut b,"public control",Locale::En,answer());
    let t=a.requests[0].ticket.clone();
    assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::LegacyAccepted);
    let late=t.clone();let worker=std::thread::spawn(move||late.render(epoch(),late.response(),1024));
    assert!(!worker.join().unwrap());assert_eq!(t.frames(),0);assert_eq!(b.calls.len(),1);
    assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::Existing);
    assert_eq!(submit(&mut c,&mut s,&mut a,&mut b,"public control",Locale::En,answer()),Outcome::Existing);
    assert_eq!(b.calls.len(),1);assert_eq!(s.turns()[0].owner,Some(Owner::Legacy));
    assert!(s.prepare_observation().unwrap().is_none());
    assert_eq!((c.counts().en.front,c.counts().en.legacy),(0,1));
}
#[test]
fn synchronous_start_error_obeys_the_same_callback_fence() {
    for rendered in [false,true] {
        let (mut c,mut s,mut a,mut b)=setup();a.fail=true;a.render_then_fail=rendered;
        let out=submit(&mut c,&mut s,&mut a,&mut b,"public control",Locale::En,answer());
        assert_eq!(out,if rendered{Outcome::PresentationUnknown}else{Outcome::LegacyAccepted});
        assert_eq!(b.calls.len(),usize::from(!rendered));
    }
}
#[test]
fn marker_and_unqualified_models_have_one_spoken_legacy_owner_and_no_front_ack() {
    for (locale,text) in [(Locale::En,"I sent it. [DELEGATE] Done."),(Locale::PtBr,"Okay, go on."),
        (Locale::PtBr,"Non sei sicuro."),(Locale::En,"We have enough coffee.")] {
        let (mut c,mut s,mut a,mut b)=setup();
        let out=submit(&mut c,&mut s,&mut a,&mut b,"public input",locale,Candidate::Model(Burst{text:text.into(),..Default::default()}));
        assert_eq!(out,Outcome::LegacyAccepted);assert!(a.requests.is_empty());assert_eq!(b.calls.len(),1);
        assert!(s.prepare_observation().unwrap().is_none());
        let counts=if locale==Locale::En {&c.counts().en}else{&c.counts().pt_br};
        assert_eq!((counts.front,counts.legacy),(0,1));
    }
}
#[test]
fn uncertain_legacy_send_and_reopened_work_never_send_again() {
    let p=path();let mut s=Store::open(&p,"session").unwrap();let mut c=Controller::new("test-clock").unwrap();c.activate(epoch()).unwrap();
    let mut a=Audio::default();let mut b=Backend{uncertain:true,..Default::default()};
    let model=||Candidate::Model(Burst{text:"unqualified".into(),..Default::default()});
    assert_eq!(submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,model()),Outcome::DeliveryUnknown);
    assert_eq!(submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,model()),Outcome::Existing);
    drop(s);let mut s=Store::open(&p,"session").unwrap();
    let mut c=Controller::new("new-clock").unwrap();c.activate(epoch()).unwrap();
    assert_eq!(submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,model()),Outcome::Existing);
    assert_eq!(b.calls.len(),1);assert!(a.requests.is_empty());
}
#[test]
fn actual_frames_and_exact_response_text_bound_terminal_spans() {
    let (mut c,mut s,mut a,mut b)=setup();
    submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,answer());
    let t=a.requests[0].ticket.clone();assert!(t.render(epoch(),t.response(),160));
    let span=|text:&str,end|vec![Span{first:0,end,text:text.into()}];
    assert!(c.finish(&mut s,&t,span("I understand.",161),Terminal::Interrupted).is_err());
    assert!(c.finish(&mut s,&t,span("Invented unheard words.",100),Terminal::Interrupted).is_err());
    assert!(c.finish(&mut s,&t,span("I understand.",100),Terminal::Completed).is_err());
    c.finish(&mut s,&t,span("I understand.",100),Terminal::Interrupted).unwrap();
    assert!(!t.render(epoch(),t.response(),10));assert!(b.calls.is_empty());
    let mut ctx=Context::new(STABLE_PROMPT).unwrap();ctx.append_rendered(&s.turns()[0]).unwrap();
    assert_eq!(ctx.prefix()[2]["content"],"I understand.");
    assert!(!serde_json::to_string(&ctx.prefix()).unwrap().contains("Please continue."));
}
#[test]
fn deliberate_cancellation_never_speaks_legacy_and_stale_ticket_cannot_retire_next_turn() {
    let (mut c,mut s,mut a,mut b)=setup();
    submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,answer());
    let t=a.requests[0].ticket.clone();c.cancel(&t,&mut a).unwrap();
    assert!(!t.render(epoch(),t.response(),10));
    assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::Cancelled);
    assert!(b.calls.is_empty());c.activate(Epoch{capture:4,turn:8}).unwrap();
    assert!(c.render_notice(&mut s,&t).is_err());
    assert!(c.finish(&mut s,&t,vec![],Terminal::Completed).is_err());
}
#[test]
fn foreign_active_epoch_and_expired_input_cannot_dispatch_or_reset_the_deadline() {
    let (mut c,mut s,mut a,mut b)=setup();let inv=Inventory::default();let cancel=AtomicBool::new(false);
    let mut sub=Submission{epoch:Epoch{capture:99,..epoch()},nonce:"one",input:"public",selection:choice(Locale::En),inventory:&inv,evidence:Evidence::default(),
        history:&[],intent:None,candidate:answer(),control:Control{started:Instant::now(),cancel:&cancel}};
    assert_eq!(c.submit(&mut s,sub,&mut a,&mut b).unwrap(),Outcome::Cancelled);assert!(s.turns().is_empty());
    sub=Submission{epoch:epoch(),nonce:"one",input:"public",selection:choice(Locale::En),inventory:&inv,evidence:Evidence::default(),
        history:&[],intent:None,candidate:answer(),control:Control{started:Instant::now()-std::time::Duration::from_secs(6),cancel:&cancel}};
    assert_eq!(c.submit(&mut s,sub,&mut a,&mut b).unwrap(),Outcome::LegacyAccepted);
    assert!(a.requests.is_empty());assert_eq!(b.calls.len(),1);
}
#[test]
fn inventory_authority_is_exact_input_bound_and_unknown_is_never_inability() {
    let (mut c,mut s,mut a,mut b)=setup();let inv=Inventory::default();let cancel=AtomicBool::new(false);
    let input="Can you use that capability?";
    let sub=Submission{epoch:epoch(),nonce:"one",input,selection:choice(Locale::PtBr),inventory:&inv,evidence:Evidence::default(),
        history:&[],intent:Some(CapabilityIntent{input,capability:Capability::Files}),candidate:answer(),
        control:Control{started:Instant::now(),cancel:&cancel}};
    assert_eq!(c.submit(&mut s,sub,&mut a,&mut b).unwrap(),Outcome::Started);
    assert_eq!(a.requests[0].text,"Ainda não tenho uma confirmação sobre esse recurso.");assert!(b.calls.is_empty());
}
#[test]
fn stage_marks_are_bounded_numeric_metadata_not_text_paths_or_provider_json() {
    let (mut c,mut s,mut a,mut b)=setup();submit(&mut c,&mut s,&mut a,&mut b,"private-like test input",Locale::En,answer());
    let marks=c.take_marks();let encoded=serde_json::to_string(&marks).unwrap();
    for bad in ["private-like","I understand","/Users/","token_usage","api_key"] {assert!(!encoded.contains(bad));}
    assert!(marks.iter().any(|m|m.stage=="local_durable_commit"));assert!(marks.iter().any(|m|m.stage=="owner_selected"));
    assert!(marks.iter().any(|m|m.stage=="tts_request"));
    assert!(marks.windows(2).all(|w|w[0].at_ns<=w[1].at_ns));assert!(c.take_marks().is_empty());
}
struct Count;
impl Counter for Count {
    fn text(&mut self,_:&str)->Result<usize,Error>{Ok(1)}
    fn prompt(&mut self,_:&[serde_json::Value])->Result<usize,Error>{Ok(10)}
}
#[test]
fn child_readiness_prefills_the_new_generation_before_turn_one_without_failed_generate() {
    use support::*;use serde_json::json;
    let mut ctx=Context::new(STABLE_PROMPT).unwrap();
    for _ in 0..2 {
        let (mut child,listener)=bound();let lease=child.request().unwrap();
        let server=json_server(listener.try_clone().unwrap(),vec![json!({"status":"ok"}),json!({"prompt":"exact prefix"}),json!({"tokens_predicted":0})]);
        ready_prefill(&lease,&mut ctx,true).unwrap();let sent=server.join().unwrap();assert_eq!(sent.len(),3);assert_eq!(sent[2]["n_predict"],0);
        assert!(!ctx.child_started(child.generation),"same child must keep a warm prefix");
        let tail=prompt_tail(choice(Locale::En),&Inventory::default(),"",false);
        let plan=ctx.prepare(&mut Count,&tail,"first user turn").unwrap();
        let server=serve(listener,response(&stream(&["First turn answer."],"stop"),false));
        assert_eq!(generate(&lease,&plan).unwrap().text,"First turn answer.");server.join().unwrap();drop(plan);drop(lease);child.close().unwrap();
    }
}
#[test]
fn an_old_child_prefill_receipt_cannot_clear_a_new_childs_hold() {
    use support::*;use serde_json::json;
    let mut ctx=Context::new(STABLE_PROMPT).unwrap();let (mut child,listener)=bound();let lease=child.request().unwrap();
    assert!(ctx.child_started(child.generation));
    let server=json_server(listener,vec![json!({"prompt":"old"}),json!({"tokens_predicted":0})]);
    let prefix=ctx.prefill_plan(true).unwrap();let receipt=prefill(&lease,&prefix).unwrap();drop(prefix);server.join().unwrap();
    drop(lease);child.close().unwrap();let (other,_listener)=bound();assert!(ctx.child_started(other.generation));
    assert!(ctx.mark_prefilled(receipt).is_err());assert!(ctx.prepare(&mut Count,&prompt_tail(choice(Locale::En),&Inventory::default(),"",false),"turn one").is_err());
}
#[test]
fn two_cancels_replay_separately_only_at_next_render_or_restart_and_context_labels_them() {
    let p=path();let mut s=Store::open(&p,"session").unwrap();
    let mut ids=Vec::new();
    for n in 0..2 {
        let t=s.commit(&format!("nonce-{n}"),&format!("Public canceled input {n}.")).unwrap();s.select_owner(&t.id,Owner::Front).unwrap();
        s.cancel_before_render(&t.id).unwrap();s.cancel_before_render(&t.id).unwrap();ids.push(t.id);
        assert!(s.prepare_observation().unwrap().is_none());assert!(all_silent(&s).is_empty());
    }
    s.compact().unwrap();assert!(s.prepare_observation().unwrap().is_none(),"compaction must not release live cancels");
    let next=s.commit("later","A later public input.").unwrap();s.select_owner(&next.id,Owner::Front).unwrap();s.first_rendered(&next.id).unwrap();
    let released=all_silent(&s);assert_eq!(released.iter().map(|t|t.id.clone()).collect::<Vec<_>>(),ids);
    let mut ctx=Context::new(STABLE_PROMPT).unwrap();for t in &released {ctx.append_silent_cancel(t).unwrap();}
    assert_eq!(ctx.prefix()[1]["content"],"Public canceled input 0.");assert_eq!(ctx.prefix()[3]["content"],"Public canceled input 1.");
    assert!(ctx.prefix()[2]["content"].as_str().unwrap().contains("cut off before speaking"));
    for id in &ids {
        let user=s.prepare_observation().unwrap().unwrap();assert_eq!(&user.turn_id,id);assert!(matches!(user.payload,Payload::User{..}));s.accept_observation(user.receipt()).unwrap();
        let assistant=s.prepare_observation().unwrap().unwrap();assert_eq!(&assistant.turn_id,id);
        assert!(matches!(&assistant.payload,Payload::Assistant{spans,terminal:Terminal::Interrupted}if spans.is_empty()));s.accept_observation(assistant.receipt()).unwrap();
    }
    let next_user=s.prepare_observation().unwrap().unwrap();assert_eq!(next_user.turn_id,next.id);
    drop(s);let s=Store::open(&p,"session").unwrap();assert_eq!(all_silent(&s).len(),2);
}
#[test]
fn cancellation_racing_first_frame_has_exactly_one_interrupted_terminal() {
    for rendered in [false,true] {
        let (mut c,mut s,mut a,mut b)=setup();submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,answer());
        let t=a.requests[0].ticket.clone();
        if rendered {assert!(t.render(epoch(),t.response(),1));}
        c.cancel(&t,&mut a).unwrap();assert!(!t.render(epoch(),t.response(),1));
        assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::Cancelled);
        let turn=&s.turns()[0];assert_eq!(turn.rendered,rendered);assert_eq!(turn.silent_replay,!rendered);
        assert!(matches!(&turn.presentation,Some(Payload::Assistant{spans,terminal:Terminal::Interrupted})if spans.is_empty()));
        assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::Existing);
        assert!(s.finish_rendered(&turn_id(&s),vec![],Terminal::Completed).is_err());assert!(b.calls.is_empty());
    }
}
fn turn_id(s:&Store)->String{s.turns()[0].id.clone()}
#[test]
fn cancel_variant_torn_tail_empty_first_append_unknown_and_restart_preserve_bytes() {
    use std::{fs,io::Write};
    for initial in [b"".as_slice(),b"{\"schema\":1,\"event\":{\"kind\":\"NoRenderCancel\"}".as_slice()] {
        let dir=path().parent().unwrap().to_path_buf();let p=dir.join("conversation-v2.jsonl");fs::write(&p,initial).unwrap();
        use std::os::unix::fs::PermissionsExt;fs::set_permissions(&p,fs::Permissions::from_mode(0o600)).unwrap();
        let mut s=Store::open_persisted(&dir,"session").unwrap();let t=s.commit("one","Public canceled input.").unwrap();s.select_owner(&t.id,Owner::Front).unwrap();s.cancel_before_render(&t.id).unwrap();drop(s);
        let clean=fs::read(&p).unwrap();let mut f=fs::OpenOptions::new().append(true).open(&p).unwrap();f.write_all(b"{\"event\":{\"kind\":\"NoRenderCancel\"").unwrap();drop(f);
        let mut s=Store::open_persisted(&dir,"session").unwrap();assert_eq!(fs::read(&p).unwrap(),clean);assert_eq!(all_silent(&s).len(),1);
        assert!(s.prepare_observation().unwrap().is_some());drop(s);
        let unknown=String::from_utf8(clean).unwrap().replace("NoRenderCancel","FutureCancelUnknown");fs::write(&p,unknown.as_bytes()).unwrap();
        assert!(matches!(Store::for_front(&dir,"session"),conversation_log::FrontStore::LegacyOnly{..}));assert_eq!(fs::read(&p).unwrap(),unknown.as_bytes());
    }
}
#[test]
fn superseded_unselected_and_unsent_legacy_turns_resolve_live_without_say_or_head_of_line_stall() {
    for owner in [None,Some(Owner::Legacy)] {
        let (mut c,mut s,mut a,mut b)=setup();let t=s.commit("old","A superseded public input.").unwrap();
        if let Some(owner)=owner{s.select_owner(&t.id,owner).unwrap();}
        assert_eq!(c.supersede(&mut s,&t.id,&mut a,&mut b).unwrap(),Outcome::Cancelled);assert!(b.calls.is_empty());
        let next=s.commit("next","A new public input.").unwrap();s.select_owner(&next.id,Owner::Front).unwrap();s.first_rendered(&next.id).unwrap();
        assert_eq!(s.prepare_observation().unwrap().unwrap().turn_id,t.id);assert!(s.front_admission_allowed());
    }
}
#[test]
fn realtime_capture_fence_is_intentional_cancel_even_before_its_worker_notice() {
    let (mut c,mut s,mut a,mut b)=setup();submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,answer());
    let t=a.requests[0].ticket.clone();assert!(!t.fence());
    assert_eq!(c.failed(&mut s,&t,&mut a,&mut b).unwrap(),Outcome::Cancelled);
    assert!(b.calls.is_empty());assert!(s.turns()[0].silent_replay);
}
#[test]
fn simultaneous_frame_and_cancel_linearize_to_one_durable_terminal() {
    for _ in 0..16 {
        let p=path();let mut s=Store::open(&p,"session").unwrap();let mut c=Controller::new("race-clock").unwrap();c.activate(epoch()).unwrap();
        let mut a=Audio::default();let mut b=Backend::default();submit(&mut c,&mut s,&mut a,&mut b,"public input",Locale::En,answer());
        let ticket=a.requests[0].ticket.clone();let other=ticket.clone();let barrier=Arc::new(std::sync::Barrier::new(2));let go=barrier.clone();
        let thread=std::thread::spawn(move||{go.wait();other.render(epoch(),other.response(),16)});
        barrier.wait();ticket.fence();let rendered=thread.join().unwrap();
        assert_eq!(c.failed(&mut s,&ticket,&mut a,&mut b).unwrap(),Outcome::Cancelled);
        assert_eq!(s.turns()[0].rendered,rendered);assert!(b.calls.is_empty());
        let events=std::fs::read_to_string(&p).unwrap();let n=events.lines().map(|l|serde_json::from_str::<serde_json::Value>(l).unwrap())
            .filter(|r|matches!(r["event"]["kind"].as_str(),Some("NoRenderCancel"|"Presentation"))).count();assert_eq!(n,1);
    }
}

fn accept_legacy_only(s:&mut Store,nonce:&str) {
    let t=s.commit(nonce,"Public legacy input.").expect("silent cancels must never refuse a legacy commit");
    s.select_owner(&t.id,Owner::Legacy).unwrap();assert!(s.begin_legacy_send(&t.id).unwrap());
    s.accept_legacy(&t.id,"backend-reply").unwrap();
}
fn drain(s:&mut Store)->Vec<conversation_log::observe::Observation> {
    let mut rows=Vec::new();while let Some(item)=s.prepare_observation().unwrap() {
        s.accept_observation(item.receipt()).unwrap();rows.push(item);
    }rows
}
#[test]
fn one_hundred_cancel_legacy_cycles_release_and_observe_in_order_without_front_sound() {
    let mut s=Store::open(&path(),"session").unwrap();let mut ids=Vec::new();let mut observed=Vec::new();
    for n in 0..100 {
        let t=s.commit(&format!("cancel-{n}"),&format!("Public cancelled input {n}.")).unwrap();
        s.cancel_before_render(&t.id).unwrap();ids.push(t.id);
        assert_eq!(all_silent(&s).len(),n);
        accept_legacy_only(&mut s,&format!("legacy-{n}"));
        assert_eq!(all_silent(&s).iter().map(|t|t.id.clone()).collect::<Vec<_>>(),ids);
        observed.extend(drain(&mut s));
        assert_eq!(observed.len(),2*(n+1));assert!(s.front_admission_allowed());
    }
    assert!(s.turns().iter().all(|t|!t.rendered));
    assert_eq!(observed.chunks_exact(2).map(|p|{assert_eq!(p[0].turn_id,p[1].turn_id);p[0].turn_id.clone()}).collect::<Vec<_>>(),ids);
    assert!(observed.iter().skip(1).step_by(2).all(|i|matches!(&i.payload,Payload::Assistant{spans,terminal:Terminal::Interrupted}if spans.is_empty())));
    assert!(observed.iter().enumerate().all(|(n,i)|i.version==n as u64+1));
}
#[test]
fn waiting_cancels_do_not_exhaust_legacy_admission_even_without_a_later_terminal() {
    let p=path();let mut s=Store::open(&p,"session").unwrap();
    for n in 0..100 {let t=s.commit(&format!("cancel-{n}"),"Public cancelled input.").unwrap();s.cancel_before_render(&t.id).unwrap();}
    assert!(all_silent(&s).is_empty());assert!(s.prepare_observation().unwrap().is_none());
    s.compact().unwrap();assert!(all_silent(&s).is_empty());
    accept_legacy_only(&mut s,"legacy");assert_eq!(all_silent(&s).len(),100);assert_eq!(drain(&mut s).len(),200);
    drop(s);let mut s=Store::open(&p,"session").unwrap();assert!(s.prepare_observation().unwrap().is_none());
    accept_legacy_only(&mut s,"after-reopen");
}
#[test]
fn first_render_then_presentation_releases_once_and_never_reorders_later_cancels() {
    let mut s=Store::open(&path(),"session").unwrap();let old=s.commit("old","Public cancelled input.").unwrap();s.cancel_before_render(&old.id).unwrap();
    let next=s.commit("front","Public front input.").unwrap();s.select_owner(&next.id,Owner::Front).unwrap();s.first_rendered(&next.id).unwrap();
    let mut seen=drain(&mut s);assert_eq!(seen.iter().filter(|i|i.turn_id==old.id).count(),2);
    let later=s.commit("later-cancel","A later cancelled input.").unwrap();s.cancel_before_render(&later.id).unwrap();
    s.finish_rendered(&next.id,vec![Span{first:0,end:16,text:"Public answer.".into()}],Terminal::Completed).unwrap();
    seen.extend(drain(&mut s));assert_eq!(seen.iter().filter(|i|i.turn_id==old.id).count(),2);
    assert_eq!(all_silent(&s).iter().map(|t|t.id.as_str()).collect::<Vec<_>>(),[old.id.as_str()]);
    assert!(s.prepare_observation().unwrap().is_none());
    accept_legacy_only(&mut s,"last");let rest=drain(&mut s);assert_eq!(rest.len(),2);assert!(rest.iter().all(|i|i.turn_id==later.id));
    let released=all_silent(&s);assert_eq!(released.iter().map(|t|t.id.as_str()).collect::<Vec<_>>(),[old.id.as_str(),later.id.as_str()]);
    let mut ctx=Context::new(STABLE_PROMPT).unwrap();ctx.append_silent_cancel(&released[0]).unwrap();assert!(ctx.append_silent_cancel(&released[0]).is_err());
}
#[test]
fn later_presentation_releases_a_delayed_cancel_even_if_first_render_was_earlier() {
    let mut s=Store::open(&path(),"session").unwrap();let old=s.commit("old","Public cancelled input.").unwrap();
    let next=s.commit("front","Public front input.").unwrap();s.select_owner(&next.id,Owner::Front).unwrap();s.first_rendered(&next.id).unwrap();
    s.cancel_before_render(&old.id).unwrap();assert!(all_silent(&s).is_empty());
    s.finish_rendered(&next.id,vec![],Terminal::Interrupted).unwrap();assert_eq!(all_silent(&s)[0].id,old.id);
    assert_eq!(drain(&mut s).iter().map(|i|i.turn_id.as_str()).collect::<Vec<_>>(),[old.id.as_str(),old.id.as_str(),next.id.as_str(),next.id.as_str()]);
}
#[test]
fn store_terminal_guards_refuse_first_render_after_cancel_and_cancel_after_render() {
    let mut s=Store::open(&path(),"session").unwrap();let old=s.commit("old","Public input.").unwrap();s.select_owner(&old.id,Owner::Front).unwrap();
    s.cancel_before_render(&old.id).unwrap();assert!(s.first_rendered(&old.id).is_err());
    let next=s.commit("next","Public next input.").unwrap();s.select_owner(&next.id,Owner::Front).unwrap();s.first_rendered(&next.id).unwrap();
    assert!(s.cancel_before_render(&next.id).is_err());
}
#[test]
fn capture_fence_returns_true_after_any_winning_render() {
    let (mut c,mut s,mut a,mut b)=setup();submit(&mut c,&mut s,&mut a,&mut b,"Public input.",Locale::En,answer());
    let t=&a.requests[0].ticket;assert!(t.render(epoch(),t.response(),1));assert!(t.fence());assert!(t.fence());
}
#[test]
fn cancel_at_selection_or_dispatch_never_speaks_sends_or_leaves_an_unresolved_turn() {
    for front in [false,true] {for dispatch in [false,true] {
        let log=path();let mut s=Store::open(&log,"session").unwrap();let mut c=Controller::new("cancel-clock").unwrap();c.activate(epoch()).unwrap();let (mut a,mut b)=(Audio::default(),Backend::default());let cancel=Arc::new(AtomicBool::new(false));let flag=cancel.clone();let inv=Inventory::default();
        on_next_select_for_test(move||{
            if dispatch {on_next_select_for_test(move||flag.store(true,Ordering::SeqCst));}
            else {flag.store(true,Ordering::SeqCst);}
        });
        let out=c.submit(&mut s,Submission{epoch:epoch(),nonce:"cancelled",input:"Public cancelled input.",selection:choice(Locale::En),inventory:&inv,
            evidence:Evidence::default(),history:&[],intent:None,candidate:if front{answer()}else{Candidate::Model(Burst{text:"unqualified".into(),..Default::default()})},
            control:Control{started:Instant::now(),cancel:&cancel}},&mut a,&mut b).unwrap();
        assert_eq!(out,Outcome::Cancelled);assert!(a.requests.is_empty());assert!(b.calls.is_empty());assert!(s.turns()[0].silent_replay);
        let bytes=std::fs::read_to_string(&log).unwrap();
        assert_eq!(bytes.lines().filter(|l|serde_json::from_str::<serde_json::Value>(l).unwrap()["event"]["kind"]=="NoRenderCancel").count(),1);
        accept_legacy_only(&mut s,"next");assert_eq!(drain(&mut s).len(),2);
    }}
}

#[test]
fn readiness_outside_a_quiet_boundary_refuses_before_health_or_prefill() {
    let (child,listener)=support::bound();let lease=child.request().unwrap();let mut ctx=Context::new(STABLE_PROMPT).unwrap();
    assert_eq!(ready_prefill(&lease,&mut ctx,false),Err(Error::Prefill));
    assert!(ctx.child_started(child.generation),"quiet refusal must not change child/context identity");
    listener.set_nonblocking(true).unwrap();assert_eq!(listener.accept().unwrap_err().kind(),std::io::ErrorKind::WouldBlock);
}

fn all_silent(s: &Store) -> Vec<Turn> {
    let mut window = conversation_log::SilentWindow::default(); let mut turns = vec![];
    loop { let batch = s.ready_silent_cancels(&mut window); turns.extend(batch.turns); if !batch.more { return turns; } }
}

#[test]
fn transport_contention_before_fence_is_not_sent_and_the_same_turn_can_retry_once() {
    struct Admission {busy:bool,calls:usize,uncertain:bool}
    impl Legacy for Admission {
        fn send(&mut self,t:&Turn)->Result<String,Error>{assert!(t.legacy_attempted);self.calls+=1;if self.uncertain{Err(Error::Http)}else{Ok("admitted-response".into())}}
        fn send_admitted(&mut self,_:&Turn,fence:&mut dyn FnMut()->Result<Turn,Error>)->Result<String,Error>{
            if self.busy{return Err(Error::Busy);}self.send(&fence()?)
        }
    }
    for uncertain in [false,true] {
        let (mut c,mut s,mut a,_)=setup();let mut b=Admission{busy:true,calls:0,uncertain};
        let cancel=AtomicBool::new(false);let inventory=Inventory::default();
        let mut attempt=|c:&mut Controller,s:&mut Store,b:&mut Admission|c.submit(s,Submission {epoch:epoch(),nonce:"admission",input:"public input",
            selection:choice(Locale::En),inventory:&inventory,evidence:Evidence::default(),history:&[],intent:None,
            candidate:Candidate::Model(Default::default()),control:Control{started:Instant::now(),cancel:&cancel}},&mut a,b).unwrap();
        assert_eq!(attempt(&mut c,&mut s,&mut b),Outcome::NotSent);assert_eq!(b.calls,0);
        assert!(!s.turns()[0].legacy_attempted);assert_eq!(s.turns().len(),1);
        b.busy=false;assert_eq!(attempt(&mut c,&mut s,&mut b),if uncertain{Outcome::DeliveryUnknown}else{Outcome::LegacyAccepted});
        assert_eq!(b.calls,1);assert!(s.turns()[0].legacy_attempted);
        assert_eq!(attempt(&mut c,&mut s,&mut b),Outcome::Existing);assert_eq!(b.calls,1);
    }
}

#[test]
fn front_request_keeps_the_original_turn_deadline_after_preparation() {
    let (mut c,mut s,mut a,mut b)=setup();let cancel=AtomicBool::new(false);let inventory=Inventory::default();
    let started=Instant::now()-std::time::Duration::from_secs(4);
    assert_eq!(c.submit(&mut s,Submission {epoch:epoch(),nonce:"deadline-original",input:"public control",selection:choice(Locale::En),
        inventory:&inventory,evidence:Evidence::default(),history:&[],intent:None,candidate:answer(),control:Control{started,cancel:&cancel}},&mut a,&mut b).unwrap(),Outcome::Started);
    assert_eq!(a.requests[0].deadline,started+std::time::Duration::from_secs(5));
}

#[test]
fn cancel_during_transport_admission_prevents_the_durable_attempt_and_write() {
    struct DuringAdmission<'a>{cancel:&'a AtomicBool,writes:usize}
    impl Legacy for DuringAdmission<'_>{
        fn send(&mut self,_:&Turn)->Result<String,Error>{self.writes+=1;Ok("must-not-send".into())}
        fn send_admitted(&mut self,_:&Turn,f:&mut dyn FnMut()->Result<Turn,Error>)->Result<String,Error>{

            self.cancel.store(true,Ordering::SeqCst);self.send(&f()?)
        }
    }
    let (mut c,mut s,mut a,_)=setup();let cancel=AtomicBool::new(false);let mut backend=DuringAdmission{cancel:&cancel,writes:0};
    let inventory=Inventory::default();
    let result=c.submit(&mut s,Submission{epoch:epoch(),nonce:"cancel-at-admission",input:"public input",selection:choice(Locale::En),inventory:&inventory,evidence:Evidence::default(),history:&[],intent:None,candidate:Candidate::Model(Burst::default()),control:Control{started:Instant::now(),cancel:&cancel}},&mut a,&mut backend).unwrap();
    assert_eq!(result,Outcome::Cancelled,"cancel at transport admission must be rechecked before fsync/write");
    assert_eq!(backend.writes,0);assert!(!s.turns()[0].legacy_attempted);assert!(s.turns()[0].silent_replay);
}
