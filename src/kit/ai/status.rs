//! Outages: the providers' public status pages (Statuspage's
//! `/api/v2/status.json`), so a slow or failing assistant shows why.

use serde_json::Value;

/// The incident to show, from a status page's JSON: `None` when all is
/// well. "Degraded" for minor trouble, "Outage" for major or critical.
pub fn parse(json: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    let st = &v["status"];
    match st["indicator"].as_str()? {
        "none" | "maintenance" => None,
        "minor" => Some("Degraded".into()),
        _ => Some("Outage".into()),
    }
}

/// Ask `url` (a status page's `status.json`). `None` when all is well or
/// the page can't be reached (no badge rather than a wrong one).
pub fn fetch(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(8))).build());
    let body = agent.get(url).call().ok()?.body_mut().read_to_vec().ok()?;
    parse(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicators() {
        assert_eq!(parse(br#"{"status":{"indicator":"none","description":"All Systems Operational"}}"#), None);
        assert_eq!(parse(br#"{"status":{"indicator":"minor","description":"Partial outage"}}"#).as_deref(), Some("Degraded"));
        assert_eq!(parse(br#"{"status":{"indicator":"major"}}"#).as_deref(), Some("Outage"));
        assert_eq!(parse(br#"{"status":{"indicator":"critical"}}"#).as_deref(), Some("Outage"));
        assert_eq!(parse(b"<html>"), None);
    }
}
