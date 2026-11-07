
use crate::conversation_state::{Conversation, Phase};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

static STATE: Mutex<Conversation> = Mutex::new(Conversation {
    phase: Phase::Off, epoch: 0, turn: 0, busy: false, submitted: false, model_done: false, speech_pending: false, barged: false, pending: None,
});

static INTERRUPTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static OVERLAP_DEGRADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static INTERRUPTION: Mutex<Option<String>> = Mutex::new(None);
pub fn rendered_interrupt(e: &Value) {
    if INTERRUPTING.load(std::sync::atomic::Ordering::SeqCst) && e["stopped"] == true {

        crate::floor_decide::playback(e, false);
        crate::floor_decide::context().lock().unwrap().echo_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
        if let Some(text) = e["rendered_text"].as_str() {
            *INTERRUPTION.lock().unwrap() = Some(interruption_marker(text));
        }
    }
}
fn interruption_marker(rendered: &str) -> String {
    format!("[Unfold was interrupted. Complete speech segments rendered before interruption: {}. Any partial final segment is omitted; the rest of the generated reply was not rendered. This is a playout observation, not a correction of saved history.]", json!(rendered))
}
pub fn interruption_context(text: &str) -> (String, Option<String>) {
    let marker = INTERRUPTION.lock().unwrap().clone();
    (marker.as_ref().map_or_else(|| text.to_string(), |m| format!("{m}\n\n{text}")), marker)
}
pub fn interruption_sent(marker: Option<String>) {
    let mut pending = INTERRUPTION.lock().unwrap();
    if marker.is_some() && *pending == marker { *pending = None; }
}
static VAD: Mutex<bool> = Mutex::new(false);
static FLOOR_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);
pub fn take_floor_ms()->Option<u64> {let n=FLOOR_MS.swap(u64::MAX,std::sync::atomic::Ordering::SeqCst);(n!=u64::MAX).then_some(n)}
static ENDPOINT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);
pub fn take_endpoint_ms() -> Option<u64> {
    let n = ENDPOINT_MS.swap(u64::MAX, std::sync::atomic::Ordering::SeqCst);
    (n != u64::MAX).then_some(n)
}
fn new_input() -> crate::conversation_input::Input {
    let mut input=crate::conversation_input::Input::default();
    input.set_semantic(crate::settings::turn_mode().endpoint);input
}
pub fn input_turn()->u64{STATE.lock().unwrap().turn}
static INPUT: Mutex<Option<crate::conversation_input::Input>> = Mutex::new(None);

#[derive(Default)]
struct NearEnd {
    key: Option<(u64,u64)>,
    segment: u64,
    clock_ms: u64,
    voice_ms: u32,
    quiet_ms: u32,
    playing: bool,
    blocked: bool,
    final_non_echo: bool,
    holding: bool,
    pending: Option<(u64,u64,crate::bc_detector::Decision)>,
    reset: u64,
}
impl NearEnd {
    fn key(&mut self, epoch:u64, turn:u64) {
        if self.key != Some((epoch,turn)) {
            let reset=self.reset.wrapping_add(1);
            *self=Self{key:Some((epoch,turn)),reset,..Self::default()};
        }
    }
    fn retire(&mut self) {
        self.segment=self.segment.wrapping_add(1);self.pending=None;self.final_non_echo=false;
        self.voice_ms=0;self.quiet_ms=0;
    }
    fn reject(&mut self) {self.retire();self.holding=false;self.blocked=true;self.reset=self.reset.wrapping_add(1);}
    fn boundary(&mut self,playing:bool) {
        if playing!=self.playing {self.reject();self.playing=playing;}
    }
    fn level(&mut self,level:f32) {
        self.clock_ms=self.clock_ms.saturating_add(u64::from(crate::conversation_input::LEVEL_MS));
        if level.is_finite() && level>=crate::conversation_input::VOICE_LEVEL {
            self.voice_ms=self.voice_ms.saturating_add(crate::conversation_input::LEVEL_MS);self.quiet_ms=0;
        } else {
            self.quiet_ms=self.quiet_ms.saturating_add(crate::conversation_input::LEVEL_MS);
            if self.quiet_ms>=crate::conversation_input::GAP_MS {self.voice_ms=0;}
        }
    }
    fn corroborated(&self)->bool {
        (self.voice_ms>=crate::conversation_input::BARGE_MS && self.quiet_ms<crate::conversation_input::GAP_MS) || self.final_non_echo
    }
}
static NEAR_END: Mutex<NearEnd> = Mutex::new(NearEnd {key:None,segment:0,clock_ms:0,voice_ms:0,quiet_ms:0,playing:false,blocked:false,final_non_echo:false,holding:false,pending:None,reset:0});

pub fn capture_boundary(epoch:u64)->u64 {
    let s=STATE.lock().unwrap();let mut n=NEAR_END.lock().unwrap();
    n.key(epoch,s.turn);
    let playing=matches!(s.phase,Phase::Thinking|Phase::Speaking|Phase::Paused);
    n.boundary(playing);

    if !playing && OVERLAP_DEGRADED.load(std::sync::atomic::Ordering::SeqCst) {n.blocked=false;}
    n.reset
}

static OVERLAP: Mutex<Option<crate::conversation_overlap::Overlap>> = Mutex::new(None);
static LISTENER: Mutex<Option<crate::conversation_overlap::Listener>> = Mutex::new(None);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

#[cfg(feature = "menu_bar")]
pub fn install_global(app: &tauri::AppHandle) -> Result<(), String> {
    let _ = APP.set(app.clone());
    crate::conversation_hotkey::install(|| { if let Some(app) = APP.get() { control(app); } })
}

pub fn active() -> bool { STATE.lock().unwrap().phase != Phase::Off }
pub fn snapshot() -> (bool, u64, bool) {
    let s = STATE.lock().unwrap(); (s.phase != Phase::Off, s.epoch, s.accepts(s.epoch))
}
pub fn accepts(epoch: u64) -> bool { STATE.lock().unwrap().accepts(epoch) }
pub fn front_interrupted(epoch:crate::voice_front::runtime::Epoch,rendered:&str) {
    let s=STATE.lock().unwrap();
    if s.epoch==epoch.capture && s.turn==epoch.turn { *INTERRUPTION.lock().unwrap()=Some(interruption_marker(rendered)); }
}
pub fn front_epoch()->Option<crate::voice_front::runtime::Epoch> {
    let s=STATE.lock().unwrap();
    (s.phase!=Phase::Off && s.busy).then_some(crate::voice_front::runtime::Epoch{capture:s.epoch,turn:s.turn})
}
pub fn front_progress(epoch:crate::voice_front::runtime::Epoch,done:bool) {
    let ready={let mut s=STATE.lock().unwrap();if !s.accepts(epoch.capture) || s.turn!=epoch.turn {return;}
        if done {s.reply_done();s.speech_ended();s.take_ready().map(|t|(t,s.turn))} else {s.speech_started();None}};
    emit(None);if let Some((text,token))=ready {let _=submit(text,token);}
}
pub fn turn() -> Option<u64> {
    let s = STATE.lock().unwrap();
    (s.phase != Phase::Off && s.busy && !s.barged).then_some(s.turn)
}
pub fn speaks_full() -> bool {
    let s = STATE.lock().unwrap(); s.phase != Phase::Off && s.busy && !s.barged
}
fn emit(text: Option<&str>) {
    let state = STATE.lock().unwrap().phase.name();
    let mut e = json!({"state": state,"overlap_mode":if OVERLAP_DEGRADED.load(std::sync::atomic::Ordering::SeqCst) {"manual-stop-only"}else{"audio"}});
    if let Some(t) = text { e["text"] = json!(t); }
    #[cfg(feature = "menu_bar")]
    if let Some(a) = APP.get() {
        let app = a.clone();

        let _ = a.run_on_main_thread(move || {
            let phase = STATE.lock().unwrap().phase.name();
            crate::tray::conversation_state(&app, phase);
        });
    }
    if let Some(w) = APP.get().and_then(|a| a.get_webview_window("main")) {
        let _ = w.eval(format!("window.__rthConversation && window.__rthConversation({e})"));
    }
}
pub fn toggle(app: &tauri::AppHandle) -> Result<bool, String> {
    let _ = APP.set(app.clone());
    let phase = STATE.lock().unwrap().phase;
    match phase {
        Phase::Speaking => { barge(); Ok(false) }
        Phase::Off => {
            #[cfg(feature = "menu_bar")]
            if crate::chat::replying() { return Err(crate::settings::t("Wait for this reply to finish.", "Espere esta resposta terminar.").into()); }
            if !crate::settings::conversation_voice_ready() {
                crate::settings::reply_warm();
                return Err(crate::settings::t("Choose and enable a voice in Settings, then try Conversation again.", "Escolha e ative uma voz nos Ajustes, depois tente a Conversa de novo.").into());
            }
            crate::settings::reply_silence();
            crate::settings::conversation_output_reset();
            *INTERRUPTION.lock().unwrap() = None;
            *INPUT.lock().unwrap() = Some(new_input());
            *OVERLAP.lock().unwrap() = None;
            *LISTENER.lock().unwrap() = None;
            crate::settings::reply_warm();
            OVERLAP_DEGRADED.store(false,std::sync::atomic::Ordering::SeqCst);
            crate::doubletalk::detector_available(true);
            *VAD.lock().unwrap()=false;STATE.lock().unwrap().start(); emit(None); Ok(true)
        }
        _ => { stop(); Ok(false) }
    }
}

