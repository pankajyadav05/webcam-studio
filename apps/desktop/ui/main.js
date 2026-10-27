// Webcam Studio: wires the page together.
//
// The picture is drawn by the engine (framing, background blur and replacement). This file
// owns everything around it: the controls, moving the picture by hand, permissions, and
// remembering the choices.

import { TUNING_DEFAULTS } from "./engine/tuning.js";
import { APPEARANCE_DEFAULTS, isLookActive } from "./engine/look.js";
import { createPreviewSources } from "./lib/preview-source.js";
import { createStore, clamp, ZOOM_MIN, ZOOM_MAX } from "./lib/store.js";
import { createCamera } from "./lib/camera.js";
import { createMic, FLOOR_DB } from "./lib/mic.js";
import { PRESETS, drawPreset, presetSource, loadImageFile, imageFromDataUrl } from "./lib/backgrounds.js";
import { createMeetings, bindOpenAtLogin } from "./lib/meetings.js";
import { bindUpdates } from "./lib/updates.js";
import { drawAwayCard } from "./lib/away-card.js";
import { addScene, removeScene, activeScene, scenePatch, describeLook, lookOf } from "./lib/scenes.js";
import { invoke, hasBackend, listen } from "./lib/tauri.js";
import { initWindow } from "./lib/window.js";

initWindow();

const $ = (id) => document.getElementById(id);
const els = {
  frame: $("frame"),
  preview: $("preview"),
  video: $("video"),
  grid: $("grid"),
  zoomChip: $("zoom-chip"),
  effectChip: $("effect-chip"),
  notice: $("notice"),
  noticeTitle: $("notice-title"),
  noticeText: $("notice-text"),
  noticePrimary: $("notice-primary"),
  noticeRetry: $("notice-retry"),
  zoom: $("zoom"),
  zoomOut: $("zoom-out"),
  zoomIn: $("zoom-in"),
  zoomReadout: $("zoom-readout"),
  mirror: $("mirror"),
  gridToggle: $("grid-toggle"),
  reset: $("reset"),
  camera: $("camera"),
  cameraInfo: $("camera-info"),
  bgGroup: $("background-mode"),
  blurField: $("blur-field"),
  blur: $("blur"),
  blurReadout: $("blur-readout"),
  replaceField: $("replace-field"),
  swatches: $("swatches"),
  imageFile: $("image-file"),
  effectProblem: $("effect-problem"),
  mic: $("mic"),
  meter: $("meter"),
  meterBar: $("meter-bar"),
  meterPeak: $("meter-peak"),
  micInfo: $("mic-info"),
  micFix: $("mic-fix"),
  meetings: $("meetings"),
  meetingsAction: $("meetings-action"),
  meetingsInfo: $("meetings-info"),
  panel: $("panel"),
  cameraButton: $("camera-button"),
  cameraButtonText: $("camera-button-text"),
  cameraMenu: $("camera-menu"),
  formatChip: $("format-chip"),
  formatText: $("format-text"),
  micSummary: $("mic-summary"),
  autoFrame: $("auto-frame"),
  autoChip: $("auto-chip"),
  touchUp: $("touch-up"),
  touchUpReadout: $("touch-up-readout"),
  lowLight: $("low-light"),
  studioLight: $("studio-light"),
  studioLightReadout: $("studio-light-readout"),
  warmth: $("warmth"),
  warmthReadout: $("warmth-readout"),
  appearanceSummary: $("appearance-summary"),
  advancedRow: $("advanced-row"),
  advancedSummary: $("advanced-summary"),
};

// What the camera is delivering, for the pill on the picture. Null until it opens.
let cameraFormat = null;

const store = createStore();
const camera = createCamera(els.video);

let engine = null;
let sources = null;
let nativeQualityAvailable = false;
let cropRect = null;
let OUTPUT = { width: 1280, height: 720 };
let customSource = null; // the picture the person chose, once loaded
// Auto-frame needs Apple Vision to find faces; the button shows only where it can.
let autoAvailable = false;
// While auto-frame moves the picture, the zoom it reached, for the controls.
let autoZoom = 1;
// Be right back: owned by the Rust side (the menu bar and a shortcut switch it too).
let away = false;
const awayCard = drawAwayCard(1280, 720);

// ---------------------------------------------------------------------------
// Drawing the controls from the saved choices
// ---------------------------------------------------------------------------

const round3 = (n) => Math.round(n * 1000) / 1000;

function paintSlider(input) {
  const min = Number(input.min);
  const max = Number(input.max);
  input.style.setProperty("--p", `${((Number(input.value) - min) / (max - min)) * 100}%`);
}

const zoomText = (zoom) => `${zoom.toFixed(1)}×`;
const autoOn = (s) => s.autoFrame && autoAvailable;

// The zoom controls show the manual zoom, or where auto-frame has moved to.
function renderZoom(zoom, auto) {
  els.zoomOut.disabled = !auto && zoom <= ZOOM_MIN;
  els.zoomIn.disabled = !auto && zoom >= ZOOM_MAX;
  els.zoom.setAttribute("aria-valuetext", `${zoom.toFixed(2)} times${auto ? ", set by auto-frame" : ""}`);
  els.zoom.value = String(zoom);
  paintSlider(els.zoom);
  els.zoomReadout.textContent = zoomText(zoom);
  els.zoomChip.textContent = zoomText(zoom);
  els.zoomChip.hidden = auto || zoom <= ZOOM_MIN + 0.001;
  els.frame.classList.toggle("pannable", auto || zoom > ZOOM_MIN + 0.001);
}

function percent(value) {
  return `${Math.round(value * 100)}%`;
}

function appearanceSummary(a) {
  if (!isLookActive(a)) return "Off";
  const parts = [];
  if (a.smoothing > 0.001) parts.push(`Touch up ${percent(a.smoothing)}`);
  if (a.studio_light > 0.001) parts.push("Studio Light");
  if (a.low_light) parts.push("Low light");
  if (Math.abs(a.warmth) > 0.001) parts.push(a.warmth > 0 ? "Warmer" : "Cooler");
  return parts.join(" · ");
}

