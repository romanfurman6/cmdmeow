//! Claude's other accounts: the Add account dialog (from the provider
//! menu and Settings › AI Usage). An account is a Claude Code config folder
//! (`CLAUDE_CONFIG_DIR`); the dialog checks the folder as it's typed or
//! chosen: whether it's signed in, its plan, how many sessions it logged.

use crate::kit::ai::claude::{check_dir, DirCheck};
use crate::kit::ai::settings::{self, expand, tilde, Account};
use crate::kit::glass::*;
use crate::kit::sync::LockExt;
use egui::{Align2, Id, Pos2, Rect, Ui, Vec2};
use std::sync::Mutex;

/// The dialog's fields while it's open.
#[derive(Clone, Default)]
struct Form {
    name: String,
    dir: String,
    /// The window it opened in (the main window or Settings).
    viewport: Option<egui::ViewportId>,
}

/// The last folder check: (folder typed, what it holds).
static CHECK: Mutex<Option<(String, Option<DirCheck>)>> = Mutex::new(None);
/// A folder picked in the system dialog, waiting for the form.
static PICKED: Mutex<Option<String>> = Mutex::new(None);

fn id() -> Id {
    Id::new("ai-add-account")
}

/// Open the dialog.
pub fn open(ctx: &egui::Context) {
    let viewport = Some(ctx.viewport_id());
    ctx.data_mut(|d| d.insert_temp(id(), Form { dir: "~/.claude-".into(), viewport, ..Default::default() }));
    *CHECK.lock_ok() = None;
}

/// Check `dir` off the ui thread unless it was just checked.
fn check(ctx: &egui::Context, dir: &str) {
    {
        let mut g = CHECK.lock_ok();
        if g.as_ref().is_some_and(|(d, _)| d == dir) {
            return;
        }
        *g = Some((dir.to_string(), None));
    }
    let (dir, ctx) = (dir.to_string(), ctx.clone());
    std::thread::spawn(move || {
        let c = check_dir(&expand(&dir));
        let mut g = CHECK.lock_ok();
        if g.as_ref().is_some_and(|(d, _)| *d == dir) {
            *g = Some((dir, Some(c)));
        }
        drop(g);
        ctx.request_repaint();
    });
}

/// The system folder dialog, off the ui thread.
fn choose(ctx: &egui::Context) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let script = "POSIX path of (choose folder with invisibles)";
        let out = std::process::Command::new("osascript").args(["-e", script]).output();
        let path = out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches('/').to_string()).filter(|s| !s.is_empty());
        if let Some(p) = path {
            *PICKED.lock_ok() = Some(tilde(std::path::Path::new(&p)));
            ctx.request_repaint();
        }
    });
}

/// Why the folder can't be added, if it can't.
fn problem(dir: &str, set: &settings::Settings) -> Option<String> {
    let path = expand(dir);
    let main = expand("~/.claude");
    if dir.trim().is_empty() {
        return Some(String::new());
    }
    if path == main || set.accounts.iter().any(|a| expand(&a.dir) == path) {
        return Some(ts!("ai.acc.already").into());
    }
    None
}

/// Draw the dialog while it's open.
pub fn dialog(ui: &mut Ui, demo: bool) {
    let ctx = ui.ctx().clone();
    let Some(mut form) = ctx.data(|d| d.get_temp::<Form>(id())).filter(|f| f.viewport == Some(ctx.viewport_id())) else { return };
    if let Some(p) = PICKED.lock_ok().take() {
        form.dir = p;
    }
    let set = settings::get();
    let blocked = problem(&form.dir, &set);
    if blocked.is_none() {
        check(&ctx, &form.dir);
    }
    let result = CHECK.lock_ok().as_ref().filter(|(d, _)| *d == form.dir).map(|(_, c)| c.clone());
    let (add, close) = crate::kit::modal::details(&ctx, id(), 400.0, 268.0, |ui, m| {
        let p = ui.painter().clone();
        let i = m.shrink(20.0);
        p.text(i.left_top(), Align2::LEFT_TOP, ts!("ai.acc.title"), bold(15.0), TEXT());
        // name
        let mut y = i.top() + 20.0 + sp::L;
        p.text(Pos2::new(i.left(), y), Align2::LEFT_TOP, ts!("ai.acc.name"), ty::label(), TEXT_DIM());
        y += 16.0 + sp::S;
        text_input(ui, Rect::from_min_size(Pos2::new(i.left(), y), Vec2::new(i.width(), BTN_H)), id().with("name"), &mut form.name, ts!("ai.acc.name_hint"), FieldStyle::default());
        // folder
        y += BTN_H + sp::M;
        p.text(Pos2::new(i.left(), y), Align2::LEFT_TOP, ts!("ai.acc.folder"), ty::label(), TEXT_DIM());
        y += 16.0 + sp::S;
        let cw = button_width(&p, ts!("ai.acc.choose"), BTN_SECONDARY);
        text_input(ui, Rect::from_min_size(Pos2::new(i.left(), y), Vec2::new(i.width() - cw - sp::S, BTN_H)), id().with("dir"), &mut form.dir, "~/.claude-work", FieldStyle::default());
        if pill_button(ui, Rect::from_min_size(Pos2::new(i.right() - cw, y), Vec2::new(cw, BTN_H)), ts!("ai.acc.choose"), BTN_SECONDARY, true).clicked() {
            choose(ui.ctx());
        }
        // what the folder holds
        y += BTN_H + sp::M;
        let (line, col, ok) = match (&blocked, &result) {
            (Some(why), _) => (why.clone(), TEXT_DIM(), false),
            (None, Some(Some(c))) if !c.exists => (ts!("ai.acc.no_folder").to_string(), AMBER(), false),
            (None, Some(Some(c))) if c.signed_in == Some(false) => (t!("ai.acc.signed_out", dir = form.dir.trim()).into_owned(), AMBER(), true),
            (None, Some(Some(c))) => {
                let plan = if c.plan.is_empty() { String::new() } else { format!("Claude {}{} · ", c.plan[..1].to_uppercase(), &c.plan[1..]) };
                (t!("ai.acc.signed_in", plan = plan, n = c.sessions).into_owned(), GREEN(), true)
            }
            _ => (ts!("ai.checking").to_string(), TEXT_DIM(), false),
        };
        text_fit(&p, Pos2::new(i.left(), y), Align2::LEFT_TOP, &line, ty::label(), col, i.width());
        // Cancel, Add
        let by = i.bottom() - BTN_H;
        let aw = button_width(&p, ts!("ai.acc.add"), BTN_PRIMARY);
        let xw = button_width(&p, ts!("ai.acc.cancel"), BTN_SECONDARY);
        let add = pill_button(ui, Rect::from_min_size(Pos2::new(i.right() - aw, by), Vec2::new(aw, BTN_H)), ts!("ai.acc.add"), BTN_PRIMARY, ok && !form.name.trim().is_empty()).clicked();
        let cancel = pill_button(ui, Rect::from_min_size(Pos2::new(i.right() - aw - sp::S - xw, by), Vec2::new(xw, BTN_H)), ts!("ai.acc.cancel"), BTN_SECONDARY, true).clicked();
        (add, cancel)
    });
    let (add, cancel) = add;
    if add {
        let acc = Account { name: form.name.trim().to_string(), dir: tilde(&expand(form.dir.trim())) };
        settings::update(!demo, |s| s.accounts.push(acc));
        super::backend::request_refresh();
    }
    if add || cancel || close {
        ctx.data_mut(|d| d.remove::<Form>(id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(id(), form));
    }
}
