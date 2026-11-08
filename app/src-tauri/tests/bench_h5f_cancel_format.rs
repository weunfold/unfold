
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"] mod conversation_log;
use conversation_log::{FrontStore, Store, Owner, observe::{Payload, Terminal}};
use serde_json::{json,Value};use sha2::{Digest,Sha256};use std::{fs,io::Write};
#[test]
fn additive_no_render_cancel_record_reopens_without_inventing_sound_or_rewriting_bytes() {
    let dir=std::path::PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(||std::env::temp_dir().into_os_string())).join(format!("h5f-format-{}",std::process::id()));
    fs::create_dir_all(&dir).unwrap();let p=dir.join("conversation-v2.jsonl");
    let mut s=Store::open(&p,"session").unwrap();let t=s.commit("one","A public canceled input.").unwrap();s.select_owner(&t.id,Owner::Front).unwrap();drop(s);
    let bytes=fs::read(&p).unwrap();let last:Value=serde_json::from_slice(bytes.split(|b|*b==b'\n').rfind(|r|!r.is_empty()).unwrap()).unwrap();
    let sequence=last["sequence"].as_u64().unwrap()+1;let event=json!({"kind":"NoRenderCancel","turn_id":t.id});let prev=last["digest"].as_str().unwrap();
    let digest=format!("{:x}",Sha256::digest(serde_json::to_vec(&(1u32,sequence,prev,&event)).unwrap()));
    let row=json!({"schema":1,"sequence":sequence,"previous":prev,"event":event,"digest":digest});
    let mut f=fs::OpenOptions::new().append(true).open(&p).unwrap();writeln!(f,"{row}").unwrap();f.sync_all().unwrap();drop(f);let before=fs::read(&p).unwrap();
    let FrontStore::Ready(mut s)=Store::for_front(&dir,"session") else {panic!("the additive no-render-cancel variant must recover")};
    assert!(!s.turns()[0].rendered);assert!(!s.turns()[0].presentation_uncertain);assert!(s.turns()[0].silent_replay);
    assert!(matches!(&s.turns()[0].presentation,Some(Payload::Assistant{spans,terminal:Terminal::Interrupted})if spans.is_empty()));
    let user=s.prepare_observation().unwrap().unwrap();assert_eq!(user.turn_id,t.id);assert_eq!(fs::read(&p).unwrap()[..before.len()],before);
}
