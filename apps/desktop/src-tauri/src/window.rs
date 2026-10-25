//! Window chrome and a small, dependency-free record of the last normal window bounds.

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow, WindowEvent,
};

const REPOSITORY: &str = "https://github.com/pankajyadav05/webcam-studio";

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Bounds {
    x: i32,
    y: i32,
    width: f64,
    height: f64,
}

pub struct WindowState {
    path: PathBuf,
    bounds: Mutex<Option<Bounds>>,
}

impl WindowState {
    fn capture(&self, window: &WebviewWindow) {
        if window.is_fullscreen().unwrap_or(false)
            || window.is_maximized().unwrap_or(false)
            || window.is_minimized().unwrap_or(false)
        {
            return;
        }
        if let (Ok(position), Ok(size), Ok(scale)) = (
            window.outer_position(),
            window.inner_size(),
            window.scale_factor(),
        ) {
            let size = size.to_logical::<f64>(scale);
            if size.width >= 900.0 && size.height >= 600.0 {
                *self.bounds.lock().unwrap() = Some(Bounds {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                });
            }
        }
    }

    pub fn save(&self) {
        let Some(bounds) = *self.bounds.lock().unwrap() else {
            return;
        };
        // Save on close/quit, rather than writing to disk throughout a drag.
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let temporary = self.path.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_vec(&bounds)?)?;
            std::fs::rename(temporary, &self.path)?;
            Ok(())
        })();
        if let Err(error) = result {
            log::warn!("Could not remember window bounds: {error}");
        }
    }
}

fn restore(window: &WebviewWindow, bounds: Bounds) -> tauri::Result<()> {
    if !bounds.width.is_finite() || !bounds.height.is_finite() {
        return Ok(());
    }
    let monitors = window.available_monitors()?;
    let monitor = monitors
        .iter()
        .find(|monitor| {
            let area = monitor.work_area();
            let right = i64::from(area.position.x) + i64::from(area.size.width);
            let bottom = i64::from(area.position.y) + i64::from(area.size.height);
            i64::from(bounds.x) >= i64::from(area.position.x)
                && i64::from(bounds.x) < right
                && i64::from(bounds.y) >= i64::from(area.position.y)
                && i64::from(bounds.y) < bottom
        })
        .or_else(|| monitors.first());
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let width = bounds
        .width
        .clamp(900.0, (f64::from(area.size.width) / scale).max(900.0));
    let height = bounds
        .height
        .clamp(600.0, (f64::from(area.size.height) / scale).max(600.0));
    // A disconnected monitor must never leave the title bar out of reach.
    let max_x = (i64::from(area.position.x) + i64::from(area.size.width) - (width * scale) as i64)
        .max(i64::from(area.position.x)) as i32;
    let max_y = (i64::from(area.position.y) + i64::from(area.size.height) - (height * scale) as i64)
        .max(i64::from(area.position.y)) as i32;
    window.set_position(PhysicalPosition::new(
        bounds.x.clamp(area.position.x, max_x),
        bounds.y.clamp(area.position.y, max_y),
    ))?;
    window.set_size(LogicalSize::new(width, height))
}

#[derive(Serialize)]
pub struct ChromeInfo {
    platform: &'static str,
    theme: String,
    fullscreen: bool,
}

#[tauri::command]
pub fn window_chrome(window: WebviewWindow) -> tauri::Result<ChromeInfo> {
    Ok(ChromeInfo {
        platform: std::env::consts::OS,
        theme: window.theme()?.to_string().to_lowercase(),
        fullscreen: window.is_fullscreen()?,
    })
}

#[tauri::command]
pub fn toggle_fullscreen(window: WebviewWindow) -> tauri::Result<bool> {
    let fullscreen = !window.is_fullscreen()?;
    window.set_fullscreen(fullscreen)?;
    Ok(fullscreen)
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window is missing")?;
    // This affects this process only. An unset override follows live system appearance.
    let theme = match std::env::var("WEBCAM_STUDIO_APPEARANCE").as_deref() {
        Ok("light") => Some(tauri::Theme::Light),
        Ok("dark") => Some(tauri::Theme::Dark),
        _ => None,
    };
    window.set_theme(theme)?;
    let path = app.path().app_config_dir()?.join("window-bounds.json");
    let bounds = std::fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok());
    if let Some(bounds) = bounds {
        if let Err(error) = restore(&window, bounds) {
            log::warn!("Could not restore window bounds: {error}");
        }
    }
    let state = WindowState {
        path,
        bounds: Mutex::new(None),
    };
    state.capture(&window);
    app.manage(state);
    menu(app)?;
    window.show()?;
    Ok(())
}

pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let Some(main) = window.app_handle().get_webview_window(window.label()) else {
        return;
    };
    let Some(state) = window.try_state::<WindowState>() else {
        return;
    };
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            state.capture(&main);
            let _ = main.emit("window-chrome-changed", ());
        }
        WindowEvent::CloseRequested { .. } => state.save(),
        WindowEvent::ThemeChanged(theme) => {
            let _ = main.emit("window-theme-changed", theme.to_string().to_lowercase());
        }
        _ => {}
    }
}

fn menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{AboutMetadata, MenuBuilder, PredefinedMenuItem, SubmenuBuilder};
    let about = AboutMetadata {
        name: Some("Webcam Studio".into()),
        version: Some(app.package_info().version.to_string()),
        short_version: Some(String::new()),
        copyright: Some("GPL-3.0-or-later".into()),
        icon: image::load_from_memory(include_bytes!("../icons/128x128@2x.png"))
            .ok()
            .map(|image| {
                let rgba = image.to_rgba8();
                let (width, height) = rgba.dimensions();
                tauri::image::Image::new_owned(rgba.into_raw(), width, height)
            }),
        // AppKit reads the linked, formatted credits from the bundled Credits.html.
        ..Default::default()
    };
    let application = SubmenuBuilder::new(app, "Webcam Studio")
        .item(&PredefinedMenuItem::about(
            app,
            Some("About Webcam Studio"),
            Some(about),
        )?)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .copy()
        .select_all()
        .build()?;
    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .close_window()
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .text("repository", "Webcam Studio on GitHub")
        .build()?;
    app.set_menu(
        MenuBuilder::new(app)
            .items(&[&application, &edit, &view, &window, &help])
            .build()?,
    )?;
    app.on_menu_event(|_app: &AppHandle, event| {
        if event.id().as_ref() == "repository" {
            #[cfg(target_os = "macos")]
            let mut command = std::process::Command::new("open");
            #[cfg(target_os = "linux")]
            let mut command = std::process::Command::new("xdg-open");
            #[cfg(target_os = "windows")]
            let mut command = {
                let mut command = std::process::Command::new("cmd");
                command.args(["/C", "start", ""]);
                command
            };
            if let Err(error) = command.arg(REPOSITORY).spawn() {
                log::warn!("Could not open project help: {error}");
            }
        }
    });
    Ok(())
}
