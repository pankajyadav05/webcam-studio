/** Sliders use 0..1 with the shipped output exactly at 0.5. See TUNING.md. */
export const TUNING_DEFAULTS = Object.freeze({ cutoff: .5, softness: .5, steadiness: .5 });
export function normalizeTuning(value = {}) {
  const result = {};
  for (const key of Object.keys(TUNING_DEFAULTS)) {
    const n = value?.[key];
    result[key] = typeof n === 'number' && Number.isFinite(n) ? Math.max(0, Math.min(1, n)) : TUNING_DEFAULTS[key];
  }
  return result;
}
const map = (n, low, middle, high) => n === .5 ? middle : n < .5
  ? low + (middle-low)*n*2 : middle + (high-middle)*(n-.5)*2;
export function tuningParams(value) {
  const { cutoff, softness, steadiness } = normalizeTuning(value);
  return {
    curveLow: map(cutoff, .025, .22, .4),
    curveHigh: map(cutoff, .975, .88, .8),
    curveMix: map(cutoff, .25, .85, 1),
    radius: map(softness, 2, 6, 12),
    epsilon: map(softness, .0002, .001, .008),
    stillMs: map(steadiness, 20, 80, 240),
  };
}
