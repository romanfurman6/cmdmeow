#!/usr/bin/env bash
# Local dev loop: build, bundle as "CmdMeow Dev.app" (bundle id
# com.romanfurman6.cmdmeow.dev, so it never clashes with a release
# install), sign, install to /Applications and relaunch with logs in
# ~/Library/Logs/CmdMeow/dev.log.
#
# The first install after the rename from CleanMeBro is a new app to macOS:
# grant Screen Recording and Accessibility again and remove the old
# "CleanMeBro Dev.app" from /Applications.
#
# Signed with a stable identity, so macOS keeps the Screen Recording /
# Accessibility grants across builds: the self-signed one from
# scripts/dev-signing.sh (created on first run), or SIGN_ID (e.g. your
# "Apple Development: …" certificate name or SHA-1 from
# `security find-identity -v -p codesigning`). SIGN_ID=- signs ad hoc, which
# macOS treats as a new app after every build.
#
# With System Integrity Protection off (a test Mac), scripts/dev-grant.sh then
# grants every permission without the prompts (sudo for the system ones).
# GRANT=0 skips that.
#
#   SIGN_ID="Apple Development: …" scripts/dev-install.sh
#   NO_PULL=1 scripts/dev-install.sh     # build what's checked out
#   PROFILE=debug scripts/dev-install.sh # unoptimized build
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${SIGN_ID:-}" ]]; then SIGN_ID="$(scripts/dev-signing.sh)"; fi
export SIGN_ID
export APP_NAME="CmdMeow Dev"
export DISPLAY_NAME="Cmd+Meow Dev"
export BUNDLE_ID="com.romanfurman6.cmdmeow.dev"
export URL_SCHEME="cmdmeow-dev"
export PROFILE="${PROFILE:-release}"

DEST="/Applications/$APP_NAME.app"
BIN_PATH="$DEST/Contents/MacOS/cmdmeow"
LOG_DIR="$HOME/Library/Logs/CmdMeow"
LOG="$LOG_DIR/dev.log"

if [[ "${NO_PULL:-0}" != "1" ]]; then
  # never pull over local edits
  if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
    echo "Working tree has local changes; not pulling. Building what's here." >&2
  else
    git pull --ff-only
  fi
fi

./scripts/bundle-mac.sh

# Stop the running dev instance (if any) before swapping the bundle.
pkill -f "$BIN_PATH" 2>/dev/null || true
sleep 1

rm -rf "$DEST"
ditto "target/bundle/$APP_NAME.app" "$DEST"
codesign -dv --verbose=2 "$DEST" 2>&1 | grep -E '^(Identifier|Authority|TeamIdentifier)=' || true

if [[ "${GRANT:-1}" == "1" ]] && csrutil status | grep -q disabled; then
  scripts/dev-grant.sh "$DEST" || echo "!! dev-grant.sh failed; the app will ask for its permissions" >&2
fi

mkdir -p "$LOG_DIR"
echo "===== $(date '+%Y-%m-%d %H:%M:%S') dev-install: $(git rev-parse --abbrev-ref HEAD)@$(git rev-parse --short HEAD) ($PROFILE) =====" >> "$LOG"
# Launch through LaunchServices (not as a child of this shell) so macOS
# attributes Screen Recording to the app itself; stdout/stderr go to the log.
open -n -a "$DEST" --stdout "$LOG" --stderr "$LOG" --env RUST_BACKTRACE=1 --env RUST_LOG=debug

sleep 5
pgrep -fl "$BIN_PATH" || echo "!! CmdMeow Dev is NOT running"
echo "--- last 30 lines of $LOG ---"
tail -30 "$LOG"
echo
echo "Follow the log:  tail -f \"$LOG\""
