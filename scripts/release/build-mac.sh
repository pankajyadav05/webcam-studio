#!/bin/bash
# Builds a release on the build Mac: the signed, notarized and stapled app in a DMG, plus the
# updater archive and its signature. Output: dist/release/<version>/.
#
# Usage: scripts/release/build-mac.sh <version>
# Needs (see scripts/release/release.env): the Developer ID identity and the notarytool
# profile in the login keychain, Xcode for the camera extension, and the updater key file.
set +x
set -euo pipefail

version=${1:?usage: build-mac.sh <version>}
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=release.env
source "$root/scripts/release/release.env"
fail() { echo "::error::$*" >&2; exit 1; }
log() { printf '==> %s\n' "$*"; }

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Not a release version: $version"
security find-identity -v -p codesigning | grep -Fq "\"$SIGNING_IDENTITY\"" ||
  fail "Missing signing identity in the login keychain: $SIGNING_IDENTITY"
if ! notary_check=$(xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" 2>&1); then
  echo "$notary_check" | tail -5 >&2
  fail "Cannot use the notarytool keychain profile '$NOTARY_PROFILE'"
fi
[[ -f "$PROVISIONING_PROFILE" ]] || fail "Missing provisioning profile: $PROVISIONING_PROFILE"
[[ -d "$XCODE_DEVELOPER_DIR" ]] || fail "Missing Xcode: $XCODE_DEVELOPER_DIR"
[[ -r "$UPDATER_KEY" ]] || fail "Missing updater key: $UPDATER_KEY"

out="$root/dist/release/$version"
work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/webcam-studio-release.XXXXXX")
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
cleanup() {
  # Signing and stapling register the staging copies; leave only the installed app.
  find "$work" -maxdepth 3 -name '*.app' -exec "$lsregister" -u {} \; 2>/dev/null || true
  "$lsregister" -u "$root/target/release/bundle/macos/Webcam Studio.app" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT
rm -rf "$out"
mkdir -p "$out"

submit() {
  local target=$1 report="$work/notary.json" status=0
  xcrun notarytool submit "$target" --keychain-profile "$NOTARY_PROFILE" --wait \
    --output-format json > "$report" || status=$?
  # notarytool can exit zero with an Invalid verdict.
  python3 - "$report" "$status" <<'PY'
import json, sys
try:
    result = json.load(open(sys.argv[1]))
except Exception:
    result = {}
if sys.argv[2] != "0" or result.get("status") != "Accepted":
    sys.exit("::error::Notarization failed (status: %s, id: %s). See: xcrun notarytool log %s --keychain-profile notary"
             % (result.get("status", "unknown"), result.get("id", "?"), result.get("id", "<id>")))
print("Notarized:", result.get("id"))
PY
}

log "Camera extension"
DEVELOPER_DIR="$XCODE_DEVELOPER_DIR" DEVELOPMENT_TEAM="$TEAM_ID" \
  "$root/platform/macos/scripts/build-extension.sh"
extension="$root/platform/macos/build/DerivedData/Build/Products/Release/CameraExtension.systemextension"
[[ -d "$extension" ]] || fail "The camera extension did not build"

log "App $version"
(
  cd "$root/apps/desktop/src-tauri"
  CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_OPT_LEVEL=0 \
    CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_DEBUG=true \
    cargo tauri build --bundles app --ci -- --locked
)
built="$root/target/release/bundle/macos/Webcam Studio.app"
built_version=$(plutil -extract CFBundleShortVersionString raw "$built/Contents/Info.plist")
[[ "$built_version" == "$version" ]] || fail "Built $built_version, expected $version (set the Cargo version first)"

log "Sign the app with its extension"
stage="$work/stage"
mkdir -p "$stage"
ditto "$built" "$stage/Webcam Studio.app"
app="$stage/Webcam Studio.app"
SIGNING_IDENTITY="$SIGNING_IDENTITY" TEAM_ID="$TEAM_ID" PROVISIONING_PROFILE="$PROVISIONING_PROFILE" \
  "$root/platform/macos/scripts/bundle-app.sh" "$app" "$extension" "$work/app.zip" >/dev/null

log "Notarize the app"
submit "$work/app.zip"
xcrun stapler staple "$app"
codesign --verify --deep --strict "$app"

log "Disk image"
dmg="$out/Webcam-Studio_${version}_aarch64.dmg"
mkdir -p "$work/dmg"
ditto "$app" "$work/dmg/Webcam Studio.app"
ln -s /Applications "$work/dmg/Applications"
hdiutil create -volname "Webcam Studio" -srcfolder "$work/dmg" -ov -format UDZO "$dmg" >/dev/null
codesign --force --timestamp --sign "$SIGNING_IDENTITY" "$dmg"
submit "$dmg"
xcrun stapler staple "$dmg"

log "Updater archive"
archive="$out/Webcam-Studio_${version}_aarch64.app.tar.gz"
COPYFILE_DISABLE=1 tar -czf "$archive" -C "$stage" "Webcam Studio.app"
# The CLI reads the key from the environment; never pass it in argv or print it.
TAURI_SIGNING_PRIVATE_KEY="$(cat "$UPDATER_KEY")" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  cargo tauri signer sign "$archive" >/dev/null
[[ -s "$archive.sig" ]] || fail "The updater archive was not signed"

log "Check the results"
spctl --assess --type execute "$app" || fail "Gatekeeper rejects the app"
spctl --assess --type open --context context:primary-signature "$dmg" || fail "Gatekeeper rejects the DMG"
xcrun stapler validate "$dmg" >/dev/null
(cd "$out" && shasum -a 256 ./*.dmg ./*.tar.gz > SHA256SUMS)
ls -la "$out"
