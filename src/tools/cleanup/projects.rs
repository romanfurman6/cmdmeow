//! Build output of code projects (node_modules, target, .build…) in the usual
//! code folders, offered only for projects nobody has touched for a while.
//! The project itself, its sources and its .git are never offered.

use super::engine::{Ctx, Group, Verdict};
use super::rules::{catalog, ProjectType};
use crate::kit::fs::home;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How deep below a code folder projects are looked for.
const MAX_DEPTH: usize = 6;
/// Untouched for less than this: in use, not offered.
pub const RECENT_DAYS: f64 = 7.0;
/// Untouched for this long: plain build output is safe to remove.
pub const IDLE_DAYS: f64 = 30.0;
/// Files looked at to tell when a project was last touched.
const ACTIVITY_FILES: usize = 4000;

#[derive(Debug, PartialEq)]
pub struct Found {
    pub project: PathBuf,
    pub kind: String,
    pub dir: PathBuf,
    pub careful: bool,
    pub idle_days: f64,
}

pub(super) fn scan(c: &mut Ctx) {
    let h = home();
    let mut roots: Vec<PathBuf> = catalog().project_roots.iter().map(|r| h.join(r)).filter(|p| p.is_dir() && !is_link(p)).collect();
    // a root inside another root is walked once
    roots.sort();
    roots.dedup_by(|b, a| b.starts_with(&*a));
    for root in roots {
        if c.stopped() {
            return;
        }
        for f in find(&root, &|| c.stopped()) {
            let Some((v, why)) = verdict(&f) else { continue };
            let name = f.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = f.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            c.add(Group::Projects, &format!("{name} · {dir}"), f.dir, "Build Data", v, &why);
        }
    }
}

/// What a found folder is offered as, or `None` while its project is in use.
pub fn verdict(f: &Found) -> Option<(Verdict, String)> {
    if f.idle_days < RECENT_DAYS {
        return None;
    }
    let days = f.idle_days as u64;
    Some(if f.careful {
        (Verdict::Ask, format!("{} build output, idle {days} days, may be kept on purpose", f.kind))
    } else if f.idle_days >= IDLE_DAYS {
        (Verdict::Safe, format!("{} build output, idle {days} days, rebuilds itself", f.kind))
    } else {
        (Verdict::Ask, format!("{} build output, used {days} days ago", f.kind))
    })
}

fn is_link(p: &Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
}

fn names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
}

