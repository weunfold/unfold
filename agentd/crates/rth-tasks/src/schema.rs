

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Delegate,
    CancelTask,

    Research,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Delegate => "delegate",
            Tool::CancelTask => "cancel_task",
            Tool::Research => "research",
        }
    }

    pub fn from_name(name: &str) -> Option<Tool> {
        match name {
            "delegate" => Some(Tool::Delegate),
            "cancel_task" => Some(Tool::CancelTask),
            "research" => Some(Tool::Research),
            _ => None,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Tool::Delegate => {
                "Asks RTH's worker to write a report of this conversation. The report appears in the chat as text when it is ready. It does not send, save, note, remember or forward anything, and it cannot contact anyone."
            }
            Tool::CancelTask => {
                "Cancels a report task. Give its task_id, or omit it to cancel the single active task. A report whose posting is already committed cannot be cancelled."
            }
            Tool::Research => {
                "Asks RTH's read worker to answer a question by reading: files in folders the person approves (the person is asked the first time a folder is needed), public web pages fetched with GET, and the person's saved memories. The answer appears in the chat as text when it is ready. It changes nothing: it does not write, send, save, delete or contact anyone."
            }
        }
    }

    fn parameters(self) -> Value {
        match self {
            Tool::Delegate => json!({
                "type": "object",
                "properties": {
                    "purpose": { "type": "string", "enum": PURPOSES, "description": "What kind of task. Only report exists." },
                    "task": { "type": "string", "description": "What to write, in the user's words (1-500 characters)." },
                    "audience": { "type": "string", "enum": AUDIENCES, "description": "Who the report is for. Default: user." }
                },
                "required": ["purpose", "task"]
            }),
            Tool::CancelTask => json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "The task to cancel; omit for the single active task." }
                }
            }),
            Tool::Research => json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The question, in the user's words (1-500 characters)." },
                    "folder": { "type": "string", "description": "Optional: the folder to look in, an absolute path or ~/..." }
                },
                "required": ["task"]
            }),
        }
    }
}

pub const PURPOSES: &[&str] = &["report"];
pub const AUDIENCES: &[&str] = &["user", "coding_agent"];
pub const MAX_TASK_CHARS: usize = 500;

pub fn render_openai(tools: &[Tool]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| json!({ "type": "function", "name": t.name(), "description": t.description(), "parameters": t.parameters() }))
            .collect(),
    )
}

