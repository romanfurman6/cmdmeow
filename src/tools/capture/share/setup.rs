//! "Ask Claude to set up" on the Sharing page: opens the main agent's web
//! chat (Claude or ChatGPT) with a prompt that walks the person through a
//! Cloudflare R2 bucket and keys for the fields on that page.

use super::s3::encode;
use super::ORIGINS;
use crate::kit::agent::Agent;

/// The chat the button opens: Codex's people use ChatGPT on the web.
pub fn chat_name(a: Agent) -> &'static str {
    match a {
        Agent::Claude => "Claude",
        Agent::Codex => "ChatGPT",
    }
}

/// The button's label.
pub fn label(a: Agent) -> String {
    format!("Ask {} to set up", chat_name(a))
}

/// The web chat, with the prompt already typed in.
pub fn chat_url(a: Agent) -> String {
    let q = encode(&prompt(), true);
    match a {
        Agent::Claude => format!("https://claude.ai/new?q={q}"),
        Agent::Codex => format!("https://chatgpt.com/?q={q}"),
    }
}

/// What the chat starts with. The secret key never goes to the chat.
pub fn prompt() -> String {
    let origins = ORIGINS.iter().take(2).map(|o| format!("\"{o}\"")).collect::<Vec<_>>().join(", ");
    format!(
        "Help me set up Cloudflare R2 so the Mac app Cmd+Meow can upload my screenshots and screen recordings and give me share links. \
Walk me through it one step at a time in the Cloudflare dashboard and wait for me after each step.

Cmd+Meow's Sharing page has these fields:
- Endpoint: the bucket's S3 API host, like <ACCOUNT_ID>.r2.cloudflarestorage.com
- Bucket: the bucket name
- Access key and Secret key: S3 credentials from an R2 API token with Object Read & Write on this bucket only
- Public URL: the bucket's public address, its r2.dev subdomain (https://pub-….r2.dev) or a custom domain
- Link: Public (never expires, file names are random) or Expires in 7 days (the bucket stays private, no Public URL needed)

Steps:
1. Create the bucket.
2. If I want Public links, turn on public access (r2.dev subdomain or a custom domain).
3. Add this CORS policy in the bucket's settings, so cmdmeow.com can show and edit shared files: \
[{{\"AllowedOrigins\": [{origins}], \"AllowedMethods\": [\"GET\", \"HEAD\"], \"AllowedHeaders\": [\"*\"], \"ExposeHeaders\": [\"Content-Length\", \"Content-Range\", \"Content-Type\", \"Accept-Ranges\"], \"MaxAgeSeconds\": 86400}}]
4. Create the API token and tell me exactly what goes in each field.

Never ask me to paste the secret key into this chat; it goes straight into Cmd+Meow, which keeps it in the macOS Keychain. \
If I'd rather use another S3-compatible storage (Hetzner, AWS S3, Backblaze B2, MinIO), adapt the steps to it."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_the_main_agents_chat_with_the_prompt() {
        assert_eq!(label(Agent::Claude), "Ask Claude to set up");
        assert_eq!(label(Agent::Codex), "Ask ChatGPT to set up");
        let u = chat_url(Agent::Codex);
        assert!(u.starts_with("https://chatgpt.com/?q=Help%20me%20set%20up%20Cloudflare%20R2"), "{u}");
        assert!(chat_url(Agent::Claude).starts_with("https://claude.ai/new?q="));
        // one query value: nothing in it may end it early
        let q = &u["https://chatgpt.com/?q=".len()..];
        assert!(!q.contains(['&', '#', ' ', '\n', '"']), "{q}");
        assert!(prompt().contains("https://cmdmeow.com"));
    }
}
