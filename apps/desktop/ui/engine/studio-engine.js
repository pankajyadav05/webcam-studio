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


import { normalizeTuning } from './tuning.js';
import { FrameRing } from './frame-ring.js';
import { MediaClock } from './media-clock.js';
import { createMatteFilter } from './matte.js';
import { createCompositor } from './compositor.js';
import { NativeProvider, nativeInvoke } from './providers/native.js';
import { MediaPipeProvider } from './providers/mediapipe.js';
import { createAutoFramer } from './autoframe.js';
import { createShowTell } from './showtell.js';
import { normalizeAppearance, isLookActive, lookParams, createLightMeter, createStudioLight, spotInOutput } from './look.js';

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
  let tuning = normalizeTuning(), view = debug === 'alpha' ? 'mask' : 'composite';
  let video, provider, compositor, choosing = false, selection = 0, preference = 'auto';
  let framing = { ...framingDefaults }, background = { mode:'none', radius:8, image:null };
  let running = false, disposed = false, frameCallback = null, animation = null;
  let lastMediaTime = -1, displayed = null, inFlight = false, nativeFailed = false;
  let filter, filterShape = '', filterTime = null, firstMaskAt = 0;
  let diagnostics = false, lastDiagnostics = 0, api;
  // Touch-ups and auto-frame.
  let appearance = normalizeAppearance(), light = createLightMeter(), lightCanvas = null, lightFrames = 0;
  let autoOn = false, autoFraming = null, faceAsk = 0, faceBusy = false, faceCanvas = null, lastFramingEmit = 0;
  const framer = createAutoFramer(), framingListeners = new Set();
  const studio = createStudioLight();
  let studioState = null;
  // Show-and-tell: zoom in on something held up to the camera (showtell.js).
  let showOn = false;
  const show = createShowTell();
  const wantsFaces = () => autoOn || showOn || appearance.studio_light > 0.001;
  // Be right back: a blurred, darker still of the last picture with the card on top
  // (crates/engine/src/away.rs). The live camera is not drawn while away.
  let away = false, awayCard = null, awayStill = null;
  function paintAway() {
    if (!awayStill) {
      awayStill = document.createElement('canvas'); awayStill.width = width; awayStill.height = height;
      const small = document.createElement('canvas'); small.width = 48; small.height = Math.max(1, Math.round(48*height/width));
      const sc = small.getContext('2d'); sc.imageSmoothingQuality = 'high'; sc.drawImage(canvas,0,0,small.width,small.height);
      const c = awayStill.getContext('2d'); c.imageSmoothingEnabled = true; c.imageSmoothingQuality = 'high';
      c.drawImage(small,0,0,width,height);
      c.fillStyle = 'rgba(0,0,0,.45)'; c.fillRect(0,0,width,height);
    }
    ctx.drawImage(awayStill,0,0,width,height);
    if (awayCard) ctx.drawImage(awayCard,0,0,width,height);
  }
  let alignmentDrops = 0, submitted = 0, received = 0, lastLatency = 0, lastSubmittedId = 0;
  let status = { state:'idle', backend:null, matting:null, message:'Background effects are off.' };
  const listeners = new Set(), ring = new FrameRing(), clock = new MediaClock(), pool = [];
  const samples = { masks:[], paints:[], latency:[], age:[], renders:[], delay:[] };
  const sourceEvents = ['loadeddata','resize','emptied','ended'];
  const now = () => performance.now();
  const effective = () => background.mode !== 'none';
  const looking = () => isLookActive(appearance);
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
  const framingInUse = () => autoFraming || framing;
  function getCrop() { return cropRect(video?.videoWidth || 0,video?.videoHeight || 0,width,height,framingInUse()); }
  // How bright the shown picture is, for low light; a tiny copy every third frame.
  function measureLight(image, timestamp) {
    if (!appearance.low_light) { light.reset(); return; }
    if (lightFrames++ % 3) return;
    try {
      if (!lightCanvas) { lightCanvas = document.createElement('canvas'); lightCanvas.width = 16; lightCanvas.height = 9; }
      const c = lightCanvas.getContext('2d', { willReadFrequently:true });
      c.drawImage(image,0,0,16,9);
      const data = c.getImageData(0,0,16,9).data;
      let sum = 0;
      for (let i = 0; i < data.length; i += 4) sum += .299*data[i] + .587*data[i+1] + .114*data[i+2];
      light.update(sum / (data.length/4) / 255, timestamp/1000);
    } catch (error) { console.warn('Brightness check failed', error); }
  }
  // Faces for auto-frame, about eight times a second, from a small copy of the whole picture.
  function askFaces(timestamp) {
    if (faceBusy || timestamp - faceAsk < 120 || !valid()) return;
    faceBusy = true; faceAsk = timestamp;
    const vw = video.videoWidth, vh = video.videoHeight, scale = Math.min(1, 640/vw);
    faceCanvas ||= document.createElement('canvas');
    faceCanvas.width = Math.round(vw*scale); faceCanvas.height = Math.round(vh*scale);
    faceCanvas.getContext('2d').drawImage(video,0,0,faceCanvas.width,faceCanvas.height);
    const asked = selection;
    new Promise((resolve, reject) => faceCanvas.toBlob(b => b ? resolve(b) : reject(new Error('No picture')), 'image/jpeg', .8))
      .then(blob => blob.arrayBuffer())
      .then(bytes => invoke('detect_faces', new Uint8Array(bytes), { headers: { want: showOn ? 'hands' : '' } }))
      .then(seen => {
        if (asked !== selection) return;
        const faces = seen?.faces || [];
        if (showOn) show.observe(seen?.hands || [], faces, now()/1000);
        if (autoOn) framer.observe(faces, [vw,vh], [width,height], now()/1000);
        if (appearance.studio_light > 0.001) studio.observe(faces, vw/vh, now()/1000);
      })
      .catch(error => { if (!String(error).includes('busy')) console.warn('Face search failed', error); })
      .finally(() => { faceBusy = false; });
  }
  // Studio Light's spot in output pixels, for the frame shown now.
  function studioSpot() {
    if (!studioState || !valid()) return null;
    return { spot: spotInOutput(studioState.spot,[video.videoWidth,video.videoHeight],getCrop(),[width,height],framingInUse().mirror),
      presence: studioState.presence };
  }
  function stepAutoFrame(timestamp) {
    if (!valid()) return;
    if (wantsFaces()) askFaces(timestamp);
    studioState = appearance.studio_light > 0.001 ? studio.step(timestamp/1000) : (studio.reset(), null);
    const focus = showOn ? show.focus() : null;
    framer.setAim(autoOn, focus);
    if (!autoOn && !focus && !framer.busy()) { framer.reset(); autoFraming = null; return; }
    // Once Show-and-tell hands back, the framer returns the manual framing itself and stops.
    const next = framer.step(framing,[video.videoWidth,video.videoHeight],[width,height],timestamp/1000);
    autoFraming = autoOn || framer.busy() ? next : null;
    if (!autoFraming) return;
    if (timestamp - lastFramingEmit > 100) {
      lastFramingEmit = timestamp;
      for (const cb of framingListeners) { try { cb({ ...autoFraming }); } catch (error) { console.error(error); } }
    }
  }
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
  let lumaCanvas = null, lumaContext = null;
  // A coarse grey copy of the frame a mask was made from (mask size / 4) lets the
  // filter tell real motion from the model changing its mind about a still area.
  function lumaGrid(image, maskWidth, maskHeight) {
    try {
      const w = Math.max(1,Math.round(maskWidth/4)), h = Math.max(1,Math.round(maskHeight/4));
      if (!lumaCanvas) {
        lumaCanvas = document.createElement('canvas');
        lumaContext = lumaCanvas.getContext('2d', { alpha:false, willReadFrequently:true });
      }
      if (lumaCanvas.width !== w) lumaCanvas.width = w;
      if (lumaCanvas.height !== h) lumaCanvas.height = h;
      lumaContext.drawImage(image,0,0,w,h);
      const rgba = lumaContext.getImageData(0,0,w,h).data, data = new Uint8Array(w*h);
      for (let i = 0; i < data.length; i++) data[i] = (rgba[i*4]*77 + rgba[i*4+1]*150 + rgba[i*4+2]*29) >> 8;
      return { width:w, height:h, data };
    } catch { return null; }
  }
  function paint(frame, mask, timestamp) {
    const start = now();
    try {
      compositor ||= createCompositor({ onStatus(state,message) {
        console.warn(message); emit(state === 'restored' ? 'loading' : state,message);
        if (state === 'restored') { reset(); schedulePaint(); }
        else safeCover();
      } });
      const result = compositor.draw({ image:frame.image, frameKey:frame.id, crop:frame.crop, mirror:frame.mirror,
        mask, background, width, height, fade:mask ? Math.min(1,(timestamp-firstMaskAt)/120) : 0,
        alphaOnly:view === 'mask', tuning, look: looking() ? lookParams(appearance, light.gain, studioSpot()) : null });
      ctx.drawImage(result,0,0);
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
      if (displayed && (effective() || looking())) {
        paint(displayed.frame,displayed.mask,timestamp);
        if (timestamp-firstMaskAt < 120) schedulePaint();
      }
    });
  }
  async function segment(frame) {
    if (!provider?.ready || inFlight || frame.id <= lastSubmittedId || frame.epoch !== ring.epoch) return;
    inFlight = true; submitted++; lastSubmittedId = frame.id;
    const current = provider, token = selection;
    try {
      const result = await current.segment(frame);
      if (!result || disposed || token !== selection || !running || document.hidden || !effective()) return;
      received++; lastLatency = now()-frame.capturedAt;
      const aligned = ring.align(result);
      if (!aligned) { alignmentDrops++; return; }
      const timestamp = now();
      const shape = `${result.width}x${result.height}`;
      if (!filter || filterShape !== shape) {
        filter = createMatteFilter(result.width,result.height,{hysteresis:current.name === 'mediapipe', tuning});
        filterShape = shape; filterTime = null;
      }
      const values = result.values || Float32Array.from(result.alpha, value => value/255);
      const alpha = filter.apply(values,filterTime !== null ? (result.mediaTime-filterTime)*1000 : 33.333,filterTime === null,
        lumaGrid(aligned.image,result.width,result.height));
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
    if (away) { paintAway(); watch(); return; }
    try {
    const mediaTime = valid() ? clock.advance(timestamp,metadata,video.currentTime) : null;
    if (valid() && mediaTime !== null) {
      const previousTime = lastMediaTime; lastMediaTime = mediaTime;
      if (previousTime >= 0 && mediaTime > previousTime) ring.frameMs = (mediaTime-previousTime)*1000;
      stepAutoFrame(timestamp);
      if (!effective() && looking()) {
        // Touch-ups without a background effect: the compositor, with no mask.
        const frame = capture(mediaTime,timestamp);
        measureLight(frame.image,timestamp);
        displayed = { frame, mask:null };
        paint(frame,null,timestamp);
      } else if (!effective()) {
        const c = getCrop(); ctx.save();
        if (framing.mirror) { ctx.translate(width,0); ctx.scale(-1,1); }
        ctx.drawImage(video,c.x,c.y,c.width,c.height,0,0,width,height); ctx.restore();
        samples.paints.push(timestamp);
        if (samples.paints.length > 300) samples.paints.shift();
      } else {
        ensureProvider();
        const frame = capture(mediaTime,timestamp);
        measureLight(frame.image,timestamp);
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
      cancel();
      for (const event of sourceEvents) video?.removeEventListener(event,sourceChanged);
      video = source; reset(); framer.reset(); studio.reset(); autoFraming = null;
      for (const event of sourceEvents) video?.addEventListener(event,sourceChanged);
      watch(); ensureProvider();
    },
    // With `auto` on, zoom and pan are only where auto-frame starts from.
    setFraming(value = {}) {
      const next = { zoom:clamp(value.zoom,1,4,1), pan_x:clamp(value.pan_x,-1,1,0),
        pan_y:clamp(value.pan_y,-1,1,0), mirror:!!value.mirror };
      const auto = !!value.auto, mirrored = next.mirror !== framing.mirror;
      if (auto !== autoOn) {
        // Taking over from auto-frame at the framing it reached is not a jump; keep the mask.
        const near = autoFraming && ['zoom','pan_x','pan_y'].every(k => Math.abs(autoFraming[k]-next[k]) < 0.02);
        autoOn = auto; framer.reset(); autoFraming = null; framing = next;
        if (mirrored || (!auto && !near)) reset();
        return;
      }
      if (auto) { framing = next; if (mirrored) reset(); return; }
      if (JSON.stringify(next) !== JSON.stringify(framing)) { framing = next; reset(); }
    },
    getFraming() { return { ...framingInUse() }; },
    setShowAndTell(on) {
      showOn = !!on;
      if (!showOn) show.reset();
    },
    showing: () => showOn && !!show.focus(),
    // `card` is a transparent canvas or image at the output size, drawn on the still.
    setAway(on, card = awayCard) {
      awayCard = card || null;
      if (!!on === away) { if (away) paintAway(); return; }
      away = !!on; awayStill = null;
      if (away) paintAway(); else { reset(); watch(); }
    },
    setAppearance(value = {}) {
      const next = normalizeAppearance(value);
      if (!next.low_light) light.reset();
      const was = looking();
      appearance = next;
      if (was && !looking() && !effective()) displayed = null;
      schedulePaint();
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
    setTuning(value = {}) {
      tuning = normalizeTuning({ ...tuning, ...value });
      filter?.setTuning(tuning); schedulePaint();
    },
    setQuality(value) {
      if (!['balanced','accurate'].includes(value)) throw new RangeError('Unknown native mask quality');
      nativeQuality = value;
      if (provider?.name === 'native') provider.quality = value;
    },
    setView(value) {
      if (!['composite','mask'].includes(value)) throw new RangeError('Unknown preview view');
      view = value; schedulePaint();
    },
    setOutputSize: size,
    start() { if (!disposed && !running) { running = true; watch(); ensureProvider(); } },
    stop() { running = false; cancel(); closeProvider(); reset(); },
    getCrop,
    on(event,callback) {
      if (event === 'framing' && typeof callback === 'function') { framingListeners.add(callback); return () => framingListeners.delete(callback); }
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
      compositor?.dispose(); listeners.clear(); framingListeners.clear(); pool.length = 0; video = null;
    },
  };
  return api;
}
