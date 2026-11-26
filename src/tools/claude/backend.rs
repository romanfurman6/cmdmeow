//! The background loop behind the usage tool. It drives every AI provider
//! that's turned on: account and limits on a slow timer (they also feed
//! the menu bar and the notifications, so they refresh while hidden),
//! local tokens while the panel or window is open, the status pages now
//! and then. Each provider's last limits and daily totals are kept, so the
//! history chart has more than this month's logs.

use crate::kit::activity::{self, Every, Wake};
use crate::kit::ai::{self, alerts, history, settings, status, UsageInfo};
use crate::kit::claude_code as cc;
use crate::kit::mechanics as m;
use crate::kit::sync::LockExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use crate::kit::ai::{human_tokens, short_label, Shared};

/// One `Shared` per provider, in `ai::PROVIDERS` order.
pub type All = Arc<Vec<Shared>>;

const STATUS_SECS: u64 = 15 * 60;

/// The provider shown: the picked one, or the first that's on when "All"
/// is picked.
pub fn current(all: &All) -> &Shared {
    let i = ai::selected();
    if i < all.len() {
        return &all[i];
    }
    let s = settings::get();
    s.ordered().into_iter().find(|k| s.is_on(ai::PROVIDERS[*k].id)).map(|i| &all[i]).unwrap_or(&all[0])
}

/// The provider the small widgets show (W13): the first in the person's
/// order that's on and installed.
pub fn widget_pick(all: &All) -> Option<usize> {
    let s = settings::get();
    s.ordered().into_iter().find(|&k| s.is_on(ai::PROVIDERS[k].id) && all.get(k).is_some_and(|i| i.lock_ok().unavailable() != Some(crate::kit::claude_code::Availability::NotInstalled)))
}

/// [`widget_pick`] for Settings, which has no `All` of its own.
pub fn widget_provider() -> Option<usize> {
    widget_pick(SHOWN.get()?)
}

/// The providers turned on in Settings, with their data.
pub fn enabled(all: &All) -> Vec<UsageInfo> {
    let s = settings::get();
    s.ordered().into_iter().filter(|k| s.is_on(ai::PROVIDERS[*k].id)).filter_map(|k| all.get(k)).map(|i| with_accounts(i.lock_ok().clone(), &s)).collect()
}

static SHOWN: std::sync::OnceLock<All> = std::sync::OnceLock::new();

/// Claude's other accounts, by config folder ("~/…"), as the loop last
/// checked them.
static EXTRA: Mutex<Vec<(String, Shared)>> = Mutex::new(Vec::new());

/// Claude's info with its other accounts attached (`UsageInfo::accounts`),
/// each named; unchanged while there are none.
pub fn with_accounts(mut info: UsageInfo, set: &settings::Settings) -> UsageInfo {
    if info.provider != ai::claude::NAME || set.accounts.is_empty() {
        return info;
    }
    let extra = EXTRA.lock_ok();
    info.account = set.main_account();
    info.folder = "~/.claude".into();
    info.accounts = set
        .accounts
        .iter()
        .map(|a| {
            let mut i = extra.iter().find(|(d, _)| *d == a.dir).map(|(_, s)| s.lock_ok().clone()).unwrap_or_else(|| UsageInfo { loading: true, ..UsageInfo::new(ai::claude::NAME) });
            i.account = a.name.clone();
            i.folder = a.dir.clone();
            i
        })
        .collect();
    info
}

/// Demo: a second Claude account, "Work", with its own limits and logs
/// (not saved).
pub fn demo_accounts() {
    let mut work = ai::claude::demo_info();
    for l in work.limits.iter_mut() {
        l.pct = (l.pct * 2.2 + 12.0).min(100.0);
    }
    for d in work.local.days.iter_mut() {
        d.tokens /= 2;
        d.usd /= 2.0;
    }
    work.scanned = true;
    let dir = "~/.claude-work".to_string();
    *EXTRA.lock_ok() = vec![(dir.clone(), Arc::new(Mutex::new(work)))];
    settings::update(false, |s| s.accounts = vec![settings::Account { name: "Work".into(), dir }]);
}

/// One of Claude's other accounts, as the loop keeps it.
struct Extra {
    dir: String,
    p: ai::claude::ClaudeCode,
    s: Shared,
    limits_at: Every,
    local_at: Every,
}

/// Keep `extras` in step with Settings: new accounts get a provider, removed
/// ones go. Publishes the list for the page.
fn sync_extras(extras: &mut Vec<Extra>, set: &settings::Settings) {
    extras.retain(|x| set.accounts.iter().any(|a| a.dir == x.dir));
    for a in &set.accounts {
        if !extras.iter().any(|x| x.dir == a.dir) {
            extras.push(Extra {
                dir: a.dir.clone(),
                p: ai::claude::ClaudeCode::for_dir(a.path()),
                s: Arc::new(Mutex::new(UsageInfo { loading: true, ..UsageInfo::new(ai::claude::NAME) })),
                limits_at: Every::default(),
                local_at: Every::default(),
            });
        }
    }
    *EXTRA.lock_ok() = extras.iter().map(|x| (x.dir.clone(), x.s.clone())).collect();
}

