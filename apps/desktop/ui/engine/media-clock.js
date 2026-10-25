/** WebKit camera callbacks can advance presentedFrames while mediaTime stays zero. */
export class MediaClock {
  reset() { this.serial = null; this.raw = null; this.wall = null; this.time = 0; }
  constructor() { this.reset(); }
  advance(timestamp, metadata = {}, currentTime = 0) {
    const raw = Number.isFinite(metadata.mediaTime) ? metadata.mediaTime : currentTime;
    const serial = Number.isFinite(metadata.presentedFrames) ? metadata.presentedFrames : raw;
    if (!Number.isFinite(raw) || serial === this.serial) return null;
    if (this.wall !== null) {
      const mediaDelta = raw - this.raw;
      const elapsed = mediaDelta > 0 && mediaDelta < 1 ? mediaDelta : Math.max(.001,(timestamp-this.wall)/1000);
      this.time += elapsed;
    } else this.time = Math.max(0,raw);
    this.raw = raw; this.serial = serial; this.wall = timestamp;
    return this.time;
  }
}
