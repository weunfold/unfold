

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

pub struct Item<'a> {
    pub service: &'a str,
    pub account: &'a str,
}

fn root() -> PathBuf {
    #[cfg(not(test))]
    let home = crate::home();
    #[cfg(test)]
    let home = std::env::temp_dir().join(format!("unfold-scratch-keys-{}", std::process::id()));
    home.join(".unfold-rehearsal/keys")
}

impl Item<'_> {
    fn path(&self) -> Result<PathBuf, String> {
        let ok = |s: &str| !s.is_empty() && !s.starts_with('.') && !s.contains('/');
        match ok(self.service) && ok(self.account) {
            true => Ok(root().join(self.service).join(self.account)),
            false => Err(format!("no scratch key for {:?}/{:?}", self.service, self.account)),
        }
    }

    pub fn read(&self) -> Result<Option<String>, String> {
        let p = self.path()?;
        match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&p) {
            Ok(mut f) => {
                let mut key = String::new();
                std::io::Read::read_to_string(&mut f, &mut key).map_err(|e| format!("{}: {e}", p.display()))?;
                Ok(Some(key))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", p.display())),
        }
    }

    pub fn present(&self) -> Result<bool, String> {
        Ok(std::fs::symlink_metadata(self.path()?).is_ok_and(|m| m.is_file()))
    }

    pub fn write(&self, key: &str) -> Result<(), String> {
        let p = self.path()?;
        let dir = p.parent().expect("a key has a folder");
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let tmp = dir.join(format!(".tmp-{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600)).and_then(|()| f.write_all(key.as_bytes())).and_then(|()| f.sync_all()).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &p).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn clear(&self) -> Result<bool, String> {
        match std::fs::remove_file(self.path()?) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rehearsal_build_keeps_every_key_in_a_scratch_file_and_never_reaches_the_keychain() {
        let fence = crate::KeychainFence::up();
        let data = root().join("../../data");

        let openai = crate::setup::openai_key();
        let status = crate::setup::OpenAiKey::key_status(&openai, &data)["key_present"].clone();
        let removed = crate::setup::OpenAiKey::remove_key(&openai);
        let cloud = crate::flash::key_present(crate::flash::KEY_SERVICE);
        #[cfg(feature = "voice_pane")]
        let voice = {
            let keys = crate::settings::VOICE_KEYS;
            let before = keys.present();
            let wrote = keys.write("lf-key-fixture-not-a-secret-0001");
            let read = keys.read();
            let cleared = keys.clear();
            (before, wrote, read, cleared, keys.present())
        };
        assert_eq!(crate::KeychainFence::calls(), 0, "a call reached the Keychain");
        drop(fence);
        assert_eq!((status, removed, cloud), (serde_json::json!(false), Ok(false), Some(false)));
        #[cfg(feature = "voice_pane")]
        assert_eq!(voice, (false, Ok(()), Ok(Some("lf-key-fixture-not-a-secret-0001".to_string())), Ok(()), false));

        let item = Item { service: crate::flash::KEY_SERVICE, account: "fixture" };
        item.write("jk-fixture-not-a-secret-0002").unwrap();
        let p = item.path().unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&p), mode(p.parent().unwrap())), (0o600, 0o700));
        assert_eq!(item.read(), Ok(Some("jk-fixture-not-a-secret-0002".to_string())));
        assert_eq!(crate::flash::key_present(crate::flash::KEY_SERVICE), Some(false), "another account's file");
        assert_eq!(item.clear(), Ok(true));
        assert!(Item { service: "../x", account: "a" }.write("k").is_err(), "a name can't leave the folder");
        let _ = std::fs::remove_dir_all(root().join("../.."));
    }
}
