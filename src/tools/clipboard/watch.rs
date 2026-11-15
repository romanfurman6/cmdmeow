//! Watches the pasteboard on a background thread. macOS has no "changed"
//! notification for it, so the thread checks its change count a few times a
//! second (a cheap integer read) and reads the content only when it moved.

use super::mac;
use super::settings::MAX_IMAGE;
use super::store::Clip;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

/// Switches the tool flips while the thread runs.
#[derive(Default)]
pub struct Shared {
    pub stop: AtomicBool,
    pub paused: AtomicBool,
    /// Read concealed items too.
    pub concealed: AtomicBool,
    pub images: AtomicBool,
    /// Change count after our own last write: that one isn't a new copy.
    pub own: AtomicIsize,
}

const EVERY: Duration = Duration::from_millis(350);

pub fn spawn(shared: Arc<Shared>, tx: Sender<Clip>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("clipboard-watch".into())
        .spawn(move || {
            let mut last = mac::change_count();
            while !shared.stop.load(Ordering::Relaxed) {
                std::thread::sleep(EVERY);
                let n = mac::change_count();
                if n == last {
                    continue;
                }
                last = n;
                if shared.paused.load(Ordering::Relaxed) || n == shared.own.load(Ordering::Relaxed) {
                    continue;
                }
                let images = shared.images.load(Ordering::Relaxed);
                let mut clip = mac::read(MAX_IMAGE, images);
                // temporary or generated clips never enter the history
                if clip.transient {
                    continue;
                }
                // one word copied while a password field has the keyboard is
                // most likely that password, even when the app didn't mark it
                if clip.secure && !super::privacy::terminal(&clip.bundle) && clip.files.is_empty() && clip.png.is_none() && clip.text.as_deref().is_some_and(super::privacy::password_shaped) {
                    clip.concealed = true;
                }
                if clip.concealed {
                    if !shared.concealed.load(Ordering::Relaxed) {
                        continue;
                    }
                    clip = mac::read_concealed(MAX_IMAGE, images);
                }
                if clip.is_empty() {
                    continue;
                }
                if tx.send(clip).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        })
        .ok();
}
