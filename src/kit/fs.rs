//! File helpers every tool may use: folder sizes, the hard-kept locations
//! nothing may remove, moving to the Trash, and the app's own data files.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Would touching `p` make macOS ask the user about a folder (Desktop,
/// Documents, Downloads, iCloud Drive, other volumes)? Only while Full Disk
/// Access isn't granted. Background checks (a startup job's program, its
/// signature) skip such paths, so no folder question comes before setup.
pub fn behind_folder_prompt(p: &Path) -> bool {
    is_guarded(&home(), p) && crate::kit::perms::Perm::FullDisk.status() != crate::kit::perms::Status::Allowed
}

/// Is `p` in a folder macOS asks about (whatever is granted)?
fn is_guarded(home: &Path, p: &Path) -> bool {
    ["Desktop", "Documents", "Downloads", "Library/Mobile Documents"].iter().any(|x| p.starts_with(home.join(x))) || p.starts_with("/Volumes")
}

#[cfg(test)]
mod guarded_tests {
    use super::is_guarded;
    use std::path::Path;

    #[test]
    fn guarded_folders() {
        let h = Path::new("/Users/me");
        for p in ["/Users/me/Downloads", "/Users/me/Downloads/x.app", "/Users/me/Desktop/a", "/Users/me/Documents/p", "/Users/me/Library/Mobile Documents/com~apple~CloudDocs", "/Volumes/USB/x"] {
            assert!(is_guarded(h, Path::new(p)), "{p}");
        }
        for p in ["/Applications/Zed.app", "/Users/me/Library/Caches", "/Users/me/DownloadsOld", "/Users/me/.claude/projects"] {
            assert!(!is_guarded(h, Path::new(p)), "{p}");
        }
    }
}

/// How deep `dir_size` walks; deeper content is not counted.
const DIR_SIZE_MAX_DEPTH: usize = 64;

/// Bytes that deleting a file or folder would free, not following links or
/// leaving its volume. A file with several hard links is counted once, and an
/// APFS clone counts only the blocks it doesn't share with its original, so
/// the number matches what a clean really gives back.
pub fn dir_size(path: &Path) -> u64 {
    #[cfg(target_os = "macos")]
    if let Some(n) = bulk::size(path, DIR_SIZE_MAX_DEPTH) {
        return n;
    }
    walk_size(path, DIR_SIZE_MAX_DEPTH)
}

/// The portable walk: one `lstat` per entry, hard links counted once.
fn walk_size(path: &Path, max: usize) -> u64 {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;
    fn file(md: &std::fs::Metadata, seen: &mut HashSet<(u64, u64)>) -> u64 {
        if md.is_dir() || (md.nlink() > 1 && !seen.insert((md.dev(), md.ino()))) {
            return 0;
        }
        md.blocks() * 512
    }
    fn walk(p: &Path, depth: usize, max: usize, dev: u64, seen: &mut HashSet<(u64, u64)>) -> u64 {
        let Ok(rd) = std::fs::read_dir(p) else { return 0 };
        let mut total = 0;
        for e in rd.flatten() {
            let Ok(md) = e.path().symlink_metadata() else { continue };
            if md.dev() != dev {
                continue;
            }
            total += file(&md, seen);
            if md.is_dir() && depth < max {
                total += walk(&e.path(), depth + 1, max, dev, seen);
            }
        }
        total
    }
    let mut seen = HashSet::new();
    match path.symlink_metadata() {
        Ok(md) if md.is_dir() => walk(path, 0, max, md.dev(), &mut seen),
        Ok(md) => file(&md, &mut seen),
        Err(_) => 0,
    }
}

