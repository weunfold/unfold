

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc::{sync_channel, SyncSender}, OnceLock};
use std::time::Instant;
use std::io::{Seek, Write};
use serde_json::{json, Value};

const HOLD_MS: u64 = crate::bc_detector::MAX_HOLD_SAMPLES * 1000 / crate::bc_detector::RATE;
const ONSET_LEVEL: f32 = crate::conversation_input::VOICE_LEVEL;
const LIMIT: u64 = 4 * 1024 * 1024;
static CLOCK: OnceLock<Instant> = OnceLock::new();
static DETECTOR: AtomicBool = AtomicBool::new(true);
static NO_SPEECH_RELEASES: AtomicU64 = AtomicU64::new(0);
static PLAYING: AtomicBool = AtomicBool::new(false);
static REPLY: AtomicBool = AtomicBool::new(false);
static RELEASE: AtomicU64 = AtomicU64::new(0);
static UNTIL: AtomicU64 = AtomicU64::new(0);
static ONSET: AtomicU64 = AtomicU64::new(u64::MAX);
static DUCK: AtomicU64 = AtomicU64::new(u64::MAX);
static TX: OnceLock<SyncSender<Row>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);
fn now() -> u64 { CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 }
pub fn playing(value: bool) {
    PLAYING.store(value, Ordering::SeqCst);
    if !value { resume(); }
}

pub fn reply(value: bool) {
    let previous=REPLY.swap(value,Ordering::SeqCst);
    if !value || !previous {NO_SPEECH_RELEASES.store(0,Ordering::SeqCst);resume();}
}

pub fn no_speech_release() {
    if reply_active() {let _=NO_SPEECH_RELEASES.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|n|Some((n+1).min(2)));}
}
pub fn no_speech_releases() -> u64 { NO_SPEECH_RELEASES.load(Ordering::SeqCst) }
pub fn resume() { UNTIL.store(0, Ordering::SeqCst); RELEASE.fetch_add(1,Ordering::SeqCst); }
pub fn detector_available(value:bool) {DETECTOR.store(value,Ordering::SeqCst);if !value {resume();}}
pub fn reply_active()->bool {REPLY.load(Ordering::SeqCst)}
pub fn hold_overlap() {
    let at=now();
    if DETECTOR.load(Ordering::SeqCst) { DUCK.store(at,Ordering::SeqCst); }
    let _=UNTIL.fetch_update(Ordering::SeqCst,Ordering::SeqCst,|until|(until<=at).then_some(at+HOLD_MS));
}
pub fn held() -> bool { now() < UNTIL.load(Ordering::SeqCst) }
pub fn playout_held()->bool {REPLY.load(Ordering::SeqCst) && (held() || crate::floor_decide::held())}
pub fn reset() {
    reply(false);playing(false);
    ONSET.store(u64::MAX,Ordering::SeqCst);
    DUCK.store(u64::MAX,Ordering::SeqCst);
}
pub fn timing() -> (Option<u64>,Option<u64>,u64) {
    let opt=|n|(n!=u64::MAX).then_some(n);
    (opt(ONSET.load(Ordering::SeqCst)),opt(DUCK.load(Ordering::SeqCst)),now())
}
#[derive(Clone, Copy)]
enum Row {
    Level { at: u64, db: f32, playing: bool, voiced: bool, onset: bool, held: bool },
    Asr { at: u64, seq: u64, words: usize, final_words: bool },
}
impl Row {
    fn json(self)->Value { match self {
        Self::Level{at,db,playing,voiced,onset,held}=>json!({"kind":"post_aec_level","t_ms":at,"dbfs":db,"agent_playing":playing && !held,"playback_state":if held {"agent_held"}else if playing {"agent_playing"}else{"idle"},"vad":voiced,"vad_onset":onset,"held":held,"threshold_dbfs":-38.0,"threshold_qualified":false}),
        Self::Asr{at,seq,words,final_words}=>json!({"kind":"asr_words","t_ms":at,"seq":seq,"words":words,"words_source":if final_words {"asr_final"}else{"asr_partial"}}),
    }}
}
fn offer(row: Row) {
    if let Some(tx)=TX.get() { if tx.try_send(row).is_err() {DROPPED.fetch_add(1,Ordering::Relaxed);} }
}
pub fn asr(seq: u64, text: &str, final_words: bool) {
    offer(Row::Asr{at:now(),seq,words:text.split_whitespace().count(),final_words});
}
fn write_row(file: &mut std::fs::File,row: &Value,limit: u64)->std::io::Result<()> {
    let mut line=serde_json::to_vec(row)?;line.push(b'\n');
    if file.metadata()?.len()+line.len() as u64>limit {
        file.set_len(0)?;file.rewind()?;
        writeln!(file,"{}",json!({"kind":"rotation","t_ms":row["t_ms"],"history_lost":true}))?;
    }
    file.write_all(&line)
}

