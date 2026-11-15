//! Code every tool may use: the glass drawing kit, macOS helpers, file
//! helpers, Claude Code discovery and the numbers from docs/MECHANICS.md.
//! Nothing here knows about any tool.

pub mod activity;
pub mod agent;
pub mod ai;
pub mod aicheck;
pub mod brand;
pub mod bundles;
pub mod charge;
pub mod claude_code;
pub mod crash_report;
pub mod empty;
pub mod fs;
pub mod fswatch;
pub mod glass;
pub mod ioreport;
pub mod helper;
pub mod hotkeys;
pub mod login;
pub mod macos;
pub mod mechanics;
pub mod native;
pub mod parakeet;
pub mod notify;
pub mod perms;
pub mod plist;
pub mod remove;
pub mod repaint_log;
pub mod settings_window;
pub mod smc;
pub mod spin;
pub mod sync;
pub mod sound;
pub mod tile;
pub mod update;
pub mod url_scheme;
pub mod vault;
pub mod viewport;
pub mod whisper;

/// Threads for a heavy model run (speech to text): every core but two, so
/// the UI keeps its frames while the model works.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn work_threads() -> usize {
    work_threads_for(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4))
}

fn work_threads_for(cores: usize) -> usize {
    cores.saturating_sub(2).clamp(2, 8)
}

#[cfg(test)]
mod work_threads_tests {
    #[test]
    fn two_cores_stay_free_for_the_ui() {
        assert_eq!(super::work_threads_for(8), 6);
        assert_eq!(super::work_threads_for(4), 2);
        assert_eq!(super::work_threads_for(2), 2);
        assert_eq!(super::work_threads_for(16), 8);
    }
}
