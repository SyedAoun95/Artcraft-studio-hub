#!/bin/zsh
# Build "Studio Hub.app" (local, unsigned) from the release binary.
#   scripts/make-app.sh [destination folder]   (default: ~/Desktop)
# Re-run after every `cargo build --release` so the app has the new binary.
set -e
cd "${0:A:h}/.."
DEST="${1:-$HOME/Desktop}"
APP="$DEST/Studio Hub.app"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/studio-hub "$APP/Contents/MacOS/studio-hub"
cp assets/icons/StudioHub.icns "$APP/Contents/Resources/StudioHub.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Studio Hub</string>
  <key>CFBundleDisplayName</key><string>Studio Hub</string>
  <key>CFBundleIdentifier</key><string>local.studiohub.app</string>
  <key>CFBundleExecutable</key><string>studio-hub</string>
  <key>CFBundleIconFile</key><string>StudioHub</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
echo "built: $APP"
