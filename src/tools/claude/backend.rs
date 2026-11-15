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
    ai::PROVIDERS.iter().position(|m| s.is_on(m.id)).map(|i| &all[i]).unwrap_or(&all[0])
}

/// The providers turned on in Settings, with their data.
pub fn enabled(all: &All) -> Vec<UsageInfo> {
    let s = settings::get();
    ai::PROVIDERS.iter().zip(all.iter()).filter(|(m, _)| s.is_on(m.id)).map(|(_, i)| i.lock_ok().clone()).collect()
}

static SHOWN: std::sync::OnceLock<All> = std::sync::OnceLock::new();

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
    let mut size_at = vec![Every::default(); n];
    let mut status_at = Every::default();
    let mut memory: Vec<alerts::Memory> = (0..n).map(|_| alerts::Memory::default()).collect();
    let mut primed = vec![false; n];
    // this pass answers a Refresh: every provider shows its spinner until its turn
    let mut asked = false;
    loop {
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
                let mut local = p.local_tokens();
                history::with(id, |h| {
                    h.merge_days(&mut local, chrono::Local::now().date_naive());
                    ((), true)
                });
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
        if asked {
            asked = false;
            REFRESHING.store(false, Ordering::Relaxed);
            activity::repaint(&ctx);
        }
        // the data folder walks every file: once per launch, and again on
        // Refresh, after every provider shows its numbers
        if !activity::recording() {
            for i in 0..n {
                if set.is_on(ai::PROVIDERS[i].id) && providers[i].enabled() && size_at[i].age().is_none() {
                    size_at[i].mark();
                    let d = providers[i].data_dir().map(|d| crate::kit::fs::dir_size(&d)).unwrap_or(0);
                    all[i].lock_ok().data_bytes = d;
                    activity::repaint(&ctx);
                }
            }
        }
        // outages: a status page per provider that has one
        if set.show_outages && status_at.due(Duration::from_secs(STATUS_SECS)) {
            status_at.mark();
            for (i, meta) in ai::PROVIDERS.iter().enumerate() {
                if set.is_on(meta.id) && !meta.status_url.is_empty() {
                    let incident = status::fetch(meta.status_url);
                    all[i].lock_ok().incident = incident;
                }
            }
            activity::repaint(&ctx);
        } else if !set.show_outages {
            for s in all.iter() {
                s.lock_ok().incident = None;
            }
        }
        // wait, but wake early when someone asks for a refresh or the panel opens
        if activity::nap(Duration::from_secs(m::CLAUDE_LOCAL_REFRESH_SECS), Some(cc::recheck())) == Wake::Kicked {
            for e in limits_at.iter_mut().chain(local_at.iter_mut()).chain(size_at.iter_mut()) {
                e.reset();
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
