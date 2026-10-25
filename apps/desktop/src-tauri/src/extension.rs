//! Installing and removing the macOS camera extension, and remembering how that went.
//!
//! The answers from macOS arrive later, on the main thread, and can come more than once
//! (first "needs approval", then "activated"). [`ExtensionTracker`] keeps the latest news
//! so the status shown in the window can follow it.

// The install bookkeeping is only fed by macOS callbacks; other systems still compile and test it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a freshly activated extension may take to show its camera.
const APPEAR_WITHIN: Duration = Duration::from_secs(6);

use serde::Serialize;

#[cfg(not(target_os = "macos"))]
use crate::vcam::UNSUPPORTED_MESSAGE;

/// Name of the event the window listens for.
pub const EVENT_NAME: &str = "camera-extension";

/// What is known about the last install request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ExtensionPhase {
    /// No request is open and none has failed.
    #[default]
    Idle,
    /// macOS waits for the user to allow the extension.
    AwaitingApproval,
    /// macOS says the extension is active. If its camera has not appeared a few seconds
    /// later, macOS only loads it after a restart (typical when replacing an old version).
    Activated(Instant),
    /// The extension will work after the next restart.
    RebootPending,
    /// The last request failed. The text says why.
    Failed(String),
}

/// One answer from macOS, reduced to what the app cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// The extension is installed and running.
    Activated,
    /// The user has to allow the extension in System Settings.
    NeedsApproval,
    /// The extension works after the next restart.
    AfterReboot,
    /// The extension was removed.
    Removed,
    /// The request failed.
    Failed(String),
}

/// Payload of the `camera-extension` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionEvent {
    /// `activated`, `needs_approval`, `after_reboot`, `removed` or `error`.
    pub outcome: &'static str,
    /// A sentence for the person using the app.
    pub message: String,
}

/// Remembers the latest install news. Shared between the commands and the macOS callbacks.
#[derive(Default)]
pub struct ExtensionTracker {
    phase: Mutex<ExtensionPhase>,
}

impl ExtensionTracker {
    /// Creates a tracker with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// The latest known phase.
    pub fn phase(&self) -> ExtensionPhase {
        let phase = self
            .phase
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match phase {
            ExtensionPhase::Activated(at) if at.elapsed() >= APPEAR_WITHIN => {
                ExtensionPhase::RebootPending
            }
            other => other,
        }
    }

    /// Forgets an old failure when the person tries again.
    pub fn begin(&self) {
        self.set(ExtensionPhase::Idle);
    }

    fn set(&self, phase: ExtensionPhase) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = phase;
    }

    /// Records an answer and returns the event to send to the window.
    pub fn apply(&self, update: Update) -> ExtensionEvent {
        match update {
            Update::Activated => {
                self.set(ExtensionPhase::Activated(Instant::now()));
                event("activated", "The camera is installed.")
            }
            Update::NeedsApproval => {
                self.set(ExtensionPhase::AwaitingApproval);
                event(
                    "needs_approval",
                    "In System Settings, under Camera Extensions, turn on Webcam Studio. Then come back here.",
                )
            }
            Update::AfterReboot => {
                self.set(ExtensionPhase::RebootPending);
                event(
                    "after_reboot",
                    "The camera will be ready after you restart your Mac.",
                )
            }
            Update::Removed => {
                self.set(ExtensionPhase::Idle);
                event("removed", "The camera was removed.")
            }
            Update::Failed(problem) => {
                self.set(ExtensionPhase::Failed(problem.clone()));
                event("error", &problem)
            }
        }
    }
}

fn event(outcome: &'static str, message: &str) -> ExtensionEvent {
    ExtensionEvent {
        outcome,
        message: message.to_string(),
    }
}

/// Where the macOS settings page for camera extensions lives.
pub const SETTINGS_URL: &str = "x-apple.systempreferences:com.apple.LoginItems-Settings.extension";

/// Opens the extension page of System Settings, or System Settings itself if that fails.
///
/// `run` starts `/usr/bin/open` with the given arguments and says whether it succeeded.
pub fn open_settings_with(run: impl Fn(&[&str]) -> bool) -> Result<(), String> {
    if run(&[SETTINGS_URL]) || run(&["-a", "System Settings"]) {
        Ok(())
    } else {
        Err("Could not open System Settings. Open it yourself and look under Login Items & Extensions.".to_string())
    }
}

/// Opens System Settings at the page where the camera extension is allowed.
pub fn open_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        open_settings_with(|args| {
            std::process::Command::new("/usr/bin/open")
                .args(args)
                .status()
                .is_ok_and(|status| status.success())
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(UNSUPPORTED_MESSAGE.to_string())
    }
}

