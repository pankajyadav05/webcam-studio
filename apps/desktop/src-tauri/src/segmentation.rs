//! A single Vision request lives on one serial thread, outside the window/runtime.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use studio_segmentation::{PersonSegmenter, VisionSegmenter, person::Error, protocol};
use tauri::{
    State,
    ipc::{InvokeBody, Request, Response},
};

type Reply = Result<Vec<u8>, String>;
struct Job {
    bytes: Vec<u8>,
    reply: mpsc::SyncSender<Reply>,
}
pub struct Segmentation {
    sender: Mutex<mpsc::SyncSender<Job>>,
    busy: Arc<AtomicBool>,
    available: bool,
}
impl Segmentation {
    pub fn new() -> Self {
        Self::with_factory(|| {
            VisionSegmenter::new().map(|provider| Box::new(provider) as Box<dyn PersonSegmenter>)
        })
    }
    fn with_factory(
        factory: impl Fn() -> Result<Box<dyn PersonSegmenter>, Error> + Send + 'static,
    ) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let busy = Arc::new(AtomicBool::new(false));
        let worker_busy = Arc::clone(&busy);
        let spawned = std::thread::Builder::new()
            .name("person-vision".into())
            .spawn(move || {
                let mut segmenter = factory();
                let mut last_epoch = None;
                let _ = ready_tx.send(segmenter.is_ok());
                while let Ok(job) = receiver.recv() {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let (id, quality, epoch, pixels) =
                            protocol::decode(&job.bytes).map_err(|e| e.to_string())?;
                        let provider = segmenter.as_mut().map_err(|e| e.to_string())?;
                        if last_epoch != Some(epoch) {
                            provider.reset().map_err(|e| e.to_string())?;
                            last_epoch = Some(epoch);
                        }
                        let start = std::time::Instant::now();
                        let mask = provider
                            .segment(pixels, quality)
                            .map_err(|e| e.to_string())?;
                        let ms = start.elapsed().as_secs_f32() * 1000.0;
                        log::debug!(
                            "person frame {id}: {ms:.2} ms, {}x{}",
                            mask.width,
                            mask.height
                        );
                        Ok(protocol::encode(id, &mask, ms))
                    }))
                    .unwrap_or_else(|_| {
                        segmenter = factory();
                        Err("Vision worker panicked; request reset".into())
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
    fn submit(&self, bytes: Vec<u8>) -> Reply {
        protocol::decode(&bytes).map_err(|e| e.to_string())?;
        if !self.available {
            return Err("Native person segmentation unavailable".into());
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err("Vision busy: stale frame dropped".into());
        }
        let (reply, receiver) = mpsc::sync_channel(1);
        if self
            .sender
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .try_send(Job { bytes, reply })
            .is_err()
        {
            self.busy.store(false, Ordering::Release);
            return Err("Vision worker unavailable".into());
        }
        // A timeout does not release busy: the native call must finish before another can start.
        receiver
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "Vision frame timed out".to_string())?
    }
}
#[derive(serde::Serialize)]
pub struct SegmenterInfo {
    backend: &'static str,
    quality: &'static str,
    available: bool,
    diagnostics: bool,
}
#[tauri::command]
pub fn segmenter_info(state: State<'_, Segmentation>) -> SegmenterInfo {
    SegmenterInfo {
        backend: if cfg!(target_os = "macos") {
            "apple-vision"
        } else {
            "unsupported"
        },
        quality: "balanced",
        available: state.available,
        diagnostics: std::env::var_os("WEBCAM_STUDIO_DIAGNOSTICS").is_some(),
    }
}
#[tauri::command]
pub async fn segment_person(
    app: tauri::AppHandle,
    request: Request<'_>,
) -> Result<Response, String> {
    use tauri::Manager;
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        _ => return Err("segment_person requires binary WSI1 body".into()),
    };
    tauri::async_runtime::spawn_blocking(move || app.state::<Segmentation>().submit(bytes))
        .await
        .map_err(|e| e.to_string())?
        .map(Response::new)
}

/// Optional local performance trace, enabled only by the launch environment. No pixels.
#[tauri::command]
pub fn segmentation_diagnostics(snapshot: serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let Some(path) = std::env::var_os("WEBCAM_STUDIO_DIAGNOSTICS") else {
        return Ok(());
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{snapshot}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_segmentation::{Mask, Quality};
    struct SlowProvider {
        started: mpsc::Sender<()>,
    }
    impl PersonSegmenter for SlowProvider {
        fn segment(&mut self, _: &[u8], _: Quality) -> Result<Mask, Error> {
            let _ = self.started.send(());
            std::thread::sleep(std::time::Duration::from_millis(40));
            Ok(Mask::full(2, 1))
        }
    }
    fn frame() -> Vec<u8> {
        let mut bytes = b"WSI1".to_vec();
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(b"jpeg");
        bytes
    }
    #[test]
    fn one_in_flight_drops_stale_requests_and_recovers() {
        let (started, receiver) = mpsc::channel();
        let worker = Arc::new(Segmentation::with_factory(move || {
            Ok(Box::new(SlowProvider {
                started: started.clone(),
            }))
        }));
        let first = Arc::clone(&worker);
        let handle = std::thread::spawn(move || first.submit(frame()));
        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(worker.submit(frame()).unwrap_err().contains("stale frame"));
        assert_eq!(handle.join().unwrap().unwrap().len(), 22);
        assert!(!worker.busy.load(Ordering::Acquire));
        assert!(worker.submit(vec![]).is_err());
        assert!(worker.submit(frame()).is_ok());
    }
}
