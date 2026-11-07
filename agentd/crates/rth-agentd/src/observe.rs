
use crate::{Agent, Event};
use rth_protocol::observe::{Reply, Request};
use std::sync::Arc;
use std::time::Duration;

pub fn enabled() -> bool { std::env::var("RTH_OBSERVE").as_deref()==Ok("1") }

pub async fn handle(agent: &Arc<Agent>, conn: u64, request: Request, cause: String) {

    if agent
        .controller
        .lock()
        .await
        .as_ref()
        .is_none_or(|(id, _)| *id != conn)
    {
        return;
    }
    let Some(store) = agent.observe.clone() else {
        agent
            .emit_to(
                Some(conn),
                Event::ObserveReply {
                    reply: Reply::Unavailable,
                },
                Some(cause),
            )
            .await;
        return;
    };
    let result = tokio::task::spawn_blocking(move || {
        let mut receiver = store.lock().unwrap();
        let session = match &request {
            Request::Observe { observation } => &observation.session,
            Request::Lookup { session, .. }
            | Request::Resume { session }
            | Request::Memory { session, .. } => session,
        };
        if *session != receiver.snapshot().session {
            return (Reply::Conflict, None);
        }
        let reply = match request {
            Request::Observe { observation } => match receiver.accept(observation) {
                Ok(receipt) => Reply::Accepted { receipt },
                Err(_) => Reply::Conflict,
            },
            Request::Lookup { event_id, .. } => receiver
                .lookup(&event_id)
                .map(|receipt| Reply::Accepted { receipt })
                .unwrap_or(Reply::Pending),
            Request::Resume { .. } => Reply::Snapshot {
                snapshot: receiver.snapshot(),
            },
            Request::Memory { event_id, .. } => receiver
                .memory(&event_id)
                .map(|receipt| Reply::Memory { receipt })
                .unwrap_or(Reply::Pending),
        };
        let observation = match &reply {
            Reply::Accepted { receipt } => receiver
                .history()
                .iter()
                .find(|i| i.event_id == receipt.event_id)
                .cloned(),
            _ => None,
        };
        (reply, observation)
    })
    .await;
    let (mut reply, observation) = result.unwrap_or((Reply::Unavailable, None));
    if let Some(observation) = observation {

        agent
            .record(Event::ObserveStored { observation }, None)
            .await;
        if agent.journal.barrier().await.is_err() {
            reply = Reply::Unavailable;
        }
    }
    agent
        .emit_to(Some(conn), Event::ObserveReply { reply }, Some(cause))
        .await;
    agent.observe_wake.notify_one();
}

pub fn attach(agent: &Arc<Agent>) {
    let Some(store) = agent.observe.clone() else {
        return;
    };
    let agent = Arc::downgrade(agent);
    tokio::spawn(async move {
        let mut delay = 100u64;
        loop {
            let Some(agent) = agent.upgrade() else {
                break;
            };
            let pending = { store.lock().unwrap().next_memory() };
            if let (Some(item), Some(memory)) = (pending, agent.memory.clone()) {
                let output =
                    tokio::task::spawn_blocking(move || memory.observe_conversation(&item)).await;
                if let Ok(Ok(receipt)) = output {
                    let save = store.clone();
                    let copy = receipt.clone();
                    let saved = tokio::task::spawn_blocking(move || {
                        save.lock().unwrap().record_memory(copy)
                    })
                    .await;
                    if matches!(saved, Ok(Ok(()))) {
                        let conn = agent.controller.lock().await.as_ref().map(|(id, _)| *id);
                        if let Some(conn) = conn {
                            agent
                                .emit_to(
                                    Some(conn),
                                    Event::ObserveReply {
                                        reply: Reply::Memory { receipt },
                                    },
                                    None,
                                )
                                .await;
                        }
                        delay = 100;
                        continue;
                    }
                }
                delay = (delay * 2).min(30_000);
            } else {
                delay = 30_000;
            }
            tokio::select! {
                _=agent.observe_wake.notified()=>{},
                _=tokio::time::sleep(Duration::from_millis(delay))=>{},
            }
        }
    });
}

#[cfg(test)]
#[path = "observe_tests.rs"]
mod tests;
