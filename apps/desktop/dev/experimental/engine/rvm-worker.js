import { RvmMatting } from './rvm.js';
import { createRatioPolicy } from './rvm-policy.js';
let automatic = true;
let model, closing = false, active = false, policy = createRatioPolicy(), backend;
const sendError = error => postMessage({ type: 'error', message: error.message || `RVM worker failure: ${String(error)} (${error.stack || 'no stack'})` });
async function init(preferWasm = false, reason = '') {
  if (!preferWasm) {
    try { model = await RvmMatting.create('webgpu', message => sendError(new Error(message))); backend = 'webgpu'; }
    catch (e) { reason = `WebGPU unavailable: ${e.message} `; }
  }
  if (!model) { try { model = await RvmMatting.create('wasm'); backend = 'wasm'; } catch (e) { throw new Error(`${reason}WASM unavailable: ${e.message}`); } }
  if (closing) { await model?.close(); close(); return; }
  postMessage({ type: 'ready', backend, message: `${reason}RVM ready (${backend === 'wasm' ? 'WASM SIMD, one thread' : 'WebGPU, JS execution provider'}).` });
}
onmessage = async ({ data }) => {
  try {
    if (data.type === 'init') { automatic = data.auto !== false; await init(data.preferWasm); }
    else if (data.type === 'close') {
      closing = true;
      if (!active) { await model?.close(); close(); }
    } else if (data.type === 'reset') model?.reset();
    else if (data.type === 'frame') {
      if (!model || closing || active) { data.bitmap.close(); return; }
      active = true;
      try {
        const result = await model.frame(data.bitmap, data.epoch, data.timestamp, policy.ratio);
        if (closing) { result.bitmap.close(); return; }
        postMessage({ type: 'mask', epoch: data.epoch, ...result }, [result.bitmap]);
        const change = policy.sample(result.segmentMs);
        if (automatic && change === 'slow') {
          if (backend === 'webgpu') {
            await model.close(); model = null; policy = createRatioPolicy(); await init(true, 'WebGPU RVM was below 15 mattes/sec. Trying WASM. ');
          } else sendError(new Error('RVM is below 15 mattes/sec on this device; using MediaPipe.'));
        }
      } finally {
        data.bitmap.close(); active = false;
        if (closing) { await model?.close(); close(); }
      }
    }
  } catch (error) {
    active = false;
    if (model && backend === 'webgpu' && !closing) {
      try {
        await model.close(); model = null; policy = createRatioPolicy();
        await init(true, `WebGPU inference failed: ${error.message}. `);
      } catch (fallbackError) { sendError(new Error(`${error.message} WASM fallback failed: ${fallbackError.message}`)); }
    } else sendError(error);
  }
};
