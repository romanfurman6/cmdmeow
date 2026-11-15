//! The Mac behaves like a cat. It purrs while a hand lies on the trackpad,
//! hisses when slapped, meows when the battery is low, eats when put on the
//! charger and says hello when the lid opens. The menu bar icon is its
//! face; a big cat shows in the middle of the screen with each reaction.
//!
//! - `face`: what the cat looks like in each mood, as plain drawing.
//! - `engine`: which mood it is in and what it does next, as plain rules.
//! - `mac`: the menu bar icon, the big cat, the sounds and what the Mac
//!   feels (trackpad, power, wake, lid, pointer).
//! - `slap`: which jolts of the body are slaps, and the sensor that feels
//!   them.
//!
//! Nothing here draws with egui or asks it for frames: the cat has its own
//! thread, which sleeps until something moves.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod engine;
pub mod face;
#[cfg(target_os = "macos")]
mod mac;
mod slap;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Where the switches are kept in the app's storage.
pub const STORAGE_KEY: &str = "cat";

/// What the cat does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Purr,
    Hiss,
    Hungry,
    Eat,
    Hello,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Purr, Kind::Hiss, Kind::Hungry, Kind::Eat, Kind::Hello];

    /// Its name in links (`cmdmeow://cat/purr`) and in what is kept.
    pub fn slug(self) -> &'static str {
        match self {
            Kind::Purr => "purr",
            Kind::Hiss => "hiss",
            Kind::Hungry => "hungry",
            Kind::Eat => "eat",
            Kind::Hello => "hello",
        }
    }

    pub fn from_slug(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.slug() == s)
    }

    fn mood(self) -> face::Mood {
        match self {
            Kind::Purr => face::Mood::Purr,
            Kind::Hiss => face::Mood::Hiss,
            Kind::Hungry => face::Mood::Hungry,
            Kind::Eat => face::Mood::Eat,
            Kind::Hello => face::Mood::Hello,
        }
    }

    /// How long it lasts, in seconds (the purr: when asked for by a link;
    /// under a hand it lasts as long as the hand stays).
    fn secs(self) -> f64 {
        match self {
            Kind::Purr => 3.2,
            Kind::Hiss => 1.5,
            Kind::Hungry => 2.0,
            Kind::Eat => 2.6,
            Kind::Hello => 3.6,
        }
    }

    /// Its sounds: the files' name and how many there are
    /// (`purr-1.mp3` … `purr-3.mp3`).
    fn sounds(self) -> (&'static str, u8) {
        match self {
            Kind::Purr => ("purr", 3),
            Kind::Hiss => ("slap", 3),
            Kind::Hungry => ("meow", 1),
            Kind::Eat => ("charger", 3),
            Kind::Hello => ("hello", 3),
        }
    }
}

/// When the big cat shows on screen.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Show {
    Always,
    /// The first time each reaction happens.
    #[default]
    FirstTime,
    Off,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Prefs {
    pub purr: bool,
    pub hiss: bool,
    pub meow: bool,
    pub eat: bool,
    pub greet: bool,
    /// The eyes follow the pointer.
    pub watch: bool,
    pub show: Show,
    /// Reactions the big cat has shown already (their slugs).
    pub seen: Vec<String>,
}

/// The cat starts switched off: each reaction is turned on in Settings.
impl Default for Prefs {
    fn default() -> Self {
        Prefs { purr: false, hiss: false, meow: false, eat: false, greet: false, watch: false, show: Show::FirstTime, seen: Vec::new() }
    }
}

impl Prefs {
    /// Every reaction on (the pointer is not one): what a demo shows.
    pub fn all_on() -> Prefs {
        Prefs { purr: true, hiss: true, meow: true, eat: true, greet: true, ..Prefs::default() }
    }

    /// Something is switched on. With everything off the cat is not there:
    /// the menu bar icon is the plain mark and links do nothing.
    pub fn awake(&self) -> bool {
        self.purr || self.hiss || self.meow || self.eat || self.greet || self.watch
    }
}

static PREFS: Mutex<Option<Prefs>> = Mutex::new(None);
/// The switches changed since they were last saved.
static UNSAVED: AtomicBool = AtomicBool::new(false);

pub fn prefs() -> Prefs {
    PREFS.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// The switches as Settings left them.
pub fn set_prefs(p: Prefs) {
    if keep(p.clone()) {
        send(engine::Event::Prefs(p));
    }
}

/// Remember `p`; says whether it differs from what was kept.
fn keep(p: Prefs) -> bool {
    let mut slot = PREFS.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref() == Some(&p) {
        return false;
    }
    *slot = Some(p);
    UNSAVED.store(true, Ordering::Relaxed);
    true
}

/// Whether the switches changed since the last call (the shell saves them then).
pub fn take_unsaved() -> bool {
    UNSAVED.swap(false, Ordering::Relaxed)
}

/// Make the cat do `kind` now, as if the Mac had felt it.
pub fn simulate(kind: Kind) {
    send(engine::Event::Sim(kind));
}

/// The app quits. Call on the way out.
pub fn quit() {
    #[cfg(target_os = "macos")]
    mac::quit();
}

fn send(ev: engine::Event) {
    #[cfg(target_os = "macos")]
    mac::send(ev);
    #[cfg(not(target_os = "macos"))]
    let _ = ev;
}

/// Bring the cat to life in the menu bar icon `status_item` (an
/// `NSStatusItem`), with the switches as they were saved. Call once, on
/// the main thread. Links `cmdmeow://cat/purr` (hiss, hungry, eat, hello)
/// make it react.
pub fn start(saved: Option<Prefs>, status_item: *mut std::ffi::c_void) {
    let p = saved.unwrap_or_default();
    *PREFS.lock().unwrap_or_else(|e| e.into_inner()) = Some(p.clone());
    crate::kit::url_scheme::route("cat", |action| Kind::from_slug(action).map(simulate).is_some());
    #[cfg(target_os = "macos")]
    mac::start(p, status_item);
    #[cfg(not(target_os = "macos"))]
    let _ = (p, status_item);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cat_starts_switched_off() {
        let p = Prefs::default();
        assert!(!(p.purr || p.hiss || p.meow || p.eat || p.greet || p.watch));
        assert!(!p.awake());
        assert_eq!(p.show, Show::FirstTime);
        let p = Prefs::all_on();
        assert!(p.purr && p.hiss && p.meow && p.eat && p.greet && !p.watch && p.awake());
        assert!(Prefs { watch: true, ..Prefs::default() }.awake());
    }

    #[test]
    fn older_saves_keep_working() {
        // a save from before a switch existed: the missing ones take their defaults
        let p: Prefs = serde_json::from_str(r#"{"purr":true,"show":"Always"}"#).unwrap();
        assert!(p.purr && !p.hiss && !p.watch);
        assert_eq!(p.show, Show::Always);
    }

    #[test]
    fn links_name_every_reaction() {
        for k in Kind::ALL {
            assert_eq!(Kind::from_slug(k.slug()), Some(k));
        }
        assert_eq!(Kind::from_slug("bark"), None);
    }
}
