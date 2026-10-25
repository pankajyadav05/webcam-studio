//! Webcam Studio desktop shell: a small window that edits the engine settings and runs the
//! camera pipeline.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auto_camera;
mod background;
mod extension;
mod image_load;
mod logfile;
mod pipeline;
mod samples;
mod segmentation;
mod state;
mod vcam;
mod window;

use std::sync::Arc;

use extension::{EVENT_NAME, ExtensionEvent};
use state::{AppState, PlatformInfo};
use studio_capture::CameraInfo;
use studio_core::settings::Settings;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use vcam::VcamStatus;

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.get_settings()
}

// Async so that loading a background picture does not block the window.
#[tauri::command]
async fn set_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    state.set_settings(settings)
}

/// The window's Replace background for the virtual camera. Body: PNG or JPEG bytes; the
/// `key` header names the picture (it starts with "window:").
#[tauri::command]
fn set_window_picture(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("set_window_picture needs the picture bytes".into());
    };
    let key = request
        .headers()
        .get("key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    state.set_window_picture(key, bytes)
}

/// The real camera meeting frames come from, by the name the window shows.
#[tauri::command]
fn set_meeting_camera(camera: State<'_, auto_camera::MeetingCamera>, name: String) {
    camera.set(name);
}

#[tauri::command]
fn platform_info(state: State<'_, AppState>) -> PlatformInfo {
    state.platform_info()
}

#[tauri::command]
async fn list_native_cameras(state: State<'_, AppState>) -> Result<Vec<CameraInfo>, String> {
    state.list_native_cameras()
}

/// Whether this app can offer the virtual camera at all: it needs a system that has one and
/// an app that carries the extension. The window hides every virtual camera control otherwise.
#[tauri::command]
fn meetings_available() -> bool {
    vcam::is_supported() && vcam::extension_bundled()
}

/// Opens the System Settings page where the camera or the microphone is allowed.
#[tauri::command]
fn open_privacy_settings(kind: String) -> Result<(), String> {
    let url = vcam::privacy_settings_url(&kind)
        .ok_or_else(|| format!("There is no privacy page called {kind}."))?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(drop)
            .map_err(|e| format!("Could not open System Settings: {e}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err("This system has no privacy page to open.".to_string())
    }
}

#[tauri::command]
fn virtual_camera_status(state: State<'_, AppState>) -> VcamStatus {
    state.virtual_camera_status()
}

/// Runs `work` on a thread meant for blocking, so the window and the async runtime stay free.
async fn off_the_window_thread<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|e| format!("The camera task did not finish: {e}"))
}

// Waits for the camera permission answer, which can take a while.
#[tauri::command]
async fn start_virtual_camera(app: AppHandle, camera_id: String) -> Result<VcamStatus, String> {
    off_the_window_thread(app, move |state| state.start_virtual_camera(&camera_id)).await?
}

#[tauri::command]
async fn stop_virtual_camera(app: AppHandle) -> Result<VcamStatus, String> {
    off_the_window_thread(app, |state| state.stop_virtual_camera()).await
}

/// Builds the function that sends install news to the window.
pub(crate) fn emitter(app: &AppHandle) -> Arc<dyn Fn(ExtensionEvent) + Send + Sync> {
    let app = app.clone();
    Arc::new(move |event| {
        if let Err(e) = app.emit(EVENT_NAME, event) {
            log::warn!("Could not tell the window about the camera extension: {e}");
        }
    })
}

// Answers arrive later as `camera-extension` events. macOS runs the request on the main
// dispatch queue, which the Tauri event loop keeps serviced.
#[tauri::command]
fn install_camera_extension(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    extension::install(state.extension(), emitter(&app))
}

#[tauri::command]
async fn uninstall_camera_extension(app: AppHandle) -> Result<(), String> {
    // The camera must be off before its extension goes away.
    let extension = off_the_window_thread(app.clone(), |state| {
        state.stop_virtual_camera();
        state.extension()
    })
    .await?;
    extension::uninstall(extension, emitter(&app))
}

#[tauri::command]
fn open_extension_settings() -> Result<(), String> {
    extension::open_settings()
}

fn main() {
    logfile::init();
    log::info!("Webcam Studio {} started", env!("CARGO_PKG_VERSION"));
    tauri::Builder::default()
        .manage(AppState::new())
        .manage(segmentation::Segmentation::new())
        .manage(samples::Samples::default())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![background::HIDDEN_ARG]),
        ))
        .manage(auto_camera::MeetingCamera::default())
        .setup(|app| {
            let switch = app.path().app_config_dir()?.join("meeting-camera.json");
            app.manage(auto_camera::MeetingSwitch::load(switch));
            window::setup(app)?;
            background::setup_tray(app)?;
            auto_camera::spawn(app.handle().clone());
            Ok(())
        })
        .on_window_event(window::on_window_event)
        .invoke_handler(tauri::generate_handler![
            window::window_chrome,
            window::toggle_fullscreen,
            samples::sample_manifest,
            samples::sample_clip,
            segmentation::segment_person,
            segmentation::segmenter_info,
            segmentation::segmentation_diagnostics,
            get_settings,
            set_settings,
            platform_info,
            list_native_cameras,
            meetings_available,
            set_window_picture,
            set_meeting_camera,
            background::window_visible,
            background::open_at_login,
            background::set_open_at_login,
            background::show_main_window,
            background::open_settings,
            background::quit_app,
            background::hide_menu_panel,
            background::fit_menu_panel,
            auto_camera::meeting_camera_enabled,
            auto_camera::set_meeting_camera_enabled,
            open_privacy_settings,
            virtual_camera_status,
            start_virtual_camera,
            stop_virtual_camera,
            install_camera_extension,
            uninstall_camera_extension,
            open_extension_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Webcam Studio")
        .run(|app, event| {
            // Clicking the app in Finder or the Dock while it sits in the menu bar.
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                background::show_window(app);
            }
            // Leave the virtual camera tidy: meeting apps then show the placeholder.
            if let RunEvent::ExitRequested { .. } | RunEvent::Exit = event {
                app.state::<window::WindowState>().save();
                app.state::<AppState>().stop_virtual_camera();
            }
        });
}
