

pub mod audio;
pub mod contained;
pub mod fixture;
pub mod registry;
pub mod results;
pub mod run;
pub mod runner;
pub mod scorer;
pub mod stats;
pub mod timeline;
pub mod util;
pub mod vm;

use std::path::PathBuf;

pub fn default_bench_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench")
}

pub fn default_agent_dir() -> PathBuf {
    match std::env::var_os("RTH_AGENTD_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/aarch64-unknown-linux-musl/release"),
    }
}
