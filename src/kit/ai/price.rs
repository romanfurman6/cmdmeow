//! API list prices per model, to put a dollar figure on local token counts.
//! For subscriptions the figure is what the same use would cost on the API
//! (so the app can say how much the plan saved); for API keys it's an
//! estimate of the bill. Unknown models stay unpriced and are left out of
//! the dollar totals, never counted as free.

/// US dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    /// Writing to the 5-minute prompt cache.
    pub cache_write: f64,
    /// Writing to the 1-hour prompt cache.
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

const fn claude(input: f64, output: f64, cache_read: f64) -> Price {
    // cache writes cost 1.25x input (5 minutes) and 2x input (1 hour)
    Price { input, output, cache_write: input * 1.25, cache_write_1h: input * 2.0, cache_read }
}

const fn openai(input: f64, output: f64, cache_read: f64) -> Price {
    // OpenAI has no cache write charge
    Price { input, output, cache_write: input, cache_write_1h: input, cache_read }
}

/// Model id prefixes, most specific first.
const TABLE: &[(&str, Price)] = &[
    // Claude
    ("claude-fable-5-2", claude(8.0, 40.0, 0.20)),
    ("claude-mythos-5-2", claude(8.0, 40.0, 0.20)),
    ("claude-fable-5-1", claude(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", claude(10.0, 50.0, 0.25)),
    ("claude-fable-5", claude(10.0, 50.0, 1.0)),
    ("claude-mythos-5", claude(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", claude(4.0, 20.0, 0.20)),
    ("claude-opus-5", claude(5.0, 25.0, 0.50)),
    ("claude-opus-4-1", claude(15.0, 75.0, 1.50)),
    ("claude-opus-4-2025", claude(15.0, 75.0, 1.50)),
    ("claude-opus-4", claude(5.0, 25.0, 0.50)),
    ("claude-3-opus", claude(15.0, 75.0, 1.50)),
    ("claude-sonnet-5-5", claude(2.0, 10.0, 0.10)),
    ("claude-sonnet-5", claude(2.0, 10.0, 0.20)),
    ("claude-sonnet-4", claude(3.0, 15.0, 0.30)),
    ("claude-3-7-sonnet", claude(3.0, 15.0, 0.30)),
    ("claude-3-5-sonnet", claude(3.0, 15.0, 0.30)),
    ("claude-haiku-5-5", claude(0.10, 0.50, 0.01)),
    ("claude-haiku-4", claude(1.0, 5.0, 0.10)),
    ("claude-3-5-haiku", claude(0.80, 4.0, 0.08)),
    // OpenAI (Codex)
    ("gpt-5-mini", openai(0.25, 2.0, 0.025)),
    ("gpt-5-nano", openai(0.05, 0.40, 0.005)),
    ("gpt-5.1-codex-mini", openai(0.25, 2.0, 0.025)),
    ("gpt-5.2", openai(1.75, 14.0, 0.175)),
    ("gpt-5.3", openai(1.75, 14.0, 0.175)),
    ("gpt-5", openai(1.25, 10.0, 0.125)),
    ("codex-mini", openai(1.50, 6.0, 0.375)),
    ("o4-mini", openai(1.10, 4.40, 0.275)),
    ("o3", openai(2.0, 8.0, 0.50)),
    ("gpt-4.1", openai(2.0, 8.0, 0.50)),
    // Gemini (prompts up to 200K)
    ("gemini-2.5-pro", openai(1.25, 10.0, 0.31)),
    ("gemini-2.5-flash-lite", openai(0.10, 0.40, 0.025)),
    ("gemini-2.5-flash", openai(0.30, 2.50, 0.075)),
    ("gemini-3-pro", openai(2.0, 12.0, 0.20)),
    ("gemini-3-flash", openai(0.50, 3.0, 0.05)),
];

/// The price of a model by its id, e.g. "claude-opus-4-6" or
/// "anthropic/claude-sonnet-4-5". `None` when unknown.
pub fn of(model: &str) -> Option<Price> {
    let m = model.trim().to_ascii_lowercase();
    // provider prefixes ("anthropic/", "openai/") and Bedrock/Vertex forms
    let m = m.rsplit('/').next().unwrap_or(&m);
    let m = m.strip_prefix("anthropic.").or_else(|| m.strip_prefix("us.anthropic.")).unwrap_or(m);
    TABLE.iter().find(|(p, _)| m.starts_with(p)).map(|(_, price)| *price)
}

/// What one reply's tokens cost at `price`.
pub fn cost(price: &Price, input: u64, output: u64, cache_write: u64, cache_write_1h: u64, cache_read: u64) -> f64 {
    (input as f64 * price.input + output as f64 * price.output + cache_write as f64 * price.cache_write + cache_write_1h as f64 * price.cache_write_1h + cache_read as f64 * price.cache_read) / 1e6
}

/// A short, readable model name: "claude-opus-4-6-20260101" → "Opus 4.6",
/// "gpt-5.1-codex" → "GPT-5.1 Codex".
pub fn display_name(model: &str) -> String {
    let m = model.rsplit('/').next().unwrap_or(model);
    if let Some(rest) = m.strip_prefix("claude-") {
        // family and version in either order: "opus-4-6" or "3-5-sonnet"
        let parts: Vec<&str> = rest.split('-').filter(|p| p.len() < 8).collect();
        let family = parts.iter().find(|p| p.chars().all(|c| c.is_ascii_alphabetic()));
        let version: Vec<&str> = parts.iter().copied().filter(|p| p.chars().all(|c| c.is_ascii_digit())).collect();
        if let Some(f) = family {
            let mut f = f.to_string();
            f[..1].make_ascii_uppercase();
            return if version.is_empty() { f } else { format!("{f} {}", version.join(".")) };
        }
        return m.to_string();
    }
    if let Some(rest) = m.strip_prefix("gpt-") {
        let mut words = rest.split('-');
        let v = words.next().unwrap_or("");
        let tail: Vec<String> = words.map(|w| {
            let mut w = w.to_string();
            if let Some(f) = w.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            w
        }).collect();
        return if tail.is_empty() { format!("GPT-{v}") } else { format!("GPT-{v} {}", tail.join(" ")) };
    }
    if let Some(rest) = m.strip_prefix("gemini-") {
        let words: Vec<String> = rest.split('-').map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }).collect();
        return format!("Gemini {}", words.join(" "));
    }
    m.to_string()
}

/// Monthly list price of a plan, in dollars, when the plan name says it.
pub fn plan_usd(provider: &str, plan: &str) -> Option<f64> {
    let p = plan.to_ascii_lowercase();
    match provider {
        "Claude" => {
            if p.contains("20x") {
                Some(200.0)
            } else if p.contains("max") {
                Some(100.0)
            } else if p.contains("pro") {
                Some(20.0)
            } else if p.contains("team") {
                Some(30.0)
            } else {
                None
            }
        }
        "Codex" => match p.as_str() {
            "plus" => Some(20.0),
            "pro" => Some(200.0),
            "team" | "business" => Some(30.0),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_most_specific_price() {
        assert_eq!(of("claude-opus-4-6").unwrap().input, 5.0);
        assert_eq!(of("claude-opus-4-1-20250805").unwrap().input, 15.0);
        assert_eq!(of("claude-opus-4-20250514").unwrap().input, 15.0);
        assert_eq!(of("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(of("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(of("claude-sonnet-4-5-20250929").unwrap().output, 15.0);
        assert_eq!(of("claude-sonnet-5-5").unwrap().cache_read, 0.10);
        assert_eq!(of("claude-haiku-4-5-20251001").unwrap().input, 1.0);
        assert_eq!(of("anthropic/claude-sonnet-4-6").unwrap().input, 3.0);
        assert_eq!(of("us.anthropic.claude-sonnet-4-6").unwrap().input, 3.0);
        assert_eq!(of("gpt-5.1-codex").unwrap().input, 1.25);
        assert_eq!(of("gpt-5-mini").unwrap().input, 0.25);
        assert!(of("<synthetic>").is_none());
        assert!(of("some-local-model").is_none());
    }

    #[test]
    fn cost_of_a_reply() {
        let p = of("claude-sonnet-4-6").unwrap();
        // 1M in, 1M out, 1M cache write, 1M 1h write, 1M cache read
        let c = cost(&p, 1_000_000, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert!((c - (3.0 + 15.0 + 3.75 + 6.0 + 0.3)).abs() < 1e-9);
    }

    #[test]
    fn readable_names() {
        assert_eq!(display_name("claude-opus-4-6"), "Opus 4.6");
        assert_eq!(display_name("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(display_name("claude-3-5-haiku-20241022"), "Haiku 3.5");
        assert_eq!(display_name("gpt-5.1-codex"), "GPT-5.1 Codex");
        assert_eq!(display_name("gpt-5"), "GPT-5");
        assert_eq!(display_name("gemini-2.5-pro"), "Gemini 2.5 Pro");
    }

    #[test]
    fn plan_prices() {
        assert_eq!(plan_usd("Claude", "max"), Some(100.0));
        assert_eq!(plan_usd("Claude", "default_claude_max_20x"), Some(200.0));
        assert_eq!(plan_usd("Claude", "pro"), Some(20.0));
        assert_eq!(plan_usd("Codex", "pro"), Some(200.0));
        assert_eq!(plan_usd("Codex", "free"), None);
    }
}