pub fn typed(text: &str) -> Result<bool, String> {
    if crate::conversation_input::has_words(text) {crate::front_native::before_typed();}
    let mut s = STATE.lock().unwrap();
    if s.phase == Phase::Off { return Ok(false); }
    if !s.typed(text) { return Err(crate::settings::t("Wait for this reply to finish.", "Espere esta resposta terminar.").into()); }
    s.submitted = true; drop(s); emit(None); Ok(true)
}
fn claim_manual(text: &str) -> Result<Option<u64>, String> {
    if text.chars().count()<=crate::setup::SAY_MAX && crate::conversation_input::has_words(text) {crate::front_native::before_typed();}
    let mut input = INPUT.lock().unwrap();
    let mut s = STATE.lock().unwrap();
    if s.phase == Phase::Off { return Ok(None); }
    if text.chars().count() > crate::setup::SAY_MAX || !crate::conversation_input::has_words(text) { return Err("not a turn".into()); }
    if !s.manual(text) { return Err(crate::settings::t("Wait for this reply to finish.", "Espere esta resposta terminar.").into()); }
    let turn = s.turn;
    *input = Some(new_input());
    drop(s); drop(input); emit(None); Ok(Some(turn))
}

pub fn send_current(text: &str) -> Result<(), String> {
    send_manual(text, false)
}

pub fn send_page(text: &str) -> Result<(), String> {
    send_manual(text, true)
}
fn send_manual(text: &str, composer: bool) -> Result<(), String> {
    let turn = claim_manual(text)?.ok_or("Conversation is off")?;
    submit_origin(text.trim().to_string(), turn, composer)
}

pub fn control(app: &tauri::AppHandle) {
    if let Err(e) = crate::dictation::conversation_control(app, true) {
        #[cfg(feature = "menu_bar")]
        crate::tray::conversation_error(app, &e);
    }
}

pub fn stop() {
    crate::front_native::fence();
    crate::doubletalk::reset();
    *VAD.lock().unwrap()=false;
    crate::floor_decide::hold(false);
    let cancel = { let s = STATE.lock().unwrap(); s.busy && s.submitted && !s.model_done };
    STATE.lock().unwrap().stop();
    *INPUT.lock().unwrap() = None;
    *OVERLAP.lock().unwrap() = None;
    *LISTENER.lock().unwrap() = None;
    crate::settings::reply_silence();
    crate::settings::conversation_output_reset();
    if cancel { let _ = crate::setup::conversation_cancel(); }
    crate::dictation::cancel_native();
    emit(None);
}
pub fn barge() {
    barge_for(None);
}
fn barge_for(expected:Option<(u64,u64)>)->bool {
    let cancel = {
        let mut s = STATE.lock().unwrap();
        if expected.is_some_and(|(epoch,turn)|!s.accepts(epoch) || s.turn!=turn || s.barged) {return false;}
        let cancel = s.busy && s.submitted && !s.model_done && !s.barged;
        let mut n=NEAR_END.lock().unwrap();n.key(s.epoch,s.turn);n.blocked=false;n.pending=None;n.holding=false;

        n.playing=false;
        s.barge(); cancel
    };
    crate::front_native::fence();
    crate::doubletalk::resume();
    crate::floor_decide::hold(false);

    INTERRUPTING.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::settings::reply_silence();
    INTERRUPTING.store(false, std::sync::atomic::Ordering::SeqCst);
    if cancel { let _ = crate::setup::conversation_cancel(); }
    emit(None);
    true
}
pub fn overlap_unavailable(epoch:u64)->bool {
    let mut s=STATE.lock().unwrap();
    if !s.accepts(epoch) {return false;}
    s.continue_floor();
    {let mut n=NEAR_END.lock().unwrap();n.key(s.epoch,s.turn);n.reject();}
    OVERLAP_DEGRADED.store(true,std::sync::atomic::Ordering::SeqCst);
    crate::doubletalk::detector_available(false);
    crate::floor_decide::hold(false);
    drop(s);emit(None);true
}
pub fn overlap_token(epoch:u64)->Option<(u64,u64)> {
    let s=STATE.lock().unwrap();s.accepts(epoch).then_some((s.epoch,s.turn))
}
fn reset_input_for(epoch:u64,turn:u64) {

    let mut input=INPUT.lock().unwrap();let s=STATE.lock().unwrap();
    if s.accepts(epoch) && s.turn==turn {*input=Some(new_input());}
}
pub fn overlap_decision(epoch:u64,turn:u64,d:&crate::bc_detector::Decision)->bool {
    use crate::bc_detector::Action;
    let mut s=STATE.lock().unwrap();
    if !s.accepts(epoch) || s.turn!=turn {return false;}
    {
        let mut n=NEAR_END.lock().unwrap();n.key(epoch,turn);
        if d.action==Action::Speech {
            if matches!(s.phase,Phase::Listening|Phase::Interrupted) {
                let fresh=n.blocked;n.blocked=false;drop(n);drop(s);

                if fresh {reset_input_for(epoch,turn);}
                return true;
            }
            return false;
        }
        if s.barged || !matches!(s.phase,Phase::Thinking|Phase::Speaking|Phase::Paused) {return false;}
        n.boundary(true);
        if matches!(d.action,Action::Hold|Action::Yield) {

            if n.pending.as_ref().is_none_or(|(_,_,old)| old.action!=Action::Yield || d.action==Action::Yield) {
                n.pending=Some((n.segment,n.clock_ms,d.clone()));
            }
            if !n.corroborated() {return false;}
            if d.action==Action::Hold {
                if crate::doubletalk::no_speech_releases()>=2 {return false;}
                n.holding=true;
            }
        } else {
            let admitted=n.holding;n.reject();
            if !admitted {

                drop(n);drop(s);reset_input_for(epoch,turn);return false;
            }
        }
    }
    let action=match d.action {Action::Hold=>"HOLD",Action::Yield=>"YIELD",Action::Resume|Action::Noise=>"RESUME",Action::Speech=>unreachable!()};
    let row=json!({"part":"floor.decide","event_type":"decision_made","source":"bc-detector",
        "candidate_schema":"bc-detector/v1","candidates":["HOLD","RESUME","YIELD"],
        "trigger_event":"post_aec_pcm","context_rev":turn,"latency_ns":0,
        "decision_id":format!("bc:{}:{epoch}:{turn}:{}:{action}",std::process::id(),d.end_sample),
        "episode_id":format!("floor:{epoch}:{turn}"),"epoch":epoch,"turn":turn,"selected":action,"action":action,
        "p_floor":d.floor,"p_bc":d.bc,"audio_start_sample":d.begin_sample,"audio_end_sample":d.end_sample,
        "audio_start_ms":d.begin_sample*1000/crate::bc_detector::RATE,"audio_end_ms":d.end_sample*1000/crate::bc_detector::RATE,
        "no_speech_fast_release":d.no_speech,"max_hold_noise":d.action==Action::Noise,"training_eligibility":"not_reviewed"});
    match d.action {
        Action::Hold=>{if crate::doubletalk::no_speech_releases()<2 {crate::doubletalk::hold_overlap();s.yield_floor();}},
        Action::Resume|Action::Noise=>{if d.no_speech || d.action==Action::Noise {crate::doubletalk::no_speech_release();}crate::doubletalk::resume();crate::floor_decide::hold(false);s.continue_floor();},
        Action::Yield=>{drop(s);if !barge_for(Some((epoch,turn))) {return false;}crate::floor_decide::journal("decision",row);return true;}
        Action::Speech=>unreachable!(),
    }
    drop(s);
    if matches!(d.action,Action::Resume|Action::Noise) {reset_input_for(epoch,turn);}
    crate::floor_decide::journal("decision",row);emit(None);true
}
pub fn speech_started() { STATE.lock().unwrap().speech_started(); emit(None); }
pub fn speech_ended(turn: u64) { STATE.lock().unwrap().speech_ended_for(turn); emit(None); }
pub fn reply_frame(f: &Value) {
    crate::floor_decide::reply(f);
    if f["wire"].is_string() || f["status"].is_string() { return; }
    if f["done"] == true || !f["error"].is_null() {
        let ready = {
            let mut s = STATE.lock().unwrap(); s.reply_done();
            s.take_ready().map(|text| (text, s.turn))
        };
        emit(None);
        if let Some((text, token)) = ready { let _ = submit(text, token); }
    }
}
pub fn unsent_epoch(epoch: u64) {
    let mut s = STATE.lock().unwrap();
    if s.turn != epoch || s.phase != Phase::Thinking { return; }
    s.reply_done(); s.speech_ended(); drop(s); emit(None);
}
pub fn unsent() {
    let mut s = STATE.lock().unwrap(); s.reply_done(); s.speech_ended(); drop(s); emit(None);
}
pub fn take_pending(epoch: u64) -> bool {
    let mut s = STATE.lock().unwrap();
    if !s.busy || s.submitted || s.turn != epoch || s.phase != Phase::Thinking { return false; }
    s.submitted = true; true
}
fn submit(text: String, token: u64) -> Result<(), String> {
    submit_origin(text, token, false)
}
fn submit_origin(text: String, token: u64, composer: bool) -> Result<(), String> {
    crate::floor_decide::begin_turn(&text);
    emit(Some(&text));
    let result = crate::setup::conversation_say(text, token, composer);
    if result.is_err() { unsent_epoch(token); }
    result
}

