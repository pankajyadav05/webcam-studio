// Run benchmark hosts sequentially so they do not compete for the GPU.
import { spawn } from 'node:child_process';
import { open,writeFile,readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
const dir='/tmp/engine-shots/v4',demo='apps/desktop/dev/engine-demo/';
const resume=process.argv.includes('--resume');
const progress=resume?JSON.parse(await readFile(dir+'/validation-progress.json','utf8')):{started:new Date().toISOString(),steps:[],engineHashes:{}};
for(const f of ['studio-engine.js','hybrid-gate.js','rvm.js','rvm-worker.js']) progress.engineHashes[f]=createHash('sha256').update(await readFile('apps/desktop/ui/engine/'+f)).digest('hex');
async function record(){await writeFile(dir+'/validation-progress.json',JSON.stringify(progress,null,2));}
async function run(name,cmd,args){
 if(resume && progress.steps.some(s=>s.name===name&&s.exitCode===0))return;
 const step={name,started:new Date().toISOString()};progress.steps.push(step);await record();
 const log=await open(dir+'/validation-'+name+'.log','w');
 const child=spawn(cmd,args,{stdio:['ignore',log.fd,log.fd]});
 const code=await new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',resolve);});await log.close();
 step.finished=new Date().toISOString();step.exitCode=code;await record();if(code!==0)throw Error(name+' failed: '+code);
}
await run('quality-chrome','node',[demo+'verify-hybrid-quality.mjs']);
await run('chrome','sh',[demo+'measure-hybrid-chrome.sh']);
await run('native','sh',[demo+'measure-hybrid-native.sh']);
const log=await open(dir+'/native-quality-server.log','w');
const server=spawn('node',[demo+'verify-hybrid-quality.mjs','--native'],{stdio:['ignore',log.fd,log.fd]});
try{
 let ready=false;for(let i=0;i<20;i++){try{ready=(await fetch('http://127.0.0.1:4181/quality-manifest.json')).ok;if(ready)break;}catch{}await new Promise(r=>setTimeout(r,250));}if(!ready)throw Error('Native asset server failed to start');
 await run('quality-native','/tmp/verify-hybrid-webkit',['--asset','--quality']);
}finally{server.kill('SIGTERM');await log.close();}
await run('soak','node',[demo+'soak-hybrid.mjs','600']);
await run('units','node',['--test','apps/desktop/ui/engine/studio-engine.test.mjs','apps/desktop/ui/engine/rvm-policy.test.mjs','apps/desktop/ui/engine/hybrid-gate.test.mjs','apps/desktop/ui/engine/offline-fetch.test.mjs']);
await run('transforms','node',[demo+'verify-rvm-transform.mjs']);
progress.finished=new Date().toISOString();await record();
