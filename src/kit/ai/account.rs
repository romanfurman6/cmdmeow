//! Plan limits straight from each provider's account, signed in with the
//! login its own command-line tool already keeps on this Mac: Claude Code's
//! token (Keychain), Codex's `~/.codex/auth.json`, Gemini CLI's
//! `~/.gemini/oauth_creds.json`. These are the endpoints the tools ask for
//! their own usage screens. The tokens are only read, never stored or
//! renewed in place of the tool that owns them (renewing a Claude or Codex
//! token would sign that tool out). Any failure returns an error, and the
//! provider falls back to its CLI or its local files.

use super::Limit;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const UA: &str = concat!("CmdMeow/", env!("CARGO_PKG_VERSION"));

/// Why an account call didn't give limits.
#[derive(Debug, PartialEq, Eq)]
pub enum ApiError {
    /// The token was refused (expired or revoked): read it again later.
    Auth,
    /// Anything else (offline, rate limited, a changed reply).
    Other(String),
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build())
}

/// The JSON body of a reply, or why there isn't one.
fn reply(r: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Value, ApiError> {
    let mut r = r.map_err(|e| ApiError::Other(e.to_string()))?;
    let code = r.status().as_u16();
    let body = r.body_mut().read_to_vec().unwrap_or_default();
    match code {
        200..=299 => serde_json::from_slice(&body).map_err(|e| ApiError::Other(format!("unreadable reply: {e}"))),
        401 | 403 => Err(ApiError::Auth),
        _ => Err(ApiError::Other(format!("HTTP {code}"))),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A limit window as the UI expects it: `pct` used of a `secs`-long window
/// that resets at `resets_at`. A window that rolled over already is unused.
pub fn limit(label: &str, pct: f32, secs: i64, resets_at: Option<i64>, now: i64) -> Option<Limit> {
    if !pct.is_finite() || !(0.0..=1000.0).contains(&pct) {
        return None;
    }
    let (pct, resets_at) = match resets_at {
        Some(r) if r <= now => (0.0, None),
        r => (pct, r),
    };
    let resets = resets_at.map(super::codex::reset_text).unwrap_or_default();
    Some(Limit { label: label.to_string(), pct, resets, resets_at, window_secs: secs })
}

/// Unix time from a number (seconds or milliseconds) or an RFC 3339 text.
fn when(v: &Value) -> Option<i64> {
    let n = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64));
    if let Some(n) = n {
        return Some(if n > 100_000_000_000 { n / 1000 } else { n });
    }
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp())
}

// ---------------------------------------------------------------- Claude

/// Claude Code's sign-in, as it keeps it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeCreds {
    pub token: String,
    /// When the token stops working (unix time), if known.
    pub expires_at: Option<i64>,
    /// "max", "pro", …
    pub plan: String,
    /// e.g. "default_claude_max_20x".
    pub tier: String,
}

impl ClaudeCreds {
    /// Still good for at least another minute.
    pub fn fresh(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|e| e > now + 60)
    }
}

/// The `claudeAiOauth` part of Claude Code's credentials. None without a
/// token, or when the token can't read usage (an inference-only token).
pub fn parse_claude_creds(bytes: &[u8]) -> Option<ClaudeCreds> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = &v["claudeAiOauth"];
    let token = o["accessToken"].as_str().filter(|t| !t.is_empty())?.to_string();
    if let Some(scopes) = o["scopes"].as_array() {
        if !scopes.iter().any(|s| s.as_str() == Some("user:profile")) {
            return None;
        }
    }
    let s = |k: &str| o[k].as_str().unwrap_or("").to_string();
    Some(ClaudeCreds { token, expires_at: when(&o["expiresAt"]), plan: s("subscriptionType"), tier: s("rateLimitTier") })
}

/// What reading Claude Code's Keychain item gave.
#[derive(Debug, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum KeychainRead {
    Found(Vec<u8>),
    /// No such item: Claude Code keeps no sign-in here.
    Missing,
    /// The person pressed Deny.
    Denied,
    Failed,
}

