

use serde_json::{Value, json};
use tokio::sync::watch;

use crate::host::TaskTools;
use crate::web::BoxFuture;

pub const MAX_ANSWER_CHARS: usize = 12_000;

pub const STEER_AT: u32 = 3;

pub const STOP_AT: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelTurn {
    pub content: String,
    pub tool_calls: Vec<Call>,

    pub finish: String,
    pub usage: Option<Value>,
}

pub trait Model: Send + Sync {
    fn turn<'a>(&'a self, messages: &'a [Value], tools: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    Returned { text: String },
    Failed { reason: String },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub end: End,
    pub turns: u32,
    pub tool_calls: u32,
    pub invalid_args: u32,
    pub loop_steers: u32,
    pub max_turn_hit: bool,
    pub usage: Vec<Value>,
}

pub fn system_message(tools: &TaskTools) -> String {
    tools.host.profile.profile.notes.clone()
}

pub fn user_message(tools: &TaskTools) -> String {
    match &tools.task.base {
        Some(base) => format!("{}\n\nFolder: {}\nRelative paths are inside this folder.", tools.task.text, base.display()),
        None => tools.task.text.clone(),
    }
}

const LAST_TURN: &str = "The tool budget for this task is used up: answer now with what you have, in plain text, and say what you could not check.";

