// Harness-only exports/readback inspect the production workers and compositor.
import { gpuCompositor } from '/engine/quality-engine.js';
import { gpuCompositor as alphaCompositor } from '/engine/alpha-engine.js';
const output=document.querySelector('canvas'),oc=output.getContext('2d',{willReadFrequently:true});
const input=document.createElement('canvas');input.width=input.videoWidth=1280;input.height=input.videoHeight=720;
const ic=input.getContext('2d'),mask=document.createElement('canvas'),gateRead=document.createElement('canvas');
const background=document.createElement('canvas');background.width=1280;background.height=720;
background.getContext('2d').fillStyle='#667a8c';background.getContext('2d').fillRect(0,0,1280,720);
const comp=gpuCompositor(),alpha=alphaCompositor(),workers={},metrics=[];
const metricCanvas=document.createElement('canvas');metricCanvas.width=320;metricCanvas.height=180;const metricContext=metricCanvas.getContext('2d',{willReadFrequently:true});
const violations=[],errors=[];window.quality={metrics,violations,errors,done:false};
document.addEventListener('securitypolicyviolation',e=>violations.push({directive:e.violatedDirective,uri:e.blockedURI}));
window.addEventListener('error',e=>errors.push(e.message));
async function save(name,value,json=false){
 const data=json?JSON.stringify(value,null,2):value.toDataURL().split(',')[1];
 if(window.webkit?.messageHandlers.artifact) window.webkit.messageHandlers.artifact.postMessage({name,data,json});
 else {const r=await fetch('/save/'+name,{method:'POST',headers:{'Content-Type':json?'application/json':'text/plain'},body:data});if(!r.ok)throw Error('Artifact save failed');}
}
async function ready(name,gate=false){
 const w=new Worker('/engine/'+(name==='rvm'?'rvm-worker.js':'studio-engine.js'),{type:'module'});workers[name]=w;
 await new Promise((resolve,reject)=>{w.onerror=e=>reject(Error(e.message));w.onmessage=({data})=>data.type==='ready'?resolve():data.type==='error'?reject(Error(data.message)):null;w.postMessage({type:'init',gate,reference:name==='reference',auto:false});});
}
async function infer(name,epoch,timestamp){
 const bitmap=await createImageBitmap(input,name==='rvm'?{}:{resizeWidth:384,resizeHeight:216}),w=workers[name];
 return new Promise((resolve,reject)=>{w.onmessage=({data})=>['mask','gate'].includes(data.type)?resolve(data):['error','ready'].includes(data.type)?reject(Error(data.message||('Unexpected worker reinitialization '+name))):null;w.postMessage({type:'frame',bitmap,epoch,timestamp},[bitmap]);});
}
async function draw(c,matte,soft,gate,mode){
 let result;
 do {result=c.draw(input,{x:0,y:0,width:1280,height:720},{mirror:false},{mode,image:background,radius:18},matte,1280,720,soft,gate?{...gate,timestamp:performance.now()-gate.ageMs}:null);if(!result)await new Promise(requestAnimationFrame);}while(!result);
 oc.drawImage(result,0,0);return output;
}
try{
 await ready('rvm');await ready('mediapipe');await ready('gate',true);await ready('reference',true);
 const manifest=await (await fetch('/quality-manifest.json')).json();let epoch=0;
 for(const [clip,files]of Object.entries(manifest)){
  epoch++;let latest=null,latestIndex=0;
  for(let i=0;i<files.length;i++){
   const im=new Image();im.src=files[i];await im.decode();ic.drawImage(im,0,0,1280,720);input.currentTime=i/30;
   const timestamp=epoch*100000+i*(clip.startsWith('wave')||clip==='exit'?1000/30:40);
   // Gate rate is lower than RVM and deliberately one input frame behind.
   const rvm=await infer('rvm',epoch,timestamp),mp=await infer('mediapipe',epoch,timestamp);
   const saved=[0,Math.floor(files.length/3),Math.floor(files.length*2/3),files.length-1].includes(i);
   const gate=latest?{...latest,ageMs:(i-latestIndex)*(clip.startsWith('wave')?1000/30:40)}:null;
   mask.width=mp.width;mask.height=mp.height;mask.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(mp.pixels),mp.width,mp.height),0,0);
   if(saved){
    await save(`${clip}/source-${i}.png`,input);
    for(const [mode,matte,soft,g]of [['rvm',rvm.bitmap,true,null],['mediapipe',mask,false,null],['hybrid',rvm.bitmap,true,gate]]){
     for(const view of ['alpha','replace','blur'])await save(`${clip}/${mode}-${view}-${i}.png`,await draw(view==='alpha'?alpha:comp,matte,soft,g,view==='alpha'?'replace':view));
    }
   }
   const reference=await infer('reference',epoch,timestamp);
   metricContext.drawImage(reference.bitmap,0,0,320,180);
   const ref=metricContext.getImageData(0,0,320,180).data;
   const outside=new Uint8Array(320*180);let outsideCount=0;
   for(let k=0;k<outside.length;k++) {outside[k]=ref[k*4+1]*256/255>24?1:0;outsideCount+=outside[k];}
   const leakage={};let rvmAlpha;
   for(const [name,matte,soft,g]of [['rvm',rvm.bitmap,true,null],['mediapipe',mask,false,null],['hybrid',rvm.bitmap,true,gate]]){
    await draw(alpha,matte,soft,g,'replace');metricContext.drawImage(output,0,0,320,180);
    const values=metricContext.getImageData(0,0,320,180).data;let leaked=0,lost=0;
    for(let k=0;k<outside.length;k++) {if(outside[k]&&values[k*4]>127)leaked++;if(name==='hybrid'&&!outside[k]&&rvmAlpha[k*4]>250&&values[k*4]<127)lost++;}
    leakage[name]=100*leaked/Math.max(1,outsideCount);
    if(name==='rvm')rvmAlpha=values;
    if(name==='hybrid')leakage.supportedOpaqueLossPixels=lost*16;
   }
   metrics.push({clip,index:i,rvm:rvm.inferMs,mp:mp.segmentMs,gate:latest?.segmentMs,ratio:rvm.ratio,gateSpeed:latest?.params.speed,gateAgeFrames:gate?i-latestIndex:null,leakage});
   rvm.bitmap.close();
   if(i%2===0 || !latest){latest?.bitmap.close();latest=await infer('gate',epoch,timestamp);latestIndex=i;}
   if(saved){
    gateRead.width=reference.bitmap.width;gateRead.height=reference.bitmap.height;gateRead.getContext('2d').drawImage(reference.bitmap,0,0);
    await save(`${clip}/reference-${i}.png`,gateRead);
   }
   reference.bitmap.close();
  }
  latest?.bitmap.close();console.log('QUALITY',clip,files.length);
 }
 await save('metrics.json',{metrics,violations,errors},true);
 for(const w of Object.values(workers))w.terminate();comp.close();alpha.close();window.quality.done=true;
}catch(e){errors.push(e.stack||String(e));window.quality.done=true;await save('error.json',{errors,violations},true);}