pub fn backchannel_current(epoch: u64, turn: u64) -> bool {
    let input = INPUT.lock().unwrap();
    let s = STATE.lock().unwrap();
    s.phase == Phase::Listening && s.epoch == epoch && s.turn == turn &&
        input.as_ref().is_some_and(|i| (300..=700).contains(&i.quiet_ms()))
}

pub fn listening_clip_started(form: &'static str) {
    if let Some(listener) = LISTENER.lock().unwrap().as_mut() { listener.clip_started(form); }
}
pub fn listening_clip_event(e: &Value) {
    crate::floor_decide::playback(e,true);
    if e["state"] == "done" || e["state"] == "failed" {
        if let Some(listener) = LISTENER.lock().unwrap().as_mut() { listener.clip_finished(); }
    }
}

fn filter_reply_echo(e: &mut crate::dictation::Event, playing: bool) {
    if let crate::dictation::Event::Text{text,settled,..}=e {
        let c=crate::floor_decide::context().lock().unwrap();
        if c.echo_until.is_some_and(|until|std::time::Instant::now()<until) || (playing && c.echo_until.is_none()) {
            if let Some(filtered)=crate::conversation_overlap::filter_echo(text,&c.sentence,&c.played) {
                *text=filtered;*settled=None;
                if text.is_empty() {
                    crate::doubletalk::resume();
                    *OVERLAP.lock().unwrap() = None;
                    crate::floor_decide::hold(false);
                    STATE.lock().unwrap().continue_floor();
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn review_fix_b3_listening_clip_echo(play: impl FnOnce()) {
    let _test = CONVERSATION_TEST.lock().unwrap();
    use crate::dictation::Event;
    *STATE.lock().unwrap() = Conversation::default();
    STATE.lock().unwrap().start();
    *INPUT.lock().unwrap() = Some(new_input());
    *OVERLAP.lock().unwrap() = None;
    *LISTENER.lock().unwrap() = Some(crate::conversation_overlap::Listener::default());
    play();
    let mut event = Event::Text { seq: 1, text: "and then I wanted to yeah".into(), settled: Some(6), is_final: true };
    let visible = dictation(&mut event);
    let text = INPUT.lock().unwrap().as_ref().unwrap().text();
    let phase = STATE.lock().unwrap().phase;

    *STATE.lock().unwrap() = Conversation::default();
    *INPUT.lock().unwrap() = None;
    *OVERLAP.lock().unwrap() = None;
    *LISTENER.lock().unwrap() = None;
    assert!(visible);
    assert_eq!(text, "and then I wanted to", "backchannel echo never enters the user's committed text");
    assert!(matches!(event, Event::Text { text, .. } if text == "and then I wanted to"), "page gets the same filtered revision");
    assert_eq!(phase, Phase::Listening);
}

fn dictation_rules(e: &mut crate::dictation::Event) -> bool {
    use crate::dictation::Event;
    use crate::conversation_overlap::Listener;
    if !active() { return true; }
    if matches!(e, Event::Denied | Event::NoModel | Event::Downloading(_) | Event::Error(_) | Event::Stopped | Event::Cancelled) { stop(); return true; }
    let (phase,epoch,turn)={let s=STATE.lock().unwrap();(s.phase,s.epoch,s.turn)};
    let playing = matches!(phase, Phase::Thinking | Phase::Speaking | Phase::Paused);

    filter_reply_echo(e,playing);
    {
        let mut listener = LISTENER.lock().unwrap();
        let listener = listener.get_or_insert_with(Listener::default);
        match e {
            Event::Level(level) => {

                if !listener.level(*level, phase == Phase::Listening) { return true; }
            }
            Event::Text { text, settled, .. } => {
                if let Some(filtered) = listener.filter_words(text) { *text = filtered; *settled = None; }
            }
            _ => {}
        }
    }
    let pending={
        let mut n=NEAR_END.lock().unwrap();n.key(epoch,turn);n.boundary(playing);
        match e {
            Event::Level(level)=>{
                n.level(*level);
                if playing && n.corroborated() {
                    n.pending.as_ref().filter(|(segment,at,_)| *segment==n.segment && n.clock_ms.saturating_sub(*at)<=3000).map(|(_,_,d)|d.clone())
                } else {None}
            },
            Event::Text{text,is_final:true,..}=>{
                let non_echo={let c=crate::floor_decide::context().lock().unwrap();
                    crate::conversation_overlap::filter_echo(text,&c.sentence,&c.played).unwrap_or_else(||text.clone())};
                if playing && crate::conversation_input::has_words(&non_echo) {

                    let candidate=n.pending.as_ref().filter(|(segment,at,d)| *segment==n.segment && n.clock_ms.saturating_sub(*at)<=3000 && d.action==crate::bc_detector::Action::Yield).map(|(_,_,d)|d.clone());
                    if candidate.is_some() {*text=non_echo;n.final_non_echo=true;}
                    candidate
                } else {None}
            },
            _=>None,
        }
    };
    if let Some(d)=pending {overlap_decision(epoch,turn,&d);}

    let phase=STATE.lock().unwrap().phase;
    let playing=matches!(phase,Phase::Thinking|Phase::Speaking|Phase::Paused);
    if matches!(e,Event::Text{is_final:true,..}) {NEAR_END.lock().unwrap().retire();}
    if matches!(e,Event::Text{..}) && NEAR_END.lock().unwrap().blocked {return false;}

    if playing {return !matches!(e,Event::Text{..});}
    crate::floor_decide::hold(false);
    let ready = {
        let mut guard = INPUT.lock().unwrap();
        let Some(input) = guard.as_mut() else { return true };
        input.set_semantic(crate::settings::turn_mode().endpoint);
        let quiet=input.quiet_ms()+if matches!(e,Event::Level(_)) {crate::conversation_input::LEVEL_MS}else{0};
        let ready=match e {
            Event::Level(level)=>{input.level(*level);input.ready(false)},
            Event::Text{text,is_final,..}=>input.words(text,*is_final,false),_=>None
        };
        if ready.is_some() { ENDPOINT_MS.store(u64::from(quiet), std::sync::atomic::Ordering::SeqCst); }
        if ready.is_none() && phase == Phase::Listening {
            let mut listener = LISTENER.lock().unwrap();
            let listener = listener.get_or_insert_with(Listener::default);
            if matches!(e,Event::Level(_)) && listener.pause_ms()==300 {
                crate::settings::conversation_log(format!("listening_gate: {:?}",listener.opportunity(&input.text(),crate::settings::listening_level())));
            }
            if let Some(clip) = listener.due(&input.text(), crate::settings::listening_level()) {
                let s = STATE.lock().unwrap();
                crate::settings::listening_sound(clip, listener.pause_ms(), s.epoch, s.turn);
                listener.played();
            }
        }
        ready
    };
    if let Some(text) = ready {
        if let Some(l) = LISTENER.lock().unwrap().as_mut() { l.reset_turn(); }
        let mut s = STATE.lock().unwrap();
        if let Some(text) = s.endpoint(&text) {
            let token = s.turn; drop(s); let _ = submit(text, token);
        }
    }
    true
}

#[cfg(test)]
static CONVERSATION_TEST: Mutex<()> = Mutex::new(());

pub fn dictation(e: &mut crate::dictation::Event)->bool {
    if let crate::dictation::Event::Text{seq,text,is_final,..}=e {crate::doubletalk::asr(*seq,text,*is_final);}
    let active=active();
    let guarded=LISTENER.lock().unwrap().as_ref().is_some_and(|l|l.echo_active());
    let visible=dictation_rules(e);
    if active {
        use crate::dictation::Event;
        use crate::floor_decide::journal;
        match e {
            Event::Level(_) if guarded=>journal("echo_gate",json!({"action":"hold_clock","source":"listening_clip"})),
            Event::Level(level)=>{
                let voiced=level.is_finite() && *level>=crate::conversation_input::VOICE_LEVEL;
                let mut vad=VAD.lock().unwrap();
                if *vad!=voiced{*vad=voiced;journal(if voiced {"local_vad_onset"}else{"input_audio_end"},json!({"level":f64::from(*level)*50.0-60.0}));}
            }
            Event::Text{text,is_final:true,..} if visible=>journal("input_transcript",json!({"text":text,"final":true})),
            Event::Text{..} if !visible=>journal("echo_gate",json!({"action":"suppress","source":"overlap"})),
            _=>{}
        }
    }
    visible
}

#[cfg(test)]
mod review_b4 {
    use super::*;
    use crate::dictation::Event;
    #[test]
    fn review_b4_continuous_speech_through_clip_and_tail_never_commits() {

        if std::env::var_os("RTH_OVERLAP_B4_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "conversation::review_b4::review_b4_continuous_speech_through_clip_and_tail_never_commits", "--test-threads", "4"])
                .env("RTH_OVERLAP_B4_CHILD", "1").output().unwrap();
            assert!(child.status.success(), "isolated B4 regression failed:\n{}\n{}",
                String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
            return;
        }
        let _test = CONVERSATION_TEST.lock().unwrap();
        *STATE.lock().unwrap() = Conversation::default();
        STATE.lock().unwrap().start();
        *INPUT.lock().unwrap() = Some(new_input());
        *OVERLAP.lock().unwrap() = None;
        *LISTENER.lock().unwrap() = Some(crate::conversation_overlap::Listener::default());
        for _ in 0..6 { dictation(&mut Event::Level(0.9)); }
        dictation(&mut Event::Text { seq: 1, text: "my name is".into(), settled: None, is_final: true });
        for _ in 0..8 { dictation(&mut Event::Level(0.0)); }
        let before = INPUT.lock().unwrap().as_ref().unwrap().quiet_ms();
        listening_clip_started("mm-hm");
        let mut commits = Vec::new();
        let mut quiet = Vec::new();
        let mut forwarded = Vec::new();

        for frame in 0..16 {
            if frame == 10 { listening_clip_event(&json!({"state":"done"})); }
            let mut level = Event::Level(0.9);
            dictation(&mut level);
            quiet.push(INPUT.lock().unwrap().as_ref().unwrap().quiet_ms());
            forwarded.push(level);
            commits.push(STATE.lock().unwrap().turn);
        }

        dictation(&mut Event::Level(0.9));
        let resumed_quiet = INPUT.lock().unwrap().as_ref().unwrap().quiet_ms();
        let resumed_turn = STATE.lock().unwrap().turn;
        let pending_text = INPUT.lock().unwrap().as_ref().unwrap().text();
        *STATE.lock().unwrap() = Conversation::default();
        *INPUT.lock().unwrap() = None;
        *OVERLAP.lock().unwrap() = None;
        *LISTENER.lock().unwrap() = None;
        assert_eq!(before, 400);
        assert!(commits.iter().all(|turn| *turn == 0), "no endpoint or commit during clip/tail: {commits:?}");
        assert!(quiet.iter().all(|ms| *ms == 400), "guarded audio is neither voiced nor quiet: {quiet:?}");
        assert!(forwarded.iter().all(|e| *e == Event::Level(0.9)), "the person's meter is never rewritten to zero");
        assert_eq!((resumed_quiet, resumed_turn), (0, 0), "normal voiced accounting resumes without a premature commit");
        assert_eq!(pending_text, "my name is");
    }
}

#[cfg(test)]
mod doubletalk_regressions {
    use super::*;
    use crate::dictation::Event;
    #[test]
    fn doubletalk_reply_echo_does_not_barge_even_with_voiced_spikes() {
        if std::env::var_os("RTH_DOUBLETALK_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","conversation::doubletalk_regressions::doubletalk_reply_echo_does_not_barge_even_with_voiced_spikes","--test-threads","4"])
                .env("RTH_DOUBLETALK_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}",String::from_utf8_lossy(&child.stdout));return;
        }
        *STATE.lock().unwrap()=Conversation::default();
        { let mut s=STATE.lock().unwrap();s.start();s.endpoint("original task");s.speech_started(); }
        *INPUT.lock().unwrap()=Some(new_input());
        crate::floor_decide::context().lock().unwrap().sentence="Here are the three main differences".into();
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(!dictation_rules(&mut Event::Text{seq:1,text:"Here are the three main differences".into(),settled:None,is_final:false}));
        assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"reply echo retains the floor");
    }
}

#[cfg(test)]
mod doubletalk_boundary_tests {
    use super::*;
    use crate::dictation::Event;
    fn isolated(name: &str)->bool {
        if std::env::var_os("RTH_DOUBLETALK_BOUNDARY_CHILD").is_some(){return false;}
        let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",name,"--test-threads","4"])
            .env("RTH_DOUBLETALK_BOUNDARY_CHILD","1").output().unwrap();
        assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));true
    }
    fn speaking() {
        *STATE.lock().unwrap()=Conversation::default();
        {let mut s=STATE.lock().unwrap();s.start();s.endpoint("original task");s.speech_started();}
        *INPUT.lock().unwrap()=Some(new_input());*OVERLAP.lock().unwrap()=None;*LISTENER.lock().unwrap()=None;
        *crate::floor_decide::context().lock().unwrap()=Default::default();
    }
    #[test]
    fn attenuated_content_waits_for_model_and_final_corroboration() {
        if isolated("conversation::doubletalk_boundary_tests::attenuated_content_waits_for_model_and_final_corroboration"){return;}
        for _ in 0..1 {
            speaking();

            STATE.lock().unwrap().submitted=true;
            crate::doubletalk::reply(true);
            crate::floor_decide::context().lock().unwrap().sentence="Here are the three important differences".into();
            let send=|e:&mut Event|dictation_rules(e);

            for db in [-52.455925_f32,-37.111904,-53.869644] {send(&mut Event::Level((db+60.0)/50.0));}
            let text=|s:&str|Event::Text{seq:1,text:s.into(),settled:None,is_final:false};
            assert!(!send(&mut text("Here are the three importent differences")));
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
            assert!(!send(&mut text("what changed")));
            let s=STATE.lock().unwrap();let (epoch,turn)=(s.epoch,s.turn);drop(s);
            assert!(!overlap_decision(epoch,turn,&crate::bc_detector::Decision{action:crate::bc_detector::Action::Yield,begin_sample:0,end_sample:1280,floor:0.9,bc:0.05,no_speech:false}),"model plus attenuated partial is not independent corroboration");
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
            assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());

            let mut final_words=Event::Text{seq:2,text:"what changed".into(),settled:None,is_final:true};
            assert!(send(&mut final_words));
            assert_eq!(STATE.lock().unwrap().phase,Phase::Interrupted,"owner ACCEPT trade-off: model plus non-echo final, not the earlier attenuated partial");
            assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"what changed");
        }
    }
    #[test]
    fn doubletalk_reply_echo_expires_even_if_speaking_state_is_stale() {
        if isolated("conversation::doubletalk_boundary_tests::doubletalk_reply_echo_expires_even_if_speaking_state_is_stale"){return;}
        speaking();
        {let mut c=crate::floor_decide::context().lock().unwrap();
         c.sentence="Here are the three important differences".into();
         c.echo_until=Some(std::time::Instant::now()-std::time::Duration::from_secs(1));}
        let mut e=Event::Text{seq:1,text:"Here are the three important differences".into(),settled:None,is_final:false};
        filter_reply_echo(&mut e,true);
        assert!(matches!(e,Event::Text{text,..} if !text.is_empty()),"expired echo source preserves words");
        assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"words cannot choose the floor");
    }

}

