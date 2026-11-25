//! What AI coding agents leave behind: caches, logs, old sessions, local
//! models and the git worktrees they work in. Each item is tagged with its
//! tool (`Ctx::tool`), so the results can list them per tool.
//!
//! Rules: anything a tool makes again by itself is Safe; anything that holds
//! the person's work or history (sessions, chats, models, worktrees) only
//! ever asks; settings, sign-ins, extensions and the tools' own databases are
//! never offered. Nothing here reads the contents of chats or code.

use super::engine::{age_days, children, fname, Action, Ctx, Group, Verdict};
use crate::kit::fs::home;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Logs of the editors younger than this are kept (the open window writes
/// into the newest).
const EDITOR_LOG_DAYS: f64 = 7.0;
/// Files a tool may still be writing: younger than this never count as junk.
const FRESH_DAYS: f64 = 1.0;
/// Old sessions: offered (to ask) once untouched this long, unless the tool
/// keeps its own setting (Claude Code's `cleanupPeriodDays`).
pub const OLD_SESSION_DAYS: f64 = 30.0;

pub const CODEX: &str = "Codex";
pub const CURSOR: &str = "Cursor";
pub const GEMINI: &str = "Gemini CLI";
pub const COPILOT: &str = "Copilot CLI";
pub const OPENCODE: &str = "OpenCode";
pub const OLLAMA: &str = "Ollama";
pub const HUGGING_FACE: &str = "Hugging Face";
pub const LM_STUDIO: &str = "LM Studio";
pub const WORKTREES: &str = "Worktrees";
pub const CHROME_MCP: &str = "Chrome DevTools MCP";

/// Kind of the worktree items (see `engine::live_kind`: never removed while
/// a process has a file or its working folder in one).
pub const WORKTREE_KIND: &str = "Worktrees";

/// Editors built on VS Code. AI editors go to AI Tools (their web caches
/// too, so the generic app-cache scan skips them); VS Code itself is a
/// developer tool.
pub struct Editor {
    /// Folder in ~/Library/Application Support.
    pub dir: &'static str,
    pub name: &'static str,
    pub group: Group,
}

pub const EDITORS: [Editor; 8] = [
    Editor { dir: "Cursor", name: "Cursor", group: Group::AiTools },
    Editor { dir: "Windsurf", name: "Windsurf", group: Group::AiTools },
    Editor { dir: "Antigravity", name: "Antigravity", group: Group::AiTools },
    Editor { dir: "Kiro", name: "Kiro", group: Group::AiTools },
    Editor { dir: "Trae", name: "Trae", group: Group::AiTools },
    Editor { dir: "Code", name: "VS Code", group: Group::Developer },
    Editor { dir: "Code - Insiders", name: "VS Code Insiders", group: Group::Developer },
    Editor { dir: "VSCodium", name: "VSCodium", group: Group::Developer },
];

/// An AI editor's folder in Application Support: its caches are listed here,
/// not by the generic app-cache scan.
pub fn ai_editor_dir(name: &str) -> bool {
    EDITORS.iter().any(|e| e.group == Group::AiTools && e.dir == name)
}

/// Tools whose open process means their newest files may be in use:
/// (tool, process names, app bundle ids).
const PROCESSES: [(&str, &[&str], &[&str]); 7] = [
    (CODEX, &["codex"], &["com.openai.codex"]),
    (CURSOR, &["cursor-agent", "cursor"], &["com.todesktop.230313mzl4w4u92"]),
    (GEMINI, &["gemini"], &[]),
    (COPILOT, &["copilot"], &[]),
    (OPENCODE, &["opencode"], &[]),
    (OLLAMA, &["ollama"], &["com.electron.ollama"]),
    (LM_STUDIO, &["lm studio", "lms"], &["ai.elementlabs.lmstudio"]),
];

pub(super) fn scan(c: &mut Ctx) {
    let h = home();
    codex(c, &env_dir("CODEX_HOME").unwrap_or_else(|| h.join(".codex")));
    gemini(c, &h);
    copilot(c, &h);
    opencode(c, &h);
    browser_profiles(c, &h);
    for e in &EDITORS {
        editor(c, &h.join("Library/Application Support").join(e.dir), e);
    }
    ollama(c, &env_dir("OLLAMA_MODELS").unwrap_or_else(|| h.join(".ollama/models")));
    hugging_face(c, &h);
    lm_studio(c, &h);
    worktrees(c, &h);
    c.tool = "";
    while_running(c);
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

// ---------------------------------------------------------------- CLIs

fn codex(c: &mut Ctx, home: &Path) {
    if !home.is_dir() {
        return;
    }
    c.tool = CODEX;
    for sub in ["cache/codex_apps_server_info", "cache/codex_apps_tools"] {
        c.add(Group::AiTools, ts!("cleanup.it.codex"), home.join(sub), "Caches", Verdict::Safe, ts!("cleanup.why.codex"));
    }
    for sub in [".tmp", "tmp"] {
        c.add(Group::AiTools, ts!("cleanup.it.temp_downloads"), home.join(sub), "Temp Files", Verdict::Safe, ts!("cleanup.why.temp_downloads"));
    }
    c.add(Group::AiTools, ts!("cleanup.it.plugin_cache"), home.join("plugins/cache"), "Caches", Verdict::Safe, ts!("cleanup.why.plugin_cache"));
    c.add(Group::AiTools, ts!("cleanup.it.logs"), home.join("log"), "Logs", Verdict::Safe, ts!("cleanup.why.tool_logs"));
    // sessions/<year>/<month>/<day>/rollout-….jsonl: one item per old day
    for day in day_folders(&home.join("sessions")) {
        if newest_age(&day) > OLD_SESSION_DAYS {
            let when = day.strip_prefix(home.join("sessions")).map(|p| p.display().to_string().replace('/', "-")).unwrap_or_default();
            c.add(Group::AiTools, &t!("cleanup.it.codex_day", day = when), day, "Old Sessions", Verdict::Ask, &t!("cleanup.why.codex_sessions", days = OLD_SESSION_DAYS as u64));
        }
    }
    for f in children(&home.join("archived_sessions")) {
        if age_days(&f) > OLD_SESSION_DAYS {
            c.add(Group::AiTools, ts!("cleanup.it.archived_session"), f, "Old Sessions", Verdict::Ask, &t!("cleanup.why.codex_sessions", days = OLD_SESSION_DAYS as u64));
        }
    }
}

/// `root/<year>/<month>/<day>` folders.
fn day_folders(root: &Path) -> Vec<PathBuf> {
    let digits = |p: &Path| p.is_dir() && fname(p).chars().all(|ch| ch.is_ascii_digit());
    let mut out = Vec::new();
    for y in children(root).into_iter().filter(|p| digits(p)) {
        for m in children(&y).into_iter().filter(|p| digits(p)) {
            out.extend(children(&m).into_iter().filter(|p| digits(p)));
        }
    }
    out
}

/// Days since anything directly in `dir` (or the folder itself) changed.
fn newest_age(dir: &Path) -> f64 {
    children(dir).iter().map(|p| age_days(p)).fold(age_days(dir), f64::min)
}

fn gemini(c: &mut Ctx, h: &Path) {
    c.tool = GEMINI;
    // ~/.gemini/tmp/<sha256 of the project>: that project's chats and logs
    for d in children(&h.join(".gemini/tmp")) {
        if d.is_dir() && is_hex_id(&fname(&d)) && newest_age(&d) > OLD_SESSION_DAYS {
            c.add(Group::AiTools, ts!("cleanup.it.old_chats"), d, "Old Sessions", Verdict::Ask, &t!("cleanup.why.gemini_chats", days = OLD_SESSION_DAYS as u64));
        }
    }
}

/// A 64-character hex name (a sha256), as Gemini CLI names project folders.
pub fn is_hex_id(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn copilot(c: &mut Ctx, h: &Path) {
    let home = h.join(".copilot");
    if !home.is_dir() {
        return;
    }
    c.tool = COPILOT;
    c.add(Group::AiTools, ts!("cleanup.it.logs"), home.join("logs"), "Logs", Verdict::Safe, ts!("cleanup.why.tool_logs"));
    for d in children(&home.join("session-state")) {
        if age_days(&d) > OLD_SESSION_DAYS && newest_age(&d) > OLD_SESSION_DAYS {
            c.add(Group::AiTools, ts!("cleanup.it.old_session"), d, "Old Sessions", Verdict::Ask, &t!("cleanup.why.old_sessions", days = OLD_SESSION_DAYS as u64));
        }
    }
}

fn opencode(c: &mut Ctx, h: &Path) {
    c.tool = OPENCODE;
    let data = h.join(".local/share/opencode");
    c.add(Group::AiTools, ts!("cleanup.it.logs"), data.join("log"), "Logs", Verdict::Safe, ts!("cleanup.why.tool_logs"));
    c.add(Group::AiTools, ts!("cleanup.it.undo_snapshots"), data.join("snapshot"), "Old Sessions", Verdict::Ask, ts!("cleanup.why.undo_snapshots"));
    c.add(Group::AiTools, ts!("cleanup.it.packages"), h.join(".cache/opencode"), "Caches", Verdict::Safe, ts!("cleanup.why.packages"));
    c.tool = CURSOR;
    for f in children(&h.join(".local/share/cursor-agent")) {
        if f.extension().is_some_and(|e| e == "log") {
            c.add(Group::AiTools, ts!("cleanup.it.logs"), f, "Logs", Verdict::Safe, ts!("cleanup.why.tool_logs"));
        }
    }
}

/// Browsers the agents drive: only their caches, never the profile.
fn browser_profiles(c: &mut Ctx, h: &Path) {
    c.tool = "Antigravity";
    let gem = h.join(".gemini/antigravity-browser-profile");
    for sub in ["Default/Cache", "Default/Code Cache", "Default/DawnGraphiteCache", "Default/DawnWebGPUCache", "Default/GPUCache", "Default/Service Worker/CacheStorage", "GraphiteDawnCache", "component_crx_cache", "extensions_crx_cache"] {
        c.add(Group::AiTools, ts!("cleanup.it.web_cache"), gem.join(sub), "Caches", Verdict::Safe, ts!("cleanup.why.agent_profile"));
    }
    c.tool = CHROME_MCP;
    let mcp = h.join(".cache/chrome-devtools-mcp/chrome-profile");
    for sub in ["Default/Cache", "Default/Code Cache", "Default/DawnCache", "Default/GPUCache", "Default/GrShaderCache", "Default/GraphiteDawnCache", "Default/Service Worker/CacheStorage", "GraphiteDawnCache", "component_crx_cache", "extensions_crx_cache"] {
        c.add(Group::AiTools, ts!("cleanup.it.web_cache"), mcp.join(sub), "Caches", Verdict::Safe, ts!("cleanup.why.mcp_profile"));
    }
}

// ---------------------------------------------------------------- editors

/// Chromium cache folders an Electron app makes again by itself.
pub const WEB_CACHES: [&str; 9] = ["Cache", "Code Cache", "GPUCache", "DawnCache", "DawnWebGPUCache", "DawnGraphiteCache", "GraphiteDawnCache", "Service Worker/CacheStorage", "Shared Dictionary"];

fn editor(c: &mut Ctx, base: &Path, e: &Editor) {
    if !base.is_dir() {
        return;
    }
    c.tool = e.name;
    // compiled code of each version it ran: only the newest is used
    let mut versions: Vec<PathBuf> = children(&base.join("CachedData")).into_iter().filter(|p| p.is_dir()).collect();
    versions.sort_by(|a, b| age_days(a).total_cmp(&age_days(b)));
    for v in versions.into_iter().skip(1) {
        c.add(e.group, ts!("cleanup.it.old_version_cache"), v, "Caches", Verdict::Safe, ts!("cleanup.why.old_version_cache"));
    }
    c.add(e.group, ts!("cleanup.it.extension_downloads"), base.join("CachedExtensionVSIXs"), "Caches", Verdict::Safe, ts!("cleanup.why.extension_downloads"));
    for d in children(&base.join("logs")) {
        if d.is_dir() && age_days(&d) > EDITOR_LOG_DAYS {
            c.add(e.group, ts!("cleanup.it.old_logs"), d, "Logs", Verdict::Safe, ts!("cleanup.why.tool_logs"));
        }
    }
    // a workspace's state (chats, open files) for a project that's gone
    for w in children(&base.join("User/workspaceStorage")) {
        let Some(project) = std::fs::read_to_string(w.join("workspace.json")).ok().and_then(|s| workspace_folder(&s)) else { continue };
        if !project.exists() && age_days(&w) > FRESH_DAYS {
            let name = fname(&project);
            c.add(e.group, &t!("cleanup.it.workspace", name = name), w, "Old Sessions", Verdict::Ask, ts!("cleanup.why.workspace_gone"));
        }
    }
    if e.group == Group::AiTools {
        for root in [base.to_path_buf(), base.join("Default")] {
            for sub in WEB_CACHES {
                let p = root.join(sub);
                if p.is_dir() {
                    c.add(e.group, ts!("cleanup.it.web_cache"), p, "Caches", Verdict::Safe, ts!("cleanup.why.webview"));
                }
            }
        }
    }
}

/// The local project folder a workspace.json points to (`"folder"` or a
/// `"workspace"` file), if it's on this Mac.
pub fn workspace_folder(json: &str) -> Option<PathBuf> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let uri = v.get("folder").or_else(|| v.get("workspace"))?.as_str()?;
    let path = uri.strip_prefix("file://")?;
    let decoded = percent_decode(path)?;
    let p = PathBuf::from(decoded);
    p.is_absolute().then_some(p)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

// ---------------------------------------------------------------- models

/// Ollama: one item per model (its manifest and the blobs no other model
/// uses), and the blobs no model uses at all.
fn ollama(c: &mut Ctx, models: &Path) {
    let manifests = models.join("manifests");
    let blobs = models.join("blobs");
    if !manifests.is_dir() {
        return;
    }
    c.tool = OLLAMA;
    let list = ollama_manifests(&manifests);
    let mut users: HashMap<String, usize> = HashMap::new();
    for (_, _, digests) in &list {
        for d in digests {
            *users.entry(d.clone()).or_default() += 1;
        }
    }
    for (name, manifest, digests) in &list {
        let own: Vec<PathBuf> = digests.iter().filter(|d| users.get(*d) == Some(&1)).map(|d| blobs.join(blob_file(d))).filter(|p| p.is_file()).collect();
        let bytes: u64 = own.iter().filter_map(|p| p.metadata().ok()).map(|m| m.len()).sum();
        let mut files = vec![manifest.clone()];
        files.extend(own);
        let when = super::engine::ago(age_days(manifest));
        c.add_action(Group::AiTools, name, manifest.clone(), "Local Models", Verdict::Ask, &t!("cleanup.why.model", when = when), Action::Model(files), bytes);
    }
    let used: HashSet<String> = users.into_keys().map(|d| blob_file(&d)).collect();
    for b in children(&blobs) {
        let n = fname(&b);
        let base = n.split("-partial").next().unwrap_or(&n).to_string();
        if b.is_file() && !used.contains(&base) && age_days(&b) > FRESH_DAYS {
            let (title, why) = if n.contains("-partial") { (ts!("cleanup.it.unfinished_downloads"), ts!("cleanup.why.unfinished")) } else { (ts!("cleanup.it.unused_model_parts"), ts!("cleanup.why.unused_blob")) };
            c.add(Group::AiTools, title, b, "Caches", Verdict::Safe, why);
        }
    }
}

/// "sha256:abc" → "sha256-abc", the blob's file name.
pub fn blob_file(digest: &str) -> String {
    digest.replacen(':', "-", 1)
}

/// (name, manifest file, layer + config digests) of every model:
/// manifests/<host>/<namespace>/<model>/<tag>.
fn ollama_manifests(root: &Path) -> Vec<(String, PathBuf, Vec<String>)> {
    let mut out = Vec::new();
    for host in children(root) {
        for ns in children(&host) {
            for model in children(&ns) {
                for tag in children(&model).into_iter().filter(|p| p.is_file()) {
                    let Ok(text) = std::fs::read_to_string(&tag) else { continue };
                    let Some(digests) = manifest_digests(&text) else { continue };
                    let name = model_name(&fname(&host), &fname(&ns), &fname(&model), &fname(&tag));
                    out.push((name, tag, digests));
                }
            }
        }
    }
    out
}

/// How `ollama list` names a model: "llama3.1:8b", "user/model:tag",
/// "host/user/model:tag".
pub fn model_name(host: &str, ns: &str, model: &str, tag: &str) -> String {
    match (host, ns) {
        ("registry.ollama.ai", "library") => format!("{model}:{tag}"),
        ("registry.ollama.ai", _) => format!("{ns}/{model}:{tag}"),
        _ => format!("{host}/{ns}/{model}:{tag}"),
    }
}

/// The blob digests an Ollama manifest names (config and layers).
pub fn manifest_digests(json: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut out: Vec<String> = Vec::new();
    if let Some(d) = v.pointer("/config/digest").and_then(|d| d.as_str()) {
        out.push(d.to_string());
    }
    for l in v.get("layers")?.as_array()? {
        if let Some(d) = l.get("digest").and_then(|d| d.as_str()) {
            out.push(d.to_string());
        }
    }
    out.retain(|d| d.starts_with("sha256:") && d.len() > 7 && d[7..].chars().all(|ch| ch.is_ascii_hexdigit()));
    (!out.is_empty()).then_some(out)
}

/// Remove a model's files. Model files are huge and Ollama fetches them
/// again with `ollama pull`, so they are deleted, not moved to the Trash.
/// The manifest goes first: a model without it is simply not listed.
pub fn remove_model(files: &[PathBuf]) -> Result<(), String> {
    let models = home().join(".ollama/models");
    let root = env_dir("OLLAMA_MODELS").unwrap_or(models);
    for f in files {
        if !f.starts_with(&root) || f.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(ts!("cleanup.why.outside_home").into());
        }
    }
    for f in files {
        match std::fs::remove_file(f) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

fn hugging_face(c: &mut Ctx, h: &Path) {
    let hf = env_dir("HF_HOME").unwrap_or_else(|| h.join(".cache/huggingface"));
    let hub = env_dir("HF_HUB_CACHE").unwrap_or_else(|| hf.join("hub"));
    c.tool = HUGGING_FACE;
    for repo in children(&hub) {
        let Some(name) = hf_repo_name(&fname(&repo)) else { continue };
        for f in children(&repo.join("blobs")) {
            if fname(&f).ends_with(".incomplete") && age_days(&f) > FRESH_DAYS {
                c.add(Group::AiTools, ts!("cleanup.it.unfinished_downloads"), f, "Caches", Verdict::Safe, ts!("cleanup.why.unfinished"));
            }
        }
        let when = super::engine::ago(newest_age(&repo.join("snapshots")));
        c.add(Group::AiTools, &name, repo, "Local Models", Verdict::Ask, &t!("cleanup.why.model", when = when));
    }
    c.add(Group::AiTools, ts!("cleanup.it.download_cache"), hf.join("xet"), "Caches", Verdict::Safe, ts!("cleanup.why.download_cache"));
}

/// "models--org--name" → "org/name" (datasets and spaces too).
pub fn hf_repo_name(dir: &str) -> Option<String> {
    let rest = ["models--", "datasets--", "spaces--"].iter().find_map(|p| dir.strip_prefix(p))?;
    (!rest.is_empty()).then(|| rest.replace("--", "/"))
}

fn lm_studio(c: &mut Ctx, h: &Path) {
    c.tool = LM_STUDIO;
    for root in [h.join(".lmstudio/models"), h.join(".cache/lm-studio/models")] {
        for publisher in children(&root).into_iter().filter(|p| p.is_dir() && !fname(p).starts_with('.')) {
            for model in children(&publisher).into_iter().filter(|p| p.is_dir()) {
                let name = format!("{}/{}", fname(&publisher), fname(&model));
                let when = super::engine::ago(age_days(&model));
                c.add(Group::AiTools, &name, model, "Local Models", Verdict::Ask, &t!("cleanup.why.model", when = when));
            }
        }
    }
}

// ---------------------------------------------------------------- worktrees

/// Folders agents keep their worktrees in. Claude Code's live inside each
/// repository (`<repo>/.claude/worktrees`).
fn worktree_roots(h: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![(h.join(".codex/worktrees"), CODEX), (h.join(".cursor/worktrees"), CURSOR), (h.join("conductor/workspaces"), "Conductor")]
}

/// A git worktree an agent made, in one of the agents' worktree folders:
/// the only kind of checkout Cleanup may offer (everything else with a .git
/// is a project and never offered).
pub fn is_agent_worktree(p: &Path) -> bool {
    let h = home();
    let in_root = worktree_roots(&h).iter().any(|(r, _)| p.starts_with(r) && p != r) || p.parent().is_some_and(|d| d.ends_with(".claude/worktrees"));
    in_root && gitdir(p).is_some() && !crate::kit::fs::never_remove(p)
}

/// The git folder a linked worktree's `.git` file points to
/// ("gitdir: <repo>/.git/worktrees/<name>").
fn gitdir(wt: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(wt.join(".git")).ok()?;
    parse_gitdir(&text, wt)
}

pub fn parse_gitdir(text: &str, wt: &Path) -> Option<PathBuf> {
    let rest = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if rest.is_empty() {
        return None;
    }
    let p = PathBuf::from(rest);
    let p = if p.is_absolute() { p } else { wt.join(p) };
    // a linked worktree, not a submodule (".git/modules/…")
    p.components().rev().nth(1).is_some_and(|c| c.as_os_str() == "worktrees").then_some(p)
}

/// The repository a worktree belongs to: <repo>/.git/worktrees/<name> → <repo>.
pub fn repo_of(gitdir: &Path) -> Option<PathBuf> {
    gitdir.parent()?.parent().filter(|g| g.ends_with(".git"))?.parent().map(Path::to_path_buf)
}

fn worktrees(c: &mut Ctx, h: &Path) {
    let mut found: Vec<(PathBuf, &'static str)> = Vec::new();
    for (root, tool) in worktree_roots(h) {
        // <root>/<name> or <root>/<id>/<name>
        for d in children(&root).into_iter().filter(|p| p.is_dir()) {
            if d.join(".git").is_file() {
                found.push((d, tool));
            } else {
                found.extend(children(&d).into_iter().filter(|p| p.join(".git").is_file()).map(|p| (p, tool)));
            }
        }
    }
    for p in crate::kit::claude_code::scan_projects() {
        if let Some(cwd) = p.cwd.filter(|_| p.cwd_exists) {
            found.extend(children(&cwd.join(".claude/worktrees")).into_iter().filter(|p| p.join(".git").is_file()).map(|p| (p, "Claude Code")));
        }
    }
    if found.is_empty() {
        return;
    }
    let Some(git) = super::run::find_tool(&["/opt/homebrew/bin/git", "/usr/local/bin/git", "/Library/Developer/CommandLineTools/usr/bin/git", "/Applications/Xcode.app/Contents/Developer/usr/bin/git"]) else { return };
    let mut seen = HashSet::new();
    c.tool = WORKTREES;
    for (wt, tool) in found {
        if c.stopped() {
            return;
        }
        if !seen.insert(wt.clone()) || !is_agent_worktree(&wt) {
            continue;
        }
        let Some(gd) = gitdir(&wt) else { continue };
        let repo = repo_of(&gd).map(|r| fname(&r)).unwrap_or_else(|| fname(&wt));
        if !gd.exists() {
            let title = format!("{repo} · {}", fname(&wt));
            c.add(Group::AiTools, &title, wt, WORKTREE_KIND, Verdict::Ask, &t!("cleanup.why.worktree_orphan", tool = tool));
            continue;
        }
        let branch = git_out(&git, &wt, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD").unwrap_or_else(|| fname(&wt));
        let title = format!("{repo} · {branch}");
        let (verdict, why) = match worktree_state(&git, &wt, &gd) {
            WtState::Merged(base) => (Verdict::Ask, t!("cleanup.why.worktree_merged", tool = tool, base = base).into_owned()),
            WtState::Changes => (Verdict::Keep, ts!("cleanup.why.worktree_changes").to_string()),
            WtState::Recent => (Verdict::Keep, ts!("cleanup.why.worktree_recent").to_string()),
            WtState::Unmerged => (Verdict::Keep, ts!("cleanup.why.worktree_unmerged").to_string()),
        };
        c.add(Group::AiTools, &title, wt.clone(), WORKTREE_KIND, verdict, &why);
        if verdict == Verdict::Keep {
            if let Some(it) = c.items.last_mut().filter(|i| i.path == wt) {
                it.hard_keep = true;
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum WtState {
    /// Clean, and everything in it is in the main branch (named).
    Merged(String),
    /// Uncommitted or untracked files.
    Changes,
    /// Used in the last day: an agent may be working in it.
    Recent,
    /// Has commits the main branch doesn't, or no main branch to compare with.
    Unmerged,
}

fn worktree_state(git: &Path, wt: &Path, gitdir: &Path) -> WtState {
    let recent = [gitdir.join("HEAD"), gitdir.join("index"), wt.to_path_buf()].iter().any(|p| p.exists() && age_days(p) < FRESH_DAYS);
    match git_out(git, wt, &["status", "--porcelain", "--untracked-files=normal"]) {
        Some(s) if s.is_empty() => {}
        // changed files, or git couldn't tell: kept
        _ => return WtState::Changes,
    }
    if recent {
        return WtState::Recent;
    }
    let base = ["origin/HEAD", "origin/main", "origin/master", "main", "master"].into_iter().find(|b| git_ok(git, wt, &["rev-parse", "--verify", "--quiet", b]));
    let Some(base) = base else { return WtState::Unmerged };
    if git_ok(git, wt, &["merge-base", "--is-ancestor", "HEAD", base]) {
        let name = git_out(git, wt, &["rev-parse", "--abbrev-ref", base]).unwrap_or_else(|| base.to_string());
        WtState::Merged(name.trim_start_matches("origin/").to_string())
    } else {
        WtState::Unmerged
    }
}

fn git_cmd(git: &Path, wt: &Path, args: &[&str]) -> Option<std::process::Output> {
    let mut cmd = Command::new(git);
    cmd.arg("-C").arg(wt).args(args).env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0");
    super::run::output(&mut cmd, Duration::from_secs(10))
}

fn git_out(git: &Path, wt: &Path, args: &[&str]) -> Option<String> {
    git_cmd(git, wt, args).filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn git_ok(git: &Path, wt: &Path, args: &[&str]) -> bool {
    git_cmd(git, wt, args).is_some_and(|o| o.status.success())
}

// ---------------------------------------------------------------- while running

/// A tool that's open may still be writing its newest files: those wait for
/// the person's OK. (Claude Code has its own check in `scan_claude`.)
fn while_running(c: &mut Ctx) {
    for (tool, names, ids) in PROCESSES {
        if !c.items.iter().any(|i| i.tool == tool && i.verdict == Verdict::Safe) {
            continue;
        }
        let on = names.iter().any(|n| c.running_names.contains(*n)) || ids.iter().any(|i| c.running_ids.contains(*i)) || names.iter().any(|n| process_running(n));
        if !on {
            continue;
        }
        let why = t!("cleanup.why.tool_running", tool = tool).into_owned();
        for it in c.items.iter_mut().filter(|i| i.tool == tool && i.verdict == Verdict::Safe && i.age_days < FRESH_DAYS && i.kind != "Caches") {
            it.verdict = Verdict::Ask;
            it.rule_verdict = Verdict::Ask;
            it.reason = why.clone();
            it.rule_reason = why.clone();
        }
    }
}

fn process_running(name: &str) -> bool {
    !name.contains(' ') && Command::new("/usr/bin/pgrep").args(["-x", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths() {
        assert_eq!(workspace_folder(r#"{"folder":"file:///Users/me/Code/my%20app"}"#), Some(PathBuf::from("/Users/me/Code/my app")));
        assert_eq!(workspace_folder(r#"{"workspace":"file:///Users/me/w.code-workspace"}"#), Some(PathBuf::from("/Users/me/w.code-workspace")));
        assert_eq!(workspace_folder(r#"{"folder":"vscode-remote://ssh-remote%2Bbox/home/me"}"#), None);
        assert_eq!(workspace_folder(r#"{"folder":"file:///Users/me/%D0%9C%D1%83%D0%B7"}"#), Some(PathBuf::from("/Users/me/Муз")));
        assert_eq!(workspace_folder("not json"), None);
        assert_eq!(workspace_folder(r#"{"folder":"file:///bad%zz"}"#), None);
    }

    #[test]
    fn ollama_names_and_digests() {
        assert_eq!(model_name("registry.ollama.ai", "library", "llama3.1", "8b"), "llama3.1:8b");
        assert_eq!(model_name("registry.ollama.ai", "me", "m", "latest"), "me/m:latest");
        assert_eq!(model_name("hf.co", "org", "m", "q4"), "hf.co/org/m:q4");
        let m = r#"{"config":{"digest":"sha256:aa11"},"layers":[{"digest":"sha256:bb22"},{"digest":"sha256:../x"},{"digest":"md5:cc"}]}"#;
        assert_eq!(manifest_digests(m), Some(vec!["sha256:aa11".to_string(), "sha256:bb22".to_string()]));
        assert_eq!(manifest_digests(r#"{"layers":[]}"#), None);
        assert_eq!(blob_file("sha256:aa11"), "sha256-aa11");
    }

    #[test]
    fn model_removal_stays_in_the_models_folder() {
        assert!(remove_model(&[PathBuf::from("/etc/hosts")]).is_err());
        let inside = home().join(".ollama/models/../../secret");
        assert!(remove_model(&[inside]).is_err());
    }

    #[test]
    fn hugging_face_names() {
        assert_eq!(hf_repo_name("models--openai--whisper-large-v3").as_deref(), Some("openai/whisper-large-v3"));
        assert_eq!(hf_repo_name("datasets--org--set").as_deref(), Some("org/set"));
        assert_eq!(hf_repo_name(".locks"), None);
        assert_eq!(hf_repo_name("models--"), None);
    }

    #[test]
    fn gitdir_of_linked_worktrees_only() {
        let wt = Path::new("/Users/me/.codex/worktrees/ab12/app");
        assert_eq!(parse_gitdir("gitdir: /Users/me/Code/app/.git/worktrees/app\n", wt), Some(PathBuf::from("/Users/me/Code/app/.git/worktrees/app")));
        assert_eq!(parse_gitdir("gitdir: /Users/me/Code/app/.git/modules/sub\n", wt), None);
        assert_eq!(parse_gitdir("ref: main", wt), None);
        assert_eq!(repo_of(Path::new("/Users/me/Code/app/.git/worktrees/app")), Some(PathBuf::from("/Users/me/Code/app")));
    }

    #[test]
    fn only_agent_worktree_folders() {
        let dir = std::env::temp_dir().join(format!("cmdmeow-wt-{}", std::process::id()));
        let wt = dir.join("app");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /x/app/.git/worktrees/app\n").unwrap();
        // a worktree outside the agents' folders is a project: never offered
        assert!(!is_agent_worktree(&wt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gemini_project_ids() {
        assert!(is_hex_id(&"a1".repeat(32)));
        assert!(!is_hex_id("bin"));
        assert!(!is_hex_id(&"z1".repeat(32)));
    }

    #[test]
    fn editors_split_by_group() {
        assert!(ai_editor_dir("Cursor"));
        assert!(!ai_editor_dir("Code"));
        assert!(!ai_editor_dir("Slack"));
    }
}
