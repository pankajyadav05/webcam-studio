# Hybrid RVM + MediaPipe verification (v4)

Only `apps/desktop/ui/engine/**` and `apps/desktop/dev/engine-demo/**` were edited.
No git operations, app installation changes, or maintainer footage were used.

## Acceptance and recommended default

This is an implemented and measured candidate, not a signed-off fix for the
reported plume. The exact head-sized bright-wall plume was not reproduced in
the public scenes. The native hybrid Replace window also exceeded the strict
16ms callback limit 11 times (56ms maximum). Blur also
recorded 5 callbacks over 16ms (37ms maximum). The pre-taper build had two
callbacks over 16ms, including a 39ms maximum. Both facts remain acceptance gaps.

The intended next default is hybrid, and `auto` implements that requested
selection. For a release based on the current evidence, I recommend explicit
MediaPipe until the plume case and native callback budget pass; retain explicit
RVM for cropped-body scenes that the coarse gate damages. No caller/UI/config
outside the permitted paths was changed to make that release choice.

## Implementation

`setMatting("auto" | "rvm" | "mediapipe" | "hybrid")` is backward compatible.
Auto requests hybrid when RVM is usable; RVM failure falls back to MediaPipe.
A failed gate worker retains RVM with a status message. Both workers have their
own backpressure. RVM uses its existing WebGPU/WASM path. The independent MP
gate uses the WASM CPU delegate, capped at 15Hz, sharing the camera capture
rather than waiting on a second decoder copy. Delivered gate rate can be lower
than the cap, particularly with slow RVM WASM.

All MediaPipe confidence components >=0.65 seed the gate, including small or
separate people. There is no largest-component/area rejection in the gate.
A 720p-normalized chamfer distance field keeps a 12px margin, then a 10px smooth
ramp. A local 16x9 motion field expands it continuously with gate age plus one
30fps frame, up to 96px. Motion in one region does not reopen the whole frame.
Near-opaque RVM can extend support along eight sampled continuous paths,
8px per step, up to 128px, with a 16px soft recovery taper and alpha 0.98–0.995 along the path. This
preserves MP-missed hair/fingers without an unrestricted opaque-alpha override.
Support is calculated at quarter output resolution; the final shader multiplies
it into full-resolution RVM alpha. No RVM alpha CPU readback was added.
The distance field and image storage are reused in the MP worker.

An empty MP core fails open to RVM. Gates fade out at 150–250ms of age; a stalled
worker times out at 1.5s. Source epochs discard stale asynchronous results. The MP worker's same-origin
fetch wrapper rejects foreign URLs before vendor telemetry can reach the
network, including during worker shutdown. This is necessary because worker
CSP is not inherited from the document's meta policy.
`stats().matting` reports the active algorithm, including RVM-only when the gate
is empty/stale. Create with `{debug:true}` to enable `getMattingDebug()`, which
returns the gate parameters, readiness, age and failure state.

## Performance

1280x720 public wave video, 30fps source. Each host runs alone. Chrome uses 25
one-second steady samples after five seconds of completed inference. WKWebView
uses 30 seconds starting ten seconds after navigation, the same rAF, decoded
frame, worker-message and bitmap-transfer wrappers as RVM-REPORT.md. Native
uses tauri://localhost and the exact production CSP. Cold setup remains in
native `overBudget` with `phase=load`, excluded from steady counts here.

MP ms includes synchronous MP inference, confidence extraction, gate/cleanup
math and worker output preparation; it is a worker latency upper bound, not an
isolated model-kernel measurement. RVM ms is its completed inference/alpha time.
WASM rows retain a slow RVM only by harness substitution; production auto
falls back to MediaPipe below 15-matte/sec capacity. Chrome policy may select
0.375; native typically remains 0.25. Quality comparisons pin RVM at 0.25.