/// Read the item Claude Code keeps its sign-in in. macOS asks the person
/// the first time (Allow / Always Allow / Deny).
#[cfg(target_os = "macos")]
pub fn read_claude_keychain() -> KeychainRead {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
    let found = ItemSearchOptions::new().class(ItemClass::generic_password()).service("Claude Code-credentials").load_data(true).limit(Limit::Max(1)).search();
    match found {
        Ok(v) => v.into_iter().find_map(|r| if let SearchResult::Data(d) = r { Some(d) } else { None }).map(KeychainRead::Found).unwrap_or(KeychainRead::Missing),
        // errSecItemNotFound
        Err(e) if e.code() == -25300 => KeychainRead::Missing,
        // errSecUserCanceled, errSecAuthFailed
        Err(e) if e.code() == -128 || e.code() == -25293 => KeychainRead::Denied,
        Err(_) => KeychainRead::Failed,
    }
}

#[cfg(not(target_os = "macos"))]
pub fn read_claude_keychain() -> KeychainRead {
    KeychainRead::Missing
}

/// Whether Claude Code keeps a sign-in in the Keychain. Only the item's
/// attributes are looked at, never its secret, so macOS doesn't ask.
#[cfg(target_os = "macos")]
pub fn claude_keychain_exists() -> bool {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    ItemSearchOptions::new().class(ItemClass::generic_password()).service("Claude Code-credentials").load_attributes(true).limit(Limit::Max(1)).search().is_ok_and(|v| !v.is_empty())
}

#[cfg(not(target_os = "macos"))]
pub fn claude_keychain_exists() -> bool {
    false
}

/// Claude Code's credentials file (used where there's no Keychain item).
pub fn claude_creds_file() -> Option<ClaudeCreds> {
    let home = dirs::home_dir()?;
    [".claude/.credentials.json", ".config/claude/.credentials.json"].iter().find_map(|f| parse_claude_creds(&std::fs::read(home.join(f)).ok()?))
}

/// The limit windows in Claude's usage reply (also the shape of Claude
/// Code's status-line `rate_limits`), named like `/usage` names them.
pub fn claude_limits(v: &Value, now: i64) -> Vec<Limit> {
    const WEEK: i64 = 7 * 86_400;
    [
        ("five_hour", "Current session", 5 * 3600),
        ("seven_day", "Current week (all models)", WEEK),
        ("seven_day_opus", "Current week (Opus)", WEEK),
        ("seven_day_sonnet", "Current week (Sonnet)", WEEK),
    ]
    .iter()
    .filter_map(|(key, label, secs)| {
        let w = &v[*key];
        let pct = w["utilization"].as_f64().or_else(|| w["used_percentage"].as_f64())? as f32;
        limit(label, pct, *secs, when(&w["resets_at"]), now)
    })
    .collect::<Vec<_>>()
}

/// Claude's limits for the page: with no 5-hour window running (nothing
/// sent in the last five hours) the reply has no `five_hour` at all, which
/// means the session is unused, so it shows as 0% rather than missing.
fn with_session(mut limits: Vec<Limit>) -> Vec<Limit> {
    if !limits.is_empty() && !limits.iter().any(|l| l.label == "Current session") {
        limits.insert(0, Limit { label: "Current session".into(), pct: 0.0, resets: String::new(), resets_at: None, window_secs: 5 * 3600 });
    }
    limits
}

/// Claude's limits for this sign-in.
pub fn claude_usage(token: &str) -> Result<Vec<Limit>, ApiError> {
    let r = agent()
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header("User-Agent", UA)
        .call();
    let v = reply(r)?;
    // which windows the reply carries (no personal data), for the dev log
    let keys: Vec<&str> = v.as_object().map(|o| o.iter().filter(|(_, x)| !x.is_null()).map(|(k, _)| k.as_str()).collect()).unwrap_or_default();
    eprintln!("[ai] claude usage windows: {keys:?}");
    let limits = with_session(claude_limits(&v, now()));
    if limits.is_empty() {
        return Err(ApiError::Other("no limits in the reply".into()));
    }
    Ok(limits)
}

