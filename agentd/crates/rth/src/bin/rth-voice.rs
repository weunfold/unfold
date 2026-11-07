

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use microsandbox::{Sandbox, SecretSource, Volume};
use rth::devices::{CpalCapture, CpalPlayout, FeedCapture, Playout, WavSink};
use rth::voice::{Control, Note, VoiceOptions, drive};
use rth_protocol::io::read_frame;
use rth_protocol::{AGENT_PORT, Envelope, Event, MIC_SAMPLE_RATE, MonotonicClock, PROTOCOL_VERSION, PlaybackPhase, Role, Source, Stamper};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

const VM_NAME: &str = "rth-voice-vm";
const STATE_VOLUME: &str = "rth-voice-state";
const HOST_PORT: u16 = 7730;
const KEY_ENV: &str = "RTH_GEMINI_API_KEY";
const GEMINI_HOST: &str = "generativelanguage.googleapis.com";
const SYSTEM: &str = "You are RTH, a friendly voice assistant. Keep answers to one short sentence unless the user asks you to count or list something.";

const GAP: Duration = Duration::from_millis(800);

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let values = |name: &str| -> Vec<String> {
        args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    match args.first().map(String::as_str) {
        Some("stop") => stop(),
        Some("run") => {
            let questions: Vec<PathBuf> = values("--mic-wav").into_iter().map(PathBuf::from).collect();
            let barges: Vec<(PathBuf, PathBuf)> = values("--barge-wav")
                .into_iter()
                .map(|v| v.split_once(',').map(|(a, b)| (PathBuf::from(a), PathBuf::from(b))).context("--barge-wav LONG.wav,CUT.wav"))
                .collect::<Result<_>>()?;
            if questions.is_empty() && barges.is_empty() {
                bail!("run needs at least one --mic-wav or --barge-wav");
            }
            if questions.len() + 2 * barges.len() > 8 {
                bail!("at most 8 utterances per run (keep API spend small)");
            }
            let barge_after = values("--barge-after-ms").first().and_then(|v| v.parse().ok()).unwrap_or(2000);
            let script = Script { questions, barges, barge_after: Duration::from_millis(barge_after) };
            script.load()?;
            with_vm(move |sb| async move { run(&sb, script).await }).await
        }
        Some("talk") => {
            if !flag("--voice") {
                bail!("talk opens the microphone and the speaker only with an explicit --voice");
            }
            let mut opts = VoiceOptions { stream_prefix: "mic".into(), ..Default::default() };

            opts.barge_in = if flag("--no-barge-in") { rth::voice::BargeIn::Off } else { rth::voice::BargeIn::Local };
            if let Some(db) = values("--vad-db").first().and_then(|v| v.parse().ok()) {
                opts.vad.threshold_dbfs = db;
            }
            with_vm(move |sb| async move { talk(&sb, opts).await }).await
        }
        _ => bail!(
            "usage: rth-voice run --mic-wav Q.wav … [--barge-wav LONG.wav,CUT.wav …] [--barge-after-ms N] | talk --voice [--no-barge-in] [--vad-db D] | stop"
        ),
    }
}

fn stop() -> Result<()> {
    let status = std::process::Command::new("msb").args(["stop", VM_NAME]).status().context("running msb stop")?;
    if !status.success() {
        bail!("msb stop {VM_NAME} exited with {status}");
    }
    Ok(())
}

async fn with_vm<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce(Sandbox) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if std::env::var(KEY_ENV).map(|v| v.trim().is_empty()).unwrap_or(true) {
        bail!("{KEY_ENV} is not set (read it from bws into this process's environment; never on argv)");
    }
    let sb = boot().await?;
    let result = f(sb).await;
    eprintln!("stopping {VM_NAME}");
    if let Err(e) = stop() {
        eprintln!("warning: {e:#}");
    }
    result
}

fn agent_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("RTH_AGENTD_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/voice-guest/aarch64-unknown-linux-musl/release"),
    };
    let dir = dir.canonicalize().with_context(|| {
        format!("agent dir {}: cargo build -p rth-agentd --release --target aarch64-unknown-linux-musl --target-dir target/voice-guest", dir.display())
    })?;
    if !dir.join("rth-agentd").exists() {
        bail!("no rth-agentd in {}", dir.display());
    }
    Ok(dir)
}

