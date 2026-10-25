import test from 'node:test';import assert from 'node:assert/strict';
import {encodeFrame,decodeMask} from './protocol.js';import {ProviderControl} from './control.js';
test('binary protocol identity, shape and row order',()=>{
 const request=encodeFrame(9,new Uint8Array([1,2]).buffer);assert.equal(request.byteLength,18);assert.equal(request[0],87);
 const out=new ArrayBuffer(24),v=new DataView(out);v.setUint32(0,0x314d5357,true);v.setUint32(4,9,true);v.setUint32(8,2,true);v.setUint32(12,2,true);new Uint8Array(out,20).set([0,64,128,255]);
 assert.deepEqual([...decodeMask(out,9).alpha],[0,64,128,255]);assert.throws(()=>decodeMask(out,10));assert.throws(()=>decodeMask(out.slice(1),9));
});
test('no throttle: accepts every distinct 30fps frame when inference fits',async()=>{
 let now=0,count=0;const control=new ProviderControl({now:()=>now});
 for(let i=0;i<30;i++){now=i*1000/30;await control.run(async()=>{count++;now+=18;});}
 assert.equal(count,30);
});
test('stuck provider times out, clears busy and retries after backoff',async()=>{
 let now=0,aborted=0;const control=new ProviderControl({timeoutMs:10,now:()=>now});
 await assert.rejects(control.run(()=>new Promise(()=>{}),()=>aborted++),/timed out/);
 assert.equal(control.busy,false);assert.equal(aborted,1);assert.equal(control.ready,false);
 now=251;assert.equal(await control.run(()=>42),42);assert.equal(control.ready,true);
});
test('preprocessing errors and cancellation cannot leave busy stuck',async()=>{
 const control=new ProviderControl();await assert.rejects(control.run(()=>{throw Error('bitmap failed');}));assert.equal(control.busy,false);
 control.retryAt=0;let finish;const pending=control.run(()=>new Promise(resolve=>finish=resolve));await Promise.resolve();control.cancel();finish(5);assert.equal(await pending,null);assert.equal(control.busy,false);
});
