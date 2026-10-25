import test from 'node:test';
import assert from 'node:assert/strict';
import { cropRect, OUTPUT_DEFAULT } from './studio-engine.js';
const close = (actual, expected) => {
  for (const key of ['x', 'y', 'width', 'height']) assert.ok(Math.abs(actual[key]-expected[key]) < 1e-8, `${key}: ${actual[key]} vs ${expected[key]}`);
};
test('default output', () => assert.deepEqual(OUTPUT_DEFAULT, { width:1280, height:720 }));
for (const [name, iw, ih, ow, oh, rect] of [
  ['equal',1920,1080,1280,720,{x:0,y:0,width:1920,height:1080}],
  ['wide input',2000,1000,1000,1000,{x:500,y:0,width:1000,height:1000}],
  ['tall input',1000,2000,1000,1000,{x:0,y:500,width:1000,height:1000}],
  ['portrait output',1920,1080,720,1280,{x:656.25,y:0,width:607.5,height:1080}],
]) test(name, () => close(cropRect(iw,ih,ow,oh,{}),rect));
for (const zoom of [1,2,4]) for (const pan of [-1,0,1]) test(`zoom ${zoom}, pan ${pan}`, () => {
  const width=1920/zoom,height=1080/zoom;
  close(cropRect(1920,1080,1280,720,{zoom,pan_x:pan,pan_y:pan}), {x:(1920-width)*(pan+1)/2,y:(1080-height)*(pan+1)/2,width,height});
});
test('clamps limits', () => {
  close(cropRect(1920,1080,1280,720,{zoom:20,pan_x:4,pan_y:-4}),{x:1440,y:0,width:480,height:270});
  close(cropRect(1920,1080,1280,720,{zoom:-1}),{x:0,y:0,width:1920,height:1080});
});
test('NaN and infinite values use defaults', () => {
  for (const value of [NaN, Infinity, undefined, 'invalid']) close(cropRect(1920,1080,1280,720,{zoom:value,pan_x:value,pan_y:value}),{x:0,y:0,width:1920,height:1080});
});
test('mirror does not affect the source rectangle', () => assert.deepEqual(cropRect(1280,720,640,480,{zoom:2,pan_x:.4,mirror:true}),cropRect(1280,720,640,480,{zoom:2,pan_x:.4,mirror:false})));
test('unavailable source and output sizes', () => {
  close(cropRect(0,0,1280,720,{}),{x:0,y:0,width:0,height:0});
  close(cropRect(1920,1080,0,0,{}),{x:0,y:0,width:1920,height:1080});
  close(cropRect(NaN,1080,1280,720,{}),{x:0,y:0,width:0,height:0});
});

test('exports remain limited to the agreed public API', async () => {
  assert.deepEqual(Object.keys(await import('./studio-engine.js')).sort(),['OUTPUT_DEFAULT','createStudioEngine','cropRect']);
});
