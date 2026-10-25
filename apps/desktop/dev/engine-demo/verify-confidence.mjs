// Controlled mask regression. verify-matte.mjs separately exercises real inference.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
const shots='/tmp/engine-shots/v2';
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
const report={};
try {for(const version of ['before','after']) {
 const source=await readFile(version==='before'?shots+'/studio-engine-before.js':new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8');
 const body=source.replace('async function makeSegmenter(delegate) {',"async function makeSegmenter(delegate) { return (await import('/synthetic-segmenter.js')).makeSyntheticSegmenter();");
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(String(e)));await page.setViewport({width:1320,height:980});
 await page.setRequestInterception(true);page.on('request',r=>r.url().endsWith('/engine/studio-engine.js')?r.respond({status:200,contentType:'text/javascript',body}):r.continue());
 await page.goto('http://127.0.0.1:4178/?portrait&chair&still');await page.waitForFunction(()=>window.demo?.video.readyState>=2);await page.evaluate(()=>(demo.engine.setMatting?.('mediapipe'), demo.mode('replace')));
 await page.waitForFunction(()=>demo.engine.stats().segmentMs>0);await new Promise(r=>setTimeout(r,1000));
 await (await page.$('#output')).screenshot({path:`${shots}/confidence-${version}.png`});
 report[version]=await page.evaluate(()=>{
  const ctx=demo.canvas.getContext('2d'),bg=demo.replacement.getContext('2d');
  const pixel=(c,x,y)=>Array.from(c.getImageData(x,y,1,1).data);
  return {weak:pixel(ctx,750,112),island:pixel(ctx,512,102),person:pixel(ctx,640,120),background:pixel(bg,512,102),violations:demo.violations};
 });assert.equal(errors.length,0);assert.equal(report[version].violations.length,0);await page.evaluate(()=>demo.engine.dispose());await page.close();
 }
 assert.notDeepEqual(report.before.island,report.before.background);
 assert.notDeepEqual(report.before.weak,report.before.background);
 assert.deepEqual(report.after.island,report.after.background);
 assert.deepEqual(report.after.weak,report.after.background);
 assert.notDeepEqual(report.after.person,report.after.background);
 await writeFile(shots+'/confidence-report.json',JSON.stringify(report,null,2));console.log(JSON.stringify(report,null,2));
}finally{await browser.close();}
