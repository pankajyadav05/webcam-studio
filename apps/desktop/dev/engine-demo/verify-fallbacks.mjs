import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
const original=await readFile(new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8');
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
const reports=[];const delay=ms=>new Promise(r=>setTimeout(r,ms));
try {
 for(const kind of ['cpu','runtime-cpu','failure','canvas2d','lifecycle']) {
  const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(String(e)));
  await page.setRequestInterception(true);
  page.on('request',r=>{
   if(r.url().endsWith('/engine/studio-engine.js')) {
    let body=original;
    if(kind==='cpu')body=body.replace('async function makeSegmenter(delegate) {',"async function makeSegmenter(delegate) { if(delegate==='GPU') throw new Error('test: unavailable GPU');");
    if(kind==='runtime-cpu')body=body.replace('segmenter.segmentForVideo(bitmap',"if(backend==='gpu') throw new Error('test: GPU context lost'); segmenter.segmentForVideo(bitmap");
    if(kind==='failure')body=body.replace('async function makeSegmenter(delegate) {',"async function makeSegmenter(delegate) { throw new Error('test: model unavailable');");
    if(kind==='canvas2d')body=body.replace('function gpuCompositor() {','function gpuCompositor() { return null;');
    r.respond({status:200,contentType:'text/javascript',body});
   }else r.continue();
  });
  await page.goto('http://127.0.0.1:4178/?portrait');await page.waitForFunction(()=>window.demo?.video.readyState>=2);
  await page.evaluate(()=>(demo.engine.setMatting?.('mediapipe'), demo.mode('blur')));
  await page.waitForFunction(()=>['ready','error'].includes(demo.statuses.at(-1)?.state),{timeout:45000});await delay(2500);
  const data=await page.evaluate(()=>({kind:'',stats:demo.engine.stats(),status:demo.statuses.at(-1),violations:demo.violations,pixel:Array.from(demo.canvas.getContext('2d').getImageData(640,160,1,1).data)}));data.kind=kind;data.errors=errors;
  if(kind==='runtime-cpu') await page.waitForFunction(()=>demo.engine.stats().backend==='cpu',{timeout:30000});
  if(kind==='cpu'||kind==='runtime-cpu')assert.equal(data.stats.backend,'cpu');
  else if(kind==='failure')assert.equal(data.status.state,'error');
  else assert.equal(data.status.state,'ready');
  assert.ok(data.pixel.slice(0,3).some(x=>x>60),'video must not be black');
  assert.equal(errors.length,0);assert.equal(data.violations.length,0);
  if(kind==='lifecycle') {
   await page.evaluate(async()=>{
    const e=demo.engine;
    e.setBackground({mode:'replace',image:null});
    if(demo.statuses.at(-1).state!=='idle')throw Error('empty replacement');
    for(let i=0;i<25;i++){e.stop();e.start();e.setOutputSize(i%2?1280:640,i%2?720:480);e.setFraming({zoom:i%4+1,pan_x:i%3-1,mirror:!!(i%2)});demo.mode(i%2?'blur':'none');}
    const blank=document.createElement('video');e.setSource(blank);await new Promise(r=>setTimeout(r,100));e.setSource(null);e.setSource(demo.video);
    e.setOutputSize(1280,720);e.setFraming({});(demo.engine.setMatting?.('mediapipe'), demo.mode('blur'));
   });await delay(1000);
  }
  await page.screenshot({path:`/tmp/engine-shots/${kind}.png`});
  reports.push(data);await page.evaluate(()=>{demo.engine.dispose();demo.engine.dispose();demo.engine.start();});await page.close();
 }
 await writeFile('/tmp/engine-shots/fallback-report.json',JSON.stringify(reports,null,2));console.log(JSON.stringify(reports,null,2));
}finally{await browser.close();}