#[cfg(test)]
mod h1_regressions {
    use super::*;
    #[test]
    fn h3_audio_actions_preserve_work_and_fence_epochs_and_cancel() {
        use crate::{dictation::Event,bc_detector::{Action,Decision}};
        if std::env::var_os("RTH_H3_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","conversation::h1_regressions::h3_audio_actions_preserve_work_and_fence_epochs_and_cancel","--test-threads","4"])
                .env("RTH_H3_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        *STATE.lock().unwrap()=Conversation::default();
        let (epoch,turn)={let mut s=STATE.lock().unwrap();s.start();s.endpoint("original");s.speech_started();s.submitted=true;(s.epoch,s.turn)};
        *INPUT.lock().unwrap()=Some(new_input());
        let decision=|action|Decision{action,begin_sample:0,end_sample:640,floor:0.8,bc:0.1,no_speech:false};
        crate::doubletalk::reply(true);
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
        {let s=STATE.lock().unwrap();assert_eq!(s.phase,Phase::Paused);assert!(s.busy && s.submitted && !s.barged && !s.model_done);}
        assert!(crate::doubletalk::playout_held());
        assert!(!dictation_rules(&mut Event::Text{seq:1,text:"yeah yeah".into(),settled:None,is_final:false}));
        assert_eq!(STATE.lock().unwrap().phase,Phase::Paused,"two words never yield");
        assert!(!overlap_decision(epoch+1,turn,&decision(Action::Yield)));
        assert!(!overlap_decision(epoch,turn+1,&decision(Action::Resume)));
        assert!(overlap_decision(epoch,turn,&decision(Action::Resume)));
        assert!(!crate::doubletalk::playout_held());
        {let s=STATE.lock().unwrap();assert_eq!(s.phase,Phase::Speaking);assert!(s.busy && s.submitted && !s.barged);}
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
        assert!(overlap_unavailable(epoch));
        assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"failure cannot strand Paused state");
        assert!(!crate::doubletalk::playout_held());
        assert!(OVERLAP_DEGRADED.load(std::sync::atomic::Ordering::SeqCst));
        {let s=STATE.lock().unwrap();assert!(s.busy && s.submitted && !s.barged);}
        assert!(!overlap_unavailable(epoch+1),"stale failure cannot degrade a new epoch");
        crate::doubletalk::detector_available(true);
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(overlap_decision(epoch,turn,&decision(Action::Yield)));
        assert_eq!(STATE.lock().unwrap().phase,Phase::Interrupted);
        assert!(!overlap_decision(epoch,turn,&decision(Action::Resume)),"late model output cannot undo cancellation");
        STATE.lock().unwrap().stop();
        assert!(!overlap_decision(epoch,turn,&decision(Action::Hold)),"old capture epoch rejected");
        crate::doubletalk::reset();
    }
    #[test]
    fn h1_interruption_marker_quotes_playout_and_preserves_user_text() {
        let _guard = super::CONVERSATION_TEST.lock().unwrap();
        let marker = interruption_marker("The first sentence.");
        *INTERRUPTION.lock().unwrap() = None;
        rendered_interrupt(&json!({"stopped":true,"rendered_text":"late old event"}));
        assert!(INTERRUPTION.lock().unwrap().is_none());
        INTERRUPTING.store(true, std::sync::atomic::Ordering::SeqCst);
        rendered_interrupt(&json!({"stopped":true,"rendered_text":"The first sentence."}));
        INTERRUPTING.store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(*INTERRUPTION.lock().unwrap(), Some(marker));
        let (wire, token) = interruption_context("What changed?");
        assert!(wire.contains("Complete speech segments rendered"));
        assert!(wire.contains("\"The first sentence.\""));
        assert!(wire.ends_with("\n\nWhat changed?"));
        assert_eq!(interruption_context("retry").1, token, "failed sends retain the observation");
        interruption_sent(token);
        assert_eq!(interruption_context("another"), ("another".into(), None));
        assert!(interruption_marker("").contains("\"\""));
    }
}

