use rth_activation::lease::{Lease, Mode};
use std::process::{Command, Stdio};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let lease = Lease::acquire(std::path::Path::new(&a[1]), Mode::Shared).unwrap_or_else(|e| {
        println!("{}", serde_json::json!({"error": e.to_string()}));
        std::process::exit(2)
    });
    let mut agent = Command::new("/bin/sleep");
    agent.arg(&a[2]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    lease.inherit_into(&mut agent);
    let child = agent.spawn().unwrap();
    let pid = child.id() as i32;
    println!("{}", serde_json::json!({"agent_pid": pid, "agent_start_us": rth_activation::effect::start_us(pid)}));

    std::process::exit(0)
}
