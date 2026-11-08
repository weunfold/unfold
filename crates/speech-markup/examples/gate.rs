
use speech_markup::say_gate::{content, Content};
use std::io::Read;

fn main() {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).unwrap();
    for r in s.split('\u{1e}') {
        println!("{}", match content(r) { Content::Short => "full", Content::Long => "first", Content::Visual(_) => "summary" });
    }
}