pub fn telemetry_enabled(version: &str, setting: Option<&str>)->bool {
    match setting {Some("0")=>false,Some("1")=>true,_=>version.starts_with("0.4.")}
}
pub fn install(mut file: std::fs::File) {
    let _=CLOCK.get_or_init(Instant::now);
    let (tx,rx)=sync_channel::<Row>(256);
    if TX.set(tx).is_err() {return;}
    std::thread::spawn(move || {
        let unix_ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let _=write_row(&mut file,&json!({"kind":"started","pid":std::process::id(),"unix_ms":unix_ms,"t_ms":now(),"audio":false,"text":false,"threshold_qualified":false}),LIMIT);
        for row in rx {
            let row=row.json();let dropped=DROPPED.swap(0,Ordering::Relaxed);
            if dropped>0 {let _=write_row(&mut file,&json!({"kind":"loss","dropped":dropped,"t_ms":row["t_ms"]}),LIMIT);}
            if write_row(&mut file,&row,LIMIT).is_err() {eprintln!("doubletalk: telemetry write failed");break;}
        }
    });
}

pub struct Probe { sum: f64, n: usize, voiced: bool, quiet: u32, voice_ms: u32, armed: bool, until: u64, release:u64 }
impl Default for Probe {
    fn default()->Self {Self{sum:0.0,n:0,voiced:false,quiet:0,voice_ms:0,armed:true,until:0,release:RELEASE.load(Ordering::SeqCst)}}
}
impl Probe {
    fn level(&mut self, level: f32, playing: bool, at: u64)->(bool,bool) {
        let voiced=level.is_finite() && level>=ONSET_LEVEL;
        let onset=voiced && !self.voiced;
        self.voiced=voiced;
        if !playing {self.armed=true;self.until=0;self.quiet=0;self.voice_ms=0;return (onset,false);}
        if voiced {self.quiet=0;self.voice_ms=self.voice_ms.saturating_add(50);}
        else {self.quiet=self.quiet.saturating_add(50);if self.quiet>=100 {self.voice_ms=0;}}
        let release=RELEASE.load(Ordering::SeqCst);
        if self.quiet>=100 && (at>=self.until || release!=self.release) {self.armed=true;self.release=release;}

        let duck=false;
        if duck {self.armed=false;self.until=at+HOLD_MS;}
        (onset,duck)
    }
    #[allow(dead_code)]
    pub fn push(&mut self, samples: &[f32]) { self.push_front(samples, None); }
    pub fn push_front(&mut self, samples: &[f32], _front: Option<&crate::voice_front::capture::Reader>) {
        for &sample in samples {
            self.sum+=f64::from(sample).powi(2);self.n+=1;
            if self.n!=crate::duplex_audio::RATE as usize/20 {continue;}
            let level=crate::dictation::level_of(self.sum,self.n);
            let db=(20.0*(self.sum/self.n as f64).sqrt().max(1e-9).log10()) as f32;
            self.sum=0.0;self.n=0;
            let at=now();let playing=PLAYING.load(Ordering::SeqCst);
            let (onset,duck)=self.level(level,playing && REPLY.load(Ordering::SeqCst),at);

            if playing && onset {ONSET.store(at,Ordering::SeqCst);}
            if duck {DUCK.store(at,Ordering::SeqCst);UNTIL.store(at+HOLD_MS,Ordering::SeqCst);}
            offer(Row::Level{at,db,playing,voiced:self.voiced,onset,held:playout_held()});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doubletalk_telemetry_is_trial_only_by_default() {
        assert!(telemetry_enabled("0.4.12",None));
        assert!(!telemetry_enabled("0.5.0",None));
        assert!(!telemetry_enabled("1.0.0",None));
        assert!(!telemetry_enabled("0.4.12",Some("0")));
        assert!(telemetry_enabled("0.5.0",Some("1")));
    }
    #[test]
    fn capture_onset_keeps_trial_calibrated_minus_38db_threshold() {
        let mut p=Probe::default();
        assert!(!p.level(0.30,true,50).1,"clean-mic -45dB replay threshold is not a capture calibration");
        assert!(!p.level(crate::conversation_input::VOICE_LEVEL,true,100).1,"energy-only -38dB cannot hold playout");
        assert!(p.voiced,"the capture observation threshold is unchanged");
    }
    #[test]
    fn h3_energy_onsets_remain_observations_without_playout_authority() {
        let mut p=Probe::default();
        assert_eq!(p.level(0.5,true,50),(true,false));
        for at in (100..3100).step_by(50) {assert!(!p.level(0.5,true,at).1);}
        assert_eq!(p.until,0);
        p.level(0.0,true,3100);p.level(0.0,true,3150);
        assert_eq!(p.level(0.5,true,3200),(true,false));
    }
    #[test]
    fn doubletalk_telemetry_rotation_is_bounded_and_has_no_audio_or_text() {
        let dir=std::env::var_os("RTH_DOUBLETALK_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path=dir.join(format!("doubletalk-telemetry-{}.jsonl",std::process::id()));
        let mut file=std::fs::OpenOptions::new().create(true).truncate(true).read(true).write(true).open(&path).unwrap();
        for at in 0..1000 {write_row(&mut file,&Row::Level{at,db:-48.0,playing:true,voiced:false,onset:false,held:false}.json(),1024).unwrap();}
        assert!(file.metadata().unwrap().len()<=1024);
        let rows=std::fs::read_to_string(path).unwrap();assert!(rows.contains("rotation"));
        for row in rows.lines() {let x:Value=serde_json::from_str(row).unwrap();assert!(x.get("samples").is_none() && x.get("text").is_none());}
        let row=Row::Asr{at:200,seq:3,words:8,final_words:false}.json();
        assert_eq!(row["words_source"],"asr_partial");assert!(row.get("text").is_none());
    }
}

#[cfg(test)]
mod review04 {
    use super::*;
    #[test]
    fn review04_b1_listening_or_ack_clip_onset_never_holds_playout() {
        if std::env::var_os("RTH_DOUBLETALK_REVIEW04_CHILD").is_none() {
            let child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","doubletalk::review04::review04_b1_listening_or_ack_clip_onset_never_holds_playout","--test-threads","4"])
                .env("RTH_DOUBLETALK_REVIEW04_CHILD","1").output().unwrap();
            assert!(child.status.success(),"{}",String::from_utf8_lossy(&child.stdout));return;
        }
        reset();playing(true);
        let mut probe=Probe::default();probe.push(&[0.02;2400]);
        assert!(!held(),"listening and acknowledgement clips must not trigger a reply hold");
        crate::floor_decide::hold(true);
        assert!(!playout_held(),"a retained semantic reply hold cannot freeze clips either");
        let q=rth_voice::queue::PlayQueue::new(24_000,16);q.push(1,vec![11,22]);
        let mut mono=Vec::new();crate::speaker::cpal_pull(&q,&mut mono,2);
        assert_eq!(mono,[11,22],"clip samples keep flowing after a user onset");
        crate::floor_decide::hold(false);reply(true);
        Probe::default().push(&[0.02;crate::duplex_audio::RATE as usize * 3 / 10]);
        assert!(!held(),"reply energy alone also has no hold authority");
        hold_overlap();assert!(held(),"only model-admitted reply evidence can hold");
        reply(false);assert!(!held() && !playout_held(),"reply termination clears its provisional hold");
        reset();
    }
}

#[cfg(test)]
mod t16_capture {
    use super::*;
    #[test]
    fn t16_capture_energy_never_holds_or_fences_a_published_front() {
        if std::env::var_os("T16_CAPTURE_CHILD").is_none() {
            let out=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","doubletalk::t16_capture::t16_capture_energy_never_holds_or_fences_a_published_front","--test-threads","4"]).env("T16_CAPTURE_CHILD","1").output().unwrap();
            assert!(out.status.success(),"{}\n{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));return;
        }
        reset();reply(true);playing(true);
        let capture=std::sync::Arc::new(crate::voice_front::capture::Capture::<2048>::new());let reader=capture.reader();reader.activate();
        let epoch=crate::voice_front::runtime::Epoch{capture:1,turn:1};
        let ticket=crate::voice_front::runtime::Ticket::new(epoch,"t16-front".into());capture.publish(&ticket,0).unwrap();
        Probe::default().push_front(&[0.1;crate::duplex_audio::RATE as usize/20],Some(&reader));
        assert!(!held(),"strong energy alone is still not near-end speech evidence");
        assert!(ticket.render(epoch,"t16-front",480),"energy-only capture cannot bypass model admission by fencing front");
        reset();
    }
}
