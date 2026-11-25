#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Hopr"
BIN_NAME="hopr"
PROFILE="${1:-release}"

if [[ "$PROFILE" == "release" ]]; then
  cargo build --release
  BIN_PATH="target/release/${BIN_NAME}"
else
  cargo build
  BIN_PATH="target/debug/${BIN_NAME}"
fi

APP_DIR="target/${PROFILE}/${APP_NAME}.app"
rm -rf "$APP_DIR"
mkdir -p "${APP_DIR}/Contents/MacOS" "${APP_DIR}/Contents/Resources"

cp "$BIN_PATH" "${APP_DIR}/Contents/MacOS/${BIN_NAME}"
cp "macos/Info.plist" "${APP_DIR}/Contents/Info.plist"

if [[ -f "logo.png" ]]; then
  ICONSET_DIR=$(mktemp -d)/AppIcon.iconset
  mkdir -p "$ICONSET_DIR"
  sips -z 16 16     logo.png --out "${ICONSET_DIR}/icon_16x16.png" >/dev/null
  sips -z 32 32     logo.png --out "${ICONSET_DIR}/icon_16x16@2x.png" >/dev/null
  sips -z 32 32     logo.png --out "${ICONSET_DIR}/icon_32x32.png" >/dev/null
  sips -z 64 64     logo.png --out "${ICONSET_DIR}/icon_32x32@2x.png" >/dev/null
  sips -z 128 128   logo.png --out "${ICONSET_DIR}/icon_128x128.png" >/dev/null
  sips -z 256 256   logo.png --out "${ICONSET_DIR}/icon_128x128@2x.png" >/dev/null
  sips -z 256 256   logo.png --out "${ICONSET_DIR}/icon_256x256.png" >/dev/null
  sips -z 512 512   logo.png --out "${ICONSET_DIR}/icon_256x256@2x.png" >/dev/null
  sips -z 512 512   logo.png --out "${ICONSET_DIR}/icon_512x512.png" >/dev/null
  sips -z 1024 1024 logo.png --out "${ICONSET_DIR}/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$ICONSET_DIR" -o "${APP_DIR}/Contents/Resources/AppIcon.icns"
fi

# Sign with Hardened Runtime + network.client entitlement (ad-hoc for local builds)
codesign --force --deep --options runtime --entitlements "macos/Hopr.entitlements" --sign - "$APP_DIR"

echo "Built ${APP_DIR}"
