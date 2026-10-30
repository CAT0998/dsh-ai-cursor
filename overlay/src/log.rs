//! Run log for the overlay.
//!
//! Written from the start of the process, unconditionally — an earlier debugging
//! attempt gated its output behind an environment variable and behind a branch that
//! turned out never to run, which cost several runs for nothing. A log that is
//! always on, always saved, and rotates itself is what makes "run it and look"
//! possible instead of guessing.
//!
//! Layout under `<exe dir>/logs/`:
//!   overlay.log      the live log
//!   overlay.1.log    previous run(s), newest first, up to `KEEP_BACKUPS`
//!
//! Rotation happens when the live file passes `MAX_BYTES`, so long sessions never
//! grow without bound and the recent past is always still on disk.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 512 * 1024;
const KEEP_BACKUPS: usize = 5;

struct Logger {
    file: File,
    path: PathBuf,
    written: u64,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

fn stamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let ms = now.subsec_millis();
    // local-ish wall clock from the epoch; good enough to order events
    let day = secs / 86_400;
    let tod = secs % 86_400;
    format!(
        "d{} {:02}:{:02}:{:02}.{:03}",
        day,
        tod / 3600,
        (tod / 60) % 60,
        tod % 60,
        ms
    )
}

/// Start the log. Returns the path it writes to.
pub fn init(dir: &Path) -> PathBuf {
    let _ = fs::create_dir_all(dir);
    let path = dir.join("overlay.log");
    let _ = rotate_if_needed(&path);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|_| File::create(&path).expect("log file"));
    let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let _ = LOGGER.set(Mutex::new(Logger {
        file,
        path: path.clone(),
        written,
    }));
    log("INFO", &format!("=== start pid={} ===", std::process::id()));
    path
}

fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size < MAX_BYTES {
        return Ok(());
    }
    // shift overlay.(n-1).log -> overlay.n.log, overlay.log -> overlay.1.log
    for i in (1..KEEP_BACKUPS).rev() {
        let from = sibling(path, i);
        let to = sibling(path, i + 1);
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    let _ = fs::rename(path, sibling(path, 1));
    Ok(())
}

fn sibling(path: &Path, n: usize) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "overlay".into());
    path.with_file_name(format!("{stem}.{n}.log"))
}

pub fn path() -> Option<PathBuf> {
    LOGGER.get().map(|l| l.lock().map(|g| g.path.clone()).unwrap_or_default())
}

/// Append one line. Never panics: a broken log must not take the overlay down.
pub fn log(level: &str, msg: &str) {
    let Some(l) = LOGGER.get() else {
        // before init: still show it on stderr so early failures are not silent
        eprintln!("[{level}] {msg}");
        return;
    };
    let Ok(mut g) = l.lock() else { return };
    let line = format!("{} [{level}] {msg}\n", stamp());
    let _ = g.file.write_all(line.as_bytes());
    let _ = g.file.flush();
    g.written += line.len() as u64;
    if g.written >= MAX_BYTES {
        let _ = g.file.flush();
        drop(g);
        let p = path().unwrap_or_default();
        let _ = rotate_if_needed(&p);
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&p) {
            if let Some(l) = LOGGER.get() {
                if let Ok(mut g) = l.lock() {
                    g.file = f;
                    g.written = 0;
                }
            }
        }
    }
}

/// Copy the live log somewhere the user asked for. Returns the destination.
pub fn export(dst: &Path) -> Result<PathBuf, String> {
    let src = path().ok_or_else(|| "log not initialised".to_string())?;
    if let Some(parent) = dst.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::copy(&src, dst).map_err(|e| format!("copy {} -> {}: {e}", src.display(), dst.display()))?;
    Ok(dst.to_path_buf())
}

#[macro_export]
macro_rules! logline {
    ($lvl:expr, $($arg:tt)*) => {
        $crate::log::log($lvl, &format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_names_are_numbered() {
        let p = Path::new("/tmp/overlay.log");
        assert_eq!(sibling(p, 1), Path::new("/tmp/overlay.1.log"));
        assert_eq!(sibling(p, 3), Path::new("/tmp/overlay.3.log"));
    }

    #[test]
    fn init_creates_the_file_and_writes_a_header() {
        let dir = std::env::temp_dir().join(format!("dl-{}", std::process::id()));
        let path = init(&dir);
        log("INFO", "hello from the test");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("=== start pid="));
        assert!(text.contains("hello from the test"));
        let _ = fs::remove_dir_all(&dir);
    }
}
