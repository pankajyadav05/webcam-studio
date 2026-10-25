// Ten-minute public-video soak; Chrome process-tree RSS includes shared pages.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import { execFileSync } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const out='/tmp/engine-shots/v4';await mkdir(out,{recursive:true});
const seconds=Number(process.argv[2]||600);
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-fake-device-for-media-stream=fps=30','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
function rss(){
 const rows=execFileSync('ps',['-axo','pid=,ppid=,rss='],{encoding:'utf8'}).trim().split('\n').map(x=>x.trim().split(/\s+/).map(Number));
 const ids=new Set([browser.process().pid]);for(let k=0;k<6;k++)for(const [pid,ppid]of rows)if(ids.has(ppid))ids.add(pid);
 return rows.filter(x=>ids.has(x[0])).reduce((s,x)=>s+x[2],0)/1024;
}
const samples=[],errors=[],violations=[],requests=[];
const delay=ms=>new Promise(r=>setTimeout(r,ms));
try{
 const page=await browser.newPage();page.on('pageerror',e=>errors.push(String(e)));page.on('request',r=>requests.push(r.url()));
 await page.goto('http://127.0.0.1:4180/?clip=wave&flat');await page.waitForFunction(()=>window.demo?.video.readyState>=2);
 const source=await page.evaluate(()=>({width:demo.video.videoWidth,height:demo.video.videoHeight,frameRate:30,kind:'public wave clip (known 30fps source)'}));assert.equal(source.width,1280);assert.equal(source.height,720);assert.equal(source.frameRate,30);
 await page.evaluate(()=>demo.mode('blur'));await page.waitForFunction(()=>demo.engine.stats().inferMs>0,{timeout:60000});await delay(15000);
 const baseline=rss();const start=Date.now();
 for(let i=0;i<seconds;i++){
  if(i%120===0)await page.evaluate(mode=>demo.mode(mode),i%240===0?'blur':'replace');
  await delay(1000);
  const v=await page.evaluate(()=>({...demo.engine.stats(),heap:performance.memory?.usedJSHeapSize}));samples.push({seconds:(Date.now()-start)/1000,rssMiB:rss(),...v});
  if((i+1)%60===0){console.log(JSON.stringify(samples.at(-1)));await writeFile(out+'/soak-progress.json',JSON.stringify({baseline,source,samples,errors},null,2));}
 }
 const median=a=>{a=[...a].sort((a,b)=>a-b);return a[Math.floor(a.length/2)];};
 const early=samples.slice(0,60),late=samples.slice(-60);
 const summary={seconds:seconds,elapsedSeconds:(Date.now()-start)/1000,baselineMiB:baseline,finalMiB:samples.at(-1).rssMiB,deltaMiB:samples.at(-1).rssMiB-baseline,firstMinuteMiB:median(early.map(x=>x.rssMiB)),lastMinuteMiB:median(late.map(x=>x.rssMiB)),firstMinuteFps:median(early.map(x=>x.fps)),lastMinuteFps:median(late.map(x=>x.fps)),firstMinuteInferMs:median(early.map(x=>x.inferMs)),lastMinuteInferMs:median(late.map(x=>x.inferMs)),firstMinuteGateMs:median(early.map(x=>x.gateInferMs)),lastMinuteGateMs:median(late.map(x=>x.gateInferMs)),firstMinuteGateFps:median(early.map(x=>x.gateFps)),lastMinuteGateFps:median(late.map(x=>x.gateFps)),firstMinuteHeapMiB:median(early.map(x=>x.heap/1048576)),lastMinuteHeapMiB:median(late.map(x=>x.heap/1048576))};
 const report={summary,source,samples,errors,violations:await page.evaluate(()=>demo.violations),requests:[...new Set(requests)]};
 await writeFile(out+'/soak.json',JSON.stringify(report,null,2));console.log('SOAK',JSON.stringify(summary));
 assert.equal(errors.length,0);assert.equal(report.violations.length,0);assert.ok(report.requests.every(x=>x.startsWith('http://127.0.0.1:4180/')||x.startsWith('data:')||x.startsWith('blob:http://127.0.0.1:4180/')));assert.ok(samples.every(x=>x.matting==='hybrid'));
 await page.evaluate(()=>demo.engine.dispose());await delay(2000);report.afterDisposeMiB=rss();await writeFile(out+'/soak.json',JSON.stringify(report,null,2));console.log('AFTER_DISPOSE_MB',report.afterDisposeMiB);
}finally{await browser.close();}
