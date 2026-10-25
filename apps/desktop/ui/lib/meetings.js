// "Meeting camera": the virtual camera that Zoom, Meet and Teams can pick.
//
// Settings holds the switch, its status and any setup step; the main sidebar has a one-line
// summary row. Both only exist when the app can really offer the camera (a build that
// carries the camera extension), so nobody is asked to install something that cannot work.
// Once set up there is nothing to start: the real camera runs while a meeting uses it.

import { invoke, listen } from "./tauri.js";

const POLL_MS = 1000;
const CAMERA_KEY = "webcam-studio.meeting-camera";

export function createMeetings({ section, row, summary, dot, toggle, info, setup, action, previewLabel }) {
  let status = null;
  let on = true;
  let busy = "";
  let timer = 0;
  // Set when the person asks to install, so System Settings opens by itself once macOS
  // wants approval, instead of making them find the page.
  let installing = false;

  // What to say, in the sidebar row (short) and in Settings (full).
  function describe() {
    const state = status ? status.state : "unknown";
    const fps = Math.round(Number(status?.fps) || 0);
    if (state === "unknown") return { short: "Checking…", long: "Checking…" };
    if (state === "not_installed") return { short: "Not set up yet", long: "Set it up once so meeting apps can choose it.", setup: "Set up meeting camera" };
    if (state === "needs_approval") return { short: "Waiting for approval", long: status.message, setup: "Open System Settings" };
    if (state === "unsupported") return { short: "Not available", long: status.message };
    if (!on) return { short: "Off", long: "Off. Meeting apps that choose it see a placeholder." };
    if (state === "error") return { short: "Problem", long: status.message, setup: "Try again", problem: true };
    if (state === "running") return { short: `In use · ${fps} fps`, long: `In use by a meeting app · ${fps} fps`, live: true };
    return { short: "On · ready", long: "On. Turns your camera on when a meeting uses it." };
  }

  function render() {
    const view = describe();
    summary.textContent = view.short;
    dot.hidden = !view.live;
    toggle.checked = on;
    toggle.disabled = !status || status.state === "unsupported";
    setup.hidden = !busy && !view.setup;
    action.disabled = !!busy;
    action.textContent = busy || view.setup || "";
    if (!busy) {
      info.textContent = view.long;
      info.classList.toggle("problem", !!view.problem);
    }
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
      [status, on] = await Promise.all([invoke("virtual_camera_status"), invoke("meeting_camera_enabled")]);
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

  async function onSetup() {
    if (busy || !status) return;
    try {
      if (status.state === "needs_approval") {
        await invoke("open_extension_settings");
      } else {
        installing = true;
        await invoke("install_camera_extension");
      }
    } catch (err) {
      installing = false;
      info.textContent = String(err.message || err);
      info.classList.add("problem");
    }
    busy = "";
    await refresh();
  }

  async function onToggle() {
    on = toggle.checked;
    render();
    try {
      on = await invoke("set_meeting_camera_enabled", { enabled: on });
    } catch {
      on = !on;
    }
    await refresh();
  }

  async function init() {
    const available = await invoke("meetings_available").catch(() => false);
    if (!available) return;
    section.hidden = false;
    row.hidden = false;
    action.addEventListener("click", onSetup);
    toggle.addEventListener("change", onToggle);
    // macOS answers install requests later, and may answer twice (approval, then done).
    listen("camera-extension", refresh);
    listen("meeting-camera-switched", refresh);
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

// "Open at login": a plain switch in Settings.
export async function bindOpenAtLogin(input) {
  input.checked = await invoke("open_at_login").catch(() => false);
  input.addEventListener("change", async () => {
    try {
      input.checked = await invoke("set_open_at_login", { enabled: input.checked });
    } catch {
      input.checked = !input.checked;
    }
  });
}
