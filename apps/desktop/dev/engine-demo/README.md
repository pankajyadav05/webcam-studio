# Studio engine verification

Everything here is dev-only and is outside the bundled `ui/` directory. No npm
or bundler is used by the engine. Assets in `ui/engine/vendor` are sufficient
for offline use; provenance and SHA-256 values are in that folder's README.

## Run

```sh
node apps/desktop/dev/engine-demo/server.mjs
# Open http://127.0.0.1:4178 for getUserMedia, or /?portrait for the fixture.
node --test apps/desktop/ui/engine/studio-engine.test.mjs
node apps/desktop/dev/engine-demo/verify.mjs
node apps/desktop/dev/engine-demo/verify-fallbacks.mjs
swift apps/desktop/dev/engine-demo/verify-webkit.swift
swift apps/desktop/dev/engine-demo/verify-webkit.swift --asset
```

On the installed Node 26, `node --test apps/desktop/ui/engine` treats the directory
as a module entry point and fails with MODULE_NOT_FOUND. The explicit test-file
command above runs all 27 tests. Adding an index.js or package.json to make
the directory invocation work would exceed the task's permitted file list.

The Puppeteer scripts use the existing `/tmp/pw` install and Google Chrome in
`/Applications`; they install nothing. The native probe uses Cocoa/WKWebView and
briefly opens a window. `--asset` serves the exact same files through a
WKURLSchemeHandler at `tauri://localhost`, without an HTTP server dependency.
The page contains the exact requested CSP, including no unsafe-inline or
unsafe-eval. The model's WASM needs only wasm-unsafe-eval.

## Public-domain fixture

The image is deliberately kept outside the repository, at
`/tmp/engine-portrait.jpg`. Fetch it before testing:

```sh
curl -fL https://upload.wikimedia.org/wikipedia/commons/8/8d/President_Barack_Obama.jpg -o /tmp/engine-portrait.jpg
```

Source and public-domain statement:
https://commons.wikimedia.org/wiki/File:President_Barack_Obama.jpg
Official White House portrait by Pete Souza, made in his U.S. federal duties.
The demo places the photo over a moving striped test backdrop and feeds it to
the engine using canvas.captureStream(). It draws frames before awaiting
video.play(), since WebKit waits for a new stream frame to resolve play().

The HTTP server binds only 127.0.0.1. ENGINE_DEMO_PORT changes the default 4178;
ENGINE_PORTRAIT changes the fixture's path. The Puppeteer verification uses 4178.

## Verified on Apple M5 Pro, 2026-10-23

- 18 math tests pass: aspect ratios, zoom 1/2/4, pan extremes, limits, NaN,
  mirror independence, unavailable dimensions.
- Chrome: approximately 30 fps at 1280x720, GPU worker inference 7.5–10.6 ms,
  render submission 0.30–0.53 ms. Ten one-second sustained samples.
- Chrome main-thread requestAnimationFrame callbacks: p95 0.8 ms, max 15.4 ms
  including startup and changing settings. No observed Long Tasks (>50 ms).
- Native WKWebView with a tauri:// asset handler: approximately 27 fps, GPU
  inference 7.5 ms, render submission 2.5 ms after warm-up.
- CPU delegate fallback, forced GPU runtime failure, model failure, Canvas2D
  compositor fallback, empty replacement, undecoded/null source, repeated
  start/stop and output/framing/mode changes all exercise the real engine.
  The failure cases inject faults in the served JS only; production bytes do
  not have test flags or special branches.
- No CSP violations or browser errors. Runtime requests stayed local. None mode
  makes no vendor requests. Every runtime vendor asset in the manifest is used.
  LICENSE and the manifest are legal/provenance documents, not runtime fetches.
- Screenshots and JSON measurements are under `/tmp/engine-shots/`:
  portrait-before.png, portrait-blur.png, portrait-replace.png,
  portrait-mirror.png, cpu.png, runtime-cpu.png, failure.png, canvas2d.png,
  lifecycle.png, webkit-blur.png, webkit-asset-blur.png, report.json,
  fallback-report.json, webkit-report.json, webkit-asset-report.json.

