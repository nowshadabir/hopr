#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Hopr"
BIN_NAME="hopr"
PROFILE="${1:-release}"
UNIVERSAL="${HOPR_UNIVERSAL:-0}"
SIGN_IDENTITY="${HOPR_SIGN_IDENTITY:--}"

SPARKLE_VERSION="2.10.1"
SPARKLE_SHA256="1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27"
SPARKLE_URL="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
SPARKLE_DIR=".sparkle/${SPARKLE_VERSION}"

if [[ ! -d "${SPARKLE_DIR}/Sparkle.framework" ]]; then
  mkdir -p "${SPARKLE_DIR}"
  ARCHIVE="${SPARKLE_DIR}/Sparkle-${SPARKLE_VERSION}.tar.xz"
  curl -fsSL -o "$ARCHIVE" "$SPARKLE_URL"
  ACTUAL_SHA=$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')
  if [[ "$ACTUAL_SHA" != "$SPARKLE_SHA256" ]]; then
    echo "Sparkle SHA-256 mismatch: expected ${SPARKLE_SHA256}, got ${ACTUAL_SHA}" >&2
    exit 1
  fi
  tar -xf "$ARCHIVE" -C "${SPARKLE_DIR}"
fi

CARGO_PROFILE_FLAG=""
TARGET_SUBDIR="debug"
if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_FLAG="--release"
  TARGET_SUBDIR="release"
  UNIVERSAL="${HOPR_UNIVERSAL:-1}"
fi

if [[ "$UNIVERSAL" == "1" ]]; then
  cargo build $CARGO_PROFILE_FLAG --target aarch64-apple-darwin
  cargo build $CARGO_PROFILE_FLAG --target x86_64-apple-darwin
  mkdir -p "target/${TARGET_SUBDIR}"
  BIN_PATH="target/${TARGET_SUBDIR}/${BIN_NAME}-universal"
  lipo -create \
    "target/aarch64-apple-darwin/${TARGET_SUBDIR}/${BIN_NAME}" \
    "target/x86_64-apple-darwin/${TARGET_SUBDIR}/${BIN_NAME}" \
    -output "$BIN_PATH"
else
  cargo build $CARGO_PROFILE_FLAG
  BIN_PATH="target/${TARGET_SUBDIR}/${BIN_NAME}"
fi

APP_DIR="target/${TARGET_SUBDIR}/${APP_NAME}.app"
rm -rf "$APP_DIR"
mkdir -p "${APP_DIR}/Contents/MacOS" "${APP_DIR}/Contents/Resources" "${APP_DIR}/Contents/Frameworks"

cp "$BIN_PATH" "${APP_DIR}/Contents/MacOS/${BIN_NAME}"
cp "macos/Info.plist" "${APP_DIR}/Contents/Info.plist"

if [[ -n "${HOPR_VERSION:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${HOPR_VERSION}" "${APP_DIR}/Contents/Info.plist"
fi
if [[ -n "${HOPR_BUILD_NUMBER:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${HOPR_BUILD_NUMBER}" "${APP_DIR}/Contents/Info.plist"
fi
if [[ -n "${HOPR_FEED_URL:-}" ]]; then
  /usr/libexec/PlistBuddy -c "Set :SUFeedURL ${HOPR_FEED_URL}" "${APP_DIR}/Contents/Info.plist"
fi

ICON_SRC=""
TMP_ICON_RENDER=""
if [[ -f "logo.svg" ]] && command -v qlmanage >/dev/null 2>&1; then
  TMP_ICON_RENDER=$(mktemp -d)
  if qlmanage -t -s 1024 -o "$TMP_ICON_RENDER" "logo.svg" >/dev/null 2>&1 && [[ -f "${TMP_ICON_RENDER}/logo.svg.png" ]]; then
    ICON_SRC="${TMP_ICON_RENDER}/logo.svg.png"
  fi
fi
if [[ -z "$ICON_SRC" && -f "logo.png" ]]; then
  ICON_SRC="logo.png"
fi

if [[ -n "$ICON_SRC" ]]; then
  ICONSET_DIR=$(mktemp -d)/AppIcon.iconset
  mkdir -p "$ICONSET_DIR"
  sips -z 16 16     "$ICON_SRC" --out "${ICONSET_DIR}/icon_16x16.png" >/dev/null
  sips -z 32 32     "$ICON_SRC" --out "${ICONSET_DIR}/icon_16x16@2x.png" >/dev/null
  sips -z 32 32     "$ICON_SRC" --out "${ICONSET_DIR}/icon_32x32.png" >/dev/null
  sips -z 64 64     "$ICON_SRC" --out "${ICONSET_DIR}/icon_32x32@2x.png" >/dev/null
  sips -z 128 128   "$ICON_SRC" --out "${ICONSET_DIR}/icon_128x128.png" >/dev/null
  sips -z 256 256   "$ICON_SRC" --out "${ICONSET_DIR}/icon_128x128@2x.png" >/dev/null
  sips -z 256 256   "$ICON_SRC" --out "${ICONSET_DIR}/icon_256x256.png" >/dev/null
  sips -z 512 512   "$ICON_SRC" --out "${ICONSET_DIR}/icon_256x256@2x.png" >/dev/null
  sips -z 512 512   "$ICON_SRC" --out "${ICONSET_DIR}/icon_512x512.png" >/dev/null
  sips -z 1024 1024 "$ICON_SRC" --out "${ICONSET_DIR}/icon_512x512@2x.png" >/dev/null
  iconutil -c icns "$ICONSET_DIR" -o "${APP_DIR}/Contents/Resources/AppIcon.icns"
fi
rm -rf "$TMP_ICON_RENDER"

# Embed Sparkle.framework (preserving symlinks)
ditto "${SPARKLE_DIR}/Sparkle.framework" "${APP_DIR}/Contents/Frameworks/Sparkle.framework"

# Inside-out code signing (never --deep)
SIGN_FLAGS=(--force --options runtime --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
  SIGN_FLAGS+=(--timestamp)
fi

FW_B="${APP_DIR}/Contents/Frameworks/Sparkle.framework/Versions/B"
codesign "${SIGN_FLAGS[@]}" "${FW_B}/XPCServices/Downloader.xpc"
codesign "${SIGN_FLAGS[@]}" "${FW_B}/XPCServices/Installer.xpc"
codesign "${SIGN_FLAGS[@]}" "${FW_B}/Autoupdate"
codesign "${SIGN_FLAGS[@]}" "${FW_B}/Updater.app"
codesign "${SIGN_FLAGS[@]}" "${APP_DIR}/Contents/Frameworks/Sparkle.framework"
codesign "${SIGN_FLAGS[@]}" --entitlements "macos/Hopr.entitlements" "$APP_DIR"

codesign --verify --deep --strict --verbose=2 "$APP_DIR"
echo "Built and verified ${APP_DIR}"
