#!/usr/bin/env bash
# Builds TubeAmp for macOS and packages it as dist/TubeAmp.app, a DMG and a zip.
#
# Default (no environment variables): ad-hoc signing only (`codesign --sign -`). Gatekeeper will
# reject the app, so users open it once with right-click > Open (see docs/INSTALL-macOS.md).
#
# Developer ID path (untested until an Apple Developer ID exists), enabled by environment variables:
#   DEVELOPER_ID="Developer ID Application: Name (TEAMID)"   sign with this identity, hardened
#                                                           runtime and a secure timestamp
#   APPLE_TEAM_ID=TEAMID                                     optional; checked against DEVELOPER_ID
#   NOTARY_PROFILE=<name>                                    notarize and staple the app and the DMG
#                                                           with a `notarytool store-credentials`
#                                                           keychain profile (needs DEVELOPER_ID)
#   ENTITLEMENTS=path/to/file.plist                          optional; added to the signing step
#
# Other environment variables:
#   SKIP_X86=1        build arm64 only (for a quick local check)
#   SKIP_DMG=1        skip the DMG
#
# Requires: rustup (for the apple-darwin targets), cargo, lipo, codesign, plutil, hdiutil, ditto.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP_NAME="TubeAmp"
BIN_NAME="tubeamp"
PKG_NAME="tubeamp"
BUNDLE_ID="app.tubeamp.TubeAmp"
# The tap API (AudioHardwareCreateProcessTap) needs macOS 14.2. Keep the binary's minimum in
# step with LSMinimumSystemVersion below.
MIN_MACOS="14.2"
COPYRIGHT_YEAR="2026"
DIST="$ROOT/dist"
APP="$DIST/$APP_NAME.app"
STAGE="$DIST/.stage"
ICON_SRC="$ROOT/assets/icon/TubeAmp.icns"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

require() {
  command -v "$1" >/dev/null 2>&1 || die "missing tool: $1"
}

for tool in cargo rustup lipo codesign plutil ditto; do require "$tool"; done
if [[ "${SKIP_DMG:-0}" != "1" ]]; then require hdiutil; fi

# ---- signing mode -----------------------------------------------------------------------------
SIGN_MODE="adhoc"
if [[ -n "${DEVELOPER_ID:-}" ]]; then
  SIGN_MODE="developer-id"
  if [[ -n "${APPLE_TEAM_ID:-}" && "$DEVELOPER_ID" != *"($APPLE_TEAM_ID)"* ]]; then
    die "APPLE_TEAM_ID=$APPLE_TEAM_ID does not match DEVELOPER_ID=$DEVELOPER_ID"
  fi
fi
if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  [[ "$SIGN_MODE" == "developer-id" ]] || die "NOTARY_PROFILE needs DEVELOPER_ID (notarization requires a Developer ID signature)"
  require xcrun
fi
log "signing mode: $SIGN_MODE${NOTARY_PROFILE:+ (+ notarization via profile $NOTARY_PROFILE)}"

# ---- version --------------------------------------------------------------------------------
# The first `version = "..."` line of crates/app/Cargo.toml is the package version.
VERSION="$(awk -F'"' '/^version[[:space:]]*=/ {print $2; exit}' "$ROOT/crates/app/Cargo.toml")"
[[ -n "$VERSION" ]] || die "could not read the version from crates/app/Cargo.toml"
log "version $VERSION"

# ---- build ----------------------------------------------------------------------------------
ARM_TARGET="aarch64-apple-darwin"
X86_TARGET="x86_64-apple-darwin"

target_installed() {
  rustup target list --installed 2>/dev/null | grep -qx "$1"
}

ensure_target() {
  if target_installed "$1"; then return 0; fi
  log "rustup target add $1"
  rustup target add "$1"
}

# Returns non-zero on failure instead of exiting: it is called from `if` conditions below, where
# `set -e` does not apply inside the function.
build_target() {
  local triple="$1"
  log "cargo build --release --locked -p tubeamp --target $triple"
  if ! MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS" \
    cargo build --release --locked -p tubeamp --target "$triple"; then
    return 1
  fi
  [[ -x "$ROOT/target/$triple/release/$BIN_NAME" ]]
}

ensure_target "$ARM_TARGET" || die "cannot add $ARM_TARGET (needed offline or with network)"
build_target "$ARM_TARGET" || die "arm64 build failed"
ARM_BIN="$ROOT/target/$ARM_TARGET/release/$BIN_NAME"

X86_BIN=""
if [[ "${SKIP_X86:-0}" == "1" ]]; then
  warn "SKIP_X86=1: building arm64 only"
elif ! ensure_target "$X86_TARGET"; then
  warn "cannot add $X86_TARGET; falling back to an arm64-only app"
elif ! build_target "$X86_TARGET"; then
  warn "x86_64 build failed; falling back to an arm64-only app"
else
  X86_BIN="$ROOT/target/$X86_TARGET/release/$BIN_NAME"
fi

# ---- assemble the bundle --------------------------------------------------------------------
rm -rf "$APP" "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$STAGE"

if [[ -n "$X86_BIN" ]]; then
  log "lipo -create (universal)"
  lipo -create -output "$STAGE/$BIN_NAME" "$ARM_BIN" "$X86_BIN"
else
  cp "$ARM_BIN" "$STAGE/$BIN_NAME"
fi
install -m 755 "$STAGE/$BIN_NAME" "$APP/Contents/MacOS/$BIN_NAME"

