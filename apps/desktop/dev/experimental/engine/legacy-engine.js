/** Offline camera compositor. Models load only when an effect is requested. */
import { createGate, GATE } from './hybrid-gate.js';
import { offlineFetch } from './offline-fetch.js';
export const OUTPUT_DEFAULT = { width: 1280, height: 720 };
const MODULE_URL = import.meta.url;
const ASSETS = new URL('./vendor/', MODULE_URL);
const finite = (n, fallback) => Number.isFinite(Number(n)) ? Number(n) : fallback;
const clamp = (n, lo, hi, fallback) => Math.max(lo, Math.min(hi, finite(n, fallback)));
const framingDefaults = { zoom: 1, pan_x: 0, pan_y: 0, mirror: false };

export function cropRect(inW, inH, outW, outH, framing = framingDefaults) {
  if (!(inW > 0 && inH > 0) || !Number.isFinite(inW + inH)) {
    return { x: 0, y: 0, width: 0, height: 0 };
  }
  const aspect = outW > 0 && outH > 0 && Number.isFinite(outW + outH) ? outW / outH : inW / inH;
  const zoom = clamp(framing?.zoom, 1, 4, 1);
  const width = Math.min(inW, inH * aspect) / zoom;
  const height = Math.min(inH, inW / aspect) / zoom;
  return {
    x: (inW - width) * (clamp(framing?.pan_x, -1, 1, 0) + 1) / 2,
    y: (inH - height) * (clamp(framing?.pan_y, -1, 1, 0) + 1) / 2,
    width, height,
  };
}

// Weak chair/background predictions must reach zero; only a narrow confidence
// band should remain translucent around hair and the person's silhouette.
const MATTE_LOW = 0.40, MATTE_HIGH = 0.75;
// A 0.65 core rejected more faint chair bridges than 0.50 in the fixture. Keeping
// sizable islands also preserves a second person who is not touching the first.
const COMPONENT_THRESHOLD = 0.65, COMPONENT_AREA_RATIO = 0.15;
const MATTE_SMOOTH_MS = 30;
function confidenceAlpha(confidence) {
  const t = Math.max(0, Math.min(1, (confidence - MATTE_LOW) / (MATTE_HIGH - MATTE_LOW)));
  return t * t * (3 - 2 * t);
}
function createMatteFilter(width, height) {
  const count = width * height, average = new Float32Array(count);
  const labels = new Uint32Array(count), queue = new Uint32Array(count);
  const areas = new Uint32Array(count + 1), keep = new Uint8Array(count);
  return (values, elapsed, reset, pixels) => {
    const blend = 1 - Math.exp(-Math.max(1, elapsed) / MATTE_SMOOTH_MS);
    labels.fill(0); let components = 0, largest = 0;
    for (let i = 0; i < count; i++) {
      // Motion gets an immediate response; small confidence noise still settles.
      const a = reset ? 1 : Math.max(blend, Math.abs(values[i] - average[i]) > 0.20 ? 0.9 : 0);
      average[i] += a * (values[i] - average[i]);
    }
    for (let i = 0; i < count; i++) {
      if (labels[i] || average[i] < COMPONENT_THRESHOLD) continue;
      const label = ++components; let head = 0, tail = 1; queue[0] = i; labels[i] = label;
      while (head < tail) {
        const at = queue[head++], x = at % width, y = (at / width) | 0;
        for (let ny = Math.max(0, y - 1); ny <= Math.min(height - 1, y + 1); ny++) {
          for (let nx = Math.max(0, x - 1); nx <= Math.min(width - 1, x + 1); nx++) {
            const next = ny * width + nx;
            if (!labels[next] && average[next] >= COMPONENT_THRESHOLD) { labels[next] = label; queue[tail++] = next; }
          }
        }
      }
      areas[label] = tail; largest = Math.max(largest, tail);
    }
    for (let i = 0; i < count; i++) keep[i] = labels[i] && areas[labels[i]] >= largest * COMPONENT_AREA_RATIO ? 1 : 0;
    for (let i = 0, j = 0; i < count; i++, j += 4) {
      let retained = keep[i];
      // One texel of soft support protects hair without growing arbitrary faint
      // regions back onto the cleaned core. Rejected islands never accumulate.
      if (!retained && !labels[i] && average[i] > MATTE_LOW) {
        const x = i % width, y = (i / width) | 0;
        for (let ny = Math.max(0, y - 1); !retained && ny <= Math.min(height - 1, y + 1); ny++) {
          for (let nx = Math.max(0, x - 1); nx <= Math.min(width - 1, x + 1); nx++) retained |= keep[ny * width + nx];
        }
      }
      pixels[j] = pixels[j + 1] = pixels[j + 2] = 255;
      pixels[j + 3] = retained ? Math.round(confidenceAlpha(average[i]) * 255) : 0;
    }
    return pixels;
  };
}

// Keep the loader out of the main thread, and reuse its explicit module export:
// MediaPipe clears globalThis.ModuleFactory after each task initialization.
async function makeSegmenter(delegate) {
  const [{ ImageSegmenter }, { default: factory }] = await Promise.all([
    import(new URL('vision_bundle.mjs', ASSETS).href),
    import(new URL('wasm/vision_wasm_module_internal.js', ASSETS).href),
  ]);
  globalThis.ModuleFactory = factory;
  return ImageSegmenter.createFromOptions({
    wasmLoaderPath: '',
    wasmBinaryPath: new URL('wasm/vision_wasm_module_internal.wasm', ASSETS).href,
  }, {
    baseOptions: { modelAssetPath: new URL('selfie_segmenter.tflite', ASSETS).href, delegate },
    runningMode: 'VIDEO', outputCategoryMask: false, outputConfidenceMasks: true,
    ...(typeof OffscreenCanvas !== 'undefined' ? { canvas: new OffscreenCanvas(1, 1) } : {}),
  });
}

