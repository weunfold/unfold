

use crate::dictation::*;
use std::path::{Path, PathBuf};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn read_wav(p: &Path) -> (Vec<f32>, u32) {
    let b = std::fs::read(p).unwrap();
    assert!(&b[..4] == b"RIFF" && &b[8..12] == b"WAVE", "{}", p.display());
    let (mut i, mut fmt, mut ch, mut rate, mut bits) = (12, 0u16, 1u16, 0u32, 0u16);
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    while i + 8 <= b.len() {
        let (id, len) = (&b[i..i + 4], u32_at(i + 4) as usize);
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " {
            (fmt, ch, rate, bits) = (u16_at(i + 8), u16_at(i + 10), u32_at(i + 12), u16_at(i + 22));
        } else if id == b"data" {
            let s: Vec<f32> = match (fmt, bits) {
                (3, 32) => body.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect(),
                (1, 16) => body.chunks_exact(2).map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0).collect(),
                f => panic!("{}: unsupported wav {f:?}", p.display()),
            };
            return (crate::capture::mono(&s, ch as usize, |x| x), rate);
        }
        i += 8 + len + (len & 1);
    }
    panic!("{}: no data chunk", p.display())
}

fn env(k: &str) -> PathBuf {
    PathBuf::from(std::env::var(k).unwrap_or_else(|_| panic!("set {k}")))
}

fn clips(fleurs: &Path, lang: &str) -> Vec<(PathBuf, String)> {
    std::fs::read_to_string(fleurs.join(format!("dl/data/{lang}/test.tsv")))
        .unwrap()
        .lines()
        .take(24)
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (fleurs.join(format!("audio/{lang}/{}", c[1])), c[3].to_string())
        })
        .collect()
}

fn norm(t: &str) -> Vec<String> {
    t.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' || c.is_whitespace() { c } else { ' ' }).collect::<String>().split_whitespace().map(str::to_string).collect()
}

fn edits(a: &[String], b: &[String]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if x == y { prev } else { 1 + prev.min(row[j]).min(row[j + 1]) };
            prev = cur;
        }
    }
    row[b.len()]
}

fn onset(a: &[f32]) -> f32 {
    let n = RATE / 20;
    a.chunks(n).position(|b| level_of(b.iter().map(|&x| f64::from(x) * f64::from(x)).sum(), b.len()) > 0.3).map_or(0.0, |k| (k * n) as f32 / RATE as f32)
}

fn recognizer() -> Arc<sherpa_onnx::OnlineRecognizer> {
    Arc::new(online_recognizer(&env("RTH_DICTATION_MODEL")).expect("the model loads"))
}

fn texts(ev: &[Event]) -> Vec<(u64, String, bool)> {
    ev.iter().filter_map(|e| if let Event::Text { seq, text, is_final, .. } = e { Some((*seq, text.clone(), *is_final)) } else { None }).collect()
}

fn bar(ev: &[Event]) -> String {
    texts(ev).into_iter().filter(|t| t.2).map(|t| t.1).collect::<Vec<_>>().join(" ")
}

#[test]
#[ignore]
fn dictation_fleurs_real_audio_streams_partials_then_finals_in_order() {
    let rec = recognizer();
    let fleurs = env("RTH_DICTATION_FLEURS");
    for (lang, code) in [("pt_br", "pt"), ("en_us", "en")] {
        let (clip, reference) = clips(&fleurs, lang).swap_remove(0);
        let (audio, rate) = read_wav(&clip);
        assert_eq!(rate as usize, RATE);
        let mut s = Session::new(Online::new(rec.clone(), code));
        let mut ev = vec![];

        for c in audio.chunks(RATE / 100) {
            ev.extend(s.feed(c));
        }
        ev.extend(s.stop());
        let t = texts(&ev);
        assert!(t.iter().filter(|x| !x.2).count() >= 3, "{lang}: live words while it is spoken");
        assert!(t.windows(2).all(|w| w[1].0 == w[0].0 + 1), "{lang}: seq grows by one");
        assert!(t.last().unwrap().2, "{lang}: the last text event is a final");
        assert_eq!(ev.last(), Some(&Event::Stopped));

        let (r, h) = (norm(&reference), norm(&bar(&ev)));
        let wer = edits(&r, &h) as f32 / r.len() as f32;

        println!("{lang}: {} text events, {} final(s), WER {:.1}%", t.len(), t.iter().filter(|x| x.2).count(), wer * 100.0);
        assert!(wer < 0.35, "{lang}: WER {wer}");
    }
}

#[test]
#[ignore]
fn dictation_fleurs_cancel_mid_sentence_sends_nothing_after() {
    let rec = recognizer();
    let (clip, _) = clips(&env("RTH_DICTATION_FLEURS"), "pt_br").swap_remove(1);
    let (audio, _) = read_wav(&clip);
    let mut s = Session::new(Online::new(rec, "pt"));
    let mut ev = vec![];
    for c in audio[..audio.len() / 2].chunks(RATE / 100) {
        ev.extend(s.feed(c));
    }
    assert!(!texts(&ev).is_empty(), "words were heard before the cancel");
    assert_eq!(s.cancel(), vec![Event::Cancelled]);
}

