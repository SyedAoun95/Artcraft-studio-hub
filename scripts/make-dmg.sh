#!/bin/zsh
# Build the suite installer: one .dmg with Studio Hub.app and the seven apps, ready to drag to Applications.
#
#   scripts/make-dmg.sh [--arch x86_64|aarch64|universal]
#
# Each app must first be packaged with its own official script, which builds and signs <Name>.app:
#   <app>/packaging/macos/package.sh --arch <arch>      (output: <app>/target/macos-package/<Name>.app)
# Signing: MACOS_SIGN_IDENTITY (default "-" = ad-hoc: fine on this Mac; other Macs need
# "Open Anyway" in System Settings until the suite is signed with a Developer ID and notarized).
set -euo pipefail
cd "${0:A:h}/.."
HUB="$PWD"; ROOT="${HUB:h}"
ARCH=x86_64
[[ "${1:-}" == "--arch" ]] && ARCH="$2"
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
APPS=(photocraft:PhotoCraft vectorcraft:VectorCraft lightcraft:LightCraft designcraft:DesignCraft filmcraft:FilmCraft effectcraft:EffectCraft pdfcraft:PdfCraft)
WORK="$HUB/target/suite-dmg"; STAGE="$WORK/stage"
DMG="$HUB/dist/Studio-Suite-$VERSION-macos-$ARCH.dmg"
sign() { codesign --force --sign "$IDENTITY" --timestamp=none "$@"; }

echo "==> checking the seven app bundles"
for pair in $APPS; do
  id=${pair%%:*}; name=${pair##*:}
  [[ -d "$ROOT/$id/target/macos-package/$name.app" ]] || { echo "missing $name.app: run $id/packaging/macos/package.sh --arch $ARCH" >&2; exit 1; }
  [[ -x "$ROOT/$id/target/macos-package/bin/$id-cli" ]] || { echo "missing $id-cli in $id/target/macos-package/bin" >&2; exit 1; }
done

echo "==> Studio Hub.app"
cargo build --release
rm -rf "$WORK"; mkdir -p "$STAGE" "$HUB/dist"
APP="$STAGE/Studio Hub.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"
cp target/release/studio-hub "$APP/Contents/MacOS/studio-hub"
cp assets/icons/StudioHub.icns "$APP/Contents/Resources/StudioHub.icns"
for pair in $APPS; do
  id=${pair%%:*}
  cp "$ROOT/$id/target/macos-package/bin/$id-cli" "$APP/Contents/Helpers/$id-cli"
done
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
plutil -lint "$APP/Contents/Info.plist" >/dev/null
printf 'APPL????' > "$APP/Contents/PkgInfo"
# Inside-out: helpers, main executable, then the bundle.
for f in "$APP"/Contents/Helpers/*; do sign --identifier "local.studiohub.$(basename "$f")" "$f"; done
sign "$APP/Contents/MacOS/studio-hub"
sign "$APP"
codesign --verify --strict --deep "$APP"

echo "==> the seven apps"
for pair in $APPS; do
  id=${pair%%:*}; name=${pair##*:}
  ditto "$ROOT/$id/target/macos-package/$name.app" "$STAGE/$name.app"
  codesign --verify --strict --deep "$STAGE/$name.app"
done

echo "==> licences and read-me"
mkdir -p "$STAGE/Licenses"
for pair in $APPS; do
  id=${pair%%:*}; name=${pair##*:}
  mkdir -p "$STAGE/Licenses/$name"
  cp "$ROOT/$id/LICENSE-MIT" "$ROOT/$id/LICENSE-APACHE" "$ROOT/$id/NOTICE" "$STAGE/Licenses/$name/"
done
cat > "$STAGE/Read Me.txt" <<TXT
Studio Suite $VERSION

1. Drag ALL the apps (Studio Hub and the seven apps) onto the Applications folder.
2. Open Studio Hub from Applications. It lists the seven apps; open each one from there
   (apps opened from Launchpad or Finder work normally but AI agents can't control them).
3. To let an AI tool control the apps, click Connect next to Claude Code, Codex or Claude Desktop
   in Studio Hub, then start a new session in that tool.

First launch on another Mac: this build is not yet notarized by Apple. If macOS says it can't
open an app, open System Settings > Privacy & Security and click "Open Anyway" (once per app).

The seven apps are open source (MIT OR Apache-2.0); their licences are in the Licenses folder.
TXT
ln -s /Applications "$STAGE/Applications"

echo "==> $DMG"
# No Finder metadata on the bundles: `hdiutil makehybrid` (used by the apps' own scripts) stamps
# com.apple.FinderInfo on every .app, which makes `codesign --verify --strict` (and Gatekeeper's
# strict checks) fail on the image and on copies dragged out of it. `hdiutil create` does not.
xattr -cr "$STAGE" 2>/dev/null || true
rm -f "$DMG"
hdiutil create -volname "Studio Suite" -srcfolder "$STAGE" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG" >/dev/null
sign "$DMG"
codesign --verify --strict "$DMG"
ls -lh "$DMG"
