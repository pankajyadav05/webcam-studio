import test from 'node:test';import assert from 'node:assert/strict';import {MediaPipeProvider} from './mediapipe.js';
test('stuck worker initialization terminates it and clears in-flight state',async()=>{
 const original=globalThis.Worker;let terminated=0;
 globalThis.Worker=class{postMessage(){} terminate(){terminated++;}};
 try{
  const provider=new MediaPipeProvider();provider.control.timeoutMs=10;
  await assert.rejects(provider.segment({id:1,epoch:0,mediaTime:0,image:{}}),/timed out/);
  assert.equal(provider.control.busy,false);assert.equal(provider.worker,null);assert.equal(terminated,1);
  provider.dispose();
 }finally{globalThis.Worker=original;}
});

test('GPU inference failure retries the actual protocol with a CPU delegate',async()=>{
 const originalWorker=globalThis.Worker,originalBitmap=globalThis.createImageBitmap,modes=[],timestamps=[];
 globalThis.createImageBitmap=async()=>({close(){}});
 globalThis.Worker=class{
  postMessage(data){
   if(data.type==='init'){
    modes.push(data.delegate);this.cpu=data.delegate==='CPU';
    queueMicrotask(()=>this.onmessage({data:{type:'ready',backend:this.cpu?'cpu':'gpu'}}));
   }else if(data.type==='frame'){
    timestamps.push(data.timestamp);
    queueMicrotask(()=>this.onmessage({data:this.cpu?{type:'mask',id:data.id,epoch:data.epoch,width:1,height:1,values:new Float32Array([1]).buffer,segmentMs:1}:{type:'error',id:data.id,message:'inference failed'}}));
   }
  }
  terminate(){}
 };
 try{
  const provider=new MediaPipeProvider(),frame={id:1,epoch:0,mediaTime:1,capturedAt:2000,image:{}};
  await assert.rejects(provider.segment(frame),/inference failed/);provider.control.retryAt=0;
  const result=await provider.segment({...frame,id:2,mediaTime:0,capturedAt:2033});assert.deepEqual(modes,['GPU','CPU']);assert.deepEqual(timestamps,[2000,2033]);assert.equal(result.values[0],1);assert.equal(provider.backend,'cpu');provider.dispose();
 }finally{globalThis.Worker=originalWorker;globalThis.createImageBitmap=originalBitmap;}
});
