# Background pipeline verification

The implementation uses Apple Vision balanced on a dedicated Rust serial thread.
An upright JPEG travels through Tauri raw binary IPC; the reply is an R8 mask.
A request and sequence handler are reused within a source/crop epoch. Accurate
quality is available through `createStudioEngine({ nativeQuality: 'accurate' })`
and the binary quality field; `segmenter_info.quality` describes the default.
Unsupported systems and native failures use the offline MediaPipe module worker.

The engine retains immutable camera captures, submits only one inference, and
consumes the newest retained frame when the provider becomes free. A completed
mask paints only its matching capture. Native camera captures and presentation use
owned HTML canvases, without an ImageBitmap dependency on the native path. Its adaptive delay is separate from the
reported alpha/frame mismatch. No effect has zero capture delay. WebKit's live
camera can report `mediaTime = 0` indefinitely; `MediaClock` deduplicates using
`presentedFrames` and maintains a monotonic inference clock in that case.

Matte history uses typed arrays, an 80 ms still-region filter with a continuous
fast-motion ramp, and bounded soft support for MediaPipe. There is no component
keep/drop filter. Capture uses the requested crop plus 12% context. WebGL2 does a
separable fast guided filter (half resolution, 6 px output radius, epsilon .001),
one mild output contrast curve, and approximately linear-light composition.
Blur uses a premultiplied background/weight dual-Kawase pyramid. Context loss
keeps a safe cover until GPU resources rebuild. Replace with no matte shows the
replacement (or neutral fill); Blur starts strongly blurred. Provider errors do
not switch compositors or expose the raw camera.

## Reproduce

Prerequisites already present on this Mac: Node 26, `/tmp/pw/node_modules/puppeteer-core`,
Google Chrome at `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`,
and `/tmp/matting-bakeoff` public fixtures plus its Python venv. Only the six
public clip names in `clips.json` are accepted by the HTTP fixture server.
The NASA astronaut test fixture has its own attribution alongside the image.

From the repository root:

```sh
cargo build -p studio-segmentation --example vision_pipe
node apps/desktop/dev/evaluation/evaluate.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/evaluation/analyze.py
node apps/desktop/dev/evaluation/realtime.mjs
node apps/desktop/dev/evaluation/browser-tests.mjs
node apps/desktop/dev/evaluation/zoom.mjs
cargo test --workspace
cargo clippy --workspace --all-targets
```

Run each `*.test.mjs` separately with `node --test <file>` under
`ui/engine/` and `dev/experimental/`. Node 26 does not accept a directory.
The evaluation HTTP shim invokes the actual Rust Vision implementation through
`vision_pipe`, using exactly the same WSI1/WSM1 payloads as Tauri. It is bound to
127.0.0.1 and is development-only. Production does not start an HTTP server.

```sh
cd apps/desktop/src-tauri
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_OPT_LEVEL=0
export CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_DEBUG=true
cargo tauri build --bundles app
```

Launch the executable inside `target/release/bundle/macos/Webcam Studio.app`,
with `WEBCAM_STUDIO_DIAGNOSTICS=/tmp/v5-shots/native.jsonl` to collect optional
statistics without pixels. The opt-in trace is disabled during ordinary use.
`/tmp/shot.sh` captures the window. `/tmp/ax.sh` was read and used; its AXPress
changed radio state without consistently sending WebKit change events on this
machine, so final mode changes were verified with native computer-use clicks.
No installation into /Applications was performed. CSP, icons, window.rs and
workspace release profiles were unchanged. No git commands were run.

## What the measurements establish

Results and counters are in `results.json`. Raw public artifacts are under
`/tmp/v5-evaluation/`, including per-frame mattes, contact sheets, exact 4x edge
crops, zoom 1/3 images, timings and browser failure reports. Native window
screenshots and traces are only under `/tmp/v5-shots/`.

Old quality mattes are the existing per-frame, zero-latency shipped-engine
outputs in `/tmp/matting-bakeoff/out/<public clip>/shipped/`. The real-time old
engine is the parked original, pinned to MediaPipe, with measurement-only hooks
served by the harness. Pixel/model/cleanup logic is unchanged for that baseline.
The new zero-latency reference is the raw Vision mask for the identical captured
frame/input and request epoch, before our temporal/guided/contrast processing.
It is a reference implementation, not annotated ground truth.

Flicker is the supplied REPORT.md definition: mean absolute alpha difference
inside the union of adjacent 8px edge rings, only on the common source-flow
screened slow pairs (p90 Farneback flow <= 1 source pixel in the fixed ROI).
Hair, glasses and exit have no eligible pairs; their flicker is N/A. The pooled
number weights slow pairs, not clips. The multi-person chair clip dominates it.

