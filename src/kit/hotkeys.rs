//! System-wide shortcuts shared by every tool. The shortcut library has one
//! event handler and one manager per app, so they live here: each tool owns a
//! [`Keys`] with its own commands, and a press goes to the tool that bound it.
//! Each press wakes the app (`request_repaint`); tools pick their commands up
//! in their `tick`. Extra mouse buttons (middle, side) can be shortcuts too:
//! the shortcut library only knows keys, so [`super::mouse_keys`] watches
//! those buttons itself and [`Keys`] reads both.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

/// A key combination. `key` is egui's key name ("2", "A", "F5"), or
/// "Mouse<n>" for mouse button n (3 middle, 4 and 5 the side buttons, as
/// macOS numbers them).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub key: String,
}

impl Shortcut {
    /// The macOS way of writing it: ⌃⌥⇧⌘ then the key.
    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('⌃');
        }
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        if self.cmd {
            s.push('⌘');
        }
        match self.mouse_button() {
            Some(n) => s.push_str(&mouse_label(n)),
            None => s.push_str(&self.key),
        }
        s
    }

    /// The mouse button this shortcut is (3 and up), if it is one.
    pub fn mouse_button(&self) -> Option<u8> {
        self.key.strip_prefix("Mouse")?.parse::<u8>().ok().filter(|n| (3..=32).contains(n))
    }

    /// Mouse button `n` (3 and up) with these modifiers.
    pub fn mouse(n: u8, cmd: bool, shift: bool, alt: bool, ctrl: bool) -> Option<Shortcut> {
        (3..=32).contains(&n).then(|| Shortcut { cmd, shift, alt, ctrl, key: format!("Mouse{n}") })
    }

    /// From a mouse button pressed in our own window (the shortcut recorder).
    /// Only the extra buttons: left and right clicks stay clicks. No modifier
    /// needed, a side button alone is a fine shortcut.
    pub fn from_pointer(button: egui::PointerButton, m: egui::Modifiers) -> Option<Shortcut> {
        let n = match button {
            egui::PointerButton::Middle => 3,
            egui::PointerButton::Extra1 => 4,
            egui::PointerButton::Extra2 => 5,
            _ => return None,
        };
        Shortcut::mouse(n, m.mac_cmd || m.command, m.shift, m.alt, m.ctrl && !m.command)
    }

    pub fn has_modifier(&self) -> bool {
        self.cmd || self.ctrl || self.alt
    }

    /// From a key press in our own window (the shortcut recorder).
    pub fn from_press(key: egui::Key, m: egui::Modifiers) -> Option<Shortcut> {
        let s = Shortcut { cmd: m.mac_cmd || m.command, shift: m.shift, alt: m.alt, ctrl: m.ctrl && !m.command, key: key.name().to_string() };
        (s.has_modifier() && key_code(&s.key).is_some()).then_some(s)
    }

    /// Why this shortcut is a bad idea, if it is.
    pub fn conflict(&self) -> Option<&'static str> {
        let mac = self.cmd && self.shift && !self.alt && !self.ctrl;
        match self.key.as_str() {
            "3" | "4" | "5" if mac => Some("macOS uses this for its own screenshots"),
            "Q" | "W" | "C" | "V" | "X" | "Z" | "A" | "S" | "Tab" if self.cmd && !self.shift && !self.alt && !self.ctrl => Some("apps use this; pick another"),
            _ => None,
        }
    }
}

/// How a mouse button reads in a shortcut ("Mouse 4").
fn mouse_label(n: u8) -> String {
    // canvas variants, picked before this ships
    match std::env::var("CMDMEOW_MOUSE_LABEL").as_deref() {
        Ok("b") => format!("Button {n}"),
        Ok("c") => match n {
            3 => "Middle".into(),
            4 => "Back".into(),
            5 => "Forward".into(),
            _ => format!("Button {n}"),
        },
        _ => format!("Mouse {n}"),
    }
}

/// What a shortcut recorder heard this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    Nothing,
    /// Esc: keep the old one.
    Cancel,
    /// ⌫: no shortcut.
    Remove,
    Set(Shortcut),
}

/// Read this frame's key presses (and extra mouse buttons) for a shortcut
/// recorder in our own window. Call it every frame while recording: it also
/// tells the mouse watcher to hand presses to the recorder meanwhile.
pub fn listen(ctx: &egui::Context) -> Heard {
    super::mouse_keys::recording();
    let mut heard = ctx.input(|i| {
        let mut heard = Heard::Nothing;
        for e in &i.events {
            let next = match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let plain = !modifiers.command && !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd;
                    if *key == egui::Key::Escape && plain {
                        Heard::Cancel
                    } else if (*key == egui::Key::Backspace || *key == egui::Key::Delete) && plain {
                        Heard::Remove
                    } else if let Some(s) = Shortcut::from_press(*key, *modifiers) {
                        Heard::Set(s)
                    } else {
                        continue;
                    }
                }
                egui::Event::PointerButton { button, pressed: true, modifiers, .. } => match Shortcut::from_pointer(*button, *modifiers) {
                    Some(s) => Heard::Set(s),
                    None => continue,
                },
                _ => continue,
            };
            heard = next;
        }
        heard
    });
    // buttons egui doesn't name (6 and up), or pressed outside the window
    if let Some(s) = super::mouse_keys::take_heard() {
        if heard == Heard::Nothing {
            heard = Heard::Set(s);
        }
    }
    heard
}

