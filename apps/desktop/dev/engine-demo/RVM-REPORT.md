# RVM integration verification

Implemented only under apps/desktop/ui/engine and apps/desktop/dev/engine-demo.
No git operations, application UI/config edits, or maintainer footage were used.

## Result and measurements

The strict combined >=30 fps / no-main-callback-over-16-ms target is **not fully
met**. The preview runs at approximately the 30 fps source cadence; the native
mean is slightly below a literal 30.000. The final GPU steady windows have no callback over 16 ms; see exact maxima
in the table. Earlier probes exposed synchronous texture-upload waits that
were reduced by finishing the worker snapshot and dispatching at decoded-frame
arrival. RVM inference now overlaps rendering instead of waiting for paint. Inference itself runs in a worker;
the table separates delivered matte rate from preview rate. WASM is too slow for
a usable RVM effect on this machine and normally falls back to MediaPipe.

Steady windows: Chrome 25 seconds after warmup, WKWebView 30 seconds after ten
seconds from navigation, all at 1280x720. Native uses tauri://localhost and the
exact requested CSP. Both play the existing public 30 fps wave clip. No other
benchmark browser ran concurrently in the final measurements. Measurements are
one run per mode/backend on this Mac, not a thermal or device-population claim.

| Host | Mode | Backend | Preview fps | Infer ms | Mattes/sec | Callbacks >16 ms | Max callback ms | rAF p95 ms | Ratio |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| chrome | blur | webgpu | 29.98 | 18.86 | 29.55 | 0 | 0.40 | 0.30 | [0.375] |
| chrome | replace | webgpu | 30.00 | 18.39 | 28.73 | 0 | 0.40 | 0.30 | [0.375] |
| chrome | blur | wasm | 30.00 | 105.82 | 7.54 | 0 | 1.70 | 0.60 | [0.25] |
| chrome | replace | wasm | 30.00 | 106.06 | 7.67 | 0 | 1.60 | 0.60 | [0.25] |
| webkit | blur | webgpu | 29.95 | 24.06 | 26.36 | 0 | 11.00 | 4.00 | [0.25] |
| webkit | replace | webgpu | 29.65 | 24.29 | 24.89 | 0 | 13.00 | 5.00 | [0.25] |
| webkit | blur | wasm | 29.89 | 104.52 | 7.71 | 0 | 6.00 | 2.00 | [0.25] |
| webkit | replace | wasm | 29.95 | 104.42 | 7.71 | 0 | 6.00 | 4.00 | [0.25] |

The WebGPU EP is **JSEP**, not ORT's native WebGPU entry. WASM is SIMD, one
thread, proxy off. WASM rows explicitly suppress automatic speed fallback only
in test responses. The production auto threshold is 15 completed-matte capacity
per second. Raw per-second samples, callback durations and resource lists are in
/tmp/engine-shots/v3/chrome-*.json and webkit-*-report.json. Bitmap-transfer calls from Promise continuations are timed separately too.
Browser "longtask" entries use a 50 ms threshold, so rAF and worker-message wrappers measure the
requested 16 ms budget instead. Cold startup is excluded from the table and is
retained separately in the native reports' overBudget entries (phase=load).

Profile fields split capture, camera upload, matte upload and final presentation.
Inference reuses its RGB tensor, recurrent tensors and alpha storage on the GPU,
with no explicit JS full-frame readback. Earlier native outliers happened at texture upload/capture boundaries,
not worker message handling; all callback classes are included in the table. A reusable
capture canvas avoids WebKit's direct createImageBitmap(video) decoder waits;
static camera/mask uploads are cached, and the worker waits for its snapshot
copy before handing the bitmap to WebGL. Keeping the live picture smooth takes
priority over waiting for a newer matte.

## Correctness and quality

31 Node unit tests pass, including ratio calibration/rollback and existing
MediaPipe filter regressions. The browser transform regression passes soft
alpha, ImageBitmap orientation, crop, mirror and resizing against Canvas2D.
The public engine exports remain createStudioEngine, cropRect and OUTPUT_DEFAULT.
setMatting is additive; existing lifecycle, framing and background methods remain.

All seven forced-selection/failure cases passed: missing WebGPU (tries WASM,
then speed fallback), worker startup failure, running worker crash, model fetch
failure, device loss, sustained slow inference, and explicit MediaPipe. All
final Chrome/WebKit runs recorded zero CSP violations and local requests only;
blob media is same-origin. The visibility check paused submissions while hidden
and resumed afterward. Rapid mode/source/size changes and double disposal caused
no asynchronous TypeError/ReferenceError. Old MediaPipe harnesses explicitly
select MediaPipe to preserve their existing assertions.

A quality check found ORT native WebGPU 1.23.2 and 1.30.0 attenuating alpha:
public curly frame mean .042 vs .398 on WASM, despite exact GPU/CPU RGB input
agreement. The JS WebGPU provider gives matching alpha. Its three unsupported
ceil-mode average pools are rewritten as mathematically equivalent SAME_UPPER
pools. Every learned weight remains unchanged. prepare-model.py is reproducible;
the bundled model hash and original source hash are in vendor/rvm/MODELS.md.

Native GPU profile, means of per-second stats (ms; worker and main-thread work overlap):

