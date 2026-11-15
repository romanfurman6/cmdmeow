//! The busiest apps and processes right now, and quitting one.

/// One running process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    /// Percent of one core (can pass 100).
    pub cpu: f32,
    /// Runs as the current user, so it can be quit without admin rights.
    pub mine: bool,
}

/// How many the CPU sheet lists.
pub const TOP_N: usize = 5;
/// Below this a process isn't worth listing.
const MIN_CPU: f32 = 1.0;

/// The `n` busiest, busiest first, without this app and the idle ones.
pub fn top(mut all: Vec<Proc>, me: u32, n: usize) -> Vec<Proc> {
    all.retain(|p| p.pid != me && p.pid != 0 && p.cpu >= MIN_CPU && !p.name.is_empty());
    all.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.pid.cmp(&b.pid)));
    all.truncate(n);
    all
}

/// Every process with its CPU use since the last refresh of `sys`.
pub fn read(sys: &mut sysinfo::System) -> Vec<Proc> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_user(UpdateKind::OnlyIfNotSet));
    let uid = unsafe { libc::getuid() };
    sys.processes()
        .values()
        .map(|p| Proc {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            cpu: p.cpu_usage(),
            mine: p.user_id().is_some_and(|u| **u == uid),
        })
        .collect()
}

/// Ask a process to quit (SIGTERM, like Activity Monitor's Quit).
pub fn quit(pid: u32) -> bool {
    pid > 1 && pid != std::process::id() && unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, cpu: f32) -> Proc {
        Proc { pid, name: name.into(), cpu, mine: true }
    }

    #[test]
    fn busiest_first_without_me_and_idle() {
        let all = vec![p(10, "Safari", 12.0), p(11, "cmdmeow", 50.0), p(12, "idle", 0.2), p(13, "WindowServer", 30.0), p(0, "kernel_task", 90.0), p(14, "", 40.0), p(15, "Xcode", 12.0)];
        let t = top(all, 11, 3);
        assert_eq!(t.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![13, 10, 15]);
    }

    #[test]
    fn never_quits_init_or_itself() {
        assert!(!quit(0));
        assert!(!quit(1));
        assert!(!quit(std::process::id()));
    }
}
