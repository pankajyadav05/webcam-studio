// Auto-frame for the preview: the same steps and numbers as crates/engine/src/autoframe.rs,
// which frames the meeting camera. Change both together.
//
// Faces (shares of the camera picture, top-left origin) come in a few times a second; step()
// returns the zoom and pan for each painted frame, moving with a critically damped spring and
// ignoring small moves so the picture stays still while you talk.

export const AUTO = Object.freeze({
  maxZoom: 2.2,
  faceHeightsTall: 3.6,
  sideSpace: 0.9,
  minFaceShare: 0.45,
  faceLine: 0.40,
  deadZone: 0.08,
  resizeZone: 0.12,
  lostSeconds: 2.0,
  spring: 4.0,
});
const STEP_SECONDS = 1 / 60;

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

// The same crop as cropRect() in studio-engine.js, kept here so this file stands alone.
function crop(iw, ih, ow, oh, f) {
  const aspect = ow > 0 && oh > 0 ? ow / oh : iw / ih;
  const zoom = clamp(f.zoom ?? 1, 1, 4);
  const width = Math.min(iw, ih * aspect) / zoom, height = Math.min(ih, iw / aspect) / zoom;
  return { x: (iw - width) * (clamp(f.pan_x ?? 0, -1, 1) + 1) / 2,
    y: (ih - height) * (clamp(f.pan_y ?? 0, -1, 1) + 1) / 2, width, height };
}

function geometry(iw, ih, ow, oh) {
  if (!(iw > 0 && ih > 0)) return null;
  const full = crop(iw, ih, ow, oh, { zoom: 1 });
  return { iw, ih, maxW: full.width, maxH: full.height };
}

function fit(g, v) {
  const size = clamp(v.size, 1 / AUTO.maxZoom, 1);
  const halfW = g.maxW * size / 2 / g.iw, halfH = g.maxH * size / 2 / g.ih;
  return { cx: clamp(v.cx, halfW, 1 - halfW), cy: clamp(v.cy, halfH, 1 - halfH), size };
}

function viewOf(g, framing, ow, oh) {
  const r = crop(g.iw, g.ih, ow, oh, framing);
  return { cx: (r.x + r.width / 2) / g.iw, cy: (r.y + r.height / 2) / g.ih, size: r.height / g.maxH };
}

function framingOf(g, v, mirror) {
  const w = g.maxW * v.size, h = g.maxH * v.size;
  const pan = (centre, length, total) => {
    const margin = total - length;
    return margin > 0.5 ? clamp((centre * total - length / 2) / margin * 2 - 1, -1, 1) : 0;
  };
  return { zoom: clamp(1 / v.size, 1, 4), pan_x: pan(v.cx, w, g.iw), pan_y: pan(v.cy, h, g.ih), mirror: !!mirror };
}

export function frameFaces(g, faces) {
  const biggest = faces.reduce((m, f) => Math.max(m, f.height), 0);
  if (!(biggest > 0)) return null;
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const f of faces) {
    if (f.height < biggest * AUTO.minFaceShare) continue;
    x0 = Math.min(x0, f.x * g.iw); y0 = Math.min(y0, f.y * g.ih);
    x1 = Math.max(x1, (f.x + f.width) * g.iw); y1 = Math.max(y1, (f.y + f.height) * g.ih);
  }
  const faceH = biggest * g.ih, aspect = g.maxW / g.maxH;
  const cropH = Math.max(faceH * AUTO.faceHeightsTall, (x1 - x0 + 2 * AUTO.sideSpace * faceH) / aspect,
    y1 - y0 + (AUTO.faceHeightsTall - 1) * faceH);
  const size = clamp(cropH / g.maxH, 1 / AUTO.maxZoom, 1);
  return fit(g, { cx: (x0 + x1) / 2 / g.iw, cy: ((y0 + y1) / 2 + (0.5 - AUTO.faceLine) * size * g.maxH) / g.ih, size });
}

export function createAutoFramer() {
  let view = null, velocity = [0, 0, 0], target = null, lastFaceAt = null, lastAt = null;
  // Show-and-tell's focus, and whether auto-frame is off (go back to the manual framing).
  let focus = null, manual = false;
  return {
    reset() { view = null; velocity = [0, 0, 0]; target = null; lastFaceAt = null; lastAt = null; },
    setAim(followFaces, nextFocus) { manual = !followFaces; focus = nextFocus || null; },
    busy: () => view !== null,
    // `input` and `out` are [width, height]; times are in seconds.
    observe(faces, input, out, now) {
      const g = geometry(input[0], input[1], out[0], out[1]);
      const wanted = g && frameFaces(g, faces || []);
      if (!wanted) return;
      lastFaceAt = now;
      const moved = !target
        || Math.abs(wanted.cx - target.cx) > AUTO.deadZone * g.maxW * target.size / g.iw
        || Math.abs(wanted.cy - target.cy) > AUTO.deadZone * g.maxH * target.size / g.ih
        || Math.abs(wanted.size - target.size) > AUTO.resizeZone * target.size;
      if (moved) target = wanted;
    },
    step(start, input, out, now) {
      const g = geometry(input[0], input[1], out[0], out[1]);
      if (!g) return { ...start };
      const current = view || fit(g, viewOf(g, start, out[0], out[1]));
      if (lastFaceAt !== null && now - lastFaceAt > AUTO.lostSeconds) target = { cx: 0.5, cy: 0.5, size: 1 };
      const home = fit(g, viewOf(g, start, out[0], out[1]));
      const goalView = focus
        ? fit(g, { cx: focus.cx, cy: focus.cy, size: Math.max(focus.height * g.ih / g.maxH, focus.width * g.iw / g.maxW) })
        : manual ? home : target ? fit(g, target) : current;
      let dt = lastAt !== null && now > lastAt ? Math.min(0.25, now - lastAt) : 0;
      lastAt = now;
      const x = [current.cx, current.cy, current.size], goal = [goalView.cx, goalView.cy, goalView.size];
      while (dt > 1e-9) {
        const h = Math.min(dt, STEP_SECONDS);
        for (let i = 0; i < 3; i++) {
          const a = AUTO.spring * AUTO.spring * (goal[i] - x[i]) - 2 * AUTO.spring * velocity[i];
          velocity[i] += a * h; x[i] += velocity[i] * h;
        }
        dt -= h;
      }
      const homeArr = [home.cx, home.cy, home.size];
      if (manual && !focus && x.every((v, i) => Math.abs(v - homeArr[i]) < 2e-3 && Math.abs(velocity[i]) < 1e-2)) {
        // Back at the manual framing: hand over exactly, and stop.
        view = null; velocity = [0, 0, 0]; lastAt = null;
        return { ...start };
      }
      view = fit(g, { cx: x[0], cy: x[1], size: x[2] });
      return framingOf(g, view, start.mirror);
    },
  };
}
