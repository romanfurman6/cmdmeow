//! Cmd+Meow — a glassy menu bar companion built from isolated tools (Mac
//! health, Claude limits, Cleanup, Disk Map) behind one panel of widgets.
//!
//! `--screenshot <file.png>` draws ~120 frames with sample data, saves the
//! window as a PNG and exits. `--view <name>` picks what it shows: panel,
//! storage (default), memory, battery, cpu, network, devices (Mac Health with
//! that card's details), claude-detail,
//! cleanup, cleanup-no-claude, results, lens, lens-live, claude, claude-missing,
//! health, customize, settings-tools, fan, fan-detail, fan-air, fan-air-detail,
//! fan-curve, fan-fixed, fan-install, setup, setup-tools, setup-tools-all, setup-tools-preview, setup-ai, setup-signins, setup-perms,
//! setup-crash, setup-ready, settings-general, settings-cat, locked-<view>, tools-off-<view>,
//! rows or all-s11 / all-s12 / all-s24 / all-s44 for every widget at one size,
//! or widget-<tool>/<widget>@<size> for one widget alone (widget-system/devices@4:4).
//! CMDMEOW_DEMO_PICK=watch shows setup-tools-preview with only "Watch my Mac" picked.
//! CMDMEOW_DEMO_LONG=1 and CMDMEOW_DEMO_DEVICES=one|long fill them with the
//! longest texts they meet. CMDMEOW_DEMO_UPDATE=available|downloading|failed
//! shows the panel's update notice in that state.
//! `--fan-report` prints every fan and temperature sensor this Mac reports.
//! `--fan-install` / `--fan-uninstall` add or remove the fan helper,
//! `--fan-set 2400,3000|auto|hello` talks to it (the lease lasts 8 s).
//! `--rec-probe <secs> [--no-mic] [--no-sys]` records the main display without
//! UI and prints what the recorder saw.
//! `--video-probe <file.mp4> [--download]` runs the video viewer's work on a
//! file without UI: sound tracks, voice cleanup, a cut saved as a copy, and
//! (with the speech model there, or downloaded with `--download`) a transcript.
//! `--dictation-probe <file.wav | mic:SECS> [--download] [--whisper] [--paste]`
//! turns a recording (or the microphone) into text the way dictation does.
//! `--fan-helper <uid>` is the helper itself (run by launchd as root).

#[macro_use]
extern crate rust_i18n;
// every text the app shows: locales/*.json (see src/i18n.rs, docs/i18n.md)
i18n!("locales", fallback = "en");

#[macro_use]
mod i18n;
mod core;
mod kit;
mod tools;

fn main() -> eframe::Result {
    kit::repaint_log::mark_start();
    kit::sync::install_panic_log();
    // official release builds only (DSN baked in by release.yml), and only
    // after the person agreed in setup or Settings; off everywhere else
    let _crash_report = kit::crash_report::start_if_allowed();
    if std::env::args().any(|a| a == "--fan-report") {
        tools::fan::probe::report();
        return Ok(());
    }
    {
        // the fan helper and its test switches
        let a: Vec<String> = std::env::args().collect();
        match a.get(1).map(String::as_str) {
            Some("--fan-helper") => tools::fan::helper::serve(a.get(2).and_then(|u| u.parse().ok()).unwrap_or(0)),
            Some("--fan-install") => {
                println!("{:?}", tools::fan::helper::install());
                return Ok(());
            }
            Some("--fan-uninstall") => {
                println!("{:?}", tools::fan::helper::uninstall());
                return Ok(());
            }
            Some("--fan-set") => {
                use tools::fan::helper::{ask, Request};
                let req = match a.get(2).map(String::as_str) {
                    None | Some("auto") => Request::Auto,
                    Some("hello") => Request::Hello,
                    Some(v) => Request::Set { rpm: v.split(',').map(|x| x.parse().ok()).collect() },
                };
                println!("{:?}", ask(&req));
                return Ok(());
            }
            _ => {}
        }
    }
    {
        let a: Vec<String> = std::env::args().collect();
        if a.get(1).map(String::as_str) == Some("--rec-probe") {
            let secs = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5.0);
            tools::capture::record::probe(secs, !a.iter().any(|v| v == "--no-sys"), !a.iter().any(|v| v == "--no-mic"));
            return Ok(());
        }
        if a.get(1).map(String::as_str) == Some("--dictation-probe") {
            if let Err(e) = tools::dictation::probe::probe(&a[2..]) {
                eprintln!("[dictation] {e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        if a.get(1).map(String::as_str) == Some("--video-probe") {
            if let Some(f) = a.get(2) {
                tools::capture::video::probe(std::path::Path::new(f), a.iter().any(|v| v == "--download"));
            }
            return Ok(());
        }
    }
    if std::env::args().any(|a| a == "--scan-report") {
        tools::cleanup::scan_report(std::env::args().any(|a| a == "--ai"));
        return Ok(());
    }
    // the app itself (no switches), release build: stderr to
    // ~/Library/Logs/CmdMeow/cmdmeow.log for Settings › About › Logs
    if std::env::args().len() == 1 {
        kit::logs::start_run_log();
    }
    let mut screenshot = None;
    let mut view = "storage".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(std::path::PathBuf::from),
            "--view" => view = args.next().unwrap_or(view),
            _ => {}
        }
    }
    if screenshot.is_none() && !std::env::args().any(|a| a == "--view") {
        view = "panel".into();
    }
    // one copy per build: a second launch (`open -n`) shows the running
    // one's window and quits (a relaunch waits for the old copy instead)
    if screenshot.is_none() && !kit::perms::relaunched() && kit::url_scheme::hand_over() {
        return Ok(());
    }
    let opts = core::shell::Options { screenshot, view };
    // the shell sizes the window for the chosen view on its first frame
    let viewport = egui::ViewportBuilder::default()
        .with_title("Cmd+Meow")
        .with_app_id(if kit::fs::is_dev() { "com.romanfurman6.cmdmeow.dev" } else { "com.romanfurman6.cmdmeow" })
        .with_inner_size([kit::mechanics::PANEL_WIDTH, kit::mechanics::PANEL_HEIGHT])
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_has_shadow(true);
    // the built-in GPU: on Intel Macs with two GPUs, wgpu's default (high
    // performance) wakes the discrete one for as long as the app runs, and
    // the fans spin up. A menu bar app never needs it.
    // `WGPU_POWER_PREF=high` still picks it, to compare.
    let mut gpu = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    gpu.power_preference = eframe::wgpu::PowerPreference::from_env().unwrap_or(eframe::wgpu::PowerPreference::LowPower);
    let wgpu_options = eframe::egui_wgpu::WgpuConfiguration { wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(gpu), ..Default::default() };
    let options = eframe::NativeOptions { viewport, wgpu_options, ..Default::default() };
    // the panel shows on a click, not by itself at launch (the shell hides
    // it on its first pass; this keeps it from flashing up before that)
    if opts.screenshot.is_none() && !kit::perms::launched_to_panel() {
        eframe::start_hidden();
    }
    // before the run loop, so the link that launched the app is caught
    kit::url_scheme::install();
    eframe::run_native("Cmd+Meow", options, Box::new(move |cc| Ok(Box::new(core::shell::App::new(cc, opts)))))
}
