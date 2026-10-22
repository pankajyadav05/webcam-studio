"use strict";

const invoke = window.__TAURI__.core.invoke;

const $ = (id) => document.getElementById(id);
const els = {
  video: $("video"),
  preview: $("preview"),
  viewfinder: $("viewfinder"),
  stageArea: $("stage-area"),
  message: $("preview-message"),
  tally: $("tally"),
  tallyText: $("tally-text"),
  zoomReadout: $("zoom-readout"),
  thirds: $("thirds"),
  thirdsToggle: $("thirds-toggle"),
  outputSpec: $("output-spec"),
  map: $("map"),
  mapCanvas: $("map-canvas"),
  mapRect: $("map-rect"),
  mapWrap: $("map-wrap"),
  mapReadout: $("map-readout"),
  camera: $("camera"),
  zoom: $("zoom"),
  zoomOut: $("zoom-out"),
  panX: $("pan-x"),
  panXOut: $("pan-x-out"),
  panY: $("pan-y"),
  panYOut: $("pan-y-out"),
  mirror: $("mirror"),
  autoFrame: $("auto-frame"),
  bgGroup: $("background-mode"),
  blurField: $("blur-field"),
  blurRadius: $("blur-radius"),
  blurOut: $("blur-out"),
  imageField: $("image-field"),
  imagePath: $("image-path"),
  noiseGroup: $("noise"),
  micToggle: $("mic-toggle"),
  micMeter: $("mic-meter"),
  micDb: $("mic-db"),
  micStatus: $("mic-status"),
  error: $("error"),
  platformOs: $("platform-os"),
  platformBackend: $("platform-backend"),
  previewNote: $("preview-note"),
  vcamGroup: $("vcam-group"),
  vcamToggle: $("vcam-toggle"),
  vcamCamera: $("vcam-camera"),
  vcamCameraName: $("vcam-camera-name"),
  vcamLine: $("vcam-line"),
  vcamSetup: $("vcam-setup"),
  vcamInstall: $("vcam-install"),
  vcamAllow: $("vcam-allow"),
  stepInstall: $("step-install"),
  stepAllow: $("step-allow"),
  vcamPill: $("vcam-status"),
  vcamPillText: $("vcam-status-text"),
};

// ---------------------------------------------------------------------------
// Framing maths. This is a port of `crop_rect` in crates/engine/src/framing.rs,
// so the preview shows exactly the region the engine will crop.
// ---------------------------------------------------------------------------

const ZOOM_MIN = 1;
const ZOOM_MAX = 4;
const DEFAULT_ZOOM = 1;
const DEFAULT_BLUR = 8;

function clampOr(value, lo, hi, fallback) {
  const n = Number(value);
  if (!Number.isFinite(n)) {
    return fallback;
  }
  return Math.min(hi, Math.max(lo, n));
}

// Same limits as `Framing::clamped`.
function clampFraming(f) {
  return {
    zoom: clampOr(f.zoom, ZOOM_MIN, ZOOM_MAX, 1),
    pan_x: clampOr(f.pan_x, -1, 1, 0),
    pan_y: clampOr(f.pan_y, -1, 1, 0),
    mirror: !!f.mirror,
    auto_frame: !!f.auto_frame,
  };
}

// The part of the input image to show: output aspect ratio, as large as fits, divided by the
// zoom, slid inside the leftover margin by the pan values.
function cropRect(inW, inH, outW, outH, framing) {
  if (!(inW > 0) || !(inH > 0)) {
    return { x: 0, y: 0, width: inW || 0, height: inH || 0 };
  }
  const f = clampFraming(framing);
  const aspect = outW > 0 && outH > 0 ? outW / outH : inW / inH;
  let maxW;
  let maxH;
  if (inW / inH > aspect) {
    maxW = inH * aspect;
    maxH = inH;
  } else {
    maxW = inW;
    maxH = inW / aspect;
  }
  const width = Math.min(maxW / f.zoom, inW);
  const height = Math.min(maxH / f.zoom, inH);
  const marginX = Math.max(inW - width, 0);
  const marginY = Math.max(inH - height, 0);
  return {
    x: Math.min(Math.max(marginX * (f.pan_x + 1) * 0.5, 0), marginX),
    y: Math.min(Math.max(marginY * (f.pan_y + 1) * 0.5, 0), marginY),
    width,
    height,
  };
}

// ---------------------------------------------------------------------------
// Settings state.
//
// `desired` is what the user has asked for. It changes immediately on every input, and
// every change sends the whole of it to the engine. A reply from the engine (which may have
// clamped values) is adopted only when it answers the newest request; an older reply is stale
// because the user has already moved on, so it must not overwrite their newer choices.
// ---------------------------------------------------------------------------

const DEFAULT_SETTINGS = {
  framing: { zoom: 1, pan_x: 0, pan_y: 0, mirror: false, auto_frame: false },
  background: { mode: "none" },
  noise: "off",
  output_width: 1280,
  output_height: 720,
  output_fps: 30,
};

let desired = structuredClone(DEFAULT_SETTINGS);
// Values the background controls keep while another mode is selected.
const remembered = { radius: DEFAULT_BLUR, imagePath: "" };
let localChanges = 0;
let requestSeq = 0;
let dirty = true;

