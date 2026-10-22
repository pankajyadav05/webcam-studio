# Webcam Studio on macOS

This page explains what the Mac build contains, how to build it, and why the virtual camera does not work from a plain build. Please read the signing section before you start. It saves a lot of time.

## What the Mac build contains

The Mac app is a normal app bundle, `Webcam Studio.app`, with a camera extension inside it.

- **The app** is the Tauri program. It opens your real webcam, applies the effects, and writes the finished frames to the virtual camera.
- **The camera extension** is a small Swift program that macOS runs on its own, as a system extension. It is what Zoom, FaceTime, Safari and other apps see as a camera called "Webcam Studio Camera".

Inside the app, the extension lives here:

```
Webcam Studio.app/Contents/Library/SystemExtensions/dev.pankajyadav.webcamstudio.CameraExtension.systemextension
```

The extension has one device with two streams:

- The **source stream** is what meeting apps read.
- The **sink stream** is where the app writes frames. Each frame the extension receives is passed on to the source stream.

If the app sends no frame for one second, or is not running, the source stream shows a dark picture with an orange dot and the text "Webcam Studio is not running". This way a meeting app never shows a frozen or black camera.

The Swift source is in `platform/macos/CameraExtension/`. It is built with Xcode, not with Cargo.

## Build on your own Mac

You need:

- A Mac with Xcode 16 or newer (the command line tools alone are not enough).
- Homebrew, then `brew install xcodegen`.
- Rust (stable), from <https://rustup.rs>.
- The Tauri command line tool: `cargo install tauri-cli --version "^2" --locked`.

Then, from the top of the repository:

```sh
# 1. Build the camera extension. The last line printed is the path of the bundle.
EXT="$(platform/macos/scripts/build-extension.sh)"

# 2. Build the app.
(cd apps/desktop/src-tauri && cargo tauri build --bundles app)

# 3. Put the extension in the app, sign it, and make a zip.
platform/macos/scripts/bundle-app.sh \
  "target/release/bundle/macos/Webcam Studio.app" \
  "$EXT" \
  Webcam-Studio-macOS.zip
```

Without any settings, step 3 signs both the app and the extension "ad-hoc". That only gives the bundle a valid structure. It does not make the camera work (see the next section).

For a real signature, set these before step 3:

| Variable | Meaning |
|---|---|
| `SIGNING_IDENTITY` | Your "Developer ID Application: ..." certificate name |
| `TEAM_ID` | Your ten character Apple team ID |
| `PROVISIONING_PROFILE` | Path to a Developer ID provisioning profile for the app (needed for the system extension entitlement) |

Also set `DEVELOPMENT_TEAM` to the same team ID before step 1, so the extension's Mach service name gets the team prefix.

Where the team ID goes:

- `DEVELOPMENT_TEAM` (environment variable) for `build-extension.sh`.
- `TEAM_ID` (environment variable) for `bundle-app.sh`.
- Or the commented `DEVELOPMENT_TEAM` line in `platform/macos/project.yml`, if you open the generated project in Xcode.

GitHub Actions builds this same bundle on every push (`.github/workflows/macos.yml`) and uploads it as `Webcam-Studio-macOS-unsigned`. That build is unsigned, so it cannot activate the extension.

## The signing reality

macOS only activates a camera extension for normal users when all of this is true:

1. The extension and the app are signed with an Apple **Developer ID** certificate.
2. The app is **notarized** by Apple.
3. The app has the system extension entitlement (`com.apple.developer.system-extension.install`), which needs a provisioning profile from your Apple developer account.
4. The app is in `/Applications`.

Developer ID certificates and notarization need the paid Apple Developer Program. There is no way around this for a build you give to other people. An unsigned or ad-hoc build, like the one from GitHub Actions, will load and run the app, but the system will refuse to activate the extension.

### Trying it on your own Mac without paying

For development on a Mac you own, you can turn on developer mode for system extensions. This lets an ad-hoc or development-signed extension load.

