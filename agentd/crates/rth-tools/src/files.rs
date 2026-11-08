

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::Serialize;

use crate::confine::{Confiner, OpenedDir, OpenedFile};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadPage {
    pub path: PathBuf,
    pub offset: u64,

    pub next_offset: Option<u64>,
    pub total_bytes: u64,
    pub more: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {

    Binary,
    OffsetPastEnd,
    Io(String),
}

impl ReadError {
    pub fn reason(&self) -> &'static str {
        match self {
            ReadError::Binary => "binary",
            ReadError::OffsetPastEnd => "offset_past_end",
            ReadError::Io(_) => "io_error",
        }
    }
}

pub fn read_page(o: &mut OpenedFile, offset: u64, page_chars: usize) -> Result<ReadPage, ReadError> {
    let total = o.len;
    if offset > total {
        return Err(ReadError::OffsetPastEnd);
    }

    let want = page_chars.saturating_mul(4).saturating_add(3);
    let buf = read_at(o, offset, want).map_err(|e| ReadError::Io(e.to_string()))?;
    if buf.contains(&0) {
        return Err(ReadError::Binary);
    }
    let skip = buf.iter().take(3).take_while(|b| (**b & 0xC0) == 0x80).count();
    let body = &buf[skip..];
    let text = match std::str::from_utf8(body) {
        Ok(t) => t,

        Err(e) if e.error_len().is_none() => std::str::from_utf8(&body[..e.valid_up_to()]).expect("valid prefix"),
        Err(_) => return Err(ReadError::Binary),
    };
    let end = text.char_indices().nth(page_chars).map(|(i, _)| i).unwrap_or(text.len());
    let start = offset + skip as u64;
    let next = start + end as u64;
    let next_offset = (next < total).then_some(next);
    Ok(ReadPage { path: o.real.clone(), offset: start, next_offset, total_bytes: total, more: next_offset.is_some(), text: text[..end].to_string() })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimits {
    pub max_files: usize,
    pub max_depth: usize,
    pub max_file_bytes: u64,
    pub page_hits: usize,
}

impl Default for SearchLimits {
    fn default() -> Self {
        SearchLimits { max_files: 20_000, max_depth: 16, max_file_bytes: 4 * 1024 * 1024, page_hits: 50 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HitKind {
    Name,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub path: PathBuf,
    pub kind: HitKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchPage {
    pub folder: PathBuf,
    pub query: String,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub hits: Vec<Hit>,
    pub files_scanned: usize,

    pub scan_limited: bool,
}

pub fn search(c: &Confiner, dir: &OpenedDir, query: &str, offset: usize, limits: SearchLimits) -> SearchPage {
    let needle = query.trim().to_lowercase();
    let mut hits = Vec::new();
    let mut files_scanned = 0;
    let mut scan_limited = false;

    let wanted = offset.saturating_add(limits.page_hits).saturating_add(1);
    let mut stack: Vec<(PathBuf, usize)> = vec![(dir.real.clone(), 0)];
    'walk: while let Some((folder, depth)) = stack.pop() {
        if c.covering(&folder).is_err() {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&folder) else { continue };
        let mut entries: Vec<(String, PathBuf, std::fs::FileType)> =
            rd.filter_map(|e| e.ok()).filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.path(), e.file_type().ok()?))).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let mut subdirs = Vec::new();
        for (name, path, ft) in entries {
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if name != ".git" && depth + 1 < limits.max_depth {
                    subdirs.push((path, depth + 1));
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if files_scanned >= limits.max_files {
                scan_limited = true;
                break 'walk;
            }
            files_scanned += 1;
            if needle.is_empty() || name.to_lowercase().contains(&needle) {
                hits.push(Hit { path: path.clone(), kind: HitKind::Name, line: None, snippet: name.clone() });
            }
            if !needle.is_empty() {
                content_hits(c, &path, &needle, limits.max_file_bytes, &mut hits);
            }
            if hits.len() >= wanted {
                break 'walk;
            }
        }

        stack.extend(subdirs.into_iter().rev());
    }
    let more = hits.len() > offset + limits.page_hits;
    let page: Vec<Hit> = hits.into_iter().skip(offset).take(limits.page_hits).collect();
    let next_offset = more.then_some(offset + page.len());
    SearchPage { folder: dir.real.clone(), query: query.to_string(), offset, next_offset, hits: page, files_scanned, scan_limited }
}

fn content_hits(c: &Confiner, path: &std::path::Path, needle: &str, max_bytes: u64, hits: &mut Vec<Hit>) {
    let Ok(mut o) = c.open_file(&path.to_string_lossy()) else { return };
    let n = max_bytes.min(o.len) as usize;
    let Ok(buf) = read_at(&mut o, 0, n) else { return };
    if buf.iter().take(8192).any(|b| *b == 0) {
        return;
    }
    let text = String::from_utf8_lossy(&buf);
    for (i, line) in text.lines().enumerate() {
        if line.to_lowercase().contains(needle) {
            let snippet: String = line.trim().chars().take(200).collect();
            hits.push(Hit { path: o.real.clone(), kind: HitKind::Content, line: Some(i as u64 + 1), snippet });
        }
    }
}

fn read_at(o: &mut OpenedFile, offset: u64, n: usize) -> std::io::Result<Vec<u8>> {
    o.file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        let k = o.file.read(&mut buf[got..])?;
        if k == 0 {
            break;
        }
        got += k;
    }
    buf.truncate(got);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confine::folder_id;
    use crate::grants::Grants;
    use crate::testdir::TempDir;

    fn granted(d: &TempDir, rel: &str) -> Grants {
        let (mut g, _) = Grants::open(&d.path().join("grants.jsonl"), "b1").unwrap();
        let root = d.path().join(rel);
        let (r, _) = g.ask("t1", &root, folder_id(&root).unwrap(), "test").unwrap();
        g.grant(&r.request_id, "e", "d").unwrap().unwrap();
        g
    }

    fn text_of(n: usize) -> String {

        let unit = "linha ação 東京 🙂\n";
        unit.repeat(n / unit.chars().count() + 1).chars().take(n).collect()
    }

    #[test]
    fn a_large_file_reads_back_byte_identical_page_by_page_with_nothing_cut() {
        let d = TempDir::new("files-page");
        let text = text_of(100_000);
        d.write("docs/big.txt", &text);
        let g = granted(&d, "docs");
        let c = Confiner { grants: &g, blocked: &[], base: None, home: None };
        let path = d.path().join("docs/big.txt").to_string_lossy().into_owned();
        let (mut all, mut offset, mut pages) = (String::new(), 0u64, 0);
        loop {
            let mut o = c.open_file(&path).unwrap();
            let p = read_page(&mut o, offset, 8_000).unwrap();
            assert!(p.text.chars().count() <= 8_000);
            assert_eq!((p.offset, p.total_bytes, p.more), (offset, text.len() as u64, p.next_offset.is_some()));
            all.push_str(&p.text);
            pages += 1;
            match p.next_offset {
                Some(n) => offset = n,
                None => break,
            }
        }
        assert_eq!(all, text, "every byte, in order");
        assert_eq!(pages, 13, "100k chars in 8k-char pages");
    }

    #[test]
    fn an_offset_inside_a_character_moves_to_the_next_boundary_and_past_the_end_is_an_error() {
        let d = TempDir::new("files-utf8");
        d.write("docs/u.txt", "aé東🙂z");
        let g = granted(&d, "docs");
        let c = Confiner { grants: &g, blocked: &[], base: None, home: None };
        let path = d.path().join("docs/u.txt").to_string_lossy().into_owned();
        let p = read_page(&mut c.open_file(&path).unwrap(), 2, 100).unwrap();
        assert_eq!((p.offset, p.text.as_str()), (3, "東🙂z"), "byte 2 is inside é");
        let p = read_page(&mut c.open_file(&path).unwrap(), 0, 2).unwrap();
        assert_eq!((p.text.as_str(), p.next_offset), ("aé", Some(3)));
        let total = "aé東🙂z".len() as u64;
        assert_eq!(read_page(&mut c.open_file(&path).unwrap(), total, 10).unwrap().text, "", "exactly at the end is an empty last page");
        assert_eq!(read_page(&mut c.open_file(&path).unwrap(), total + 1, 10).unwrap_err(), ReadError::OffsetPastEnd);
    }

    #[test]
    fn binary_files_are_refused() {
        let d = TempDir::new("files-bin");
        std::fs::create_dir_all(d.path().join("docs")).unwrap();
        std::fs::write(d.path().join("docs/b.bin"), [0x89u8, b'P', b'N', b'G', 0, 0, 1]).unwrap();
        std::fs::write(d.path().join("docs/latin1.txt"), [b'c', 0xE7, b'a']).unwrap();
        let g = granted(&d, "docs");
        let c = Confiner { grants: &g, blocked: &[], base: None, home: None };
        for f in ["docs/b.bin", "docs/latin1.txt"] {
            let path = d.path().join(f).to_string_lossy().into_owned();
            assert_eq!(read_page(&mut c.open_file(&path).unwrap(), 0, 100).unwrap_err(), ReadError::Binary, "{f}");
        }
    }

    #[test]
    fn search_finds_names_and_contents_in_order_pages_them_and_never_follows_a_symlink() {
        let d = TempDir::new("files-search");
        d.write("docs/billing/deploy.toml", "port = 7421\n");
        d.write("docs/billing/env/staging.env", "PORT=7421\nREGION=sa-east\n");
        d.write("docs/notes.md", "nothing here\n");
        d.write("outside/leak.txt", "port 9999 outside\n");
        std::os::unix::fs::symlink(d.path().join("outside"), d.path().join("docs/out")).unwrap();
        std::os::unix::fs::symlink(d.path().join("outside/leak.txt"), d.path().join("docs/leak.txt")).unwrap();
        let g = granted(&d, "docs");
        let c = Confiner { grants: &g, blocked: &[], base: None, home: None };
        let dir = c.open_dir(&d.path().join("docs").to_string_lossy()).unwrap();
        let s = search(&c, &dir, "PORT", 0, SearchLimits::default());
        let got: Vec<(String, HitKind, Option<u64>)> = s.hits.iter().map(|h| (h.path.strip_prefix(d.path()).unwrap().to_string_lossy().into_owned(), h.kind, h.line)).collect();
        assert_eq!(got, [("docs/billing/deploy.toml".into(), HitKind::Content, Some(1)), ("docs/billing/env/staging.env".into(), HitKind::Content, Some(1))]);
        assert!(s.hits.iter().all(|h| !h.snippet.contains("9999")), "nothing from behind a symlink");
        let names = search(&c, &dir, "staging", 0, SearchLimits::default());
        assert_eq!(names.hits[0].kind, HitKind::Name);
        let all = search(&c, &dir, "", 0, SearchLimits { page_hits: 2, ..SearchLimits::default() });
        assert_eq!((all.hits.len(), all.next_offset), (2, Some(2)), "an empty query lists files, paged");
        let rest = search(&c, &dir, "", 2, SearchLimits { page_hits: 2, ..SearchLimits::default() });
        assert_eq!((rest.hits.len(), rest.next_offset), (1, None));
        assert_eq!(all.files_scanned, 3, "symlinks are not files of the folder");
        let capped = search(&c, &dir, "", 0, SearchLimits { max_files: 1, ..SearchLimits::default() });
        assert!(capped.scan_limited);
    }
}
