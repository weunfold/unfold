

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

pub const MAX_MESSAGE_BYTES: usize = 2048;

pub fn record_panics(journal: Option<PathBuf>, last: PathBuf, label: String) {
    let journal: Option<File> = journal.and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
    let started = Instant::now();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let mut message = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "(non-string panic payload)".into());
        if message.len() > MAX_MESSAGE_BYTES {
            let mut cut = MAX_MESSAGE_BYTES;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
            message.push_str("…(truncated)");
        }
        let row = json!({
            "kind": "host_panic",
            "uptime_ms": started.elapsed().as_millis() as u64,
            "utc_ms": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            "pid": std::process::id(),
            "thread": std::thread::current().name().unwrap_or("?"),
            "location": info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default(),
            "message": message,
        });

        if let Some(mut f) = journal.as_ref() {
            let line = format!("{row}\n");
            let _ = f.write(line.as_bytes());
            let _ = f.sync_all();
        }
        let mut full = row;
        full["label"] = label.clone().into();
        full["backtrace"] = std::backtrace::Backtrace::force_capture().to_string().into();
        let tmp = last.with_extension("json.tmp");
        if File::create(&tmp).and_then(|mut f| f.write_all(full.to_string().as_bytes()).and_then(|()| f.sync_all())).is_ok() {
            let _ = std::fs::rename(&tmp, &last);
        }
        previous(info);
    }));
}