/// One entry of a folder, as [`list_dir`] reads it.
#[derive(Debug, Clone)]
pub struct Listed {
    pub name: std::ffi::OsString,
    pub dir: bool,
    /// File id, to count a file with several hard links once.
    pub id: u64,
    pub links: u32,
    /// Allocated bytes on disk (0 for folders).
    pub bytes: u64,
    /// APFS: bytes no other file shares (less than `bytes` for a clone).
    pub private: Option<u64>,
    /// APFS: the clone family, the same for files cloned from one another.
    pub clone: Option<u64>,
}

/// The device a folder is on and every entry in it, not following links.
/// macOS reads a whole batch of entries per call (`getattrlistbulk`);
/// elsewhere it's one `lstat` per entry. `None` when it can't be read.
pub fn list_dir(dir: &Path) -> Option<(u64, Vec<Listed>)> {
    #[cfg(target_os = "macos")]
    if let Some(v) = bulk::list_path(dir) {
        return Some(v);
    }
    list_portable(dir)
}

fn list_portable(dir: &Path) -> Option<(u64, Vec<Listed>)> {
    use std::os::unix::fs::MetadataExt;
    let dev = dir.symlink_metadata().ok().filter(|m| m.is_dir())?.dev();
    let rd = std::fs::read_dir(dir).ok()?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(md) = e.path().symlink_metadata() else { continue };
        let is_dir = md.is_dir();
        out.push(Listed { name: e.file_name(), dir: is_dir, id: md.ino(), links: md.nlink() as u32, bytes: if is_dir { 0 } else { md.blocks() * 512 }, private: None, clone: None });
    }
    Some((dev, out))
}

/// Free space as `df` shows it, without what macOS can purge on its own.
pub fn df_free(path: &str) -> u64 {
    statvfs_space(path).1
}

/// macOS: `getattrlistbulk` reads a whole batch of entries (type, id, link
/// count, size) in one call, instead of an `lstat` per file, and asks APFS
/// for each file's private size, the part not shared with clones.
#[cfg(target_os = "macos")]
mod bulk {
    use libc::{attribute_set_t, attrlist, attrreference_t, c_int};
    use std::collections::HashSet;
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const ATTR_CMN_ERROR: u32 = 0x2000_0000;
    const VREG: u32 = 1;
    const VDIR: u32 = 2;

    struct Fd(c_int);
    impl Drop for Fd {
        fn drop(&mut self) {
            unsafe { libc::close(self.0) };
        }
    }

    /// One entry of a batch, as far as sizing needs it.
    #[derive(Debug, Default, PartialEq)]
    pub(super) struct Entry {
        pub name: Vec<u8>,
        pub dev: Option<i32>,
        pub kind: u32,
        pub id: u64,
        pub links: u32,
        pub alloc: Option<u64>,
        pub private: Option<u64>,
        pub clone: Option<u64>,
        pub error: u32,
    }

    impl Entry {
        /// Bytes this entry frees; hard links after the first count nothing.
        fn bytes(&self, seen: &mut HashSet<u64>) -> u64 {
            if self.kind == VDIR || self.error != 0 {
                return 0;
            }
            if self.links > 1 && !seen.insert(self.id) {
                return 0;
            }
            self.private.or(self.alloc).unwrap_or(0)
        }
    }

