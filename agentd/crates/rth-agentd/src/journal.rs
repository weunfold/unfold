

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use rth_protocol::{Envelope, Event};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};

use crate::context::Fold;

pub const MAX_TAIL_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub offset: u64,
    pub prefix_sha256: String,
}

enum Msg {
    Line { id: String, decision: bool, bytes: Vec<u8> },
    Barrier(oneshot::Sender<Result<Committed, String>>),

    #[cfg(test)]
    FailNextWrite,
}

struct Inner {
    tx: mpsc::Sender<Msg>,
    fold: Fold,
}

pub struct Journal {
    inner: std::sync::Mutex<Inner>,
}

pub struct Recovery {
    pub truncated_bytes: u64,

    pub len: u64,

    pub prefix_sha256: Option<String>,

    pub tail: Option<Vec<u8>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Journal {

    pub fn open(path: &Path, faults: mpsc::UnboundedSender<String>, prefix_at: Option<u64>) -> std::io::Result<(Self, Recovery)> {
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        let cut = repair_tail(&mut file)?;
        let len = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        let mut hasher = Sha256::new();
        let mut prefix_sha256 = None;
        let mut tail = None;
        let mut pos = 0u64;
        let mut buf = vec![0u8; 1 << 16];
        let want = prefix_at.filter(|p| *p <= len);
        if want == Some(0) {
            prefix_sha256 = Some(hex(&hasher.clone().finalize()));
        }
        let keep_tail = want.is_some_and(|p| len - p <= MAX_TAIL_BYTES);
        let mut tail_buf = Vec::new();
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            let start = pos;
            pos += n as u64;
            match want {
                Some(p) if p > start && p <= pos => {
                    let split = (p - start) as usize;
                    hasher.update(&chunk[..split]);
                    prefix_sha256 = Some(hex(&hasher.clone().finalize()));
                    hasher.update(&chunk[split..]);
                    if keep_tail {
                        tail_buf.extend_from_slice(&chunk[split..]);
                    }
                }
                Some(p) if p <= start => {
                    hasher.update(chunk);
                    if keep_tail {
                        tail_buf.extend_from_slice(chunk);
                    }
                }
                _ => hasher.update(chunk),
            }
        }
        if keep_tail {
            tail = Some(tail_buf);
        }
        let (tx, mut rx) = mpsc::channel::<Msg>(4096);

        std::thread::spawn(move || {
            let mut offset = len;
            let mut poisoned: Option<String> = None;
            #[cfg(test)]
            let mut fail_next = false;
            while let Some(msg) = rx.blocking_recv() {
                match msg {
                    #[cfg(test)]
                    Msg::FailNextWrite => fail_next = true,
                    Msg::Line { id, decision, bytes } => {
                        #[cfg(test)]
                        let bytes = if std::mem::take(&mut fail_next) {
                            let _ = file.write_all(&bytes[..bytes.len() / 2]);
                            let _ = faults.send(format!("journal write failed for {id}: injected"));
                            poisoned.get_or_insert_with(|| format!("write of {id} failed: injected partial write"));
                            continue;
                        } else {
                            bytes
                        };
                        let result = file.write_all(&bytes).and_then(|_| {

                            if decision { file.sync_data() } else { Ok(()) }
                        });
                        match result {
                            Ok(()) => {
                                offset += bytes.len() as u64;
                                hasher.update(&bytes);
                            }
                            Err(e) => {
                                poisoned.get_or_insert_with(|| format!("write of {id} failed: {e}"));
                                let _ = faults.send(format!("journal write failed for {id}: {e}"));
                            }
                        }
                    }
                    Msg::Barrier(reply) => {
                        let r = match &poisoned {
                            Some(why) => Err(format!("journal commit unknown since {why}")),
                            None => match file.sync_data() {
                                Ok(()) => Ok(Committed { offset, prefix_sha256: hex(&hasher.clone().finalize()) }),
                                Err(e) => {
                                    let why = format!("sync failed: {e}");
                                    poisoned = Some(why.clone());
                                    Err(format!("journal commit unknown: {why}"))
                                }
                            },
                        };
                        let _ = reply.send(r);
                    }
                }
            }
        });
        let journal = Self { inner: std::sync::Mutex::new(Inner { tx, fold: Fold::default() }) };
        Ok((journal, Recovery { truncated_bytes: cut.truncated_bytes, len, prefix_sha256, tail }))
    }

    pub fn set_fold(&self, fold: Fold) {
        self.inner.lock().unwrap().fold = fold;
    }

    pub fn turns(&self) -> Vec<crate::context::Turn> {
        self.inner.lock().unwrap().fold.turns.iter().cloned().collect()
    }

    pub fn append(&self, env: &Envelope) -> Result<(), String> {
        let decision = matches!(env.event, Event::Decision { .. });
        let mut inner = self.inner.lock().unwrap();
        inner
            .tx
            .try_send(Msg::Line { id: env.id.clone(), decision, bytes: env.to_line().into_bytes() })
            .map_err(|e| format!("journal queue rejected {}: {e}", env.id))?;
        inner.fold.apply(env);
        Ok(())
    }

