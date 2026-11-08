

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use rth_protocol::{Envelope, Event, ResponseStatus, Role, Source};

use crate::Msg;

const MAX_EVENTS: usize = 500;

struct Row {
    who: &'static str,
    text: String,
}

pub struct App {
    status: String,
    boot: Option<String>,
    agent: Option<String>,
    role: Option<Role>,
    uptime_ms: u64,
    linked: bool,
    chat: Vec<Row>,
    events: VecDeque<Line<'static>>,
    input: String,
    requests: u64,

    submitted: HashMap<String, Instant>,
    response_request: HashMap<String, String>,
    response_row: HashMap<String, usize>,
    streaming: Vec<String>,
    last_turn_ms: Option<f64>,
    turns: u64,
    audio_chunks: u64,
    audio_refused: u64,
}

impl App {
    pub fn new() -> Self {
        Self {
            status: "starting".into(),
            boot: None,
            agent: None,
            role: None,
            uptime_ms: 0,
            linked: false,
            chat: Vec::new(),
            events: VecDeque::new(),
            input: String::new(),
            requests: 0,
            submitted: HashMap::new(),
            response_request: HashMap::new(),
            response_row: HashMap::new(),
            streaming: Vec::new(),
            last_turn_ms: None,
            turns: 0,
            audio_chunks: 0,
            audio_refused: 0,
        }
    }

    pub fn type_char(&mut self, c: char) {
        self.input.push(c);
    }

    pub fn backspace(&mut self) {
        self.input.pop();
    }

    pub fn take_input(&mut self) -> Option<String> {
        let text = self.input.trim().to_string();
        self.input.clear();
        (!text.is_empty() && self.linked && self.role == Some(Role::Controller)).then_some(text)
    }

    pub fn next_request_id(&mut self) -> String {
        self.requests += 1;
        format!("q{}", self.requests)
    }

    pub fn active_response(&self) -> Option<String> {
        self.streaming.last().cloned()
    }

    pub fn note_error(&mut self, text: &str) {
        self.note(text, Color::Red);
    }

    pub fn sent(&mut self, env: &Envelope, now: Instant) {
        if let Event::UserText { request_id, text, .. } = &env.event {
            self.chat.push(Row { who: "you", text: text.clone() });
            self.submitted.insert(request_id.clone(), now);
        }
        self.push_event(env);
    }

    fn first_text(&mut self, response_id: &str, now: Instant) {
        let request = self.response_request.get(response_id);
        if let Some(sent) = request.and_then(|q| self.submitted.remove(q)) {
            self.last_turn_ms = Some(now.duration_since(sent).as_secs_f64() * 1000.0);
            self.turns += 1;
        }
    }

    pub fn apply(&mut self, msg: Msg, now: Instant) {
        match msg {
            Msg::Status(s) => self.note(&s, Color::Yellow),
            Msg::Booted { boot_ms, agent_started } => {
                self.boot = Some(format!("{boot_ms} ms"));
                let a = if agent_started { "agent started" } else { "agent already living" };
                self.note(&format!("microVM ready in {boot_ms} ms · {a}"), Color::Green);
            }
            Msg::Ready { attachment_id, role } => {
                self.linked = true;
                self.role = Some(role);
                let r = if role == Role::Controller { "controller" } else { "observer (another app controls the agent)" };
                self.note(&format!("attached {attachment_id} as {r}"), Color::Green);
            }
            Msg::Disconnected(why) => {
                self.linked = false;
                self.streaming.clear();
                self.note(&why, Color::Red);
            }
            Msg::Incoming(env) => self.incoming(env, now),
            Msg::Audio { accepted } => {
                self.audio_chunks += 1;
                self.audio_refused += u64::from(!accepted);
            }
        }
    }

