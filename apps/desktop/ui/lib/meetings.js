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

  // What to say, in the sidebar row (short) and in Settings (full). `blocked` means the
  // camera cannot be used yet, so the switch is off and greyed out until setup is done.
  function describe() {
    const state = status ? status.state : "unknown";
    const fps = Math.round(Number(status?.fps) || 0);
    switch (state) {
      case "unknown": return { short: "Checking…", long: "Checking…", blocked: true };
      case "unsupported": return { short: "Not available", long: status.message, blocked: true };
      case "not_installed": return { short: "Not set up", long: "Not set up yet. Set it up once so meeting apps can choose it.", setup: "Set up meeting camera", blocked: true, warn: true };
      case "needs_approval": return { short: "Waiting for your approval", long: "Almost there: in System Settings, under Camera Extensions, turn on Webcam Studio.", setup: "Open System Settings", blocked: true, warn: true };
      case "finishing": return { short: "Finishing setup…", long: "Finishing setup…", blocked: true };
      case "needs_restart": return { short: "Restart to finish setup", long: "Installed. Restart your Mac to finish setting it up.", setup: "Restart Mac", blocked: true, warn: true };
    }
    if (!on) return { short: "Off", long: "Off. Meeting apps that choose it see a placeholder." };
    if (state === "error") return { short: "Problem", long: status.message, setup: "Try again", warn: true };
    if (state === "running") return { short: `In use · ${fps} fps`, long: `In use by a meeting app · ${fps} fps`, live: true };
    return { short: "On · waiting for a meeting", long: "On. Turns your camera on when a meeting uses it." };
  }

  function render() {
    const view = describe();
    summary.textContent = view.short;
    summary.classList.toggle("warn", !!view.warn);
    dot.hidden = !view.live;
    toggle.checked = on && !view.blocked;
    toggle.disabled = !!view.blocked;
    setup.hidden = !busy && !view.setup;
    action.disabled = !!busy;
    action.textContent = busy || view.setup || "";
    if (!busy) {
      info.textContent = view.long;
      info.classList.toggle("warn", !!view.warn);
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
      } else if (status.state === "needs_restart") {
        await invoke("restart_mac");
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
