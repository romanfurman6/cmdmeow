//! Is a cache folder in use right now? A running app or background helper
//! with one of the folder's databases open would keep writing to a file that
//! no longer exists (and can fill the disk doing so). Checked with `lsof` just
//! before removal, on the folder's database files (with their -wal and -shm
//! companions). "Couldn't tell" counts as in use.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Idle,
    Busy,
    /// lsof didn't run, timed out or failed: treated as busy.
    Unknown,
}

/// How deep to look for databases inside a cache folder, and how many to probe.
const DEPTH: usize = 4;
const MAX_FILES: usize = 200;
const CHUNK: usize = 50;

fn is_db(name: &str) -> bool {
    let n = name.to_lowercase();
    [".db", ".sqlite", ".sqlite3", ".db-wal", ".db-shm", ".sqlite-wal", ".sqlite-shm", ".sqlite3-wal", ".sqlite3-shm", ".realm", ".ldb"].iter().any(|e| n.ends_with(e)) || n.ends_with("-wal") || n.ends_with("-shm") || n == "lock" || n == "singletonlock"
}

/// Database-like files below `dir` (or `dir` itself if it is one). `None` if
/// there are more than we are willing to probe (then the caller can't tell).
pub fn db_files(dir: &Path) -> Option<Vec<PathBuf>> {
    let mut out = Vec::new();
    let md = dir.symlink_metadata().ok()?;
    if md.is_file() {
        if dir.file_name().is_some_and(|n| is_db(&n.to_string_lossy())) {
            out.push(dir.to_path_buf());
        }
        return Some(out);
    }
    fn walk(p: &Path, depth: usize, out: &mut Vec<PathBuf>) -> bool {
        let Ok(rd) = std::fs::read_dir(p) else { return true };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path();
            if ft.is_dir() {
                if depth + 1 < DEPTH && !walk(&path, depth + 1, out) {
                    return false;
                }
            } else if ft.is_file() && is_db(&e.file_name().to_string_lossy()) {
                out.push(path);
                if out.len() > MAX_FILES {
                    return false;
                }
            }
        }
        true
    }
    walk(dir, 0, &mut out).then_some(out)
}

/// Read lsof's answer for one query. With several names, lsof exits 0 only if
/// all are open and 1 if any is not, so a printed process id also counts.
pub fn read_lsof(status_ok: bool, code: Option<i32>, stdout: &[u8]) -> Probe {
    if !String::from_utf8_lossy(stdout).trim().is_empty() || status_ok {
        Probe::Busy
    } else if code == Some(1) {
        Probe::Idle
    } else {
        Probe::Unknown
    }
}

/// Ask lsof whether any of `files` is open by any process.
pub fn probe(files: &[PathBuf]) -> Probe {
    if files.is_empty() {
        return Probe::Idle;
    }
    let Some(lsof) = super::run::find_tool(&["/usr/sbin/lsof", "/usr/bin/lsof"]) else { return Probe::Unknown };
    for chunk in files.chunks(CHUNK) {
        let mut cmd = std::process::Command::new(&lsof);
        cmd.args(["-w", "-t", "--"]).args(chunk);
        let p = match super::run::output(&mut cmd, Duration::from_secs(8)) {
            Some(o) => read_lsof(o.status.success(), o.status.code(), &o.stdout),
            None => Probe::Unknown,
        };
        if p != Probe::Idle {
            return p;
        }
    }
    Probe::Idle
}

/// Every file open on the Mac right now, as one sorted list (one lsof run for
/// the whole clean instead of one per item).
pub struct OpenFiles(Vec<PathBuf>);

impl OpenFiles {
    /// `None` if lsof couldn't give a list.
    pub fn snapshot() -> Option<OpenFiles> {
        let lsof = super::run::find_tool(&["/usr/sbin/lsof", "/usr/bin/lsof"])?;
        let o = super::run::output(std::process::Command::new(lsof).args(["-w", "-n", "-P", "-F", "n"]), Duration::from_secs(20))?;
        let list = parse_names(&o.stdout);
        (!list.is_empty()).then(|| OpenFiles::from(list))
    }
    pub fn from(mut v: Vec<PathBuf>) -> OpenFiles {
        v.sort();
        v.dedup();
        OpenFiles(v)
    }
    /// Some open file is `path` or lies inside it. lsof names files by their
    /// real path (/private/var/… for /var/…), so the resolved path counts too.
    pub fn holds(&self, path: &Path) -> bool {
        let inside = |q: &Path| {
            let i = self.0.partition_point(|p| p.as_path() < q);
            self.0.get(i).is_some_and(|p| p.starts_with(q))
        };
        inside(path) || path.canonicalize().is_ok_and(|real| real != path && inside(&real))
    }
}

