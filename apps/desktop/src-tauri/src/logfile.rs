//! A small log file at ~/Library/Logs/Webcam Studio/webcam-studio.log (or the temp folder
//! elsewhere), so problems in the camera pipeline can be looked into after the fact. Holds
//! numbers and messages only, never pictures. Starts fresh past 1 MB.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;

struct FileLog {
    file: Mutex<File>,
}

impl log::Log for FileLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(
                file,
                "{secs} {} {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Where the log goes.
pub fn path() -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Logs"))
        .filter(|_| cfg!(target_os = "macos"))
        .unwrap_or_else(std::env::temp_dir);
    base.join("Webcam Studio").join("webcam-studio.log")
}

/// Sends `log` records to the file. Failing to open it only means no log.
pub fn init() {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let fresh = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES);
    let file = OpenOptions::new()
        .create(true)
        .append(!fresh)
        .write(true)
        .truncate(fresh)
        .open(&path);
    if let Ok(file) = file {
        // Lives for the whole run, as `log` requires.
        let logger: &'static FileLog = Box::leak(Box::new(FileLog {
            file: Mutex::new(file),
        }));
        if log::set_logger(logger).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
    }
}
