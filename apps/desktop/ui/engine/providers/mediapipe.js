import { ProviderControl } from './control.js';
export class MediaPipeProvider {
  constructor() {
    this.name = 'mediapipe';
    this.backend = 'loading';
    this.preferCpu = false;
    this.control = new ProviderControl({ timeoutMs: 15000 });
    this.worker = null;
    this.pending = null;
    this.clock = 0;
  }
  get ready() { return this.control.ready; }
  initialize() {
    if (this.worker) return this.loading;
    this.worker = new Worker(new URL('./mediapipe-worker.js', import.meta.url), { type: 'module', name: 'studio-person' });
    this.loading = new Promise((resolve, reject) => {
      this.pending = { resolve, reject };
      this.worker.onmessage = ({ data }) => {
        if (data.type === 'ready') { this.backend = data.backend; this.control.timeoutMs = 2000; }
        const pending = this.pending;
        if (!pending) return;
        if (data.type !== 'ready' && data.id !== pending.id) return;
        this.pending = null;
        if (data.type === 'error') pending.reject(new Error(data.message));
        else pending.resolve(data);
      };
      this.worker.onerror = event => {
        event.preventDefault();
        this.pending?.reject(new Error(event.message || 'MediaPipe worker failed'));
        this.pending = null;
      };
      this.worker.postMessage({ type: 'init', delegate:this.preferCpu ? 'CPU' : 'GPU' });
    });
    return this.loading;
  }
  async segment(frame) {
    return this.control.run(async () => {
      await this.initialize();
      // The model's input is 256 square; avoid the old 384px intermediate resize.
      const bitmap = await createImageBitmap(frame.image, { resizeWidth: 256, resizeHeight: 256, resizeQuality: 'high' });
      if (!this.worker) { bitmap.close(); throw new Error('MediaPipe worker was reset'); }
      const timestamp = this.clock = Math.max(this.clock + .001, (frame.capturedAt ?? frame.mediaTime * 1000));
      const result = await new Promise((resolve, reject) => {
        this.pending = { id: frame.id, resolve, reject };
        this.worker.postMessage({ type: 'frame', id: frame.id, epoch: frame.epoch, timestamp, bitmap }, [bitmap]);
      });
      return { ...result, values: new Float32Array(result.values), mediaTime: frame.mediaTime };
    }, () => this.reset()).catch(error => { this.preferCpu = true; this.reset(); throw error; });
  }
  reset() {
    this.worker?.terminate(); this.worker = null;
    this.pending?.reject(new Error('MediaPipe worker reset'));
    this.pending = null;
  }
  dispose() { this.reset(); this.control.cancel(); }
}
