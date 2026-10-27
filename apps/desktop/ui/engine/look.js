// Touch-ups for the preview: the same numbers as crates/engine/src/appearance.rs, which
// touches up the meeting camera. Change both together. The pixel work is in compositor.js.

export const LOOK = Object.freeze({
  smoothRadius: 0.006,     // at half size, share of the picture height
  smoothSigma: [0.02, 0.06],
  smoothMix: [0.3, 0.85],
  lightTarget: 0.45,
  lightMaxGain: 2.4,
  lightSeconds: 0.8,
  warmthShift: 0.12,
});

// Studio Light: crates/engine/src/studio_light.rs.
export const STUDIO = Object.freeze({
  dim: 0.45, lift: 0.7, reach: [1.0, 2.0], drop: 0.35, falloff: [0.55, 1.35],
  followSeconds: 0.25, fadeSeconds: 0.5, lostSeconds: 1.0, minFaceShare: 0.45,
});

export const APPEARANCE_DEFAULTS = Object.freeze({ smoothing: 0, low_light: false, warmth: 0, studio_light: 0 });

const finite = (v, fallback) => Number.isFinite(Number(v)) ? Number(v) : fallback;
const clamp = (v, lo, hi, fallback) => Math.min(hi, Math.max(lo, finite(v, fallback)));

export function normalizeAppearance(value = {}) {
  return {
    smoothing: clamp(value.smoothing, 0, 1, 0),
    low_light: value.low_light === true,
    warmth: clamp(value.warmth, -1, 1, 0),
    studio_light: clamp(value.studio_light, 0, 1, 0),
  };
}

export const changesColour = (a) => a.smoothing > 0.001 || a.low_light || Math.abs(a.warmth) > 0.001;
export const isLookActive = (a) => changesColour(a) || a.studio_light > 0.001;

export function lightGain(mean) {
  const m = clamp(mean, 0.02, 1, 0.5);
  if (m >= LOOK.lightTarget) return 1;
  return Math.min(LOOK.lightMaxGain, Math.max(1, LOOK.lightTarget * (1 - m) / (m * (1 - LOOK.lightTarget))));
}

export const lightCurve = (x, gain) => x * gain / (1 + (gain - 1) * x);

// Shader settings for one painted frame.
export function lookParams(a, gain, light = null) {
  const s = a.smoothing;
  const sigma = LOOK.smoothSigma[0] + (LOOK.smoothSigma[1] - LOOK.smoothSigma[0]) * s;
  return {
    smooth: s > 0.001,
    epsilon: sigma * sigma,
    mix: LOOK.smoothMix[0] + (LOOK.smoothMix[1] - LOOK.smoothMix[0]) * s,
    gain: a.low_light ? gain : 1,
    warmth: a.warmth * LOOK.warmthShift,
    // Studio Light: `light` is { spot (output pixels), presence } when there is a face.
    spot: a.studio_light > 0.001 && light ? light.spot : null,
    spotPresence: light ? light.presence : 0,
    spotShape: [STUDIO.dim * a.studio_light, STUDIO.lift * a.studio_light, STUDIO.falloff[0], STUDIO.falloff[1]],
  };
}

// Eases the low-light gain towards what the picture needs; `mean` is its brightness (0..1)
// and times are in seconds.
export function createLightMeter() {
  let gain = 1, last = null;
  return {
    get gain() { return gain; },
    reset() { gain = 1; last = null; },
    update(mean, now) {
      const dt = last === null ? 1 / 30 : Math.min(0.5, Math.max(0, now - last));
      last = now;
      gain += (lightGain(mean) - gain) * (1 - Math.exp(-dt / LOOK.lightSeconds));
      return gain;
    },
  };
}

// The spot that lights `faces` (shares of a picture `aspect` = width / height wide).
export function spotFor(faces, aspect) {
  const biggest = (faces || []).reduce((m, f) => Math.max(m, f.height), 0);
  if (!(biggest > 0) || !(aspect > 0)) return null;
  const kept = faces.filter((f) => f.height >= biggest * STUDIO.minFaceShare);
  const x0 = Math.min(...kept.map((f) => f.x)), x1 = Math.max(...kept.map((f) => f.x + f.width));
  const cy = kept.reduce((s, f) => s + f.y + f.height / 2, 0) / kept.length;
  return { cx: (x0 + x1) / 2, cy: cy + STUDIO.drop * biggest,
    rx: (x1 - x0) / 2 + STUDIO.reach[0] * biggest / aspect, ry: STUDIO.reach[1] * biggest };
}

// Follows the faces; times are in seconds. step() gives { spot, presence } or null.
export function createStudioLight() {
  let spot = null, wanted = null, presence = 0, lastFace = null, last = null;
  return {
    reset() { spot = wanted = lastFace = last = null; presence = 0; },
    observe(faces, aspect, now) {
      const next = spotFor(faces, aspect);
      if (next) { wanted = next; lastFace = now; }
    },
    step(now) {
      const dt = last === null ? 1 / 30 : Math.min(0.5, Math.max(0, now - last));
      last = now;
      const ease = (seconds) => 1 - Math.exp(-dt / seconds);
      const seen = lastFace !== null && now - lastFace <= STUDIO.lostSeconds;
      presence += ((seen ? 1 : 0) - presence) * ease(STUDIO.fadeSeconds);
      if (wanted) {
        const k = ease(STUDIO.followSeconds);
        spot = spot ? Object.fromEntries(Object.keys(spot).map((key) => [key, spot[key] + (wanted[key] - spot[key]) * k])) : { ...wanted };
      }
      return spot ? { spot, presence } : null;
    },
  };
}

// The spot in output pixels for a crop of the camera picture (see spot_in_output in Rust).
export function spotInOutput(spot, [iw, ih], crop, [ow, oh], mirror) {
  const sx = ow / Math.max(1e-3, crop.width), sy = oh / Math.max(1e-3, crop.height);
  let cx = (spot.cx * iw - crop.x) * sx;
  if (mirror) cx = ow - cx;
  return { cx, cy: (spot.cy * ih - crop.y) * sy, rx: spot.rx * iw * sx, ry: spot.ry * ih * sy };
}