function backgroundFor(mode) {
  switch (mode) {
    case "blur":
      return { mode: "blur", radius: remembered.radius };
    case "replace":
      return { mode: "replace", image_path: remembered.imagePath };
    default:
      return { mode: "none" };
  }
}

function adopt(settings) {
  desired = structuredClone(settings);
  if (desired.background.mode === "blur") {
    remembered.radius = desired.background.radius;
  } else if (desired.background.mode === "replace") {
    remembered.imagePath = desired.background.image_path;
  }
}

function showError(text) {
  els.error.textContent = text;
  els.error.hidden = !text;
}

// Apply a change to the desired state, show it now, and send the whole state to the engine.
function change(mutate) {
  mutate(desired);
  localChanges += 1;
  render();
  send();
}

function send() {
  const id = ++requestSeq;
  const settings = structuredClone(desired);
  invoke("set_settings", { settings }).then(
    (reply) => {
      if (id !== requestSeq) {
        return;
      }
      showError("");
      adopt(reply);
      render();
    },
    (err) => {
      if (id === requestSeq) {
        // The engine words its own errors (for example a background picture that would not
        // open). Anything else gets a general lead-in.
        const text = String(err);
        showError(text.startsWith("Could not") ? text : `Could not save the settings: ${text}`);
      }
    },
  );
}

// ---------------------------------------------------------------------------
// Number formatting
// ---------------------------------------------------------------------------

function zoomText(zoom) {
  const text = Number(zoom).toFixed(2);
  return `${text.endsWith("0") ? text.slice(0, -1) : text}×`;
}

function panText(value) {
  const n = Number(value);
  if (Math.abs(n) < 0.005) {
    return "0.00";
  }
  return `${n < 0 ? "−" : "+"}${Math.abs(n).toFixed(2)}`;
}

function round3(n) {
  return Math.round(n * 1000) / 1000;
}

// ---------------------------------------------------------------------------
// Faders: ticks, fill, double-click reset
// ---------------------------------------------------------------------------

function buildTicks(fader) {
  const input = fader.querySelector("input[type=range]");
  const host = fader.querySelector(".ticks");
  const min = Number(input.min);
  const max = Number(input.max);
  const pos = (v) => (v - min) / (max - min);

  const minorStep = Number(fader.dataset.minor);
  const majors = fader.dataset.ticks.split(",").map((t) => {
    const cut = t.indexOf(":");
    return { value: Number(t.slice(0, cut)), label: t.slice(cut + 1) };
  });
  const isMajor = (v) => majors.some((m) => Math.abs(m.value - v) < 1e-9);

  const add = (value, label, classes) => {
    const tick = document.createElement("span");
    tick.className = `tick ${classes}`;
    tick.style.setProperty("--p", String(pos(value)));
    if (label) {
      const text = document.createElement("span");
      text.className = "tick-label";
      text.textContent = label;
      tick.append(text);
    }
    host.append(tick);
  };

  if (minorStep > 0) {
    for (let v = min; v <= max + 1e-9; v += minorStep) {
      if (!isMajor(v)) {
        add(v, "", "minor");
      }
    }
  }
  majors.forEach((m, i) => {
    const classes = ["major"];
    if (i === 0) classes.push("first");
    if (i === majors.length - 1) classes.push("last");
    if (min < 0 && m.value === 0) classes.push("centre");
    add(m.value, m.label, classes.join(" "));
  });
}

// Fill from the start, or from the centre for pan faders.
function paintFader(input, value) {
  const min = Number(input.min);
  const max = Number(input.max);
  const p = (clampOr(value, min, max, min) - min) / (max - min);
  const centre = input.dataset.origin === "center" ? 0.5 : 0;
  input.style.setProperty("--fill-a", `${Math.min(p, centre) * 100}%`);
  input.style.setProperty("--fill-b", `${Math.max(p, centre) * 100}%`);
}

function setFader(input, value) {
  if (document.activeElement !== input || Number(input.value) !== value) {
    input.value = String(value);
  }
  paintFader(input, value);
}

function bindFader(input, apply) {
  const fader = input.closest(".fader");
  input.addEventListener("input", () => apply(Number(input.value)));
  fader.addEventListener("dblclick", () => apply(Number(fader.dataset.default)));
}

// ---------------------------------------------------------------------------
// Rendering the controls from `desired`
// ---------------------------------------------------------------------------

function setRadio(group, value) {
  const inputs = [...group.querySelectorAll("input[type=radio]")];
  inputs.forEach((input, i) => {
    input.checked = input.value === value;
    if (input.checked) {
      group.style.setProperty("--i", String(i));
      group.dataset.idle = String(value === "none" || value === "off");
    }
  });
}

function setReveal(panel, open) {
  panel.classList.toggle("open", open);
  panel.inert = !open;
  panel.setAttribute("aria-hidden", open ? "false" : "true");
}

function cameraSize() {
  const w = els.video.videoWidth;
  const h = els.video.videoHeight;
  if (w > 0 && h > 0) {
    return { w, h, live: true };
  }
  return { w: 16, h: 9, live: false };
}