    fn request() -> attrlist {
        attrlist {
            bitmapcount: libc::ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: libc::ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_ERROR | libc::ATTR_CMN_NAME | libc::ATTR_CMN_DEVID | libc::ATTR_CMN_OBJTYPE | libc::ATTR_CMN_FILEID,
            volattr: 0,
            dirattr: 0,
            fileattr: libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_ALLOCSIZE,
            forkattr: libc::ATTR_CMNEXT_PRIVATESIZE | libc::ATTR_CMNEXT_CLONEID,
        }
    }

    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        b.get(at..at + 4).map(|v| u32::from_ne_bytes(v.try_into().unwrap()))
    }
    fn u64_at(b: &[u8], at: usize) -> Option<u64> {
        b.get(at..at + 8).map(|v| u64::from_ne_bytes(v.try_into().unwrap()))
    }

    /// Parses one record: the attributes come in a fixed order (returned set,
    /// error, then common, file and extended ones by bit), each only when the
    /// returned set says it's there, packed on 4-byte boundaries.
    pub(super) fn parse(rec: &[u8]) -> Option<Entry> {
        let mut at = 4;
        let set = std::mem::size_of::<attribute_set_t>();
        let common = u32_at(rec, at)?;
        let file = u32_at(rec, at + 12)?;
        let ext = u32_at(rec, at + 16)?;
        at += set;
        let mut e = Entry::default();
        if common & ATTR_CMN_ERROR != 0 {
            e.error = u32_at(rec, at)?;
            at += 4;
        }
        if common & libc::ATTR_CMN_NAME != 0 {
            let off = u32_at(rec, at)? as i32;
            let len = u32_at(rec, at + 4)? as usize;
            let from = (at as isize + off as isize) as usize;
            let raw = rec.get(from..from + len)?;
            e.name = raw.split(|c| *c == 0).next().unwrap_or(&[]).to_vec();
            at += std::mem::size_of::<attrreference_t>();
        }
        if common & libc::ATTR_CMN_DEVID != 0 {
            e.dev = Some(u32_at(rec, at)? as i32);
            at += 4;
        }
        if common & libc::ATTR_CMN_OBJTYPE != 0 {
            e.kind = u32_at(rec, at)?;
            at += 4;
        }
        if common & libc::ATTR_CMN_FILEID != 0 {
            e.id = u64_at(rec, at)?;
            at += 8;
        }
        if file & libc::ATTR_FILE_LINKCOUNT != 0 {
            e.links = u32_at(rec, at)?;
            at += 4;
        }
        if file & libc::ATTR_FILE_ALLOCSIZE != 0 {
            e.alloc = Some(u64_at(rec, at)?);
            at += 8;
        }
        if ext & libc::ATTR_CMNEXT_PRIVATESIZE != 0 {
            e.private = Some(u64_at(rec, at)?);
            at += 8;
        }
        if ext & libc::ATTR_CMNEXT_CLONEID != 0 {
            e.clone = Some(u64_at(rec, at)?).filter(|c| *c != 0);
        }
        Some(e)
    }

    /// Every entry of an open folder, batch by batch.
    fn list(fd: c_int, buf: &mut [u64], out: &mut Vec<Entry>) -> bool {
        let mut req = request();
        let bytes = buf.len() * 8;
        loop {
            let n = unsafe { libc::getattrlistbulk(fd, &mut req as *mut _ as *mut _, buf.as_mut_ptr() as *mut _, bytes, libc::FSOPT_ATTR_CMN_EXTENDED as u64) };
            if n < 0 {
                return false;
            }
            if n == 0 {
                return true;
            }
            let b = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, bytes) };
            let mut at = 0;
            for _ in 0..n {
                let Some(len) = u32_at(b, at).map(|l| l as usize).filter(|l| *l >= 4) else { return false };
                let Some(rec) = b.get(at..at + len) else { return false };
                if let Some(e) = parse(rec) {
                    out.push(e);
                }
                at += len;
            }
        }
    }

    fn open_dir(parent: c_int, name: &CStr) -> Option<Fd> {
        let fd = unsafe { libc::openat(parent, name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        (fd >= 0).then_some(Fd(fd))
    }

    fn dev_of(fd: c_int) -> Option<i32> {
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        (unsafe { libc::fstat(fd, &mut st) } == 0).then_some(st.st_dev)
    }

    /// `None` when the folder can't be listed in bulk.
    fn walk(fd: c_int, dev: i32, depth: usize, max: usize, buf: &mut [u64], seen: &mut HashSet<u64>) -> Option<u64> {
        let mut entries = Vec::new();
        if !list(fd, buf, &mut entries) && entries.is_empty() {
            return None;
        }
        let mut total = 0;
        for e in entries {
            if e.dev.is_some_and(|d| d != dev) {
                continue;
            }
            total += e.bytes(seen);
            if e.kind == VDIR && depth < max && e.error == 0 {
                let Ok(name) = CString::new(e.name) else { continue };
                if let Some(sub) = open_dir(fd, &name) {
                    // a volume mounted here is somebody else's space
                    if dev_of(sub.0) == Some(dev) {
                        total += walk(sub.0, dev, depth + 1, max, buf, seen).unwrap_or(0);
                    }
                }
            }
        }
        Some(total)
    }

    /// A folder's device and entries, read in bulk. `None` when it can't be
    /// opened or listed that way.
    pub fn list_path(path: &Path) -> Option<(u64, Vec<super::Listed>)> {
        thread_local! {
            static BUF: std::cell::RefCell<Vec<u64>> = std::cell::RefCell::new(vec![0u64; 16 * 1024]);
        }
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let fd = open_dir(libc::AT_FDCWD, &c)?;
        let dev = dev_of(fd.0)?;
        let mut entries = Vec::new();
        let ok = BUF.with(|b| list(fd.0, &mut b.borrow_mut(), &mut entries));
        if !ok && entries.is_empty() {
            return None;
        }
        let out = entries
            .into_iter()
            .filter(|e| e.error == 0 && !e.name.is_empty())
            .map(|e| {
                let dir = e.kind == VDIR;
                super::Listed { name: std::ffi::OsStr::from_bytes(&e.name).to_os_string(), dir, id: e.id, links: e.links, bytes: if dir { 0 } else { e.alloc.unwrap_or(0) }, private: if dir { None } else { e.private }, clone: e.clone }
            })
            .collect();
        Some((dev as u64, out))
    }

    /// `None` when the path isn't a folder that can be listed in bulk; the
    /// caller then falls back to the portable walk.
    pub fn size(path: &Path, max: usize) -> Option<u64> {
        if !path.symlink_metadata().ok()?.is_dir() {
            return None;
        }
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let root = open_dir(libc::AT_FDCWD, &c)?;
        let dev = dev_of(root.0)?;
        let mut buf = vec![0u64; 16 * 1024];
        walk(root.0, dev, 0, max, &mut buf, &mut HashSet::new())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn bulk_matches_the_portable_walk() {
            let dir = std::env::temp_dir().join(format!("cmb-bulk-{}", std::process::id()));
            let deep = dir.join("a/b/c");
            std::fs::create_dir_all(&deep).unwrap();
            for i in 0..300 {
                std::fs::write(dir.join(format!("f{i}.bin")), vec![7u8; 5_000 + i]).unwrap();
            }
            std::fs::write(deep.join("big.bin"), vec![1u8; 300_000]).unwrap();
            std::os::unix::fs::symlink("/Applications", dir.join("link")).unwrap();
            let bulk = size(&dir, 64).unwrap();
            let walk = super::super::walk_size(&dir, 64);
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(bulk >= 300_000 + 300 * 5_000, "bulk {bulk}");
            assert!(bulk.abs_diff(walk) <= 64 * 1024, "bulk {bulk} vs walk {walk}");
        }

        #[test]
        fn hard_links_and_clones_free_nothing_extra() {
            let dir = std::env::temp_dir().join(format!("cmb-links-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let orig = dir.join("orig.bin");
            {
                use std::io::Write;
                let mut f = std::fs::File::create(&orig).unwrap();
                f.write_all(&vec![3u8; 2_000_000]).unwrap();
                // APFS allocates blocks lazily: before a flush st_blocks can still be 0
                f.sync_all().unwrap();
            }
            let one = size(&dir, 64).unwrap();
            std::fs::hard_link(&orig, dir.join("link.bin")).unwrap();
            let linked = size(&dir, 64).unwrap();
            let walk_linked = super::super::walk_size(&dir, 64);
            let a = CString::new(orig.as_os_str().as_bytes()).unwrap();
            let b = CString::new(dir.join("clone.bin").as_os_str().as_bytes()).unwrap();
            let cloned = unsafe { libc::clonefile(a.as_ptr(), b.as_ptr(), 0) } == 0;
            let with_clone = size(&dir, 64).unwrap();
            std::fs::remove_dir_all(&dir).unwrap();
            assert!(one >= 2_000_000, "one {one}");
            assert_eq!(linked, one, "a hard link frees nothing");
            assert_eq!(walk_linked, one, "the portable walk agrees");
            if cloned {
                // on APFS a clone shares every block with its original
                assert!(with_clone < one + 100_000, "clone counted again: {with_clone} vs {one}");
            }
        }
    }
}

// ---------------------------------------------------------------- app data

/// A file or folder of the app's own data:
/// `~/Library/Application Support/cmdmeow/<name>`.
pub fn data_path(name: &str) -> PathBuf {
    data_root().join(name)
}

/// `~/Library/Application Support/cmdmeow`. The first call moves the folder
/// over from the app's first name (`cleanmebro`), so settings, clipboard
/// history and capture settings survive the rename.
fn data_root() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let base = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
        // the dev build keeps its own data, so it never marks setup done or
        // shares history and settings with the installed app
        if is_dev() {
            return base.join("cmdmeow-dev");
        }
        migrate_dir(&base.join("cleanmebro"), &base.join("cmdmeow"));
        base.join("cmdmeow")
    })
    .clone()
}