fn model_decision_row(state:&Conversation,d:&crate::turn_detect::Decision,epoch:u64,action:&str,shadow:bool)->Value {
    json!({
        "part":"turn.detect","event_type":"decision_made","source":"smart-turn","shadow":shadow,"would_commit":d.commit,
        "candidate_schema":"turn.detect/v1","candidates":["WAIT","TAKE_TURN"],
        "trigger_event":"post_aec_silero_silence","context_rev":state.turn,"latency_ns":d.latency_ns,
        "decision_id":format!("smart-turn:{}:{}:{}:{}",std::process::id(),epoch,state.turn,d.audio_end_ms),
        "episode_id":format!("floor:{}:{}",epoch,state.turn),"selected":action,"action":action,
        "epoch":epoch,"turn":state.turn,"probability":d.probability,
        "S_ms":d.knobs.silence_ms,"T":d.knobs.threshold,"max_hold_ms":d.knobs.max_hold_ms,
        "effective_threshold":d.effective_threshold,"threshold_floor":d.knobs.threshold_floor,"retry_ms":d.knobs.retry_ms,
        "max_hold":d.max_hold,"quiet_ms":d.quiet_ms,"audio_start_ms":d.audio_start_ms,"audio_end_ms":d.audio_end_ms,
        "training_eligibility":"not_reviewed"
    })
}

pub fn shadow_decision(epoch:u64,input_turn:u64,d:&crate::turn_detect::Decision,stats:crate::turn_shadow::Stats)->bool {
    let mode=crate::settings::turn_mode();
    let state=STATE.lock().unwrap();
    if !state.accepts(epoch) || state.turn!=input_turn || mode.endpoint || !mode.shadow {return false;}
    let mut row=model_decision_row(&state,d,epoch,"WAIT",true);
    row["input_turn"]=json!(input_turn);
    row["dropped_requests"]=json!(stats.dropped_requests);
    row["worker_lag_ns"]=json!(stats.worker_lag_ns);
    row["audio_gap_samples"]=json!(stats.audio_gap_samples);
    drop(state);
    crate::floor_decide::journal("decision",row);true
}

pub fn model_decision(epoch:u64, d:&crate::turn_detect::Decision)->bool {
    model_decision_in_mode(epoch,input_turn(),d,crate::settings::turn_mode())
}

pub fn model_decision_in_mode(epoch:u64,input_turn:u64,d:&crate::turn_detect::Decision,began:crate::turn_detect::Mode)->bool {
    let mode=crate::settings::turn_mode();
    let mut input=INPUT.lock().unwrap();
    let mut state=STATE.lock().unwrap();
    if !state.accepts(epoch) || !mode.runs() {return false;}
    let shadow=!(began.endpoint && mode.endpoint);
    let eligible=!shadow && matches!(state.phase,Phase::Listening|Phase::Interrupted);
    if let Some(input)=input.as_mut(){input.set_semantic(mode.endpoint);}
    let text=if eligible && d.commit {input.as_mut().and_then(|i|i.model_ready(true))}else{None};
    let committed=text.is_some();
    let action=if committed {"TAKE_TURN"} else {"WAIT"};
    crate::settings::conversation_log(format!("decision_made source=smart-turn shadow={shadow} action={action} probability={}",d.probability));
    let mut row=model_decision_row(&state,d,epoch,action,shadow);
    row["input_turn"]=json!(input_turn);
    crate::floor_decide::journal("decision",row);
    let submit_text=text.and_then(|text|state.endpoint(&text));
    let turn=state.turn;
    drop(state);drop(input);
    if committed {
        ENDPOINT_MS.store(d.quiet_ms,std::sync::atomic::Ordering::SeqCst);
        if let Some(l)=LISTENER.lock().unwrap().as_mut(){l.reset_turn();}
    }
    if let Some(text)=submit_text {let _=submit(text,turn);}
    committed
}

#[cfg(test)]
mod h2_regressions {
    use super::*;
    #[test]
    fn h2_model_decides_before_the_old_timer() {
        let mut input = crate::conversation_input::Input::semantic();
        input.words("a complete request", false, false);
        for _ in 0..6 { input.level(0.0); }
        assert_eq!(input.model_ready(true), Some("a complete request".into()));
    }
    #[test]
    fn h2_trailing_clause_pause_waits_for_the_audio_model() {
        let mut input = crate::conversation_input::Input::semantic();
        input.words("tell me about", true, false);
        for _ in 0..40 { input.level(0.0); }
        assert_eq!(input.ready(false), None, "a low model probability cannot become a timer commit");
        input.words("the project", false, false);
        assert_eq!(input.model_ready(true), Some("tell me about the project".into()));
    }
    #[test]
    fn h2_journals_model_probability_and_audio_window() {
        let source = include_str!("conversation.rs");
        for contract in ["\"source\":\"smart-turn\"", "\"probability\"", "\"audio_start_ms\"", "\"audio_end_ms\""] {

            assert!(source.split("mod h2_regressions").next().unwrap().contains(contract), "missing journal contract {contract}");
        }
    }
}