function render() {
  const { framing, background, noise } = desired;

  setFader(els.zoom, framing.zoom);
  els.zoomOut.textContent = zoomText(framing.zoom);
  els.zoomReadout.textContent = zoomText(framing.zoom);
  setFader(els.panX, framing.pan_x);
  els.panXOut.textContent = panText(framing.pan_x);
  setFader(els.panY, framing.pan_y);
  els.panYOut.textContent = panText(framing.pan_y);
  els.mirror.checked = framing.mirror;
  els.autoFrame.checked = framing.auto_frame;

  setRadio(els.bgGroup, background.mode);
  setReveal(els.blurField, background.mode === "blur");
  setReveal(els.imageField, background.mode === "replace");
  setFader(els.blurRadius, remembered.radius);
  els.blurOut.textContent = `${remembered.radius} px`;
  if (document.activeElement !== els.imagePath) {
    els.imagePath.value = remembered.imagePath;
  }

  setRadio(els.noiseGroup, noise);

  els.outputSpec.textContent = `${desired.output_width} × ${desired.output_height} · ${desired.output_fps} fps`;
  layoutViewfinder();
  updateMap();
  dirty = true;
}

// ---------------------------------------------------------------------------
// Viewfinder canvas
// ---------------------------------------------------------------------------

const previewCtx = els.preview.getContext("2d");
const mapCtx = els.mapCanvas.getContext("2d");

function fit(boxW, boxH, aspect) {
  let w = boxW;
  let h = w / aspect;
  if (h > boxH) {
    h = boxH;
    w = h * aspect;
  }
  return { w: Math.max(1, Math.floor(w)), h: Math.max(1, Math.floor(h)) };
}

function outputAspect() {
  const w = desired.output_width;
  const h = desired.output_height;
  return w > 0 && h > 0 ? w / h : 16 / 9;
}

function layoutViewfinder() {
  const area = els.stageArea;
  const cs = getComputedStyle(area);
  const boxW = area.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const boxH = area.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  if (boxW <= 0 || boxH <= 0) {
    return;
  }
  const { w, h } = fit(boxW, boxH, outputAspect());
  const dpr = window.devicePixelRatio || 1;
  els.viewfinder.style.width = `${w}px`;
  els.viewfinder.style.height = `${h}px`;
  const cw = Math.round(w * dpr);
  const ch = Math.round(h * dpr);
  if (els.preview.width !== cw || els.preview.height !== ch) {
    els.preview.width = cw;
    els.preview.height = ch;
    dirty = true;
  }
  layoutMap();
}

function layoutMap() {
  const cam = cameraSize();
  const maxW = Math.max(120, els.mapWrap.clientWidth - 18);
  const { w, h } = fit(maxW, 120, cam.w / cam.h);
  els.map.style.width = `${w}px`;
  els.map.style.height = `${h}px`;
  const dpr = window.devicePixelRatio || 1;
  const cw = Math.round(w * dpr);
  const ch = Math.round(h * dpr);
  if (els.mapCanvas.width !== cw || els.mapCanvas.height !== ch) {
    els.mapCanvas.width = cw;
    els.mapCanvas.height = ch;
    dirty = true;
  }
}

// The crop rectangle in camera pixels, plus the camera size it refers to.
function currentCrop() {
  const cam = cameraSize();
  const rect = cropRect(cam.w, cam.h, desired.output_width, desired.output_height, desired.framing);
  return { cam, rect };
}

// Position the draggable rectangle. With mirror on, the map is mirrored like the preview,
// so the rectangle sits where the picture appears on screen.
function updateMap() {
  const { cam, rect } = currentCrop();
  const fx = rect.x / cam.w;
  const fy = rect.y / cam.h;
  const fw = rect.width / cam.w;
  const fh = rect.height / cam.h;
  const left = desired.framing.mirror ? 1 - (fx + fw) : fx;
  els.mapRect.style.left = `${left * 100}%`;
  els.mapRect.style.top = `${fy * 100}%`;
  els.mapRect.style.width = `${fw * 100}%`;
  els.mapRect.style.height = `${fh * 100}%`;

  const data = els.map.dataset;
  data.x = rect.x.toFixed(2);
  data.y = rect.y.toFixed(2);
  data.w = rect.width.toFixed(2);
  data.h = rect.height.toFixed(2);
  data.cameraW = String(cam.live ? cam.w : 0);
  data.cameraH = String(cam.live ? cam.h : 0);
  data.mirror = String(desired.framing.mirror);

  if (cam.live) {
    els.mapReadout.textContent =
      `${Math.round(rect.width)} × ${Math.round(rect.height)} from ${cam.w} × ${cam.h}`;
  } else {
    els.mapReadout.textContent = "Waiting for the camera";
  }
}

let lastVideoTime = -1;
let lastDrawnRect = "";

