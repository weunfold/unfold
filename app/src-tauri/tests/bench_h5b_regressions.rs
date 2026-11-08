
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
use conversation_log::{Owner, Store, FrontStore};
use conversation_log::observe::{Observation, Payload, Snapshot, Span, Terminal, MemoryReceipt, MemoryOutcome};
use std::{path::PathBuf, io::Write, time::{Duration, Instant}};
use sha2::{Digest, Sha256};
fn dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n=NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    let p=PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string())).join(format!("b02-{}-{n}",std::process::id()));
    std::fs::create_dir_all(&p).unwrap(); p
}
fn rendered(s:&mut Store, n:&str)->String {
    let t=s.commit(n,"Public input.").unwrap(); s.select_owner(&t.id,Owner::Front).unwrap();
    s.first_rendered(&t.id).unwrap(); t.id
}
fn finish(s:&mut Store,id:&str) { s.finish_rendered(id,vec![Span {first:0,end:1600,text:"Rendered words.".into()}],Terminal::Completed).unwrap(); }
fn ack(s:&mut Store)->Observation { let o=s.prepare_observation().unwrap().unwrap();s.accept_observation(o.receipt()).unwrap();o }
#[test]
fn empty_initial_record_recovers_on_the_production_path() {
    let d=dir(); let p=d.join("conversation-v2.jsonl");
    std::fs::File::create(&p).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p,std::fs::Permissions::from_mode(0o600)).unwrap();
    let FrontStore::Ready(mut s)=Store::for_front(&d,"session") else {panic!("empty initial append must recover")};
    s.commit("new","Recovered public input.").unwrap();drop(s);
    assert_eq!(Store::open_persisted(&d,"session").unwrap().turns().len(),1);
}
#[test]
fn torn_initial_record_recovers_but_complete_corruption_is_preserved() {
    let d=dir();let p=d.join("conversation-v2.jsonl");
    use std::os::unix::fs::{OpenOptionsExt,PermissionsExt};
    let mut f=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p).unwrap();
    f.write_all(b"{\"schema\":1,\"sequence\":").unwrap();drop(f);
    let FrontStore::Ready(mut s)=Store::for_front(&d,"session") else {panic!("torn initial append must recover")};
    s.commit("new","Recovered public input.").unwrap();drop(s);
    assert_eq!(Store::open_persisted(&d,"session").unwrap().turns().len(),1);
    std::fs::write(&p,b"corrupt\n").unwrap();
    assert!(matches!(Store::for_front(&d,"session"),FrontStore::LegacyOnly{..}));
    assert_eq!(std::fs::read(&p).unwrap(),b"corrupt\n");
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode()&0o777,0o600);
}
#[test]
fn known_version_with_foreign_tip_is_a_conflict() {
    let mut s=Store::open(&dir().join("log"),"session").unwrap();let id=rendered(&mut s,"one");finish(&mut s,&id);
    let o=ack(&mut s);
    assert!(s.reconcile_mirror(Snapshot{session:"session".into(),version:o.version,tip:"a".repeat(64)}).is_err());
    assert!(!s.front_admission_allowed());
}
#[test]
fn unresolved_first_turn_blocks_a_later_rendered_turn() {
    let mut s=Store::open(&dir().join("log"),"session").unwrap();s.commit("first","Unresolved public input.").unwrap();
    let id=rendered(&mut s,"second");finish(&mut s,&id);assert!(s.prepare_observation().unwrap().is_none());
}
#[test]
fn uncertain_legacy_send_requires_lookup_after_restart() {
    let p=dir().join("log");let mut s=Store::open(&p,"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    s.select_owner(&t.id,Owner::Legacy).unwrap();assert!(s.begin_legacy_send(&t.id).unwrap());drop(s);
    let mut s=Store::open(&p,"session").unwrap();assert!(s.reconcile_without_speech(&t.id).is_err());
}
#[test]
fn recovered_held_turn_blocks_admission_until_reconciled() {
    let p=dir().join("log");let mut s=Store::open(&p,"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    s.select_owner(&t.id,Owner::Legacy).unwrap();drop(s);let mut s=Store::open(&p,"session").unwrap();
    assert!(!s.front_admission_allowed());s.reconcile_without_speech(&t.id).unwrap();assert!(s.front_admission_allowed());
}
#[test]
fn presentation_before_first_render_is_refused() {
    let mut s=Store::open(&dir().join("log"),"session").unwrap();let t=s.commit("one","Public input.").unwrap();
    s.select_owner(&t.id,Owner::Front).unwrap();assert!(s.finish_rendered(&t.id,vec![],Terminal::Completed).is_err());
}
#[test]
fn filtered_memory_is_not_synced() {
    let mut s=Store::open(&dir().join("log"),"session").unwrap();rendered(&mut s,"one");let o=ack(&mut s);
    s.accept_memory_receipt(MemoryReceipt{receipt:o.receipt(),items:vec![],outcome:MemoryOutcome::Filtered}).unwrap();
    assert_eq!(s.memory_outcome(&o.event_id),Some(MemoryOutcome::Filtered));assert!(!s.memory_synced(&o.event_id));
}
#[test]
fn unknown_presentation_cannot_contain_words() {
    assert!(Payload::Assistant{spans:vec![Span{first:0,end:1,text:"Invented words.".into()}],terminal:Terminal::PresentationUnknown}.validate().is_err());
}
#[test]
fn capacity_counts_unreceipted_presentations_after_user_ack() {
    let mut s=Store::open(&dir().join("log"),"session").unwrap();
    for n in 0..64 {let id=rendered(&mut s,&format!("turn-{n}"));finish(&mut s,&id);s.ack_observation(&id).unwrap();}
    assert!(s.commit("overflow","Must remain pending.").is_err());
    ack(&mut s);ack(&mut s);s.commit("after-receipt","One slot is free.").unwrap();
}
fn hash<T:serde::Serialize>(v:&T)->String {format!("{:x}",Sha256::digest(serde_json::to_vec(v).unwrap()))}

#[derive(serde::Serialize)]
#[serde(tag="kind")]
enum FixtureEvent {
    Snapshot {projection:FixtureProjection},
    Session { session:String }, Commit {nonce:String,input:String},
    Select {turn_id:String,owner:Owner}, FirstRendered {turn_id:String},
    Presentation {turn_id:String,payload:Payload}, MirrorPrepared {item:Observation},
    MirrorAccepted {receipt:conversation_log::observe::Receipt},
}
#[derive(serde::Serialize)]
struct FixtureProjection {
    session:String, turns:Vec<conversation_log::Turn>, observations:Vec<Observation>,
    receipts:Vec<conversation_log::observe::Receipt>, memory:Vec<MemoryReceipt>,
    mirror_blocked:bool, mirror_replay:Option<usize>,
}
fn snapshot_from_log(p:&std::path::Path, target:&std::path::Path) {
    use serde_json::Value;
    let mut projection=FixtureProjection{session:"session".into(),turns:vec![],observations:vec![],receipts:vec![],memory:vec![],mirror_blocked:false,mirror_replay:None};
    for line in std::fs::read_to_string(p).unwrap().lines() {
        let r:Value=serde_json::from_str(line).unwrap();let e=&r["event"];
        match e["kind"].as_str().unwrap() {
            "Commit" => {let nonce=e["nonce"].as_str().unwrap();let id=hash(&("session",nonce));
                projection.turns.push(conversation_log::Turn{id:format!("turn-{id}"),response_id:format!("reply-{id}"),observation_id:format!("observe-{id}"),nonce:nonce.into(),
        input:e["input"].as_str().unwrap().into(),owner:Some(Owner::Front),rendered:true,presentation_uncertain:false,legacy_attempted:false,backend_response:None,acknowledged:true,
        presentation:None,silent_replay:false});},
            "Presentation" => projection.turns.last_mut().unwrap().presentation=Some(serde_json::from_value(e["payload"].clone()).unwrap()),
            "MirrorPrepared" => projection.observations.push(serde_json::from_value(e["item"].clone()).unwrap()),
            "MirrorAccepted" => projection.receipts.push(serde_json::from_value(e["receipt"].clone()).unwrap()),
            _=>{},
        }
    }
    let event=FixtureEvent::Snapshot{projection};let digest=hash(&(1u32,1u64,"",&event));
    let row=serde_json::json!({"schema":1,"sequence":1,"previous":"","event":event,"digest":digest});
    use std::os::unix::fs::OpenOptionsExt;
    let mut f=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(target).unwrap();
    serde_json::to_writer(&mut f,&row).unwrap();f.write_all(b"\n").unwrap();f.sync_all().unwrap();
}
#[test]
fn pre_audio_commit_is_bounded_at_2000_turns_and_63_pending_presentations() {
    let d=dir();let log=d.join("events");let p=d.join("snapshot");scale_log(&log,1937);snapshot_from_log(&log,&p);
    let mut s=Store::open(&p,"session").unwrap();
    for n in 0..63 {let id=rendered(&mut s,&format!("pending-{n}"));finish(&mut s,&id);s.ack_observation(&id).unwrap();}
    let start=Instant::now();s.commit("next","Public bounded commit.").unwrap();let us=start.elapsed().as_micros();
    println!("commit_2000_with_63_pending_us={us}");assert!(us<30_000,"pre-audio commit exceeded 30ms: {us}us");
    assert!(s.commit("overflow","Capacity must hold.").is_err());
}
fn scale_log(p:&std::path::Path,n:usize) {
    use serde_json::json;
    use std::os::unix::fs::OpenOptionsExt;
    let mut out=std::io::BufWriter::new(std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(p).unwrap());
    let (mut seq,mut prev)=(0u64,String::new());
    let mut write=|event:FixtureEvent| {
        seq+=1;let digest=hash(&(1u32,seq,&prev,&event));
        let r=json!({"schema":1,"sequence":seq,"previous":prev,"event":event,"digest":digest});
        serde_json::to_writer(&mut out,&r).unwrap();out.write_all(b"\n").unwrap();prev=digest;
    };
    write(FixtureEvent::Session{session:"session".into()});
    let mut parent=String::new();
    for n in 0..n {
        let nonce=format!("scale-{n}");let identity=hash(&("session",&nonce));let id=format!("turn-{identity}");let response=format!("reply-{identity}");let input="Public input. ".repeat(15);
        write(FixtureEvent::Commit{nonce,input:input.clone()});
        write(FixtureEvent::Select{turn_id:id.clone(),owner:Owner::Front});
        write(FixtureEvent::FirstRendered{turn_id:id.clone()});
        let terminal=Payload::Assistant{spans:vec![Span{first:0,end:1600,text:"Rendered words. ".repeat(25)}],terminal:Terminal::Completed};
        write(FixtureEvent::Presentation{turn_id:id.clone(),payload:terminal.clone()});
        for (offset,event_id,payload) in [(1,format!("observe-{identity}"),Payload::User{input}),(2,format!("presentation-{response}"),terminal)] {
            let o=Observation{session:"session".into(),event_id,turn_id:id.clone(),response_id:response.clone(),version:(2*n+offset) as u64,parent:parent.clone(),payload};
            let receipt=o.receipt();parent=receipt.digest.clone();
            write(FixtureEvent::MirrorPrepared{item:o});write(FixtureEvent::MirrorAccepted{receipt});
        }
    }
    out.flush().unwrap();out.get_ref().sync_all().unwrap();
}
#[test]
fn scale_reopen_child() {
    let Some(p)=std::env::var_os("H5_SCALE_LOG") else {return};
    let start=Instant::now();let mut s=Store::open(std::path::Path::new(&p),"session").unwrap();
    println!("reopen_2000_ms={}",start.elapsed().as_millis());assert_eq!(s.turns().len(),2000);assert!(s.prepare_observation().unwrap().is_none());
    let start=Instant::now();s.commit("next","A new public input.").unwrap();let us=start.elapsed().as_micros();println!("commit_2000_us={us}");
    assert!(us<30_000,"pre-audio commit exceeded 30ms fsync-inclusive bound: {us}us");
    s.compact().unwrap();drop(s);
    let mut s=Store::open(std::path::Path::new(&p),"session").unwrap();assert_eq!(s.turns().len(),2001);

    assert!(s.commit("scale-0","different").is_err());
    for n in 0..63 {s.commit(&format!("pending-{n}"),"Pending public input.").unwrap();}
    assert!(s.commit("overflow","Must be refused.").is_err());
}
#[test]
fn two_thousand_turn_reopen_and_pre_audio_commit_are_bounded() {
    let d=dir();let p=d.join("scale-log");scale_log(&p,2000);
    let out=std::fs::File::create(d.join("child-output")).unwrap();
    let mut child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","scale_reopen_child","--nocapture"])
        .env("H5_SCALE_LOG",&p).stdout(out).stderr(std::process::Stdio::inherit()).spawn().unwrap();
    let start=Instant::now();loop {
        if let Some(status)=child.try_wait().unwrap() {let log=std::fs::read_to_string(d.join("child-output")).unwrap();println!("{log}");assert!(status.success());break;}
        if start.elapsed()>Duration::from_secs(8) {child.kill().unwrap();child.wait().unwrap();panic!("2000-turn replay exceeded 8s bound (child terminated)");}
        std::thread::sleep(Duration::from_millis(10));
    }
}
