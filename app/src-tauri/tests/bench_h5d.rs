
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"] mod conversation_log;
#[allow(dead_code)]
#[path = "../src/voice_front.rs"] mod voice_front;
use conversation_log::{Store,Owner};
use conversation_log::observe::{Span,Terminal};
use voice_front::{policy::*,http::Burst,Error};
use std::{sync::atomic::{AtomicBool,Ordering},time::{Instant,Duration}};
use serde_json::Value;
fn path()->std::path::PathBuf {
    static N:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let p=std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string()))
        .join(format!("front-policy-{}-{}",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));
    std::fs::create_dir_all(&p).unwrap();p.join("log")
}
fn heard(id:&str,input:&str,text:&str)->RenderedReply {
    RenderedReply::from_renderer(id,input,vec![Span{first:0,end:1600,text:text.into()}],Terminal::Completed).unwrap()
}
fn burst(text:&str)->Candidate { Candidate::Model(Burst{text:text.into(),..Default::default()}) }
fn selection(locale:Locale)->Selection {Selection{locale,source:LocaleSource::Setting,confidence:None}}
fn request<'a>(id:&'a str,locale:Locale,inventory:&'a Inventory,history:&'a [RenderedReply],cancel:&'a AtomicBool)->TurnRequest<'a> {
    TurnRequest{id,selection:selection(locale),inventory,evidence:Evidence::default(),history,intent:None,repeat:None,control:Control{started:Instant::now(),cancel}}
}
struct FixtureValidator;
impl Validator for FixtureValidator {
    fn qualification(&self,locale:Locale)->Option<&'static str> { (locale==Locale::En).then_some("cpu-fixture-only") }
    fn assess(&mut self,c:&Check<'_>)->Option<Approval> {
        if !["One two three four five.","A different useful answer here.","Another distinct public answer."].contains(&c.reply) {return None}
        Some(Approval{input:c.input.into(),reply:c.reply.into(),locale:c.locale,inventory_revision:c.inventory_revision,
            evidence_revision:c.evidence.revision,evidence:c.evidence.text.into(),policy_version:POLICY_VERSION,validator_id:"cpu-fixture-only",language_qualified:true,claims_qualified:true})
    }
}
#[test]
fn locale_uses_settings_and_short_ambiguous_inputs_never_switch_it() {
    let base=settings_locale(Some("pt-BR"),LocaleSource::System).unwrap();let mut lock=LocaleLock::new(base);
    for input in ["sei","uhum","entendi","pois é"] {
        let got=lock.for_committed(input,Some(LanguageEvidence{input,tag:"it",confidence:100,complete:true,explicit:false})).unwrap();
        assert_eq!(got.locale,Locale::PtBr);
    }
    let input="Please answer in English.";
    assert_eq!(lock.for_committed(input,Some(LanguageEvidence{input,tag:"en",confidence:100,complete:true,explicit:true})).unwrap().locale,Locale::En);
    assert_eq!(lock.for_committed("uhum",None).unwrap().locale,Locale::En);
    assert!(settings_locale(Some("pt-PT"),LocaleSource::System).is_none());
    assert!(settings_locale(None,LocaleSource::System).is_none());
}
#[test]
fn synthetic_wrong_language_controls_never_reach_front_speech() {
    let rows:Value=serde_json::from_str(include_str!("fixtures/language-controls.json")).unwrap();
    let mut n=0;
    for row in rows["rows"].as_array().unwrap().iter().filter(|r|r["id"].as_str().unwrap().starts_with("control-pt-BR-bc-")) {
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("input",row["text"].as_str().unwrap()).unwrap();
        let inv=Inventory::default();let cancel=AtomicBool::new(false);let req=request(&t.id,Locale::PtBr,&inv,&[],&cancel);
        let d=route_committed(&mut s,&req,burst(row["reply"].as_str().unwrap()),&mut Unqualified,|_|panic!("no speculative guard retry")).unwrap();
        assert_eq!(d.owner,Owner::Legacy);assert!(d.speech.is_none());n+=1;
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("input",row["text"].as_str().unwrap()).unwrap();
        let req=request(&t.id,Locale::PtBr,&inv,&[],&cancel);
        let d=route_committed(&mut s,&req,Candidate::Controller(ControllerLine::Continue),&mut Unqualified,|_|panic!()).unwrap();
        assert_eq!(d.speech.as_deref(),Some("Entendi. Pode continuar."));
    }
    assert_eq!(n,4);
}
#[test]
fn markers_drop_all_model_claims_and_persist_the_legacy_route_before_observe() {
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","A public request.").unwrap();
    let inv=Inventory::default();let cancel=AtomicBool::new(false);let req=request(&t.id,Locale::En,&inv,&[],&cancel);
    let d=route_committed(&mut s,&req,burst("I already emailed it. [DELEGATE] I cannot help."),&mut Unqualified,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Marker);assert!(d.speech.is_none());
    assert!(s.prepare_observation().unwrap().is_none());
    assert!(s.begin_legacy_send(&t.id).unwrap());assert!(!s.begin_legacy_send(&t.id).unwrap());
    let d=route_committed(&mut s,&req,burst("late answer"),&mut Unqualified,|_|panic!()).unwrap();
    assert_eq!(d.reason,Reason::Existing);assert!(d.speech.is_none());
}
#[test]
fn unsupported_personal_state_refusal_and_effect_claims_use_spoken_legacy() {
    for locale in [Locale::En,Locale::PtBr] { for text in ["We don't need more blue folders right now.","No, we haven't discussed a folder plan yet.",
        "The blue folder is now archived and ready for your review.","I can't discuss blue folders right now.",
        "Já enviei a mensagem.","Não precisamos de mais café.","Não tenho acesso aos seus arquivos."] {
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public request.").unwrap();
        let inv=Inventory::default();let cancel=AtomicBool::new(false);let req=request(&t.id,locale,&inv,&[],&cancel);
        let d=route_committed(&mut s,&req,burst(text),&mut Unqualified,|_|panic!()).unwrap();
        assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Guard);assert!(d.speech.is_none());
    }
}
}
#[test]
fn capability_answers_are_controller_owned_and_unknown_never_implies_inability() {
    for (availability,word) in [(Availability::Available,"available."),(Availability::Unavailable,"unavailable"),(Availability::Unknown,"verified status")] {
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Can you use that capability?").unwrap();
        let inv=Inventory::new(7,&[(Capability::Files,availability)]);let cancel=AtomicBool::new(false);
        let mut req=request(&t.id,Locale::En,&inv,&[],&cancel);req.intent=Some(CapabilityIntent{input:&t.input,capability:Capability::Files});
        let d=route_committed(&mut s,&req,burst("I sent the files already."),&mut Unqualified,|_|panic!()).unwrap();
        let text=d.speech.unwrap();assert!(text.contains(word));assert!(!text.contains("sent"));
        if availability==Availability::Unknown {assert!(!text.contains("unavailable"));assert!(!text.contains("cannot"));}
        assert!(s.prepare_observation().unwrap().is_none(),"nothing mirrored before render");
    }
    assert_eq!(ack(Locale::En),"Understood.");assert_eq!(ack(Locale::PtBr),"Entendido.");
}
#[test]
fn repeat_rule_keeps_unicode_short_exact_near_and_targeted_exemptions() {
    assert_eq!(normalize("CAFÉ—AÇÃO_2! \u{301}¾"),"café ação_2 ¾");
    assert!(!is_repeat("one two three four","one two three four five"));
    assert!(is_repeat("Okay!","okay"));assert!(!is_repeat("one two three four","one two three five"));
    assert!(is_repeat("one two three four five","one two three four six"));
    let history=vec![heard("reply-old","different input","Okay."),heard("reply-target","another input","Target words only.")];
    assert_eq!(repeat_hits("new input","Okay!",&history,None),vec!["reply-old"]);
    assert!(repeat_hits("different input","Okay!",&history,None).is_empty());
    let permit=RepeatPermit{input:"new input",target_response:"reply-target"};
    assert!(!repeat_hits("new input","Okay!",&history,Some(permit)).is_empty());
    let permit=RepeatPermit{input:"new input",target_response:"reply-old"};
    assert!(repeat_hits("new input","Okay!",&history,Some(permit)).is_empty());
    assert!(RenderedReply::from_renderer("reply","input",vec![Span{first:0,end:1,text:"invented".into()}],Terminal::PresentationUnknown).is_err());
}
#[test]
fn retry_is_once_revalidates_markers_and_never_inserts_discarded_text() {
    for retry_reply in ["A different useful answer here.","One two three four five.","Buongiorno! Ecco una cartella blu per te.","False effect [DELEGATE] refusal"] {
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","New public input.").unwrap();
        let inv=Inventory::default();let cancel=AtomicBool::new(false);let history=vec![heard("old","Different public input.","One two three four five.")];
        let req=request(&t.id,Locale::En,&inv,&history,&cancel);let mut calls=0;
        let d=route_committed(&mut s,&req,burst("One two three four five."),&mut FixtureValidator,|r| {
            calls+=1;assert_eq!(r.locale,Locale::En);assert_eq!(r.note,RETRY_NOTE);Ok(Burst{text:retry_reply.into(),..Default::default()})
        }).unwrap();
        assert_eq!(calls,1);assert!(d.retried);
        assert_eq!(d.owner,if retry_reply.starts_with("A different") {Owner::Front}else{Owner::Legacy});
        assert!(s.turns()[0].presentation.is_none());assert!(s.prepare_observation().unwrap().is_none());
    }
}
#[test]
fn one_shared_deadline_and_cancel_fence_survive_a_retry() {
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    let inv=Inventory::default();let cancel=AtomicBool::new(false);let mut req=request(&t.id,Locale::En,&inv,&[],&cancel);
    req.control.started=Instant::now()-Duration::from_secs(6);
    let d=route_committed(&mut s,&req,burst("One two three four five."),&mut FixtureValidator,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Deadline);
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    let history=vec![heard("old","different","One two three four five.")];let req=request(&t.id,Locale::En,&inv,&history,&cancel);
    let result=route_committed(&mut s,&req,burst("One two three four five."),&mut FixtureValidator,|_| {
        cancel.store(true,Ordering::SeqCst);Ok(Burst{text:"A different useful answer here.".into(),..Default::default()})
    });
    assert!(matches!(result,Err(Error::Cancelled)));assert_eq!(s.turns()[0].owner,None);
}
#[test]
fn tail_is_volatile_after_the_stable_prefix_and_wrong_proof_bindings_fail() {
    let inv=Inventory::new(2,&[(Capability::Memory,Availability::Unknown)]);
    let tail=prompt_tail(selection(Locale::PtBr),&inv,"quoted evidence",true);
    assert_eq!(tail.locale,"pt-BR");assert!(tail.status.contains(RETRY_NOTE));assert!(tail.status.contains("unknown"));
    let ctx=voice_front::context::Context::new(STABLE_PROMPT).unwrap();
    assert_eq!(ctx.prefix()[0]["content"],STABLE_PROMPT);
    struct Wrong(usize);
    impl Validator for Wrong {
        fn qualification(&self,_:Locale)->Option<&'static str>{Some("fixture")}
        fn assess(&mut self,c:&Check<'_>)->Option<Approval>{
            let mut a=Approval{input:c.input.into(),reply:c.reply.into(),locale:c.locale,inventory_revision:c.inventory_revision,
                evidence_revision:c.evidence.revision,evidence:c.evidence.text.into(),policy_version:POLICY_VERSION,validator_id:"fixture",language_qualified:true,claims_qualified:true};
            match self.0 {0=>a.input="different".into(),1=>a.reply="different".into(),2=>a.locale=Locale::PtBr,
                3=>a.inventory_revision+=1,4=>a.policy_version="other",5=>a.validator_id="other",6=>a.language_qualified=false,7=>a.claims_qualified=false,8=>a.evidence_revision+=1,_=>a.evidence="foreign evidence".into()};Some(a)
        }
    }
    for mode in 0..10 {
        let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();let cancel=AtomicBool::new(false);
        let req=request(&t.id,Locale::En,&inv,&[],&cancel);
        let d=route_committed(&mut s,&req,burst("A different useful answer here."),&mut Wrong(mode),|_|panic!()).unwrap();
        assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Guard);
    }
}

