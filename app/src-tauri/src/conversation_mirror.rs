
use super::{
    observe::{MemoryReceipt, Observation, Receipt, Reply, Request, Snapshot},
    Store,
};
use std::{
    io,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    thread::JoinHandle,
};
pub trait Transport: Send + 'static {

    fn observe(&mut self, observation: &Observation) -> io::Result<Receipt>;
    fn resume(&mut self, _session: &str) -> io::Result<Snapshot> {
        Err(io::Error::other("resume unsupported"))
    }
    fn memory(&mut self, _session: &str, _event_id: &str) -> io::Result<Option<MemoryReceipt>> {
        Ok(None)
    }
}

pub struct RpcTransport<F> {
    exchange: F,
    session: String,
}
impl<F> RpcTransport<F>
where
    F: FnMut(Request) -> io::Result<Reply> + Send + 'static,
{
    pub fn new(schema: &str, session: &str, exchange: F) -> io::Result<Self> {
        if schema != super::observe::SCHEMA || !super::observe::opaque(session) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Observe not negotiated",
            ));
        }
        Ok(Self {
            exchange,
            session: session.into(),
        })
    }
}
impl<F> Transport for RpcTransport<F>
where
    F: FnMut(Request) -> io::Result<Reply> + Send + 'static,
{
    fn observe(&mut self, item: &Observation) -> io::Result<Receipt> {
        if item.session != self.session {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign mirror session",
            ));
        }
        let reply = (self.exchange)(Request::Lookup {
            session: item.session.clone(),
            event_id: item.event_id.clone(),
        })?;
        let reply = if matches!(reply, Reply::Pending) {
            (self.exchange)(Request::Observe {
                observation: item.clone(),
            })?
        } else {
            reply
        };
        match reply {
            Reply::Accepted { receipt } => Ok(receipt),
            Reply::Conflict => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "mirror divergence",
            )),
            _ => Err(io::Error::other("mirror not ready")),
        }
    }
    fn resume(&mut self, session: &str) -> io::Result<Snapshot> {
        if session != self.session {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign mirror session",
            ));
        }
        match (self.exchange)(Request::Resume {
            session: session.into(),
        })? {
            Reply::Snapshot { snapshot } => Ok(snapshot),
            _ => Err(io::Error::other("mirror reconciliation unavailable")),
        }
    }
    fn memory(&mut self, session: &str, event_id: &str) -> io::Result<Option<MemoryReceipt>> {
        if session != self.session {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "foreign mirror session",
            ));
        }
        match (self.exchange)(Request::Memory {
            session: session.into(),
            event_id: event_id.into(),
        })? {
            Reply::Memory { receipt } => Ok(Some(receipt)),
            Reply::Pending => Ok(None),
            _ => Err(io::Error::other("memory receipt unavailable")),
        }
    }
}
enum Work {
    Observe(Observation),
    Resume(String),
    Memory(String, String),
}
enum Update {
    Observed(Receipt),
    Resumed(Snapshot),
    Memory(Option<MemoryReceipt>),
}
pub struct Client {
    tx: Option<SyncSender<Work>>,
    rx: Receiver<io::Result<Update>>,
    worker: Option<JoinHandle<()>>,
    busy: bool,
    reconciled:bool,
    resume_session:Option<String>,
    retry_at: u64,
    delay: u64,
}
impl Client {
    pub fn start(mut transport: impl Transport) -> io::Result<Self> {
        let (tx, requests) = mpsc::sync_channel::<Work>(1);
        let (results, rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("conversation-mirror".into())
            .spawn(move || {
                while let Ok(work) = requests.recv() {
                    let result = match work {
                        Work::Observe(item) => transport.observe(&item).map(Update::Observed),
                        Work::Resume(session) => transport.resume(&session).map(Update::Resumed),
                        Work::Memory(session, id) => {
                            transport.memory(&session, &id).map(Update::Memory)
                        }
                    };
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            rx,
            worker: Some(worker),
            busy: false,
            reconciled:false,
            resume_session:None,
            retry_at: 0,
            delay: 100,
        })
    }
    pub fn busy(&self) -> bool { self.busy }
    pub fn reconciled(&self)->bool {self.reconciled}
    fn send(&mut self, work: Work) -> io::Result<bool> {
        if self.busy {
            return Ok(false);
        }
        self.tx
            .as_ref()
            .ok_or_else(|| io::Error::other("mirror closed"))?
            .try_send(work)
            .map_err(|_| io::Error::other("mirror mailbox unavailable"))?;
        self.busy = true;
        Ok(true)
    }
    pub fn resume(&mut self, session: &str) -> io::Result<bool> {
        if self.busy { return Ok(false); }
        self.reconciled=false;
        self.resume_session=Some(session.into());
        self.send(Work::Resume(session.into()))
    }
    pub fn memory(&mut self, session: &str, id: &str) -> io::Result<bool> {
        self.send(Work::Memory(session.into(), id.into()))
    }

    pub fn pump(&mut self, store: &mut Store, now_ms: u64) -> io::Result<bool> {
        if self.busy {
            match self.rx.try_recv() {
                Ok(result) => {
                    self.busy = false;
                    match result {
                        Ok(Update::Observed(receipt)) => {
                            store.accept_observation(receipt)?;
                            self.delay = 100;
                        }
                        Ok(Update::Resumed(snapshot)) => {
                            store.reconcile_mirror(snapshot)?;
                            self.reconciled=true;
                            self.resume_session=None;
                            self.retry_at = 0;
                            self.delay = 100;
                        }
                        Ok(Update::Memory(Some(receipt))) => {
                            store.accept_memory_receipt(receipt)?
                        }
                        Ok(Update::Memory(None)) => {}
                        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                            store.mark_mirror_conflict()?;
                            return Err(e);
                        }
                        Err(_) => {
                            self.retry_at = now_ms.saturating_add(self.delay);
                            self.delay = (self.delay * 2).min(30_000);
                            return Ok(false);
                        }
                    }
                }
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => {
                    return Err(io::Error::other("mirror worker stopped"));
                }
            }
        }
        if now_ms < self.retry_at {
            return Ok(false);
        }
        if let Some(session)=self.resume_session.clone() {
            return self.send(Work::Resume(session));
        }
        let Some(item) = store.prepare_observation()? else {
            return Ok(false);
        };
        self.send(Work::Observe(item))
    }

    pub fn close(&mut self) -> io::Result<()> {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("mirror panicked"))?;
        }
        Ok(())
    }
}