1. Shut down. Start in Recovery (Apple silicon: hold the power button until the start-up options appear, then choose Options).
2. Open Terminal from the Utilities menu and run `csrutil disable`. Restart.
3. In a normal Terminal, run `systemextensionsctl developer on`.
4. Build with `ADHOC_WITH_ENTITLEMENTS=1` set for `bundle-app.sh`, and copy the app to `/Applications`.

The trade-off: System Integrity Protection is a core security feature of macOS. With it off, any program with administrator rights can change protected parts of the system. Use this only on a development Mac, and turn it back on when you are done:

1. Start in Recovery again.
2. Run `csrutil enable` in Terminal. Restart.

Treat these steps as a guide, not a promise. Apple changes the details between macOS releases, and the GitHub Actions build has not been tested on a Mac with activation turned on.

## Use it from the app

1. Move `Webcam Studio.app` to `/Applications` and open it.
2. In the Virtual camera section, press **Install the camera**. macOS asks you to allow it.
3. Press **Allow it in System Settings**. This opens the page where the extension is approved (see below).
4. Pick your real webcam and press **Start virtual camera**. The first time, macOS asks for camera access.
5. In Zoom, FaceTime or a browser, choose **Webcam Studio Camera**.

While it runs, the panel shows the frame rate and how many frames were dropped. The status at the bottom of the window shows the real state: not installed, needs approval, ready, on, or error. When the app is closed, meeting apps see a placeholder card that says Webcam Studio is not running.

## Approve the extension

The first time the app asks to install the camera, macOS shows a message and you must approve it:

- **macOS 14 and 15:** System Settings, then General, then Login Items & Extensions, then Camera Extensions. Turn on Webcam Studio.
- **macOS 26:** System Settings, then Privacy & Security, then Extensions. The camera is listed as a media extension.

The app must be in `/Applications`, or macOS refuses to activate the extension.

## Check, debug and remove

See whether the extension is registered:

```sh
systemextensionsctl list
```

Watch its log messages while you open the camera in another app:

```sh
log stream --predicate 'subsystem == "dev.pankajyadav.webcamstudio.CameraExtension"'
```

Remove it:

```sh
systemextensionsctl uninstall <TEAMID> dev.pankajyadav.webcamstudio.CameraExtension
```

Or delete the app. macOS removes the extension with it. If you have developer mode on, the uninstall command above is the reliable way.

## The contract between the app and the extension

The Rust app and the Swift extension must agree on these values. If you change one side, change the other.

| Item | Value |
|---|---|
| Host app bundle ID | `dev.pankajyadav.webcamstudio` |
| Product name | Webcam Studio |
| Extension bundle ID | `dev.pankajyadav.webcamstudio.CameraExtension` |
| Extension location | `Webcam Studio.app/Contents/Library/SystemExtensions/dev.pankajyadav.webcamstudio.CameraExtension.systemextension` |
| Device name | Webcam Studio Camera |
| Manufacturer | Webcam Studio |
| Model | Virtual Camera |
| Device UUID | `E0A5D920-6291-412E-80C9-99704E4E5218` |
| Source stream UUID | `83148DC8-A158-4E84-9166-75E50BB46FB1` (name "Webcam Studio Camera") |
| Sink stream UUID | `01C4DA86-551A-49BD-882D-170FABF06336` (name "Webcam Studio Camera Input") |
| Format | 1280x720, 30 fps, 32BGRA pixels, IOSurface-backed pixel buffers |
| Mach service name | `<TEAMID>.dev.pankajyadav.webcamstudio` |
| App group | `<TEAMID>.dev.pankajyadav.webcamstudio` |
| Idle time before the placeholder | 1 second without an app frame |

How the app uses it: the app is a CoreMediaIO client. It finds the device by its UUID, starts the sink stream, and queues sample buffers into the stream's buffer queue. The extension gives each buffer fresh host-time timing and sends it to the source stream. Frames that are not 1280x720 BGRA are dropped.

## Known limits

- Any app may write to the sink stream. A later version should check that the writer is Webcam Studio.
- The extension is built for the architecture of the Mac that builds it (Apple silicon on GitHub Actions).
- The Swift code has only been compiled by GitHub Actions. Places that may need a fix are marked `CHECK:` in `CameraExtensionProvider.swift`.
- The virtual microphone is a separate piece of work and is not part of this build.
