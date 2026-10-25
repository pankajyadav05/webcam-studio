import test from 'node:test';import assert from 'node:assert/strict';
import {FrameRing} from './frame-ring.js';
test('late mask paints exactly its captured camera frame',()=>{
 const ring=new FrameRing();const a=ring.add({},1,0);ring.add({},1+1/30,33);ring.add({},1+2/30,66);
 assert.equal(ring.align({id:a.id,epoch:a.epoch}),a);
 ring.observeLatency(65);assert.equal(ring.depth,3);
 ring.clear();assert.equal(ring.align({id:a.id,epoch:a.epoch}),null);
});
test('bounded ring closes stale captures and preserves pinned display',()=>{
 let closed=0;const ring=new FrameRing({maxDepth:3});const a=ring.add({close(){closed++;}},0,0);ring.pinned=a.id;
 for(let i=1;i<10;i++)ring.add({close(){closed++;}},i,i*33);
 assert.equal(ring.frames.size,3);assert.equal(ring.align(a),a);assert.equal(closed,7);
 ring.clear();assert.equal(closed,10);
});

test('backpressure consumes the newest retained frame immediately without a stale queue',()=>{
 const ring=new FrameRing();const first=ring.add({},0,0);ring.add({},1/30,33);const newest=ring.add({},2/30,66);
 assert.equal(ring.latest(first.id),newest);assert.equal(ring.latest(newest.id),null);
 ring.clear();assert.equal(ring.latest(),null);
});

test('reset closes owned immutable bitmaps and captures exactly once',()=>{
 let bitmapClosed=0,captureClosed=0;const ring=new FrameRing();
 const frame=ring.add({close(){captureClosed++;}},0,0);frame.bitmap={close(){bitmapClosed++;}};
 ring.clear();ring.clear();assert.equal(bitmapClosed,1);assert.equal(captureClosed,1);
});
