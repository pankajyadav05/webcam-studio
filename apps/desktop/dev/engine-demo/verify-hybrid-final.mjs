// Recheck shader output/timings after the recovery-edge taper. No model or
// allocation path changed after the resource soak.
import {spawn} from 'node:child_process';
import {open,readFile,writeFile,copyFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const dir='/tmp/engine-shots/v4',demo='apps/desktop/dev/engine-demo/',steps=[];
async function run(name,cmd,args){
 steps.push({name,started:new Date().toISOString()});await writeFile(dir+'/final-progress.json',JSON.stringify(steps,null,2));
 const log=await open(dir+'/final-'+name+'.log','w'),p=spawn(cmd,args,{stdio:['ignore',log.fd,log.fd]});
 const code=await new Promise((r,j)=>{p.on('exit',r);p.on('error',j);});await log.close();steps.at(-1).exitCode=code;steps.at(-1).finished=new Date().toISOString();await writeFile(dir+'/final-progress.json',JSON.stringify(steps,null,2));if(code!==0)throw Error(name+' failed: '+code);
}
for(const name of ['chrome-webgpu-blur.json','chrome-webgpu-replace.json','webkit-hybrid-webgpu-blur-normal-report.json','webkit-hybrid-webgpu-replace-normal-report.json'])await copyFile(dir+'/'+name,dir+'/pre-taper-'+name);
await run('gpu','node',[demo+'verify-hybrid-gpu.mjs']);
await run('quality-chrome','node',[demo+'verify-hybrid-quality.mjs']);
for(const mode of ['blur','replace'])await run('chrome-'+mode,'node',[demo+'verify-hybrid.mjs','webgpu','25',mode]);
await run('native-blur','/tmp/verify-hybrid-webkit',['--asset','--clip']);
await run('native-replace','/tmp/verify-hybrid-webkit',['--asset','--clip','--replace']);
const log=await open(dir+'/final-native-quality-server.log','w'),server=spawn('node',[demo+'verify-hybrid-quality.mjs','--native'],{stdio:['ignore',log.fd,log.fd]});
try{
 let ready=false;for(let i=0;i<20;i++){try{ready=(await fetch('http://127.0.0.1:4181/quality-manifest.json')).ok;if(ready)break;}catch{}await new Promise(r=>setTimeout(r,250));}if(!ready)throw Error('Asset server failed');
 await run('quality-native','/tmp/verify-hybrid-webkit',['--asset','--quality']);
}finally{server.kill('SIGTERM');await log.close();}
const hashes={};for(const f of ['studio-engine.js','hybrid-gate.js','offline-fetch.js','rvm.js','rvm-worker.js'])hashes[f]=createHash('sha256').update(await readFile('apps/desktop/ui/engine/'+f)).digest('hex');await writeFile(dir+'/final-engine-hashes.json',JSON.stringify(hashes,null,2));
