//! The Keep Awake state machine: the switch, the main AI agent's activity,
//! the wait after it goes quiet, the assertions that follow from them and today's
//! log. Times are unix seconds so tests can drive it.

use super::power::{Power, Want};
use super::sessions::{Session, Source};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A session that wrote to its log this recently is working.
pub const WORKING_SECS: i64 = 120;
/// Choices for the wait after the agent goes quiet, in minutes.
pub const GRACE_CHOICES: [u32; 3] = [1, 5, 15];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Stay awake while the main agent works. (Saved as "claude", from
    /// when Claude Code was the only one.)
    #[serde(rename = "claude")]
    pub agent: bool,
    /// Keep the screen on when turned on by hand.
    pub manual_screen: bool,
    /// Keep the screen on while the agent works.
    #[serde(rename = "claude_screen")]
    pub agent_screen: bool,
    /// Minutes to stay awake after the agent goes quiet.
    pub grace_min: u32,
    /// System-wide shortcut that flips the switch.
    pub shortcut: Option<crate::kit::hotkeys::Shortcut>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { agent: true, manual_screen: true, agent_screen: false, grace_min: 5, shortcut: None }
    }
}

impl Settings {
    fn sanitize(mut self) -> Self {
        if !GRACE_CHOICES.contains(&self.grace_min) {
            self.grace_min = 5;
        }
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    /// The switch is on.
    Manual,
    /// The main agent is working.
    Agent,
    /// The agent went quiet; sleep is allowed again in `left` seconds.
    WindDown { left: i64 },
}

impl State {
    pub fn awake(self) -> bool {
        self != State::Off
    }
}

/// A stretch of awake time today.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub start: i64,
    pub end: i64,
    /// Kept awake for an agent (else by hand). Saved as "claude".
    #[serde(rename = "claude")]
    pub agent: bool,
    /// Which agent; spans saved before there was a choice were Claude's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Source>,
}

impl Span {
    /// The agent it was kept awake for, None when by hand.
    pub fn who(&self) -> Option<Source> {
        self.agent.then(|| self.by.unwrap_or(Source::Claude))
    }
}

pub struct Engine {
    pub settings: Settings,
    /// When the switch was turned on.
    pub manual_since: Option<i64>,
    /// The agent being watched: the main one, when it's on this Mac.
    pub agent: Option<Source>,
    /// Its recent sessions, newest first.
    pub sessions: Vec<Session>,
    /// When the Mac last went from sleeping-allowed to awake.
    pub awake_since: Option<i64>,
    /// Closed spans of today (and the open one is `open`).
    pub log: Vec<Span>,
    /// The span under way: its start and who it's for (None: by hand).
    open: Option<(i64, Option<Source>)>,
    /// When the agent was last seen working, while its streak lasts.
    agent_last: Option<i64>,
    /// The tool is turned on in Settings.
    enabled: bool,
    power: Power,
    dir: Option<PathBuf>,
}