    fn incoming(&mut self, env: Envelope, now: Instant) {
        match &env.event {
            Event::Hello { version, boot_id, .. } => {
                self.agent = Some(format!("agentd {version} · boot {boot_id}"));
            }
            Event::Heartbeat { uptime_ms, .. } => {

                self.uptime_ms = *uptime_ms;
                return;
            }
            Event::ResponseStarted { response_id, request_id, .. } => {
                self.chat.push(Row { who: "rth", text: String::new() });
                self.response_row.insert(response_id.clone(), self.chat.len() - 1);
                self.response_request.insert(response_id.clone(), request_id.clone());
                self.streaming.push(response_id.clone());
            }
            Event::AgentDelta { response_id, text } => {
                self.first_text(response_id, now);
                if let Some(&row) = self.response_row.get(response_id) {
                    self.chat[row].text.push_str(text);
                }
            }

            Event::AgentReplace { response_id, text } => {
                if !text.is_empty() {
                    self.first_text(response_id, now);
                }
                if let Some(&row) = self.response_row.get(response_id) {
                    self.chat[row].text = text.clone();
                }
            }
            Event::AgentDone { response_id, status } => {
                self.streaming.retain(|r| r != response_id);
                if *status != ResponseStatus::Completed {
                    if let Some(&row) = self.response_row.get(response_id) {
                        self.chat[row].text.push_str(if *status == ResponseStatus::Cancelled { " ⨯ interrupted" } else { " ⨯ failed" });
                    }
                }
            }
            Event::JournalFault { detail } => self.note(&format!("journal fault: {detail}"), Color::Red),
            Event::Rejected { reason } => self.note(&format!("agent rejected: {reason}"), Color::Red),
            _ => {}
        }
        self.push_event(&env);
    }

    fn note(&mut self, text: &str, color: Color) {
        self.status = text.to_string();
        self.push_line(Line::from(Span::styled(format!("· {text}"), Style::default().fg(color))));
    }

