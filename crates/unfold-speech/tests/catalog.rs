mod common;

use std::collections::BTreeSet;

use base64::Engine;
use common::*;
use sha2::{Digest, Sha256};
use speech_markup::{parse, plan, Event, Lang, Mood, Step};
use unfold_speech::{Catalog, CatalogError, Language, Source, VoiceEntry};

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn save_then_load_round_trips() {
    let (catalog, voice) = voice("roundtrip", &[Mood::Joy, Mood::Calm, Mood::Question]);
    let loaded = catalog.load("sample").unwrap();
    assert_eq!(loaded, voice);
    assert_eq!(loaded.temperature, Some(1.0));
    assert_eq!(loaded.language, Language::PtBr);
    assert_eq!(loaded.source, Source::Personal);
    assert_eq!(loaded.moods.len(), 3);
    assert!(loaded.events.contains_key(&Event::Laugh));
    let listed = catalog.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_ref().unwrap(), &voice);
    assert!(voice.dir().join("voice.json").is_file());
}

#[test]
fn a_changed_clip_is_a_sha_mismatch_and_a_missing_one_is_typed() {
    let (catalog, voice) = voice("sha", &[Mood::Joy, Mood::Calm]);
    let joy = voice.clip_path(&voice.moods[&Mood::Joy]);
    std::fs::write(&joy, wav(1.5, 42)).unwrap();
    match catalog.load("sample") {
        Err(CatalogError::ShaMismatch { voice, file, expected, actual }) => {
            assert_eq!((voice.as_str(), file.as_str()), ("sample", "mood-joy.wav"));
            assert_ne!(expected, actual);
        }
        other => panic!("expected ShaMismatch, got {other:?}"),
    }

    assert!(matches!(voice.request(Mood::Joy, "oi"), Err(CatalogError::ShaMismatch { .. })));
    std::fs::remove_file(&joy).unwrap();
    assert!(matches!(catalog.load("sample"), Err(CatalogError::MissingClip { .. })));
}

#[test]
fn broken_manifests_and_ids_are_errors_not_panics() {
    let catalog = Catalog::new(root("broken"));
    assert_eq!(catalog.load("nobody"), Err(CatalogError::NotFound("nobody".into())));
    assert!(matches!(catalog.load("../etc"), Err(CatalogError::InvalidId(_))));
    assert!(VoiceEntry::new("Sample!", "x", Language::En, Source::Stock, "m").is_err());
    let (catalog, mut voice) = voice("broken2", &[Mood::Joy]);
    std::fs::write(voice.dir().join("voice.json"), b"{not json").unwrap();
    assert!(matches!(catalog.load("sample"), Err(CatalogError::Manifest { .. })));

    assert!(matches!(catalog.set_ref(&mut voice, Mood::Calm, &wav(0.5, 1), "Curta."), Err(CatalogError::BadClip { .. })));
    assert!(matches!(catalog.set_ref(&mut voice, Mood::Calm, b"not a wav", "x"), Err(CatalogError::BadClip { .. })));
    assert!(matches!(catalog.set_ref(&mut voice, Mood::Calm, &wav(1.5, 1), "  "), Err(CatalogError::BadClip { .. })));
}

#[test]
fn set_ref_and_clear_ref() {
    let (catalog, mut voice) = voice("setclear", &[]);
    let audio = wav(2.0, 3);
    catalog.set_ref(&mut voice, Mood::Tender, &audio, "Fala baixinho.").unwrap();
    let clip = voice.moods[&Mood::Tender].clone();
    assert_eq!((clip.file.as_str(), clip.transcript.as_str(), clip.duration_s), ("mood-tender.wav", "Fala baixinho.", 2.0));
    assert_eq!(clip.sha256, hex(&audio));
    assert_eq!(std::fs::read(voice.clip_path(&clip)).unwrap(), audio);
    assert_eq!(catalog.load("sample").unwrap().moods[&Mood::Tender], clip);

    catalog.clear_ref(&mut voice, Mood::Tender).unwrap();
    assert!(!voice.moods.contains_key(&Mood::Tender));
    assert!(!voice.dir().join("mood-tender.wav").exists());
    assert!(catalog.load("sample").unwrap().moods.is_empty());

    catalog.clear_event_clip(&mut voice, Event::Laugh).unwrap();
    assert!(catalog.load("sample").unwrap().events.is_empty());
}

#[test]
fn markup_voice_offers_only_the_moods_with_references() {
    let (_c, voice) = voice("markup", &[Mood::Joy, Mood::Calm]);
    let mv = voice.to_markup_voice();
    assert_eq!(mv.lang, Lang::Pt);
    assert_eq!(mv.moods, BTreeSet::from([Mood::Joy, Mood::Calm]));
    assert_eq!(mv.clips, BTreeSet::from([Event::Laugh]));

    let (steps, _) = plan(&parse("⟨say:speech mood=serious⟩\nNão dá."), &mv);
    assert_eq!(steps, vec![Step::Speak { text: "Não dá.".into(), mood: Mood::Calm, requested: Mood::Serious }]);
}

#[test]
fn the_request_carries_the_resolved_moods_reference_only() {
    let (_c, voice) = voice("request", &[Mood::Joy, Mood::Calm]);
    for mood in [Mood::Joy, Mood::Calm] {
        let req = voice.request(mood, "Oi!").unwrap();
        assert_eq!(req.references.len(), 1);
        assert_eq!(req.references[0].text, transcript(mood));
        let file = std::fs::read(voice.clip_path(&voice.moods[&mood])).unwrap();
        assert_eq!(req.references[0].audio_base64, base64::engine::general_purpose::STANDARD.encode(file));
        assert_eq!((req.model.as_str(), req.input.as_str(), req.temperature), ("qwen3-tts-1.7b-base", "Oi!", Some(1.0)));
        assert!(req.stream && req.stream_format == "events");
    }
    assert!(matches!(voice.request(Mood::Tired, "x"), Err(CatalogError::NoReference { mood: Mood::Tired, .. })));
}

#[test]
fn the_playback_pitch_is_saved_with_the_voice_and_bounded() {
    let (catalog, mut voice) = voice("pitch", &[Mood::Neutral]);
    assert_eq!(catalog.load("sample").unwrap().pitch_semitones, None, "a voice without it: no shift");
    voice.pitch_semitones = Some(-3.0);
    catalog.save(&mut voice).unwrap();
    assert_eq!(catalog.load("sample").unwrap().pitch_semitones, Some(-3.0), "the owner's -3 survives a reload");
    voice.pitch_semitones = Some(-13.0);
    assert!(catalog.save(&mut voice).is_err(), "beyond an octave: refused");
    voice.pitch_semitones = Some(f32::NAN);
    assert!(catalog.save(&mut voice).is_err(), "not a number: refused");
}
