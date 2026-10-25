// Start server.mjs first. BASELINE=1 serves the saved pre-change engine to both module entries.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
const shots='/tmp/engine-shots/v2';await mkdir(shots,{recursive:true});
const version=process.env.BASELINE ? 'before' : 'after';
const baseline=process.env.BASELINE ? await readFile(shots+'/studio-engine-before.js','utf8') : null;
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
const delay=ms=>new Promise(r=>setTimeout(r,ms));const report={};
try {
 for(const fixture of ['portrait','chair']) {
  const page=await browser.newPage();await page.setViewport({width:1320,height:980});
  const errors=[],requests=[];page.on('pageerror',e=>errors.push(String(e)));page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});page.on('request',r=>requests.push(r.url()));
  if(baseline){await page.setRequestInterception(true);page.on('request',r=>r.url().endsWith('/engine/studio-engine.js')?r.respond({status:200,contentType:'text/javascript',body:baseline}):r.continue());}
  await page.evaluateOnNewDocument(()=>{
   window.frameCallbackTimes=[];window.maskCallbackTimes=[];
   const raf=requestAnimationFrame;window.requestAnimationFrame=cb=>raf.call(window,t=>{const start=performance.now();try{cb(t);}finally{frameCallbackTimes.push(performance.now()-start);}});
   const Native=Worker;window.Worker=class extends Native{set onmessage(cb){super.onmessage=e=>{const start=performance.now();try{cb(e);}finally{maskCallbackTimes.push(performance.now()-start);}};}};
  });
  await page.goto('http://127.0.0.1:4178/?portrait'+(fixture==='chair'?'&chair':''));await page.waitForFunction(()=>window.demo?.video.readyState>=2);
  await writeFile(`${shots}/${fixture}-source.png`,Buffer.from(await page.evaluate(()=>fixtureInput.toDataURL().split(',')[1]),'base64'));
  await page.evaluate(()=>(demo.engine.setMatting?.('mediapipe'), demo.mode('replace')));await page.waitForFunction(()=>demo.statuses.at(-1)?.state==='ready',{timeout:45000});await page.waitForFunction(()=>{const s=demo.engine.stats();return s.segmentMs>0&&s.segmentMs<30;},{timeout:45000});await delay(2000);
  await (await page.$('#output')).screenshot({path:`${shots}/${fixture}-${version}.png`});
  await page.evaluate(()=>{frameCallbackTimes.length=0;maskCallbackTimes.length=0;demo.longTasks.length=0;});
  const samples=[];for(let i=0;i<10;i++){await delay(1000);samples.push(await page.evaluate(()=>demo.engine.stats()));}
  report[fixture]=await page.evaluate(()=>{
   const summary=a=>{a.sort((x,y)=>x-y);return {count:a.length,p95:a[Math.floor(a.length*.95)],max:a.at(-1)};};
   return {frameCallbacks:summary(frameCallbackTimes),maskCallbacks:summary(maskCallbackTimes),violations:demo.violations,longTasks:demo.longTasks};
  });Object.assign(report[fixture],{samples,errors,requests:[...new Set(requests)]});
  assert.equal(errors.length,0);assert.equal(report[fixture].violations.length,0);
  assert.ok(report[fixture].frameCallbacks.max<=16,'frame callback exceeded 16 ms');
  assert.ok(report[fixture].maskCallbacks.max<=16,'mask callback exceeded 16 ms');
  assert.ok(requests.every(u=>u.startsWith('http://127.0.0.1:4178/')||u.startsWith('data:')));
  assert.ok(samples.every(s=>s.fps>=24));await page.evaluate(()=>demo.engine.dispose());await page.close();
 }
 await writeFile(`${shots}/${version}-report.json`,JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
}finally{await browser.close();}
