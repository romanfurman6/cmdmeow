//! Running helper tools (lsof, xcrun, brew) with a time limit, so a hung tool
//! can never hold a scan or a clean.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd` and collect its output, or `None` if it can't start or takes
/// longer than `limit` (it is killed then).
pub fn output(cmd: &mut Command, limit: Duration) -> Option<Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    // read both pipes on their own threads so a chatty tool can't fill a pipe and stall
    let mut so = child.stdout.take()?;
    let mut se = child.stderr.take()?;
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = so.read_to_end(&mut b);
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    Some(Output { status, stdout: out_t.join().unwrap_or_default(), stderr: err_t.join().unwrap_or_default() })
}

/// `name`'s full path if it is one of the usual install places (never asks
/// macOS to install developer tools just to look).
pub fn find_tool(candidates: &[&str]) -> Option<std::path::PathBuf> {
    candidates.iter().map(std::path::PathBuf::from).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_output() {
        let o = output(Command::new("sh").args(["-c", "echo hi; echo err >&2; exit 3"]), Duration::from_secs(5)).unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hi");
        assert_eq!(String::from_utf8_lossy(&o.stderr).trim(), "err");
        assert_eq!(o.status.code(), Some(3));
    }

    #[test]
    fn stops_a_hung_tool() {
        let t = Instant::now();
        assert!(output(Command::new("sleep").arg("10"), Duration::from_millis(200)).is_none());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_tool() {
        assert!(output(&mut Command::new("/definitely/not/a/tool"), Duration::from_secs(1)).is_none());
        assert!(find_tool(&["/definitely/not/a/tool"]).is_none());
    }
}
