// Pictures to put behind the person. The presets are drawn, not shipped, so there are no
// image files to carry and they stay sharp at any size.

export const PRESETS = [
  { id: "studio", name: "Studio", a: "#41444c", b: "#17181b", glow: "rgba(255,255,255,0.16)" },
  { id: "paper", name: "Paper", a: "#f1ebe1", b: "#d6cdbf", glow: "rgba(255,255,255,0.55)" },
  { id: "sage", name: "Sage", a: "#93a99a", b: "#566b5f", glow: "rgba(255,255,255,0.24)" },
  { id: "dusk", name: "Dusk", a: "#3f6072", b: "#1b2933", glow: "rgba(255,190,140,0.2)" },
  { id: "sand", name: "Sand", a: "#dcbd98", b: "#a98260", glow: "rgba(255,255,255,0.32)" },
  { id: "slate", name: "Slate", a: "#707684", b: "#333842", glow: "rgba(255,255,255,0.18)" },
];

export function presetById(id) {
  return PRESETS.find((p) => p.id === id) || PRESETS[0];
}

// A soft diagonal wash with a light pool top left, so it reads as a room and not a swatch.
export function drawPreset(ctx, width, height, preset) {
  const wash = ctx.createLinearGradient(0, 0, width, height);
  wash.addColorStop(0, preset.a);
  wash.addColorStop(1, preset.b);
  ctx.fillStyle = wash;
  ctx.fillRect(0, 0, width, height);

  const pool = ctx.createRadialGradient(width * 0.28, height * 0.22, 0, width * 0.28, height * 0.22, width * 0.75);
  pool.addColorStop(0, preset.glow);
  pool.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = pool;
  ctx.fillRect(0, 0, width, height);

  const edge = ctx.createRadialGradient(width / 2, height / 2, height * 0.45, width / 2, height / 2, width * 0.75);
  edge.addColorStop(0, "rgba(0,0,0,0)");
  edge.addColorStop(1, "rgba(0,0,0,0.28)");
  ctx.fillStyle = edge;
  ctx.fillRect(0, 0, width, height);
}

const cache = new Map();

// The full-size picture the engine draws behind the person. Made once per preset.
export function presetSource(id, width = 1280, height = 720) {
  const key = `${id}:${width}x${height}`;
  if (!cache.has(key)) {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    drawPreset(canvas.getContext("2d"), width, height, presetById(id));
    cache.set(key, canvas);
  }
  return cache.get(key);
}

const MAX_CUSTOM_WIDTH = 1920;

// Reads a picture the person chose. It is shrunk so it can also be remembered between
// launches without filling up storage.
export async function loadImageFile(file) {
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_CUSTOM_WIDTH / bitmap.width);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  canvas.getContext("2d").drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  return { source: canvas, dataUrl: canvas.toDataURL("image/jpeg", 0.85) };
}

export function imageFromDataUrl(dataUrl) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("The saved picture could not be read."));
    image.src = dataUrl;
  });
}
