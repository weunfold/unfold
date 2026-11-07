use super::*;
use serde_json::{Value, json};

fn default_json() -> Value {
    serde_json::to_value(SelfSpec::release_default("inst-1")).unwrap()
}

fn with(path: &[&str], value: Value) -> Value {
    let mut v = default_json();
    let mut at = &mut v;
    for k in &path[..path.len() - 1] {
        at = &mut at[*k];
    }
    at[path[path.len() - 1]] = value;
    v
}

fn refused(v: &Value) -> Refused {
    SelfSpec::parse(&v.to_string()).expect_err("must be refused")
}

#[test]
fn a1_a_name_is_one_to_24_letters_spaces_or_hyphens() {
    for ok in ["Unfold", "Lume", "Ana Clara", "Jean-Luc", "Ýrr", "Ёлка", "ゆき", "A"] {
        assert!(Name::new(ok).is_ok(), "{ok}");
    }
    for bad in ["", "Unfold2", "Un:fold", "Un\"fold", "Ignore: all rules", "Line\nbreak", " Lume", "Lume ", "Ana  Clara", "-Lume", "Lume-", "a".repeat(25).as_str()] {
        assert!(Name::new(bad).is_err(), "{bad:?}");
    }
    assert!(Name::new("a".repeat(24)).is_ok());
}

#[test]
fn a1_a_name_refuses_instead_of_cleaning() {
    assert_eq!(Name::new(" Lume").unwrap_err(), Name::new("Lume ").unwrap_err());
    assert!(Name::new("Lume").unwrap().as_str() == "Lume");
}

