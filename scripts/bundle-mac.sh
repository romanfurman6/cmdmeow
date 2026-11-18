#!/usr/bin/env bash
# Build CmdMeow.app (shown as "Cmd+Meow") and a zip of it from this repo. macOS only.
#
#   scripts/bundle-mac.sh              # native arch
#   UNIVERSAL=1 scripts/bundle-mac.sh  # Apple silicon + Intel in one binary
#   TARGET=aarch64-apple-darwin scripts/bundle-mac.sh  # one explicit target
#   SIGN_ID=... HARDENED=1 APP_NAME=... DISPLAY_NAME=... BUNDLE_ID=... URL_SCHEME=... PROFILE=debug override the defaults
#
# Output: target/bundle/CmdMeow.app and target/bundle/CmdMeow-<version>-macos.zip
set -euo pipefail

cd "$(dirname "$0")/.."
APP_NAME="${APP_NAME:-CmdMeow}"
DISPLAY_NAME="${DISPLAY_NAME:-Cmd+Meow}"
BIN="cmdmeow"
BUNDLE_ID="${BUNDLE_ID:-com.romanfurman6.cmdmeow}"
# Links into the app (cmdmeow://awake/on). The Dev build uses its own.
URL_SCHEME="${URL_SCHEME:-cmdmeow}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
# Build number (CFBundleVersion): commit count, so every build on main is higher
# than the last. BUILD_NUMBER overrides it. The app shows it in Settings.
BUILD="${BUILD_NUMBER:-$(git rev-list --count HEAD 2>/dev/null || echo 1)}"
export CMDMEOW_BUILD="$BUILD"
OUT="target/bundle"
APP="$OUT/$APP_NAME.app"

if [[ "${UNIVERSAL:-0}" == "1" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/release
  lipo -create -output "target/release/$BIN" \
    "target/aarch64-apple-darwin/release/$BIN" \
    "target/x86_64-apple-darwin/release/$BIN"
elif [[ -n "${TARGET:-}" ]]; then
  # one explicit target (CI pull requests: reuses the test build's deps)
  cargo build --release --target "$TARGET"
  mkdir -p target/release
  cp "target/$TARGET/release/$BIN" "target/release/$BIN"
elif [[ "${PROFILE:-release}" == "debug" ]]; then
  cargo build
else
  cargo build --release
fi
BUILD_DIR="target/${PROFILE:-release}"
[[ "${UNIVERSAL:-0}" == "1" ]] && BUILD_DIR="target/release"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BUILD_DIR/$BIN" "$APP/Contents/MacOS/$BIN"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# the cat's sounds (src/kit/cat)
if [[ -d assets/cat ]]; then cp -R assets/cat "$APP/Contents/Resources/cat"; fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$DISPLAY_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$BUILD</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <!-- Menu bar app: no Dock icon until the full window opens. -->
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key>
  <array><dict>
    <key>CFBundleURLName</key><string>$BUNDLE_ID</string>
    <key>CFBundleURLSchemes</key><array><string>$URL_SCHEME</string></array>
  </dict></array>
  <!-- The storage file Claude saves after setting up sharing (cmdmeow-sharing.cmdmeow). -->
  <key>UTExportedTypeDeclarations</key>
  <array><dict>
    <key>UTTypeIdentifier</key><string>com.romanfurman6.cmdmeow.sharing</string>
    <key>UTTypeDescription</key><string>Cmd+Meow Sharing Setup</string>
    <key>UTTypeConformsTo</key><array><string>public.json</string></array>
    <key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>cmdmeow</string></array></dict>
  </dict></array>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeName</key><string>Cmd+Meow Sharing Setup</string>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Owner</string>
    <key>LSItemContentTypes</key><array><string>com.romanfurman6.cmdmeow.sharing</string></array>
  </dict></array>
  <key>NSAppleEventsUsageDescription</key><string>$DISPLAY_NAME asks Finder to pick a folder for saved captures and where your desktop icons lie when the cat tidies them, and System Events for the apps that open at login.</string>
  <key>NSDesktopFolderUsageDescription</key><string>$DISPLAY_NAME looks at your Desktop to clean it up, and its cat pushes the desktop icons into Finder's grid when you press its shortcut.</string>
  <key>NSMicrophoneUsageDescription</key><string>$DISPLAY_NAME hears your voice for dictation, and in screen recordings when you turn the microphone on.</string>
  <key>NSBluetoothAlwaysUsageDescription</key><string>$DISPLAY_NAME shows the battery of your AirPods, iPhone and other Bluetooth devices.</string>
  <key>NSCameraUsageDescription</key><string>$DISPLAY_NAME shows your camera in screen recordings when you turn the camera on.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature by default so Apple silicon runs it; SIGN_ID signs with a
# real identity instead. HARDENED=1 adds the hardened runtime + entitlements
# and a secure timestamp, which notarization requires (release.yml).
SIGN_ARGS=(--force --deep --sign "${SIGN_ID:--}")
if [[ "${HARDENED:-0}" == "1" ]]; then
  SIGN_ARGS+=(--options runtime --timestamp --entitlements assets/CmdMeow.entitlements)
fi
[[ -n "${SIGN_KEYCHAIN:-}" ]] && SIGN_ARGS+=(--keychain "$SIGN_KEYCHAIN")
codesign "${SIGN_ARGS[@]}" "$APP"

ZIP="$OUT/$APP_NAME-$VERSION-macos.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "Built $APP ($VERSION, build $BUILD)"
echo "Zipped $ZIP"