#[test]
#[ignore]
fn dictation_fleurs_latency_first_partial_after_speech_starts() {
    let rec = recognizer();
    let fleurs = env("RTH_DICTATION_FLEURS");
    let out = std::env::var("RTH_DICTATION_OUT").ok();
    let mut rows = String::new();
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
        let (mut from_token, mut from_onset, mut rtf, mut worst, mut stop_lag) = (vec![], vec![], vec![], 0f64, vec![]);
        let (mut errs, mut words) = (0, 0);
        for (clip, reference) in clips(&fleurs, lang) {
            let (audio, _) = read_wav(&clip);
            let mut s = Session::new(Online::new(rec.clone(), code));
            let (mut first, mut first_tok, mut compute, mut ev) = (None, None, 0f64, vec![]);
            for (k, c) in audio.chunks(RATE / 100).enumerate() {
                let t0 = Instant::now();
                let e = s.feed(c);
                let dt = t0.elapsed().as_secs_f64();
                compute += dt;
                worst = worst.max(dt);
                if first.is_none() && e.iter().any(|x| matches!(x, Event::Text { .. })) {
                    first = Some((k * RATE / 100 + c.len()) as f64 / RATE as f64 + dt);
                    first_tok = s.recognizer().first_token_s();
                }
                ev.extend(e);
            }
            let t0 = Instant::now();
            ev.extend(s.stop());
            stop_lag.push(t0.elapsed().as_secs_f64());
            let first = first.unwrap();
            let on = onset(&audio) as f64;
            from_onset.push(first - on);
            if let Some(ft) = first_tok {
                from_token.push(first - ft as f64);
            }
            rtf.push(compute / (audio.len() as f64 / RATE as f64));
            let (r, h) = (norm(&reference), norm(&bar(&ev)));
            errs += edits(&r, &h);
            words += r.len();
            rows += &format!(
                "{}\n",
                serde_json::json!({"lang": lang, "clip": clip.file_name().unwrap().to_string_lossy(), "audio_s": audio.len() as f64 / RATE as f64,
                    "first_partial_s": first, "first_token_s": first_tok, "onset_s": on, "partial_after_token_s": first_tok.map(|t| first - t as f64),
                    "partial_after_onset_s": first - on, "stream_rtf": compute / (audio.len() as f64 / RATE as f64), "stop_to_final_s": stop_lag.last()})
            );
        }
        let p50 = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let p90 = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() * 9 / 10]
        };
        println!(
            "{lang}: first partial after first token p50 {:.3} s p90 {:.3} s | after energy onset p50 {:.3} s p90 {:.3} s | stream RTF p50 {:.3} | slowest 10 ms buffer {:.1} ms | stop→final p50 {:.3} s | WER {:.1}% ({} clips)",
            p50(&mut from_token), p90(&mut from_token), p50(&mut from_onset), p90(&mut from_onset), p50(&mut rtf), worst * 1000.0, p50(&mut stop_lag), errs as f64 * 100.0 / words as f64, 24
        );
    }
    if let Some(o) = out {
        std::fs::write(o, rows).unwrap();
    }
}

#[test]
#[ignore]
fn dictation_fleurs_paced_worker_latency() {
    struct FileMic;
    impl Mic for FileMic {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, _sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn std::any::Any>, u32, String), MicError> {
            Ok((Box::new(()), 48_000, "Fake mic, 1 ch".into()))
        }
    }
    struct One(Arc<sherpa_onnx::OnlineRecognizer>);
    impl Models for One {
        type R = Online;
        fn state(&self) -> ModelState {
            ModelState::Ready
        }
        fn load(&mut self, lang: &str) -> Result<Online, String> {
            Ok(Online::new(self.0.clone(), lang))
        }
    }
    let rec = recognizer();
    let fleurs = env("RTH_DICTATION_FLEURS");
    let mut lat = vec![];
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
        for (clip, _) in clips(&fleurs, lang).into_iter().take(6) {
            let (audio, _) = read_wav(&clip);

            let mut probe = Session::new(Online::new(rec.clone(), code));
            let mut ft = None;
            for c in audio.chunks(RATE / 100) {
                if !probe.feed(c).is_empty() && ft.is_none() {
                    ft = probe.recognizer().first_token_s();
                }
            }
            let ft = ft.unwrap() as f64;
            let up: Vec<f32> = (0..audio.len() * 3)
                .map(|i| {
                    let (k, f) = (i / 3, (i % 3) as f32 / 3.0);
                    audio[k] * (1.0 - f) + audio.get(k + 1).copied().unwrap_or(0.0) * f
                })
                .collect();
            let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(512);
            let stamps: Arc<std::sync::Mutex<Vec<(Instant, Event)>>> = Arc::default();

            let (tx2, _unused) = std::sync::mpsc::sync_channel::<Msg>(1);
            let (st, r2, code2) = (stamps.clone(), rec.clone(), code.to_string());
            let worker = std::thread::spawn(move || {
                let w = Worker::new(
                    FileMic,
                    One(r2),
                    Box::new(move |e: &Event| st.lock().unwrap().push((Instant::now(), e.clone()))),
                    tx2,
                    Arc::new(std::sync::atomic::AtomicBool::new(true)),
                    Box::new(resampler),
                );
                w.run(rx, Duration::from_secs(60));
            });
            tx.send(Msg::Start { lang: code2 }).unwrap();
            let t0 = Instant::now();
            for (k, c) in up.chunks(512).enumerate() {

                let due = t0 + Duration::from_secs_f64(((k + 1) * 512) as f64 / 48_000.0);
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                tx.send(Msg::Audio(c.to_vec())).unwrap();
            }
            tx.send(Msg::Stop).unwrap();
            drop(tx);
            worker.join().unwrap();
            let first = stamps.lock().unwrap().iter().find(|(_, e)| matches!(e, Event::Text { .. })).map(|(t, _)| t.duration_since(t0).as_secs_f64()).unwrap();
            lat.push(first - ft);
        }
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("paced worker (48 kHz, 12 clips): first partial after first token p50 {:.3} s, max {:.3} s", lat[lat.len() / 2], lat[lat.len() - 1]);
}

