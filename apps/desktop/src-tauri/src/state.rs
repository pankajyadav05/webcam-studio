//! App state and command logic, kept free of any window code so it can be tested.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use studio_capture::CameraInfo;
use studio_core::frame::VideoFrame;
use studio_core::settings::{BackgroundMode, Settings};
use studio_engine::Engine;

use crate::extension::ExtensionTracker;
use crate::image_load::load_background;
use crate::pipeline::Pipeline;
use crate::vcam::{self, Facts, UNSUPPORTED_MESSAGE, VcamStatus};

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
    /// Shared with the pipeline's worker thread, which locks it once per frame.
    engine: Arc<Mutex<Engine>>,
    /// Path of the replacement picture the engine holds now. Locked for the whole of
    /// `set_settings`, so two changes cannot load pictures at the same time.
    replacement_path: Mutex<Option<String>>,
    pipeline: Arc<Pipeline>,
    extension: Arc<ExtensionTracker>,
}

impl AppState {
    /// Creates the state with an engine that uses default settings.
    pub fn new() -> Self {
        Self::with_settings(Settings::default())
    }

    /// Creates the state with an engine that starts from the given settings.
    pub fn with_settings(settings: Settings) -> Self {
        let engine = Arc::new(Mutex::new(Engine::new(settings)));
        Self {
            pipeline: Arc::new(Pipeline::new(Arc::clone(&engine))),
            engine,
            replacement_path: Mutex::new(None),
            extension: Arc::new(ExtensionTracker::new()),
        }
    }

    fn engine(&self) -> MutexGuard<'_, Engine> {
        // A panic in another thread must not lock the settings out for good.
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The tracker of install news, shared with the macOS callbacks.
    pub fn extension(&self) -> Arc<ExtensionTracker> {
        Arc::clone(&self.extension)
    }

    /// Returns the current settings.
    pub fn get_settings(&self) -> Settings {
        self.engine().settings().clone()
    }

    /// Applies new settings and returns what the engine now holds.
    /// The engine limits out-of-range framing values, so the result can differ from the input.
    ///
    /// When the background is Replace and its picture changed, the picture is loaded first
    /// (without holding the engine, so the camera keeps flowing). If that fails, every other
    /// setting is still applied, the previous picture stays, and the error is returned. The
    /// picture is cleared when the mode is not Replace.
    pub fn set_settings(&self, settings: Settings) -> Result<Settings, String> {
        let mut loaded = self
            .replacement_path
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        let wanted = match &settings.background {
            BackgroundMode::Replace { image_path } if !image_path.trim().is_empty() => {
                Some(image_path.clone())
            }
            _ => None,
        };

        // `Some(picture)` replaces what the engine holds; `None` leaves it alone.
        let mut new_picture: Option<Option<VideoFrame>> = None;
        let mut image_error = None;
        if wanted != *loaded {
            match &wanted {
                None => {
                    new_picture = Some(None);
                    *loaded = None;
                }
                Some(path) => match load_background(path) {
                    Ok(frame) => {
                        new_picture = Some(Some(frame));
                        *loaded = wanted.clone();
                    }
                    Err(message) => image_error = Some(message),
                },
            }
        }

        let applied = {
            let mut engine = self.engine();
            engine.update_settings(settings);
            if let Some(picture) = new_picture {
                engine.set_replacement_image(picture);
            }
            engine.settings().clone()
        };
        match image_error {
            Some(message) => Err(message),
            None => Ok(applied),
        }
    }

    /// Returns the operating system and camera backend names.
    pub fn platform_info(&self) -> PlatformInfo {
        PlatformInfo {
            os: std::env::consts::OS.to_string(),
            camera_backend: studio_vdevice::backend_name().to_string(),
        }
    }

    /// Lists the real cameras, never the virtual one.
    pub fn list_native_cameras(&self) -> Result<Vec<CameraInfo>, String> {
        studio_capture::list_cameras()
            .map_err(|e| format!("Could not list the cameras: {}", vcam::describe(&e)))
    }

    /// What the window shows about the virtual camera right now.
    pub fn virtual_camera_status(&self) -> VcamStatus {
        let supported = vcam::is_supported();
        let pipeline = self.pipeline.snapshot();
        let extension = self.extension.phase();
        vcam::compute_status(&Facts {
            supported,
            installed: supported && vcam::extension_installed(),
            extension: &extension,
            pipeline: &pipeline,
        })
    }