/// The `keyboard-types` code name for an egui key name, as the global
/// shortcut library wants it. None for keys we don't offer.
pub fn key_code(key: &str) -> Option<String> {
    if key.len() == 1 {
        let c = key.chars().next()?;
        if c.is_ascii_digit() {
            return Some(format!("Digit{c}"));
        }
        if c.is_ascii_uppercase() {
            return Some(format!("Key{c}"));
        }
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=20).contains(&n) {
            return Some(format!("F{n}"));
        }
    }
    match key {
        "Space" => Some("Space".into()),
        "Minus" => Some("Minus".into()),
        "Equals" | "Plus" => Some("Equal".into()),
        "Comma" => Some("Comma".into()),
        "Period" => Some("Period".into()),
        "Slash" => Some("Slash".into()),
        "Semicolon" => Some("Semicolon".into()),
        "OpenBracket" => Some("BracketLeft".into()),
        "CloseBracket" => Some("BracketRight".into()),
        "Backtick" => Some("Backquote".into()),
        _ => None,
    }
}

/// The library's hotkey for one of our shortcuts.
pub fn to_hotkey(s: &Shortcut) -> Option<HotKey> {
    // Esc and Return are never offered to the recorder, but a tool may bind
    // them while something of its own is on screen (Dictation's cancel/done)
    let name = match s.key.as_str() {
        "Escape" => "Escape".to_string(),
        "Return" => "Enter".to_string(),
        k => key_code(k)?,
    };
    let code = Code::from_str(&name).ok()?;
    let mut m = Modifiers::empty();
    if s.cmd {
        m |= Modifiers::SUPER;
    }
    if s.shift {
        m |= Modifiers::SHIFT;
    }
    if s.alt {
        m |= Modifiers::ALT;
    }
    if s.ctrl {
        m |= Modifiers::CONTROL;
    }
    Some(HotKey::new(Some(m), code))
}

/// The app's one manager plus the presses not yet picked up by their owner.
struct Hub {
    mgr: GlobalHotKeyManager,
    rx: Receiver<GlobalHotKeyEvent>,
    /// Ids of pressed hotkeys, oldest first.
    pressed: Vec<u32>,
    /// Ids of released hotkeys, oldest first (only push-to-talk reads
    /// these, so the list is kept short).
    released: Vec<u32>,
}

/// Most releases kept for owners that never read them.
const RELEASES_KEPT: usize = 32;

/// Contexts woken on a press (one per tool that made a [`Keys`]).
static WAKE: Mutex<Vec<egui::Context>> = Mutex::new(Vec::new());

thread_local! {
    // the manager must stay on the main thread on macOS
    static HUB: RefCell<Option<Hub>> = const { RefCell::new(None) };
}

fn with_hub<T>(f: impl FnOnce(&mut Hub) -> T) -> Option<T> {
    HUB.with(|h| {
        let mut h = h.borrow_mut();
        if h.is_none() {
            let mgr = GlobalHotKeyManager::new().ok()?;
            let (tx, rx) = channel();
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                let _ = tx.send(e);
                wake();
            }));
            *h = Some(Hub { mgr, rx, pressed: Vec::new(), released: Vec::new() });
        }
        let hub = h.as_mut()?;
        while let Ok(e) = hub.rx.try_recv() {
            match e.state {
                HotKeyState::Pressed => hub.pressed.push(e.id),
                HotKeyState::Released => hub.released.push(e.id),
            }
        }
        if hub.released.len() > RELEASES_KEPT {
            let extra = hub.released.len() - RELEASES_KEPT;
            hub.released.drain(..extra);
        }
        Some(f(hub))
    })
}

/// One registered shortcut: a key through the library, or a mouse button
/// through [`super::mouse_keys`] (by its id there).
#[derive(Clone, Copy)]
enum Bind {
    Key(HotKey),
    Mouse(u32),
}

/// One tool's shortcuts. `C` is the tool's own command type.
pub struct Keys<C: Copy> {
    bound: Vec<(Bind, C)>,
}

impl<C: Copy> Keys<C> {
    pub fn new(ctx: egui::Context) -> Option<Keys<C>> {
        with_hub(|_| ())?;
        if let Ok(mut w) = WAKE.lock() {
            w.push(ctx);
        }
        Some(Keys { bound: Vec::new() })
    }