#[test]
fn retry_cannot_reset_the_original_five_second_deadline() {
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","New public input.").unwrap();
    let inv=Inventory::default();let cancel=AtomicBool::new(false);let history=vec![heard("old","other input","One two three four five.")];
    let mut req=request(&t.id,Locale::En,&inv,&history,&cancel);req.control.started=Instant::now()-Duration::from_secs(4);
    let mut calls=0;
    let d=route_committed(&mut s,&req,burst("One two three four five."),&mut FixtureValidator,|r| {
        calls+=1;assert!(r.remaining<=Duration::from_secs(1));std::thread::sleep(Duration::from_millis(1100));
        Ok(Burst{text:"A different useful answer here.".into(),..Default::default()})
    }).unwrap();assert_eq!(calls,1);assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Deadline);assert!(d.speech.is_none());
}
#[test]
fn locale_evidence_and_capability_intent_are_bound_to_this_committed_input() {
    let mut lock=LocaleLock::new(selection(Locale::PtBr));let input="Please describe the public picture.";
    let got=lock.for_committed(input,Some(LanguageEvidence{input:"other",tag:"en",confidence:100,complete:true,explicit:true})).unwrap();
    assert_eq!(got.locale,Locale::PtBr);
    let got=lock.for_committed(input,Some(LanguageEvidence{input,tag:"en",confidence:97,complete:true,explicit:false})).unwrap();
    assert_eq!(got.source,LocaleSource::Utterance);assert_eq!(got.confidence,Some(97));
    let inv=Inventory::default();let status:Value=serde_json::from_str(&prompt_tail(got,&inv,"",false).status).unwrap();
    assert_eq!(status["locale_source"],"utterance");assert_eq!(status["locale_confidence"],97);
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Current public input.").unwrap();let cancel=AtomicBool::new(false);
    let mut req=request(&t.id,Locale::En,&inv,&[],&cancel);req.intent=Some(CapabilityIntent{input:"other input",capability:Capability::Files});
    let d=route_committed(&mut s,&req,Candidate::Controller(ControllerLine::Continue),&mut Unqualified,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Guard);
}
#[test]
fn repeat_window_uses_only_four_rendered_replies_and_no_blanket_permit() {
    let mut history=vec![heard("old","old input","Okay.")];
    for n in 0..4 {history.push(heard(&format!("reply-{n}"),"another input",&format!("Distinct rendered response {n}.")));}
    assert!(repeat_hits("new input","Okay!",&history,None).is_empty());
    assert!(!repeat_hits("new input","Distinct rendered response 3.",&history,Some(RepeatPermit{input:"other",target_response:"reply-3"})).is_empty());
    let history=vec![heard("old","DIFFERENT—INPUT!","Okay.")];
    assert!(repeat_hits("different input","Okay!",&history,None).is_empty());
}

