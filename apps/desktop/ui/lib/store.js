import { TUNING_DEFAULTS, normalizeTuning } from '../engine/tuning.js';
// What the person chose, remembered between launches.
//
// The window is the one place these choices live. Anything that reads them subscribes, and
// anything that changes them calls `set`, so the picture, the controls and the saved copy
// cannot drift apart.

const KEY = "webcam-studio:v2";

export const ZOOM_MIN = 1;
export const ZOOM_MAX = 4;
export const BLUR_MIN = 1;
export const BLUR_MAX = 50;

export const DEFAULTS = Object.freeze({
  camera: "",
  zoom: 1,
  pan_x: 0,
  pan_y: 0,
  mirror: false,
  grid: false,
  background: { mode: "none", radius: 12, preset: "studio", custom: null, ...TUNING_DEFAULTS, quality: "balanced", view: "composite" },
  mic: { deviceId: "", noise: true },
});

export function clamp(value, lo, hi, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : fallback;
}

// Anything read from storage may be old, hand edited or from another version.
function clean(raw) {
  const r = raw && typeof raw === "object" ? raw : {};
  const bg = r.background && typeof r.background === "object" ? r.background : {};
  const mic = r.mic && typeof r.mic === "object" ? r.mic : {};
  return {
    camera: typeof r.camera === "string" ? r.camera : DEFAULTS.camera,
    zoom: clamp(r.zoom, ZOOM_MIN, ZOOM_MAX, DEFAULTS.zoom),
    pan_x: clamp(r.pan_x, -1, 1, 0),
    pan_y: clamp(r.pan_y, -1, 1, 0),
    mirror: r.mirror === true,
    grid: r.grid === true,
    background: {
      ...normalizeTuning(bg),
      quality: bg.quality === "accurate" ? "accurate" : "balanced",
      view: bg.view === "mask" ? "mask" : "composite",
      mode: ["none", "blur", "replace"].includes(bg.mode) ? bg.mode : "none",
      radius: Math.round(clamp(bg.radius, BLUR_MIN, BLUR_MAX, DEFAULTS.background.radius)),
      preset: typeof bg.preset === "string" ? bg.preset : DEFAULTS.background.preset,
      custom: typeof bg.custom === "string" && bg.custom.startsWith("data:image/") ? bg.custom : null,
    },
    mic: {
      deviceId: typeof mic.deviceId === "string" ? mic.deviceId : "",
      noise: mic.noise !== false,
    },
  };
}

export function createStore(storage = safeStorage()) {
  let state = clean(read(storage));
  const listeners = new Set();
  let saveTimer = 0;

  function save() {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      try {
        storage.setItem(KEY, JSON.stringify(state));
      } catch {
        // Storage may be full or blocked. The choices still hold for this session.
      }
    }, 150);
  }

  return {
    get: () => state,
    // `patch` may hold `background` or `mic` parts; those are merged, not replaced.
    set(patch) {
      const next = {
        ...state,
        ...patch,
        background: { ...state.background, ...(patch.background || {}) },
        mic: { ...state.mic, ...(patch.mic || {}) },
      };
      state = clean(next);
      save();
      listeners.forEach((fn) => fn(state));
    },
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
  };
}

function read(storage) {
  try {
    return JSON.parse(storage.getItem(KEY) || "null");
  } catch {
    return null;
  }
}

function safeStorage() {
  try {
    return window.localStorage;
  } catch {
    return { getItem: () => null, setItem: () => {} };
  }
}