async fn boot() -> Result<Sandbox> {
    let _ = Volume::builder(STATE_VOLUME).create().await;
    let dir = agent_dir()?.to_string_lossy().to_string();
    let t0 = Instant::now();
    let sb = Sandbox::builder(VM_NAME)
        .image("alpine")
        .cpus(2)
        .memory(1024)
        .replace()
        .port_bind(std::net::Ipv4Addr::LOCALHOST.into(), HOST_PORT, AGENT_PORT)
        .volume("/rth", |m| m.named(STATE_VOLUME))
        .volume("/opt/rth", |m| m.bind(&dir).readonly())
        .secret(|s| {
            s.env("GEMINI_API_KEY").source(SecretSource::env(KEY_ENV)).allow(GEMINI_HOST).substitute_in_header_fields(["x-goog-api-key"])
        })
        .detached(true)
        .create()
        .await
        .context("creating rth-voice-vm")?;
    eprintln!("{VM_NAME} up in {} ms", t0.elapsed().as_millis());
    Ok(sb)
}

async fn sh(sb: &Sandbox, script: &str) -> Result<String> {
    let out = sb.shell(script.to_string()).await.with_context(|| format!("guest shell: {script}"))?;
    Ok(out.stdout()?)
}

async fn binary_identity(sb: &Sandbox) -> Result<Value> {
    let host_path = agent_dir()?.join("rth-agentd");
    let out = std::process::Command::new("shasum").args(["-a", "256"]).arg(&host_path).output()?;
    let host = String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or("").to_string();
    let guest = sh(sb, "sha256sum /opt/rth/rth-agentd").await?.split_whitespace().next().unwrap_or("").to_string();
    if host.is_empty() || host != guest {
        bail!("guest binary {guest:?} differs from host {host:?}");
    }
    Ok(json!({"host_path": host_path.display().to_string(), "sha256": host, "guest_matches": true}))
}

async fn start_agent(sb: &Sandbox) -> Result<()> {
    let script = format!(
        "pkill rth-agentd; sleep 0.2; RTH_BACKEND=gemini RTH_GEMINI_SYSTEM='{SYSTEM}' RTH_JOURNAL=/rth/journal.jsonl \
         nohup /opt/rth/rth-agentd >>/rth/agentd.log 2>&1 & echo started"
    );
    if sh(sb, &script).await?.trim() != "started" {
        bail!("agent did not start");
    }
    Ok(())
}

struct Link {
    reader: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    stamper: Stamper,
    boot_id: String,
}

async fn attach(name: &str) -> Result<Link> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > deadline {
            bail!("agent did not accept an attach within 20 s");
        }
        let Ok(stream) = TcpStream::connect(("127.0.0.1", HOST_PORT)).await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let _ = stream.set_nodelay(true);
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut buf = Vec::new();
        let Ok(Ok(line)) = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut reader, &mut buf)).await else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let hello = Envelope::parse_from(&line, Source::Guest).map_err(|e| anyhow::anyhow!("hello: {e}"))?;
        let Event::Hello { protocol: PROTOCOL_VERSION, boot_id, .. } = hello.event else { bail!("first frame was not a v{PROTOCOL_VERSION} hello") };
        let mut stamper = Stamper::new(Source::Host, name, Box::new(MonotonicClock::start()));
        let attach = stamper.stamp(Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: name.into(), role: Role::Controller }, None);
        write.write_all(attach.to_line().as_bytes()).await?;
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), read_frame(&mut reader, &mut buf)).await.context("no ready")?.map_err(|e| anyhow::anyhow!("{e}"))?;
            let Some(env) = Envelope::parse_known(&line, Source::Guest).map_err(|e| anyhow::anyhow!("{e}"))? else { continue };
            match env.event {
                Event::Ready { attachment_id, role: Role::Controller } if attachment_id == name => return Ok(Link { reader, write, stamper, boot_id }),
                Event::Ready { attachment_id, role: Role::Observer } if attachment_id == name => bail!("admitted as observer: another controller owns speech"),
                _ => {}
            }
        }
    }
}