These are submission timings, not GPU completion timings. Native WebKit's first
inference can take several seconds to compile GPU kernels. The video continues
unmodified during this warm-up; its one-time cost is kept from throttling later
inference. The first completed mask fades in over 180 ms.

The supplied 256px selfie model can retain bits of background near hair or
shoulders; it cannot produce studio-quality matting. The test fixture is a
slowly moving photograph, not a person moving on a live webcam. Very fast motion
can outrun the last mask. Missing WebGL uses a softer downsample/upsample tent
blur; it is less Gaussian than the two-pass GPU compositor. Safari 17+ with
module workers, ImageBitmap, WebGL, and WASM SIMD is the supported baseline.

## API

Exports and engine method names exactly match the brief. No extra exports,
configuration, or methods. `on('status', callback)` also returns an unsubscribe
function. Engine methods are safe after dispose; the source stream and supplied
replacement image remain caller-owned. The engine never stops camera tracks.

## Matte cleanup follow-up (v2)

`studio-engine.js` keeps its original exports and methods. Its worker now uses
smoothstep confidence contrast (0.40–0.75), an eight-connected 0.65 confidence
core, and an area cutoff of 15% of the largest component. A one-texel soft rim
keeps edge detail; rejected islands cannot leak back in through that rim. The
0.65 core rejected more chair bridges than 0.50 in the comparison fixture.
Temporal confidence smoothing uses a 30 ms time constant, with 90% response to
changes over 0.20, so a moving silhouette does not leave a translucent trail.
All component traversal and confidence processing happen in the worker, with
reused arrays. The GPU reconstructs the mask at half output resolution using
nine camera-color-guided samples, then bilinear sampling and the existing
1.5 output-pixel feather. The Canvas2D fallback receives the same cleaned matte
and uses high-quality canvas interpolation, without the camera-guided pass.
Shader setup is spread across three frames to bound synchronous WebKit startup
work; the camera continues painting while shaders finish.

The v2 verification uses the same portrait plus a traced photo cutout placed
in front of a dark chair/headrest. No additional image download is required.
`/?portrait&chair` selects the chair fixture; `&still` freezes its motion.
The hand-traced cutout includes a few pixels of the original photo backdrop;
it is a controlled segmentation challenge rather than a ground-truth matte.

```sh
# Save the old engine BEFORE making changes (the snapshot is not in the repo).
mkdir -p /tmp/engine-shots/v2
cp apps/desktop/ui/engine/studio-engine.js /tmp/engine-shots/v2/studio-engine-before.js
# With server.mjs running, after applying the change:
BASELINE=1 node apps/desktop/dev/engine-demo/verify-matte.mjs
node apps/desktop/dev/engine-demo/verify-matte.mjs
node apps/desktop/dev/engine-demo/verify-confidence.mjs
node apps/desktop/dev/engine-demo/tune-matte.mjs
swift apps/desktop/dev/engine-demo/verify-webkit.swift --asset --v2 --replace
swift apps/desktop/dev/engine-demo/verify-webkit.swift --asset --v2 --replace --chair
# Optional native comparison against the saved old engine:
swift apps/desktop/dev/engine-demo/verify-webkit.swift --asset --v2 --replace --baseline
node --test apps/desktop/ui/engine/studio-engine.test.mjs
```

`verify-matte.mjs` runs real GPU inference, waits for a completed mask and warm-up,
collects ten sustained one-second samples per fixture, checks >=24 fps and <=16 ms steady callbacks, and
asserts no browser errors, CSP violations, or external requests. It saves
`portrait-{before,after}.png`, `chair-{before,after}.png`, source images, and JSON
reports in `/tmp/engine-shots/v2/`. `verify-confidence.mjs` separately substitutes
a deterministic confidence-map producer **only in the served test response**:
0.35 and 0.60 chair islands must disappear exactly, and the face must survive.
It saves `confidence-{before,after}.png` and `confidence-report.json`. This is a
controlled worker/compositor regression, not evidence of real model accuracy.
The real-inference screenshots and controlled-mask screenshots were opened and
visually inspected. The latter removes both floating patches completely.

