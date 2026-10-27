import test from 'node:test';
import assert from 'node:assert/strict';
import { LOOK, lightGain, lightCurve, lookParams, normalizeAppearance, isLookActive, createLightMeter } from './look.js';

test('appearance values are cleaned and off by default', () => {
  const a = normalizeAppearance({ smoothing: 3, warmth: 'x', low_light: 'yes' });
  assert.deepEqual(a, { smoothing: 1, low_light: false, warmth: 0, studio_light: 0, eye_contact: false });
  assert.equal(isLookActive(normalizeAppearance({ eye_contact: true })), true);
  assert.equal(isLookActive(normalizeAppearance()), false);
  assert.equal(isLookActive({ smoothing: 0, low_light: false, warmth: -0.3 }), true);
});

test('low light lifts only dark pictures, and never past the limit', () => {
  assert.equal(lightGain(0.6), 1);
  assert.ok(lightGain(0.2) > 2);
  assert.equal(lightGain(0), LOOK.lightMaxGain);
  assert.equal(lightCurve(0, 2), 0);
  assert.ok(Math.abs(lightCurve(1, 2) - 1) < 1e-9);
  assert.ok(lightCurve(0.2, 2) > 0.2);
});

test('the gain eases in over time instead of jumping', () => {
  const meter = createLightMeter();
  const first = meter.update(0.15, 0);
  assert.ok(first > 1 && first < 1.2, `${first}`);
  let gain = first;
  for (let i = 1; i <= 90; i++) gain = meter.update(0.15, i / 30);
  assert.ok(Math.abs(gain - lightGain(0.15)) < 0.05, `${gain}`);
});

test('shader settings follow the strength', () => {
  const off = lookParams({ smoothing: 0, low_light: false, warmth: 0 }, 2);
  assert.equal(off.smooth, false);
  assert.equal(off.gain, 1);
  const full = lookParams({ smoothing: 1, low_light: true, warmth: 1 }, 2);
  assert.ok(Math.abs(full.epsilon - LOOK.smoothSigma[1] ** 2) < 1e-9);
  assert.ok(Math.abs(full.mix - LOOK.smoothMix[1]) < 1e-9);
  assert.equal(full.gain, 2);
  assert.equal(full.warmth, LOOK.warmthShift);
});

test('studio light sits on the face, eases, and fades without one', async () => {
  const { createStudioLight, spotInOutput } = await import('./look.js');
  const face = (cx, cy, h) => ({ x: cx - h * 0.28, y: cy - h / 2, width: h * 0.56, height: h });
  const light = createStudioLight();
  assert.equal(light.step(0), null);
  light.observe([face(0.3, 0.4, 0.2)], 16 / 9, 0);
  const first = light.step(1 / 30);
  assert.ok(Math.abs(first.spot.cx - 0.3) < 1e-6 && first.spot.cy > 0.4);
  assert.ok(first.presence > 0 && first.presence < 0.2);
  light.observe([face(0.7, 0.4, 0.2)], 16 / 9, 2 / 30);
  const moving = light.step(2 / 30);
  assert.ok(moving.spot.cx > 0.3 && moving.spot.cx < 0.5);
  let last;
  for (let i = 3; i < 90; i++) last = light.step(i / 30);
  assert.ok(last.presence < 0.05);
  const out = spotInOutput({ cx: 0.2, cy: 0.5, rx: 0.1, ry: 0.2 }, [1280, 720], { x: 0, y: 0, width: 640, height: 360 }, [1280, 720], true);
  assert.deepEqual(out, { cx: 768, cy: 720, rx: 256, ry: 288 });
});
