import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import fs from 'node:fs/promises';import {evaluationServer} from './server.mjs';
const server=await evaluationServer(),browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
try{
 const page=await browser.newPage();await page.goto(server.url);
 await page.evaluate(async()=>{
  const {createStudioEngine}=await import('/engine/studio-engine.js');
  const invoke=async(command,body)=>{if(command==='segmenter_info')return {available:true};const r=await fetch('/vision',{method:'POST',body});if(!r.ok)throw Error(await r.text());return r.arrayBuffer();};
  window.video=document.createElement('video');video.src='/footage/wave.mp4';video.loop=true;video.muted=true;document.body.append(video);await video.play();
  window.engine=createStudioEngine({canvas:document.querySelector('canvas'),invoke,debug:'alpha'});engine.setSource(video);engine.setBackground({mode:'replace'});engine.start();
  window.capture=()=>{
   const output=document.querySelector('canvas'),oc=output.getContext('2d',{willReadFrequently:true}),alpha=oc.getImageData(0,0,1280,720);
   const {frame}=engine.getMattingDebug().displayed,c=frame.crop;
   const person=document.createElement('canvas');person.width=1280;person.height=720;const pc=person.getContext('2d');
   if(frame.mirror){pc.translate(1280,0);pc.scale(-1,1);}
   pc.drawImage(frame.image,c[0]*frame.image.width,c[1]*frame.image.height,c[2]*frame.image.width,c[3]*frame.image.height,0,0,1280,720);
   const pixels=pc.getImageData(0,0,1280,720);let levels=new Set();
   for(let i=0;i<1280*720;i++){pixels.data[i*4+3]=alpha.data[i*4];if(alpha.data[i*4]>0&&alpha.data[i*4]<255)levels.add(alpha.data[i*4]);}
   pc.putImageData(pixels,0,0);const composite=document.createElement('canvas');composite.width=1280;composite.height=720;
   const ctx=composite.getContext('2d');ctx.fillStyle='#50b878';ctx.fillRect(0,0,1280,720);ctx.drawImage(person,0,0);
   return {png:composite.toDataURL().split(',')[1],alpha:output.toDataURL().split(',')[1],levels:levels.size,stats:engine.stats()};
  };
 });
 const reports={};for(const zoom of [1,3]){
  await page.evaluate(zoom=>engine.setFraming({zoom,pan_x:zoom===3?-.45:0,pan_y:zoom===3?-.5:0}),zoom);
  await new Promise(r=>setTimeout(r,2500));const r=await page.evaluate(()=>capture());const {png,alpha,...report}=r;reports[zoom]=report;
  await fs.writeFile(`/tmp/v5-evaluation/zoom-${zoom}.png`,Buffer.from(png,'base64'));await fs.writeFile(`/tmp/v5-evaluation/zoom-${zoom}-alpha.png`,Buffer.from(alpha,'base64'));
 }
 await fs.writeFile('/tmp/v5-evaluation/zoom.json',JSON.stringify(reports,null,2));await page.evaluate(()=>{engine.dispose();video.pause();});console.log('PASS zoom captures',reports[1].levels,reports[3].levels);
}finally{await browser.close();server.close();}
