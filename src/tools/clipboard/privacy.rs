//! Text that should stay out of the history unless the user wants it:
//! API tokens, private keys and card numbers, and what looks like a password
//! copied while a password field is focused.
//!
//! Only a clip that is the secret on its own counts (a token pasted alone,
//! not a sentence that mentions one), so ordinary text and code never trip it.

/// Token prefixes and the fewest characters that follow them. Each family's
/// body is letters, digits, `_` and `-` (a JWT also has dots).
const PREFIXES: [(&str, usize); 22] = [
    ("sk-ant-", 20), // Anthropic
    ("sk-proj-", 20),
    ("sk-", 20), // OpenAI and the like
    ("ghp_", 30), // GitHub
    ("gho_", 30),
    ("ghu_", 30),
    ("ghs_", 30),
    ("ghr_", 30),
    ("github_pat_", 30),
    ("glpat-", 20), // GitLab
    ("xoxb-", 20),  // Slack
    ("xoxp-", 20),
    ("xoxa-", 20),
    ("xapp-", 20),
    ("sk_live_", 20), // Stripe secret keys
    ("rk_live_", 20),
    ("AIza", 35), // Google
    ("npm_", 30),
    ("pypi-", 40),
    ("hf_", 30), // Hugging Face
    ("dop_v1_", 40), // DigitalOcean
    ("SG.", 40),     // SendGrid
];

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// AWS access key ids: AKIA/ASIA and 16 upper-case letters or digits.
fn aws_key(s: &str) -> bool {
    (s.starts_with("AKIA") || s.starts_with("ASIA")) && s.len() == 20 && s[4..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// A JSON Web Token: three base64url parts, the first one a JSON header.
fn jwt(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && s.starts_with("eyJ") && s.len() >= 40 && parts.iter().all(|p| !p.is_empty() && p.chars().all(token_char))
}

fn prefixed_token(s: &str) -> bool {
    PREFIXES.iter().any(|(pre, min)| {
        s.strip_prefix(pre).is_some_and(|rest| {
            let body_ok = if *pre == "SG." { rest.chars().all(|c| token_char(c) || c == '.') } else { rest.chars().all(token_char) };
            rest.len() >= *min && body_ok
        })
    })
}

/// A PEM private key block anywhere in the clip.
fn private_key(s: &str) -> bool {
    s.contains("-----BEGIN") && s.contains("PRIVATE KEY-----")
}

/// 13–19 digits (spaces or dashes between groups allowed) that pass the Luhn check.
pub fn card_number(s: &str) -> bool {
    if !s.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-') {
        return false;
    }
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) || digits.iter().all(|d| *d == digits[0]) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Is this whole clip an API token, a private key or a card number?
pub fn is_secret(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if private_key(t) {
        return true;
    }
    if t.chars().any(char::is_whitespace) {
        return card_number(t);
    }
    prefixed_token(t) || aws_key(t) || jwt(t) || card_number(t)
}

/// Terminals can hold secure input on for as long as they're in front
/// (Secure Keyboard Entry), so secure input says nothing about a copy there.
const TERMINALS: [&str; 7] = [
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "com.mitchellh.ghostty",
    "org.alacritty",
    "com.github.wez.wezterm",
];

pub fn terminal(bundle: &str) -> bool {
    TERMINALS.contains(&bundle)
}

/// One run of printable characters, as long as a password can be.
pub fn password_shaped(text: &str) -> bool {
    let t = text.trim();
    let n = t.chars().count();
    (4..=128).contains(&n) && !t.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!(is_secret("sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz012345"));
        assert!(is_secret("  ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\n"));
        assert!(is_secret("github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz"));
        assert!(is_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(is_secret("AIzaSyA-1234567890abcdefghijklmnopqrstu"));
        assert!(is_secret("xoxb-123456789012-1234567890123-AbCdEfGhIjKl"));
        assert!(is_secret("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
        assert!(is_secret("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n-----END OPENSSH PRIVATE KEY-----"));
    }

    #[test]
    fn ordinary_text_is_not_a_secret() {
        for t in [
            "sk-",
            "sk-short",
            "ghp_ is the GitHub token prefix",
            "export OPENAI_KEY=$(cat key)",
            "AKIA",
            "AKIAiosfodnn7example",
            "eyJ.not.it",
            "https://github.com/romanfurman6/cmdmeow/pull/9",
            "fn main() {}",
            "-----BEGIN CERTIFICATE-----",
            "1234 5678 9012 3456", // fails Luhn
            "0000 0000 0000 0000",
            "+380 67 123 4567",
            "2026-11-12",
        ] {
            assert!(!is_secret(t), "{t:?}");
        }
    }

    #[test]
    fn cards() {
        assert!(is_secret("4111 1111 1111 1111"));
        assert!(is_secret("4111-1111-1111-1111"));
        assert!(is_secret("5555555555554444"));
        assert!(is_secret("378282246310005")); // Amex, 15 digits
        assert!(!card_number("41111111111111112"));
        assert!(!card_number("4111 1111 1111 111a"));
    }

    #[test]
    fn passwords() {
        assert!(password_shaped("hunter2!"));
        assert!(password_shaped("  Tr0ub4dor&3 \n"));
        assert!(!password_shaped("abc"));
        assert!(!password_shaped("two words"));
        assert!(!password_shaped(&"x".repeat(129)));
        assert!(terminal("com.apple.Terminal"));
        assert!(!terminal("com.apple.Safari"));
    }
}