// This same URL is a module-worker entry point. No blob code, eval, or inline scripts.
const inWorker = typeof WorkerGlobalScope !== 'undefined' && globalThis instanceof WorkerGlobalScope;
if (inWorker) {
  globalThis.fetch = offlineFetch(MODULE_URL);
  let segmenter, backend, filter, maskWidth = 0, maskHeight = 0, pixels, previousTime = 0, generation = -1, gateMode = false, referenceMode = false, gateFilter, gateCanvas, gateContext, gateImage;
  globalThis.onmessage = async ({ data }) => {
    if (data.type === 'init') {
      gateMode = !!data.gate;referenceMode=!!data.reference;
      try {
        // A coarse gate does not need to compete with RVM for GPU scheduling.
        if(gateMode) {segmenter=await makeSegmenter('CPU');backend='cpu';}
        else {
          try { segmenter = await makeSegmenter('GPU'); backend = 'gpu'; }
          catch { segmenter = await makeSegmenter('CPU'); backend = 'cpu'; }
        }
        globalThis.postMessage({ type: 'ready', backend });
      } catch (error) { globalThis.postMessage({ type: 'error', message: error.message }); }
    } else if (data.type === 'close') {
      segmenter?.close(); globalThis.close();
    } else if (data.type === 'frame') {
      const { bitmap, epoch, timestamp, recycled } = data;
      const start = performance.now();
      try {
        if (!segmenter) return;
        // Copy out during the callback: the task owns the confidence buffer and
        // releases it as soon as this callback returns.
        segmenter.segmentForVideo(bitmap, timestamp, result => {
          const masks = result.confidenceMasks;
          const labels = segmenter.getLabels();
          const person = labels.findIndex(label => /person|selfie/i.test(label));
          // This model has one foreground confidence mask (some Tasks builds
          // expose background+person instead). Never mistake background for person.
          const mask = masks?.[person >= 0 ? person : masks.length - 1];
          if (!mask) throw new Error('The person model returned no confidence mask.');
          const values = mask.getAsFloat32Array();
          const reset = generation !== epoch || maskWidth !== mask.width || maskHeight !== mask.height;
          if (reset) { filter = createMatteFilter(mask.width, mask.height); maskWidth = mask.width; maskHeight = mask.height; }
          if (gateMode) {
            if (reset || !gateFilter) { gateFilter = createGate(mask.width, mask.height, bitmap.width/bitmap.height, referenceMode); gateCanvas = new OffscreenCanvas(mask.width, mask.height); gateContext = gateCanvas.getContext('2d');gateImage=gateContext.createImageData(mask.width,mask.height); }
            const image = gateImage;
            const params = gateFilter(values, timestamp, reset, image.data);
            gateContext.putImageData(image, 0, 0);
            const gateBitmap = gateCanvas.transferToImageBitmap();
            globalThis.postMessage({ type: 'gate', epoch, timestamp, bitmap: gateBitmap, params, segmentMs: performance.now()-start }, [gateBitmap]);
            generation = epoch; previousTime = timestamp;
            return;
          }
          generation = epoch;
          pixels = recycled?.byteLength === values.length * 4 ? new Uint8ClampedArray(recycled) : new Uint8ClampedArray(values.length * 4);
          filter(values, timestamp - previousTime, reset, pixels);
          previousTime = timestamp;
          globalThis.postMessage({ type: 'mask', epoch, width: mask.width, height: mask.height,
            pixels: pixels.buffer, segmentMs: performance.now() - start }, [pixels.buffer]);
        });
      } catch (error) {
        if (backend === 'gpu') {
          try {
            segmenter?.close(); segmenter = await makeSegmenter('CPU'); backend = 'cpu';
            globalThis.postMessage({ type: 'ready', backend, message: `MediaPipe GPU failed: ${error.message}; using CPU.` });
          } catch (fallbackError) { globalThis.postMessage({ type: 'error', message: fallbackError.message }); }
        } else globalThis.postMessage({ type: 'error', message: error.message });
      } finally { bitmap.close(); }
    }
  };
}

function surface(width = 1, height = 1) {
  // HTML canvas also works in WebKit versions with incomplete OffscreenCanvas 2D.
  const c = document.createElement('canvas'); c.width = width; c.height = height; return c;
}
function imageSize(image) {
  return [image?.videoWidth || image?.naturalWidth || image?.displayWidth || image?.width || 0,
    image?.videoHeight || image?.naturalHeight || image?.displayHeight || image?.height || 0];
}
function cover(ctx, image, width, height) {
  const [iw, ih] = imageSize(image);
  if (!iw || !ih) return false;
  const r = cropRect(iw, ih, width, height);
  ctx.drawImage(image, r.x, r.y, r.width, r.height, 0, 0, width, height); return true;
}