/// Sends an install request to macOS. Answers go to `tracker` and to `emit`.
///
/// The request is handed to the main dispatch queue, which the Tauri event loop runs, so
/// this can be called from any thread.
pub fn install(
    tracker: Arc<ExtensionTracker>,
    emit: Arc<dyn Fn(ExtensionEvent) + Send + Sync>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use studio_vdevice::macos::{ActivationOutcome, request_activation};

        tracker.begin();
        request_activation(Box::new(move |result| {
            let update = match result {
                Ok(ActivationOutcome::Activated) => Update::Activated,
                Ok(ActivationOutcome::NeedsUserApproval) => Update::NeedsApproval,
                Ok(ActivationOutcome::WillCompleteAfterReboot) => Update::AfterReboot,
                Err(error) => Update::Failed(crate::vcam::describe(&error)),
            };
            emit(tracker.apply(update));
        }));
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (tracker, emit);
        Err(UNSUPPORTED_MESSAGE.to_string())
    }
}

/// Sends a removal request to macOS. The answer goes to `tracker` and to `emit`.
pub fn uninstall(
    tracker: Arc<ExtensionTracker>,
    emit: Arc<dyn Fn(ExtensionEvent) + Send + Sync>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use studio_vdevice::macos::request_deactivation;

        tracker.begin();
        request_deactivation(Box::new(move |result| {
            let update = match result {
                Ok(()) => Update::Removed,
                Err(error) => Update::Failed(crate::vcam::describe(&error)),
            };
            emit(tracker.apply(update));
        }));
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (tracker, emit);
        Err(UNSUPPORTED_MESSAGE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn approval_then_activation_follows_the_news() {
        let tracker = ExtensionTracker::new();
        assert_eq!(tracker.phase(), ExtensionPhase::Idle);

        let event = tracker.apply(Update::NeedsApproval);
        assert_eq!(event.outcome, "needs_approval");
        assert_eq!(tracker.phase(), ExtensionPhase::AwaitingApproval);

        let event = tracker.apply(Update::Activated);
        assert_eq!(event.outcome, "activated");
        assert!(matches!(tracker.phase(), ExtensionPhase::Activated(_)));
    }

    #[test]
    fn an_activation_whose_camera_never_appears_needs_a_restart() {
        let tracker = ExtensionTracker::new();
        tracker.set(ExtensionPhase::Activated(Instant::now() - APPEAR_WITHIN));
        assert_eq!(tracker.phase(), ExtensionPhase::RebootPending);
    }

    #[test]
    fn failure_is_remembered_until_the_next_try() {
        let tracker = ExtensionTracker::new();
        let event = tracker.apply(Update::Failed("Move the app to /Applications.".into()));
        assert_eq!(event.outcome, "error");
        assert_eq!(event.message, "Move the app to /Applications.");
        assert_eq!(
            tracker.phase(),
            ExtensionPhase::Failed("Move the app to /Applications.".into())
        );
        tracker.begin();
        assert_eq!(tracker.phase(), ExtensionPhase::Idle);
    }

    #[test]
    fn reboot_and_removal_have_their_own_outcomes() {
        let tracker = ExtensionTracker::new();
        assert_eq!(tracker.apply(Update::AfterReboot).outcome, "after_reboot");
        assert_eq!(tracker.phase(), ExtensionPhase::RebootPending);
        assert_eq!(tracker.apply(Update::Removed).outcome, "removed");
        assert_eq!(tracker.phase(), ExtensionPhase::Idle);
    }

    #[test]
    fn event_payload_has_outcome_and_message() {
        let json = serde_json::to_value(event("activated", "Done.")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"outcome": "activated", "message": "Done."})
        );
    }

    #[test]
    fn settings_page_is_tried_first() {
        let calls = RefCell::new(Vec::new());
        let result = open_settings_with(|args| {
            calls.borrow_mut().push(args.join(" "));
            true
        });
        assert!(result.is_ok());
        assert_eq!(*calls.borrow(), [SETTINGS_URL]);
    }

    #[test]
    fn plain_system_settings_is_the_fallback() {
        let calls = RefCell::new(Vec::new());
        let result = open_settings_with(|args| {
            calls.borrow_mut().push(args.join(" "));
            args[0] != SETTINGS_URL
        });
        assert!(result.is_ok());
        assert_eq!(
            *calls.borrow(),
            [SETTINGS_URL.to_string(), "-a System Settings".to_string()]
        );
    }

    #[test]
    fn failing_both_ways_is_an_error() {
        assert!(open_settings_with(|_| false).is_err());
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn install_is_refused_off_macos() {
        let tracker = Arc::new(ExtensionTracker::new());
        let emit: Arc<dyn Fn(ExtensionEvent) + Send + Sync> = Arc::new(|_| {});
        assert_eq!(
            install(tracker.clone(), emit.clone()),
            Err(UNSUPPORTED_MESSAGE.to_string())
        );
        assert_eq!(
            uninstall(tracker, emit),
            Err(UNSUPPORTED_MESSAGE.to_string())
        );
        assert!(open_settings().is_err());
    }
}