The band metric is symmetric mean contour distance to the SAME frame's raw
Vision alpha=0.5 boundary. For the old model it includes model disagreement as
well as cleanup; it must not be called a pure lag measurement. Actual displayed
mask/camera mismatch is measured separately in the real-time engine. Leakage is
alpha>0.5 more than 12px outside that reference, expressed as a fraction of the
whole frame. Zero additional leakage does not mean Vision's own predictions
are correct. Missing area is measured more than 8px inside the same reference.

Chrome static `renderMs` is submission/presentation; `readbackMs` separately
synchronizes GPU completion. The native engine's render statistic includes its
WebGL canvas presentation into the output canvas. Inference/JPEG/temporal
processing are separate. Throughput is from an actual playing 30fps public
wave video, after 5s warm-up, over an 8s sample. Long tasks are measured with
Chrome PerformanceObserver; this is not a WKWebView long-task claim. Background
system activity is not controlled. Native CPU includes desktop/Vision and its
identified WebKit GPU, WebContent and Networking processes; 100% means one CPU
core, not the entire machine.

## Acceptance limits and remaining decision

**The absolute 0.012 flicker target is not met.** The supplied public chair
reference itself has much higher flicker than the maintainer-local evidence in
the brief. The new pipeline improves wave/curly flicker and reduces flicker
relative to raw Vision, but chair flicker is worse than the old cutout. The old
cutout drops much of the second person; a lower number there is not evidence
of a better silhouette. Contact sheets retain this failure rather than hiding it.

Vision balanced can retain chair/desk residue and lose fine flyaways. Linear
composition cannot recover uncontaminated foreground RGB from camera pixels
whose hair already contains background colour. Accurate quality works, but is
not promised to sustain 25 masks/s and produces a larger mask/CPU filter load.
Cold shader compilation can cause a startup long task; steady-state public-clip
long-task counts and actual per-clip cold observations are retained separately.
A wedged native framework call cannot safely be killed as a Rust thread; the
JS deadline switches to MediaPipe while the serial worker remains bounded.

A bitmap snapshot/presentation trial passed Chrome tests but stalled in WKWebView
during a mode/visibility change. It is parked under dev/experimental; the shipped
native path avoids that dependency and retains the safer canvas presentation.

The camera initially showed a wall/laptop, and later a person was present. Both
Replace and Blur were captured in the actual built app. The final retained-canvas
build was checked with the camera again pointing at the wall/laptop, so final
live-person acceptance remains a maintainer check. A faint wall/shadow
residue remains visible in Replace on the leaning pose; this is not presented as
flawless live-person acceptance. Camera pixels were not saved as evaluation
footage. All numerical silhouette/flicker comparisons use the licensed public
clips. Native rendering is also not universally below 6 ms: the trace contains
9–12 ms render tails/intervals despite faster Chrome results and some 3–5 ms native
intervals. Both missed gates are retained in the results. The maintainer must
judge the remaining chair/hair/shadow errors and decide whether the optional
accurate tier or further WK surface optimization is warranted. No UI redesign
or quality selector was added in this task.

## Review findings

| Finding | Resolution |
| --- | --- |
| H1 | Immutable frame ring; mask ID/epoch selects exactly its captured RGB frame; measured latency adapts retention depth. |
| H2 | Media-frame identity and one in-flight request; newest retained frame consumed immediately after completion; no time throttle. |
| H3 | Continuous seeds/support growth and bounded soft falloff; no shoulder confidence cliff. |
| H4 | Per-frame connected-component keep/drop removed. |
| M5 | Typed-array 80 ms still-region history and smooth 2 ms motion response. |
| M6 | Segment crop plus 12% context; guided output upsampling; zoom 1/3 captures retained. |
| M7 | One mild contrast curve, evaluated once at output size. |
| M8 | Separable guided moments/coefficients filter, half size, camera LOD luminance, radius 6 px, epsilon .001. |
| M9 | Blur linearized background*weight and weight together through a dual-Kawase pyramid, normalize, then blend. |
| M10 | Logged/status-visible GPU failures, safe cover, explicit loss/restoration, one effect compositor. |
| M11 | Safe background/neutral cover or strong blur before first mask and on reset/error; person fades in; pixel assertion. |
| M12 | Deadlines, backoff, worker termination/reinitialization, guaranteed worker error replies and native fallback. |
| L13 | High-quality retained capture; native JPEG at capture size, MP directly at model's 256 input; no 384px low-quality intermediate. |
| L14 | R8 uploaded on mask change; replacement texture cached by image identity. |
| L16 | highp fragment shaders throughout. |
| L17 | Explicit DOM upload flip; explicit typed-mask no-flip plus matching shader coordinates; GPU mapping tests. |
| L19 | RVM, hybrid and 40,008,824 bytes of model/runtime assets parked outside ui; auto selects native/MediaPipe only. |
| L20 | Alignment, clock/rate, realistic temporal continuity, protocol, stuck worker, GPU mapping, privacy and failure-matrix tests. |

L15 and L18 were not findings in the supplied brief.
