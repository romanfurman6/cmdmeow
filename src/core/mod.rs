//! The shell. It owns the tray icon, the panel, the full window and the
//! settings, and talks to tools only through the [`tool::Tool`] trait.

pub mod editor;
pub mod gate;
pub mod layout;
pub mod onboard;
pub mod panel;
pub mod settings;
pub mod shell;
pub mod tool;
pub mod updated;
pub mod window;