function drawFrame() {
  const video = els.video;
  const W = els.preview.width;
  const H = els.preview.height;
  const MW = els.mapCanvas.width;
  const MH = els.mapCanvas.height;
  previewCtx.setTransform(1, 0, 0, 1, 0, 0);
  previewCtx.clearRect(0, 0, W, H);
  mapCtx.setTransform(1, 0, 0, 1, 0, 0);
  mapCtx.clearRect(0, 0, MW, MH);
  if (!(video.videoWidth > 0 && video.videoHeight > 0) || video.readyState < 2) {
    return;
  }
  const { rect } = currentCrop();
  const mirror = desired.framing.mirror;

  previewCtx.imageSmoothingQuality = "high";
  if (mirror) {
    previewCtx.setTransform(-1, 0, 0, 1, W, 0);
  }
  previewCtx.drawImage(video, rect.x, rect.y, rect.width, rect.height, 0, 0, W, H);
  previewCtx.setTransform(1, 0, 0, 1, 0, 0);

  // The map shows the whole camera frame.
  if (mirror) {
    mapCtx.setTransform(-1, 0, 0, 1, MW, 0);
  }
  mapCtx.drawImage(video, 0, 0, video.videoWidth, video.videoHeight, 0, 0, MW, MH);
  mapCtx.setTransform(1, 0, 0, 1, 0, 0);

  // Expose the source rectangle drawn, so it can be checked against the engine maths.
  const d = els.preview.dataset;
  const text = [rect.x, rect.y, rect.width, rect.height, mirror].join();
  if (text !== lastDrawnRect) {
    lastDrawnRect = text;
    d.srcX = rect.x.toFixed(3);
    d.srcY = rect.y.toFixed(3);
    d.srcW = rect.width.toFixed(3);
    d.srcH = rect.height.toFixed(3);
    d.mirror = String(mirror);
  }
  d.videoW = String(video.videoWidth);
  d.videoH = String(video.videoHeight);
  d.outW = String(desired.output_width);
  d.outH = String(desired.output_height);
  d.frames = String((Number(d.frames) || 0) + 1);
}

function loop() {
  requestAnimationFrame(loop);
  // Nothing to paint while the window is hidden.
  if (document.hidden) {
    return;
  }
  const t = els.video.currentTime;
  if (dirty || t !== lastVideoTime) {
    lastVideoTime = t;
    dirty = false;
    drawFrame();
  }
}

// ---------------------------------------------------------------------------
// Pan and zoom by hand: framing map and wheel
// ---------------------------------------------------------------------------

function zoomBy(amount) {
  change((s) => {
    s.framing.zoom = round3(clampOr(s.framing.zoom + amount, ZOOM_MIN, ZOOM_MAX, 1));
  });
}

function onWheel(event) {
  event.preventDefault();
  // One notch of a mouse wheel is about 100; trackpads send small steady deltas.
  const delta = Math.max(-200, Math.min(200, event.deltaMode === 1 ? event.deltaY * 33 : event.deltaY));
  zoomBy(-delta * 0.001);
}

// Pointer position inside the map as a fraction of the camera frame (mirror undone).
function mapPoint(event) {
  const box = els.map.getBoundingClientRect();
  let fx = (event.clientX - box.left) / box.width;
  const fy = (event.clientY - box.top) / box.height;
  if (desired.framing.mirror) {
    fx = 1 - fx;
  }
  return { fx, fy };
}

const drag = { active: false, dx: 0, dy: 0 };

function panFromMap(point) {
  const { cam, rect } = currentCrop();
  const fw = rect.width / cam.w;
  const fh = rect.height / cam.h;
  const marginX = 1 - fw;
  const marginY = 1 - fh;
  const left = Math.min(Math.max(point.fx - drag.dx, 0), Math.max(marginX, 0));
  const top = Math.min(Math.max(point.fy - drag.dy, 0), Math.max(marginY, 0));
  change((s) => {
    // On an axis with no spare room the pan has no effect, so leave it alone.
    if (marginX > 1e-6) {
      s.framing.pan_x = round3(clampOr((left / marginX) * 2 - 1, -1, 1, 0));
    }
    if (marginY > 1e-6) {
      s.framing.pan_y = round3(clampOr((top / marginY) * 2 - 1, -1, 1, 0));
    }
  });
}

function bindMap() {
  els.map.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) {
      return;
    }
    event.preventDefault();
    els.map.focus();
    els.map.setPointerCapture(event.pointerId);
    const { cam, rect } = currentCrop();
    const r = { x: rect.x / cam.w, y: rect.y / cam.h, w: rect.width / cam.w, h: rect.height / cam.h };
    const p = mapPoint(event);
    const inside = p.fx >= r.x && p.fx <= r.x + r.w && p.fy >= r.y && p.fy <= r.y + r.h;
    // Grab the rectangle where it was hit; a click outside moves its centre to the pointer.
    drag.dx = inside ? p.fx - r.x : r.w / 2;
    drag.dy = inside ? p.fy - r.y : r.h / 2;
    drag.active = true;
    els.map.classList.add("dragging");
    panFromMap(p);
  });
  els.map.addEventListener("pointermove", (event) => {
    if (drag.active) {
      panFromMap(mapPoint(event));
    }
  });
  const end = () => {
    drag.active = false;
    els.map.classList.remove("dragging");
  };
  els.map.addEventListener("pointerup", end);
  els.map.addEventListener("pointercancel", end);
  els.map.addEventListener("lostpointercapture", end);

  els.map.addEventListener("wheel", onWheel, { passive: false });
  els.preview.addEventListener("wheel", onWheel, { passive: false });

  els.map.addEventListener("keydown", (event) => {
    const step = event.shiftKey ? 0.25 : 0.05;
    const nudge = (key, field, amount) => {
      event.preventDefault();
      change((s) => {
        s.framing[field] = round3(clampOr(s.framing[field] + amount, -1, 1, 0));
      });
    };
    switch (event.key) {
      case "ArrowLeft":
        // With mirror on, the picture moves the other way on screen.
        return nudge(event.key, "pan_x", desired.framing.mirror ? step : -step);
      case "ArrowRight":
        return nudge(event.key, "pan_x", desired.framing.mirror ? -step : step);
      case "ArrowUp":
        return nudge(event.key, "pan_y", -step);
      case "ArrowDown":
        return nudge(event.key, "pan_y", step);
      case "+":
      case "=":
        event.preventDefault();
        return zoomBy(0.1);
      case "-":
      case "_":
        event.preventDefault();
        return zoomBy(-0.1);
      default:
    }
  });
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

