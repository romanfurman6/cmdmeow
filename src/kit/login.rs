//! Opening Cmd+Meow at login, through macOS's own login items
//! (SMAppService, macOS 13+). Setup's last step shows it on and applies it
//! when setup finishes; the switch is also in Settings › General. Nothing is
//! registered at launch (macOS announces every new login item).

/// Whether the app opens at login.
pub fn enabled() -> bool {
    imp::status() == Some(1)
}

/// The person turned Cmd+Meow off in System Settings › Login Items: it is
/// registered but macOS won't open it until they turn it back on there.
/// Read at most once a second.
pub fn needs_approval() -> bool {
    use std::sync::Mutex;
    use std::time::Instant;
    static SEEN: Mutex<Option<(bool, Instant)>> = Mutex::new(None);
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    match *g {
        Some((v, at)) if at.elapsed().as_secs_f32() < 1.0 => v,
        _ => {
            let v = imp::status() == Some(2);
            *g = Some((v, Instant::now()));
            v
        }
    }
}

/// System Settings › General › Login Items.
pub fn open_settings() {
    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.LoginItems-Settings.extension").spawn();
}

/// Turn opening at login on or off.
pub fn set(on: bool) -> Result<(), String> {
    imp::set(on)
}

fn marker() -> std::path::PathBuf {
    crate::kit::fs::data_path("login-default-done")
}

/// Setup hasn't settled Open at login yet: its switch shows on.
pub fn default_pending() -> bool {
    !marker().exists() && in_bundle()
}

/// The person picked (or setup applied the default): later runs leave it alone.
pub fn settle() {
    let m = marker();
    if let Some(dir) = m.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&m, b"");
}

/// End of setup: turn it on once, unless the person switched it off.
pub fn default_on() {
    if !default_pending() {
        return;
    }
    settle();
    if let Err(e) = set(true) {
        eprintln!("[login] couldn't add Cmd+Meow to login items: {e}");
    }
}

/// Running from CmdMeow.app (not `cargo run`).
fn in_bundle() -> bool {
    std::env::current_exe().ok().is_some_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use objc2_foundation::NSError;

    #[link(name = "ServiceManagement", kind = "framework")]
    extern "C" {}

    fn service() -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"SMAppService")?;
        unsafe { msg_send![cls, mainAppService] }
    }

    /// 0 not registered, 1 enabled, 2 needs approval in System Settings,
    /// 3 not found.
    pub fn status() -> Option<isize> {
        let s = service()?;
        Some(unsafe { msg_send![&*s, status] })
    }

    pub fn set(on: bool) -> Result<(), String> {
        let s = service().ok_or(ts!("kit.login.needs_13"))?;
        let r: Result<(), Retained<NSError>> = unsafe {
            if on {
                msg_send![&*s, registerAndReturnError: _]
            } else {
                msg_send![&*s, unregisterAndReturnError: _]
            }
        };
        r.map_err(|e| e.localizedDescription().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn status() -> Option<isize> {
        None
    }
    pub fn set(_on: bool) -> Result<(), String> {
        Err("Only on macOS".into())
    }
}
