//! The live path: real camera, then the engine, then the virtual camera.
//!
//! A [`Pipeline`] owns one worker thread. The worker asks for camera permission, opens the
//! camera and the virtual camera, and then loops: wait for a picture, run the engine on it
//! and push the result. Every operating system call happens on that thread, never on the
//! window's. Any failure ends the run with a message in plain words; nothing panics out of
//! the worker.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use studio_capture::CameraCapture;
use studio_core::device::{OutputFormat, VirtualCamera};
use studio_core::error::Result as StudioResult;
use studio_engine::Engine;
use studio_segmentation::{
    FaceDetector, PersonSegmenter, Quality, VisionFaceDetector, VisionSegmenter,
};
use studio_vdevice_macos::{OUTPUT_FPS, OUTPUT_HEIGHT, OUTPUT_WIDTH};

use crate::vcam::describe;

/// The name meeting apps show for the virtual camera.
pub const VIRTUAL_CAMERA_NAME: &str = "Webcam Studio Camera";

/// With auto-frame or Studio Light on, faces are looked for in every this many frames (10
/// times a second).
const FACE_EVERY: u32 = 3;

/// Pushes that fail this many times in a row end the run.
const MAX_FAILURES_IN_A_ROW: u32 = 30;

/// What the worker thread is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// No run, and the last one ended cleanly (or there was none).
    #[default]
    Idle,
    /// The worker is asking for permission and opening devices.
    Starting,
    /// Frames are flowing.
    Running,
    /// The last run ended with an error. The message says why.
    Failed,
}

/// A copy of the pipeline's numbers at one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// What the worker is doing.
    pub phase: Phase,
    /// Frames sent during the last second.
    pub fps: f32,
    /// Frames pushed to the virtual camera since the last start.
    pub frames_sent: u64,
    /// Frames that could not be pushed since the last start.
    pub frames_dropped: u64,
    /// Name of the real camera, once it is open.
    pub camera_name: Option<String>,
    /// The failure (phase `Failed`) or the latest push problem (phase `Running`).
    pub message: Option<String>,
}

/// How long the pipeline waits at each point.
#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    /// For the person to answer the camera permission question.
    pub permission: Duration,
    /// For the whole start, including the permission question.
    pub start: Duration,
    /// For the worker to wind down after a stop request.
    pub stop: Duration,
    /// For one camera picture.
    pub frame: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            permission: Duration::from_secs(60),
            start: Duration::from_secs(75),
            stop: Duration::from_secs(3),
            frame: Duration::from_secs(1),
        }
    }
}

/// The parts of the system the pipeline uses. Replaced by fakes in tests.
pub trait Devices: Send + Sync + 'static {
    /// Asks for camera permission. `Ok(false)` means the person said no; `Err` means no
    /// answer came in time.
    fn request_permission(&self, timeout: Duration) -> Result<bool, String>;
    /// Opens a real camera at the given size and rate.
    fn open_camera(
        &self,
        id: &str,
        width: u32,
        height: u32,
        fps: u32,
    ) -> StudioResult<Box<dyn CameraCapture>>;
    /// Creates the virtual camera.
    fn create_virtual_camera(&self, name: &str) -> StudioResult<Box<dyn VirtualCamera>>;
    /// The person model that finds who to keep sharp. Called on the worker thread, which
    /// it must then stay on (Apple Vision keeps per-thread state).
    fn person_model(&self) -> Result<Box<dyn PersonSegmenter>, String> {
        VisionSegmenter::new()
            .map(|vision| Box::new(vision) as Box<dyn PersonSegmenter>)
            .map_err(|e| e.to_string())
    }
    /// The face finder for auto-frame. Called on the worker thread, which it stays on.
    fn face_detector(&self) -> Result<Box<dyn FaceDetector>, String> {
        VisionFaceDetector::new()
            .map(|vision| Box::new(vision) as Box<dyn FaceDetector>)
            .map_err(|e| e.to_string())
    }
}

/// The real thing: the capture crate and the platform's virtual camera.
pub struct SystemDevices;

impl Devices for SystemDevices {
    fn request_permission(&self, timeout: Duration) -> Result<bool, String> {
        let (tx, rx) = mpsc::channel();
        studio_capture::request_camera_permission(Box::new(move |granted| {
            let _ = tx.send(granted);
        }));
        rx.recv_timeout(timeout)
            .map_err(|_| "Timed out waiting for camera permission.".to_string())
    }

