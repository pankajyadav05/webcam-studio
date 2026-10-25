// Regression: ImageBitmap orientation, soft alpha, crop and mirror in both compositors.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import {readFile,writeFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream','--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
try{
 const page=await browser.newPage();await page.setRequestInterception(true);
 page.on('request',async r=>r.url().endsWith('/engine/studio-engine.js')?r.respond({status:200,contentType:'text/javascript',body:await readFile(new URL('../../ui/engine/studio-engine.js',import.meta.url),'utf8')+'\nexport {gpuCompositor,cpuCompositor};'}):r.continue());
 await page.goto('http://127.0.0.1:4180/');await page.waitForFunction(()=>window.demo);
 const report=await page.evaluate(async()=>{
  demo.engine.stop();const {gpuCompositor,cpuCompositor,cropRect}=await import('/engine/studio-engine.js');
  const camera=document.createElement('canvas');camera.width=160;camera.height=80;camera.videoWidth=160;camera.videoHeight=80;camera.currentTime=0;const cc=camera.getContext('2d');cc.fillStyle='white';cc.fillRect(0,0,160,80);
  const black=document.createElement('canvas');black.width=160;black.height=80;const bc=black.getContext('2d');bc.fillStyle='black';bc.fillRect(0,0,160,80);
  const mask=document.createElement('canvas');mask.width=160;mask.height=80;const mc=mask.getContext('2d');const pixels=mc.createImageData(160,80);
  for(let y=0;y<80;y++)for(let x=0;x<160;x++){const i=(y*160+x)*4;pixels.data[i]=pixels.data[i+1]=pixels.data[i+2]=255;pixels.data[i+3]=Math.round((.1+.6*y/79+.2*x/159)*255);}mc.putImageData(pixels,0,0);
  const bitmap=await createImageBitmap(mask),gpu=gpuCompositor(),cpu=cpuCompositor(),out=[];
  for(const [w,h,f]of [[160,80,{zoom:1,mirror:false}],[80,80,{zoom:2,pan_x:1,pan_y:-1,mirror:true}],[320,180,{zoom:1.5,pan_x:-1,pan_y:1,mirror:false}]]){
   const crop=cropRect(160,80,w,h,f),bg={mode:'replace',image:black};
   let g;while(!(g=gpu.draw(camera,crop,f,bg,bitmap,w,h,true)))await new Promise(requestAnimationFrame);
   const c=cpu.draw(camera,crop,f,bg,bitmap,w,h,true),read=document.createElement('canvas');read.width=w;read.height=h;const rc=read.getContext('2d',{willReadFrequently:true});rc.drawImage(g,0,0);const gp=rc.getImageData(0,0,w,h).data;rc.drawImage(c,0,0);const cp=rc.getImageData(0,0,w,h).data;
   const samples=[[4,4],[w-5,4],[4,h-5],[w-5,h-5],[Math.floor(w/2),Math.floor(h/2)]];
   out.push({w,h,f,samples:samples.map(([x,y])=>({x,y,gpu:gp[(y*w+x)*4],cpu:cp[(y*w+x)*4]}))});
  }
  // MediaPipe updates one mutable canvas. Its texture must be uploaded again.
  mc.clearRect(0,0,160,80);
  const crop=cropRect(160,80,160,80,{}),bg={mode:'replace',image:black};
  const check=()=>{const c=gpu.draw(camera,crop,{},bg,mask,160,80,false);const t=document.createElement('canvas');t.width=160;t.height=80;const x=t.getContext('2d');x.drawImage(c,0,0);return x.getImageData(80,40,1,1).data[0];};
  const empty=check();mc.fillStyle='white';mc.fillRect(0,0,160,80);const full=check();
  if(empty>2||full<253)throw new Error(`Mutable MediaPipe mask did not update: ${empty}/${full}`);
  bitmap.close();gpu.close();cpu.close();return out;
 });
 for(const r of report)for(const s of r.samples)assert.ok(Math.abs(s.gpu-s.cpu)<=3,JSON.stringify({r,s}));
 assert.ok(report[0].samples[0].gpu>25&&report[0].samples[0].gpu<45,'low soft alpha must survive');
 await writeFile('/tmp/engine-shots/v3/transforms.json',JSON.stringify(report,null,2));console.log('PASS RVM soft alpha / orientation / crop / mirror / resize');
}finally{await browser.close();}
