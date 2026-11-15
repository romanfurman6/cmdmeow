//! The Keep Awake state machine: the Off · Auto · On mode, the AI agents'
//! activity, the wait after they go quiet, the assertions that follow from
//! them and today's log. Times are unix seconds so tests can drive it.

use super::power::{Power, Want};
use super::sessions::{Session, Source};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A session that wrote to its log this recently is working.
pub const WORKING_SECS: i64 = 120;
/// Auto stays awake this long after the last agent goes quiet.
pub const GRACE_SECS: i64 = 5 * 60;

/// The one control: the Mac sleeps as usual, stays awake while an AI agent
/// works (and a little after), or stays awake with the screen on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Auto,
    On,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::Auto, Mode::On];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Off => "Off",
            Mode::Auto => "Auto",
            Mode::On => "On",
        }
    }
}

/// Where On was turned on, for today's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// A panel widget.
    Panel,
    /// The tool's shortcut.
    Shortcut,
    /// The control on the page.
    Page,
    /// `cmdmeow://awake/on`.
    Link,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Auto: awake while an agent works. On (the switch) sits on top of it,
    /// so turning On off goes back to Auto. New installs start at Off; a
    /// saved "Stay awake while Claude works" carries over as Auto.
    pub auto: bool,
    /// System-wide shortcut that turns On on and off.
    pub shortcut: Option<crate::kit::hotkeys::Shortcut>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    /// On: awake, screen on, until turned off.
    On,
    /// Auto, and an agent is working.
    Agent,
    /// Auto, the agents went quiet; sleep is allowed again in `left` seconds.
    WindDown { left: i64 },
}

impl State {
    pub fn awake(self) -> bool {
        self != State::Off
    }
}

/// A stretch of awake time today.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: i64,
    pub end: i64,
    /// Kept awake by Auto (else On). Saved as "claude".
    #[serde(rename = "claude")]
    pub agent: bool,
    /// The first agent; spans saved before there was a choice were Claude's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Source>,
    /// Auto: each agent and project that worked during it, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub who: Vec<(Source, String)>,
    /// On: where it was turned on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<Via>,
}

impl Span {
    /// The agents in it, in order; spans from before `who` name only `by`.
    pub fn agents(&self) -> Vec<Source> {
        if !self.agent {
            return Vec::new();
        }
        let mut v: Vec<Source> = Vec::new();
        for (a, _) in &self.who {
            if !v.contains(a) {
                v.push(*a);
            }
        }
        if v.is_empty() {
            v.push(self.by.unwrap_or(Source::Claude));
        }
        v
    }

    /// Its projects, in order, without repeats.
    pub fn projects(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for (_, p) in &self.who {
            if !v.contains(&p.as_str()) {
                v.push(p);
            }
        }
        v
    }
}

/// The span under way.
#[derive(Clone, Debug, PartialEq)]
struct Open {
    start: i64,
    /// None: Auto, else On and where from.
    via: Option<Via>,
    who: Vec<(Source, String)>,
}

pub struct Engine {
    pub settings: Settings,
    /// On: since when, and where it was turned on.
    pub on: Option<(i64, Via)>,
    /// Agents' recent sessions, newest first.
    pub sessions: Vec<Session>,
    /// When the Mac last went from sleeping-allowed to awake.
    pub awake_since: Option<i64>,
    /// Closed spans of today (and the open one is `open`).
    pub log: Vec<Span>,
    open: Option<Open>,
    /// When an agent was last seen working, while the streak lasts.
    agent_last: Option<i64>,
    /// The agents that worked last, for "Claude finished".
    pub last_agents: Vec<Source>,
    /// The tool is turned on in Settings.
    enabled: bool,
    power: Power,
    dir: Option<PathBuf>,
}

