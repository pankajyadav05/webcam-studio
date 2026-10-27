import test from 'node:test';
import assert from 'node:assert/strict';
import { createShowTell } from './showtell.js';
import { createAutoFramer } from './autoframe.js';

// The same cases as crates/engine/src/showtell.rs.
const FACE = { x: 0.44, y: 0.25, width: 0.12, height: 0.2 };
const hand = (cx, cy, size) => ({ x: cx - size * 0.4, y: cy - size / 2, width: size * 0.8, height: size });
const feed = (show, hands, from, to) => { for (let t = from; t < to; t += 100) show.observe(hands, [FACE], t / 1000); };

test('a hand held up and still zooms in after a moment', () => {
  const show = createShowTell();
  feed(show, [hand(0.7, 0.4, 0.12)], 0, 600);
  assert.equal(show.focus(), null);
  feed(show, [hand(0.7, 0.4, 0.12)], 600, 900);
  const f = show.focus();
  assert.ok(f && Math.abs(f.cx - 0.7) < 0.1 && f.cy < 0.4 && f.height > 0.25);
});

test('waving, small, desk and chin-resting hands do not trigger it', () => {
  let show = createShowTell();
  for (let i = 0; i < 30; i++) show.observe([hand(i % 2 ? 0.8 : 0.7, 0.4, 0.15)], [FACE], i / 10);
  assert.equal(show.focus(), null);
  for (const h of [hand(0.7, 0.5, 0.06), hand(0.5, 0.92, 0.12), hand(0.5, 0.4, 0.12)]) {
    show = createShowTell(); feed(show, [h], 0, 2000); assert.equal(show.focus(), null);
  }
});

test('lowering the object goes back after a short wait', () => {
  const show = createShowTell();
  feed(show, [hand(0.7, 0.4, 0.15)], 0, 1000);
  assert.ok(show.focus());
  feed(show, [], 1000, 1500);
  assert.ok(show.focus());
  feed(show, [], 1500, 2000);
  assert.equal(show.focus(), null);
});

test('the framer zooms to the object and hands the manual framing back exactly', () => {
  const framer = createAutoFramer(), manual = { zoom: 1, pan_x: 0, pan_y: 0, mirror: false };
  framer.setAim(false, null);
  assert.deepEqual(framer.step(manual, [1280, 720], [1280, 720], 0), manual);
  assert.equal(framer.busy(), false);
  framer.setAim(false, { cx: 0.7, cy: 0.5, width: 0.3, height: 0.5 });
  let f;
  for (let i = 0; i < 90; i++) f = framer.step(manual, [1280, 720], [1280, 720], i / 30);
  assert.ok(Math.abs(f.zoom - 2) < 0.05, `${f.zoom}`);
  framer.setAim(false, null);
  for (let i = 90; i < 240; i++) f = framer.step(manual, [1280, 720], [1280, 720], i / 30);
  assert.deepEqual(f, manual);
  assert.equal(framer.busy(), false);
});
