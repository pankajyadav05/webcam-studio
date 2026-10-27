//! Faces, hands and eyes for the window (auto-frame, Studio Light, Show-and-tell, and the
//! Eye contact maps). Vision lives on its own serial thread, like the person masks in
//! `segmentation.rs`; a request that arrives while one is running is turned away, as the
//! window simply asks again with a newer picture. Eye contact has a second worker of its own,
//! so its frequent requests never crowd out the faces.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use studio_segmentation::{FaceDetector, Sighting, VisionFaceDetector, Want};
use tauri::ipc::{InvokeBody, Request};

enum Output {
    Sighting(Sighting),
    Bytes(Vec<u8>),
}
type Reply = Result<Output, String>;
enum Task {
    Look(Want),
    EyeMaps,
}
struct Job {
    jpeg: Vec<u8>,
    task: Task,
    reply: mpsc::SyncSender<Reply>,
}

/// The Eye contact maps for a picture: the size of the picture, then for each eye its box
/// (top, left, height, width), whether the box is mirrored, and the network's maps (x and y
/// warp and the share to keep, per pixel of 64 × 48). All little-endian f32. No eyes it can
/// help: just the size and a count of 0.
fn eye_maps(detector: &mut dyn FaceDetector, jpeg: &[u8]) -> Result<Vec<u8>, String> {
    use studio_engine::eyecontact;
    let frame = crate::image_load::decode_background(jpeg)?;
    let seen = detector
        .detect_frame(
            &frame,
            Want {
                hands: false,
                eyes: true,
            },
        )
        .map_err(|e| e.to_string())?;
    let size = (frame.width, frame.height);
    let whole = studio_engine::CropRect {
        x: 0.0,
        y: 0.0,
        width: size.0 as f32,
        height: size.1 as f32,
    };
    let plans = eyecontact::biggest_pair(&seen.eyes, size, &whole, size, false)
        .filter(|(l, r)| eyecontact::usable(l, r))
        .and_then(|(l, r)| eyecontact::plan_pair(&frame, &l, &r));
    let mut out: Vec<f32> = vec![size.0 as f32, size.1 as f32];
    match plans {
        None => out.push(0.0),
        Some(plans) => {
            out.push(plans.len() as f32);
            for p in &plans {
                out.extend([p.top as f32, p.left as f32, p.height as f32, p.width as f32]);
                out.push(if p.mirrored { 1.0 } else { 0.0 });
                for (flow, keep) in p.warp.flow.chunks_exact(2).zip(&p.warp.keep) {
                    out.extend([flow[0], flow[1], *keep]);
                }
            }
        }
    }
    Ok(out.iter().flat_map(|v| v.to_le_bytes()).collect())
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
                        match job.task {
                            Task::Look(want) => detector
                                .detect(&job.jpeg, want)
                                .map(Output::Sighting)
                                .map_err(|e| e.to_string()),
                            Task::EyeMaps => {
                                eye_maps(detector.as_mut(), &job.jpeg).map(Output::Bytes)
                            }
                        }
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

    fn submit(&self, jpeg: Vec<u8>, want: Want) -> Result<Sighting, String> {
        match self.run(jpeg, Task::Look(want))? {
            Output::Sighting(seen) => Ok(seen),
            Output::Bytes(_) => Err("unexpected answer".into()),
        }
    }

    fn submit_eyes(&self, jpeg: Vec<u8>) -> Result<Vec<u8>, String> {
        match self.run(jpeg, Task::EyeMaps)? {
            Output::Bytes(bytes) => Ok(bytes),
            Output::Sighting(_) => Err("unexpected answer".into()),
        }
    }

    fn run(&self, jpeg: Vec<u8>, task: Task) -> Reply {
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
            .try_send(Job { jpeg, task, reply })
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

/// The second worker, for the Eye contact maps.
pub struct EyeFinder(pub Faces);

/// The Eye contact maps for a JPEG of the camera picture (raw body); see [`eye_maps`].
#[tauri::command]
pub async fn eye_contact_maps(
    app: tauri::AppHandle,
    request: Request<'_>,
) -> Result<tauri::ipc::Response, String> {
    use tauri::Manager;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("eye_contact_maps needs a JPEG body".into());
    };
    let bytes = bytes.clone();
    tauri::async_runtime::spawn_blocking(move || app.state::<EyeFinder>().0.submit_eyes(bytes))
        .await
        .map_err(|e| e.to_string())?
        .map(tauri::ipc::Response::new)
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

    /// Says it sees two open eyes 40 px wide in a 220 × 120 picture.
    struct Eyes;
    impl FaceDetector for Eyes {
        fn detect(&mut self, _: &[u8], _: Want) -> Result<Sighting, Error> {
            Err(Error::Unsupported)
        }
        fn detect_frame(&mut self, _: &VideoFrame, want: Want) -> Result<Sighting, Error> {
            assert!(want.eyes);
            let eye = |cx: f32| studio_segmentation::Eye {
                points: [
                    (-20.0, 0.0),
                    (-7.0, -6.0),
                    (7.0, -6.0),
                    (20.0, 0.0),
                    (7.0, 6.0),
                    (-7.0, 6.0),
                ]
                .map(|(x, y)| [(cx + x) / 220.0, (60.0 + y) / 120.0]),
            };
            Ok(Sighting {
                eyes: vec![studio_segmentation::EyePair {
                    left: eye(70.0),
                    right: eye(150.0),
                }],
                ..Sighting::default()
            })
        }
    }

    #[test]
    fn eye_maps_say_where_each_eye_is_and_how_to_redraw_it() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(220, 120, image::Rgb([200, 170, 150]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let bytes = eye_maps(&mut Eyes, &png).unwrap();
        let floats: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(&floats[..3], &[220.0, 120.0, 2.0]);
        let per_eye = 5 + 48 * 64 * 3;
        assert_eq!(floats.len(), 3 + 2 * per_eye);
        // The left eye's box is around x = 70; the right one is read mirrored.
        let (left, second) = (&floats[3..8], &floats[3 + per_eye..8 + per_eye]);
        assert!(left[1] < 70.0 && left[1] + left[3] > 70.0 && left[4] == 0.0);
        assert_eq!(second[4], 1.0);
    }
}
