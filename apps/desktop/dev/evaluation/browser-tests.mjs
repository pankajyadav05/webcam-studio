import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import fs from 'node:fs/promises';import assert from 'node:assert/strict';import {evaluationServer} from './server.mjs';
const server=await evaluationServer(),browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
try{
 const page=await browser.newPage();page.on('pageerror',error=>console.error(error));await page.goto(server.url);
 const result=await page.evaluate(async()=>{
  const {createStudioEngine}=await import('/engine/studio-engine.js');
  const {createCompositor}=await import('/engine/compositor.js');
  const delay=ms=>new Promise(r=>setTimeout(r,ms));
  const source=document.createElement('canvas');source.width=320;source.height=180;
  const sc=source.getContext('2d');let tick=0;
  function sourceFrame(){sc.fillStyle=tick++%2?'#ff0000':'#00ff00';sc.fillRect(0,0,320,180);requestAnimationFrame(sourceFrame);}sourceFrame();
  const video=document.createElement('video');video.srcObject=source.captureStream(30);video.muted=true;document.body.append(video);await video.play();
  const output=document.querySelector('canvas'),bg=document.createElement('canvas');bg.width=320;bg.height=180;bg.getContext('2d').fillStyle='#0000ff';bg.getContext('2d').fillRect(0,0,320,180);
  let produce=false,epochs=[],bitmapAttempts=0;
  const originalCreateBitmap=globalThis.createImageBitmap;
  globalThis.createImageBitmap=()=>{bitmapAttempts++;return new Promise(()=>{});};
  const providerFactory=()=>({name:'test',backend:'synthetic',ready:true,async segment(frame){
   await delay(18);epochs.push(frame.epoch);if(!produce)return null;
   return {id:frame.id,epoch:frame.epoch,mediaTime:frame.mediaTime,width:16,height:9,alpha:new Uint8Array(16*9).fill(255)};
  },dispose(){}});
  const engine=createStudioEngine({canvas:output,width:320,height:180,debug:true,invoke:()=>Promise.reject(Error('command missing')),providerFactory});
  engine.setSource(video);engine.setBackground({mode:'replace',image:bg});engine.start();
  const oc=output.getContext('2d',{willReadFrequently:true});let maxRoom=0,blueMin=255;
  for(let i=0;i<12;i++){await delay(20);const p=oc.getImageData(160,90,1,1).data;maxRoom=Math.max(maxRoom,p[0],p[1]);blueMin=Math.min(blueMin,p[2]);}
  const privacy={maxRoom,blueMin};
  produce=true;await delay(450);const ready=engine.stats();globalThis.createImageBitmap=originalCreateBitmap;
  const compositor=engine.getMattingDebug().compositor;
  compositor.loseContext();await delay(150);const lost=[...oc.getImageData(160,90,1,1).data];
  compositor.restoreContext();await delay(500);const restored=engine.stats();
  engine.setOutputSize(640,360);await delay(250);const resized=[output.width,output.height,engine.stats().maskAgeAtPaint.p90];
  const video2=document.createElement('video');video2.srcObject=source.captureStream(30);video2.muted=true;document.body.append(video2);await video2.play();
  engine.setSource(video2);const sourceCover=[...oc.getImageData(160,90,1,1).data];await delay(300);const sourceChanged=engine.stats();
  for(let i=0;i<12;i++){engine.setBackground({mode:i%2?'replace':'blur',image:bg,radius:30});engine.setFraming({zoom:i%3+1,mirror:!!(i%2)});}
  await delay(300);const switched=engine.stats();
  Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));const hiddenProvider=engine.getMattingDebug().provider;
  await delay(80);Object.defineProperty(document,'hidden',{configurable:true,value:false});document.dispatchEvent(new Event('visibilitychange'));await delay(300);const visible=engine.stats();
  engine.dispose();
  // Exact orientation/mirror/crop mapping through DOM and R8 uploads.
  const quadrants=document.createElement('canvas');quadrants.width=64;quadrants.height=64;const q=quadrants.getContext('2d');
  for(const [x,y,c]of[[0,0,'red'],[32,0,'lime'],[0,32,'blue'],[32,32,'white']]){q.fillStyle=c;q.fillRect(x,y,32,32);}
  const comp=createCompositor(),check=document.createElement('canvas');check.width=check.height=64;const cc=check.getContext('2d',{willReadFrequently:true});
  const mask={width:2,height:2,alpha:new Uint8Array([255,0,0,0])};
  function draw(mirror,crop=[0,0,1,1]){cc.drawImage(comp.draw({image:quadrants,mask,crop,mirror,background:{mode:'replace',image:bg},width:64,height:64}),0,0);return [[8,8],[56,8],[8,56],[56,56]].map(([x,y])=>[...cc.getImageData(x,y,1,1).data].slice(0,3));}
  const orientation=draw(false),mirror=draw(true),crop=draw(false,[0,0,.5,.5]);
  const bitmapCamera=await createImageBitmap(quadrants),bitmapBackground=await createImageBitmap(quadrants);
  cc.drawImage(comp.draw({image:bitmapCamera,mask,background:{mode:'replace',image:bg},width:64,height:64}),0,0);
  const bitmapOrientation=[[8,8],[56,8],[8,56],[56,56]].map(([x,y])=>[...cc.getImageData(x,y,1,1).data].slice(0,3));
  cc.drawImage(comp.draw({image:quadrants,mask:null,background:{mode:'replace',image:bitmapBackground},width:64,height:64}),0,0);
  const bitmapBackgroundTop=[...cc.getImageData(8,8,1,1).data].slice(0,3);
  bitmapCamera.close();bitmapBackground.close();
  // Strong blur with no mask must not expose source pixels; premultiplication must stay finite.
  const blurred=comp.draw({image:quadrants,mask:null,background:{mode:'blur',radius:50},width:64,height:64});cc.drawImage(blurred,0,0);const blurCenter=[...cc.getImageData(32,32,1,1).data];
  comp.dispose();video.pause();video2.pause();video.srcObject.getTracks().forEach(t=>t.stop());video2.srcObject.getTracks().forEach(t=>t.stop());
  const worker=new Worker('/engine/providers/mediapipe-worker.js',{type:'module'});
  const bitmap=await createImageBitmap(source);
  const workerError=await new Promise(resolve=>{worker.onmessage=e=>resolve(e.data);worker.postMessage({type:'frame',id:77,epoch:0,timestamp:1,bitmap},[bitmap]);});worker.terminate();
  return {privacy,ready,bitmapAttempts,lost,restored,resized,sourceCover,sourceChanged,switched,hiddenProvider,visible,orientation,mirror,crop,blurCenter,workerError,bitmapOrientation,bitmapBackgroundTop};
 });
 assert.ok(result.privacy.maxRoom<2 && result.privacy.blueMin>250,JSON.stringify(result.privacy));
 assert.equal(result.bitmapAttempts,0);assert.ok(result.ready.maskRate>0);
 assert.deepEqual(result.lost,[0,0,255,255]);assert.equal(result.restored.maskAgeAtPaint.p90,0);
 assert.deepEqual(result.resized,[640,360,0]);assert.deepEqual(result.sourceCover,[0,0,255,255]);assert.equal(result.sourceChanged.maskAgeAtPaint.p90,0);assert.equal(result.workerError.id,77);assert.equal(result.workerError.type,'error');assert.equal(result.hiddenProvider,null);assert.ok(result.visible.maskRate>0);
 assert.deepEqual(result.bitmapOrientation,result.orientation);assert.ok(result.bitmapBackgroundTop[0]>240 && result.bitmapBackgroundTop[1]<10);
 assert.ok(result.orientation[0][0]>240 && result.orientation[3][2]>240,JSON.stringify(result.orientation));
 assert.ok(result.mirror[1][0]>240 && result.mirror[0][2]>240,JSON.stringify(result.mirror));
 assert.ok(result.crop[0][0]>240,JSON.stringify(result.crop));
 await fs.writeFile('/tmp/v5-evaluation/browser-tests.json',JSON.stringify(result,null,2));console.log('PASS privacy, context restore, resize, rapid modes, visibility, orientation/mirror/crop, blur startup');
 // Exercise actual MediaPipe worker fallback after a native inference error.
 await page.reload();
 const fallback=await page.evaluate(async()=>{
  const {createStudioEngine}=await import('/engine/studio-engine.js');const video=document.createElement('video');video.src='/footage/wave.mp4';video.loop=true;video.muted=true;document.body.append(video);await video.play();
  const statuses=[];const invoke=command=>command==='segmenter_info'?Promise.resolve({available:true}):Promise.reject(Error('injected native error'));
  const engine=createStudioEngine({canvas:document.querySelector('canvas'),width:320,height:180,invoke});engine.on('status',s=>statuses.push(s));engine.setSource(video);engine.setBackground({mode:'replace'});engine.start();
  await new Promise((resolve,reject)=>{let start=performance.now();const timer=setInterval(()=>{if(engine.stats().maskRate>0){clearInterval(timer);resolve();}else if(performance.now()-start>20000){clearInterval(timer);reject(Error(JSON.stringify(statuses)));}},100);});
  const stats=engine.stats();engine.dispose();
  const missingStatuses=[];const missing=createStudioEngine({canvas:document.querySelector('canvas'),width:320,height:180});missing.on('status',s=>missingStatuses.push(s));missing.setSource(video);missing.setBackground({mode:'replace'});missing.start();
  await new Promise((resolve,reject)=>{const start=performance.now();const timer=setInterval(()=>{if(missing.stats().maskRate>0){clearInterval(timer);resolve();}else if(performance.now()-start>20000){clearInterval(timer);reject(Error(JSON.stringify(missingStatuses)));}},100);});
  const missingStats=missing.stats();missing.dispose();video.pause();return {stats,statuses,missingStats,missingStatuses};
 });
 assert.equal(fallback.stats.matting,'mediapipe');assert.equal(fallback.missingStats.matting,'mediapipe');assert.ok(fallback.missingStatuses.some(s=>s.message.includes('fallback')));assert.ok(fallback.statuses.some(s=>s.message.includes('Vision failed')));
 await fs.writeFile('/tmp/v5-evaluation/fallback-test.json',JSON.stringify(fallback,null,2));console.log('PASS native error and missing command -> real offline MediaPipe workers');
}finally{await browser.close();server.close();}
