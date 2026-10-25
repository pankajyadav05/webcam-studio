import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
import fs from 'node:fs/promises';
import {evaluationServer} from './server.mjs';
const out='/tmp/v5-evaluation';await fs.mkdir(out,{recursive:true});
const server=await evaluationServer();
const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,
 args:['--autoplay-policy=no-user-gesture-required','--enable-webgl','--ignore-gpu-blocklist']});
try {
 const page=await browser.newPage();page.on('pageerror',error=>console.error(error));page.on('console',message=>{if(message.type()==='error')console.error(message.text());});
 await page.goto(server.url);
 await page.evaluate(async()=>{
  const {createCompositor}=await import('/engine/compositor.js');
  const {createMatteFilter}=await import('/engine/matte.js');
  const {NativeProvider}=await import('/engine/providers/native.js');
  window.invoke=async(command,body)=>{
    if(command==='segmenter_info')return {available:true};
    const response=await fetch('/vision',{method:'POST',body});
    if(!response.ok)throw Error(await response.text());return response.arrayBuffer();
  };
  window.comp=createCompositor();window.provider=new NativeProvider({invoke});
  window.input=document.createElement('canvas');input.width=1280;input.height=720;
  window.output=document.querySelector('canvas');output.width=1280;output.height=720;
  window.oc=output.getContext('2d',{willReadFrequently:true});
  window.reference=document.createElement('canvas');reference.width=1280;reference.height=720;
  window.rc=reference.getContext('2d',{willReadFrequently:true});
  window.longTasks=[];new PerformanceObserver(list=>longTasks.push(...list.getEntries().map(e=>({start:e.startTime,duration:e.duration})))).observe({type:'longtask',buffered:true});
  let filter,shape='',previous=0;
  window.epoch=0;window.reset=()=>{window.epoch++;filter=null;shape='';previous=0;longTasks.length=0;};
  window.frame=async(clip,name,index,fps)=>{
    const image=new Image();image.src=`/frames/${clip}/${name}`;await image.decode();
    input.getContext('2d').drawImage(image,0,0,1280,720);
    const started=performance.now();
    const mask=await provider.segment({id:index+1,epoch:window.epoch || 0,image:input,mediaTime:(index+1)/fps});
    const latency=performance.now()-started;
    if(shape!==`${mask.width}x${mask.height}`){filter=createMatteFilter(mask.width,mask.height,{hysteresis:false});shape=`${mask.width}x${mask.height}`;}
    const values=Float32Array.from(mask.alpha,v=>v/255);
    const before=performance.now(),alpha=filter.apply(values,1000/fps,index===0),temporalMs=performance.now()-before;
    const renderStart=performance.now();
    const result=comp.draw({image:input,frameKey:index,mask:{...mask,alpha},background:{mode:'replace'},width:1280,height:720,alphaOnly:true});
    oc.drawImage(result,0,0);
    const renderMs=performance.now()-renderStart;
    // Readback synchronizes GPU work; recorded separately from submission/presentation.
    const sync=performance.now();oc.getImageData(0,0,1280,720);const readbackMs=performance.now()-sync;
    const small=document.createElement('canvas');small.width=mask.width;small.height=mask.height;
    const data=small.getContext('2d').createImageData(mask.width,mask.height);
    for(let i=0;i<mask.alpha.length;i++){data.data[i*4]=data.data[i*4+1]=data.data[i*4+2]=mask.alpha[i];data.data[i*4+3]=255;}
    small.getContext('2d').putImageData(data,0,0);rc.drawImage(small,0,0,1280,720);
    return {latency,segmentMs:mask.segmentMs,temporalMs,renderMs,readbackMs,png:output.toDataURL().split(',')[1],raw:reference.toDataURL().split(',')[1]};
  };
 });
 const clips=JSON.parse(await fs.readFile('/tmp/matting-bakeoff/clips.json','utf8')),timings={};
 for(const clip of (process.argv.slice(2).length?process.argv.slice(2):['wave','hair','curly','chair','glasses','exit'])) {
  const files=(await fs.readdir(`/tmp/matting-bakeoff/out/.inputs/${clip}`)).filter(n=>n.endsWith('.png')).sort();
  await fs.mkdir(`${out}/${clip}/new`,{recursive:true});await fs.mkdir(`${out}/${clip}/raw`,{recursive:true});
  await page.evaluate(()=>reset());timings[clip]=[];
  for(let i=0;i<files.length;i++) {
    const result=await page.evaluate(args=>frame(...args),[clip,files[i],i,clips[clip].fps]);
    const {png,raw,...timing}=result;timings[clip].push(timing);
    await fs.writeFile(`${out}/${clip}/new/${files[i]}`,Buffer.from(png,'base64'));
    await fs.writeFile(`${out}/${clip}/raw/${files[i]}`,Buffer.from(raw,'base64'));
  }
  await fs.writeFile(`${out}/${clip}/timing.json`,JSON.stringify({frames:timings[clip],longTasks:await page.evaluate(()=>longTasks)},null,2));
  console.log('DONE',clip,files.length);
 }
 await fs.writeFile(`${out}/timings.json`,JSON.stringify(timings,null,2));
}finally{await browser.close();server.close();}