#[test]
#[ignore]
fn dictation_fleurs_48khz_round_trip_costs_little() {
    let rec = recognizer();
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
        let (mut e0, mut e1, mut n) = (0, 0, 0);
        for (clip, reference) in clips(&env("RTH_DICTATION_FLEURS"), lang) {
            let (audio, _) = read_wav(&clip);
            let up = sherpa_onnx::LinearResampler::create(16_000, 48_000).unwrap().resample(&audio, true);
            let mut lin = resampler(48_000).unwrap();
            let down: Vec<f32> = up.chunks(512).flat_map(|c| lin.to_16k(c)).collect();
            let run = |a: &[f32]| {
                let mut s = Session::new(Online::new(rec.clone(), code));
                let mut ev = vec![];
                for c in a.chunks(160) {
                    ev.extend(s.feed(c));
                }
                ev.extend(s.stop());
                norm(&bar(&ev))
            };
            let r = norm(&reference);
            e0 += edits(&r, &run(&audio));
            e1 += edits(&r, &run(&down));
            n += r.len();
        }
        let (w0, w1) = (e0 as f64 * 100.0 / n as f64, e1 as f64 * 100.0 / n as f64);
        println!("{lang}: WER 16 kHz {w0:.1}% | 48 kHz round trip {w1:.1}%");
        assert!(w1 - w0 < 2.0, "{lang}: {w0} → {w1}");
    }
}

#[test]
#[ignore]
fn dictation_real_install_downloads_resumes_and_verifies() {
    use crate::dictation_models::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    let data = env("RTH_DICTATION_DATA");
    let id = std::env::var("RTH_DICTATION_INSTALL").unwrap_or_else(|_| "parakeet-tdt-0.6b-v3-int8".into());
    let m = model(catalog(), &id).unwrap();
    if !installed(&data, m) {
        let cancel = AtomicBool::new(false);
        let r = install(&data, &id, &Curl::default(), &cancel, &|s| {
            if matches!(s, Step::Bytes { done, .. } if done > 50_000_000) {
                cancel.store(true, Ordering::SeqCst)
            }
        });
        assert_eq!(r.err(), Some(Failure::Cancelled));
        let kept = partial_bytes(&data, m);
        assert!(kept > 50_000_000, "{kept}");
        let t0 = Instant::now();
        install(&data, &id, &Curl::default(), &AtomicBool::new(false), &|_| {}).unwrap();
        println!("{id}: resumed from {:.0} MB, the rest in {:.0} s, all {} files verified", kept as f64 / 1e6, t0.elapsed().as_secs_f64(), m["files"].as_array().unwrap().len());
    }
    assert!(installed(&data, m));
    let d = dir(&data, &id);
    for f in ["NOTICE.txt", m["licence_file"].as_str().unwrap(), ".verified"] {
        assert!(d.join(f).is_file(), "{f}");
    }
}

#[test]
#[ignore]
fn dictation_fleurs_parakeet_phrases_at_each_pause() {
    use crate::dictation_models::*;
    let data = env("RTH_DICTATION_DATA");
    set_default(&data, "parakeet-tdt-0.6b-v3-int8").unwrap();
    let mut models = Installed::new(data);
    assert_eq!(models.state(), ModelState::Ready);
    let fleurs = env("RTH_DICTATION_FLEURS");
    for (lang, code) in [("pt_br", "pt"), ("en_us", "en")] {
        let (mut errs, mut words, mut lag) = (0, 0, vec![]);
        for (clip, reference) in clips(&fleurs, lang).into_iter().take(8) {
            let (audio, _) = read_wav(&clip);
            let mut s = Session::new(models.load(code).unwrap());
            let mut ev = vec![];
            for c in audio.chunks(RATE / 100) {
                ev.extend(s.feed(c));
            }
            let t0 = Instant::now();
            ev.extend(s.stop());
            lag.push(t0.elapsed().as_secs_f64());
            assert!(texts(&ev).iter().any(|t| t.2), "{lang}: no phrase ended");
            let r = norm(&reference);
            let e = edits(&r, &norm(&bar(&ev)));
            if std::env::var_os("RTH_DICTATION_SHOW").is_some() {

                println!("{lang} {e}/{} | ref: {} | phrases: {:?}", r.len(), r.join(" "), texts(&ev).iter().map(|t| t.1.clone()).collect::<Vec<_>>());
            }
            errs += e;
            words += r.len();
        }
        lag.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("parakeet {lang}: WER {:.1}% on 8 clips, stop→text p50 {:.2} s", errs as f64 * 100.0 / words as f64, lag[lag.len() / 2]);
    }
}

#[test]
#[ignore]
fn dictation_fleurs_wer_like_the_bench() {
    let dir = env("RTH_DICTATION_MODEL");
    let (on, off) = (Arc::new(online_recognizer_with(&dir, true).unwrap()), Arc::new(online_recognizer_with(&dir, false).unwrap()));
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {

        let mut t = [(0, 0); 4];
        for (clip, reference) in clips(&env("RTH_DICTATION_FLEURS"), lang) {
            let (audio, _) = read_wav(&clip);
            let run = |rec: &Arc<sherpa_onnx::OnlineRecognizer>| {
                let mut s = Session::new(Online::new(rec.clone(), code));
                let mut ev = vec![];
                for c in audio.chunks(2560) {
                    ev.extend(s.feed(c));
                }
                ev.extend(s.stop());
                norm(&bar(&ev))
            };
            let r = norm(&reference);
            let digits = reference.chars().any(|c| c.is_ascii_digit());
            for (k, h) in [(0, run(&on)), (2, run(&off))] {
                let e = edits(&r, &h);
                t[k] = (t[k].0 + e, t[k].1 + r.len());
                if !digits {
                    t[k + 1] = (t[k + 1].0 + e, t[k + 1].1 + r.len());
                }
            }
        }
        let w = |(e, n): (usize, usize)| e as f64 * 100.0 / n as f64;
        println!("{lang}: endpoints on (app) WER all {:.1}% / no-digit {:.1}% | off (bench) all {:.1}% / no-digit {:.1}%", w(t[0]), w(t[1]), w(t[2]), w(t[3]));
    }
}

