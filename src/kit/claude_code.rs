//! Claude Code on this Mac: where its CLI lives and which projects it knows.
//! Shared by every tool that talks to Claude Code (limits, the AI judge,
//! keeping in-use projects safe).

use crate::kit::mechanics as m;
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::kit::activity::Kick;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::SystemTime;

// ---------------------------------------------------------------- availability

/// Whether the Claude tool is on in Settings → Tools.
static TOOL_ON: AtomicBool = AtomicBool::new(true);
/// What `claude auth status` last said: 0 unknown, 1 signed in, 2 signed out.
static SIGNED_IN: AtomicU8 = AtomicU8::new(0);
/// Someone asked to look for Claude Code again (install, sign-in, limits),
/// or the tool was switched on or off.
static RECHECK: Kick = Kick::new();

/// Whether Claude can be used right now, and if not, why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    /// The person turned the Claude tool off in Settings → Tools.
    Off,
    NotInstalled,
    SignedOut,
}

impl Availability {
    pub fn ready(self) -> bool {
        self == Availability::Ready
    }
    /// How to enable it.
    pub fn how(self) -> &'static str {
        match self {
            Availability::Ready => "",
            Availability::Off => "Turn Claude on in Settings → Tools.",
            Availability::NotInstalled => "Install Claude Code from claude.com/claude-code, then run claude in Terminal once to sign in.",
            Availability::SignedOut => "Run claude in Terminal and sign in with your Claude account.",
        }
    }
    /// A few words for a subtitle.
    pub fn short(self) -> &'static str {
        match self {
            Availability::Ready => "",
            Availability::Off => "Claude is off",
            Availability::NotInstalled => "Claude Code not installed",
            Availability::SignedOut => "Claude Code signed out",
        }
    }
}

/// Can Claude be used? The tool switch, the CLI on disk, then the last sign-in check.
pub fn availability() -> Availability {
    if !tool_on() {
        Availability::Off
    } else if claude_bin().is_none() {
        Availability::NotInstalled
    } else if SIGNED_IN.load(Ordering::Relaxed) == 2 {
        Availability::SignedOut
    } else {
        Availability::Ready
    }
}

/// The CLI is there and not known to be signed out, whatever the AI Usage
/// switch says (AI Check runs the CLI without that tool).
pub fn cli_ready() -> bool {
    claude_bin().is_some() && SIGNED_IN.load(Ordering::Relaxed) != 2
}

pub fn tool_on() -> bool {
    TOOL_ON.load(Ordering::Relaxed)
}

pub fn set_tool_on(on: bool) {
    TOOL_ON.store(on, Ordering::Relaxed);
    RECHECK.kick();
}

/// Record what `claude auth status` said (`None`: it couldn't tell).
pub fn set_signed_in(v: Option<bool>) {
    SIGNED_IN.store(
        match v {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::Relaxed,
    );
}

/// Ask the Claude tool to look for Claude Code again soon.
pub fn request_recheck() {
    RECHECK.kick();
}

/// The recheck request, for the loop that waits on it.
pub fn recheck() -> &'static Kick {
    &RECHECK
}

/// Open Terminal (to install or sign in).
pub fn open_terminal() {
    let _ = Command::new("open").args(["-a", "Terminal"]).spawn();
}

#[derive(Clone, Debug, Default)]
pub struct Project {
    pub folder: PathBuf, // the ~/.claude/projects/<x> folder
    pub cwd: Option<PathBuf>,
    pub cwd_exists: bool,
    pub last_used: Option<SystemTime>,
}

impl Project {
    pub fn display_name(&self) -> String {
        match &self.cwd {
            Some(c) => c.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| c.display().to_string()),
            None => self.folder.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }
    pub fn days_idle(&self) -> f64 {
        self.last_used
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .map(|d| d.as_secs_f64() / 86_400.0)
            .unwrap_or(9999.0)
    }
    /// A project the person still works on: its folder exists and it was used recently.
    pub fn in_use(&self) -> bool {
        self.cwd_exists && self.days_idle() <= m::CLAUDE_PROJECT_ACTIVE_DAYS as f64
    }
}

/// Find the claude executable even when the app is started from Finder (no shell PATH).
pub fn claude_bin() -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut cands = vec![
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        home.join(".claude/local/claude"),
        home.join(".npm-global/bin/claude"),
    ];
    if let Ok(path) = std::env::var("PATH") {
        for d in path.split(':') {
            cands.push(Path::new(d).join("claude"));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

pub fn claude_cmd(bin: &Path) -> Command {
    let mut c = Command::new(bin);
    // a neutral working folder so no project settings or CLAUDE().md get involved
    c.current_dir(std::env::temp_dir());
    let path = std::env::var("PATH").unwrap_or_default();
    c.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{path}"));
    c
}

pub fn projects_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude/projects")
}

/// Every folder Claude Code keeps session logs in: `~/.claude/projects`,
/// and `~/.config/claude/projects`, where newer versions can write them.
pub fn log_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    log_dirs_in(&home)
}

