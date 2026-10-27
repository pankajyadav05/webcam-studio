//! Faces for the window's auto-frame. One Vision request lives on its own serial thread,
//! like the person masks in `segmentation.rs`; a request that arrives while one is running
//! is turned away, as the window simply asks again with a newer picture.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use studio_segmentation::{FaceDetector, Sighting, VisionFaceDetector, Want};
use tauri::ipc::{InvokeBody, Request};

type Reply = Result<Sighting, String>;
struct Job {
    jpeg: Vec<u8>,
    want: Want,
    reply: mpsc::SyncSender<Reply>,
}

pub struct Faces {
    sender: Mutex<mpsc::SyncSender<Job>>,
    busy: Arc<AtomicBool>,
    available: bool,
}

impl Faces {
    pub fn new() -> Self {
        Self::with_factory(|| {
            VisionFaceDetector::new()
                .map(|d| Box::new(d) as Box<dyn FaceDetector>)
                .map_err(|e| e.to_string())
        })
    }

    fn with_factory(
        factory: impl Fn() -> Result<Box<dyn FaceDetector>, String> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = Arc::clone(&busy);
        let spawned = std::thread::Builder::new()
            .name("face-vision".into())
            .spawn(move || {
                let mut detector = factory();
                let _ = ready_tx.send(detector.is_ok());
                while let Ok(job) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let detector = detector.as_mut().map_err(|e| e.clone())?;
                        detector
                            .detect(&job.jpeg, job.want)
                            .map_err(|e| e.to_string())
                    }))
                    .unwrap_or_else(|_| {
                        detector = factory();
                        Err("The face finder stopped; it was restarted".into())
                    });
                    worker_busy.store(false, Ordering::Release);
                    let _ = job.reply.send(result);
                }
            });
        let available = spawned.is_ok()
            && ready_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap_or(false);
        Self {
            sender: Mutex::new(sender),
            busy,
            available,
        }
    }

    pub fn available(&self) -> bool {
        self.available
    }

    fn submit(&self, jpeg: Vec<u8>, want: Want) -> Reply {
        if !self.available {
            return Err("Face finding is not available on this Mac".into());
        }
        if jpeg.is_empty() || jpeg.len() > 4 * 1024 * 1024 {
            return Err("The picture is empty or too large".into());
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("busy".into());
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        if self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_send(Job { jpeg, want, reply })
            .is_err()
        {
            self.busy.store(false, Ordering::Release);
            return Err("The face finder is not running".into());
        }
        receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "Face search timed out".to_string())?
    }
}

/// Whether the window can use auto-frame.
#[tauri::command]
pub fn faces_available(state: tauri::State<'_, Faces>) -> bool {
    state.available()
}

/// What to look for, from the `want` header: "hands", "eyes", both, or neither.
fn want_from(header: &str) -> Want {
    Want {
        hands: header.split(',').any(|w| w.trim() == "hands"),
        eyes: header.split(',').any(|w| w.trim() == "eyes"),
    }
}

/// Faces (and hands and eyes when asked) in a JPEG of the whole camera picture (raw body),
/// in shares of the picture.
#[tauri::command]
pub async fn detect_faces(app: tauri::AppHandle, request: Request<'_>) -> Result<Sighting, String> {
    use tauri::Manager;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("detect_faces needs a JPEG body".into());
    };
    let want = want_from(
        request
            .headers()
            .get("want")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    );
    let bytes = bytes.clone();
    tauri::async_runtime::spawn_blocking(move || app.state::<Faces>().submit(bytes, want))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::frame::VideoFrame;
    use studio_segmentation::FaceBox;
    use studio_segmentation::person::Error;

    struct Slow(mpsc::Sender<()>);
    impl FaceDetector for Slow {
        fn detect(&mut self, _: &[u8], want: Want) -> Result<Sighting, Error> {
            let _ = self.0.send(());
            std::thread::sleep(std::time::Duration::from_millis(40));
            let face = FaceBox {
                x: 0.4,
                y: 0.2,
                width: 0.2,
                height: 0.3,
                confidence: 0.9,
            };
            // Says what it was asked for in the number of faces.
            Ok(Sighting {
                faces: vec![face; 1 + usize::from(want.hands) + usize::from(want.eyes)],
                ..Sighting::default()
            })
        }
        fn detect_frame(&mut self, _: &VideoFrame, _: Want) -> Result<Sighting, Error> {
            Err(Error::Unsupported)
        }
    }

    #[test]
    fn one_search_at_a_time_and_the_rest_are_turned_away() {
        let (started, receiver) = mpsc::channel();
        let faces = Arc::new(Faces::with_factory(move || {
            Ok(Box::new(Slow(started.clone())) as Box<dyn FaceDetector>)
        }));
        assert!(faces.available());
        let first = Arc::clone(&faces);
        let handle = std::thread::spawn(move || first.submit(vec![1, 2, 3], Want::default()));
        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(faces.submit(vec![1], Want::default()).unwrap_err(), "busy");
        assert_eq!(handle.join().unwrap().unwrap().faces.len(), 1);
        assert!(faces.submit(Vec::new(), Want::default()).is_err());
        assert_eq!(
            faces
                .submit(vec![1], want_from("eyes, hands"))
                .unwrap()
                .faces
                .len(),
            3
        );
    }

    #[test]
    fn without_vision_it_says_so() {
        let faces = Faces::with_factory(|| Err("unsupported".into()));
        assert!(!faces.available());
        assert!(faces.submit(vec![1], Want::default()).is_err());
    }
}