#[test]
#[ignore]
fn dictation_settings_test_button_transcribes_the_bundled_clips() {
    let cat = crate::dictation_models::catalog();
    let m = crate::dictation_models::model(cat, &std::env::var("RTH_DICTATION_TEST_ID").unwrap_or_else(|_| "nemotron-3.5-streaming-0.6b-int8".into())).unwrap();
    let dir = std::path::PathBuf::from(env("RTH_DICTATION_MODEL"));
    for pt in [false, true] {
        let r = crate::dictation::run_test(&dir, m, &dir, pt).unwrap();
        println!("{}", serde_json::json!({"clip": r["clip"], "seconds": r["seconds"], "load_s": r["load_s"], "audio_s": r["audio_s"], "text": r["text"]}));
        let want = if pt {
            "logo após o início das hostilidades a grã-bretanha começou um bloqueio naval à alemanha"
        } else {
            "unfortunately studying traffic flow is difficult because driver behavior cannot be predicted with one hundred percent certainty"
        };
        let (h, w) = (norm(r["text"].as_str().unwrap()), norm(want));
        assert!(edits(&w, &h) * 10 <= w.len(), "{pt}: {h:?}");
        assert!(r["seconds"].as_f64().unwrap() < r["audio_s"].as_f64().unwrap());
    }
}

#[test]
#[ignore]
fn nemo_real_sidecar_streams_a_clip() {
    use crate::nemo::*;
    let home = std::env::temp_dir().join(format!("rth-nemo-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let bin = std::path::PathBuf::from(env("RTH_NEMO_BIN"));
    let mut launch = Serve { bin: bin.clone(), model: env("RTH_NEMO_MODEL").into(), itn: std::env::var_os("RTH_NEMO_ITN").map(Into::into), home: home.clone(), warmup: std::env::var_os("RTH_NEMO_NO_WARMUP").is_none(), sandbox: std::env::var_os("RTH_NEMO_NO_SANDBOX").is_none() };
    let t0 = Instant::now();
    let side = Sidecar::start(Start { launch: &mut launch, pidfile: home.join("pid"), bin, ready_within: Duration::from_secs(60), port: &mut free_port, closing: &Default::default() }, &|l| eprintln!("{l}")).unwrap();
    eprintln!("ready in {:.2} s", t0.elapsed().as_secs_f64());
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
        let (clip, reference) = clips(&env("RTH_DICTATION_FLEURS"), lang).into_iter().next().unwrap();
        let (audio, _) = read_wav(&clip);
        let mut s = Session::new(side.stream(code).unwrap());
        let mut ev = vec![];
        let t1 = Instant::now();
        let mut first = None;
        for c in audio.chunks(RATE / 100) {
            let e = s.feed(c);
            if first.is_none() && texts(&e).iter().any(|x| !x.1.is_empty()) {
                first = Some(t1.elapsed().as_secs_f64());
            }
            ev.extend(e);
        }
        eprintln!("{lang}: first words {first:?} s after the first sample (unpaced), all audio sent in {:.2} s", t1.elapsed().as_secs_f64());
        ev.extend(s.stop());
        let t = texts(&ev);
        eprintln!("{lang}: {} events, finals {:?}\n  ref: {reference}", t.len(), t.iter().filter(|x| x.2).map(|x| x.1.clone()).collect::<Vec<_>>());
        assert!(t.iter().any(|x| x.2));
    }

    let pid = side.pid().to_string();
    let lsof = std::process::Command::new("/usr/sbin/lsof").args(["-a", "-p", &pid, "-i", "-n", "-P"]).output().unwrap();
    let socks = String::from_utf8_lossy(&lsof.stdout).to_string();
    eprintln!("sockets of pid {pid}:\n{socks}");
    assert!(socks.lines().skip(1).all(|l| l.contains("127.0.0.1")), "a socket beyond loopback");
    drop(side);
    let left: Vec<_> = std::fs::read_dir(&home).unwrap().flatten().map(|e| e.file_name()).collect();
    eprintln!("left in its HOME: {left:?}");
    let _ = std::fs::remove_dir_all(&home);
}

struct LocalFetch(Vec<PathBuf>);

impl crate::dictation_models::Fetch for LocalFetch {
    fn fetch(&self, url: &str, part: &Path, _c: &std::sync::atomic::AtomicBool, progress: &dyn Fn(u64), _r: &dyn Fn(u32, u32)) -> Result<(), String> {
        let name = url.rsplit('/').next().unwrap_or("");
        let src = self.0.iter().map(|d| d.join(name)).find(|p| p.exists()).ok_or(format!("no local {name}"))?;
        let _ = std::fs::remove_file(part);
        let ok = std::process::Command::new("/bin/cp").arg("-c").arg(&src).arg(part).status().map_err(|e| e.to_string())?.success();
        progress(std::fs::metadata(part).map(|m| m.len()).unwrap_or(0));
        ok.then_some(()).ok_or("cp failed".into())
    }
}

#[test]
#[ignore]
fn nemo_real_install_from_local_copies() {
    let data = env("RTH_DICTATION_DATA");
    let fetch = LocalFetch(std::env::var("RTH_LOCAL_SOURCES").unwrap().split(':').map(PathBuf::from).collect());
    for id in std::env::var("RTH_DICTATION_INSTALL_IDS").unwrap().split(',') {
        let d = crate::dictation_models::install(&data, id, &fetch, &std::sync::atomic::AtomicBool::new(false), &|_| {}).unwrap();
        eprintln!("{id}: installed at {}", d.display());
    }
}

#[test]
#[ignore]
fn nemo_real_app_path_each_model_and_the_fallback() {
    use crate::dictation_models::*;
    let data = env("RTH_DICTATION_DATA");
    let fleurs = env("RTH_DICTATION_FLEURS");
    let ids: Vec<String> = std::env::var("RTH_DICTATION_IDS").unwrap().split(',').map(String::from).collect();
    let mut models = Installed::new(data.clone());
    let logged = Arc::new(std::sync::Mutex::new(vec![]));
    let l2 = logged.clone();
    models.log = Box::new(move |l| l2.lock().unwrap().push(l.to_string()));
    for id in &ids {
        set_default(&data, id).unwrap();
        for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
            if id.contains("-en") && code == "pt" {
                continue;
            }
            let (clip, reference) = clips(&fleurs, lang).into_iter().nth(1).unwrap();
            let (audio, _) = read_wav(&clip);
            let t0 = Instant::now();
            let mut s = Session::new(models.load(code).unwrap());
            let load = t0.elapsed().as_secs_f64();
            let mut ev = vec![];
            for c in audio.chunks(RATE / 100) {
                ev.extend(s.feed(c));
            }
            assert!(s.failed().is_none());
            ev.extend(s.stop());
            let t = texts(&ev);
            let (h, r) = (norm(&bar(&ev)), norm(&reference));
            println!("{id} {lang}: load {load:.2} s, {} partials, {} finals, {}/{} word errors | {}", t.iter().filter(|x| !x.2).count(), t.iter().filter(|x| x.2).count(), edits(&r, &h), r.len(), bar(&ev));
            assert!(edits(&r, &h) * 4 <= r.len(), "{id} {lang}");
        }
        let m = model(catalog(), id).unwrap();
        let r = crate::dictation::run_test(&data, m, &dir(&data, id), true).unwrap();
        println!("{id} Test button: {}", serde_json::json!({"clip": r["clip"], "seconds": r["seconds"], "load_s": r["load_s"], "text": r["text"]}));
    }
    models.unload();
    println!("log: {:?}", logged.lock().unwrap());
}

