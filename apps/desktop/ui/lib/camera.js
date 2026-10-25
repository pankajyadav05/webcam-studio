// The camera, and what to tell the person when it cannot be opened.

// Ideals, not demands: a camera that cannot do 1080p still opens. Without a size, the web
// view hands back 640 x 480, which makes the 16:9 picture soft.
const SIZE = { width: { ideal: 1920 }, height: { ideal: 1080 }, frameRate: { ideal: 30 } };

// Turns a getUserMedia error into a short message and what the window should offer.
export function describeCameraError(err) {
  const name = err && err.name;
  if (name === "NotAllowedError" || name === "SecurityError") {
    return {
      kind: "denied",
      title: "Camera access is off",
      text: "Allow Webcam Studio in System Settings, under Privacy & Security, then Camera.",
    };
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return {
      kind: "none",
      title: "No camera found",
      text: "Connect a camera, or check that another app is not using it.",
    };
  }
  if (name === "NotReadableError" || name === "AbortError") {
    return {
      kind: "busy",
      title: "The camera is busy",
      text: "Another app is using it. Close that app, then try again.",
    };
  }
  return {
    kind: "other",
    title: "The camera did not start",
    text: err && err.message ? err.message : "Something went wrong while opening the camera.",
  };
}

export function createCamera(video) {
  let stream = null;

  function stop() {
    if (stream) {
      stream.getTracks().forEach((track) => track.stop());
    }
    stream = null;
    video.srcObject = null;
  }

  // Opens `deviceId`, or the default camera when it is empty or no longer there.
  async function start(deviceId, onEnded) {
    stop();
    const attempts = deviceId ? [{ ...SIZE, deviceId: { exact: deviceId } }, SIZE] : [SIZE];
    let lastError = null;
    for (const constraints of attempts) {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: constraints, audio: false });
        break;
      } catch (err) {
        lastError = err;
        // Only a camera that has gone away is worth retrying with the default one.
        if (err.name !== "OverconstrainedError" && err.name !== "NotFoundError") {
          break;
        }
      }
    }
    if (!stream) {
      return { ok: false, problem: describeCameraError(lastError) };
    }
    const track = stream.getVideoTracks()[0];
    track.addEventListener("ended", () => onEnded && onEnded());
    video.srcObject = stream;
    try {
      await video.play();
    } catch {
      // Autoplay is allowed for a muted video; if not, frames still arrive once it can play.
    }
    const settings = track.getSettings();
    return { ok: true, deviceId: settings.deviceId || "", settings };
  }

  async function list() {
    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices.filter((d) => d.kind === "videoinput");
  }

  return { start, stop, list };
}
