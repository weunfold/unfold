use rth_activation::effect;
use serde_json::json;
use std::path::Path;
use std::process::Stdio;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (exe, fence, record, point) = (&a[1], &a[2], &a[3], a[4].as_str());
    let command: Vec<String> = a[6..].to_vec();
    let rec = |v: serde_json::Value| rth_activation::record::write(Path::new(record), &v, 0o600).unwrap();
    let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
    let pending = effect::start(Path::new(exe), Path::new(fence), &command, &env, Path::new("/"), Stdio::null()).unwrap();
    let id = pending.identity;

    println!("{}", serde_json::to_string(&id).unwrap());
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    let pause = || loop { std::thread::sleep(std::time::Duration::from_secs(60)) };
    if point == "started" { pause() }
    rec(json!({"seq": 1, "intent": true, "helper": id, "release_intent": false}));
    if point == "intent" { pause() }
    rec(json!({"seq": 2, "intent": true, "helper": id, "release_intent": true}));
    let mut child = pending.go().unwrap();
    if point == "released" { pause() }
    let status = child.wait().unwrap();
    rec(json!({"seq": 3, "intent": true, "helper": id, "release_intent": true, "released": true, "exit": status.code()}));
    pause()
}