function bindControls() {
  document.querySelectorAll(".fader").forEach(buildTicks);

  bindFader(els.zoom, (v) =>
    change((s) => (s.framing.zoom = round3(clampOr(v, ZOOM_MIN, ZOOM_MAX, DEFAULT_ZOOM)))),
  );
  bindFader(els.panX, (v) => change((s) => (s.framing.pan_x = round3(clampOr(v, -1, 1, 0)))));
  bindFader(els.panY, (v) => change((s) => (s.framing.pan_y = round3(clampOr(v, -1, 1, 0)))));
  els.mirror.addEventListener("change", () => change((s) => (s.framing.mirror = els.mirror.checked)));

  els.bgGroup.addEventListener("change", (event) => {
    const mode = event.target.value;
    change((s) => (s.background = backgroundFor(mode)));
  });
  bindFader(els.blurRadius, (v) => {
    remembered.radius = Math.round(clampOr(v, 1, 50, DEFAULT_BLUR));
    change((s) => {
      if (s.background.mode === "blur") {
        s.background = backgroundFor("blur");
      }
    });
  });
  els.imagePath.addEventListener("input", () => {
    remembered.imagePath = els.imagePath.value;
  });
  els.imagePath.addEventListener("change", () => {
    remembered.imagePath = els.imagePath.value;
    change((s) => {
      if (s.background.mode === "replace") {
        s.background = backgroundFor("replace");
      }
    });
  });

  els.noiseGroup.addEventListener("change", (event) => {
    const value = event.target.value;
    change((s) => (s.noise = value));
  });

  bindMap();
}

// Thirds guides: a per-person convenience, so it is only remembered when storage works.
function bindGuides() {
  const set = (on) => {
    els.thirds.classList.toggle("on", on);
    els.thirdsToggle.setAttribute("aria-pressed", String(on));
    try {
      localStorage.setItem("thirds", on ? "1" : "0");
    } catch (err) {
      // Storage may be blocked. The toggle still works for this session.
    }
  };
  let on = false;
  try {
    on = localStorage.getItem("thirds") === "1";
  } catch (err) {
    on = false;
  }
  els.thirds.classList.toggle("on", on);
  els.thirdsToggle.setAttribute("aria-pressed", String(on));
  els.thirdsToggle.addEventListener("click", () => set(els.thirdsToggle.getAttribute("aria-pressed") !== "true"));
}

// ---------------------------------------------------------------------------
// Camera preview
// ---------------------------------------------------------------------------

function previewProblem(err) {
  switch (err && err.name) {
    case "NotAllowedError":
    case "SecurityError":
      return "Camera access was denied. Allow camera access for Webcam Studio in your system settings, then restart the app.";
    case "NotFoundError":
    case "OverconstrainedError":
      return "No camera was found. Connect a camera and pick it from the list.";
    case "NotReadableError":
      return "The camera could not be opened. Another app may be using it.";
    default:
      return "The preview could not start.";
  }
}

function setMessage(text) {
  els.message.textContent = text;
  els.message.hidden = !text;
}

function setLive(live) {
  els.tally.dataset.live = String(live);
  els.tallyText.textContent = live ? "Live" : "Camera off";
}

function stopPreview() {
  const stream = els.video.srcObject;
  if (stream) {
    stream.getTracks().forEach((track) => track.stop());
    els.video.srcObject = null;
  }
  setLive(false);
  dirty = true;
}

async function startPreview(deviceId) {
  stopPreview();
  setMessage("");
  if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
    setMessage("This system's web view cannot show a camera preview.");
    return;
  }
  try {
    const video = deviceId ? { deviceId: { exact: deviceId } } : true;
    const stream = await navigator.mediaDevices.getUserMedia({ video, audio: false });
    stream.getVideoTracks().forEach((track) => track.addEventListener("ended", () => setLive(false)));
    els.video.srcObject = stream;
    await refreshCameraList();
  } catch (err) {
    setMessage(previewProblem(err));
  }
}