#[cfg(test)]
mod h2_admission_tests {
    use super::*;
    #[test]
    fn h2_model_result_is_epoch_fenced_and_waits_for_old_cancel() {
        if std::env::var_os("RTH_H2_ADMISSION_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","conversation::h2_admission_tests::h2_model_result_is_epoch_fenced_and_waits_for_old_cancel","--test-threads","4"])
                .env("RTH_H2_ADMISSION_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        crate::settings::sync_turn_mode(&crate::settings::Settings{smart_turn_endpoint:true,..Default::default()});
        let mut state=Conversation::default();state.start();let epoch=state.epoch;
        state.endpoint("old request");state.submitted=true;state.barge();
        *STATE.lock().unwrap()=state;
        let mut input=new_input();input.words("the new request",false,false);*INPUT.lock().unwrap()=Some(input);
        let k=crate::turn_detect::Knobs::default();let mut p=crate::turn_detect::Policy::default();
        p.frame(true,16000,k);p.frame(false,16000,k);
        let low=p.decide(0.1,k).unwrap();assert!(!model_decision(epoch,&low));
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"the new request");
        let high=p.decide(0.9,k).unwrap();assert!(!model_decision(epoch.wrapping_sub(1),&high));
        assert!(model_decision(epoch,&high));
        assert_eq!(STATE.lock().unwrap().pending.as_deref(),Some("the new request"));
        assert!(!model_decision(epoch,&high),"one model result cannot release the same text twice");
        let row=model_decision_row(&STATE.lock().unwrap(),&high,epoch,"TAKE_TURN",false);
        assert_eq!(row["event_type"],"decision_made");assert_eq!(row["source"],"smart-turn");
        assert_eq!(row["probability"],serde_json::json!(high.probability));
        assert_eq!(row["S_ms"],k.silence_ms);assert_eq!(row["audio_end_ms"],2000);
    }
}

#[cfg(test)]
mod review_h1_probe {
    use super::*;
    #[test]
    fn corroborated_hold_clears_when_reply_ends_while_held() {
        use crate::dictation::Event;
        if std::env::var_os("REVIEW_H1_CHILD").is_none() {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "conversation::review_h1_probe::corroborated_hold_clears_when_reply_ends_while_held", "--test-threads", "4"])
                .env("REVIEW_H1_CHILD", "1").output().unwrap();
            assert!(child.status.success(), "{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
            return;
        }
        *STATE.lock().unwrap() = Conversation::default();
        { let mut s = STATE.lock().unwrap(); s.start(); s.endpoint("original"); s.submitted=true; s.speech_started(); }
        *INPUT.lock().unwrap() = Some(new_input());
        let s=STATE.lock().unwrap();let (epoch,turn)=(s.epoch,s.turn);drop(s);
        crate::doubletalk::reset();crate::floor_decide::hold(false);
        crate::doubletalk::reply(true);
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(overlap_decision(epoch,turn,&crate::bc_detector::Decision{action:crate::bc_detector::Action::Hold,begin_sample:0,end_sample:640,floor:0.1,bc:0.1,no_speech:false}),"the lifetime invariant starts with an admitted corroborated HOLD");
        assert!(crate::doubletalk::held());
        assert!(crate::doubletalk::playout_held(), "the admitted HOLD fences active reply playout before terminal delivery");
        assert_eq!(STATE.lock().unwrap().phase, Phase::Paused);

        crate::doubletalk::reply(false);
        { let mut s = STATE.lock().unwrap(); let t = s.turn; s.speech_ended_for(t); s.reply_done(); }
        let phase = STATE.lock().unwrap().phase;
        assert!(!matches!(phase, Phase::Thinking | Phase::Speaking | Phase::Paused), "phase {phase:?}");
        for _ in 0..4 { dictation_rules(&mut Event::Level(0.0)); }
        assert!(!crate::doubletalk::held(), "terminal reply delivery releases the admitted native HOLD");

        assert!(!crate::floor_decide::held(), "floor hold outlived its reply; the next reply's playout starts held");

        crate::doubletalk::reply(true);
        assert!(!crate::doubletalk::playout_held(), "floor hold outlived its reply; the next reply's playout starts held");
    }
}

#[cfg(test)]
mod shadow_mode_regressions {
    use super::*;
    fn decision() -> crate::turn_detect::Decision {
        let k=crate::turn_detect::Knobs::default();
        let mut p=crate::turn_detect::Policy::default();
        p.frame(true,16000,k);p.frame(false,8192,k);p.decide(0.9,k).unwrap()
    }
    #[test]
    fn default_endpoint_commits_by_the_existing_timer() {
        let mut input=new_input();
        input.words("a complete request",true,false);
        for _ in 0..15 {input.level(0.0);assert_eq!(input.ready(false),None);}
        input.level(0.0);
        assert_eq!(input.ready(false),Some("a complete request".into()));
    }
    #[test]
    fn default_model_row_is_explicitly_shadow_and_keeps_probability_and_window() {
        let s=Conversation::default();let d=decision();
        let row=model_decision_row(&s,&d,1,"WAIT",!crate::settings::turn_mode().endpoint);
        assert_eq!(row["shadow"],true);
        assert_eq!(row["source"],"smart-turn");
        assert_eq!(row["event_type"],"decision_made");
        assert_eq!(row["probability"],json!(d.probability));
        assert_eq!(row["audio_start_ms"],d.audio_start_ms);
        assert_eq!(row["audio_end_ms"],d.audio_end_ms);
    }
    #[test]
    fn shadow_never_commits_or_consumes_pending_input() {
        if std::env::var_os("RTH_SHADOW_CHILD").is_none(){
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","conversation::shadow_mode_regressions::shadow_never_commits_or_consumes_pending_input","--test-threads","4"])
                .env("RTH_SHADOW_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}\n{}",String::from_utf8_lossy(&child.stdout),String::from_utf8_lossy(&child.stderr));return;
        }
        let mut s=Conversation::default();s.start();let epoch=s.epoch;
        s.endpoint("old request");s.submitted=true;s.barge();*STATE.lock().unwrap()=s;
        let mut i=new_input();i.words("the next request",true,false);*INPUT.lock().unwrap()=Some(i);
        use std::io::BufRead;
        let (writer,reader)=std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        crate::floor_decide::journal_install(std::fs::File::from(std::os::fd::OwnedFd::from(writer)));
        assert!(!model_decision(epoch,&decision()),"shadow cannot take a turn");
        let rows=std::io::BufReader::new(reader).lines().take(2)
            .map(|line|serde_json::from_str::<Value>(&line.unwrap()).unwrap()).collect::<Vec<_>>();
        let written=rows.iter().find(|r|r["kind"]=="decision").expect("shadow row reaches the real journal writer");
        assert_eq!(written["shadow"],true);assert_eq!(written["source"],"smart-turn");
        assert_eq!(written["event_type"],"decision_made");assert_eq!(written["selected"],"WAIT");
        assert_eq!(written["probability"],json!(decision().probability));
        assert_eq!(written["audio_end_ms"],decision().audio_end_ms);
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"the next request");
        assert!(STATE.lock().unwrap().pending.is_none());
        let input_turn=input_turn();
        assert!(!shadow_decision(epoch+1,input_turn,&decision(),Default::default()));
        assert!(!shadow_decision(epoch,input_turn+1,&decision(),Default::default()));
        assert!(shadow_decision(epoch,input_turn,&decision(),Default::default()));
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"the next request");
        crate::settings::sync_turn_mode(&crate::settings::Settings{smart_turn_endpoint:true,..Default::default()});
        assert!(!model_decision_in_mode(epoch,0,&decision(),crate::turn_detect::Mode::default()));
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"the next request");
    }
}

#[cfg(test)]
pub(crate) fn review_busy_front(run: impl FnOnce(crate::voice_front::runtime::Epoch)) {
    let _serial=CONVERSATION_TEST.lock().unwrap_or_else(|e|e.into_inner());
    struct Restore(Option<Conversation>);
    impl Drop for Restore {fn drop(&mut self){if let Some(saved)=self.0.take(){*STATE.lock().unwrap_or_else(|e|e.into_inner())=saved;}}}
    let mut s=STATE.lock().unwrap_or_else(|e|e.into_inner());
    let saved=std::mem::take(&mut *s);s.start();let _=s.endpoint("Older voice input.");s.submitted=true;
    let epoch=crate::voice_front::runtime::Epoch{capture:s.epoch,turn:s.turn};drop(s);
    let _restore=Restore(Some(saved));run(epoch);
}

