"""Summarize measured artifacts without converting a failed budget into a pass."""
from pathlib import Path
import json,statistics,hashlib
r=Path('/tmp/engine-shots/v4');rows=[];failures=[]
def mean(samples,key):
 a=[x[key]for x in samples if isinstance(x.get(key),(float,int))];return statistics.mean(a)if a else 0
def metrics(host,mode,j):
 samples=j['samples'];matting=j['stats']['matting'];is_mp=matting=='mediapipe'
 if host=='Chrome':
  over=sum(j[k+'Summary']['over16']for k in ['callbacks','maskCallbacks','sourceCallbacks','transferCallbacks']);maximum=max(j[k+'Summary']['max']or 0 for k in ['callbacks','maskCallbacks','sourceCallbacks','transferCallbacks'])
 else:
  over=sum(x['phase']=='steady'for x in j['overBudget']);maximum=max(j[k].get('max',0)or 0 for k in ['frameCallbacks','maskCallbacks','sourceCallbacks','transferCallbacks'])
 return {'host':host,'mode':mode,'matting':matting,'backend':j['stats']['backend'],'gateBackend':j['stats'].get('gateBackend'),'fps':mean(samples,'fps'),'rvmMs':0 if is_mp else mean(samples,'inferMs'),'mpMs':mean(samples,'inferMs')if is_mp else mean(samples,'gateInferMs'),'matteFps':mean(samples,'inferFps'),'gateFps':mean(samples,'gateFps'),'over16':over,'maxCallback':maximum,'violations':len(j['violations']),'file':''}
for kind in ['webgpu','rvm','mediapipe','wasm']:
 for mode in ['blur','replace']:
  p=r/f'chrome-{kind}-{mode}.json'
  if p.exists():
   j=json.loads(p.read_text());row=metrics('Chrome',mode,j);row['file']=str(p);rows.append(row)
for model,backend in [('hybrid','webgpu'),('rvm','webgpu'),('mediapipe','webgpu'),('hybrid','wasm')]:
 for mode in ['blur','replace']:
  p=r/f'webkit-{model}-{backend}-{mode}-normal-report.json'
  if p.exists():
   j=json.loads(p.read_text());row=metrics('WKWebView',mode,j);row['file']=str(p);rows.append(row)
for kind in ['gpu-fail','worker-fail','runtime-worker-fail','model-fail','device-lost','slow','gate-fail','gate-crash']:
 p=r/f'chrome-{kind}-replace.json'
 if p.exists():
  j=json.loads(p.read_text());expected='rvm'if kind.startswith('gate-')else'mediapipe';failures.append({'host':'Chrome','case':kind,'expected':expected,'actual':j['stats']['matting'],'fps':j['stats']['fps'],'violations':len(j['violations']),'pass':j['stats']['matting']==expected and not j['violations']})
for kind,expected in [('gate-fail','rvm'),('rvm-fail','mediapipe')]:
 p=r/f'webkit-hybrid-webgpu-replace-{kind}-report.json'
 if p.exists():
  j=json.loads(p.read_text());failures.append({'host':'WKWebView','case':kind,'expected':expected,'actual':j['stats']['matting'],'fps':j['stats']['fps'],'violations':len(j['violations']),'pass':j['stats']['matting']==expected and not j['violations']})
quality={}
for host in ['chrome-quality','native-quality']:
 p=r/host/'summary.json'
 if p.exists():quality[host]=json.loads(p.read_text())
