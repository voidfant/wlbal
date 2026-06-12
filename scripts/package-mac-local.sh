#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_PATH="$ROOT_DIR/src-tauri/target/release/bundle/macos/wlbal.app"
OUT_DIR="$ROOT_DIR/src-tauri/target/release/bundle/local"
DMG_PATH="$OUT_DIR/wlbal-local-unsigned.dmg"

if [[ ! -d "$APP_PATH" ]]; then
  echo "Missing app bundle: $APP_PATH" >&2
  echo "Run npm run tauri build first." >&2
  exit 1
fi

mkdir -p "$OUT_DIR"

codesign --force --deep --sign - "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

rm -f "$DMG_PATH"
hdiutil create \
  -volname "wlbal" \
  -srcfolder "$APP_PATH" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

echo "Created $DMG_PATH"
echo "This DMG is ad-hoc signed, not notarized. Gatekeeper may still block it on other Macs."