/// Check Claude's other accounts that are due: limits through the CLI with
/// their folder, tokens from their own logs.
fn run_extras(ctx: &egui::Context, extras: &mut [Extra], on: bool) {
    use ai::Provider;
    for x in extras.iter_mut() {
        if !on {
            let mut g = x.s.lock_ok();
            g.loading = false;
            g.scanned = true;
            continue;
        }
        let retry_empty = x.s.lock_ok().limits.is_empty() && x.limits_at.age().is_some_and(|a| a.as_secs() > 60);
        if retry_empty || x.limits_at.due(Duration::from_secs(m::CLAUDE_LIMITS_REFRESH_SECS)) {
            x.limits_at.mark();
            x.p.fetch_account(&x.s);
        }
        if (activity::visible() || x.local_at.age().is_none()) && x.local_at.due(Duration::from_secs(m::CLAUDE_LOCAL_REFRESH_SECS)) {
            let local = x.p.local_tokens();
            x.s.lock_ok().local = local;
        }
        let mut g = x.s.lock_ok();
        g.loading = false;
        g.scanned = true;
        drop(g);
        activity::repaint(ctx);
    }
}

/// What provider `i` (an index into `ai::PROVIDERS`) last reported, for
/// Settings.
pub fn snapshot(i: usize) -> Option<UsageInfo> {
    SHOWN.get()?.get(i).map(|s| s.lock_ok().clone())
}

pub fn start(ctx: egui::Context, demo: bool) -> All {
    let providers: Vec<_> = (0..ai::ALL).map(ai::make).collect();
    if demo {
        let all: All = Arc::new(providers.iter().map(|p| Arc::new(Mutex::new(UsageInfo { scanned: true, ..p.demo() }))).collect());
        let _ = SHOWN.set(all.clone());
        return all;
    }
    let all: All = Arc::new(providers.iter().map(|p| Arc::new(Mutex::new(UsageInfo { loading: true, ..UsageInfo::new(p.name()) }))).collect());
    let shared = all.clone();
    let _ = SHOWN.set(all.clone());
    std::thread::Builder::new()
        .name("ai-usage".into())
        .spawn(move || {
            // the old status-line hook is no longer needed
            ai::statusline::cleanup();
            run(ctx, providers, shared)
        })
        .ok();
    all
}

/// A check someone asked for (Refresh, Check again) is running. Set on the
/// click, so the spinner shows at once, and cleared when the loop has been
/// through every provider: clicks in between start nothing new.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// A refresh someone asked for is still running.
pub fn refreshing() -> bool {
    REFRESHING.load(Ordering::Relaxed)
}

/// Providers in the order one pass checks them: the one on screen first,
/// so its page fills before the others (Claude's CLI takes seconds).
fn order(n: usize, shown: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    if shown < n {
        v.retain(|i| *i != shown);
        v.insert(0, shown);
    }
    v
}

