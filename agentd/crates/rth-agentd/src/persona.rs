

pub const PERSONA_V1_NAME: &str = "rth-persona-v1";
pub const PERSONA_V2_NAME: &str = "rth-persona-v2";
pub const PERSONA_V2_1_NAME: &str = "rth-persona-v2.1";

pub const DEFAULT_PERSONA: &str = PERSONA_V2_1_NAME;

pub const PERSONA_V1: &str = "You are RTH, a real-time voice assistant.
Reply in the language the user speaks to you, and mirror their register: casual when they are casual, formal when they are formal, technical when they are technical.
Keep replies short: one or two spoken sentences, unless the user asks for more.
Be honest about what you can do. Right now you can only talk with the user in this conversation. You have no tools: you cannot take notes, save anything, set reminders, look things up, act on the computer, delegate tasks, or forward anything to a team or to another person. Never claim or imply that you did, or will do, any of these. When the user asks for something you cannot do yet, say so plainly in one sentence.
Do not say that you are a Google or OpenAI model, or name the company behind you, unless the user asks which model you run on.";

pub const PERSONA_V2: &str = "You are RTH, a real-time voice assistant.
Reply in the language the user speaks to you, and mirror their register: casual when they are casual, formal when they are formal, technical when they are technical.
Keep every spoken reply to one or two short sentences, under about eight seconds of speech (about 20 words), unless the user asks for more detail; when there is more worth saying, offer to say more instead of saying it.
Be honest about what you can do. Right now you can only talk with the user in this conversation. You have no tools: you cannot take notes, save anything, set reminders, look things up, act on the computer, delegate tasks, or forward anything to a team or to another person. Never claim or imply that you did, or will do, any of these.
Mention a limit only when the user's request actually runs into one: then say so plainly in one short sentence and, where you can, offer what you can do instead. When asked who you are or what you can do, say briefly what you can do; do not list what you cannot.
Do not say that you are a Google or OpenAI model, or name the company behind you, unless the user asks which model you run on; then answer only with what you actually know, and do not guess.";

pub const PERSONA_V2_1: &str = "You are RTH, the agent of this RTH instance. When asked who or what you are, say first that you are RTH.
Reply in the language the user speaks to you, and mirror their register: casual when they are casual, formal when they are formal, technical when they are technical.
Keep every spoken reply to one or two short sentences, under about eight seconds of speech (about 20 words), unless the user asks for more detail; when there is more worth saying, offer to say more instead of saying it.
Your inputs are speech and typed text only: you cannot see images, files or the screen.
{{live_status}}
Use only the declared tools, subject to their permissions and results. Mention a limit only when the user's request actually runs into one: then say so plainly in one short sentence and, where you can, offer what you can do instead.
Describe only the runtime facts and tool results supplied here. Do not invent causes for timing or delays, narrate hidden processing, or claim knowledge of internals you have not been given.
Do not say that you are a Google or OpenAI model, or name the company behind you, unless the user asks which model you run on; then answer only with what you actually know, and do not guess.";

pub fn parse(knob: Option<&str>) -> Result<&'static str, String> {
    match knob {
        None => Ok(DEFAULT_PERSONA),
        Some(PERSONA_V2_1_NAME) => Ok(PERSONA_V2_1_NAME),
        Some(PERSONA_V2_NAME) => Ok(PERSONA_V2_NAME),
        Some(PERSONA_V1_NAME) => Ok(PERSONA_V1_NAME),
        Some(ORIGIN_V0_1_NAME) => Ok(ORIGIN_V0_1_NAME),
        Some("none") => Ok("none"),
        Some(other) => Err(format!("persona {other:?} (expected {PERSONA_V2_1_NAME}, {PERSONA_V2_NAME}, {PERSONA_V1_NAME}, {ORIGIN_V0_1_NAME} or none)")),
    }
}

pub fn text(name: &str) -> Option<&'static str> {
    match name {
        PERSONA_V2_1_NAME => Some(PERSONA_V2_1),
        PERSONA_V2_NAME => Some(PERSONA_V2),
        PERSONA_V1_NAME => Some(PERSONA_V1),
        ORIGIN_V0_1_NAME => Some(ORIGIN_TEMPLATE),
        _ => None,
    }
}

pub fn persona<'a>(custom: Option<&'a str>, name: &str) -> Option<&'a str> {
    custom.or(text(name))
}

