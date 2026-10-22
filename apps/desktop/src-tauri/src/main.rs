//! Webcam Studio desktop shell: a small window that edits the engine settings and runs the
//! camera pipeline.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod extension;
mod image_load;
mod pipeline;
mod state;
mod vcam;

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

#[tauri::command]
fn platform_info(state: State<'_, AppState>) -> PlatformInfo {
    state.platform_info()
}

#[tauri::command]
async fn list_native_cameras(state: State<'_, AppState>) -> Result<Vec<CameraInfo>, String> {
    state.list_native_cameras()
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
fn emitter(app: &AppHandle) -> Arc<dyn Fn(ExtensionEvent) + Send + Sync> {
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
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            platform_info,
            list_native_cameras,
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
            // Leave the virtual camera tidy: meeting apps then show the placeholder.
            if let RunEvent::Exit = event {
                app.state::<AppState>().stop_virtual_camera();
            }
        });
}