/// True when running from the dev build ("… Dev.app"), which keeps its own
/// data, window state and Keychain items apart from the installed app.
pub fn is_dev() -> bool {
    static DEV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEV.get_or_init(|| std::env::current_exe().ok().is_some_and(|p| is_dev_bundle(&p)))
}

/// True for a binary inside a "… Dev.app" bundle.
fn is_dev_bundle(exe: &Path) -> bool {
    exe.ancestors().any(|a| a.extension().is_some_and(|e| e == "app") && a.file_stem().is_some_and(|n| n.to_string_lossy().ends_with(" Dev")))
}

/// Rename `old` to `new` when only `old` exists.
fn migrate_dir(old: &Path, new: &Path) {
    if old.is_dir() && !new.exists() {
        if let Err(e) = std::fs::rename(old, new) {
            eprintln!("[data] couldn't move {} to {}: {e}", old.display(), new.display());
        }
    }
}

/// A JSON file's value, or the default when it is missing or unreadable.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// `v` as JSON; `pretty` for files people may open.
fn to_json<T: Serialize + ?Sized>(v: &T, pretty: bool) -> Option<Vec<u8>> {
    if pretty { serde_json::to_vec_pretty(v) } else { serde_json::to_vec(v) }.ok()
}

/// Write `bytes` through a temp file beside `path` and move it in place,
/// so a crash never leaves half a file. Creates the folder.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> bool {
    write_atomic_as(path, bytes, false)
}