function render() {
  const s = store.get();
  const auto = autoOn(s);

  renderZoom(auto ? autoZoom : s.zoom, auto);
  els.autoFrame.hidden = !autoAvailable;
  els.autoFrame.setAttribute("aria-pressed", String(auto));
  els.autoChip.hidden = !auto;
  $("smart-camera").hidden = !autoAvailable;
  $("show-and-tell").checked = s.showAndTell;

  const a = s.appearance;
  els.touchUp.value = String(Math.round(a.smoothing * 100));
  paintSlider(els.touchUp);
  els.touchUp.setAttribute("aria-valuetext", a.smoothing > 0.001 ? percent(a.smoothing) : "Off");
  els.touchUpReadout.textContent = a.smoothing > 0.001 ? percent(a.smoothing) : "Off";
  els.lowLight.checked = a.low_light;
  $("studio-light-field").hidden = !autoAvailable;
  els.studioLight.value = String(Math.round(a.studio_light * 100));
  paintSlider(els.studioLight);
  els.studioLight.setAttribute("aria-valuetext", a.studio_light > 0.001 ? percent(a.studio_light) : "Off");
  els.studioLightReadout.textContent = a.studio_light > 0.001 ? percent(a.studio_light) : "Off";
  els.warmth.value = String(Math.round(a.warmth * 100));
  paintSlider(els.warmth);
  const warmthText = Math.abs(a.warmth) < 0.005 ? "Neutral" : `${a.warmth > 0 ? "Warmer" : "Cooler"} ${percent(Math.abs(a.warmth))}`;
  els.warmth.setAttribute("aria-valuetext", warmthText);
  els.warmthReadout.textContent = Math.abs(a.warmth) < 0.005 ? "0" : `${a.warmth > 0 ? "+" : "−"}${Math.round(Math.abs(a.warmth) * 100)}`;
  $("reset-appearance").disabled = !isLookActive(a);
  els.appearanceSummary.textContent = appearanceSummary(a);

  renderScenes(s);
  $("away-button").setAttribute("aria-pressed", String(away));
  $("away-chip").hidden = !away;

  els.mirror.setAttribute("aria-pressed", String(s.mirror));
  els.gridToggle.setAttribute("aria-pressed", String(s.grid));
  els.grid.classList.toggle("on", s.grid);

  const mode = s.background.mode;
  for (const radio of els.bgGroup.querySelectorAll("input")) {
    radio.checked = radio.value === mode;
  }
  const effectOn = mode !== "none";
  els.advancedRow.disabled = !effectOn;
  const tuned = Object.keys(TUNING_DEFAULTS).some((key) => Math.abs(s.background[key] - TUNING_DEFAULTS[key]) > 0.005)
    || s.background.quality !== "balanced" || s.background.view === "mask";
  els.advancedSummary.textContent = !effectOn ? "Turn on Blur or Replace first" : tuned ? "Cutout: custom" : "Cutout: default";
  if (!effectOn && currentPage() === "advanced") back();
  for (const key of Object.keys(TUNING_DEFAULTS)) {
    const input = $(key); input.value = String(s.background[key]*100);
    input.setAttribute("aria-valuetext", `${Math.round(s.background[key]*100)} percent`); paintSlider(input);
  }
  $("show-mask").checked = s.background.view === "mask";
  for (const radio of $("quality").querySelectorAll("input")) {
    radio.checked = radio.value === s.background.quality; radio.disabled = !nativeQualityAvailable;
  }
  $("quality-hint").textContent = nativeQualityAvailable
    ? "Accurate follows hair and hands more closely, but uses more power."
    : "Quality needs Apple Vision, which is not running on this Mac.";
  els.blurField.hidden = mode !== "blur";
  els.replaceField.hidden = mode !== "replace";
  els.blur.value = String(s.background.radius);
  paintSlider(els.blur);
  els.blurReadout.textContent = String(s.background.radius);
  for (const swatch of els.swatches.querySelectorAll("[data-preset]")) {
    const selected = swatch.dataset.preset === s.background.preset;
    swatch.setAttribute("aria-checked", String(selected));
    swatch.tabIndex = selected ? 0 : -1;
  }

  if (engine) {
    engine.setAppearance(s.appearance);
    engine.setShowAndTell(s.showAndTell && autoAvailable);
    engine.setTuning(s.background);
    engine.setQuality(s.background.quality);
    engine.setView(s.background.view);
    engine.setFraming({ zoom: s.zoom, pan_x: s.pan_x, pan_y: s.pan_y, mirror: s.mirror, auto });
    engine.setBackground({
      mode,
      radius: s.background.radius,
      image: mode === "replace" ? backgroundImage(s) : null,
    });
  }
  syncBackend();
}

function backgroundImage(s) {
  if (s.background.preset === "custom") {
    return customSource;
  }
  return presetSource(s.background.preset, OUTPUT.width, OUTPUT.height);
}

store.subscribe(render);

// The Rust side keeps its own copy of the settings for the virtual camera, so meetings get
// the same framing, effect and cutout as the preview. It is best effort: the window works
// without it, so a failure is not worth showing.
let syncTimer = 0;
let sentPicture = "";
function syncBackend() {
  if (!hasBackend()) {
    return;
  }
  clearTimeout(syncTimer);
  syncTimer = setTimeout(() => {
    const s = store.get();
    const b = s.background;
    const picture = b.mode === "replace" ? pictureKey(s) : "";
    invoke("set_settings", {
      settings: {
        framing: { zoom: s.zoom, pan_x: s.pan_x, pan_y: s.pan_y, mirror: s.mirror, auto_frame: autoOn(s), show_and_tell: s.showAndTell && autoAvailable },
        background:
          b.mode === "blur" ? { mode: "blur", radius: b.radius }
          : picture ? { mode: "replace", image_path: picture }
          : { mode: "none" },
        noise: "off",
        output_width: OUTPUT.width,
        output_height: OUTPUT.height,
        output_fps: 30,
        cutout: { cutoff: b.cutoff, softness: b.softness, steadiness: b.steadiness },
        appearance: { ...s.appearance },
      },
    }).catch(() => {});
    if (picture && picture !== sentPicture) {
      sendPicture(picture, backgroundImage(s));
    }
  }, 200);
}

// Names the Replace picture; the Rust side keeps a picture while its name stays the same.
function pictureKey(s) {
  const b = s.background;
  if (b.preset !== "custom") {
    return `window:${b.preset}`;
  }
  if (!b.custom || !customSource) {
    return "";
  }
  let hash = 0;
  for (let i = 0; i < b.custom.length; i += 97) {
    hash = (hash * 31 + b.custom.charCodeAt(i)) | 0;
  }
  return `window:custom:${b.custom.length}:${hash >>> 0}`;
}

