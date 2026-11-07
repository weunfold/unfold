

use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

pub const SUN_PATH_MAX: usize = 103;

pub fn dir_for(uid: u32, instance_id: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let hex: String = Sha256::digest(instance_id.as_bytes())[..4].iter().map(|b| format!("{b:02x}")).collect();
    PathBuf::from(format!("/tmp/ufd-{uid}")).join(hex)
}

pub fn check(dir: &Path, uid: u32) -> Result<(), &'static str> {
    let m = std::fs::symlink_metadata(dir).map_err(|_| "the socket dir is missing")?;
    if m.file_type().is_symlink() {
        return Err("the socket dir is a symlink");
    }
    if !m.file_type().is_dir() {
        return Err("the socket dir is not a directory");
    }
    if m.uid() != uid {
        return Err("the socket dir is not owned by this user");
    }
    if m.mode() & 0o777 != 0o700 {
        return Err("the socket dir is not mode 0700");
    }
    Ok(())
}

pub fn ensure(instance_id: &str) -> Result<PathBuf, String> {
    let uid = unsafe { libc::geteuid() };
    let dir = dir_for(uid, instance_id);
    for d in [dir.parent().unwrap_or(Path::new("/tmp")), dir.as_path()] {
        match std::fs::DirBuilder::new().mode(0o700).create(d) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(format!("{}: {e}", d.display())),
            _ => {}
        }
        check(d, uid).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    Ok(dir)
}

pub fn socket_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let p = dir.join(name);
    let n = p.as_os_str().len();
    if n > SUN_PATH_MAX {
        return Err(format!("the socket path is {n} bytes, over the {SUN_PATH_MAX} macOS binds"));
    }
    Ok(p)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn uid() -> u32 {
        unsafe { libc::geteuid() }
    }

    pub(crate) fn test_id() -> String {
        static N: AtomicU32 = AtomicU32::new(0);
        format!("ufd-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst))
    }

    pub(crate) fn remove(dir: &Path) {
        if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            let _ = std::fs::remove_file(dir);
        } else {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(test_id());
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_dir_is_tmp_ufd_uid_and_eight_hex_of_the_ids_sha256() {

        assert_eq!(dir_for(501, "abc"), PathBuf::from("/tmp/ufd-501/ba7816bf"));
        let leaf = dir_for(uid(), &test_id());
        let name = leaf.file_name().unwrap().to_str().unwrap();
        assert!(name.len() == 8 && name.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{name}");
    }

    #[test]
    fn any_id_keeps_the_socket_path_within_103_bytes() {
        let id64 = "i".repeat(64);
        let p = socket_path(&dir_for(u32::MAX, &id64), "control.sock").unwrap();
        assert!(p.as_os_str().len() <= SUN_PATH_MAX, "{}", p.display());
        let p = socket_path(&dir_for(u32::MAX, &"x".repeat(4096)), ".s.PGSQL.65535").unwrap();
        assert!(p.as_os_str().len() <= SUN_PATH_MAX);
        let long = PathBuf::from(format!("/tmp/{}", "d".repeat(100)));
        let e = socket_path(&long, "control.sock").unwrap_err();
        assert!(e.contains("103"), "{e}");
    }

    #[test]
    fn ensure_makes_both_levels_0700_and_this_users() {
        let id = test_id();
        let dir = ensure(&id).unwrap();
        assert_eq!(dir, dir_for(uid(), &id));
        for d in [dir.parent().unwrap(), dir.as_path()] {
            let m = std::fs::symlink_metadata(d).unwrap();
            assert!(m.file_type().is_dir());
            assert_eq!(m.permissions().mode() & 0o777, 0o700, "{}", d.display());
        }
        assert_eq!(check(&dir, uid()), Ok(()));
        remove(&dir);
    }

    #[test]
    fn a_64_character_id_is_served() {
        let id = format!("{}{}", test_id(), "q".repeat(64))[..64].to_string();
        assert_eq!(id.chars().count(), 64);
        let dir = ensure(&id).unwrap();
        assert!(socket_path(&dir, "control.sock").is_ok());
        remove(&dir);
    }

    #[test]
    fn a_planted_symlink_is_refused_and_never_followed() {
        let id = test_id();
        let target = scratch();
        let dir = dir_for(uid(), &id);

        let _ = ensure(&format!("{id}-parent"));
        std::os::unix::fs::symlink(&target, &dir).unwrap();
        let e = ensure(&id).unwrap_err();
        assert!(e.contains("symlink"), "{e}");
        assert_eq!(check(&dir, uid()), Err("the socket dir is a symlink"));
        assert!(std::fs::read_dir(&target).unwrap().next().is_none(), "nothing is made through the link");
        remove(&dir);
        remove(&target);
        remove(&dir_for(uid(), &format!("{id}-parent")));
    }

    #[test]
    fn another_users_dir_a_0755_dir_and_a_file_are_refused() {
        let d = scratch();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(check(&d, uid()), Ok(()));
        assert_eq!(check(&d, uid() + 1), Err("the socket dir is not owned by this user"));
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(check(&d, uid()), Err("the socket dir is not mode 0700"));
        let f = d.join("file");
        std::fs::write(&f, b"").unwrap();
        assert_eq!(check(&f, uid()), Err("the socket dir is not a directory"));
        assert_eq!(check(&d.join("missing"), uid()), Err("the socket dir is missing"));
        remove(&d);
    }

    #[test]
    fn ensure_refuses_a_0755_dir_at_the_real_location_without_repairing_it() {
        let id = test_id();
        let dir = ensure(&id).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let e = ensure(&id).unwrap_err();
        assert!(e.contains("0700"), "{e}");
        assert_eq!(std::fs::symlink_metadata(&dir).unwrap().permissions().mode() & 0o777, 0o755);
        remove(&dir);
    }
}
