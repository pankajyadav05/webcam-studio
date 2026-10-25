import { offlineFetch } from '../offline-fetch.js';
const assets = new URL('../vendor/', import.meta.url);
globalThis.fetch = offlineFetch(import.meta.url);
let segmenter;
async function initialize(delegate) {
  const [{ ImageSegmenter }, { default: factory }] = await Promise.all([
    import(new URL('vision_bundle.mjs', assets)),
    import(new URL('wasm/vision_wasm_module_internal.js', assets)),
  ]);
  globalThis.ModuleFactory = factory;
  return ImageSegmenter.createFromOptions({
    wasmLoaderPath: '', wasmBinaryPath: new URL('wasm/vision_wasm_module_internal.wasm', assets).href,
  }, {
    baseOptions: { modelAssetPath: new URL('selfie_segmenter.tflite', assets).href, delegate },
    runningMode: 'VIDEO', outputCategoryMask: false, outputConfidenceMasks: true,
    canvas: new OffscreenCanvas(1, 1),
  });
}
self.onmessage = async ({ data }) => {
  try {
    if (data.type === 'init') {
      let backend = 'gpu';
      try { segmenter = await initialize(data.delegate === 'CPU' ? 'CPU' : 'GPU'); if (data.delegate === 'CPU') backend = 'cpu'; }
      catch { segmenter = await initialize('CPU'); backend = 'cpu'; }
      postMessage({ type: 'ready', backend });
    } else if (data.type === 'frame') {
      if (!segmenter) throw new Error('MediaPipe is not initialized');
      const started = performance.now();
      let reply;
      segmenter.segmentForVideo(data.bitmap, data.timestamp, result => {
        const masks = result.confidenceMasks;
        const person = segmenter.getLabels().findIndex(label => /person|selfie/i.test(label));
        const mask = masks?.[person >= 0 ? person : masks.length - 1];
        if (!mask) throw new Error('MediaPipe returned no person mask');
        const values = new Float32Array(mask.getAsFloat32Array());
        reply = { type: 'mask', id: data.id, epoch: data.epoch, width: mask.width, height: mask.height,
          values: values.buffer, segmentMs: performance.now() - started };
      });
      if (!reply) throw new Error('MediaPipe did not return a mask');
      postMessage(reply, [reply.values]);
    } else if (data.type === 'close') {
      segmenter?.close(); close();
    }
  } catch (error) {
    postMessage({ type: 'error', id: data.id, message: error.message || String(error) });
  } finally { data.bitmap?.close(); }
};
