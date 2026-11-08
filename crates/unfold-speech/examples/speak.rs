use speech_markup::Mood;
use std::io::Write;
use unfold_speech::{ApiKey, Catalog, Client};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, base, root, id, out] = &a[..] else { eprintln!("usage: speak BASE_URL ROOT ID OUT_DIR"); std::process::exit(2); };
    let key = std::env::var("UNFOLD_SPEECH_KEY").ok().map(ApiKey::new);
    let client = Client::new(base, key).expect("base url");
    let voice = Catalog::new(root).load(id).expect("load voice");
    std::fs::create_dir_all(out).expect("out dir");
    let lines = [
        (Mood::Neutral, "neutral", "Oi, aqui é o Unfold. Estou pronto para conversar."),
        (Mood::Calm, "calm", "Tudo certo por aqui. Pode ir com calma."),
        (Mood::Joy, "joy", "Que notícia boa! Isso deu muito certo."),
        (Mood::Serious, "serious", "Atenção: essa ação não pode ser desfeita."),
        (Mood::Question, "question", "Quer que eu continue daqui?"),
    ];
    for (mood, tag, text) in lines {
        if !voice.moods.contains_key(&mood) {
            continue;
        }
        let t = std::time::Instant::now();
        let s = client.synthesize(&voice.request(mood, text).expect("request")).unwrap_or_else(|e| panic!("{tag}: {e}"));
        let rate = s.chunks.first().map_or(24_000, |c| c.sample_rate);
        let pcm: Vec<i16> = s.chunks.iter().flat_map(|c| c.samples.iter().copied()).collect();
        write_wav(&format!("{out}/{tag}.wav"), rate, &pcm);
        println!("{tag}: {:.2} s audio in {:.2} s, truncated={}, error={:?}", s.done.audio_seconds, t.elapsed().as_secs_f64(), s.done.truncated, s.done.error);
    }
}

fn write_wav(path: &str, rate: u32, pcm: &[i16]) {
    let data = (pcm.len() * 2) as u32;
    let mut f = std::fs::File::create(path).expect("wav");
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + data).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&rate.to_le_bytes()).unwrap();
    f.write_all(&(rate * 2).to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&data.to_le_bytes()).unwrap();
    for s in pcm {
        f.write_all(&s.to_le_bytes()).unwrap();
    }
}
