import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import fs from 'node:fs/promises';import {evaluationServer} from './server.mjs';
const server=await evaluationServer();const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
try {
 const results={};
 for(const which of ['old','new']) {
  const page=await browser.newPage();page.on('pageerror',e=>console.error(e));await page.goto(server.url);
  await page.evaluate(async which=>{
    window.ages=[];window.tasks=[];window.statuses=[];
    window.__oldPaint=({mediaTime,maskTime})=>{if(Number.isFinite(maskTime))ages.push((mediaTime-maskTime)*30);};
    new PerformanceObserver(list=>tasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration})))).observe({type:'longtask',buffered:true});
    const invoke=async(command,body)=>{
      if(command==='segmenter_info')return {available:true};
      const r=await fetch('/vision',{method:'POST',body});if(!r.ok)throw Error(await r.text());return r.arrayBuffer();
    };
    const {createStudioEngine}=await import(which==='old'?'/legacy/studio-engine.js':'/engine/studio-engine.js');
    window.video=document.createElement('video');video.muted=true;video.loop=true;video.src='/footage/wave.mp4';document.body.append(video);
    await new Promise(r=>video.onloadeddata=r);await video.play();
    const bg=document.createElement('canvas');bg.width=1280;bg.height=720;bg.getContext('2d').fillStyle='#50b878';bg.getContext('2d').fillRect(0,0,1280,720);
    window.engine=createStudioEngine({canvas:document.querySelector('canvas'),invoke,debug:true});
    engine.on('status',s=>statuses.push(s));engine.setMatting(which==='old'?'mediapipe':'auto');engine.setSource(video);
    engine.setBackground({mode:'replace',image:bg});engine.start();
  },which);
  await new Promise(r=>setTimeout(r,5000));
  await page.evaluate(()=>{window.ages=[];window.tasks=[];window.samples=[];window.collect=setInterval(()=>samples.push(engine.stats()),250);});
  await new Promise(r=>setTimeout(r,8000));
  results[which]=await page.evaluate(()=>{clearInterval(collect);return {samples,ages,tasks,statuses};});
  await page.screenshot({path:`/tmp/v5-evaluation/realtime-${which}.png`});
  await page.evaluate(()=>{engine.dispose();video.pause();});await page.close();
  console.log(which,JSON.stringify(results[which].samples.at(-1)));
 }
 await fs.writeFile('/tmp/v5-evaluation/realtime.json',JSON.stringify(results,null,2));
} finally {await browser.close();server.close();}
