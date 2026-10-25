import test from 'node:test';
import assert from 'node:assert/strict';
import { createRatioPolicy, RESET_GAP_MS, MIN_MATTE_FPS } from './rvm-policy.js';
const window = (p, ms) => { let result; for(let i=0;i<35;i++) result=p.sample(ms); return result; };
test('starts at .25 and excludes five warmup samples', () => {
  const p=createRatioPolicy(); for(let i=0;i<5;i++) assert.equal(p.sample(900),null);
  for(let i=0;i<29;i++) assert.equal(p.sample(12),null);
  assert.equal(p.sample(12),'higher'); assert.equal(p.ratio,.375);
});
test('higher resolution is measured then rolled back without oscillation', () => {
  const p=createRatioPolicy(); assert.equal(window(p,15),'higher');
  assert.equal(window(p,35),'lower'); assert.equal(p.ratio,.25);
  assert.equal(window(p,10),null); assert.equal(p.ratio,.25);
});
test('usable matte reuse and slow fallback have separate budgets', () => {
  const p=createRatioPolicy(); assert.equal(window(p,45),null);
  assert.equal(window(p,70),'slow'); assert.equal(MIN_MATTE_FPS,15); assert.equal(RESET_GAP_MS,1000);
});
test('invalid timings cannot alter the policy', () => {
  const p=createRatioPolicy(); for(const ms of [NaN,Infinity,-1,0]) assert.equal(p.sample(ms),null);
  assert.equal(p.ratio,.25);
});