// ---------------------------------------------------------------- Codex

/// The plan and limit windows in Codex's usage reply.
pub fn codex_limits(v: &Value, now: i64) -> (String, Vec<Limit>) {
    let rl = &v["rate_limit"];
    let limits = [&rl["primary_window"], &rl["secondary_window"]]
        .into_iter()
        .filter_map(|w| {
            let pct = w["used_percent"].as_f64()? as f32;
            let secs = w["limit_window_seconds"].as_i64().unwrap_or(0);
            let resets_at = when(&w["reset_at"]).or_else(|| w["reset_after_seconds"].as_i64().map(|s| now + s));
            limit(&super::codex::window_label(secs / 60), pct, secs, resets_at, now)
        })
        .collect();
    (v["plan_type"].as_str().unwrap_or("").to_string(), limits)
}

/// Codex's plan and limits for the ChatGPT sign-in in `auth.json`. An API
/// key sign-in has no plan limits.
pub fn codex_usage(auth: &[u8]) -> Result<(String, Vec<Limit>), ApiError> {
    let v: Value = serde_json::from_slice(auth).map_err(|_| ApiError::Auth)?;
    let t = &v["tokens"];
    let token = t["access_token"].as_str().filter(|s| !s.is_empty()).ok_or(ApiError::Auth)?;
    let mut req = agent().get("https://chatgpt.com/backend-api/wham/usage").header("Authorization", format!("Bearer {token}")).header("Accept", "application/json").header("User-Agent", UA);
    if let Some(id) = t["account_id"].as_str().filter(|s| !s.is_empty()) {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let (plan, limits) = codex_limits(&reply(req.call())?, now());
    if limits.is_empty() {
        return Err(ApiError::Other("no limits in the reply".into()));
    }
    Ok((plan, limits))
}

// ---------------------------------------------------------------- Gemini

const CODE_ASSIST: &str = "https://cloudcode-pa.googleapis.com/v1internal";

/// Gemini CLI's Google sign-in, as it keeps it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoogleCreds {
    pub access: String,
    pub refresh: String,
    pub expires_at: Option<i64>,
}

pub fn parse_google_creds(bytes: &[u8]) -> Option<GoogleCreds> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    let c = GoogleCreds { access: s("access_token"), refresh: s("refresh_token"), expires_at: when(&v["expiry_date"]) };
    (!c.access.is_empty() || !c.refresh.is_empty()).then_some(c)
}

/// The OAuth client id and secret inside a Gemini CLI source file. They
/// ship in the CLI itself (an installed app's client, not a secret of the
/// person's), so they are read from there rather than kept here.
pub fn find_client(text: &str) -> Option<(String, String)> {
    let host = ".apps.googleusercontent.com";
    let at = text.find(host)?;
    let start = text[..at].char_indices().rev().find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-')).map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    let id = &text[start..at + host.len()];
    let s = text.find("GOCSPX-")?;
    let end = text[s..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).map(|i| s + i).unwrap_or(text.len());
    (id.len() > host.len() && end - s > 10).then(|| (id.to_string(), text[s..end].to_string()))
}

/// The installed Gemini CLI package, from the `gemini` command or the
/// usual install places.
fn gemini_packages() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut bins: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    bins.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    bins.extend([".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin"].map(|d| home.join(d)));
    let mut out = Vec::new();
    for b in bins.into_iter().map(|d| d.join("gemini")) {
        let Ok(real) = std::fs::canonicalize(&b) else { continue };
        // …/node_modules/@google/gemini-cli/dist/index.js, or a Homebrew
        // libexec/bin/gemini next to libexec/lib/node_modules
        for a in real.ancestors() {
            if a.ends_with("@google/gemini-cli") {
                out.push(a.to_path_buf());
                break;
            }
            let brew = a.join("lib/node_modules/@google/gemini-cli");
            if brew.is_dir() {
                out.push(brew);
                break;
            }
        }
    }
    for root in ["/opt/homebrew", "/usr/local"] {
        out.push(PathBuf::from(root).join("opt/gemini-cli/libexec/lib/node_modules/@google/gemini-cli"));
        out.push(PathBuf::from(root).join("lib/node_modules/@google/gemini-cli"));
    }
    out.dedup();
    out.retain(|p| p.is_dir());
    out
}