// Labels are only available once the user has allowed camera access.
async function refreshCameraList() {
  const devices = await navigator.mediaDevices.enumerateDevices();
  const cameras = devices.filter((d) => d.kind === "videoinput");
  const active = els.video.srcObject?.getVideoTracks()[0]?.getSettings().deviceId;
  vcam.previewDevices = cameras;
  if (vcam.cameras.length > 0) {
    // The picker lists the native cameras. Make the preview show the one chosen there.
    syncPreviewToSelection(active);
    return;
  }
  els.camera.replaceChildren(
    ...cameras.map((cam, i) => {
      const option = document.createElement("option");
      option.value = cam.deviceId;
      option.textContent = cam.label || `Camera ${i + 1}`;
      return option;
    }),
  );
  if (active) {
    els.camera.value = active;
  }
}

function bindVideo() {
  const refresh = () => {
    layoutMap();
    updateMap();
    dirty = true;
  };
  els.video.addEventListener("loadedmetadata", refresh);
  els.video.addEventListener("resize", refresh);
  els.video.addEventListener("playing", () => {
    setLive(true);
    refresh();
  });
  els.video.addEventListener("emptied", () => setLive(false));
  document.addEventListener("visibilitychange", () => {
    dirty = true;
  });
}

// ---------------------------------------------------------------------------
// Microphone test: starts only when the person asks, and listens only to show a level.
// ---------------------------------------------------------------------------

const METER_SEGMENTS = 24;
const METER_FLOOR_DB = -60;
const CLIP_SEGMENTS = 2;
const PEAK_HOLD_MS = 1200;
const PEAK_FALL_MS = 70;

const mic = { stream: null, audio: null, analyser: null, buffer: null, raf: 0, level: 0, peak: 0, peakAt: 0, fallAt: 0 };
const meterCells = [];

function buildMeter() {
  for (let i = 0; i < METER_SEGMENTS; i += 1) {
    const cell = document.createElement("i");
    if (i >= METER_SEGMENTS - CLIP_SEGMENTS) {
      cell.classList.add("clip");
    }
    els.micMeter.append(cell);
    meterCells.push(cell);
  }
}

function paintMeter(lit, peak) {
  meterCells.forEach((cell, i) => {
    cell.classList.toggle("lit", i < lit);
    cell.classList.toggle("peak", peak > 0 && i === peak - 1 && peak > lit);
  });
  els.micMeter.dataset.lit = String(lit);
  els.micMeter.dataset.peak = String(peak);
}

function micProblem(err) {
  switch (err && err.name) {
    case "NotAllowedError":
    case "SecurityError":
      return "Microphone access was denied. Allow it in your system settings, then try again.";
    case "NotFoundError":
    case "OverconstrainedError":
      return "No microphone was found.";
    case "NotReadableError":
      return "The microphone could not be opened. Another app may be using it.";
    default:
      return "The microphone test could not start.";
  }
}

function micTick(now) {
  mic.raf = requestAnimationFrame(micTick);
  mic.analyser.getFloatTimeDomainData(mic.buffer);
  let sum = 0;
  for (let i = 0; i < mic.buffer.length; i += 1) {
    sum += mic.buffer[i] * mic.buffer[i];
  }
  const rms = Math.sqrt(sum / mic.buffer.length);
  const db = rms > 0 ? 20 * Math.log10(rms) : -Infinity;
  const target = Math.max(0, Math.min(1, (db - METER_FLOOR_DB) / -METER_FLOOR_DB));
  // Rises at once, falls steadily.
  mic.level = target > mic.level ? target : Math.max(target, mic.level - 0.02);
  const lit = Math.round(mic.level * METER_SEGMENTS);

  if (lit >= mic.peak) {
    mic.peak = lit;
    mic.peakAt = now;
    mic.fallAt = now;
  } else if (now - mic.peakAt > PEAK_HOLD_MS && now - mic.fallAt > PEAK_FALL_MS) {
    mic.peak = Math.max(lit, mic.peak - 1);
    mic.fallAt = now;
  }
  paintMeter(lit, mic.peak);

  // The readout follows the same smoothed level as the bar, so the two never disagree.
  const shown = METER_FLOOR_DB + mic.level * -METER_FLOOR_DB;
  els.micDb.textContent = mic.level <= 0 ? "\u2212\u221e dB" : `${shown.toFixed(0).replace("-", "\u2212")} dB`;
  els.micMeter.setAttribute("aria-valuenow", shown.toFixed(0));
}

async function startMic() {
  if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
    els.micStatus.textContent = "This system's web view cannot open a microphone.";
    return;
  }
  els.micToggle.disabled = true;
  els.micStatus.textContent = "Waiting for permission…";
  try {
    mic.stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const AudioCtx = window.AudioContext || window.webkitAudioContext;
    mic.audio = new AudioCtx();
    if (mic.audio.state === "suspended") {
      await mic.audio.resume();
    }
    mic.analyser = mic.audio.createAnalyser();
    mic.analyser.fftSize = 1024;
    mic.analyser.smoothingTimeConstant = 0;
    mic.buffer = new Float32Array(mic.analyser.fftSize);
    // The analyser is not connected to the speakers, so there is no echo.
    mic.audio.createMediaStreamSource(mic.stream).connect(mic.analyser);
    mic.level = 0;
    mic.peak = 0;
    els.micToggle.textContent = "Stop test";
    els.micToggle.setAttribute("aria-pressed", "true");
    els.micStatus.textContent = "Listening. Say something.";
    mic.raf = requestAnimationFrame(micTick);
  } catch (err) {
    stopMic();
    els.micStatus.textContent = micProblem(err);
  } finally {
    els.micToggle.disabled = false;
  }
}