/// [`write_atomic`] for private data: the file is readable and writable by
/// its owner only (0600) from the moment it exists.
pub fn write_private(path: &Path, bytes: &[u8]) -> bool {
    write_atomic_as(path, bytes, true)
}

fn write_atomic_as(path: &Path, bytes: &[u8], private: bool) -> bool {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    // a temp file left by a crash keeps its old mode: start fresh
    let _ = std::fs::remove_file(&tmp);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let written = o.open(&tmp).and_then(|mut f| f.write_all(bytes)).is_ok();
    written && std::fs::rename(&tmp, path).is_ok()
}

/// Save `v` as JSON to `path` (see [`write_atomic`]).
pub fn save_json<T: Serialize + ?Sized>(path: &Path, v: &T, pretty: bool) -> bool {
    to_json(v, pretty).is_some_and(|b| write_atomic(path, &b))
}

enum Write {
    /// Path, bytes, owner-only.
    File(PathBuf, Vec<u8>, bool),
    Flush(Sender<()>),
}

/// The background writer: the newest bytes per file win, so a burst of
/// saves writes each file once.
fn writer() -> &'static Mutex<Sender<Write>> {
    static TX: OnceLock<Mutex<Sender<Write>>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Write>();
        std::thread::Builder::new().name("writer".into()).spawn(move || write_loop(rx)).ok();
        Mutex::new(tx)
    })
}

