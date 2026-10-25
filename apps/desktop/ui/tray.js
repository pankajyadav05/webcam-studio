// The menu bar panel: switch the meeting camera on or off, see what it is doing, and get
// into the app. Effects and every other setting live in the window.
import { invoke, listen } from "./lib/tauri.js";

const $ = (id) => document.getElementById(id);
const module = $("module"), enabled = $("enabled"), statusText = $("status-text");

// `blocked`: the camera cannot be used yet. The switch gives way to a Set Up button that
// opens Settings, where the next step is.
function describe(on, status) {
  switch (status?.state) {
    case undefined: return { text: "Checking…", blocked: true };
    case "unsupported": return { text: "Not available on this Mac", blocked: true };
    case "not_installed": return { text: "Not set up yet", blocked: true, warn: true };
    case "needs_approval": return { text: "Waiting for your approval", blocked: true, warn: true };
    case "finishing": return { text: "Finishing setup…", blocked: true };
    case "needs_restart": return { text: "Restart your Mac to finish", blocked: true, warn: true };
  }
  if (!on) return { text: "Off" };
  if (status.state === "running") return { text: `In use · ${Math.round(Number(status.fps) || 0)} fps`, live: true };
  if (status.state === "error") return { text: "Problem · see Settings", warn: true };
  return { text: "On · waiting for a meeting" };
}

async function refresh() {
  const [on, status] = await Promise.all([
    invoke("meeting_camera_enabled").catch(() => false),
    invoke("virtual_camera_status").catch(() => null),
  ]);
  const view = describe(on, status);
  enabled.checked = on && !view.blocked;
  enabled.hidden = !!view.blocked;
  $("setup").hidden = !view.blocked || status?.state === "unsupported";
  module.classList.toggle("on", on && !view.blocked);
  module.classList.toggle("live", !!view.live);
  module.classList.toggle("warn", !!view.warn);
  statusText.textContent = view.text;
}

enabled.addEventListener("change", async () => {
  module.classList.toggle("on", enabled.checked);
  await invoke("set_meeting_camera_enabled", { enabled: enabled.checked }).catch(() => {});
  refresh();
});
$("open").addEventListener("click", () => invoke("show_main_window"));
$("settings").addEventListener("click", () => invoke("open_settings"));
$("setup").addEventListener("click", (event) => {
  event.preventDefault();
  invoke("open_settings");
});
$("quit").addEventListener("click", () => invoke("quit_app"));

// A downloaded update: one click installs it and starts the new version.
const update = $("update");
function showUpdate(status) {
  const ready = status?.state === "ready";
  update.hidden = !ready;
  if (ready) update.textContent = `Restart to Update (${status.version})`;
}
update.addEventListener("click", () => invoke("restart_to_update").catch(() => invoke("open_settings")));
invoke("update_status").then(showUpdate).catch(() => {});
listen("update-status", (event) => showUpdate(event.payload));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") invoke("hide_menu_panel");
  if (event.metaKey && event.key === ",") { event.preventDefault(); invoke("open_settings"); }
  if (event.metaKey && event.key.toLowerCase() === "q") { event.preventDefault(); invoke("quit_app"); }
});

// The window is as tall as its content, whatever the system font measures.
function fit() {
  const height = Math.ceil(document.querySelector(".panel").getBoundingClientRect().height);
  invoke("fit_menu_panel", { height }).catch(() => {});
}
new ResizeObserver(fit).observe(document.querySelector(".panel"));

listen("panel-shown", refresh);
listen("meeting-camera-switched", refresh);
setInterval(() => { if (!document.hidden) refresh(); }, 1000);
refresh();
