//! Password managers and other secret keepers that Clipboard never records
//! from by default, but only the ones this Mac actually has.

use crate::kit::bundles as apps;
use std::path::PathBuf;

/// One app in the catalog: bundle ids it ships under (current and older
/// versions, App Store and direct builds) and the file names it installs as,
/// for builds whose id we don't know.
pub struct Known {
    /// The app's usual name (tests and logs).
    #[allow(dead_code)]
    pub name: &'static str,
    pub ids: &'static [&'static str],
    pub files: &'static [&'static str],
}

pub const CATALOG: &[Known] = &[
    Known {
        name: "1Password",
        ids: &["com.1password.1password", "com.agilebits.onepassword7", "com.agilebits.onepassword4", "com.agilebits.onepassword-osx", "com.agilebits.onepassword"],
        files: &["1Password", "1Password 7", "1Password 8"],
    },
    Known { name: "Bitwarden", ids: &["com.bitwarden.desktop"], files: &["Bitwarden"] },
    Known { name: "Dashlane", ids: &["com.dashlane.dashlane", "com.dashlane.dashlanephonefinal"], files: &["Dashlane"] },
    Known { name: "LastPass", ids: &["com.lastpass.lastpass", "com.lastpass.lastpassmacdesktop"], files: &["LastPass"] },
    Known { name: "KeePassXC", ids: &["org.keepassxc.keepassxc"], files: &["KeePassXC"] },
    Known { name: "KeePassX", ids: &["org.keepassx.keepassx"], files: &["KeePassX"] },
    Known { name: "KeePassium", ids: &["com.keepassium.ios", "com.keepassium.pro"], files: &["KeePassium", "KeePassium Pro"] },
    Known { name: "KeeWeb", ids: &["net.antelle.keeweb"], files: &["KeeWeb"] },
    Known { name: "Keeper", ids: &["com.keepersecurity.passwordmanager", "com.callpod.keepermac.lite"], files: &["Keeper Password Manager", "Keeper"] },
    Known { name: "Enpass", ids: &["in.sinew.enpass-desktop", "in.sinew.enpass-desktop.app"], files: &["Enpass"] },
    Known { name: "NordPass", ids: &["com.nordsec.nordpass"], files: &["NordPass"] },
    Known { name: "Proton Pass", ids: &["me.proton.pass.electron", "me.proton.pass"], files: &["Proton Pass"] },
    Known { name: "RoboForm", ids: &["com.sibersystems.roboform", "com.sibersystems.roboformmac"], files: &["RoboForm"] },
    Known { name: "Strongbox", ids: &["com.markmcguill.strongbox.mac", "com.markmcguill.strongbox.mac.pro"], files: &["Strongbox", "Strongbox Pro"] },
    Known { name: "MacPass", ids: &["com.hicknhacksoftware.macpass"], files: &["MacPass"] },
    Known { name: "Secrets", ids: &["com.outercorner.secrets-macos", "com.outercorner.secrets"], files: &["Secrets"] },
    Known { name: "Buttercup", ids: &["pw.buttercup.desktop"], files: &["Buttercup"] },
    Known { name: "Padloc", ids: &["app.padloc"], files: &["Padloc"] },
    Known { name: "Passwords", ids: &["com.apple.passwords"], files: &["Passwords"] },
    Known { name: "Keychain Access", ids: &["com.apple.keychainaccess"], files: &["Keychain Access"] },
];

/// The defaults older builds wrote into every settings file, installed or not.
pub const OLD_DEFAULTS: &[&str] = &["com.1password.1password", "com.agilebits.onepassword7", "com.apple.keychainaccess", "com.apple.Passwords", "com.bitwarden.desktop"];

/// An installed app.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub bundle: String,
    /// What Finder shows (the bundle's file name).
    pub name: String,
    pub path: PathBuf,
}

/// The catalog entry an installed app belongs to, if any.
pub fn known(bundle: &str, file_name: &str) -> Option<&'static Known> {
    let id = bundle.to_lowercase();
    CATALOG
        .iter()
        .find(|k| k.ids.contains(&id.as_str()))
        .or_else(|| CATALOG.iter().find(|k| k.files.iter().any(|f| f.eq_ignore_ascii_case(file_name))))
}

/// Of `installed`, the password managers.
pub fn password_managers(installed: &[Found]) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for f in installed {
        if known(&f.bundle, &f.name).is_some() && !out.iter().any(|o| o.bundle.eq_ignore_ascii_case(&f.bundle)) {
            out.push(f.clone());
        }
    }
    out
}

/// Where apps live, Apple's own included (Passwords, Keychain Access).
fn roots() -> Vec<PathBuf> {
    let mut r = apps::app_roots();
    r.push(PathBuf::from("/System/Applications"));
    r.push(PathBuf::from("/System/Library/CoreServices/Applications"));
    r
}

/// Every installed app, read from its Info.plist. Slow-ish: run on a thread.
pub fn scan() -> Vec<Found> {
    apps::find_bundles(&roots())
        .into_iter()
        .filter_map(|path| {
            let info = apps::read_info(&path)?;
            Some(Found { bundle: info.id, name: info.name, path })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(bundle: &str, name: &str) -> Found {
        Found { bundle: bundle.into(), name: name.into(), path: PathBuf::from(format!("/Applications/{name}.app")) }
    }

    #[test]
    fn matches_by_id_any_case() {
        assert_eq!(known("com.bitwarden.desktop", "Whatever").map(|k| k.name), Some("Bitwarden"));
        assert_eq!(known("com.apple.Passwords", "Passwords").map(|k| k.name), Some("Passwords"));
        assert_eq!(known("in.sinew.Enpass-Desktop", "Enpass").map(|k| k.name), Some("Enpass"));
    }

    #[test]
    fn falls_back_to_file_name() {
        assert_eq!(known("com.example.unknown", "Dashlane").map(|k| k.name), Some("Dashlane"));
        assert!(known("com.apple.Safari", "Safari").is_none());
    }

    #[test]
    fn picks_only_managers_once() {
        let all = [f("com.apple.Safari", "Safari"), f("com.1password.1password", "1Password"), f("com.1password.1password", "1Password"), f("org.keepassxc.keepassxc", "KeePassXC")];
        let pm = password_managers(&all);
        assert_eq!(pm.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["1Password", "KeePassXC"]);
    }

    #[test]
    fn catalog_ids_are_lowercase() {
        for k in CATALOG {
            for id in k.ids {
                assert_eq!(*id, id.to_lowercase(), "{}", k.name);
            }
        }
    }
}
