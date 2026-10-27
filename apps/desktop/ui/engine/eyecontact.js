// Eye contact for the preview. The network runs in Rust (crates/engine/src/eyecontact.rs);
// this reads its answer and turns it into what the compositor's shader needs.

export const EYE_MAP = Object.freeze({ width: 64, height: 48, easeSeconds: 0.15, staleSeconds: 0.4 });
const PER_EYE = 5 + EYE_MAP.width * EYE_MAP.height * 3;

// The bytes from `eye_contact_maps`: the picture size, then each eye's box, mirror flag and
// maps (x warp, y warp, share to keep). Returns null when there are no eyes it can help.
export function parseEyeMaps(buffer) {
  const f = new Float32Array(buffer instanceof ArrayBuffer ? buffer : new Uint8Array(buffer).buffer);
  if (f.length < 3) return null;
  const [frameWidth, frameHeight, count] = f;
  if (!count || f.length < 3 + count * PER_EYE) return null;
  const eyes = [];
  for (let i = 0; i < count; i++) {
    const at = 3 + i * PER_EYE;
    // RGBA for a float texture: x warp, y warp, keep, unused.
    const maps = new Float32Array(EYE_MAP.width * EYE_MAP.height * 4);
    for (let p = 0; p < EYE_MAP.width * EYE_MAP.height; p++) {
      maps[p * 4] = f[at + 5 + p * 3];
      maps[p * 4 + 1] = f[at + 5 + p * 3 + 1];
      maps[p * 4 + 2] = f[at + 5 + p * 3 + 2];
    }
    const mirrored = f[at + 4] > 0.5;
    // A mirrored eye's box is given in the mirrored picture; put it back.
    const left = mirrored ? frameWidth - f[at + 1] - f[at + 3] : f[at + 1];
    eyes.push({ top: f[at], left, height: f[at + 2], width: f[at + 3], mirrored, maps });
  }
  return { frameWidth, frameHeight, eyes };
}

// An eye's box in the captured picture's shares (top-down), for the shader. `scale` turns the
// answer's pixels into video pixels; `capture` is the part of the video the frame holds.
export function eyeBox(eye, scale, capture) {
  return [
    (eye.left * scale - capture.x) / capture.width,
    (eye.top * scale - capture.y) / capture.height,
    eye.width * scale / capture.width,
    eye.height * scale / capture.height,
  ];
}
