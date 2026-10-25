export const OUTPUT_DEFAULT = { width: 1280, height: 720 };
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


import { FrameRing } from './frame-ring.js';
import { MediaClock } from './media-clock.js';
import { createMatteFilter } from './matte.js';
import { createCompositor } from './compositor.js';
import { NativeProvider, nativeInvoke } from './providers/native.js';
import { MediaPipeProvider } from './providers/mediapipe.js';

const percentile = (values, fraction) => {
  if (!values.length) return 0;
  return [...values].sort((a,b) => a-b)[Math.min(values.length-1, Math.floor(values.length*fraction))];
};
export function createStudioEngine({ canvas, width = 1280, height = 720, debug = false,
  invoke = nativeInvoke, nativeQuality = 'balanced', providerFactory } = {}) {
  if (!['balanced','accurate'].includes(nativeQuality)) throw new RangeError('Unknown native mask quality');
  if (!canvas?.getContext) throw new TypeError('A canvas element is required');
  const ctx = canvas.getContext('2d', { alpha: false });
  if (!ctx) throw new Error('A 2D output canvas is required');
  let video, provider, compositor, choosing = false, selection = 0, preference = 'auto';
  let framing = { ...framingDefaults }, background = { mode:'none', radius:8, image:null };
  let running = false, disposed = false, frameCallback = null, animation = null;
  let lastMediaTime = -1, displayed = null, inFlight = false, nativeFailed = false;
  let filter, filterShape = '', filterTime = null, firstMaskAt = 0;
  let diagnostics = false, lastDiagnostics = 0, api, bitmapPending = false;
  let alignmentDrops = 0, submitted = 0, received = 0, lastLatency = 0, lastSubmittedId = 0;
  let status = { state:'idle', backend:null, matting:null, message:'Background effects are off.' };
  const listeners = new Set(), ring = new FrameRing(), clock = new MediaClock(), pool = [];
  const samples = { masks:[], paints:[], latency:[], age:[], renders:[], delay:[] };
  const sourceEvents = ['loadeddata','resize','emptied','ended'];
  const now = () => performance.now();
  const effective = () => background.mode !== 'none';
  const valid = () => video?.readyState >= 2 && video.videoWidth > 0 && video.videoHeight > 0;
  function emit(state, message) {
    status = { state, message, backend:provider?.backend || null, matting:provider?.name || null };
    for (const cb of listeners) { try { cb({ ...status }); } catch (error) { console.error(error); } }
  }
  function safeCover() {
    ctx.globalAlpha = 1;
    ctx.fillStyle = '#15171b'; ctx.fillRect(0,0,width,height);
    if (background.mode === 'replace' && background.image) {
      const image = background.image;
      const iw = image.naturalWidth || image.width, ih = image.naturalHeight || image.height;
      if (iw && ih) {
        const c = cropRect(iw,ih,width,height);
        try { ctx.drawImage(image,c.x,c.y,c.width,c.height,0,0,width,height); }
        catch (error) { console.warn('Replacement image is not decoded yet', error); }
      }
    }
  }
  function reset() {
    ring.clear(); ring.pinned = null; displayed = null; filter = null; filterShape = '';
    filterTime = null; firstMaskAt = 0; lastMediaTime = -1; clock.reset();
    if (effective()) safeCover();
  }
  function closeProvider() {
    selection++; choosing = false;
    provider?.dispose(); provider = null; inFlight = false;
  }
  async function ensureProvider() {
    if (provider || choosing || disposed || !effective() || !running) return;
    choosing = true;
    const token = ++selection;
    emit('loading','Preparing private background effects…');
    try {
      let native = false;
      if (preference !== 'mediapipe' && !nativeFailed) {
        let timer;
        try {
          const info = await Promise.race([invoke('segmenter_info'), new Promise((_,reject) => {
            timer = setTimeout(() => reject(new Error('Native provider discovery timed out')), 1200);
          })]);
          native = !!info.available; diagnostics = !!info.diagnostics;
        } catch (error) {
          console.warn('Native segmentation unavailable; using MediaPipe', error);
        } finally { clearTimeout(timer); }
      }
      if (disposed || token !== selection || !effective() || !running) return;
      provider = providerFactory ? providerFactory(native ? 'native' : 'mediapipe') :
        native ? new NativeProvider({ invoke, quality:nativeQuality }) : new MediaPipeProvider();
      emit('loading',native ? `Apple Vision ${nativeQuality} is preparing the person mask.` :
        'Native segmentation unavailable; using offline MediaPipe fallback.');
    } finally { if (token === selection) choosing = false; }
  }
  function getCrop() { return cropRect(video?.videoWidth || 0,video?.videoHeight || 0,width,height,framing); }
  function capture(mediaTime, timestamp) {
    const c = getCrop();
    // Give the model context around a zoomed crop instead of magnifying a full-frame mask.
    const marginX = c.width * .12, marginY = c.height * .12;
    const x = Math.max(0,c.x-marginX), y = Math.max(0,c.y-marginY);
    const w = Math.min(video.videoWidth,c.x+c.width+marginX)-x;
    const h = Math.min(video.videoHeight,c.y+c.height+marginY)-y;
    const scale = Math.min(1,1280/w);
    const image = pool.pop() || document.createElement('canvas');
    const captureWidth = Math.max(1,Math.round(w*scale)), captureHeight = Math.max(1,Math.round(h*scale));
    if (image.width !== captureWidth) image.width = captureWidth;
    if (image.height !== captureHeight) image.height = captureHeight;
    const captureContext = image.getContext('2d', {alpha:false});
    captureContext.imageSmoothingEnabled = true; captureContext.imageSmoothingQuality = 'high';
    captureContext.drawImage(video,x,y,w,h,0,0,image.width,image.height);
    image.close = () => { if (pool.length < 14) pool.push(image); };
    const frame = ring.add(image,mediaTime,timestamp, { crop:[(c.x-x)/w,(c.y-y)/h,c.width/w,c.height/h], mirror:framing.mirror });
    return frame;
  }
  function paint(frame, mask, timestamp) {
    const start = now();
    try {
      compositor ||= createCompositor({ onStatus(state,message) {
        console.warn(message); emit(state === 'restored' ? 'loading' : state,message);
        if (state === 'restored') { reset(); schedulePaint(); }
        else safeCover();
      } });
      const result = compositor.draw({ image:frame.bitmap || frame.image, frameKey:frame.id, crop:frame.crop, mirror:frame.mirror,
        mask, background, width, height, fade:mask ? Math.min(1,(timestamp-firstMaskAt)/120) : 0,
        alphaOnly:debug === 'alpha' });
      ctx.drawImage(result,0,0); result.close?.();
      samples.renders.push(now()-start); samples.paints.push(timestamp);
      if (mask) {
        // Age measures mask vs the *displayed* frame, independently of camera delay.
        samples.age.push(Math.abs(mask.mediaTime-frame.mediaTime)*30);
        samples.delay.push((timestamp-frame.capturedAt)/ring.frameMs);
      }
    } catch (error) {
      console.error('Background compositor failed',error);
      emit('error',`Background remains covered: ${error.message}`); safeCover();
      return false;
    }
    for (const values of Object.values(samples)) if (values.length > 300) values.splice(0,values.length-300);
    return true;
  }
  function schedulePaint() {
    if (animation !== null || !running || disposed || document.hidden) return;
    animation = requestAnimationFrame(timestamp => {
      animation = null;
      if (displayed && effective()) {
        paint(displayed.frame,displayed.mask,timestamp);
        if (timestamp-firstMaskAt < 120) schedulePaint();
      }
    });
  }
  async function segment(frame) {
    if (!provider?.ready || inFlight || frame.id <= lastSubmittedId || frame.epoch !== ring.epoch) return;
    inFlight = true; submitted++; lastSubmittedId = frame.id;
    if (!frame.bitmap && !bitmapPending && typeof createImageBitmap === 'function') {
      bitmapPending = true;
      let deadline;
      // Snapshot in parallel with inference. Do not block future work on a stuck bitmap API.
      const snapshot = Promise.resolve().then(() => createImageBitmap(frame.image)).then(bitmap => {
        if (ring.frames.get(frame.id) === frame) frame.bitmap = bitmap;
        else bitmap.close();
      }).catch(error => console.warn('Camera bitmap unavailable; retaining its canvas capture',error))
        .finally(() => { bitmapPending = false; clearTimeout(deadline); });
      frame.bitmapReady = Promise.race([snapshot,new Promise(resolve => { deadline = setTimeout(resolve,80); })]);
    }
    const current = provider, token = selection;
    try {
      const result = await current.segment(frame);
      if (frame.bitmapReady) await frame.bitmapReady;
      if (!result || disposed || token !== selection || !running || document.hidden || !effective()) return;
      received++; lastLatency = now()-frame.capturedAt;
      const aligned = ring.align(result);
      if (!aligned) { alignmentDrops++; return; }
      const timestamp = now();
      const shape = `${result.width}x${result.height}`;
      if (!filter || filterShape !== shape) {
        filter = createMatteFilter(result.width,result.height,{hysteresis:current.name === 'mediapipe'});
        filterShape = shape; filterTime = null;
      }
      const values = result.values || Float32Array.from(result.alpha, value => value/255);
      const alpha = filter.apply(values,filterTime !== null ? (result.mediaTime-filterTime)*1000 : 33.333,filterTime === null);
      filterTime = result.mediaTime;
      const mask = { ...result,alpha };
      const latency = timestamp-frame.capturedAt;
      ring.observeLatency(latency);
      samples.masks.push(timestamp); samples.latency.push(latency);
      ring.pinned = aligned.id;
      displayed = { frame:aligned,mask };
      if (!firstMaskAt) firstMaskAt = timestamp;
      const painted = paint(aligned,mask,timestamp);
      if (timestamp-firstMaskAt < 120) schedulePaint();
      if (painted && status.state !== 'ready') emit('ready',`Person mask ready (${current.name}, ${current.backend}).`);
    } catch (error) {
      if (token !== selection || disposed) return;
      console.error('Person mask provider failed',error);
      if (current.name === 'native') {
        nativeFailed = true; closeProvider(); reset();
        emit('loading',`Apple Vision failed; using MediaPipe fallback. ${error.message}`);
        ensureProvider();
      } else emit('error',`Person mask interrupted; retrying with backoff. ${error.message}`);
    } finally {
      if (current === provider) {
        inFlight = false;
        // A callback can arrive just before inference finishes. Consume only the newest
        // retained frame, without waiting another camera interval or queuing stale work.
        const latest = ring.latest(lastSubmittedId);
        if (running && !disposed && !document.hidden && effective() && current.ready && latest?.id > lastSubmittedId) {
          queueMicrotask(() => segment(latest));
        }
      }
    }
  }
  function onFrame(timestamp, metadata = {}) {
    frameCallback = null;
    if (!running || disposed || document.hidden) return;
    try {
    const mediaTime = valid() ? clock.advance(timestamp,metadata,video.currentTime) : null;
    if (valid() && mediaTime !== null) {
      const previousTime = lastMediaTime; lastMediaTime = mediaTime;
      if (previousTime >= 0 && mediaTime > previousTime) ring.frameMs = (mediaTime-previousTime)*1000;
      if (!effective()) {
        const c = getCrop(); ctx.save();
        if (framing.mirror) { ctx.translate(width,0); ctx.scale(-1,1); }
        ctx.drawImage(video,c.x,c.y,c.width,c.height,0,0,width,height); ctx.restore();
        samples.paints.push(timestamp);
        if (samples.paints.length > 300) samples.paints.shift();
      } else {
        ensureProvider();
        const frame = capture(mediaTime,timestamp);
        if (provider?.ready && !inFlight) segment(frame);
        if (!displayed) paint(frame,null,timestamp);
      }
    }
    if (diagnostics && timestamp-lastDiagnostics >= 2000) {
      lastDiagnostics = timestamp;
      invoke('segmentation_diagnostics',{snapshot:{...api.stats(),mode:background.mode,status}}).catch(console.warn);
    }
    } catch (error) {
      console.error('Camera frame capture failed',error);
      emit('error',`Camera frame interrupted; retrying. ${error.message}`);
      if (effective()) safeCover();
    } finally { watch(); }
  }
  function watch() {
    if (!running || disposed || document.hidden || !video || frameCallback !== null) return;
    frameCallback = video.requestVideoFrameCallback ? video.requestVideoFrameCallback(onFrame) : requestAnimationFrame(onFrame);
  }
  function cancel() {
    if (frameCallback !== null) {
      if (video?.cancelVideoFrameCallback) video.cancelVideoFrameCallback(frameCallback);
      else cancelAnimationFrame(frameCallback);
    }
    if (animation !== null) cancelAnimationFrame(animation);
    frameCallback = animation = null;
  }
  function sourceChanged() { reset(); watch(); }
  function visibility() {
    cancel(); closeProvider(); reset();
    if (!document.hidden) { watch(); ensureProvider(); }
  }
  document.addEventListener('visibilitychange',visibility);
  function size(w,h) {
    width = Math.round(clamp(w,1,8192,1280)); height = Math.round(clamp(h,1,8192,720));
    canvas.width = width; canvas.height = height; reset();
  }
  size(width,height);
  api = {
    setSource(source) {
      if (disposed || source === video) return;
      cancel(); closeProvider();
      for (const event of sourceEvents) video?.removeEventListener(event,sourceChanged);
      video = source; reset();
      for (const event of sourceEvents) video?.addEventListener(event,sourceChanged);
      watch(); ensureProvider();
    },
    setFraming(value = {}) {
      const next = { zoom:clamp(value.zoom,1,4,1), pan_x:clamp(value.pan_x,-1,1,0),
        pan_y:clamp(value.pan_y,-1,1,0), mirror:!!value.mirror };
      if (JSON.stringify(next) !== JSON.stringify(framing)) { framing = next; reset(); }
    },
    setBackground(value = {}) {
      const mode = ['none','blur','replace'].includes(value.mode) ? value.mode : 'none';
      const changed = mode !== background.mode;
      background = { mode,radius:clamp(value.radius,1,50,8),image:value.image || null };
      if (changed) { reset(); if (mode === 'none') { closeProvider(); emit('idle','Background effects are off.'); } }
      if (mode !== 'none') { if (!displayed) safeCover(); ensureProvider(); schedulePaint(); }
    },
    setMatting(value = 'auto') {
      if (!['auto','native','mediapipe'].includes(value)) throw new RangeError('Unknown matting provider');
      if (value === preference && !nativeFailed) return;
      preference = value; nativeFailed = false; closeProvider(); reset(); ensureProvider();
    },
    setOutputSize: size,
    start() { if (!disposed && !running) { running = true; watch(); ensureProvider(); } },
    stop() { running = false; cancel(); closeProvider(); reset(); },
    getCrop,
    on(event,callback) {
      if (event !== 'status' || typeof callback !== 'function') return () => {};
      listeners.add(callback); callback({...status}); return () => listeners.delete(callback);
    },
    stats() {
      const timestamp = now();
      const rate = values => values.filter(value => value >= timestamp-2000).length/2;
      return { matting:provider?.name || null, backend:provider?.backend || null,
        providerState:{inFlight,ready:provider?.ready,busy:provider?.control?.busy,submitted,received,alignmentDrops,lastLatency,ringSize:ring.frames.size,lastMediaTime,stages:provider?.stages},
        fps:rate(samples.paints), maskRate:rate(samples.masks), inferFps:rate(samples.masks),
        foregroundArea:displayed ? displayed.mask.alpha.reduce((sum,a) => sum+(a>127),0)/displayed.mask.alpha.length : 0,
        maskLatencyMs:percentile(samples.latency,.5), segmentMs:displayed?.mask.segmentMs || 0,
        delayLineDepth:effective() ? ring.depth : 0, displayDelayFrames:percentile(samples.delay,.9),
        maskAgeAtPaint:{p50:percentile(samples.age,.5),p90:percentile(samples.age,.9)},
        renderMs:percentile(samples.renders,.5), renderP90Ms:percentile(samples.renders,.9) };
    },
    ...(debug ? { getMattingDebug:() => ({ status,ring,displayed,provider,compositor }) } : {}),
    dispose() {
      if (disposed) return;
      running = false; cancel(); closeProvider(); reset(); disposed = true;
      document.removeEventListener('visibilitychange',visibility);
      for (const event of sourceEvents) video?.removeEventListener(event,sourceChanged);
      compositor?.dispose(); listeners.clear(); pool.length = 0; video = null;
    },
  };
  return api;
}
