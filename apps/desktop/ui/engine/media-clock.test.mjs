import test from 'node:test';import assert from 'node:assert/strict';import {MediaClock} from './media-clock.js';
test('real WKWebView camera: zero mediaTime does not stop inference',()=>{
 const clock=new MediaClock();let frames=0,last=-1;
 for(let i=0;i<30;i++){const time=clock.advance(i*1000/30,{mediaTime:0,presentedFrames:i},0);assert.ok(time>last);last=time;frames++;}
 assert.equal(frames,30);assert.ok(Math.abs(last-29/30)<1e-8);
});
test('media time deduplicates rAF and seeks/loops keep inference monotonic',()=>{
 const clock=new MediaClock();assert.equal(clock.advance(0,{},1),1);assert.equal(clock.advance(16,{},1),null);
 assert.ok(clock.advance(33,{},1+1/30)>1);assert.ok(clock.advance(66,{},0)>1);
 clock.reset();assert.equal(clock.advance(100,{},0),0);
});
