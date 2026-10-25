// Dev only. The test portrait lives outside the repository and is never bundled.
import http from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const demo=path.dirname(fileURLToPath(import.meta.url)), engine=path.resolve(demo,'../../ui/engine');
const port=Number(process.env.ENGINE_DEMO_PORT || 4178);
const types={'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.wasm':'application/wasm','.jpg':'image/jpeg','.mp4':'video/mp4'};
const server=http.createServer(async(req,res)=>{
  const pathname=decodeURIComponent(new URL(req.url,'http://localhost').pathname);
  const root=pathname.startsWith('/engine/')?engine:demo;
  const publicClip=/^\/fixture\/(hair|curly|wave|glasses|chair)\.mp4$/.test(pathname);
  const file=publicClip?'/tmp/matting-bakeoff/footage/'+path.basename(pathname):pathname==='/fixture/portrait.jpg'?(process.env.ENGINE_PORTRAIT || '/tmp/engine-portrait.jpg'):path.resolve(root,'.'+(root===engine?pathname.slice(7):pathname==='/'?'/index.html':pathname));
  if(!publicClip&&pathname!=='/fixture/portrait.jpg'&&!file.startsWith(root+path.sep)){res.writeHead(403).end();return;}
  try {const info=await stat(file);if(!info.isFile())throw Error();res.writeHead(200,{'Content-Type':types[path.extname(file)] || 'application/octet-stream','Cache-Control':'no-store'});res.end(await readFile(file));}
  catch{res.writeHead(404).end('Not found');}
});
server.listen(port,'127.0.0.1',()=>console.log(`Engine demo: http://127.0.0.1:${port}`));
