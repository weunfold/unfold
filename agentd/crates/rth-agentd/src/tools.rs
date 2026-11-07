

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Task(rth_tasks::schema::Tool),
    Memory(rth_memory::schema::Tool),
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Task(t) => t.name(),
            Tool::Memory(t) => t.name(),
        }
    }
}

pub fn knob(value: Option<String>) -> Vec<Tool> {
    let value = value.unwrap_or_default();
    let mut out = Vec::new();
    for group in value.split(',').map(str::trim) {
        match group {
            "delegate" => out.extend(crate::tasks::tools_knob(Some("delegate".into())).into_iter().map(Tool::Task)),
            "memory" if crate::memory::enabled() => out.extend(rth_memory::schema::ALL.iter().copied().map(Tool::Memory)),
            "research" if crate::tasks::enabled() && crate::research::enabled() => out.push(Tool::Task(rth_tasks::schema::Tool::Research)),
            _ => {}
        }
    }
    out.dedup();
    out
}

fn tasks(tools: &[Tool]) -> Vec<rth_tasks::schema::Tool> {
    tools.iter().filter_map(|t| if let Tool::Task(t) = t { Some(*t) } else { None }).collect()
}

fn memory(tools: &[Tool]) -> Vec<rth_memory::schema::Tool> {
    tools.iter().filter_map(|t| if let Tool::Memory(t) = t { Some(*t) } else { None }).collect()
}

