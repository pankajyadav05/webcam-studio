//! Turns the real camera on only while a meeting app uses Webcam Studio Camera.
//!
//! The camera extension says how many apps read it. A background thread looks twice a
//! second: when a meeting app starts reading, the pipeline starts (camera, person mask,
//! frames into the extension); when none has read for a short while, it stops, so the
//! camera light goes off. An extension too old to say is replaced, once per run.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::pipeline::Phase;
use crate::state::AppState;

const POLL: Duration = Duration::from_millis(500);
/// Meeting apps often close and reopen a camera when switching views; wait before stopping.
const STOP_AFTER: Duration = Duration::from_secs(2);
/// After a failed start, wait this long before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Start,
    Stop,
    /// Ask macOS to replace the extension with the one inside this app.
    Upgrade,
}

/// The rules, kept apart from the devices so they can be tested.
#[derive(Debug, Default)]
pub struct Decider {
    idle_since: Option<Instant>,
    retry_at: Option<Instant>,
    upgrade_asked: bool,
}

impl Decider {
    /// `readers` is `None` when the extension does not report them.
    pub fn decide(
        &mut self,
        now: Instant,
        installed: bool,
        readers: Option<u32>,
        phase: Phase,
    ) -> Action {
        if !installed {
            self.idle_since = None;
            return Action::Nothing;
        }
        let Some(readers) = readers else {
            if self.upgrade_asked {
                return Action::Nothing;
            }
            self.upgrade_asked = true;
            return Action::Upgrade;
        };
        let active = matches!(phase, Phase::Starting | Phase::Running);
        if readers > 0 {
            self.idle_since = None;
            let may_retry = self.retry_at.is_none_or(|at| now >= at);
            if !active && may_retry {
                return Action::Start;
            }
            return Action::Nothing;
        }
        if !active {
            self.idle_since = None;
            return Action::Nothing;
        }
        let since = *self.idle_since.get_or_insert(now);
        if now.duration_since(since) >= STOP_AFTER {
            self.idle_since = None;
            return Action::Stop;
        }
        Action::Nothing
    }

    pub fn start_failed(&mut self, now: Instant) {
        self.retry_at = Some(now + RETRY_AFTER);
    }

    pub fn start_worked(&mut self) {
        self.retry_at = None;
    }
}

/// The real camera meeting frames come from, by name, as the window last chose it.
#[derive(Debug, Default)]
pub struct MeetingCamera {
    name: Mutex<Option<String>>,
}

impl MeetingCamera {
    pub fn set(&self, name: String) {
        *self.name.lock().unwrap_or_else(PoisonError::into_inner) = Some(name);
    }

    fn name(&self) -> Option<String> {
        self.name
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The camera id to open: the one named by the window if it is still there, else the first.
fn camera_id(state: &AppState, wanted: Option<String>) -> Result<String, String> {
    let cameras = state.list_native_cameras()?;
    cameras
        .iter()
        .find(|c| Some(&c.name) == wanted.as_ref())
        .or_else(|| cameras.first())
        .map(|c| c.id.clone())
        .ok_or_else(|| "No camera was found.".to_string())
}

/// Starts the watcher thread. It runs for the life of the app.
pub fn spawn(app: AppHandle) {
    if !crate::vcam::is_supported() || !crate::vcam::extension_bundled() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("meeting-camera-watch".to_string())
        .spawn(move || {
            let mut decider = Decider::default();
            let mut said_ready = false;
            loop {
                std::thread::sleep(POLL);
                let state = app.state::<AppState>();
                let installed = crate::vcam::extension_installed();
                let readers = crate::vcam::meeting_readers();
                let phase = state.pipeline_phase();
                if installed && readers.is_some() && !said_ready {
                    said_ready = true;
                    crate::background::set_status(&app, "Meeting camera: ready, camera off");
                }
                match decider.decide(Instant::now(), installed, readers, phase) {
                    Action::Nothing => {}
                    Action::Upgrade => {
                        log::info!(
                            "The installed camera extension is older than this app's; replacing it"
                        );
                        if let Err(e) =
                            crate::extension::install(state.extension(), crate::emitter(&app))
                        {
                            log::warn!("Could not replace the camera extension: {e}");
                        }
                    }
                    Action::Start => {
                        let wanted = app.state::<MeetingCamera>().name();
                        let started = camera_id(&state, wanted)
                            .and_then(|id| state.start_virtual_camera(&id));
                        match started {
                            Ok(_) => {
                                log::info!("A meeting app opened Webcam Studio Camera; camera on");
                                crate::background::set_status(&app, "Meeting camera: in use");
                                decider.start_worked();
                            }
                            Err(e) => {
                                log::warn!("Could not start the meeting camera: {e}");
                                decider.start_failed(Instant::now());
                            }
                        }
                    }
                    Action::Stop => {
                        log::info!("No meeting app uses Webcam Studio Camera; camera off");
                        state.stop_virtual_camera();
                        crate::background::set_status(&app, "Meeting camera: ready, camera off");
                    }
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("Could not start the meeting camera watcher: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_meeting_app_turns_the_camera_on_and_leaving_turns_it_off_after_a_pause() {
        let t = Instant::now();
        let mut d = Decider::default();
        assert_eq!(d.decide(t, true, Some(0), Phase::Idle), Action::Nothing);
        assert_eq!(
            d.decide(at(t, 500), true, Some(1), Phase::Idle),
            Action::Start
        );
        assert_eq!(
            d.decide(at(t, 1000), true, Some(1), Phase::Running),
            Action::Nothing
        );
        // The meeting app lets go; a short gap is ignored.
        assert_eq!(
            d.decide(at(t, 1500), true, Some(0), Phase::Running),
            Action::Nothing
        );
        assert_eq!(
            d.decide(at(t, 2500), true, Some(1), Phase::Running),
            Action::Nothing
        );
        assert_eq!(
            d.decide(at(t, 3000), true, Some(0), Phase::Running),
            Action::Nothing
        );
        assert_eq!(
            d.decide(at(t, 4900), true, Some(0), Phase::Running),
            Action::Nothing
        );
        assert_eq!(
            d.decide(at(t, 5000), true, Some(0), Phase::Running),
            Action::Stop
        );
    }

    #[test]
    fn a_failed_start_waits_before_trying_again() {
        let t = Instant::now();
        let mut d = Decider::default();
        assert_eq!(d.decide(t, true, Some(1), Phase::Idle), Action::Start);
        d.start_failed(t);
        assert_eq!(
            d.decide(at(t, 500), true, Some(1), Phase::Failed),
            Action::Nothing
        );
        assert_eq!(
            d.decide(at(t, 5000), true, Some(1), Phase::Failed),
            Action::Start
        );
    }

    #[test]
    fn an_old_extension_is_replaced_once_and_a_missing_one_is_left_alone() {
        let t = Instant::now();
        let mut d = Decider::default();
        assert_eq!(d.decide(t, false, None, Phase::Idle), Action::Nothing);
        assert_eq!(d.decide(t, true, None, Phase::Idle), Action::Upgrade);
        assert_eq!(
            d.decide(at(t, 500), true, None, Phase::Idle),
            Action::Nothing
        );
    }
}