/// Look through a package's JavaScript for the client (bounded: the code
/// assist file first, then the bundles).
fn client_in(pkg: &Path) -> Option<(String, String)> {
    let known = ["node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js", "../gemini-cli-core/dist/src/code_assist/oauth2.js"];
    for k in known {
        if let Some(c) = std::fs::read_to_string(pkg.join(k)).ok().and_then(|t| find_client(&t)) {
            return Some(c);
        }
    }
    let mut stack = vec![(pkg.join("bundle"), 2), (pkg.join("dist"), 3)];
    let mut seen = 0;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth > 0 {
                    stack.push((p, depth - 1));
                }
                continue;
            }
            if p.extension().is_none_or(|x| x != "js") || e.metadata().is_ok_and(|m| m.len() > 40 << 20) {
                continue;
            }
            seen += 1;
            if seen > 400 {
                return None;
            }
            if let Some(c) = std::fs::read_to_string(&p).ok().and_then(|t| find_client(&t)) {
                return Some(c);
            }
        }
    }
    None
}

fn gemini_client() -> Option<(String, String)> {
    static FOUND: Mutex<Option<(String, String)>> = Mutex::new(None);
    let mut g = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = gemini_packages().iter().find_map(|p| client_in(p));
    }
    g.clone()
}

/// A working Google access token: Gemini CLI's own while it's fresh, else
/// one renewed with its refresh token and kept in memory (Google's refresh
/// tokens stay valid, so Gemini CLI keeps working).
fn google_token(creds: &GoogleCreds) -> Result<String, ApiError> {
    static RENEWED: Mutex<Option<(String, i64, String)>> = Mutex::new(None);
    let now = now();
    if !creds.access.is_empty() && creds.expires_at.is_some_and(|e| e > now + 60) {
        return Ok(creds.access.clone());
    }
    let mut g = RENEWED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((tok, exp, refresh)) = g.as_ref() {
        if *refresh == creds.refresh && *exp > now + 60 {
            return Ok(tok.clone());
        }
    }
    if creds.refresh.is_empty() {
        return Err(ApiError::Auth);
    }
    // without Gemini CLI's client there's no way to sign in: same as refused
    let (id, secret) = gemini_client().ok_or(ApiError::Auth)?;
    let r = agent().post("https://oauth2.googleapis.com/token").header("User-Agent", UA).send_form([
        ("client_id", id.as_str()),
        ("client_secret", secret.as_str()),
        ("refresh_token", creds.refresh.as_str()),
        ("grant_type", "refresh_token"),
    ]);
    let v = reply(r)?;
    let tok = v["access_token"].as_str().filter(|s| !s.is_empty()).ok_or(ApiError::Auth)?.to_string();
    let exp = now + v["expires_in"].as_i64().unwrap_or(3600);
    *g = Some((tok.clone(), exp, creds.refresh.clone()));
    Ok(tok)
}

/// The plan name and quota project in a `loadCodeAssist` reply.
pub fn gemini_tier(v: &Value) -> (String, String) {
    let tier = v["paidTier"]["id"].as_str().or_else(|| v["currentTier"]["id"].as_str()).unwrap_or("");
    let plan = match tier {
        "free-tier" => "free",
        "standard-tier" => "standard",
        "legacy-tier" => "legacy",
        t if t.contains("enterprise") => "enterprise",
        t if !t.is_empty() => "paid",
        _ => "",
    };
    let project = v["cloudaicompanionProject"].as_str().or_else(|| v["cloudaicompanionProject"]["id"].as_str()).unwrap_or("");
    (plan.to_string(), project.to_string())
}

