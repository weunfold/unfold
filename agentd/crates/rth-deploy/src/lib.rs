

pub mod controller;
pub mod durable;
pub mod manifest;
pub mod migrate;
pub mod record;
pub mod recreate;
pub mod selfrepo;
pub mod source;
pub mod store;

pub fn data_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("RTH_DATA_DIR") {
        return d.into();
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_else(|| ".".into());
    home.join(".local/share/rth")
}