| Mode | GPU inference/alpha | Worker prep | Capture | Render | Camera upload | Alpha upload | Present | Transfer p95 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| blur | 24.06 | 1.03 | 2.29 | 2.70 | 0.13 | 2.23 | 0.23 | 2.00 |
| replace | 24.29 | 1.10 | 2.52 | 3.99 | 0.32 | 2.06 | 0.48 | 2.00 |

These timings locate the engine cost, but do not independently trace WebKit decoder/frame-scheduling waits. The remaining fractional FPS shortfall cannot be attributed more precisely from callback timings alone. The chosen production policy keeps .25 on native WebKit, reuses the latest alpha, and preserves soft edges.

Across 12 recurrent public curly frames, GPU versus the **original upstream** WASM model: maximum per-frame MAE 1.69973387e-08; maximum pixel difference 5.00679016e-06. See parity.json.

I opened the contact sheets and edge crops under /tmp/engine-shots/v3/quality:

- hair: RVM keeps the white sleeves/raised arms that MediaPipe cuts into holes.
  It retains a diffuse fringe at the sleeve edge and some wall color in wisps.
- curly: more thin flyaways remain above the head. MediaPipe makes a smoother,
  harder cap. RVM is softer and carries a little green/background haze.
- wave: RVM gives a more coherent palm/finger silhouette and drops most laptop
  fragments; motion-blurred fingertips are still translucent.
- glasses: face/lens area stays continuous, with a softer hair/neck border and
  more coherent hand than the aggressively cleaned fallback.
- chair: dark shoulder smears and translucent second-person/desk residue remain.
  The adaptive sequence reached .375; a separate fresh .25 sequence reproduces
  the second-person fading stress case. This is a known model limitation.

The <3% alpha-removal sheet row is a comparison only. It barely improves the
shoulder smear and can remove faint hair tips; no threshold, component filter,
extra temporal smoothing or sharpening is applied to production RVM alpha.
RVM reuses the last matte between inferences, so fast motion still has latency.
No ground-truth alpha was available; these are visual observations, not an
accuracy score. Public footage sources/licenses remain in the bake-off SOURCES.md.

## Memory soak

Chrome fake camera, 1280x720, 15-second warmup then 600 one-second samples,
alternating Blur/Replace every two minutes. RSS is the summed Chrome process tree;
shared pages may be counted twice. It is not comparable to a native single-process
RSS figure and does not fully attribute separate system GPU allocations.

```json
{
  "seconds": 600,
  "elapsedSeconds": 614.355,
  "baselineMiB": 1134.046875,
  "finalMiB": 593.5,
  "deltaMiB": -540.546875,
  "firstMinuteMiB": 1156.96875,
  "lastMinuteMiB": 657.15625,
  "firstMinuteFps": 30,
  "lastMinuteFps": 30.00096777315262,
  "firstMinuteInferMs": 18.5,
  "lastMinuteInferMs": 17.5,
  "firstMinuteHeapMiB": 1.594273567199707,
  "lastMinuteHeapMiB": 1.6470060348510742
}
```

RSS after engine disposal and a two-second wait: 599.28 MiB. Browser allocators may retain freed resources.

Read the first/last-minute medians as allocator plateau evidence, not proof that
no browser allocation ever occurs. GPU input/pixel storage is reused and GPU
buffers/textures/session resources are explicitly released on disposal. Bitmap,
tensor and command wrappers still allocate per frame, and forced WASM readback
uses getImageData. The literal zero-allocation requirement is not achieved.

## Distribution and integration

Added vendor footprint: **40,008,824 bytes** (38.16 MiB), including runtime, model, licenses, notices and model notes/script. Existing MediaPipe assets remain. Model bytes: 14,975,666; bundled SHA-256 `27f4d977d7aca993e3f5311b7e47090f42d46db5e050ee03c877637309640bf0`.

RVM code is GPL-3.0, compatible with this GPL-3.0-or-later app. No separate
license grant for the released weights was found. The runtime is MIT and its
third-party notices are included. Sources and every redistributed file hash are
in ui/engine/vendor/README.md.

**Required CSP/UI changes: none.** Optional UI can call setMatting; default auto
works with existing callers. No application or Rust/Tauri config was edited.

Files changed/added:

- engine/studio-engine.js; rvm.js; rvm-worker.js; rvm-policy.js;
  rvm-policy.test.mjs; vendor/README.md; vendor/ort/*; vendor/rvm/*.
- engine-demo/demo.js; server.mjs; verify-webkit.swift; README.md;
  verify.mjs; verify-fallbacks.mjs; verify-confidence.mjs; verify-matte.mjs.
- engine-demo/verify-rvm.mjs; verify-rvm-transform.mjs; verify-rvm-parity.mjs;
  verify-rvm-quality.mjs; probe-rvm-io.mjs; soak-rvm.mjs;
  measure-rvm-native.sh; measure-rvm-chrome.sh; rvm-contact-sheets.py;
  rvm-vendor-manifest.py; write-rvm-report.py; RVM-REPORT.md.

Reproduction commands are in engine-demo/README.md. Screenshots:
/tmp/engine-shots/v3/chrome-webgpu-{blur,replace}.png,
/tmp/engine-shots/v3/webkit-webgpu-{blur,replace}-{blur,replace}.png,
/tmp/engine-shots/v3/quality/{hair,curly,wave,glasses,chair}-{contact,edges}.png,
and /tmp/engine-shots/v3/quality025/chair-contact.png.