    fn push_event(&mut self, env: &Envelope) {
        let (tag, color) = match env.source {
            Source::Host => ("host ", Color::Cyan),
            Source::Guest => ("guest", Color::Magenta),
        };
        let mut json = serde_json::to_value(&env.event).unwrap_or_default();
        let kind = json.get("kind").and_then(|k| k.as_str()).unwrap_or("?").to_string();
        if let Some(obj) = json.as_object_mut() {
            obj.remove("kind");
        }
        let seq = if env.seq == 0 { "~".to_string() } else { env.seq.to_string() };
        let line = Line::from(vec![
            Span::styled(format!("{:>9.3}s ", env.t_ns as f64 / 1e9), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{tag} #{seq:<4} "), Style::default().fg(color)),
            Span::styled(format!("{kind:<15} "), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(json.to_string(), Style::default().fg(Color::Gray)),
        ]);
        self.push_line(line);
    }

    fn push_line(&mut self, line: Line<'static>) {
        self.events.push_back(line);
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(3)])
        .split(f.area());

    let green = Style::default().fg(Color::Green);
    let header = Line::from(vec![
        Span::styled(" RTH ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(" real time harness · microVM rth-core ", green),
        Span::raw(format!(
            "· boot {} · {} · up {}s · {} · submit→first delta {} ({} turns) · audio {} chunks ({} refused, saved to WAV, no playback)",
            app.boot.as_deref().unwrap_or("…"),
            app.agent.as_deref().unwrap_or("agent …"),
            app.uptime_ms / 1000,
            if app.linked { "linked" } else { "offline" },
            app.last_turn_ms.map(|m| format!("{m:.1} ms")).unwrap_or_else(|| "–".into()),
            app.turns,
            app.audio_chunks,
            app.audio_refused,
        )),
    ]);
    f.render_widget(
        Paragraph::new(vec![header, Line::styled(format!(" {}", app.status), Style::default().fg(Color::DarkGray))])
            .block(Block::default().borders(Borders::BOTTOM).border_style(green)),
        rows[0],
    );

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(rows[1]);

    let chat: Vec<Line> = app
        .chat
        .iter()
        .flat_map(|row| {
            let color = if row.who == "you" { Color::Cyan } else { Color::Green };
            let who = Span::styled(format!("{}> ", row.who), Style::default().fg(color).add_modifier(Modifier::BOLD));
            [Line::from(vec![who, Span::raw(row.text.clone())]), Line::raw("")]
        })
        .collect();

    let inner_w = cols[0].width.saturating_sub(2).max(1) as usize;
    let chat_h = cols[0].height.saturating_sub(2) as usize;
    let wrapped: usize = chat.iter().map(|l| l.width().div_ceil(inner_w).max(1)).sum();
    let scroll = wrapped.saturating_sub(chat_h) as u16;
    f.render_widget(
        Paragraph::new(chat)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(Block::default().title(" conversation ").borders(Borders::ALL).border_style(green)),
        cols[0],
    );

    let ev_h = cols[1].height.saturating_sub(2) as usize;
    let skip = app.events.len().saturating_sub(ev_h);
    f.render_widget(
        Paragraph::new(app.events.iter().skip(skip).cloned().collect::<Vec<_>>())
            .block(Block::default().title(" event bus · every action, every decision ").borders(Borders::ALL).border_style(green)),
        cols[1],
    );

    f.render_widget(
        Paragraph::new(format!("› {}▌", app.input)).block(
            Block::default()
                .title(" say hi · Enter send · Ctrl-X interrupt · Esc quit (agent keeps living) ")
                .borders(Borders::ALL)
                .border_style(green),
        ),
        rows[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_protocol::{Clock, Stamper};

    struct Zero;
    impl Clock for Zero {
        fn now_ns(&self) -> u64 {
            0
        }
    }

    fn guest(s: &mut Stamper, event: Event) -> Msg {
        Msg::Incoming(s.stamp(event, None))
    }

    #[test]
    fn overlapping_turns_keep_their_own_rows_and_timing() {
        let mut app = App::new();
        let mut host = Stamper::new(Source::Host, "h", Box::new(Zero));
        let mut g = Stamper::new(Source::Guest, "g", Box::new(Zero));
        let t0 = Instant::now();
        let started = |r: &str, q: &str| Event::ResponseStarted { response_id: r.into(), request_id: q.into(), utterance_id: None };
        let delta = |r: &str, t: &str| Event::AgentDelta { response_id: r.into(), text: t.into() };

        app.sent(&host.stamp(Event::UserText { request_id: "q1".into(), text: "a".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None), t0);
        app.apply(guest(&mut g, started("r1", "q1")), t0);
        app.apply(guest(&mut g, delta("r1", "A1 ")), t0);

        app.sent(&host.stamp(Event::UserText { request_id: "q2".into(), text: "b".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None), t0);
        app.apply(guest(&mut g, started("r2", "q2")), t0);
        app.apply(guest(&mut g, delta("r1", "A2")), t0);
        app.apply(guest(&mut g, Event::AgentDone { response_id: "r1".into(), status: ResponseStatus::Completed }), t0);
        app.apply(guest(&mut g, delta("r2", "B1")), t0);

        let replies: Vec<&str> = app.chat.iter().filter(|r| r.who == "rth").map(|r| r.text.as_str()).collect();
        assert_eq!(replies, ["A1 A2", "B1"]);

        app.apply(guest(&mut g, Event::AgentReplace { response_id: "r2".into(), text: "4".into() }), t0);
        app.apply(guest(&mut g, delta("r2", " euros")), t0);
        assert_eq!(app.chat.iter().filter(|r| r.who == "rth").last().unwrap().text, "4 euros");
        assert_eq!(app.turns, 2, "each request timed once, by its own first delta");
        assert_eq!(app.active_response().as_deref(), Some("r2"));

        app.sent(&host.stamp(Event::UserText { request_id: "q3".into(), text: "c".into(), speaker: Default::default(), reply_voice: None, effort: None, reply_max_tokens: None }, None), t0);
        app.apply(guest(&mut g, started("r3", "q3")), t0);
        app.apply(guest(&mut g, Event::AgentReplace { response_id: "r3".into(), text: String::new() }), t0);
        assert_eq!(app.turns, 2, "an empty replace is not first text");
        app.apply(guest(&mut g, Event::AgentReplace { response_id: "r3".into(), text: "4".into() }), t0);
        assert_eq!((app.turns, app.chat.iter().filter(|r| r.who == "rth").last().unwrap().text.as_str()), (3, "4"));
    }
}