#[cfg(test)]
mod overlap_evidence_tests {
    use super::*;
    use crate::bc_detector::{Action,Decision,Policy,Prediction,CHUNK};
    use crate::dictation::Event;
    fn isolated(name: &str) -> bool {
        if std::env::var_os("UNFOLD_OVERLAP_EVIDENCE_TEST_CHILD").is_some() {return false;}
        let out=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact",name,"--test-threads","4"]).env("UNFOLD_OVERLAP_EVIDENCE_TEST_CHILD","1").output().unwrap();
        assert!(out.status.success(),"{}\n{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));true
    }
    fn start(epoch:u64)->(u64,u64) {
        crate::settings::sync_turn_mode(&crate::settings::Settings {smart_turn_endpoint:false,..Default::default()});
        let mut s=STATE.lock().unwrap();*s=Conversation::default();s.epoch=epoch;s.start();s.endpoint("a request");s.submitted=true;s.speech_started();let key=(s.epoch,s.turn);drop(s);
        *INPUT.lock().unwrap()=Some(crate::conversation_input::Input::default());
        *LISTENER.lock().unwrap()=None;*OVERLAP.lock().unwrap()=None;
        crate::doubletalk::reply(false);crate::doubletalk::reply(true);
        let mut c=crate::floor_decide::context().lock().unwrap();c.sentence="The generated answer is here.".into();c.played.clear();c.echo_until=None;
        key
    }
    fn decision(action:Action)->Decision {Decision{action,begin_sample:0,end_sample:1280,floor:0.9,bc:0.05,no_speech:false}}
    fn level(value:f32,n:usize) {for _ in 0..n {dictation(&mut Event::Level(value));}}
    fn final_words(text:&str) {dictation(&mut Event::Text{seq:1,text:text.into(),settled:None,is_final:true});}
    fn model(p:&mut Policy,playing:bool,positive:bool)->Vec<Decision> {
        let prediction=Prediction{intent:[[0.05,0.,0.,if positive {0.9}else{0.1},if positive {0.05}else{0.85}];2],vad:[if positive {0.9}else{0.1};2],vap:[[0.;8];2]};
        p.step(&[0.001;CHUNK],playing,&prediction)
    }

    #[test]
    fn recorded_genuine_episodes_admit_with_bounded_corroboration_delay() {
        if isolated("conversation::overlap_evidence_tests::recorded_genuine_episodes_admit_with_bounded_corroboration_delay") {return;}
        let fixtures:Value=serde_json::from_str(include_str!("corroborated-overlap-episodes.json")).unwrap();
        for (i,c) in fixtures.as_array().unwrap().iter().enumerate() {
            let (epoch,turn)=start(600+i as u64);
            let at=c["t_ms"].as_f64().unwrap();
            let mut d=decision(if c["action"]=="HOLD" {Action::Hold}else{Action::Yield});d.floor=c["p_floor"].as_f64().unwrap() as f32;d.bc=c["p_bc"].as_f64().unwrap() as f32;
            let events=c["events_actual"].as_array().unwrap();
            for e in events.iter().filter(|e|e["t_ms"].as_f64().unwrap()<=at) {
                dictation(&mut Event::Level(((e["dbfs"].as_f64().unwrap()+60.)/50.).clamp(0.,1.) as f32));
            }
            let mut accepted=overlap_decision(epoch,turn,&d);
            let mut delay=0.;
            for e in events.iter().filter(|e|e["t_ms"].as_f64().unwrap()>at) {
                if accepted {break;}
                delay=e["t_ms"].as_f64().unwrap()-at;
                if e["kind"]=="level" {dictation(&mut Event::Level(((e["dbfs"].as_f64().unwrap()+60.)/50.).clamp(0.,1.) as f32));}
                else {final_words(e["text"].as_str().unwrap());}
                accepted=matches!(STATE.lock().unwrap().phase,Phase::Paused|Phase::Interrupted);
            }
            assert!(accepted && delay<=3000.,"genuine journal row {} failed bounded admission (delay={delay})",c["journal_row"]);
            if c["action"]=="YIELD" {assert_eq!(STATE.lock().unwrap().phase,Phase::Interrupted);}
        }
    }
    #[test]
    fn capture_corroboration_retries_earlier_low_floor_hold() {
        if isolated("conversation::overlap_evidence_tests::capture_corroboration_retries_earlier_low_floor_hold") {return;}
        let (epoch,turn)=start(700);let mut d=decision(Action::Hold);d.floor=0.1;
        assert!(!overlap_decision(epoch,turn,&d));
        level(0.8,5);assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
        level(0.8,1);assert_eq!(STATE.lock().unwrap().phase,Phase::Paused,"300ms of capture evidence retries existing model-positive HOLD, no ASR needed");
    }

    #[test]
    fn listening_clip_echo_is_filtered_before_final_corroboration() {
        if isolated("conversation::overlap_evidence_tests::listening_clip_echo_is_filtered_before_final_corroboration") {return;}
        let (epoch,turn)=start(800);let mut p=Policy::default();
        for _ in 0..3 {for d in model(&mut p,true,true) {overlap_decision(epoch,turn,&d);}}
        *LISTENER.lock().unwrap()=Some(crate::conversation_overlap::Listener::default());
        listening_clip_started("yeah");final_words("yeah");
        assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"a listening-clip echo cannot corroborate a pending model YIELD");
        assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
    }

