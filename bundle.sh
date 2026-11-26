#!/bin/bash
# Build Ghost.app: the release binary in a macOS bundle with the ghost as its
# icon, so that Finder, Launchpad and the Dock show it like any other app.
#
#   ./bundle.sh               # -> target/release/Ghost.app
#   open target/release/Ghost.app
#
# Needs rsvg-convert (brew install librsvg) to draw the icon, and iconutil
# and codesign, which macOS has.
#
# It is signed with GHOST_SIGN_IDENTITY if that is set ("-" for ad hoc),
# else with a code-signing certificate called "Ghost Dev" in your keychain
# if there is one, else ad hoc. Signed with the same identity, a rebuilt app
# is the same app to macOS and keeps what it was allowed, such as the
# Desktop folder an agent works in; signed ad hoc, every build is a new
# app, and is asked again. README.md says how to make the certificate.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

app=target/release/Ghost.app
iconset=target/release/Ghost.iconset
rm -rf "$app" "$iconset"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iconset"

# Every size macOS asks an .icns for, each drawn from the SVG rather than
# scaled from another picture.
for size in 16 32 128 256 512; do
    rsvg-convert -w "$size" -h "$size" assets/app/icon.svg -o "$iconset/icon_${size}x${size}.png"
    double=$((size * 2))
    rsvg-convert -w "$double" -h "$double" assets/app/icon.svg -o "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Ghost.icns"
rm -rf "$iconset"

cp target/release/ghost-desk "$app/Contents/MacOS/ghost-desk"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# The bundle identifier is also the preference domain an organisation's
# profile sets the policy in (crates/core/src/policy/profile.rs).
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Ghost</string>
    <key>CFBundleDisplayName</key><string>Ghost</string>
    <key>CFBundleIdentifier</key><string>local.ghost.desk</string>
    <key>CFBundleExecutable</key><string>ghost-desk</string>
    <key>CFBundleIconFile</key><string>Ghost</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${version}</string>
    <key>CFBundleVersion</key><string>${version}</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleURLTypes</key>
    <array><dict>
        <key>CFBundleURLName</key><string>Ghost link</string>
        <key>CFBundleURLSchemes</key><array><string>ghost</string></array>
    </dict></array>
    <key>CFBundleDocumentTypes</key>
    <array><dict>
        <key>CFBundleTypeName</key><string>Folder</string>
        <key>CFBundleTypeRole</key><string>Viewer</string>
        <key>LSHandlerRank</key><string>None</string>
        <key>LSItemContentTypes</key><array><string>public.folder</string></array>
    </dict></array>
    <key>NSDesktopFolderUsageDescription</key><string>The agents you start work in the folders you choose, which may be in your Desktop folder.</string>
    <key>NSMicrophoneUsageDescription</key><string>What you dictate is heard through the microphone, on this Mac.</string>
    <key>NSSpeechRecognitionUsageDescription</key><string>What you dictate is turned into text by speech recognition on this Mac.</string>
</dict>
</plist>
EOF

identity="${GHOST_SIGN_IDENTITY:-}"
if [ -z "$identity" ]; then
    # Without -v, which lists only identities the system trusts: a
    # self-signed certificate is not, and signs all the same.
    identity=$(security find-identity -p codesigning 2>/dev/null | awk '/"Ghost Dev"/ { print $2; exit }')
fi
identity="${identity:--}"
codesign --force --sign "$identity" "$app"

echo "built $app"
if [ "$identity" = "-" ]; then
    echo "signed ad hoc: macOS will ask again for what it allowed the last build (see README.md)"
else
    echo "signed with $identity"
fi
