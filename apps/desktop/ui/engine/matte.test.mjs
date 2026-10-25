import test from 'node:test';
import assert from 'node:assert/strict';
import {createMatteFilter,temporalBlend} from './matte.js';
test('shoulder confidence crosses 0.65 continuously at realistic update intervals',()=>{
  for(const elapsed of [33.333,40,66.667]) {
    const filter=createMatteFilter(8,8), values=new Float32Array(64).fill(.62);
    const before=filter.apply(values,elapsed)[0]; values.fill(.655);
    const after=filter.apply(values,elapsed)[0];
    assert.ok(after>=before && after-before<12,`${elapsed}: ${before}->${after}`);
  }
});
test('isolated fingers fade continuously; no per-frame keep/drop',()=>{
  const filter=createMatteFilter(8,8), values=new Float32Array(64);values[27]=.8;
  const before=filter.apply(values)[27];values[27]=.79;
  const after=filter.apply(values)[27];assert.ok(before>150 && Math.abs(after-before)<5);
});
test('80ms still-region smoothing and smooth motion ramp',()=>{
  assert.ok(Math.abs(temporalBlend(.01,33.333)-(1-Math.exp(-33.333/80)))<1e-6);
  assert.ok(temporalBlend(.5,33.333)>.999);
  let last=0;for(let d=0;d<=.25;d+=.001){const b=temporalBlend(d,33.333);assert.ok(b>=last);last=b;}
});
test('motion responds in one inference; reset cannot retain the old silhouette',()=>{
 const filter=createMatteFilter(2,1,{hysteresis:false});filter.apply(new Float32Array([1,0]));
 assert.deepEqual([...filter.apply(new Float32Array([0,1]),33.333)],[0,255]);
 filter.reset();assert.deepEqual([...filter.apply(new Float32Array([0,0]))],[0,0]);
});
// A 4x4 mask with a 1x1 luma grid: the whole picture is one motion cell.
const lumaOf = value => ({ width:1, height:1, data:new Uint8Array([value]) });
test('a still picture holds back a mask flip the model makes on its own',()=>{
  const filter=createMatteFilter(4,4,{hysteresis:false}), off=new Float32Array(16), on=new Float32Array(16).fill(1);
  filter.apply(off,33.333,true,lumaOf(120)); filter.apply(off,33.333,false,lumaOf(120));
  const flicker=filter.apply(on,33.333,false,lumaOf(120))[0];
  assert.ok(flicker<12,`one-frame flip leaked ${flicker}`);
  // If the model keeps insisting, the area joins slowly; when it lets go, it leaves quickly.
  let held=0;for(let i=0;i<15;i++)held=filter.apply(on,33.333,false,lumaOf(120))[0];
  assert.ok(held>12 && held<128,`sustained flip after 0.5 s: ${held}`);
  let gone=held;for(let i=0;i<8;i++)gone=filter.apply(off,33.333,false,lumaOf(120))[0];
  assert.ok(gone<held*.45,`release ${held}->${gone}`);
});
test('a moving picture lets the mask follow in one inference',()=>{
  const filter=createMatteFilter(4,4,{hysteresis:false}), off=new Float32Array(16), on=new Float32Array(16).fill(1);
  filter.apply(off,33.333,true,lumaOf(60)); filter.apply(off,33.333,false,lumaOf(60));
  assert.ok(filter.apply(on,33.333,false,lumaOf(160))[0]>250);
});
test('reset forgets the previous picture, so the first mask is taken as is',()=>{
  const filter=createMatteFilter(4,4,{hysteresis:false});
  filter.apply(new Float32Array(16),33.333,true,lumaOf(90));
  assert.equal(filter.apply(new Float32Array(16).fill(1),33.333,true,lumaOf(90))[0],255);
});
