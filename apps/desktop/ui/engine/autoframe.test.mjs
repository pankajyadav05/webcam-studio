import test from 'node:test';
import assert from 'node:assert/strict';
import { AUTO, createAutoFramer } from './autoframe.js';

// The same cases as the tests in crates/engine/src/autoframe.rs.
const IN = [1280, 720], OUT = [1280, 720];
const face = (cx, cy, h) => ({ x: cx - h * 0.25, y: cy - h / 2, width: h * 0.5, height: h });

function run(framer, faces, from, seconds) {
  let framing = { zoom: 1, pan_x: 0, pan_y: 0 }, t = from;
  for (let i = 0; i < seconds * 30; i++) {
    t = from + i / 30;
    if (i % 3 === 0) framer.observe(faces, IN, OUT, t);
    framing = framer.step({ zoom: 1, pan_x: 0, pan_y: 0 }, IN, OUT, t);
  }
  return [framing, t];
}

function centre(f) {
  const width = 1280 / f.zoom, height = 720 / f.zoom;
  const x = (1280 - width) * (f.pan_x + 1) / 2, y = (720 - height) * (f.pan_y + 1) / 2;
  return { cx: (x + width / 2) / 1280, cy: (y + height / 2) / 720, h: height / 720, x: x / 1280, right: (x + width) / 1280 };
}

test('a person off to one side ends up framed in the middle', () => {
  const [f] = run(createAutoFramer(), [face(0.6, 0.45, 0.2)], 0, 4);
  assert.ok(f.zoom > 1.3, `zoom ${f.zoom}`);
  const c = centre(f);
  assert.ok(Math.abs(c.cx - 0.6) < 0.02, `cx ${c.cx}`);
  const faceLine = (0.45 - (c.cy - c.h / 2)) / c.h;
  assert.ok(Math.abs(faceLine - AUTO.faceLine) < 0.03, `face line ${faceLine}`);
});

test('the move is smooth and starts from the framing in use', () => {
  const framer = createAutoFramer();
  framer.observe([face(0.7, 0.4, 0.2)], IN, OUT, 0);
  let last = framer.step({ zoom: 1, pan_x: 0, pan_y: 0 }, IN, OUT, 0).zoom;
  assert.equal(last, 1);
  for (let i = 1; i < 30; i++) {
    const z = framer.step({ zoom: 1, pan_x: 0, pan_y: 0 }, IN, OUT, i / 30).zoom;
    assert.ok(z - last < 0.08 && z >= last - 1e-4);
    last = z;
  }
});

test('small moves are ignored and big ones followed', () => {
  const framer = createAutoFramer();
  let [settled, t] = run(framer, [face(0.5, 0.4, 0.2)], 0, 4);
  let still;
  [still, t] = run(framer, [face(0.51, 0.41, 0.205)], t, 2);
  for (const key of ['cx', 'cy', 'h']) assert.ok(Math.abs(centre(still)[key] - centre(settled)[key]) < 0.002);
  const [moved] = run(framer, [face(0.6, 0.4, 0.2)], t, 4);
  assert.ok(Math.abs(centre(moved).cx - 0.6) < 0.02);
});

test('two people are both in the picture', () => {
  const [f] = run(createAutoFramer(), [face(0.3, 0.4, 0.18), face(0.7, 0.42, 0.17)], 0, 4);
  const c = centre(f);
  assert.ok(c.x < 0.3 - 0.045 && c.right > 0.7 + 0.045);
});

test('a tiny face never zooms past the limit', () => {
  const [f] = run(createAutoFramer(), [face(0.5, 0.5, 0.03)], 0, 5);
  assert.ok(Math.abs(f.zoom - AUTO.maxZoom) < 0.01, `${f.zoom}`);
});

test('with no face for a while it goes back to the full picture', () => {
  const framer = createAutoFramer();
  let [zoomed, t] = run(framer, [face(0.6, 0.4, 0.2)], 0, 4);
  let held;
  [held, t] = run(framer, [], t, 1.5);
  assert.ok(Math.abs(held.zoom - zoomed.zoom) < 0.01);
  const [back] = run(framer, [], t, 4);
  assert.ok(Math.abs(back.zoom - 1) < 0.01, `${back.zoom}`);
});
