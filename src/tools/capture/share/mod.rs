//! Sharing a recording or a screenshot: upload it to the person's own
//! S3-compatible storage (Hetzner Object Storage and the like) and copy the
//! link. One storage, set up on the Sharing page; screenshots and videos
//! each have their folder (cmdmeow/screenshots/, cmdmeow/videos/) and their
//! own "upload each" and "copy the link" switches. The secret key is kept
//! in the Keychain.

/// The Keychain account of the storage's secret key.
pub const VIDEO_ACCOUNT: &str = "secret-key";
/// Folders new storage settings start with.
pub const VIDEO_FOLDER: &str = "cmdmeow/videos/";
pub const SHOT_FOLDER: &str = "cmdmeow/screenshots/";

pub mod s3;
pub mod setup;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

/// What a link does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkKind {
    /// Anyone with the link, for good (the file is made public-read; its
    /// name is random, so it can't be guessed).
    Public,
    /// The bucket stays private; the link stops working after 7 days.
    Week,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Upload every recording (or screenshot) as soon as it's saved.
    pub auto: bool,
    /// Put the link on the clipboard when the upload is done.
    pub copy_link: bool,
    /// "fsn1.your-objectstorage.com".
    pub endpoint: String,
    /// Empty: taken from the endpoint.
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    /// Where public files are read from when that isn't the bucket itself:
    /// R2's "https://pub-….r2.dev" or a custom domain. Empty: the bucket.
    pub public_url: String,
    /// Folder inside the bucket ("cmdmeow/videos/"); may be empty.
    pub folder: String,
    pub link: LinkKind,
    /// A secret key was saved to the Keychain from here: lets the UI say
    /// "set up" without touching the Keychain at all.
    #[serde(default)]
    pub secret_saved: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto: false, copy_link: true, endpoint: String::new(), region: String::new(), bucket: String::new(), access_key: String::new(), public_url: String::new(), folder: VIDEO_FOLDER.into(), link: LinkKind::Public, secret_saved: false }
    }
}

impl Settings {
    /// Fresh settings for screenshots.
    pub fn shots() -> Settings {
        Settings { folder: SHOT_FOLDER.into(), ..Settings::default() }
    }
    /// The bucket, with the secret kept under `account`.
    pub fn bucket(&self, account: &str) -> s3::Bucket {
        s3::Bucket::new(&self.endpoint, &self.region, &self.bucket, &self.access_key, &secret::get(account).unwrap_or_default()).public_at(&self.public_url)
    }
    /// The storage is Cloudflare R2 (by its endpoint).
    pub fn is_r2(&self) -> bool {
        s3::Bucket::new(&self.endpoint, "", &self.bucket, "", "").is_r2()
    }
    /// Public links can be made: R2 has no per-file ACL, so it needs the
    /// bucket's public URL.
    pub fn links_ready(&self) -> bool {
        self.link == LinkKind::Week || !self.is_r2() || !self.public_url.trim().is_empty()
    }
    /// Everything filled in (the secret included). Reads the secret, which
    /// can make macOS ask for the Keychain password: only when uploading.
    pub fn ready(&self, account: &str) -> bool {
        self.bucket(account).ready()
    }
    /// Filled in, with a secret saved, from our own settings only: no
    /// Keychain call, so it's safe anywhere (the sidebar, every launch).
    pub fn set_up(&self, _account: &str) -> bool {
        !self.endpoint.trim().is_empty() && !self.bucket.trim().is_empty() && !self.access_key.trim().is_empty() && self.secret_saved && self.links_ready()
    }
}

/// The secret key, in the Keychain (read once, then kept in memory).
pub mod secret {
    use std::sync::Mutex;

    use std::collections::HashMap;

    /// Per account: read once, then kept.
    static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

    /// Is a secret saved under `account`? Asks the Keychain for the item's
    /// attributes only, never its data, so macOS doesn't ask for a password.
    pub fn exists(account: &str) -> bool {
        if let Some(Some(v)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(account)) {
            return !v.is_empty();
        }
        static SEEN: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        *seen.get_or_insert_with(HashMap::new).entry(account.to_string()).or_insert_with(|| keychain::exists(account))
    }

