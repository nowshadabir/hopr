#!/usr/bin/env bash
set -euo pipefail

APP_DIR="${1:?Usage: package-dmg.sh <path/to/Hopr.app> <version> [output_dir]}"
VERSION="${2:?Usage: package-dmg.sh <path/to/Hopr.app> <version> [output_dir]}"
OUT_DIR="${3:-$(dirname "$APP_DIR")}"
SIGN_IDENTITY="${HOPR_SIGN_IDENTITY:--}"

DMG_NAME="Hopr-${VERSION}.dmg"
DMG_PATH="${OUT_DIR}/${DMG_NAME}"
STAGE_DIR=$(mktemp -d)
trap 'rm -rf "$STAGE_DIR"' EXIT

ditto "$APP_DIR" "${STAGE_DIR}/Hopr.app"
ln -s /Applications "${STAGE_DIR}/Applications"

rm -f "$DMG_PATH"
hdiutil create \
  -volname "Hopr ${VERSION}" \
  -srcfolder "$STAGE_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

if [[ "$SIGN_IDENTITY" != "-" ]]; then
  codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"
else
  codesign --force --sign - "$DMG_PATH"
fi

codesign --verify --verbose=2 "$DMG_PATH"
echo "Created ${DMG_PATH}"
