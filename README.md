<p align="center">
  <img src="docs/images/icon.png" width="96" alt="Cmd+Meow icon">
</p>

<h1 align="center">Cmd+Meow</h1>

<p align="center">
  <b>Everything your Mac needs. Free.</b><br>
  All your Mac tools in one menu bar app. Open source, no account, runs on your Mac.
</p>

<p align="center">
  <a href="https://github.com/romanfurman6/cmdmeow/releases/latest/download/CmdMeow.dmg"><img src="https://img.shields.io/badge/Download_for_macOS-FF5FA2?style=for-the-badge&logo=apple&logoColor=0E1405&labelColor=FF5FA2&color=FF5FA2" height="40" alt="Download for macOS"></a>
</p>

<p align="center">
  <a href="https://github.com/romanfurman6/cmdmeow/releases/latest"><img src="https://img.shields.io/github/v/release/romanfurman6/cmdmeow?label=version&color=FF5FA2&labelColor=1A1248" alt="Latest version"></a>
  <img src="https://img.shields.io/badge/macOS-13%2B-FF5FA2?labelColor=1A1248" alt="macOS 13+">
  <img src="https://img.shields.io/badge/Apple_silicon-native-FF5FA2?labelColor=1A1248" alt="Apple silicon">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-FF5FA2?labelColor=1A1248" alt="MIT license"></a>
  <a href="https://github.com/romanfurman6/cmdmeow/actions/workflows/macos.yml"><img src="https://img.shields.io/github/actions/workflow/status/romanfurman6/cmdmeow/macos.yml?branch=main&label=build&labelColor=1A1248" alt="Build"></a>
</p>

<p align="center">
  <a href="https://cmdmeow.com">Website</a> ·
  <a href="https://cmdmeow.com/promo.mp4">40-second video</a> ·
  <a href="https://github.com/romanfurman6/cmdmeow/releases">Releases</a>
</p>

<img src="docs/images/hero.jpg" alt="Cmd+Meow window with Disk Map, next to the menu bar panel">

## Install

