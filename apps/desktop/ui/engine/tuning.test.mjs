import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeTuning,tuningParams,TUNING_DEFAULTS} from './tuning.js';
import {createMatteFilter,temporalBlend} from './matte.js';
import {createStudioEngine} from './studio-engine.js';

test('default uniforms exactly equal the shipped chair-wedge curve and guided filter', () => {
  assert.deepEqual(tuningParams(),{curveLow:.22,curveHigh:.88,curveMix:.85,radius:6,epsilon:.001,stillMs:80});
  assert.deepEqual(tuningParams(TUNING_DEFAULTS),tuningParams());
});
test('slider endpoints, piecewise interpolation, finite values and clamping', () => {
  assert.deepEqual(tuningParams({cutoff:0,softness:0,steadiness:0}),{curveLow:.025,curveHigh:.975,curveMix:.25,radius:2,epsilon:.0002,stillMs:20});
  assert.deepEqual(tuningParams({cutoff:1,softness:1,steadiness:1}),{curveLow:.4,curveHigh:.8,curveMix:1,radius:12,epsilon:.008,stillMs:240});
  assert.deepEqual(normalizeTuning({cutoff:-4,softness:4,steadiness:NaN}),{cutoff:0,softness:1,steadiness:.5});
  for(const value of [null,undefined,Infinity,'0.1',{},NaN]) assert.deepEqual(normalizeTuning({cutoff:value,softness:value,steadiness:value}),TUNING_DEFAULTS);
  assert.equal(tuningParams({steadiness:.25}).stillMs,50);
});
test('live steadiness preserves matte history and responds more slowly', () => {
  const run = steadiness => {
    const f=createMatteFilter(1,1,{hysteresis:false}); f.apply([.5]); f.setTuning({steadiness});
    return f.apply([.53],16)[0];
  };
  assert.ok(run(0)>run(.5)); assert.ok(run(.5)>run(1));
  assert.equal(temporalBlend(.02,16),1-Math.exp(-16/80));
});
test('runtime API changes preserve frame ring and native provider; rejects invalid enums', async () => {
  const old=globalThis.document, raf=globalThis.requestAnimationFrame, caf=globalThis.cancelAnimationFrame;
  globalThis.requestAnimationFrame=()=>1;globalThis.cancelAnimationFrame=()=>{};
  globalThis.document={hidden:false,addEventListener(){},removeEventListener(){}};
  let created=0,disposed=0;
  const provider={name:'native',backend:'apple-vision',ready:true,quality:'balanced',dispose(){disposed++;}};
  const canvas={getContext:()=>({fillRect(){}})};
  const video={readyState:0,addEventListener(){},removeEventListener(){},requestVideoFrameCallback:()=>1,cancelVideoFrameCallback(){}};
  try {
    const engine=createStudioEngine({canvas,debug:true,invoke:async()=>({available:true}),providerFactory:()=>{created++;return provider;}});
    engine.setSource(video); engine.setBackground({mode:'blur'}); engine.start();
    await new Promise(resolve=>setImmediate(resolve));
    const ring=engine.getMattingDebug().ring,epoch=ring.epoch;
    ring.add({close(){}},0,0,{}); const count=ring.frames.size;
    engine.setTuning({cutoff:1});engine.setQuality('accurate');engine.setView('mask');
    assert.equal(engine.getMattingDebug().ring.epoch,epoch); assert.equal(ring.frames.size,count);
    assert.equal(provider.quality,'accurate');assert.equal(created,1);assert.equal(disposed,0);
    assert.throws(()=>engine.setQuality('bad'),RangeError);assert.throws(()=>engine.setView('bad'),RangeError);
    engine.dispose();
  } finally {globalThis.document=old;globalThis.requestAnimationFrame=raf;globalThis.cancelAnimationFrame=caf;}
});
