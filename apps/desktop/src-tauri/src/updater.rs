//! Updates, as in Clippy: look for a new release now and then, download it and check its
//! signature in the background, and install it only when the person clicks Restart to Update.
//! A meeting may be using the camera, so nothing restarts on its own.
//!
//! The feed (`latest.json`) and the packages live in the `webcam-studio` R2 bucket; the
//! release workflow writes them. Only the signed app bundle updates itself: a plain
//! `cargo run` has no extension inside and never looks.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Tells the window and the menu bar panel the update status changed.
pub const EVENT: &str = "update-status";

/// The first look waits until the app has settled; then once an hour.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum UpdateStatus {
    /// This build does not update itself (not the signed app bundle).
    Disabled,
    /// Up to date, as far as the last look knows.
    Idle,
    Checking,
    Downloading {
        version: String,
    },
    /// Downloaded and verified; installs on Restart to Update.
    Ready {
        version: String,
    },
    /// The app runs from a disk image or a quarantined copy and cannot replace itself.
    MoveToApplications {
        version: String,
    },
    Failed {
        message: String,
    },
}

pub enum Event {
    Checking,
    NoUpdate,
    Found(UpdateStatus),
    Failed(String),
}

/// The status after `event`. A downloaded update stays ready until it is installed.
pub fn next_status(current: &UpdateStatus, event: Event) -> UpdateStatus {
    if matches!(current, UpdateStatus::Ready { .. } | UpdateStatus::Disabled) {
        return current.clone();
    }
    match event {
        Event::Checking => UpdateStatus::Checking,
        Event::NoUpdate => UpdateStatus::Idle,
        Event::Found(status) => status,
        Event::Failed(message) => UpdateStatus::Failed { message },
    }
}

/// Whether the app can replace itself from where it runs. macOS runs apps opened from a
/// disk image, or straight from Downloads, from a read-only or translocated copy.
pub fn can_replace_itself(exe: &str) -> bool {
    !(exe.contains("/AppTranslocation/") || exe.starts_with("/Volumes/"))
}

struct Inner {
    status: UpdateStatus,
    pending: Option<(Update, Vec<u8>)>,
}

pub struct UpdaterState {
    inner: Mutex<Inner>,
    busy: tokio::sync::Mutex<()>,
}

impl UpdaterState {
    fn status(&self) -> UpdateStatus {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .status
            .clone()
    }
}

fn publish(app: &AppHandle, event: Event) -> UpdateStatus {
    let state = app.state::<UpdaterState>();
    let status = {
        let mut inner = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.status = next_status(&inner.status, event);
        inner.status.clone()
    };
    let _ = app.emit(EVENT, &status);
    status
}

fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

async fn check(app: &AppHandle) -> UpdateStatus {
    let state = app.state::<UpdaterState>();
    let Ok(_busy) = state.busy.try_lock() else {
        return state.status();
    };
    if matches!(
        state.status(),
        UpdateStatus::Ready { .. } | UpdateStatus::Disabled
    ) {
        return state.status();
    }
    publish(app, Event::Checking);
    let result: Result<Event, String> = async {
        let updater = app
            .updater_builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
            return Ok(Event::NoUpdate);
        };
        let version = update.version.clone();
        if !can_replace_itself(&exe_path()) {
            return Ok(Event::Found(UpdateStatus::MoveToApplications { version }));
        }
        log::info!("Downloading Webcam Studio {version}");
        publish(
            app,
            Event::Found(UpdateStatus::Downloading {
                version: version.clone(),
            }),
        );
        let bytes = update
            .download(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        state
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending = Some((update, bytes));
        log::info!("Webcam Studio {version} is ready to install");
        Ok(Event::Found(UpdateStatus::Ready { version }))
    }
    .await;
    publish(
        app,
        result.unwrap_or_else(|message| {
            log::warn!("Update check failed: {message}");
            Event::Failed(message)
        }),
    )
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    app.state::<UpdaterState>().status()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> UpdateStatus {
    check(&app).await
}

/// Installs the downloaded update and starts the new version.
#[tauri::command]
pub async fn restart_to_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UpdaterState>();
    let _busy = state
        .busy
        .try_lock()
        .map_err(|_| "An update is already in progress.".to_string())?;
    let (update, bytes) = state
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .pending
        .take()
        .ok_or("No update is ready to install.")?;
    let version = update.version.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || update.install(&bytes))
        .await
        .map_err(|e| e.to_string())?;
    if let Err(e) = installed {
        let message = format!("Could not install the update: {e}");
        state.inner.lock().unwrap_or_else(|e| e.into_inner()).status = UpdateStatus::Failed {
            message: message.clone(),
        };
        let _ = app.emit(EVENT, state.status());
        return Err(message);
    }
    log::info!("Installed Webcam Studio {version}; restarting");
    app.state::<crate::window::WindowState>().save();
    app.state::<crate::state::AppState>().stop_virtual_camera();
    app.restart();
}

/// Registers the state and, for the signed app bundle, starts the hourly look.
pub fn initialize(app: &AppHandle) {
    let enabled = cfg!(target_os = "macos") && crate::vcam::extension_bundled();
    app.manage(UpdaterState {
        inner: Mutex::new(Inner {
            status: if enabled {
                UpdateStatus::Idle
            } else {
                UpdateStatus::Disabled
            },
            pending: None,
        }),
        busy: tokio::sync::Mutex::new(()),
    });
    if !enabled {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(&handle).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> UpdateStatus {
        UpdateStatus::Ready {
            version: "0.2.0".into(),
        }
    }

    #[test]
    fn a_check_moves_through_checking_to_idle_or_found() {
        let s = next_status(&UpdateStatus::Idle, Event::Checking);
        assert_eq!(s, UpdateStatus::Checking);
        assert_eq!(next_status(&s, Event::NoUpdate), UpdateStatus::Idle);
        assert_eq!(next_status(&s, Event::Found(ready())), ready());
        assert_eq!(
            next_status(&s, Event::Failed("offline".into())),
            UpdateStatus::Failed {
                message: "offline".into()
            }
        );
    }

    #[test]
    fn a_ready_update_waits_for_the_restart() {
        assert_eq!(next_status(&ready(), Event::Checking), ready());
        assert_eq!(next_status(&ready(), Event::Failed("x".into())), ready());
    }

    #[test]
    fn a_build_without_updates_stays_disabled() {
        assert_eq!(
            next_status(&UpdateStatus::Disabled, Event::Checking),
            UpdateStatus::Disabled
        );
    }

    #[test]
    fn copies_on_disk_images_or_translocated_cannot_replace_themselves() {
        assert!(can_replace_itself(
            "/Applications/Webcam Studio.app/Contents/MacOS/webcam-studio-desktop"
        ));
        assert!(!can_replace_itself(
            "/Volumes/Webcam Studio/Webcam Studio.app/Contents/MacOS/webcam-studio-desktop"
        ));
        assert!(!can_replace_itself(
            "/private/var/folders/x/AppTranslocation/ABC/d/Webcam Studio.app/Contents/MacOS/w"
        ));
    }

    #[test]
    fn the_status_names_match_what_the_window_expects() {
        let json = serde_json::to_value(UpdateStatus::Downloading {
            version: "1.0.0".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "downloading", "version": "1.0.0"})
        );
    }
}