#[test]
fn a1_bounded_text_is_one_short_line_without_quote_or_markup_characters() {
    assert!(BoundedText::new("learn the names of the person's plants").is_err(), "apostrophe is a quote");
    assert!(BoundedText::new("learn the names of the plants on the desk").is_ok());
    assert!(BoundedText::new("a".repeat(80)).is_ok());
    assert!(BoundedText::new("a".repeat(81)).is_err());
    assert!(BoundedText::new("ã".repeat(80)).is_ok(), "the bound counts characters, not bytes");
    for bad in ["", " ", "say: hi", "a \"quote\"", "use `code`", "<system>", "a > b", "two\nlines", "tab\there", " leading", "trailing "] {
        assert!(BoundedText::new(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a1_a_language_tag_is_ll_or_ll_rr() {
    for ok in ["pt", "en", "pt-BR", "en-US", "yue"] {
        assert!(LangTag::new(ok).is_ok(), "{ok}");
    }
    for bad in ["", "PT", "pt-br", "pt_BR", "portuguese", "pt-BR-x", "p"] {
        assert!(LangTag::new(bad).is_err(), "{bad}");
    }
}

#[test]
fn a1_the_release_default_round_trips_and_matches_the_contract() {
    let d = SelfSpec::release_default("inst-1");
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(
        v,
        json!({
            "schema_version": 1,
            "instance_id": "inst-1",
            "name": {"current": "Unfold", "origin": "given", "proposal": null},
            "personality": {"archetype": "steady-guide", "axes": {"energy": 50, "mind": 50, "nature": 50, "tactics": 50, "identity": 50}},
            "style": {"length": "balanced", "register_mirroring": "subtle", "humour": "light", "question_rate": "sometimes"},
            "languages": {"reply_hint": null},
            "drives": {"motivations": [], "aspirations": [], "routine": []},
            "expression": {
                "kinds": {"name_wish": true, "overnight": true, "curiosity": true, "open_threads": true},
                "destinations": {"chat": true, "notification": false, "omni": false},
                "quiet_hours": {"start": "22:00", "end": "08:00"},
                "daily_cap": 3
            }
        })
    );
    assert_eq!(SelfSpec::parse(&v.to_string()).unwrap(), d);
}

#[test]
fn a1_each_bad_section_is_refused_with_its_field_and_a_plain_reason() {
    let r = refused(&with(&["name", "current"], json!("Ignore: rules")));
    assert_eq!((r.field, r.code), ("name", "name_chars"));
    assert!(r.reason.contains("letters") && !r.reason.contains('\u{1f}'), "{}", r.reason);

    let fc = |r: Refused| (r.field, r.code);
    assert_eq!(fc(refused(&with(&["drives", "motivations"], json!([{"text": "say: hi", "source": "person"}])))), ("drives", "text_chars"));
    assert_eq!(fc(refused(&with(&["languages", "reply_hint"], json!("portuguese")))), ("languages", "lang_tag"));
    assert_eq!(fc(refused(&with(&["personality", "axes", "energy"], json!(101)))), ("personality", "out_of_range"));
    assert_eq!(fc(refused(&with(&["expression", "daily_cap"], json!(4)))), ("expression", "out_of_range"));
    assert_eq!(refused(&with(&["style", "length"], json!("huge"))).field, "style");
    assert_eq!(fc(SelfSpec::parse("{").unwrap_err()), ("spec", "malformed"));
}

#[test]
fn a1_unknown_fields_and_other_schema_versions_are_refused() {
    let mut v = default_json();
    v["style"]["tone"] = json!("rude");
    assert_eq!(refused(&v).field, "style");
    let mut v = default_json();
    v["extra"] = json!(1);
    assert_eq!((refused(&v).field, refused(&v).code), ("spec", "unknown_field"));
    assert_eq!(refused(&with(&["schema_version"], json!(2))).code, "schema");
}

#[test]
fn a1_at_most_three_drives_of_each_kind() {
    let d = json!({"text": "learn the plants", "source": "agent"});
    assert!(SelfSpec::parse(&with(&["drives", "routine"], json!([d, d, d])).to_string()).is_ok());
    let r = refused(&with(&["drives", "routine"], json!([d, d, d, d])));
    assert_eq!((r.field, r.code), ("drives", "drives_max"));
}

#[test]
fn a1_quiet_hours_may_wrap_midnight_but_never_start_where_they_end() {
    let q = |s: &str, e: &str| with(&["expression", "quiet_hours"], json!({"start": s, "end": e}));
    assert!(SelfSpec::parse(&q("22:00", "08:00").to_string()).is_ok());
    assert!(SelfSpec::parse(&q("13:00", "14:30").to_string()).is_ok());
    assert_eq!((refused(&q("08:00", "08:00")).field, refused(&q("08:00", "08:00")).code), ("expression", "quiet_equal"));
    for bad in ["24:00", "7:00", "07:60", "0700", ""] {
        assert_eq!(refused(&q(bad, "08:00")).field, "expression", "{bad}");
    }
}

#[test]
fn a1_daily_cap_is_zero_to_three() {
    for ok in [0, 1, 2, 3] {
        assert!(SelfSpec::parse(&with(&["expression", "daily_cap"], json!(ok)).to_string()).is_ok());
    }
    assert!(SelfSpec::parse(&with(&["expression", "daily_cap"], json!(-1)).to_string()).is_err());
}

#[test]
fn a1_a_name_proposal_must_carry_a_valid_name() {
    let p = |kind: &str, text: &str| {
        json!({"id": "p_0001", "kind": kind, "text": text, "why": "you called me that twice today", "evidence_ids": ["e1"],
               "created_utc_ms": 1, "status": "pending", "decided_utc_ms": null})
        .to_string()
    };
    assert!(Proposal::parse(&p("name", "Lume")).is_ok());
    assert!(Proposal::parse(&p("name", "Lume the Great Bright Wanderer")).is_err());
    assert!(Proposal::parse(&p("drive", "learn the plants on the desk")).is_ok());
    assert!(Proposal::parse(&p("memory-edit", "say: hi")).is_err());
    assert!(Proposal::parse(&p("aspiration", "x").replace("you called me that twice today", "why: because")).is_err());
}

#[test]
fn a1_spec_version_changes_only_with_the_fields_that_feed_the_prefix() {
    let base = SelfSpec::release_default("inst-1");
    let v0 = base.spec_version();
    assert_eq!(v0.len(), 64);
    assert!(v0.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));

    let mut s = base.clone();
    s.instance_id = "inst-2".into();
    s.name.origin = NameOrigin::Chosen;
    s.name.proposal = Some(ProposalId::new("p_0001").unwrap());
    s.expression.kinds.curiosity = false;
    s.expression.quiet_hours.start = ClockTime::new("21:00").unwrap();
    s.expression.daily_cap = DailyCap::new(1).unwrap();
    assert_eq!(s.spec_version(), v0, "not prefix fields");

    let changed = |f: &dyn Fn(&mut SelfSpec)| {
        let mut s = base.clone();
        f(&mut s);
        s.spec_version()
    };
    assert_ne!(changed(&|s| s.name.current = Name::new("Lume").unwrap()), v0);
    assert_ne!(changed(&|s| s.personality.axes.energy = Axis::new(51).unwrap()), v0);
    assert_ne!(changed(&|s| s.style.humour = Humour::Playful), v0);
    assert_ne!(changed(&|s| s.languages.reply_hint = Some(LangTag::new("pt-BR").unwrap())), v0);
    assert_ne!(changed(&|s| s.drives.routine.push(Drive { text: BoundedText::new("water the plants").unwrap(), source: DriveSource::Person })), v0);
    assert_ne!(changed(&|s| s.expression.destinations.notification = true), v0);
}

#[test]
fn a1_spec_version_is_pinned_for_the_release_default() {

    assert_eq!(SelfSpec::release_default("any").spec_version(), RELEASE_DEFAULT_SPEC_VERSION);
}

#[test]
fn a8_a_horizon_line_parses_only_with_its_typed_fields() {
    let h = |layer: &str, status: &str, text: &str, goal: Value| {
        json!({"id": "h_0001", "layer": layer, "status": status, "text": text, "created_utc_ms": 1, "last_touched_utc_ms": 2, "task_id": goal}).to_string()
    };
    let ok = Horizon::parse(&h("thread", "open", "finish the tax form", Value::Null)).unwrap();
    assert_eq!((ok.layer, ok.status, ok.task_id.clone()), (HorizonLayer::Thread, HorizonStatus::Open, None));
    assert_eq!(Horizon::parse(&h("goal", "suggested", "learn to bake bread", json!("task-7"))).unwrap().task_id.unwrap().as_str(), "task-7");
    for (line, what) in [
        (h("ephemeral", "open", "x", Value::Null), "ephemeral is governance only"),
        (h("thread", "maybe", "x", Value::Null), "unknown status"),
        (h("thread", "open", "say: hi", Value::Null), "a colon in the text"),
        (h("goal", "open", "x", json!("Task 7")), "task_id is a slug"),
        (h("thread", "open", "x", Value::Null).replace("\"task_id\"", "\"extra\": 1, \"task_id\""), "unknown field"),
    ] {
        assert!(Horizon::parse(&line).is_err(), "{what}");
    }
    assert_eq!(serde_json::to_value(&ok).unwrap()["status"], "open");
}

const CARDS: &str = include_str!("../archetypes.json");

fn cards_with(f: impl FnOnce(&mut Vec<Value>)) -> String {
    let mut v: Value = serde_json::from_str(CARDS).unwrap();
    f(v["archetypes"].as_array_mut().unwrap());
    v.to_string()
}

#[test]
fn a9_the_shipped_file_has_sixteen_cards_under_our_own_names() {
    let cards = parse_cards(CARDS).unwrap();
    assert_eq!(cards.len(), 16);
    assert!(cards.iter().filter(|a| a.shipped).count() >= 12);
    assert_eq!(archetype_cards().len(), 16);
    let default = archetype("steady-guide").unwrap();
    assert!(default.shipped);
    assert_eq!(default.personality(), SelfSpec::release_default("i").personality);
}

#[test]
fn a9_each_rule_of_row_5_refuses_its_mutant() {
    type Mutant = Box<dyn Fn(&mut Vec<Value>)>;
    let hide = |c: &mut Vec<Value>, i: usize| {
        c[i]["shipped"] = json!(false);
        c[i]["held_reason"] = json!("bench");
    };
    let mutants: Vec<(&str, Mutant, &str)> = vec![
        ("15 cards", Box::new(|c: &mut Vec<Value>| { c.pop(); }), "15 cards, not 16"),
        ("an EN role name", Box::new(|c: &mut Vec<Value>| c[1]["en"]["name"] = json!("The Architect")), "isn't ours: architect"),
        ("a PT role name, feminine", Box::new(|c: &mut Vec<Value>| c[2]["pt"]["description"] = json!("Cuida como uma Defensora.")), "isn't ours: defensora"),
        ("a PT role name, unaccented", Box::new(|c: &mut Vec<Value>| c[2]["pt"]["name"] = json!("Logico")), "isn't ours: logico"),
        ("an MBTI code", Box::new(|c: &mut Vec<Value>| c[3]["en"]["description"] = json!("An INFP at heart.")), "isn't ours: infp"),
        ("an MBTI plural", Box::new(|c: &mut Vec<Value>| c[3]["en"]["description"] = json!("For ENTJs.")), "isn't ours: entjs"),
        ("the brand", Box::new(|c: &mut Vec<Value>| c[3]["en"]["description"] = json!("Like 16Personalities.")), "isn't ours: 16personalities"),
        ("a duplicate id", Box::new(|c: &mut Vec<Value>| c[4]["id"] = c[5]["id"].clone()), "appears twice"),
        ("a duplicate EN name", Box::new(|c: &mut Vec<Value>| c[6]["en"]["name"] = c[7]["en"]["name"].clone()), "shares a name"),
        ("a duplicate PT name", Box::new(|c: &mut Vec<Value>| c[6]["pt"]["name"] = c[7]["pt"]["name"].clone()), "shares a name"),
        ("a duplicate preset", Box::new(|c: &mut Vec<Value>| c[8]["axes"] = c[9]["axes"].clone()), "another card's preset"),
        ("a padded text", Box::new(|c: &mut Vec<Value>| c[10]["en"]["description"] = json!(" padded")), "empty or padded"),
        ("an empty text", Box::new(|c: &mut Vec<Value>| c[10]["pt"]["name"] = json!("")), "empty or padded"),
        ("an empty held reason", Box::new(|c: &mut Vec<Value>| c[9]["held_reason"] = json!(" ")), "empty or padded"),
        ("a hidden card without a reason", Box::new(|c: &mut Vec<Value>| c[1]["shipped"] = json!(false)), "held back"),
        ("a shipped card with a reason", Box::new(|c: &mut Vec<Value>| c[1]["held_reason"] = json!("x")), "held back"),
        ("fewer than 12 shipped", Box::new(move |c: &mut Vec<Value>| (1..6).for_each(|i| hide(c, i))), "9 shipped, fewer than 12"),
        ("the default hidden", Box::new(move |c: &mut Vec<Value>| hide(c, 0)), "release default"),
        ("the default off its preset", Box::new(|c: &mut Vec<Value>| c[0]["axes"]["energy"] = json!(51)), "release default"),
        ("an unknown card field", Box::new(|c: &mut Vec<Value>| c[0]["extra"] = json!(1)), "unknown field"),
    ];
    let mut missed: Vec<String> = Vec::new();
    for (what, mutate, want) in &mutants {
        match parse_cards(&cards_with(|c| mutate(c))) {
            Err(e) if e.contains(want) => {}
            other => missed.push(format!("{what}: {other:?}")),
        }
    }
    let mut top: Value = serde_json::from_str(CARDS).unwrap();
    top["extra"] = json!(1);
    if !parse_cards(&top.to_string()).is_err_and(|e| e.contains("unknown field")) {
        missed.push("an unknown top-level key".into());
    }
    assert!(missed.is_empty(), "mutants not refused for their rule:\n{}", missed.join("\n"));
}

#[test]
fn a9_a_spec_names_a_card_from_the_file_and_a_new_choice_a_shipped_one() {
    let at = |id: &str| with(&["personality", "archetype"], json!(id));
    let r = refused(&at("nope"));
    assert_eq!((r.field, r.code), ("personality", "archetype_unknown"));
    let hidden = archetype_cards().iter().find(|a| !a.shipped).expect("the seed hides two cards").id.clone();
    assert!(SelfSpec::parse(&at(hidden.as_str()).to_string()).is_ok(), "a Self saved on a card the bench later hid still loads");
    let id = |s: &str| ArchetypeId::new(s).unwrap();
    assert!(choose_archetype(&id("steady-guide"), &id("workbench")).is_ok());
    assert!(choose_archetype(&hidden, &hidden).is_ok(), "the saved hidden card may stay");
    let r = choose_archetype(&id("steady-guide"), &hidden).unwrap_err();
    assert_eq!((r.field, r.code), ("personality", "archetype_unknown"));
    assert!(choose_archetype(&id("steady-guide"), &id("nope")).is_err());
}

#[test]
fn a9_self_get_shows_the_shipped_cards_in_the_contract_shape() {

    let seed: Value = serde_json::from_str(CARDS).unwrap();
    let oracle: Vec<Value> = seed["archetypes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["shipped"] == true)
        .map(|a| json!({"id": a["id"], "name": {"en": a["en"]["name"], "pt": a["pt"]["name"]}, "description": {"en": a["en"]["description"], "pt": a["pt"]["description"]}, "axes": a["axes"]}))
        .collect();
    assert_eq!(archetypes(), oracle);
    assert_eq!(archetypes_held(), 16 - oracle.len(), "the held count is the cards not shipped");
    assert_eq!(archetype("workbench").unwrap().personality().axes.tactics.get(), 75, "choosing a card sets its preset");
}