/// Daily request quotas by model family, from `retrieveUserQuota`: the
/// most used model of each family counts.
pub fn gemini_limits(v: &Value, now: i64) -> Vec<Limit> {
    let families = [("flash-lite", "Flash Lite (daily)"), ("flash", "Flash (daily)"), ("pro", "Pro (daily)")];
    let mut worst: Vec<(usize, f64, Option<i64>)> = Vec::new();
    for b in v["buckets"].as_array().into_iter().flatten() {
        let Some(left) = b["remainingFraction"].as_f64() else { continue };
        let model = b["modelId"].as_str().unwrap_or("").to_lowercase();
        let Some(f) = families.iter().position(|(k, _)| model.contains(k)) else { continue };
        let reset = when(&b["resetTime"]);
        match worst.iter_mut().find(|w| w.0 == f) {
            Some(w) if left < w.1 => *w = (f, left, reset),
            Some(_) => {}
            None => worst.push((f, left, reset)),
        }
    }
    // Pro first, then Flash, then Flash Lite
    worst.sort_by_key(|w| std::cmp::Reverse(w.0));
    worst.into_iter().filter_map(|(f, left, reset)| limit(families[f].1, ((1.0 - left.clamp(0.0, 1.0)) * 100.0) as f32, 86_400, reset, now)).collect()
}

