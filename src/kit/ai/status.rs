//! Outages: the providers' public status pages (Statuspage's
//! `/api/v2/summary.json`), so a slow or failing assistant shows why and
//! which incident it is.

use chrono::{DateTime, Local};
use serde_json::Value;

/// One open incident on a status page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Incident {
    /// "Elevated errors on Claude Opus 5.5".
    pub name: String,
    /// Statuspage's state: "investigating", "identified", "monitoring",
    /// "resolved" (empty when the page shows degraded parts but no incident).
    pub state: String,
    /// When it started, in local time.
    pub since: Option<DateTime<Local>>,
    /// The parts it touches: "Claude API", "Claude Code".
    pub affects: Vec<String>,
    /// The incident's own page.
    pub link: String,
}

/// What a status page reports right now: `level` "Degraded" for minor
/// trouble, "Outage" for major or critical.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub level: String,
    pub incidents: Vec<Incident>,
}

/// The report to show, from a status page's `summary.json` (or plain
/// `status.json`): `None` when all is well.
pub fn parse(json: &[u8]) -> Option<Report> {
    let v: Value = serde_json::from_slice(json).ok()?;
    let level = match v["status"]["indicator"].as_str()? {
        "none" | "maintenance" => return None,
        "minor" => "Degraded",
        _ => "Outage",
    };
    let s = |x: &Value| x.as_str().unwrap_or("").trim().to_owned();
    let mut incidents: Vec<Incident> = v["incidents"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| s(&i["status"]) != "resolved" && s(&i["status"]) != "postmortem")
                .map(|i| Incident {
                    name: s(&i["name"]),
                    state: s(&i["status"]),
                    since: i["started_at"].as_str().or(i["created_at"].as_str()).and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Local)),
                    affects: i["components"].as_array().map(|c| c.iter().map(|c| s(&c["name"])).filter(|n| !n.is_empty()).collect()).unwrap_or_default(),
                    link: s(&i["shortlink"]),
                })
                .filter(|i| !i.name.is_empty())
                .collect()
        })
        .unwrap_or_default();
    // degraded parts with no incident written up yet: name the parts
    if incidents.is_empty() {
        let parts: Vec<String> = v["components"]
            .as_array()
            .map(|c| c.iter().filter(|c| !c["group"].as_bool().unwrap_or(false) && s(&c["status"]) != "operational" && !s(&c["status"]).is_empty()).map(|c| s(&c["name"])).filter(|n| !n.is_empty()).collect())
            .unwrap_or_default();
        if !parts.is_empty() {
            incidents.push(Incident { name: parts.join(" · "), ..Default::default() });
        }
    }
    Some(Report { level: level.into(), incidents })
}

/// The summary of the status page whose `status.json` is `url`.
fn summary_url(url: &str) -> String {
    match url.strip_suffix("status.json") {
        Some(base) => format!("{base}summary.json"),
        None => url.to_owned(),
    }
}

/// Ask the status page at `url` (its `status.json`). `None` when all is
/// well or the page can't be reached (no badge rather than a wrong one).
pub fn fetch(url: &str) -> Option<Report> {
    if url.is_empty() {
        return None;
    }
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).build());
    let body = agent.get(&summary_url(url)).call().ok()?.body_mut().read_to_vec().ok()?;
    parse(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(j: &[u8]) -> Option<String> {
        parse(j).map(|r| r.level)
    }

    #[test]
    fn indicators() {
        assert_eq!(level(br#"{"status":{"indicator":"none","description":"All Systems Operational"}}"#), None);
        assert_eq!(level(br#"{"status":{"indicator":"minor","description":"Partial outage"}}"#).as_deref(), Some("Degraded"));
        assert_eq!(level(br#"{"status":{"indicator":"major"}}"#).as_deref(), Some("Outage"));
        assert_eq!(level(br#"{"status":{"indicator":"critical"}}"#).as_deref(), Some("Outage"));
        assert_eq!(parse(b"<html>"), None);
    }

    #[test]
    fn incidents_from_the_summary() {
        let j = br#"{"status":{"indicator":"minor"},
          "components":[{"name":"Claude API","status":"degraded_performance","group":false}],
          "incidents":[
            {"name":"Elevated errors on Claude Opus 5.5","status":"investigating","started_at":"2026-11-26T12:05:00.000Z",
             "shortlink":"https://stspg.io/abc","components":[{"name":"Claude API"},{"name":"Claude Code"}]},
            {"name":"Old one","status":"resolved","started_at":"2026-11-25T10:00:00Z","shortlink":"x","components":[]}]}"#;
        let r = parse(j).unwrap();
        assert_eq!(r.incidents.len(), 1, "resolved ones are left out");
        let i = &r.incidents[0];
        assert_eq!(i.name, "Elevated errors on Claude Opus 5.5");
        assert_eq!(i.state, "investigating");
        assert_eq!(i.affects, vec!["Claude API", "Claude Code"]);
        assert_eq!(i.link, "https://stspg.io/abc");
        assert_eq!(i.since.unwrap().with_timezone(&chrono::Utc).to_rfc3339(), "2026-11-26T12:05:00+00:00");
    }

    #[test]
    fn degraded_parts_without_an_incident() {
        let j = br#"{"status":{"indicator":"minor"},"incidents":[],
          "components":[{"name":"Group","status":"partial_outage","group":true},
                        {"name":"claude.ai","status":"partial_outage","group":false},
                        {"name":"Console","status":"operational","group":false}]}"#;
        let r = parse(j).unwrap();
        assert_eq!(r.incidents.len(), 1);
        assert_eq!(r.incidents[0].name, "claude.ai");
        assert!(r.incidents[0].state.is_empty());
    }

    #[test]
    fn asks_the_summary() {
        assert_eq!(summary_url("https://status.anthropic.com/api/v2/status.json"), "https://status.anthropic.com/api/v2/summary.json");
    }
}
