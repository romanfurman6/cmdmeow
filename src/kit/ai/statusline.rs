//! Clean-up of the old Claude Code status-line hook. Earlier versions read
//! live Claude limits by putting a small script in Claude Code's status line
//! (in `~/.claude/settings.json`). Limits now come from Claude's account, so
//! on launch the hook is taken out and the person's own status line put
//! back, as it was before the hook.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn dir() -> PathBuf {
    crate::kit::fs::data_path("ai")
}

/// The hook script in `dir`.
#[cfg(test)]
fn script(dir: &Path) -> PathBuf {
    dir.join("statusline.sh")
}

/// The person's previous status line, run after saving.
fn prev_script(dir: &Path) -> PathBuf {
    dir.join("statusline-prev.sh")
}

fn claude_settings() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude").join("settings.json")
}

/// The hook is set in Claude Code's settings.
pub fn installed() -> bool {
    is_ours(&read_json(&claude_settings())["statusLine"], &dir())
}

fn is_ours(status_line: &Value, dir: &Path) -> bool {
    status_line["command"].as_str().is_some_and(|c| c.contains("statusline.sh") && c.contains(&*dir.to_string_lossy()))
}

fn read_json(p: &Path) -> Value {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

/// Take the hook out and put the previous status line back.
pub fn remove() -> Result<(), String> {
    remove_at(&claude_settings(), &dir())
}

fn remove_at(settings: &Path, dir: &Path) -> Result<(), String> {
    let mut v = read_json(settings);
    if !v.is_object() || !is_ours(&v["statusLine"], dir) {
        return Ok(());
    }
    match std::fs::read_to_string(prev_script(dir)).ok().and_then(|s| s.strip_prefix("#!/bin/sh\n").map(|c| c.trim_end().to_string())) {
        Some(cmd) if !cmd.is_empty() => v["statusLine"] = json!({ "type": "command", "command": cmd }),
        _ => {
            if let Some(o) = v.as_object_mut() {
                o.remove("statusLine");
            }
        }
    }
    let _ = std::fs::remove_file(prev_script(dir));
    write_json(settings, &v)
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.cmdmeow-tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

/// Take the hook out if it's there, and drop its files. Runs once per launch.
pub fn cleanup() {
    if installed() {
        if let Err(e) = remove() {
            eprintln!("[ai] status line: {e}");
            return;
        }
    }
    let d = dir();
    for f in ["statusline.sh", "claude-status.json", "claude-status.json.tmp"] {
        let _ = std::fs::remove_file(d.join(f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_puts_the_old_status_line_back() {
        let d = std::env::temp_dir().join(format!("cmb-statusline-{}", std::process::id()));
        let hook = d.join("app data");
        std::fs::create_dir_all(&hook).unwrap();
        let f = d.join("settings.json");
        let ours = format!("\"{}\"", script(&hook).display());
        std::fs::write(&f, json!({"model":"opus","statusLine":{"type":"command","command":ours}}).to_string()).unwrap();
        std::fs::write(prev_script(&hook), "#!/bin/sh\n~/bin/my-line\n").unwrap();
        assert!(is_ours(&read_json(&f)["statusLine"], &hook));
        remove_at(&f, &hook).unwrap();
        let v = read_json(&f);
        assert_eq!(v["statusLine"]["command"], "~/bin/my-line");
        assert_eq!(v["model"], "opus");
        assert!(!prev_script(&hook).exists());
        // no old line: the key goes
        std::fs::write(&f, json!({"statusLine":{"type":"command","command":ours}}).to_string()).unwrap();
        remove_at(&f, &hook).unwrap();
        assert!(read_json(&f).get("statusLine").is_none());
        // someone else's status line is left alone
        std::fs::write(&f, r#"{"statusLine":{"type":"command","command":"x"}}"#).unwrap();
        remove_at(&f, &hook).unwrap();
        assert_eq!(read_json(&f)["statusLine"]["command"], "x");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
