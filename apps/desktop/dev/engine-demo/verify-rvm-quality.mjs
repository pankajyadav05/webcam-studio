// Uses only the public bake-off frames. Test-only responses expose compositor alpha.
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import http from 'node:http';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
const fixed025=process.argv[2]==='025';
const root='/tmp/matting-bakeoff', out='/tmp/engine-shots/v3/'+(fixed025?'quality025':'quality');
const engine=path.resolve('apps/desktop/ui/engine');
const current=await readFile(engine+'/studio-engine.js','utf8');
const original=await readFile(root+'/engine-snapshot/studio-engine.js','utf8');
const csp="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; img-src 'self' data: blob:; media-src 'self' blob: mediastream:; connect-src 'self' ipc: http://ipc.localhost";
const server=http.createServer(async(req,res)=>{try{
 const p=new URL(req.url,'http://localhost').pathname;let bytes,mime='application/octet-stream';
 if(p==='/'){bytes='<html><body><canvas></canvas></body></html>';mime='text/html';}
 else if(p.endsWith('/alpha.js')){bytes=current.replace('color=vec4(mix(bg.rgb,sharp.rgb,a),1.);','color=vec4(vec3(a),1.);')+'\nexport {gpuCompositor};';mime='text/javascript';}
 else if(p.endsWith('/old.js')){bytes=original.replace('color=vec4(mix(bg.rgb,sharp.rgb,a),1.);','color=vec4(vec3(a),1.);')+'\nexport {gpuCompositor};';mime='text/javascript';}
 else {const f=p.startsWith('/frames/')?root+p:engine+p.replace(/^\/engine/,'');bytes=await readFile(f);if(fixed025&&p.endsWith('/rvm-worker.js'))bytes=bytes.toString().replace('policy.sample(result.segmentMs)','null');mime=/\.m?js$/.test(f)?'text/javascript':f.endsWith('.wasm')?'application/wasm':f.endsWith('.png')?'image/png':mime;}
 res.writeHead(200,{'Content-Type':mime,'Content-Security-Policy':csp});res.end(bytes);
}catch(e){res.writeHead(404).end(String(e));}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
try {
 const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}`);
 await page.evaluate(async()=>{
  window.alphaComp=(await import('/engine/alpha.js')).gpuCompositor();window.oldComp=(await import('/engine/old.js')).gpuCompositor();
  window.input=document.createElement('canvas'); input.width=1280;input.height=720;input.videoWidth=1280;input.videoHeight=720;window.ic=input.getContext('2d');
  window.output=document.querySelector('canvas');output.width=1280;output.height=720;window.oc=output.getContext('2d');
  window.mask=document.createElement('canvas');window.black=document.createElement('canvas');black.width=1280;black.height=720;
  window.qualityMetrics=[];window.workers={};
  for(const [name,url]of [['rvm','rvm-worker.js'],['old','old.js']]){
   const w=new Worker('/engine/'+url,{type:'module'});workers[name]=w;
   await new Promise((resolve,reject)=>{w.onmessage=e=>e.data.type==='ready'?resolve():reject(Error(e.data.message));w.postMessage({type:'init'});});
  }
  window.frame=async(clip,file,index,epoch)=>{
   const image=new Image();image.src='/frames/'+clip+'/'+file;await image.decode();ic.drawImage(image,0,0);input.currentTime=index/30;
   const saved={};
   for(const name of ['rvm','old']){
    const bitmap=await createImageBitmap(input,name==='old'?{resizeWidth:384,resizeHeight:216}:{}),worker=workers[name];
    const d=await new Promise((resolve,reject)=>{worker.onmessage=e=>e.data.type==='mask'?resolve(e.data):e.data.type==='error'?reject(Error(e.data.message)):null;worker.postMessage({type:'frame',bitmap,epoch,timestamp:epoch*100000+index*(clip==='wave'?1000/30:40)},[bitmap]);});
    qualityMetrics.push({clip,index,name,inferMs:d.inferMs,segmentMs:d.segmentMs,ratio:d.ratio});
    let matte=d.bitmap;
    if(!matte){mask.width=d.width;mask.height=d.height;mask.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(d.pixels),d.width,d.height),0,0);matte=mask;}
    const comp=name==='rvm'?alphaComp:oldComp;
    let result=comp.draw(input,{x:0,y:0,width:1280,height:720},{mirror:false},{mode:'replace',image:black},matte,1280,720,name==='rvm');
    while(!result){await new Promise(requestAnimationFrame); result=comp.draw(input,{x:0,y:0,width:1280,height:720},{mirror:false},{mode:'replace',image:black},matte,1280,720,name==='rvm');}
    oc.drawImage(result,0,0);if([0,25,50,74,30,60,89].includes(index))saved[name]=output.toDataURL().split(',')[1];
    d.bitmap?.close();
   }
   return saved;
  };
 });
 let epoch=0;
 for(const clip of (fixed025?['chair']:['hair','curly','wave','glasses','chair'])){
  const files=(await readdir(root+'/frames/'+clip)).filter(x=>x.endsWith('.png')).sort();await mkdir(out+'/'+clip,{recursive:true});
  epoch++;
  for(let i=0;i<files.length;i++){
   const r=await page.evaluate(args=>frame(...args),[clip,files[i],i,epoch]);
   for(const [name,png]of Object.entries(r))await writeFile(`${out}/${clip}/${name}-${files[i]}`,Buffer.from(png,'base64'));
  }
  console.log('quality',clip,files.length);
 }
 await writeFile(out+'/metrics.json',JSON.stringify(await page.evaluate(()=>qualityMetrics),null,2));
 await page.evaluate(()=>{for(const w of Object.values(workers))w.terminate();alphaComp.close();oldComp.close();});
} finally {await browser.close();server.close();}
