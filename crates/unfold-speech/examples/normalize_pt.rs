
use std::io::BufRead;

fn main() {
    for line in std::io::stdin().lock().lines() {
        println!("{}", unfold_speech::normalize_pt(&line.expect("stdin")));
    }
}
