
use serde_json::{json, Value};
use std::io::{Seek, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
static HELD: AtomicBool = AtomicBool::new(false);
pub fn held() -> bool { HELD.load(Ordering::SeqCst) }
pub fn hold(value: bool) { HELD.store(value, Ordering::SeqCst); }
static JOURNAL: OnceLock<Mutex<std::sync::mpsc::SyncSender<Value>>> = OnceLock::new();
static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();
pub fn journal_install(mut file: std::fs::File) {
    if JOURNAL.get().is_some() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<Value>(256);
    let _ = CLOCK.set(Instant::now());
    if JOURNAL.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::spawn(move || {
        for row in rx {
            let dropped = DROPPED.swap(0, Ordering::SeqCst);
            if dropped > 0 {
                let fault = json!({"kind":"journal_fault","detail":format!("bounded journal queue dropped {dropped} records"),"t_ms":row["t_ms"]});
                let _ = write_row(&mut file, &fault, 4 * 1024 * 1024);
            }
            if write_row(&mut file, &row, 4 * 1024 * 1024).is_err() {
                eprintln!("floor.decide: journal write failed");
            }
        }
    });
    journal(
        "started",
        json!({"pid":std::process::id(),"producer":"unfold-app-floor","clock":"app-floor-monotonic"}),
    );
}
fn write_row(file: &mut std::fs::File, row: &Value, limit: u64) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(row)?;
    if line.len() as u64 > limit / 2 {
        line = serde_json::to_vec(
            &json!({"kind":"journal_fault","detail":"oversize_record","t_ms":row["t_ms"]}),
        )?;
    }
    line.push(b'\n');
    if file.metadata()?.len() + line.len() as u64 > limit {
        file.set_len(0)?;
        file.rewind()?;

        let start = json!({"kind":"started","pid":std::process::id(),"producer":"unfold-app-floor","rotation":true,"t_ms":row["t_ms"]});
        writeln!(file, "{start}")?;
    }
    file.write_all(&line)
}
pub fn journal(kind: &str, mut row: Value) {
    if let Some(tx) = JOURNAL.get() {
        let tx = tx.lock().unwrap();
        row["kind"] = json!(kind);
        row["t_ms"] = json!(CLOCK.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0);
        if tx.try_send(row).is_err() {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
}
fn tail(s: &str, n: usize) -> String {
    let words: Vec<_> = s.split_whitespace().collect();
    words[words.len().saturating_sub(n)..].join(" ")
}

#[derive(Default)]
pub struct Context {
    pub echo_until: Option<Instant>,
    pub sentence: String,
    pub played: String,
    pub last: String,
    pub user: String,
    pub reply: String,
}
static CONTEXT: OnceLock<Mutex<Context>> = OnceLock::new();
pub fn context() -> &'static Mutex<Context> {
    CONTEXT.get_or_init(|| Mutex::new(Context::default()))
}
pub fn begin_turn(user: &str) {
    let mut c = context().lock().unwrap();
    c.last = tail(&format!("User: {} Agent: {}", c.user, c.reply), 80);
    c.user = tail(user, 40);
    c.reply.clear();
    c.sentence.clear();
    c.played.clear();
}
pub fn reply(frame: &Value) {
    let mut c = context().lock().unwrap();
    if let Some(s) = frame["replace"].as_str() {
        c.reply = tail(s, 80);
    } else if let Some(s) = frame["delta"].as_str() {
        c.reply = tail(&format!("{}{s}", c.reply), 80);
    }
}
static PLAYBACKS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
pub fn playback(e: &Value, backchannel: bool) {
    let Some(id) = e["preview_id"].as_str() else {
        return;
    };
    let mut playing = PLAYBACKS
        .get_or_init(|| Mutex::new(std::collections::HashSet::new()))
        .lock()
        .unwrap();
    if e["state"] == "streaming" && playing.insert(id.into()) {
        journal(
            "playback",
            json!({"response_id":id,"phase":"started","backchannel":backchannel}),
        );
    }
    if e["state"] == "done" || e["state"] == "failed" {
        if playing.remove(id) {
            journal(
                "playback",
                json!({"response_id":id,"phase":if e["stopped"]==true || e["state"]=="failed" {"cleared"} else {"finished"},"backchannel":backchannel,"audio_seconds":e["audio_seconds"]}),
            );
        }
        if e["stopped"] == true {
            journal("speaker_cleared", json!({"response_id":id}));
        }
    }
}