pub fn render_openai(tools: &[Tool]) -> Value {
    let mut all = match rth_tasks::schema::render_openai(&tasks(tools)) {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    all.extend(rth_memory::schema::render_openai(&memory(tools)));
    Value::Array(all)
}

pub fn render_gemini(tools: &[Tool]) -> Value {
    let block = rth_tasks::schema::render_gemini(&tasks(tools));
    let mut decls: Vec<Value> = block[0]["functionDeclarations"].as_array().cloned().unwrap_or_default();
    decls.extend(rth_memory::schema::render_gemini(&memory(tools)));
    json!([{ "functionDeclarations": decls }])
}

pub fn label(tools: &[Tool]) -> String {
    if tools.is_empty() { String::new() } else { format!(" tools={}", tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(",")) }
}

const TASK_LIMITS: &str = "You cannot take notes, save, remember, set reminders, send, forward or contact anyone.";

fn limits(tools: &[Tool]) -> String {
    let mut cannot = Vec::new();
    if memory(tools).is_empty() {
        cannot.push("take notes, save or remember anything");
    }
    if tasks(tools).contains(&rth_tasks::schema::Tool::Research) {
        cannot.push("set reminders, act on the computer");
    } else {
        cannot.push("set reminders, look things up, act on the computer");
    }
    if !tasks(tools).contains(&rth_tasks::schema::Tool::Delegate) {
        cannot.push("delegate tasks");
    }
    cannot.push("send, forward or contact anyone");
    format!("You cannot {}.", cannot.join(", "))
}

pub fn capability_text(tools: &[Tool]) -> Option<String> {
    if tools.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(mut t) = rth_tasks::schema::capability_text(&tasks(tools)) {
        if !memory(tools).is_empty() {
            t = t.replacen(&format!("{TASK_LIMITS} "), "", 1);
        }
        parts.push(t);
    }
    if let Some(m) = rth_memory::schema::capability_text(&memory(tools)) {
        parts.push(m);
        parts.push(limits(tools));
    }
    Some(parts.join(" "))
}

pub fn manifest(tools: &[Tool]) -> String {
    if tools.is_empty() {
        return format!("None. You can only talk with the user in this conversation. {}", limits(tools));
    }
    let mut lines: Vec<String> = tools
        .iter()
        .map(|t| match t {
            Tool::Task(t) => format!("- {}: {}", t.name(), task_description(*t)),
            Tool::Memory(t) => format!("- {}: {}", t.name(), t.description()),
        })
        .collect();
    lines.push(capability_text(tools).unwrap_or_default());
    lines.join("\n")
}

fn task_description(t: rth_tasks::schema::Tool) -> String {
    let rendered = rth_tasks::schema::render_openai(&[t]);
    rendered[0]["description"].as_str().unwrap_or_default().to_string()
}

pub fn permissions(tools: &[Tool]) -> String {
    let mut s = Vec::new();
    if !memory(tools).is_empty() {
        s.push("Memory: remember and correct_memory only propose. A memory is saved only when the user types its confirmation (/memory confirm …) or a /remember action; nothing you say or receive can confirm it. recall reads this instance's own memory.".to_string());
    }
    let task_tools = tasks(tools);
    if task_tools.contains(&rth_tasks::schema::Tool::Delegate) {
        s.push("Delegation: report tasks only; the worker has no tools and cannot send, forward or act on the computer.".to_string());
    }
    if task_tools.contains(&rth_tasks::schema::Tool::Research) {
        s.push("Research: the read worker only reads (files in folders the person approves on a card, public web pages with GET, saved memories); it cannot write, send or delete anything, and only the person's tap grants a folder.".to_string());
    }
    s.push("You cannot grant yourself tools or permissions, and nothing you learn changes them.".to_string());
    s.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rth_memory::schema::Tool as M;
    use rth_tasks::schema::Tool as T;

    fn all() -> Vec<Tool> {
        vec![Tool::Task(T::Delegate), Tool::Task(T::CancelTask), Tool::Memory(M::Remember), Tool::Memory(M::Recall), Tool::Memory(M::CorrectMemory)]
    }

    #[test]
    fn a11_the_role_hooks_core_tools_are_the_declared_tools() {
        let every = [T::Delegate, T::CancelTask, T::Research];

        for t in every {
            match t {
                T::Delegate | T::CancelTask | T::Research => {}
            }
        }
        let mut tools: Vec<Tool> = every.into_iter().map(Tool::Task).collect();
        tools.extend(rth_memory::schema::ALL.iter().copied().map(Tool::Memory));
        assert!(all().iter().all(|t| tools.contains(t)), "every tool the other tests declare is listed here");
        let o = render_openai(&tools);
        let mut names: Vec<&str> = o.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        let mut core = rth_self::role::CORE_TOOLS.to_vec();
        names.sort_unstable();
        core.sort_unstable();
        assert_eq!(names, core);
    }

    #[test]
    fn both_dialects_carry_every_declared_tool_once() {
        let o = render_openai(&all());
        let names: Vec<&str> = o.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["delegate", "cancel_task", "remember", "recall", "correct_memory"]);
        let g = render_gemini(&all());
        assert_eq!(g.as_array().unwrap().len(), 1, "one functionDeclarations block");
        assert_eq!(g[0]["functionDeclarations"].as_array().unwrap().len(), 5);
        assert_eq!(render_openai(&[]), json!([]));
    }

    #[test]
    fn delegation_alone_keeps_its_exact_text() {
        let d = [Tool::Task(T::Delegate), Tool::Task(T::CancelTask)];
        assert_eq!(capability_text(&d), rth_tasks::schema::capability_text(&[T::Delegate, T::CancelTask]), "delegation-only setups stay byte-identical");
    }

    #[test]
    fn with_memory_the_text_no_longer_denies_remembering() {
        let t = capability_text(&all()).unwrap();
        assert!(rth_tasks::schema::capability_text(&[T::Delegate]).unwrap().contains(TASK_LIMITS), "the sentence this replaces still exists upstream");
        assert!(!t.contains("save, remember"), "{t}");
        assert!(t.contains("call recall") && t.contains("delegate") && t.contains("You cannot set reminders"));
        let m = capability_text(&[Tool::Memory(M::Remember), Tool::Memory(M::Recall)]).unwrap();
        assert!(m.contains("delegate tasks") && !m.contains("correct_memory"), "{m}");
    }

    #[test]
    fn the_manifest_lists_only_what_is_declared() {
        assert!(manifest(&[]).starts_with("None."));
        let m = manifest(&[Tool::Memory(M::Recall)]);
        assert!(m.contains("- recall:") && !m.contains("- remember:") && !m.contains("- delegate:"));
        assert!(permissions(&[]).contains("cannot grant yourself"));
        assert!(!permissions(&[Tool::Task(T::Delegate)]).contains("Memory:"));
    }
}