    /// Starts the virtual camera with the real camera `camera_id`. Blocks until it runs or
    /// fails, which can take as long as the person needs to answer the permission question.
    pub fn start_virtual_camera(&self, camera_id: &str) -> Result<VcamStatus, String> {
        if !vcam::is_supported() {
            return Err(UNSUPPORTED_MESSAGE.to_string());
        }
        if !vcam::extension_installed() {
            return Err(
                "The camera is not installed yet. Use the setup steps above to install it."
                    .to_string(),
            );
        }
        self.pipeline.start(camera_id)?;
        Ok(self.virtual_camera_status())
    }

    /// Stops the virtual camera and clears an earlier error.
    pub fn stop_virtual_camera(&self) -> VcamStatus {
        self.pipeline.stop();
        self.virtual_camera_status()
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
    use studio_core::settings::{Framing, NoiseMode};

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
        let returned = state.set_settings(wanted.clone()).unwrap();
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
        let returned = state.set_settings(wanted).unwrap();
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

    fn replace(path: &str) -> Settings {
        Settings {
            background: BackgroundMode::Replace {
                image_path: path.to_string(),
            },
            ..Settings::default()
        }
    }

    /// A small PNG in the temp folder, removed at the end of the test.
    struct Png(std::path::PathBuf);

    impl Png {
        fn new(name: &str, width: u32, height: u32) -> Self {
            let path = std::env::temp_dir().join(format!(
                "webcam-studio-state-{}-{name}.png",
                std::process::id()
            ));
            image::RgbaImage::from_pixel(width, height, image::Rgba([9, 8, 7, 255]))
                .save(&path)
                .unwrap();
            Self(path)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for Png {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn replace_loads_the_picture_once_and_clears_it_when_the_mode_changes() {
        let state = AppState::new();
        let png = Png::new("load", 4, 4);

        let applied = state.set_settings(replace(png.path())).unwrap();
        assert_eq!(applied, replace(png.path()));
        assert_eq!(
            *state.replacement_path.lock().unwrap(),
            Some(png.path().to_string())
        );

        // Removing the file does not matter while the path is unchanged: nothing is reloaded.
        std::fs::remove_file(png.path()).unwrap();
        let mut again = replace(png.path());
        again.framing.zoom = 2.0;
        assert!(state.set_settings(again).is_ok());

        // Leaving Replace clears the picture.
        state.set_settings(Settings::default()).unwrap();
        assert_eq!(*state.replacement_path.lock().unwrap(), None);
    }

    #[test]
    fn a_bad_picture_reports_the_error_but_applies_the_other_settings() {
        let state = AppState::new();
        let mut wanted = replace("/definitely/not/here.png");
        wanted.framing.zoom = 3.0;
        let error = state.set_settings(wanted).unwrap_err();
        assert_eq!(error, "Could not open that image: the file was not found.");
        assert_eq!(state.get_settings().framing.zoom, 3.0);
        assert_eq!(*state.replacement_path.lock().unwrap(), None);
    }

    #[test]
    fn a_bad_picture_keeps_the_previous_one() {
        let state = AppState::new();
        let good = Png::new("keep", 4, 4);
        state.set_settings(replace(good.path())).unwrap();

        let error = state.set_settings(replace("/nope.png")).unwrap_err();
        assert!(error.starts_with("Could not open that image:"));
        // The old picture is still the one the engine holds, so a retry with it is a no-op.
        assert_eq!(
            *state.replacement_path.lock().unwrap(),
            Some(good.path().to_string())
        );
        assert!(state.set_settings(replace(good.path())).is_ok());
    }

    #[test]
    fn an_empty_picture_path_is_not_an_error() {
        let state = AppState::new();
        assert!(state.set_settings(replace("")).is_ok());
        assert_eq!(*state.replacement_path.lock().unwrap(), None);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn the_virtual_camera_is_unsupported_off_macos() {
        use crate::vcam::VcamState;

        let state = AppState::new();
        let status = state.virtual_camera_status();
        assert_eq!(status.state, VcamState::Unsupported);
        assert_eq!(status.message, UNSUPPORTED_MESSAGE);
        assert_eq!(
            state.start_virtual_camera("cam"),
            Err(UNSUPPORTED_MESSAGE.to_string())
        );
        assert_eq!(state.stop_virtual_camera().state, VcamState::Unsupported);
        assert!(state.list_native_cameras().is_err());
    }
}