    fn open_camera(
        &self,
        id: &str,
        width: u32,
        height: u32,
        fps: u32,
    ) -> StudioResult<Box<dyn CameraCapture>> {
        studio_capture::open_camera(id, width, height, fps)
    }

    fn create_virtual_camera(&self, name: &str) -> StudioResult<Box<dyn VirtualCamera>> {
        studio_vdevice::create_camera(name)
    }
}

/// Frames per second over a rolling second.
#[derive(Debug, Default)]
pub struct FpsMeter {
    stamps: VecDeque<Instant>,
}

impl FpsMeter {
    /// Notes that a frame went out at `now`.
    pub fn tick(&mut self, now: Instant) {
        self.stamps.push_back(now);
        self.prune(now);
    }

    /// The rate over the last second ending at `now`.
    pub fn fps(&mut self, now: Instant) -> f32 {
        self.prune(now);
        match (self.stamps.front(), self.stamps.back()) {
            (Some(first), Some(last)) if self.stamps.len() >= 2 => {
                let span = last.duration_since(*first).as_secs_f32();
                if span > 0.0 {
                    (self.stamps.len() - 1) as f32 / span
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// Forgets everything.
    pub fn reset(&mut self) {
        self.stamps.clear();
    }

    fn prune(&mut self, now: Instant) {
        while let Some(first) = self.stamps.front() {
            if now.saturating_duration_since(*first) > Duration::from_secs(1) {
                self.stamps.pop_front();
            } else {
                break;
            }
        }
    }
}

#[derive(Default)]
struct Inner {
    phase: Phase,
    camera_name: Option<String>,
    message: Option<String>,
}

/// Numbers the worker writes and the window reads.
struct Shared {
    inner: Mutex<Inner>,
    sent: AtomicU64,
    dropped: AtomicU64,
    meter: Mutex<FpsMeter>,
}

impl Shared {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn meter(&self) -> MutexGuard<'_, FpsMeter> {
        self.meter.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_phase(&self, phase: Phase) {
        self.inner().phase = phase;
    }

    fn fail(&self, message: &str) {
        let mut inner = self.inner();
        inner.phase = Phase::Failed;
        inner.message = Some(message.to_string());
    }
}

struct Worker {
    stop: Arc<AtomicBool>,
    /// Closes (disconnects) when the worker thread ends.
    ended: mpsc::Receiver<()>,
    handle: JoinHandle<()>,
}

/// Runs the camera, the engine and the virtual camera on a worker thread.
pub struct Pipeline {
    engine: Arc<Mutex<Engine>>,
    devices: Arc<dyn Devices>,
    timeouts: Timeouts,
    shared: Arc<Shared>,
    worker: Mutex<Option<Worker>>,
}

impl Pipeline {
    /// Creates a pipeline that uses the real devices.
    pub fn new(engine: Arc<Mutex<Engine>>) -> Self {
        Self::with_devices(engine, Arc::new(SystemDevices), Timeouts::default())
    }

    /// Creates a pipeline with the given devices and timeouts.
    pub fn with_devices(
        engine: Arc<Mutex<Engine>>,
        devices: Arc<dyn Devices>,
        timeouts: Timeouts,
    ) -> Self {
        Self {
            engine,
            devices,
            timeouts,
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner::default()),
                sent: AtomicU64::new(0),
                dropped: AtomicU64::new(0),
                meter: Mutex::new(FpsMeter::default()),
            }),
            worker: Mutex::new(None),
        }
    }

    /// The numbers right now.
    pub fn snapshot(&self) -> Snapshot {
        let (phase, camera_name, message) = {
            let inner = self.shared.inner();
            (
                inner.phase,
                inner.camera_name.clone(),
                inner.message.clone(),
            )
        };
        let fps = if phase == Phase::Running {
            self.shared.meter().fps(Instant::now())
        } else {
            0.0
        };
        Snapshot {
            phase,
            fps,
            frames_sent: self.shared.sent.load(Ordering::Relaxed),
            frames_dropped: self.shared.dropped.load(Ordering::Relaxed),
            camera_name,
            message,
        }
    }

    /// Starts the run and waits until the first step is done or has failed.
    ///
    /// This blocks, possibly for as long as the person takes to answer the camera permission
    /// question, so call it from a thread that is not the window's.
    pub fn start(&self, camera_id: &str) -> Result<(), String> {
        let mut slot = self.worker.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = slot.take() {
            if old.handle.is_finished() {
                let _ = old.handle.join();
            } else {
                *slot = Some(old);
                return Err("The virtual camera is already on.".to_string());
            }
        }

        if let Err(message) = self.check_output_size() {
            self.reset_counters();
            self.shared.fail(&message);
            return Err(message);
        }

        self.reset_counters();
        self.shared.set_phase(Phase::Starting);

        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (ended_tx, ended_rx) = mpsc::channel::<()>();
        let job = Job {
            camera_id: camera_id.to_string(),
            engine: Arc::clone(&self.engine),
            devices: Arc::clone(&self.devices),
            timeouts: self.timeouts,
            shared: Arc::clone(&self.shared),
            stop: Arc::clone(&stop),
        };
        let handle = std::thread::Builder::new()
            .name("virtual-camera".to_string())
            .spawn(move || {
                job.run_guarded(&ready_tx);
                drop(ended_tx);
            })
            .map_err(|e| {
                let message = format!("Could not start the camera thread: {e}.");
                self.shared.fail(&message);
                message
            })?;
        *slot = Some(Worker {
            stop: Arc::clone(&stop),
            ended: ended_rx,
            handle,
        });

        match ready_rx.recv_timeout(self.timeouts.start) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::SeqCst);
                let message = "Starting the camera took too long. Try again.".to_string();
                self.shared.fail(&message);
                Err(message)
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The worker ended without saying anything. It has already recorded why.
                Err(self
                    .snapshot()
                    .message
                    .unwrap_or_else(|| "The virtual camera could not start.".to_string()))
            }
        }
    }

    /// Stops the run, if any, and waits a short while for the worker to close the devices.
    /// Also clears an earlier error.
    pub fn stop(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            worker.stop.store(true, Ordering::SeqCst);
            match worker.ended.recv_timeout(self.timeouts.stop) {
                Err(RecvTimeoutError::Timeout) => {
                    log::warn!(
                        "The virtual camera worker did not stop in time; leaving it behind."
                    );
                }
                _ => {
                    let _ = worker.handle.join();
                }
            }
        }
        let mut inner = self.shared.inner();
        inner.phase = Phase::Idle;
        inner.message = None;
        inner.camera_name = None;
        drop(inner);
        self.shared.meter().reset();
    }

    fn reset_counters(&self) {
        self.shared.sent.store(0, Ordering::Relaxed);
        self.shared.dropped.store(0, Ordering::Relaxed);
        self.shared.meter().reset();
        let mut inner = self.shared.inner();
        inner.message = None;
        inner.camera_name = None;
    }

    fn check_output_size(&self) -> Result<(), String> {
        let engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
        let settings = engine.settings();
        size_problem(settings.output_width, settings.output_height).map_or(Ok(()), Err)
    }
}