fn log_dirs_in(home: &std::path::Path) -> Vec<PathBuf> {
    let mut v = vec![home.join(".claude/projects")];
    let xdg = home.join(".config/claude/projects");
    if xdg.is_dir() {
        v.push(xdg);
    }
    v
}

/// Claude Code's projects and whether their folders still exist. Looks at
/// each project's own folder (and walks the disk to resolve its name), so a
/// project in Downloads, Documents or Desktop makes macOS ask about that
/// folder: call it only once Full Disk Access is granted (Cleanup's scan).
pub fn scan_projects() -> Vec<Project> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(projects_dir()) else { return out };
    for e in rd.flatten() {
        let folder = e.path();
        if !folder.is_dir() {
            continue;
        }
        let mut last: Option<SystemTime> = None;
        let mut cwd: Option<PathBuf> = None;
        if let Ok(files) = std::fs::read_dir(&folder) {
            let mut jsonls: Vec<(SystemTime, PathBuf)> = files
                .flatten()
                .filter(|f| f.path().extension().map(|x| x == "jsonl").unwrap_or(false))
                .filter_map(|f| f.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, f.path())))
                .collect();
            jsonls.sort_by_key(|x| std::cmp::Reverse(x.0));
            last = jsonls.first().map(|x| x.0);
            for (_, p) in jsonls.iter().take(3) {
                if let Some(c) = first_cwd(p) {
                    cwd = Some(c);
                    break;
                }
            }
        }
        if last.is_none() {
            last = folder.metadata().ok().and_then(|m| m.modified().ok());
        }
        // the folder name is the project path with every non-alphanumeric
        // character turned into '-'; find the real folder it stands for
        let enc = folder.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // (a session's logged cwd may be a subfolder; trust it when it exists)
        if let Some(real) = resolve_encoded(&enc) {
            cwd = Some(real);
        } else if cwd.as_ref().is_none_or(|c| !exists_quiet(c)) {
            cwd = Some(PathBuf::from(enc.replace('-', "/")));
        }
        let cwd_exists = cwd.as_ref().map(|c| exists_quiet(c)).unwrap_or(false);
        out.push(Project { folder, cwd, cwd_exists, last_used: last });
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.last_used));
    out
}

/// How Claude Code names a project folder: non-alphanumerics become '-'.
pub fn encode(path: &str) -> String {
    path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Does `p` exist? A path in Downloads/Desktop/Documents (without Full
/// Disk Access) counts as there: looking would make macOS ask the user about
/// the folder, and a project must never be called gone because we didn't look.
fn exists_quiet(p: &Path) -> bool {
    crate::kit::fs::behind_folder_prompt(p) || p.exists()
}

/// Walk the disk to find the folder whose encoded path equals `enc`.
fn resolve_encoded(enc: &str) -> Option<PathBuf> {
    fn go(dir: &Path, rest: &str, depth: usize) -> Option<PathBuf> {
        // never list a folder macOS guards with a question (the first launch
        // asked about Downloads because of a project there)
        if depth > 14 || crate::kit::fs::behind_folder_prompt(dir) {
            return None;
        }
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let en = encode(&name);
            if en.is_empty() || !rest.starts_with(&en) {
                continue;
            }
            let p = e.path();
            if rest.len() == en.len() {
                if p.is_dir() {
                    return Some(p);
                }
            } else if rest.as_bytes()[en.len()] == b'-' && p.is_dir() {
                if let Some(found) = go(&p, &rest[en.len() + 1..], depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    let rest = enc.strip_prefix('-')?;
    go(Path::new("/"), rest, 0)
}

pub fn first_cwd(p: &Path) -> Option<PathBuf> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(p).ok()?;
    for line in BufReader::new(f).lines().take(60).map_while(Result::ok) {
        if let Some(i) = line.find("\"cwd\":\"") {
            let rest = &line[i + 7..];
            if let Some(end) = rest.find('"') {
                return Some(PathBuf::from(rest[..end].replace("\\/", "/")));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_dirs_add_xdg_folder_only_when_present() {
        let home = std::env::temp_dir().join(format!("cmdmeow-logdirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(log_dirs_in(&home), vec![home.join(".claude/projects")]);
        std::fs::create_dir_all(home.join(".config/claude/projects")).unwrap();
        assert_eq!(log_dirs_in(&home), vec![home.join(".claude/projects"), home.join(".config/claude/projects")]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn availability_follows_switch_and_sign_in() {
        set_tool_on(false);
        assert_eq!(availability(), Availability::Off);
        set_tool_on(true);
        set_signed_in(Some(false));
        let a = availability();
        assert!(a == Availability::SignedOut || a == Availability::NotInstalled);
        set_signed_in(None);
        assert_ne!(availability(), Availability::SignedOut);
        assert!(Availability::Ready.how().is_empty());
        assert!(!Availability::Off.how().is_empty());
    }

    #[test]
    fn project_folder_names() {
        assert_eq!(encode("/Users/me/my.app"), "-Users-me-my-app");
        assert_eq!(encode("/Users/me/проект"), "-Users-me-------");
    }
}