    pub fn snapshot_request(&self) -> Result<(Fold, oneshot::Receiver<Result<Committed, String>>), String> {
        let inner = self.inner.lock().unwrap();
        let (tx, rx) = oneshot::channel();
        inner.tx.try_send(Msg::Barrier(tx)).map_err(|e| format!("journal queue rejected a barrier: {e}"))?;
        Ok((inner.fold.clone(), rx))
    }

    #[cfg(test)]
    pub fn fail_next_write(&self) {
        let _ = self.inner.lock().unwrap().tx.try_send(Msg::FailNextWrite);
    }

    #[allow(dead_code)]
    pub async fn barrier(&self) -> Result<Committed, String> {
        let (_, rx) = self.snapshot_request()?;
        rx.await.map_err(|_| "journal writer gone".to_string())?
    }
}

pub struct Cut {
    pub truncated_bytes: u64,
}

pub(crate) fn repair_tail(file: &mut std::fs::File) -> std::io::Result<Cut> {
    let len = file.seek(SeekFrom::End(0))?;
    if len == 0 {
        return Ok(Cut { truncated_bytes: 0 });
    }
    let window = len.min(1 << 20);
    file.seek(SeekFrom::Start(len - window))?;
    let mut tail = Vec::with_capacity(window as usize);
    file.read_to_end(&mut tail)?;
    if tail.last() == Some(&b'\n') {
        return Ok(Cut { truncated_bytes: 0 });
    }
    let keep = match tail.iter().rposition(|&b| b == b'\n') {
        Some(i) => len - window + i as u64 + 1,
        None if window == len => 0,
        None => return Err(std::io::Error::other("final record longer than 1 MiB; refusing to guess")),
    };
    file.set_len(keep)?;
    Ok(Cut { truncated_bytes: len - keep })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::{MonotonicClock, Source, Stamper};

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rth-journal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn torn_final_record_is_cut_back_to_the_last_newline() {
        let d = dir("torn");
        let path = d.join("j.jsonl");
        std::fs::write(&path, "{\"a\":1}\n{\"b\":").unwrap();
        let mut f = std::fs::OpenOptions::new().read(true).append(true).open(&path).unwrap();
        assert_eq!(repair_tail(&mut f).unwrap().truncated_bytes, 5);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"a\":1}\n");
    }

    #[tokio::test]
    async fn a_barrier_commits_exactly_the_bytes_queued_before_it_and_hashes_that_prefix() {
        let d = dir("barrier");
        let path = d.join("j.jsonl");
        std::fs::write(&path, "{\"old\":1}\n").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let (j, rec) = Journal::open(&path, tx, Some(10)).unwrap();
        assert_eq!((rec.len, rec.tail.as_deref()), (10, Some(&b""[..])));
        assert_eq!(rec.prefix_sha256.as_deref(), Some(hex(&Sha256::digest(b"{\"old\":1}\n")).as_str()));
        let mut g = Stamper::new(Source::Guest, "guest-j", Box::new(MonotonicClock::start()));
        let e = g.stamp(Event::Log { level: "info".into(), msg: "one".into() }, None);
        j.append(&e).unwrap();
        let c = j.barrier().await.unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(c.offset, bytes.len() as u64);
        assert_eq!(c.prefix_sha256, hex(&Sha256::digest(&bytes)));

        let e2 = g.stamp(Event::Log { level: "info".into(), msg: "two".into() }, None);
        j.append(&e2).unwrap();
        j.barrier().await.unwrap();
        drop(j);
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_j, rec) = Journal::open(&path, tx, Some(c.offset)).unwrap();
        assert_eq!(rec.prefix_sha256.as_deref(), Some(c.prefix_sha256.as_str()));
        assert_eq!(rec.tail.unwrap(), e2.to_line().into_bytes());

        let (tx, _rx) = mpsc::unbounded_channel();
        let (_j, rec) = Journal::open(&path, tx, Some(1 << 40)).unwrap();
        assert!(rec.prefix_sha256.is_none() && rec.tail.is_none());
    }

    #[tokio::test]
    async fn after_an_uncertain_write_no_barrier_ever_commits_again() {
        let d = dir("poison");
        let (tx, _rx) = mpsc::unbounded_channel();
        let (j, _) = Journal::open(&d.join("j.jsonl"), tx, None).unwrap();
        let mut g = Stamper::new(Source::Guest, "guest-p", Box::new(MonotonicClock::start()));
        j.append(&g.stamp(Event::Log { level: "info".into(), msg: "a".into() }, None)).unwrap();
        let good = j.barrier().await.unwrap();
        j.fail_next_write();
        j.append(&g.stamp(Event::Log { level: "info".into(), msg: "torn".into() }, None)).unwrap();
        j.append(&g.stamp(Event::Log { level: "info".into(), msg: "after".into() }, None)).unwrap();
        let e = j.barrier().await.unwrap_err();
        assert!(e.contains("commit unknown"), "{e}");
        assert!(j.barrier().await.is_err(), "still poisoned");
        assert!(std::fs::metadata(d.join("j.jsonl")).unwrap().len() > good.offset);
    }
}
