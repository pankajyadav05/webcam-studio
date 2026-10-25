#!/bin/sh
# Run from the repository root, without other benchmark browsers running.
set -eu
mkdir -p /tmp/engine-shots/v3
swiftc apps/desktop/dev/engine-demo/verify-webkit.swift -o /tmp/verify-webkit-v3
/tmp/verify-webkit-v3 --asset --v3 --clip > /tmp/engine-shots/v3/webkit-webgpu-blur.log 2>&1
/tmp/verify-webkit-v3 --asset --v3 --clip --replace > /tmp/engine-shots/v3/webkit-webgpu-replace.log 2>&1
/tmp/verify-webkit-v3 --asset --v3 --clip --wasm --keep-slow > /tmp/engine-shots/v3/webkit-wasm-blur.log 2>&1
/tmp/verify-webkit-v3 --asset --v3 --clip --wasm --keep-slow --replace > /tmp/engine-shots/v3/webkit-wasm-replace.log 2>&1