// The virtual camera cannot read the window's canvas, so it gets the picture as a JPEG.
function sendPicture(key, source) {
  if (!source) {
    return;
  }
  const width = source.naturalWidth || source.width, height = source.naturalHeight || source.height;
  const scale = Math.min(1, 1920 / width, 1080 / height);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  canvas.getContext("2d").drawImage(source, 0, 0, canvas.width, canvas.height);
  sentPicture = key;
  canvas.toBlob(async (blob) => {
    try {
      if (!blob) throw new Error("no picture");
      await invoke("set_window_picture", new Uint8Array(await blob.arrayBuffer()), { headers: { key } });
    } catch {
      if (sentPicture === key) sentPicture = "";
    }
  }, "image/jpeg", 0.92);
}

// ---------------------------------------------------------------------------
// Moving the picture: zoom, pan, reset
// ---------------------------------------------------------------------------

// Moving the picture by hand ends auto-frame where it got to, so nothing jumps.
function takeManual() {
  const s = store.get();
  if (!s.autoFrame) return;
  const f = autoOn(s) ? engine?.getFraming() : null;
  store.set({ autoFrame: false, ...(f ? { zoom: round3(f.zoom), pan_x: round3(f.pan_x), pan_y: round3(f.pan_y) } : {}) });
}

function toggleAutoFrame() {
  if (!autoAvailable) return;
  if (store.get().autoFrame) takeManual();
  else store.set({ autoFrame: true });
}

// Zooms to `zoom`, keeping the point under `anchor` (0..1 across the frame) where it is.
function zoomTo(zoom, anchor) {
  takeManual();
  const s = store.get();
  const next = round3(clamp(zoom, ZOOM_MIN, ZOOM_MAX, 1));
  const vw = (sources?.video || els.video).videoWidth;
  const vh = (sources?.video || els.video).videoHeight;
  if (!(vw > 0) || !cropRect) {
    store.set({ zoom: next });
    return;
  }
  const from = cropRect(vw, vh, OUTPUT.width, OUTPUT.height, s);
  const to = cropRect(vw, vh, OUTPUT.width, OUTPUT.height, { ...s, zoom: next });
  // The picture may be mirrored, so a point on screen is the opposite point in the camera.
  const fx = s.mirror ? 1 - (anchor ? anchor.x : 0.5) : anchor ? anchor.x : 0.5;
  const fy = anchor ? anchor.y : 0.5;
  const px = from.x + fx * from.width;
  const py = from.y + fy * from.height;
  store.set({ zoom: next, ...panFor(to, px - fx * to.width, py - fy * to.height, vw, vh) });
}

// Turns a wanted top-left corner of the crop into pan values, which are -1..1 across the
// space the crop has to move in.
function panFor(crop, x, y, vw, vh) {
  const marginX = vw - crop.width;
  const marginY = vh - crop.height;
  return {
    pan_x: marginX > 0.5 ? round3(clamp((x / marginX) * 2 - 1, -1, 1, 0)) : 0,
    pan_y: marginY > 0.5 ? round3(clamp((y / marginY) * 2 - 1, -1, 1, 0)) : 0,
  };
}

function resetFraming() {
  store.set({ autoFrame: false, zoom: 1, pan_x: 0, pan_y: 0 });
}

function bindFraming() {
  els.zoom.addEventListener("input", () => zoomTo(Number(els.zoom.value), null));
  els.zoomOut.addEventListener("click", () => zoomTo(store.get().zoom - 0.25, null));
  els.zoomIn.addEventListener("click", () => zoomTo(store.get().zoom + 0.25, null));
  els.mirror.addEventListener("click", () => store.set({ mirror: !store.get().mirror }));
  els.autoFrame.addEventListener("click", toggleAutoFrame);
  els.gridToggle.addEventListener("click", () => store.set({ grid: !store.get().grid }));
  els.reset.addEventListener("click", resetFraming);
  els.frame.addEventListener("dblclick", resetFraming);

  const anchorOf = (event) => {
    const box = els.frame.getBoundingClientRect();
    return { x: clamp((event.clientX - box.left) / box.width, 0, 1, 0.5), y: clamp((event.clientY - box.top) / box.height, 0, 1, 0.5) };
  };

  els.frame.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      const delta = Math.max(-120, Math.min(120, event.deltaMode === 1 ? event.deltaY * 33 : event.deltaY));
      zoomTo(store.get().zoom * Math.exp(-delta * 0.004), anchorOf(event));
    },
    { passive: false },
  );

  // Pinch on a trackpad arrives as gesture events in this web view.
  let pinchFrom = 1;
  els.frame.addEventListener("gesturestart", (event) => {
    event.preventDefault();
    takeManual();
    pinchFrom = store.get().zoom;
  });
  els.frame.addEventListener("gesturechange", (event) => {
    event.preventDefault();
    zoomTo(pinchFrom * event.scale, anchorOf(event));
  });

  // Drag the picture: it follows the pointer, the way a map does.
  let drag = null;
  els.frame.addEventListener("pointerdown", (event) => {
    const s = store.get();
    const auto = autoOn(s);
    if (event.target.closest("button") || event.button !== 0 || (!auto && s.zoom <= ZOOM_MIN + 0.001) || !cropRect || !engine) {
      return;
    }
    els.frame.setPointerCapture(event.pointerId);
    els.frame.classList.add("panning");
    // With auto-frame on, a click alone keeps it; only a real drag takes over.
    drag = { x: event.clientX, y: event.clientY, crop: engine.getCrop(), box: els.frame.getBoundingClientRect(), pending: auto };
  });
  els.frame.addEventListener("pointermove", (event) => {
    if (!drag) {
      return;
    }
    if (drag.pending) {
      if (Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 4) return;
      takeManual();
      drag = { ...drag, crop: engine.getCrop(), pending: false };
    }
    const s = store.get();
    const vw = (sources?.video || els.video).videoWidth;
    const vh = (sources?.video || els.video).videoHeight;
    const dx = (event.clientX - drag.x) * (drag.crop.width / drag.box.width);
    const dy = (event.clientY - drag.y) * (drag.crop.height / drag.box.height);
    // A mirrored picture moves the opposite way in the camera.
    const x = drag.crop.x - (s.mirror ? -dx : dx);
    const y = drag.crop.y - dy;
    store.set(panFor(drag.crop, x, y, vw, vh));
  });
  const endDrag = () => {
    drag = null;
    els.frame.classList.remove("panning");
  };
  els.frame.addEventListener("pointerup", endDrag);
  els.frame.addEventListener("pointercancel", endDrag);

  // Keyboard: arrows move the picture, plus and minus zoom, zero resets.
  document.addEventListener("keydown", (event) => {
    if (event.metaKey || event.ctrlKey || event.altKey) {
      return;
    }
    // Keys that belong to the control that has focus are left alone: a dropdown types to
    // search, and radio buttons and sliders use the arrows. The other shortcuts still work
    // after clicking one of them, which is where focus usually sits.
    const target = event.target;
    const tag = target && target.tagName;
    const usesArrows = tag === "SELECT" || tag === "TEXTAREA" || (tag === "INPUT" && ["radio", "range", "text"].includes(target.type));
    if (tag === "SELECT" || tag === "TEXTAREA" || (tag === "INPUT" && target.type === "text")) {
      return;
    }
    if (usesArrows && event.key.startsWith("Arrow")) {
      return;
    }
    const s = store.get();
    const step = event.shiftKey ? 0.25 : 0.08;
    const nudge = (key, value) => {
      event.preventDefault();
      const before = store.get();
      takeManual();
      const after = store.get();
      // `value` was worked out from the framing before auto-frame let go.
      store.set({ [key]: round3(clamp(value - before[key] + after[key], -1, 1, 0)) });
    };
    switch (event.key) {
      case "ArrowLeft":
        return nudge("pan_x", s.pan_x + (s.mirror ? step : -step));
      case "ArrowRight":
        return nudge("pan_x", s.pan_x + (s.mirror ? -step : step));
      case "ArrowUp":
        return nudge("pan_y", s.pan_y - step);
      case "ArrowDown":
        return nudge("pan_y", s.pan_y + step);
      case "+":
      case "=":
        event.preventDefault();
        return zoomTo(s.zoom + 0.25, null);
      case "-":
      case "_":
        event.preventDefault();
        return zoomTo(s.zoom - 0.25, null);
      case "0":
        event.preventDefault();
        return resetFraming();
      case "m":
      case "M":
        return store.set({ mirror: !s.mirror });
      case "g":
      case "G":
        return store.set({ grid: !s.grid });
      case "f":
      case "F":
        return toggleAutoFrame();
      default:
    }
  });
}

