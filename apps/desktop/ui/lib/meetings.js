// "Use in meetings": the virtual camera that Zoom, Meet and Teams can pick.
//
// This card only exists when the app can really offer it (a build that carries the camera
// extension). Everywhere else it stays hidden, so nobody is asked to install something that
// cannot work.

import { invoke, listen } from "./tauri.js";

const POLL_MS = 1000;
const CAMERA_KEY = "webcam-studio.meeting-camera";

export function createMeetings({ card, action, info, login, previewLabel }) {
  let status = null;
  let busy = "";
  let timer = 0;
  // Set when the person asks to install, so System Settings opens by itself once macOS
  // wants approval, instead of making them find the page.
  let installing = false;

  const setInfo = (text, problem = false) => {
    info.textContent = text;
    info.classList.toggle("problem", problem);
  };

  function render() {
    const state = status ? status.state : "unknown";
    // Ready and in use need nothing from the person: the camera turns on by itself while a
    // meeting app uses Webcam Studio Camera, and off again afterwards.
    const label = {
      not_installed: "Set up virtual camera",
      needs_approval: "Open System Settings",
      error: "Try again",
    }[state];
    action.hidden = !busy && !label;
    action.disabled = !!busy || state === "unknown" || state === "unsupported";
    action.textContent = busy || label || "";
    if (!status || busy) {
      return;
    }
    const fps = Math.round(Number(status.fps) || 0);
    const text = {
      ready: "Ready. In Zoom, Meet or Teams, choose Webcam Studio Camera. Your camera turns on only while a meeting uses it.",
      running: `In use by a meeting app · ${fps} fps. Closing this window keeps it going.`,
    }[state];
    setInfo(text || status.message, state === "error");
  }

  // The meeting camera opens the real camera by name. Remember the last one the window
  // used, so it is known even when the app starts in the menu bar without opening it.
  let toldCamera = "";
  function tellCamera() {
    const name = previewLabel() || localStorage.getItem(CAMERA_KEY) || "";
    if (!name || name === toldCamera) return;
    toldCamera = name;
    localStorage.setItem(CAMERA_KEY, name);
    invoke("set_meeting_camera", { name }).catch(() => {});
  }

  async function refresh() {
    try {
      status = await invoke("virtual_camera_status");
    } catch (err) {
      status = { state: "error", message: String(err.message || err), fps: 0 };
    }
    tellCamera();
    if (installing && status.state !== "not_installed") {
      installing = false;
      if (status.state === "needs_approval") {
        invoke("open_extension_settings").catch(() => {});
      }
    }
    render();
  }

  async function onClick() {
    if (busy || !status) {
      return;
    }
    const state = status.state;
    try {
      if (state === "needs_approval") {
        await invoke("open_extension_settings");
      } else {
        installing = true;
        await invoke("install_camera_extension");
      }
    } catch (err) {
      installing = false;
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
    if (login) {
      login.checked = await invoke("open_at_login").catch(() => false);
      login.addEventListener("change", async () => {
        try {
          login.checked = await invoke("set_open_at_login", { enabled: login.checked });
        } catch (err) {
          login.checked = !login.checked;
          setInfo(String(err.message || err), true);
        }
      });
    }
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
