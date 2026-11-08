

use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use security_framework::os::macos::keychain_item::SecKeychainItem;

pub const GET_MAY_PROMPT: bool = true;

pub const SERVICE: &str = "ai.namastex.rth.desktop";
pub const ACCOUNT: &str = "openai-api-key";

const ERR_ITEM_NOT_FOUND: i32 = -25300;
const ERR_INTERACTION_NOT_ALLOWED: i32 = -25308;
const ERR_AUTH_FAILED: i32 = -25293;
const ERR_USER_CANCELED: i32 = -128;

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {

    Format(&'static str),

    Denied(String),
    Other(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyError::Format(why) => write!(f, "that does not look like an OpenAI API key: {why}"),
            KeyError::Denied(why) => write!(f, "the Keychain did not allow access to the RTH key ({why}); unlock it or allow RTH, then retry"),
            KeyError::Other(why) => write!(f, "Keychain: {why}"),
        }
    }
}

fn map(e: security_framework::base::Error) -> KeyError {
    match e.code() {
        ERR_INTERACTION_NOT_ALLOWED | ERR_AUTH_FAILED | ERR_USER_CANCELED => KeyError::Denied(e.to_string()),
        _ => KeyError::Other(e.to_string()),
    }
}

pub fn check_format(key: &str) -> Result<(), KeyError> {
    if key.is_empty() {
        return Err(KeyError::Format("it is empty"));
    }
    if key.len() < 20 || key.len() > 512 {
        return Err(KeyError::Format("its length is outside 20-512 characters"));
    }
    if !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(KeyError::Format("it contains spaces, line breaks or non-ASCII characters"));
    }
    Ok(())
}

pub struct KeyStore {

    keychain: Option<SecKeychain>,
}

impl KeyStore {
    pub fn user_default() -> KeyStore {
        KeyStore { keychain: None }
    }

    pub fn file(path: &std::path::Path, password: &str) -> Result<KeyStore, KeyError> {
        let mut kc = if path.exists() {
            SecKeychain::open(path).map_err(map)?
        } else {
            CreateOptions::new().password(password).prompt_user(false).create(path).map_err(map)?
        };
        kc.unlock(Some(password)).map_err(map)?;
        Ok(KeyStore { keychain: Some(kc) })
    }

    fn refs(&self) -> Result<Vec<SecKeychainItem>, KeyError> {
        let mut q = ItemSearchOptions::new();
        q.class(ItemClass::generic_password()).service(SERVICE).account(ACCOUNT).load_refs(true).limit(Limit::All);
        if let Some(kc) = &self.keychain {
            q.keychains(std::slice::from_ref(kc));
        }
        match q.search() {
            Ok(found) => Ok(found.into_iter().filter_map(|r| match r {
                SearchResult::Ref(Reference::KeychainItem(i)) => Some(i),
                _ => None,
            }).collect()),
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => Ok(Vec::new()),
            Err(e) => Err(map(e)),
        }
    }

    fn find(&self) -> Result<Option<(security_framework::os::macos::passwords::SecKeychainItemPassword, security_framework::os::macos::keychain_item::SecKeychainItem)>, KeyError> {
        let r = match &self.keychain {
            Some(kc) => kc.find_generic_password(SERVICE, ACCOUNT),
            None => security_framework::os::macos::passwords::find_generic_password(None, SERVICE, ACCOUNT),
        };
        match r {
            Ok(found) => Ok(Some(found)),
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(map(e)),
        }
    }

    pub fn save(&self, key: &str) -> Result<(), KeyError> {
        check_format(key)?;
        for item in self.refs()? {
            item.delete();
        }
        if !self.refs()?.is_empty() {
            return Err(KeyError::Other("the existing RTH key could not be replaced (delete it in Keychain Access, then save again)".into()));
        }
        match &self.keychain {
            Some(kc) => kc.add_generic_password(SERVICE, ACCOUNT, key.as_bytes()).map_err(map),
            None => SecKeychain::default().map_err(map)?.add_generic_password(SERVICE, ACCOUNT, key.as_bytes()).map_err(map),
        }
    }

    pub fn get(&self) -> Result<Option<String>, KeyError> {
        match self.find()? {
            Some((pw, _)) => String::from_utf8(pw.to_vec()).map(Some).map_err(|_| KeyError::Other("the stored key is not UTF-8".into())),
            None => Ok(None),
        }
    }

    pub fn present(&self) -> Result<bool, KeyError> {
        Ok(!self.refs()?.is_empty())
    }

    pub fn remove(&self) -> Result<bool, KeyError> {
        let items = self.refs()?;
        if items.is_empty() {
            return Ok(false);
        }
        for item in items {
            item.delete();
        }
        Ok(self.refs()?.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_list() -> String {
        String::from_utf8(std::process::Command::new("/usr/bin/security").args(["list-keychains", "-d", "user"]).output().unwrap().stdout).unwrap()
    }

    #[test]
    fn the_format_check_is_local_and_strict() {
        assert!(check_format("sk-proj-0123456789abcdefABCDEF").is_ok());
        for bad in ["", "short", "sk-proj-0123456789 abcdef", "sk-proj-0123456789abcdef\n", "sk-prój-0123456789abcdefgh"] {
            assert!(matches!(check_format(bad), Err(KeyError::Format(_))), "{bad:?}");
        }
    }

    #[test]
    fn save_replace_get_and_remove_on_a_scratch_keychain_file_only() {
        let before = search_list();
        let dir = std::env::temp_dir().join(format!("rth-keychain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rth-test.keychain-db");
        let ks = KeyStore::file(&path, "rth-test-keychain-not-a-secret").unwrap();
        assert_eq!(ks.get().unwrap(), None);
        assert!(!ks.present().unwrap());
        assert!(matches!(ks.save("not a key"), Err(KeyError::Format(_))));
        assert!(!ks.present().unwrap(), "a failed format check stores nothing");
        ks.save("sk-test-fictional-0000000000000001").unwrap();
        assert_eq!(ks.get().unwrap().as_deref(), Some("sk-test-fictional-0000000000000001"));
        ks.save("sk-test-fictional-0000000000000002").unwrap();
        assert_eq!(ks.get().unwrap().as_deref(), Some("sk-test-fictional-0000000000000002"), "Replace updates the one item");

        assert_eq!(ks.refs().unwrap().len(), 1);
        assert!(ks.present().unwrap());
        assert!(ks.remove().unwrap());
        assert_eq!(ks.get().unwrap(), None);
        assert!(!ks.present().unwrap());
        assert!(!ks.remove().unwrap(), "removing again finds nothing");

        let kc = ks.keychain.as_ref().unwrap();
        kc.add_generic_password(SERVICE, ACCOUNT, b"sk-test-fictional-old-a-000000000").unwrap();
        assert!(kc.add_generic_password(SERVICE, ACCOUNT, b"sk-test-fictional-old-b-000000000").is_err(), "the keychain itself keeps one item per service/account");
        ks.save("sk-test-fictional-0000000000000003").unwrap();
        assert_eq!(ks.refs().unwrap().len(), 1);
        assert_eq!(ks.get().unwrap().as_deref(), Some("sk-test-fictional-0000000000000003"));
        assert!(ks.remove().unwrap());
        assert_eq!(search_list(), before, "the user's keychain search list is unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