fn run(ctx: egui::Context, mut providers: Vec<Box<dyn ai::Provider>>, all: All) {
    activity::low_priority();
    let n = providers.len();
    // due right away the first time
    let mut limits_at = vec![Every::default(); n];
    let mut local_at = vec![Every::default(); n];
    let mut status_at = Every::default();
    let mut memory: Vec<alerts::Memory> = (0..n).map(|_| alerts::Memory::default()).collect();
    let mut primed = vec![false; n];
    let mut extras: Vec<Extra> = Vec::new();
    // this pass answers a Refresh: every provider shows its spinner until its turn
    let mut asked = false;
    loop {
        let _pool = crate::kit::macos::pool();
        let set = settings::get();
        let now = chrono::Utc::now().timestamp();
        if asked {
            for (i, s) in all.iter().enumerate() {
                if set.is_on(ai::PROVIDERS[i].id) {
                    s.lock_ok().loading = true;
                }
            }
            activity::repaint(&ctx);
        }
        for i in order(n, ai::selected()) {
            let (p, s) = (&mut providers[i], &all[i]);
            let id = ai::PROVIDERS[i].id;
            // turned off: no CLI calls, no log reading
            if !set.is_on(id) || !p.enabled() {
                limits_at[i].reset();
                let mut g = s.lock_ok();
                g.loading = false;
                g.scanned = true;
                continue;
            }
            // a provider with no limits yet asks again each minute, quietly:
            // `loading` is only for the first check and a Refresh, so the
            // page doesn't swap its layout every minute
            let retry_empty = ai::PROVIDERS[i].limits && s.lock_ok().limits.is_empty() && limits_at[i].age().is_some_and(|a| a.as_secs() > 60);
            if retry_empty || limits_at[i].due(Duration::from_secs(m::CLAUDE_LIMITS_REFRESH_SECS)) {
                limits_at[i].mark();
                p.fetch_account(s);
            }
            // local logs of every provider that's on, so switching tabs only
            // changes what's drawn; the cache reads only what was added since.
            // Hidden, it waits until the panel or window opens (the nap ends then)
            if (activity::visible() || local_at[i].age().is_none()) && local_at[i].due(Duration::from_secs(m::CLAUDE_LOCAL_REFRESH_SECS)) {
                let today = chrono::Local::now().date_naive();
                // once: every old log into the saved days, read outside the
                // history lock (it can take a while)
                if !history::with(id, |h| (h.backfilled, false)) {
                    let mut old = p.old_tokens();
                    history::with(id, |h| {
                        if let Some(old) = old.as_mut() {
                            h.merge_days(old, today);
                        }
                        h.backfilled = true;
                        ((), true)
                    });
                }
                let mut local = p.local_tokens();
                // saved only when a day changed: the file holds every day kept
                history::with(id, |h| ((), h.merge_days(&mut local, today)));
                let mut g = s.lock_ok();
                g.local = local;
            }
            let info = {
                let mut g = s.lock_ok();
                g.loading = false;
                g.scanned = true;
                g.clone()
            };
            activity::repaint(&ctx);
            if !info.limits.is_empty() {
                history::with(id, |h| ((), h.record(&info.limits, now)));
            }
            // the first check after launch only learns what's already past,
            // so a restart doesn't repeat alerts
            let first = !primed[i];
            let due = alerts::check(&info, &set, &mut memory[i], now);
            if !info.limits.is_empty() || !info.local.days.is_empty() {
                primed[i] = true;
            }
            if !first {
                for a in due {
                    crate::kit::notify::send(&a.title, &a.body);
                }
            }
        }
        // Claude's other accounts, after the providers
        sync_extras(&mut extras, &set);
        let claude_on = set.is_on("claude") && cc::tool_on();
        if asked {
            for x in extras.iter() {
                x.s.lock_ok().loading = claude_on;
            }
        }
        run_extras(&ctx, &mut extras, claude_on);
        if asked {
            asked = false;
            REFRESHING.store(false, Ordering::Relaxed);
            activity::repaint(&ctx);
        }
        // outages: a status page per provider that has one
        if set.show_outages && status_at.due(Duration::from_secs(STATUS_SECS)) {
            status_at.mark();
            for (i, meta) in ai::PROVIDERS.iter().enumerate() {
                if set.is_on(meta.id) && !meta.status_url.is_empty() {
                    let report = status::fetch(meta.status_url);
                    let mut s = all[i].lock_ok();
                    s.incident = report.as_ref().map(|r| r.level.clone());
                    s.incidents = report.map(|r| r.incidents).unwrap_or_default();
                }
            }
            activity::repaint(&ctx);
        } else if !set.show_outages {
            for s in all.iter() {
                let mut s = s.lock_ok();
                s.incident = None;
                s.incidents.clear();
            }
        }
        // wait, but wake early when someone asks for a refresh or the panel opens
        if activity::nap(Duration::from_secs(m::CLAUDE_LOCAL_REFRESH_SECS), Some(cc::recheck())) == Wake::Kicked {
            for e in limits_at.iter_mut().chain(local_at.iter_mut()) {
                e.reset();
            }
            for x in extras.iter_mut() {
                x.limits_at.reset();
                x.local_at.reset();
            }
            status_at.reset();
            asked = true;
        }
    }
}

/// Check every provider again now. Does nothing while a check someone
/// asked for is still running (a second click, or Refresh then Check again).
pub fn request_refresh() {
    if !REFRESHING.swap(true, Ordering::Relaxed) {
        cc::request_recheck();
    }
}

/// Show `i` (an index into `ai::PROVIDERS`, or `ai::ALL`) from now on.
/// Only what's drawn changes: every provider's data is already there.
pub fn select(i: usize, demo: bool) {
    ai::select(i, !demo);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shown_provider_goes_first() {
        assert_eq!(order(4, 1), vec![1, 0, 2, 3]);
        assert_eq!(order(4, 0), vec![0, 1, 2, 3]);
        // "All": the usual order
        assert_eq!(order(4, 4), vec![0, 1, 2, 3]);
    }

    #[test]
    fn a_second_refresh_waits_for_the_first() {
        REFRESHING.store(false, Ordering::Relaxed);
        request_refresh();
        assert!(refreshing());
        // the loop clears it after its pass; until then clicks start nothing
        assert!(cc::recheck().take());
        request_refresh();
        assert!(!cc::recheck().take());
        REFRESHING.store(false, Ordering::Relaxed);
    }
}
