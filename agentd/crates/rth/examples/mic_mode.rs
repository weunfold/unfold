

fn main() {
    #[cfg(target_os = "macos")]
    println!("{}", rth::vpio::mic_mode());
    #[cfg(not(target_os = "macos"))]
    println!("{{\"active\": \"unknown\", \"note\": \"not macOS\"}}");
}
