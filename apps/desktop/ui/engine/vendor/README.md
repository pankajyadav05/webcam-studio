# Offline segmentation assets

Package: `@mediapipe/tasks-vision` **1.1.1** (Apache-2.0).
Downloaded with `npm pack @mediapipe/tasks-vision@1.1.1` outside the repository.
Tarball: https://registry.npmjs.org/@mediapipe/tasks-vision/-/tasks-vision-1.1.1.tgz
Upstream: https://github.com/google-ai-edge/mediapipe
License: https://www.apache.org/licenses/LICENSE-2.0.txt (included as `LICENSE`).

Model: MediaPipe Selfie Segmenter, float16, version 1 (Apache-2.0).
Requested source: https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_segmenter/float16/latest/selfie_segmenter.tflite
Pinned source: https://storage.googleapis.com/mediapipe-models/image_segmenter/selfie_segmenter/float16/1/selfie_segmenter.tflite
Model documentation: https://ai.google.dev/edge/mediapipe/solutions/vision/image_segmenter#selfie_segmentation_model

All assets are unmodified upstream bytes. There are no runtime CDN requests.
The engine dynamically imports the ESM bundle and the module WASM loader only
when an effect is first requested. It supplies the exported loader factory to
MediaPipe directly; no classic-script injection is necessary. The module loader
and its corresponding SIMD binary are the only WASM variant used. Safari 17+
(the supported minimum) supports WASM SIMD; the non-SIMD and classic loaders are
therefore not needed or shipped. GPU and CPU delegates use the same binary.
No source maps, CommonJS bundle, or unused task assets are bundled.

Per-file source URLs:
- `vision_bundle.mjs`: https://unpkg.com/@mediapipe/tasks-vision@1.1.1/vision_bundle.mjs
- `wasm/vision_wasm_module_internal.js`: https://unpkg.com/@mediapipe/tasks-vision@1.1.1/wasm/vision_wasm_module_internal.js
- `wasm/vision_wasm_module_internal.wasm`: https://unpkg.com/@mediapipe/tasks-vision@1.1.1/wasm/vision_wasm_module_internal.wasm
- `selfie_segmenter.tflite`: pinned model URL above
- `LICENSE`: Apache license URL above

SHA-256 covers every redistributed file below (this manifest excludes itself):

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `LICENSE` | 11358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` |
| `selfie_segmenter.tflite` | 249537 | `191ac9529ae506ee0beefa6b2c945a172dab9d07d1e802a290a4e4038226658b` |
| `vision_bundle.mjs` | 156139 | `a84921e02321e582484c1f8126d344b5f33b791fa09c640a52e20ee8632068f1` |
| `wasm/vision_wasm_module_internal.js` | 334166 | `539a4386fd12d0723d6e900ec80467d168ba7290dda2f79e77a2181b8e906115` |
| `wasm/vision_wasm_module_internal.wasm` | 13519940 | `3b57e4b55ac6346bf940212e3e11250b308a6a474077385e2f527b119cf74525` |

Redistributed asset total: **14,271,140 bytes** (excluding this manifest).