#[test]
#[ignore]
fn nemo_real_wer_and_latency_on_the_app_path() {
    let data = env("RTH_DICTATION_DATA");
    let fleurs = env("RTH_DICTATION_FLEURS");
    let paced: usize = std::env::var("RTH_DICTATION_PACED").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let mut out = std::fs::File::create(env("RTH_DICTATION_OUT")).unwrap();
    let mut models = Installed::new(data.clone());
    let id = crate::dictation_models::default_id(&data);
    let skip_wer = std::env::var_os("RTH_DICTATION_SKIP_WER").is_some();
    let langs: Vec<(&str, &str)> = [("en_us", "en"), ("pt_br", "pt")].into_iter().filter(|l| std::env::var("RTH_DICTATION_LANGS").map_or(true, |v| v.split(',').any(|x| x == l.1))).collect();
    for (lang, code) in langs {
        let mut t = [(0, 0); 2];
        for (clip, reference) in clips(&fleurs, lang).into_iter().take(if skip_wer { 0 } else { usize::MAX }) {
            let (audio, _) = read_wav(&clip);
            let mut s = Session::new(models.load(code).unwrap());
            let mut ev = vec![];
            for c in audio.chunks(RATE / 100) {
                ev.extend(s.feed(c));
            }
            ev.extend(s.stop());
            let (r, h) = (norm(&reference), norm(&bar(&ev)));
            let e = edits(&r, &h);
            t[0] = (t[0].0 + e, t[0].1 + r.len());
            if !reference.chars().any(|c| c.is_ascii_digit()) {
                t[1] = (t[1].0 + e, t[1].1 + r.len());
            }
        }
        let w = |(e, n): (usize, usize)| (e as f64 * 1000.0 / n as f64).round() / 10.0;
        if !skip_wer {
            let line = serde_json::json!({"model": id, "lang": lang, "wer_all": w(t[0]), "wer_nodigit": w(t[1]), "words": t[0].1});
            println!("{line}");
            writeln!(out, "{line}").unwrap();
        }
        let (mut first, mut fin, mut slowest) = (vec![], vec![], vec![]);
        for (clip, _) in clips(&fleurs, lang).into_iter().take(paced) {
            let (audio, _) = read_wav(&clip);
            let on = (onset(&audio) * RATE as f32) as usize;
            let mut s = Session::new(models.load(code).unwrap());
            let t0 = Instant::now();
            let mut got = None;
            for (i, c) in audio.chunks(RATE / 100).enumerate() {

                let due = t0 + Duration::from_millis(10 * i as u64);
                if let Some(d) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(d);
                }
                let e = s.feed(c);
                if got.is_none() && texts(&e).iter().any(|x| !x.1.trim().is_empty()) {
                    got = Some(t0.elapsed().as_secs_f64() - on as f64 / RATE as f64);
                }
            }
            let t1 = Instant::now();
            let _ = s.stop();
            fin.push(t1.elapsed().as_secs_f64());
            if let Some(d) = s.recognizer().slowest_pass() {
                slowest.push(d.as_secs_f64());
            }
            if let Some(g) = got {
                first.push(g);
            }
        }
        let p = |v: &mut Vec<f64>, q: f64| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (v[((v.len() - 1) as f64 * q).round() as usize] * 1000.0).round() / 1000.0
        };
        let line = serde_json::json!({"model": id, "lang": lang, "paced_clips": paced, "with_words": first.len(), "onset_to_first_words_p50": p(&mut first, 0.5), "p90": p(&mut first, 0.9), "stop_to_final_p50": p(&mut fin, 0.5), "stop_to_final_max": p(&mut fin, 1.0), "clips_with_passes": slowest.len(), "slowest_pass_max": if slowest.is_empty() { 0.0 } else { p(&mut slowest, 1.0) }});
        println!("{line}");
        writeln!(out, "{line}").unwrap();
    }
    models.unload();
}

