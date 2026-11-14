#!/usr/bin/env bash
# Grant a CmdMeow build every macOS privacy permission it asks for, without
# the prompts: writes the rows straight into the TCC databases. Only for a
# test Mac with System Integrity Protection off (macOS refuses the writes
# otherwise); the system database needs sudo.
#
#   scripts/dev-grant.sh                          # /Applications/CmdMeow Dev.app
#   scripts/dev-grant.sh path/to/Some.app
#   scripts/dev-grant.sh --revoke [path/to/app]   # forget them, to test the prompts
#
# The grants are tied to the app's signature (its designated requirement).
# Sign with a stable identity (scripts/dev-signing.sh, the dev-install.sh
# default) and they survive rebuilds; an ad hoc build needs this again after
# every build. Notifications aren't a TCC permission and still ask once.
#
# It also moves the app's history key into the dev keychain and lets this
# build read it, so the Keychain doesn't ask for the login password either.
set -euo pipefail

REVOKE=0
if [[ "${1:-}" == "--revoke" ]]; then
  REVOKE=1
  shift
fi
APP="${1:-/Applications/CmdMeow Dev.app}"
[[ -d "$APP" ]] || { echo "No app at $APP" >&2; exit 1; }
APP="$(cd "$APP" && pwd)"

if ! csrutil status | grep -q disabled; then
  echo "System Integrity Protection is on; macOS won't take TCC writes. Turn it off in recoveryOS (csrutil disable)." >&2
  exit 1
fi

USER_DB="$HOME/Library/Application Support/com.apple.TCC/TCC.db"
SYSTEM_DB="/Library/Application Support/com.apple.TCC/TCC.db"

# Accessibility also covers the media key event tap (Displays)
SYSTEM_SERVICES=(kTCCServiceAccessibility kTCCServiceScreenCapture kTCCServiceSystemPolicyAllFiles kTCCServiceListenEvent kTCCServicePostEvent)
USER_SERVICES=(kTCCServiceMicrophone kTCCServiceCamera kTCCServiceBluetoothAlways)
# Apple Events targets: System Events (login items), Finder (save folder)
EVENT_TARGETS=("com.apple.systemevents:/System/Library/CoreServices/System Events.app" "com.apple.finder:/System/Library/CoreServices/Finder.app")

BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP/Contents/Info.plist")"
EXE="$APP/Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"

sql_user() { sqlite3 "$USER_DB" "$1"; }
sql_system() { sudo sqlite3 "$SYSTEM_DB" "$1"; }

if [[ "$REVOKE" == "1" ]]; then
  sql_user "DELETE FROM access WHERE client = '$BUNDLE_ID';"
  sql_system "DELETE FROM access WHERE client = '$BUNDLE_ID';"
  defaults delete "$HOME/Library/Group Containers/group.com.apple.replayd/ScreenCaptureApprovals" "$EXE" 2>/dev/null || true
  killall tccd 2>/dev/null || true
  sudo killall tccd 2>/dev/null || true
  echo "Revoked every permission of $BUNDLE_ID"
  exit 0
fi

# The code requirement TCC checks the app against, as a hex blob.
csreq_hex() {
  local req
  req="$(codesign -d -r- "$1" 2>&1 | sed -n 's/^designated => //p')"
  [[ -n "$req" ]] || { echo "Can't read the signature of $1" >&2; exit 1; }
  local bin
  bin="$(mktemp)"
  csreq -r- -b "$bin" <<< "$req"
  xxd -p "$bin" | tr -d '\n'
  rm -f "$bin"
}

if codesign -dv "$APP" 2>&1 | grep -q 'Signature=adhoc'; then
  echo "Note: $APP is signed ad hoc; these grants last until the next build. Sign with scripts/dev-signing.sh to keep them." >&2
fi
REQ="$(csreq_hex "$APP")"
NOW="$(date +%s)"