    /// Read the key now, from the Keychain itself, on a thread: if macOS
    /// wants to ask, it asks while the person is on the Sharing page (not in
    /// the middle of an upload), and the answer is kept for this run.
    pub fn approve_now(account: &str) {
        let account = account.to_string();
        std::thread::spawn(move || {
            let v = keychain::get(&account);
            CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(account, v);
        });
    }

    pub fn get(account: &str) -> Option<String> {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        c.get_or_insert_with(HashMap::new).entry(account.to_string()).or_insert_with(|| keychain::get(account)).clone()
    }

    /// Save it (empty removes it).
    pub fn set(account: &str, v: &str) -> Result<(), String> {
        keychain::set(account, v)?;
        CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(account.to_string(), Some(v.to_string()).filter(|s| !s.is_empty()));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    mod keychain {
        fn service() -> &'static str {
            if crate::kit::fs::is_dev() { "Cmd+Meow Dev sharing" } else { "Cmd+Meow sharing" }
        }
        /// Where the key lived under the app's first name; moved over on first read.
        const OLD_SERVICE: &str = "CleanMeBro sharing";
        /// The account the first name used (videos only).
        const OLD_ACCOUNT: &str = "secret-key";

        fn read(service: &str, account: &str) -> Option<String> {
            security_framework::passwords::get_generic_password(service, account).ok().and_then(|b| String::from_utf8(b).ok()).filter(|s| !s.is_empty())
        }
        pub fn get(account: &str) -> Option<String> {
            read(service(), account).or_else(|| {
                if crate::kit::fs::is_dev() || account != OLD_ACCOUNT {
                    return None;
                }
                let v = read(OLD_SERVICE, OLD_ACCOUNT)?;
                if set(account, &v).is_ok() {
                    let _ = security_framework::passwords::delete_generic_password(OLD_SERVICE, OLD_ACCOUNT);
                }
                Some(v)
            })
        }
        pub fn exists(account: &str) -> bool {
            use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
            let found = |service: &str| {
                ItemSearchOptions::new().class(ItemClass::generic_password()).service(service).account(account).load_attributes(true).limit(Limit::Max(1)).search().is_ok_and(|r| !r.is_empty())
            };
            found(service()) || (!crate::kit::fs::is_dev() && account == OLD_ACCOUNT && found(OLD_SERVICE))
        }
        pub fn set(account: &str, v: &str) -> Result<(), String> {
            if v.is_empty() {
                let _ = security_framework::passwords::delete_generic_password(service(), account);
                return Ok(());
            }
            security_framework::passwords::set_generic_password(service(), account, v.as_bytes()).map_err(|e| e.to_string())
        }
    }

    /// Elsewhere (tests): only in memory.
    #[cfg(not(target_os = "macos"))]
    mod keychain {
        pub fn get(_account: &str) -> Option<String> {
            None
        }
        pub fn exists(_account: &str) -> bool {
            false
        }
        pub fn set(_account: &str, _v: &str) -> Result<(), String> {
            Ok(())
        }
    }
}

/// A file that was shared: where it went and its last link.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shared {
    pub key: String,
    pub link: String,
    /// When it was uploaded (unix seconds); 0 for uploads from before.
    #[serde(default)]
    pub at: i64,
}

/// Everything shared, newest first: the local file and where it went.
pub fn all() -> Vec<(PathBuf, Shared)> {
    let mut v: Vec<(PathBuf, Shared)> = load_store().into_iter().map(|(p, s)| (PathBuf::from(p), Shared { link: on_page(&s.link), ..s })).collect();
    v.sort_by(|a, b| b.1.at.cmp(&a.1.at).then_with(|| b.0.cmp(&a.0)));
    v
}

/// Forget `path`'s upload here (the file in the bucket stays).
pub fn forget(path: &Path) {
    let mut all = load_store();
    if all.remove(&path.display().to_string()).is_some() {
        crate::kit::fs::save_json(&store_path(), &all, true);
    }
}

fn store_path() -> PathBuf {
    crate::kit::fs::data_path("shared.json")
}

fn load_store() -> HashMap<String, Shared> {
    crate::kit::fs::load_json(&store_path())
}

/// The last link of `path`, if it was shared.
pub fn link_for(path: &Path) -> Option<String> {
    load_store().get(&path.display().to_string()).map(|s| on_page(&s.link))
}

fn remember(path: &Path, s: Shared) {
    let mut all = load_store();
    all.insert(path.display().to_string(), s);
    crate::kit::fs::save_json(&store_path(), &all, true);
}

