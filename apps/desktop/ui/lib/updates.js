// "Updates" in Settings. Rust looks for updates and downloads them; this shows the status
// and offers Check for updates, or Restart to Update once a download is ready.

import { invoke, listen } from "./tauri.js";

export function describeUpdate(status) {
  switch (status?.state) {
    case "checking": return { text: "Checking for updates…", action: null };
    case "downloading": return { text: `Downloading version ${status.version}…`, action: null };
    case "ready": return { text: `Version ${status.version} is ready to install.`, action: "restart", label: "Restart to Update", primary: true };
    case "move_to_applications": return { text: `Version ${status.version} is available. Move Webcam Studio to the Applications folder to update.`, action: "check", label: "Check again", warn: true };
    case "failed": return { text: `Could not check for updates. ${status.message}`, action: "check", label: "Try again", warn: true };
    case "idle": return { text: "You have the latest version.", action: "check", label: "Check for updates" };
    default: return null;
  }
}

export async function bindUpdates({ section, version, info, action }) {
  const status = await invoke("update_status").catch(() => null);
  if (!status || status.state === "disabled") return;
  section.hidden = false;
  version.textContent = `Webcam Studio ${await invoke("app_version").catch(() => "")}`.trim();

  let current = status;
  const render = () => {
    const view = describeUpdate(current);
    if (!view) return;
    info.textContent = view.text;
    info.classList.toggle("warn", !!view.warn);
    action.hidden = !view.action;
    action.textContent = view.label || "";
    action.classList.toggle("primary", !!view.primary);
    action.dataset.action = view.action || "";
  };
  action.addEventListener("click", async () => {
    action.disabled = true;
    try {
      if (action.dataset.action === "restart") await invoke("restart_to_update");
      else current = await invoke("check_for_updates");
    } catch (err) {
      current = { state: "failed", message: String(err.message || err) };
    }
    action.disabled = false;
    render();
  });
  listen("update-status", (event) => {
    current = event.payload;
    render();
  });
  render();
}
