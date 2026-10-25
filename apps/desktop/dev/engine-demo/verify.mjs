// Run: node apps/desktop/dev/engine-demo/verify.mjs (start server.mjs first).
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
const shots='/tmp/engine-shots';await mkdir(shots,{recursive:true});
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
const page=await browser.newPage();await page.setViewport({width:1320,height:980});
await page.evaluateOnNewDocument(() => {
 window.frameCallbackTimes=[]; const native=requestAnimationFrame;
 window.requestAnimationFrame=cb=>native.call(window,t=>{const start=performance.now();try{cb(t);}finally{frameCallbackTimes.push(performance.now()-start);}});
});
const errors=[],warnings=[],requests=[];
page.on('pageerror',e=>errors.push(String(e)));page.on('console',m=>{if(m.type()==='error') errors.push(m.text());if(m.type()==='warn')warnings.push(m.text());});
page.on('request',r=>requests.push(r.url()));
const delay=ms=>new Promise(r=>setTimeout(r,ms));
try {
 await page.goto('http://127.0.0.1:4178');await page.waitForFunction(()=>window.demo?.video.readyState>=2);
 await delay(1000);
 const lazyRequests=requests.filter(u=>/vendor/.test(u));assert.equal(lazyRequests.length,0,'none must not load MediaPipe');
 await page.evaluate(()=>demo.portrait());await delay(1000);
 await page.screenshot({path:shots+'/portrait-before.png'});
 await page.evaluate(()=>(demo.engine.setMatting?.('mediapipe'), demo.mode('blur')));
 await page.waitForFunction(()=>['ready','error'].includes(demo.statuses.at(-1)?.state),{timeout:45000});
 console.log('STATUS',await page.evaluate(()=>demo.statuses));await delay(3000);
 await page.screenshot({path:shots+'/portrait-blur.png'});
 const samples=[];
 for(let i=0;i<10;i++){await delay(1000);samples.push(await page.evaluate(()=>demo.engine.stats()));}
 await page.evaluate(()=>(demo.engine.setMatting?.('mediapipe'), demo.mode('replace')));await delay(1000);await page.screenshot({path:shots+'/portrait-replace.png'});
 const pixels=await page.evaluate(()=>{
  const c=demo.canvas.getContext('2d');return {left:Array.from(c.getImageData(20,360,1,1).data),right:Array.from(c.getImageData(1260,360,1,1).data),person:Array.from(c.getImageData(640,160,1,1).data)};
 });
 await page.evaluate(()=>demo.engine.setFraming({mirror:true}));await delay(500);await page.screenshot({path:shots+'/portrait-mirror.png'});
 const mirrored=await page.evaluate(()=>Array.from(demo.canvas.getContext('2d').getImageData(20,360,1,1).data));
 assert.deepEqual(mirrored,pixels.left,'replacement image must not mirror');
 assert.ok(Math.abs(pixels.person[0]-pixels.right[0])>15,'person survives replacement');
 await page.evaluate(()=>{demo.engine.setFraming({zoom:2,pan_x:1,pan_y:-1});demo.engine.setOutputSize(640,480);});await delay(300);
 assert.deepEqual(await page.evaluate(()=>[demo.canvas.width,demo.canvas.height]),[640,480]);
 await page.evaluate(()=>{demo.engine.setSource(null);demo.engine.stop();demo.engine.start();demo.engine.setSource(demo.video);demo.engine.setOutputSize(1280,720);demo.engine.setFraming({});(demo.engine.setMatting?.('mediapipe'), demo.mode('blur'));});
 await delay(1000);
 const callbackTimes=await page.evaluate(()=>frameCallbackTimes);
 callbackTimes.sort((a,b)=>a-b);
 const frameCallbacks={count:callbackTimes.length,p95:callbackTimes[Math.floor(callbackTimes.length*.95)],max:callbackTimes.at(-1)};
 const report={samples,frameCallbacks,pixels,errors,warnings,requests:[...new Set(requests)],violations:await page.evaluate(()=>demo.violations),longTasks:await page.evaluate(()=>demo.longTasks)};
 await writeFile(shots+'/report.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
 assert.equal(report.violations.length,0,'CSP violations');assert.equal(errors.length,0,'console errors');
 assert.ok(requests.every(u=>u.startsWith('http://127.0.0.1:4178/')||u.startsWith('data:')),'runtime network must stay local');
 assert.equal((await page.evaluate(()=>demo.engine.stats())).backend,'gpu');
 await page.evaluate(()=>demo.engine.dispose());
} finally {await page.close().catch(()=>{});await browser.close();}
