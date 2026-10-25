#!/bin/bash
# Publishes a built release to the R2 bucket that the app and the website read:
#   releases/<version>/...       the DMG and the updater archive (never change once written)
#   Webcam-Studio.dmg            the newest DMG, behind webcam.codifyit.dev/download/mac
#   latest.json                  the updater feed, written last
# Then reads everything back from the public URL and compares the bytes.
#
# Usage: scripts/release/publish.sh <version> <notes-file>
# Uses the build Mac's wrangler login; no Cloudflare keys are stored anywhere else.
set +x
set -euo pipefail

version=${1:?usage: publish.sh <version> <notes-file>}
notes=${2:?Missing release notes file}
root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck source=release.env
source "$root/scripts/release/release.env"
fail() { echo "::error::$*" >&2; exit 1; }
out="$root/dist/release/$version"
dmg="Webcam-Studio_${version}_aarch64.dmg"
archive="Webcam-Studio_${version}_aarch64.app.tar.gz"
for f in "$dmg" "$archive" "$archive.sig"; do [[ -s "$out/$f" ]] || fail "Missing $out/$f"; done

wrangler() { npx --yes wrangler@4 "$@"; }
put() { # key file content-type cache-control
  wrangler r2 object put "$R2_BUCKET/$1" --remote --file "$2" --content-type "$3" --cache-control "$4" >/dev/null
  echo "Uploaded $1"
}

# A version is published once. Refuse to go backwards or to overwrite.
live=$(curl -fsS "$R2_PUBLIC_BASE_URL/latest.json?nocache=$RANDOM" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])' 2>/dev/null || true)
if [[ -n "$live" ]]; then
  node -e 'import("./scripts/release/feed.mjs").then(m => process.exit(m.isNewer(process.argv[1], process.argv[2]) ? 0 : 1))' "$version" "$live" ||
    fail "Version $version is not newer than the published $live"
fi

node "$root/scripts/release/feed.mjs" "$version" "$R2_PUBLIC_BASE_URL" "$out/$archive.sig" "$notes" > "$out/latest.json"

immutable="public, max-age=31536000, immutable"
put "releases/$version/$dmg" "$out/$dmg" application/x-apple-diskimage "$immutable"
put "releases/$version/$archive" "$out/$archive" application/gzip "$immutable"
put "releases/$version/$archive.sig" "$out/$archive.sig" text/plain "$immutable"
put "releases/$version/SHA256SUMS" "$out/SHA256SUMS" text/plain "$immutable"
put "Webcam-Studio.dmg" "$out/$dmg" application/x-apple-diskimage "no-cache"
put "latest.json" "$out/latest.json" application/json "no-cache"

check() { # key local-file
  local got
  got=$(curl -fsSL "$R2_PUBLIC_BASE_URL/$1?nocache=$RANDOM" | shasum -a 256 | cut -d' ' -f1)
  [[ "$got" == "$(shasum -a 256 < "$2" | cut -d' ' -f1)" ]] || fail "Public $1 does not match the build"
  echo "Verified $1"
}
check "releases/$version/$dmg" "$out/$dmg"
check "releases/$version/$archive" "$out/$archive"
check "Webcam-Studio.dmg" "$out/$dmg"
check "latest.json" "$out/latest.json"
echo "Published Webcam Studio $version to $R2_PUBLIC_BASE_URL"