/// The open paths in `lsof -F n` output ("n/path" lines).
pub fn parse_names(out: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter(|n| n.starts_with('/'))
        // "(deleted)" and similar notes after the path
        .map(|n| PathBuf::from(n.split(" (").next().unwrap_or(n)))
        .collect()
}

/// Why `path` must not be removed right now, if it mustn't. Uses the
/// snapshot when there is one, else probes the folder's databases.
pub fn blocked(path: &Path, open: Option<&OpenFiles>) -> Option<&'static str> {
    if let Some(o) = open {
        return o.holds(path).then_some("a running app has files in it open");
    }
    let Some(files) = db_files(path) else { return Some("too many files to check; quit its app and scan again") };
    match probe(&files) {
        Probe::Idle => None,
        Probe::Busy => Some("a running app has it open"),
        Probe::Unknown => Some("couldn't check whether a running app uses it"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_names() {
        for n in ["Cache.db", "data.sqlite", "x.sqlite3", "Cache.db-wal", "Cache.db-shm", "store-wal", "LOCK", "000003.ldb"] {
            assert!(is_db(n), "{n}");
        }
        for n in ["image.png", "index.html", "dbx", "notes.txt"] {
            assert!(!is_db(n), "{n}");
        }
    }

    #[test]
    fn finds_databases_with_companions() {
        let base = std::env::temp_dir().join(format!("cmdmeow-inuse-{}", std::process::id()));
        let deep = base.join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        for f in ["Cache.db", "Cache.db-wal", "Cache.db-shm"] {
            std::fs::write(deep.join(f), "x").unwrap();
        }
        std::fs::write(base.join("pic.png"), "x").unwrap();
        let mut f = db_files(&base).unwrap();
        f.sort();
        assert_eq!(f.len(), 3);
        assert!(f.iter().all(|p| p.starts_with(&deep)));
        // a plain file that is not a database has nothing to probe
        assert_eq!(db_files(&base.join("pic.png")), Some(vec![]));
        assert_eq!(probe(&[]), Probe::Idle);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn open_files_inside_a_folder() {
        let o = OpenFiles::from(parse_names(b"p12\nfcwd\nn/Users/me/Library/Caches/App/Cache.db\nf3\nn/dev/null\nn/tmp/x (deleted)\nnpipe\n"));
        assert!(o.holds(Path::new("/Users/me/Library/Caches/App")));
        assert!(o.holds(Path::new("/Users/me/Library/Caches/App/Cache.db")));
        assert!(o.holds(Path::new("/tmp/x")));
        assert!(!o.holds(Path::new("/Users/me/Library/Caches/Other")));
        // a sibling with a longer name is not "inside"
        assert!(!o.holds(Path::new("/Users/me/Library/Caches/Ap")));
        assert!(!o.holds(Path::new("/Users/me/Library/Caches/App2")));
    }

    #[test]
    fn open_files_are_matched_by_their_real_path() {
        // macOS: /var is a link to /private/var, and lsof prints the latter
        let base = std::env::temp_dir().join(format!("cmdmeow-inuse-real-{}", std::process::id()));
        std::fs::create_dir_all(base.join("real")).unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        let real = base.join("real").canonicalize().unwrap();
        let o = OpenFiles::from(vec![real.join("Cache.db")]);
        assert!(o.holds(&base.join("link")));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn lsof_answers() {
        assert_eq!(read_lsof(false, Some(1), b""), Probe::Idle);
        assert_eq!(read_lsof(false, Some(1), b"412\n"), Probe::Busy);
        assert_eq!(read_lsof(true, Some(0), b""), Probe::Busy);
        // a crash, a signal or an odd exit code: can't tell
        assert_eq!(read_lsof(false, None, b""), Probe::Unknown);
        assert_eq!(read_lsof(false, Some(2), b""), Probe::Unknown);
    }

    #[test]
    fn an_open_database_is_seen() {
        // only where lsof exists (macOS, most Linux boxes)
        if super::super::run::find_tool(&["/usr/sbin/lsof", "/usr/bin/lsof"]).is_none() {
            return;
        }
        let base = std::env::temp_dir().join(format!("cmdmeow-inuse-open-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let db = base.join("Cache.db");
        std::fs::write(&db, "x").unwrap();
        assert_eq!(probe(&[db.clone()]), Probe::Idle);
        let held = std::fs::File::open(&db).unwrap();
        assert_eq!(probe(&[db.clone()]), Probe::Busy);
        assert!(blocked(&base, None).is_some());
        let snap = OpenFiles::snapshot().expect("lsof lists open files");
        assert!(snap.holds(&base), "the snapshot sees the open file");
        assert!(blocked(&base, Some(&snap)).is_some());
        drop(held);
        assert!(blocked(&base, None).is_none());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