/// The message to show when the engine's output is not the size the virtual camera takes.
fn size_problem(width: u32, height: u32) -> Option<String> {
    if width == OUTPUT_WIDTH && height == OUTPUT_HEIGHT {
        None
    } else {
        Some(format!(
            "The output size is set to {width} × {height}, but the virtual camera is fixed at \
             {OUTPUT_WIDTH} × {OUTPUT_HEIGHT}. Set the output size back to \
             {OUTPUT_WIDTH} × {OUTPUT_HEIGHT}."
        ))
    }
}

/// Everything the worker thread needs.
struct Job {
    camera_id: String,
    engine: Arc<Mutex<Engine>>,
    devices: Arc<dyn Devices>,
    timeouts: Timeouts,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
}

impl Job {
    /// Runs the job, turning panics and errors into a recorded failure.
    fn run_guarded(&self, ready: &mpsc::Sender<Result<(), String>>) {
        let outcome = catch_unwind(AssertUnwindSafe(|| self.run(ready)))
            .unwrap_or_else(|_| Err("The camera pipeline stopped unexpectedly.".to_string()));
        match outcome {
            Ok(()) => self.shared.set_phase(Phase::Idle),
            Err(message) => {
                self.shared.fail(&message);
                // Tells `start` why, if it is still waiting. Ignored once it has answered.
                let _ = ready.send(Err(message));
            }
        }
    }

    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn run(&self, ready: &mpsc::Sender<Result<(), String>>) -> Result<(), String> {
        let granted = self.devices.request_permission(self.timeouts.permission)?;
        if !granted {
            return Err(
                "Camera access was denied. Allow Webcam Studio under System Settings, \
                        Privacy & Security, Camera, then try again."
                    .to_string(),
            );
        }
        if self.cancelled() {
            return Ok(());
        }

        let mut camera = self
            .devices
            .open_camera(&self.camera_id, OUTPUT_WIDTH, OUTPUT_HEIGHT, OUTPUT_FPS)
            .map_err(|e| format!("Could not open the camera: {}", describe(&e)))?;
        let mut virtual_camera = self
            .devices
            .create_virtual_camera(VIRTUAL_CAMERA_NAME)
            .map_err(|e| format!("Could not create the virtual camera: {}", describe(&e)))?;
        camera
            .start()
            .map_err(|e| format!("Could not start the camera: {}", describe(&e)))?;
        let format = OutputFormat {
            width: OUTPUT_WIDTH,
            height: OUTPUT_HEIGHT,
            fps: OUTPUT_FPS,
        };
        if let Err(e) = virtual_camera.start(format) {
            camera.stop();
            return Err(format!(
                "Could not start the virtual camera: {}",
                describe(&e)
            ));
        }

        {
            let mut inner = self.shared.inner();
            log::info!("Virtual camera reading from {}", camera.name());
            inner.camera_name = Some(camera.name().to_string());
            inner.phase = Phase::Running;
        }
        let _ = ready.send(Ok(()));

        let outcome = self.frame_loop(camera.as_mut(), virtual_camera.as_mut());
        camera.stop();
        if let Err(e) = virtual_camera.stop() {
            log::warn!("Stopping the virtual camera failed: {e}");
        }
        outcome
    }

