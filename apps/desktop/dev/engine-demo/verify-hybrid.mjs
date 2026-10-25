import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import { mkdir, writeFile, readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const kind = process.argv[2] || 'webgpu', seconds = Number(process.argv[3] || 25), mode = process.argv[4] || 'blur';
const dir = '/tmp/engine-shots/v4'; await mkdir(dir, { recursive: true });
const browser = await puppeteer.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true, args: ['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist'] });
const page = await browser.newPage(); await page.setViewport({ width: 1320, height: 980 });
const requests = [], errors = [], warnings = [], samples = [];
page.on('request', r => requests.push(r.url())); page.on('pageerror', e => errors.push(String(e)));
page.on('console', m => { if(m.type() === 'error') (/\[W:onnxruntime|^INFO: Created TensorFlow Lite XNNPACK delegate for CPU\.$/.test(m.text())?warnings:errors).push(m.text()); });
await page.evaluateOnNewDocument(() => {
  window.transferTimes=[];window.sourceTimes=[];window.callbackTimes = []; window.maskTimes = []; window.phase = 'load';
  const raf = requestAnimationFrame;
  window.requestAnimationFrame = cb => raf(t => { const s = performance.now(); try { cb(t); } finally { callbackTimes.push({ ms: performance.now()-s, phase }); } });
  const vf=HTMLVideoElement.prototype.requestVideoFrameCallback;
  if(vf)HTMLVideoElement.prototype.requestVideoFrameCallback=function(cb){return vf.call(this,(t,m)=>{const s=performance.now();try{cb(t,m);}finally{sourceTimes.push({ms:performance.now()-s,phase});}});};
  const Native = Worker;
  window.Worker = class extends Native { postMessage(...args) {const s=performance.now();try{return super.postMessage(...args);}finally{transferTimes.push({ms:performance.now()-s,phase});}} set onmessage(cb) { super.onmessage = cb && (e => { const s = performance.now(); try { cb(e); } finally { maskTimes.push({ ms:performance.now()-s,phase }); } }); } };
});
if (['wasm','gpu-fail','worker-fail','runtime-worker-fail','model-fail','device-lost','slow','fp16','gate-fail','gate-crash'].includes(kind)) {
  await page.setRequestInterception(true);
  page.on('request', async r => {
    if (['gate-fail','gate-crash'].includes(kind) && r.url().endsWith('/studio-engine.js')) {
      let code=await readFile(new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8');
      if(kind==='gate-fail') code=code.replace('gateMode = !!data.gate;', "gateMode = !!data.gate;if(gateMode)setTimeout(()=>{throw new Error('Forced gate worker startup failure');},0);");
      else code=code.replace('const params = gateFilter', "if(timestamp>8000)throw new Error('Forced gate runtime failure');const params = gateFilter");
      await r.respond({status:200,contentType:'text/javascript',body:code});
    } else if (r.url().endsWith('/rvm-worker.js')) {
      let code = await readFile(new URL('../../ui/engine/rvm-worker.js', import.meta.url), 'utf8');
      if (kind === 'wasm') code = code.replace('await init(data.preferWasm)', 'await init(true)').replace('policy.sample(result.segmentMs)', 'null');
      if (kind === 'gpu-fail') code = "Object.defineProperty(navigator,'gpu',{value:undefined});\n"+code;
      if (kind === 'runtime-worker-fail') code = code.replace('const change = policy.sample',"setTimeout(()=>{throw new Error('Forced runtime worker crash');},0); const change = policy.sample");
      if (kind === 'worker-fail') code = "throw new Error('Forced worker failure');";
      if (kind === 'device-lost') code = code.replace("const change = policy.sample", "if (data.timestamp > 8000 && backend === 'webgpu') model.device.destroy(); const change = policy.sample");
      if (kind === 'slow') code = code.replace('policy.sample(result.segmentMs)', 'policy.sample(100)');
      await r.respond({status:200,contentType:'text/javascript',body:code});
    } else if (kind === 'fp16' && r.url().endsWith('/rvm.js')) {
      let code = await readFile(new URL('../../ui/engine/rvm.js',import.meta.url),'utf8');
      code=code.replaceAll('rvm_mobilenetv3_fp32','rvm_mobilenetv3_fp16')
        .replaceAll("new ort.Tensor('float32', new Float32Array(1)","new ort.Tensor('float16', new Uint16Array(1)")
        .replace('w * h * 3 * 4','w * h * 3 * 2').replace("dataType: 'float32'","dataType: 'float16'")
        .replace('code: `\n      @group','code: `enable f16;\n      @group')
        .replace('rgb: array<f32>','rgb: array<f16>').replace('rgb[i]=v.r; rgb[i+n]=v.g; rgb[i+2u*n]=v.b;','rgb[i]=f16(v.r); rgb[i+n]=f16(v.g); rgb[i+2u*n]=f16(v.b);')
        .replace('code: `\n        @group','code: `enable f16;\n        @group')
        .replace('alpha:array<f32>','alpha:array<f16>').replace('clamp(alpha[','clamp(f32(alpha[').replace('u32(p.x)],0.,1.)','u32(p.x)]),0.,1.)');
      await r.respond({status:200,contentType:'text/javascript',body:code});
    } else if (kind === 'fp16' && r.url().endsWith('fp16.onnx')) {
      await r.respond({status:200,body:await readFile('/tmp/rvm-pool-fp16.onnx')});
    } else if (kind === 'model-fail' && r.url().endsWith('.onnx')) await r.respond({status:404,body:'Forced model fetch failure'});
    else await r.continue();
  });
}
const delay = ms => new Promise(r => setTimeout(r,ms));
try {
  await page.goto('http://127.0.0.1:4180/?clip=wave&flat'); await page.waitForFunction(() => window.demo?.video.readyState >= 2);
  if (['mediapipe','rvm'].includes(kind)) await page.evaluate(kind => demo.engine.setMatting(kind),kind);
  await page.evaluate(mode => demo.mode(mode), mode);
  await page.waitForFunction(() => demo.engine.stats().inferMs > 0, {timeout:60000});
  await delay(5000); await page.evaluate(() => { phase='steady'; callbackTimes=[]; maskTimes=[]; sourceTimes=[]; transferTimes=[]; });
  for(let i=0; i<seconds; i++) { await delay(1000); samples.push(await page.evaluate(() => demo.engine.stats())); }
  const report = await page.evaluate(() => ({ statuses:demo.statuses, violations:demo.violations, transferCallbacks:transferTimes, sourceCallbacks:sourceTimes, callbacks:callbackTimes, maskCallbacks:maskTimes, stats:demo.engine.stats(), heap:performance.memory?.usedJSHeapSize }));
  await page.screenshot({path:`${dir}/chrome-${kind}-${mode}.png`});
  Object.assign(report,{kind,seconds,mode,samples,errors,warnings,requests:[...new Set(requests)]});
  for(const name of ['callbacks','maskCallbacks','sourceCallbacks','transferCallbacks']) { const a=report[name].map(x=>x.ms).sort((a,b)=>a-b); report[name+'Summary']={count:a.length,over16:a.filter(x=>x>16).length,max:a.at(-1),p95:a[Math.floor(a.length*.95)]}; }
  await writeFile(`${dir}/chrome-${kind}-${mode}.json`,JSON.stringify(report,null,2));
  console.log(JSON.stringify({kind,mode,stats:report.stats,callbacks:report.callbacksSummary,maskCallbacks:report.maskCallbacksSummary,statuses:report.statuses,violations:report.violations,errors},null,2));
  assert.equal(report.violations.length,0); assert.ok(requests.every(u=>u.startsWith('http://127.0.0.1:4180/')||u.startsWith('data:')||u.startsWith('blob:http://127.0.0.1:4180/')));
  assert.ok(report.stats.inferMs>0);
  if(['worker-fail','runtime-worker-fail','model-fail','device-lost','slow','mediapipe'].includes(kind)) assert.equal(report.stats.matting,'mediapipe');
  if(kind==='gpu-fail') assert.ok(report.statuses.some(s=>s.backend==='wasm'),'WebGPU failure must try WASM');
  if(kind==='webgpu') {assert.equal(report.stats.backend,'webgpu');assert.equal(report.stats.matting,'hybrid');}
  if(['gate-fail','gate-crash'].includes(kind)){assert.equal(report.stats.matting,'rvm');assert.ok(report.statuses.some(s=>s.message.includes('RVM-only')));}
  if(['webgpu','wasm','mediapipe'].includes(kind)) assert.equal(errors.length,0,'unexpected browser errors');
  if(kind==='webgpu' && mode==='blur') {
    const visibility = await page.evaluate(async()=>{
      Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));
      await new Promise(r=>setTimeout(r,200));const before=maskTimes.length;
      await new Promise(r=>setTimeout(r,300));const after=maskTimes.length;
      delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));await new Promise(r=>setTimeout(r,1200));
      return {before,after,resumedFps:demo.engine.stats().fps};
    });
    assert.equal(visibility.before,visibility.after,'hidden tab must stop submitting inference');assert.ok(visibility.resumedFps>0);
    await writeFile(`${dir}/visibility.json`,JSON.stringify(visibility,null,2));
  }
  // Source loss, output changes and racing mode switches must keep the engine usable.
  await page.evaluate(() => { demo.engine.setSource(null); demo.engine.setOutputSize(640,480); demo.engine.stop(); demo.engine.start(); demo.engine.setSource(demo.video); demo.engine.setMatting('mediapipe'); demo.engine.setMatting('rvm'); demo.mode('none'); demo.mode('replace'); demo.engine.dispose(); demo.engine.dispose(); });
  await delay(300);
  assert.ok(!errors.some(x=>/TypeError|ReferenceError/.test(x)),'lifecycle must not cause async errors');
} finally { await browser.close(); }
