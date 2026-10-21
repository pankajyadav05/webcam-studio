//! App state and command logic, kept free of any window code so it can be tested.

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use studio_core::settings::Settings;
use studio_engine::Engine;

/// Facts about the machine the app runs on, shown at the bottom of the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlatformInfo {
    /// Operating system name, such as "linux", "macos" or "windows".
    pub os: String,
    /// Name of the virtual camera backend compiled into this build.
    pub camera_backend: String,
}

/// State shared by all commands. Holds the engine behind a lock.
pub struct AppState {
    engine: Mutex<Engine>,
}

impl AppState {
    /// Creates the state with an engine that uses default settings.
    pub fn new() -> Self {
        Self::with_settings(Settings::default())
    }

    /// Creates the state with an engine that starts from the given settings.
    pub fn with_settings(settings: Settings) -> Self {
        Self {
            engine: Mutex::new(Engine::new(settings)),
        }
    }

    fn engine(&self) -> MutexGuard<'_, Engine> {
        // A panic in another thread must not lock the settings out for good.
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Returns the current settings.
    pub fn get_settings(&self) -> Settings {
        self.engine().settings().clone()
    }

    /// Applies new settings and returns what the engine now holds.
    /// The engine limits out-of-range framing values, so the result can differ from the input.
    pub fn set_settings(&self, settings: Settings) -> Settings {
        let mut engine = self.engine();
        engine.update_settings(settings);
        engine.settings().clone()
    }

    /// Returns the operating system and camera backend names.
    pub fn platform_info(&self) -> PlatformInfo {
        PlatformInfo {
            os: std::env::consts::OS.to_string(),
            camera_backend: studio_vdevice::backend_name().to_string(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::settings::{BackgroundMode, Framing, NoiseMode};

    #[test]
    fn starts_with_default_settings() {
        let state = AppState::new();
        assert_eq!(state.get_settings(), Settings::default());
    }

    #[test]
    fn settings_round_trip() {
        let state = AppState::new();
        let wanted = Settings {
            framing: Framing {
                zoom: 2.5,
                pan_x: 0.25,
                pan_y: -0.5,
                mirror: true,
                auto_frame: false,
            },
            background: BackgroundMode::Blur { radius: 12 },
            noise: NoiseMode::Light,
            ..Settings::default()
        };
        let returned = state.set_settings(wanted.clone());
        assert_eq!(returned, wanted);
        assert_eq!(state.get_settings(), wanted);
    }

    #[test]
    fn out_of_range_framing_is_clamped() {
        let state = AppState::new();
        let wanted = Settings {
            framing: Framing {
                zoom: 10.0,
                pan_x: -5.0,
                pan_y: 5.0,
                ..Framing::default()
            },
            ..Settings::default()
        };
        let returned = state.set_settings(wanted);
        assert_eq!(returned.framing.zoom, 4.0);
        assert_eq!(returned.framing.pan_x, -1.0);
        assert_eq!(returned.framing.pan_y, 1.0);
        assert_eq!(state.get_settings(), returned);
    }

    #[test]
    fn platform_info_names_the_os() {
        let info = AppState::new().platform_info();
        assert_eq!(info.os, std::env::consts::OS);
        assert!(!info.camera_backend.is_empty());
    }
}
