use sha2::{Digest, Sha256};

include!("src/sherpa_pins.rs");

fn sherpa_dylibs() {
    println!("cargo:rerun-if-env-changed=SHERPA_ONNX_LIB_DIR");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    assert!(arch == "aarch64", "no pinned sherpa-onnx shared library for {arch} (../README.md)");
    let dir = std::env::var("SHERPA_ONNX_LIB_DIR").unwrap_or_else(|_| panic!("set SHERPA_ONNX_LIB_DIR to the absolute path of the directory holding the pinned sherpa-onnx dylibs (../README.md)"));
    let dir = std::path::Path::new(&dir);

    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    let mut want: Vec<&str> = SHERPA_DYLIBS.iter().map(|d| d.0).collect();
    want.sort();
    assert!(names == want, "{} must hold only {want:?}, not {names:?} (../README.md)", dir.display());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let profile = out.ancestors().nth(3).expect("OUT_DIR in cargo's layout");
    let dests = [std::path::PathBuf::from("gen/frameworks"), profile.join("Frameworks")];
    for (name, size, pin, _) in SHERPA_DYLIBS {
        let p = dir.join(name);
        println!("cargo:rerun-if-changed={}", p.display());
        let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let got: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert!(bytes.len() as u64 == size && got == pin, "{} is not the pinned {name}: sha256 {got}, pinned {pin}", p.display());
        for d in &dests {
            let to = d.join(name);

            println!("cargo:rerun-if-changed={}", to.display());

            if std::fs::read(&to).ok().as_deref() != Some(&bytes[..]) {
                std::fs::create_dir_all(d).unwrap();
                std::fs::write(&to, &bytes).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
            }
        }
    }
}

fn app_version() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let json = |s: &str, what: &str| serde_json::from_str::<serde_json::Value>(s).unwrap_or_else(|e| panic!("{what} is not JSON: {e}"));
    let base = json(&std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json"), "tauri.conf.json");

    let overlay = std::env::var("TAURI_CONFIG").ok().map(|c| json(&c, "TAURI_CONFIG"));
    let v = overlay.as_ref().and_then(|o| o["version"].as_str()).or(base["version"].as_str()).expect("tauri.conf.json names no version");
    println!("cargo:rustc-env=UNFOLD_APP_VERSION={v}");
}

fn main() {
    sherpa_dylibs();
    app_version();
    turn_models();

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&["rth_setup", "rth_settings", "rth_dictation"]))).expect("tauri build")
}

fn turn_models() {

    println!("cargo:rerun-if-env-changed=UNFOLD_TURN_MODELS_DIR");
    let source = std::path::PathBuf::from(std::env::var_os("UNFOLD_TURN_MODELS_DIR").expect("set UNFOLD_TURN_MODELS_DIR to the directory containing the pinned ONNX files"));
    let pins = std::fs::read_to_string("turn-models/pins.json").expect("turn model pins");
    println!("cargo:rerun-if-changed=turn-models/pins.json");
    let pins: serde_json::Value = serde_json::from_str(&pins).expect("turn model pins JSON");
    for pin in pins.as_array().expect("turn model pin list") {
        let name=pin["name"].as_str().expect("model name");
        assert!(name.ends_with(".onnx") && !name.contains('/') && !name.contains(".."));
        let path=source.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes=std::fs::read(&path).expect("pinned local model file");
        assert_eq!(bytes.len() as u64,pin["bytes"].as_u64().expect("model size"));
        assert_eq!(format!("{:x}",Sha256::digest(&bytes)),pin["sha256"].as_str().expect("model sha256"));
        let dest = std::path::Path::new("gen/turn-models").join(name);
        if std::fs::read(&dest).ok().as_deref() != Some(bytes.as_slice()) {
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::write(&dest, &bytes).expect("write generated model");
        }
    }
}
