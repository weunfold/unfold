

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

include!("sherpa_pins.rs");

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn core_sha(p: &Path) -> String {
    let t = std::env::temp_dir().join(format!("rth-sherpa-core-{}-{}", std::process::id(), p.file_name().unwrap().to_string_lossy()));
    std::fs::copy(p, &t).unwrap();
    assert!(Command::new("/usr/bin/codesign").arg("--remove-signature").arg(&t).status().unwrap().success(), "{}", p.display());
    let mut b = std::fs::read(&t).unwrap();
    let _ = std::fs::remove_file(&t);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    if b[..4] == [0xcf, 0xfa, 0xed, 0xfe] {
        let mut off = 32;
        for _ in 0..u32_at(&b, 16) {
            let (cmd, size) = (u32_at(&b, off), u32_at(&b, off + 4) as usize);
            if cmd == 0x19 && b[off + 8..off + 24].split(|&c| c == 0).next() == Some(b"__LINKEDIT") {
                b[off + 32..off + 40].fill(0);
            }
            off += size;
        }
    }
    sha(&b)
}

fn otool(flag: &str, p: &Path) -> String {
    let o = Command::new("/usr/bin/otool").arg(flag).arg(p).output().unwrap();
    assert!(o.status.success(), "otool {flag} {}", p.display());
    String::from_utf8(o.stdout).unwrap()
}

fn linked(p: &Path) -> Vec<String> {
    let own = p.extension().is_some_and(|e| e == "dylib");
    otool("-L", p).lines().skip(if own { 2 } else { 1 }).map(|l| l.trim().split(" (").next().unwrap().to_string()).collect()
}

fn rpaths(p: &Path) -> Vec<String> {
    let l = otool("-l", p);
    let lines: Vec<&str> = l.lines().map(str::trim).collect();
    lines.windows(3).filter(|w| w[0] == "cmd LC_RPATH").map(|w| w[2].trim_start_matches("path ").split(" (offset").next().unwrap().to_string()).collect()
}

fn system(dep: &str) -> bool {
    dep.starts_with("/usr/lib/") || dep.starts_with("/System/Library/")
}

fn exe() -> PathBuf {
    std::env::current_exe().unwrap().canonicalize().unwrap()
}

fn frameworks() -> PathBuf {
    exe().parent().unwrap().join("../Frameworks")
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_build_puts_the_pinned_dylibs_in_gen_frameworks_and_next_to_the_binary() {
    for dir in [manifest().join("gen/frameworks"), frameworks()] {
        for (name, size, pin, _) in SHERPA_DYLIBS {
            let b = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{}/{name}: {e}", dir.display()));
            assert_eq!((b.len() as u64, sha(&b)), (size, pin.to_string()), "{}/{name}", dir.display());
        }
    }
}

#[test]
fn every_bundled_engine_ships_its_licence_files() {
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(manifest().join("tauri.conf.json")).unwrap()).unwrap();
    assert_eq!(conf["bundle"]["resources"]["licenses/"], "licenses/", "licenses/ is not a bundle resource");
    let notices = std::fs::read_to_string(manifest().join("licenses/NOTICES.txt")).expect("licenses/NOTICES.txt");

    let symbols = Command::new("/usr/bin/nm").arg("-C").arg(manifest().join("gen/frameworks/libsherpa-onnx-c-api.dylib")).output().unwrap();
    let symbols = String::from_utf8_lossy(&symbols.stdout);
    for (marker, file) in [("kiss_fft", "kissfft-LICENSE.txt"), ("knf::", "kaldi-native-fbank-LICENSE.txt"), ("kaldi_decoder::", "kaldi-decoder-LICENSE.txt"), ("kaldifst::", "kaldifst-LICENSE.txt"), ("fst::", "openfst-COPYING.txt"), ("Eigen::", "eigen-COPYING.MPL2.txt"), ("ssentencepiece::", "simple-sentencepiece-LICENSE.txt"), ("Darts::", "darts-clone-LICENSE.txt"), ("utf8proc_", "utf8proc-LICENSE.txt"), ("nlohmann::", "nlohmann-json-LICENSE.txt"), ("fastclustercpp::", "hclust-cpp-LICENSE.txt")] {
        assert!(symbols.contains(marker), "no {marker} in the c-api dylib: drop {file}");
        assert!(SHERPA_LICENCES.iter().any(|l| l.0 == "libsherpa-onnx-c-api.dylib" && l.1 == file), "{marker} is in the c-api dylib with no licence file");
    }
    for f in conf["bundle"]["macOS"]["frameworks"].as_array().unwrap() {
        let dylib = Path::new(f.as_str().unwrap()).file_name().unwrap().to_str().unwrap();
        let files: Vec<_> = SHERPA_LICENCES.iter().filter(|l| l.0 == dylib).collect();
        assert!(!files.is_empty(), "{dylib} ships with no licence file");
        for (_, file, pin, _) in files {
            let b = std::fs::read(manifest().join("licenses").join(file)).unwrap_or_else(|e| panic!("licenses/{file}: {e}"));
            assert_eq!(sha(&b), *pin, "licenses/{file}");
            assert!(notices.contains(file), "NOTICES.txt doesn't name {file}");
        }
    }
}

