import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../../..');
export async function evaluationServer() {
  const child = spawn(path.join(repo,'target/debug/examples/vision_pipe'),[],{stdio:['pipe','pipe','inherit']});
  let buffered=Buffer.alloc(0), waiting=[];
  child.stdout.on('data',chunk=>{
    buffered=Buffer.concat([buffered,chunk]);
    while(buffered.length>=4 && buffered.length>=4+buffered.readUInt32LE(0)) {
      const size=buffered.readUInt32LE(0), reply=buffered.subarray(4,4+size);
      buffered=buffered.subarray(4+size);waiting.shift()?.(reply);
    }
  });
  const server=http.createServer(async(req,res)=>{
    try {
      const url=new URL(req.url,'http://localhost');
      if(url.pathname==='/favicon.ico'){res.writeHead(204);res.end();return;}
      if(url.pathname==='/vision') {
        const chunks=[];for await(const part of req)chunks.push(part);
        const bytes=Buffer.concat(chunks),size=Buffer.alloc(4);size.writeUInt32LE(bytes.length);
        const reply=await new Promise(resolve=>{waiting.push(resolve);child.stdin.write(size);child.stdin.write(bytes);});
        res.writeHead(reply.subarray(0,4).toString()==='ERR:'?500:200,{'Content-Type':'application/octet-stream'});res.end(reply);return;
      }
      let file;
      if(url.pathname==='/') {res.setHeader('Content-Type','text/html');res.end('<!doctype html><canvas id="output"></canvas>');return;}
      if(url.pathname.startsWith('/engine/')) file=path.join(repo,'apps/desktop/ui',url.pathname);
      else if(url.pathname.startsWith('/frames/')) {
        const [, ,clip,name]=url.pathname.split('/');
        if(!['chair','curly','exit','glasses','hair','wave'].includes(clip))throw Error('not a public clip');
        file=path.join('/tmp/matting-bakeoff/out/.inputs',clip,name);
      } else if(url.pathname.startsWith('/footage/')) {
        const name=path.basename(url.pathname);
        if(!['chair.mp4','curly.mp4','exit.mp4','glasses.mp4','hair.mp4','wave.mp4'].includes(name))throw Error('not a public clip');
        file=path.join('/tmp/matting-bakeoff/footage',name);
      } else if(url.pathname.startsWith('/legacy/vendor/'))file=path.join(repo,'apps/desktop/ui/engine/vendor',url.pathname.slice('/legacy/vendor/'.length));
      else if(url.pathname==='/legacy/studio-engine.js')file=path.join(repo,'apps/desktop/dev/experimental/engine/legacy-engine.js');
      else if(url.pathname.startsWith('/legacy/'))file=path.join(repo,'apps/desktop/dev/experimental/engine',url.pathname.slice('/legacy/'.length));
      else file=path.join(repo,'apps/desktop/dev/evaluation',url.pathname);
      const ext=path.extname(file);let data=await fs.readFile(file);
      if(url.pathname==='/legacy/studio-engine.js') {
        // Measurement-only hooks preserve the old model/cleanup/scheduler.
        data=data.toString().replace('pixels: pixels.buffer, segmentMs:', 'mediaTime: data.mediaTime, captureTimestamp: timestamp, pixels: pixels.buffer, segmentMs:')
          .replace('timestamp: now, recycled: buffer','timestamp: now, mediaTime: capturedMediaTime, recycled: buffer')
          .replace('busy = false; segmentMs =','globalThis.__oldMaskTime=data.mediaTime; globalThis.__oldMaskLatency=performance.now()-data.captureTimestamp; busy = false; segmentMs =')
          .replace('stats() { return { gateInferMs:', 'stats() { return { maskLatencyMs:globalThis.__oldMaskLatency, gateInferMs:')
          .replace('const isNewFrame = fresh;', 'globalThis.__oldPaint?.({mediaTime:video.currentTime,maskTime:globalThis.__oldMaskTime}); const isNewFrame = fresh;');
      }
      res.setHeader('Content-Type',({'.js':'text/javascript','.mjs':'text/javascript','.png':'image/png','.wasm':'application/wasm','.mp4':'video/mp4','.html':'text/html'})[ext]||'application/octet-stream');
      res.end(data);
    }catch(error){res.writeHead(404);res.end(String(error));}
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {url:`http://127.0.0.1:${server.address().port}`,close(){server.close();child.kill();}};
}