pub fn render_gemini(tools: &[Tool]) -> Value {
    let decls: Vec<Value> =
        tools.iter().map(|t| json!({ "name": t.name(), "description": t.description(), "parameters": t.parameters() })).collect();
    json!([{ "functionDeclarations": decls }])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegateArgs {
    pub purpose: String,
    pub task: String,
    pub audience: String,
}

pub fn validate_delegate(args: &Value) -> Result<DelegateArgs, String> {
    let obj = args.as_object().ok_or("arguments are not an object")?;
    for k in obj.keys() {
        if !matches!(k.as_str(), "purpose" | "task" | "audience") {
            return Err(format!("unknown argument {k:?}"));
        }
    }
    let purpose = obj.get("purpose").and_then(Value::as_str).ok_or("purpose missing")?;
    if !PURPOSES.contains(&purpose) {
        return Err(format!("purpose {purpose:?} is not wired"));
    }
    let task = obj.get("task").and_then(Value::as_str).ok_or("task missing")?.trim();
    let n = task.chars().count();
    if n == 0 || n > MAX_TASK_CHARS {
        return Err(format!("task must be 1-{MAX_TASK_CHARS} characters, got {n}"));
    }
    let audience = match obj.get("audience") {
        None | Some(Value::Null) => "user",
        Some(v) => v.as_str().filter(|a| AUDIENCES.contains(a)).ok_or("audience is not user or coding_agent")?,
    };
    Ok(DelegateArgs { purpose: purpose.to_string(), task: task.to_string(), audience: audience.to_string() })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchArgs {
    pub task: String,

    pub folder: Option<String>,
}

pub fn validate_research(args: &Value) -> Result<ResearchArgs, String> {
    let obj = args.as_object().ok_or("arguments are not an object")?;
    if let Some(k) = obj.keys().find(|k| !matches!(k.as_str(), "task" | "folder")) {
        return Err(format!("unknown argument {k:?}"));
    }
    let task = obj.get("task").and_then(Value::as_str).ok_or("task missing")?.trim();
    let n = task.chars().count();
    if n == 0 || n > MAX_TASK_CHARS {
        return Err(format!("task must be 1-{MAX_TASK_CHARS} characters, got {n}"));
    }
    let folder = match obj.get("folder") {
        None | Some(Value::Null) => None,
        Some(Value::String(f)) if f.starts_with('/') || f == "~" || f.starts_with("~/") => Some(f.trim().to_string()),
        Some(_) => return Err("folder must be an absolute path or start with ~/".into()),
    };
    Ok(ResearchArgs { task: task.to_string(), folder })
}

pub fn validate_cancel(args: &Value) -> Result<Option<String>, String> {
    let obj = args.as_object().ok_or("arguments are not an object")?;
    for k in obj.keys() {
        if k != "task_id" {
            return Err(format!("unknown argument {k:?}"));
        }
    }
    match obj.get("task_id") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(|s| Some(s.to_string())).ok_or_else(|| "task_id is not a string".to_string()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NoExecutor,
    Busy,
    Budget,
    BadArguments,
    Conflict,
    Disabled,
    Ambiguous,
    NotActive,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::NoExecutor => "no_executor",
            Refusal::Busy => "busy",
            Refusal::Budget => "budget",
            Refusal::BadArguments => "bad_arguments",
            Refusal::Conflict => "conflict",
            Refusal::Disabled => "disabled",
            Refusal::Ambiguous => "ambiguous",
            Refusal::NotActive => "not_active",
        }
    }
}

pub fn output_accepted(task_id: &str) -> Value {
    json!({
        "status": "accepted",
        "task_id": task_id,
        "say": "One short line: the request is accepted and the report will appear in the chat when it is ready. Do not say you are writing it or that it has started. Do not describe its content."
    })
}

pub fn output_accepted_research(task_id: &str) -> Value {
    json!({
        "status": "accepted",
        "task_id": task_id,
        "say": "One short line: the request is accepted and the answer will appear in the chat when it is ready. Do not say you are looking it up or that it has started. Do not guess the answer."
    })
}

pub fn output_unavailable(reason: Refusal) -> Value {
    json!({
        "status": "unavailable",
        "reason": reason.as_str(),
        "say": "One sentence: you cannot do that right now. Do not promise to do it later."
    })
}

pub fn output_cancel_requested(task_id: &str) -> Value {
    json!({ "status": "cancel_requested", "task_id": task_id, "say": "One short line: you asked for the task to be cancelled." })
}

pub fn output_publication_committed(task_id: &str) -> Value {
    json!({
        "status": "publication_committed",
        "task_id": task_id,
        "say": "One short line: posting is already committed and delivery is pending. Do not say it was cancelled or that it is in the chat."
    })
}

pub fn capability_text(tools: &[Tool]) -> Option<String> {
    let has = |t| tools.contains(&t);
    let research = has(Tool::Research).then_some(
        "You can ask a read worker to look something up in the person's approved folders, public web pages or saved memories with research. When research answers \"accepted\", say only that the request is accepted and the answer will appear in the chat when it is ready; never guess the answer.",
    );
    if !has(Tool::Delegate) {
        return research.map(str::to_string);
    }
    let mut s = String::from("You can ask a worker to write a report of this conversation with delegate");
    if has(Tool::CancelTask) {
        s.push_str(", and cancel it with cancel_task");
    }
    s.push_str(
        ". When delegate answers \"accepted\", say only that the request is accepted and the report will appear in the chat when it is ready. Never say you are writing it or that it has started, and never describe a report before it is in the chat. You cannot take notes, save, remember, set reminders, send, forward or contact anyone. If a tool answers \"unavailable\", say in one sentence that you can't do it right now.",
    );
    if let Some(r) = research {
        s.push(' ');
        s.push_str(r);
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_and_gemini_renderings_are_pinned() {
        let o = render_openai(&[Tool::Delegate]);
        assert_eq!(o[0]["type"], "function");
        assert_eq!(o[0]["name"], "delegate");
        assert_eq!(o[0]["parameters"]["properties"]["purpose"]["enum"], json!(["report"]));
        assert_eq!(o[0]["parameters"]["required"], json!(["purpose", "task"]));
        let g = render_gemini(&[Tool::Delegate, Tool::CancelTask]);
        assert_eq!(g[0]["functionDeclarations"][0]["name"], "delegate");
        assert_eq!(g[0]["functionDeclarations"][1]["name"], "cancel_task");
        assert!(g[0]["functionDeclarations"][0].get("behavior").is_none(), "a plain call, answered at once");

        assert_eq!(o[0]["parameters"], g[0]["functionDeclarations"][0]["parameters"]);
        let golden = sha(&serde_json::to_string(&render_openai(&[Tool::Delegate, Tool::CancelTask])).unwrap());
        assert_eq!(golden, OPENAI_GOLDEN, "the OpenAI tool rendering changed: review it, then update the golden");
    }

    const OPENAI_GOLDEN: &str = "fb860b2522d86522";

    fn sha(s: &str) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(s.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn schema_lists_only_wired_purposes() {
        assert_eq!(PURPOSES, &["report"]);
        assert!(validate_delegate(&json!({"purpose":"research","task":"x"})).unwrap_err().contains("not wired"));
    }

    #[test]
    fn delegate_arguments_are_validated_strictly() {
        let ok = validate_delegate(&json!({"purpose":"report","task":"  relatório pro meu agente  ","audience":"coding_agent"})).unwrap();
        assert_eq!((ok.task.as_str(), ok.audience.as_str()), ("relatório pro meu agente", "coding_agent"));
        assert_eq!(validate_delegate(&json!({"purpose":"report","task":"x"})).unwrap().audience, "user");
        for bad in [
            json!([]),
            json!({"purpose":"report"}),
            json!({"purpose":"report","task":""}),
            json!({"purpose":"report","task":"a".repeat(501)}),
            json!({"purpose":"report","task":"x","audience":"boss"}),
            json!({"purpose":"report","task":"x","send_to":"team"}),
        ] {
            assert!(validate_delegate(&bad).is_err(), "{bad}");
        }

        assert!(validate_delegate(&json!({"purpose":"report","task":"é".repeat(500)})).is_ok());
        assert_eq!(validate_cancel(&json!({})).unwrap(), None);
        assert_eq!(validate_cancel(&json!({"task_id":"b-t1"})).unwrap().as_deref(), Some("b-t1"));
        assert!(validate_cancel(&json!({"other":1})).is_err());
    }

    #[test]
    fn outputs_never_claim_work_has_started() {
        let a = output_accepted("b-t1").to_string();
        assert!(a.contains("\"accepted\"") && a.contains("Do not say you are writing it"));
        assert_eq!(output_unavailable(Refusal::NoExecutor)["reason"], "no_executor");
        assert!(output_publication_committed("b-t1")["say"].as_str().unwrap().contains("delivery is pending"));
    }

    #[test]
    fn capability_text_follows_the_declared_tools() {
        assert_eq!(capability_text(&[]), None, "no tool: persona v1 unchanged");
        let one = capability_text(&[Tool::Delegate]).unwrap();
        assert!(!one.contains("cancel_task"));
        let both = capability_text(&[Tool::Delegate, Tool::CancelTask]).unwrap();
        assert!(both.contains("delegate") && both.contains("cancel_task"));
        assert!(!both.contains("no other tool"), "never hard-code a tool count (rereview non-blocking)");
        assert!(both.contains("cannot take notes"));
    }
}
