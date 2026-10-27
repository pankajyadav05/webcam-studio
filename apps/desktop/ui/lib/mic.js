// The microphone level, to check it works. Only for the meter: the sound goes nowhere.
//
// It is opened raw, with no echo cancellation, noise suppression or gain control. With any of
// those on, macOS starts voice processing for this app, which can turn down or take over the
// microphone of a meeting app that is running at the same time.

const FLOOR_DB = -60;
const PEAK_HOLD_MS = 1200;

export function describeMicError(err) {
  const name = err && err.name;
  if (name === "NotAllowedError" || name === "SecurityError") {
    return { kind: "denied", text: "Microphone access is off." };
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return { kind: "none", text: "No microphone found." };
  }
  if (name === "NotReadableError" || name === "AbortError") {
    return { kind: "busy", text: "Another app is using the microphone." };
  }
  return { kind: "other", text: err && err.message ? err.message : "The microphone did not start." };
}

export function createMic(onLevel) {
  let stream = null;
  let context = null;
  let analyser = null;
  let buffer = null;
  let raf = 0;
  let peak = FLOOR_DB;
  let peakAt = 0;

  function stop() {
    cancelAnimationFrame(raf);
    raf = 0;
    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
    }
    if (context) {
      context.close().catch(() => {});
    }
    stream = context = analyser = buffer = null;
    peak = FLOOR_DB;
    onLevel(FLOOR_DB, FLOOR_DB);
  }

  function tick(now) {
    raf = requestAnimationFrame(tick);
    if (document.hidden || !analyser) {
      return;
    }
    analyser.getFloatTimeDomainData(buffer);
    let sum = 0;
    for (let i = 0; i < buffer.length; i += 1) {
      sum += buffer[i] * buffer[i];
    }
    const rms = Math.sqrt(sum / buffer.length);
    const db = rms > 0 ? Math.max(FLOOR_DB, 20 * Math.log10(rms)) : FLOOR_DB;
    if (db >= peak || now - peakAt > PEAK_HOLD_MS) {
      peak = db;
      peakAt = now;
    }
    onLevel(db, peak);
  }

  async function start({ deviceId }) {
    stop();
    const audio = {
      noiseSuppression: false,
      echoCancellation: false,
      autoGainControl: false,
      ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
    };
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio, video: false });
    } catch (err) {
      // A microphone that was unplugged: fall back to the default one.
      if (deviceId && (err.name === "OverconstrainedError" || err.name === "NotFoundError")) {
        return start({ deviceId: "" });
      }
      return { ok: false, problem: describeMicError(err) };
    }
    const Context = window.AudioContext || window.webkitAudioContext;
    context = new Context();
    if (context.state === "suspended") {
      await context.resume().catch(() => {});
    }
    analyser = context.createAnalyser();
    analyser.fftSize = 1024;
    analyser.smoothingTimeConstant = 0.3;
    buffer = new Float32Array(analyser.fftSize);
    // Not connected to the speakers, so there is no echo.
    context.createMediaStreamSource(stream).connect(analyser);
    raf = requestAnimationFrame(tick);
    const track = stream.getAudioTracks()[0];
    return { ok: true, deviceId: track.getSettings().deviceId || "", label: track.label };
  }

  async function list() {
    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices.filter((d) => d.kind === "audioinput");
  }

  return { start, stop, list };
}

export { FLOOR_DB };