/// Gemini's plan and daily quotas for Gemini CLI's Google sign-in. Fails
/// for API-key sign-ins and for plans Google no longer serves this way.
pub fn gemini_usage(home: &Path) -> Result<(String, Vec<Limit>), ApiError> {
    static PROJECT: OnceLock<Mutex<Option<(String, String)>>> = OnceLock::new();
    let creds = std::fs::read(home.join("oauth_creds.json")).ok().and_then(|b| parse_google_creds(&b)).ok_or(ApiError::Auth)?;
    let token = google_token(&creds)?;
    let post = |what: &str, body: Value| reply(agent().post(format!("{CODE_ASSIST}:{what}")).header("Authorization", format!("Bearer {token}")).header("User-Agent", UA).send_json(body));
    let cell = PROJECT.get_or_init(|| Mutex::new(None));
    let known = cell.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (plan, project) = match known {
        Some(k) => k,
        None => {
            let t = gemini_tier(&post("loadCodeAssist", json!({ "metadata": { "ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" } }))?);
            *cell.lock().unwrap_or_else(|e| e.into_inner()) = Some(t.clone());
            t
        }
    };
    let body = if project.is_empty() { json!({}) } else { json!({ "project": project }) };
    let limits = gemini_limits(&post("retrieveUserQuota", body)?, now());
    if limits.is_empty() {
        return Err(ApiError::Other("no quotas in the reply".into()));
    }
    Ok((plan, limits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_creds_need_a_usage_scope() {
        let ok = br#"{"claudeAiOauth":{"accessToken":"t","expiresAt":1900000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#;
        let c = parse_claude_creds(ok).unwrap();
        assert_eq!((c.token.as_str(), c.expires_at, c.plan.as_str(), c.tier.as_str()), ("t", Some(1_900_000_000), "max", "default_claude_max_20x"));
        assert!(c.fresh(1_800_000_000));
        assert!(!c.fresh(1_900_000_000));
        assert!(parse_claude_creds(br#"{"claudeAiOauth":{"accessToken":"t","scopes":["user:inference"]}}"#).is_none());
        assert!(parse_claude_creds(br#"{"mcpOAuth":{}}"#).is_none());
        // no scope list: try it
        assert!(parse_claude_creds(br#"{"claudeAiOauth":{"accessToken":"t"}}"#).is_some_and(|c| c.fresh(0)));
    }

    #[test]
    fn claude_usage_reply() {
        let now = 1_000_000;
        let v: Value = serde_json::from_str(r#"{"five_hour":{"utilization":19.0,"resets_at":"1970-01-12T14:00:00+00:00"},"seven_day":{"utilization":42,"resets_at":"1970-01-01T00:00:01Z"},"seven_day_opus":null,"seven_day_sonnet":{"utilization":3.5,"resets_at":null}}"#).unwrap();
        let l = claude_limits(&v, now);
        assert_eq!(l.len(), 3);
        assert_eq!((l[0].label.as_str(), l[0].pct, l[0].resets_at, l[0].window_secs), ("Current session", 19.0, Some(1_000_800), 5 * 3600));
        // reset since: unused
        assert_eq!((l[1].pct, l[1].resets_at), (0.0, None));
        assert_eq!((l[2].label.as_str(), l[2].pct, l[2].resets_at), ("Current week (Sonnet)", 3.5, None));
        // the status-line shape and unix seconds work too
        let v: Value = serde_json::from_str(r#"{"five_hour":{"used_percentage":50,"resets_at":1000600}}"#).unwrap();
        assert_eq!(claude_limits(&v, now)[0].resets_at, Some(1_000_600));
        assert!(claude_limits(&json!({"error":"x"}), now).is_empty());
        // no 5-hour window running: the session shows as unused
        let l = with_session(claude_limits(&json!({"five_hour":null,"seven_day":{"utilization":30,"resets_at":null}}), now));
        assert_eq!(l.iter().map(|l| (l.label.as_str(), l.pct)).collect::<Vec<_>>(), vec![("Current session", 0.0), ("Current week (all models)", 30.0)]);
        assert!(with_session(Vec::new()).is_empty());
    }

    #[test]
    fn codex_usage_reply() {
        let now = 1_000_000;
        let v = json!({"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000,"reset_at":1_003_600},"secondary_window":{"used_percent":40,"limit_window_seconds":604800,"reset_after_seconds":86400}}});
        let (plan, l) = codex_limits(&v, now);
        assert_eq!(plan, "plus");
        assert_eq!((l[0].label.as_str(), l[0].pct, l[0].resets_at, l[0].window_secs), ("Current session", 12.0, Some(1_003_600), 18_000));
        assert_eq!((l[1].label.as_str(), l[1].pct, l[1].resets_at), ("Current week (all models)", 40.0, Some(now + 86_400)));
        assert!(codex_limits(&json!({"rate_limit":null}), now).1.is_empty());
    }

    #[test]
    fn gemini_client_from_cli_source() {
        let js = "const OAUTH_CLIENT_ID =\n  '123456-abcdef.apps.googleusercontent.com';\nconst OAUTH_CLIENT_SECRET = 'GOCSPX-abc_DEF-123456';";
        assert_eq!(find_client(js), Some(("123456-abcdef.apps.googleusercontent.com".into(), "GOCSPX-abc_DEF-123456".into())));
        assert_eq!(find_client("no client here"), None);
    }

    #[test]
    fn gemini_quota_reply() {
        let now = 1_000_000;
        let v = json!({"buckets":[
            {"modelId":"gemini-2.5-flash","remainingFraction":0.9,"resetTime":"1970-01-13T00:00:00Z","tokenType":"REQUESTS"},
            {"modelId":"gemini-2.5-pro","remainingFraction":0.75,"resetTime":"1970-01-13T00:00:00Z"},
            {"modelId":"gemini-2.5-pro-preview","remainingFraction":0.5,"resetTime":"1970-01-13T00:00:00Z"},
            {"modelId":"gemini-2.5-flash-lite","remainingFraction":1.0},
            {"modelId":"other","remainingFraction":0.1}
        ]});
        let l = gemini_limits(&v, now);
        let got: Vec<(&str, f32)> = l.iter().map(|l| (l.label.as_str(), l.pct)).collect();
        assert_eq!(got, vec![("Pro (daily)", 50.0), ("Flash (daily)", 10.0), ("Flash Lite (daily)", 0.0)]);
        assert_eq!(l[0].resets_at, Some(1_036_800));
        let (plan, project) = gemini_tier(&json!({"currentTier":{"id":"standard-tier"},"cloudaicompanionProject":{"id":"p-1"}}));
        assert_eq!((plan.as_str(), project.as_str()), ("standard", "p-1"));
        let c = parse_google_creds(br#"{"access_token":"a","refresh_token":"r","expiry_date":1900000000000}"#).unwrap();
        assert_eq!(c.expires_at, Some(1_900_000_000));
    }
}