| Host | Active | Effect | RVM/MP delegate | FPS | RVM ms | MP worker ms | Mattes/s | Gate/s | >16ms | Max ms |
|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| Chrome | hybrid | blur | webgpu/cpu | 30.00 | 25.31 | 11.02 | 28.38 | 14.21 | 0 | 0.60 |
| Chrome | hybrid | replace | webgpu/cpu | 29.90 | 25.30 | 9.84 | 27.73 | 13.98 | 0 | 0.50 |
| Chrome | rvm | blur | webgpu/— | 30.02 | 17.97 | 0.00 | 28.02 | 0.00 | 0 | 0.80 |
| Chrome | rvm | replace | webgpu/— | 29.92 | 17.98 | 0.00 | 25.73 | 0.00 | 0 | 1.00 |
| Chrome | mediapipe | blur | gpu/— | 29.98 | 0.00 | 11.42 | 17.45 | 0.00 | 0 | 0.90 |
| Chrome | mediapipe | replace | gpu/— | 30.00 | 0.00 | 11.14 | 17.22 | 0.00 | 0 | 1.40 |
| Chrome | hybrid | blur | wasm/cpu | 29.94 | 106.46 | 8.99 | 7.52 | 7.53 | 0 | 1.40 |
| Chrome | hybrid | replace | wasm/cpu | 29.96 | 104.55 | 8.54 | 7.71 | 7.68 | 0 | 0.90 |
| WKWebView | hybrid | blur | webgpu/cpu | 29.90 | 31.71 | 8.19 | 15.38 | 14.22 | 5 | 37.00 |
| WKWebView | hybrid | replace | webgpu/cpu | 29.73 | 32.65 | 8.68 | 15.49 | 13.94 | 11 | 56.00 |
| WKWebView | rvm | blur | webgpu/— | 29.94 | 23.81 | 0.00 | 28.65 | 0.00 | 0 | 7.00 |
| WKWebView | rvm | replace | webgpu/— | 29.95 | 23.90 | 0.00 | 28.02 | 0.00 | 0 | 14.00 |
| WKWebView | mediapipe | blur | gpu/— | 29.94 | 0.00 | 9.26 | 18.26 | 0.00 | 0 | 7.00 |
| WKWebView | mediapipe | replace | gpu/— | 29.94 | 0.00 | 8.23 | 18.65 | 0.00 | 0 | 14.00 |
| WKWebView | hybrid | blur | wasm/cpu | 29.95 | 101.68 | 7.00 | 7.68 | 7.70 | 0 | 7.00 |
| WKWebView | hybrid | replace | wasm/cpu | 29.90 | 101.81 | 6.90 | 7.66 | 7.65 | 0 | 5.00 |

The strict callback budget FAILED in the final measurements.

The final native hybrid Replace window still exceeds the callback budget; the
raw report identifies each frame/source wait. The pre-taper run included a
35ms alpha texture upload inside a 39ms frame callback, and 17ms source capture. These are measured
callback waits; this probe does not independently attribute driver/decoder CPU
versus GPU time. The built-in mitigation already uses MP CPU at 15Hz. A hard
main-thread bound likely needs worker-owned compositing/capture transport; a
lower gate rate alone is not established as a fix.

## Leakage proxy and visual evidence

The reference is MP CPU confidence >=0.65 dilated by 24px in 720p source space.
The all-frame metric uses the chamfer distance field and 320x180 harness-only
readback. It counts alpha>0.5 outside this band, divided by outside-band pixels.
The first ten recurrent frames are excluded. It is a proxy, NOT ground truth:
real sleeves, hair or people missed by MP count as leakage. The separate
full-resolution saved-frame metric uses a 24px square dilation, so values can
differ slightly. Production never reads alpha back for this measurement.

### chrome-quality

| Clip | Frames | RVM % | MP % | Hybrid % |
|---|---:|---:|---:|---:|
| bright-chair | 65 | 0.0065 | 0.0271 | 0.0013 |
| chair | 65 | 0.6142 | 0.0014 | 0.2546 |
| chair-close | 65 | 0.0065 | 0.0271 | 0.0013 |
| curly | 65 | 0.0970 | 0.0000 | 0.0436 |
| exit | 95 | 0.9905 | 0.0134 | 0.7012 |
| glasses | 65 | 0.3390 | 0.0006 | 0.2948 |
| hair | 65 | 7.6006 | 0.2154 | 5.4024 |
| office | 65 | 0.0000 | 0.0000 | 0.0000 |
| spin | 65 | 8.8440 | 0.0053 | 5.4243 |
| wave | 80 | 0.1146 | 0.0014 | 0.1101 |
| wave-fast | 35 | 0.1155 | 0.0000 | 0.1047 |
### native-quality

