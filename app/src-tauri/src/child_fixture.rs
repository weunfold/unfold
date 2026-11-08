
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub(crate) struct Paths {
    pub dsh_pids: PathBuf,
    pub dsh_fence: PathBuf,
    pub omni_pids: PathBuf,
}

fn fresh_root(configured: Option<PathBuf>) -> Result<PathBuf, String> {
    let root = configured.unwrap_or_else(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rth-child-fixtures-{}-{now}", std::process::id()))
    });
    if !root.is_absolute() {
        return Err("fixture root must be absolute".into());
    }

    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|_| "fixture root must be a new directory under an existing parent".to_string())?;
    Ok(root)
}

fn receipt(root: &Path, configured: Option<PathBuf>, leaf: &str) -> Result<PathBuf, String> {
    let file = configured.unwrap_or_else(|| root.join(leaf));
    if file.parent() != Some(root) || file.file_name().is_none_or(|n| n == "." || n == "..") {
        return Err("fixture receipt must be directly inside its fresh invocation root".into());
    }
    if std::fs::symlink_metadata(&file).is_ok() {
        return Err("fixture receipt must not already exist".into());
    }
    Ok(file)
}

pub(crate) fn paths() -> &'static Paths {
    static PATHS: OnceLock<Paths> = OnceLock::new();
    PATHS.get_or_init(|| {
        let env = |key| {
            std::env::var_os(key)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        };
        let root = fresh_root(env("UNFOLD_CHILD_FIXTURE_ROOT")).expect("isolated fixture root");
        let p = Paths {
            dsh_pids: receipt(&root, env("UNFOLD_DSH_CHILD_PIDS"), "dsh-pids.txt")
                .expect("isolated DSH ledger"),
            dsh_fence: receipt(&root, env("UNFOLD_DSH_CHILD_FENCE"), "dsh-fence.txt")
                .expect("isolated DSH fence"),
            omni_pids: receipt(&root, env("UNFOLD_OMNI_CHILD_PIDS"), "omni-pids.txt")
                .expect("isolated Omni ledger"),
        };
        assert!(
            p.dsh_pids != p.dsh_fence && p.dsh_pids != p.omni_pids && p.dsh_fence != p.omni_pids,
            "fixture receipts must be distinct"
        );
        eprintln!("fixture receipts: {}", root.display());
        p
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_receipt_outside_its_invocation_root_is_refused() {
        let root = fresh_root(None).unwrap();
        let other = fresh_root(None).unwrap();
        let outside = other.join("not-created.txt");
        assert!(!outside.try_exists().unwrap(), "existing files must not mask the root guard");
        assert_eq!(
            receipt(&root, Some(outside.clone()), "unused"),
            Err("fixture receipt must be directly inside its fresh invocation root".into()),
            "a nonexistent receipt in another directory must be refused by the root guard"
        );
        assert!(!outside.try_exists().unwrap(), "rejection must not create the outside receipt");
        let inside = root.join("not-created.txt");
        assert_eq!(receipt(&root, Some(inside.clone()), "unused"), Ok(inside));
    }

    #[test]
    fn roots_are_fresh_and_existing_ledgers_are_untouched() {
        let a = fresh_root(None).unwrap();
        let b = fresh_root(None).unwrap();
        assert_ne!(a, b);
        let sentinel = a.join("children.json");
        std::fs::write(&sentinel, "planted-app-ledger").unwrap();
        assert!(fresh_root(Some(a.clone())).is_err());
        assert!(receipt(&b, Some(sentinel.clone()), "unused").is_err());
        assert!(receipt(&a, Some(sentinel.clone()), "unused").is_err());
        let link = b.join("link");
        std::os::unix::fs::symlink(&sentinel, &link).unwrap();
        assert!(receipt(&b, Some(link), "unused").is_err());
        assert_eq!(
            std::fs::read_to_string(sentinel).unwrap(),
            "planted-app-ledger"
        );
    }
}