impl Engine {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let (settings, log, manual_since) = match &dir {
            Some(d) => (load_settings(d), load_log(d, today_start(now()) - 6 * 86_400), load_switch(d)),
            None => (Settings::default(), Vec::new(), None),
        };
        // the switch stays on across a quit, an update or a restart
        // without a data folder (tests, demos) it watches Claude
        let agent = if dir.is_some() { Source::main() } else { Some(Source::Claude) };
        Engine { settings, manual_since, agent, sessions: Vec::new(), awake_since: None, log, open: None, agent_last: None, enabled: true, power: Power::default(), dir }
    }

    /// The trigger is on and there's an agent to watch.
    pub fn watching(&self) -> bool {
        self.settings.agent && self.agent.is_some()
    }

    /// The watched agent's name ("Claude", "Codex"…), "" when none.
    pub fn agent_name(&self) -> &'static str {
        self.agent.map(Source::name).unwrap_or("")
    }

    /// Watch `agent` from now on (the main agent changed); its sessions
    /// come with the next scan.
    pub fn set_agent(&mut self, agent: Option<Source>, now: i64) {
        if agent != self.agent {
            self.agent = agent;
            self.sessions.clear();
            // the old agent's streak ends with it: no wind-down for an
            // agent that isn't watched any more
            self.agent_last = None;
            self.step(now);
        }
    }

    pub fn working(&self, now: i64) -> Vec<&Session> {
        self.sessions.iter().filter(|s| s.working(now)).collect()
    }

    pub fn state(&self, now: i64) -> State {
        if !self.enabled {
            return State::Off;
        }
        if self.manual_since.is_some() {
            return State::Manual;
        }
        if !self.watching() {
            return State::Off;
        }
        if !self.working(now).is_empty() {
            return State::Agent;
        }
        match self.agent_last {
            Some(t) => {
                let left = t + self.settings.grace_min as i64 * 60 - now;
                if left > 0 { State::WindDown { left } } else { State::Off }
            }
            None => State::Off,
        }
    }

    #[cfg(test)]
    pub fn held(&self) -> Want {
        self.power.held()
    }

    /// Bring everything in line with the state at `now`.
    pub fn step(&mut self, now: i64) {
        if self.enabled && self.watching() {
            if let Some(last) = self.working(now).iter().map(|s| s.active_at(now)).max() {
                // the agent counts as working until WORKING_SECS after its last write
                self.agent_last = Some(self.agent_last.unwrap_or(0).max(last + WORKING_SECS));
            }
        }
        let st = self.state(now);
        if st == State::Off {
            self.agent_last = None;
        }
        let display = match st {
            State::Off => false,
            State::Manual => self.settings.manual_screen,
            State::Agent | State::WindDown { .. } => self.settings.agent_screen,
        };
        let reason = self.reason(st, now);
        self.power.apply(Want { system: st.awake(), display }, &reason);
        match (st.awake(), self.awake_since) {
            (true, None) => self.awake_since = Some(now),
            (false, Some(_)) => self.awake_since = None,
            _ => {}
        }
        let kind = st.awake().then(|| if st == State::Manual { None } else { self.agent });
        if self.open.map(|o| o.1) != kind {
            self.close_span(now);
            self.open = kind.map(|k| (now, k));
        }
    }

    /// Who's keeping the Mac up, for `pmset -g assertions`.
    fn reason(&self, st: State, now: i64) -> String {
        match st {
            State::Off => String::new(),
            State::Manual => "by hand".into(),
            State::Agent => {
                let mut names: Vec<&str> = Vec::new();
                for s in self.working(now) {
                    if !names.contains(&s.project.as_str()) {
                        names.push(&s.project);
                    }
                }
                let who = self.agent_name();
                if names.is_empty() { who.into() } else { format!("{who}: {}", names.join(", ")) }
            }
            State::WindDown { .. } => format!("{} just finished", self.agent_name()),
        }
    }

    pub fn set_manual(&mut self, on: bool, now: i64) {
        if on != self.manual_since.is_some() {
            crate::kit::sound::play(if on { crate::kit::sound::Cue::Awake } else { crate::kit::sound::Cue::Asleep });
        }
        self.set_switch(if on { Some(self.manual_since.unwrap_or(now)) } else { None });
        self.step(now);
    }

    pub fn set_enabled(&mut self, on: bool, now: i64) {
        self.enabled = on;
        if !on {
            self.set_switch(None);
        }
        self.step(now);
    }

    fn set_switch(&mut self, since: Option<i64>) {
        if since == self.manual_since {
            return;
        }
        self.manual_since = since;
        if let Some(d) = &self.dir {
            save_json(d, SWITCH_FILE, &since);
        }
    }

    pub fn change(&mut self, now: i64, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        self.settings = self.settings.clone().sanitize();
        if let Some(d) = &self.dir {
            save_json(d, "settings.json", &self.settings);
        }
        self.step(now);
    }

    /// Today's spans, the open one included up to `now`.
    pub fn today(&self, now: i64) -> Vec<Span> {
        let day = today_start(now);
        let mut v: Vec<Span> = self.log.iter().filter(|s| s.end > day).copied().collect();
        if let Some((start, by)) = self.open {
            v.push(Span { start, end: now, agent: by.is_some(), by });
        }
        for s in &mut v {
            s.start = s.start.max(day);
        }
        v
    }

    /// The last 7 days, oldest first: (day start, seconds for agents, by hand).
    pub fn week(&self, now: i64) -> Vec<(i64, i64, i64)> {
        let today = today_start(now);
        let mut spans = self.log.clone();
        if let Some((start, by)) = self.open {
            spans.push(Span { start, end: now, agent: by.is_some(), by });
        }
        (0..7)
            .map(|k| {
                let d0 = today_start(today - (6 - k) * 86_400 + 3600);
                let d1 = if k == 6 { now } else { today_start(d0 + 86_400 + 3600) };
                let (mut c, mut h) = (0, 0);
                for s in &spans {
                    let len = (s.end.min(d1) - s.start.max(d0)).max(0);
                    if s.agent { c += len } else { h += len }
                }
                (d0, c, h)
            })
            .collect()
    }

    /// Agents that kept the Mac up in the last 7 days, in a fixed order.
    pub fn week_agents(&self, now: i64) -> Vec<Source> {
        let since = today_start(now) - 6 * 86_400;
        let mut spans: Vec<Span> = self.log.iter().filter(|s| s.end > since).copied().collect();
        spans.extend(self.today(now));
        Source::ALL.into_iter().filter(|a| spans.iter().any(|s| s.who() == Some(*a))).collect()
    }

    fn close_span(&mut self, now: i64) {
        let Some((start, by)) = self.open.take() else { return };
        if now - start < 30 {
            return;
        }
        // a week is kept, for the page's last 7 days
        let day = today_start(now) - 6 * 86_400;
        self.log.retain(|s| s.end > day);
        self.log.push(Span { start, end: now, agent: by.is_some(), by });
        if let Some(d) = &self.dir {
            save_json(d, "today.json", &self.log);
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close_span(now());
    }
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Seconds per agent in `spans`, in a fixed order, agents with none left out.
pub fn agent_time(spans: &[Span]) -> Vec<(Source, i64)> {
    Source::ALL
        .into_iter()
        .map(|a| (a, spans.iter().filter(|s| s.who() == Some(a)).map(|s| s.end - s.start).sum::<i64>()))
        .filter(|(_, t)| *t > 0)
        .collect()
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
    crate::kit::fs::load_json::<Settings>(&dir.join("settings.json")).sanitize()
}

/// When the switch was turned on, if it's on.
const SWITCH_FILE: &str = "switch.json";

fn load_switch(dir: &Path) -> Option<i64> {
    crate::kit::fs::load_json::<Option<i64>>(&dir.join(SWITCH_FILE))
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
        Session { project: "cmdmeow".into(), last_write: t, tool_running: false }
    }

    #[test]
    fn switch_keeps_awake_with_screen() {
        let mut e = Engine::new(None);
        e.step(1000);
        assert_eq!(e.state(1000), State::Off);
        assert_eq!(e.held(), Want::default());
        e.set_manual(true, 1000);
        assert_eq!(e.state(1000), State::Manual);
        assert_eq!(e.held(), Want { system: true, display: true });
        e.set_manual(false, 1100);
        assert_eq!(e.held(), Want::default());
    }

    #[test]
    fn claude_work_then_wind_down_then_sleep() {
        let mut e = Engine::new(None);
        e.sessions = vec![session(1000)];
        e.step(1010);
        assert_eq!(e.state(1010), State::Agent);
        assert_eq!(e.held(), Want { system: true, display: false });
        // quiet: working until 1000 + 120, then 5 min of grace
        e.step(1200);
        assert_eq!(e.state(1200), State::WindDown { left: 1120 + 300 - 1200 });
        assert!(e.held().system);
        e.step(1500);
        assert_eq!(e.state(1500), State::Off);
        assert_eq!(e.held(), Want::default());
        // a new write starts a fresh streak
        e.sessions = vec![session(2000)];
        e.step(2001);
        assert_eq!(e.state(2001), State::Agent);
    }

    #[test]
    fn claude_off_or_tool_off_means_no_trigger() {
        let mut e = Engine::new(None);
        e.sessions = vec![session(1000)];
        e.change(1001, |s| s.agent = false);
        assert_eq!(e.state(1001), State::Off);
        e.change(1002, |s| s.agent = true);
        assert_eq!(e.state(1002), State::Agent);
        e.set_manual(true, 1003);
        e.set_enabled(false, 1004);
        assert_eq!(e.state(1004), State::Off);
        assert_eq!(e.manual_since, None);
        assert_eq!(e.held(), Want::default());
    }

    #[test]
    fn claude_screen_setting_turns_display_on() {
        let mut e = Engine::new(None);
        e.sessions = vec![session(1000)];
        e.change(1001, |s| s.agent_screen = true);
        assert_eq!(e.held(), Want { system: true, display: true });
    }

    #[test]
    fn spans_follow_the_reason() {
        let mut e = Engine::new(None);
        let day = today_start(now());
        let t = day + 3600;
        e.set_manual(true, t);
        e.set_manual(false, t + 600);
        e.sessions = vec![session(t + 1000)];
        e.step(t + 1000);
        let v = e.today(t + 2000);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].start, v[0].end, v[0].agent), (t, t + 600, false));
        assert_eq!((v[1].start, v[1].agent), (t + 1000, true));
    }

    #[test]
    fn switch_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("cmdmeow-awake-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let mut e = Engine::new(Some(dir.clone()));
            e.set_manual(true, 1000);
        }
        let mut e = Engine::new(Some(dir.clone()));
        assert_eq!(e.manual_since, Some(1000));
        e.step(2000);
        assert_eq!(e.state(2000), State::Manual);
        assert!(e.held().system);
        e.set_manual(false, 2100);
        drop(e);
        assert_eq!(Engine::new(Some(dir.clone())).manual_since, None);
        // turning the tool off in Settings turns the switch off for good
        let mut e = Engine::new(Some(dir.clone()));
        e.set_manual(true, 3000);
        e.set_enabled(false, 3001);
        drop(e);
        assert_eq!(Engine::new(Some(dir.clone())).manual_since, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reason_names_who_keeps_it_up() {
        let mut e = Engine::new(None);
        e.step(1000);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake");
        e.set_manual(true, 1000);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (by hand)");
        e.set_manual(false, 1001);
        e.sessions = vec![session(1002), Session { project: "api".into(), last_write: 1002, tool_running: false }, session(1002)];
        e.step(1003);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude: cmdmeow, api)");
        e.step(1200);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Claude just finished)");
    }

    #[test]
    fn follows_the_main_agent() {
        let mut e = Engine::new(None);
        e.set_agent(Some(Source::Codex), 1000);
        e.sessions = vec![session(1000)];
        e.step(1001);
        assert_eq!(e.state(1001), State::Agent);
        assert_eq!(e.power.name(), "Cmd+Meow Keep Awake (Codex: cmdmeow)");
        // switching agents drops the old one's sessions
        e.set_agent(Some(Source::Gemini), 1002);
        assert!(e.sessions.is_empty());
        // no agent on this Mac: no trigger, whatever the setting says
        e.set_agent(None, 1003);
        e.sessions = vec![session(1003)];
        e.step(1004);
        assert!(e.settings.agent && !e.watching());
        assert_eq!(e.state(1004), State::Off);
        assert_eq!(e.agent_name(), "");
    }

    #[test]
    fn switching_agents_keeps_who_did_what() {
        let mut e = Engine::new(None);
        let day = today_start(now());
        let t = day + 3600;
        e.sessions = vec![session(t)];
        e.step(t);
        assert_eq!(e.state(t), State::Agent);
        // Claude is still busy when the main agent becomes Codex: no
        // leftover wind-down from Claude (was 6:54 instead of off/5:00)
        e.set_agent(Some(Source::Codex), t + 60);
        assert_eq!(e.state(t + 60), State::Off);
        // Codex works, then goes quiet: the wait is the full 5 minutes
        e.sessions = vec![session(t + 100)];
        e.step(t + 100);
        assert_eq!(e.state(t + 100), State::Agent);
        e.sessions = vec![session(t + 100)];
        let quiet = t + 100 + WORKING_SECS + 1;
        e.step(quiet);
        assert_eq!(e.state(quiet), State::WindDown { left: 299 });
        let spans = e.today(t + 200);
        let times = agent_time(&spans);
        assert_eq!(times, vec![(Source::Claude, 60), (Source::Codex, 100)], "{spans:?}");
        assert_eq!(e.week_agents(t + 200), vec![Source::Claude, Source::Codex]);
    }

    #[test]
    fn old_settings_files_still_read() {
        let s: Settings = serde_json::from_str(r#"{"claude": false, "claude_screen": true}"#).unwrap();
        assert!(!s.agent && s.agent_screen);
        let back = serde_json::to_string(&s).unwrap();
        assert!(back.contains("\"claude\":false"), "{back}");
        let v: Vec<Span> = serde_json::from_str(r#"[{"start":1,"end":2,"claude":true},{"start":3,"end":4,"claude":true,"by":"codex"},{"start":5,"end":6,"claude":false}]"#).unwrap();
        let who: Vec<_> = v.iter().map(Span::who).collect();
        assert_eq!(who, vec![Some(Source::Claude), Some(Source::Codex), None]);
        assert!(!serde_json::to_string(&v[2]).unwrap().contains("by"));
    }

    #[test]
    fn bad_grace_falls_back() {
        let s: Settings = serde_json::from_str("{\"grace_min\": 7}").unwrap();
        assert_eq!(s.sanitize().grace_min, 5);
    }
}