impl Engine {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let (settings, log, on) = match &dir {
            Some(d) => (load_settings(d), load_log(d, today_start(now())), load_switch(d)),
            None => (Settings::default(), Vec::new(), None),
        };
        // On stays on across a quit, an update or a restart (turning the
        // tool off in Settings ends it)
        Engine { settings, on, sessions: Vec::new(), awake_since: None, log, open: None, agent_last: None, last_agents: Vec::new(), enabled: true, power: Power::default(), dir }
    }

    pub fn mode(&self) -> Mode {
        if self.on.is_some() {
            Mode::On
        } else if self.settings.auto {
            Mode::Auto
        } else {
            Mode::Off
        }
    }

    /// Agents are read only with Auto picked (also under On, so turning
    /// On off finds a working agent at once).
    pub fn watching(&self) -> bool {
        self.enabled && self.settings.auto
    }

    pub fn working(&self, now: i64) -> Vec<&Session> {
        self.sessions.iter().filter(|s| s.working(now)).collect()
    }

    /// The agents working now, in a fixed order.
    pub fn working_agents(&self, now: i64) -> Vec<Source> {
        let w = self.working(now);
        Source::ALL.into_iter().filter(|a| w.iter().any(|s| s.source == *a)).collect()
    }

    pub fn state(&self, now: i64) -> State {
        if !self.enabled {
            return State::Off;
        }
        match self.mode() {
            Mode::Off => State::Off,
            Mode::On => State::On,
            Mode::Auto => {
                if !self.working(now).is_empty() {
                    return State::Agent;
                }
                match self.agent_last {
                    Some(t) if t + GRACE_SECS > now => State::WindDown { left: t + GRACE_SECS - now },
                    _ => State::Off,
                }
            }
        }
    }

    #[cfg(test)]
    pub fn held(&self) -> Want {
        self.power.held()
    }

    /// Bring everything in line with the state at `now`.
    pub fn step(&mut self, now: i64) {
        if self.watching() {
            let working = self.working(now);
            if let Some(last) = working.iter().map(|s| s.active_at(now)).max() {
                // an agent counts as working until WORKING_SECS after its last write
                self.agent_last = Some(self.agent_last.unwrap_or(0).max(last + WORKING_SECS));
                self.last_agents = self.working_agents(now);
            }
        } else {
            self.agent_last = None;
        }
        let st = self.state(now);
        if st == State::Off {
            self.agent_last = None;
        }
        let reason = self.reason(st, now);
        // On keeps the screen on; Auto lets it sleep
        self.power.apply(Want { system: st.awake(), display: st == State::On }, &reason);
        match (st.awake(), self.awake_since) {
            (true, None) => self.awake_since = Some(now),
            (false, Some(_)) => self.awake_since = None,
            _ => {}
        }
        // the open span follows what keeps the Mac up: On (from where) or Auto
        let kind = st.awake().then(|| if st == State::On { self.on.map(|o| o.1) } else { None });
        if self.open.as_ref().map(|o| o.via) != kind {
            self.close_span(now);
            self.open = kind.map(|via| Open { start: now, via, who: Vec::new() });
        }
        if st == State::Agent {
            let mut seen: Vec<(Source, String)> = self.working(now).iter().map(|s| (s.source, s.project.clone())).collect();
            // in a fixed order: agent, then as found
            seen.sort_by_key(|(a, _)| Source::ALL.iter().position(|x| x == a));
            if let Some(o) = self.open.as_mut() {
                for w in seen {
                    if !o.who.contains(&w) {
                        o.who.push(w);
                    }
                }
            }
        }
    }

    /// Who's keeping the Mac up, for `pmset -g assertions`.
    fn reason(&self, st: State, now: i64) -> String {
        match st {
            State::Off => String::new(),
            State::On => "by hand".into(),
            State::Agent => self
                .working_agents(now)
                .into_iter()
                .map(|a| {
                    let mut names: Vec<&str> = Vec::new();
                    for s in self.working(now).into_iter().filter(|s| s.source == a) {
                        if !names.contains(&s.project.as_str()) {
                            names.push(&s.project);
                        }
                    }
                    format!("{}: {}", a.name(), names.join(", "))
                })
                .collect::<Vec<_>>()
                .join("; "),
            State::WindDown { .. } => format!("{} just finished", names(&self.last_agents)),
        }
    }

    /// Pick a mode on the control.
    pub fn set_mode(&mut self, mode: Mode, via: Via, now: i64) {
        let auto = match mode {
            Mode::Off => false,
            Mode::Auto => true,
            Mode::On => self.settings.auto,
        };
        if auto != self.settings.auto {
            self.change(now, |s| s.auto = auto);
        }
        self.set_on(mode == Mode::On, via, now);
    }

    /// The shortcut, a tap on the small widget and `/toggle`: On, or back
    /// to the mode before it.
    pub fn toggle(&mut self, via: Via, now: i64) {
        let on = self.on.is_none();
        self.set_on(on, via, now);
    }

    fn set_on(&mut self, on: bool, via: Via, now: i64) {
        if on != self.on.is_some() {
            crate::kit::sound::play(if on { crate::kit::sound::Cue::Awake } else { crate::kit::sound::Cue::Asleep });
        }
        self.set_switch(if on { Some(self.on.unwrap_or((now, via))) } else { None });
        self.step(now);
    }

    pub fn set_enabled(&mut self, on: bool, now: i64) {
        self.enabled = on;
        if !on {
            self.set_switch(None);
        }
        self.step(now);
    }

    fn set_switch(&mut self, on: Option<(i64, Via)>) {
        if on == self.on {
            return;
        }
        self.on = on;
        if let Some(d) = &self.dir {
            save_json(d, SWITCH_FILE, &on.map(|(since, via)| Switch::Now { since, via }));
        }
    }

    pub fn change(&mut self, now: i64, f: impl FnOnce(&mut Settings)) {
        let auto = self.settings.auto;
        f(&mut self.settings);
        if auto != self.settings.auto {
            super::MODE_CHANGED.kick();
        }
        if let Some(d) = &self.dir {
            save_json(d, "settings.json", &self.settings);
        }
        self.step(now);
    }

    /// Today's spans, oldest first, the open one included up to `now`.
    pub fn today(&self, now: i64) -> Vec<Span> {
        let day = today_start(now);
        let mut v: Vec<Span> = self.log.iter().filter(|s| s.end > day).cloned().collect();
        if let Some(o) = &self.open {
            v.push(open_span(o, now));
        }
        for s in &mut v {
            s.start = s.start.max(day);
        }
        v
    }

    /// The span under way, if any.
    pub fn current(&self, now: i64) -> Option<Span> {
        self.open.as_ref().map(|o| open_span(o, now))
    }

    fn close_span(&mut self, now: i64) {
        let Some(o) = self.open.take() else { return };
        if now - o.start < 30 {
            return;
        }
        // only today is shown
        let day = today_start(now);
        self.log.retain(|s| s.end > day);
        self.log.push(open_span(&o, now));
        if let Some(d) = &self.dir {
            save_json(d, "today.json", &self.log);
        }
    }
}

