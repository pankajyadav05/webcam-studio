#!/bin/sh
set -eu
if [ "${HYBRID_FAILURES_ONLY:-0}" != 1 ]; then
for model in webgpu rvm mediapipe wasm; do
 for mode in blur replace; do
  node apps/desktop/dev/engine-demo/verify-hybrid.mjs "$model" 25 "$mode" > "/tmp/engine-shots/v4/final-chrome-$model-$mode.log" 2>&1
 done
done
fi
for kind in gpu-fail worker-fail runtime-worker-fail model-fail device-lost slow gate-fail gate-crash; do
 node apps/desktop/dev/engine-demo/verify-hybrid.mjs "$kind" 8 replace > "/tmp/engine-shots/v4/final-chrome-$kind.log" 2>&1
 done