#[test]
fn blocked_front_admission_routes_new_input_legacy_and_a_superseded_turn_never_routes() {
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();s.mark_mirror_conflict().unwrap();
    let inv=Inventory::default();let cancel=AtomicBool::new(false);let req=request(&t.id,Locale::En,&inv,&[],&cancel);
    let d=route_committed(&mut s,&req,Candidate::Controller(ControllerLine::Continue),&mut Unqualified,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::FrontFailure);
    let mut s=Store::open(&path(),"session").unwrap();let old=s.commit("old","Old public input.").unwrap();s.commit("new","New public input.").unwrap();
    let req=request(&old.id,Locale::En,&inv,&[],&cancel);
    assert!(matches!(route_committed(&mut s,&req,Candidate::Controller(ControllerLine::Continue),&mut Unqualified,|_|panic!()),Err(Error::Cancelled)));
    assert!(s.turns().iter().all(|t|t.owner.is_none()));
}

#[test]
fn the_deadline_expiring_after_durable_owner_selection_cannot_return_front_speech() {
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    let inv=Inventory::default();let cancel=AtomicBool::new(false);let mut req=request(&t.id,Locale::En,&inv,&[],&cancel);
    req.control.started=Instant::now()-Duration::from_secs(4);
    on_next_select_for_test(||std::thread::sleep(Duration::from_millis(1100)));
    let d=route_committed(&mut s,&req,burst("A different useful answer here."),&mut FixtureValidator,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Legacy);assert_eq!(d.reason,Reason::Deadline);assert!(d.speech.is_none());assert_eq!(s.turns()[0].owner,Some(Owner::Legacy));
}
#[test]
fn validation_receipt_binds_the_exact_nonempty_evidence_and_revision() {
    struct Bound;
    impl Validator for Bound {
        fn qualification(&self,_:Locale)->Option<&'static str>{Some("cpu-evidence-fixture")}
        fn assess(&mut self,c:&Check<'_>)->Option<Approval>{
            assert_eq!(c.evidence.revision,19);assert_eq!(c.evidence.text,"Trusted public evidence.");
            Some(Approval{input:c.input.into(),reply:c.reply.into(),locale:c.locale,inventory_revision:c.inventory_revision,
                evidence_revision:c.evidence.revision,evidence:c.evidence.text.into(),policy_version:POLICY_VERSION,
                validator_id:"cpu-evidence-fixture",language_qualified:true,claims_qualified:true})
        }
    }
    let mut s=Store::open(&path(),"session").unwrap();let t=s.commit("one","Public input.").unwrap();let inv=Inventory::default();let cancel=AtomicBool::new(false);
    let mut req=request(&t.id,Locale::En,&inv,&[],&cancel);req.evidence=Evidence{revision:19,text:"Trusted public evidence."};
    let d=route_committed(&mut s,&req,burst("A different useful answer here."),&mut Bound,|_|panic!()).unwrap();
    assert_eq!(d.owner,Owner::Front);
}