fn open_span(o: &Open, now: i64) -> Span {
    Span { start: o.start, end: now, agent: o.via.is_none(), by: o.who.first().map(|w| w.0), who: o.who.clone(), via: o.via }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close_span(now());
    }
}

/// "Claude", "Claude, Codex".
pub fn names(agents: &[Source]) -> String {
    agents.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Unix seconds of local midnight before `t`.
pub fn today_start(t: i64) -> i64 {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(t, 0)
        .single()
        .and_then(|d| d.date_naive().and_hms_opt(0, 0, 0))
        .and_then(|m| Local.from_local_datetime(&m).earliest())
        .map(|m| m.timestamp())
        .unwrap_or(t - t.rem_euclid(86_400))
}

pub fn data_dir() -> PathBuf {
    crate::kit::fs::data_path("keep_awake")
}

fn load_settings(dir: &Path) -> Settings {
    std::fs::read(dir.join("settings.json")).map(|b| settings_from(&b)).unwrap_or_default()
}

/// settings.json; one saved before Off · Auto · On (no "auto", the agent
/// switch as "claude") keeps working: its agent switch on becomes Auto.
fn settings_from(json: &[u8]) -> Settings {
    let raw: serde_json::Value = serde_json::from_slice(json).unwrap_or_default();
    let mut s: Settings = serde_json::from_value(raw.clone()).unwrap_or_default();
    if raw.get("auto").is_none() && raw.get("claude").and_then(|v| v.as_bool()) == Some(true) {
        s.auto = true;
    }
    s
}

/// When On was turned on, if it's on.
const SWITCH_FILE: &str = "switch.json";

/// switch.json: the time alone before On said where it came from.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Switch {
    Now { since: i64, via: Via },
    Old(i64),
}