    #[test]
    fn rejected_hold_terminal_does_not_consume_release_budget() {
        if isolated("conversation::overlap_evidence_tests::rejected_hold_terminal_does_not_consume_release_budget") {return;}
        let (epoch,turn)=start(900);
        assert!(!overlap_decision(epoch,turn,&decision(Action::Hold)));
        assert!(!overlap_decision(epoch,turn,&decision(Action::Noise)));
        assert_eq!(crate::doubletalk::no_speech_releases(),0);
        level(0.8,6);assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
        assert!(overlap_decision(epoch,turn,&decision(Action::Noise)));
        assert_eq!(crate::doubletalk::no_speech_releases(),1);
        level(0.8,6);assert!(overlap_decision(epoch,turn,&decision(Action::Yield)),"a genuine YIELD survives rejected terminals");
    }
    #[test]
    fn trace_entries_reject_even_positive_model_candidates() {
        if isolated("conversation::overlap_evidence_tests::trace_entries_reject_even_positive_model_candidates") {return;}
        let fixtures:Value=serde_json::from_str(include_str!("echo-residual-traces.json")).unwrap();
        assert_eq!(fixtures.as_array().unwrap().len(),12);
        for (i,c) in fixtures.as_array().unwrap().iter().enumerate() {
            let (epoch,turn)=start(100+i as u64);
            for window in c["capture_actual"].as_array().unwrap() {
                let db=window["dbfs"].as_f64().unwrap();
                dictation(&mut Event::Level(((db+60.)/50.).clamp(0.,1.) as f32));
            }

            assert!(c["asr_actual_prior_3s"].as_array().unwrap().iter().all(|r|r["t_ms"].as_f64().unwrap()<c["decision_t_ms"].as_f64().unwrap()-500. || r["words"]==0 || r["source"]!="asr_final"));
            let mut d=decision(if c["action"]=="HOLD" {Action::Hold}else{Action::Yield});d.floor=c["p_floor"].as_f64().unwrap() as f32;d.bc=c["p_bc"].as_f64().unwrap() as f32;
            assert!(!overlap_decision(epoch,turn,&d),"journal row {} must reject at ENTRY with recorded capture/ASR and synthetic positive model VAD",c["journal_row"]);
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
            assert!(!crate::doubletalk::held());
            for event in c["post_entry_actual"].as_array().unwrap() {
                if event["kind"]=="post_aec_level" {
                    let db=event["dbfs"].as_f64().unwrap();dictation(&mut Event::Level(((db+60.)/50.).clamp(0.,1.) as f32));
                } else if event["source"]=="asr_final" {

                    if event["words"].as_u64().unwrap()>0 {final_words("novel");}
                    else {final_words("");}
                }
                assert!(!crate::doubletalk::held(),"late final cannot admit HOLD-only candidate at journal row {}",c["journal_row"]);
                assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
                assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
            }
        }
    }
    #[test]
    fn quiet_one_final_can_corroborate_earlier_model_segment() {
        if isolated("conversation::overlap_evidence_tests::quiet_one_final_can_corroborate_earlier_model_segment") {return;}
        for (i,text) in ["Para","Não","Stop"].into_iter().enumerate() {
            let (epoch,turn)=start(200+i as u64);let mut p=Policy::default();
            for _ in 0..3 {for d in model(&mut p,true,true) {overlap_decision(epoch,turn,&d);}}
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"model alone must wait for independent corroboration");

            level(0.,10);
            final_words(text);
            assert_eq!(STATE.lock().unwrap().phase,Phase::Interrupted,"single non-echo final corroborates pending model YIELD");
            assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),text,"quiet prefix is kept");
            level(0.,18);
            if text=="Para" {

                assert!(STATE.lock().unwrap().pending.is_none(),"Para must not commit early at 900ms");
                assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),text,"admitted Para prefix survives endpoint grace");
                level(0.,6);
            }
            assert_eq!(STATE.lock().unwrap().pending.as_deref(),Some(text));
            assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty(),"final consumed exactly once");
            level(0.,6);
            assert_eq!(STATE.lock().unwrap().pending.as_deref(),Some(text),"more quiet cannot duplicate an already committed final");
            assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
        }
    }
    #[test]
    fn rejected_residual_closed_but_fresh_quiet_listening_and_valid_yield_survive() {
        if isolated("conversation::overlap_evidence_tests::rejected_residual_closed_but_fresh_quiet_listening_and_valid_yield_survive") {return;}
        let (epoch,turn)=start(300);
        assert!(!overlap_decision(epoch,turn,&decision(Action::Yield)));
        overlap_decision(epoch,turn,&decision(Action::Noise));
        {let mut s=STATE.lock().unwrap();s.reply_done();s.speech_ended();}
        final_words("We can");level(0.,20);
        assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
        assert_eq!(STATE.lock().unwrap().phase,Phase::Listening);
        let mut p=Policy::default();
        for _ in 0..2 {for d in model(&mut p,false,true) {overlap_decision(epoch,turn,&d);}}

        final_words("Eu quero uma nova pergunta.");
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"Eu quero uma nova pergunta.");
        assert!(!overlap_decision(epoch.wrapping_sub(1),turn,&decision(Action::Yield)),"stale epoch cannot own input");
        let (epoch,turn)=start(301);level(0.8,6);
        for _ in 0..6 {dictation_rules(&mut Event::Level(0.8));}
        assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
        assert!(overlap_decision(epoch,turn,&decision(Action::Yield)));
        level(0.,20);final_words("Minha pergunta verdadeira.");level(0.,18);
        assert_eq!(STATE.lock().unwrap().pending.as_deref(),Some("Minha pergunta verdadeira."),"late real final after VALID yield still commits");
        assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
    }
    #[test]
    fn no_speech_resume_cap_and_next_reply_rearms_corroborated_hold() {
        if isolated("conversation::overlap_evidence_tests::no_speech_resume_cap_and_next_reply_rearms_corroborated_hold") {return;}
        let (epoch,turn)=start(1300);
        for expected in 1..=2 {
            level(0.8,6);
            assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
            assert!(crate::doubletalk::held());
            assert_eq!(STATE.lock().unwrap().phase,Phase::Paused);
            let mut resume=decision(Action::Resume);resume.no_speech=true;
            assert!(overlap_decision(epoch,turn,&resume));
            assert!(!crate::doubletalk::held());
            assert_eq!(crate::doubletalk::no_speech_releases(),expected,"an admitted no-speech Resume counts a release");
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
        }
        crate::doubletalk::reply(true);
        level(0.8,6);
        assert!(!overlap_decision(epoch,turn,&decision(Action::Hold)),"capped HOLD is rejected, not accepted for journaling");
        assert!(!crate::doubletalk::held() && !crate::doubletalk::playout_held());
        assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking,"capped HOLD cannot pause the reply");
        assert!(overlap_decision(epoch,turn,&decision(Action::Yield)),"a genuine corroborated YIELD still wins at the cap");
        assert!(STATE.lock().unwrap().barged);
        assert_eq!(STATE.lock().unwrap().phase,Phase::Interrupted);
        let (epoch,turn)=start(1301);
        assert_eq!(crate::doubletalk::no_speech_releases(),0);
        level(0.8,6);
        assert!(overlap_decision(epoch,turn,&decision(Action::Hold)),"new reply re-arms an actually corroborated HOLD");
        assert!(crate::doubletalk::held() && crate::doubletalk::playout_held());
        assert_eq!(STATE.lock().unwrap().phase,Phase::Paused);
    }

    #[test]
    fn noise_cap_resets_per_reply_but_never_disables_yield() {
        if isolated("conversation::overlap_evidence_tests::noise_cap_resets_per_reply_but_never_disables_yield") {return;}
        let (epoch,turn)=start(400);
        for _ in 0..2 {
            level(0.8,6);assert!(overlap_decision(epoch,turn,&decision(Action::Hold)));
            assert!(crate::doubletalk::held());
            assert!(overlap_decision(epoch,turn,&decision(Action::Noise)));
        }
        assert_eq!(crate::doubletalk::no_speech_releases(),2,"max-duration Noise counts even with no_speech=false");
        level(0.8,6);
        overlap_decision(epoch,turn,&decision(Action::Hold));assert!(!crate::doubletalk::held(),"release cap blocks another HOLD");
        assert!(overlap_decision(epoch,turn,&decision(Action::Yield)),"genuine YIELD survives cap");
        crate::doubletalk::reply(false);crate::doubletalk::reply(true);
        assert_eq!(crate::doubletalk::no_speech_releases(),0);
    }
    #[test]
    fn uncorroborated_one_word_finals_never_manufacture_model_evidence() {
        if isolated("conversation::overlap_evidence_tests::uncorroborated_one_word_finals_never_manufacture_model_evidence") {return;}
        for (i,text) in ["Para","Não","Stop"].into_iter().enumerate() {
            let (epoch,_)=start(500+i as u64);level(0.,10);final_words(text);
            assert_eq!(STATE.lock().unwrap().phase,Phase::Speaking);
            assert!(INPUT.lock().unwrap().as_ref().unwrap().text().is_empty());
            assert!(!overlap_decision(epoch-1,1,&decision(Action::Yield)));
        }
    }
}

#[cfg(test)]
mod decoder_prefix_identity_tests {
    use super::*;
    use crate::dictation::{Recognizer,Session,Event};
    use crate::bc_detector::{Action,Decision};
    #[derive(Default)] struct Prefix {seen:Vec<f32>,resets:usize}
    impl Recognizer for Prefix {
        fn accept(&mut self,a:&[f32]) {self.seen.extend_from_slice(a);}
        fn text(&mut self)->String {String::new()}
        fn endpoint(&mut self)->bool {false}
        fn reset(&mut self) {self.seen.clear();self.resets+=1;}
        fn finish(&mut self)->String {String::new()}
    }
    #[test]
    fn decoder_reset_preserves_full_fresh_prefix_and_valid_yield_segment() {
        if std::env::var_os("UNFOLD_DECODER_PREFIX_TEST_CHILD").is_none() {
            let out=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","conversation::decoder_prefix_identity_tests::decoder_reset_preserves_full_fresh_prefix_and_valid_yield_segment","--test-threads","4"]).env("UNFOLD_DECODER_PREFIX_TEST_CHILD","1").output().unwrap();
            assert!(out.status.success(),"{}\n{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));return;
        }
        let (epoch,turn)={let mut s=STATE.lock().unwrap();*s=Conversation::default();s.start();s.endpoint("original");s.submitted=true;s.speech_started();(s.epoch,s.turn)};
        *INPUT.lock().unwrap()=Some(new_input());
        let mut session=Session::new(Prefix::default());let mut reset=0;
        session.sync_overlap(epoch,&mut reset);session.feed(&[0.3;640]);
        let decision=|action|Decision{action,begin_sample:0,end_sample:1280,floor:0.9,bc:0.05,no_speech:false};
        overlap_decision(epoch,turn,&decision(Action::Noise));
        {let mut s=STATE.lock().unwrap();s.reply_done();s.speech_ended();}
        session.sync_overlap(epoch,&mut reset);
        let fresh_segment=NEAR_END.lock().unwrap().segment;

        session.feed(&[0.001;16000]);let before=session.recognizer().resets;
        overlap_decision(epoch,turn,&decision(Action::Speech));
        session.sync_overlap(epoch,&mut reset);session.feed(&[0.002;16000]);
        assert_eq!(session.recognizer().resets,before);
        assert_eq!(session.recognizer().seen.len(),32000);
        assert_eq!(session.recognizer().seen[0],0.001);
        assert_eq!(NEAR_END.lock().unwrap().segment,fresh_segment);
        {let mut s=STATE.lock().unwrap();s.phase=Phase::Speaking;s.busy=true;s.barged=false;s.submitted=true;s.model_done=false;s.speech_pending=true;}
        session.sync_overlap(epoch,&mut reset);session.feed(&[0.001;16000]);
        for _ in 0..6 {dictation(&mut Event::Level(0.8));}
        let before_yield=session.recognizer().resets;
        assert!(overlap_decision(epoch,turn,&decision(Action::Yield)));
        session.sync_overlap(epoch,&mut reset);session.feed(&[0.002;16000]);
        assert_eq!(session.recognizer().resets,before_yield,"valid YIELD cannot reset already captured quiet prefix");
        assert_eq!(session.recognizer().seen.len(),32000);
        assert_eq!(session.recognizer().seen[0],0.001);

        {let mut s=STATE.lock().unwrap();s.phase=Phase::Speaking;s.busy=true;s.barged=false;s.submitted=true;}
        session.sync_overlap(epoch,&mut reset);assert!(overlap_unavailable(epoch));
        {let mut s=STATE.lock().unwrap();s.reply_done();s.speech_ended();}
        session.sync_overlap(epoch,&mut reset);
        assert!(!NEAR_END.lock().unwrap().blocked,"degraded soft listening reopens on fresh decoder boundary");
        dictation(&mut Event::Text{seq:1,text:"soft complete request".into(),settled:None,is_final:true});
        assert_eq!(INPUT.lock().unwrap().as_ref().unwrap().text(),"soft complete request");

        {let mut s=STATE.lock().unwrap();s.epoch+=1;}
        session.sync_overlap(epoch+1,&mut reset);assert!(session.recognizer().seen.is_empty());
    }
}
