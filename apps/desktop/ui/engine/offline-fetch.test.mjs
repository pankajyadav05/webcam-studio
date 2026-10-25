import test from 'node:test';
import assert from 'node:assert/strict';
import { offlineFetch } from './offline-fetch.js';
test('telemetry never reaches fetch and same-origin model requests do', async()=>{
 const calls=[],fetch=offlineFetch('https://studio.local/engine/studio-engine.js',(...args)=>{calls.push(args);return Promise.resolve('ok');});
 assert.equal(await fetch('/engine/vendor/model'), 'ok');
 await assert.rejects(fetch('https://odml.pa.googleapis.com/v1/log'),/blocks external/);
 assert.equal(calls.length,1);
});
test('custom-scheme origins compare protocol and host instead of null origins',async()=>{
 const fetch=offlineFetch('tauri://localhost/engine/studio-engine.js',()=>Promise.resolve('ok'));
 assert.equal(await fetch('tauri://localhost/engine/vendor/model'),'ok');
 await assert.rejects(fetch('tauri://other-host/private'),/blocks external/);
 await assert.rejects(fetch('https://localhost/model'),/blocks external/);
});