Native WKWebView, GPU delegate, tauri:// asset handler, 1280×720 on Apple M5 Pro:
the final twenty sustained samples for the portrait measured 26.0–28.5 fps,
segmentMs 7.08–9.10 ms, renderMs 3.49–4.59 ms; the chair fixture measured
27.0–28.5 fps, segmentMs 6.98–8.75 ms, renderMs 3.90–4.80 ms. The final chair
probe's largest frame callback was 8 ms (p95 6 ms). The final portrait probe
recorded a 34 ms callback at initial graphics setup (206 ms after load), and
an 18 ms callback during steady rendering (24.7 seconds after load), with p95
6 ms. Earlier native runs peaked at 14 ms. Mask callbacks were at most 1 ms.
No CSP violations. The strict 16 ms main-thread budget therefore **did not pass**
the final portrait run, although sustained frame rate passed on both fixtures.
`verify-webkit.swift --v2` prints BUDGET FAIL and exits nonzero on that condition;
the report retains all over-budget callbacks. Timing is elapsed submission time,
not GPU completion or isolated CPU time. Main-thread graphics setup/upload/copy
calls can stall; moving the compositor to a worker would be a larger follow-up
than the mask processing implemented here.

The real model still labels some chair pixels with high confidence, connected
to the head or shoulders. These remain visible, more opaque, in the natural
chair test. Confidence contrast and component filtering cannot distinguish
those from a person without a different model or additional scene assumptions.
This change addresses weak translucent patches and small detached islands;
it does not establish perfect chair removal. Moving fixtures are photographs,
not live webcam motion; there is no new real-camera recording in this probe.

Chrome v2, real inference, ten one-second samples per fixture: portrait 29.0–30.5
fps, segmentMs 7.61–11.03 ms, renderMs 0.19–0.61 ms; chair 29.5–31.0 fps,
segmentMs 8.50–11.24 ms, renderMs 0.25–0.50 ms. Steady frame callbacks peaked at
1.2 ms and mask message callbacks at 0.2 ms. All 27 unit tests pass, including
the 15% component boundary, separate people, disappearing/moving silhouettes,
weak bridges, empty/full masks, noise smoothing, curve behavior, and export
compatibility. The existing Chrome smoke and all five fallback/lifecycle cases
also pass with no browser errors or CSP violations.

The final broader Chrome smoke run (startup plus settings/output/mode changes)
recorded a 21.5 ms callback, despite p95 0.8 ms and no Long Tasks. Its functional
assertions pass, but it provides another counterexample to the strict 16 ms
bound. `smoke-report.json` and `fallback-report.json` have been copied into the
v2 artifact directory alongside the per-fixture measurements. The native
portrait failure report is intentionally retained, rather than replaced by a
passing retry. The original smoke harness now closes its page before closing
Chrome, so live media/worker targets cannot leave its cleanup waiting.

## RVM integration (v3)

The default engine uses RVM MobileNetV3 FP32 on the ONNX Runtime Web 1.23.2
**JS WebGPU EP**. `engine.setMatting('auto' | 'rvm' | 'mediapipe')` selects it;
`auto` tries WASM SIMD on GPU failure/poor speed, then MediaPipe below 15 mattes
per second. Explicit `rvm` retains a slow RVM backend for diagnosis. GPU device
loss or a worker crash falls back to MediaPipe and reports the reason.

`stats()` retains its prior fields and adds `matting`, `inferMs`, `inferFps`.
Diagnostic fields include `ratio`, `preprocessMs`, `snapshotMs`, `presentMs`,
`cameraUploadMs`, and `maskUploadMs`. `inferMs` includes completed GPU work and
alpha rendering, not just command submission. `segmentMs` additionally includes
preprocessing and the bitmap snapshot. `inferFps` is actual completed mattes/sec;
preview FPS can stay at camera cadence while the last matte is reused.