    fn frame_loop(
        &self,
        camera: &mut dyn CameraCapture,
        virtual_camera: &mut dyn VirtualCamera,
    ) -> Result<(), String> {
        let mut failures_in_a_row = 0u32;
        // Apple Vision's request keeps its own state and must stay on this thread. Without it,
        // Blur and Replace leave the picture as it is.
        let mut person = match self.devices.person_model() {
            Ok(model) => Some(model),
            Err(e) => {
                log::warn!("No person model for the virtual camera: {e}");
                None
            }
        };
        let mut model_errors = 0u32;
        let mut faces = None;
        let (mut frame_number, mut face_errors) = (0u32, 0u32);
        log::info!(
            "Virtual camera running; person model: {}",
            if person.is_some() {
                "Apple Vision"
            } else {
                "none"
            }
        );
        // Numbers for the log every five seconds: is the picture lit, does the model find a person?
        let (mut seen, mut luma_sum, mut person_sum, mut masks) = (0u32, 0f64, 0f64, 0u32);
        while !self.cancelled() {
            let frame = match camera.next_frame(self.timeouts.frame) {
                Ok(Some(frame)) => frame,
                Ok(None) => continue,
                Err(e) => {
                    return Err(format!(
                        "The camera stopped sending pictures: {}",
                        describe(&e)
                    ));
                }
            };

            frame_number = frame_number.wrapping_add(1);
            let wants = self
                .engine
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .wants();
            // Eye contact needs the eyes in every picture; the rest a few times a second.
            let due = wants.is_some_and(|w| w.eyes || frame_number % FACE_EVERY == 0);
            // Vision runs outside the engine lock, so the window's changes never wait.
            let found = if let (true, Some(want)) = (due, wants) {
                if faces.is_none() {
                    faces = Some(self.devices.face_detector().map_err(|e| {
                        log::warn!("No face finder for auto-frame: {e}");
                    }));
                }
                match faces.as_mut() {
                    Some(Ok(detector)) => detector
                        .detect_frame(&frame, want)
                        .map_err(|e| {
                            face_errors += 1;
                            if face_errors.is_power_of_two() {
                                log::warn!("Face search failed ({face_errors} so far): {e}");
                            }
                        })
                        .ok(),
                    _ => None,
                }
            } else {
                None
            };

            let output = {
                let mut engine = self.engine.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(found) = &found {
                    engine.observe(found, frame.width, frame.height, frame.timestamp_us);
                }
                let settings = engine.settings();
                if let Some(problem) = size_problem(settings.output_width, settings.output_height) {
                    return Err(problem);
                }
                match person.as_mut() {
                    Some(vision) => engine.process_video_with(&frame, &mut |framed| {
                        let mask = vision
                            .segment_frame(framed, Quality::Balanced)
                            .map_err(|e| {
                                model_errors += 1;
                                if model_errors.is_power_of_two() {
                                    log::warn!("Person mask failed ({model_errors} so far): {e}");
                                }
                            })
                            .ok();
                        if let Some(mask) = &mask {
                            masks += 1;
                            person_sum += mask_coverage(mask);
                        }
                        mask
                    }),
                    None => engine.process_video(&frame),
                }
            };
            if let Some(problem) = size_problem(output.width, output.height) {
                return Err(problem);
            }
            seen += 1;
            luma_sum += mean_luma(&frame);
            if seen == 150 {
                let background = match &engine_settings_mode(&self.engine) {
                    Some(mode) => mode.clone(),
                    None => "unknown".to_string(),
                };
                log::info!(
                    "Last 150 frames: {}x{}, brightness {:.0}/255, background {background}, masks {masks}, person {:.1}% of the picture, model errors so far {model_errors}",
                    frame.width,
                    frame.height,
                    luma_sum / f64::from(seen),
                    if masks > 0 {
                        person_sum / f64::from(masks) * 100.0
                    } else {
                        0.0
                    },
                );
                (seen, luma_sum, person_sum, masks) = (0, 0.0, 0.0, 0);
            }

            match virtual_camera.push_frame(&output) {
                Ok(()) => {
                    failures_in_a_row = 0;
                    self.shared.sent.fetch_add(1, Ordering::Relaxed);
                    self.shared.meter().tick(Instant::now());
                }
                Err(e) => {
                    failures_in_a_row += 1;
                    self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                    let problem = describe(&e);
                    self.shared.inner().message = Some(problem.clone());
                    if failures_in_a_row >= MAX_FAILURES_IN_A_ROW {
                        return Err(format!(
                            "The virtual camera stopped accepting pictures: {problem}"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Share of a mask that is the person, 0..=1.
fn mask_coverage(mask: &studio_segmentation::Mask) -> f64 {
    if mask.alpha.is_empty() {
        return 0.0;
    }
    mask.alpha.iter().map(|&a| u64::from(a)).sum::<u64>() as f64 / (mask.alpha.len() as f64 * 255.0)
}

/// Average brightness of a BGRA frame, 0..=255, from a sparse sample of pixels.
fn mean_luma(frame: &studio_core::frame::VideoFrame) -> f64 {
    let (mut sum, mut n) = (0u64, 0u64);
    for y in (0..frame.height as usize).step_by(16) {
        let Some(row) = frame.data.get(y * frame.stride..) else {
            break;
        };
        for x in (0..frame.width as usize).step_by(16) {
            if let Some(p) = row.get(x * 4..x * 4 + 3) {
                sum += (u64::from(p[0]) * 29 + u64::from(p[1]) * 150 + u64::from(p[2]) * 77) >> 8;
                n += 1;
            }
        }
    }
    if n == 0 { 0.0 } else { sum as f64 / n as f64 }
}

/// The background mode the engine is using, for the log.
fn engine_settings_mode(engine: &Mutex<Engine>) -> Option<String> {
    let engine = engine.lock().ok()?;
    Some(match &engine.settings().background {
        studio_core::settings::BackgroundMode::None => "none".to_string(),
        studio_core::settings::BackgroundMode::Blur { radius } => format!("blur {radius}"),
        studio_core::settings::BackgroundMode::Replace { image_path } => {
            format!("replace ({image_path})")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use studio_core::error::StudioError;
    use studio_core::frame::VideoFrame;
    use studio_core::settings::Settings;

    #[derive(Clone, Copy, PartialEq)]
    enum CameraFault {
        None,
        FailsAfter(u32),
        PanicsAfter(u32),
    }

    /// What the fake devices saw and how they should misbehave.
    struct Fake {
        permission: Mutex<Result<bool, String>>,
        permission_delay: Duration,
        camera_fault: Mutex<CameraFault>,
        push_fails: AtomicBool,
        cameras_opened: AtomicUsize,
        camera_stopped: Arc<AtomicBool>,
        vcam_started: Arc<AtomicBool>,
        vcam_stopped: Arc<AtomicBool>,
        pushed: Arc<Mutex<Vec<(u32, u32)>>>,
    }

    impl Fake {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                permission: Mutex::new(Ok(true)),
                permission_delay: Duration::ZERO,
                camera_fault: Mutex::new(CameraFault::None),
                push_fails: AtomicBool::new(false),
                cameras_opened: AtomicUsize::new(0),
                camera_stopped: Arc::new(AtomicBool::new(false)),
                vcam_started: Arc::new(AtomicBool::new(false)),
                vcam_stopped: Arc::new(AtomicBool::new(false)),
                pushed: Arc::new(Mutex::new(Vec::new())),
            })
        }
    }

    struct FakeCamera {
        fault: CameraFault,
        given: u32,
        stopped: Arc<AtomicBool>,
    }

    impl CameraCapture for FakeCamera {
        fn start(&mut self) -> StudioResult<()> {
            Ok(())
        }
        fn next_frame(&mut self, _timeout: Duration) -> StudioResult<Option<VideoFrame>> {
            std::thread::sleep(Duration::from_millis(2));
            self.given += 1;
            match self.fault {
                CameraFault::FailsAfter(n) if self.given > n => {
                    Err(StudioError::Backend("unplugged".to_string()))
                }
                CameraFault::PanicsAfter(n) if self.given > n => panic!("boom"),
                _ => Ok(Some(VideoFrame::new_bgra(640, 480, u64::from(self.given)))),
            }
        }
        fn stop(&mut self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
        fn name(&self) -> &str {
            "Fake Camera"
        }
    }

    struct FakeVirtualCamera {
        fails: bool,
        started: Arc<AtomicBool>,
        stopped: Arc<AtomicBool>,
        pushed: Arc<Mutex<Vec<(u32, u32)>>>,
    }

    impl VirtualCamera for FakeVirtualCamera {
        fn name(&self) -> &str {
            "Fake"
        }
        fn start(&mut self, format: OutputFormat) -> StudioResult<()> {
            assert_eq!((format.width, format.height, format.fps), (1280, 720, 30));
            self.started.store(true, Ordering::SeqCst);
            Ok(())
        }
        fn push_frame(&mut self, frame: &VideoFrame) -> StudioResult<()> {
            if self.fails {
                return Err(StudioError::Backend("the extension is gone".to_string()));
            }
            self.pushed
                .lock()
                .unwrap()
                .push((frame.width, frame.height));
            Ok(())
        }
        fn stop(&mut self) -> StudioResult<()> {
            self.stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    impl Devices for Fake {
        fn request_permission(&self, timeout: Duration) -> Result<bool, String> {
            if self.permission_delay > timeout {
                std::thread::sleep(timeout);
                return Err("Timed out waiting for camera permission.".to_string());
            }
            self.permission.lock().unwrap().clone()
        }
        fn open_camera(
            &self,
            _id: &str,
            width: u32,
            height: u32,
            fps: u32,
        ) -> StudioResult<Box<dyn CameraCapture>> {
            assert_eq!((width, height, fps), (1280, 720, 30));
            self.cameras_opened.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(FakeCamera {
                fault: *self.camera_fault.lock().unwrap(),
                given: 0,
                stopped: Arc::clone(&self.camera_stopped),
            }))
        }
        fn create_virtual_camera(&self, _name: &str) -> StudioResult<Box<dyn VirtualCamera>> {
            Ok(Box::new(FakeVirtualCamera {
                fails: self.push_fails.load(Ordering::SeqCst),
                started: Arc::clone(&self.vcam_started),
                stopped: Arc::clone(&self.vcam_stopped),
                pushed: Arc::clone(&self.pushed),
            }))
        }
        // Real Vision is slow on CI machines without a Neural Engine; frame flow is what
        // these tests check.
        fn person_model(&self) -> Result<Box<dyn PersonSegmenter>, String> {
            Err("no model in tests".to_string())
        }
        fn face_detector(&self) -> Result<Box<dyn FaceDetector>, String> {
            Err("no face finder in tests".to_string())
        }
    }

    fn quick() -> Timeouts {
        Timeouts {
            permission: Duration::from_millis(200),
            start: Duration::from_secs(5),
            stop: Duration::from_secs(5),
            frame: Duration::from_millis(100),
        }
    }

    fn pipeline_with(fake: &Arc<Fake>, settings: Settings) -> Pipeline {
        let engine = Arc::new(Mutex::new(Engine::new(settings)));
        Pipeline::with_devices(engine, Arc::clone(fake) as Arc<dyn Devices>, quick())
    }

    // Generous: only a stuck test waits this long, and CI runners process debug-build
    // frames many times slower than a Mac.
    fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn frames_flow_from_camera_through_engine_to_virtual_camera() {
        let fake = Fake::new();
        let pipeline = pipeline_with(&fake, Settings::default());
        pipeline.start("cam-1").expect("should start");

        wait_for("frames", || pipeline.snapshot().frames_sent >= 10);
        let running = pipeline.snapshot();
        assert_eq!(running.phase, Phase::Running);
        assert_eq!(running.camera_name.as_deref(), Some("Fake Camera"));
        assert_eq!(running.frames_dropped, 0);
        assert!(running.fps > 0.0, "fps was {}", running.fps);
        assert!(fake.vcam_started.load(Ordering::SeqCst));

        pipeline.stop();
        let stopped = pipeline.snapshot();
        assert_eq!(stopped.phase, Phase::Idle);
        assert_eq!(stopped.fps, 0.0);
        assert!(fake.camera_stopped.load(Ordering::SeqCst));
        assert!(fake.vcam_stopped.load(Ordering::SeqCst));
        // The engine scaled the 640x480 camera picture to the virtual camera's size.
        let pushed = fake.pushed.lock().unwrap();
        assert!(!pushed.is_empty());
        assert!(pushed.iter().all(|&size| size == (1280, 720)));
    }

    #[test]
    fn starting_twice_is_refused_and_restarting_works() {
        let fake = Fake::new();
        let pipeline = pipeline_with(&fake, Settings::default());
        pipeline.start("cam-1").unwrap();
        let error = pipeline.start("cam-1").unwrap_err();
        assert!(error.contains("already"), "{error}");
        assert_eq!(pipeline.snapshot().phase, Phase::Running);

        pipeline.stop();
        pipeline.start("cam-1").unwrap();
        wait_for("frames after restart", || {
            pipeline.snapshot().frames_sent >= 3
        });
        pipeline.stop();
        assert_eq!(fake.cameras_opened.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn wrong_output_size_is_reported_and_nothing_is_opened() {
        let fake = Fake::new();
        let settings = Settings {
            output_width: 1920,
            output_height: 1080,
            ..Settings::default()
        };
        let pipeline = pipeline_with(&fake, settings);
        let error = pipeline.start("cam-1").unwrap_err();
        assert!(error.contains("1920 × 1080"), "{error}");
        assert!(error.contains("1280 × 720"), "{error}");
        assert_eq!(fake.cameras_opened.load(Ordering::SeqCst), 0);
        assert!(fake.pushed.lock().unwrap().is_empty());
        let snap = pipeline.snapshot();
        assert_eq!(snap.phase, Phase::Failed);
        assert_eq!(snap.message.as_deref(), Some(error.as_str()));
    }

    #[test]
    fn changing_the_size_while_running_ends_the_run_without_pushing_bad_frames() {
        let fake = Fake::new();
        let engine = Arc::new(Mutex::new(Engine::new(Settings::default())));
        let pipeline = Pipeline::with_devices(
            Arc::clone(&engine),
            Arc::clone(&fake) as Arc<dyn Devices>,
            quick(),
        );
        pipeline.start("cam-1").unwrap();
        wait_for("frames", || pipeline.snapshot().frames_sent >= 3);
        engine.lock().unwrap().update_settings(Settings {
            output_width: 640,
            output_height: 360,
            ..Settings::default()
        });
        wait_for("failure", || pipeline.snapshot().phase == Phase::Failed);
        assert!(pipeline.snapshot().message.unwrap().contains("640 × 360"));
        assert!(
            fake.pushed
                .lock()
                .unwrap()
                .iter()
                .all(|&s| s == (1280, 720))
        );
        assert!(fake.camera_stopped.load(Ordering::SeqCst));
        assert!(fake.vcam_stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn denied_permission_stops_before_opening_the_camera() {
        let fake = Fake::new();
        *fake.permission.lock().unwrap() = Ok(false);
        let pipeline = pipeline_with(&fake, Settings::default());
        let error = pipeline.start("cam-1").unwrap_err();
        assert!(error.contains("denied"), "{error}");
        assert_eq!(fake.cameras_opened.load(Ordering::SeqCst), 0);
        assert_eq!(pipeline.snapshot().phase, Phase::Failed);
    }

    #[test]
    fn no_answer_to_the_permission_question_times_out() {
        let fake = Arc::new(Fake {
            permission_delay: Duration::from_secs(60),
            ..Arc::try_unwrap(Fake::new()).ok().unwrap()
        });
        let pipeline = pipeline_with(&fake, Settings::default());
        let error = pipeline.start("cam-1").unwrap_err();
        assert!(error.contains("Timed out"), "{error}");
        assert_eq!(fake.cameras_opened.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_camera_that_stops_ends_the_run_with_a_message() {
        let fake = Fake::new();
        *fake.camera_fault.lock().unwrap() = CameraFault::FailsAfter(5);
        let pipeline = pipeline_with(&fake, Settings::default());
        pipeline.start("cam-1").unwrap();
        wait_for("failure", || pipeline.snapshot().phase == Phase::Failed);
        let message = pipeline.snapshot().message.unwrap();
        assert!(message.contains("unplugged"), "{message}");
        assert!(fake.camera_stopped.load(Ordering::SeqCst));
        assert!(fake.vcam_stopped.load(Ordering::SeqCst));
        // Stopping afterwards clears the error.
        pipeline.stop();
        assert_eq!(pipeline.snapshot().phase, Phase::Idle);
        assert_eq!(pipeline.snapshot().message, None);
    }

    #[test]
    fn a_panic_in_the_worker_becomes_an_error() {
        let fake = Fake::new();
        *fake.camera_fault.lock().unwrap() = CameraFault::PanicsAfter(3);
        let pipeline = pipeline_with(&fake, Settings::default());
        pipeline.start("cam-1").unwrap();
        wait_for("failure", || pipeline.snapshot().phase == Phase::Failed);
        assert!(
            pipeline
                .snapshot()
                .message
                .unwrap()
                .contains("unexpectedly")
        );
        pipeline.stop();
    }

    #[test]
    fn a_virtual_camera_that_rejects_frames_ends_the_run_and_counts_drops() {
        let fake = Fake::new();
        fake.push_fails.store(true, Ordering::SeqCst);
        let pipeline = pipeline_with(&fake, Settings::default());
        pipeline.start("cam-1").unwrap();
        wait_for("failure", || pipeline.snapshot().phase == Phase::Failed);
        let snap = pipeline.snapshot();
        assert_eq!(snap.frames_sent, 0);
        assert_eq!(snap.frames_dropped, u64::from(MAX_FAILURES_IN_A_ROW));
        assert!(snap.message.unwrap().contains("the extension is gone"));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn starting_fails_cleanly_where_capture_is_unsupported() {
        let engine = Arc::new(Mutex::new(Engine::new(Settings::default())));
        let pipeline = Pipeline::new(engine);
        let error = pipeline.start("anything").unwrap_err();
        assert!(error.starts_with("Could not open the camera"), "{error}");
        assert!(error.contains("not available"), "{error}");
        let snap = pipeline.snapshot();
        assert_eq!(snap.phase, Phase::Failed);
        assert_eq!(snap.frames_sent, 0);
        pipeline.stop();
        assert_eq!(pipeline.snapshot().phase, Phase::Idle);
    }

    #[test]
    fn fps_is_measured_over_a_rolling_second() {
        let mut meter = FpsMeter::default();
        let t0 = Instant::now();
        assert_eq!(meter.fps(t0), 0.0);
        for i in 0..=60u32 {
            meter.tick(t0 + Duration::from_millis(u64::from(i) * 1000 / 30));
        }
        let at = t0 + Duration::from_millis(2000);
        let fps = meter.fps(at);
        assert!((fps - 30.0).abs() < 1.5, "fps was {fps}");
        // Without new frames the rate falls to zero.
        assert_eq!(meter.fps(at + Duration::from_secs(2)), 0.0);
    }
}
