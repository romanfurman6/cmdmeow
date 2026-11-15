//! The installed tools. This is the only place that names them: to add a
//! tool, create `src/tools/<name>/` implementing [`crate::core::tool::Tool`]
//! and add one line below. Tools must not import each other; shared code
//! belongs in `crate::kit`.

pub mod capture;
pub mod claude;
pub mod clipboard;
pub mod dictation;
pub mod displays;
pub mod cleanup;
pub mod keep_awake;
pub mod fan;
pub mod lens;
pub mod startup;
pub mod system;
pub mod uninstaller;

use crate::core::tool::{Tool, ToolEnv};

/// Every tool, in the order of the window sidebar.
pub fn all(env: &ToolEnv) -> Vec<Box<dyn Tool>> {
    let (shot, video, sharing) = capture::tools(env);
    vec![
        Box::new(system::System::new(env)),
        Box::new(cleanup::Cleanup::new(env)),
        Box::new(lens::Lens::new(env)),
        Box::new(uninstaller::Uninstaller::new(env)),
        Box::new(startup::Startup::new(env)),
        Box::new(shot),
        Box::new(video),
        Box::new(sharing),
        Box::new(dictation::Dictation::new(env)),
        Box::new(clipboard::Clipboard::new(env)),
        Box::new(claude::Claude::new(env)),
        Box::new(keep_awake::KeepAwake::new(env)),
        Box::new(fan::FanControl::new(env)),
        Box::new(displays::Displays::new(env)),
    ]
}

/// The panel's widgets on first launch, as `tool/widget` keys. Widgets not
/// listed follow in tool order. The user can change this in the panel.
pub const DEFAULT_WIDGET_ORDER: &[&str] = &[
    "cleanup/clean",
    "capture/capture",
    "video/record",
    "keep_awake/card",
    "clipboard/recent",
    "dictation/card",
    "system/storage",
    "system/memory",
    "system/cpu",
    "system/battery",
    "system/network",
    "startup/summary",
    "system/devices",
    "claude/limits",
    "fan/fans",
    "displays/levels",
];

#[cfg(test)]
mod tests {
    #[test]
    fn tool_ids_and_widget_ids_are_unique() {
        let env = crate::core::tool::ToolEnv { ctx: egui::Context::default(), demo: true };
        let tools = super::all(&env);
        let mut ids: Vec<&str> = tools.iter().map(|t| t.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), tools.len());
        for t in &tools {
            let mut w: Vec<&str> = t.widgets().iter().map(|w| w.id).collect();
            w.sort();
            let n = w.len();
            w.dedup();
            assert_eq!(w.len(), n, "duplicate widget id in {}", t.id());
            for spec in t.widgets() {
                assert!(!spec.sizes.is_empty(), "{}/{} offers no size", t.id(), spec.id);
                assert!(spec.sizes.contains(&spec.size), "{}/{} default size not offered", t.id(), spec.id);
                assert!(spec.sizes.windows(2).all(|w| w[0].dims() < w[1].dims()), "{}/{} sizes not smallest first", t.id(), spec.id);
                assert!(!spec.about.is_empty());
            }
        }
    }

    /// Tools stay isolated: no tool module refers to another tool's module.
    #[test]
    fn tools_do_not_import_each_other() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
        let names: Vec<String> = std::fs::read_dir(&root).unwrap().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        // every file of a tool, sub-folders too
        fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().is_dir() {
                    files(&e.path(), out);
                } else {
                    out.push(e.path());
                }
            }
        }
        for me in &names {
            let mut all = Vec::new();
            files(&root.join(me), &mut all);
            for f in all {
                let src = std::fs::read_to_string(&f).unwrap();
                for other in names.iter().filter(|o| *o != me) {
                    for pat in [format!("tools::{other}"), format!("super::super::{other}")] {
                        assert!(!src.contains(&pat), "{} uses {pat}", f.display());
                    }
                }
            }
        }
    }
}
