// The Be right back card: drawn once here, shown on the preview and sent to the meeting
// camera as a PNG with transparency (crates/engine/src/away.rs puts it on the still).

export function drawAwayCard(width = 1280, height = 720) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const c = canvas.getContext("2d");
  const s = height / 720;
  const font = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';
  c.textAlign = "center";
  c.textBaseline = "middle";
  // A soft round mark with a pause sign, then the words.
  const cx = width / 2, cy = height / 2 - 70 * s;
  c.fillStyle = "rgba(255,255,255,0.14)";
  c.beginPath(); c.arc(cx, cy, 54 * s, 0, Math.PI * 2); c.fill();
  c.fillStyle = "#ffffff";
  const bar = 12 * s, tall = 42 * s, gap = 10 * s;
  c.beginPath();
  c.roundRect(cx - gap - bar, cy - tall / 2, bar, tall, 4 * s);
  c.roundRect(cx + gap, cy - tall / 2, bar, tall, 4 * s);
  c.fill();
  c.shadowColor = "rgba(0,0,0,0.35)";
  c.shadowBlur = 18 * s;
  c.font = `700 ${64 * s}px ${font}`;
  c.fillText("Be right back", width / 2, height / 2 + 34 * s);
  c.shadowBlur = 0;
  c.fillStyle = "rgba(255,255,255,0.72)";
  c.font = `500 ${26 * s}px ${font}`;
  c.fillText("I will be back in a moment", width / 2, height / 2 + 98 * s);
  return canvas;
}
