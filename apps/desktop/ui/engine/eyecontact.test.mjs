import test from 'node:test';
import assert from 'node:assert/strict';
import { parseEyeMaps, eyeBox, EYE_MAP } from './eyecontact.js';

const per = 5 + EYE_MAP.width * EYE_MAP.height * 3;

test('the maps from Rust are read eye by eye', () => {
  const f = new Float32Array(3 + 2 * per);
  f.set([1280, 720, 2]);
  f.set([300, 500, 40, 60, 0], 3);
  f[3 + 5] = 0.1; f[3 + 6] = -0.2; f[3 + 7] = 0.9;
  f.set([302, 700, 40, 60, 1], 3 + per);
  const out = parseEyeMaps(f.buffer);
  assert.equal(out.frameWidth, 1280);
  assert.equal(out.eyes.length, 2);
  assert.deepEqual([out.eyes[0].top, out.eyes[0].left, out.eyes[0].mirrored, out.eyes[1].mirrored], [300, 500, false, true]);
  // The mirrored box at 700..760 in the mirrored picture is at 520..580 in the real one.
  assert.equal(out.eyes[1].left, 1280 - 700 - 60);
  assert.deepEqual(Array.from(out.eyes[0].maps.slice(0, 4)).map((v) => +v.toFixed(3)), [0.1, -0.2, 0.9, 0]);
});

test('no eyes, or a short answer, is nothing to draw', () => {
  assert.equal(parseEyeMaps(new Float32Array([1280, 720, 0]).buffer), null);
  assert.equal(parseEyeMaps(new Float32Array([1280, 720, 2, 1]).buffer), null);
});

test('an eye box is placed in the captured part of the video', () => {
  const box = eyeBox({ top: 100, left: 200, height: 20, width: 40 }, 2, { x: 100, y: 50, width: 1000, height: 500 });
  assert.deepEqual(box, [0.3, 0.3, 0.08, 0.08]);
});
