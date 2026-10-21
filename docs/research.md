# Cross-platform webcam and mic studio app — research (21 Oct 2026)

Goal: an NVIDIA Broadcast-style app for macOS, Windows and Linux that works with **any** webcam
(especially external USB cameras): noise suppression, background blur/replace, digital zoom and
pan, frame set-up (crop, mirror, auto-frame), exposed to every meeting app as a virtual camera and
virtual microphone.

Everything below is from public sources read on 21 Oct 2026. Where a source did not say
something, this says "not confirmed".

---

## 1. The gap is real

| Platform feature | What it covers | What it does **not** cover |
|---|---|---|
| macOS Portrait / Studio Light | Apple-silicon Macs, or an iPhone via Continuity Camera | Apple's page lists no third-party USB webcam. Not confirmed either way; in practice these effects apply to the built-in camera and Continuity Camera only. |
| macOS Center Stage (auto-framing) | Studio Display, Macs with the 12 MP camera, iPhone via Continuity Camera | External webcams. |
| Windows Studio Effects (blur, eye contact, auto-framing, voice focus) | Copilot+ PCs with an NPU; some looks Snapdragon-only | Older PCs without an NPU; external webcam support not stated. |
| NVIDIA Broadcast | Windows + RTX GPU only | macOS, Linux, non-NVIDIA machines. |
| NVIDIA Maxine SDK on Linux | Datacenter GPUs via NVIDIA AI Enterprise / NGC | Consumer RTX on Linux; the public GitHub repos were taken down in 2025. |

So on a Mac with an external webcam, or any PC without an NPU or RTX card, there is no built-in
blur, framing, or digital zoom. That is the gap.

---

## 2. Apps that already exist

| App | Platforms | Camera effects | Audio | Any webcam? | Model / price | Notes |
|---|---|---|---|---|---|---|
| **Camo Studio** (Reincubate) | macOS 12.3+, Windows 10+ | Blur, replace, Spotlight, zoom/pan/crop, auto-framing (per 2023 press), up to 4K | Not a focus | Yes, webcams, built-in, DSLR, phones (since Camo 2, 2023) | Free tier (720p); Pro monthly/annual/lifetime; last known pricing 2023: $4.99/mo, $39.99/yr, $79.99 lifetime | Closest commercial match. No Linux. |
| **XSplit VCam** | Windows, macOS | Blur, remove, replace | No | Yes | Freemium; lifetime deals ~$29 | Mac requirements listed are old (Big Sur). No Linux. No zoom. |
| **Krisp** | Windows, macOS (Apple silicon, macOS 12+); Linux listed by Discord's page, other sources disagree | Virtual background was Mac-only beta in 2022; current status not confirmed | Best-in-class noise cancellation, virtual mic | n/a | Subscription | Audio first; camera side unclear. |
| **OBS Studio + Background Removal plugin** (occ-ai) | Windows, macOS, Linux | Blur/remove/replace, low-light enhance; crop/zoom via OBS transforms | Filters via plugins | Yes | Free, GPL-3.0 | Works, but it is a streaming studio, not a one-click utility. Virtual camera on all three OSes. |
| **NV Broadcast** (nvidia-broadcast-linux, open source) | Linux; macOS 15+ Apple silicon (CPU only, camera via OBS) | Blur/replace/remove, eye contact, relight, auto-framing, enhancement | Noise removal, voice EQ (Linux); no virtual mic on macOS | Yes | Open source, donations | Python 3.11 + GTK4 + GStreamer + ONNX Runtime; v4l2loopback + PipeWire. No Windows. Shows the whole pipeline can be built from open parts. |
| **Webcamoid** | Windows, macOS, Linux | 60+ effects, face tracking, per-camera controls; no AI background removal | No | Yes | Free, open source, C++/Qt | Its virtual-camera drivers (akvcam on Linux; Mac/Windows split into its own project in 9.0) are reusable. |
| **SplitCam**, ManyCam, CyberLink YouCam | Windows (+ Mac for some) | Background removal, scenes | Some | Yes | Commercial | Streamer-oriented, no Linux. |
| Vendor apps (Logi Tune / G HUB, OBSBOT Center, Insta360 Link) | Win/Mac | Auto-framing, zoom for **their own** cameras | Some | No | Free with hardware | Not a fit for "any webcam". |

