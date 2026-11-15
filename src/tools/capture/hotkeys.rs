//! Capture's system-wide shortcuts, on the app's shared hotkey hub.

use super::settings::Command;

pub type Keys = crate::kit::hotkeys::Keys<Command>;

#[cfg(test)]
mod tests {
    use crate::kit::hotkeys::to_hotkey;

    #[test]
    fn shortcuts_map_to_distinct_hotkeys() {
        let s = super::super::settings::Settings::default();
        let mut ids: Vec<u32> = s.bound().iter().map(|(_, k)| to_hotkey(k).expect("default maps").id()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn every_command_has_its_own_link() {
        use super::super::settings::Command;
        for c in Command::ALL {
            assert_eq!(Command::from_slug(c.slug()), Some(c));
        }
        assert_eq!(Command::from_slug("nope"), None);
    }
}
