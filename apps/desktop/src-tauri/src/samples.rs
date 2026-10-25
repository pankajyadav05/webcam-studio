//! Explicit, bounded sample downloads. The webview receives verified binary data only.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tauri::{Manager, ipc::Response};

const BASE: &str = "https://pub-e39e62d066fa40368708d6b84e4727b9.r2.dev/stock-clips/";
const MAX_CLIP: usize = 25 * 1024 * 1024;
const MAX_MANIFEST: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    id: String,
    title: String,
    file: String,
    bytes: usize,
    sha256: String,
    width: u32,
    height: u32,
    duration: f64,
    credit: String,
    source: String,
    #[serde(default, skip_deserializing)]
    cached: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    version: u32,
    clips: Vec<Clip>,
}
fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    if bytes.len() > MAX_MANIFEST {
        return Err("Sample list is too large".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid sample list".to_string())?;
    if manifest.version != 1 || manifest.clips.is_empty() || manifest.clips.len() > 16 {
        return Err("Unsupported sample list".into());
    }
    let mut ids = std::collections::HashSet::new();
    for clip in &manifest.clips {
        if !safe_name(&clip.id)
            || !ids.insert(&clip.id)
            || !clip.file.strip_suffix(".mp4").is_some_and(safe_name)
            || clip.bytes == 0
            || clip.bytes > MAX_CLIP
            || clip.sha256.len() != 64
            || !clip.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || clip.width == 0
            || clip.width > 8192
            || clip.height == 0
            || clip.height > 8192
            || !clip.duration.is_finite()
            || clip.duration <= 0.0
            || clip.duration > 300.0
            || clip.title.is_empty()
            || clip.title.len() > 160
            || clip.credit.len() > 200
            || !clip.source.starts_with("https://")
            || clip.source.len() > 2048
        {
            return Err("Invalid sample description".into());
        }
    }
    Ok(manifest)
}
fn verify(clip: &Clip, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_CLIP || bytes.len() != clip.bytes {
        return Err("Sample size did not match".into());
    }
    let hash = format!("{:x}", Sha256::digest(bytes));
    if !hash.eq_ignore_ascii_case(&clip.sha256) {
        return Err("Sample checksum did not match".into());
    }
    Ok(())
}
fn bounded_read(reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((cap + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read sample: {e}"))?;
    if bytes.len() > cap {
        return Err("Sample exceeds the size limit".into());
    }
    Ok(bytes)
}
trait Fetch {
    fn get(&self, file: &str, cap: usize) -> Result<Vec<u8>, String>;
}
struct Https;
impl Fetch for Https {
    fn get(&self, file: &str, cap: usize) -> Result<Vec<u8>, String> {
        // Callers supply only the literal manifest name or validated leaf filenames.
        // Never follow redirects: no other host or scheme can receive a request.
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        let agent: ureq::Agent = config.into();
        let mut response = agent
            .get(format!("{BASE}{file}"))
            .call()
            .map_err(|e| format!("Download failed: {e}"))?;
        if response.status() != 200 {
            return Err(format!("Sample server returned {}", response.status()));
        }
        bounded_read(response.body_mut().as_reader(), cap)
    }
}
fn clip_path(dir: &Path, clip: &Clip) -> PathBuf {
    dir.join(format!(
        "{}-{}.mp4",
        clip.id,
        clip.sha256.to_ascii_lowercase()
    ))
}
fn cached_clip(dir: &Path, clip: &Clip) -> Option<Vec<u8>> {
    let data = bounded_read(fs::File::open(clip_path(dir, clip)).ok()?, clip.bytes).ok()?;
    verify(clip, &data).ok()?;
    Some(data)
}
fn write_cache(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Missing cache directory")?)
        .map_err(|e| e.to_string())?;
    let temp = path.with_extension("partial");
    fs::write(&temp, bytes)
        .and_then(|()| fs::rename(&temp, path))
        .map_err(|e| format!("Could not cache sample: {e}"))
}
fn manifest_with(dir: &Path, fetch: &impl Fetch) -> Result<Manifest, String> {
    let path = dir.join("samples.json");
    let fresh = fetch.get("samples.json", MAX_MANIFEST).and_then(|bytes| {
        let manifest = parse_manifest(&bytes)?;
        write_cache(&path, &bytes)?;
        Ok(manifest)
    });
    let mut manifest = match fresh {
        Ok(value) => value,
        Err(network_error) => {
            let cached = fs::File::open(&path)
                .ok()
                .and_then(|file| bounded_read(file, MAX_MANIFEST).ok());
            cached
                .and_then(|bytes| parse_manifest(&bytes).ok())
                .ok_or(network_error)?
        }
    };
    for clip in &mut manifest.clips {
        clip.cached = cached_clip(dir, clip).is_some();
    }
    Ok(manifest)
}
fn clip_with(dir: &Path, clip: &Clip, fetch: &impl Fetch) -> Result<Vec<u8>, String> {
    if let Some(bytes) = cached_clip(dir, clip) {
        return Ok(bytes);
    }
    let bytes = fetch.get(&clip.file, clip.bytes.min(MAX_CLIP))?;
    verify(clip, &bytes)?;
    write_cache(&clip_path(dir, clip), &bytes)?;
    Ok(bytes)
}
#[derive(Default)]
pub struct Samples {
    lock: Mutex<()>,
}
fn cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("sample-clips-v1"))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn sample_manifest(app: tauri::AppHandle) -> Result<Manifest, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Samples>();
        let _guard = state.lock.lock().map_err(|e| e.to_string())?;
        manifest_with(&cache_dir(&app)?, &Https)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn sample_clip(app: tauri::AppHandle, id: String) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Samples>();
        let _guard = state.lock.lock().map_err(|e| e.to_string())?;
        let dir = cache_dir(&app)?;
        // The manifest is already fetched by the picker. Loading a clip doesn't fetch it again.
        let bytes = bounded_read(
            fs::File::open(dir.join("samples.json")).map_err(|_| "Choose Sample first")?,
            MAX_MANIFEST,
        )?;
        let manifest = parse_manifest(&bytes)?;
        let clip = manifest
            .clips
            .iter()
            .find(|clip| clip.id == id)
            .ok_or("Unknown sample")?;
        clip_with(&dir, clip, &Https).map(Response::new)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Mock {
        bytes: Vec<u8>,
        fail: bool,
        calls: AtomicUsize,
    }
    impl Fetch for Mock {
        fn get(&self, _: &str, cap: usize) -> Result<Vec<u8>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail {
                Err("Offline".into())
            } else {
                bounded_read(self.bytes.as_slice(), cap)
            }
        }
    }
    fn mock(bytes: Vec<u8>) -> Mock {
        Mock {
            bytes,
            fail: false,
            calls: AtomicUsize::new(0),
        }
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "webcam-sample-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Clip, Vec<u8>) {
        let data = b"public sample bytes".to_vec();
        (
            Clip {
                id: "wave".into(),
                title: "Video call".into(),
                file: "wave.mp4".into(),
                bytes: data.len(),
                sha256: format!("{:x}", Sha256::digest(&data)),
                width: 1280,
                height: 720,
                duration: 10.8,
                credit: "Artem Podrez, Pexels".into(),
                source: "https://www.pexels.com/video/4492644/".into(),
                cached: false,
            },
            data,
        )
    }
    fn manifest(clip: Clip) -> Vec<u8> {
        serde_json::to_vec(&Manifest {
            version: 1,
            clips: vec![clip],
        })
        .unwrap()
    }
    #[test]
    fn good_hash_download_and_offline_cache_hit() {
        let dir = Temp::new();
        let (clip, data) = fixture();
        let network = mock(data.clone());
        assert_eq!(clip_with(&dir.0, &clip, &network).unwrap(), data);
        let offline = Mock {
            fail: true,
            ..mock(vec![])
        };
        assert_eq!(clip_with(&dir.0, &clip, &offline).unwrap(), data);
        assert_eq!(offline.calls.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn bad_hash_and_byte_count_never_cached() {
        let dir = Temp::new();
        let (clip, mut data) = fixture();
        data[0] ^= 1;
        assert!(
            clip_with(&dir.0, &clip, &mock(data))
                .unwrap_err()
                .contains("checksum")
        );
        assert!(!clip_path(&dir.0, &clip).exists());
        assert!(
            clip_with(&dir.0, &clip, &mock(vec![0; clip.bytes - 1]))
                .unwrap_err()
                .contains("size")
        );
        assert!(clip_with(&dir.0, &clip, &mock(vec![0; clip.bytes + 1])).is_err());
    }
    #[test]
    fn oversize_is_bounded() {
        let (mut clip, data) = fixture();
        clip.bytes = MAX_CLIP + 1;
        assert!(parse_manifest(&manifest(clip)).is_err());
        assert!(bounded_read(data.as_slice(), 2).is_err());
        assert!(parse_manifest(&vec![b' '; MAX_MANIFEST + 1]).is_err());
    }
    #[test]
    fn manifest_parsing_and_path_confinement() {
        let (clip, _) = fixture();
        let parsed = parse_manifest(&manifest(clip.clone())).unwrap();
        assert_eq!(parsed.clips[0].id, "wave");
        for file in [
            "../wave.mp4",
            "https://evil.test/wave.mp4",
            "wave.mp4?redirect=1",
            "a/b.mp4",
        ] {
            let mut bad = clip.clone();
            bad.file = file.into();
            assert!(parse_manifest(&manifest(bad)).is_err());
        }
        let mut bad = clip;
        bad.sha256 = "xyz".into();
        assert!(parse_manifest(&manifest(bad)).is_err());
        assert!(parse_manifest(b"{\"version\":2,\"clips\":[]}").is_err());
    }
    #[test]
    fn manifest_offline_cache_and_network_failure() {
        let dir = Temp::new();
        let (clip, _) = fixture();
        manifest_with(&dir.0, &mock(manifest(clip))).unwrap();
        let offline = Mock {
            fail: true,
            ..mock(vec![])
        };
        assert_eq!(manifest_with(&dir.0, &offline).unwrap().clips[0].id, "wave");
        let empty = Temp::new();
        assert_eq!(manifest_with(&empty.0, &offline).unwrap_err(), "Offline");
        let (clip, _) = fixture();
        assert_eq!(clip_with(&empty.0, &clip, &offline).unwrap_err(), "Offline");
    }
    #[test]
    fn corrupt_cache_is_reverified_and_replaced() {
        let dir = Temp::new();
        let (clip, data) = fixture();
        fs::write(clip_path(&dir.0, &clip), vec![0; clip.bytes]).unwrap();
        assert_eq!(clip_with(&dir.0, &clip, &mock(data.clone())).unwrap(), data);
    }
    #[test]
    fn local_mock_server_download_then_offline_cache() {
        use std::{io::Write, net::TcpListener};
        let dir = Temp::new();
        let (clip, data) = fixture();
        let list = manifest(clip.clone());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let expected = data.clone();
        let worker = std::thread::spawn(move || {
            for body in [list, data] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                let count = stream.read(&mut request).unwrap();
                assert!(count > 0);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        // HTTP is confined to this local test transport; production accepts only BASE.
        struct Local(std::net::SocketAddr);
        impl Fetch for Local {
            fn get(&self, file: &str, cap: usize) -> Result<Vec<u8>, String> {
                let mut response = ureq::get(format!("http://{}/{file}", self.0))
                    .call()
                    .map_err(|e| e.to_string())?;
                bounded_read(response.body_mut().as_reader(), cap)
            }
        }
        let network = Local(addr);
        let list = manifest_with(&dir.0, &network).unwrap();
        assert_eq!(
            clip_with(&dir.0, &list.clips[0], &network).unwrap(),
            expected
        );
        worker.join().unwrap();
        assert!(manifest_with(&dir.0, &network).unwrap().clips[0].cached);
        assert_eq!(clip_with(&dir.0, &clip, &network).unwrap(), expected);
    }
    #[test]
    #[ignore = "explicit maintainer endpoint smoke test; never runs at launch"]
    fn real_r2_manifest_and_clips() {
        let dir = Temp::new();
        let list = manifest_with(&dir.0, &Https).unwrap();
        for clip in &list.clips {
            let bytes = clip_with(&dir.0, clip, &Https).unwrap();
            println!("R2 {}: {} bytes verified", clip.id, bytes.len());
        }
    }
}
