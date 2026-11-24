#!/usr/bin/env bash
# Build target/bundle/CmdMeow.dmg from target/bundle/CmdMeow.app with the
# drag-to-Applications window (background, icon places, volume icon).
#
#   scripts/make-dmg.sh [path/to/CmdMeow.app] [out.dmg]
#
# Needs macOS (hdiutil, tiffutil) and python3. dmgbuild finds background@2x.png
# next to background.png and joins both into one Retina TIFF. dmgbuild writes the window
# layout straight into .DS_Store, so no Finder/AppleScript or GUI session.
# Signing and notarizing the dmg stay in release.yml.
set -euo pipefail
cd "$(dirname "$0")/.."

APP="${1:-target/bundle/CmdMeow.app}"
OUT="${2:-target/bundle/CmdMeow.dmg}"
DMGBUILD_VERSION="1.6.7"

[[ -d "$APP" ]] || { echo "no app at $APP" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

python3 -m venv "$WORK/venv"
"$WORK/venv/bin/pip" install --quiet --disable-pip-version-check "dmgbuild==$DMGBUILD_VERSION"

rm -f "$OUT"
"$WORK/venv/bin/dmgbuild" -s scripts/dmg-settings.py \
  -D app="$APP" \
  -D background=assets/dmg/background.png \
  -D volume_icon=assets/AppIcon.icns \
  "Cmd+Meow" "$OUT"
echo "built $OUT"
