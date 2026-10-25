#!/usr/bin/env bash
# Puts the camera extension inside the Webcam Studio app, signs both and zips the app.
#
# Usage:
#   bundle-app.sh "path/to/Webcam Studio.app" path/to/CameraExtension.systemextension [out.zip]
#
# The app comes from `cargo tauri build --bundles app`, the extension from
# build-extension.sh. The zip defaults to Webcam-Studio-macOS.zip next to the app.
#
# Environment variables:
#   SIGNING_IDENTITY       Developer ID Application identity, for example
#                          "Developer ID Application: Your Name (ABCDE12345)". When set, the
#                          extension and the app are signed for real (hardened runtime,
#                          timestamp, entitlements). When empty, both are ad-hoc signed.
#   TEAM_ID                Your team ID (ABCDE12345). Needed with SIGNING_IDENTITY. Falls back
#                          to DEVELOPMENT_TEAM.
#   PROVISIONING_PROFILE   Optional path to a Developer ID provisioning profile for the app.
#                          It is copied to Contents/embedded.provisionprofile. The
#                          system-extension.install entitlement needs one.
#   ADHOC_WITH_ENTITLEMENTS=1
#                          For ad-hoc signing only: also attach the entitlements. This is
#                          for a Mac with SIP off and `systemextensionsctl developer on`.
#                          Without it the app is signed bare so that it still launches.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "${here}/../../.." && pwd)"
app_entitlements="${repo}/apps/desktop/src-tauri/macos/WebcamStudio.entitlements"
ext_entitlements="${repo}/platform/macos/CameraExtension/CameraExtension.entitlements"
ext_id="dev.pankajyadav.webcamstudio.CameraExtension"

log() { printf '%s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

[[ $# -ge 2 ]] || die "usage: $0 <Webcam Studio.app> <CameraExtension.systemextension> [out.zip]"
app="${1%/}"
ext="${2%/}"
[[ -d "${app}" ]] || die "app not found: ${app}"
[[ -d "${ext}" ]] || die "extension not found: ${ext}"
out_zip="${3:-$(dirname "${app}")/Webcam-Studio-macOS.zip}"

identity="${SIGNING_IDENTITY:-}"
team="${TEAM_ID:-${DEVELOPMENT_TEAM:-}}"
profile="${PROVISIONING_PROFILE:-}"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

# 1. Copy the extension into the app.
dest_dir="${app}/Contents/Library/SystemExtensions"
dest="${dest_dir}/${ext_id}.systemextension"
log "==> Copying the extension to ${dest}"
mkdir -p "${dest_dir}"
rm -rf "${dest}"
ditto "${ext}" "${dest}"

# 2. Sign.
if [[ -n "${identity}" ]]; then
  [[ -n "${team}" ]] || die "SIGNING_IDENTITY is set, so TEAM_ID (or DEVELOPMENT_TEAM) is needed too"

  # The extension entitlements say $(TeamIdentifierPrefix), which only Xcode fills in.
  # Do it here with the real team ID.
  ext_signed_entitlements="${tmp}/CameraExtension.entitlements"
  sed "s/\\\$(TeamIdentifierPrefix)/${team}./g" "${ext_entitlements}" > "${ext_signed_entitlements}"
  plutil -lint "${ext_signed_entitlements}" >&2

  if [[ -n "${profile}" ]]; then
    [[ -f "${profile}" ]] || die "provisioning profile not found: ${profile}"
    log "==> Embedding the provisioning profile"
    cp "${profile}" "${app}/Contents/embedded.provisionprofile"
  else
    log "warning: no PROVISIONING_PROFILE. macOS may refuse the system-extension.install entitlement."
  fi

  log "==> Signing the extension"
  codesign --force --options runtime --timestamp \
    --entitlements "${ext_signed_entitlements}" \
    --sign "${identity}" "${dest}"

  # Anything else that ships inside the app is signed before the app itself.
  if [[ -d "${app}/Contents/Frameworks" ]]; then
    while IFS= read -r -d '' item; do
      log "==> Signing ${item#"${app}"/}"
      codesign --force --options runtime --timestamp --sign "${identity}" "${item}"
    done < <(find "${app}/Contents/Frameworks" -mindepth 1 -maxdepth 1 -print0)
  fi

  # The profile only vouches for the restricted entitlement when the signature names the
  # same app and team, so add those two identifiers to the app's entitlements.
  app_signed_entitlements="${tmp}/WebcamStudio.entitlements"
  cp "${app_entitlements}" "${app_signed_entitlements}"
  app_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "${app}/Contents/Info.plist")"
  /usr/libexec/PlistBuddy -c "Add :com.apple.application-identifier string ${team}.${app_id}" "${app_signed_entitlements}" >&2
  /usr/libexec/PlistBuddy -c "Add :com.apple.developer.team-identifier string ${team}" "${app_signed_entitlements}" >&2

  log "==> Signing the app"
  codesign --force --options runtime --timestamp \
    --entitlements "${app_signed_entitlements}" \
    --sign "${identity}" "${app}"
else
  log "==> No SIGNING_IDENTITY: ad-hoc signing for structure only."
  log "    macOS will not activate the extension from this build."
  if [[ "${ADHOC_WITH_ENTITLEMENTS:-}" == "1" ]]; then
    log "    ADHOC_WITH_ENTITLEMENTS=1: attaching the entitlements (developer mode Macs only)."
    # No team: the app group is just the bare name.
    ext_adhoc_entitlements="${tmp}/CameraExtension.entitlements"
    sed "s/\\\$(TeamIdentifierPrefix)//g" "${ext_entitlements}" > "${ext_adhoc_entitlements}"
    codesign --force --sign - --entitlements "${ext_adhoc_entitlements}" "${dest}"
    codesign --force --sign - --entitlements "${app_entitlements}" "${app}"
  else
    codesign --force --sign - "${dest}"
    codesign --force --sign - "${app}"
  fi
fi

# 3. Check the signature is consistent, then zip. ditto keeps the bundle intact.
log "==> Verifying signatures"
codesign --verify --strict --verbose=2 "${dest}" >&2
codesign --verify --strict --verbose=2 "${app}" >&2

log "==> Writing ${out_zip}"
rm -f "${out_zip}"
ditto -c -k --keepParent "${app}" "${out_zip}"
log "==> Done"
printf '%s\n' "${out_zip}"