| Clip | Frames | RVM % | MP % | Hybrid % |
|---|---:|---:|---:|---:|
| bright-chair | 65 | 0.0058 | 0.0256 | 0.0014 |
| chair | 65 | 0.5791 | 0.0010 | 0.2418 |
| chair-close | 65 | 0.0058 | 0.0256 | 0.0013 |
| curly | 65 | 0.0933 | 0.0000 | 0.0477 |
| exit | 95 | 0.9791 | 0.0135 | 0.6951 |
| glasses | 65 | 0.3230 | 0.0004 | 0.2825 |
| hair | 65 | 7.4464 | 0.1705 | 5.3243 |
| office | 65 | 0.0000 | 0.0000 | 0.0000 |
| spin | 65 | 8.6288 | 0.0062 | 5.3891 |
| wave | 80 | 0.1000 | 0.0014 | 0.0977 |
| wave-fast | 35 | 0.0957 | 0.0000 | 0.0874 |

The original public gaming-chair clip demonstrates a leakage reduction, but it
also exposes MP retaining a chair and RVM fading a second person. The new office
source has a bright wall, dark office chair and hand near the face; it does not
reproduce the specific head-sized pale plume. chair-close is an actual crop;
bright-chair is an explicitly altered RGB wall stress scene. Neither establishes
an exact reproduction of the reported MacBook camera scene. A lower leakage
proxy alone is not proof that the specific plume regression is fixed.

