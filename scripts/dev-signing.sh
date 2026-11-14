#!/usr/bin/env bash
# A stable code signing identity for local dev builds: a self-signed
# "CmdMeow Dev Signing" certificate in its own keychain
# (~/Library/Keychains/cmdmeow-dev.keychain-db). Every build signed with it
# has the same designated requirement, so macOS keeps privacy grants
# (Screen Recording, Accessibility, …) across rebuilds; an ad hoc signature
# is a new app to macOS after every build.
#
# Creates the keychain and certificate the first time, then just unlocks the
# keychain and prints the identity's SHA-1 (what codesign --sign takes):
#
#   SIGN_ID="$(scripts/dev-signing.sh)" scripts/bundle-mac.sh
#
# The keychain password is not a secret: the certificate is trusted by
# nothing and only names this Mac's dev builds. dev-install.sh uses it by
# default; SIGN_ID overrides.
set -euo pipefail

NAME="CmdMeow Dev Signing"
KC="$HOME/Library/Keychains/cmdmeow-dev.keychain-db"
PASS="cmdmeow-dev"

find_id() {
  security find-identity -p codesigning "$KC" 2>/dev/null | awk -v n="\"$NAME\"" 'index($0, n) { print $2; exit }'
}

if [[ ! -f "$KC" ]]; then
  tmp="$(mktemp -d)"
  made=0
  # a half-made keychain would pass the check above on every later run
  trap 'rm -rf "$tmp"; [[ $made == 1 ]] || security delete-keychain "$KC" 2>/dev/null || rm -f "$KC"' EXIT
  cat > "$tmp/cfg" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$tmp/cfg" \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
  openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -name "$NAME" \
    -passout "pass:$PASS" -out "$tmp/id.p12"
  security create-keychain -p "$PASS" "$KC"
  security set-keychain-settings "$KC" # no auto-lock
  security unlock-keychain -p "$PASS" "$KC"
  security import "$tmp/id.p12" -k "$KC" -P "$PASS" -T /usr/bin/codesign -f pkcs12 >/dev/null
  # let codesign use the key without a dialog
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$PASS" "$KC" >/dev/null
  made=1
  echo "Created $NAME in $KC" >&2
fi

# locked again after every restart
security unlock-keychain -p "$PASS" "$KC"

# codesign only finds identities in keychains on the search list
if ! security list-keychains -d user | grep -qF "$KC"; then
  list=()
  while IFS= read -r k; do
    k="${k#"${k%%[![:space:]]*}"}"
    list+=("${k//\"/}")
  done < <(security list-keychains -d user)
  security list-keychains -d user -s "${list[@]}" "$KC"
fi

id="$(find_id)"
[[ -n "$id" ]] || { echo "No \"$NAME\" identity in $KC" >&2; exit 1; }
echo "$id"