#[test]
#[ignore]
fn nemo_real_sidecar_paced_worker_shows_live_words() {
    struct FileMic;
    impl Mic for FileMic {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, _sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn std::any::Any>, u32, String), MicError> {
            Ok((Box::new(()), 48_000, "Fake mic, 1 ch".into()))
        }
    }
    struct Running(u16, String, bool);
    impl Models for Running {
        type R = Box<dyn Recognizer>;
        fn state(&self) -> ModelState {
            ModelState::Ready
        }
        fn load(&mut self, lang: &str) -> Result<Box<dyn Recognizer>, String> {
            let s = crate::nemo::Stream::open(self.0, &self.1, lang)?;
            if !self.2 {
                return Ok(Box::new(s));
            }
            let (port, key, lang) = (self.0, self.1.clone(), lang.to_string());
            Ok(Box::new(crate::dictation_settle::Settled::new(s, move |a: &[f32], _: bool| crate::nemo::transcribe_words(port, &key, &lang, a, Duration::from_secs(30)))))
        }
    }
    let port: u16 = std::env::var("RTH_NEMO_PORT").expect("set RTH_NEMO_PORT").parse().unwrap();
    let key = std::env::var("RTH_NEMO_KEY").expect("set RTH_NEMO_KEY");
    let settle = std::env::var_os("RTH_NEMO_SETTLE").is_some();
    let fleurs = env("RTH_DICTATION_FLEURS");
    let mut silent = vec![];
    for (lang, code) in [("pt_br", "pt"), ("en_us", "en")] {
        for (clip, reference) in clips(&fleurs, lang).into_iter().take(2) {
            let (audio, _) = read_wav(&clip);
            let up: Vec<f32> = (0..audio.len() * 3)
                .map(|i| {
                    let (k, f) = (i / 3, (i % 3) as f32 / 3.0);
                    audio[k] * (1.0 - f) + audio.get(k + 1).copied().unwrap_or(0.0) * f
                })
                .collect();
            let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(512);
            let stamps: Arc<std::sync::Mutex<Vec<(Instant, Event)>>> = Arc::default();
            let (tx2, _unused) = std::sync::mpsc::sync_channel::<Msg>(1);
            let (st, key2, code2) = (stamps.clone(), key.clone(), code.to_string());
            let worker = std::thread::spawn(move || {
                let w = Worker::new(FileMic, Running(port, key2, settle), Box::new(move |e: &Event| st.lock().unwrap().push((Instant::now(), e.clone()))), tx2, Arc::new(std::sync::atomic::AtomicBool::new(true)), Box::new(resampler));
                w.run(rx, Duration::from_secs(60));
            });
            tx.send(Msg::Start { lang: code2 }).unwrap();
            let t0 = Instant::now();
            for (k, c) in up.chunks(512).enumerate() {
                let due = t0 + Duration::from_secs_f64(((k + 1) * 512) as f64 / 48_000.0);
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                tx.send(Msg::Audio(c.to_vec())).unwrap();
            }
            let stop_at = Instant::now();
            tx.send(Msg::Stop).unwrap();
            drop(tx);
            worker.join().unwrap();
            let ev = stamps.lock().unwrap().clone();
            let live: Vec<&(Instant, Event)> = ev.iter().filter(|(t, e)| *t < stop_at && matches!(e, Event::Text { is_final: false, text, .. } if !text.is_empty())).collect();
            let first = live.first().map(|(t, _)| t.duration_since(t0).as_secs_f64());
            let finals: Vec<String> = ev.iter().filter_map(|(_, e)| if let Event::Text { is_final: true, text, .. } = e { Some(text.clone()) } else { None }).collect();
            let last_live = live.last().map(|(_, e)| if let Event::Text { text, .. } = e { text.clone() } else { String::new() });
            println!(
                "{lang} {}: {:.1} s audio, {} live partials before Stop, first at {first:?} s, levels {}, last live {last_live:?}\n  finals {finals:?}\n  ref    {reference}",
                clip.file_name().unwrap().to_string_lossy(),
                audio.len() as f64 / RATE as f64,
                live.len(),
                ev.iter().filter(|(_, e)| matches!(e, Event::Level(_))).count()
            );
            if live.is_empty() {
                silent.push(clip.display().to_string());
            }
        }
    }
    assert!(silent.is_empty(), "no live words for {silent:?}");
}

#[test]
#[ignore]
fn dictation_cpu_model_real_time_factor_on_clips() {
    let dir = env("RTH_DICTATION_MODEL");
    let limit: f64 = std::env::var("RTH_DICTATION_RTF_BAR").map_or(0.25, |b| b.parse().expect("RTH_DICTATION_RTF_BAR is a number"));
    let t0 = Instant::now();
    let rec = Arc::new(online_recognizer(&dir).unwrap());
    println!("{}: loaded in {:.2} s", dir.display(), t0.elapsed().as_secs_f64());
    let mut rtf = vec![];
    for clip in std::env::var("RTH_DICTATION_CLIPS").expect("set RTH_DICTATION_CLIPS").split(':') {
        let (audio, rate) = read_wav(Path::new(clip));
        assert_eq!(rate as usize, RATE, "{clip}");
        let mut s = Session::new(Online::new(rec.clone(), ""));
        let mut ev = vec![];
        let t1 = Instant::now();
        for c in audio.chunks(RATE / 100) {
            ev.extend(s.feed(c));
        }
        ev.extend(s.stop());
        let r = t1.elapsed().as_secs_f64() / (audio.len() as f64 / RATE as f64);
        rtf.push(r);
        let partials = texts(&ev).iter().filter(|x| !x.2).count();
        println!("{}: {:.1} s, {partials} partials, RTF {r:.3}\n  {:?}", Path::new(clip).file_name().unwrap().to_string_lossy(), audio.len() as f64 / RATE as f64, bar(&ev));
    }
    rtf.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = rtf[rtf.len() / 2];
    println!("median RTF {median:.3} (bar {limit})");
    assert!(median <= limit, "median RTF {median:.3} is over the bar {limit}");
}

