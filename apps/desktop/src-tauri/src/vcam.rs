//! What the virtual camera status block of the window says, worked out from plain facts.
//!
//! Nothing here touches a window, a camera or the system, so every case can be tested.

use serde::Serialize;
use studio_core::error::StudioError;

use crate::extension::ExtensionPhase;
use crate::pipeline::{Phase, Snapshot};

/// Shown on every system that has no virtual camera backend yet.
pub const UNSUPPORTED_MESSAGE: &str =
    "The virtual camera is built for macOS first. Linux and Windows support is in progress.";

/// The coarse state of the virtual camera, as the window shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VcamState {
    /// This operating system has no virtual camera yet.
    Unsupported,
    /// The camera extension is not installed.
    NotInstalled,
    /// The extension was sent to macOS and waits for the user to allow it.
    NeedsApproval,
    /// The extension is installed and the camera can be started.
    Ready,
    /// Frames are going to the virtual camera.
    Running,
    /// The last start or run failed. The message says why.
    Error,
}

/// The status the window polls for.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VcamStatus {
    /// Which of the six states applies.
    pub state: VcamState,
    /// One or two plain sentences for the person using the app.
    pub message: String,
    /// Frames per second sent in the last second. Zero when not running.
    pub fps: f32,
    /// Frames sent to the virtual camera since it was started.
    pub frames_sent: u64,
    /// Frames that were lost since it was started.
    pub frames_dropped: u64,
    /// Name of the real camera in use, while running.
    pub camera_name: Option<String>,
}

/// Facts that decide the status.
pub struct Facts<'a> {
    /// Whether this operating system has a virtual camera backend at all.
    pub supported: bool,
    /// Whether macOS lists the camera extension's device.
    pub installed: bool,
    /// What happened to the last install request.
    pub extension: &'a ExtensionPhase,
    /// What the worker thread is doing.
    pub pipeline: &'a Snapshot,
}

/// Turns the facts into the status.
///
/// Order matters: a running or failed pipeline is reported first, because it is the newest
/// and most specific news. Then the install state decides between the setup steps and ready.
pub fn compute_status(facts: &Facts<'_>) -> VcamStatus {
    let p = facts.pipeline;
    let status = |state: VcamState, message: &str| VcamStatus {
        state,
        message: message.to_string(),
        fps: if state == VcamState::Running {
            p.fps
        } else {
            0.0
        },
        frames_sent: p.frames_sent,
        frames_dropped: p.frames_dropped,
        camera_name: p.camera_name.clone(),
    };

    if !facts.supported {
        return VcamStatus {
            camera_name: None,
            frames_sent: 0,
            frames_dropped: 0,
            ..status(VcamState::Unsupported, UNSUPPORTED_MESSAGE)
        };
    }

    match p.phase {
        Phase::Running => {
            let message = match &p.message {
                Some(problem) => format!(
                    "The virtual camera is on, but some pictures were not delivered: {problem}"
                ),
                None => {
                    "The virtual camera is on. Choose Webcam Studio Camera in your meeting app."
                        .to_string()
                }
            };
            return status(VcamState::Running, &message);
        }
        Phase::Starting => return status(VcamState::Ready, "Starting the camera…"),
        Phase::Failed => {
            let message = p
                .message
                .as_deref()
                .unwrap_or("The virtual camera stopped because of an error.");
            return status(VcamState::Error, message);
        }
        Phase::Idle => {}
    }

    if facts.installed {
        return status(
            VcamState::Ready,
            "Ready. Start it, then choose Webcam Studio Camera in your meeting app.",
        );
    }
    match facts.extension {
        ExtensionPhase::AwaitingApproval => status(
            VcamState::NeedsApproval,
            "In System Settings, under Camera Extensions, turn on Webcam Studio. Then come back here.",
        ),
        ExtensionPhase::RebootPending => status(
            VcamState::NotInstalled,
            "Restart your Mac to finish installing the camera.",
        ),
        ExtensionPhase::Failed(problem) => status(VcamState::NotInstalled, problem),
        ExtensionPhase::Idle => status(
            VcamState::NotInstalled,
            "The camera is not installed yet. Install it, then allow it in System Settings.",
        ),
    }
}