**Takeaway:** nobody ships one utility across all three desktop OSes that does mic + background +
zoom/frame for any webcam. Camo Studio is the nearest on Mac/Windows; Linux is served only by
OBS-style tooling and new open-source projects.

---

## 3. Open-source models

### Background segmentation / matting (video, real time)

| Model | License | Size / speed | Fit |
|---|---|---|---|
| **MediaPipe Selfie Segmentation** (square 256×256, landscape 144×256) | Apache-2.0 code; model card to check | ~33 ms on a Pixel 6 CPU, so a few ms on a desktop | Best default: tiny, fast, binary person/background, runs via TFLite or converted to ONNX. |
| **MediaPipe Multi-class Selfie** (hair, skin, clothes) | same | 70–200 ms on phone | Only if you need hair/clothes masks. |
| **Robust Video Matting (RVM)** | **GPL-3.0** | Recurrent, temporally stable alpha matte, 1080p real time on a GPU | Best quality edges (hair). GPL forces open-sourcing the app if you link it, unless you keep it as a separate process. |
| **MODNet** | Apache-2.0 (verify) | Real-time portrait matting | Good middle ground; older. |
| **PP-HumanSeg, SINet** | Apache-2.0 | Lightweight | Used by the OBS plugin; fine fallbacks. |
| **BiRefNet** | MIT (verify) | ~0.2 B params, 1024×1024 | Image-grade quality, too heavy for 30 fps on CPU; usable on a GPU at low res. |
| **RMBG-2.0** (Bria) | **CC BY-NC 4.0, non-commercial** | BiRefNet-based | Not usable in a product without a Bria licence. |
| SAM 2 / SAM 3 | Apache-2.0 / custom Meta | Huge | Not real-time portrait tooling. |

Recommendation: MediaPipe selfie segmentation as the default (runs everywhere, including CPU-only
Linux boxes), with an optional "high quality" mode using RVM or MODNet when a GPU/NPU is present.
Temporal smoothing and guided-filter edge refinement matter more than the model choice for
perceived quality.

### Noise suppression

| Model | License | Latency / compute | Fit |
|---|---|---|---|
| **RNNoise** (Xiph) | BSD-3 | ~10 ms, trivial CPU | Lightest. The `noise-suppression-for-voice` plugin (GPL-3.0) wraps it as LADSPA/LV2/VST/AU on Linux, Windows, macOS. |
| **DeepFilterNet 3** | Apache-2.0 **or** MIT (dual) | 20 ms minimum plus host latency; still real time on CPU | Better quality than RNNoise in a 2025 peer-reviewed comparison. Rust core, LADSPA plugin included. Best default. |
| Silero / DTLN / Demucs denoiser | various | heavier | Not needed for v1. |

Recommendation: DeepFilterNet 3 (permissive licence, Rust, strong quality), RNNoise as a
low-CPU mode. Add a compressor/limiter and gate after the denoiser (plain DSP).

### Auto-framing / zoom
No model needed for digital zoom and pan (it is a crop plus scale). Auto-framing needs a face or
person detector: MediaPipe Face Detection or BlazePose (Apache-2.0), or YOLO-class detectors
(note Ultralytics is AGPL). Smooth the crop box over time to avoid "Center Stage jitter".

### Inference runtime
**ONNX Runtime** gives one API with per-platform accelerators: CoreML (preview status) on
macOS, DirectML on Windows (any GPU), CUDA/TensorRT or OpenVINO on Linux, CPU everywhere as
fallback. Providers load as shared libraries, so one build can ship a CPU path and optionally
pick up a GPU path. This is what both the OBS plugin and NV Broadcast use.

---

## 4. Virtual camera and virtual mic: the hard, platform-specific part