function stopMic(note) {
  cancelAnimationFrame(mic.raf);
  mic.raf = 0;
  if (mic.stream) {
    mic.stream.getTracks().forEach((track) => track.stop());
  }
  if (mic.audio) {
    mic.audio.close().catch(() => {});
  }
  mic.stream = null;
  mic.audio = null;
  mic.analyser = null;
  mic.level = 0;
  mic.peak = 0;
  paintMeter(0, 0);
  els.micDb.textContent = "Off";
  els.micMeter.setAttribute("aria-valuenow", String(METER_FLOOR_DB));
  els.micToggle.textContent = "Test microphone";
  els.micToggle.setAttribute("aria-pressed", "false");
  els.micStatus.textContent = note || "Level check only. Nothing is recorded.";
}

function bindMic() {
  buildMeter();
  els.micToggle.addEventListener("click", () => (mic.stream ? stopMic() : startMic()));
  window.addEventListener("pagehide", () => {
    if (mic.stream) {
      stopMic();
    }
  });
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

const OS_NAMES = { linux: "Linux", macos: "macOS", windows: "Windows" };

async function showPlatform() {
  try {
    const info = await invoke("platform_info");
    els.platformOs.textContent = OS_NAMES[info.os] || String(info.os);
    els.platformBackend.textContent = String(info.camera_backend);
  } catch (err) {
    els.platformOs.textContent = "Not available";
    els.platformBackend.textContent = "Not available";
  }
}

// ---------------------------------------------------------------------------
// Virtual camera: native camera choice, start and stop, setup steps, live status
// ---------------------------------------------------------------------------

const POLL_MS = 1000;
const PILL_TEXT = {
  running: "Virtual camera: on",
  ready: "Virtual camera: ready",
  needs_approval: "Virtual camera: needs approval",
  not_installed: "Virtual camera: not installed",
  unsupported: "Virtual camera: not available on this system",
  error: "Virtual camera: error",
};
const NOTE_IDLE = "Preview shows framing only. Other effects are applied in the camera output.";
const NOTE_RUNNING = "Preview shows framing only. The virtual camera carries all effects.";

const vcam = {
  status: null,
  cameras: [], // native cameras from `list_native_cameras`
  previewDevices: [], // getUserMedia devices, used for the preview only
  selectedId: "",
  previewSynced: false,
  busy: "", // "starting" or "stopping" while a command is in flight
  statusSeq: 0,
  timer: 0,
};

function tauriEvents() {
  return window.__TAURI__ && window.__TAURI__.event;
}

function rememberCamera(id) {
  try {
    localStorage.setItem("native-camera", id);
  } catch (err) {
    // Storage may be blocked. The choice still holds for this session.
  }
}

function rememberedCamera() {
  try {
    return localStorage.getItem("native-camera") || "";
  } catch (err) {
    return "";
  }
}

function selectedCamera() {
  return vcam.cameras.find((cam) => cam.id === vcam.selectedId) || null;
}

async function loadNativeCameras() {
  try {
    const list = await invoke("list_native_cameras");
    vcam.cameras = Array.isArray(list) ? list : [];
  } catch (err) {
    // Not available on this system: the picker keeps listing the preview cameras.
    vcam.cameras = [];
  }
  if (vcam.cameras.length > 0) {
    const wanted = rememberedCamera();
    vcam.selectedId = vcam.cameras.some((cam) => cam.id === wanted) ? wanted : vcam.cameras[0].id;
    els.camera.replaceChildren(
      ...vcam.cameras.map((cam) => {
        const option = document.createElement("option");
        option.value = cam.id;
        option.textContent = cam.name;
        return option;
      }),
    );
    els.camera.value = vcam.selectedId;
  }
  renderVcam();
}

// The preview comes from the web view, which names cameras differently from the native
// side. The two are matched by name.
function previewDeviceFor(cam) {
  if (!cam) {
    return undefined;
  }
  const match = vcam.previewDevices.find((d) => d.label === cam.name);
  return match ? match.deviceId : undefined;
}

function previewDeviceForSelection() {
  return previewDeviceFor(selectedCamera());
}

// Once the preview camera names are known, switch the preview to the chosen camera (once).
function syncPreviewToSelection(activeDeviceId) {
  if (vcam.previewSynced) {
    return;
  }
  vcam.previewSynced = true;
  const wanted = previewDeviceForSelection();
  if (wanted && wanted !== activeDeviceId) {
    startPreview(wanted);
  }
}

function onCameraPicked() {
  if (vcam.cameras.length === 0) {
    startPreview(els.camera.value);
    return;
  }
  vcam.selectedId = els.camera.value;
  rememberCamera(vcam.selectedId);
  renderVcam();
  startPreview(previewDeviceForSelection());
}

function setPill(state, text, title) {
  els.vcamPill.dataset.state = state;
  els.vcamPillText.textContent = text;
  els.vcamPill.title = title || "";
}

function statText(status) {
  const fps = Math.round(Number(status.fps) || 0);
  return `${fps} fps · ${status.frames_dropped} dropped`;
}

function renderVcam() {
  const status = vcam.status;
  const state = status ? status.state : "unknown";
  const running = state === "running";
  els.vcamGroup.dataset.state = state;

  // Button
  let label = running ? "Stop virtual camera" : "Start virtual camera";
  if (vcam.busy === "starting") {
    label = "Starting…";
  } else if (vcam.busy === "stopping") {
    label = "Stopping…";
  }
  els.vcamToggle.textContent = label;
  els.vcamToggle.setAttribute("aria-pressed", String(running));
  const canStart = (state === "ready" || state === "error") && !!selectedCamera();
  els.vcamToggle.disabled = !!vcam.busy || !(running || canStart);
  els.vcamToggle.title = !running && state === "ready" && !selectedCamera() ? "No camera was found." : "";

  // The camera in use
  const cam = selectedCamera();
  const shownName = running && status.camera_name ? status.camera_name : cam ? cam.name : "";
  els.vcamCamera.hidden = !shownName && state !== "ready";
  els.vcamCameraName.textContent = shownName || "No camera found";

  // The picker is locked while the camera runs, because the choice only applies at start.
  els.camera.disabled = running || !!vcam.busy;
  els.camera.title = running ? "Stop the virtual camera to change the camera." : "";
  if (running && status.camera_name) {
    const live = vcam.cameras.find((c) => c.name === status.camera_name);
    if (live && live.id !== vcam.selectedId) {
      vcam.selectedId = live.id;
      els.camera.value = live.id;
    }
  }

  // One line: the live numbers while running, otherwise what is going on.
  els.vcamLine.textContent = !status ? "" : running ? statText(status) : status.message;
  els.vcamLine.classList.toggle("is-stat", running);
  els.vcamLine.classList.toggle("mono", running);
  els.vcamLine.title = running && status.message ? status.message : "";

  // Setup steps
  const setup = state === "not_installed" || state === "needs_approval";
  els.vcamSetup.hidden = !setup;
  els.stepInstall.classList.toggle("is-done", state === "needs_approval");
  els.stepInstall.classList.toggle("is-active", state === "not_installed");
  els.stepAllow.classList.toggle("is-active", state === "needs_approval");

  // Status bar and preview label
  if (status) {
    setPill(state, PILL_TEXT[state] || "Virtual camera: error", status.message);
  }
  els.previewNote.textContent = running ? NOTE_RUNNING : NOTE_IDLE;
}

async function refreshStatus() {
  const id = ++vcam.statusSeq;
  try {
    const status = await invoke("virtual_camera_status");
    if (id !== vcam.statusSeq) {
      return;
    }
    vcam.status = status;
  } catch (err) {
    if (id !== vcam.statusSeq) {
      return;
    }
    vcam.status = { state: "error", message: `Could not read the virtual camera status: ${err}`, fps: 0, frames_sent: 0, frames_dropped: 0, camera_name: null };
  }
  renderVcam();
}

// Poll only while the window can be seen.
function startPolling() {
  if (!vcam.timer) {
    vcam.timer = setInterval(refreshStatus, POLL_MS);
  }
}

function stopPolling() {
  clearInterval(vcam.timer);
  vcam.timer = 0;
}

async function toggleVirtualCamera() {
  if (vcam.busy) {
    return;
  }
  const running = vcam.status && vcam.status.state === "running";
  vcam.busy = running ? "stopping" : "starting";
  renderVcam();
  try {
    if (running) {
      await invoke("stop_virtual_camera");
    } else {
      await invoke("start_virtual_camera", { cameraId: vcam.selectedId });
    }
    showError("");
  } catch (err) {
    showError(String(err));
  }
  vcam.busy = "";
  await refreshStatus();
}

async function runSetupStep(command) {
  try {
    await invoke(command);
  } catch (err) {
    showError(String(err));
  }
  refreshStatus();
}

function bindVirtualCamera() {
  els.vcamToggle.addEventListener("click", toggleVirtualCamera);
  els.vcamInstall.addEventListener("click", () => runSetupStep("install_camera_extension"));
  els.vcamAllow.addEventListener("click", () => runSetupStep("open_extension_settings"));

  const events = tauriEvents();
  if (events && events.listen) {
    // macOS answers install requests later, and may answer twice (approval, then done).
    Promise.resolve(events.listen("camera-extension", () => refreshStatus())).catch(() => {});
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      stopPolling();
    } else {
      refreshStatus();
      startPolling();
    }
  });
  refreshStatus();
  if (!document.hidden) {
    startPolling();
  }
}

// ---------------------------------------------------------------------------

async function init() {
  bindControls();
  bindGuides();
  bindVideo();
  bindMic();
  els.camera.addEventListener("change", onCameraPicked);
  new ResizeObserver(layoutViewfinder).observe(els.stageArea);
  render();
  requestAnimationFrame(loop);
  showPlatform();
  bindVirtualCamera();
  const before = localChanges;
  try {
    const settings = await invoke("get_settings");
    // If the person already changed something while this was loading, keep their choice.
    if (localChanges === before) {
      adopt(settings);
      render();
    }
  } catch (err) {
    showError(`Could not read the settings: ${err}`);
  }
  await loadNativeCameras();
  startPreview(previewDeviceForSelection());
}

init();
