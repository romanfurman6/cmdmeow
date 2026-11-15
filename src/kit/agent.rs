//! The AI agent CLIs Cmd+Meow can ask (Claude Code or Codex): Cleanup's
//! path check and every tool's AI Check.
//! Scanned file names are untrusted text in the prompt, so an agent is only
//! offered when it runs without tools (Claude) or in a read-only sandbox
//! (Codex).
//! Each one gets the same system rules and numbered path lines, and answers
//! with the same JSON verdicts, so the judge doesn't care which one ran.

use crate::kit::claude_code;
use crate::kit::mechanics as m;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "Claude",
            Agent::Codex => "Codex",
        }
    }

    fn exe(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    /// The agent's CLI on this Mac.
    pub fn bin(self) -> Option<PathBuf> {
        match self {
            Agent::Claude => claude_code::claude_bin(),
            _ => find_bin(self.exe()),
        }
    }

    /// Whether it can judge paths now: its CLI is installed (Claude: and not
    /// known to be signed out). The AI Usage tool being off doesn't matter.
    /// A signed-out CLI fails and the built-in rules are used.
    pub fn ready(self) -> bool {
        match self {
            Agent::Claude => claude_code::cli_ready(),
            _ => self.bin().is_some(),
        }
    }

    /// A few words on why it can't be used, for the results subtitle.
    pub fn why_not(self) -> String {
        match self {
            Agent::Claude => claude_code::availability().short().to_string(),
            _ => format!("{} CLI not installed", self.name()),
        }
    }

    /// Ask the agent: `system` rules, then the `lines` to judge. Returns its
    /// answer text and what it cost: tokens from the CLI's own report
    /// (Claude), or a size estimate when the CLI doesn't give one. Gives up
    /// (Err) when `cancel` is set, killing the CLI.
    pub fn ask_usage(self, system: &str, lines: &str, cancel: &Arc<AtomicBool>) -> Result<(String, Usage), String> {
        let bin = self.bin().ok_or_else(|| self.why_not())?;
        let tmp = std::env::temp_dir();
        let mut cmd = Command::new(&bin);
        cmd.current_dir(&tmp);
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{path}"));
        let both = format!("{system}\n\n{lines}");
        let last = tmp.join(format!("cmdmeow-cleanup-{}.txt", std::process::id()));
        match self {
            Agent::Claude => {
                cmd.args(["-p", lines, "--system-prompt", system, "--output-format", "json", "--no-session-persistence", "--model", m::AI_MODEL, "--tools", "", "--strict-mcp-config"]);
            }
            Agent::Codex => {
                // read-only sandbox, no repo needed; the last message goes to a file
                let _ = std::fs::remove_file(&last);
                cmd.args(["exec", "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never", "--output-last-message"]).arg(&last).arg(&both);
            }
        }
        let out = run(cmd, cancel)?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("").to_string();
            return Err(err);
        }
        Ok(match self {
            Agent::Claude => {
                let v = serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap_or_default();
                let text = v["result"].as_str().unwrap_or_default().to_string();
                (text, Usage::from_claude(&v))
            }
            Agent::Codex => {
                let s = std::fs::read_to_string(&last).unwrap_or_else(|_| String::from_utf8_lossy(&out.stdout).into_owned());
                let _ = std::fs::remove_file(&last);
                let u = Usage::estimate(both.len(), s.len());
                (s, u)
            }
        })
    }
}

/// Tokens one answer took, and its price when the CLI reports one.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    /// Dollars at API prices, as the CLI reported it.
    pub cost: Option<f64>,
    /// True when the tokens are guessed from the text size.
    pub estimate: bool,
}

impl Usage {
    /// From `claude -p --output-format json`: `usage` (input, cache and
    /// output tokens) and `total_cost_usd`.
    pub fn from_claude(v: &serde_json::Value) -> Usage {
        let u = &v["usage"];
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        Usage {
            input: n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens"),
            output: n("output_tokens"),
            cost: v["total_cost_usd"].as_f64(),
            estimate: false,
        }
    }

    /// About four characters a token.
    pub fn estimate(sent: usize, got: usize) -> Usage {
        Usage { input: (sent / 4) as u64, output: (got / 4) as u64, cost: None, estimate: true }
    }

    pub fn tokens(&self) -> u64 {
        self.input + self.output
    }
}

/// Run `cmd` to the end, or kill it as soon as `cancel` is set.
fn run(mut cmd: Command, cancel: &Arc<AtomicBool>) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    // read both pipes on their own threads so a big answer can't block the CLI
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let ot = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });
    let et = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("stopped".into());
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(std::process::Output { status, stdout: ot.join().unwrap_or_default(), stderr: et.join().unwrap_or_default() })
}

/// An executable named `exe` in the usual install places, even when the app
/// was started from Finder (no shell PATH).
fn find_bin(exe: &str) -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs_: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs_.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    dirs_.extend([".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin"].map(|d| home.join(d)));
    dirs_.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}


// ---------------------------------------------------------------- the choice

impl Agent {
    pub fn id(self) -> &'static str {
        self.exe()
    }
}

/// The agent Cleanup checks with: the main agent picked in setup (Settings ›
/// AI Usage), when it can judge paths safely; otherwise the first one that
/// can and is turned on; Claude when none is.
pub fn main() -> Agent {
    let s = crate::kit::ai::settings::get();
    pick(&s.main, |id| s.is_on(id))
}

fn pick(main: &str, on: impl Fn(&str) -> bool) -> Agent {
    Agent::ALL
        .into_iter()
        .find(|a| a.id() == main)
        .or_else(|| Agent::ALL.into_iter().find(|a| on(a.id())))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_agent_falls_back_to_one_that_can_judge() {
        assert_eq!(pick("codex", |_| true), Agent::Codex);
        assert_eq!(pick("claude", |_| true), Agent::Claude);
        // Gemini can't judge paths with its tools off: the next one on does
        assert_eq!(pick("gemini", |id| id == "codex"), Agent::Codex);
        assert_eq!(pick("gemini", |_| false), Agent::Claude);
    }

    #[test]
    fn usage_reads_claude_json() {
        let v: serde_json::Value = serde_json::from_str(r#"{"result":"[]","total_cost_usd":0.0123,"usage":{"input_tokens":10,"cache_read_input_tokens":900,"cache_creation_input_tokens":90,"output_tokens":40}}"#).unwrap();
        let u = Usage::from_claude(&v);
        assert_eq!((u.input, u.output, u.tokens()), (1000, 40, 1040));
        assert_eq!(u.cost, Some(0.0123));
        assert!(!u.estimate);
        let e = Usage::estimate(400, 80);
        assert_eq!((e.input, e.output, e.cost, e.estimate), (100, 20, None, true));
    }

    #[test]
    fn names_round_trip() {
        for a in Agent::ALL {
            let s = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Agent>(&s).unwrap(), a);
        }
        assert_eq!(serde_json::to_string(&Agent::Codex).unwrap(), "\"codex\"");
    }

    #[test]
    fn cancel_kills_a_running_command() {
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.store(true, Ordering::SeqCst);
        });
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let t = std::time::Instant::now();
        assert!(run(cmd, &cancel).is_err());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn run_returns_output() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut cmd = Command::new("echo");
        cmd.arg("hi");
        let out = run(cmd, &cancel).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }
}