Download [CmdMeow.dmg](https://github.com/romanfurman6/cmdmeow/releases/latest/download/CmdMeow.dmg) and drag Cmd+Meow to Applications. Setup asks only for the permissions of the tools you turn on.

Builds are signed and notarized. Cmd+Meow needs macOS 13 or later and is tested on Apple silicon. It also runs on Intel Macs, but Displays and the power readings in Mac Health work on Apple silicon only.

## Clean

Find what takes space and remove it. Everything goes to the Trash first, and you see the list before anything moves.

- **Cleanup** finds caches, logs, old installers and leftovers.
- **Disk Map** shows folders as bubbles you can open one level at a time.
- **Uninstaller** removes an app together with its leftovers.
- **Startup** lists login items, agents and daemons, and flags the ones you don't need.

<img src="docs/images/clean.jpg" alt="Cleanup">

## Monitor

- **Mac Health** shows CPU, memory, disk, battery, Wi-Fi speed and the batteries of your AirPods, mouse and keyboard.
- **AI Usage** tracks limits, pace and cost for Claude, Codex, Gemini and OpenCode.
- **Fan Control** shows fan speeds and chip heat, and runs a fixed speed or your own curve.

<img src="docs/images/monitor.jpg" alt="Mac Health">

## Create

- **Screenshot** takes an area, a window or the whole screen, then opens an editor with arrows, blur, text and zoom.
- **Video** records the screen with camera, mic and system audio.
- **Sharing** uploads a shot or a recording to your own storage and copies the link.
- **Dictation** types what you say where the cursor is. Press ⌥D. Speech never leaves the Mac.
- **Clipboard** keeps what you copy, with search, pins and quick paste.

<img src="docs/images/create.jpg" alt="Clipboard history">

### Set up Sharing

Sharing uploads to storage you own, so nobody else hosts your files. You set it up once.

**You need** a free [Cloudflare](https://dash.cloudflare.com/sign-up) account. R2 gives 10 GB free and charges nothing for downloads. The Claude desktop app makes setup automatic. Without it, Claude opens on the web.

**Steps**

1. In Cmd+Meow, open **Sharing** in the sidebar (under Create).
2. Press **Ask Claude to set up**. Claude opens with the instructions already typed in. Press Send.
3. Sign in to Cloudflare when Claude asks. If R2 asks you to accept its plan, do that yourself. Claude never types payment details.
4. Wait while Claude creates the bucket, turns on public links, adds the CORS rule and makes an access key for this bucket only.
5. Open `cmdmeow-sharing.cmdmeow` from Downloads. Cmd+Meow asks "Use this storage for sharing?". Confirm it.
6. Take a screenshot and press **Share** in the editor. The link is now on your clipboard.

Cmd+Meow keeps the secret key in your Keychain and moves the file to the Trash. Claude never writes the keys into the chat.

<details>
<summary>Set it up by hand, or use other S3 storage</summary>

Fill in these fields on the Sharing page:

| Field | Cloudflare R2 example |
| --- | --- |
| Endpoint | `<account id>.r2.cloudflarestorage.com` |
| Bucket | `cmdmeow-shares` |
| Access key, Secret key | from an R2 API token with Object Read & Write on this bucket |
| Public URL | `https://pub-….r2.dev`, or your own domain |

Any S3-compatible storage works. Turn on public read for the bucket so links open in a browser.

</details>

## Utilities

- **Keep Awake** has three modes. Off, On, and Auto, which keeps the Mac awake while an AI agent is working.
- **Displays** sets the brightness and volume of external monitors from your keyboard.

## Your panel, your theme

Click the cat in the menu bar to open the panel. It is a grid of widgets you arrange yourself, and any tool you don't use can be turned off. Dark Miami is the default. You can switch to Light Classic or Dark Classic during setup or in Settings › Theme.

<img src="docs/images/themes.jpg" alt="The menu bar panel in Dark Miami, Light Classic and Dark Classic">

### Instead of

CleanMyMac, DaisyDisk, CleanShot X, Loom, Paste, iStat Menus, Wispr Flow and Amphetamine each do one of these jobs. Cmd+Meow does all of them for free.

<sub>CleanMyMac, DaisyDisk, CleanShot X, Loom, Paste, iStat Menus, Wispr Flow and Amphetamine are trademarks of their owners. Cmd+Meow is not affiliated with them.</sub>

## Automation

Every action has a link you can call from scripts, Raycast or Shortcuts, for example `open cmdmeow://awake/toggle`. The Dev build uses `cmdmeow-dev://`.

| Link | Does |
| --- | --- |
| `awake/on`, `awake/auto`, `awake/off`, `awake/toggle` | Keep Awake |
| `capture/area`, `window`, `screen`, `delayed`, `text`, `color`, `ruler`, `record`, `history` | Screenshot and Video |
| `dictation/toggle`, `start`, `stop`, `cancel` | Dictation |
| `cat/purr`, `hiss`, `hungry`, `eat`, `hello` | The cat's reactions |
| `open/<tool>[/<page>]`, e.g. `open/disk-map`, `open/capture/settings`, `open/settings` | Open a page |
| `open/window` | Open the window |

## Privacy and safety

- No account, no telemetry. Files, clipboard, recordings and dictation never leave the Mac unless you share them.
- Clipboard and dictation history are encrypted on disk with XChaCha20-Poly1305. The key lives in your Keychain. Clipboard skips passwords from password managers, and copied tokens, keys and card numbers.
- Crash reports are opt-in. Official releases ask once during setup, and you can change it in Settings › General. A crash then sends the error, stack trace, app version and macOS version to Sentry. It sends no files, hostname or IP address, and writes your home folder as `~`. Builds from source never send anything.
- Cleanup never reads file contents. If you turn on the AI review, only paths, sizes and ages go to Claude.
- Documents, Desktop, Projects, Keychains, `.ssh` and any folder with `.git` are always kept.
- Removed items go to the Trash unless you choose "Delete right away".
- Settings › General › **Remove Cmd+Meow** deletes the app with its data, Keychain items, permissions, fan helper and login item. Screenshots and recordings in your own folders stay.

## Development

- macOS (uses AppKit for the glass material and menu bar behavior)
- Rust stable (edition 2021)
- Optional: [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) signed in, for Claude limits in AI Usage and the Cleanup AI judge

```bash
git clone https://github.com/romanfurman6/cmdmeow
cd cmdmeow
cargo run --release            # menu bar app
cargo test                     # unit tests
```

```bash
scripts/bundle-mac.sh              # target/bundle/CmdMeow.app + .zip
UNIVERSAL=1 scripts/bundle-mac.sh  # Apple silicon + Intel
```

CI (`.github/workflows/macos.yml`) runs the tests and builds a universal
`CmdMeow.app` on every push and PR; download it from the run's
**Artifacts**.

<details>
<summary><b>Releases, signing and notarization</b></summary>

Versions follow semver; `Cargo.toml` is the source of truth. The app's
Info.plist gets it as `CFBundleShortVersionString`, the commit count as
`CFBundleVersion`, and Settings › General shows both ("Cmd+Meow 0.2.0 (57)").

```sh
scripts/release.sh patch   # or minor, major, or an exact 0.2.0
```

On a clean, up-to-date `main` (needs `gh`, signed in) this bumps the version
on a `release/vX.Y.Z` branch, merges it through a PR (`main` only takes PRs:
no direct pushes, force pushes or deletion), tags the merged commit `vX.Y.Z`
and pushes the tag. Then:

1. The tag starts `.github/workflows/release.yml` on the M1 runner. The job
   runs in the protected **`release`** environment and waits.
2. GitHub notifies Roman: open the run › **Review deployments** › tick
   `release` › **Approve and deploy**. Only then does the job get the signing
   secrets.
3. Tests, universal build, sign, notarize, then a GitHub Release with
   generated notes and `CmdMeow.dmg`, `CmdMeow.zip`, `SHA256SUMS.txt`,
   `latest.json`.

The asset names never change, so
`https://github.com/romanfurman6/cmdmeow/releases/latest/download/CmdMeow.zip`
always points to the newest version (the website's Download button uses it).

#### In-app updates

`latest.json` holds the version, this tag's zip URL and its sha256. The app
reads `releases/latest/download/latest.json` 15 s after launch and every 6 h;
a newer version shows a banner in the panel, and **Update** downloads the zip,
checks the sha256, the code signature (same Apple team), bundle id and
version, swaps the app and relaunches (`src/kit/update.rs`). Publishing the
Release is the whole rollout. Tags with a `-` (`v0.2.0-beta.1`) are
prereleases and are never offered. Test locally with
`CMDMEOW_UPDATE_URL=file:///path/latest.json` (works in the dev build too).

#### Signing and notarization

Release builds are signed with **Developer ID Application** and notarized with
these secrets. They are **environment secrets of `release`** (Settings ›
Environments › release), not repository secrets, so no other workflow can use
them. The environment requires Roman's approval, allows only `v*` tags, and
admins can't bypass it. Without the secrets the release is ad-hoc signed and
users must right-click › Open.

| Secret | Value |
| --- | --- |
| `MACOS_CERT_P12` | `base64 -i DeveloperID.p12` (certificate + private key) |
| `MACOS_CERT_PASSWORD` | password of that .p12 |
| `NOTARY_KEY_P8` | contents of the App Store Connect API key `AuthKey_XXXX.p8` |
| `NOTARY_KEY_ID` | that key's Key ID |
| `NOTARY_ISSUER_ID` | Issuer ID from App Store Connect › Users and Access › Integrations |
| `SENTRY_DSN` | Sentry project DSN for crash reports (Project Settings › Client Keys). Compiled into release builds only |

The signed app runs with the hardened runtime and `assets/CmdMeow.entitlements`
(microphone, camera, Apple Events).

Releases from 0.2.0 on are notarized. If notarization fails (it did for 0.1.0
and 0.1.1 while Apple set up the developer account), `release.yml` still
publishes the Developer ID signed build and the release name says
"(not notarized)".

Coming from CleanMeBro (the old name; bundle id is now
`com.romanfurman6.cmdmeow`): the new bundle id makes it a new app to macOS. Grant
Screen Recording, Accessibility and Full Disk Access again, turn **Open at
login** back on, and delete the old app. Settings, clipboard history and the
sharing key move over on first launch. The fan helper keeps its old id, so an
installed helper keeps working.

The icon artwork is `assets/icon/AppIcon-art.png`; regenerate `assets/AppIcon.icns`
with `python3 scripts/make-icon.py` (needs `pip install pillow`). The menu bar
icon is drawn from vector paths in `src/kit/brand.rs`.

</details>

<details>
<summary><b>Test and preview modes</b></summary>

```bash
# Read-only Cleanup check: prints what would be offered, removes nothing
cargo run --release -- --scan-report
cargo run --release -- --scan-report --ai     # also ask Claude to judge

# Render a view with sample data to a PNG and exit
cargo run --release -- --screenshot out.png --view panel
```

`--view` values: `panel`, `customize` (Settings › Widgets), `settings-tools`,
`storage`, `memory`, `battery`, `cpu`, `network`, `devices` (Mac Health with that card's details), `claude-detail` (AI Usage with the limits popup),
`cleanup`, `results`, `lens`, `lens-live`, `claude`, `clipboard`,
`clipboard-settings`, `clipboard-quick`, `clipboard-list`.

</details>

<details>
<summary><b>Architecture</b></summary>

The app is a small **core** (the shell) plus independent **tools**. Each tool
is one folder under `src/tools/` and implements the `Tool` trait
(`src/core/tool.rs`). A tool can offer any of: panel widgets, detail sheets,
a page in the full window, recommendations and the menu-bar title.

- Tools never import each other (a test enforces it). To link to another tool
  they ask the core by id, e.g. `cx.open_page("cleanup", "claude")`; if that
  tool isn't installed, nothing happens.
- Shared code that several tools need lives in `src/kit/` and knows no tool.
- The panel lays widgets out on a 4-column grid: each widget spans 1, 2 or 4
  columns and reports its own height. The panel grows with its widgets and
  scrolls past the screen height.

#### Adding a tool

1. Create `src/tools/<name>/mod.rs` with a struct implementing `Tool`
   (`id`, `name`, `badge` are required; everything else is optional).
2. Add one line to `all()` in `src/tools/mod.rs`.

Its widgets appear in the gallery of **Settings › Widgets**, it gets an
on/off switch in **Settings › Tools**, and if it has a page it gets a sidebar
entry. Nothing in `src/core/` changes.

</details>

<details>
<summary><b>Project layout</b></summary>

```
cmdmeow/
├── Cargo.toml / Cargo.lock
├── src/
│   ├── main.rs              entry point, CLI flags
│   ├── core/                the shell; knows tools only through the trait
│   │   ├── tool.rs          Tool trait, widget specs, actions, Cx
│   │   ├── shell.rs         tray icon, panel ⇄ full window, actions, screenshots
│   │   ├── panel.rs         widget grid, recommendation, footer, Open button, detail sheet
│   │   ├── editor.rs        Settings: drag-and-drop widget editor, tools on/off
│   │   ├── window.rs        full window: sidebar of tool pages and Settings
│   │   ├── layout.rs        4-column widget packing
│   │   └── settings.rs      saved widget order, visibility and tools turned off
│   ├── kit/                 shared by all tools
│   │   ├── glass.rs         drawing kit: glass cards, gradients, glows, vector icons
│   │   ├── brand.rs         the Cmd+Meow mark as vector paths (menu bar icon)
│   │   ├── controls.rs      shared page controls (round action button)
│   │   ├── native.rs        macOS: glass material, menu-bar app, thermal state, Trash
│   │   ├── fs.rs            folder sizes, protected paths, move to Trash, disk space
│   │   ├── claude_code.rs   Claude Code CLI and project discovery
│   │   ├── ai/              AI usage providers and API prices
│   │   └── mechanics.rs     numbers/thresholds from docs/MECHANICS.md
│   └── tools/
│       ├── mod.rs           the tool list (the only place that names tools)
│       ├── system/          Mac Health: readings, widgets, detail sheets
│       ├── cleanup/         junk rules, AI judge, removal, --scan-report
│       ├── lens/            Disk Map: folder measuring and live bubbles
│       ├── uninstaller/     apps, leftovers, drop-to-uninstall widget
│       ├── startup/         login items, launch agents and daemons
│       ├── capture/         Screenshot, Video and Sharing (one engine, three tools)
│       ├── dictation/       speech to text at the caret
│       ├── clipboard/       clipboard history and quick paste
│       ├── fan/             fan speeds, temperatures, helper
│       ├── keep_awake/      keep the Mac awake: Off, Auto (AI agents) or On
│       ├── displays/        brightness and volume of external displays
│       └── claude/          AI Usage: usage of Claude, Codex, Gemini, OpenCode
├── vendor/eframe/        eframe 0.37 with one patch (see Cargo.toml)
├── docs/
│   ├── images/          README images
│   ├── MECHANICS.md     rules, thresholds and where each came from
│   └── mechanics.json   same data, machine-readable
├── assets/
│   ├── AppIcon.icns     app icon, all sizes (generated)
│   └── icon/AppIcon-art.png icon artwork
├── scripts/
│   ├── bundle-mac.sh    build CmdMeow.app + zip
│   └── make-icon.py     artwork → AppIcon.icns
└── .github/workflows/macos.yml   CI: test, build, upload the app
```

The screenshots in `docs/images/` are rendered from sample data with
`CMDMEOW_THEME=light|dark|miami` and `--screenshot <file.png> --view <name>`,
then placed on the website's desktop pictures (`site/desk*.jpg`).

</details>

## License

MIT, see [LICENSE](LICENSE).