// A separable blur at quarter resolution keeps both blur passes cheap. Unlike
// Canvas2D.filter this also works on Safari 17, and clamp-to-edge avoids dark rims.
function gpuCompositor() {
  const canvas = surface();
  const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, depth: false, preserveDrawingBuffer: false });
  if (!gl) return null;
  const parallel = gl.getExtension('KHR_parallel_shader_compile');
  const vertex = `#version 300 es
    in vec2 p; out vec2 uv;
    void main(){ uv=p*.5+.5; gl_Position=vec4(p,0.,1.); }`;
  const common = `#version 300 es
    precision mediump float;
    in vec2 uv; out vec4 color;
    uniform sampler2D camera, mask, background, blurred, gate;
    uniform vec4 crop;
    uniform bool mirror;
    vec2 cameraUV(vec2 v){ if(mirror) v.x=1.-v.x; return crop.xy+v*crop.zw; }
  `;
  function program(fragment) {
    const shaders = [gl.VERTEX_SHADER, gl.FRAGMENT_SHADER].map((kind, i) => {
      const s = gl.createShader(kind); gl.shaderSource(s, i ? fragment : vertex); gl.compileShader(s);
      if (!parallel && !gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s;
    });
    const p = gl.createProgram(); shaders.forEach(s => gl.attachShader(p, s)); gl.linkProgram(p);
    shaders.forEach(s => gl.deleteShader(s));
    return { p, uniforms: null };
  }
  const blurSource = common + `
    uniform vec2 stepSize; uniform bool firstPass;
    vec4 readPixel(vec2 v){ return firstPass ? texture(camera,cameraUV(clamp(v,0.,1.))) : texture(blurred,clamp(v,0.,1.)); }
    void main(){ color=vec4(0.); float total=0.;
      for(int i=-4;i<=4;i++){ float w=exp(-float(i*i)/8.); color+=readPixel(uv+float(i)*stepSize)*w; total+=w; }
      color/=total;
    }`;
  const refineSource = common + `
    uniform vec2 maskSize;
    void main(){
      vec2 c=cameraUV(uv);
      vec3 guide=texture(camera,c).rgb;
      float sum=0., total=0.;
      for(int y=-1;y<=1;y++) for(int x=-1;x<=1;x++){
        vec2 delta=vec2(float(x),float(y)), v=c+delta/maskSize;
        vec3 diff=texture(camera,v).rgb-guide;
        float w=exp(-dot(delta,delta)/1.6-dot(diff,diff)*32.);
        sum+=texture(mask,v).a*w; total+=w;
      }
      color=vec4(1.,1.,1.,sum/max(total,.0001));
    }`;
  const hybridSource = common + `
    uniform bool hybrid; uniform vec2 gateStep; uniform float gateAge;
    float support(vec2 c){
      vec3 sampleGate=texture(gate,c).rgb;
      float distance=sampleGate.g*256.;
      float motion=min(96.,sampleGate.b*1.2*(gateAge+34.));
      return 1.-smoothstep(12.,22.,distance-motion);
    }
    float hybridGate(vec2 c,float a){
      float g=support(vec2(c.x,1.-c.y));
      // Only extend through a bounded, continuous opaque RVM path into the MP
      // support. A global high-alpha override would restore furniture leaks.
      float activation=smoothstep(.98,.995,a);
      float recovery=1.-smoothstep(112.,128.,texture(gate,vec2(c.x,1.-c.y)).g*256.);
      float maximum=activation*recovery;
      if(g<maximum) {
        for(int d=0;d<8;d++){
          float angle=float(d)*.78539816;vec2 delta=vec2(cos(angle),sin(angle))*gateStep*8.;
          float connected=activation;
          for(int j=1;j<=16;j++) {
            vec2 at=vec2(c.x,1.-c.y)+delta*float(j);
            connected=min(connected,smoothstep(.98,.995,texture(mask,at).a));
            g=max(g,connected*texture(gate,at).r*recovery);
            if(connected<.001 || g>=maximum) break;
          }
          // No other direction can exceed the activation and distance bound.
          if(g>=maximum)break;
        }
      }
      return mix(g,1.,smoothstep(150.,250.,gateAge));
    }
    void main(){vec2 c=cameraUV(uv);float a=texture(mask,vec2(c.x,1.-c.y)).a;color=vec4(vec3(hybridGate(c,a)),1.);}`;
  const compositeSource = common + `
    uniform bool hybrid; uniform sampler2D hybridSupport;
    uniform int mode; uniform bool softMatte; uniform vec2 feather; uniform vec4 imageCrop; uniform sampler2D refined;
    void main(){
      vec2 c=cameraUV(uv); vec4 sharp=texture(camera,c);
      if(mode==0){ color=sharp; return; }
      float a;
      if (softMatte) {
        // ImageBitmap ignores UNPACK_FLIP_Y_WEBGL; canvas masks do not.
        a=texture(mask,vec2(c.x,1.-c.y)).a;
        if(hybrid) a*=texture(hybridSupport,uv).r;
      } else {
        a=texture(refined,uv).a*4.;
        a+=texture(refined,uv+vec2(feather.x,0.)).a+texture(refined,uv-vec2(feather.x,0.)).a;
        a+=texture(refined,uv+vec2(0.,feather.y)).a+texture(refined,uv-vec2(0.,feather.y)).a;
        a=smoothstep(.06,.94,a/8.);
      }
      vec4 bg= mode==1 ? texture(blurred,uv) : texture(background,imageCrop.xy+uv*imageCrop.zw);
      color=vec4(mix(bg.rgb,sharp.rgb,a),1.);
    }`;
  let blur, refine, composite, hybridProgram;
  const quad = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, quad);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1,1,-1,-1,1,1,1]), gl.STATIC_DRAW);
  const textures = Array.from({ length: 6 }, () => {
    const t = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, t);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([0,0,0,255])); return t;
  });
  const gateTexture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,gateTexture);
  for(const p of [gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER]) gl.texParameteri(gl.TEXTURE_2D,p,gl.LINEAR);
  for(const p of [gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D,p,gl.CLAMP_TO_EDGE);
  gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,1,1,0,gl.RGBA,gl.UNSIGNED_BYTE,new Uint8Array([255,255,255,255]));
  const supportTexture=gl.createTexture();gl.bindTexture(gl.TEXTURE_2D,supportTexture);
  for(const p of [gl.TEXTURE_MIN_FILTER,gl.TEXTURE_MAG_FILTER]) gl.texParameteri(gl.TEXTURE_2D,p,gl.LINEAR);
  for(const p of [gl.TEXTURE_WRAP_S,gl.TEXTURE_WRAP_T]) gl.texParameteri(gl.TEXTURE_2D,p,gl.CLAMP_TO_EDGE);
  const fb = gl.createFramebuffer(); let width = 0, height = 0;
  let uploadedGate = null;
  let uploadedMask = null, uploadedVideo = null, uploadedTime = -1;
  const timings = { cameraUploadMs: 0, maskUploadMs: 0 };
  function bind(unit, texture) { gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, texture); }
  function upload(unit, image) {
    bind(unit, textures[unit]); gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
  }
  function use(program, rect, mirror) {
    gl.useProgram(program.p);
    const loc = gl.getAttribLocation(program.p, 'p'); gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    const u = program.uniforms;
    gl.uniform1i(u.camera, 0); gl.uniform1i(u.mask, 1); gl.uniform1i(u.background, 2); gl.uniform1i(u.blurred, 3); gl.uniform1i(u.refined, 5); gl.uniform1i(u.gate, 6);gl.uniform1i(u.hybridSupport,7);
    gl.uniform4f(u.crop, ...rect); gl.uniform1i(u.mirror, mirror); return u;
  }
  return {
    draw(video, crop, framing, background, mask, w, h, softMatte = false, gateState = null) {
      if (gl.isContextLost()) throw new Error('The graphics context was lost.');
      if(gateState?.params.hasPerson===false)gateState=null;
      // WebKit can synchronously spend several milliseconds per shader even
      // with parallel compilation. Spread startup across camera frames.
      if (!blur) { blur = program(blurSource); return null; }
      if (!refine) { refine = program(refineSource); return null; }
      if (!composite) { composite = program(compositeSource); return null; }
      if (!hybridProgram) { hybridProgram = program(hybridSource); return null; }
      // Poll compilation without blocking a camera frame on the driver.
      for (const program of [blur, refine, composite, hybridProgram]) {
        if (program.uniforms) continue;
        if (parallel && !gl.getProgramParameter(program.p, parallel.COMPLETION_STATUS_KHR)) return null;
        if (!gl.getProgramParameter(program.p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program.p));
        program.uniforms = {};
        for (let i = 0; i < gl.getProgramParameter(program.p, gl.ACTIVE_UNIFORMS); i++) {
          const name = gl.getActiveUniform(program.p, i).name;
          program.uniforms[name] = gl.getUniformLocation(program.p, name);
        }
      }
      if (width !== w || height !== h) {
        canvas.width = width = w; canvas.height = height = h;
        bind(7,supportTexture);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,Math.ceil(w/4),Math.ceil(h/4),0,gl.RGBA,gl.UNSIGNED_BYTE,null);
        for (let i = 3; i < textures.length; i++) {
          const divisor = i === 5 ? 2 : 4;
          bind(i, textures[i]); gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, Math.ceil(w/divisor), Math.ceil(h/divisor), 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
        }
      }
      let stage = performance.now();
      if (uploadedVideo !== video || uploadedTime !== video.currentTime || uploadedMask === null) { upload(0, video); uploadedVideo = video; uploadedTime = video.currentTime; }
      timings.cameraUploadMs = performance.now()-stage;
      const iw = video.videoWidth, ih = video.videoHeight;
      const rect = [crop.x/iw, 1-(crop.y+crop.height)/ih, crop.width/iw, crop.height/ih];
      const mode = mask ? background.mode : 'none';
      if (mode !== 'none') {
        stage = performance.now();
        if (!softMatte || uploadedMask !== mask) { upload(1, mask); uploadedMask = mask; }
        timings.maskUploadMs = performance.now()-stage;
        if (!softMatte) {
        bind(5, textures[2]);
        const u = use(refine, rect, framing.mirror);
        gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.viewport(0, 0, Math.ceil(w/2), Math.ceil(h/2));
        // Continuous sample positions avoid snapping back onto the model grid.
        // Camera color guides a smooth nine-tap reconstruction. Half resolution
        // avoids paying for this neighborhood at every 720p output pixel.
        gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, textures[5], 0);
        gl.uniform2f(u.maskSize, mask.width, mask.height); gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        bind(5, textures[5]);
        }
      }
      if (mode === 'blur') {
        const u = use(blur, rect, framing.mirror);
        gl.bindFramebuffer(gl.FRAMEBUFFER, fb); gl.viewport(0, 0, Math.ceil(w/4), Math.ceil(h/4));
        for (let pass = 0; pass < 2; pass++) {
          bind(3, textures[pass ? 3 : 4]);
          gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, textures[pass ? 4 : 3], 0);
          gl.uniform1i(u.firstPass, pass === 0);
          const sigma = background.radius * w/1280;
          gl.uniform2f(u.stepSize, pass ? 0 : sigma/(2*w), pass ? sigma/(2*h) : 0);
          gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        }
      }
      if(gateState && softMatte && mode!=='none') {
        bind(6,gateTexture);
        if(uploadedGate!==gateState.bitmap) { gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL,false);gl.texImage2D(gl.TEXTURE_2D,0,gl.RGBA,gl.RGBA,gl.UNSIGNED_BYTE,gateState.bitmap);uploadedGate=gateState.bitmap; }
        const u=use(hybridProgram,rect,framing.mirror);
        gl.uniform2f(u.gateStep,video.videoHeight/(720*video.videoWidth),1/720);
        gl.uniform1f(u.gateAge,Math.max(0,performance.now()-gateState.timestamp));
        // Gate support varies slowly; quarter resolution avoids paying for its
        // neighborhood at every hair pixel. RVM alpha stays full resolution.
        bind(7,textures[2]);gl.bindFramebuffer(gl.FRAMEBUFFER,fb);gl.viewport(0,0,Math.ceil(w/4),Math.ceil(h/4));
        gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,supportTexture,0);gl.drawArrays(gl.TRIANGLE_STRIP,0,4);
        bind(7,supportTexture);
      }
      const u = use(composite, rect, framing.mirror);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null); gl.viewport(0, 0, w, h);
      gl.uniform1i(u.mode, mode === 'blur' ? 1 : mode === 'replace' ? 2 : 0);
      gl.uniform2f(u.feather, 1.5/w, 1.5/h);
      gl.uniform1i(u.softMatte, softMatte);
      gl.uniform1i(u.hybrid, !!gateState && softMatte);
      bind(3, textures[4]);
      if (mode === 'replace') {
        upload(2, background.image); const [bw, bh] = imageSize(background.image); const r = cropRect(bw, bh, w, h);
        gl.uniform4f(u.imageCrop, r.x/bw, 1-(r.y+r.height)/bh, r.width/bw, r.height/bh);
      }
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4); return canvas;
    },
    timings,
    invalidate() { uploadedGate=null; uploadedMask = uploadedVideo = null; uploadedTime = -1; },
    close() {
      gl.deleteTexture(supportTexture);gl.deleteTexture(gateTexture); textures.forEach(t => gl.deleteTexture(t)); gl.deleteFramebuffer(fb); gl.deleteBuffer(quad);
      for (const program of [blur, refine, composite, hybridProgram]) if (program) gl.deleteProgram(program.p); gl.getExtension('WEBGL_lose_context')?.loseContext();
      canvas.width = canvas.height = 1;
    },
  };
}

