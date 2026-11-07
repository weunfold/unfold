
use ort::session::Session;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

pub fn runtime_path(executable: &Path) -> Result<PathBuf, String> {
    if !executable.is_absolute() {
        return Err("ONNX executable path must be absolute".into());
    }
    let parent = executable.parent().ok_or("ONNX executable has no parent")?;
    let path = parent.join("../Frameworks/libonnxruntime.dylib");
    path.canonicalize()
        .map_err(|e| format!("Bundled ONNX runtime unavailable: {e}"))
}
pub fn init() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let path = runtime_path(&std::env::current_exe().map_err(|e| e.to_string())?)?;
        ort::init_from(path)
            .map_err(|e| e.to_string())?
            .with_name("unfold-cpu")
            .commit();
        Ok(())
    })
    .clone()
}
pub fn session(model: &[u8]) -> Result<Session, String> {
    init()?;

    Session::builder()
        .map_err(|e| e.to_string())?
        .with_intra_threads(1)
        .map_err(|e| e.to_string())?
        .with_inter_threads(1)
        .map_err(|e| e.to_string())?
        .commit_from_memory(model)
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    #[test]
    fn refuses_relative_runtime_resolution() {
        assert!(super::runtime_path(std::path::Path::new("rth-app")).is_err());
    }
}
