# Webcam Studio

Webcam Studio is a desktop app for macOS, Windows and Linux. It sits between your webcam and your meeting app. It cleans up the picture and the sound, then offers the result as a virtual camera and a virtual microphone.

It is free software under the GPL-3.0-or-later licence.

## The gap it fills

Built-in effects on macOS and Windows mostly do not cover external webcams:

- On macOS, Portrait mode and Center Stage work with the built-in camera, some Apple displays, and an iPhone used as a webcam.
- On Windows, Studio Effects needs a Copilot+ PC with an NPU.
- NVIDIA Broadcast works with any webcam, but it needs Windows and an RTX graphics card.

If you use a USB webcam on a Mac, or a PC without an NPU or RTX card, you get no background blur, no framing and no digital zoom. Webcam Studio aims to cover that case on all three systems. The details are in [docs/research.md](docs/research.md).

## Planned features

- Digital zoom, pan and mirror.
- Auto-framing that follows you.
- Background blur or replacement with an image.
- Microphone noise removal.
- A virtual camera and a virtual microphone that any meeting app can pick.

## Status

This is an early skeleton. Nothing works end to end yet.

The desktop window edits the settings and shows a live preview with zoom, pan and mirror applied. The video engine, the segmentation model, the audio processing and the virtual devices are placeholders or only partly written.

## Platforms

How each part is expected to work, from section 4 of the research notes. These are plans, not features that exist today.

| | macOS 14+ (Apple silicon) | Windows 11 | Linux (PipeWire) |
|---|---|---|---|
| Virtual camera | CoreMediaIO Camera Extension (needs Developer ID signing and approval in System Settings) | Media Foundation Virtual Camera API | PipeWire camera node, plus optional v4l2loopback |
| Virtual microphone | Core Audio driver (AudioServerPlugIn) | Hardest case. Needs a signed kernel driver, or processing on the real microphone instead | PipeWire filter-chain with a noise-removal plugin |
| Inference backend | ONNX Runtime with CoreML, CPU as fallback | ONNX Runtime with DirectML, CPU as fallback | ONNX Runtime with CUDA, TensorRT or OpenVINO when present, CPU as fallback |

## Repository layout

- `apps/desktop/` is the Tauri v2 desktop app. The Rust side is in `src-tauri/`. The user interface is plain HTML, CSS and JavaScript in `ui/`. There is no npm step.
- `crates/core/` has the shared types, such as settings, frames and the virtual device trait.
- `crates/engine/` runs the video and audio processing.
- `crates/segmentation/` separates the person from the background.
- `crates/audio/` removes noise from the microphone.
- `crates/vdevice/` picks the virtual device backend for the current system.
- `crates/vdevice-linux/`, `crates/vdevice-windows/` and `crates/vdevice-macos/` hold the backend for each system.
- `docs/research.md` has the research behind these choices.

## Build and run

You need a recent stable Rust toolchain.

Run the tests for the core crates:

```sh
cargo test
```

The desktop app is left out of the default build because it needs the system web view. To test everything, including the app:

```sh
cargo test --workspace
```

On Linux, install these packages first (Debian and Ubuntu names):

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev libssl-dev pkg-config
```

Then start the app:

```sh
cargo run -p webcam-studio-desktop
```

The macOS Camera Extension is a separate Xcode target. It is not built by Cargo or by CI yet.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).

Eye contact uses the pretrained eye-warping model from
[Look at Me! Correcting Eye Gaze in Live Video Communication](https://github.com/chihfanhsu/gaze_correction)
(Hsu, Wang, Lei and Chen, 2019), under the BSD-3-Clause licence. Its notice is in
[crates/engine/assets/LICENSE-gaze-warp.txt](crates/engine/assets/LICENSE-gaze-warp.txt) and inside the
app at `Contents/Resources/Licenses/gaze-warp.txt`.
