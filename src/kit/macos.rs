//! Small macOS helpers any tool may use: quoting for the shell and
//! AppleScript, running AppleScript or an admin command, the user id,
//! opening or revealing files and an app's icon as pixels. Elsewhere the
//! system calls are inert so tools build and test on Linux.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// Quote for a POSIX shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Quote for an AppleScript string literal.
pub fn as_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Run AppleScript; Ok(stdout) or Err(message).
pub fn osascript(script: &str) -> Result<String, String> {
    let o = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        // "User canceled. (-128)"
        Err(if err.contains("-128") { "Cancelled".into() } else { err })
    }
}

/// Run a shell command as root after macOS asks for the admin password.
pub fn run_admin(cmd: &str, prompt: &str) -> Result<String, String> {
    osascript(&format!("do shell script {} with prompt {} with administrator privileges", as_quote(cmd), as_quote(prompt)))
}

pub fn uid() -> u32 {
    unsafe { libc::getuid() }
}

/// Open a URL or file with its default app.
pub fn open(target: &str) {
    let _ = std::process::Command::new("/usr/bin/open").arg(target).spawn();
}

/// Show a file in Finder.
pub fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
}

/// The app's own icon as straight RGBA pixels, `px` square. Main thread only.
/// Finder puts file reference URLs (`file:///.file/id=…`) on the drag and
/// copy pasteboards: turn one into the plain `file:///path` URL. Other
/// strings come back as they are.
#[cfg(target_os = "macos")]
pub fn file_path_url(url: &str) -> String {
    use objc2_foundation::{NSString, NSURL};
    if !url.starts_with("file:///.file/") {
        return url.to_string();
    }
    #[allow(unused_unsafe)]
    let resolved = unsafe { NSURL::URLWithString(&NSString::from_str(url)).and_then(|u| u.filePathURL()).and_then(|u| u.absoluteString()) };
    resolved.map(|s| s.to_string()).unwrap_or_else(|| url.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn file_path_url(url: &str) -> String {
    url.to_string()
}

#[cfg(target_os = "macos")]
pub fn app_icon(path: &std::path::Path, px: usize) -> Option<(usize, Vec<u8>)> {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    use objc2_app_kit::{NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    MainThreadMarker::new()?;
    // an app in Downloads/Desktop/Documents: reading it makes macOS ask
    // about the folder (the row shows its letter instead)
    if crate::kit::fs::behind_folder_prompt(path) {
        return None;
    }
    let img = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()));
    let n = px as isize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            n,
            n,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            n * 4,
            32,
        )
    }?;
    let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
    let previous = NSGraphicsContext::currentContext();
    NSGraphicsContext::setCurrentContext(Some(&ctx));
    img.drawInRect_fromRect_operation_fraction(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px as f64, px as f64)),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        NSCompositingOperation::SourceOver,
        1.0,
    );
    ctx.flushGraphics();
    NSGraphicsContext::setCurrentContext(previous.as_deref());
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    // premultiplied → straight
    let bytes = unsafe { std::slice::from_raw_parts(data, px * px * 4) };
    let mut out = bytes.to_vec();
    for p in out.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some((px, out))
}

#[cfg(not(target_os = "macos"))]
pub fn app_icon(_path: &std::path::Path, _px: usize) -> Option<(usize, Vec<u8>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/Users/a/It's here"), "'/Users/a/It'\\''s here'");
        assert_eq!(as_quote("say \"hi\" \\ ok"), "\"say \\\"hi\\\" \\\\ ok\"");
    }
}
