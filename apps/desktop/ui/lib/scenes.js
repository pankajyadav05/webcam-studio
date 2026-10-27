// Scenes: a whole look saved under a name, put back in one click from the window or the
// menu bar. A scene holds the background, the appearance and auto-frame; not the camera,
// the zoom or the mirror, which belong to where you sit.

import { normalizeAppearance, isLookActive } from "../engine/look.js";

export const MAX_SCENES = 12;
export const MAX_NAME = 40;

const BACKGROUNDS = ["none", "blur", "replace"];

function cleanLook(raw) {
  const r = raw && typeof raw === "object" ? raw : {};
  const bg = r.background && typeof r.background === "object" ? r.background : {};
  const radius = Math.round(Number(bg.radius));
  return {
    background: {
      mode: BACKGROUNDS.includes(bg.mode) ? bg.mode : "none",
      radius: Number.isFinite(radius) ? Math.min(50, Math.max(1, radius)) : 12,
      preset: typeof bg.preset === "string" ? bg.preset : "studio",
    },
    appearance: normalizeAppearance(r.appearance && typeof r.appearance === "object" ? r.appearance : {}),
    autoFrame: r.autoFrame === true,
  };
}

// Anything read from storage may be old or hand edited.
export function cleanScenes(raw) {
  if (!Array.isArray(raw)) return [];
  const seen = new Set();
  const out = [];
  for (const s of raw) {
    if (!s || typeof s !== "object" || typeof s.id !== "string" || !s.id || seen.has(s.id)) continue;
    const name = typeof s.name === "string" ? s.name.trim().slice(0, MAX_NAME) : "";
    if (!name) continue;
    seen.add(s.id);
    out.push({ id: s.id, name, look: cleanLook(s.look) });
    if (out.length === MAX_SCENES) break;
  }
  return out;
}

// The part of the window's choices a scene keeps.
export function lookOf(state) {
  return cleanLook({ background: state.background, appearance: state.appearance, autoFrame: state.autoFrame });
}

const r2 = (n) => Math.round(n * 100);
export function sameLook(a, b) {
  const x = cleanLook(a), y = cleanLook(b);
  if (x.background.mode !== y.background.mode || x.autoFrame !== y.autoFrame) return false;
  if (x.background.mode === "blur" && x.background.radius !== y.background.radius) return false;
  if (x.background.mode === "replace" && x.background.preset !== y.background.preset) return false;
  return Object.keys(x.appearance).every((k) => (typeof x.appearance[k] === "number"
    ? r2(x.appearance[k]) === r2(y.appearance[k]) : x.appearance[k] === y.appearance[k]));
}

export function activeScene(scenes, state) {
  const look = lookOf(state);
  return scenes.find((s) => sameLook(s.look, look))?.id || null;
}

// Saves the current look. A scene with the same name is replaced; the newest is last.
export function addScene(scenes, name, state, id = `s${Date.now().toString(36)}`) {
  const clean = String(name || "").trim().slice(0, MAX_NAME);
  if (!clean) return scenes;
  const others = scenes.filter((s) => s.name.toLowerCase() !== clean.toLowerCase());
  return [...others, { id, name: clean, look: lookOf(state) }].slice(-MAX_SCENES);
}

export const removeScene = (scenes, id) => scenes.filter((s) => s.id !== id);

// What to give store.set to put a scene back.
export function scenePatch(scene) {
  const look = cleanLook(scene.look);
  return { background: { ...look.background }, appearance: { ...look.appearance }, autoFrame: look.autoFrame };
}

const pct = (v) => `${Math.round(v * 100)}%`;
export function describeLook(raw) {
  const look = cleanLook(raw);
  const parts = [];
  if (look.background.mode === "blur") parts.push("Blur");
  if (look.background.mode === "replace") parts.push("Picture");
  const a = look.appearance;
  if (a.smoothing > 0.001) parts.push(`Touch up ${pct(a.smoothing)}`);
  if (a.studio_light > 0.001) parts.push("Studio Light");
  if (a.low_light) parts.push("Low light");
  if (Math.abs(a.warmth) > 0.001) parts.push(a.warmth > 0 ? "Warmer" : "Cooler");
  if (look.autoFrame) parts.push("Auto-frame");
  if (!parts.length && !isLookActive(a)) return "Plain camera";
  return parts.join(" · ");
}