pub fn persona_name(custom: Option<&str>, name: &'static str) -> &'static str {
    if custom.is_some() { "custom" } else { name }
}

pub const NO_TOOLS: &str = "Right now you can only talk with the user in this conversation. You have no tools: you cannot take notes, save anything, set reminders, look things up, act on the computer, delegate tasks, or forward anything to a team or to another person.";

pub fn with_tools(text: String, tools: &[crate::tools::Tool]) -> String {
    if text.contains("{{capability_manifest}}") {
        return render_origin(&text, tools);
    }

    if text.contains("{{live_status}}") {
        let snapshot = READINESS.read().unwrap().clone();
        return render_v2_1(&text, tools, snapshot.as_ref());
    }
    match crate::tools::capability_text(tools) {
        None => text,
        Some(cap) if text.contains(NO_TOOLS) => text.replacen(NO_TOOLS, &cap, 1),
        Some(cap) => format!("{text}\n\n{cap}"),
    }
}

pub const ORIGIN_V0_1_NAME: &str = "rth-origin-v0.1";
pub const ORIGIN_TEMPLATE: &str = include_str!("origin/system-prompt-v0.1.txt");
pub const ORIGIN_MANIFESTO: &str = include_str!("origin/manifesto-v0.1.txt");

pub const SPEAKING: &str = "Reply in the language the user speaks to you, and mirror their register: casual when they are casual, formal when they are formal, technical when they are technical.
Keep every spoken reply to one or two short sentences, under about eight seconds of speech (about 20 words), unless the user asks for more detail; when there is more worth saying, offer to say more instead of saying it.";

static INSTANCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn set_instance(id: &str) {
    let _ = INSTANCE.set(id.to_string());
}

fn manifesto_body() -> String {
    ORIGIN_MANIFESTO.lines().filter(|l| !l.starts_with("# ") && !l.starts_with("Draft saved")).collect::<Vec<_>>().join("\n").trim().to_string()
}

#[cfg(test)]
pub const ORIGIN_RENDER_VERSION: &str = "origin-render@2";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Readiness {

    pub memory: Option<String>,

    pub delegation: Option<String>,
}

static READINESS: std::sync::RwLock<Option<Readiness>> = std::sync::RwLock::new(None);

pub fn set_readiness(r: Readiness) {
    *READINESS.write().unwrap() = Some(r);
}

#[cfg(test)]
pub static READINESS_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
pub fn reset_readiness_for_test() {
    *READINESS.write().unwrap() = None;
}

pub fn fill_once(template: &str, values: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("}}") {
            Some(j) => {
                let name = &after[..j];
                match values.iter().find(|(k, _)| *k == name) {
                    Some((_, v)) => out.push_str(v),
                    None => {
                        out.push_str("{{");
                        out.push_str(name);
                        out.push_str("}}");
                    }
                }
                rest = &after[j + 2..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn render_origin(template: &str, tools: &[crate::tools::Tool]) -> String {
    let snapshot = READINESS.read().unwrap().clone();
    render_origin_with(template, tools, INSTANCE.get().map(String::as_str), snapshot.as_ref())
}

pub fn render_origin_with(template: &str, tools: &[crate::tools::Tool], instance: Option<&str>, readiness: Option<&Readiness>) -> String {
    let memory = tools.iter().any(|t| matches!(t, crate::tools::Tool::Memory(_)));
    let persistent = if memory {
        "Saved memories are not preloaded here; relevant ones may arrive with a typed message, marked as RTH memory recall."
    } else {
        "None beyond the conversation history given to you."
    };
    let r = readiness.cloned().unwrap_or_default();
    let observed = |v: &Option<String>| v.clone().unwrap_or_else(|| "not observed at setup".into());

    let capabilities = format!(
        "\nDeclared operations:\n{}\nLive status at setup (observed, may change; a failing call says so):\n- memory: {}\n- delegation: {}\nInputs: typed text and speech. No image, file or screen input.",
        crate::tools::manifest(tools),
        if memory { observed(&r.memory) } else { "not declared".into() },
        if tools.iter().any(|t| matches!(t, crate::tools::Tool::Task(_))) { observed(&r.delegation) } else { "not declared".into() },
    );
    let values = [
        ("origin_manifesto", manifesto_body()),
        ("instance_id", instance.unwrap_or("unknown").to_string()),
        ("capability_manifest", capabilities),
        ("permission_state", format!("\n{}", crate::tools::permissions(tools))),
        ("persistent_context", persistent.to_string()),
        ("recent_events", "given as the conversation history when a restart restored it; it may be incomplete.".to_string()),
        ("self_understanding", "(empty)".to_string()),
    ];
    let body = fill_once(template, &values);
    format!("{}\n\nHOW TO SPEAK\n{SPEAKING}", body.trim_end())
}

pub fn live_status_v2_1(tools: &[crate::tools::Tool], readiness: Option<&Readiness>) -> String {
    let r = readiness.cloned().unwrap_or_default();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    let declared = if names.is_empty() { "none".to_string() } else { names.join(", ") };
    let state = |declared: bool, observed: Option<String>| if declared { observed.unwrap_or_else(|| "not observed".into()) } else { "not declared".into() };
    let memory = state(tools.iter().any(|t| matches!(t, crate::tools::Tool::Memory(_))), r.memory);
    let delegation = state(tools.iter().any(|t| matches!(t, crate::tools::Tool::Task(_))), r.delegation);
    format!("Declared operations: {declared}\nReadiness observed at setup (may change):\n- memory: {memory}\n- delegation: {delegation}\nOnly tool results establish that an action was accepted, completed or saved.")
}

pub fn render_v2_1(template: &str, tools: &[crate::tools::Tool], readiness: Option<&Readiness>) -> String {
    let text = fill_once(template, &[("live_status", live_status_v2_1(tools, readiness))]);
    let Some(cap) = crate::tools::capability_text(tools) else { return text };
    let memory: Vec<&str> = tools.iter().filter_map(|t| if let crate::tools::Tool::Memory(m) = t { Some(m.name()) } else { None }).collect();
    let cap = if memory.is_empty() {
        cap
    } else {
        let names = memory.join(", ");
        cap.replacen(
            &format!("You have a durable memory for this conversation, through {names}."),
            &format!("The memory tools ({names}) can propose, retrieve and correct saved memories when the store is available."),
            1,
        )
    };
    format!("{text}\n\n{cap}")
}

#[cfg(test)]
pub fn origin_digest(rendered: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(ORIGIN_RENDER_VERSION.as_bytes());
    h.update([0]);
    h.update(ORIGIN_TEMPLATE.as_bytes());
    h.update([0]);
    h.update(rendered.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn system_instruction(policy: Option<&str>, custom: Option<&str>, name: &str) -> Option<String> {
    match (policy, persona(custom, name)) {
        (Some(p), Some(slot)) => Some(format!("{p}\n\n{slot}")),
        (Some(p), None) => Some(p.to_string()),
        (None, Some(slot)) => Some(slot.to_string()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_persona_is_the_default_and_a_custom_text_replaces_it_after_the_policy() {
        assert_eq!(parse(None).unwrap(), PERSONA_V2_1_NAME, "v2.1 is the default (conversation)");
        let v2 = parse(Some(PERSONA_V2_NAME)).unwrap();
        assert_eq!(system_instruction(None, None, v2).as_deref(), Some(PERSONA_V2));
        assert_eq!(system_instruction(Some("[p]"), None, v2), Some(format!("[p]\n\n{PERSONA_V2}")));
        assert_eq!(system_instruction(Some("[p]"), Some("Your name is RTH."), v2).as_deref(), Some("[p]\n\nYour name is RTH."));

        assert_eq!(system_instruction(None, None, parse(Some("rth-persona-v1")).unwrap()).as_deref(), Some(PERSONA_V1));

        let none = parse(Some("none")).unwrap();
        assert_eq!(system_instruction(Some("[p]"), None, none).as_deref(), Some("[p]"));
        assert_eq!(system_instruction(None, None, none), None);
        assert_eq!((persona_name(None, v2), persona_name(Some("x"), v2), persona_name(None, none)), ("rth-persona-v2", "custom", "none"));
        assert!(parse(Some("rth-persona-v3")).is_err());

        assert!(!PERSONA_V1.contains("[rth-voice-policy") && !PERSONA_V2.contains("[rth-voice-policy"));
    }

    #[test]
    fn v2_keeps_every_v1_honesty_rule() {
        for rule in [
            "You are RTH, a real-time voice assistant.",
            "Reply in the language the user speaks to you, and mirror their register",
            "Right now you can only talk with the user in this conversation.",
            "You have no tools: you cannot take notes, save anything, set reminders, look things up, act on the computer, delegate tasks, or forward anything to a team or to another person.",
            "Never claim or imply that you did, or will do, any of these.",
            "Do not say that you are a Google or OpenAI model, or name the company behind you, unless the user asks which model you run on",
        ] {
            assert!(PERSONA_V1.contains(rule) && PERSONA_V2.contains(rule), "{rule}");
        }
        assert!(PERSONA_V2.contains("Mention a limit only when the user's request actually runs into one"));
        assert!(PERSONA_V2.contains("one or two short sentences, under about eight seconds of speech (about 20 words)"));
        assert!(PERSONA_V2.contains("offer to say more instead of saying it"));
    }

    #[test]
    fn declared_tools_replace_the_no_tools_sentence_and_none_change_nothing() {
        use crate::tools::Tool as D;
        use rth_tasks::schema::Tool;
        for p in [PERSONA_V1, PERSONA_V2] {
            assert!(p.contains(NO_TOOLS), "v1 and v2 carry the sentence the hook replaces");
            assert_eq!(with_tools(p.to_string(), &[]), p, "off: byte-identical");
            let on = with_tools(p.to_string(), &[D::Task(Tool::Delegate), D::Task(Tool::CancelTask)]);
            assert!(!on.contains("You have no tools") && on.contains("delegate") && on.contains("cancel_task"));
        }
        let custom = with_tools("Your name is RTH.".into(), &[D::Task(Tool::Delegate)]);
        assert!(custom.starts_with("Your name is RTH.\n\n") && custom.contains("delegate"));
    }

    #[test]
    fn the_origin_prompt_renders_only_wired_capabilities() {
        use crate::tools::Tool as D;
        use rth_memory::schema::Tool as M;
        use sha2::Digest;
        let hex = |s: &str| sha2::Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(ORIGIN_TEMPLATE), "bdc7ecdc8314f5cf7e6ead06387cea9d0f0cd017e4c86b315943e47fd0bae569", "the origin prompt");

        assert_eq!(hex(ORIGIN_MANIFESTO), "b70f0abc67991dd7dfface5ea1708be94947d47bdc01692b473efb6a273e3618", "the origin manifesto");
        assert!(!ORIGIN_MANIFESTO.contains("private operator") && !ORIGIN_MANIFESTO.contains("Draft saved"));
        assert!(PERSONA_V2.contains(SPEAKING), "the speaking rules are v2's, word for word");
        assert_eq!(parse(Some("rth-origin-v0.1")), Ok(ORIGIN_V0_1_NAME));
        let none = with_tools(ORIGIN_TEMPLATE.to_string(), &[]);
        assert!(!none.contains("{{"), "every placeholder is filled");
        assert!(none.contains("I begin without a personal history.") && !none.contains("Draft saved"));
        assert!(none.contains("Available capabilities: \nDeclared operations:\nNone.") && none.contains("Current self-understanding: (empty)"));
        assert!(!none.contains("You are RTH, a real-time voice assistant"), "no predefined identity line");
        let mem = with_tools(ORIGIN_TEMPLATE.to_string(), &[D::Memory(M::Remember), D::Memory(M::Recall), D::Memory(M::CorrectMemory)]);
        assert!(mem.contains("- remember:") && mem.contains("- recall:") && !mem.contains("- delegate:"));
        assert!(mem.contains("marked as RTH memory recall") && mem.contains("Memory: remember and correct_memory only propose"));
        assert!(mem.ends_with(SPEAKING));
    }

    #[test]
    fn origin_substitution_is_single_pass() {
        let t = "A {{instance_id}} B {{self_understanding}} C {{unknown}} D {{unterminated";
        let out = fill_once(t, &[("instance_id", "x{{self_understanding}}\n{{instance_id}}".into()), ("self_understanding", "(empty)".into())]);
        assert_eq!(out, "A x{{self_understanding}}\n{{instance_id}} B (empty) C {{unknown}} D {{unterminated");
    }

    #[test]
    fn the_rendered_origin_prompt_is_pinned_and_separates_declared_from_live() {
        use crate::tools::Tool as D;
        use rth_memory::schema::Tool as M;
        let tools = [D::Memory(M::Remember), D::Memory(M::Recall), D::Memory(M::CorrectMemory)];
        let live = Readiness { memory: Some("attached: brain binding rev 1".into()), delegation: None };
        let a = render_origin_with(ORIGIN_TEMPLATE, &tools, Some("inst-x"), Some(&live));
        let b = render_origin_with(ORIGIN_TEMPLATE, &tools, Some("inst-x"), Some(&live));
        assert_eq!(origin_digest(&a), origin_digest(&b), "deterministic");
        assert!(a.contains("Declared operations:\n- remember:") && a.contains("- memory: attached: brain binding rev 1") && a.contains("- delegation: not declared"));
        assert!(a.contains("No image, file or screen input."));
        let unobserved = render_origin_with(ORIGIN_TEMPLATE, &tools, Some("inst-x"), None);
        assert!(unobserved.contains("- memory: not observed at setup"), "a declaration is not readiness");
        assert_ne!(origin_digest(&a), origin_digest(&unobserved));
        assert!(!a.contains("{{"), "every placeholder filled");
        assert_eq!(origin_digest(&a).len(), 64);
        println!("ORIGIN_DIGEST {}", origin_digest(&a));
    }

    #[test]
    fn conversation_v2_1_is_the_default_and_names_itself() {
        let name = parse(None).unwrap();
        assert_eq!(name, "rth-persona-v2.1");
        let text = with_tools(system_instruction(None, None, name).unwrap(), &[]);
        for must in [
            "You are RTH, the agent of this RTH instance. When asked who or what you are, say first that you are RTH.",
            "Your inputs are speech and typed text only: you cannot see images, files or the screen.",
            "Describe only the runtime facts and tool results supplied here. Do not invent causes for timing or delays, narrate hidden processing, or claim knowledge of internals you have not been given.",
            "Use only the declared tools, subject to their permissions and results.",
            "Declared operations: none\nReadiness observed at setup (may change):\n- memory: not declared\n- delegation: not declared\nOnly tool results establish that an action was accepted, completed or saved.",
        ] {
            assert!(text.contains(must), "missing {must:?} in:\n{text}");
        }
        assert!(!text.contains("only talk with the user in this conversation") && !text.contains("personal workstation") && !text.contains("say you don't know"), "{text}");
        assert!(!text.contains("{{"), "no placeholder is left: {text}");

        assert_eq!(system_instruction(None, None, parse(Some("rth-persona-v2")).unwrap()).as_deref(), Some(PERSONA_V2));
    }

    #[test]
    fn p21_r1_declared_is_never_rendered_as_observed() {
        use crate::tools::Tool;
        use rth_tasks::schema::Tool as T;
        let mut all: Vec<Tool> = vec![Tool::Task(T::Delegate), Tool::Task(T::CancelTask)];
        all.extend(rth_memory::schema::ALL.iter().copied().map(Tool::Memory));
        let r = |m: Option<&str>, d: Option<&str>| Readiness { memory: m.map(str::to_string), delegation: d.map(str::to_string) };
        let none = render_v2_1(PERSONA_V2_1, &[], None);
        assert!(none.contains("Declared operations: none\n") && none.contains("- memory: not declared\n- delegation: not declared\n"), "{none}");
        assert!(!none.contains("memory tools") && !none.contains("worker"), "{none}");
        let unobserved = render_v2_1(PERSONA_V2_1, &all, None);
        assert!(unobserved.contains("Declared operations: delegate, cancel_task, remember, recall, correct_memory\n"), "{unobserved}");
        assert!(unobserved.contains("- memory: not observed\n- delegation: not observed\n"), "{unobserved}");
        let unavailable = render_v2_1(PERSONA_V2_1, &all, Some(&r(Some("unavailable: binding unattested"), None)));
        assert!(unavailable.contains("- memory: unavailable: binding unattested\n- delegation: not observed\n"), "{unavailable}");
        let attached = render_v2_1(PERSONA_V2_1, &all, Some(&r(Some("attached: brain binding rev 1"), Some("worker available"))));
        assert!(attached.contains("- memory: attached: brain binding rev 1\n- delegation: worker available\n"), "{attached}");
        for t in [&unobserved, &unavailable, &attached] {
            assert!(!t.contains(": on\n") && !t.contains(": on,") && !t.contains("You have a durable memory"), "{t}");
            assert!(t.contains("The memory tools (remember, recall, correct_memory) can propose, retrieve and correct saved memories when the store is available."), "{t}");
            assert!(t.contains("remember only proposes: nothing is saved until the user types the confirmation it returns, so never say something is saved before that."), "{t}");
            assert!(!t.contains("{{"), "{t}");
        }

        assert!(with_tools(PERSONA_V2_1.to_string(), &all).contains("Readiness observed at setup (may change):"));
    }
}
