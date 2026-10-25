//! Webcam Studio keeps running after its window closes, so meeting apps can use the camera
//! without the window open. While the window is closed the app lives in the menu bar only
//! (no Dock icon), and the window lets go of the camera and microphone.
//!
//! Clicking the menu bar icon opens a small panel (`ui/tray.html`): the meeting camera
//! switch, Open Webcam Studio, Settings and Quit. Everything else lives in the window.
//! Launched at login, the app starts with its window closed.

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::{Effect, EffectState};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;

/// Passed by the login item, so the app starts in the menu bar with its window closed.
pub const HIDDEN_ARG: &str = "--hidden";
/// Tells the window it was shown (`true`) or closed (`false`).
pub const VISIBILITY_EVENT: &str = "window-visibility";
/// Asks the window to show its Settings page.
pub const SETTINGS_EVENT: &str = "open-settings";
/// Tells the panel it was opened, so it refreshes at once.
pub const PANEL_EVENT: &str = "panel-shown";

const PANEL: &str = "panel";
const PANEL_WIDTH: f64 = 304.0;
const PANEL_HEIGHT: f64 = 206.0;

/// Whether this run was started by the login item.
pub fn started_hidden() -> bool {
    std::env::args().any(|arg| arg == HIDDEN_ARG)
}

/// Shows the window and puts the app back in the Dock.
pub fn show_window(app: &AppHandle) {
    hide_panel(app);
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

fn hide_panel(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
}

/// Opens the panel centred under the menu bar icon, or closes it if it is open.
fn toggle_panel(app: &AppHandle, icon: Rect) {
    let Some(panel) = app.get_webview_window(PANEL) else {
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    let scale = panel.scale_factor().unwrap_or(2.0);
    let position = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let x = position.x + size.width / 2.0 - PANEL_WIDTH * scale / 2.0;
    let y = position.y + size.height + 6.0 * scale;
    let _ = panel.set_position(PhysicalPosition::new(x.max(0.0) as i32, y as i32));
    let _ = panel.show();
    let _ = panel.set_focus();
    let _ = panel.emit(PANEL_EVENT, ());
}

/// Whether the window is on screen.
#[tauri::command]
pub fn window_visible(app: AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    show_window(&app);
}

/// Shows the window on its Settings page.
#[tauri::command]
pub fn open_settings(app: AppHandle) {
    show_window(&app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit(SETTINGS_EVENT, ());
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn hide_menu_panel(app: AppHandle) {
    hide_panel(&app);
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
    result.map_err(|e| format!("Could not change Open at login: {e}"))?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

/// Builds the menu bar icon and its (hidden) panel.
pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let panel = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("tray.html".into()))
        .title("Webcam Studio")
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(true)
        .effects(WindowEffectsConfig {
            effects: vec![Effect::Popover],
            state: Some(EffectState::Active),
            radius: Some(12.0),
            color: None,
        })
        .build()?;
    let _ = panel.set_size(LogicalSize::new(PANEL_WIDTH, PANEL_HEIGHT));
    // Like a menu: clicking anywhere else closes it.
    let handle = app.handle().clone();
    panel.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            hide_panel(&handle);
        }
    });

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("webcam-studio")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Webcam Studio")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left | MouseButton::Right,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle(), rect);
            }
        })
        .build(app)?;
    Ok(())
}
