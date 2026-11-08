use unfold_speech::Catalog;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, root, id, st] = &a[..] else { eprintln!("usage: set_pitch ROOT ID SEMITONES"); std::process::exit(2); };
    let catalog = Catalog::new(root);
    let mut v = catalog.load(id).expect("load voice");
    v.pitch_semitones = Some(st.parse().expect("a number"));
    catalog.save(&mut v).unwrap_or_else(|e| panic!("save: {e}"));
    let back = catalog.load(id).expect("reload");
    println!("{}: pitch_semitones {:?}, {} moods, model {}", back.id, back.pitch_semitones, back.moods.len(), back.model);
}