// ---------------------------------------------------------------------------
// Background: none, blur, or a picture
// ---------------------------------------------------------------------------

function buildSwatches() {
  const tiles = PRESETS.map((preset) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "swatch";
    button.dataset.preset = preset.id;
    button.setAttribute("role", "radio");
    button.setAttribute("aria-label", preset.name);
    button.title = preset.name;
    const canvas = document.createElement("canvas");
    canvas.width = 160;
    canvas.height = 90;
    drawPreset(canvas.getContext("2d"), 160, 90, preset);
    button.append(canvas);
    button.addEventListener("click", () => store.set({ background: { preset: preset.id } }));
    return button;
  });

  const choose = document.createElement("button");
  choose.type = "button";
  choose.className = "swatch add";
  choose.title = "Use your own picture";
  choose.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg><span>Choose…</span>';
  choose.addEventListener("click", () => els.imageFile.click());

  els.swatches.replaceChildren(...tiles, choose);
}

// The person's own picture gets a tile once there is one.
function showCustomSwatch(dataUrl) {
  els.swatches.querySelector("[data-preset=custom]")?.remove();
  const button = document.createElement("button");
  button.type = "button";
  button.className = "swatch";
  button.dataset.preset = "custom";
  button.setAttribute("role", "radio");
  button.setAttribute("aria-label", "Your picture");
  button.title = "Your picture";
  const image = document.createElement("img");
  image.alt = "";
  image.src = dataUrl;
  button.append(image);
  button.addEventListener("click", () => store.set({ background: { preset: "custom" } }));
  els.swatches.insertBefore(button, els.swatches.querySelector(".add"));
  render();
}

function showEffectProblem(text) {
  els.effectProblem.hidden = !text;
  els.effectProblem.textContent = text || "";
}

