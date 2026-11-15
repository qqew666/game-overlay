//! Optional file log.
//!
//! The app is built as a Windows GUI subsystem binary, so there is no console
//! to print to. `--verbose` writes `%LOCALAPPDATA%\GameOverlay\overlay.log`
//! instead, which is also what the user is asked to send with a bug report.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

static SINK: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init(enable: bool) {
    let slot = SINK.get_or_init(|| Mutex::new(None));
    if !enable {
        return;
    }
    let Some(dir) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir).join("GameOverlay");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("overlay.log"));
    if let (Ok(file), Ok(mut slot)) = (file, slot.lock()) {
        *slot = Some(file);
    }
}

pub fn write(args: std::fmt::Arguments<'_>) {
    let Some(slot) = SINK.get() else {
        return;
    };
    let Ok(mut guard) = slot.lock() else {
        return;
    };
    if let Some(file) = guard.as_mut() {
        let _ = writeln!(file, "{args}");
        let _ = file.flush();
    }
}

#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        $crate::log::write(format_args!($($arg)*))
    };
}
