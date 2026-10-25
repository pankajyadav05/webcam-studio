#!/bin/sh
set -eu
mkdir -p /tmp/engine-shots/v4
swiftc apps/desktop/dev/engine-demo/verify-hybrid-webkit.swift -o /tmp/verify-hybrid-webkit
for model in hybrid rvm mediapipe; do
  for mode in blur replace; do
    if [ "$mode" = replace ]; then
      /tmp/verify-hybrid-webkit --asset --clip --replace "--$model" > "/tmp/engine-shots/v4/native-$model-$mode.log" 2>&1
    else
      /tmp/verify-hybrid-webkit --asset --clip "--$model" > "/tmp/engine-shots/v4/native-$model-$mode.log" 2>&1
    fi
  done
done
for mode in blur replace; do
  if [ "$mode" = replace ]; then
    /tmp/verify-hybrid-webkit --asset --clip --wasm --keep-slow --replace > /tmp/engine-shots/v4/native-wasm-replace.log 2>&1
  else
    /tmp/verify-hybrid-webkit --asset --clip --wasm --keep-slow > /tmp/engine-shots/v4/native-wasm-blur.log 2>&1
  fi
done
for kind in gate-fail rvm-fail; do
 /tmp/verify-hybrid-webkit --asset --clip --replace "--case=$kind" > "/tmp/engine-shots/v4/native-$kind.log" 2>&1
done
