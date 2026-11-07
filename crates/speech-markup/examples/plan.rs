

use speech_markup::{parse, plan, Event, Lang, Mood, Step, Voice};
use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = if args.first().map(String::as_str) == Some("en") { Lang::En } else { Lang::Pt };
    let list = |i: usize| args.get(i).map(|s| s.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    let voice = Voice {
        lang,
        moods: list(1).iter().filter_map(|m| Mood::parse(m)).collect(),
        clips: list(2).iter().filter_map(|e| Event::parse(e)).collect(),
    };
    let mut reply = String::new();
    std::io::stdin().read_to_string(&mut reply).expect("stdin");
    let parsed = parse(&reply);
    println!("speak: {}  reply mood: {:?}", parsed.speak, parsed.reply_mood);
    println!("display: {}", parsed.display);
    let (steps, warnings) = plan(&parsed, &voice);
    if args.iter().any(|a| a == "--json") {
        let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let low = |d: String| d.to_lowercase();
        let st: Vec<String> = steps.iter().map(|s| match s {
            Step::Speak { text, mood, .. } => format!("{{\"speak\":{},\"mood\":{}}}", q(text), q(&low(format!("{mood:?}")))),
            Step::Clip(e) => format!("{{\"clip\":{}}}", q(&low(format!("{e:?}")))),
            Step::Silence { ms } => format!("{{\"silence_ms\":{ms}}}"),
        }).collect();
        let w: Vec<String> = parsed.warnings.iter().chain(&warnings).map(|w| q(&format!("{w:?}"))).collect();
        println!("{{\"display\":{},\"steps\":[{}],\"warnings\":[{}]}}", q(&parsed.display), st.join(","), w.join(","));
        return;
    }
    for s in &steps {
        println!("  {s:?}");
    }
    for w in parsed.warnings.iter().chain(&warnings) {
        println!("  warning: {w:?}");
    }
}
