"use strict";

const invoke = window.__TAURI__.core.invoke;

const $ = (id) => document.getElementById(id);
const els = {
  video: $("video"),
  message: $("preview-message"),
  camera: $("camera"),
  zoom: $("zoom"),
  zoomOut: $("zoom-out"),
  panX: $("pan-x"),
  panXOut: $("pan-x-out"),
  panY: $("pan-y"),
  panYOut: $("pan-y-out"),
  mirror: $("mirror"),
  autoFrame: $("auto-frame"),
  bgMode: $("background-mode"),
  blurField: $("blur-field"),
  blurRadius: $("blur-radius"),
  blurOut: $("blur-out"),
  imageField: $("image-field"),
  imagePath: $("image-path"),
  noise: $("noise"),
  error: $("error"),
  platform: $("platform"),
};

// Latest settings returned by the engine.
let current = null;
// Values the background controls keep while another mode is selected.
const remembered = { radius: 8, imagePath: "" };
// Used to ignore answers that arrive after a newer request was sent.
let requestId = 0;

function fixed(value, digits) {
  return Number(value).toFixed(digits);
}

// Show the framing now, in the preview. The mirror is applied last, to the whole
// picture, so panning keeps the same meaning on screen when mirrored.
function applyPreviewTransform(framing) {
  const shift = ((framing.zoom - 1) / 2) * 100;
  const tx = -framing.pan_x * shift;
  const ty = -framing.pan_y * shift;
  const flip = framing.mirror ? "scaleX(-1) " : "";
  els.video.style.transform = `${flip}translate(${tx}%, ${ty}%) scale(${framing.zoom})`;
}

function render(settings) {
  current = settings;
  const { framing, background, noise } = settings;

  els.zoom.value = framing.zoom;
  els.zoomOut.textContent = `${fixed(framing.zoom, 2)}×`;
  els.panX.value = framing.pan_x;
  els.panXOut.textContent = fixed(framing.pan_x, 2);
  els.panY.value = framing.pan_y;
  els.panYOut.textContent = fixed(framing.pan_y, 2);
  els.mirror.checked = framing.mirror;
  els.autoFrame.checked = framing.auto_frame;

  els.bgMode.value = background.mode;
  if (background.mode === "blur") {
    remembered.radius = background.radius;
  } else if (background.mode === "replace") {
    remembered.imagePath = background.image_path;
  }
  els.blurRadius.value = remembered.radius;
  els.blurOut.textContent = `${remembered.radius} px`;
  els.imagePath.value = remembered.imagePath;
  els.blurField.hidden = background.mode !== "blur";
  els.imageField.hidden = background.mode !== "replace";

  els.noise.value = noise;

  applyPreviewTransform(framing);
}

function showError(text) {
  els.error.textContent = text;
  els.error.hidden = !text;
}

// Send new settings to the engine and draw what it sends back.
async function update(change) {
  const next = structuredClone(current);
  change(next);
  const id = ++requestId;
  try {
    const result = await invoke("set_settings", { settings: next });
    if (id === requestId) {
      showError("");
      render(result);
    }
  } catch (err) {
    if (id === requestId) {
      showError(`Could not save the settings: ${err}`);
    }
  }
}

function backgroundFromControls() {
  switch (els.bgMode.value) {
    case "blur":
      return { mode: "blur", radius: Number(els.blurRadius.value) };
    case "replace":
      return { mode: "replace", image_path: els.imagePath.value };
    default:
      return { mode: "none" };
  }
}

function bindControls() {
  els.zoom.addEventListener("input", () =>
    update((s) => (s.framing.zoom = Number(els.zoom.value))),
  );
  els.panX.addEventListener("input", () =>
    update((s) => (s.framing.pan_x = Number(els.panX.value))),
  );
  els.panY.addEventListener("input", () =>
    update((s) => (s.framing.pan_y = Number(els.panY.value))),
  );
  els.mirror.addEventListener("change", () =>
    update((s) => (s.framing.mirror = els.mirror.checked)),
  );
  els.bgMode.addEventListener("change", () => {
    remembered.radius = Number(els.blurRadius.value);
    remembered.imagePath = els.imagePath.value;
    update((s) => (s.background = backgroundFromControls()));
  });
  els.blurRadius.addEventListener("input", () => {
    remembered.radius = Number(els.blurRadius.value);
    update((s) => (s.background = backgroundFromControls()));
  });
  els.imagePath.addEventListener("change", () => {
    remembered.imagePath = els.imagePath.value;
    update((s) => (s.background = backgroundFromControls()));
  });
  els.noise.addEventListener("change", () =>
    update((s) => (s.noise = els.noise.value)),
  );
}

// ---- Live preview ----

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

function stopPreview() {
  const stream = els.video.srcObject;
  if (stream) {
    stream.getTracks().forEach((track) => track.stop());
    els.video.srcObject = null;
  }
}

async function startPreview(deviceId) {
  stopPreview();
  els.message.textContent = "";
  if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
    els.message.textContent = "This system's web view cannot show a camera preview.";
    return;
  }
  try {
    const video = deviceId ? { deviceId: { exact: deviceId } } : true;
    els.video.srcObject = await navigator.mediaDevices.getUserMedia({ video, audio: false });
    await refreshCameraList();
  } catch (err) {
    els.message.textContent = previewProblem(err);
  }
}

// Labels are only available once the user has allowed camera access.
async function refreshCameraList() {
  const devices = await navigator.mediaDevices.enumerateDevices();
  const cameras = devices.filter((d) => d.kind === "videoinput");
  const active = els.video.srcObject?.getVideoTracks()[0]?.getSettings().deviceId;
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

async function showPlatform() {
  try {
    const info = await invoke("platform_info");
    els.platform.textContent = `Platform: ${info.os}. Virtual camera backend: ${info.camera_backend}.`;
  } catch (err) {
    els.platform.textContent = "Platform details are not available.";
  }
}

async function init() {
  bindControls();
  els.camera.addEventListener("change", () => startPreview(els.camera.value));
  showPlatform();
  try {
    render(await invoke("get_settings"));
  } catch (err) {
    showError(`Could not read the settings: ${err}`);
  }
  startPreview();
}

init();
