//! Webcam Studio desktop shell: a small window that edits the engine settings.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod state;

use state::{AppState, PlatformInfo};
use studio_core::settings::Settings;
use tauri::State;

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.get_settings()
}

#[tauri::command]
fn set_settings(state: State<'_, AppState>, settings: Settings) -> Settings {
    state.set_settings(settings)
}

#[tauri::command]
fn platform_info(state: State<'_, AppState>) -> PlatformInfo {
    state.platform_info()
}

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            platform_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running Webcam Studio");
}
