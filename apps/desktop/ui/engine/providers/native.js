import { ProviderControl } from './control.js';
import { encodeFrame, decodeMask } from './protocol.js';
export const nativeInvoke = (command, body) => {
  const invoke = globalThis.__TAURI__?.core?.invoke;
  if (!invoke) return Promise.reject(new Error('Native segmentation command unavailable'));
  return invoke(command, body);
};
export class NativeProvider {
  constructor({ invoke = nativeInvoke, quality = 'balanced', timeoutMs } = {}) {
    this.invoke = invoke;
    this.quality = quality;
    this.control = new ProviderControl({ timeoutMs });
    this.name = 'native';
    this.backend = 'apple-vision';
    this.canvas = document.createElement('canvas');
    this.context = this.canvas.getContext('2d');
  }
  get ready() { return this.control.ready; }
  async segment(frame) {
    return this.control.run(async () => {
      const started = performance.now(); this.stages = {phase:"capture"};
      // Vision accepts its own aspect ratio; one high quality downscale, then JPEG IPC.
      const scale = Math.min(1, 1280 / frame.image.width);
      const width = Math.round(frame.image.width * scale), height = Math.round(frame.image.height * scale);
      if (this.canvas.width !== width) this.canvas.width = width;
      if (this.canvas.height !== height) this.canvas.height = height;
      this.context.imageSmoothingEnabled = true;
      this.context.imageSmoothingQuality = 'high';
      this.context.drawImage(frame.image, 0, 0, this.canvas.width, this.canvas.height);
      const blob = await new Promise((resolve, reject) => this.canvas.toBlob(
        value => value ? resolve(value) : reject(new Error('JPEG capture failed')), 'image/jpeg', .97));
      const jpeg = await blob.arrayBuffer();
      this.stages = {phase:"ipc",jpegMs:performance.now()-started};
      const result = await this.invoke('segment_person', encodeFrame(frame.id, jpeg, this.quality, frame.epoch));
      this.stages = {...this.stages,phase:'done',totalMs:performance.now()-started};
      return { ...decodeMask(result, frame.id), epoch: frame.epoch, mediaTime: frame.mediaTime };
    });
  }
  dispose() { this.control.cancel(); this.canvas.width = this.canvas.height = 1; }
}
