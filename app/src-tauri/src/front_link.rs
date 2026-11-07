
use crate::conversation_log::observe::{opaque, Reply, Request, SCHEMA};
use serde_json::Value;
use std::{
    io,
    sync::{mpsc, Mutex},
    time::Duration,
};
const WAIT: Duration = Duration::from_secs(2);
struct Pending {
    id: String,
    tx: mpsc::SyncSender<io::Result<Reply>>,
}
struct LegacyPending {
    command: String,
    frame: String,
    tx: mpsc::SyncSender<io::Result<String>>,
}
#[derive(Default)]
struct State {
    ready: bool,
    closed: bool,
    pending: Option<Pending>,
    legacy: Option<LegacyPending>,
}
pub struct Link {
    session: String,
    stamp: fn(Request) -> Value,
    state: Mutex<State>,
}
impl Link {
    pub fn new(session: &str, stamp: fn(Request) -> Value) -> Self {
        Self {
            session: session.into(),
            stamp,
            state: Mutex::new(State {
                closed: !opaque(session),
                ..State::default()
            }),
        }
    }
    pub fn ready(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.ready && !s.closed
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn close(&self) {
        let mut s = self.state.lock().unwrap();
        s.closed = true;
        s.ready = false;

        s.pending = None;
        s.legacy = None;
    }
    pub fn on_frame(&self, f: &Value) -> bool {
        let kind = f["kind"].as_str().unwrap_or("");
        let mut s = self.state.lock().unwrap();
        if kind == "observe_ready" {
            if f["schema"] != SCHEMA || f["session"] != self.session {
                s.closed = true;
                s.pending = None;
                s.legacy = None;
            }
            if !s.closed {
                s.ready = true;
            }
            return true;
        }
        let acceptance = s.legacy.as_ref().and_then(|p| {
            if (kind == "response_started" && f["request_id"] == p.command)
                || (kind == "command_admitted"
                    && f["command_id"] == p.command
                    && f["duplicate"] == true)
            {
                Some(
                    f["response_id"]
                        .as_str()
                        .filter(|r| opaque(r))
                        .map(String::from)
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid backend response identity",
                            )
                        }),
                )
            } else if kind == "rejected" && f["cause"] == p.frame {
                Some(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "legacy command refused",
                )))
            } else {
                None
            }
        });
        if let Some(result) = acceptance {
            if let Some(p) = s.legacy.take() {
                let _ = p.tx.try_send(result);
            }

            return false;
        }
        if kind != "observe_reply" && kind != "rejected" {
            return false;
        }
        let cause = f["cause"].as_str().unwrap_or("");
        let mine = s.pending.as_ref().is_some_and(|p| p.id == cause);
        if mine {
            let p = s.pending.take().expect("matched");
            let reply = if kind == "observe_reply" {

                serde_json::from_value::<Reply>(f["reply"].clone())
                    .and_then(|reply| {
                        if serde_json::to_value(&reply)? == f["reply"] {
                            Ok(reply)
                        } else {
                            Err(serde::de::Error::custom("unexpected Observe fields"))
                        }
                    })
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid Observe reply")
                    })
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Observe refused",
                ))
            };
            let _ = p.tx.try_send(reply);
            return true;
        }
        kind == "observe_reply"
            || cause
                .strip_prefix("rth-front-observe-")
                .is_some_and(|n| n.parse::<u64>().is_ok())
    }

    pub fn legacy(
        &self,
        command: &str,
        frame: &str,
        send: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<String> {
        self.legacy_with_wait(command, frame, WAIT, send)
    }
    fn legacy_with_wait(
        &self,
        command: &str,
        frame: &str,
        wait: Duration,
        send: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<String> {
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut s = self.state.lock().unwrap();
            if s.closed || !s.ready {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "legacy attachment unavailable",
                ));
            }
            if s.legacy.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "legacy command pending",
                ));
            }
            s.legacy = Some(LegacyPending {
                command: command.into(),
                frame: frame.into(),
                tx,
            });
        }
        let result = send().and_then(|()| {
            rx.recv_timeout(wait).map_err(|e| {
                io::Error::new(
                    if matches!(e, mpsc::RecvTimeoutError::Timeout) {
                        io::ErrorKind::TimedOut
                    } else {
                        io::ErrorKind::NotConnected
                    },
                    "legacy acceptance unknown",
                )
            })?
        });
        let mut s = self.state.lock().unwrap();
        if s.legacy
            .as_ref()
            .is_some_and(|p| p.command == command && p.frame == frame)
        {
            s.legacy = None;
        }
        result
    }
    pub fn exchange(
        &self,
        request: Request,
        send: impl FnOnce(Value) -> io::Result<()>,
    ) -> io::Result<Reply> {
        self.exchange_with_wait(request, WAIT, send)
    }
    fn exchange_with_wait(
        &self,
        request: Request,
        wait: Duration,
        send: impl FnOnce(Value) -> io::Result<()>,
    ) -> io::Result<Reply> {
        let session = match &request {
            Request::Observe { observation } => &observation.session,
            Request::Lookup { session, .. }
            | Request::Resume { session }
            | Request::Memory { session, .. } => session,
        };
        if *session != self.session {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign Observe session",
            ));
        }
        let frame = (self.stamp)(request);
        let id = frame["id"].as_str().expect("host envelope").to_string();
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut s = self.state.lock().unwrap();
            if s.closed || !s.ready {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Observe not negotiated",
                ));
            }
            if s.pending.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Observe already pending",
                ));
            }
            s.pending = Some(Pending { id: id.clone(), tx });
        }
        let result = send(frame).and_then(|()| {
            rx.recv_timeout(wait).map_err(|e| {
                io::Error::new(
                    if matches!(e, mpsc::RecvTimeoutError::Timeout) {
                        io::ErrorKind::TimedOut
                    } else {
                        io::ErrorKind::NotConnected
                    },
                    "Observe reply unavailable",
                )
            })?
        });
        let mut s = self.state.lock().unwrap();
        if s.pending.as_ref().is_some_and(|p| p.id == id) {
            s.pending = None;
        }
        result
    }
}
#[cfg(test)]
#[path = "front_link_tests.rs"]
mod tests;