/// A picture (a screenshot), not a video: picks which sharing settings apply.
pub fn is_image(path: &Path) -> bool {
    content_type(path).starts_with("image/")
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        _ => "application/octet-stream",
    }
}

/// A new random object name in `folder`, keeping the file's extension.
fn new_key(folder: &str, path: &Path) -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        b = t.to_le_bytes();
    }
    let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default();
    let folder = folder.trim().trim_start_matches('/');
    let folder = if folder.is_empty() || folder.ends_with('/') { folder.to_string() } else { format!("{folder}/") };
    format!("{folder}{id}{ext}")
}

/// The share page on the website: shows the file and lets people edit a copy.
pub const PAGE: &str = "https://cmdmeow.com";
/// Where the site lived before: links made then are moved to `PAGE`.
const OLD_PAGES: [&str; 1] = ["https://cmdmeow.furman.cat"];
/// Sites allowed to read the bucket (CORS), so Edit works on all of them.
pub const ORIGINS: [&str; 3] = [PAGE, "https://www.cmdmeow.com", "https://cmdmeow.furman.cat"];

/// `link` on today's share page (links saved before the move point at the old one).
fn on_page(link: &str) -> String {
    for old in OLD_PAGES {
        if let Some(rest) = link.strip_prefix(old) {
            return format!("{PAGE}{rest}");
        }
    }
    link.to_string()
}

/// The link people get: the share page for the file at `file_url`.
pub fn page_link(file_url: &str, title: &str) -> String {
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|c| match c {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (c as char).to_string(),
                _ => format!("%{c:02X}"),
            })
            .collect()
    };
    let mut link = format!("{PAGE}/view/?u={}", enc(file_url));
    if !title.is_empty() {
        link.push_str(&format!("&t={}", enc(title)));
    }
    link
}

/// The title shown on the share page: the file's name without extension.
fn title_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `allow_page` off the UI thread: for links copied again, so buckets set
/// up before a site move get the new origins too.
pub fn allow_page_later(b: s3::Bucket) {
    let _ = std::thread::Builder::new().name("share-cors".into()).spawn(move || allow_page(&b));
}

/// Once per bucket and run: let the share page read the bucket (CORS), so
/// Edit works there. A failure only costs editing, so it is just logged.
fn allow_page(b: &s3::Bucket) {
    // Buckets checked (or being checked) this run; one check at a time each.
    static DONE: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let id = b.bucket_host();
    match DONE.lock() {
        Ok(mut d) if !d.contains(&id) => d.push(id.clone()),
        _ => return,
    }
    match s3::ensure_cors(b, &ORIGINS) {
        Ok(changed) => eprintln!("[share] CORS for {} on {id}: {}", ORIGINS.join(", "), if changed { "added" } else { "already there" }),
        Err(e) => {
            eprintln!("[share] {e}");
            // Try again next time.
            if let Ok(mut d) = DONE.lock() {
                d.retain(|x| x != &id);
            }
        }
    }
}

struct Job {
    path: PathBuf,
    progress: Arc<s3::Progress>,
    rx: Receiver<Result<String, String>>,
}

/// Uploads under way.
#[derive(Default)]
pub struct Uploads {
    jobs: Vec<Job>,
}