function cpuCompositor() {
  const camera = surface(), person = surface(), maskCrop = surface(), small = surface();
  const cc = camera.getContext('2d'), pc = person.getContext('2d'), mc = maskCrop.getContext('2d');
  const sc = small.getContext('2d');
  return {
    draw(video, crop, framing, background, mask, w, h, softMatte = false) {
      for (const c of [camera, person, maskCrop]) if (c.width !== w || c.height !== h) { c.width = w; c.height = h; }
      cc.setTransform(framing.mirror ? -1 : 1, 0, 0, 1, framing.mirror ? w : 0, 0);
      cc.drawImage(video, crop.x, crop.y, crop.width, crop.height, 0, 0, w, h); cc.resetTransform();
      if (!mask || background.mode === 'none') return camera;
      pc.globalCompositeOperation = 'copy'; pc.drawImage(camera, 0, 0);
      mc.clearRect(0, 0, w, h); mc.save();
      if (framing.mirror) { mc.translate(w, 0); mc.scale(-1, 1); }
      const sx = mask.width/video.videoWidth, sy = mask.height/video.videoHeight;
      mc.imageSmoothingEnabled = true; mc.imageSmoothingQuality = 'high';
      mc.drawImage(mask, crop.x*sx, crop.y*sy, crop.width*sx, crop.height*sy, 0, 0, w, h); mc.restore();
      pc.globalCompositeOperation = 'destination-in'; pc.drawImage(maskCrop, 0, 0); pc.globalCompositeOperation = 'source-over';
      if (background.mode === 'replace') cover(cc, background.image, w, h);
      else {
        // Bilinear down/up sampling is a soft tent blur on Safari without filter.
        const factor = Math.max(4, background.radius*w/1280*1.5);
        const sw = Math.max(2, Math.round(w/factor)), sh = Math.max(2, Math.round(h/factor));
        if (small.width !== sw || small.height !== sh) { small.width = sw; small.height = sh; }
        sc.drawImage(camera, 0, 0, sw, sh); cc.imageSmoothingEnabled = true; cc.imageSmoothingQuality = 'high';
        cc.drawImage(small, 0, 0, w, h);
      }
      cc.drawImage(person, 0, 0); return camera;
    },
    close() { for (const c of [camera, person, maskCrop, small]) c.width = c.height = 1; },
  };
}

