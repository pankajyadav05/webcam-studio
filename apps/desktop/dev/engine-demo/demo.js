import { createStudioEngine } from '/engine/studio-engine.js';
import { chairFixture, drawChair } from './matte-fixture.js';
const canvas = document.querySelector('#output'), video = document.querySelector('#source');
const engine = createStudioEngine({ canvas, debug: true });
const statuses = [], violations = [], longTasks = [];
engine.on('status', s => statuses.push(s));
document.addEventListener('securitypolicyviolation', event => violations.push({ directive:event.violatedDirective, uri:event.blockedURI }));
if (PerformanceObserver.supportedEntryTypes.includes('longtask')) new PerformanceObserver(list => longTasks.push(...list.getEntries().map(e => ({start:e.startTime,duration:e.duration})))).observe({type:'longtask'});
let stream, timer, mirror = false;
async function useStream(next) {
  clearInterval(timer); stream?.getTracks().forEach(t => t.stop()); stream=next;
  video.srcObject=stream; await video.play(); engine.setSource(video); engine.start();
}
async function clip(name = 'wave') {
  clearInterval(timer); stream?.getTracks().forEach(t => t.stop()); video.srcObject = null;
  const response = await fetch(`/fixture/${name}.mp4`);
  if (!response.ok) throw new Error('Public test clip is unavailable.');
  if (video.src.startsWith('blob:')) URL.revokeObjectURL(video.src);
  video.src = URL.createObjectURL(new Blob([await response.arrayBuffer()], { type: 'video/mp4' }));
  video.loop = true; await video.play();
  engine.setSource(video); engine.start();
}
async function camera() { await useStream(await navigator.mediaDevices.getUserMedia({video:{width:1280,height:720,frameRate:30},audio:false})); }
async function portrait(chair = false) {
  const image=new Image(); image.src='/fixture/portrait.jpg'; await image.decode();
  const input=document.createElement('canvas'); input.width=1280; input.height=720;
  const ctx=input.getContext('2d'); let tick=0;
  const cutout=chair ? chairFixture(image) : null;
  const still=new URLSearchParams(location.search).has('still');
  window.fixtureInput=input;
  function paint() {
    if (chair) { drawChair(ctx,cutout,still?0:tick++); return; }
    ctx.fillStyle='#a25119';ctx.fillRect(0,0,1280,720);
    for(let x=0;x<1280;x+=32) { ctx.fillStyle=x%64?'#21b2ce':'#2939b0';ctx.fillRect(x,0,16,720); }
    const width=720*image.width/image.height;
    ctx.drawImage(image,(1280-width)/2+Math.sin((still?0:tick++)/40)*12,0,width,720);
  }
  paint(); const playing=useStream(input.captureStream(30)); timer=setInterval(paint,1000/30); await playing;
}
const replacement=document.createElement('canvas'); replacement.width=1280; replacement.height=720;
const rc=replacement.getContext('2d');rc.fillStyle='#159c7c';rc.fillRect(0,0,1280,720);
rc.fillStyle='#103a58';rc.fillRect(0,0,320,720);rc.fillStyle='white';rc.font='72px system-ui';rc.fillText('LEFT',36,110);rc.fillText('STUDIO',900,650);
if(new URLSearchParams(location.search).has('flat')) {rc.fillStyle='#667a8c';rc.fillRect(0,0,1280,720);}
if(new URLSearchParams(location.search).has('matting')) engine.setMatting(new URLSearchParams(location.search).get('matting'));
function mode(mode) { engine.setBackground({mode,radius:18,image:replacement}); }
for(const name of ['none','blur','replace']) document.querySelector('#'+name).onclick=()=>mode(name);
document.querySelector('#camera').onclick=()=>camera();document.querySelector('#portrait').onclick=()=>portrait();
document.querySelector('#mirror').onclick=()=>engine.setFraming({mirror:mirror=!mirror});
setInterval(()=>document.querySelector('#status').textContent=JSON.stringify({status:statuses.at(-1),...engine.stats()},null,2),500);
window.demo={engine,canvas,video,camera,portrait,clip,mode,replacement,statuses,violations,longTasks};
if (new URLSearchParams(location.search).has('clip')) await clip(new URLSearchParams(location.search).get('clip'));
else if (new URLSearchParams(location.search).has('portrait')) await portrait(new URLSearchParams(location.search).has('chair')); else await camera();