impl Uploads {
    /// Upload `path` (again: the same name, so a link to it keeps working).
    pub fn start(&mut self, s: &Settings, account: &str, path: PathBuf, ctx: &egui::Context) -> Result<(), String> {
        let b = s.bucket(account);
        if !b.ready() {
            return Err(if is_image(&path) { "Set up sharing in Screenshot settings first".into() } else { "Set up sharing in Video settings first".into() });
        }
        if s.link == LinkKind::Public && !b.public_ready() {
            return Err("Add the bucket's Public URL on the Sharing page".into());
        }
        self.stop(&path);
        let key = load_store().get(&path.display().to_string()).map(|x| x.key.clone()).unwrap_or_else(|| new_key(&s.folder, &path));
        let progress = Arc::new(s3::Progress::default());
        let (tx, rx) = channel();
        let (p2, ctx2, path2, link) = (progress.clone(), ctx.clone(), path.clone(), s.link);
        std::thread::Builder::new()
            .name("share-upload".into())
            .spawn(move || {
                let r = s3::put_file(&b, &key, &path2, content_type(&path2), link == LinkKind::Public, &p2).map(|()| {
                    allow_page(&b);
                    let file = match link {
                        LinkKind::Public => b.url(&key),
                        LinkKind::Week => s3::presign_get(&b, &key, 7 * 24 * 3600, chrono::Utc::now()),
                    };
                    let url = page_link(&file, &title_of(&path2));
                    remember(&path2, Shared { key, link: url.clone(), at: chrono::Utc::now().timestamp() });
                    url
                });
                let _ = tx.send(r);
                ctx2.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        self.jobs.push(Job { path, progress, rx });
        Ok(())
    }

    /// How far the upload of `path` is (0..1), while it runs.
    pub fn progress(&self, path: &Path) -> Option<f32> {
        self.jobs.iter().find(|j| j.path == path).map(|j| j.progress.fraction())
    }

    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    pub fn stop(&mut self, path: &Path) {
        self.jobs.retain(|j| {
            let same = j.path == path;
            if same {
                j.progress.cancel.store(true, Ordering::Relaxed);
            }
            !same
        });
    }

    /// Finished uploads: the file and its link (or why it failed).
    pub fn poll(&mut self) -> Vec<(PathBuf, Result<String, String>)> {
        let mut done = Vec::new();
        self.jobs.retain(|j| match j.rx.try_recv() {
            Ok(r) => {
                done.push((j.path.clone(), r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done.push((j.path.clone(), Err("The upload stopped".into())));
                false
            }
            Err(_) => true,
        });
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r2_public_links_need_the_public_url() {
        let mut s = Settings { endpoint: "abc.r2.cloudflarestorage.com".into(), bucket: "b".into(), access_key: "k".into(), secret_saved: true, ..Settings::default() };
        assert!(s.is_r2() && !s.links_ready() && !s.set_up(VIDEO_ACCOUNT));
        s.link = LinkKind::Week;
        assert!(s.links_ready() && s.set_up(VIDEO_ACCOUNT));
        s.link = LinkKind::Public;
        s.public_url = "pub-1.r2.dev".into();
        assert!(s.set_up(VIDEO_ACCOUNT));
        // other storages make files public themselves
        let h = Settings { endpoint: "fsn1.your-objectstorage.com".into(), ..s.clone() };
        assert!(!h.is_r2() && Settings { public_url: String::new(), ..h }.links_ready());
    }

    #[test]
    fn keys_are_random_and_keep_the_extension() {
        let a = new_key("recordings", Path::new("/x/Recording 1.MP4"));
        let b = new_key("/recordings/", Path::new("/x/Recording 1.mp4"));
        assert!(a.starts_with("recordings/") && a.ends_with(".mp4") && a.len() == "recordings/".len() + 32 + 4, "{a}");
        assert_ne!(a, b);
        assert!(new_key("", Path::new("a.png")).ends_with(".png"));
        assert_eq!(content_type(Path::new("a.mp4")), "video/mp4");
    }

    #[test]
    fn links_open_the_share_page() {
        let l = page_link("https://clips.fsn1.your-objectstorage.com/recordings/ab12.mp4", "Recording 1");
        assert_eq!(l, "https://cmdmeow.com/view/?u=https%3A%2F%2Fclips.fsn1.your-objectstorage.com%2Frecordings%2Fab12.mp4&t=Recording%201");
        // presigned links keep their query intact inside u=
        let p = page_link("https://b.x/k.mp4?X-Amz-Signature=a&X-Amz-Date=b", "");
        assert!(p.ends_with("u=https%3A%2F%2Fb.x%2Fk.mp4%3FX-Amz-Signature%3Da%26X-Amz-Date%3Db"), "{p}");
        assert_eq!(title_of(Path::new("/x/Shot 2026-11-12 at 10.00.png")), "Shot 2026-11-12 at 10.00");
    }

    #[test]
    fn old_site_links_move_to_the_new_site() {
        assert_eq!(on_page("https://cmdmeow.furman.cat/view/?u=x&t=y"), "https://cmdmeow.com/view/?u=x&t=y");
        assert_eq!(on_page("https://cmdmeow.com/view/?u=x"), "https://cmdmeow.com/view/?u=x");
        assert!(ORIGINS.contains(&PAGE));
    }
}
