//! Cleanup's catalog: the fixed places to look, the caches never to touch and
//! the project types whose build output can go. Kept as data (rules.json, built
//! into the app) so adding an app or a tool is a one-line change with a test.

use super::engine::Verdict;
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Catalog {
    pub keep_caches: Vec<KeepCache>,
    pub app_caches: Vec<Place>,
    pub developer: Vec<Place>,
    pub project_roots: Vec<String>,
    pub projects: Vec<ProjectType>,
    pub installers: Installers,
}

/// A fixed place under the home folder.
#[derive(Deserialize)]
pub struct Place {
    pub path: String,
    pub title: String,
    verdict: String,
    pub reason: String,
}

impl Place {
    pub fn verdict(&self) -> Verdict {
        parse_verdict(&self.verdict)
    }
}

/// A folder in ~/Library/Caches that stays, whatever Claude thinks.
#[derive(Deserialize)]
pub struct KeepCache {
    name: String,
    #[serde(rename = "match")]
    how: String,
    pub reason: String,
}

impl KeepCache {
    pub fn matches(&self, folder: &str) -> bool {
        let (f, n) = (folder.to_lowercase(), self.name.to_lowercase());
        match self.how.as_str() {
            "prefix" => f.starts_with(&n),
            "contains" => f.contains(&n),
            _ => f == n,
        }
    }
}

/// A kind of code project: files that mark it, and the folders its tools
/// rebuild. `careful` folders are build output too, but sometimes kept in the
/// repository or slow to get back, so they always wait for the person's OK.
#[derive(Deserialize)]
pub struct ProjectType {
    pub name: String,
    pub markers: Vec<String>,
    pub dirs: Vec<String>,
    #[serde(default)]
    pub careful: Vec<String>,
}

impl ProjectType {
    /// `dir` holds one of the marker files ("*.csproj" = any file ending so).
    pub fn is_in(&self, dir: &Path, names: &[String]) -> bool {
        self.markers.iter().any(|m| match m.strip_prefix('*') {
            Some(suffix) => names.iter().any(|n| n.ends_with(suffix) && n.len() > suffix.len()),
            None if m.contains('/') => dir.join(m).is_file(),
            None => names.iter().any(|n| n == m),
        })
    }
    pub fn all_dirs(&self) -> impl Iterator<Item = (&str, bool)> {
        self.dirs.iter().map(|d| (d.as_str(), false)).chain(self.careful.iter().map(|d| (d.as_str(), true)))
    }
}

#[derive(Deserialize)]
pub struct Installers {
    pub extensions: Vec<String>,
    pub min_age_days: u64,
}

fn parse_verdict(s: &str) -> Verdict {
    match s {
        "safe" => Verdict::Safe,
        "keep" => Verdict::Keep,
        _ => Verdict::Ask,
    }
}

pub fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("rules.json")).expect("cleanup/rules.json is valid (checked by a test)"))
}

/// Every folder name any project type rebuilds.
pub fn artifact_names() -> Vec<&'static str> {
    let mut v: Vec<&str> = catalog().projects.iter().flat_map(|p| p.all_dirs().map(|(d, _)| d)).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_valid() {
        let c = catalog();
        assert!(!c.app_caches.is_empty() && !c.developer.is_empty() && !c.projects.is_empty());
        for p in c.app_caches.iter().chain(c.developer.iter()) {
            assert!(!p.path.starts_with('/') && !p.path.contains(".."), "{} must be relative to home", p.path);
            assert!(["safe", "ask", "keep"].contains(&p.verdict.as_str()), "{}: verdict {}", p.title, p.verdict);
            assert!(!p.title.is_empty() && !p.reason.is_empty());
            // personal folders are never offered
            assert!(crate::kit::fs::is_protected(&crate::kit::fs::home().join(&p.path)).is_none(), "{} is protected", p.path);
        }
        for k in &c.keep_caches {
            assert!(["exact", "prefix", "contains"].contains(&k.how.as_str()), "{}", k.name);
        }
        for t in &c.projects {
            assert!(!t.markers.is_empty(), "{}", t.name);
            assert!(t.dirs.len() + t.careful.len() > 0, "{}", t.name);
            for (d, _) in t.all_dirs() {
                assert!(!d.contains('/') && d != "." && d != ".." && d != ".git", "{}: {d}", t.name);
            }
        }
        assert!(c.installers.min_age_days >= 1);
    }

    #[test]
    fn keep_rules_match() {
        let c = catalog();
        let kept = |n: &str| c.keep_caches.iter().any(|k| k.matches(n));
        assert!(kept("com.apple.iconservices.store"));
        assert!(kept("com.apple.LaunchServices-1234"));
        assert!(kept("com.1password.1password"));
        assert!(kept("com.tailscale.ipn.macos"));
        assert!(!kept("com.apple.Music"));
        assert!(!kept("com.spotify.client"));
        // exact names only match exactly
        assert!(!kept("CloudKitStuff"));
    }

    #[test]
    fn project_markers() {
        let c = catalog();
        let node = c.projects.iter().find(|p| p.name == "Node").unwrap();
        let dotnet = c.projects.iter().find(|p| p.name == ".NET").unwrap();
        let p = Path::new("/nowhere");
        assert!(node.is_in(p, &["package.json".into(), "README.md".into()]));
        assert!(!node.is_in(p, &["package.json.bak".into()]));
        assert!(dotnet.is_in(p, &["App.csproj".into()]));
        assert!(!dotnet.is_in(p, &[".csproj".into()]));
        assert!(artifact_names().contains(&"node_modules"));
    }
}