soak=json.loads((r/'soak.json').read_text())if(r/'soak.json').exists()else None
report={'performance':rows,'fallbacks':failures,'quality':quality,'soak':soak['summary']if soak else None,'engineHashes':{p.name:hashlib.sha256(p.read_bytes()).hexdigest()for p in Path('apps/desktop/ui/engine').glob('*.js')}}
(r/'results.json').write_text(json.dumps(report,indent=2))
text='''# Hybrid RVM + MediaPipe verification (v4)

Only `apps/desktop/ui/engine/**` and `apps/desktop/dev/engine-demo/**` were edited.
No git operations, app installation changes, or maintainer footage were used.

## Acceptance and recommended default

This is an implemented and measured candidate, not a signed-off fix for the
reported plume. The exact head-sized bright-wall plume was not reproduced in
the public scenes. The native hybrid Replace window also exceeded the strict
16ms callback limit __REPLACE_COUNT__ times (__REPLACE_MAX__ms maximum). Blur also
recorded __BLUR_COUNT__ callbacks over 16ms (__BLUR_MAX__ms maximum). The pre-taper build had two
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
'''
for x in rows:text+=f"| {x['host']} | {x['matting']} | {x['mode']} | {x['backend']}/{x['gateBackend']or '—'} | {x['fps']:.2f} | {x['rvmMs']:.2f} | {x['mpMs']:.2f} | {x['matteFps']:.2f} | {x['gateFps']:.2f} | {x['over16']} | {x['maxCallback']:.2f} |\n"
text+='\nThe strict callback budget '+('FAILED in the final measurements.\n'if any(x['over16']for x in rows)else'passed these steady windows; it is not a universal device guarantee.\n')
text+='\nThe final native hybrid Replace window still exceeds the callback budget; the\nraw report identifies each frame/source wait. The pre-taper run included a\n35ms alpha texture upload inside a 39ms frame callback, and 17ms source capture. These are measured\ncallback waits; this probe does not independently attribute driver/decoder CPU\nversus GPU time. The built-in mitigation already uses MP CPU at 15Hz. A hard\nmain-thread bound likely needs worker-owned compositing/capture transport; a\nlower gate rate alone is not established as a fix.\n\n## Leakage proxy and visual evidence\n\nThe reference is MP CPU confidence >=0.65 dilated by 24px in 720p source space.\nThe all-frame metric uses the chamfer distance field and 320x180 harness-only\nreadback. It counts alpha>0.5 outside this band, divided by outside-band pixels.\nThe first ten recurrent frames are excluded. It is a proxy, NOT ground truth:\nreal sleeves, hair or people missed by MP count as leakage. The separate\nfull-resolution saved-frame metric uses a 24px square dilation, so values can\ndiffer slightly. Production never reads alpha back for this measurement.\n\n'
for host,q in quality.items():
 text+=f'### {host}\n\n| Clip | Frames | RVM % | MP % | Hybrid % |\n|---|---:|---:|---:|---:|\n'
 for x in q['summary']:text+=f"| {x['clip']} | {x['frames']} | {x['rvm']:.4f} | {x['mediapipe']:.4f} | {x['hybrid']:.4f} |\n"
text+='''
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
'''
for x in failures:text+=f"| {x['host']} | {x['case']} | {x['expected']} | {x['actual']} | {x['pass']} | {x['violations']} |\n"
text+='\nExplicit MediaPipe is covered by both effect measurements. All forced faults\nare substituted in served test responses; no production fault flags were added.\n\n## Ten-minute soak\n\n'
if soak:text+='```json\n'+json.dumps(soak['summary'],indent=2)+'\n```\n'
text+='''
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
'''
native={x['mode']:x for x in rows if x['host']=='WKWebView' and x['matting']=='hybrid' and x['backend']=='webgpu'}
for mode in ['blur','replace']:
 if mode in native:
  text=text.replace('__'+mode.upper()+'_COUNT__',str(native[mode]['over16'])).replace('__'+mode.upper()+'_MAX__',f"{native[mode]['maxCallback']:.0f}")
text+='''
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
'''
Path('apps/desktop/dev/engine-demo/HYBRID-REPORT.md').write_text(text)
print(json.dumps({'performanceRows':len(rows),'fallbacks':len(failures),'qualityHosts':list(quality),'soak':bool(soak)},indent=2))