RVM dispatches from decoded-frame callbacks before paint and skips duplicate
media timestamps, so inference overlaps rendering. MediaPipe keeps its prior
scheduling. The worker imports `rvm.js`; GPU preprocessing converts a full camera bitmap
into a persistent planar RGB buffer. Recurrent and alpha tensors stay on the
same GPU. Alpha is rendered into a bitmap on that device, then sampled by the
existing compositor with the same crop and mirror as camera RGB. ImageBitmap
ignores WebGL's UNPACK_FLIP_Y_WEBGL: the soft-matte shader accounts for this
explicitly. RVM bypasses the MediaPipe confidence curve, island cleanup, guided
reconstruction and final contrast/feather. It uses the existing effect fade.

Do not substitute ORT's native WebGPU entry without checking image parity.
Both 1.23.2 and 1.30.0 native EPs produced attenuated alpha on the public curly
frame (mean ~.042 vs ~.398 with WASM). GPU input bytes matched CPU RGB exactly.
The JS EP needs an equivalent three-node AveragePool rewrite; source hashes,
proof, preparation script and weight license caveat are in vendor/rvm/MODELS.md.
FP16 on the JS EP ran ~40 ms in the probe, versus ~17 ms for FP32; it is not
shipped. All model/runtime assets are offline, with hashes in vendor/README.md.

Reproduce from the repository root (public bake-off assets must exist in /tmp;
Puppeteer is /tmp/pw/node_modules/puppeteer-core and Chrome is the system app):

```sh
node --test apps/desktop/ui/engine/*.test.mjs
ENGINE_DEMO_PORT=4180 node apps/desktop/dev/engine-demo/server.mjs
# In another terminal, run benchmarks sequentially:
sh apps/desktop/dev/engine-demo/measure-rvm-native.sh
sh apps/desktop/dev/engine-demo/measure-rvm-chrome.sh
node apps/desktop/dev/engine-demo/soak-rvm.mjs 600
node apps/desktop/dev/engine-demo/verify-rvm-quality.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/rvm-contact-sheets.py
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/rvm-contact-sheets.py quality025 chair
```

