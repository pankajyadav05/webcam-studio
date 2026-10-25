// The bridge to the Rust side. In a plain browser (for tests and design work) there is no
// bridge, and every call fails with a clear message instead of a crash.

export function hasBackend() {
  return !!(window.__TAURI__ && window.__TAURI__.core);
}

export function invoke(command, args, options) {
  if (!hasBackend()) {
    return Promise.reject(new Error("Not running inside the app."));
  }
  return window.__TAURI__.core.invoke(command, args, options);
}

export function listen(name, handler) {
  const events = window.__TAURI__ && window.__TAURI__.event;
  if (!events || !events.listen) {
    return Promise.resolve(() => {});
  }
  return Promise.resolve(events.listen(name, handler)).catch(() => () => {});
}
