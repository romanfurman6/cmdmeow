//! Private history at rest (Clipboard, Dictation): each file is sealed with
//! XChaCha20-Poly1305 under one key that lives in the Keychain, and written
//! readable by its owner only. Another process can read the files, not what
//! is in them.
//!
//! The key is made on first use. If the Keychain can't hand it over (locked,
//! refused), [`shared`] is None: histories stay in memory for that run and
//! nothing on disk is touched, so a passing problem never wipes them.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// File header: format and version.
const MAGIC: &[u8; 4] = b"CMV1";
const NONCE: usize = 24;

pub struct Vault {
    cipher: XChaCha20Poly1305,
}

impl Vault {
    pub fn from_key(key: &[u8; 32]) -> Vault {
        Vault { cipher: XChaCha20Poly1305::new(key.into()) }
    }

    /// A throwaway key: sample data and tests, never the Keychain.
    pub fn random() -> Vault {
        Vault::from_key(&random_key())
    }

    /// `plain` sealed: header, a fresh random nonce, then the ciphertext.
    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE];
        fill(&mut nonce);
        let body = self.cipher.encrypt(XNonce::from_slice(&nonce), plain).expect("in-memory encryption");
        let mut out = Vec::with_capacity(MAGIC.len() + NONCE + body.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&body);
        out
    }

    /// The plain bytes, or None when `data` isn't ours or was changed.
    pub fn open(&self, data: &[u8]) -> Option<Vec<u8>> {
        if !is_sealed(data) || data.len() < MAGIC.len() + NONCE {
            return None;
        }
        let (nonce, body) = data[MAGIC.len()..].split_at(NONCE);
        self.cipher.decrypt(XNonce::from_slice(nonce), body).ok()
    }

    /// Seal `plain` into `path` (owner-only, written atomically).
    pub fn write(&self, path: &Path, plain: &[u8]) -> bool {
        crate::kit::fs::write_private(path, &self.seal(plain))
    }

    pub fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.open(&std::fs::read(path).ok()?)
    }
}

/// Whether `data` starts like one of our sealed files.
pub fn is_sealed(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

fn fill(b: &mut [u8]) {
    if getrandom::fill(b).is_err() {
        // no system randomness is not something to paper over
        panic!("no system randomness for the history key");
    }
}

fn random_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    fill(&mut k);
    k
}

/// The app's vault, its key read from (or first saved to) the Keychain
/// once per run. None when the Keychain gave no key.
pub fn shared() -> Option<Arc<Vault>> {
    static V: OnceLock<Option<Arc<Vault>>> = OnceLock::new();
    V.get_or_init(|| match keychain::key() {
        Ok(k) => Some(Arc::new(Vault::from_key(&k))),
        Err(e) => {
            eprintln!("[vault] no key, histories stay in memory this run: {e}");
            None
        }
    })
    .clone()
}

/// A JSON history sealed at `sealed`. Without a vault: the default (and
/// nothing is read). A plain file from before encryption at `plain` is
/// sealed in its place and removed.
pub fn load_json<T: serde::de::DeserializeOwned + serde::Serialize + Default>(v: Option<&Vault>, sealed: &Path, plain: &Path) -> T {
    let Some(v) = v else { return T::default() };
    if let Some(t) = v.read(sealed).and_then(|b| serde_json::from_slice(&b).ok()) {
        let _ = std::fs::remove_file(plain);
        return t;
    }
    let Some(t) = std::fs::read(plain).ok().and_then(|b| serde_json::from_slice::<T>(&b).ok()) else { return T::default() };
    if serde_json::to_vec(&t).is_ok_and(|b| v.write(sealed, &b)) {
        let _ = std::fs::remove_file(plain);
    }
    t
}

/// Seal `t` as JSON into `path` on the background writer (no vault: nothing).
pub fn save_json_later<T: serde::Serialize + ?Sized>(v: Option<&Vault>, path: &Path, t: &T) {
    if let (Some(v), Ok(b)) = (v, serde_json::to_vec(t)) {
        crate::kit::fs::save_private_later(path, v.seal(&b));
    }
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords::{get_generic_password, set_generic_password};

    fn service() -> &'static str {
        if crate::kit::fs::is_dev() { "Cmd+Meow Dev history" } else { "Cmd+Meow history" }
    }
    const ACCOUNT: &str = "key";
    /// errSecItemNotFound: no key yet, so make one.
    const NOT_FOUND: i32 = -25300;

    pub fn key() -> Result<[u8; 32], String> {
        match get_generic_password(service(), ACCOUNT) {
            Ok(b) => b.try_into().map_err(|_| "the clipboard key in the Keychain is damaged".to_string()),
            Err(e) if e.code() == NOT_FOUND => {
                let k = super::random_key();
                set_generic_password(service(), ACCOUNT, &k).map_err(|e| e.to_string())?;
                Ok(k)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod keychain {
    pub fn key() -> Result<[u8; 32], String> {
        Err("no Keychain here".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let v = Vault::random();
        let s = v.seal(b"hunter2 and friends");
        assert!(is_sealed(&s));
        assert!(!s.windows(7).any(|w| w == b"hunter2"));
        assert_eq!(v.open(&s).as_deref(), Some(&b"hunter2 and friends"[..]));
        // a fresh nonce every time
        assert_ne!(s, v.seal(b"hunter2 and friends"));
    }

    #[test]
    fn refuses_other_keys_changes_and_plain_files() {
        let v = Vault::random();
        let mut s = v.seal(b"secret");
        assert!(Vault::random().open(&s).is_none());
        let last = s.len() - 1;
        s[last] ^= 1;
        assert!(v.open(&s).is_none());
        assert!(v.open(b"[{\"id\":1}]").is_none());
        assert!(v.open(b"CMV1").is_none());
    }

    #[test]
    fn plain_history_is_sealed_in_place() {
        let dir = std::env::temp_dir().join(format!("cmb-vault-mig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (sealed, plain) = (dir.join("h.bin"), dir.join("h.json"));
        std::fs::write(&plain, b"[\"said this\"]").unwrap();
        // no key: nothing read, nothing touched
        assert!(load_json::<Vec<String>>(None, &sealed, &plain).is_empty());
        assert!(plain.exists() && !sealed.exists());
        let v = Vault::random();
        assert_eq!(load_json::<Vec<String>>(Some(&v), &sealed, &plain), vec!["said this".to_string()]);
        assert!(!plain.exists());
        assert!(!std::fs::read(&sealed).unwrap().windows(4).any(|w| w == b"said"));
        assert_eq!(load_json::<Vec<String>>(Some(&v), &sealed, &plain), vec!["said this".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("cmb-vault-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let v = Vault::random();
        let p = dir.join("a.bin");
        assert!(v.write(&p, b"x"));
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(v.read(&p).as_deref(), Some(&b"x"[..]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