export function createStudioEngine({ canvas, width = 1280, height = 720, debug = false }) {
  if (!canvas?.getContext) throw new TypeError('A canvas element is required.');
  const ctx = canvas.getContext('2d', { alpha: false });
  if (!ctx) throw new Error('A 2D canvas context is required.');
  let video = null, framing = { ...framingDefaults }, background = { mode: 'none', radius: 8, image: null };
  let running = false, disposed = false, dirty = true, fresh = false, epoch = 0;
  let frameId = null, animationId = null, lastMediaTime = -1, lastFrames = -1, lastInferenceMediaTime = -1;
  let worker = null, workerReady = false, busy = false, failed = false, snapshotPending = false;
  let mask = null, maskData = null, recycled = null, backend = null, lastSegment = -Infinity, maskStarted = 0;
  let loadTimer = null, compositor = null, gpuFailed = false;
  let preference = 'auto', matting = 'rvm', rvmFailed = false;
  let gateWorker=null, gateReady=false, gateBusy=false, gatePending=false, gateFailed=false, gateState=null;
  let gateLast=-Infinity, gateInferMs=0, gateFps=0, gateCount=0, gateStart=performance.now(), gateBackend=null;
  let gateTimer=null, gateFrameTimer=null, gateEpoch=0;
  const wantsGate=()=> !rvmFailed && !gateFailed && (preference==='auto'||preference==='hybrid');
  function clearGate() { gateState?.bitmap.close();gateState=null; }
  function closeGate() {
    clearTimeout(gateFrameTimer);clearTimeout(gateTimer);const old=gateWorker;gateWorker=null;gateReady=gateBusy=false;gateEpoch++;clearGate();
    if(old){old.onmessage=old.onerror=null;old.postMessage({type:'close'});setTimeout(()=>old.terminate(),250);}
  }
  function failGate(message) {
    if(disposed)return;
    closeGate();gateFailed=true;matting='rvm';
    emit(workerReady?'ready':'loading',`MediaPipe gate unavailable; using RVM-only. ${message}`);invalidate();
  }
  function ensureGate() {
    if(gateWorker || !wantsGate() || disposed || !effective())return;
    try {
      const current=new Worker(new URL(MODULE_URL),{type:'module',name:'studio-gate'});gateWorker=current;
      current.onmessage=({data})=>{
        if(disposed||gateWorker!==current){data.bitmap?.close();return;}
        if(data.type==='ready'){clearTimeout(gateTimer);gateReady=true;gateBusy=false;gateBackend=data.backend;}
        else if(data.type==='error')failGate(data.message);
        else if(data.type==='gate'){
          clearTimeout(gateFrameTimer);gateBusy=false;gateInferMs=data.segmentMs;gateCount++;
          const now=performance.now();if(now-gateStart>=1000){gateFps=gateCount*1000/(now-gateStart);gateStart=now;gateCount=0;}
          if(data.epoch!==epoch||!running||!effective()||document.hidden){data.bitmap.close();return;}
          clearGate();gateState=data;
          const active=data.params.hasPerson?'hybrid':'rvm';
          if(workerReady&&matting!==active){matting=active;emit('ready',active==='hybrid'?`Hybrid ready: RVM ${backend}, MediaPipe ${gateBackend} gate at ${GATE.rateHz} Hz.`:'MediaPipe has no confident person; using RVM-only until it recovers.');}
          invalidate();
        }
      };
      current.onerror=e=>{e.preventDefault();if(gateWorker===current)failGate(e.message||'Gate worker crashed.');};
      current.postMessage({type:'init',gate:true});
      gateTimer=setTimeout(()=>{if(gateWorker===current)failGate('Gate loading timed out.');},30000);
    }catch(e){failGate(e.message);}
  }
  function segmentGate(now, captured = false) {
    if(!captured||!running||disposed||document.hidden||!validVideo()||!gateReady||gateBusy||gatePending||!effective()||now-gateLast<1000/GATE.rateHz-1)return;
    const current=gateWorker,source=video,currentEpoch=epoch,token=gateEpoch;
    const scale=Math.min(1,384/Math.max(video.videoWidth,video.videoHeight));
    const w=Math.max(1,Math.round(video.videoWidth*scale)),h=Math.max(1,Math.round(video.videoHeight*scale));
    gatePending=true;
    try {
      // Share the RVM capture so a gate tick never waits on a second video decode.
      createImageBitmap(capture,{resizeWidth:w,resizeHeight:h}).then(bitmap=>{
        gatePending=false;
        if(disposed||!running||document.hidden||current!==gateWorker||source!==video||currentEpoch!==epoch||token!==gateEpoch||!effective()){bitmap.close();return;}
        gateBusy=true;gateLast=now;gateFrameTimer=setTimeout(()=>{if(current===gateWorker)failGate('Gate inference timed out.');},1500);current.postMessage({type:'frame',bitmap,epoch:currentEpoch,timestamp:now},[bitmap]);
      }).catch(e=>{gatePending=false;if(current===gateWorker)failGate(e.message);});
    }catch(e){gatePending=false;failGate(e.message);}
  }
  let snapshotMs = 0, presentMs = 0;
  let ratio = .25, preprocessMs = 0;
  let inferMs = 0, inferFps = 0, inferenceCount = 0, inferenceStart = performance.now();
  let status = { state: 'idle', backend: null, matting, message: 'Background effects are off.' };
  let segmentMs = 0, renderMs = 0, fps = 0, frameCount = 0, fpsStart = performance.now();
  const listeners = new Set(), capture = surface(); const captureContext = capture.getContext('2d');
  function emit(state, message) {
    status = { state, backend, matting, message };
    for (const cb of listeners) { try { cb({ ...status }); } catch (e) { console.error(e); } }
  }
  function effective() {
    return background.mode !== 'none' && (background.mode !== 'replace' || imageSize(background.image).every(n => n > 0));
  }
  function clearMask() { mask?.close?.(); mask = null; maskData = null; }
  function closeWorker() {
    clearTimeout(loadTimer); workerReady = false; busy = false;
    const old = worker; worker = null;
    if (old) {
      old.onmessage = old.onerror = null;
      old.postMessage({ type: 'close' }); setTimeout(() => old.terminate(), 250);
    }
  }
  function fail(message) {
    if (disposed) return;
    closeWorker(); clearMask(); backend = null;
    if (matting !== 'mediapipe') {
      closeGate();
      rvmFailed = true; matting = 'mediapipe'; failed = false;
      emit('loading', `RVM unavailable; falling back to MediaPipe. ${message}`);
      ensureSegmenter(message);
    } else {
      failed = true;
      emit('error', `Background effects are unavailable. The camera is still live. ${message}`);
    }
    invalidate();
  }
  function ensureSegmenter(fallbackMessage = '') {
    if (worker || failed || disposed || !effective()) return;
    matting = preference === 'mediapipe' || rvmFailed ? 'mediapipe' : 'rvm';
    if(matting==='rvm')ensureGate();
    emit('loading', fallbackMessage ? `Loading MediaPipe after RVM fallback: ${fallbackMessage}` : `Loading the offline ${matting !== 'mediapipe' ? 'RVM' : 'MediaPipe'} person model.`);
    try {
      if (typeof Worker === 'undefined' || typeof createImageBitmap !== 'function') throw new Error('This WebView needs module workers and ImageBitmap support.');
      const current = new Worker(matting !== 'mediapipe' ? new URL('./rvm-worker.js', MODULE_URL) : new URL(MODULE_URL), { type: 'module', name: `studio-${matting}` });
      worker = current;
      current.onmessage = ({ data }) => {
        if (disposed || worker !== current) { data.bitmap?.close(); return; }
        if (data.type === 'ready') {
          clearTimeout(loadTimer); workerReady = true; busy = false; backend = data.backend;
          if(gateState?.params.hasPerson && wantsGate()) matting='hybrid';
          if (effective()) emit('ready', `${data.message || `Person segmentation is ready (${backend.toUpperCase()}).`}${fallbackMessage ? ` RVM fallback: ${fallbackMessage}` : ''}`);
          invalidate();
        } else if (data.type === 'error') fail(data.message);
        else if (data.type === 'mask') {
          busy = false; segmentMs = segmentMs ? segmentMs*.8+data.segmentMs*.2 : data.segmentMs;
          inferMs = data.inferMs ?? data.segmentMs;
          ratio = data.ratio ?? null; preprocessMs = data.preprocessMs ?? 0;
          inferenceCount++;
          const now = performance.now();
          if (now-inferenceStart >= 1000) { inferFps = inferenceCount*1000/(now-inferenceStart); inferenceCount = 0; inferenceStart = now; }
          if (data.epoch !== epoch || !effective() || !running || document.hidden) { data.bitmap?.close(); recycled = data.pixels || null; return; }
          if (data.bitmap) {
            if (!mask) maskStarted = now;
            mask?.close?.(); mask = data.bitmap;
          } else {
            if (!mask || mask.width !== data.width || mask.height !== data.height) {
              clearMask(); maskStarted = now;
              mask = surface(data.width, data.height); maskData = mask.getContext('2d').createImageData(data.width, data.height);
            }
            maskData.data.set(new Uint8ClampedArray(data.pixels)); mask.getContext('2d').putImageData(maskData, 0, 0);
            recycled = data.pixels;
          }
          invalidate();
        }
      };
      current.onerror = event => { event.preventDefault(); if (worker === current) fail(event.message || 'The matting worker could not start.'); };
      current.postMessage({ type: 'init', auto: preference === 'auto' || preference === 'hybrid' });
      loadTimer = setTimeout(() => { if (worker === current) fail('The offline model did not finish loading.'); }, 30000);
    } catch (error) { fail(error.message); }
  }
  function invalidate() { dirty = true; schedulePaint(); }
  function validVideo() { return video && video.readyState >= 2 && video.videoWidth > 0 && video.videoHeight > 0; }
  function getCrop() { return cropRect(video?.videoWidth || 0, video?.videoHeight || 0, width, height, framing); }
  function segment(now) {
    // One in-flight frame, no queued stale video. Inference slows independently
    // of rendering; the last mask remains usable at the camera's full frame rate.
    if (!running || disposed || document.hidden || !validVideo() || !workerReady || busy || snapshotPending || !effective() || now-lastSegment < (matting !== 'mediapipe' ? 0 : Math.min(100, Math.max(1000/30, segmentMs*1.15)))) return;
    if (matting !== 'mediapipe' && lastInferenceMediaTime === lastMediaTime) return;
    snapshotPending = true; const capturedMediaTime = lastMediaTime; const currentEpoch = epoch, source = video, currentWorker = worker;
    const scale = matting !== 'mediapipe' ? Math.min(1, 1280 / Math.max(video.videoWidth, video.videoHeight)) : Math.min(1, 384 / Math.max(video.videoWidth, video.videoHeight));
    const w = Math.max(1, Math.round(video.videoWidth*scale)), h = Math.max(1, Math.round(video.videoHeight*scale));
    if (capture.width !== w || capture.height !== h) { capture.width = w; capture.height = h; }
    const snapshotStart = performance.now();
    try {
      // WebKit's direct video bitmap path can wait on the decoder. A reusable
      // canvas avoids that wait and still keeps full-frame RGB off the CPU.
      captureContext.drawImage(video, 0, 0, w, h);
      if(matting!=='mediapipe')segmentGate(now,true);
      createImageBitmap(capture).then(bitmap => {
        snapshotPending = false;
        if (disposed || !running || document.hidden || currentEpoch !== epoch || source !== video || currentWorker !== worker || !workerReady || !effective()) { bitmap.close(); return; }
        busy = true; lastSegment = now; lastInferenceMediaTime = capturedMediaTime;
        const buffer = recycled; recycled = null;
        worker.postMessage({ type: 'frame', bitmap, epoch, timestamp: now, recycled: buffer }, buffer ? [bitmap, buffer] : [bitmap]);
      }).catch(error => { snapshotPending = false; if (currentWorker === worker && currentEpoch === epoch) fail(error.message); });
    } catch (error) { snapshotPending = false; fail(error.message); }
    snapshotMs = performance.now()-snapshotStart;
  }
  function drawCamera(crop) {
    ctx.save();
    try {
      if (framing.mirror) { ctx.translate(width, 0); ctx.scale(-1, 1); }
      ctx.drawImage(video, crop.x, crop.y, crop.width, crop.height, 0, 0, width, height);
    } finally { ctx.restore(); }
  }
  function draw(now) {
    animationId = null;
    if (!running || disposed || document.hidden || !validVideo()) return;
    if (!fresh && !dirty) return;
    const isNewFrame = fresh; fresh = false; dirty = false;
    const start = performance.now(); const crop = getCrop();
    const bg = effective() ? background : { mode: 'none' };
    try {
      if (!compositor) {
        if (!gpuFailed) { try { compositor = gpuCompositor();if(!compositor)gpuFailed=true; } catch { gpuFailed = true; } }
        compositor ||= cpuCompositor();
        if(gpuFailed && wantsGate()) { fail('GPU compositor unavailable for hybrid gating.'); }
      }
      const result = compositor.draw(video, crop, framing, bg, mask, width, height, matting !== 'mediapipe', matting === 'hybrid' ? gateState : null);
      const strength = mask && bg.mode !== 'none' ? clamp((now-maskStarted)/180, 0, 1, 1) : 1;
      if (!result || strength < 1) drawCamera(crop);
      if (result) { const presentStart = performance.now(); ctx.globalAlpha = strength; ctx.drawImage(result, 0, 0); ctx.globalAlpha = 1; presentMs = performance.now()-presentStart; }
      if (!result || strength < 1) { dirty = true; schedulePaint(); }
    } catch {
      // Context loss, undecoded images, and a source changing resolution must
      // never replace a usable preview with black.
      compositor?.close(); compositor = cpuCompositor(); gpuFailed = true;
      if(wantsGate()) fail('GPU compositor lost; using MediaPipe.');
      ctx.globalAlpha = 1;
      try { drawCamera(crop); } catch { /* The next decoded frame will retry. */ }
    }
    const elapsed = performance.now()-start; renderMs = renderMs ? renderMs*.8+elapsed*.2 : elapsed;
    if (isNewFrame) frameCount++;
    if (now-fpsStart >= 1000) { fps = frameCount*1000/(now-fpsStart); frameCount = 0; fpsStart = now; }
    if (isNewFrame || !mask) { segment(now); }
  }
  function schedulePaint() {
    if (running && !disposed && !document.hidden && animationId === null) animationId = requestAnimationFrame(draw);
  }
  function cancelFrame() {
    if (frameId !== null) { video?.cancelVideoFrameCallback?.(frameId); frameId = null; }
    if (animationId !== null) { cancelAnimationFrame(animationId); animationId = null; }
  }
  function watchFrames() {
    if (!running || disposed || document.hidden || !video) return;
    if (video.requestVideoFrameCallback) {
      if (frameId !== null) return;
      frameId = video.requestVideoFrameCallback((now, metadata) => {
        frameId = null; lastMediaTime = metadata.mediaTime; fresh = true;
        if (matting !== 'mediapipe' && validVideo()) { segment(now); }
        schedulePaint(); watchFrames();
      });
    } else if (frameId === null) {
      const poll = () => {
        frameId = null;
        if (!running || disposed || document.hidden || !video) return;
        const count = video.getVideoPlaybackQuality?.().totalVideoFrames;
        if (validVideo() && (count === undefined ? video.currentTime !== lastMediaTime : count !== lastFrames)) {
          lastMediaTime = video.currentTime; lastFrames = count; fresh = true;
          if (matting !== 'mediapipe') { const now=performance.now();segment(now); }
          schedulePaint();
        }
        frameId = requestAnimationFrame(poll);
      };
      frameId = requestAnimationFrame(poll);
    }
  }
  function cancelAll() {
    if (video?.requestVideoFrameCallback) cancelFrame();
    else { if (frameId !== null) cancelAnimationFrame(frameId); frameId = null; cancelFrame(); }
  }
  function resetSourceMask() { epoch++; clearGate();gateLast=-Infinity;clearMask(); compositor?.invalidate?.(); lastInferenceMediaTime = -1; lastMediaTime = -1; lastFrames = -1; lastSegment = -Infinity; }
  function sourceReady() { resetSourceMask(); fresh = true; invalidate(); watchFrames(); }
  function visibility() {
    cancelAll(); frameCount = inferenceCount = 0; fpsStart = inferenceStart = performance.now(); fps = inferFps = 0;
    if (!document.hidden) { fresh = true; invalidate(); watchFrames(); }
  }
  document.addEventListener('visibilitychange', visibility);
  function size(w, h) {
    width = Math.round(clamp(w, 1, 8192, 1280)); height = Math.round(clamp(h, 1, 8192, 720));
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
    invalidate();
  }
  size(width, height);
  return {
    setSource(source) {
      if (disposed || source === video) return;
      cancelAll(); video?.removeEventListener('loadeddata', sourceReady); video?.removeEventListener('resize', sourceReady); video?.removeEventListener('emptied', sourceReady); video?.removeEventListener('ended', sourceReady);
      video = source; resetSourceMask();
      video?.addEventListener('loadeddata', sourceReady); video?.addEventListener('resize', sourceReady); video?.addEventListener('emptied', sourceReady); video?.addEventListener('ended', sourceReady);
      fresh = true; invalidate(); watchFrames();
    },
    setMatting(value = 'auto') {
      if (disposed || !['auto', 'rvm', 'mediapipe', 'hybrid'].includes(value) || (value === preference && !rvmFailed && !gateFailed && !failed)) return;
      preference = value; closeGate();gateFailed=false;closeWorker(); resetSourceMask(); rvmFailed = failed = false;
      segmentMs = inferMs = inferFps = 0; inferenceCount = 0; inferenceStart = performance.now();
      matting = value === 'mediapipe' ? 'mediapipe' : 'rvm';
      if (effective()) ensureSegmenter(); else emit('idle', 'Background effects are off.');
      invalidate();
    },
    setFraming(f = {}) {
      if (disposed) return;
      framing = { zoom: clamp(f.zoom,1,4,1), pan_x: clamp(f.pan_x,-1,1,0), pan_y: clamp(f.pan_y,-1,1,0), mirror: !!f.mirror };
      invalidate();
    },
    setBackground(bg = {}) {
      if (disposed) return;
      const wasEffective = effective();
      background = { mode: ['none','blur','replace'].includes(bg.mode) ? bg.mode : 'none', radius: clamp(bg.radius,1,50,8), image: bg.image || null };
      if (effective()) {
        if (!wasEffective) { resetSourceMask(); failed = false; }
        ensureSegmenter();
        if (workerReady) emit('ready', `Person segmentation is ready (${backend.toUpperCase()}).`);
      } else { resetSourceMask(); emit('idle', 'Background effects are off.'); }
      invalidate();
    },
    setOutputSize(w, h) { if (!disposed) size(w,h); },
    start() {
      if (disposed || running) return;
      running = true; fresh = true; fpsStart = inferenceStart = performance.now(); frameCount = inferenceCount = 0;
      invalidate(); watchFrames();
    },
    stop() { running = false; cancelAll(); resetSourceMask(); fps = inferFps = 0; frameCount = 0; },
    getCrop,
    on(event, cb) {
      if (event !== 'status' || typeof cb !== 'function' || disposed) return () => {};
      listeners.add(cb); cb({ ...status }); return () => listeners.delete(cb);
    },
    ...(debug ? { getMattingDebug() { return { preference, gate: { ...GATE, ...gateState?.params, ageMs: gateState ? performance.now()-gateState.timestamp : null }, gateReady, gateFailed, gpuFailed }; } } : {}),
    stats() { return { gateInferMs: gateWorker ? gateInferMs : 0, gateFps: gateWorker ? gateFps : 0, gateBackend: gateWorker ? gateBackend : null, fps, segmentMs, renderMs, backend, matting: matting==='hybrid' && (!gateState || gateState.params.hasPerson===false || performance.now()-gateState.timestamp>=GATE.staleMs) ? 'rvm' : matting, inferMs, inferFps, ratio, preprocessMs, snapshotMs, presentMs, ...compositor?.timings }; },
    dispose() {
      if (disposed) return;
      running = false; cancelAll(); disposed = true; clearTimeout(loadTimer);
      document.removeEventListener('visibilitychange', visibility);
      video?.removeEventListener('loadeddata', sourceReady); video?.removeEventListener('resize', sourceReady); video?.removeEventListener('emptied', sourceReady); video?.removeEventListener('ended', sourceReady);
      closeGate();closeWorker(); clearMask();
      worker = null; compositor?.close(); capture.width = capture.height = 1;
      video = mask = maskData = recycled = null; listeners.clear();
    },
  };
}
