#!/bin/sh
# Start ENGINE_DEMO_PORT=4180 node .../server.mjs first. Run without other benchmarks.
set -eu
for backend in webgpu wasm; do
  for mode in blur replace; do
    node apps/desktop/dev/engine-demo/verify-rvm.mjs "$backend" 25 "$mode" > "/tmp/engine-shots/v3/chrome-$backend-$mode.log" 2>&1
  done
done
for kind in gpu-fail worker-fail runtime-worker-fail model-fail device-lost slow mediapipe; do
  node apps/desktop/dev/engine-demo/verify-rvm.mjs "$kind" 5 blur > "/tmp/engine-shots/v3/chrome-$kind-blur.log" 2>&1
done
node apps/desktop/dev/engine-demo/verify-rvm-transform.mjs > /tmp/engine-shots/v3/transforms.log 2>&1
node apps/desktop/dev/engine-demo/verify-rvm-parity.mjs > /tmp/engine-shots/v3/parity.log 2>&1
node apps/desktop/dev/engine-demo/verify-rvm-quality.mjs 025 > /tmp/engine-shots/v3/quality025.log 2>&1
