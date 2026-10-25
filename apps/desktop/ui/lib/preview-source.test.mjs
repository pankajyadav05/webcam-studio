import test from 'node:test';import assert from 'node:assert/strict';
import {createPreviewSources,recordingMime} from './preview-source.js';
const tick=()=>new Promise(r=>setImmediate(r));
function fixture(invoke=async cmd=>cmd==='sample_manifest'?{version:1,clips:[{id:'wave',title:'Video call',bytes:4,credit:'Public fixture'},{id:'chair',bytes:4}]}:new Uint8Array([0,1,2,3]),extra={}) {
  const active=[],revoked=[],calls=[];let id=0;
  const camera={srcObject:{getVideoTracks:()=>[{kind:'video',readyState:'live'}]}};
  const api=createPreviewSources({camera,invoke:(...args)=>{calls.push(args);return invoke(...args);},onSource:v=>active.push(v),
    makeVideo:()=>({pause(){this.paused=true;},removeAttribute(){},load(){},async play(){this.paused=false;}}),
    urls:{createObjectURL:()=>`blob:${++id}`,revokeObjectURL:url=>revoked.push(url)},...extra});
  return {api,camera,active,revoked,calls};
}
test('no launch network; sample > camera > sample reuses blob and keeps camera alive',async()=>{
  const {api,calls,camera}=fixture(); assert.equal(calls.length,0);assert.equal(api.video,camera);
  await api.select('sample');assert.equal(api.get().active,'sample');assert.equal(calls.length,2);const video=api.video;
  await api.select('camera');assert.equal(api.video,camera);assert.equal(video.paused,true);
  await api.select('sample');assert.equal(api.video,video);assert.equal(calls.length,2);
  await api.chooseClip('chair');assert.equal(api.get().clipId,'chair');assert.equal(calls.length,3);api.dispose();
});
test('failed download stays on camera; retry works; invalid IDs rejected',async()=>{
  let offline=true;const {api,camera}=fixture(async cmd=>{if(offline)throw Error('Offline');return cmd==='sample_manifest'?{clips:[{id:'wave'}]}:new Uint8Array(4);});
  await api.select('sample');assert.match(api.get().error,/Offline/);assert.equal(api.video,camera);
  offline=false;await api.retry();assert.equal(api.get().active,'sample');
  await assert.rejects(api.chooseClip('../evil'),RangeError);await assert.rejects(api.select('raw'),RangeError);api.dispose();
});
test('late downloads never override newer source selections',async()=>{
  let resolve; const {api,camera}=fixture(()=>new Promise(r=>resolve=r));
  const pending=api.select('sample');await api.select('camera');resolve({clips:[{id:'wave'}]});await pending;
  assert.equal(api.video,camera);assert.equal(api.get().selected,'camera');api.dispose();
});
class Recorder {
  static isTypeSupported(type){return type==='video/mp4';}
  constructor(stream,options){assert.deepEqual(stream,['video']);this.mimeType=options.mimeType;this.state='inactive';}
  start(){this.state='recording';}
  stop(){this.state='inactive';this.ondataavailable({data:new Blob(['recording'])});this.onstop();}
}
test('record 5 seconds path chooses mp4, loops muted, replaces and revokes old blob',async()=>{
  assert.equal(recordingMime(Recorder),'video/mp4');
  const {api,revoked}=fixture(undefined,{Recorder,makeStream:tracks=>tracks.map(t=>t.kind),duration:15});
  await api.select('clip');api.record();assert.equal(api.get().recording,true);
  await new Promise(r=>setTimeout(r,30));assert.equal(api.get().hasClip,true);assert.equal(api.get().active,'clip');assert.equal(api.video.muted,true);assert.equal(api.video.loop,true);
  api.record();await new Promise(r=>setTimeout(r,30));assert.deepEqual(revoked,['blob:1']);api.dispose();assert.deepEqual(revoked,['blob:1','blob:2']);
});
test('changing source cancels recording and never replaces the existing preview',async()=>{
  const {api}=fixture(undefined,{Recorder,makeStream:()=>['video'],duration:40});
  await api.select('clip');api.record();await api.select('camera');await tick();
  assert.equal(api.get().recording,false);assert.equal(api.get().hasClip,false);api.dispose();
});
test('recording unsupported and missing camera errors are recoverable',async()=>{
  assert.throws(()=>recordingMime(null),/unavailable/);
  assert.throws(()=>recordingMime({isTypeSupported:()=>false}),/format/);
  const {api,camera}=fixture(undefined,{Recorder,makeStream:()=>[]});await api.select('clip');camera.srcObject=null;api.record();assert.match(api.get().error,/camera/);api.dispose();
});
test('failed second clip retains playing clip identity for correct credit',async()=>{
  let fail=false;
  const {api}=fixture(async(cmd,args)=>{
    if(cmd==='sample_manifest')return {clips:[{id:'wave',credit:'Wave author'},{id:'chair',credit:'Chair author'}]};
    if(fail)throw Error('Network failed');return new Uint8Array(4);
  });
  await api.select('sample');assert.equal(api.get().activeSampleId,'wave');fail=true;
  await api.chooseClip('chair');assert.equal(api.get().activeSampleId,'wave');assert.equal(api.get().clipId,'chair');assert.equal(api.get().status,'error');api.dispose();
});
test('oversize final recording chunk is rejected after recorder becomes inactive',async()=>{
  class Oversize extends Recorder {
    stop(){this.state='inactive';this.ondataavailable({data:{size:26*1024*1024}});this.onstop();}
  }
  const {api}=fixture(undefined,{Recorder:Oversize,makeStream:()=>['video'],duration:10});
  await api.select('clip');api.record();await new Promise(r=>setTimeout(r,25));assert.equal(api.get().hasClip,false);assert.match(api.get().error,/too large/);assert.equal(api.get().recording,false);api.dispose();
});
test('listing samples fetches only the manifest, stays on camera, and reports failures',async()=>{
  const {api,calls,camera}=fixture();
  const manifest=await api.listSamples();
  assert.equal(manifest.clips.length,2);assert.deepEqual(calls.map(c=>c[0]),['sample_manifest']);
  assert.equal(api.get().listing,'ready');assert.equal(api.video,camera);assert.equal(api.get().selected,'camera');
  await api.listSamples();assert.equal(calls.length,1);
  await api.chooseClip('chair');assert.deepEqual(calls.map(c=>c[0]),['sample_manifest','sample_clip']);api.dispose();
  const failing=fixture(async()=>{throw Error('Offline');});
  assert.equal(await failing.api.listSamples(),null);assert.equal(failing.api.get().listing,'error');
  assert.match(failing.api.get().listError,/Offline/);assert.equal(failing.api.get().error,'');failing.api.dispose();
});
