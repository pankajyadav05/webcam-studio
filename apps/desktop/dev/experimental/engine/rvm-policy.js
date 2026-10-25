// A slow matte may be reused, but below 15 Hz motion lag is too conspicuous.
export const MIN_MATTE_FPS = 15;
export const RESET_GAP_MS = 1000;
export function createRatioPolicy() {
  let ratio = .25, count = 0, total = 0, tried = false;
  return {
    get ratio() { return ratio; },
    sample(ms) {
      if (!Number.isFinite(ms) || ms <= 0) return null;
      // Exclude shader compilation and let each resolution settle first.
      count++; if (count <= 5) return null;
      total += ms;
      if (count < 35) return null;
      const mean = total / (count - 5); count = total = 0;
      if (ratio === .375 && mean > 30) { ratio = .25; return 'lower'; }
      if (ratio === .25 && !tried && mean < 18) { ratio = .375; tried = true; return 'higher'; }
      if (ratio === .25 && mean > 1000 / MIN_MATTE_FPS) return 'slow';
      return null;
    },
  };
}