Sources, licenses, hashes, extraction and alterations: `/tmp/engine-shots/v4/SOURCES.md`
and `prepare-hybrid-fixtures.py`. The clips use the Pexels License, not CC0:
[office](https://www.pexels.com/video/man-sitting-on-a-chair-while-working-8865594/),
[chair rotation](https://www.pexels.com/video/a-man-sitting-on-a-spinning-office-chair-9065728/).
Existing footage is documented in `/tmp/matting-bakeoff/SOURCES.md`.

Rows in contact sheets are RVM / MediaPipe / hybrid. Columns are recurrent
frames. Replace uses flat RGB #667a8c; blur uses radius 18. `*-edges-4x.png`
contains exact 4x nearest-neighbour crops. Every underlying full-resolution
source, alpha, replace and blur frame remains in the clip subdirectory.

## Visual inspection and remaining limitations

42 Node unit tests pass, plus the controlled GPU gate and compositor transform
regressions. The contact sheets and 4x crops were opened and inspected in both hosts.

- Curly hair: the diffuse flyaways above the head remain much closer to RVM than
  the harder MP cap. The hybrid does not sharpen the original RVM edge.
- Glasses: a short recovery candidate visibly cut the back of the hair on frame
  50. The final 128px sampled-path recovery restores that region. Its last 16px and
  its alpha activation are tapered to avoid an abrupt recovery-distance cutoff. The original
  RVM hair halo and some wall-colored wisps remain.
- Fast wave: saved head/hand crops retain the palm and translucent motion-blurred
  fingertips. Full-resolution ROI counts are in each host's `summary.json`.
  These four saved instants do not prove zero clipping on every possible motion.
- Raised white sleeves: most MP holes are avoided, but gate attenuation remains
  where MP misses broad soft sleeve regions. The low proxy error of MP must not
  be read as higher true accuracy on this clip.
- Gaming chair: the hybrid removes some detached/right-shoulder residue, while
  shared chair predictions near the head remain. The second person is already
  translucent/fades in RVM at .25; multiplying by a gate cannot restore them.
- Lower-body rotation: MP loses large portions of trousers. The bounded recovery
  cannot reach all of them, producing new clipped/jagged trouser borders in the
  hybrid. RVM-only is preferable for this cropped-body scene.
- Exit/turned-away person: the straight vertical occlusion is the actual door
  frame, also present in source and RVM, not a new gate edge. An empty MP core
  fails open, but an incomplete nonempty MP mask can still attenuate real parts.

A completely missed second person farther than the recovery reach from every
MP seed can still be clipped. Conversely, near-opaque furniture continuously
connected to a seed may survive recovery. The eight sampled rays are bounded
support checks, not an exact connected-component proof; gaps below the sampling
step can be bridged. Motion expansion can temporarily admit extra background.
Soft detail farther than the gate margin and without near-opaque support can
be attenuated. These are real limits of a multiplicative coarse gate.

## Failure/fallback matrix

| Host | Injected failure | Expected | Observed | Pass | CSP violations |
|---|---|---|---|---|---:|
| Chrome | gpu-fail | mediapipe | mediapipe | True | 0 |
| Chrome | worker-fail | mediapipe | mediapipe | True | 0 |
| Chrome | runtime-worker-fail | mediapipe | mediapipe | True | 0 |
| Chrome | model-fail | mediapipe | mediapipe | True | 0 |
| Chrome | device-lost | mediapipe | mediapipe | True | 0 |
| Chrome | slow | mediapipe | mediapipe | True | 0 |
| Chrome | gate-fail | rvm | rvm | True | 0 |
| Chrome | gate-crash | rvm | rvm | True | 0 |
| WKWebView | gate-fail | rvm | rvm | True | 0 |
| WKWebView | rvm-fail | mediapipe | mediapipe | True | 0 |

Explicit MediaPipe is covered by both effect measurements. All forced faults
are substituted in served test responses; no production fault flags were added.

## Ten-minute soak

```json
{
  "seconds": 600,
  "elapsedSeconds": 615.208,
  "baselineMiB": 1142.15625,
  "finalMiB": 613.546875,
  "deltaMiB": -528.609375,
  "firstMinuteMiB": 943.734375,
  "lastMinuteMiB": 640.359375,
  "firstMinuteFps": 30,
  "lastMinuteFps": 30,
  "firstMinuteInferMs": 19.100000008940697,
  "lastMinuteInferMs": 18.900000005960464,
  "firstMinuteGateMs": 11.900000005960464,
  "lastMinuteGateMs": 11.799999997019768,
  "firstMinuteGateFps": 14.759421430723199,
  "lastMinuteGateFps": 14.757969303553667,
  "firstMinuteHeapMiB": 1.672316551208496,
  "lastMinuteHeapMiB": 1.672170639038086
}
```

The soak used the pre-taper shader build recorded in validation-progress.json.
A subsequent shader-only recovery taper changes no allocation, worker, model,
or transport path; final quality, unit and timing probes were rerun afterward.

The soak loops public wave footage and alternates Blur/Replace every two minutes.
It samples engine FPS, both worker latencies, JS heap, and Chrome process-tree
RSS. Shared pages may be counted twice; separate OS GPU allocations are not
fully attributed. Compare warm first/last-minute medians, not just the single
endpoint delta. This is plateau evidence, not a claim of zero allocation.

## Reproduce

```sh
node --test apps/desktop/ui/engine/studio-engine.test.mjs apps/desktop/ui/engine/rvm-policy.test.mjs apps/desktop/ui/engine/hybrid-gate.test.mjs apps/desktop/ui/engine/offline-fetch.test.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/prepare-hybrid-fixtures.py
ENGINE_DEMO_PORT=4180 node apps/desktop/dev/engine-demo/server.mjs
# In another terminal; hosts and soak run sequentially:
node apps/desktop/dev/engine-demo/run-hybrid-verification.mjs
node apps/desktop/dev/engine-demo/verify-hybrid-gpu.mjs
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/hybrid-contact-sheets.py
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/hybrid-contact-sheets.py native-quality
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/analyze-hybrid-quality.py
/tmp/matting-bakeoff/venv/bin/python apps/desktop/dev/engine-demo/analyze-hybrid-quality.py native-quality
python3 apps/desktop/dev/engine-demo/write-hybrid-report.py
```

The quality harness drives production worker/compositor modules with deterministic
public frame sequences, including delayed gates at half the RVM input cadence.
The performance/fallback harness drives createStudioEngine through its real API.
The 2x wave keeps every second public source frame on a 30fps input timeline.

Raw reports, startup outliers, intermediate failed probes and source hashes are
retained under `/tmp/engine-shots/v4/`. `validation-progress.json` records the
final sequential run. No app UI/configuration or installed application was edited.

## Cost/latency conclusion

The final native hybrid runs have approximately 32ms RVM time and 15 delivered
mattes/sec, versus approximately 24ms and a higher delivered matte rate for
RVM-only, although the preview still follows the ~30fps source. GPU contention
from the compositor/recovery work is a likely bottleneck in this run; this is
not an isolated GPU-kernel or thermal experiment. MP already uses CPU/WASM at
15Hz. An additional candidate is caching/capping expensive GPU recovery at
15Hz while retaining cheap motion-aware base support, but that has NOT been
implemented or measured. Explicit MediaPipe is the measured fallback today.

The fast-wave quality harness is deterministic and processes every RVM input
frame. It demonstrates delayed-gate behavior and sampled detail retention, but
does not prove that the live native pipeline at ~15 mattes/sec never clips fast
motion. A separate temporal flicker score was not recomputed. The exact plume,
partial-body failure and strict callback budget remain unresolved sign-off work.
