//! Webcam Studio keeps running after its window closes, so meeting apps can use the camera
//! without the window open. While the window is closed the app lives in the menu bar only
//! (no Dock icon), and the window lets go of the camera and microphone.
//!
//! The menu bar icon opens the window, switches the background, turns "Open at Login" on
//! or off and quits. Launched at login, the app starts with its window closed.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use studio_core::settings::BackgroundMode;

/// Passed by the login item, so the app starts in the menu bar with its window closed.
pub const HIDDEN_ARG: &str = "--hidden";
/// Tells the window it was shown (`true`) or closed (`false`).
pub const VISIBILITY_EVENT: &str = "window-visibility";
/// Asks the window to switch the background: "none", "blur" or "replace".
pub const BACKGROUND_EVENT: &str = "tray-background";

/// The menu items that change after the menu is built.
pub struct TrayItems {
    _tray: TrayIcon,
    status: MenuItem<Wry>,
    none: CheckMenuItem<Wry>,
    blur: CheckMenuItem<Wry>,
    replace: CheckMenuItem<Wry>,
    login: CheckMenuItem<Wry>,
}

/// Whether this run was started by the login item.
pub fn started_hidden() -> bool {
    std::env::args().any(|arg| arg == HIDDEN_ARG)
}

/// Shows the window and puts the app back in the Dock.
pub fn show_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit(VISIBILITY_EVENT, true);
    }
}

/// Closes the window into the menu bar: hidden, out of the Dock, camera released.
pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(VISIBILITY_EVENT, false);
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// Whether the window is on screen.
#[tauri::command]
pub fn window_visible(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

#[tauri::command]
pub fn open_at_login(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_open_at_login(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| format!("Could not change Open at Login: {e}"))?;
    let now = launcher.is_enabled().unwrap_or(enabled);
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.login.set_checked(now);
    }
    Ok(now)
}

/// Ticks the background the engine uses now.
pub fn reflect_background(app: &AppHandle, mode: &BackgroundMode) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let _ = items.none.set_checked(matches!(mode, BackgroundMode::None));
    let _ = items
        .blur
        .set_checked(matches!(mode, BackgroundMode::Blur { .. }));
    let _ = items
        .replace
        .set_checked(matches!(mode, BackgroundMode::Replace { .. }));
}

/// The first line of the menu, about the meeting camera.
pub fn set_status(app: &AppHandle, text: &str) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.status.set_text(text);
    }
}

/// Builds the menu bar icon and its menu.
pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let handle = app.handle();
    let status = MenuItem::with_id(handle, "status", "Meeting camera: off", false, None::<&str>)?;
    let none = CheckMenuItem::with_id(handle, "bg-none", "No Effect", true, true, None::<&str>)?;
    let blur = CheckMenuItem::with_id(
        handle,
        "bg-blur",
        "Blur Background",
        true,
        false,
        None::<&str>,
    )?;
    let replace = CheckMenuItem::with_id(
        handle,
        "bg-replace",
        "Replace Background",
        true,
        false,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(handle, "open", "Open Webcam Studio", true, None::<&str>)?;
    let login_on = handle.autolaunch().is_enabled().unwrap_or(false);
    let login = CheckMenuItem::with_id(
        handle,
        "login",
        "Open at Login",
        true,
        login_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        handle,
        "quit",
        "Quit Webcam Studio",
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let menu = Menu::with_items(
        handle,
        &[
            &status,
            &PredefinedMenuItem::separator(handle)?,
            &none,
            &blur,
            &replace,
            &PredefinedMenuItem::separator(handle)?,
            &open,
            &login,
            &PredefinedMenuItem::separator(handle)?,
            &quit,
        ],
    )?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    let tray = TrayIconBuilder::with_id("webcam-studio")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Webcam Studio")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "quit" => app.exit(0),
            "login" => {
                let wanted = !open_at_login(app.clone());
                if let Err(e) = set_open_at_login(app.clone(), wanted) {
                    log::warn!("{e}");
                }
            }
            id @ ("bg-none" | "bg-blur" | "bg-replace") => {
                let mode = id.trim_start_matches("bg-");
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit(BACKGROUND_EVENT, mode);
                }
                // Tick it now; the window confirms through the settings it sends back.
                let mode = match mode {
                    "blur" => BackgroundMode::Blur { radius: 12 },
                    "replace" => BackgroundMode::Replace {
                        image_path: String::new(),
                    },
                    _ => BackgroundMode::None,
                };
                reflect_background(app, &mode);
            }
            _ => {}
        })
        .build(app)?;
    app.manage(TrayItems {
        _tray: tray,
        status,
        none,
        blur,
        replace,
        login,
    });
    Ok(())
}