async fn cancelled(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow_and_update() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

fn loop_key(name: &str, arguments: &str) -> String {
    let args = serde_json::from_str::<Value>(arguments).map(|v| v.to_string()).unwrap_or_else(|_| arguments.to_string());
    format!("{name}\u{0}{args}")
}

fn chars(m: &Value) -> usize {
    m["content"].as_str().map(|s| s.chars().count()).unwrap_or(0) + m.get("tool_calls").map(|t| t.to_string().chars().count()).unwrap_or(0)
}

fn compact(messages: &mut [Value], budget: usize) {
    let note = json!({"elided": true, "note": "This earlier result was elided to fit the context. Call the same tool again with the same arguments to read it."}).to_string();
    let last_tool = messages.iter().rposition(|m| m["role"] == "tool");
    let mut total: usize = messages.iter().map(chars).sum();
    for (i, m) in messages.iter_mut().enumerate() {
        if total <= budget {
            break;
        }
        if m["role"] != "tool" || Some(i) == last_tool || m["content"].as_str() == Some(note.as_str()) {
            continue;
        }
        total -= chars(m);
        m["content"] = Value::String(note.clone());
        total += chars(m);
    }
}

pub async fn run(tools: &TaskTools, model: &dyn Model, cancel: &mut watch::Receiver<bool>) -> Report {
    let limits = tools.host.profile.profile.limits.clone();
    let declared = tools.declarations();
    let none = json!([]);
    let mut messages = vec![json!({"role": "system", "content": system_message(tools)}), json!({"role": "user", "content": user_message(tools)})];
    let mut rep = Report { end: End::Cancelled, turns: 0, tool_calls: 0, invalid_args: 0, loop_steers: 0, max_turn_hit: false, usage: Vec::new() };
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let finish = |mut rep: Report, end: End| {
        rep.end = end;
        rep
    };
    loop {
        if *cancel.borrow() {
            tools.end("cancelled");
            return finish(rep, End::Cancelled);
        }
        let last = rep.turns + 1 >= limits.max_turns || rep.tool_calls >= limits.max_tool_calls;
        if last && rep.turns > 0 {
            rep.max_turn_hit = true;
            messages.push(json!({"role": "user", "content": LAST_TURN}));
        }
        compact(&mut messages, limits.max_context_chars);
        let offered = if last && rep.turns > 0 { &none } else { &declared };
        rep.turns += 1;
        let turn = tokio::select! {
            biased;
            _ = cancelled(cancel) => {
                tools.end("cancelled");
                return finish(rep, End::Cancelled);
            }
            t = model.turn(&messages, offered) => t,
        };
        let turn = match turn {
            Ok(t) => t,
            Err(e) => {
                tools.end("failed");
                return finish(rep, End::Failed { reason: format!("model: {e}") });
            }
        };
        if let Some(u) = &turn.usage {
            rep.usage.push(u.clone());
        }
        if turn.tool_calls.is_empty() || offered.as_array().is_some_and(|a| a.is_empty()) {
            let text = turn.content.trim();
            let end = if turn.finish == "length" {
                End::Failed { reason: "invalid_output:length".into() }
            } else if text.is_empty() {
                End::Failed { reason: "invalid_output:empty".into() }
            } else if text.chars().count() > MAX_ANSWER_CHARS {
                End::Failed { reason: "invalid_output:too_long".into() }
            } else {
                End::Returned { text: text.to_string() }
            };
            tools.end("ended");
            return finish(rep, end);
        }
        let calls: Vec<Value> = turn.tool_calls.iter().map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}})).collect();
        let content = if turn.content.is_empty() { Value::Null } else { Value::String(turn.content.clone()) };
        messages.push(json!({"role": "assistant", "content": content, "tool_calls": calls}));
        for c in &turn.tool_calls {
            let over_budget = rep.tool_calls >= limits.max_tool_calls;
            rep.tool_calls += 1;
            let n = {
                let n = seen.entry(loop_key(&c.name, &c.arguments)).or_insert(0);
                *n += 1;
                *n
            };
            let answered = if over_budget {
                tools.refuse(&c.id, &c.name, &c.arguments, "tool_budget", "The tool budget for this task is used up; this call did not run.")
            } else if n >= STOP_AT {
                let _ = tools.refuse(&c.id, &c.name, &c.arguments, "loop_detected", "The same call was made too many times; the task stops.");
                tools.end("loop_detected");
                return finish(rep, End::Failed { reason: "loop_detected".into() });
            } else if n >= STEER_AT {
                rep.loop_steers += 1;
                tools.refuse(&c.id, &c.name, &c.arguments, "repeated_call", "You already made this exact call; its result is above. Use it, or make a different call.")
            } else {

                let mut call = Box::pin(tools.call(&c.id, &c.name, &c.arguments));
                tokio::select! {
                    biased;
                    _ = cancelled(cancel) => {
                        drop(call);
                        let _ = tools.cancelled(&c.id, &c.name, &c.arguments);
                        tools.end("cancelled");
                        return finish(rep, End::Cancelled);
                    }
                    a = &mut call => a,
                }
            };
            match answered {
                Ok(a) => {
                    if matches!(a.receipt.reason.as_deref(), Some("invalid_arguments" | "unknown_tool")) {
                        rep.invalid_args += 1;
                    }
                    messages.push(json!({"role": "tool", "tool_call_id": c.id, "content": a.content.to_string()}));
                }
                Err(e) => {
                    tools.end("failed");
                    return finish(rep, End::Failed { reason: format!("receipt_failed: {}", e.0) });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::fakes::*;
    use crate::host::Task;
    use crate::profile;
    use crate::receipt::Outcome;
    use std::sync::Mutex;

    struct Script {
        turns: Mutex<Vec<ModelTurn>>,
        seen: Mutex<Vec<(Vec<Value>, Value)>>,
        hang_after: Option<usize>,
    }

    impl Script {
        fn new(turns: Vec<ModelTurn>) -> Script {
            Script { turns: Mutex::new(turns), seen: Mutex::new(Vec::new()), hang_after: None }
        }
    }

    impl Model for Script {
        fn turn<'a>(&'a self, messages: &'a [Value], tools: &'a Value) -> BoxFuture<'a, Result<ModelTurn, String>> {
            let n = {
                let mut s = self.seen.lock().unwrap();
                s.push((messages.to_vec(), tools.clone()));
                s.len()
            };
            Box::pin(async move {
                if self.hang_after.is_some_and(|h| n > h) {
                    std::future::pending::<()>().await;
                }
                let mut t = self.turns.lock().unwrap();
                if t.is_empty() { Err("script ended".into()) } else { Ok(t.remove(0)) }
            })
        }
    }

    fn call(id: &str, name: &str, args: Value) -> ModelTurn {
        ModelTurn { content: String::new(), tool_calls: vec![Call { id: id.into(), name: name.into(), arguments: args.to_string() }], finish: "tool_calls".into(), usage: None }
    }

    fn answer(text: &str) -> ModelTurn {
        ModelTurn { content: text.into(), tool_calls: vec![], finish: "stop".into(), usage: Some(json!({"prompt_tokens": 10, "completion_tokens": 2})) }
    }

    fn task_tools(r: &Rig) -> TaskTools {
        TaskTools::new(r.host.clone(), Task { task_id: "b1-t1".into(), text: "Which port does billing staging use?".into(), base: Some(r.dir.path().join("docs")) })
    }

    fn no_cancel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
        watch::channel(false)
    }

    #[tokio::test]
    async fn a_multi_step_read_ends_with_the_answer_and_every_result_goes_back_to_the_model() {
        let r = rig("worker-steps", profile::research());
        r.cards.folder_answers.lock().unwrap().push(approved());
        let t = task_tools(&r);
        let m = Script::new(vec![call("c1", "files_search", json!({"path": ".", "query": "alpha"})), call("c2", "files_read", json!({"path": "a.md"})), answer("ANSWER: alpha")]);
        let (_tx, mut rx) = no_cancel();
        let rep = run(&t, &m, &mut rx).await;
        assert_eq!(rep.end, End::Returned { text: "ANSWER: alpha".into() });
        assert_eq!((rep.turns, rep.tool_calls, rep.invalid_args, rep.max_turn_hit), (3, 2, 0, false));
        let seen = m.seen.lock().unwrap();
        assert_eq!(seen[0].0[0]["role"], "system");
        assert!(seen[0].0[1]["content"].as_str().unwrap().contains(&r.dir.path().join("docs").to_string_lossy().into_owned()), "the task's folder is in the task message");
        let last = &seen[2].0;
        let tool_msgs: Vec<&Value> = last.iter().filter(|m| m["role"] == "tool").collect();
        assert_eq!(tool_msgs.len(), 2);
        assert_eq!(tool_msgs[1]["tool_call_id"], "c2");
        assert!(tool_msgs[1]["content"].as_str().unwrap().contains("alpha line"));
        assert_eq!(seen[0].1.as_array().unwrap().len(), 4, "the profile's four tools are declared");
        assert_eq!(r.receipts.all().len(), 2);
        assert_eq!(rep.usage.len(), 1);
    }

    #[tokio::test]
    async fn calls_past_the_tool_budget_in_one_turn_are_refused_each_with_a_receipt() {
        let mut p = profile::research();
        p.profile.limits.max_tool_calls = 5;
        let r = rig("worker-budget", p);
        let t = task_tools(&r);
        let many: Vec<Call> = (0..8).map(|i| Call { id: format!("c{i}"), name: "memory_recall".into(), arguments: json!({"query": format!("q{i}")}).to_string() }).collect();
        let burst = ModelTurn { content: String::new(), tool_calls: many, finish: "tool_calls".into(), usage: None };
        let m = Script::new(vec![burst, answer("ANSWER: done")]);
        let (_tx, mut rx) = no_cancel();
        let rep = run(&t, &m, &mut rx).await;
        assert_eq!(rep.end, End::Returned { text: "ANSWER: done".into() });
        let all = r.receipts.all();
        assert_eq!(all.len(), 8, "every proposed call has a receipt");
        assert!(all[..5].iter().all(|x| x.outcome == Outcome::Ok));
        assert!(all[5..].iter().all(|x| x.outcome == Outcome::Refused && x.reason.as_deref() == Some("tool_budget")), "{:?}", all.iter().map(|x| x.reason.clone()).collect::<Vec<_>>());
        assert_eq!(m.seen.lock().unwrap()[1].1, json!([]), "the next turn is the last, without tools");
    }

    #[tokio::test]
    async fn the_last_turn_withdraws_the_tools_and_asks_for_the_answer() {
        let mut p = profile::research();
        p.profile.limits.max_turns = 3;
        let r = rig("worker-maxturns", p);
        r.cards.folder_answers.lock().unwrap().push(approved());
        let t = task_tools(&r);
        let m = Script::new(vec![call("c1", "files_read", json!({"path": "a.md"})), call("c2", "files_read", json!({"path": "big.txt"})), answer("ANSWER: unknown")]);
        let (_tx, mut rx) = no_cancel();
        let rep = run(&t, &m, &mut rx).await;
        assert_eq!(rep.end, End::Returned { text: "ANSWER: unknown".into() });
        assert!(rep.max_turn_hit);
        let seen = m.seen.lock().unwrap();
        assert_eq!(seen[2].1, json!([]), "no tools on the last turn");
        assert!(seen[2].0.last().unwrap()["content"].as_str().unwrap().contains("answer now"));
    }

    #[tokio::test]
    async fn invalid_arguments_are_counted_and_answered_and_a_repeated_call_is_steered_then_stopped() {
        let r = rig("worker-loop", profile::research());
        r.cards.folder_answers.lock().unwrap().push(approved());
        let t = task_tools(&r);
        let same = || call("c", "files_read", json!({"path": "a.md"}));
        let m = Script::new(vec![call("bad", "files_read", json!({"nope": 1})), same(), same(), same(), same(), same(), answer("never")]);
        let (_tx, mut rx) = no_cancel();
        let rep = run(&t, &m, &mut rx).await;
        assert_eq!(rep.end, End::Failed { reason: "loop_detected".into() });
        assert_eq!((rep.invalid_args, rep.loop_steers), (1, 2));
        let ran = r.receipts.all().iter().filter(|x| x.outcome == Outcome::Ok).count();
        assert_eq!(ran, 2, "only the first two identical calls ran");
    }

    #[tokio::test]
    async fn cancel_stops_a_model_turn_at_once() {
        let r = rig("worker-cancel-model", profile::research());
        let t = task_tools(&r);
        let mut m = Script::new(vec![]);
        m.hang_after = Some(0);
        let (tx, mut rx) = no_cancel();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.send(true).unwrap();
        });
        let rep = tokio::time::timeout(std::time::Duration::from_secs(2), run(&t, &m, &mut rx)).await.expect("cancel settles");
        assert_eq!(rep.end, End::Cancelled);
    }

    #[tokio::test]
    async fn cancel_during_a_card_wait_retires_the_card_and_receipts_the_call() {
        let r = rig("worker-cancel-card", profile::research());
        r.cards.hang.store(true, std::sync::atomic::Ordering::SeqCst);
        let t = task_tools(&r);
        let m = Script::new(vec![call("c1", "files_read", json!({"path": "a.md"}))]);
        let (tx, mut rx) = no_cancel();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            tx.send(true).unwrap();
        });
        let rep = tokio::time::timeout(std::time::Duration::from_secs(2), run(&t, &m, &mut rx)).await.expect("cancel settles");
        assert_eq!(rep.end, End::Cancelled);
        assert_eq!(r.cards.dropped.load(std::sync::atomic::Ordering::SeqCst), 1, "the card's wait was dropped");
        let rec = r.receipts.all();
        assert_eq!((rec.len(), rec[0].outcome), (1, Outcome::Cancelled));
        let g = r.host.grants.lock().unwrap();
        assert!(g.list().is_empty(), "nothing granted");
        let ev = r.events.0.lock().unwrap();
        assert!(matches!(ev.last(), Some(crate::grants::Line::Denied { reason, .. }) if reason == "cancelled"), "the open request is closed: {ev:?}");
    }

    #[tokio::test]
    async fn a_cut_or_empty_answer_fails_the_task() {
        for (turn, why) in [
            (ModelTurn { content: "half".into(), tool_calls: vec![], finish: "length".into(), usage: None }, "invalid_output:length"),
            (answer("   "), "invalid_output:empty"),
            (answer(&"x".repeat(MAX_ANSWER_CHARS + 1)), "invalid_output:too_long"),
        ] {
            let r = rig("worker-invalid", profile::research());
            let t = task_tools(&r);
            let (_tx, mut rx) = no_cancel();
            assert_eq!(run(&t, &Script::new(vec![turn]), &mut rx).await.end, End::Failed { reason: why.into() });
        }
        let r = rig("worker-error", profile::research());
        let t = task_tools(&r);
        let (_tx, mut rx) = no_cancel();
        assert!(matches!(run(&t, &Script::new(vec![]), &mut rx).await.end, End::Failed { reason } if reason.starts_with("model:")));
    }

    #[tokio::test]
    async fn old_tool_results_past_the_context_budget_become_a_note_on_how_to_read_them_again() {
        let mut p = profile::research();
        p.profile.limits.max_context_chars = 20_000;
        let r = rig("worker-compact", p);
        r.cards.folder_answers.lock().unwrap().push(approved());
        let t = task_tools(&r);
        let m = Script::new(vec![call("c1", "files_read", json!({"path": "big.txt"})), call("c2", "files_read", json!({"path": "big.txt", "offset": 8000})), call("c3", "files_read", json!({"path": "big.txt", "offset": 16000})), answer("done")]);
        let (_tx, mut rx) = no_cancel();
        assert_eq!(run(&t, &m, &mut rx).await.end, End::Returned { text: "done".into() });
        let seen = m.seen.lock().unwrap();
        let last = &seen[3].0;
        let total: usize = last.iter().map(|m| m["content"].as_str().map(|s| s.chars().count()).unwrap_or(0)).sum();
        assert!(total <= 20_000 + 2_000, "{total}");
        let first_tool = last.iter().find(|m| m["role"] == "tool").unwrap();
        assert!(first_tool["content"].as_str().unwrap().contains("elided"), "{}", first_tool["content"]);
        assert!(last.iter().rfind(|m| m["role"] == "tool").unwrap()["content"].as_str().unwrap().contains("0123456789"), "the newest result is whole");
    }
}
