import * as ort from './vendor/ort/ort.all.min.mjs';
import { RESET_GAP_MS } from './rvm-policy.js';

const modelURL = new URL('./vendor/rvm/rvm_mobilenetv3_fp32.onnx', import.meta.url);
const outputs = ['pha', 'r1o', 'r2o', 'r3o', 'r4o'];
ort.env.wasm.wasmPaths = new URL('./vendor/ort/', import.meta.url).href;
ort.env.wasm.numThreads = 1;
ort.env.wasm.simd = true;
ort.env.wasm.proxy = false;
ort.env.logLevel = 'error';

// All full-frame operations stay in the worker. WebGPU never downloads src,
// pha, or the recurrent tensors; only the compositable bitmap crosses threads.
export class RvmMatting {
  static async create(backend, onLost = () => {}) {
    const self = new RvmMatting(); self.backend = backend;
    try {
      if (backend === 'webgpu') {
        const adapter = await navigator.gpu?.requestAdapter({ powerPreference: 'high-performance' });
        if (!adapter) throw new Error('WebGPU adapter is unavailable.');
      }
      self.session = await ort.InferenceSession.create(modelURL.href, {
        executionProviders: [backend], graphOptimizationLevel: 'all',
        ...(backend === 'webgpu' ? { preferredOutputLocation: Object.fromEntries(outputs.map(n => [n, 'gpu-buffer'])) } : {}),
      });
      if (backend === 'webgpu') {
        self.device = await ort.env.webgpu.device;
        self.device.lost.then(info => { if (!self.closed) onLost(`WebGPU device lost: ${info.message}`); });
      }
      self.ratio = new ort.Tensor('float32', new Float32Array([.25]), [1]);
      self.reset();
      return self;
    } catch (error) { await self.close(); throw error; }
  }
  reset() {
    this.rec?.forEach(t => t.dispose());
    this.rec = Array.from({ length: 4 }, () => new ort.Tensor('float32', new Float32Array(1), [1, 1, 1, 1]));
    this.previousTime = 0;
  }
  allocate(w, h) {
    this.src?.dispose(); this.buffer?.destroy(); this.texture?.destroy();
    this.width = w; this.height = h; this.reset();
    this.canvas ||= new OffscreenCanvas(w, h); this.canvas.width = w; this.canvas.height = h;
    if (!this.device) {
      this.context ||= this.canvas.getContext('2d', { willReadFrequently: true });
      this.src = new ort.Tensor('float32', new Float32Array(w * h * 3), [1, 3, h, w]);
      this.pixels = this.context.createImageData(w, h);
      return;
    }
    const d = this.device;
    this.context ||= this.canvas.getContext('webgpu');
    this.context.configure({ device: d, format: navigator.gpu.getPreferredCanvasFormat(), alphaMode: 'premultiplied' });
    this.texture = d.createTexture({ size: [w, h], format: 'rgba8unorm', usage: GPUTextureUsage.COPY_DST | GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.RENDER_ATTACHMENT });
    this.buffer = d.createBuffer({ size: w * h * 3 * 4, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST });
    this.src = ort.Tensor.fromGpuBuffer(this.buffer, { dataType: 'float32', dims: [1, 3, h, w] });
    this.preprocess = d.createComputePipeline({ layout: 'auto', compute: { module: d.createShaderModule({ code: `
      @group(0) @binding(0) var image: texture_2d<f32>;
      @group(0) @binding(1) var<storage, read_write> rgb: array<f32>;
      @compute @workgroup_size(16,16) fn main(@builtin(global_invocation_id) p: vec3u) {
        let size = textureDimensions(image); if (p.x >= size.x || p.y >= size.y) { return; }
        let v = textureLoad(image, vec2i(p.xy), 0); let n = size.x*size.y; let i = p.y*size.x+p.x;
        rgb[i]=v.r; rgb[i+n]=v.g; rgb[i+2u*n]=v.b;
      }` }), entryPoint: 'main' } });
    this.preBind = d.createBindGroup({ layout: this.preprocess.getBindGroupLayout(0), entries: [{ binding: 0, resource: this.texture.createView() }, { binding: 1, resource: { buffer: this.buffer } }] });
    this.alphaPipeline = d.createRenderPipeline({ layout: 'auto',
      vertex: { module: d.createShaderModule({ code: `
        @vertex fn main(@builtin(vertex_index) i:u32)->@builtin(position) vec4f {
          let p=array<vec2f,3>(vec2f(-1.,-1.),vec2f(3.,-1.),vec2f(-1.,3.)); return vec4f(p[i],0.,1.);
        }` }), entryPoint: 'main' },
      fragment: { module: d.createShaderModule({ code: `
        @group(0) @binding(0) var<storage,read> alpha:array<f32>;
        @fragment fn main(@builtin(position) p:vec4f)->@location(0) vec4f {
          let a=clamp(alpha[u32(p.y)*${w}u+u32(p.x)],0.,1.); return vec4f(a,a,a,a);
        }` }), entryPoint: 'main', targets: [{ format: navigator.gpu.getPreferredCanvasFormat() }] }, primitive: { topology: 'triangle-list' },
    });
  }
  async frame(bitmap, epoch, timestamp, ratio = .25) {
    const start = performance.now();
    const w = bitmap.width, h = bitmap.height;
    if (w !== this.width || h !== this.height) this.allocate(w, h);
    if (epoch !== this.epoch || timestamp - this.previousTime > RESET_GAP_MS || this.ratio.data[0] !== ratio) this.reset();
    this.epoch = epoch; this.previousTime = timestamp; this.ratio.data[0] = ratio;
    const d = this.device;
    if (d) {
      d.queue.copyExternalImageToTexture({ source: bitmap }, { texture: this.texture }, [w, h]);
      const encoder = d.createCommandEncoder(), pass = encoder.beginComputePass();
      pass.setPipeline(this.preprocess); pass.setBindGroup(0, this.preBind); pass.dispatchWorkgroups(Math.ceil(w / 16), Math.ceil(h / 16)); pass.end(); d.queue.submit([encoder.finish()]);
    } else {
      this.context.drawImage(bitmap, 0, 0);
      const rgba = this.context.getImageData(0, 0, w, h).data, rgb = this.src.data, n = w * h;
      for (let i = 0; i < n; i++) { rgb[i] = rgba[i*4]/255; rgb[i+n] = rgba[i*4+1]/255; rgb[i+2*n] = rgba[i*4+2]/255; }
    }
    const prepared = performance.now();
    let result; try { result = await this.session.run({ src: this.src, r1i: this.rec[0], r2i: this.rec[1], r3i: this.rec[2], r4i: this.rec[3], downsample_ratio: this.ratio }, outputs); } catch (e) { throw new Error(`RVM session.run (${this.backend}, state ${this.rec[0].location}): ${String(e)}`); }
    this.rec.forEach(t => t.dispose()); this.rec = [result.r1o, result.r2o, result.r3o, result.r4o];
    if (d) {
      const encoder = d.createCommandEncoder();
      const pass = encoder.beginRenderPass({ colorAttachments: [{ view: this.context.getCurrentTexture().createView(), loadOp: 'clear', storeOp: 'store', clearValue: [0,0,0,0] }] });
      const bind = d.createBindGroup({ layout: this.alphaPipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: result.pha.gpuBuffer } }] });
      pass.setPipeline(this.alphaPipeline); pass.setBindGroup(0, bind); pass.draw(3); pass.end(); d.queue.submit([encoder.finish()]);
      // Submission alone understates inference cost. Measure completed GPU work.
      await d.queue.onSubmittedWorkDone();
    } else {
      const a = result.pha.data, pixels = this.pixels.data;
      for (let i = 0; i < a.length; i++) { pixels[i*4] = pixels[i*4+1] = pixels[i*4+2] = 255; pixels[i*4+3] = Math.round(Math.max(0, Math.min(1, a[i]))*255); }
      this.context.putImageData(this.pixels, 0, 0);
    }
    const inferred = performance.now(); result.pha.dispose();
    const matte = this.canvas.transferToImageBitmap();
    // Snapshotting can enqueue a texture copy. Finish it here so the WebGL
    // consumer does not inherit that GPU wait on the main thread.
    if (d) await d.queue.onSubmittedWorkDone();
    return { bitmap: matte, width: w, height: h, segmentMs: performance.now()-start, inferMs: inferred-prepared, preprocessMs: prepared-start, ratio };
  }
  async close() {
    this.closed = true;
    this.rec?.forEach(t => t.dispose()); this.src?.dispose(); this.ratio?.dispose();
    await this.session?.release();
    this.buffer?.destroy(); this.texture?.destroy(); this.context?.unconfigure?.(); this.device?.destroy();
    if (this.canvas) this.canvas.width = this.canvas.height = 1;
  }
}
