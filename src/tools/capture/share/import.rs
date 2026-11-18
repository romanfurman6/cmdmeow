//! The storage file Claude saves after setting up Cloudflare R2
//! (`cmdmeow-sharing.cmdmeow` in Downloads): opening it (double-click, or a
//! drop on the Sharing page) fills the Sharing page, puts the secret key in
//! the Keychain and moves the file to the Trash. Only on the person's own
//! action, and after a macOS alert that names the bucket: a downloaded file
//! must not quietly point uploads at someone else's bucket.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The file's extension (registered in Info.plist).
pub const EXT: &str = "cmdmeow";
/// The name the setup prompt asks for.
pub const FILE_NAME: &str = "cmdmeow-sharing.cmdmeow";
/// Bigger than this is not ours.
const MAX_BYTES: u64 = 16 * 1024;

/// What the file holds, checked.
#[derive(Debug, PartialEq)]
pub struct Storage {
    pub endpoint: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    pub public_url: String,
}

#[derive(Deserialize)]
struct Raw {
    cmdmeow: String,
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    #[serde(default)]
    public_url: String,
}

/// Read a storage file's text. None: not a Cmd+Meow sharing file.
pub fn parse(text: &str) -> Option<Storage> {
    let r: Raw = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    if r.cmdmeow.trim() != "sharing" {
        return None;
    }
    // the endpoint field takes a host: drop a scheme or a trailing slash
    let host = r.endpoint.trim();
    let host = host.strip_prefix("https://").or_else(|| host.strip_prefix("http://")).unwrap_or(host).trim_end_matches('/');
    let s = Storage {
        endpoint: host.to_string(),
        bucket: r.bucket.trim().to_string(),
        access_key: r.access_key.trim().to_string(),
        secret_key: r.secret_key.trim().to_string(),
        public_url: r.public_url.trim().trim_end_matches('/').to_string(),
    };
    let ok = !s.endpoint.is_empty()
        && !s.endpoint.contains(['/', ' '])
        && !s.bucket.is_empty()
        && !s.access_key.is_empty()
        && !s.secret_key.is_empty()
        && (s.public_url.is_empty() || s.public_url.starts_with("https://"));
    ok.then_some(s)
}

/// Read a file from disk (small files only).
pub fn read(path: &Path) -> Option<Storage> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES {
        return None;
    }
    parse(&std::fs::read_to_string(path).ok()?)
}

/// The endpoint short enough for an alert line (it can't wrap mid-word):
/// R2's 32-character account id keeps its ends, other hosts their ends.
pub fn short_host(host: &str) -> String {
    const R2: &str = ".r2.cloudflarestorage.com";
    let cut = |s: &str, n: usize| -> String {
        let c: Vec<char> = s.chars().collect();
        if c.len() <= n * 2 + 1 {
            return s.to_string();
        }
        format!("{}…{}", c[..n].iter().collect::<String>(), c[c.len() - n..].iter().collect::<String>())
    };
    match host.strip_suffix(R2) {
        Some(account) => format!("Cloudflare R2, account {}", cut(account, 4)),
        None => cut(host, 14),
    }
}

/// A storage file by its name.
pub fn is_storage_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXT))
}

/// Files Finder asked the app to open, waiting for Capture's tick.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Finder opened `path` with the app: keep it for Capture and show the
/// Sharing page (which also wakes the app).
pub fn opened(path: PathBuf) {
    if !is_storage_file(&path) {
        return;
    }
    OPENED.lock().unwrap_or_else(|e| e.into_inner()).push(path);
    // in-app link: the scheme part is not looked at
    crate::kit::url_scheme::open("cmdmeow://open/sharing");
}

/// Files opened since the last call.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"cmdmeow": "sharing", "endpoint": "https://abc123.r2.cloudflarestorage.com/", "bucket": "cmdmeow-shares", "access_key": "AK", "secret_key": "SK", "public_url": "https://pub-1.r2.dev/"}"#;

    #[test]
    fn reads_the_file_claude_saves() {
        let s = parse(GOOD).unwrap();
        assert_eq!(s.endpoint, "abc123.r2.cloudflarestorage.com");
        assert_eq!(s.bucket, "cmdmeow-shares");
        assert_eq!((s.access_key.as_str(), s.secret_key.as_str()), ("AK", "SK"));
        assert_eq!(s.public_url, "https://pub-1.r2.dev");
    }

    #[test]
    fn refuses_anything_else() {
        assert!(parse("").is_none());
        assert!(parse("not json").is_none());
        assert!(parse(&GOOD.replace("\"sharing\"", "\"other\"")).is_none());
        assert!(parse(&GOOD.replace("\"SK\"", "\"\"")).is_none());
        assert!(parse(&GOOD.replace("https://pub-1", "http://pub-1")).is_none());
        assert!(parse(&GOOD.replace("abc123.r2", "abc 123.r2")).is_none());
        // the public URL may be missing (a private bucket)
        assert!(parse(r#"{"cmdmeow":"sharing","endpoint":"h","bucket":"b","access_key":"a","secret_key":"s"}"#).is_some());
    }

    #[test]
    fn alert_hosts_fit_one_line() {
        assert_eq!(short_host("0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com"), "Cloudflare R2, account 0123…cdef");
        assert_eq!(short_host("fsn1.your-objectstorage.com"), "fsn1.your-objectstorage.com");
        assert_eq!(short_host("s3.eu-central-1.amazonaws.com.example.internal"), "s3.eu-central-…ample.internal");
    }

    #[test]
    fn knows_its_files_by_extension() {
        assert!(is_storage_file(Path::new("/x/cmdmeow-sharing.cmdmeow")));
        assert!(is_storage_file(Path::new("/x/A.CMDMEOW")));
        assert!(!is_storage_file(Path::new("/x/cmdmeow-sharing.json")));
    }
}
