// "Use in meetings": the virtual camera that Zoom, Meet and Teams can pick.
//
// This card only exists when the app can really offer it (a build that carries the camera
// extension). Everywhere else it stays hidden, so nobody is asked to install something that
// cannot work.

import { invoke, listen } from "./tauri.js";

const POLL_MS = 1000;

export function createMeetings({ card, action, info, previewLabel }) {
  let status = null;
  let busy = "";
  let timer = 0;

  const setInfo = (text, problem = false) => {
    info.textContent = text;
    info.classList.toggle("problem", problem);
  };

  function render() {
    const state = status ? status.state : "unknown";
    action.disabled = !!busy || state === "unknown" || state === "unsupported";
    action.classList.toggle("primary", state !== "running");
    action.textContent =
      busy ||
      {
        not_installed: "Set up virtual camera",
        needs_approval: "Open System Settings",
        ready: "Start virtual camera",
        running: "Stop virtual camera",
        error: "Try again",
      }[state] ||
      "Set up virtual camera";
    if (!status || busy) {
      return;
    }
    const fps = Math.round(Number(status.fps) || 0);
    setInfo(
      state === "running" ? `Choose Webcam Studio Camera in your meeting app. ${fps} fps` : status.message,
      state === "error",
    );
  }

  async function refresh() {
    try {
      status = await invoke("virtual_camera_status");
    } catch (err) {
      status = { state: "error", message: String(err.message || err), fps: 0 };
    }
    render();
  }

  // The real camera to send, matched by name to the one shown in the preview.
  async function nativeCameraId() {
    const cameras = await invoke("list_native_cameras");
    const wanted = previewLabel();
    const match = cameras.find((c) => c.name === wanted) || cameras[0];
    if (!match) {
      throw new Error("No camera was found.");
    }
    return match.id;
  }

  async function onClick() {
    if (busy || !status) {
      return;
    }
    const state = status.state;
    try {
      if (state === "running") {
        busy = "Stopping…";
        render();
        await invoke("stop_virtual_camera");
      } else if (state === "ready" || state === "error") {
        busy = "Starting…";
        render();
        await invoke("start_virtual_camera", { cameraId: await nativeCameraId() });
      } else if (state === "needs_approval") {
        await invoke("open_extension_settings");
      } else {
        await invoke("install_camera_extension");
      }
    } catch (err) {
      setInfo(String(err.message || err), true);
    }
    busy = "";
    await refresh();
  }

  async function init() {
    let available = false;
    try {
      available = await invoke("meetings_available");
    } catch {
      available = false;
    }
    if (!available) {
      return;
    }
    card.hidden = false;
    action.addEventListener("click", onClick);
    // macOS answers install requests later, and may answer twice (approval, then done).
    listen("camera-extension", refresh);
    document.addEventListener("visibilitychange", () => {
      clearInterval(timer);
      timer = 0;
      if (!document.hidden) {
        refresh();
        timer = setInterval(refresh, POLL_MS);
      }
    });
    await refresh();
    timer = setInterval(refresh, POLL_MS);
  }

  return { init };
}
