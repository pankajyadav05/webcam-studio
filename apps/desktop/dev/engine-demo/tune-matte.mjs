// Dev-only threshold comparison; real model inference is unchanged.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import { readFile, writeFile } from 'node:fs/promises';
const source=await readFile(new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8');
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required']});
try {for(const threshold of [.5,.65,.7]) {
 const page=await browser.newPage();await page.setViewport({width:1320,height:980});await page.setRequestInterception(true);
 page.on('request',r=>r.url().endsWith('/engine/studio-engine.js')?r.respond({status:200,contentType:'text/javascript',body:source.replace(/COMPONENT_THRESHOLD = [\d.]+/,`COMPONENT_THRESHOLD = ${threshold}`)}):r.continue());
 await page.goto('http://127.0.0.1:4178/?portrait&chair&still');await page.waitForFunction(()=>window.demo?.video.readyState>=2);await page.evaluate(()=>demo.mode('replace'));
 await page.waitForFunction(()=>{const s=demo.engine.stats();return s.segmentMs>0&&s.segmentMs<30;},{timeout:60000});await new Promise(r=>setTimeout(r,1800));
 await (await page.$('#output')).screenshot({path:`/tmp/engine-shots/v2/tune-${threshold}.png`});console.log(threshold,await page.evaluate(()=>demo.engine.stats()));await page.evaluate(()=>demo.engine.dispose());await page.close();
}}finally{await browser.close();}
