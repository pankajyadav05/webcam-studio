/** Deadline and backpressure include preprocessing, not just provider inference. */
export class ProviderControl {
  constructor({ timeoutMs = 1800, now = () => performance.now() } = {}) {
    this.timeoutMs = timeoutMs;
    this.now = now;
    this.busy = false;
    this.failures = 0;
    this.retryAt = 0;
    this.token = 0;
  }
  get ready() { return !this.busy && this.now() >= this.retryAt; }
  async run(work, abort = () => {}) {
    if (!this.ready) return null;
    this.busy = true;
    const token = ++this.token;
    let timer;
    try {
      const result = await Promise.race([
        Promise.resolve().then(work),
        new Promise((_, reject) => { timer = setTimeout(() => {
          abort(); reject(new Error('Person mask provider timed out'));
        }, this.timeoutMs); }),
      ]);
      if (token !== this.token) return null;
      this.failures = 0;
      this.retryAt = 0;
      return result;
    } catch (error) {
      if (token !== this.token) return null;
      this.failures++;
      this.retryAt = this.now() + Math.min(8000, 250 * 2 ** (this.failures - 1));
      throw error;
    } finally {
      clearTimeout(timer);
      if (token === this.token) this.busy = false;
    }
  }
  cancel() { this.token++; this.busy = false; }
}