/// Whether macOS lists the camera extension's device. Always `false` where there is none.
pub fn extension_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        studio_vdevice::macos::is_extension_installed()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Whether the running app carries a camera extension that could be installed.
///
/// A plain `cargo run` or an unsigned build has none, and then installing can never work, so
/// the window should not offer it. Only the app bundle layout is checked: the extension
/// lives in `Contents/Library/SystemExtensions/*.systemextension`.
pub fn extension_bundled() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(std::path::Path::to_path_buf))
            .is_some_and(|contents| extension_bundled_in(&contents))
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Looks for an extension bundle inside the `Contents` folder of an app. Only macOS
/// builds look; the tests cover it everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn extension_bundled_in(contents: &std::path::Path) -> bool {
    std::fs::read_dir(contents.join("Library/SystemExtensions")).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|e| e == "systemextension")
                && entry.path().is_dir()
        })
    })
}

/// The System Settings address for a privacy page. Only fixed pages are allowed, so the
/// window cannot make the app open anything else.
pub fn privacy_settings_url(kind: &str) -> Option<&'static str> {
    match kind {
        "camera" => Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Camera"),
        "microphone" => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        }
        _ => None,
    }
}

/// Whether this operating system has a virtual camera backend.
pub const fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

/// Plain-language text for an error from one of the device crates.
pub fn describe(error: &StudioError) -> String {
    match error {
        StudioError::Unsupported(why) => format!("not available on this system yet ({why})"),
        StudioError::Backend(message) | StudioError::Invalid(message) => message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_without_the_extension_cannot_offer_the_virtual_camera() {
        let contents = std::env::temp_dir().join(format!("ws-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&contents);
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        assert!(!extension_bundled_in(&contents));

        let ext = contents.join("Library/SystemExtensions/dev.example.Camera.systemextension");
        std::fs::create_dir_all(&ext).unwrap();
        assert!(extension_bundled_in(&contents));

        // A stray file that is not an extension bundle does not count.
        std::fs::remove_dir_all(&ext).unwrap();
        std::fs::write(contents.join("Library/SystemExtensions/readme.txt"), "x").unwrap();
        assert!(!extension_bundled_in(&contents));
        let _ = std::fs::remove_dir_all(&contents);
    }

    #[test]
    fn privacy_pages_are_known_for_camera_and_microphone_only() {
        assert!(
            privacy_settings_url("camera")
                .unwrap()
                .ends_with("Privacy_Camera")
        );
        assert!(
            privacy_settings_url("microphone")
                .unwrap()
                .ends_with("Privacy_Microphone")
        );
        assert!(privacy_settings_url("../../etc").is_none());
    }

    fn snapshot(phase: Phase) -> Snapshot {
        Snapshot {
            phase,
            fps: 29.7,
            frames_sent: 120,
            frames_dropped: 2,
            camera_name: Some("FaceTime HD Camera".to_string()),
            message: None,
        }
    }

    fn status_for(
        supported: bool,
        installed: bool,
        extension: &ExtensionPhase,
        pipeline: &Snapshot,
    ) -> VcamStatus {
        compute_status(&Facts {
            supported,
            installed,
            extension,
            pipeline,
        })
    }

    #[test]
    fn unsupported_systems_say_so_and_report_no_activity() {
        let s = status_for(
            false,
            true,
            &ExtensionPhase::Idle,
            &snapshot(Phase::Running),
        );
        assert_eq!(s.state, VcamState::Unsupported);
        assert_eq!(s.message, UNSUPPORTED_MESSAGE);
        assert_eq!((s.fps, s.frames_sent, s.frames_dropped), (0.0, 0, 0));
        assert_eq!(s.camera_name, None);
    }

    #[test]
    fn running_reports_the_counters() {
        let s = status_for(true, true, &ExtensionPhase::Idle, &snapshot(Phase::Running));
        assert_eq!(s.state, VcamState::Running);
        assert_eq!(s.fps, 29.7);
        assert_eq!((s.frames_sent, s.frames_dropped), (120, 2));
        assert_eq!(s.camera_name.as_deref(), Some("FaceTime HD Camera"));
    }

    #[test]
    fn running_with_a_problem_says_so() {
        let mut p = snapshot(Phase::Running);
        p.message = Some("the camera is busy".to_string());
        let s = status_for(true, true, &ExtensionPhase::Idle, &p);
        assert_eq!(s.state, VcamState::Running);
        assert!(s.message.contains("the camera is busy"));
    }

    #[test]
    fn running_wins_over_install_state() {
        // The extension list can lag behind; frames are flowing, so it is running.
        let s = status_for(
            true,
            false,
            &ExtensionPhase::Idle,
            &snapshot(Phase::Running),
        );
        assert_eq!(s.state, VcamState::Running);
    }

    #[test]
    fn a_failed_run_is_an_error_with_its_message() {
        let mut p = snapshot(Phase::Failed);
        p.message = Some("Camera access was denied.".to_string());
        let s = status_for(true, true, &ExtensionPhase::Idle, &p);
        assert_eq!(s.state, VcamState::Error);
        assert_eq!(s.message, "Camera access was denied.");
        assert_eq!(s.fps, 0.0);
    }

    #[test]
    fn starting_is_not_yet_running() {
        let s = status_for(
            true,
            true,
            &ExtensionPhase::Idle,
            &snapshot(Phase::Starting),
        );
        assert_eq!(s.state, VcamState::Ready);
        assert!(s.message.contains("Starting"));
    }

    #[test]
    fn missing_extension_is_not_installed() {
        let s = status_for(true, false, &ExtensionPhase::Idle, &snapshot(Phase::Idle));
        assert_eq!(s.state, VcamState::NotInstalled);
    }

    #[test]
    fn approval_is_pending_until_the_extension_is_installed() {
        let waiting = ExtensionPhase::AwaitingApproval;
        let before = status_for(true, false, &waiting, &snapshot(Phase::Idle));
        assert_eq!(before.state, VcamState::NeedsApproval);
        let after = status_for(true, true, &waiting, &snapshot(Phase::Idle));
        assert_eq!(after.state, VcamState::Ready);
    }

    #[test]
    fn install_problems_and_reboots_stay_in_the_setup_flow() {
        let failed = ExtensionPhase::Failed("Move the app to /Applications.".to_string());
        let s = status_for(true, false, &failed, &snapshot(Phase::Idle));
        assert_eq!(s.state, VcamState::NotInstalled);
        assert_eq!(s.message, "Move the app to /Applications.");

        let s = status_for(
            true,
            false,
            &ExtensionPhase::RebootPending,
            &snapshot(Phase::Idle),
        );
        assert_eq!(s.state, VcamState::NotInstalled);
        assert!(s.message.contains("Restart"));
    }

    #[test]
    fn installed_and_idle_is_ready() {
        let s = status_for(true, true, &ExtensionPhase::Idle, &snapshot(Phase::Idle));
        assert_eq!(s.state, VcamState::Ready);
    }

    #[test]
    fn state_names_match_what_the_window_expects() {
        let names: Vec<String> = [
            VcamState::Unsupported,
            VcamState::NotInstalled,
            VcamState::NeedsApproval,
            VcamState::Ready,
            VcamState::Running,
            VcamState::Error,
        ]
        .iter()
        .map(|s| serde_json::to_string(s).unwrap())
        .collect();
        assert_eq!(
            names,
            [
                "\"unsupported\"",
                "\"not_installed\"",
                "\"needs_approval\"",
                "\"ready\"",
                "\"running\"",
                "\"error\""
            ]
        );
    }

    #[test]
    fn describe_keeps_backend_messages() {
        assert_eq!(describe(&StudioError::Backend("boom".into())), "boom");
        assert!(describe(&StudioError::Unsupported("x")).contains("not available"));
    }
}
