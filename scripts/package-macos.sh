#!/usr/bin/env bash
#
# Build OpenLogi.app (icon + bundled device assets) and wrap it in a DMG.
#
# Unsigned by default — Gatekeeper will warn, and macOS Accessibility grants
# reset on each rebuild. For a real release set OPENLOGI_SIGN_IDENTITY to a
# "Developer ID Application: …" identity (and notarize separately); signing
# with a stable identity is what keeps the Accessibility grant sticky across
# updates.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Force the Xcode toolchain (Metal shader compiler + libSystem). Without this
# devenv's Nix apple-sdk hook can shadow them and the GPUI build fails.
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
export SDKROOT="${SDKROOT:-$(/usr/bin/xcrun --sdk macosx --show-sdk-path)}"

echo "==> app icon"
"$ROOT/scripts/macos-icns.sh"

# Device renders are fetched on demand at first launch by default (lean DMG;
# the app pulls only the connected device's render from assets.openlogi.org
# into its per-user cache). Set OPENLOGI_BUNDLE_ASSETS=1 for an offline build
# that bakes every device's render into the .app instead (larger, fully
# offline). cargo-bundle's `resources = ["assets/**/*"]` glob bundles whatever
# is in the crate's assets/ dir at bundle time — so we populate it or empty it.
if [ "${OPENLOGI_BUNDLE_ASSETS:-0}" = "1" ]; then
  echo "==> device assets: bundling (offline build)"
  cargo run -p openlogi-cli --release -- assets sync
else
  echo "==> device assets: on-demand (not bundled; fetched at first launch)"
  rm -rf "$ROOT/crates/openlogi-gui/assets"
  mkdir -p "$ROOT/crates/openlogi-gui/assets"
fi

echo "==> bundle (.app)"
command -v cargo-bundle >/dev/null 2>&1 || cargo install cargo-bundle --locked
# cargo-bundle 0.10 resolves `resources` / `icon` globs against the process
# cwd, so run it from the crate directory (matches the icon path
# "icon/AppIcon.icns" and the "assets/**/*" resources glob).
( cd crates/openlogi-gui && cargo bundle --release )
APP="$ROOT/target/release/bundle/osx/OpenLogi.app"
[ -d "$APP" ] || { echo "error: bundle not found at $APP" >&2; exit 1; }

if [ -n "${OPENLOGI_SIGN_IDENTITY:-}" ]; then
  echo "==> codesign ($OPENLOGI_SIGN_IDENTITY)"
  codesign --force --deep --options runtime --timestamp \
           --sign "$OPENLOGI_SIGN_IDENTITY" "$APP"
  codesign --verify --deep --strict "$APP"
else
  echo "==> codesign: skipped (unsigned — set OPENLOGI_SIGN_IDENTITY to sign)"
fi

echo "==> dmg"
stage="$(mktemp -d)/OpenLogi"
mkdir -p "$stage"
cp -R "$APP" "$stage/"
ln -s /Applications "$stage/Applications"
DMG="$ROOT/target/release/OpenLogi.dmg"
rm -f "$DMG"
hdiutil create -volname "OpenLogi" -srcfolder "$stage" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$(dirname "$stage")"

echo
echo "done → $DMG"