async fn wait_ready(sb: &Sandbox, boot_id: &str) -> Result<String> {
    let script = format!("grep -F '\"producer\":\"guest-{boot_id}\"' /rth/journal.jsonl | grep -F '\"msg\":\"gemini ' | head -n 3");
    for _ in 0..60 {
        for line in sh(sb, &script).await?.lines() {
            let Ok(env) = serde_json::from_str::<Envelope>(line) else { continue };
            if let Event::Log { msg, .. } = env.event {
                if msg.starts_with("gemini ready") {
                    return Ok(msg);
                }
                if msg.starts_with("gemini connect") {
                    bail!("backend could not connect: {msg}");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("no gemini ready line for boot {boot_id} within ~30 s")
}

struct Script {
    questions: Vec<PathBuf>,
    barges: Vec<(PathBuf, PathBuf)>,
    barge_after: Duration,
}

impl Script {
    fn load(&self) -> Result<()> {
        for p in self.questions.iter().chain(self.barges.iter().flat_map(|(a, b)| [a, b])) {
            mic_samples(p)?;
        }
        Ok(())
    }
}

fn mic_samples(p: &Path) -> Result<Vec<i16>> {
    let w = rth_voice::wav::read(p).map_err(anyhow::Error::msg)?;
    Ok(w.to_i16_at(MIC_SAMPLE_RATE))
}

#[derive(Default, Debug)]
struct Resp {
    started: Option<Instant>,
    first_audio: Option<Instant>,
    done: Option<(Instant, String)>,
    played: Option<Instant>,
    ended_playback: Option<(Instant, String)>,
    transcript: String,
    chunks: u64,
    chunks_after_clear: u64,
    cleared_at: Option<Instant>,
}

struct Obs {
    t0: Instant,
    notes: mpsc::UnboundedReceiver<Note>,
    events: std::fs::File,
    resp: HashMap<String, Resp>,
    order: Vec<String>,

    mic_out: Vec<(Instant, String, u64, u64)>,
    onsets: Vec<(Instant, String, u64, u64, bool)>,
    clears: Vec<(Instant, Instant, Value)>,
    signals: Vec<(Instant, Option<String>, String)>,
    heard: Vec<(Instant, String)>,
    ingress: Vec<Value>,
    logs: Vec<String>,
}

impl Obs {
    fn ms(&self, t: Instant) -> f64 {
        (t - self.t0).as_secs_f64() * 1000.0
    }

    async fn until(&mut self, wait: Duration, mut pred: impl FnMut(&Obs) -> bool) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if pred(self) {
                return Ok(true);
            }
            match tokio::time::timeout_at(deadline, self.notes.recv()).await {
                Err(_) => return Ok(false),
                Ok(None) => bail!("voice loop ended"),
                Ok(Some(n)) => self.absorb(n)?,
            }
        }
    }

    fn absorb(&mut self, note: Note) -> Result<()> {
        match note {
            Note::In { t, env } => {
                self.log_env(t, "in", &env)?;
                match &env.event {
                    Event::ResponseStarted { response_id, .. } => {
                        self.order.push(response_id.clone());
                        self.resp.entry(response_id.clone()).or_default().started = Some(t);
                    }
                    Event::AgentAudio { response_id, .. } => {
                        let r = self.resp.entry(response_id.clone()).or_default();
                        r.chunks += 1;
                        r.first_audio.get_or_insert(t);
                        if r.cleared_at.is_some() {
                            r.chunks_after_clear += 1;
                        }
                    }
                    Event::AgentDelta { response_id, text } => self.resp.entry(response_id.clone()).or_default().transcript.push_str(text),

                    Event::AgentReplace { response_id, text } => self.resp.entry(response_id.clone()).or_default().transcript = text.clone(),
                    Event::AgentDone { response_id, status } => {
                        self.resp.entry(response_id.clone()).or_default().done = Some((t, format!("{status:?}").to_lowercase()));
                    }
                    Event::ProviderSignal { response_id, signal } => self.signals.push((t, response_id.clone(), signal.clone())),
                    Event::InputTranscript { text, .. } => self.heard.push((t, text.clone())),
                    Event::AudioIngress { stream_id, chunks, samples, bytes, gaps, sha256 } => self.ingress.push(
                        json!({"stream_id": stream_id, "chunks": chunks, "samples": samples, "bytes": bytes, "gaps": gaps, "sha256": sha256, "t_ms": self.ms(t)}),
                    ),
                    Event::Log { msg, .. } if msg.starts_with("gemini") || msg.starts_with("dropped") => self.logs.push(msg.clone()),
                    _ => {}
                }
            }
            Note::Out { t, env } => {
                self.log_env(t, "out", &env)?;
                match &env.event {
                    Event::UserAudio { stream_id, sample_offset, data, .. } => {
                        self.mic_out.push((t, stream_id.clone(), *sample_offset, (data.len() / 4 * 3 / 2) as u64));
                    }
                    Event::Playback { response_id, phase, reason, .. } => {
                        let r = self.resp.entry(response_id.clone()).or_default();
                        match phase {
                            PlaybackPhase::Started => r.played = Some(t),
                            PlaybackPhase::Finished => r.ended_playback = Some((t, "finished".into())),
                            PlaybackPhase::Cleared => r.ended_playback = Some((t, reason.clone().unwrap_or_default())),
                        }
                    }
                    _ => {}
                }
            }
            Note::Onset { t, stream_id, start_sample, detected_sample, audible, .. } => {
                writeln!(self.events, "{}", json!({"t_ms": self.ms(t), "dir": "local", "onset": {"stream": stream_id, "start_sample": start_sample, "detected_sample": detected_sample, "audible": audible}}))?;
                self.onsets.push((t, stream_id, start_sample, detected_sample, audible));
            }
            Note::SpeechEnd { t, end_sample } => {
                writeln!(self.events, "{}", json!({"t_ms": self.ms(t), "dir": "local", "speech_end_sample": end_sample}))?;
            }
            Note::NearEnd { .. } | Note::EchoProbe { .. } | Note::Talk { .. } | Note::Mute { .. } => {}
            Note::Cleared { t, trigger, cleared, cancelled } => {
                for (rid, _, _) in &cleared.responses {
                    self.resp.entry(rid.clone()).or_default().cleared_at = Some(t);
                }
                let v = json!({
                    "reason": cleared.reason.as_str(),
                    "responses": cleared.responses.iter().map(|(r, e, open)| json!({"response_id": r, "epoch": e, "was_open": open})).collect::<Vec<_>>(),
                    "cancel_sent_for": cancelled,
                    "dropped_samples": cleared.dropped_samples,
                    "queue_flush_ns": cleared.flushed.took_ns,
                    "trigger_to_cleared_us": (t - trigger).as_secs_f64() * 1e6,
                });
                writeln!(self.events, "{}", json!({"t_ms": self.ms(t), "dir": "local", "cleared": v}))?;
                self.clears.push((t, trigger, v));
            }
        }
        Ok(())
    }

    fn log_env(&mut self, t: Instant, dir: &str, env: &Envelope) -> Result<()> {
        let mut v = serde_json::to_value(env)?;
        if let Event::AgentAudio { data, .. } | Event::UserAudio { data, .. } = &env.event {
            v["data"] = json!(format!("<{} base64 chars>", data.len()));
        }
        writeln!(self.events, "{}", json!({"t_ms": self.ms(t), "dir": dir, "env": v}))?;
        Ok(())
    }

    fn mic_sent_at(&self, stream: &str, s: u64) -> Option<Instant> {
        self.mic_out.iter().find(|(_, st, off, n)| st == stream && off + n >= s).map(|(t, ..)| *t)
    }

    fn response_after(&self, t: Instant) -> Option<String> {
        self.order.iter().find(|r| self.resp[*r].started.is_some_and(|s| s > t)).cloned()
    }

    fn playback_over(&self, rid: &str) -> bool {
        self.resp.get(rid).is_some_and(|r| r.ended_playback.is_some() || r.done.as_ref().is_some_and(|(_, s)| s != "completed"))
    }
}

fn ms(a: Instant, b: Instant) -> f64 {
    (b.saturating_duration_since(a)).as_secs_f64() * 1000.0
}

async fn run(sb: &Sandbox, script: Script) -> Result<()> {
    let stamp = utc_stamp();
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/voice").join(&stamp);
    std::fs::create_dir_all(out.join("mic"))?;
    let identity = binary_identity(sb).await?;
    start_agent(sb).await?;
    let link = attach("voice-run").await?;
    let ready = wait_ready(sb, &link.boot_id).await?;
    let boot_id = link.boot_id.clone();
    eprintln!("{ready}");

    let feed = FeedCapture::new();
    let mut capture = feed.clone();
    let mut sink = WavSink::new(out.join("speaker.wav"), 48_000, 10);
    let (ctl_tx, mut ctl_rx) = mpsc::channel(16);
    let (notes_tx, notes_rx) = mpsc::unbounded_channel();

    let opts = VoiceOptions { stream_prefix: format!("{}-mic", link.boot_id), barge_in: rth::voice::BargeIn::Local, ..Default::default() };
    let vad_cfg = opts.vad;
    let driver = tokio::spawn(async move {
        let Link { mut reader, write, stamper, .. } = link;
        drive(&mut reader, write, stamper, &mut capture, &mut sink, opts, &mut ctl_rx, notes_tx).await
    });
    let t0 = Instant::now();
    let mut obs = Obs {
        t0,
        notes: notes_rx,
        events: std::fs::File::create(out.join("events.jsonl"))?,
        resp: HashMap::new(),
        order: Vec::new(),
        mic_out: Vec::new(),
        onsets: Vec::new(),
        clears: Vec::new(),
        signals: Vec::new(),
        heard: Vec::new(),
        ingress: Vec::new(),
        logs: vec![ready.clone()],
    };
    let stream = |o: &Obs| o.mic_out.first().map(|(_, s, ..)| s.clone()).unwrap_or_default();
    let mut turns: Vec<Value> = Vec::new();
    let mut barges: Vec<Value> = Vec::new();
    obs.until(Duration::from_millis(500), |_| false).await?;

    let turn = |obs: &Obs, kind: &str, wav: &Path, said_at: Instant, end_sample: u64, rid: Option<&String>| -> Value {
        let s = stream(obs);
        let speech_end = obs.mic_sent_at(&s, end_sample);
        let r = rid.and_then(|r| obs.resp.get(r));
        let heard: Vec<&str> = obs.heard.iter().filter(|(t, _)| *t > said_at && r.and_then(|r| r.started).is_none_or(|st| *t <= st + Duration::from_secs(3))).map(|(_, x)| x.as_str()).collect();
        json!({
            "kind": kind, "wav": wav.display().to_string(), "response_id": rid,
            "speech_end_to_response_started_ms": speech_end.zip(r.and_then(|r| r.started)).map(|(a, b)| ms(a, b)),
            "speech_end_to_first_audio_ms": speech_end.zip(r.and_then(|r| r.first_audio)).map(|(a, b)| ms(a, b)),
            "speech_end_to_first_played_ms": speech_end.zip(r.and_then(|r| r.played)).map(|(a, b)| ms(a, b)),
            "first_audio_to_first_played_ms": r.and_then(|r| r.first_audio.zip(r.played)).map(|(a, b)| ms(a, b)),
            "status": r.and_then(|r| r.done.as_ref().map(|d| d.1.clone())),
            "playback_end": r.and_then(|r| r.ended_playback.as_ref().map(|d| d.1.clone())),
            "audio_chunks": r.map(|r| r.chunks),
            "heard": heard.concat().trim(),
            "said": r.map(|r| r.transcript.trim().to_string()),
        })
    };

    for wav in &script.questions {
        let samples = mic_samples(wav)?;
        let said_at = Instant::now();
        let (_, end) = feed.say(&samples);
        eprintln!("say {} ({:.1} s)", wav.display(), samples.len() as f64 / 16_000.0);
        let got = obs.until(Duration::from_secs(40), |o| o.response_after(said_at).is_some_and(|r| o.playback_over(&r))).await?;
        let rid = obs.response_after(said_at);
        let v = turn(&obs, "question", wav, said_at, end, rid.as_ref());
        eprintln!("  {}", v);
        if !got {
            eprintln!("  (no complete response within 40 s)");
        }
        turns.push(v);
        obs.until(GAP, |_| false).await?;
    }

    for (long, cut) in &script.barges {
        let said_at = Instant::now();
        let (_, long_end) = feed.say(&mic_samples(long)?);
        eprintln!("say {} then barge in after {:?} of playback with {}", long.display(), script.barge_after, cut.display());
        obs.until(Duration::from_secs(40), |o| o.response_after(said_at).is_some_and(|r| o.resp[&r].played.is_some())).await?;
        let Some(r1) = obs.response_after(said_at) else {
            eprintln!("  no response to the long prompt; skipping this barge-in");
            continue;
        };
        let played = obs.resp[&r1].played.unwrap_or_else(Instant::now);
        let wait = (played + script.barge_after).saturating_duration_since(Instant::now());
        obs.until(wait, |_| false).await?;
        let cut_samples = mic_samples(cut)?;
        let cut_said = Instant::now();
        let (cut_start, cut_end) = feed.say(&cut_samples);
        let onset_seen = obs.until(Duration::from_secs(5), |o| o.onsets.iter().any(|(_, _, s, ..)| *s + 640 >= cut_start)).await?;
        let onset = obs.onsets.iter().find(|(_, _, s, ..)| *s + 640 >= cut_start).cloned();
        obs.until(Duration::from_secs(40), |o| o.response_after(cut_said).is_some_and(|r| o.playback_over(&r))).await?;

        obs.until(Duration::from_secs(2), |_| false).await?;
        let r2 = obs.response_after(cut_said);
        let first = turn(&obs, "barge_long", long, said_at, long_end, Some(&r1));
        let second = turn(&obs, "barge_cut", cut, cut_said, cut_end, r2.as_ref());
        let clear = onset.as_ref().and_then(|(ot, ..)| obs.clears.iter().find(|(_, trig, _)| trig == ot)).cloned();
        let interrupted = onset.as_ref().and_then(|(ot, ..)| obs.signals.iter().find(|(t, r, s)| t > ot && s == "interrupted" && r.as_ref() == Some(&r1)).map(|(t, ..)| ms(*ot, *t)));
        let r1o = &obs.resp[&r1];
        let v = json!({
            "long": first, "cut": second,
            "onset_seen": onset_seen,
            "vad_algorithmic_delay_ms": onset.as_ref().map(|(_, _, s, d, _)| (d - s) as f64 / 16.0),
            "cut_first_sample_sent_to_onset_ms": onset.as_ref().and_then(|(ot, st, ..)| obs.mic_sent_at(st, cut_start + 1).map(|t| ms(t, *ot))),
            "agent_audible_at_onset": onset.as_ref().map(|o| o.4),
            "played_before_clear_ms": r1o.played.zip(clear.as_ref().map(|c| c.0)).map(|(a, b)| ms(a, b)),
            "local_clear": clear.as_ref().map(|c| c.2.clone()),
            "onset_to_provider_interrupted_ms": interrupted,
            "old_response_terminal": r1o.done.as_ref().map(|(t, s)| json!({"status": s, "after_onset_ms": onset.as_ref().map(|o| ms(o.0, *t))})),
            "old_response_chunks_after_clear_reached_host": r1o.chunks_after_clear,
        });
        eprintln!("  barge-in: {}", v);
        barges.push(v);
        obs.until(GAP, |_| false).await?;
    }

    ctl_tx.send(Control::EndStream).await?;
    obs.until(Duration::from_secs(5), |o| !o.ingress.is_empty()).await?;
    ctl_tx.send(Control::Quit).await?;
    while let Some(n) = obs.notes.recv().await {
        obs.absorb(n)?;
    }
    let summary = driver.await??;

    let ingress_match = summary.writer.streams.iter().map(|st| {
        let (sid, tally) = (&st.stream_id, &st.written);
        let guest = obs.ingress.iter().find(|g| g["stream_id"] == sid.as_str());
        json!({"stream_id": sid, "host_written_chunks": tally.chunks, "host_written_samples": tally.samples, "host_written_sha256": tally.sha256(),
            "host_accepted_chunks": st.chunks.accepted, "host_refused_chunks": st.chunks.refused, "end": st.end.as_str(),
            "guest": guest, "sha256_match": guest.is_some_and(|g| g["sha256"] == tally.sha256().as_str()), "guest_gaps": guest.map(|g| g["gaps"].clone())})
    }).collect::<Vec<_>>();

    let journal = sh(sb, "cat /rth/journal.jsonl").await.unwrap_or_default();
    let boot_lines: Vec<&str> = journal.lines().filter(|l| l.contains(&format!("\"producer\":\"guest-{boot_id}\""))).collect();
    let mut mic_records = 0u64;
    let mut mic_with_payload = 0u64;
    for l in journal.lines().filter(|l| l.contains(r#""kind":"user_audio""#)) {
        mic_records += 1;
        if let Ok(env) = serde_json::from_str::<Envelope>(l)
            && let Event::UserAudio { data, digest, .. } = env.event
            && (!data.is_empty() || digest.is_none())
        {
            mic_with_payload += 1;
        }
    }
    let key = std::env::var(KEY_ENV)?;
    let env_value = sh(sb, "printenv GEMINI_API_KEY").await?.trim_end().to_string();
    let agent_log = sh(sb, "tail -c 200000 /rth/agentd.log").await.unwrap_or_default();
    std::fs::write(out.join("guest-agentd.log"), &agent_log)?;
    let journal_logs: Vec<&str> = journal.lines().filter(|l| l.contains(r#""kind":"log""#) || l.contains(r#""kind":"provider_signal""#) || l.contains(r#""kind":"playback""#) || l.contains(r#""kind":"decision""#)).collect();
    std::fs::write(out.join("guest-journal-excerpt.jsonl"), journal_logs.join("\n") + "\n")?;
    for (i, p) in script.questions.iter().chain(script.barges.iter().flat_map(|(a, b)| [a, b])).enumerate() {
        let _ = std::fs::copy(p, out.join("mic").join(format!("{i:02}-{}", p.file_name().unwrap().to_string_lossy())));
    }
    let (scanned, hits) = scan_for(&key, &[&out]);

    let pick = |xs: &[Value], k: &str| xs.iter().filter_map(|v| v[k].as_f64()).collect::<Vec<_>>();
    let answer_turns: Vec<Value> = turns.iter().cloned().chain(barges.iter().map(|b| b["cut"].clone())).collect();
    let clears: Vec<Value> = barges.iter().filter_map(|b| b["local_clear"].as_object().cloned().map(Value::Object)).collect();
    let summary_v = json!({
        "model": "gemini-3.8-live (automatic activity detection, realtimeInput audio 16 kHz)",
        "clock": "host monotonic (Instant); queue flush time measured inside the queue lock",
        "vm": VM_NAME, "guest_binary": identity, "gemini_ready": ready,
        "devices": {"capture": summary.capture, "playout": summary.playout, "note": "synthetic: scripted WAV microphone, 48 kHz WAV sink; no microphone or speaker was opened"},
        "vad": {"threshold_dbfs": vad_cfg.threshold_dbfs, "frame_ms": vad_cfg.frame_ms, "onset_frames": vad_cfg.onset_frames, "hang_frames": vad_cfg.hang_frames},
        "latency": {
            "speech_end_to_first_audio_ms (last utterance chunk written → first AgentAudio read)": stats(&pick(&answer_turns, "speech_end_to_first_audio_ms")),
            "speech_end_to_first_played_ms (→ WAV sink took its first sample)": stats(&pick(&answer_turns, "speech_end_to_first_played_ms")),
            "speech_end_to_response_started_ms": stats(&pick(&answer_turns, "speech_end_to_response_started_ms")),
            "local_onset_to_queue_cleared_us": stats(&pick(&clears, "trigger_to_cleared_us")),
            "queue_flush_hold_ns": stats(&pick(&clears, "queue_flush_ns")),
            "onset_to_provider_interrupted_ms": stats(&barges.iter().filter_map(|b| b["onset_to_provider_interrupted_ms"].as_f64()).collect::<Vec<_>>()),
        },
        "turns": turns, "barge_ins": barges,
        "ingress": ingress_match,
        "writer": {
            "control": {"accepted": summary.writer.control.accepted, "refused": summary.writer.control.refused, "written": summary.writer.control.written, "unresolved": summary.writer.control.unresolved()},
            "media": {"accepted": summary.writer.media.accepted, "refused": summary.writer.media.refused, "written": summary.writer.media.written, "unresolved": summary.writer.media.unresolved()},
            "in_flight_at_abort": summary.writer.in_flight, "aborted": summary.writer.aborted,
            "devices_stopped_after_ms": summary.devices_stopped_after.as_secs_f64() * 1e3},
        "owner": {"chunks_queued": summary.owner.chunks_queued, "late_chunks_dropped": summary.owner.late_chunks_dropped, "overflow_samples": summary.owner.overflow_samples, "gaps": summary.owner.gaps, "clears": summary.owner.clears},
        "queue": {"rendered_after_fence": summary.queue.rendered_after_fence, "underrun_samples": summary.queue.underrun_samples, "overflow_samples": summary.queue.overflow_samples, "queued_at_end": summary.queue.queued},
        "mic": {"frames_dropped": summary.mic_frames_dropped, "onsets": summary.onsets},
        "provider_signals": obs.signals.iter().map(|(t, r, s)| json!({"t_ms": obs.ms(*t), "response_id": r, "signal": s})).collect::<Vec<_>>(),
        "journal": {"bytes": journal.len(), "this_boot_lines": boot_lines.len(), "user_audio_records": mic_records, "user_audio_records_with_pcm_or_no_digest": mic_with_payload},
        "secret": {"guest_env_is_placeholder": env_value.starts_with("$MSB_") && env_value != key, "guest_journal_contains_real_key": journal.contains(&key),
            "guest_agentd_log_contains_real_key": agent_log.contains(&key), "run_dir_files_scanned": scanned, "run_dir_files_containing_real_key": hits},
        "backend_log": obs.logs,
        "out_dir": out.display().to_string(),
    });
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary_v)?)?;
    let mut md = std::fs::File::create(out.join("transcript.md"))?;
    writeln!(md, "# M4b voice transcript (synthetic microphone, WAV speaker)\n")?;
    for (i, t) in answer_turns.iter().enumerate() {
        writeln!(md, "{}. **heard (Gemini input transcription):** {}  \n   **rth:** {}  \n   _{} · {}_\n", i + 1, t["heard"].as_str().unwrap_or(""), t["said"].as_str().unwrap_or(""), t["status"], t["playback_end"])?;
    }
    println!("{}", serde_json::to_string_pretty(&summary_v["latency"])?);
    println!("results: {}", out.display());
    Ok(())
}

async fn talk(sb: &Sandbox, opts: VoiceOptions) -> Result<()> {
    let identity = binary_identity(sb).await?;
    start_agent(sb).await?;
    let link = attach("voice-talk").await?;
    let ready = wait_ready(sb, &link.boot_id).await?;
    let mut capture = CpalCapture::new();
    let mut speaker = CpalPlayout::new()?;
    let rate = f64::from(speaker.rate());
    println!("rth-voice: {ready}\nguest binary {}\n{}\nheadphones recommended (no echo cancellation). Ctrl-X cancels · q or Esc quits.", identity["sha256"], speaker.describe());

    let (ctl_tx, mut ctl_rx) = mpsc::channel(16);
    let (notes_tx, mut notes) = mpsc::unbounded_channel();
    use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};
    ratatui::crossterm::terminal::enable_raw_mode()?;
    let keys = ctl_tx.clone();
    std::thread::spawn(move || {
        loop {
            let Ok(TermEvent::Key(k)) = event::read() else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
            let c = match k.code {
                KeyCode::Char('x') if ctrl => Control::Cancel,
                KeyCode::Char('c') if ctrl => Control::Quit,
                KeyCode::Char('q') | KeyCode::Esc => Control::Quit,
                _ => continue,
            };
            let quit = matches!(c, Control::Quit);
            if keys.blocking_send(c).is_err() || quit {
                return;
            }
        }
    });
    let driver = tokio::spawn(async move {
        let Link { mut reader, write, stamper, .. } = link;
        drive(&mut reader, write, stamper, &mut capture, &mut speaker, opts, &mut ctl_rx, notes_tx).await
    });
    let say = |s: String| {
        print!("{}\r\n", s.replace('\n', "\r\n"));
        let _ = std::io::stdout().flush();
    };

    let mut lines: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    while let Some(n) = notes.recv().await {
        match n {
            Note::In { env, .. } => match env.event {
                Event::InputTranscript { text, .. } => say(format!("you: {}", text.trim())),
                Event::AgentDelta { response_id, text } => lines.entry(response_id).or_default().push_str(&text),
                Event::AgentReplace { response_id, text } => {
                    lines.insert(response_id, text);
                }
                Event::AgentDone { response_id, status } => {
                    let line = lines.remove(&response_id).unwrap_or_default();
                    say(format!("rth: {} [{status:?}]", line.trim()));
                }
                Event::Log { level, msg } if level == "error" || level == "warn" => say(format!("· {msg}")),
                _ => {}
            },
            Note::Cleared { cleared, t, trigger, .. } => say(format!(
                "· {}: cleared {:.1} s of queued speech in {:.0} µs",
                cleared.reason.as_str(),
                cleared.dropped_samples as f64 / rate,
                (t - trigger).as_secs_f64() * 1e6
            )),
            _ => {}
        }
    }
    let _ = ratatui::crossterm::terminal::disable_raw_mode();
    let summary = driver.await??;
    println!(
        "mic dropped frames {}, onsets {}, clears {}, late chunks dropped {}, queue underrun {:.1} s · {}",
        summary.mic_frames_dropped,
        summary.onsets,
        summary.owner.clears,
        summary.owner.late_chunks_dropped,
        summary.queue.underrun_samples as f64 / rate,
        summary.playout
    );
    Ok(())
}

fn stats(xs: &[f64]) -> Value {
    if xs.is_empty() {
        return json!({"n": 0});
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = |p: f64| v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    let round = |x: f64| (x * 10.0).round() / 10.0;
    json!({"n": v.len(), "min": round(v[0]), "p50": round(rank(0.5)), "p95": round(rank(0.95)), "max": round(v[v.len() - 1]),
        "mean": round(v.iter().sum::<f64>() / v.len() as f64), "method": "nearest-rank"})
}

fn scan_for(needle: &str, roots: &[&Path]) -> (u64, Vec<String>) {
    let mut scanned = 0;
    let mut hits = Vec::new();
    let mut stack: Vec<PathBuf> = roots.iter().map(|p| p.to_path_buf()).collect();
    let n = needle.as_bytes();
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() && meta.len() <= 64 << 20 {
            if let Ok(bytes) = std::fs::read(&p) {
                scanned += 1;
                if bytes.windows(n.len()).any(|w| w == n) {
                    hits.push(p.display().to_string());
                }
            }
        }
    }
    (scanned, hits)
}

fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}{mo:02}{d:02}T{h:02}{m:02}{s:02}Z")
}