#[test]
fn the_bundle_ships_the_two_dylibs_in_contents_frameworks() {
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(manifest().join("tauri.conf.json")).unwrap()).unwrap();
    let want: Vec<String> = SHERPA_DYLIBS.iter().map(|d| format!("gen/frameworks/{}", d.0)).collect();
    assert_eq!(conf["bundle"]["macOS"]["frameworks"], serde_json::json!(want));
}

#[test]
fn the_binary_loads_sherpa_onnx_only_from_its_own_frameworks_dir() {
    let deps = linked(&exe());
    for (name, ..) in SHERPA_DYLIBS {
        assert!(deps.contains(&format!("@rpath/{name}")), "{name} not linked by @rpath: {deps:?}");
    }
    let names: Vec<String> = SHERPA_DYLIBS.iter().map(|d| format!("@rpath/{}", d.0)).collect();
    assert!(deps.iter().all(|d| names.contains(d) || system(d)), "a non-system library: {deps:?}");
    assert!(!deps.iter().any(|d| d.contains("/usr/local") || d.contains("/opt/homebrew")), "{deps:?}");
    assert_eq!(rpaths(&exe()), ["@executable_path/../Frameworks"]);
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(i: u32) -> *const std::ffi::c_char;
    }
    let loaded: Vec<PathBuf> = (0..unsafe { _dyld_image_count() })
        .filter_map(|i| {
            let n = unsafe { _dyld_get_image_name(i) };
            (!n.is_null()).then(|| PathBuf::from(unsafe { std::ffi::CStr::from_ptr(n) }.to_string_lossy().into_owned()))
        })
        .collect();
    for (name, ..) in SHERPA_DYLIBS {
        let from: Vec<&PathBuf> = loaded.iter().filter(|p| p.file_name().is_some_and(|f| f == name)).collect();
        assert_eq!(from.len(), 1, "{name} loaded {} times", from.len());
        assert_eq!(from[0].canonicalize().unwrap(), frameworks().join(name).canonicalize().unwrap(), "{name}");
    }
}

#[test]
fn the_dylibs_link_only_each_other_and_the_system() {
    let names: Vec<String> = SHERPA_DYLIBS.iter().map(|d| format!("@rpath/{}", d.0)).collect();
    for (name, ..) in SHERPA_DYLIBS {
        let p = manifest().join("gen/frameworks").join(name);
        let deps = linked(&p);
        assert!(deps.iter().all(|d| names.contains(d) || system(d)), "{name}: {deps:?}");
        assert!(rpaths(&p).iter().all(|r| r == "@loader_path"), "{name}: {:?}", rpaths(&p));
    }
}

fn app_bundle() -> PathBuf {
    PathBuf::from(std::env::var("RTH_APP_BUNDLE").expect("set RTH_APP_BUNDLE to the .app"))
}

fn team(p: &Path) -> Option<String> {
    let o = Command::new("/usr/bin/codesign").arg("-dv").arg(p).output().unwrap();
    String::from_utf8_lossy(&o.stderr).lines().find_map(|l| l.strip_prefix("TeamIdentifier=")).filter(|t| *t != "not set").map(str::to_string)
}

#[test]
#[ignore]
fn a_built_app_bundle_carries_the_pinned_dylibs() {
    let app = app_bundle();
    let plist = app.join("Contents/Info.plist");
    let o = Command::new("/usr/bin/plutil").args(["-extract", "CFBundleExecutable", "raw"]).arg(&plist).output().unwrap();
    let bin = app.join("Contents/MacOS").join(String::from_utf8(o.stdout).unwrap().trim());
    assert_eq!(rpaths(&bin), ["@executable_path/../Frameworks"]);
    let mut shipped: Vec<String> = std::fs::read_dir(app.join("Contents/Frameworks")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    shipped.sort();
    let mut want: Vec<String> = SHERPA_DYLIBS.iter().map(|d| d.0.to_string()).collect();
    want.sort();
    assert_eq!(shipped, want);
    for (name, _, _, pin) in SHERPA_DYLIBS {
        let p = app.join("Contents/Frameworks").join(name);
        assert_eq!(core_sha(&p), pin, "{name}");
    }
    for (_, file, pin, _) in SHERPA_LICENCES {
        let b = std::fs::read(app.join("Contents/Resources/licenses").join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(sha(&b), pin, "{file}");
    }
}

#[test]
#[ignore]
fn a_signed_app_bundle_signs_its_dylibs_with_its_own_team() {
    let app = app_bundle();
    let t = team(&app).expect("the app is not signed by a team");
    for (name, ..) in SHERPA_DYLIBS {
        let p = app.join("Contents/Frameworks").join(name);
        assert!(Command::new("/usr/bin/codesign").args(["--verify", "--strict"]).arg(&p).status().unwrap().success(), "{name}: codesign --verify --strict");
        assert_eq!(team(&p).as_deref(), Some(t.as_str()), "{name} is not signed by the app's team");
    }
}
