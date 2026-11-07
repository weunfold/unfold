

use speech_markup::Mood;
use unfold_speech::{Catalog, Language, Source, VoiceEntry};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, root, id, name, refs] = &a[..] else {
        eprintln!("usage: register ROOT ID NAME REF_DIR");
        std::process::exit(2);
    };
    let catalog = Catalog::new(root);
    let mut v = VoiceEntry::new(id, name, Language::PtBr, Source::Personal, "qwen3-tts-1.7b-base").expect("voice id");
    v.temperature = Some(1.0);
    for (mood, tag) in [(Mood::Neutral, "neutral"), (Mood::Calm, "calm"), (Mood::Joy, "joy"), (Mood::Serious, "serious"), (Mood::Question, "question")] {
        let wav = format!("{refs}/ref-{tag}.wav");
        let Ok(bytes) = std::fs::read(&wav) else { continue };
        let text = std::fs::read_to_string(format!("{refs}/ref-{tag}.txt")).expect("transcript");
        catalog.set_ref(&mut v, mood, &bytes, &text).unwrap_or_else(|e| panic!("{tag}: {e}"));
        println!("{tag}: {:.2} s", v.moods[&mood].duration_s);
    }
    let back = catalog.load(id).expect("reload");
    println!("registered {} ({} moods) at {}", back.id, back.moods.len(), back.dir().display());
}