function bindBackground() {
  for (const key of Object.keys(TUNING_DEFAULTS)) $(key).addEventListener("input", () => {
    store.set({background:{[key]:Number($(key).value)/100}});
  });
  $("quality").addEventListener("change", event => store.set({background:{quality:event.target.value}}));
  $("show-mask").addEventListener("change", () => store.set({background:{view:$("show-mask").checked ? "mask" : "composite"}}));
  $("reset-tuning").addEventListener("click", () => store.set({background:{...TUNING_DEFAULTS,quality:"balanced",view:"composite"}}));
  els.swatches.addEventListener("keydown", (event) => {
    const choices = [...els.swatches.querySelectorAll("[data-preset]")];
    const index = choices.indexOf(event.target);
    if (index < 0 || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    const next = event.key === "Home" ? 0 : event.key === "End" ? choices.length - 1
      : (index + (event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1) + choices.length) % choices.length;
    choices[next].click();
    choices[next].focus();
  });
  els.bgGroup.addEventListener("change", (event) => {
    store.set({ background: { mode: event.target.value } });
  });
  els.blur.addEventListener("input", () => store.set({ background: { radius: Number(els.blur.value) } }));

  els.imageFile.addEventListener("change", async () => {
    const file = els.imageFile.files[0];
    els.imageFile.value = "";
    if (!file) {
      return;
    }
    try {
      const picked = await loadImageFile(file);
      customSource = picked.source;
      showCustomSwatch(picked.dataUrl);
      store.set({ background: { mode: "replace", preset: "custom", custom: picked.dataUrl } });
      showEffectProblem("");
    } catch {
      showEffectProblem("That file could not be opened as a picture. Try a JPEG or PNG.");
    }
  });
}

// ---------------------------------------------------------------------------
// Appearance: touch up, low light, warmth
// ---------------------------------------------------------------------------

function bindAppearance() {
  els.touchUp.addEventListener("input", () => store.set({ appearance: { smoothing: Number(els.touchUp.value) / 100 } }));
  els.studioLight.addEventListener("input", () => store.set({ appearance: { studio_light: Number(els.studioLight.value) / 100 } }));
  els.lowLight.addEventListener("change", () => store.set({ appearance: { low_light: els.lowLight.checked } }));
  els.warmth.addEventListener("input", () => {
    // Neutral is easy to find again: values near the middle snap to it.
    const value = Number(els.warmth.value);
    store.set({ appearance: { warmth: Math.abs(value) <= 3 ? 0 : value / 100 } });
  });
  els.warmth.addEventListener("dblclick", () => store.set({ appearance: { warmth: 0 } }));
  $("reset-appearance").addEventListener("click", () => store.set({ appearance: { ...APPEARANCE_DEFAULTS } }));
}

// ---------------------------------------------------------------------------
// Scenes: a saved look, back in one click (here, the menu bar, or ⌘1 to ⌘9)
// ---------------------------------------------------------------------------

let publishedScenes = "";
function renderScenes(s) {
  const active = activeScene(s.scenes, s);
  const current = s.scenes.find((scene) => scene.id === active);
  $("scenes-summary").textContent = current ? current.name : s.scenes.length ? "Custom look" : "None saved";
  $("scene-now").textContent = `Now: ${describeLook(lookOf(s))}${current ? ` · saved as ${current.name}` : ""}`;
  const list = $("scene-list");
  list.hidden = s.scenes.length === 0;
  const focused = document.activeElement?.closest?.("[data-scene]")?.dataset.scene;
  list.replaceChildren(...s.scenes.map((scene, index) => {
    const row = document.createElement("div");
    row.className = "row scene-row";
    row.dataset.scene = scene.id;
    const pick = document.createElement("button");
    pick.type = "button";
    pick.className = "scene-pick";
    pick.setAttribute("role", "radio");
    pick.setAttribute("aria-checked", String(scene.id === active));
    pick.innerHTML = '<span class="radio" aria-hidden="true"></span><span class="row-text"><b></b><small></small></span><span class="scene-key" aria-hidden="true"></span>';
    pick.querySelector("b").textContent = scene.name;
    pick.querySelector("small").textContent = describeLook(scene.look);
    pick.querySelector(".scene-key").textContent = index < 9 ? `⌘${index + 1}` : "";
    pick.addEventListener("click", () => applyScene(scene.id));
    const remove = document.createElement("button");
    remove.type = "button";
    remove.className = "icon-btn scene-delete";
    remove.setAttribute("aria-label", `Delete ${scene.name}`);
    remove.title = "Delete scene";
    remove.innerHTML = '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>';
    remove.addEventListener("click", () => store.set({ scenes: removeScene(store.get().scenes, scene.id) }));
    row.append(pick, remove);
    return row;
  }));
  if (focused) list.querySelector(`[data-scene="${focused}"] .scene-pick`)?.focus({ preventScroll: true });
  $("scene-save").disabled = !$("scene-name").value.trim();
  // The menu bar lists the scenes too.
  const board = JSON.stringify({ scenes: s.scenes.map(({ id, name }) => ({ id, name })), active });
  if (hasBackend() && board !== publishedScenes) {
    publishedScenes = board;
    invoke("publish_scenes", { board: JSON.parse(board) }).catch(() => {});
  }
}

function applyScene(id) {
  const scene = store.get().scenes.find((s) => s.id === id);
  if (!scene) return;
  const patch = scenePatch(scene);
  // A Replace picture that is no longer there falls back to the studio backdrop.
  if (patch.background.preset === "custom" && !store.get().background.custom) patch.background.preset = "studio";
  store.set(patch);
}

function bindShowAndTell() {
  $("show-and-tell").addEventListener("change", (event) => store.set({ showAndTell: event.target.checked }));
}

function bindScenes() {
  const name = $("scene-name");
  name.addEventListener("input", () => { $("scene-save").disabled = !name.value.trim(); });
  $("scene-form").addEventListener("submit", (event) => {
    event.preventDefault();
    if (!name.value.trim()) return;
    store.set({ scenes: addScene(store.get().scenes, name.value, store.get()) });
    name.value = "";
    $("scene-save").disabled = true;
  });
  document.addEventListener("keydown", (event) => {
    if (!event.metaKey || event.altKey || event.ctrlKey || !/^[1-9]$/.test(event.key)) return;
    const scene = store.get().scenes[Number(event.key) - 1];
    if (!scene) return;
    event.preventDefault();
    applyScene(scene.id);
  });
  if (hasBackend()) listen("apply-scene", (event) => applyScene(event.payload));
}

// ---------------------------------------------------------------------------
// Be right back
// ---------------------------------------------------------------------------

function showAway(on) {
  away = !!on;
  engine?.setAway(away, awayCard);
  render();
}

function setAway(on) {
  if (!hasBackend()) return showAway(on);
  invoke("set_away", { away: on }).then(showAway).catch(() => {});
}

function bindAway() {
  $("away-button").addEventListener("click", () => setAway(!away));
  $("away-back").addEventListener("click", () => setAway(false));
  if (!hasBackend()) return;
  listen("away-changed", (event) => showAway(event.payload));
  invoke("away").then(showAway).catch(() => {});
  // The meeting camera draws the same card.
  awayCard.toBlob(async (blob) => {
    if (!blob) return;
    try { await invoke("set_away_card", new Uint8Array(await blob.arrayBuffer())); } catch { /* the card is optional */ }
  }, "image/png");
}

// ---------------------------------------------------------------------------
// Settings pages: the first page holds Background; the rest are one step in
// ---------------------------------------------------------------------------

const pages = new Map([...els.panel.querySelectorAll(".page")].map((page) => [page.dataset.page, page]));
const pageStack = ["root"];
const currentPage = () => pageStack[pageStack.length - 1];

function showPage(name, direction) {
  for (const [key, page] of pages) {
    page.hidden = key !== name;
  }
  const page = pages.get(name);
  page.classList.remove("from-right", "from-left");
  // Restart the entrance: a page arrives from the side its row or back button points to.
  void page.offsetWidth;
  page.classList.add(direction === "back" ? "from-left" : "from-right");
  page.scrollTop = 0;
  const focusTarget = direction === "back"
    ? page.querySelector(`[data-go="${pageStack.lastLeft || ""}"]`)
    : page.querySelector(".page-title");
  focusTarget?.focus({ preventScroll: true });
}

function go(name) {
  if (!pages.has(name) || currentPage() === name) return;
  // Opening samples from Advanced and going back should land on Advanced, not loop.
  const existing = pageStack.indexOf(name);
  if (existing > 0) pageStack.length = existing;
  pageStack.push(name);
  pageStack.lastLeft = null;
  showPage(name, "forward");
  if (name === "samples") sources?.listSamples();
  micForPage();
}

function back() {
  if (pageStack.length < 2) return;
  pageStack.lastLeft = pageStack.pop();
  showPage(currentPage(), "back");
  micForPage();
}

function bindPages() {
  $("settings-button").addEventListener("click", () => (currentPage() === "settings" ? back() : go("settings")));
  document.addEventListener("keydown", (event) => {
    if (event.metaKey && event.key === ",") {
      event.preventDefault();
      go("settings");
    }
  });
  els.panel.addEventListener("click", (event) => {
    const target = event.target.closest("[data-go], [data-back]");
    if (!target || target.disabled) return;
    if (target.hasAttribute("data-back")) back();
    else go(target.dataset.go);
  });
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    if (!els.cameraMenu.hidden) return closeCameraMenu(true);
    if (event.target?.tagName === "SELECT") return;
    back();
  });
}

