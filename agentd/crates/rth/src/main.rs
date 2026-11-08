

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ratatui::crossterm::event::{self, Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rth_protocol::io::read_frame;
use rth_protocol::{AGENT_PORT, Envelope, Event, MonotonicClock, PROTOCOL_VERSION, Role, Source, Stamper, producer_name};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;

mod ui;
mod vm;

use rth::audio::{AudioStore, Chunk, Playback, data_dir};
use ui::App;

pub enum Msg {
    Status(String),
    Booted { boot_ms: u128, agent_started: bool },
    Ready { attachment_id: String, role: Role },
    Incoming(Envelope),

    Audio { accepted: bool },
    Disconnected(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("stop") {
        let status = std::process::Command::new("msb").args(["stop", vm::VM_NAME]).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
    let restart = args.iter().any(|a| a == "--restart");

    let agent_dir = if vm::managed() { PathBuf::new() } else { agent_dir()? };

    let producer = producer_name("host");

    let (link_tx, mut link_rx) = mpsc::channel::<Msg>(4096);
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(64);
    let (out_tx, out_rx) = mpsc::channel::<Envelope>(64);

    std::thread::spawn(move || {
        loop {
            if let Ok(TermEvent::Key(k)) = event::read() {
                if k.kind == KeyEventKind::Press && key_tx.blocking_send(k).is_err() {
                    break;
                }
            }
        }
    });

    tokio::spawn(boot_and_link(agent_dir, restart, producer.clone(), link_tx, out_rx));

    let mut terminal = ratatui::init();
    let mut app = App::new();
    let mut stamper = Stamper::new(Source::Host, producer, Box::new(MonotonicClock::start()));
    let mut redraw = tokio::time::interval(Duration::from_millis(33));
    let result: Result<()> = loop {
        tokio::select! {
            Some(k) = key_rx.recv() => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                let command = match k.code {
                    KeyCode::Esc => break Ok(()),
                    KeyCode::Char('c') if ctrl => break Ok(()),
                    KeyCode::Char('x') if ctrl => app.active_response().map(|response_id| Event::Cancel { response_id }),
                    KeyCode::Enter => app.take_input().map(|text| Event::UserText { request_id: app.next_request_id(), text, speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }),
                    KeyCode::Backspace => {
                        app.backspace();
                        None
                    }
                    KeyCode::Char(c) => {
                        app.type_char(c);
                        None
                    }
                    _ => None,
                };
                if let Some(event) = command {
                    let env = stamper.stamp(event, None);
                    app.sent(&env, Instant::now());
                    if out_tx.try_send(env).is_err() {
                        app.note_error("link busy; input not sent");
                    }
                }
            }
            Some(msg) = link_rx.recv() => app.apply(msg, Instant::now()),
            _ = redraw.tick() => {
                if let Err(e) = terminal.draw(|f| ui::draw(f, &app)) {
                    break Err(e.into());
                }
            }
        }
    };
    ratatui::restore();
    println!("rth: app closed. The agent keeps living in microVM '{}' (stop it with `rth stop`).", vm::VM_NAME);
    result
}

fn agent_dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("RTH_AGENTD_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/aarch64-unknown-linux-musl/release"),
    };
    let dir = dir.canonicalize().with_context(|| format!("agent dir {}", dir.display()))?;
    if !dir.join("rth-agentd").exists() {
        bail!(
            "no rth-agentd in {}. Build it: cargo build -p rth-agentd --release --target aarch64-unknown-linux-musl",
            dir.display()
        );
    }
    Ok(dir)
}

struct UiSink {
    tx: mpsc::Sender<Msg>,
    dropped: u64,
}

impl UiSink {
    fn send(&mut self, msg: Msg) -> bool {
        match self.tx.try_send(msg) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.dropped += 1;
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }
}

