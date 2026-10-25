import { tuningParams, normalizeTuning } from './tuning.js';
/** Continuous confidence processing. Contrast belongs only in the output shader. */
export const clamp01 = value => Math.min(1, Math.max(0, value));
export function smoothstep(low, high, value) {
  const t = clamp01((value - low) / (high - low));
  return t * t * (3 - 2 * t);
}
export function temporalBlend(delta, elapsed, stillMs = 80) {
  // Small confidence noise settles over 80 ms. Motion approaches an immediate response.
  const moving = smoothstep(0.04, 0.18, Math.abs(delta));
  const tau = stillMs * (1 - moving) + 2 * moving;
  return 1 - Math.exp(-Math.max(1, elapsed) / tau);
}
/**
 * Where the camera picture is still, a sudden mask change is the model changing its mind
 * (e.g. a chair behind the shoulders flipping in and out), not motion. There, the mask
 * joins slowly and leaves quickly, so an ambiguous still area settles on background.
 * `luma` is the frame the mask was made from, downscaled to a coarse grid.
 */
function createMotionGate() {
  let gw = 0, gh = 0, previous = null, gate = null, spread = null, blurred = null;
  return {
    reset() { previous = null; },
    update(luma, dt) {
      const { width, height, data } = luma, cells = width * height;
      if (width !== gw || height !== gh) {
        gw = width; gh = height; previous = null;
        gate = new Float32Array(cells); spread = new Float32Array(cells); blurred = new Float32Array(cells);
      }
      if (!previous) { previous = Float32Array.from(data); gate.fill(0); return gate; }
      // Motion reaches two cells (about 8 mask pixels) past what moved, then softens.
      for (let y = 0; y < gh; y++) for (let x = 0; x < gw; x++) {
        let most = 0;
        for (let yy = Math.max(0, y - 2); yy <= Math.min(gh - 1, y + 2); yy++)
          for (let xx = Math.max(0, x - 2); xx <= Math.min(gw - 1, x + 2); xx++) {
            const j = yy * gw + xx;
            most = Math.max(most, Math.abs(data[j] - previous[j]));
          }
        spread[y * gw + x] = smoothstep(5, 15, most);
      }
      const memory = Math.exp(-dt / 50);
      for (let y = 0; y < gh; y++) for (let x = 0; x < gw; x++) {
        let sum = 0, n = 0;
        for (let yy = Math.max(0, y - 1); yy <= Math.min(gh - 1, y + 1); yy++)
          for (let xx = Math.max(0, x - 1); xx <= Math.min(gw - 1, x + 1); xx++) { sum += spread[yy * gw + xx]; n++; }
        const i = y * gw + x;
        blurred[i] = Math.max(sum / n, gate[i] * memory);
      }
      [gate, blurred] = [blurred, gate];
      previous.set(data);
      return gate;
    },
    // Bilinear lookup keeps the gate from drawing cell-sized blocks into the mask.
    sampler(width, height) {
      const sx = gw / width, sy = gh / height;
      return (x, y) => {
        const gx = Math.min(gw - 1, Math.max(0, (x + .5) * sx - .5)), gy = Math.min(gh - 1, Math.max(0, (y + .5) * sy - .5));
        const x0 = gx | 0, y0 = gy | 0, x1 = Math.min(gw - 1, x0 + 1), y1 = Math.min(gh - 1, y0 + 1), fx = gx - x0, fy = gy - y0;
        return (gate[y0 * gw + x0] * (1 - fx) + gate[y0 * gw + x1] * fx) * (1 - fy)
          + (gate[y1 * gw + x0] * (1 - fx) + gate[y1 * gw + x1] * fx) * fy;
      };
    },
  };
}
export function createMatteFilter(width, height, { hysteresis = true, tuning } = {}) {
  let stillMs = tuningParams(tuning).stillMs;
  const count = width * height;
  const history = new Float32Array(count);
  let support = hysteresis ? new Float32Array(count) : null;
  let next = hysteresis ? new Float32Array(count) : null;
  const motion = createMotionGate();
  let initialized = false;
  return {
    setTuning(value) { stillMs = tuningParams(normalizeTuning(value)).stillMs; },
    reset() { initialized = false; support?.fill(0); motion.reset(); },
    apply(values, elapsed = 33.333, reset = false, luma = null) {
      if (values.length !== count) throw new RangeError('Mask shape changed');
      const first = !initialized || reset;
      const dt = Math.max(1,elapsed), stillBlend = 1-Math.exp(-dt/stillMs), fastBlend = 1-Math.exp(-dt/2);
      const supportDecay = Math.exp(-dt/100);
      if (first) motion.reset();
      const gateAt = luma ? (motion.update(luma, dt), motion.sampler(width, height)) : null;
      // Still areas: join over ~25x and leave over ~3x the steadiness time.
      const joinBlend = 1-Math.exp(-dt/(stillMs*25)), leaveBlend = 1-Math.exp(-dt/(stillMs*3));
      for (let y = 0, i = 0; y < height; y++) for (let x = 0; x < width; x++, i++) {
        const value = clamp01(values[i]);
        const delta = value-history[i], change = Math.abs(delta);
        let blend;
        if (change <= .04) blend = stillBlend;
        else if (!gateAt) blend = change >= .18 ? fastBlend : temporalBlend(delta,dt,stillMs);
        else {
          const still = delta > 0 ? joinBlend : leaveBlend;
          const big = still + (fastBlend - still) * gateAt(x, y);
          blend = change >= .18 ? big : stillBlend + (big - stillBlend) * smoothstep(.04, .18, change);
        }
        history[i] = first ? value : history[i] + blend*delta;
        // Seed continuously, retain support briefly, and never classify/drop components.
        if (hysteresis) support[i] = Math.max(smoothstep(.55, .85, history[i]), first ? 0 : support[i] * supportDecay);
      }
      if (hysteresis) {
        for (let pass = 0; pass < 4; pass++) {
          for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
            const i = y * width + x;
            let grow = support[i];
            if (x) grow = Math.max(grow, support[i - 1] * .96);
            if (x + 1 < width) grow = Math.max(grow, support[i + 1] * .96);
            if (y) grow = Math.max(grow, support[i - width] * .96);
            if (y + 1 < height) grow = Math.max(grow, support[i + width] * .96);
            next[i] = Math.max(support[i], grow * smoothstep(.15, .65, history[i]));
          }
          [support, next] = [next, support];
        }
      }
      const alpha = new Uint8Array(count);
      for (let i = 0; i < count; i++) {
        // Bounded falloff preserves uncertain fine hair and isolated fingers.
        alpha[i] = Math.round(history[i] * (hysteresis ? .92 + .08 * support[i] : 1) * 255);
      }
      initialized = true;
      return alpha;
    },
  };
}
