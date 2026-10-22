#!/usr/bin/env bash
# Builds the camera extension (the .systemextension bundle).
#
# Needs a Mac with Xcode 15 or newer and XcodeGen (brew install xcodegen).
# Prints the path of the built bundle on the last line of standard output. Everything
# else goes to standard error, so you can write:  EXT="$(./build-extension.sh)"
#
# The bundle is built unsigned on purpose. bundle-app.sh signs it after it has been copied
# into the app, so it is signed once, in its final place.
#
# Optional environment variables:
#   DEVELOPMENT_TEAM  Apple Developer team ID. Fills in the team prefix of the Mach
#                     service name in the extension's Info.plist.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
macos_dir="$(cd "${here}/.." && pwd)"
derived="${macos_dir}/build/DerivedData"
team="${DEVELOPMENT_TEAM:-}"

log() { printf '%s\n' "$*" >&2; }

command -v xcodegen >/dev/null 2>&1 || { log "error: xcodegen not found. Install it with: brew install xcodegen"; exit 1; }
command -v xcodebuild >/dev/null 2>&1 || { log "error: xcodebuild not found. Install Xcode 15 or newer."; exit 1; }

cd "${macos_dir}"

log "==> Generating the Xcode project"
xcodegen generate --spec project.yml >&2

log "==> Building CameraExtension (Release)"
xcode_args=(
  -project WebcamStudio.xcodeproj
  -scheme CameraExtension
  -configuration Release
  -derivedDataPath "${derived}"
  CODE_SIGNING_ALLOWED=NO
)
if [[ -n "${team}" ]]; then
  xcode_args+=("DEVELOPMENT_TEAM=${team}")
fi
xcodebuild "${xcode_args[@]}" build >&2

ext="${derived}/Build/Products/Release/CameraExtension.systemextension"
if [[ ! -d "${ext}" ]]; then
  log "error: expected the built extension at ${ext}"
  exit 1
fi

# Make sure the Mach service name has the team prefix, whatever Xcode did with it.
if [[ -n "${team}" ]]; then
  log "==> Setting the Mach service name for team ${team}"
  /usr/libexec/PlistBuddy \
    -c "Set :CMIOExtension:CMIOExtensionMachServiceName ${team}.dev.pankajyadav.webcamstudio" \
    "${ext}/Contents/Info.plist"
else
  log "note: DEVELOPMENT_TEAM is not set. The bundle is unsigned and has no team prefix,"
  log "      so macOS will not activate it (see docs/macos.md)."
fi

log "==> Built ${ext}"
printf '%s\n' "${ext}"
