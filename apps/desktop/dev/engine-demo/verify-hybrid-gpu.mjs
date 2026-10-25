// Controlled mattes test GPU gating independently of either model's accuracy.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import { readFile,writeFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--ignore-gpu-blocklist']});
try {
 const page=await browser.newPage();await page.setRequestInterception(true);
 page.on('request',async r=>{
  if(r.url()==='http://127.0.0.1:4180/')await r.respond({status:200,contentType:'text/html',body:'<!doctype html><canvas></canvas>'});
  else if(r.url().endsWith('/studio-engine.js'))await r.respond({status:200,contentType:'text/javascript',body:await readFile(new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8')+'\nexport {gpuCompositor};'});
  else await r.continue();
 });
 await page.goto('http://127.0.0.1:4180/');
 const report=await page.evaluate(async()=>{
  const {gpuCompositor,cropRect}=await import('/engine/studio-engine.js'),{createGate}=await import('/engine/hybrid-gate.js');
  const canvas=(w,h)=>{const c=document.createElement('canvas');c.width=w;c.height=h;return c;};
  const camera=canvas(1280,720);camera.videoWidth=1280;camera.videoHeight=720;camera.currentTime=0;
  camera.getContext('2d').fillStyle='white';camera.getContext('2d').fillRect(0,0,1280,720);
  const black=canvas(1280,720),matte=canvas(1280,720),mc=matte.getContext('2d');
  black.getContext('2d').fillStyle='black';black.getContext('2d').fillRect(0,0,1280,720);
  const rect=(x,y,w,h,a)=>{mc.fillStyle=`rgba(255,255,255,${a})`;mc.fillRect(x,y,w,h);};
  rect(450,100,200,500,1);rect(650,180,170,150,.75);rect(438,150,12,70,.25);
  rect(650,400,100,25,1);rect(1000,100,80,80,1);
  const bitmap=await createImageBitmap(matte),values=new Float32Array(256*144),rgba=new Uint8ClampedArray(values.length*4);
  for(let y=20;y<120;y++)for(let x=90;x<130;x++)values[y*256+x]=1;
  const params=createGate(256,144)(values,0,true,rgba),gateCanvas=canvas(256,144);
  gateCanvas.getContext('2d').putImageData(new ImageData(rgba,256,144),0,0);
  const gateBitmap=await createImageBitmap(gateCanvas),comp=gpuCompositor(),read=canvas(1280,720),rc=read.getContext('2d',{willReadFrequently:true});
  async function draw(age=0,hasPerson=true,f={}){
   let result;
   do{result=comp.draw(camera,cropRect(1280,720,1280,720,f),f,{mode:'replace',image:black},bitmap,1280,720,true,{bitmap:gateBitmap,params:{...params,hasPerson},timestamp:performance.now()-age});if(!result)await new Promise(requestAnimationFrame);}while(!result);
   rc.drawImage(result,0,0);return rc.getImageData(0,0,1280,720).data;
  }
  const at=(p,x,y)=>p[(y*1280+x)*4],p=await draw(),stale=await draw(300),empty=await draw(0,false),mirror=await draw(0,true,{mirror:true});
  const r={person:at(p,550,250),plume:at(p,760,250),hair:at(p,444,180),connectedFinger:at(p,720,410),detached:at(p,1040,140),ramp:at(p,668,250),stalePlume:at(stale,760,250),emptyPlume:at(empty,760,250),mirrorFinger:at(mirror,1279-720,410),mirrorPlume:at(mirror,1279-760,250)};
  const crop={zoom:1.5,pan_x:0,pan_y:0,mirror:true},cr=cropRect(1280,720,1280,720,crop),cropped=await draw(0,true,crop);
  const mapped=(x,y)=>[Math.round((1-(x-cr.x)/cr.width)*1280),Math.round((y-cr.y)/cr.height*720)];
  const [x,y]=mapped(720,410);r.cropFinger=at(cropped,x,y);
  bitmap.close();gateBitmap.close();comp.close();return r;
 });
 assert.ok(report.person>250);assert.equal(report.plume,0);assert.ok(report.hair>=55&&report.hair<=70);assert.ok(report.connectedFinger>245);assert.equal(report.detached,0);assert.ok(report.ramp>5&&report.ramp<180);assert.ok(report.stalePlume>185);assert.ok(report.emptyPlume>185);assert.ok(report.mirrorFinger>245);assert.equal(report.mirrorPlume,0);assert.ok(report.cropFinger>245);
 await writeFile('/tmp/engine-shots/v4/gpu-gate-tests.json',JSON.stringify(report,null,2));console.log('PASS hybrid plume, hair, connected finger, disconnected island, stale/empty fail-open, mirror and crop',report);
}finally{await browser.close();}
