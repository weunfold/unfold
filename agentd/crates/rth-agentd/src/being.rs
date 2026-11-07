

use std::sync::RwLock;

use rth_self::Name;

static NAME: RwLock<Option<Name>> = RwLock::new(None);

pub fn name() -> Name {
    NAME.read().unwrap().clone().unwrap_or_else(|| rth_self::SelfSpec::release_default("").name.current)
}

pub const LOCAL_TEMPLATE: &str = "unfold-local-text-v4.4";

pub const FLASH_TEMPLATE: &str = "unfold-flash-text-v4.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Templates {

    Legacy,

    V4,
}

pub const SHIPPED_TEMPLATES: Templates = Templates::Legacy;

pub fn compiled(spec: &rth_self::SelfSpec, local_model: &str, consented: rth_self::compile::Consented, memory: bool, shared: bool) -> (Option<crate::backend::Compiled>, Option<crate::backend::Compiled>) {
    use rth_self::compile::{compile, self_template, Backend, Facts};
    let device = crate::device::this().kind.to_string();
    let one = |backend, model: &str, template| {
        let facts = Facts { model: model.to_string(), device: device.clone(), speech: false, consented, dsh_watch: dsh_watch(model, &device), memory_shared: shared };
        compile(spec, &facts, backend, template).ok().map(|p| crate::backend::Compiled { text: p.text, template_version: p.template_version, spec_version: p.spec_version })
    };
    (one(Backend::Local, local_model, self_template(Backend::Local, memory)), one(Backend::Flash, crate::flash::DISPLAY_NAME, self_template(Backend::Flash, memory)))
}

pub fn budget(spec: &rth_self::SelfSpec, n_ctx: usize) -> Result<usize, rth_self::Refused> {
    budget_within(spec, crate::local::self_budget(n_ctx), n_ctx)
}

pub fn dsh_watch(model: &str, device: &str) -> rth_self::compile::DshWatch {
    rth_self::compile::DshWatch { route: crate::dsh_watch::route_fact().to_string(), local: crate::dsh_watch::local_fact(model, device) }
}

fn budget_facts() -> rth_self::compile::Facts {
    use rth_self::compile::{Consented, DshWatch, Facts};
    let longest = |names: Vec<&str>| names.into_iter().max_by_key(|n| n.chars().count()).unwrap_or_default().to_string();

    let model = longest(crate::local::DISPLAY_NAMES.iter().map(|(_, n)| *n).chain([crate::flash::DISPLAY_NAME]).collect());
    let device = longest(crate::device::ALL.iter().map(|d| d.kind).collect());
    let local = crate::dsh_watch::watched_local_fact(&model, &device);

    Facts { model, device, speech: true, consented: Consented { notification: true, omni: true }, dsh_watch: DshWatch { route: String::new(), local }, memory_shared: false }
}

