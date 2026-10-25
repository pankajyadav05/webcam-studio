import test from 'node:test'; import assert from 'node:assert/strict';
import {createStore,DEFAULTS} from './store.js';
const storage = value => ({getItem:key=>{assert.equal(key,'webcam-studio:v2');return value;},setItem(){}});
test('older v2 choices migrate with exact tuning defaults',()=>{
  const s=createStore(storage(JSON.stringify({zoom:2,mirror:true,background:{mode:'replace',radius:22,preset:'office'}}))).get();
  assert.equal(s.zoom,2);assert.equal(s.mirror,true);assert.equal(s.background.mode,'replace');assert.equal(s.background.preset,'office');
  for(const key of ['cutoff','softness','steadiness','quality','view']) assert.equal(s.background[key],DEFAULTS.background[key]);
});
test('corrupt state defaults and hostile persisted tuning clamps',()=>{
  assert.deepEqual(createStore(storage('{bad')).get(),DEFAULTS);
  const s=createStore(storage(JSON.stringify({background:{cutoff:-2,softness:20,steadiness:'oops',quality:'gpu',view:'raw'}}))).get();
  assert.equal(s.background.cutoff,0);assert.equal(s.background.softness,1);assert.equal(s.background.steadiness,.5);assert.equal(s.background.quality,'balanced');assert.equal(s.background.view,'composite');
});
test('partial updates merge, persist and reload tuning without persisting media',async()=>{
  let saved; const local={getItem:()=>saved,setItem:(key,value)=>{assert.equal(key,'webcam-studio:v2');saved=value;}};
  const store=createStore(local);store.set({background:{cutoff:.9,view:'mask'}});store.set({background:{quality:'accurate'}});
  await new Promise(r=>setTimeout(r,180));const next=createStore(local).get();
  assert.equal(next.background.cutoff,.9);assert.equal(next.background.view,'mask');assert.equal(next.background.quality,'accurate');assert.equal(next.background.softness,.5);
  assert.equal('source' in next,false); assert.equal('clip' in next,false);
});