// ---------------------------------------------------------------------------
// Camera picker under the picture
// ---------------------------------------------------------------------------

function openCameraMenu() {
  if (els.cameraButton.disabled) return;
  els.cameraMenu.hidden = false;
  els.cameraButton.setAttribute("aria-expanded", "true");
  (els.cameraMenu.querySelector('[aria-checked="true"]') || els.cameraMenu.querySelector("[role^=menuitem]"))?.focus();
}

function closeCameraMenu(restoreFocus) {
  if (els.cameraMenu.hidden) return;
  els.cameraMenu.hidden = true;
  els.cameraButton.setAttribute("aria-expanded", "false");
  if (restoreFocus) els.cameraButton.focus();
}

function chooseCamera(deviceId) {
  if (deviceId === store.get().camera && sources?.get().active === "camera") return;
  store.set({ camera: deviceId });
  sources?.select("camera");
  startCamera();
}

function buildCameraMenu(devices, activeId) {
  const items = devices.map((device, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.setAttribute("role", "menuitemradio");
    item.setAttribute("aria-checked", String(device.deviceId === activeId));
    item.textContent = device.label || `Camera ${index + 1}`;
    item.addEventListener("click", () => {
      closeCameraMenu(true);
      chooseCamera(device.deviceId);
    });
    return item;
  });
  const settings = document.createElement("button");
  settings.type = "button";
  settings.setAttribute("role", "menuitem");
  settings.textContent = "Camera settings…";
  settings.addEventListener("click", () => {
    closeCameraMenu(false);
    go("camera");
  });
  const rule = document.createElement("hr");
  els.cameraMenu.replaceChildren(...(items.length ? items : [Object.assign(document.createElement("p"), { textContent: "No camera found" })]), rule, settings);
  const active = devices.find((d) => d.deviceId === activeId);
  els.cameraButtonText.textContent = active?.label || (devices.length ? "Camera" : "No camera");
}

function bindCameraMenu() {
  els.cameraButton.addEventListener("click", () => (els.cameraMenu.hidden ? openCameraMenu() : closeCameraMenu(false)));
  els.cameraMenu.addEventListener("keydown", (event) => {
    const items = [...els.cameraMenu.querySelectorAll("button")];
    const index = items.indexOf(document.activeElement);
    if (!["ArrowDown", "ArrowUp", "Home", "End", "Tab"].includes(event.key)) return;
    if (event.key === "Tab") return closeCameraMenu(false);
    event.preventDefault();
    event.stopPropagation();
    const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1
      : (index + (event.key === "ArrowUp" ? -1 : 1) + items.length) % items.length;
    items[next]?.focus();
  });
  document.addEventListener("pointerdown", (event) => {
    if (!event.target.closest(".camera-pick")) closeCameraMenu(false);
  });
}

