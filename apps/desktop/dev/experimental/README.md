# Parked matting experiments

Nothing in this directory ships in the desktop app (Tauri bundles `ui/`).

`engine/legacy-engine.js` preserves the pre-rewrite MediaPipe/RVM/hybrid engine.
RVM produced a white-wall plume in WKWebView and took seconds to warm up on the
maintainer's machine. The hybrid gate added translucent ghosting. The production
engine uses Apple Vision, with MediaPipe as its offline browser/platform fallback.

RVM's model and ONNX runtime are under `engine/vendor/`. MediaPipe assets remain
under `ui/engine/vendor/`; the legacy snapshot needs that directory served as its
`vendor/` too if revived. The pure RVM policy and hybrid gate tests remain runnable:
`node --test apps/desktop/dev/experimental/engine/rvm-policy.test.mjs`
`node --test apps/desktop/dev/experimental/engine/hybrid-gate.test.mjs`
Do not import these experiments from the shipped UI.
