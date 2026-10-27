// Show-and-tell for the preview: the same rules and numbers as crates/engine/src/showtell.rs.
// Hold something up to the camera, keep it still for a moment, and the picture zooms in on
// it; lower it and the picture goes back. Times are in seconds.

export const SHOW = Object.freeze({
  minHand: 0.5, reachDown: 1.6, minHandAlone: 0.12, deskLine: 0.72,
  holdSeconds: 0.7, still: 0.35, releaseSeconds: 0.8, spread: 2.6, lift: 0.15, deadZone: 0.12,
});

function heldUp(hand, face) {
  const cx = hand.x + hand.width / 2, cy = hand.y + hand.height / 2;
  if (!face) return hand.height >= SHOW.minHandAlone && cy < SHOW.deskLine;
  const onFace = cx > face.x && cx < face.x + face.width && cy > face.y && cy < face.y + face.height;
  return Math.max(hand.height, hand.width) >= SHOW.minHand * face.height
    && cy <= face.y + face.height * (1 + SHOW.reachDown) && !onFace;
}

export function createShowTell() {
  let anchor = null, since = null, focus = null, lastSeen = null;
  return {
    reset() { anchor = since = focus = lastSeen = null; },
    focus: () => focus,
    observe(hands = [], faces = [], now) {
      const face = faces.reduce((best, f) => (!best || f.height > best.height ? f : best), null);
      const held = hands.filter((h) => heldUp(h, face));
      const still = face ? SHOW.still * face.height : SHOW.still * 0.2;
      if (!held.length) {
        anchor = since = null;
        if (lastSeen === null || now - lastSeen > SHOW.releaseSeconds) focus = null;
        return;
      }
      const x0 = Math.min(...held.map((h) => h.x)), y0 = Math.min(...held.map((h) => h.y));
      const x1 = Math.max(...held.map((h) => h.x + h.width)), y1 = Math.max(...held.map((h) => h.y + h.height));
      const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2, w = (x1 - x0) * SHOW.spread, h = (y1 - y0) * SHOW.spread;
      const wanted = { cx, cy: cy - SHOW.lift * h, width: w, height: h };
      if (focus) {
        lastSeen = now;
        const moved = Math.abs(wanted.cx - focus.cx) > SHOW.deadZone * focus.width
          || Math.abs(wanted.cy - focus.cy) > SHOW.deadZone * focus.height
          || Math.abs(wanted.height - focus.height) > 2 * SHOW.deadZone * focus.height;
        if (moved) focus = wanted;
        return;
      }
      if (anchor && Math.abs(cx - anchor[0]) < still && Math.abs(cy - anchor[1]) < still) {
        if (now - since >= SHOW.holdSeconds) { focus = wanted; lastSeen = now; }
      } else {
        anchor = [cx, cy]; since = now;
      }
    },
  };
}