async fn boot_and_link(
    agent_dir: PathBuf,
    restart: bool,
    producer: String,
    tx: mpsc::Sender<Msg>,
    mut out_rx: mpsc::Receiver<Envelope>,
) {
    let mut ui = UiSink { tx, dropped: 0 };
    let (playback, warning) = Playback::from_env();
    if let Some(w) = warning {
        ui.send(Msg::Status(w));
    }

    let mut audio = match AudioStore::new(data_dir().join("audio")) {
        Ok(mut store) => {
            store.playback = playback;
            Some(store)
        }
        Err(e) => {
            ui.send(Msg::Status(format!("audio store unavailable ({e}); audio chunks will be counted only")));
            None
        }
    };
    ui.send(Msg::Status(format!("booting microVM {} …", vm::VM_NAME)));
    match vm::ensure(&agent_dir, restart).await {
        Ok(b) => {
            ui.send(Msg::Booted { boot_ms: b.boot_ms, agent_started: b.agent_started });
        }
        Err(e) => {
            ui.send(Msg::Disconnected(format!("VM boot failed: {e:#}")));
            return;
        }
    }

    let mut attempt = 0u64;

    loop {
        attempt += 1;
        let attachment_id = format!("{producer}-a{attempt}");
        let Some((mut reader, mut write)) = attach(&mut ui, &attachment_id).await else { return };
        let mut buf = Vec::new();
        let why = loop {
            tokio::select! {
                frame = read_frame(&mut reader, &mut buf) => match frame {
                    Ok(line) => match Envelope::parse_known(&line, Source::Guest) {

                        Ok(None) => continue,
                        Ok(Some(env)) => {
                            if !store_audio(&mut audio, &mut ui, &env) {
                                continue;
                            }
                            if !ui.send(Msg::Incoming(env)) {
                                return;
                            }
                        }
                        Err(e) => {
                            ui.send(Msg::Status(format!("rejected frame from agent: {e}")));
                        }
                    },
                    Err(e) => break e.to_string(),
                },
                out = out_rx.recv() => match out {
                    Some(env) => {

                        if let (Event::Cancel { response_id }, Some(store)) = (&env.event, audio.as_mut()) {
                            let _ = store.end(response_id);
                        }
                        if let Err(e) = write.write_all(env.to_line().as_bytes()).await {
                            break format!("send failed: {e}");
                        }
                    }
                    None => return,
                },
            }
        };
        if ui.dropped > 0 {
            ui.send(Msg::Status(format!("view skipped {} events (the journal has them)", ui.dropped)));
            ui.dropped = 0;
        }
        if !ui.send(Msg::Disconnected(format!("{why} · reattaching"))) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn store_audio(audio: &mut Option<AudioStore>, ui: &mut UiSink, env: &Envelope) -> bool {
    match &env.event {
        Event::AgentAudio { response_id, chunk, sample_offset, sample_rate, format, data } => {
            let accepted = match audio.as_mut() {
                Some(store) => store.chunk(response_id, *chunk, *sample_offset, *sample_rate, format, data) == Chunk::Accepted,
                None => false,
            };
            ui.send(Msg::Audio { accepted });
            false
        }
        Event::AgentDone { response_id, .. } => {
            match audio.as_mut().and_then(|s| s.end(response_id)) {
                Some(Ok(saved)) => {
                    let secs = saved.samples as f64 / f64::from(saved.sample_rate.max(1));
                    ui.send(Msg::Status(format!("saved {} ({secs:.1} s, {} gaps)", saved.path.display(), saved.gaps)));
                }
                Some(Err(e)) => {
                    ui.send(Msg::Status(format!("could not finish the WAV for {response_id}: {e}")));
                }
                None => {}
            }
            true
        }
        _ => true,
    }
}

async fn attach(ui: &mut UiSink, attachment_id: &str) -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let mut stamper = Stamper::new(Source::Host, attachment_id, Box::new(MonotonicClock::start()));
    loop {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", AGENT_PORT)).await {
            let _ = stream.set_nodelay(true);
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut buf = Vec::new();
            match handshake(&mut reader, &mut write, &mut buf, &mut stamper, attachment_id, ui).await {
                Ok(true) => return Some((reader, write)),
                Ok(false) => {}
                Err(reason) => {
                    ui.send(Msg::Disconnected(reason));
                    return None;
                }
            }
        }
        if ui.tx.is_closed() {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn handshake(
    reader: &mut BufReader<OwnedReadHalf>,
    write: &mut OwnedWriteHalf,
    buf: &mut Vec<u8>,
    stamper: &mut Stamper,
    attachment_id: &str,
    ui: &mut UiSink,
) -> Result<bool, String> {
    let timeout = Duration::from_secs(2);
    let Ok(Ok(line)) = tokio::time::timeout(timeout, read_frame(reader, buf)).await else { return Ok(false) };
    let hello = match Envelope::parse_from(&line, Source::Guest) {
        Ok(env @ Envelope { event: Event::Hello { .. }, .. }) => env,
        Ok(_) => return Ok(false),
        Err(e) => return Err(format!("agent speaks an incompatible protocol: {e}")),
    };
    ui.send(Msg::Incoming(hello));

    let attach = stamper.stamp(
        Event::Attach { protocol: PROTOCOL_VERSION, attachment_id: attachment_id.to_string(), role: Role::Controller },
        None,
    );
    if write.write_all(attach.to_line().as_bytes()).await.is_err() {
        return Ok(false);
    }

    loop {
        let Ok(Ok(line)) = tokio::time::timeout(timeout, read_frame(reader, buf)).await else { return Ok(false) };
        let Ok(env) = Envelope::parse_from(&line, Source::Guest) else { continue };
        match &env.event {
            Event::Ready { attachment_id: id, role } if id == attachment_id => {
                ui.send(Msg::Ready { attachment_id: id.clone(), role: *role });
                ui.send(Msg::Incoming(env));
                return Ok(true);
            }
            Event::Rejected { reason } => return Err(format!("agent rejected attach: {reason}")),
            _ => {
                ui.send(Msg::Incoming(env));
            }
        }
    }
}