fn write_loop(rx: Receiver<Write>) {
    while let Ok(first) = rx.recv() {
        let mut files: Vec<(PathBuf, Vec<u8>, bool)> = Vec::new();
        let mut flushes = Vec::new();
        for w in std::iter::once(first).chain(rx.try_iter()) {
            match w {
                Write::File(p, b, private) => {
                    files.retain(|(q, _, _)| *q != p);
                    files.push((p, b, private));
                }
                Write::Flush(done) => flushes.push(done),
            }
        }
        for (p, b, private) in files {
            write_atomic_as(&p, &b, private);
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

/// Save `v` as JSON to `path` on the background writer, off the UI thread.
/// Serializing happens here, so later changes to `v` don't leak in.
pub fn save_json_later<T: Serialize + ?Sized>(path: &Path, v: &T, pretty: bool) {
    if let Some(b) = to_json(v, pretty) {
        let _ = writer().lock().unwrap_or_else(|e| e.into_inner()).send(Write::File(path.to_path_buf(), b, false));
    }
}

/// Write `bytes` to `path` owner-only ([`write_private`]) on the background writer.
pub fn save_private_later(path: &Path, bytes: Vec<u8>) {
    let _ = writer().lock().unwrap_or_else(|e| e.into_inner()).send(Write::File(path.to_path_buf(), bytes, true));
}

/// Wait (up to 2 s) until every background save so far is on disk.
pub fn flush_writes() {
    let (tx, rx) = std::sync::mpsc::channel();
    if writer().lock().unwrap_or_else(|e| e.into_inner()).send(Write::Flush(tx)).is_ok() {
        let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
    }
}

/// (total, available) bytes of the volume holding `path`. On macOS
/// "available" is what Finder shows: free space plus what macOS can purge
/// on its own (local snapshots, caches it manages), so a nearly full disk
/// isn't reported as full. Kept for a few seconds, since pages ask for it
/// every frame and the purgeable part takes a moment to work out.
pub fn volume_space(path: &str) -> (u64, u64) {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<HashMap<String, (Instant, (u64, u64))>>> = Mutex::new(None);
    if let Some((at, v)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(path) {
        if at.elapsed() < Duration::from_secs(5) {
            return *v;
        }
    }
    #[cfg(target_os = "macos")]
    let v = finder_space(path).unwrap_or_else(|| statvfs_space(path));
    #[cfg(not(target_os = "macos"))]
    let v = statvfs_space(path);
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(path.to_string(), (Instant::now(), v));
    v
}

/// Free space without purgeable: what `df` shows.
fn statvfs_space(path: &str) -> (u64, u64) {
    let Ok(c) = std::ffi::CString::new(path) else { return (0, 0) };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
        let total = st.f_blocks as u64 * st.f_frsize as u64;
        let free = st.f_bavail as u64 * st.f_frsize as u64;
        (total, free)
    } else {
        (0, 0)
    }
}

/// Total and "available for important usage" from the volume's URL keys.
#[cfg(target_os = "macos")]
fn finder_space(path: &str) -> Option<(u64, u64)> {
    use objc2_foundation::{NSArray, NSNumber, NSString, NSURLVolumeAvailableCapacityForImportantUsageKey, NSURLVolumeTotalCapacityKey, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let (total_key, avail_key) = unsafe { (NSURLVolumeTotalCapacityKey, NSURLVolumeAvailableCapacityForImportantUsageKey) };
    let keys = NSArray::from_slice(&[total_key, avail_key]);
    let values = url.resourceValuesForKeys_error(&keys).ok()?;
    let num = |k: &NSString| values.objectForKey(k).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.longLongValue().max(0) as u64);
    let (total, avail) = (num(total_key)?, num(avail_key)?);
    (total > 0).then_some((total, avail.min(total)))
}

/// Paths that are never offered for removal, whatever the rules or the AI say.
/// A path is also judged by where it really leads: a symlinked parent (say a
/// cache folder linked into Documents) can't route around these rules. That
/// second look can only refuse, never allow.
pub fn is_protected(p: &Path) -> Option<&'static str> {
    is_protected_in(p, &home())
}

fn is_protected_in(p: &Path, h: &Path) -> Option<&'static str> {
    if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Some("Not a plain full path");
    }
    if let Some(why) = protected_literal(p, h) {
        return Some(why);
    }
    // where it really is: the parent resolved (the item itself may be a link,
    // which removal treats as the link alone), checked against the real home too
    let real_home = h.canonicalize().unwrap_or_else(|_| h.to_path_buf());
    if let (Some(parent), Some(name)) = (p.parent(), p.file_name()) {
        if let Ok(rp) = parent.canonicalize() {
            let resolved = rp.join(name);
            if resolved != p {
                if let Some(why) = protected_literal(&resolved, h).or_else(|| protected_literal(&resolved, &real_home)) {
                    return Some(why);
                }
            }
        }
    }
    None
}

