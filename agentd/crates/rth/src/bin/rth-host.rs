

use std::io::Write as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rth::call::{Call, CallCapture, CallDevices, CallPlayout, NoSpeaker};
use rth::devices::{Capture, CpalCapture, CpalPlayout, FeedCapture, NullCapture, NullPlayout, Playout, WavSink};
use rth::host::{Outbox, Shared, StatusSource, run_link, serve, token};
use rth::voice::{BargeIn, Control, TalkMode, VoiceOptions};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

struct Args(Vec<String>);

impl Args {
    fn value(&self, k: &str) -> Option<String> {
        self.0.iter().position(|a| a == k).and_then(|i| self.0.get(i + 1)).cloned()
    }
    fn flag(&self, k: &str) -> bool {
        self.0.iter().any(|a| a == k)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args(std::env::args().skip(1).collect());
    let agent: SocketAddr = args.value("--agent").context("--agent HOST:PORT is required (never assumed: 127.0.0.1:7700 is rth-core)")?.parse()?;
    if !agent.ip().is_loopback() {
        bail!("--agent must be a loopback address");
    }
    let listen: SocketAddr = args.value("--listen").unwrap_or_else(|| "127.0.0.1:7781".into()).parse()?;
    if !listen.ip().is_loopback() {
        bail!("--listen must be a loopback address");
    }
    let data = args.value("--data").map(PathBuf::from).unwrap_or_else(rth_deploy::data_dir);
    let label = args.value("--label").unwrap_or_else(|| format!("host-{}", agent.port()));
    let host_dir = data.join("host").join(&label);
    let token_file = args.value("--token-file").map(PathBuf::from).unwrap_or_else(|| host_dir.join("token"));
    let token = token(&token_file)?;

    rth::crash::record_panics(args.value("--log").map(PathBuf::from), data.join("host").join("last-panic.json"), label.clone());
    let outbox = Outbox::open(&host_dir.join("outbox.jsonl"))?;
    let status_source = args.value("--vm").map(|vm| StatusSource { data: data.clone(), vm });
    let log = match args.value("--log") {
        Some(p) => Some(std::fs::OpenOptions::new().create(true).append(true).open(p)?),
        None => None,
    };

    let mut opts = VoiceOptions::default();
    opts.barge_in = match (args.value("--barge-in"), args.flag("--no-barge-in")) {
        (Some(m), _) => BargeIn::parse(&m)?,
        (None, true) => BargeIn::Off,
        (None, false) => BargeIn::Provider,
    };
    opts.talk = match args.value("--talk").as_deref() {
        None | Some("open") => TalkMode::Open,
        Some("ptt") => TalkMode::PushToTalk,
        Some(o) => bail!("--talk open|ptt, not {o}"),
    };
    if let Some(db) = args.value("--vad-db") {
        opts.vad.threshold_dbfs = db.parse()?;
        opts.echo.floor_dbfs = opts.vad.threshold_dbfs;
    }
    for (flag, slot) in [
        ("--echo-delay-ms", &mut opts.echo.delay_ms),
        ("--echo-coupling-db", &mut opts.echo.coupling_db),
        ("--echo-margin-db", &mut opts.echo.margin_db),
        ("--echo-tail-ms", &mut opts.echo.tail_ms),
    ] {
        if let Some(v) = args.value(flag) {
            *slot = v.parse().with_context(|| flag.to_string())?;
        }
    }
    if let Some(v) = args.value("--echo-hold-ms") {
        opts.echo.hold_ms = v.parse()?;
    }
    let voice_io = args.value("--voice-io").unwrap_or_else(|| "vpio".into());
    if !matches!(voice_io.as_str(), "vpio" | "raw") {
        bail!("--voice-io vpio|raw, not {voice_io}");
    }
    let mic_wav = args.value("--mic-wav");
    let speaker_wav = args.value("--speaker-wav");
    let call_ready = args.flag("--call-ready");
    if call_ready && args.flag("--voice") {
        bail!("--voice starts a call at launch, --call-ready waits for one: pick one");
    }

    let real = args.flag("--voice") || (call_ready && mic_wav.is_none() && speaker_wav.is_none());
    #[cfg(target_os = "macos")]
    let vpio = (real && voice_io == "vpio").then(|| {
        rth::vpio::VpioShared::new(rth::vpio::VpioOptions {
            agc: args.value("--vpio-agc").as_deref() != Some("off"),
            ducking: args.value("--vpio-ducking").unwrap_or_else(|| "min".into()),
            ..Default::default()
        })
    });
    #[cfg(not(target_os = "macos"))]
    if real && voice_io == "vpio" {
        bail!("--voice-io vpio needs macOS (Apple voice processing); use --voice-io raw with headphones");
    }
    let mut feed_handle = None;
    let (capture, voice): (Box<dyn Capture>, String) = if real && voice_io == "raw" {
        (Box::new(CpalCapture::new()), "RAW microphone and speaker (--voice-io raw): NO echo cancellation, headphones only".into())
    } else if real {
        #[cfg(target_os = "macos")]
        {
            let v = vpio.clone().expect("vpio");
            (Box::new(rth::vpio::VpioCapture(v)), "Apple voice processing (VoiceProcessingIO: echo cancellation + noise suppression) microphone and speaker".into())
        }
        #[cfg(not(target_os = "macos"))]
        unreachable!()
    } else if let Some(wavs) = &mic_wav {
        let feed = FeedCapture::new();

        if let Some(ms) = args.value("--mic-after-ms") {
            feed.say(&vec![0i16; 16 * ms.parse::<usize>()?]);
        }
        for w in wavs.split(',') {
            let samples = rth_voice::wav::read(std::path::Path::new(w)).map_err(anyhow::Error::msg).with_context(|| format!("mic wav {w}"))?.to_i16_at(16_000);
            feed.say(&samples);
            feed.say(&vec![0i16; 16_000 * 2]);
        }
        feed_handle = Some(feed.clone());
        (Box::new(feed), format!("scripted microphone from {wavs}"))
    } else {
        (Box::new(NullCapture), "off".into())
    };
    let configured = real || mic_wav.is_some() || speaker_wav.is_some();
    let real_speaker = real && voice_io == "raw";
    let voice = match (&speaker_wav, real) {
        (Some(p), false) => format!("{voice}; speaker = WAV sink {p}"),
        _ => voice,
    };
    #[cfg(target_os = "macos")]
    let vpio_play = vpio.clone();

    let make_playout = move |n: u64| -> Box<dyn Playout> {
        #[cfg(target_os = "macos")]
        if let Some(v) = &vpio_play {
            return Box::new(rth::vpio::VpioPlayout(v.clone()));
        }
        if real_speaker {
            return match CpalPlayout::new() {
                Ok(p) => Box::new(p),
                Err(e) => Box::new(NoSpeaker { rate: 48_000, why: format!("speaker unavailable ({e}); speech is not played") }),
            };
        }
        match &speaker_wav {
            Some(p) => Box::new(WavSink::new(if n <= 1 { PathBuf::from(p) } else { PathBuf::from(format!("{p}.{n}.wav")) }, 48_000, 10)),
            None => Box::new(NullPlayout::default()),
        }
    };

    let rate = make_playout(0).rate();
    let devices = configured.then(|| CallDevices { capture, playout: Box::new(make_playout), rate, describe: voice.clone() });

    let (ctl_tx, ctl_rx) = mpsc::channel::<Control>(256);
    let shared = Shared::new(ctl_tx.clone(), outbox, agent.to_string(), voice.clone(), status_source, log);

    if let Err(e) = shared.set_launch_registry(&host_dir.join("launches.jsonl")) {
        shared.note("launch_registry_refused", serde_json::json!({"error": e.to_string()}));
    }
    let expect = rth::host::Expect { backend: args.value("--expect-backend"), boot: args.value("--expect-boot") };
    for (flag, v) in [("--expect-backend", &expect.backend), ("--expect-boot", &expect.boot)] {
        if args.flag(flag) && v.as_deref().is_none_or(|s| s.is_empty() || s.starts_with("--")) {
            bail!("{flag} needs a value");
        }
    }
    if expect.backend.is_some() || expect.boot.is_some() {
        *shared.expect.lock().unwrap() = Some(expect);
    }
    let mut policy = opts.policy_json();
    policy["voice_io"] = serde_json::json!(if real { voice_io.as_str() } else if feed_handle.is_some() { "scripted" } else if configured { "wav_speaker" } else { "off" });
    policy["call_at_launch"] = serde_json::json!(configured && !call_ready);
    policy["echo_cancellation"] = serde_json::json!(match (real, voice_io.as_str()) {
        (true, "vpio") => "apple_voice_processing (see voice_device for the unit's own state)",
        (true, _) => "none (raw microphone: headphones only)",
        _ => "not applicable (no real devices)",
    });
    *shared.voice_policy.lock().unwrap() = policy.clone();
    *shared.feed.lock().unwrap() = feed_handle;
    #[cfg(target_os = "macos")]
    if let Some(v) = &vpio {
        let v = v.clone();
        *shared.voice_device.lock().unwrap() = Some(Arc::new(move || v.status()));
    }
    let weak = Arc::downgrade(&shared);
    let call = Call::new(
        devices,
        shared.urgent.clone(),
        Box::new(move |kind: &str, v: serde_json::Value| {
            if let Some(s) = weak.upgrade() {
                s.note_later(kind, v);
            }
        }),
    );
    *shared.call.lock().unwrap() = Some(call.clone());
    if configured && !call_ready {
        call.request(true, "launch");
    }
    let listener = TcpListener::bind(listen).await.with_context(|| format!("listen {listen}"))?;
    tokio::spawn(serve(shared.clone(), listener, token));
    println!(
        "rth-host: controller owner for {agent} · endpoint http://{listen} (token {}) · voice {voice}\n\
         native stop: SIGUSR1 or Ctrl-X · mute: m (stdin `mute on` / `mute off`) · call: v (stdin `call start` / `call end`) · quit: SIGINT, SIGTERM or q",
        token_file.display()
    );
    let _ = std::io::stdout().flush();
    shared.note("started", serde_json::json!({"agent": agent.to_string(), "listen": listen.to_string(), "pid": std::process::id(), "voice": voice, "voice_policy": policy}));

    native_controls(ctl_tx.clone(), shared.clone(), args.flag("--lifeline-stdin"), args.value("--mic-wav").is_some())?;
    let mut capture = CallCapture(call.clone());
    let playout = move |_n: u64| -> Box<dyn Playout> { Box::new(CallPlayout(call.clone())) };
    run_link(shared.clone(), agent, label, &mut capture, playout, opts, ctl_rx).await?;
    let _ = std::io::stdout().flush();

    let _ = ratatui::crossterm::terminal::disable_raw_mode();
    Ok(())
}

fn native_controls(_ctl: mpsc::Sender<Control>, shared: Arc<Shared>, lifeline: bool, mic_wav: bool) -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut usr1 = signal(SignalKind::user_defined1())?;
    let mut int = signal(SignalKind::interrupt())?;
    let mut term = signal(SignalKind::terminate())?;
    let s = shared.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(()) = usr1.recv() => s.native_intent(false, "SIGUSR1"),
                Some(()) = int.recv() => s.native_intent(true, "SIGINT"),
                Some(()) = term.recv() => s.native_intent(true, "SIGTERM"),
            }
        }
    });
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    if lifeline && interactive {
        bail!("--lifeline-stdin needs stdin to be a pipe from the parent, not a terminal");
    }
    std::thread::spawn(move || {
        if interactive {
            use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};
            if ratatui::crossterm::terminal::enable_raw_mode().is_err() {
                return;
            }
            let mut talk_down = false;
            loop {
                let Ok(TermEvent::Key(k)) = event::read() else { continue };
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('x') if ctrl => shared.native_intent(false, "ctrl-x"),
                    KeyCode::Char('t') => {
                        talk_down = !talk_down;
                        let _ = shared.talk_request(talk_down, "key-t");
                        continue;
                    }
                    KeyCode::Char('m') => {
                        let on = !shared.urgent.mute.requested();
                        shared.mute_request(on, "key-m");
                        continue;
                    }
                    KeyCode::Char('v') => {
                        let on = !shared.call.lock().unwrap().as_ref().is_some_and(|c| c.desired());
                        let _ = shared.call_request(on, "key-v");
                        continue;
                    }
                    KeyCode::Char('c') if ctrl => shared.native_intent(true, "ctrl-c"),
                    KeyCode::Char('q') | KeyCode::Esc => shared.native_intent(true, "q"),
                    _ => continue,
                }

            }
        } else {
            let mut line = String::new();
            while std::io::stdin().read_line(&mut line).map(|n| n > 0).unwrap_or(false) {
                match line.trim() {
                    "stop" => shared.native_intent(false, "stdin"),
                    "quit" | "q" => shared.native_intent(true, "stdin"),
                    "end-stream" => {
                        let _ = shared.end_stream_request(mic_wav);
                    }
                    "talk down" => {
                        let _ = shared.talk_request(true, "stdin");
                    }
                    "talk up" => {
                        let _ = shared.talk_request(false, "stdin");
                    }
                    "mute on" => {
                        shared.mute_request(true, "stdin");
                    }
                    "mute off" => {
                        shared.mute_request(false, "stdin");
                    }
                    "call start" => {
                        let _ = shared.call_request(true, "stdin");
                    }
                    "call end" => {
                        let _ = shared.call_request(false, "stdin");
                    }
                    "mic-modes" => {
                        #[cfg(target_os = "macos")]
                        let shown = rth::vpio::show_mic_modes();
                        #[cfg(not(target_os = "macos"))]
                        let shown = false;
                        shared.note("mic_modes_ui", serde_json::json!({"shown": shown}));
                    }
                    l if l.starts_with("say ") => {

                        if let Err(e) = shared.say_request(l[4..].trim()) {
                            shared.note("say_refused", serde_json::json!({"error": e}));
                        }
                    }
                    _ => {}
                }
                line.clear();
            }

            if lifeline {
                shared.native_intent(true, "stdin-eof");
            }
        }
    });
    Ok(())
}
