

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FolderId {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub grant_id: String,
    pub request_id: String,

    pub root: PathBuf,
    pub id: FolderId,
    pub scope: String,

    pub effect_id: String,
    pub args_sha256: String,
    pub utc_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    Open,
    Granted,
    Denied,
    Lapsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub request_id: String,
    pub task_id: String,
    pub root: PathBuf,
    pub id: FolderId,

    pub reason: String,
    pub state: RequestState,
    pub utc_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Line {
    GrantRequested { request_id: String, task_id: String, root: PathBuf, id: FolderId, reason: String, utc_ms: u64 },
    Granted { grant: Grant },
    Denied { request_id: String, reason: String, utc_ms: u64 },
    Revoked { grant_id: String, by: String, utc_ms: u64 },
    Lapsed { request_id: String, reason: String, utc_ms: u64 },
}

pub struct Grants {
    path: PathBuf,
    file: std::fs::File,
    boot: String,
    next_n: u64,
    grants: BTreeMap<String, Grant>,
    requests: BTreeMap<String, Request>,
}

impl Grants {

    pub fn open(path: &Path, boot: &str) -> std::io::Result<(Grants, Vec<Line>)> {
        let existed = path.exists();
        let mut file = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path)?;
        if !existed {
            file.sync_all()?;
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            std::fs::File::open(dir)?.sync_all()?;
        }
        let mut text = Self::read_all(&mut file)?;
        let keep = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
        if keep < text.len() {
            file.set_len(keep as u64)?;
            file.sync_all()?;
            text.truncate(keep);
        }
        let mut g = Grants { path: path.to_path_buf(), file, boot: boot.to_string(), next_n: 0, grants: BTreeMap::new(), requests: BTreeMap::new() };
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rec: Line = serde_json::from_str(line).map_err(|e| std::io::Error::other(format!("{}:{}: {e}", path.display(), n + 1)))?;
            g.fold(rec);
            g.next_n += 1;
        }
        let open: Vec<String> = g.requests.values().filter(|r| r.state == RequestState::Open).map(|r| r.request_id.clone()).collect();
        let mut lapsed = Vec::new();
        for request_id in open {
            let line = Line::Lapsed { request_id, reason: "restart".into(), utc_ms: crate::utc_ms() };
            g.commit(&line)?;
            lapsed.push(line);
        }
        Ok((g, lapsed))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Vec<Grant> {
        let mut v: Vec<Grant> = self.grants.values().cloned().collect();
        v.sort_by_key(|g| (g.utc_ms, g.grant_id.clone()));
        v
    }

    pub fn get(&self, grant_id: &str) -> Option<&Grant> {
        self.grants.get(grant_id)
    }

    pub fn request(&self, request_id: &str) -> Option<&Request> {
        self.requests.get(request_id)
    }

    pub fn for_folder(&self, id: FolderId) -> Option<&Grant> {
        self.grants.values().find(|g| g.id == id)
    }

    pub fn ask(&mut self, task_id: &str, root: &Path, id: FolderId, reason: &str) -> std::io::Result<(Request, Option<Line>)> {
        if let Some(r) = self.requests.values().find(|r| r.state == RequestState::Open && r.task_id == task_id && r.id == id) {
            return Ok((r.clone(), None));
        }
        let request_id = format!("gr-{}-{}", self.boot, self.next_n + 1);
        let line = Line::GrantRequested { request_id: request_id.clone(), task_id: task_id.to_string(), root: root.to_path_buf(), id, reason: reason.to_string(), utc_ms: crate::utc_ms() };
        self.commit(&line)?;
        Ok((self.requests[&request_id].clone(), Some(line)))
    }

    pub fn grant(&mut self, request_id: &str, effect_id: &str, args_sha256: &str) -> std::io::Result<Result<(Grant, Line), &'static str>> {
        let Some(r) = self.requests.get(request_id) else { return Ok(Err("unknown_request")) };
        if r.state != RequestState::Open {
            return Ok(Err("request_not_open"));
        }
        let grant = Grant {
            grant_id: format!("g-{}-{}", self.boot, self.next_n + 1),
            request_id: request_id.to_string(),
            root: r.root.clone(),
            id: r.id,
            scope: "read".into(),
            effect_id: effect_id.to_string(),
            args_sha256: args_sha256.to_string(),
            utc_ms: crate::utc_ms(),
        };
        let line = Line::Granted { grant: grant.clone() };
        self.commit(&line)?;
        Ok(Ok((grant, line)))
    }

    pub fn deny(&mut self, request_id: &str, reason: &str) -> std::io::Result<Result<Line, &'static str>> {
        let Some(r) = self.requests.get(request_id) else { return Ok(Err("unknown_request")) };
        if r.state != RequestState::Open {
            return Ok(Err("request_not_open"));
        }
        let line = Line::Denied { request_id: request_id.to_string(), reason: reason.to_string(), utc_ms: crate::utc_ms() };
        self.commit(&line)?;
        Ok(Ok(line))
    }

    pub fn revoke(&mut self, grant_id: &str, by: &str) -> std::io::Result<Result<(Grant, Line), &'static str>> {
        let Some(g) = self.grants.get(grant_id).cloned() else { return Ok(Err("unknown_grant")) };
        let line = Line::Revoked { grant_id: grant_id.to_string(), by: by.to_string(), utc_ms: crate::utc_ms() };
        self.commit(&line)?;
        Ok(Ok((g, line)))
    }

    fn commit(&mut self, line: &Line) -> std::io::Result<()> {
        let mut text = serde_json::to_string(line).map_err(std::io::Error::other)?;
        text.push('\n');
        self.file.write_all(text.as_bytes())?;
        self.file.sync_data()?;
        self.fold(line.clone());
        self.next_n += 1;
        Ok(())
    }

    fn fold(&mut self, line: Line) {
        match line {
            Line::GrantRequested { request_id, task_id, root, id, reason, utc_ms } => {
                self.requests.insert(request_id.clone(), Request { request_id, task_id, root, id, reason, state: RequestState::Open, utc_ms });
            }
            Line::Granted { grant } => {
                if let Some(r) = self.requests.get_mut(&grant.request_id) {
                    r.state = RequestState::Granted;
                }
                self.grants.insert(grant.grant_id.clone(), grant);
            }
            Line::Denied { request_id, .. } => {
                if let Some(r) = self.requests.get_mut(&request_id) {
                    r.state = RequestState::Denied;
                }
            }
            Line::Lapsed { request_id, .. } => {
                if let Some(r) = self.requests.get_mut(&request_id) {
                    r.state = RequestState::Lapsed;
                }
            }
            Line::Revoked { grant_id, .. } => {
                self.grants.remove(&grant_id);
            }
        }
    }

    fn read_all(file: &mut std::fs::File) -> std::io::Result<String> {
        let mut text = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut text)?;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdir::TempDir;

    const A: FolderId = FolderId { dev: 1, ino: 10 };
    const B: FolderId = FolderId { dev: 1, ino: 20 };

    #[test]
    fn ask_grant_and_revoke_are_each_one_durable_line() {
        let d = TempDir::new("grants");
        let log = d.path().join("grants.jsonl");
        let (mut g, lapsed) = Grants::open(&log, "b1").unwrap();
        assert!(lapsed.is_empty() && g.list().is_empty());
        let (req, line) = g.ask("b1-t1", Path::new("/x/docs"), A, "files.read /x/docs/a.md").unwrap();
        assert!(matches!(line, Some(Line::GrantRequested { .. })));
        assert_eq!(req.state, RequestState::Open);
        let (again, line2) = g.ask("b1-t1", Path::new("/x/docs"), A, "files.search /x/docs").unwrap();
        assert_eq!((again.request_id.as_str(), line2), (req.request_id.as_str(), None), "one card per folder per task");
        assert!(g.for_folder(A).is_none(), "asking grants nothing");
        let (grant, _) = g.grant(&req.request_id, "e-1", "digest").unwrap().unwrap();
        assert_eq!(g.for_folder(A).unwrap().grant_id, grant.grant_id);
        assert!(g.for_folder(B).is_none());
        assert_eq!(g.grant(&req.request_id, "e-1", "digest").unwrap().unwrap_err(), "request_not_open", "a request is granted once");
        let (revoked, _) = g.revoke(&grant.grant_id, "settings").unwrap().unwrap();
        assert_eq!(revoked.grant_id, grant.grant_id);
        assert!(g.for_folder(A).is_none(), "a revoke takes effect at once");
        assert_eq!(g.revoke(&grant.grant_id, "settings").unwrap().unwrap_err(), "unknown_grant");
        let text = std::fs::read_to_string(&log).unwrap();
        let events: Vec<String> = text.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["event"].as_str().unwrap().to_string()).collect();
        assert_eq!(events, ["grant_requested", "granted", "revoked"]);
    }

    #[test]
    fn a_grant_is_remembered_after_a_restart_and_an_open_request_lapses() {
        let d = TempDir::new("grants-boot");
        let log = d.path().join("grants.jsonl");
        let kept = {
            let (mut g, _) = Grants::open(&log, "b1").unwrap();
            let (r1, _) = g.ask("b1-t1", Path::new("/x/docs"), A, "r").unwrap();
            let (kept, _) = g.grant(&r1.request_id, "e-1", "d").unwrap().unwrap();
            let (r2, _) = g.ask("b1-t1", Path::new("/x/other"), B, "r").unwrap();
            assert_eq!(g.request(&r2.request_id).unwrap().state, RequestState::Open);
            kept
        };
        let (g, lapsed) = Grants::open(&log, "b2").unwrap();
        assert_eq!(g.for_folder(A).unwrap(), &kept, "remembered");
        assert!(g.for_folder(B).is_none(), "recovery never grants");
        assert_eq!(lapsed.len(), 1);
        assert!(matches!(&lapsed[0], Line::Lapsed { reason, .. } if reason == "restart"));
        let (g3, again) = Grants::open(&log, "b3").unwrap();
        assert!(again.is_empty(), "a lapse is recorded once");
        assert_eq!(g3.list().len(), 1);
    }

    #[test]
    fn denied_requests_cannot_be_granted_and_ids_never_repeat_across_boots() {
        let d = TempDir::new("grants-deny");
        let log = d.path().join("grants.jsonl");
        let first = {
            let (mut g, _) = Grants::open(&log, "b1").unwrap();
            let (r, _) = g.ask("b1-t1", Path::new("/x/docs"), A, "r").unwrap();
            g.deny(&r.request_id, "person").unwrap().unwrap();
            assert_eq!(g.grant(&r.request_id, "e", "d").unwrap().unwrap_err(), "request_not_open");
            assert_eq!(g.deny(&r.request_id, "person").unwrap().unwrap_err(), "request_not_open");
            assert_eq!(g.grant("nope", "e", "d").unwrap().unwrap_err(), "unknown_request");
            r.request_id
        };
        let (mut g, _) = Grants::open(&log, "b1").unwrap();
        let (r, _) = g.ask("b1-t2", Path::new("/x/docs"), A, "r").unwrap();
        assert_ne!(r.request_id, first, "the same boot id after a restart still mints new ids");
    }

    #[test]
    fn a_torn_tail_is_cut_and_the_rest_is_kept() {
        let d = TempDir::new("grants-torn");
        let log = d.path().join("grants.jsonl");
        {
            let (mut g, _) = Grants::open(&log, "b1").unwrap();
            let (r, _) = g.ask("b1-t1", Path::new("/x/docs"), A, "r").unwrap();
            g.grant(&r.request_id, "e-1", "d").unwrap().unwrap();
        }
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(b"{\"event\":\"revoked\",\"grant_").unwrap();
        drop(f);
        let (g, _) = Grants::open(&log, "b2").unwrap();
        assert!(g.for_folder(A).is_some(), "a half-written revoke is not a revoke");
        assert!(std::fs::read_to_string(&log).unwrap().ends_with('\n'));
    }
}
