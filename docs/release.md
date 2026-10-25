# Releasing Webcam Studio

Use **GitHub → Actions → Release → Run workflow** on `main`. Enter a version (`x.y.z`), or
leave it empty to release the version in `Cargo.toml`. Notes are optional; without them the
release lists the commits since the last release.

The workflow runs on the owner's Mac (self-hosted runner, labels `self-hosted, macOS,
webcam-studio-mac`), because the signing credentials live there and nowhere else:

| What | Where |
|---|---|
| Developer ID certificate | login keychain (`Developer ID Application: Pankaj Yadav (88LKBXATSW)`) |
| Notarization | notarytool keychain profile `notary` |
| Provisioning profile | `~/Documents/apple dev/CodifyIt.provisionprofile` |
| Xcode (camera extension) | `~/Downloads/Archives/Xcode-beta.app` |
| Updater signing key | `~/.webcam-studio-release/updater.key` (back it up; see below) |
| Cloudflare R2 | the Mac's `npx wrangler login` (account `admin@codifyit.dev`) |

Paths are in `scripts/release/release.env`. Nothing secret is in the repository or in GitHub.

## What a release does

1. Tests: `cargo fmt`, `clippy`, `cargo test`, and the UI tests.
2. `scripts/release/build-mac.sh <version>` builds the camera extension and the app, embeds
   and signs the extension, notarizes and staples the app, makes and notarizes the DMG,
   then makes the updater archive (`.app.tar.gz`) and signs it with the updater key.
3. `scripts/release/publish.sh` uploads to the `webcam-studio` R2 bucket:
   `releases/<version>/…` (never changed afterwards), `Webcam-Studio.dmg` (the newest DMG,
   behind `webcam.codifyit.dev/download/mac`), and last `latest.json`, the updater feed. It
   reads each file back from the public URL and compares the bytes. It refuses a version that
   is not newer than the one already published.
4. Commits the version (if it changed), tags `v<version>` and creates the GitHub release with
   the same files.

## Updates in the app

The signed app reads `https://pub-e39e62d066fa40368708d6b84e4727b9.r2.dev/latest.json` 30
seconds after launch and then every hour (`apps/desktop/src-tauri/src/updater.rs`). It
downloads a newer version in the background and checks its signature against the public key
in `tauri.conf.json`. Settings and the menu bar panel then show **Restart to Update**. The app
never restarts by itself, because a meeting may be using the camera.

**Back up `~/.webcam-studio-release/updater.key`.** Installed apps accept only updates signed
with it. If it is lost, existing users must download the next version by hand.

If a release changes the camera extension (`CURRENT_PROJECT_VERSION` in
`platform/macos/project.yml`), macOS loads the new extension only after a restart, and the
app asks for one. Change the extension rarely.

## The runner

The runner is in `~/actions-runner-webcam-studio`, installed as a launchd agent for the login
user. Check it with `cd ~/actions-runner-webcam-studio && ./svc.sh status`. If Cloudflare
uploads fail with a login error, run `npx wrangler login` on the Mac.
