// The menu bar panel: switch the meeting camera on or off, see what it is doing, and get
// into the app. Effects and every other setting live in the window.
import { invoke, listen } from "./lib/tauri.js";

const $ = (id) => document.getElementById(id);
const module = $("module"), enabled = $("enabled"), statusText = $("status-text");

function describe(on, status) {
  const state = status?.state;
  if (state === "unsupported") return { text: "Not available on this Mac", problem: true };
  if (state === "not_installed" || state === "needs_approval") return { text: "Needs setup · see Settings", problem: true };
  if (!on) return { text: "Off" };
  if (state === "running") return { text: `In use · ${Math.round(Number(status.fps) || 0)} fps`, live: true };
  if (state === "error") return { text: "Problem · open Settings", problem: true };
  return { text: "On · waiting for a meeting" };
}

async function refresh() {
  const [on, status] = await Promise.all([
    invoke("meeting_camera_enabled").catch(() => false),
    invoke("virtual_camera_status").catch(() => null),
  ]);
  const view = describe(on, status);
  enabled.checked = on;
  enabled.disabled = status?.state === "unsupported";
  module.classList.toggle("on", on);
  module.classList.toggle("live", !!view.live);
  module.classList.toggle("problem", !!view.problem);
  statusText.textContent = view.text;
}

enabled.addEventListener("change", async () => {
  module.classList.toggle("on", enabled.checked);
  await invoke("set_meeting_camera_enabled", { enabled: enabled.checked }).catch(() => {});
  refresh();
});
$("open").addEventListener("click", () => invoke("show_main_window"));
$("settings").addEventListener("click", () => invoke("open_settings"));
$("quit").addEventListener("click", () => invoke("quit_app"));
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
