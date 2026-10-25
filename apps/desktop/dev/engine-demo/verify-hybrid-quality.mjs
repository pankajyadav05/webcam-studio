import http from 'node:http';
import { readFile, readdir, mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import puppeteer from '/tmp/pw/node_modules/puppeteer-core/lib/puppeteer/puppeteer-core.js';
const root=path.resolve('apps/desktop/dev/engine-demo'),engine=path.resolve('apps/desktop/ui/engine');
const out='/tmp/engine-shots/v4/'+(process.argv.includes('--native')?'native-quality':'chrome-quality');await mkdir(out,{recursive:true});
const manifest={};for(const clip of (process.env.QUALITY_CLIPS ? process.env.QUALITY_CLIPS.split(',') : ['chair','office','curly','wave','glasses','hair','exit','spin','chair-close','bright-chair'])){
 const folder=['office','spin','chair-close','bright-chair'].includes(clip)?'/tmp/engine-shots/v4/frames/'+clip:'/tmp/matting-bakeoff/frames/'+clip;
 manifest[clip]=(await readdir(folder)).filter(x=>x.endsWith('.png')).sort().map(x=>'/frames/'+clip+'/'+x);
}
if(manifest.wave)manifest['wave-fast']=manifest.wave.filter((_,i)=>i%2===0);
const csp="default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; img-src 'self' data: blob:; media-src 'self' blob: mediastream:; connect-src 'self' ipc: http://ipc.localhost";
const server=http.createServer(async(req,res)=>{try{
 const p=new URL(req.url,'http://localhost').pathname;let bytes,mime='application/octet-stream';
 if(p.startsWith('/save/')){const f=path.resolve(out,p.slice(6));if(!f.startsWith(out+'/'))throw Error('Invalid artifact');const chunks=[];for await(const c of req)chunks.push(c);const data=Buffer.concat(chunks);await mkdir(path.dirname(f),{recursive:true});await writeFile(f,p.endsWith('.json')?data:Buffer.from(data.toString(),'base64'));res.end('ok');return;}
 if(p==='/quality-manifest.json'){bytes=JSON.stringify(manifest);mime='application/json';}
 else if(['/engine/quality-engine.js','/engine/alpha-engine.js'].includes(p)){bytes=(await readFile(engine+'/studio-engine.js','utf8'))+'\nexport { gpuCompositor };';if(p.includes('alpha-engine'))bytes=bytes.replace('color=vec4(mix(bg.rgb,sharp.rgb,a),1.);','color=vec4(vec3(a),1.);');mime='text/javascript';}
 else {const f=p.startsWith('/frames/')?(['office','spin','chair-close','bright-chair'].includes(p.split('/')[2])?'/tmp/engine-shots/v4':'/tmp/matting-bakeoff')+p:p.startsWith('/engine/')?engine+p.slice(7):root+p;bytes=await readFile(f);if(p.endsWith('rvm-worker.js'))bytes=bytes.toString().replace('policy.sample(result.segmentMs)','null');mime=/\.m?js$/.test(f)?'text/javascript':f.endsWith('.html')?'text/html':f.endsWith('.png')?'image/png':f.endsWith('.wasm')?'application/wasm':mime;}
 res.writeHead(200,{'Content-Type':mime,'Content-Security-Policy':csp});res.end(bytes);
 }catch(e){res.writeHead(404).end(String(e));}});
await new Promise(r=>server.listen(4181,'127.0.0.1',r));
if(process.argv.includes('--native')){console.log('Native quality asset server on 4181');}
else {
 const browser=await puppeteer.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true,args:['--autoplay-policy=no-user-gesture-required','--ignore-gpu-blocklist']});
 try{const page=await browser.newPage();page.on('console',m=>{if(m.text().startsWith('QUALITY'))console.log(m.text());});await page.goto('http://127.0.0.1:4181/hybrid-quality.html');await page.waitForFunction(()=>window.quality?.done,{timeout:300000});console.log(await page.evaluate(()=>({errors:quality.errors,violations:quality.violations,frames:quality.metrics.length})));}finally{await browser.close();server.close();}
}