[[ -f "$ICON_SRC" ]] || die "missing $ICON_SRC (run: cargo run -p tubeamp-skingen -- icon assets/icon)"
cp "$ICON_SRC" "$APP/Contents/Resources/$APP_NAME.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>$APP_NAME</string>
	<key>CFBundleExecutable</key>
	<string>$BIN_NAME</string>
	<key>CFBundleIconFile</key>
	<string>$APP_NAME</string>
	<key>CFBundleIdentifier</key>
	<string>$BUNDLE_ID</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>$APP_NAME</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$VERSION</string>
	<key>CFBundleVersion</key>
	<string>$VERSION</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.music</string>
	<key>LSMinimumSystemVersion</key>
	<string>$MIN_MACOS</string>
	<key>NSAudioCaptureUsageDescription</key>
	<string>TubeAmp routes YouTube Music's audio through its equalizer and visualizer. Audio never leaves your Mac.</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>Copyright © $COPYRIGHT_YEAR TubeAmp contributors. Not affiliated with YouTube or Google.</string>
</dict>
</plist>
PLIST
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint "$APP/Contents/Info.plist"

# ---- sign -----------------------------------------------------------------------------------
# No nested code is bundled, so one signing pass over the bundle is enough (no --deep).
SIGN_ARGS=(--force --identifier "$BUNDLE_ID")
if [[ -n "${ENTITLEMENTS:-}" ]]; then
  [[ -f "$ENTITLEMENTS" ]] || die "ENTITLEMENTS file not found: $ENTITLEMENTS"
  SIGN_ARGS+=(--entitlements "$ENTITLEMENTS")
fi
if [[ "$SIGN_MODE" == "developer-id" ]]; then
  SIGN_ARGS+=(--sign "$DEVELOPER_ID" --options runtime --timestamp)
else
  # Ad-hoc: no hardened runtime. Gatekeeper ignores it without a Developer ID, and it only adds
  # restrictions (library validation, JIT) that the app does not need.
  SIGN_ARGS+=(--sign -)
fi
log "codesign (${SIGN_MODE})"
codesign "${SIGN_ARGS[@]}" "$APP"

# Notarization: zip the signed app, submit, wait, staple the app. The shipped zip is made later
# from the stapled app.
notarize() {
  local file="$1"
  log "notarytool submit $(basename "$file") (profile $NOTARY_PROFILE)"
  local out
  # `|| true`: notarytool can exit non-zero on an Invalid result; the status check below decides.
  out="$(xcrun notarytool submit "$file" --keychain-profile "$NOTARY_PROFILE" --wait --output-format json || true)"
  printf '%s\n' "$out"
  if ! printf '%s' "$out" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"Accepted"'; then
    die "notarization of $(basename "$file") was not accepted; inspect with: xcrun notarytool log <id> --keychain-profile $NOTARY_PROFILE"
  fi
}

if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  NOTARY_ZIP="$STAGE/$APP_NAME-notarize.zip"
  ditto -c -k --keepParent "$APP" "$NOTARY_ZIP"
  notarize "$NOTARY_ZIP"
  xcrun stapler staple "$APP"
fi

# ---- verify the signature -------------------------------------------------------------------
log "codesign --verify --deep --strict"
codesign --verify --deep --strict --verbose=2 "$APP"
log "codesign -dv (identifier, signature, flags)"
codesign -dv --verbose=2 "$APP" 2>&1 | grep -E '^(Identifier|Format|CodeDirectory|Signature|Authority|TeamIdentifier|Timestamp|Info.plist|Sealed)' || true
if [[ "$SIGN_MODE" == "developer-id" ]]; then
  spctl -a -t exec -vv "$APP" || die "Gatekeeper rejected the Developer ID build"
  if [[ -n "${NOTARY_PROFILE:-}" ]]; then xcrun stapler validate "$APP"; fi
else
  # Expected to be rejected: an ad-hoc signature is not a Developer ID signature.
  log "spctl (informational; ad-hoc builds are expected to be rejected)"
  spctl -a -t exec -vv "$APP" || true
fi

# ---- DMG and zip ----------------------------------------------------------------------------
if [[ "${SKIP_DMG:-0}" != "1" ]]; then
  DMG="$DIST/$APP_NAME-$VERSION-macos.dmg"
  DMG_SRC="$STAGE/dmg"
  rm -rf "$DMG_SRC" "$DMG"
  mkdir -p "$DMG_SRC"
  ditto "$APP" "$DMG_SRC/$APP_NAME.app"
  ln -s /Applications "$DMG_SRC/Applications"
  log "hdiutil create $(basename "$DMG")"
  hdiutil create -quiet -volname "$APP_NAME" -srcfolder "$DMG_SRC" -ov -format UDZO "$DMG"
  if [[ "$SIGN_MODE" == "developer-id" ]]; then
    codesign --force --sign "$DEVELOPER_ID" --timestamp "$DMG"
    if [[ -n "${NOTARY_PROFILE:-}" ]]; then
      notarize "$DMG"
      xcrun stapler staple "$DMG"
    fi
  fi
fi

ZIP="$DIST/$APP_NAME-$VERSION-macos.zip"
rm -f "$ZIP"
log "ditto zip $(basename "$ZIP")"
ditto -c -k --keepParent "$APP" "$ZIP"

rm -rf "$STAGE"

# ---- summary --------------------------------------------------------------------------------
log "artifacts"
lipo -info "$APP/Contents/MacOS/$BIN_NAME"
du -sh "$APP" "$ZIP" ${DMG:+"$DMG"} | sed 's/^/  /'
printf '  %s\n' "$APP" "$ZIP" ${DMG:+"$DMG"}
log "signing: $SIGN_MODE"