// The pill on the picture says what the camera is delivering, and only while it is shown.
function renderFormat() {
  const showing = cameraFormat && els.notice.hidden && (!sources || sources.get().active === "camera");
  els.formatChip.hidden = !showing;
  if (cameraFormat) els.formatText.textContent = cameraFormat;
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

function showNotice(problem) {
  const hasSettings = problem.kind === "denied" && hasBackend();
  els.noticeTitle.textContent = problem.title;
  els.noticeText.textContent = problem.text;
  els.noticePrimary.hidden = !hasSettings;
  els.noticePrimary.onclick = () => invoke("open_privacy_settings", { kind: "camera" }).catch(() => {});
  els.noticeRetry.hidden = false;
  els.notice.hidden = sources?.get().active !== "camera";
  renderFormat();
}

function fillSelect(select, devices, selectedId, fallbackLabel) {
  select.replaceChildren(
    ...devices.map((device, index) => {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || `${fallbackLabel} ${index + 1}`;
      return option;
    }),
  );
  select.disabled = devices.length === 0;
  if (devices.some((d) => d.deviceId === selectedId)) {
    select.value = selectedId;
  }
}

async function refreshCameraList(activeId) {
  const devices = await camera.list();
  fillSelect(els.camera, devices, activeId, "Camera");
  buildCameraMenu(devices, activeId);
  if (devices.length === 0) {
    els.camera.replaceChildren(new Option("No camera found", ""));
  }
}

let cameraPending = false;
async function startCamera() {
  if (windowHidden || cameraPending || sources?.get().recording) return;
  cameraPending = true;
  els.camera.disabled = true;
  els.cameraButton.disabled = true;
  els.cameraInfo.textContent = "Opening camera…";
  cameraFormat = null;
  renderFormat();
  els.noticeTitle.textContent = "Opening your camera…";
  els.noticeText.textContent = "Allow camera access when asked. Your picture stays on this Mac.";
  els.noticePrimary.hidden = true;
  els.noticeRetry.hidden = true;
  els.notice.hidden = sources?.get().active !== "camera";
  const result = await camera.start(store.get().camera, () =>
    showNotice({ kind: "other", title: "The camera was disconnected", text: "Reconnect it, or pick another camera, then try again." }),
  );
  cameraPending = false;
  els.cameraButton.disabled = false;
  if (windowHidden) {
    // Closed while the camera was opening: let go of it straight away.
    camera.stop();
    return;
  }
  if (!result.ok) {
    showNotice(result.problem);
    els.cameraInfo.textContent = result.problem.title;
    await refreshCameraList("").catch(() => {});
    return;
  }
  els.notice.hidden = true;
  const { width, height, frameRate } = result.settings;
  const fps = frameRate ? ` · ${Math.round(frameRate)} fps` : "";
  els.cameraInfo.textContent = `${width} × ${height}${fps}`;
  cameraFormat = `${Math.min(width, height)}p${fps}`;
  renderFormat();
  await refreshCameraList(result.deviceId).catch(() => {});
  if (result.deviceId && result.deviceId !== store.get().camera) {
    store.set({ camera: result.deviceId });
  }
}

function bindCamera() {
  els.camera.addEventListener("change", () => chooseCamera(els.camera.value));
  els.noticeRetry.addEventListener("click", startCamera);
  navigator.mediaDevices?.addEventListener?.("devicechange", () => {
    if (cameraPending || !els.notice.hidden) {
      return;
    }
    refreshCameraList(store.get().camera).catch(() => {});
    refreshMicList().catch(() => {});
  });
}

// ---------------------------------------------------------------------------
// Microphone
// ---------------------------------------------------------------------------

const mic = createMic((db, peak) => {
  const level = clamp((db - FLOOR_DB) / -FLOOR_DB, 0, 1, 0);
  els.meterBar.style.transform = `scaleX(${level})`;
  els.meter.classList.toggle("hot", db > -12 && db <= -3);
  els.meter.classList.toggle("clip", db > -3);
  const peakLevel = clamp((peak - FLOOR_DB) / -FLOOR_DB, 0, 1, 0);
  els.meterPeak.style.left = `calc(${peakLevel * 100}% - 2px)`;
  els.meterPeak.style.opacity = peak > FLOOR_DB + 1 ? "0.8" : "0";
  els.meter.setAttribute("aria-valuenow", String(Math.round(db)));
});

async function refreshMicList(activeId = store.get().mic.deviceId) {
  const devices = await mic.list();
  fillSelect(els.mic, devices, activeId, "Microphone");
  if (devices.length === 0) {
    els.mic.replaceChildren(new Option("No microphone found", ""));
  }
}

// The microphone is open only while its page is showing, and never in the background: a
// meeting app may be using it.
let micOpen = false;
let micPending = false;
const micWanted = () => !windowHidden && currentPage() === "mic";

function micForPage() {
  if (micWanted()) {
    if (!micOpen) startMic();
  } else if (micOpen) {
    mic.stop();
    micOpen = false;
    els.micInfo.textContent = "The microphone is off.";
  }
}

async function startMic() {
  if (!micWanted() || micPending) return;
  micPending = true;
  els.mic.disabled = true;
  els.micInfo.textContent = "Opening microphone…";
  els.mic.replaceChildren(new Option("Opening microphone…", ""));
  els.micFix.hidden = true;
  const s = store.get().mic;
  const result = await mic.start({ deviceId: s.deviceId });
  micPending = false;
  if (!micWanted()) {
    // The page closed while the microphone was opening: let go of it straight away.
    mic.stop();
    micOpen = false;
    return;
  }
  if (!result.ok) {
    micOpen = false;
    els.micInfo.textContent = result.problem.text;
    els.micSummary.textContent = result.problem.text;
    els.micFix.hidden = !(result.problem.kind === "denied" && hasBackend());
    await refreshMicList("").catch(() => {});
    return;
  }
  micOpen = true;
  els.micInfo.textContent = "Speak to see the level.";
  await refreshMicList(result.deviceId).catch(() => {});
  els.micSummary.textContent = els.mic.selectedOptions[0]?.textContent || "Microphone";
  if (result.deviceId && result.deviceId !== s.deviceId) {
    store.set({ mic: { deviceId: result.deviceId } });
  }
}

function bindMic() {
  els.mic.addEventListener("change", () => {
    store.set({ mic: { deviceId: els.mic.value } });
    mic.stop();
    micOpen = false;
    startMic();
  });
  els.micFix.addEventListener("click", () => invoke("open_privacy_settings", { kind: "microphone" }).catch(() => {}));
}

// ---------------------------------------------------------------------------
// Start up
// ---------------------------------------------------------------------------

async function startEngine() {
  try {
    const module = await import("./engine/studio-engine.js");
    cropRect = module.cropRect;
    OUTPUT = { ...module.OUTPUT_DEFAULT };
    engine = module.createStudioEngine({ canvas: els.preview, width: OUTPUT.width, height: OUTPUT.height });
  } catch (err) {
    showNotice({ kind: "other", title: "The picture could not start", text: String((err && err.message) || err) });
    return false;
  }
  els.frame.style.setProperty("--ar", String(OUTPUT.width / OUTPUT.height));
  engine.on("framing", (framing) => {
    autoZoom = framing.zoom;
    if (autoOn(store.get())) renderZoom(autoZoom, true);
    $("show-chip").hidden = !engine.showing();
  });
  engine.on("status", (status) => {
    nativeQualityAvailable = status.matting === "native";
    render();
    els.effectChip.hidden = status.state !== "loading";
    showEffectProblem(status.state === "error" ? status.message || "The background effect is not available." : "");
  });
  sources = createPreviewSources({ camera:els.video, invoke,
    onSource(video) {
      engine.setSource(video);
      els.notice.hidden = video !== els.video || els.video.readyState >= 2;
      els.frame.setAttribute("aria-label", `${video === els.video ? "Camera" : "Clip"} preview. Drag to move the picture, scroll to zoom, double-click to reset.`);
      renderFormat();
    },
    onChange: renderSource,
  });
  pages.get("samples").addEventListener("click", (event) => {
    const row = event.target.closest("[data-source], [data-sample]");
    if (!row || row.disabled) return;
    if (row.dataset.sample) sources.chooseClip(row.dataset.sample).catch(() => {});
    else sources.select(row.dataset.source);
  });
  $("record-clip").addEventListener("click", () => sources.record());
  $("source-retry").addEventListener("click", () => {
    const s = sources.get();
    if (s.listing === "error") sources.listSamples();
    if (s.error) sources.retry();
  });
  $("use-camera").addEventListener("click", () => sources.select("camera"));
  renderSource(sources.get());
  window.addEventListener("pagehide", () => sources.dispose(), {once:true});
  engine.setMatting("auto");
  engine.setSource(els.video);
  engine.start();
  return true;
}

const SOURCE_NAMES = { camera: "Your camera", clip: "My clip" };
const megabytes = (bytes) => `${Math.max(0.1, bytes / 1e6).toFixed(1)} MB`;

function sampleRow(clip, s) {
  const row = document.createElement("button");
  row.type = "button";
  row.className = "row";
  row.dataset.sample = clip.id;
  row.setAttribute("role", "radio");
  const selected = s.selected === "sample" && s.clipId === clip.id;
  row.setAttribute("aria-checked", String(selected));
  const downloading = selected && s.status === "loading";
  const tag = downloading ? "Downloading…" : clip.cached ? "Downloaded" : megabytes(clip.bytes);
  row.innerHTML = '<span class="radio" aria-hidden="true"></span><span class="clip-tile" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M10 8.5v7l5.5-3.5z"/></svg></span><span class="row-text"><b></b><small></small></span><span class="tag"></span>';
  row.querySelector("b").textContent = clip.title || clip.id;
  row.querySelector("small").textContent = clip.credit ? `${clip.credit}` : "";
  row.querySelector(".tag").textContent = tag;
  row.querySelector(".tag").classList.toggle("busy", downloading);
  return row;
}

function renderSource(s) {
  // Sample videos page
  for (const row of pages.get("samples").querySelectorAll("[data-source]")) {
    row.setAttribute("aria-checked", String(row.dataset.source === s.selected));
  }
  $("clip-options").hidden = s.selected !== "clip";
  $("record-clip").disabled = s.recording;
  $("record-clip").textContent = s.recording ? `Recording… ${Math.max(1, Math.ceil(5 - s.progress))}s` : s.hasClip ? "Record again" : "Record 5 seconds";
  $("record-progress").hidden = !s.recording;
  $("record-progress").value = s.progress;
  $("record-hint").textContent = s.recording ? "Recording your camera, without sound" : "Picture only, no sound · kept in memory until you quit";
  $("clip-summary").textContent = s.hasClip ? "Your 5-second clip · stays on this Mac" : "Record 5 seconds of yourself · stays on this Mac";

  const list = $("sample-list");
  if (s.manifest) {
    list.replaceChildren(...s.manifest.clips.map((clip) => sampleRow(clip, s)));
  } else {
    const row = document.createElement("div");
    row.className = "row static";
    row.innerHTML = '<span class="hint"></span>';
    row.firstChild.textContent = "Loading samples…";
    list.replaceChildren(row);
  }
  list.hidden = !s.manifest && s.listing === "error";
  const problem = s.error || s.listError;
  $("source-error").hidden = !problem;
  $("source-problem").textContent = problem;
  $("source-retry").hidden = !(s.listing === "error" || (s.error && s.selected === "sample"));

  els.camera.disabled = s.recording || cameraPending || !els.camera.options.length;
  els.cameraButton.disabled = s.recording || cameraPending;

  // What the preview shows, said everywhere it matters.
  const playing = s.active === "sample" ? s.manifest?.clips.find((c) => c.id === s.activeSampleId) : null;
  const name = playing ? playing.title : SOURCE_NAMES[s.active] || "Your camera";
  $("source-summary").textContent = s.recording ? "Recording 5 seconds…" : s.active === "camera" ? "Off · showing your camera" : `Showing: ${name}`;
  $("preview-on").textContent = name;

  const badge = $("source-badge");
  badge.hidden = s.active === "camera" && !s.recording;
  $("source-badge-text").textContent = s.recording ? "Recording 5 seconds…" : s.active === "clip" ? "Your 5-second clip" : playing ? `Sample clip · not your camera · ${playing.credit}` : "Sample clip · not your camera";
  $("use-camera").hidden = s.active === "camera";
  renderFormat();
}

// While the window is closed the app lives in the menu bar: the preview, camera and
// microphone are off, and only meeting apps (through the virtual camera) use the camera.
let windowHidden = false;

function rest() {
  if (windowHidden) return;
  windowHidden = true;
  sources?.select("camera");
  engine?.stop();
  camera.stop();
  micForPage();
}

function wake() {
  if (!windowHidden) return;
  windowHidden = false;
  engine?.start();
  startCamera();
  micForPage();
}

async function init() {
  buildSwatches();
  bindFraming();
  bindBackground();
  bindCamera();
  bindMic();
  bindAppearance();
  bindScenes();
  bindShowAndTell();
  bindPages();
  bindCameraMenu();

  const saved = store.get().background.custom;
  if (saved) {
    imageFromDataUrl(saved)
      .then((image) => {
        customSource = image;
        showCustomSwatch(saved);
        syncBackend();
      })
      .catch(() => store.set({ background: { custom: null, preset: "studio" } }));
  }

  if (hasBackend()) {
    autoAvailable = await invoke("faces_available").catch(() => false);
    windowHidden = !(await invoke("window_visible").catch(() => true));
    listen("window-visibility", (event) => (event.payload ? wake() : rest()));
    listen("open-settings", () => go("settings"));
  }

  const ready = await startEngine();
  bindAway();
  if (windowHidden) engine?.stop();
  render();
  if (ready && !windowHidden) {
    // The only thing the app asks for on its own is the camera. The microphone waits until
    // its page is opened.
    await startCamera();
  }

  createMeetings({
    section: els.meetings,
    row: $("meeting-row-group"),
    summary: $("meeting-summary"),
    dot: $("meeting-dot"),
    toggle: $("meeting-enabled"),
    info: els.meetingsInfo,
    setup: $("meetings-setup"),
    action: els.meetingsAction,
    previewLabel: () => els.camera.selectedOptions[0]?.textContent || "",
  }).init();
  if (hasBackend()) bindOpenAtLogin($("open-at-login"));
  if (hasBackend()) bindUpdates({ section: $("updates"), version: $("update-version"), info: $("update-info"), action: $("update-action") });
}

init();

// Live bindings also let the local evaluation harness exercise the actual app.
export { engine, store, sources };