fn protected_literal(p: &Path, h: &Path) -> Option<&'static str> {
    let personal = ["Documents", "Desktop", "Pictures", "Movies", "Music", "Projects", "Developer", "Dropbox", "Library/Mobile Documents", "Library/Keychains", "Library/Messages", ".ssh", ".gnupg"];
    for d in personal {
        if p.starts_with(h.join(d)) {
            return Some("Personal files and settings are never removed");
        }
    }
    if p.starts_with(h.join("Library/Mail")) {
        return Some("Mail data is never removed");
    }
    let claude_keep = ["settings.json", "settings.local.json", "CLAUDE.md", "skills", "plugins/marketplaces", "plugins/repos", "agents", "commands", "history.jsonl", "scheduled-tasks", "sessions"];
    for d in claude_keep {
        if p.starts_with(h.join(".claude").join(d)) {
            return Some("Your Claude settings, skills and history are kept");
        }
    }
    if p.join(".git").exists() {
        return Some("Looks like a code project (has .git)");
    }
    None
}

/// Move to the Trash (so it can be put back). Never falls back to a permanent
/// delete: an item that can't be trashed is reported as failed.
pub fn move_to_trash(p: &Path) -> Result<(), String> {
    crate::kit::native::trash_item(p)
}

/// Like `move_to_trash`, and returns where in the Trash the item landed.
pub fn move_to_trash_at(p: &Path) -> Result<PathBuf, String> {
    crate::kit::native::trash_item_at(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_folder_moves_over_from_the_old_name() {
        let base = std::env::temp_dir().join(format!("cmdmeow-migrate-{}", std::process::id()));
        assert!(is_dev_bundle(Path::new("/Applications/CmdMeow Dev.app/Contents/MacOS/cmdmeow")));
        assert!(!is_dev_bundle(Path::new("/Applications/CmdMeow.app/Contents/MacOS/cmdmeow")));
        let (old, new) = (base.join("cleanmebro"), base.join("cmdmeow"));
        std::fs::create_dir_all(old.join("clipboard")).unwrap();
        std::fs::write(old.join("clipboard/history.json"), "[]").unwrap();
        migrate_dir(&old, &new);
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(new.join("clipboard/history.json")).unwrap(), "[]");
        // an existing new folder is never overwritten
        std::fs::create_dir_all(&old).unwrap();
        migrate_dir(&old, &new);
        assert!(old.exists() && new.join("clipboard/history.json").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn personal_folders_are_protected() {
        let h = home();
        assert!(is_protected(&h.join("Documents/report.pdf")).is_some());
        assert!(is_protected(&h.join(".ssh")).is_some());
        assert!(is_protected(&h.join("Library/Mail/V10")).is_some());
        assert!(is_protected(&h.join(".claude/settings.json")).is_some());
        assert!(is_protected(&h.join(".claude/skills/x")).is_some());
        assert!(is_protected(&h.join(".claude/debug")).is_none());
        assert!(is_protected(&h.join("Library/Caches/com.example.app")).is_none());
        // "Documents" must match a whole folder name, not a prefix
        assert!(is_protected(&h.join("DocumentsOld")).is_none());
    }

    #[test]
    fn odd_paths_are_refused() {
        let h = home();
        assert!(is_protected(Path::new("Library/Caches/x")).is_some());
        assert!(is_protected(&h.join("Library/Caches/../../Documents")).is_some());
        // ".." inside a name is fine
        assert!(is_protected(&h.join("Library/Caches/a..b")).is_none());
    }

    #[test]
    fn a_symlinked_parent_cant_route_around_the_rules() {
        // a cache folder that is really a link into Documents
        let h = std::env::temp_dir().join(format!("cmb-link-{}", std::process::id()));
        std::fs::create_dir_all(h.join("Documents/thesis")).unwrap();
        std::fs::create_dir_all(h.join("Library/Caches/real")).unwrap();
        std::os::unix::fs::symlink(h.join("Documents"), h.join("Library/Caches/evil")).unwrap();
        assert!(is_protected_in(&h.join("Library/Caches/evil/thesis"), &h).is_some());
        assert!(is_protected_in(&h.join("Library/Caches/real"), &h).is_none());
        // the link itself is only a link: removing it leaves Documents alone
        assert!(is_protected_in(&h.join("Library/Caches/evil"), &h).is_none());
        std::fs::remove_dir_all(&h).unwrap();
    }

    #[test]
    fn git_folders_are_protected() {
        let dir = std::env::temp_dir().join(format!("cmb-git-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let r = is_protected(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(r.is_some());
    }

    #[test]
    fn json_round_trips_atomically_and_in_the_background() {
        let dir = std::env::temp_dir().join(format!("cmb-json-{}", std::process::id()));
        let f = dir.join("sub/v.json");
        assert!(save_json(&f, &vec![1, 2, 3], true));
        assert_eq!(load_json::<Vec<i32>>(&f), vec![1, 2, 3]);
        assert!(!dir.join("sub/v.json.tmp").exists());
        // a burst of background saves: the last one wins
        for i in 0..20 {
            save_json_later(&f, &vec![i], false);
        }
        flush_writes();
        assert_eq!(load_json::<Vec<i32>>(&f), vec![19]);
        assert_eq!(load_json::<Vec<i32>>(&dir.join("missing.json")), Vec::<i32>::new());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hard_links_are_counted_once() {
        let dir = std::env::temp_dir().join(format!("cmb-hardlink-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.bin"), vec![5u8; 400_000]).unwrap();
        let one = dir_size(&dir);
        std::fs::hard_link(dir.join("a.bin"), dir.join("sub/b.bin")).unwrap();
        let two = dir_size(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(one >= 400_000, "one {one}");
        assert_eq!(one, two);
    }

    #[test]
    fn dir_size_counts_nested_files() {
        let dir = std::env::temp_dir().join(format!("cmb-dirsize-{}", std::process::id()));
        let deep = dir.join("a/b/c/d/e/f/g/h");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("big.bin"), vec![1u8; 200_000]).unwrap();
        let size = dir_size(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        // the file sits 8 levels down and must be counted
        assert!(size >= 200_000, "size {size}");
        assert_eq!(dir_size(Path::new("/definitely/not/here")), 0);
    }
}
