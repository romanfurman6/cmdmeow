//! Known exceptions, as data: files some apps keep where name and id
//! matching can't see them (a vendor folder named after the company, a
//! dot-folder in the home folder), and names that must never be offered for
//! an app. A rule applies to an app whose bundle id is the rule's id or
//! below it.

use super::leftovers::{kind_of, tidy, under_id, Item, Kind, Owner, Roots};
use crate::kit::fs::is_protected;
use std::path::PathBuf;

pub struct Rule {
    pub id: &'static str,
    /// "~/…" is the home folder, "/…" the system root.
    pub include: &'static [&'static str],
    /// File names (any case) never offered for the app.
    pub exclude: &'static [&'static str],
}

pub const RULES: &[Rule] = &[
    Rule { id: "com.google.Chrome", include: &["~/Library/Application Support/Google/Chrome", "~/Library/Caches/Google/Chrome"], exclude: &[] },
    Rule { id: "com.google.Chrome.canary", include: &["~/Library/Application Support/Google/Chrome Canary", "~/Library/Caches/Google/Chrome Canary"], exclude: &[] },
    Rule { id: "com.brave.Browser", include: &["~/Library/Application Support/BraveSoftware/Brave-Browser", "~/Library/Caches/BraveSoftware/Brave-Browser"], exclude: &[] },
    Rule { id: "com.microsoft.VSCode", include: &["~/Library/Application Support/Code", "~/.vscode"], exclude: &[] },
    Rule { id: "com.todesktop.230313mzl4w4u92", include: &["~/.cursor"], exclude: &[] },
    Rule { id: "com.docker.docker", include: &["~/.docker"], exclude: &[] },
    Rule { id: "com.github.GitHubClient", include: &["~/Library/Application Support/GitHub Desktop"], exclude: &[] },
    // Steam's folder holds every installed game: never take it with the client
    Rule { id: "com.valvesoftware.steam", include: &[], exclude: &["Steam"] },
];

/// Apply the rules of `table` that fit `app` to `items`.
pub fn apply(table: &[Rule], app: &Owner, items: &mut Vec<Item>, roots: &Roots) {
    let id = app.id.to_lowercase();
    if id.is_empty() {
        return;
    }
    for r in table.iter().filter(|r| under_id(&id, &r.id.to_lowercase())) {
        items.retain(|it| it.kind == Kind::App || !it.path.file_name().is_some_and(|n| r.exclude.iter().any(|x| n.to_string_lossy().eq_ignore_ascii_case(x))));
        for inc in r.include {
            let p = expand(inc, roots);
            if p.symlink_metadata().is_err() || is_protected(&p).is_some() || items.iter().any(|i| i.path == p) {
                continue;
            }
            let admin = !p.starts_with(roots.home()) && super::remove::needs_root(&p);
            items.push(Item { path: p.clone(), kind: kind_of(&p, roots), bytes: 0, admin, selected: true });
        }
    }
    tidy(items);
}

/// "~/x" under the home folder, "/x" under the system root.
pub fn expand(p: &str, roots: &Roots) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        roots.home().join(rest)
    } else {
        roots.system.join(p.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::leftovers::tests::Fake;

    #[test]
    fn includes_and_excludes() {
        let f = Fake::new("rules");
        f.lib("Application Support/Google/Chrome/Default/x");
        f.file("home/.docker/config.json", "{}");
        f.lib("Application Support/Steam/steamapps/x");
        let table = [
            Rule { id: "com.google.Chrome", include: &["~/Library/Application Support/Google/Chrome", "~/Library/Caches/Google/Chrome"], exclude: &[] },
            Rule { id: "com.docker", include: &["~/.docker"], exclude: &[] },
            Rule { id: "com.valvesoftware.steam", include: &[], exclude: &["Steam"] },
        ];
        let chrome = Owner { id: "com.google.Chrome".into(), name: "Google Chrome".into(), ..Default::default() };
        let mut items = Vec::new();
        apply(&table, &chrome, &mut items, &f.roots);
        assert_eq!(items.len(), 1, "{items:?}");
        assert_eq!(items[0].kind, Kind::Support);
        let docker = Owner { id: "com.docker.docker".into(), ..Default::default() };
        let mut items = Vec::new();
        apply(&table, &docker, &mut items, &f.roots);
        assert_eq!(items.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), vec![f.roots.home().join(".docker")]);
        let steam = Owner { id: "com.valvesoftware.steam".into(), ..Default::default() };
        let mut items = vec![Item { path: f.roots.user_library.join("Application Support/Steam"), kind: Kind::Support, bytes: 0, admin: false, selected: true }];
        apply(&table, &steam, &mut items, &f.roots);
        assert!(items.is_empty());
        // nothing for an app no rule names
        let other = Owner { id: "com.googlex.Chrome".into(), ..Default::default() };
        let mut items = Vec::new();
        apply(&table, &other, &mut items, &f.roots);
        assert!(items.is_empty());
    }
}
