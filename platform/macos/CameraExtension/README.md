# macOS Camera Extension

This folder holds the macOS virtual camera for Webcam Studio. It is a CoreMediaIO
Camera Extension: a small system extension that shows up as a camera named "Webcam
Studio" in Zoom, Teams, Safari, FaceTime and other apps. The Rust app does the video
work and hands finished frames to the extension.

The Rust side of this feature lives in `crates/vdevice-macos`. It does not talk to the
extension yet.

## Status

Skeleton only. The Swift files compile as a starting point modelled on Apple's Camera
Extension template, but there is no Xcode project yet and **this target is not built in
CI**. Anything marked `TODO` in the sources is either unfinished work or an API detail
that has to be checked against Apple's documentation before use.

## Why Swift and Xcode

macOS only loads a virtual camera from a system extension bundle (`.systemextension`)
that uses the `CMIOExtension` API. That API is Swift/Objective-C only, and the bundle
needs an Info.plist, entitlements, code signing and embedding inside the app bundle.
Xcode is the supported way to build all of that, so this target stays outside the Cargo
workspace. Rust cannot produce this bundle on its own.

## How frames get from the app to the camera

The extension exposes two streams on one device:

- A **source stream** that other apps read from. This is the camera they see.
- A **sink stream** that the Webcam Studio app writes to. Frames are passed as
  IOSurface-backed pixel buffers, so no pixel copy is needed between the two processes.

Every frame the app writes to the sink stream is forwarded to the source stream.

## Build and signing plan

1. Create an Xcode project with a macOS app target (a small host that installs the
   extension) and a Camera Extension target using the files in this folder.
2. Embed the extension in the app at
   `Contents/Library/SystemExtensions/`.
3. Sign with a Developer ID Application certificate and the hardened runtime, then
   notarize the app with Apple.
4. The host app asks macOS to activate the extension with an `OSSystemExtensionRequest`.
   The user must approve it in System Settings. Where that setting lives has moved
   between macOS versions, so the first-run screen needs to explain it.
5. The app must be installed in `/Applications`, or macOS will refuse to activate the
   extension.

Needs a paid Apple Developer account. Requires macOS 12.3 or later.

## Files

| File | Purpose |
|---|---|
| `CameraExtensionProvider.swift` | Provider, device, source stream and sink stream |
| `main.swift` | Starts the provider service |
| `Info.plist` | Extension bundle settings, including the Mach service name |
| `CameraExtension.entitlements` | App sandbox and app group |

The bundle identifier, team ID and app group name in these files are placeholders.
