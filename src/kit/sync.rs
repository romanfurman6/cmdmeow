//! Locks that survive a panic. A thread that panics while holding a mutex
//! "poisons" it, and `lock().unwrap()` then panics on every later lock, so
//! one failed background job used to take the whole app down with it.
//! The data behind these locks is plain status the next update overwrites,
//! so carrying on with it is safe.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub trait LockExt<T: ?Sized> {
    /// Like `lock().unwrap()`, but takes the data back from a poisoned lock.
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockExt<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Writes every panic, with its thread, place and backtrace, to
/// `~/Library/Logs/CmdMeow/crash.log` (`crash-dev.log` for the dev build),
/// then lets the default hook print it to stderr as before.
pub fn install_panic_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_panic(info);
        default(info);
    }));
}

fn write_panic(info: &std::panic::PanicHookInfo) -> std::io::Result<()> {
    use std::io::Write;
    let dir = dirs::home_dir().unwrap_or_else(std::env::temp_dir).join("Library/Logs/CmdMeow");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(if super::fs::is_dev() { "crash-dev.log" } else { "crash.log" });
    // keep the file small: start over once it passes 1 MB
    let append = std::fs::metadata(&path).map(|m| m.len() < 1 << 20).unwrap_or(true);
    let mut f = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&path)?;
    let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into());
    let place = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
    let thread = std::thread::current();
    writeln!(f, "{} v{} [{}] {} at {}\n{}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), thread.name().unwrap_or("?"), msg, place, std::backtrace::Backtrace::force_capture())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_lock_still_opens() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("boom");
        })
        .join();
        assert!(m.is_poisoned());
        *m.lock_ok() += 1;
        assert_eq!(*m.lock_ok(), 2);
    }
}