pub fn budget_within(spec: &rth_self::SelfSpec, max: usize, n_ctx: usize) -> Result<usize, rth_self::Refused> {
    use rth_self::compile::{compile, Backend};
    let facts = budget_facts();
    let refused = |code, reason: String, need: Option<usize>| rth_self::Refused { field: "spec", code, reason, need: need.map(|n| n as u64), max: Some(max as u64) };
    let prompt = compile(spec, &facts, Backend::Local, LOCAL_TEMPLATE).map_err(|e| refused("malformed", format!("the Self settings don't compile: {e:?}"), None))?;

    let need = crate::local::system_tokens(&prompt.text);
    if need <= max {
        Ok(need)
    } else {
        Err(refused("budget", format!("these Self settings make the on-device prompt {need} tokens, over its {max}-token limit for a {n_ctx}-token context; shorten the drives or turn off a channel"), Some(need)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{self_budget, CTX_MARGIN_TOKENS, DEFAULT_MAX_TOKENS, DEFAULT_N_CTX, HISTORY_FLOOR_TOKENS, SAVED_MEMORIES_ALLOWANCE_TOKENS, SELF_HEADROOM_TOKENS, V42_DEFAULT_TOKENS};
    use rth_self::SelfSpec;

    fn largest() -> SelfSpec {
        let drive = |c: char| serde_json::json!({"text": std::iter::repeat_n(c, 80).collect::<String>(), "source": "person"});
        let three = |c| serde_json::json!([drive(c), drive(c), drive(c)]);
        let mut v = serde_json::to_value(SelfSpec::release_default("inst-test")).unwrap();
        v["name"]["current"] = serde_json::json!("Abcdefghijk Lmnopqrstuvw");
        v["drives"] = serde_json::json!({"motivations": three('m'), "aspirations": three('a'), "routine": three('r')});
        v["expression"]["destinations"] = serde_json::json!({"chat": true, "notification": true, "omni": true});
        SelfSpec::parse(&v.to_string()).expect("the largest spec is valid")
    }

    #[test]
    fn a7b_a_full_self_from_the_page_is_over_the_default_bound() {
        let drive = |c: char| serde_json::json!({"text": std::iter::repeat_n(c, 80).collect::<String>(), "source": "person"});
        let three = |c| serde_json::json!([drive(c), drive(c), drive(c)]);
        let mut v = serde_json::to_value(SelfSpec::release_default("inst-test")).unwrap();
        v["drives"] = serde_json::json!({"motivations": three('m'), "aspirations": three('a'), "routine": three('r')});
        v["personality"] = serde_json::json!({"archetype": "workbench", "axes": {"energy": 25, "mind": 20, "nature": 30, "tactics": 75, "identity": 30}});
        v["style"] = serde_json::json!({"length": "full", "register_mirroring": "strong", "humour": "playful", "question_rate": "often"});
        let full = SelfSpec::parse(&v.to_string()).expect("the page's full Self is valid");
        let need = budget_within(&full, usize::MAX, DEFAULT_N_CTX).unwrap();
        println!("A7B_FULL_SELF need={need} bound={}", self_budget(DEFAULT_N_CTX));
        assert!(need > self_budget(DEFAULT_N_CTX), "{need}");
    }

    #[test]
    fn watch_the_compiled_v4_prompts_carry_the_legacy_words() {
        crate::dsh_watch::set_watching();
        let spec = SelfSpec::release_default("inst-test");
        let kind = crate::device::this().kind;
        assert!(!crate::dsh_watch::route_fact().is_empty());
        for memory in [true, false] {
            let (local, flash) = compiled(&spec, "Granite 4.1 3B", Default::default(), memory, true);
            let (local, flash) = (local.unwrap().text, flash.unwrap().text);
            assert!(local.contains(&crate::dsh_watch::local_fact("Granite 4.1 3B", kind)), "{memory}: {local}");
            let route = flash.lines().find(|l| l.starts_with("Where messages go:")).unwrap();
            assert!(route.ends_with(crate::dsh_watch::route_fact()), "{memory}: {route}");
        }
    }

    #[test]
    fn watch_the_granite_budget_counts_the_watch_fact() {
        let f = budget_facts();
        assert_eq!(f.dsh_watch.local, crate::dsh_watch::watched_local_fact(&f.model, &f.device));
        assert!(f.dsh_watch.local.contains("Unfold also watches the person's DSH agents."));
        let spec = SelfSpec::release_default("inst-test");
        let p = rth_self::compile::compile(&spec, &f, rth_self::compile::Backend::Local, LOCAL_TEMPLATE).unwrap();
        assert!(p.text.contains(&f.dsh_watch.local));
        assert_eq!(budget_within(&spec, usize::MAX, crate::local::DEFAULT_N_CTX), Ok(crate::local::system_tokens(&p.text)));
    }

    #[test]
    fn a6_the_bound_follows_the_configured_context() {
        let term1 = V42_DEFAULT_TOKENS + SELF_HEADROOM_TOKENS;
        let term2 = |n: usize| n - DEFAULT_MAX_TOKENS as usize - CTX_MARGIN_TOKENS - SAVED_MEMORIES_ALLOWANCE_TOKENS - HISTORY_FLOOR_TOKENS;
        for (n_ctx, want) in [(4096, 1608), (8192, term1), (16384, term1)] {
            assert_eq!(self_budget(n_ctx), want, "n_ctx {n_ctx}");
            assert_eq!(self_budget(n_ctx), term1.min(term2(n_ctx)), "n_ctx {n_ctx}");
        }
        assert_eq!((term1, term2(8192)), (2621, 5704));
        assert_eq!(self_budget(512), 0, "a context too small for the reserves has no room");
    }

    #[test]
    fn a6_at_the_default_context_the_release_default_fits_and_its_measure_is_pinned() {
        assert_eq!(DEFAULT_N_CTX, 8192);
        assert_eq!(budget(&SelfSpec::release_default("inst-test"), DEFAULT_N_CTX), Ok(V42_DEFAULT_TOKENS), "a template change moves this measure: update V42_DEFAULT_TOKENS on purpose");
    }

    #[test]
    fn a6_at_the_default_context_the_largest_spec_is_refused_with_its_numbers() {
        let big = largest();
        let need = budget_within(&big, usize::MAX, DEFAULT_N_CTX).unwrap();
        let r = budget(&big, DEFAULT_N_CTX).unwrap_err();
        assert_eq!((r.field, r.code, r.need, r.max), ("spec", "budget", Some(need as u64), Some(self_budget(DEFAULT_N_CTX) as u64)));
        assert!(r.reason.contains("for a 8192-token context") && !r.reason.contains('{'), "{}", r.reason);
    }

    #[test]
    fn a6_the_refusal_is_worded_against_the_configured_context() {
        let r = budget(&SelfSpec::release_default("inst-test"), 4096).unwrap_err();
        assert_eq!((r.code, r.max), ("budget", Some(1608)));
        assert!(r.reason.contains("for a 4096-token context"), "{}", r.reason);
        assert!(budget(&largest(), 16384).is_err(), "term 1 binds above 8192 too");
    }

    #[test]
    fn a6_the_measure_does_not_depend_on_the_instance() {
        assert_eq!(budget(&SelfSpec::release_default("a"), DEFAULT_N_CTX), budget(&SelfSpec::release_default("other-instance"), DEFAULT_N_CTX));
    }

    #[test]
    fn a6_a_spec_just_over_any_bound_is_refused_and_at_it_fits() {
        let big = largest();
        let need = budget_within(&big, usize::MAX, DEFAULT_N_CTX).unwrap();
        assert!(need > V42_DEFAULT_TOKENS, "the largest spec is larger: {need}");
        assert_eq!(budget_within(&big, need - 1, DEFAULT_N_CTX).unwrap_err().need, Some(need as u64));
        assert_eq!(budget_within(&big, need, DEFAULT_N_CTX), Ok(need));
    }
}
