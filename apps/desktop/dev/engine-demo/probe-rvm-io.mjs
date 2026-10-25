import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import {readFile,writeFile} from 'node:fs/promises';
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
try{
 const page=await browser.newPage();page.on('console',m=>console.log(m.text()));page.on('pageerror',console.log);
 await page.setRequestInterception(true);page.on('request',async r=>{
  if(process.env.NATIVE_EP && r.url().includes('/engine/vendor/ort/')) {
   let name=new URL(r.url()).pathname.split('/').at(-1);if(name==='ort.all.min.mjs')name='ort.webgpu.min.mjs';
   const dist=process.env.ORT_NEXT?'/tmp/ort-rvm-1.30/package/dist/':'/tmp/matting-bakeoff/web/node_modules/onnxruntime-web/dist/';
   await r.respond({status:200,contentType:name.endsWith('.wasm')?'application/wasm':'text/javascript',body:await readFile(dist+name)});
  }
  else if(r.url().endsWith('/probe.png')) {await r.respond({status:200,contentType:'image/png',body:await readFile('/tmp/matting-bakeoff/frames/curly/00001.png')});}
  else if(r.url().endsWith('/rvm.js')){
   let code=await readFile(new URL('../../ui/engine/rvm.js',import.meta.url),'utf8');
   code=code.replace("backend === 'webgpu' ? { preferredOutputLocation", "backend === 'webgpu' && !globalThis.cpuOutputs ? { preferredOutputLocation");
   code=code.replace("    this.rec.forEach(t => t.dispose()); this.rec = [result.r1o, result.r2o, result.r3o, result.r4o];", "    this.rec.forEach(t => t.dispose()); this.rec = [result.r1o, result.r2o, result.r3o, result.r4o]; this.alphaCopy=new Float32Array(await result.pha.getData()); result.pha.dispose(); return {bitmap:await createImageBitmap(bitmap)};");
   await r.respond({status:200,contentType:'text/javascript',body:code});
  }else await r.continue();
 });
 await page.goto('http://127.0.0.1:4180/?clip=wave');await page.waitForFunction(()=>window.demo?.video.videoWidth>0);
 const r=await page.evaluate(async()=>{
  demo.engine.stop();
  const {RvmMatting}=await import('/engine/rvm.js'),ort=await import('/engine/vendor/ort/ort.all.min.mjs');
  const c=document.createElement('canvas');c.width=1280;c.height=720;const ctx=c.getContext('2d',{willReadFrequently:true});const im=new Image();im.src='/probe.png';await im.decode();ctx.drawImage(im,0,0);
  const bm=await createImageBitmap(c),models=[],report={},n=1280*720;
  for(const [label,backend,cpu]of [['gpu-buffer','webgpu',false],['gpu-cpu-output','webgpu',true],['wasm','wasm',true]]){
   globalThis.cpuOutputs=cpu;
   const m=await RvmMatting.create(backend);models.push(m);
   const r=await m.frame(bm,1,1000,.25);r.bitmap.close();
   const a=m.alphaCopy;report[label]={mean:a.reduce((s,x)=>s+x,0)/n,high:a.filter(x=>x>.9).length/n};
   const im=ctx.createImageData(1280,720);for(let i=0;i<n;i++){im.data[i*4]=im.data[i*4+1]=im.data[i*4+2]=Math.round(a[i]*255);im.data[i*4+3]=255;}ctx.putImageData(im,0,0);
   report[label].png=c.toDataURL().split(',')[1];
  }
  bm.close(); for(const m of models)await m.close();return {report};
 });
 for(const [label,v]of Object.entries(r.report)){await writeFile('/tmp/engine-shots/v3/io-'+label+'.png',Buffer.from(v.png,'base64'));delete v.png;}
 console.log(JSON.stringify(r.report,null,2));await writeFile('/tmp/engine-shots/v3/io-backends.json',JSON.stringify(r.report,null,2));
}finally{await browser.close();}