# auth_value 2 = allowed, auth_reason 3 = set by the user, auth_version 1
row() { # service [target_id target_req_hex]
  local target="'UNUSED'" target_type="NULL" target_req="NULL"
  if [[ $# -gt 1 ]]; then
    target="'$2'" target_type=0 target_req="X'$3'"
  fi
  echo "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version, csreq, policy_id, indirect_object_identifier_type, indirect_object_identifier, indirect_object_code_identity, flags, last_modified, last_reminded) VALUES ('$1', '$BUNDLE_ID', 0, 2, 3, 1, X'$REQ', NULL, $target_type, $target, $target_req, 0, $NOW, $NOW);"
}

sys_sql=""
for s in "${SYSTEM_SERVICES[@]}"; do sys_sql+="$(row "$s")"; done
user_sql=""
for s in "${USER_SERVICES[@]}"; do user_sql+="$(row "$s")"; done
for t in "${EVENT_TARGETS[@]}"; do
  user_sql+="$(row kTCCServiceAppleEvents "${t%%:*}" "$(csreq_hex "${t#*:}")")"
done

sql_user "BEGIN; $user_sql COMMIT;"
sql_system "BEGIN; $sys_sql COMMIT;"

# Skip the monthly "keep allowing screen recording?" reminder (macOS 15+).
defaults write "$HOME/Library/Group Containers/group.com.apple.replayd/ScreenCaptureApprovals" "$EXE" -date "3024-01-01 00:00:00 +0000"

# tccd caches; restart both (they come back on demand)
killall tccd 2>/dev/null || true
sudo killall tccd 2>/dev/null || true

# The history key (kit/vault.rs). macOS ties a Keychain item to the one
# build that made it (a cdhash partition), so every rebuild would ask for the
# login password. Keep it in the dev keychain instead, whose password we know,
# and let this build in. The app finds it there through the search list.
DEV_KC="$HOME/Library/Keychains/cmdmeow-dev.keychain-db"
DEV_KC_PASS="cmdmeow-dev" # same as dev-signing.sh
if [[ "$(basename "$APP" .app)" == *" Dev" ]]; then
  SERVICE="Cmd+Meow Dev history"
else
  SERVICE="Cmd+Meow history"
fi
"$(dirname "$0")/dev-signing.sh" >/dev/null # makes and unlocks the dev keychain
LOGIN_KC="$HOME/Library/Keychains/login.keychain-db"
KEY_NOTE="Keychain ($SERVICE)"
if ! security find-generic-password -s "$SERVICE" -a key "$DEV_KC" >/dev/null 2>&1; then
  if security find-generic-password -s "$SERVICE" -a key "$LOGIN_KC" >/dev/null 2>&1 && [[ "${RESET_KEY:-0}" != 1 ]]; then
    # A key in the login keychain can't be read without its password, and
    # replacing it makes history sealed with it unreadable: keep it unless
    # asked. Rebuilds then ask for the login password once each.
    KEY_NOTE="Keychain skipped: $SERVICE key is in the login keychain (RESET_KEY=1 replaces it, old history is lost)"
  else
    security delete-generic-password -s "$SERVICE" -a key "$LOGIN_KC" >/dev/null 2>&1 || true
    security add-generic-password -s "$SERVICE" -a key -X "$(openssl rand -hex 32)" -T "$APP" "$DEV_KC"
  fi
fi
if security find-generic-password -s "$SERVICE" -a key "$DEV_KC" >/dev/null 2>&1; then
  CDHASH="$(codesign -dvvv "$APP" 2>&1 | sed -n 's/^CDHash=//p')"
  security set-generic-password-partition-list -s "$SERVICE" -a key -S "apple-tool:,apple:,cdhash:$CDHASH" -k "$DEV_KC_PASS" "$DEV_KC" >/dev/null
fi

echo "Granted $BUNDLE_ID: ${SYSTEM_SERVICES[*]#kTCCService} ${USER_SERVICES[*]#kTCCService} AppleEvents(System Events, Finder), $KEY_NOTE"