| | macOS | Windows | Linux |
|---|---|---|---|
| **Virtual camera** | **CoreMediaIO Camera Extension** (macOS 12.3+, Apple's replacement for DAL plug-ins). Ships inside the app bundle, activated with a system-extension request, user approves in System Settings (location moved again in macOS 26 to Privacy & Security → Extensions → Media Extension). Needs Developer ID, notarization, the system-extension entitlement, and the app must live in /Applications. Works in Safari/FaceTime/Zoom/Teams. OBS 30+ uses this. | **Media Foundation Virtual Camera API** (`MFCreateVirtualCamera`, Windows 11 build 22000+): user-mode COM media source, no kernel driver, no driver signing; per-user or all-users; can wrap a physical camera (build 22621+). Microsoft's Windows-Camera sample is the template. Whether DirectShow-only apps see it is **not confirmed** in public docs, so test with Zoom/Teams/Chrome early. For Windows 10 you still need a **DirectShow filter** (what OBS ships; reaches almost every app). Plan: MF on Win11, DirectShow fallback for Win10. | **v4l2loopback** kernel module today (what OBS and NV Broadcast use); works with every app but needs a kernel module install and has PipeWire 1.6 regressions. **PipeWire virtual camera** (`media.class=Video/Source, media.role=Camera`) is the future and needs no kernel module, but sandboxed/portal apps filter out software nodes and OBS cannot consume it yet. Ship both: PipeWire node plus optional v4l2loopback. |
| **Virtual microphone** | **Core Audio AudioServerPlugIn (HAL driver)**, e.g. BlackHole (GPL-3.0 or paid licence) or your own from Apple's NullAudio sample. Documentation is thin. Needs a driver install into /Library/Audio/Plug-Ins/HAL and a coreaudiod restart. No app-only way exists. | Hardest piece. A virtual audio device needs a **kernel driver** (WaveRT / SysVAD-based), signed via Microsoft attestation signing; developers report Code 52 signature failures on some machines. Alternatives: licence Thesycon's TVirtAudio (commercial), or process audio in an **APO** attached to the real mic (what Equalizer APO + RNNoise do, no new device appears). | Easy: **PipeWire filter-chain** with a LADSPA plugin (DeepFilterNet or RNNoise) exposed as `Audio/Source`. This is standard, no kernel module. PulseAudio fallback via module-ladspa-sink + loopback. |

Effort ranking for the plumbing: Linux audio (easy) < Linux video (medium) < Windows video
(medium) ≈ macOS video (medium, plus Apple signing) < macOS audio (hard) < Windows audio (hardest).

---

## 5. Suggested architecture (for discussion)

- **One native core** in Rust or C++: capture (AVFoundation / Media Foundation / V4L2-PipeWire),
  ONNX Runtime inference, compositing on GPU (wgpu or per-OS), DeepFilterNet audio, state machine.
  Reasons: real-time frame loop, direct access to each OS's virtual-device API, DeepFilterNet is
  already Rust.
- **Thin UI shell** per platform or one shared UI (Tauri over the Rust core, or Qt). Electron adds
  100+ MB and a second process for video; avoid for the frame path.
- **Virtual-device adapters** behind one interface: CMIO extension (Swift, must be a separate
  target), MF media source DLL (C++), PipeWire/v4l2loopback (Rust).
- **Model tiers**: "Fast" (MediaPipe selfie, CPU) and "Quality" (RVM/MODNet on GPU/NPU),
  auto-selected per machine.
- **Licensing guardrails**: avoid RMBG-2.0 and Ultralytics YOLO; isolate any GPL component
  (RVM, BlackHole, noise-suppression-for-voice) in its own process or replace it.

What existing open source to reuse rather than rewrite: Webcamoid's virtual camera drivers
(Mac/Windows, split into their own project), OBS's CMIO extension and DirectShow filter as
references, Microsoft's Windows-Camera sample, DeepFilterNet's LADSPA plugin, PipeWire
filter-chain configs.

---

## 6. Risks worth deciding early

1. **Windows virtual mic** may need a signed kernel driver. Decide: build/sign one, licence
   Thesycon, or ship APO-style processing on the real mic for v1.
2. **macOS distribution**: Camera Extension requires Developer ID + notarization and user approval
   in System Settings; App Store distribution is possible but adds sandbox constraints. Budget a
   paid Apple developer account and the approval UX.
3. **Linux fragmentation**: PipeWire-only cameras are invisible to some apps; v4l2loopback needs
   kernel headers and DKMS. Document both paths.
4. **CPU-only machines**: MediaPipe selfie at 720p30 is fine on CPU; RVM is not. Keep the fast
   tier honest.
5. **Licences**: RVM is GPL-3.0, RMBG is non-commercial, BlackHole needs a licence for non-GPL
   apps.

---

## Sources
- NV Broadcast site: https://nvbroadcast.com/ and repo https://github.com/missmoode/nvidia-broadcast-linux
- Alternatives roundups: https://machow2.com/nvidia-broadcast-mac/ , https://alternativeto.net/software/nvidia-broadcast/?platform=mac
- Camo pricing/platforms: https://camo.com/pricing ; Camo 2 launch: https://techcrunch.com/2023/03/16/camo-2-launches-with-support-for-any-camera-new-effects-and-more
- XSplit VCam listing: https://www.stacksocial.com/sales/xsplit-vcam-premium-lifetime-subscription-mac-windows
- Krisp platforms (Discord): https://support.discord.com/hc/en-us/articles/360040843952
- Webcamoid releases: https://github.com/hipersayanX/webcamoid/releases
- OBS Background Removal: https://github.com/occ-ai/obs-backgroundremoval ; Lite: https://obsproject.com/forum/resources/live-background-removal-lite.2226/
- OBS virtual camera troubleshooting: https://obsproject.com/kb/virtual-camera-troubleshooting ; macOS 26 approval: https://obsproject.com/forum/threads/virtual-camera-mac-os-tahoe-26-3-1.194658/
- Apple camera effects requirements: https://support.apple.com/guide/mac-help/use-the-camera-mchlp2980/mac
- Apple WWDC22 Camera Extensions: https://developer.apple.com/videos/play/wwdc2022-10022 ; notarization: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution ; CMIO forum threads: https://developer.apple.com/forums/thread/736838 , https://developer.apple.com/forums/thread/840312
- Virtual mic on macOS: https://developer.apple.com/forums/thread/657619
- Windows MFCreateVirtualCamera: https://learn.microsoft.com/en-us/windows/win32/api/mfvirtualcamera/nf-mfvirtualcamera-mfcreatevirtualcamera ; sample: https://github.com/microsoft/Windows-Camera/blob/master/Samples/VirtualCamera/README.md ; Q&A: https://learn.microsoft.com/en-us/answers/questions/872883/which-api-to-use-for-creating-virtual-webcam
- Windows Studio Effects: https://support.microsoft.com/en-us/windows/ai/ai-features/windows-studio-effects
- Windows virtual audio driver signing: https://community.osr.com/t/can-i-skip-some-test-in-hlk-for-a-virtual-audio-driver/56641 ; Thesycon: https://thesycon.de/eng/virtual_audiodriver.shtml
- Linux virtual camera: https://ideasonboard.com/news/pipewire-is-the-new-v4l2loopback ; Ubuntu bug: https://lists.ubuntu.com/archives/ubuntu-sponsors/2025-November/085906.html ; Firefox bug: https://bugzilla.mozilla.org/show_bug.cgi?id=2007675
- PipeWire filter-chain: https://wiki.gentoo.org/wiki/PipeWire/Microphone_Noise_Suppression ; RNNoise plugin: https://github.com/werman/noise-suppression-for-voice
- DeepFilterNet: https://github.com/rikorose/deepfilternet ; RNNoise vs DFN3 study: https://publikasi.mercubuana.ac.id/index.php/sinergi/article/view/21917
- MediaPipe Image Segmenter: https://developers.google.com/edge/mediapipe/solutions/vision/image_segmenter
- RVM licence: https://github.com/PeterL1n/RobustVideoMatting ; RMBG-2.0: https://huggingface.co/briaai/RMBG-2.0
- NVIDIA Maxine Linux: https://docs.nvidia.com/maxine/afx/latest/LinuxAFXSDK/GetStartedOnLinux.html ; repos gone: https://forums.developer.nvidia.com/t/maxine-sdk-approval/356037
- ONNX Runtime execution providers: https://onnxruntime.ai/docs/execution-providers/
