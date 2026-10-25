// Native window controls are optional: the UI also runs in a regular browser.
import { hasBackend, invoke, listen } from "./tauri.js";

export function initWindow() {
  const root = document.documentElement;
  const button = document.getElementById("fullscreen");
  if (/Mac/.test(navigator.platform)) root.dataset.platform = "macos";

  // WebKit may focus a pop-up after a permission dialog; that is not keyboard navigation.
  document.addEventListener("keydown", (event) => {
    if (["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      root.dataset.keyboardFocus = "true";
    }
  }, true);
  document.addEventListener("pointerdown", () => delete root.dataset.keyboardFocus, true);

  if (!hasBackend()) return;
  const paint = ({ platform, theme, fullscreen }) => {
    if (platform) root.dataset.platform = platform;
    if (theme === "light" || theme === "dark") root.dataset.theme = theme;
    button.hidden = false;
    const label = fullscreen ? "Exit full screen" : "Enter full screen";
    button.setAttribute("aria-label", label);
    button.title = label;
  };
  const refresh = () => invoke("window_chrome").then(paint).catch(() => {});
  let refreshTimer;
  listen("window-theme-changed", ({ payload }) => {
    if (payload === "light" || payload === "dark") root.dataset.theme = payload;
  });
  listen("window-chrome-changed", () => {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(refresh, 100);
  });
  button.addEventListener("click", async () => {
    button.disabled = true;
    try {
      await invoke("toggle_fullscreen");
      await refresh();
    } catch {
      // The green traffic light and View menu remain available during a transition.
    } finally {
      button.disabled = false;
    }
  });
  refresh();
}