    /// Register exactly these shortcuts. Returns the ones the system refused
    /// (usually: another app, or another tool, holds them).
    pub fn apply(&mut self, wanted: &[(C, Shortcut)]) -> Vec<(C, String)> {
        self.clear();
        let mut failed = Vec::new();
        for (cmd, s) in wanted {
            if s.mouse_button().is_some() {
                match super::mouse_keys::register(s) {
                    Ok(id) => self.bound.push((Bind::Mouse(id), *cmd)),
                    Err(e) => failed.push((*cmd, format!("{} {e}", s.display()))),
                }
                continue;
            }
            let Some(hk) = to_hotkey(s) else {
                failed.push((*cmd, format!("{} can't be used as a shortcut", s.display())));
                continue;
            };
            match with_hub(|h| h.mgr.register(hk)) {
                Some(Ok(())) => self.bound.push((Bind::Key(hk), *cmd)),
                Some(Err(e)) => failed.push((*cmd, format!("{} is taken: {e}", s.display()))),
                None => failed.push((*cmd, "shortcuts are unavailable".into())),
            }
        }
        failed
    }

    /// Stop listening (while the user records a new shortcut, or the tool is off).
    pub fn clear(&mut self) {
        for (b, _) in self.bound.drain(..) {
            match b {
                Bind::Key(hk) => {
                    with_hub(|h| {
                        let _ = h.mgr.unregister(hk);
                        h.pressed.retain(|id| *id != hk.id());
                        h.released.retain(|id| *id != hk.id());
                    });
                }
                Bind::Mouse(id) => super::mouse_keys::unregister(id),
            }
        }
    }

    fn key_of(&self, id: u32) -> Option<C> {
        self.bound.iter().find_map(|(b, c)| matches!(b, Bind::Key(hk) if hk.id() == id).then_some(*c))
    }

    fn mouse_of(&self, id: u32) -> Option<C> {
        self.bound.iter().find_map(|(b, c)| matches!(b, Bind::Mouse(m) if *m == id).then_some(*c))
    }

    /// This tool's commands pressed since the last call. Presses of other
    /// tools' shortcuts stay for them.
    pub fn poll(&self) -> Vec<C> {
        let mut out = take(&self.bound, |h| &mut h.pressed, |id| self.key_of(id));
        if self.bound.iter().any(|(b, _)| matches!(b, Bind::Mouse(_))) {
            out.extend(super::mouse_keys::take_pressed(|id| self.mouse_of(id)));
        }
        out
    }

    /// This tool's commands whose keys were let go since the last call
    /// (push-to-talk).
    pub fn poll_released(&self) -> Vec<C> {
        let mut out = take(&self.bound, |h| &mut h.released, |id| self.key_of(id));
        if self.bound.iter().any(|(b, _)| matches!(b, Bind::Mouse(_))) {
            out.extend(super::mouse_keys::take_released(|id| self.mouse_of(id)));
        }
        out
    }
}

/// Take the ids `mine` knows out of one of the hub's lists, oldest first.
fn take<C: Copy>(bound: &[(Bind, C)], list: impl Fn(&mut Hub) -> &mut Vec<u32>, mine: impl Fn(u32) -> Option<C>) -> Vec<C> {
    if !bound.iter().any(|(b, _)| matches!(b, Bind::Key(_))) {
        return Vec::new();
    }
    with_hub(|h| {
        let mut out = Vec::new();
        list(h).retain(|id| match mine(*id) {
            Some(c) => {
                out.push(c);
                false
            }
            None => true,
        });
        out
    })
    .unwrap_or_default()
}

/// Wake every tool that made a [`Keys`] (a shortcut was pressed).
pub(super) fn wake() {
    if let Ok(w) = WAKE.lock() {
        for c in w.iter() {
            c.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_the_library_hotkey() {
        let k = Shortcut { cmd: true, shift: true, alt: false, ctrl: false, key: "2".into() };
        assert_eq!(to_hotkey(&k), Some(HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Digit2)));
        let v = Shortcut { cmd: true, shift: false, alt: true, ctrl: false, key: "V".into() };
        assert_eq!(to_hotkey(&v), Some(HotKey::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyV)));
        assert_eq!(v.display(), "⌥⌘V");
    }

    #[test]
    fn mouse_buttons_are_shortcuts_without_a_key() {
        let none = egui::Modifiers::NONE;
        assert_eq!(Shortcut::from_pointer(egui::PointerButton::Primary, none), None);
        assert_eq!(Shortcut::from_pointer(egui::PointerButton::Secondary, none), None);
        let back = Shortcut::from_pointer(egui::PointerButton::Extra1, none).expect("side button");
        assert_eq!((back.mouse_button(), back.has_modifier()), (Some(4), false));
        assert_eq!(to_hotkey(&back), None);
        let mid = Shortcut::from_pointer(egui::PointerButton::Middle, egui::Modifiers { alt: true, ..none }).expect("middle");
        assert_eq!((mid.key.as_str(), mid.alt), ("Mouse3", true));
        assert_eq!(Shortcut::from_pointer(egui::PointerButton::Extra2, none).and_then(|s| s.mouse_button()), Some(5));
        // stored the same way as a key, so old settings files read fine
        let json = serde_json::to_string(&back).unwrap();
        assert_eq!(serde_json::from_str::<Shortcut>(&json).unwrap(), back);
        assert_eq!(Shortcut::mouse(2, false, false, false, false), None);
        let k = Shortcut { cmd: false, shift: false, alt: false, ctrl: false, key: "Mouse".into() };
        assert_eq!(k.mouse_button(), None);
    }
}