#[test]
#[ignore]
fn nemo_real_first_stream_after_a_start_ends_in_time() {
    let res = env("RTH_NEMO_RES");
    let model = env("RTH_NEMO_MODEL");
    let home = std::env::temp_dir().join(format!("rth-nemo-first-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let h2 = home.clone();
    std::thread::spawn(move || {
        let bin = res.join("bin/nemo-speech");
        let (b2, itn, h3) = (bin.clone(), res.join("itn"), h2.clone());
        let launch = move |_: &str| -> Result<Box<dyn crate::nemo::Launch>, String> {
            Ok(Box::new(crate::nemo::Serve { bin: b2.clone(), model: model.clone(), itn: Some(itn.clone()), home: h3.clone(), warmup: false, sandbox: true }))
        };
        let mut sup = crate::nemo::Supervisor::new(h2.join("ledger"), bin, Box::new(launch), Box::new(|l: &str| println!("{l}")));
        let t0 = Instant::now();
        let side = match sup.sidecar("nemotron-3.5-streaming-0.6b-q8") {
            Ok(s) => s,
            Err(e) => return tx.send(format!("start failed: {e}")).unwrap(),
        };
        let _ = tx.send(format!("ready in {:.1} s (pid {})", t0.elapsed().as_secs_f64(), side.pid()));
        let t1 = Instant::now();
        let r = side.settled_stream("pt-BR").map(|_| ());
        let _ = tx.send(format!("open ended in {:.1} s: {r:?}", t1.elapsed().as_secs_f64()));
        let _ = stop_rx.recv_timeout(Duration::from_secs(5));
        sup.stop();
    });
    let ready = rx.recv_timeout(Duration::from_secs(90)).expect("the sidecar start ended (90 s)");
    println!("{ready}");
    assert!(ready.starts_with("ready"), "{ready}");
    let open = rx.recv_timeout(Duration::from_secs(15));
    println!("{open:?}");
    let _ = stop_tx.send(());
    std::thread::sleep(Duration::from_secs(1));
    let _ = std::fs::remove_dir_all(&home);
    assert!(open.is_ok(), "the first stream's open did not end within 15 s: the worker would hang");
}

#[test]
#[ignore]
fn nemo_real_offline_model_live_words_paced() {
    let port: u16 = std::env::var("RTH_NEMO_PORT").expect("set RTH_NEMO_PORT").parse().unwrap();
    let key = std::env::var("RTH_NEMO_KEY").expect("set RTH_NEMO_KEY");
    let fleurs = env("RTH_DICTATION_FLEURS");
    let paced: usize = std::env::var("RTH_DICTATION_PACED").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let today = |lang: &str| {
        let (k, l) = (key.clone(), lang.to_string());
        Phrases::new(move |a: &[f32]| crate::nemo::transcribe(port, &k, &l, a, crate::nemo::LAST_PASS_WITHIN).unwrap_or_default())
    };
    let mut bad = vec![];
    for (lang, code) in [("en_us", "en"), ("pt_br", "pt")] {
        for (clip, _) in clips(&fleurs, lang).into_iter().take(paced) {
            let (audio, _) = read_wav(&clip);
            let on = onset(&audio) as f64;
            let mut s = Session::new(crate::dictation::posted(port, key.clone(), code));
            let (t0, mut first, mut ev) = (Instant::now(), None, vec![]);
            for (i, c) in audio.chunks(RATE / 100).enumerate() {
                let due = t0 + Duration::from_millis(10 * i as u64);
                if let Some(d) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(d);
                }
                let e = s.feed(c);
                if first.is_none() && e.iter().any(|x| matches!(x, Event::Text { is_final: false, text, .. } if !text.trim().is_empty())) {
                    first = Some(t0.elapsed().as_secs_f64() - on);
                }
                ev.extend(e);
            }
            let t1 = Instant::now();
            ev.extend(s.stop());
            let stop = t1.elapsed().as_secs_f64();
            let slowest = s.recognizer().slowest_pass().map(|d| d.as_secs_f64());
            let off = s.recognizer().settling_off_at().is_some();
            let mut today = Session::new(today(code));
            let mut ev2 = vec![];
            for c in audio.chunks(RATE / 100) {
                ev2.extend(today.feed(c));
            }
            ev2.extend(today.stop());
            let finals = |ev: &[Event]| texts(ev).into_iter().filter(|t| t.2).map(|t| t.1).collect::<Vec<_>>();
            let same = finals(&ev) == finals(&ev2);

            let key = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
            let (mut settled, mut respelled) = (vec![], 0);
            for e in &ev {
                match e {
                    Event::Text { text, settled: Some(n), is_final: false, .. } => settled = text.split_whitespace().take(*n).map(key).collect(),
                    Event::Text { text, is_final: true, .. } => {
                        respelled += usize::from(!text.split_whitespace().map(key).take(settled.len()).eq(settled.iter().cloned()));
                        settled.clear();
                    }
                    _ => {}
                }
            }
            println!(
                "{}",
                serde_json::json!({"lang": lang, "clip": clip.file_name().unwrap().to_string_lossy(), "audio_s": audio.len() as f64 / RATE as f64, "onset_to_first_words_s": first, "partials": texts(&ev).iter().filter(|t| !t.2).count(), "slowest_pass_s": slowest, "passes_off": off, "stop_to_final_s": stop, "finals_as_without_passes": same, "finals_respelling_settled_words": respelled})
            );
            if first.is_none() {
                bad.push(clip.display().to_string());
            }
        }
    }
    assert!(bad.is_empty(), "no live words: {bad:?}");
}

#[test]
#[ignore]
fn stt_quality_pipeline_matrix() {
    use crate::nemo::{Launch, Serve};
    use std::sync::atomic::AtomicBool;
    let lane = env("UNFOLD_STT_TEST_SET");
    let model = std::env::var("RTH_STT_MODEL").unwrap();
    let kind = std::env::var("RTH_STT_KIND").unwrap();
    let res = env("RTH_NEMO_RES");
    let home = lane.join(format!("home-{model}"));
    std::fs::create_dir_all(&home).unwrap();
    let bin = res.join("bin/nemo-speech");
    let model_path = env("RTH_STT_WEIGHTS");
    let mut launch = Serve { bin: bin.clone(), model: model_path, itn: Some(res.join("itn")), home: home.clone(), warmup: false, sandbox: true };
    let closing = AtomicBool::new(false);
    let side = crate::nemo::Sidecar::start(crate::nemo::Start {
        launch: &mut launch as &mut dyn Launch, pidfile: home.join("ledger"), bin,
        ready_within: Duration::from_secs(60), port: &mut crate::nemo::free_port, closing: &closing,
    }, &|s| println!("{s}")).unwrap();
    let (port, key) = side.endpoint();
    struct FileMic(u32);
    impl Mic for FileMic {
        fn permission(&self) -> Permission { Permission::Granted }
        fn ask(&self, _: Box<dyn FnOnce(bool) + Send>) {}
        fn open(&mut self, _: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn std::any::Any>, u32, String), MicError> {
            Ok((Box::new(()), self.0, "Recorded public speech".into()))
        }
    }
    struct Running(u16, String, bool);
    impl Models for Running {
        type R = Box<dyn Recognizer>;
        fn state(&self) -> ModelState { ModelState::Ready }
        fn load(&mut self, lang: &str) -> Result<Self::R, String> {
            if self.2 { return Ok(crate::dictation::posted(self.0, self.1.clone(), lang)); }
            let live = crate::nemo::Stream::open_with_endpoint(self.0, &self.1, lang, Some(900))?;
            let (port, key, lang) = (self.0, self.1.clone(), lang.to_string());
            Ok(Box::new(crate::dictation_settle::Settled::new(live, move |a: &[f32], last: bool| {
                crate::nemo::transcribe_words(port, &key, &lang, a, if last { crate::nemo::LAST_PASS_WITHIN } else { crate::nemo::PASS_WITHIN })
            })))
        }
    }
    let rows: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(lane.join("selected.json")).unwrap()).unwrap();
    let mut out = std::fs::OpenOptions::new().create(true).append(true).open(env("RTH_DICTATION_OUT")).unwrap();
    for row in rows {
        let (lang, clip, reference) = (row["lang"].as_str().unwrap(), row["clip"].as_str().unwrap(), row["ref"].as_str().unwrap());
        let code = if lang == "pt_br" { "pt" } else { "en" };
        let variants = std::env::var("RTH_STT_VARIANTS").unwrap_or_else(|_| "live48,offline48,offline_hq,offline_native,offline_auto".into());
        let (audio48, _) = read_wav(&lane.join(format!("audio/{lang}/{clip}.48k.wav")));
        let (hq, _) = read_wav(&lane.join(format!("audio/{lang}/{clip}.hq.wav")));
        let (native, _) = read_wav(&env("RTH_STT_AUDIO").join(lang).join(clip));
        let mut down = resampler(48_000).unwrap();
        let audio16: Vec<f32> = audio48.chunks(512).flat_map(|c| down.to_16k(c)).collect();
        for variant in variants.split(',') {
            let now = Instant::now();
            let (text, partials, finals, pass_off) = if variant.starts_with("live") || variant == "send48" {
                let (audio, rate) = if variant == "live_hq" { (&hq, 16_000u32) } else { (&audio48, 48_000u32) };
                let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(512);
                let (dummy, _unused) = std::sync::mpsc::sync_channel(1);
                let events: Arc<std::sync::Mutex<Vec<Event>>> = Arc::default();
                let logs: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
                let (e2, l2, k2, offline) = (events.clone(), logs.clone(), key.clone(), kind == "offline");
                let worker = std::thread::spawn(move || {
                    let mut w = Worker::new(FileMic(rate), Running(port, k2, offline), Box::new(move |e| e2.lock().unwrap().push(e.clone())), dummy, Arc::new(AtomicBool::new(true)), Box::new(resampler));
                    w.log = Box::new(move |s| l2.lock().unwrap().push(s.to_string()));
                    w.run(rx, Duration::from_secs(60));
                });
                tx.send(Msg::Start { lang: code.into() }).unwrap();
                let t0 = Instant::now();
                let chunk = if rate == 48_000 { 512 } else { 171 };
                for (i, c) in audio.chunks(chunk).enumerate() {
                    let due = t0 + Duration::from_secs_f64(((i + 1) * chunk) as f64 / rate as f64);
                    if let Some(d) = due.checked_duration_since(Instant::now()) { std::thread::sleep(d); }
                    tx.send(Msg::Audio(c.to_vec())).unwrap();
                }
                let cancel_send = variant == "send48" && std::env::var_os("RTH_STT_CANCEL_SEND").is_some();
                let submitted = if cancel_send {
                    let (ack, ready) = std::sync::mpsc::channel();
                    tx.send(Msg::Checkpoint(Box::new(move || { ack.send(()).unwrap(); }))).unwrap();
                    ready.recv_timeout(Duration::from_secs(60)).unwrap();
                    let mut closed = Vec::new();
                    let mut partial = String::new();
                    for (_, text, final_) in texts(&events.lock().unwrap()) {
                        if final_ { closed.push(text); partial.clear(); } else { partial = text; }
                    }
                    if !partial.is_empty() { closed.push(partial); }
                    Some(closed.join(" "))
                } else { None };
                tx.send(if cancel_send { Msg::Cancel } else { Msg::Stop }).unwrap(); drop(tx); worker.join().unwrap();
                let events = events.lock().unwrap();
                assert!(!events.iter().any(|e| matches!(e, Event::Error(_))), "{events:?}");
                let partials = texts(&events).iter().filter(|t| !t.2).count();
                let finals = texts(&events).iter().filter(|t| t.2).count();
                { let off = logs.lock().unwrap().iter().any(|s| s.contains("settling off")); (submitted.unwrap_or_else(|| bar(&events)), partials, finals, off) }
            } else {
                let audio = match variant { "offline_hq" => &hq, "offline_native" | "offline_auto" => &native, _ => &audio16 };
                let lang = if variant == "offline_auto" { "auto" } else { code };
                let text = crate::nemo::pass_decode(port, &key, lang, audio, Duration::from_secs(30)).unwrap().text;
                (text, 0, 1, false)
            };
            let (r, h) = (norm(reference), norm(&text));
            let record = serde_json::json!({"model": model, "kind": kind, "variant": variant, "lang": lang, "clip": clip, "reference": reference, "text": text, "errors": edits(&r, &h), "words": r.len(), "elapsed_s": now.elapsed().as_secs_f64(), "partials": partials, "finals": finals, "settling_off": pass_off});
            writeln!(out, "{record}").unwrap(); out.flush().unwrap();
            println!("{model} {lang} {clip} {variant}: {}/{} edits/words", record["errors"], record["words"]);
        }
    }
    drop(side);
}
