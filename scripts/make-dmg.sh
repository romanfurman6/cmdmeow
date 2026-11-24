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

# dmgbuild 1.6.7 is the first that drops the pBBk background bookmark from
# .DS_Store; with that record Finder on macOS 26.2+ shows no background
# (dmgbuild#273, FB21405103). It needs Python 3.10+, and /usr/bin/python3 is 3.9.
PY=""
for exe in /opt/homebrew/bin/python3 /usr/local/bin/python3 \
           /opt/homebrew/opt/python@3.*/bin/python3.* \
           $(compgen -c python3. | sort -t. -k2 -rn | uniq) python3; do
  if command -v "$exe" >/dev/null 2>&1 && "$exe" -c 'import sys; sys.exit(sys.version_info < (3, 10))' 2>/dev/null; then
    PY="$exe"; break
  fi
done
[[ -n "$PY" ]] || { echo "need Python 3.10+ for dmgbuild (brew install python)" >&2; exit 1; }

"$PY" -m venv "$WORK/venv"
"$WORK/venv/bin/pip" install --quiet --disable-pip-version-check "dmgbuild==$DMGBUILD_VERSION"

rm -f "$OUT"
"$WORK/venv/bin/dmgbuild" -s scripts/dmg-settings.py \
  -D app="$APP" \
  -D background=assets/dmg/background.png \
  -D volume_icon=assets/AppIcon.icns \
  "Cmd+Meow" "$OUT"
# Fail the build if Finder would show a blank window: no pBBk record, and
# the background is a picture whose alias points at a file on the volume.
MNT="$WORK/mnt"
mkdir -p "$MNT"
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$MNT" "$OUT" >/dev/null
trap 'hdiutil detach -quiet "$MNT" 2>/dev/null || true; rm -rf "$WORK"' EXIT
"$WORK/venv/bin/python" - "$MNT" <<'PY'
import sys
from ds_store import DSStore
mnt = sys.argv[1]
codes, bg = set(), None
with DSStore.open(f"{mnt}/.DS_Store", "r") as d:
    for e in d:
        if e.filename == ".":
            codes.add(e.code)
            if e.code == b"icvp":
                bg = e.value
if b"pBBk" in codes:
    sys.exit("dmg: .DS_Store has a pBBk record (blank background on macOS 26.2+)")
if not bg or bg.get("backgroundType") != 2 or not bg.get("backgroundImageAlias"):
    sys.exit("dmg: .DS_Store has no picture background")
print("dmg: background ok, no pBBk")
PY
hdiutil detach -quiet "$MNT"
echo "built $OUT"