The native probe uses tauri://localhost + the exact production CSP. `--v3
--asset --clip` plays the existing public 1280x720 30 fps wave clip through a
local blob URL; WKURLSchemeHandler media URLs do not supply normal media range
semantics. It records engine rAF, decoded-frame and worker-message callbacks, including
all callbacks over 16 ms, including bitmap-transfer postMessage calls,
with a 30-second steady window starting 10 seconds after load. Browser longtask
entries have a 50 ms threshold and are NOT substituted for the 16 ms metric.
`--wasm --keep-slow` is benchmark-only: it prevents auto fallback so WASM can be
measured for the full window. Normal auto mode remains speed gated.

Chrome's new scripts use port 4180 so they can coexist with the older demo
server on 4178. The legacy MediaPipe checks now explicitly select MediaPipe.
`verify-rvm.mjs` injects GPU, worker, model-fetch, device-loss and slow-inference
failures into test responses only. It also exercises disposal and rapid mode
switches. `verify-rvm-transform.mjs` compares both compositors on a synthetic
asymmetric soft matte, including crop/mirror/resize. `verify-rvm-parity.mjs`
compares 12 recurrent WebGPU frames with the **original upstream model** on WASM.
`probe-rvm-io.mjs` diagnoses output placement; `NATIVE_EP=1` uses the original
bake-off runtime, and `ORT_NEXT=1` additionally selects /tmp/ort-rvm-1.30.

`verify-rvm-quality.mjs` processes all frames of five public clips, preserving
recurrence; it writes selected alpha frames and timing/ratio history. Passing
`025` fixes the ratio at .25 and repeats the chair stress case. The contact
sheet's final row removes alpha below 3% for comparison only. This mitigation
is not in production: it barely changes the smears and can trim faint flyaways.

The soak uses Chrome's fake camera at 1280x720, alternates Blur/Replace every
two minutes, warms for 15 seconds, and samples for 600 seconds. Its RSS is the
sum of Chrome's process tree (shared pages may be counted twice), not JS heap
alone. Warm first/last-minute medians and FPS are reported separately from the
single-point delta. Browser/ORT objects and command encoders are necessarily
created per frame; persistent pixel/tensor storage is reused. WASM's Canvas2D
getImageData still allocates a readback buffer per inference. This is not a
literal zero-allocation implementation.

Artifacts and final measurements: `/tmp/engine-shots/v3/`, with the integration
report in `RVM-REPORT.md` in this directory. No maintainer footage is used by
these checks. No CSP or application UI edits are required.

The soak pins `--use-fake-device-for-media-stream=fps=30` and asserts the
capture track's settings. Chrome's unconfigured fake device can otherwise
supply 20 fps even when the page asks for 30. Flag configuration is documented
in Chromium's FakeVideoCaptureDeviceFactory::ParseFakeDevicesConfigFromOptionsString:
https://chromium.googlesource.com/chromium/src/+/main/media/capture/video/fake_video_capture_device_factory.cc

## Hybrid integration (v4)

The current `auto` selection requests RVM plus an independent MediaPipe gate.
`setMatting('auto' | 'rvm' | 'mediapipe' | 'hybrid')` remains additive. The gate
uses MP's CPU/WASM delegate at up to 15 Hz; RVM keeps its existing worker and
WebGPU/WASM inference. Its multiplication happens in the GPU compositor.
`stats().matting` reports active hybrid, RVM-only, or MediaPipe-only operation.
Creating the engine with `{ debug: true }` adds `getMattingDebug()` for gate
parameters, age, readiness and failures. No production fault-injection flags
were added. The offline MP worker rejects foreign fetches before the browser
can send vendor usage telemetry, including when a worker closes during fallback.

```sh
node --test apps/desktop/ui/engine/studio-engine.test.mjs apps/desktop/ui/engine/rvm-policy.test.mjs apps/desktop/ui/engine/hybrid-gate.test.mjs apps/desktop/ui/engine/offline-fetch.test.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/prepare-hybrid-fixtures.py
ENGINE_DEMO_PORT=4180 node apps/desktop/dev/engine-demo/server.mjs
# Another terminal; sequential Chrome/native quality, timings, failures, soak:
node apps/desktop/dev/engine-demo/run-hybrid-verification.mjs
# Resume completed stages after a harness failure:
node apps/desktop/dev/engine-demo/run-hybrid-verification.mjs --resume
# Run after the benchmarks, so it does not compete for the GPU:
node apps/desktop/dev/engine-demo/verify-hybrid-gpu.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/hybrid-contact-sheets.py
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/hybrid-contact-sheets.py native-quality
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/analyze-hybrid-quality.py
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/analyze-hybrid-quality.py native-quality
python3 apps/desktop/dev/engine-demo/write-hybrid-report.py
```

`HYBRID-REPORT.md` records results and acceptance limitations. v4 artifacts live
under `/tmp/engine-shots/v4/`. The quality harness processes public frame
sequences through production workers and the production compositor, with RVM
pinned to .25 and delayed MP gates. It writes real Replace/Blur outputs and
harness-only alpha readbacks. All-frame metrics use a 24px band around MP's
high-confidence core as a proxy reference. They cannot establish true matte
accuracy when MP misses a sleeve or a person. The sources and derived scenes
are explicitly identified; the headrest plume must not be claimed fixed merely
because this proxy decreases.

The native harness uses `tauri://localhost`, the exact CSP from the v3 probe,
and its same callback wrappers. Shader setup is excluded only from the steady
window and retained separately. The ten-minute soak loops public wave footage,
not a webcam. No maintainer recording is used. Both benchmark hosts should run
alone; `run-hybrid-verification.mjs` enforces sequential execution. The final
report must retain any observed callback-budget failure and any new clipping.
