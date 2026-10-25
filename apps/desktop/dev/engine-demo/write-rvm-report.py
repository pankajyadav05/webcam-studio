from pathlib import Path
import json
import statistics as st
import hashlib

out = Path('/tmp/engine-shots/v3')
dev = Path(__file__).resolve().parent
mean = lambda a: sum(a)/len(a)
rows = []
for host in ['chrome', 'webkit']:
    for backend in ['webgpu', 'wasm']:
        for mode in ['blur', 'replace']:
            file = out/f'{host}-{backend}-{mode}{"-report" if host == "webkit" else ""}.json'
            r = json.loads(file.read_text())
            samples = r['samples']
            assert len(samples) >= 20
            assert all(x['matting'] == 'rvm' and x['backend'] == backend for x in samples)
            assert not r['violations']
            if host == 'chrome':
                a, b, c, d = r['callbacksSummary'], r['maskCallbacksSummary'], r['sourceCallbacksSummary'], r['transferCallbacksSummary']
                over = a['over16'] + b['over16'] + c['over16'] + d['over16']
                assert not r['errors']
            else:
                a, b, c, d = r['frameCallbacks'], r['maskCallbacks'], r['sourceCallbacks'], r['transferCallbacks']
                over = len([x for x in r['overBudget'] if x['phase'] == 'steady'])
                assert b['max'] <= 16
                assert all(x.startswith('tauri://localhost/') for x in r['nativeRequests'])
            rows.append(dict(host=host, backend=backend, mode=mode,
                fps=mean([x['fps'] for x in samples]), inferMs=mean([x['inferMs'] for x in samples]),
                inferFps=mean([x['inferFps'] for x in samples]), over16=over,
                maxMs=max(a['max'], b['max'], c['max'], d['max']), p95Ms=a['p95'], samples=len(samples),
                ratio=sorted(set(x['ratio'] for x in samples))))
(out/'measurements.json').write_text(json.dumps(rows, indent=2))
profiles=[]
for mode in ['blur','replace']:
    r=json.loads((out/f'webkit-webgpu-{mode}-report.json').read_text())
    profiles.append({'mode':mode, **{k:mean([x[k] for x in r['samples']]) for k in ['inferMs','preprocessMs','snapshotMs','renderMs','cameraUploadMs','maskUploadMs','presentMs']},'transferP95Ms':r['transferCallbacks']['p95']})
soak = json.loads((out/'soak.json').read_text())
assert soak['summary']['seconds'] >= 600
assert not soak['errors'] and not soak['violations']
parity = json.loads((out/'parity.json').read_text())
assert len(parity) == 12 and max(x['mae'] for x in parity) < .00001
fails = ['gpu-fail', 'worker-fail', 'runtime-worker-fail', 'model-fail', 'device-lost', 'slow', 'mediapipe']
for kind in fails:
    r = json.loads((out/f'chrome-{kind}-blur.json').read_text())
    assert not r['violations']
    assert r['stats']['matting'] == 'mediapipe'
model = dev.parents[1]/'ui/engine/vendor/rvm/rvm_mobilenetv3_fp32.onnx'
vendor = model.parent.parent
size = sum(f.stat().st_size for d in ['ort', 'rvm'] for f in (vendor/d).iterdir() if f.is_file())
text = '''# RVM integration verification

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
'''
for r in rows:
    text += f'| {r["host"]} | {r["mode"]} | {r["backend"]} | {r["fps"]:.2f} | {r["inferMs"]:.2f} | {r["inferFps"]:.2f} | {r["over16"]} | {r["maxMs"]:.2f} | {r["p95Ms"]:.2f} | {r["ratio"]} |\n'
text += '''
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
'''
text += '\nNative GPU profile, means of per-second stats (ms; worker and main-thread work overlap):\n\n| Mode | GPU inference/alpha | Worker prep | Capture | Render | Camera upload | Alpha upload | Present | Transfer p95 |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n'
for v in profiles:
    text += '| '+v['mode']+' | '+' | '.join(f'{v[k]:.2f}' for k in ['inferMs','preprocessMs','snapshotMs','renderMs','cameraUploadMs','maskUploadMs','presentMs','transferP95Ms'])+' |\n'
text += '\nThese timings locate the engine cost, but do not independently trace WebKit decoder/frame-scheduling waits. The remaining fractional FPS shortfall cannot be attributed more precisely from callback timings alone. The chosen production policy keeps .25 on native WebKit, reuses the latest alpha, and preserves soft edges.\n'
text += f'\nAcross 12 recurrent public curly frames, GPU versus the **original upstream** WASM model: maximum per-frame MAE {max(x["mae"] for x in parity):.9g}; maximum pixel difference {max(x["maxError"] for x in parity):.9g}. See parity.json.\n'
text += '''
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

'''
text += '```json\n'+json.dumps(soak['summary'],indent=2)+'\n```\n'
text += f'\nRSS after engine disposal and a two-second wait: {soak["afterDisposeMiB"]:.2f} MiB. Browser allocators may retain freed resources.\n'
text += '''
Read the first/last-minute medians as allocator plateau evidence, not proof that
no browser allocation ever occurs. GPU input/pixel storage is reused and GPU
buffers/textures/session resources are explicitly released on disposal. Bitmap,
tensor and command wrappers still allocate per frame, and forced WASM readback
uses getImageData. The literal zero-allocation requirement is not achieved.

## Distribution and integration

'''
text += f'Added vendor footprint: **{size:,} bytes** ({size/1024/1024:.2f} MiB), including runtime, model, licenses, notices and model notes/script. Existing MediaPipe assets remain. Model bytes: {model.stat().st_size:,}; bundled SHA-256 `{hashlib.sha256(model.read_bytes()).hexdigest()}`.\n'
text += '''
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
'''
(dev/'RVM-REPORT.md').write_text(text)
print(json.dumps(rows, indent=2))
