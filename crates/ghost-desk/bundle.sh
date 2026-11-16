#!/bin/bash
# Build Ghost.app: the release binary in a macOS bundle with the ghost as its
# icon, so that Finder, Launchpad and the Dock show it like any other app.
#
#   crates/ghost-desk/bundle.sh            # -> crates/ghost-desk/target/release/Ghost.app
#   open crates/ghost-desk/target/release/Ghost.app
#
# Needs rsvg-convert (brew install librsvg) to draw the icon, and iconutil,
# which macOS has.
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
</dict>
</plist>
EOF

echo "built $app"
