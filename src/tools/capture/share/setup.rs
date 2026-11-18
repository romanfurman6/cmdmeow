//! "Ask Claude to set up" on the Sharing page: opens Claude (its desktop
//! app when installed, else the web) or ChatGPT with a prompt that has the
//! agent set up a Cloudflare R2 bucket in its browser and save the keys in
//! a file Cmd+Meow imports (see [`super::import`]).

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

/// Where the button sends them: Claude's desktop app (a Cowork task with
/// Downloads, so Claude can use its browser and save the storage file
/// there) when it's installed, else the web chat. Codex: ChatGPT on the web.
pub fn chat_url(a: Agent) -> String {
    let q = encode(&prompt(), true);
    match a {
        Agent::Claude if claude_desktop() => {
            let downloads = std::env::var("HOME").map(|h| format!("{h}/Downloads")).unwrap_or_default();
            format!("claude://cowork/new?q={q}&folder={}", encode(&downloads, true))
        }
        Agent::Claude => format!("https://claude.ai/new?q={q}"),
        Agent::Codex => format!("https://chatgpt.com/?q={q}"),
    }
}

/// An app answers `claude://` links (Claude's desktop app is installed).
#[cfg(target_os = "macos")]
fn claude_desktop() -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    let Some(url) = NSURL::URLWithString(&NSString::from_str("claude://cowork/new")) else { return false };
    NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url).is_some()
}

#[cfg(not(target_os = "macos"))]
fn claude_desktop() -> bool {
    false
}

/// What the chat starts with. Claude does the work in its browser and
/// hands the keys over in a file, never in the chat.
pub fn prompt() -> String {
    let origins = ORIGINS.iter().take(2).map(|o| format!("\"{o}\"")).collect::<Vec<_>>().join(", ");
    let file = super::import::FILE_NAME;
    format!(
        "Set up Cloudflare R2 for the Mac app Cmd+Meow so it can upload my screenshots and screen recordings and give me share links. Do the work yourself in your browser.

1. Open dash.cloudflare.com. If I'm not signed in, ask me to sign in and wait. If R2 asks me to accept its plan or add a payment method, ask me to do it and wait. Never type payment details yourself.
2. Create an R2 bucket named cmdmeow-shares (add -2, -3… if the name is taken). Location Automatic, storage class Standard.
3. In the bucket's settings, turn on public access through the r2.dev subdomain.
4. In the bucket's CORS policy, add: \
[{{\"AllowedOrigins\": [{origins}], \"AllowedMethods\": [\"GET\", \"HEAD\"], \"AllowedHeaders\": [\"*\"], \"ExposeHeaders\": [\"Content-Length\", \"Content-Range\", \"Content-Type\", \"Accept-Ranges\"], \"MaxAgeSeconds\": 86400}}]
5. Create an R2 API token with Object Read & Write on this bucket only.
6. Save a file named {file} in my Downloads folder with only this JSON, values filled in: \
{{\"cmdmeow\": \"sharing\", \"endpoint\": \"<ACCOUNT_ID>.r2.cloudflarestorage.com\", \"bucket\": \"…\", \"access_key\": \"…\", \"secret_key\": \"…\", \"public_url\": \"https://pub-….r2.dev\"}}
If you can't write to my Downloads folder, give me the file to download instead.

Never write the access key or the secret key in this chat, not even part of them. Copy them from the Cloudflare page into the file only. \
When you're done, reply with one line: the bucket name and \"Double-click {file} in Downloads to add it to Cmd+Meow.\"

If you can't use a browser, walk me through the same steps one at a time instead and tell me what goes in each field of Cmd+Meow's Sharing page \
(Endpoint, Bucket, Access key, Secret key, Public URL). Then never ask me to paste the secret key into this chat; it goes straight into Cmd+Meow."
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
        assert!(u.starts_with("https://chatgpt.com/?q=Set%20up%20Cloudflare%20R2"), "{u}");
        // no desktop app here (tests run on Linux in CI too)
        if !claude_desktop() {
            assert!(chat_url(Agent::Claude).starts_with("https://claude.ai/new?q="));
        }
        // one query value: nothing in it may end it early
        let q = &u["https://chatgpt.com/?q=".len()..];
        assert!(!q.contains(['&', '#', ' ', '\n', '"']), "{q}");
        assert!(prompt().contains("https://cmdmeow.com"));
        assert!(prompt().contains(super::super::import::FILE_NAME));
    }
}
