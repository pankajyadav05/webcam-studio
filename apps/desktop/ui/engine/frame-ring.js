/** Captures are retained until their matching mask is painted or the epoch changes. */
export class FrameRing {
  constructor({ maxDepth = 12, frameMs = 1000 / 30 } = {}) {
    this.maxDepth = maxDepth;
    this.frameMs = frameMs;
    this.depth = 1;
    this.frames = new Map();
    this.epoch = 0;
    this.nextId = 1;
  }
  add(image, mediaTime, capturedAt, metadata = {}) {
    const frame = { id: this.nextId++, epoch: this.epoch, image, mediaTime, capturedAt, ...metadata };
    this.frames.set(frame.id, frame);
    while (this.frames.size > Math.min(this.maxDepth, Math.max(3, this.depth + 1))) {
      const oldest = [...this.frames.keys()].find(id => id !== this.pinned);
      if (oldest === undefined) break;
      this.release(oldest);
    }
    return frame;
  }
  latest(afterId = 0) {
    const frame = [...this.frames.values()].at(-1);
    return frame?.id > afterId ? frame : null;
  }
  align(mask) {
    const frame = this.frames.get(mask.id);
    return frame && frame.epoch === mask.epoch && mask.epoch === this.epoch ? frame : null;
  }
  observeLatency(ms) {
    this.depth = Math.min(this.maxDepth, Math.max(1, Math.ceil(ms / this.frameMs) + 1));
  }
  release(id) {
    const frame = this.frames.get(id);
    frame?.bitmap?.close();
    frame?.image?.close?.();
    this.frames.delete(id);
  }
  clear() {
    for (const id of this.frames.keys()) this.release(id);
    this.epoch++;
    this.depth = 1;
  }
}