/// Every project build folder below `root`.
pub fn find(root: &Path, stop: &dyn Fn() -> bool) -> Vec<Found> {
    let artifacts = super::rules::artifact_names();
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if stop() {
            break;
        }
        let ns = names(&dir);
        let mut skip: Vec<String> = Vec::new();
        let types: Vec<&ProjectType> = catalog().projects.iter().filter(|t| t.is_in(&dir, &ns)).collect();
        if !types.is_empty() {
            let mut here: Vec<(PathBuf, String, bool)> = Vec::new();
            for t in &types {
                for (d, careful) in t.all_dirs() {
                    let p = dir.join(d);
                    if is_artifact_dir(&p) && !here.iter().any(|(q, ..)| q == &p) {
                        here.push((p, t.name.clone(), careful));
                        skip.push(d.to_string());
                    }
                }
            }
            if !here.is_empty() {
                let idle = idle_days(&dir, &artifacts);
                for (p, kind, careful) in here {
                    // a folder one type rebuilds but another treats with care stays careful
                    let careful = careful || types.iter().any(|t| t.careful.iter().any(|c| dir.join(c) == p));
                    out.push(Found { project: dir.clone(), kind, dir: p, careful, idle_days: idle });
                }
            }
        }
        if depth + 1 >= MAX_DEPTH {
            continue;
        }
        for n in ns {
            // hidden folders, build output and app bundles hold no projects worth walking
            if n.starts_with('.') || skip.contains(&n) || artifacts.contains(&n.as_str()) || n.ends_with(".app") || n == "Library" {
                continue;
            }
            let p = dir.join(&n);
            if p.symlink_metadata().is_ok_and(|m| m.is_dir()) {
                stack.push((p, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// A real folder (not a link), with no repository of its own inside.
fn is_artifact_dir(p: &Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.is_dir()) && !p.join(".git").exists()
}

/// The project's guard at removal time: still a build folder of a project.
pub fn still_build_output(p: &Path) -> Option<&'static str> {
    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else { return Some("Not a build folder") };
    let name = name.to_string_lossy();
    if !is_artifact_dir(p) {
        return Some("Not a plain build folder any more");
    }
    let ns = names(parent);
    let known = catalog().projects.iter().any(|t| t.is_in(parent, &ns) && t.all_dirs().any(|(d, _)| d == name));
    (!known).then_some("Its project files are gone; not sure it's build output")
}

/// Days since anything in the project (outside its build output) changed.
pub fn idle_days(project: &Path, artifacts: &[&str]) -> f64 {
    let mut newest = project.symlink_metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    // git touches its index on every commit, checkout and add
    for f in [".git/index", ".git/HEAD", ".git/FETCH_HEAD"] {
        if let Ok(t) = project.join(f).symlink_metadata().and_then(|m| m.modified()) {
            newest = newest.max(t);
        }
    }
    let mut seen = 0usize;
    let mut stack = vec![project.to_path_buf()];
    'walk: while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > ACTIVITY_FILES {
                break 'walk;
            }
            let n = e.file_name().to_string_lossy().into_owned();
            let Ok(md) = e.path().symlink_metadata() else { continue };
            if md.is_dir() {
                if n != ".git" && !artifacts.contains(&n.as_str()) {
                    stack.push(e.path());
                }
            } else if let Ok(t) = md.modified() {
                newest = newest.max(t);
            }
        }
    }
    SystemTime::now().duration_since(newest).map(|d| d.as_secs_f64() / 86_400.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn age(p: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 86_400);
        let f = std::fs::File::open(p).unwrap();
        f.set_modified(t).unwrap();
    }

    fn tree(base: &Path) {
        let _ = std::fs::remove_dir_all(base);
        // an old Node app, a fresh Rust crate, a PHP app with vendor, and a
        // plain folder with node_modules but no package.json
        for (d, f) in [("old-web", "package.json"), ("old-web/src", "index.js"), ("fresh/sub", "Cargo.toml"), ("php", "composer.json"), ("loose", "notes.txt")] {
            std::fs::create_dir_all(base.join(d)).unwrap();
            std::fs::write(base.join(d).join(f), "x").unwrap();
        }
        for d in ["old-web/node_modules/left-pad", "old-web/.next", "fresh/sub/target/debug", "php/vendor", "loose/node_modules"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("old-web/node_modules/left-pad/index.js"), "x").unwrap();
        for p in ["old-web/package.json", "old-web/src/index.js", "old-web/src", "old-web", "php/composer.json", "php"] {
            age(&base.join(p), 60);
        }
        // a vendored repository inside a build folder name stays
        std::fs::create_dir_all(base.join("php/vendor/.git")).unwrap();
    }

    #[test]
    fn finds_build_output_of_projects_only() {
        let base = std::env::temp_dir().join(format!("cmdmeow-proj-{}", std::process::id()));
        tree(&base);
        let f = find(&base, &|| false);
        let rel: Vec<String> = f.iter().map(|x| x.dir.strip_prefix(&base).unwrap().display().to_string()).collect();
        assert_eq!(rel, vec!["fresh/sub/target", "old-web/.next", "old-web/node_modules"], "{f:?}");
        let web = f.iter().find(|x| x.dir.ends_with("node_modules")).unwrap();
        assert!(web.idle_days > 50.0, "build output doesn't count as activity: {}", web.idle_days);
        assert!(!web.careful);
        let fresh = f.iter().find(|x| x.dir.ends_with("target")).unwrap();
        assert!(fresh.idle_days < 1.0);
        // the fresh one isn't offered, the old one is safe
        assert_eq!(verdict(fresh), None);
        assert_eq!(verdict(web).unwrap().0, Verdict::Safe);
        // at removal time it is still checked to be build output
        assert_eq!(still_build_output(&web.dir), None);
        assert!(still_build_output(&base.join("loose/node_modules")).is_some());
        assert!(still_build_output(&base.join("php/vendor")).is_some());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn careful_folders_always_ask() {
        let f = Found { project: "/p".into(), kind: "PHP".into(), dir: "/p/vendor".into(), careful: true, idle_days: 400.0 };
        assert_eq!(verdict(&f).unwrap().0, Verdict::Ask);
        let f = Found { project: "/p".into(), kind: "Node".into(), dir: "/p/node_modules".into(), careful: false, idle_days: 10.0 };
        assert_eq!(verdict(&f).unwrap().0, Verdict::Ask);
    }

    #[test]
    fn stops_when_asked() {
        let base = std::env::temp_dir().join(format!("cmdmeow-proj-stop-{}", std::process::id()));
        tree(&base);
        assert!(find(&base, &|| true).is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
