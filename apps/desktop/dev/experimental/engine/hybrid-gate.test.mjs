import test from 'node:test';
import assert from 'node:assert/strict';
import { createGate, distanceToRegion, gateRamp, recoveryRamp, GATE } from './hybrid-gate.js';
test('gate is flat inside a generous margin and has a continuous soft ramp',()=>{
  assert.equal(gateRamp(0),1);assert.equal(gateRamp(12),1);assert.equal(gateRamp(17),.5);assert.equal(gateRamp(22),0);
  let previous=1;for(let d=0;d<100;d+=.1){const g=gateRamp(d);assert.ok(g<=previous);assert.ok(g>=0&&g<=1);previous=g;}
});
test('distance transform measures 720p pixels without wrapping at boundaries',()=>{
  const a=new Float32Array(128*72);a[30*128+40]=1;
  const d=distanceToRegion(a,128,72,.35);
  assert.equal(d[30*128+41],10);assert.equal(d[31*128+40],10);assert.ok(Math.abs(d[31*128+41]-Math.sqrt(200))<.001);
  assert.ok(d[31*128]>300);
});
test('every person component survives, including tiny separate people and fingers',()=>{
  const a=new Float32Array(128*72),pixels=new Uint8ClampedArray(a.length*4);a[20*128+20]=.9;a[40*128+100]=.7;
  createGate(128,72)(a,100,true,pixels);
  for(const i of [20*128+20,40*128+100])assert.equal(pixels[i*4],255);
  assert.equal(pixels[(60*128+60)*4],0);
});
test('empty/full masks, source reset and storage reuse cannot leave a gate trail',()=>{
  const a=new Float32Array(64*36),p=new Uint8ClampedArray(a.length*4),gate=createGate(64,36);
  a.fill(1);gate(a,100,true,p);assert.equal(p[0],255);
  a.fill(0);const params=gate(a,200,true,p);assert.equal(p[0],0);assert.equal(params.speed,0);
});
test('measured expansion widens for motion, decays and resets',()=>{
  const a=new Float32Array(128*72),p=new Uint8ClampedArray(a.length*4),gate=createGate(128,72);
  const fill=x=>{a.fill(0);for(let y=20;y<40;y++)for(let c=x;c<x+20;c++)a[y*128+c]=1;};
  fill(20);gate(a,100,true,p);fill(25);let params=gate(a,167,false,p);assert.ok(params.speed>.5);assert.ok(params.speed<=1.2);
  for(let i=0;i<12;i++)params=gate(a,234+i*67,false,p);assert.ok(params.speed<.02);
  assert.equal(gate(a,2000,true,p).speed,0);assert.equal(GATE.rateHz,15);
});
test('hand motion widens only its neighborhood, leaving a still head gated',()=>{
  const w=128,h=72,a=new Float32Array(w*h),p=new Uint8ClampedArray(w*h*4),gate=createGate(w,h);
  function scene(handX){a.fill(0);for(let y=8;y<20;y++)for(let x=30;x<45;x++)a[y*w+x]=1;for(let y=50;y<60;y++)for(let x=handX;x<handX+10;x++)a[y*w+x]=1;}
  scene(60);gate(a,100,true,p);scene(67);gate(a,167,false,p);
  assert.equal(p[(10*w+35)*4+2],0);assert.ok(p[(55*w+72)*4+2]>100);
});
test('portrait distances keep the same physical 720p margin',()=>{
  const a=new Float32Array(72*128);a[64*72+36]=1;
  const d=distanceToRegion(a,72,128,.35,undefined,9/16);
  assert.equal(d[64*72+37],5.625);assert.equal(d[65*72+36],5.625);
});
test('an entirely missing MP person is reported so the compositor can retain RVM',()=>{
 const a=new Float32Array(32*18),p=new Uint8ClampedArray(a.length*4),gate=createGate(32,18);
 assert.equal(gate(a,10,true,p).hasPerson,false);a[100]=1;assert.equal(gate(a,80,false,p).hasPerson,true);
});

test('opaque recovery fades continuously at its reach and alpha threshold',()=>{
 assert.equal(recoveryRamp(112,1),1);assert.equal(recoveryRamp(120,1),.5);assert.equal(recoveryRamp(128,1),0);
 assert.equal(recoveryRamp(50,.98),0);assert.ok(recoveryRamp(50,.981)>0&&recoveryRamp(50,.981)<.02);
 assert.ok(recoveryRamp(127.9,1)<.001);assert.equal(recoveryRamp(129,1),0);
});
