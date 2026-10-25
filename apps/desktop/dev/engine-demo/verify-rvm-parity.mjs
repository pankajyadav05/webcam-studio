import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import {readFile,writeFile} from 'node:fs/promises';
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
try{
 const page=await browser.newPage();page.on('console',m=>console.log(m.text()));page.on('pageerror',console.log);
 await page.setRequestInterception(true);page.on('request',async r=>{
  if(r.url().includes('/probe/')) {await r.respond({status:200,contentType:'image/png',body:await readFile('/tmp/matting-bakeoff/frames/curly/'+new URL(r.url()).pathname.split('/').at(-1))});}
  else if(r.url().includes('.onnx?original')) {await r.respond({status:200,body:await readFile('/tmp/matting-bakeoff/models/rvm_mobilenetv3_fp32.onnx')});}
  else if(r.url().endsWith('/rvm.js')){
   let code=await readFile(new URL('../../ui/engine/rvm.js',import.meta.url),'utf8');
   code=code.replace('InferenceSession.create(modelURL.href,',"InferenceSession.create(modelURL.href+(backend==='wasm'?'?original':''),");
   code=code.replace("    this.rec.forEach(t => t.dispose()); this.rec = [result.r1o, result.r2o, result.r3o, result.r4o];", "    this.rec.forEach(t => t.dispose()); this.rec = [result.r1o, result.r2o, result.r3o, result.r4o]; this.alphaCopy=new Float32Array(await result.pha.getData()); result.pha.dispose(); return {bitmap:await createImageBitmap(bitmap)};");
   await r.respond({status:200,contentType:'text/javascript',body:code});
  }else await r.continue();
 });
 await page.goto('http://127.0.0.1:4180/?clip=wave');await page.waitForFunction(()=>window.demo?.video.videoWidth>0);
 const r=await page.evaluate(async()=>{
  demo.engine.stop();
  const {RvmMatting}=await import('/engine/rvm.js'),ort=await import('/engine/vendor/ort/ort.all.min.mjs');
  const c=document.createElement('canvas');c.width=1280;c.height=720;const ctx=c.getContext('2d');
  const gpu=await RvmMatting.create('webgpu'),cpu=await RvmMatting.create('wasm'),report=[];
  for(let i=1;i<=12;i++){
   const im=new Image();im.src='/probe/'+String(i).padStart(5,'0')+'.png';await im.decode();ctx.drawImage(im,0,0);
   const bitmap=await createImageBitmap(c);const a=await gpu.frame(bitmap,1,1000+i*40,.25),b=await cpu.frame(bitmap,1,1000+i*40,.25);
   let sum=0,max=0;for(let j=0;j<gpu.alphaCopy.length;j++){const e=Math.abs(gpu.alphaCopy[j]-cpu.alphaCopy[j]);sum+=e;max=Math.max(max,e);}
   report.push({frame:i,mae:sum/gpu.alphaCopy.length,maxError:max});a.bitmap.close();b.bitmap.close();bitmap.close();
  }
  await gpu.close();await cpu.close();return {report};
 });
 console.log(JSON.stringify(r.report,null,2));await writeFile('/tmp/engine-shots/v3/parity.json',JSON.stringify(r.report,null,2));
 if(r.report.some(x=>x.mae>.00001||x.maxError>.001))throw new Error('RVM WebGPU / original WASM parity failed');
}finally{await browser.close();}