fn load_switch(dir: &Path) -> Option<(i64, Via)> {
    crate::kit::fs::load_json::<Option<Switch>>(&dir.join(SWITCH_FILE)).map(|s| match s {
        Switch::Now { since, via } => (since, via),
        Switch::Old(since) => (since, Via::Panel),
    })
}

fn load_log(dir: &Path, day: i64) -> Vec<Span> {
    let v: Vec<Span> = crate::kit::fs::load_json(&dir.join("today.json"));
    v.into_iter().filter(|s| s.end > day).collect()
}

fn save_json<T: Serialize>(dir: &Path, name: &str, v: &T) {
    crate::kit::fs::save_json(&dir.join(name), v, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(t: i64) -> Session {
        Session { source: Source::Claude, project: "cmdmeow".into(), last_write: t, tool_running: false }
    }

    fn auto() -> Engine {
        let mut e = Engine::new(None);
        e.set_mode(Mode::Auto, Via::Page, 0);
        e
    }

    #[test]
    fn off_by_default() {
        let mut e = Engine::new(None);
        e.sessions = vec![session(1000)];
        e.step(1000);
        assert_eq!(e.mode(), Mode::Off);
        assert_eq!(e.state(1000), State::Off);
        assert_eq!(e.held(), Want::default());
    }

    #[test]
    fn on_keeps_awake_with_screen() {
        let mut e = Engine::new(None);
        e.set_mode(Mode::On, Via::Page, 1000);
        assert_eq!(e.state(1000), State::On);
        assert_eq!(e.held(), Want { system: true, display: true });
        e.set_mode(Mode::Off, Via::Page, 1100);
        assert_eq!(e.held(), Want::default());
    }

    #[test]
    fn auto_work_then_wind_down_then_sleep() {
        let mut e = auto();
        e.sessions = vec![session(1000)];
        e.step(1010);
        assert_eq!(e.state(1010), State::Agent);
        // the screen may sleep
        assert_eq!(e.held(), Want { system: true, display: false });
        // quiet: working until 1000 + 120, then 5 min
        e.step(1200);
        assert_eq!(e.state(1200), State::WindDown { left: 1120 + 300 - 1200 });
        assert!(e.held().system);
        e.step(1500);
        assert_eq!(e.state(1500), State::Off);
        assert_eq!(e.mode(), Mode::Auto);
        assert_eq!(e.held(), Want::default());
        // a new write starts a fresh streak
        e.sessions = vec![session(2000)];
        e.step(2001);
        assert_eq!(e.state(2001), State::Agent);
    }

    #[test]
    fn any_agent_counts() {
        let mut e = auto();
        e.sessions = vec![Session { source: Source::Codex, project: "api".into(), last_write: 1000, tool_running: false }, session(990)];
        e.step(1001);
        assert_eq!(e.state(1001), State::Agent);
        assert_eq!(e.working_agents(1001), vec![Source::Claude, Source::Codex]);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude: cmdmeow; Codex: api)");
        e.step(1200);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude, Codex just finished)");
    }

    #[test]
    fn leaving_on_finds_the_agent_at_once() {
        let mut e = auto();
        e.set_mode(Mode::On, Via::Page, 900);
        assert!(e.watching());
        e.sessions = vec![session(1000)];
        e.step(1000);
        e.toggle(Via::Page, 1010);
        assert_eq!(e.state(1010), State::Agent);
        // Off reads nothing
        e.set_mode(Mode::Off, Via::Page, 1020);
        assert!(!e.watching());
    }

    #[test]
    fn toggle_goes_back_to_the_mode_before() {
        let mut e = auto();
        e.toggle(Via::Shortcut, 10);
        assert_eq!(e.mode(), Mode::On);
        assert_eq!(e.on, Some((10, Via::Shortcut)));
        e.toggle(Via::Shortcut, 20);
        assert_eq!(e.mode(), Mode::Auto);
        e.set_mode(Mode::Off, Via::Page, 30);
        e.toggle(Via::Panel, 40);
        e.toggle(Via::Panel, 50);
        assert_eq!(e.mode(), Mode::Off);
    }

    #[test]
    fn tool_off_means_no_trigger() {
        let mut e = auto();
        e.sessions = vec![session(1000)];
        e.step(1001);
        assert_eq!(e.state(1001), State::Agent);
        e.set_mode(Mode::On, Via::Page, 1003);
        e.set_enabled(false, 1004);
        assert_eq!(e.state(1004), State::Off);
        assert_eq!(e.on, None);
        assert_eq!(e.held(), Want::default());
    }

    #[test]
    fn spans_follow_the_reason() {
        let mut e = Engine::new(None);
        let day = today_start(now());
        let t = day + 3600;
        e.set_mode(Mode::On, Via::Shortcut, t);
        e.set_mode(Mode::Auto, Via::Page, t + 600);
        e.sessions = vec![session(t + 1000), Session { source: Source::Codex, project: "api".into(), last_write: t + 1000, tool_running: false }];
        e.step(t + 1000);
        let v = e.today(t + 2000);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].start, v[0].end, v[0].agent, v[0].via), (t, t + 600, false, Some(Via::Shortcut)));
        assert_eq!((v[1].start, v[1].agent), (t + 1000, true));
        assert_eq!(v[1].agents(), vec![Source::Claude, Source::Codex]);
        assert_eq!(v[1].projects(), vec!["cmdmeow", "api"]);
    }

    #[test]
    fn on_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("cmdmeow-awake-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let mut e = Engine::new(Some(dir.clone()));
            e.set_mode(Mode::On, Via::Link, 1000);
        }
        let mut e = Engine::new(Some(dir.clone()));
        assert_eq!(e.on, Some((1000, Via::Link)));
        e.step(2000);
        assert_eq!(e.state(2000), State::On);
        assert!(e.held().system);
        e.set_mode(Mode::Auto, Via::Page, 2100);
        drop(e);
        let e = Engine::new(Some(dir.clone()));
        assert_eq!((e.on, e.mode()), (None, Mode::Auto));
        drop(e);
        // turning the tool off in Settings turns On off for good
        let mut e = Engine::new(Some(dir.clone()));
        e.set_mode(Mode::On, Via::Page, 3000);
        e.set_enabled(false, 3001);
        drop(e);
        assert_eq!(Engine::new(Some(dir.clone())).on, None);
        // an old switch.json (the time alone) still reads as On
        std::fs::write(dir.join(SWITCH_FILE), "1234").unwrap();
        assert_eq!(Engine::new(Some(dir.clone())).on, Some((1234, Via::Panel)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reason_names_who_keeps_it_up() {
        let mut e = auto();
        e.step(1000);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake");
        e.set_mode(Mode::On, Via::Page, 1000);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (by hand)");
        e.set_mode(Mode::Auto, Via::Page, 1001);
        e.sessions = vec![session(1002), Session { source: Source::Claude, project: "api".into(), last_write: 1002, tool_running: false }, session(1002)];
        e.step(1003);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude: cmdmeow, api)");
        e.step(1200);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude just finished)");
    }

    #[test]
    fn old_files_still_read() {
        // an old agent switch that was on carries over as Auto
        assert!(settings_from(br#"{"claude": true, "claude_screen": true, "grace_min": 15}"#).auto);
        assert!(!settings_from(br#"{"claude": false}"#).auto);
        // once saved in the new form, "auto" wins
        assert!(!settings_from(br#"{"claude": true, "auto": false}"#).auto);
        assert!(!settings_from(b"").auto);
        let v: Vec<Span> = serde_json::from_str(r#"[{"start":1,"end":2,"claude":true},{"start":3,"end":4,"claude":true,"by":"codex"},{"start":5,"end":6,"claude":false}]"#).unwrap();
        let who: Vec<_> = v.iter().map(Span::agents).collect();
        assert_eq!(who, vec![vec![Source::Claude], vec![Source::Codex], vec![]]);
        assert!(!serde_json::to_string(&v[2]).unwrap().contains("by"));
    }
}
